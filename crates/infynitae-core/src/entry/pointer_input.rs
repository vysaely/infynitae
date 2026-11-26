//! Pointer intake: motion, buttons, camera pans, window drags, wheel zoom.

use super::*;

impl Core {
    /// Motion follows the interaction (INV-INPUT-1): a pan moves the camera and nothing reaches a
    /// client (INV-INPUT-3, KeepFocus); an orphaned interaction or NoOutput consumes it; otherwise
    /// the pointer moves in the Arrangement and the event is routed, with its relative motion.
    pub(super) fn pointer_motion(
        &mut self,
        proof: Option<authority::Authorized>,
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
        out: &mut Vec<Effect>,
    ) {
        match self.input.interaction {
            Interaction::CameraPan { output, camera, .. } => {
                if matches!(self.input.gesture, Some(TouchpadGesture::Pan)) {
                    return;
                }
                let _panned = self.pan_step(proof, output, camera, delta, out);
                // NEGATIVE CONTROL (INV-INPUT-3 bypass switch, TST-05): pan motion also reaches
                // the client. The x8 tests must fail under it.
                #[cfg(feature = "probe_pan_motion_bypass")]
                if _panned {
                    self.routed_motion(delta, delta_unaccel, time_us, out);
                }
            }
            Interaction::Orphaned { .. } => {}
            // Super+drag on a window: the pointer moves as usual, the window follows it, and
            // nothing reaches a client (KeepFocus, INV-INPUT-3)
            // Super+middle during a move: the camera pans under the fixed pointer, and the window
            // keeps its grabbed point under it, so it travels across the Canvas with the view
            Interaction::ElementMove { pan: Some(p), .. } => {
                if self.pan_step(proof, p.output, p.camera, delta, out) {
                    self.window_drag_motion((0.0, 0.0), out);
                }
            }
            Interaction::ElementMove { .. } | Interaction::ElementResize { .. } => {
                self.window_drag_motion(delta, out);
            }
            Interaction::Idle => {
                let PointerState::Located { output, p } = self.pointer else {
                    return;
                };
                if let Some(c) = self.constraint {
                    if c.kind == pointer::ConstraintKind::Lock {
                        out.extend(self.locked_motion(delta, delta_unaccel, time_us));
                        return;
                    }
                    if c.kind == pointer::ConstraintKind::Confine {
                        self.pointer = self.confined_motion(output, p, delta, &c);
                        self.routed_motion(delta, delta_unaccel, time_us, out);
                        return;
                    }
                }
                if let Some(moved) = pointer::moved(&self.outputs, output, p, delta) {
                    self.pointer = moved;
                    if let PointerState::Located {
                        output: cur_o,
                        p: cur_p,
                    } = self.pointer
                        && let (Some(camera), Some(extent)) = (
                            self.cameras.camera_for_output(cur_o),
                            self.output(cur_o).map(|m| m.logical_extent()),
                        )
                    {
                        self.cameras.update_glide_pointer(camera, cur_p, extent);
                    }
                }
                // a button an app holds is the user working in that app (turning a game's camera
                // with the right button, dragging a slider): crossing another window then is no
                // reason to move the focus. The crossing counts after the release (owner,
                // 2026-11-15: a game lost and regained the focus several times a second;
                // AUDIT_FOCUS_HOVER finding 1)
                let client_holds_button = self
                    .input
                    .ownership
                    .pressed()
                    .iter()
                    .any(|(_, owner)| *owner == Owner::Client);
                if self.focus_follows_mouse
                    && !client_holds_button
                    && let PointerState::Located {
                        output: cur_o,
                        p: cur_p,
                    } = self.pointer
                {
                    // over a panel or overlay no window is under the pointer: coming back to the
                    // window counts as a crossing again (finding 2)
                    if self.layer_hit(cur_o, cur_p, true).is_some() {
                        self.hover_seen = None;
                    } else {
                        let pt = crate::geom::OutputLogicalPoint {
                            x: cur_p.0,
                            y: cur_p.1,
                        };
                        // only a crossing moves the focus, not every motion: Super+Tab's window
                        // stays when the mouse is nudged (owner, 2026-11-01)
                        let under = self.hit_test(cur_o, pt).map(|hit| hit.element);
                        if under != self.hover_seen {
                            self.hover_seen = under;
                            let target = self.focus.target();
                            // a clicked panel holding the keyboard gives it back when the pointer
                            // crosses into a window, also the one focused before it (finding 2)
                            let takes_back_from_panel =
                                under.is_some() && under == target && self.focus.layer_on_demand();
                            if under.is_some() && under != target
                                || takes_back_from_panel
                                || under.is_none()
                                    && self.focus_follows_mouse_strict
                                    && target.is_some()
                            {
                                out.extend(self.focus.hover_focus(under, time_us));
                                // a pointer lock or confine the newly focused window asked for
                                // starts now, as after any focus change (finding 4)
                                out.extend(self.reconcile_constraints());
                            }
                        }
                    }
                }
                self.routed_motion(delta, delta_unaccel, time_us, out);
            }
        }
    }

    /// The window Super+Shift+C closes: the focused one; with strict focus only while it is under the
    /// pointer, so a key pressed over empty canvas closes nothing (focus can be stale there: the
    /// canvas moved under a still pointer), unless the keyboard chose it.
    pub(super) fn close_target(&mut self) -> Option<crate::stage::ElementId> {
        let focused = self.focus.target()?;
        if self.keyed_choice().is_none()
            && self.focus_follows_mouse
            && self.focus_follows_mouse_strict
            && matches!(self.pointer, PointerState::Located { .. })
            && self.hovered_window() != Some(focused)
        {
            return None;
        }
        Some(focused)
    }

    /// The focused window, when the keyboard chose it and the pointer has not moved since.
    pub(super) fn keyed_choice(&self) -> Option<crate::stage::ElementId> {
        self.keyed_focus
            .filter(|&id| self.focus.target() == Some(id))
    }

    /// One motion of a pan (F-PAN-1): the camera moves by the accumulated delta; the pointer stays.
    /// Returns whether the camera was written.
    pub(super) fn pan_step(
        &mut self,
        proof: Option<authority::Authorized>,
        output: crate::output::OutputId,
        camera: usize,
        delta: (f64, f64),
        out: &mut Vec<Effect>,
    ) -> bool {
        let (Some(scale), Some(zoom)) = (
            self.output(output).map(|o| o.scale),
            self.cameras.zoom(camera),
        ) else {
            return false;
        };
        let (Some(proof), Some((camera, start, sum))) =
            (proof, self.input.interaction.pan_motion(delta, scale, zoom))
        else {
            return false;
        };
        self.cameras.pan(proof, camera, start, sum, self.pan_mode);
        if self.fullscreen_pan
            && self.presentation(output).is_some()
            && !self.is_presentation_suppressed(output)
        {
            self.suppress_presentation(output, out);
        }
        if (self.pan_mode == crate::navigation::PanMode::Free
            || self.pan_mode == crate::navigation::PanMode::Rts)
            && let PointerState::Located { output: p_out, p } = self.pointer
            && p_out == output
        {
            let extent = self
                .output(output)
                .map(|o| o.logical_extent())
                .unwrap_or((1920.0, 1080.0));
            let new_p = match self.pan_mode {
                crate::navigation::PanMode::Free => (
                    (p.0 + delta.0).clamp(0.0, extent.0 - 1.0),
                    (p.1 + delta.1).clamp(0.0, extent.1 - 1.0),
                ),
                crate::navigation::PanMode::Rts => (
                    (p.0 - delta.0).clamp(0.0, extent.0 - 1.0),
                    (p.1 - delta.1).clamp(0.0, extent.1 - 1.0),
                ),
                _ => unreachable!(),
            };
            self.pointer = PointerState::Located { output, p: new_p };
        }
        // F-LATTICE-1: the reference is kept while panning, unless the bound is exceeded
        self.update_lattice(output);
        self.invalidate_frame(output);
        self.on_top_follow(camera, out);
        self.visibility_pass_camera(camera, out);
        if self.pan_mode == crate::navigation::PanMode::Free
            || self.pan_mode == crate::navigation::PanMode::Rts
        {
            self.true_position_into(out);
        }
        true
    }

    /// One motion of a WindowMove or WindowResize: the pointer moves in the Arrangement
    /// as usual; then, from its Canvas position `q_now` (R1, through the camera of the output it
    /// is on), the moved window's anchor is `R3(q_now − grab_offset)` (F-DRAG-1, OB-34 decision:
    /// the grabbed point stays under the pointer across outputs) and the resized window is asked
    /// for `start_size ± (q_now − q_press)` (the dragged corner's sides; at least 32 px).
    fn window_drag_motion(&mut self, delta: (f64, f64), out: &mut Vec<Effect>) {
        let PointerState::Located { output, p } = self.pointer else {
            return;
        };
        let (output, p) = match pointer::moved(&self.outputs, output, p, delta) {
            Some(moved) => {
                self.pointer = moved;
                match moved {
                    PointerState::Located { output, p } => (output, p),
                    PointerState::NoOutput => return,
                }
            }
            None => (output, p),
        };
        let Some(view) = self.view(output) else {
            return;
        };
        let q_now = view.unproject(crate::geom::OutputLogicalPoint { x: p.0, y: p.1 });
        match self.input.interaction {
            Interaction::ElementMove {
                element,
                grab_offset,
                is_cluster,
                ..
            } => {
                let target = crate::geom::CanvasPos {
                    x: q_now.x - grab_offset.x,
                    y: q_now.y - grab_offset.y,
                };
                if let Some(anchor) = target.round_to_point()
                    && self.stage.anchor(element) != Some(anchor)
                {
                    if is_cluster && !self.input.cluster_drag.is_empty() {
                        let cluster_drag = self.input.cluster_drag.clone();
                        let primary_initial = cluster_drag
                            .iter()
                            .find(|(e, _, _)| *e == element)
                            .map(|(_, a, _)| *a)
                            .unwrap_or(anchor);
                        let dx = anchor.x - primary_initial.x;
                        let dy = anchor.y - primary_initial.y;
                        for (e, orig_a, _) in cluster_drag {
                            let new_a = crate::geom::CanvasPoint {
                                x: orig_a.x.saturating_add(dx),
                                y: orig_a.y.saturating_add(dy),
                            };
                            let _ = self.place_by_user(e, new_a, out);
                        }
                    } else {
                        let _ = self.place_by_user(element, anchor, out);
                    }
                }
            }
            Interaction::ElementResize {
                left,
                top,
                axes,
                start_size,
                press,
                is_cluster,
                ..
            } => {
                let d = |now: i64, then: i64| (now - then) as f64 / crate::geom::Q16_ONE as f64;
                let grow = |start: i32, d: f64, from_start: bool| {
                    let d = if from_start { -d } else { d };
                    (f64::from(start) + d)
                        .round()
                        .clamp(32.0, f64::from(i32::MAX)) as i32
                };
                let size = (
                    if axes.0 {
                        grow(start_size.0, d(q_now.x, press.x), left)
                    } else {
                        start_size.0
                    },
                    if axes.1 {
                        grow(start_size.1, d(q_now.y, press.y), top)
                    } else {
                        start_size.1
                    },
                );
                if is_cluster && !self.input.cluster_drag.is_empty() {
                    let cluster_drag = self.input.cluster_drag.clone();
                    let mut min_x = i64::MAX;
                    let mut min_y = i64::MAX;
                    let mut max_x = i64::MIN;
                    let mut max_y = i64::MIN;
                    for &(_, a, s) in &cluster_drag {
                        min_x = min_x.min(a.x);
                        min_y = min_y.min(a.y);
                        max_x = max_x.max(a.x.saturating_add(s.0 as i64));
                        max_y = max_y.max(a.y.saturating_add(s.1 as i64));
                    }
                    let orig_w = (max_x - min_x).max(1) as f64;
                    let orig_h = (max_y - min_y).max(1) as f64;
                    let scale_x = (size.0 as f64 / orig_w).clamp(0.2, 5.0);
                    let scale_y = (size.1 as f64 / orig_h).clamp(0.2, 5.0);

                    let origin_x = if left { max_x as f64 } else { min_x as f64 };
                    let origin_y = if top { max_y as f64 } else { min_y as f64 };

                    for (e, orig_a, orig_s) in cluster_drag {
                        let rel_x = orig_a.x as f64 - origin_x;
                        let rel_y = orig_a.y as f64 - origin_y;
                        let new_a = crate::geom::CanvasPoint {
                            x: (origin_x + rel_x * scale_x).round() as i64,
                            y: (origin_y + rel_y * scale_y).round() as i64,
                        };
                        let new_w =
                            ((orig_s.0 as f64) * scale_x).round().clamp(64.0, 32768.0) as i32;
                        let new_h =
                            ((orig_s.1 as f64) * scale_y).round().clamp(64.0, 32768.0) as i32;

                        let _ = self.place_by_user(e, new_a, out);
                        out.push(Effect::ToplevelConfigureSize {
                            element: e,
                            size: crate::geom::LocalSize { w: new_w, h: new_h },
                        });
                    }
                } else {
                    self.resize_to(size, out);
                }
            }
            _ => {}
        }
        self.invalidate_frame(output);
    }

    /// The client path of one motion event: the true-position motion, the relative motion of the
    /// physical delta to the same routing (INV-INPUT-4), one frame.
    fn routed_motion(
        &mut self,
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
        out: &mut Vec<Effect>,
    ) {
        let Some((target, location)) = self.route_pointer() else {
            return;
        };
        out.extend([
            Effect::PointerMotion {
                target,
                location,
                time_ms: ms(time_us),
            },
            Effect::PointerRelative {
                target,
                delta,
                delta_unaccel,
                time_us,
            },
            Effect::PointerFrame,
        ]);
    }

    /// Super+Shift+Scroll: zoom camera in/out centered around the pointer.
    pub(super) fn zoom_axis(
        &mut self,
        proof: authority::Authorized,
        axis: crate::input::Axis,
        out: &mut Vec<Effect>,
    ) {
        // Scroll amount: negative is scroll up (away from user, zoom in)
        // positive is scroll down (towards user, zoom out)
        let delta = if axis.v120.1 != 0 {
            axis.v120.1 as f64 / 120.0
        } else if axis.amount.1.abs() > 0.001 {
            axis.amount.1 / 15.0
        } else {
            0.0
        };
        if delta == 0.0 {
            return;
        }

        // Each full scroll detent changes zoom by ~15%
        let factor = 1.15_f64.powf(-delta);
        self.zoom_by_factor(proof, factor, out);
    }

    /// Zooms the pointer output's camera by `factor` around the pointer position.
    pub(super) fn zoom_by_factor(
        &mut self,
        proof: authority::Authorized,
        factor: f64,
        out: &mut Vec<Effect>,
    ) {
        let PointerState::Located { output, p } = self.pointer else {
            return;
        };
        if let Some(pres) = self.presentation(output)
            && !self.is_presentation_suppressed(output)
        {
            if self.fullscreen_pan {
                if let Some(camera) = self.cameras.camera_for_output(output)
                    && let Some((anchor, tree, _)) = self.stage.mapped(pres)
                {
                    let extent = self
                        .output(output)
                        .map(|m| m.logical_extent())
                        .unwrap_or((1920.0, 1080.0));
                    let margin_x = ((extent.0 - f64::from(tree.geometry.size.w)) / 2.0).max(0.0);
                    let margin_y = ((extent.1 - f64::from(tree.geometry.size.h)) / 2.0).max(0.0);
                    let pres_center = crate::geom::CanvasPos {
                        x: crate::geom::q16(anchor.x as f64 - margin_x + extent.0 / 2.0),
                        y: crate::geom::q16(anchor.y as f64 - margin_y + extent.1 / 2.0),
                    };
                    self.cameras
                        .set_zoom_and_center(proof, camera, 1.0, pres_center);
                }
                self.suppress_presentation(output, out);
                return;
            } else {
                return;
            }
        }
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return;
        };
        let (Some(old_zoom), Some(old_center), Some(extent)) = (
            self.cameras.zoom(camera),
            self.cameras.center(camera),
            self.output(output).map(|m| m.logical_extent()),
        ) else {
            return;
        };

        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        if self.zoom_feel != crate::navigation::ZoomFeel::Instant {
            if self
                .cameras
                .glide_zoom_step(proof, camera, p, extent, factor.ln(), self.zoom_feel)
            {
                self.camera_written(output, camera, out);
            }
            return;
        }
        let new_zoom = (old_zoom * factor).clamp(crate::geom::ZOOM_MIN, crate::geom::ZOOM_MAX);
        if (new_zoom - old_zoom).abs() < 1e-9 {
            return;
        }

        // Keep the canvas point under the cursor fixed:
        // C_new = C_old + (p - extent / 2) * (1 / old_zoom - 1 / new_zoom)
        let dx = (p.0 - extent.0 / 2.0) * (1.0 / old_zoom - 1.0 / new_zoom);
        let dy = (p.1 - extent.1 / 2.0) * (1.0 / old_zoom - 1.0 / new_zoom);
        let limit = crate::geom::CANVAS_LIMIT_PX * crate::geom::Q16_ONE;
        let new_center = crate::geom::CanvasPos {
            x: old_center
                .x
                .saturating_add(crate::geom::q16(dx))
                .clamp(-limit, limit),
            y: old_center
                .y
                .saturating_add(crate::geom::q16(dy))
                .clamp(-limit, limit),
        };

        self.cameras
            .set_zoom_and_center(proof, camera, new_zoom, new_center);
        self.camera_written(output, camera, out);
    }

    /// Resets camera zoom to 1.0 centered around pointer.
    pub(super) fn zoom_reset(
        &mut self,
        proof: authority::Authorized,
        output: OutputId,
        camera: usize,
        out: &mut Vec<Effect>,
    ) {
        if self.presentation(output).is_some() && !self.is_presentation_suppressed(output) {
            return;
        }
        let (Some(old_zoom), Some(old_center), Some(extent)) = (
            self.cameras.zoom(camera),
            self.cameras.center(camera),
            self.output(output).map(|m| m.logical_extent()),
        ) else {
            return;
        };
        let p = match self.pointer {
            PointerState::Located { p, .. } => p,
            PointerState::NoOutput => (extent.0 / 2.0, extent.1 / 2.0),
        };
        let new_zoom = 1.0;
        if self.zoom_feel != crate::navigation::ZoomFeel::Instant {
            if self
                .cameras
                .glide_zoom_to(proof, camera, p, extent, new_zoom, self.zoom_feel)
            {
                self.camera_written(output, camera, out);
            }
            return;
        }
        let dx = (p.0 - extent.0 / 2.0) * (1.0 / old_zoom - 1.0 / new_zoom);
        let dy = (p.1 - extent.1 / 2.0) * (1.0 / old_zoom - 1.0 / new_zoom);
        let limit = crate::geom::CANVAS_LIMIT_PX * crate::geom::Q16_ONE;
        let new_center = crate::geom::CanvasPos {
            x: old_center
                .x
                .saturating_add(crate::geom::q16(dx))
                .clamp(-limit, limit),
            y: old_center
                .y
                .saturating_add(crate::geom::q16(dy))
                .clamp(-limit, limit),
        };

        self.cameras
            .set_zoom_and_center(proof, camera, new_zoom, new_center);
        self.camera_written(output, camera, out);
    }

    /// Ownership is decided here, at press, before any client dispatch (INV-INPUT-1, C4.2 P2).
    /// The effects are appended to `out` (the caller's reusable buffer, C14.3, INV-ALLOC-1).
    pub(super) fn button_press(
        &mut self,
        button: u32,
        mods: bindings::Modifiers,
        time_ms: u32,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        if self.input.ownership.owner(button).is_some() {
            return; // a repeated press keeps its first owner
        }
        // M22: any press during an interaction is owned by it; M10: NoOutput consumes presses
        let PointerState::Located { output, p } = self.pointer else {
            self.input.ownership.press(button, Owner::Compositor);
            return;
        };
        // Super+middle while moving a window: a pan nested in the move (owner request 2026-10-22)
        if matches!(
            self.input.interaction,
            Interaction::ElementMove { pan: None, .. }
        ) && bindings::is_pan_binding(button, mods)
            && let Some(camera) = self.cameras.camera_for_output(output)
            && let Some(start) = self.cameras.center(camera)
            && let Interaction::ElementMove { pan, .. } = &mut self.input.interaction
        {
            *pan = Some(crate::input::interaction::MovePan {
                output,
                camera,
                button,
                start,
                sum: (0.0, 0.0),
            });
            self.input.ownership.press(button, Owner::Compositor);
            return;
        }
        if self.input.interaction.active() {
            self.input.ownership.press(button, Owner::Compositor);
            return;
        }
        if !self.input.ownership.client_owns_any()
            && self.layer_hit(output, p, true).is_none()
            && let Some((_, action)) = self
                .key_bindings
                .chords
                .iter()
                .find(|(c, _)| c.key == button && c.mods == mods)
        {
            self.input.ownership.press(button, Owner::Compositor);
            if let Some(action) = *action {
                self.apply_key_action(action, proof, out);
            }
            return;
        }
        // G-OB24 is met (C12.F14.5): a pan at any zoom keeps the output's lattice reference
        let acquire = bindings::is_pan_binding(button, mods)
            // INV-INPUT-2: never while the client owns a button
            && !self.input.ownership.client_owns_any();
        // Super+left / Super+right on a window: move or resize it (owner request 2026-10-22, "like
        // Hyprland"); not on a panel above it, never while the client owns a button
        // (INV-INPUT-2). A fullscreen window can only be moved: Super+left takes it out of
        // fullscreen and drags it (owner, 2026-10-25).
        if let Some(drag) = bindings::window_drag_binding(button, mods)
            && !self.input.ownership.client_owns_any()
            && self.layer_hit(output, p, true).is_none()
            && let Some(hit) =
                self.hit_test(output, crate::geom::OutputLogicalPoint { x: p.0, y: p.1 })
            && (self.fullscreen_of(hit.element).is_none()
                || matches!(drag, bindings::WindowDrag::Move))
            && let (Some(anchor), Some(size)) = (
                self.stage.anchor(hit.element),
                self.stage.tree(hit.element).map(|t| t.geometry.size),
            )
        {
            let element = hit.element;
            self.raise_by_user(element);
            out.extend(
                self.focus
                    .set_focus_at(self.input.clock.now_us(), Some(element)),
            );
            out.push(Effect::DismissPopupGrab(element));
            // the pointer's Canvas position (R1): the hit's surface origin plus its local position
            let press = crate::geom::CanvasPos {
                x: hit.origin.x * crate::geom::Q16_ONE + hit.local.x,
                y: hit.origin.y * crate::geom::Q16_ONE + hit.local.y,
            };
            // out of fullscreen: the same relative point of the window, at its size before
            // fullscreen (half the screen when unknown), stays under the pointer
            let (anchor, size) = if self.fullscreen_of(element).is_some() {
                let rel = |q: i64, a: i64, w: i32| {
                    let local = q as f64 / crate::geom::Q16_ONE as f64 - a as f64;
                    (local / f64::from(w.max(1))).clamp(0.0, 1.0)
                };
                let (rx, ry) = (
                    rel(press.x, anchor.x, size.w),
                    rel(press.y, anchor.y, size.h),
                );
                let windowed = self.fullscreen_of(element).and_then(|f| f.saved).unwrap_or(
                    crate::geom::LocalSize {
                        w: size.w / 2,
                        h: size.h / 2,
                    },
                );
                let at = crate::geom::CanvasPoint {
                    x: (press.x / crate::geom::Q16_ONE) - (rx * f64::from(windowed.w)) as i64,
                    y: (press.y / crate::geom::Q16_ONE) - (ry * f64::from(windowed.h)) as i64,
                };
                out.extend(self.leave_fullscreen(element, Some(at)));
                (at, windowed)
            } else {
                (anchor, size)
            };
            let is_cluster = matches!(
                drag,
                bindings::WindowDrag::ClusterMove | bindings::WindowDrag::ClusterResize
            );
            if is_cluster {
                let cluster = self.cluster_for_element(element);
                self.input.cluster_drag = cluster
                    .iter()
                    .filter_map(|&e| {
                        let a = self.stage.anchor(e)?;
                        let s = self
                            .stage
                            .tree(e)
                            .map(|t| (t.geometry.size.w, t.geometry.size.h))?;
                        Some((e, a, s))
                    })
                    .collect();
            } else {
                self.input.cluster_drag.clear();
            }

            self.input.interaction = match drag {
                bindings::WindowDrag::Move | bindings::WindowDrag::ClusterMove => {
                    Interaction::ElementMove {
                        element,
                        initiator: button,
                        grab_offset: crate::geom::CanvasPos {
                            x: press.x - anchor.x * crate::geom::Q16_ONE,
                            y: press.y - anchor.y * crate::geom::Q16_ONE,
                        },
                        pan: None,
                        is_cluster,
                    }
                }
                bindings::WindowDrag::Resize | bindings::WindowDrag::ClusterResize => {
                    if is_cluster && !self.input.cluster_drag.is_empty() {
                        // a scaled cluster's maximized windows are maximized no longer
                        let members: Vec<_> =
                            self.input.cluster_drag.iter().map(|(e, ..)| *e).collect();
                        for e in members {
                            self.resized_out_of_maximize(e);
                        }
                        let mut min_x = i64::MAX;
                        let mut min_y = i64::MAX;
                        let mut max_x = i64::MIN;
                        let mut max_y = i64::MIN;
                        for &(_, a, s) in &self.input.cluster_drag {
                            min_x = min_x.min(a.x);
                            min_y = min_y.min(a.y);
                            max_x = max_x.max(a.x.saturating_add(s.0 as i64));
                            max_y = max_y.max(a.y.saturating_add(s.1 as i64));
                        }
                        let span_w = (max_x - min_x).max(1);
                        let span_h = (max_y - min_y).max(1);
                        let mid_x = (min_x + span_w / 2) * crate::geom::Q16_ONE;
                        let mid_y = (min_y + span_h / 2) * crate::geom::Q16_ONE;
                        let left = press.x < mid_x;
                        let top = press.y < mid_y;
                        Interaction::ElementResize {
                            element,
                            initiator: button,
                            left,
                            top,
                            axes: (true, true),
                            start_size: (span_w as i32, span_h as i32),
                            press,
                            is_cluster: true,
                        }
                    } else {
                        // the corner nearest the pointer: its quadrant of the window geometry
                        let mid = |a: i64, s: i32| {
                            a * crate::geom::Q16_ONE + i64::from(s) * (crate::geom::Q16_ONE / 2)
                        };
                        let left = press.x < mid(anchor.x, size.w);
                        let top = press.y < mid(anchor.y, size.h);
                        let start_size = self
                            .begin_resize(element, left, top)
                            .unwrap_or((size.w, size.h));
                        Interaction::ElementResize {
                            element,
                            initiator: button,
                            left,
                            top,
                            axes: (true, true),
                            start_size,
                            press,
                            is_cluster: false,
                        }
                    }
                }
            };
            self.input.ownership.press(button, Owner::Compositor);
            return;
        }
        if acquire && self.begin_camera_pan(output, button, proof, out) {
            self.input.ownership.press(button, Owner::Compositor);
            return;
        }
        self.input.ownership.press(button, Owner::Client);
        self.tap_focus(output, p, out);
        out.push(Effect::PointerButton {
            button,
            pressed: true,
            time_ms,
        });
        out.push(Effect::PointerFrame);
    }

    /// Click- and tap-to-focus (INV-INPUT-8, M11): a window under `(output, p)` is raised and
    /// focused; an on-demand layer surface above the Canvas (or below it where no window is)
    /// takes the keyboard (L3, L5).
    pub(super) fn tap_focus(&mut self, output: OutputId, p: (f64, f64), out: &mut Vec<Effect>) {
        let pt = crate::geom::OutputLogicalPoint { x: p.0, y: p.1 };
        let on_layer = self.layer_hit(output, p, true);
        let element = if on_layer.is_none() {
            self.hit_test(output, pt)
        } else {
            None
        };
        let on_layer = on_layer.or_else(|| {
            element
                .is_none()
                .then(|| self.layer_hit(output, p, false))
                .flatten()
        });
        if let Some(hit) = element {
            // the clicked window is under the pointer: raising it changes no pointer target
            self.raise_by_user(hit.element);
            self.note_used(hit.element);
            out.extend(
                self.focus
                    .set_focus_at(self.input.clock.now_us(), Some(hit.element)),
            );
        } else if let Some(h) = on_layer
            && self
                .layer(h.layer)
                .is_some_and(|r| r.state.keyboard == crate::layer::KeyboardInteractivity::OnDemand)
        {
            out.extend(self.focus.layer_take(h.layer, false));
        }
    }

    /// Starts a `CameraPan` interaction on `output` with `initiator` (`0` for touchpad swipe).
    pub(super) fn begin_camera_pan(
        &mut self,
        output: OutputId,
        initiator: u32,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) -> bool {
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return false;
        };
        let Some(mut start) = self.cameras.center(camera) else {
            return false;
        };
        if self.fullscreen_pan
            && let Some(pres) = self.presentation(output)
            && !self.is_presentation_suppressed(output)
            && let Some((anchor, tree, _)) = self.stage.mapped(pres)
        {
            let extent = self
                .output(output)
                .map(|m| m.logical_extent())
                .unwrap_or((1920.0, 1080.0));
            let margin_x = ((extent.0 - f64::from(tree.geometry.size.w)) / 2.0).max(0.0);
            let margin_y = ((extent.1 - f64::from(tree.geometry.size.h)) / 2.0).max(0.0);
            let pres_center = crate::geom::CanvasPos {
                x: crate::geom::q16(anchor.x as f64 - margin_x + extent.0 / 2.0),
                y: crate::geom::q16(anchor.y as f64 - margin_y + extent.1 / 2.0),
            };
            if let Some(p) = proof {
                self.cameras
                    .set_zoom_and_center(p, camera, 1.0, pres_center);
            }
            start = pres_center;
        }
        // M6 Idle → Panning: the press is consumed; KeepFocus (no event).
        // OB-4: Dismiss active popup grabs before the camera change.
        self.input.interaction = Interaction::CameraPan {
            output,
            camera,
            initiator,
            start,
            sum: (0.0, 0.0),
        };
        if self.fullscreen_pan
            && self.presentation(output).is_some()
            && !self.is_presentation_suppressed(output)
        {
            self.suppress_presentation(output, out);
        }
        if let Some(target) = self.focus.target() {
            out.push(Effect::DismissPopupGrab(target));
        }
        true
    }

    /// The end of a compositor interaction, by its initiator's release or by a cancel (M22):
    /// batch-end reconciliation (INV-INPUT-5), exactly one true-position event unless the
    /// reconciliation keeps a lock (M10, C12.F17 Q3: a kept confinement receives it), and the
    /// CameraPan epoch settles (INV-SCALE-3: it ends at interaction termination).
    pub(super) fn end_interaction(&mut self) -> Vec<Effect> {
        let resized = matches!(
            self.input.interaction,
            Interaction::ElementResize {
                is_cluster: false,
                ..
            }
        );
        self.input.cluster_drag.clear();
        self.input.interaction = Interaction::Idle;
        if matches!(self.input.gesture, Some(TouchpadGesture::Pan)) {
            self.input.gesture = None;
        }
        // a pan or a move put other windows under the pointer: no crossing either (finding 5)
        self.hover_seen = self.hovered_window();
        let mut fx = Vec::new();
        if resized {
            self.end_resize(&mut fx);
        }
        fx.extend(self.reconcile_constraints());
        if !self.locked() {
            fx.extend(self.true_position());
        }
        fx.extend(self.settle_interaction(crate::scale::InteractionKind::CameraPan));
        fx
    }

    /// A release follows the press owner (INV-INPUT-1). The initiator's release ends the
    /// interaction with batch-end reconciliation: one true-position event (M10). The effects are
    /// appended to `out` (the caller's reusable buffer, C14.3, INV-ALLOC-1).
    pub(super) fn button_release(&mut self, button: u32, time_ms: u32, out: &mut Vec<Effect>) {
        match self.input.ownership.release(button) {
            None => {}
            Some(Owner::Compositor) => {
                if !matches!(self.input.gesture, Some(TouchpadGesture::Pan))
                    && self.input.interaction.initiator() == Some(button)
                {
                    out.extend(self.end_interaction());
                } else if let Interaction::ElementMove { pan, .. } = &mut self.input.interaction
                    && pan.is_some_and(|p| p.button == button)
                {
                    // the nested pan ends; the window stays held; its pan epoch settles
                    *pan = None;
                    out.extend(self.settle_interaction(crate::scale::InteractionKind::CameraPan));
                }
            }
            // In NoOutput the zero-output grab receives it and delivers nothing, keeping
            // Smithay's pressed-button set truthful (no stuck implicit grab after reconnect).
            Some(Owner::Client) => {
                out.extend([
                    Effect::PointerButton {
                        button,
                        pressed: false,
                        time_ms,
                    },
                    Effect::PointerFrame,
                ]);
                // an app's own title bar drag: the press stays the app's, its release ends the drag
                if self.input.interaction.initiator() == Some(button) {
                    out.extend(self.end_interaction());
                }
                if !self.input.ownership.client_owns_any() {
                    let had_constraint = self.constraint.is_some();
                    out.extend(self.reconcile_constraints());
                    if had_constraint && self.constraint.is_none() {
                        self.true_position_into(out);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Core;
    use crate::entry::tests::*;
    use crate::geom::CanvasPos;
    use crate::input::bindings::{BTN_MIDDLE, Modifiers};
    use crate::navigation::PersistedCameras;
    use crate::output::Transform;
    use crate::output::tests::key;
    use crate::pointer::PointerState;
    use crate::protocol::Effect;

    fn pan(c: &mut Core, port: &mut crate::InputPort, d: (f64, f64)) {
        use crate::PhysicalEvent;
        use crate::input::bindings::{BTN_MIDDLE, Modifiers};
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let button = |pressed| PhysicalEvent::PointerButton {
            button: BTN_MIDDLE,
            pressed,
            modifiers: logo,
            time_us: 1,
        };
        c.physical_input_vec(port, button(true)).unwrap();
        let motion = PhysicalEvent::PointerMotion {
            delta: d,
            delta_unaccel: d,
            time_us: 2,
        };
        c.physical_input_vec(port, motion).unwrap();
        c.physical_input_vec(port, button(false)).unwrap();
    }

    /// `floor(v / 120)·120` per axis: the reference a (re)defined view takes.
    fn floor_reference(v: CanvasPos) -> (i64, i64) {
        (
            (v.x >> 16).div_euclid(120) * 120,
            (v.y >> 16).div_euclid(120) * 120,
        )
    }

    /// F-LATTICE-1's transitions on the production path: set at connect, kept across a zoomed pan
    /// that moves the view origin many lattice cells, kept by a reconfiguration that changes
    /// neither `k_O` nor `view_O`, set again when `k_O` or `view_O` changes, and dropped with the
    /// output. Render, membership and hit test read `Core::view`, so they share it.
    #[test]
    fn the_lattice_reference_follows_f_lattice_1_transitions() {
        let persisted = PersistedCameras::from_entries(vec![(key("A"), pos(0, 0), 0.73)]).unwrap();
        let (mut c, mut i, _s, mut t) = Core::new(persisted);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let v0 = c.view(a).unwrap();
        assert_eq!(
            v0.r,
            floor_reference(v0.v),
            "connect: set from the view origin"
        );
        pan(&mut c, &mut i, (1000.0, -700.0));
        let v1 = c.view(a).unwrap();
        assert_ne!(floor_reference(v1.v), v0.r, "the pan crossed lattice cells");
        assert_eq!(v1.r, v0.r, "kept while panning");
        // refresh only: k_O and view_O unchanged
        let refresh = desc_k(key("A"), (1920, 1080), Transform::Normal, 120, 75_000);
        c.output_reconfigured(&mut t, a, refresh).unwrap();
        assert_eq!(
            c.view(a).unwrap().r,
            v0.r,
            "kept: neither k_O nor view_O changed"
        );
        // a 180° transform keeps view_O
        let flipped = desc_k(key("A"), (1920, 1080), Transform::R180, 120, 75_000);
        c.output_reconfigured(&mut t, a, flipped).unwrap();
        assert_eq!(c.view(a).unwrap().r, v0.r, "kept: view_O unchanged by R180");
        // k_O changes: set again
        let scaled = desc_k(key("A"), (1920, 1080), Transform::R180, 150, 75_000);
        c.output_reconfigured(&mut t, a, scaled).unwrap();
        let v2 = c.view(a).unwrap();
        assert_eq!(
            v2.r,
            floor_reference(v2.v),
            "k_O changed: set from the origin"
        );
        assert_ne!(v2.r, v0.r);
        pan(&mut c, &mut i, (-900.0, 400.0));
        let v3 = c.view(a).unwrap();
        assert_eq!(v3.r, v2.r);
        assert_ne!(floor_reference(v3.v), v2.r);
        // view_O changes (90° exchanges the sides): set again
        let rotated = desc_k(key("A"), (1920, 1080), Transform::R90, 150, 75_000);
        c.output_reconfigured(&mut t, a, rotated).unwrap();
        let v4 = c.view(a).unwrap();
        assert_eq!(
            v4.r,
            floor_reference(v4.v),
            "view_O changed: set from the origin"
        );
        assert!(c.lattice.iter().any(|l| l.output == a));
        c.output_disconnected(&mut t, a).unwrap();
        assert!(
            c.lattice.iter().all(|l| l.output != a),
            "dropped with the output"
        );
        // the remembered camera is re-bound to a new OutputId: a new reference from its view
        let (b, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let v5 = c.view(b).unwrap();
        assert_eq!(
            v5.r,
            floor_reference(v5.v),
            "(re-)binding: set from the origin"
        );
    }

    /// TST-23 R1 (PD-F2-06, `experiments/phase-d/d5_model_1r.py`) as a T0 property on the
    /// production path: over the model's grid — scale {1, 1.5} × zoom {0.5, 0.75, 1, 1080/651,
    /// 2.5}, camera centre (5000.3, 3000.7) — one pan gesture of 200 events, each a hand
    /// displacement of (+3.7, −1.3) physical px, moves the content's edge at Canvas (5100, 3050)
    /// on screen by the accumulated hand displacement within the lattice rounding (≤ 1 px), after
    /// every event, on both axes. The sealed formula (camera moved by the logical Σ, not Σ / zoom)
    /// fails at every zoom ≠ 1 (seeded control).
    #[test]
    fn tst23_r1_content_follows_the_hand_at_every_zoom_and_scale() {
        use crate::PhysicalEvent;
        use crate::input::bindings::{BTN_MIDDLE, Modifiers};
        let q16 = |v: f64| (v * 65536.0).round_ties_even() as i64;
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        for scale_120 in [120u32, 180] {
            let s = scale_120 as f64 / 120.0;
            for zoom in [0.5, 0.75, 1.0, 1080.0 / 651.0, 2.5] {
                let centre = CanvasPos {
                    x: q16(5000.3),
                    y: q16(3000.7),
                };
                let persisted =
                    PersistedCameras::from_entries(vec![(key("A"), centre, zoom)]).unwrap();
                let (mut c, mut i, _s, mut t) = Core::new(persisted);
                let d = desc_k(key("A"), (1920, 1080), Transform::Normal, scale_120, 60_000);
                let (a, _) = c.output_connected(&mut t, d).unwrap();
                let v0 = c.view(a).unwrap();
                let e0 = (v0.edge_x(5100), v0.edge_y(3050));
                let mut time = 1;
                let mut ev = |c: &mut Core, e: PhysicalEvent| {
                    c.physical_input_vec(&mut i, e).unwrap();
                };
                ev(
                    &mut c,
                    PhysicalEvent::PointerButton {
                        button: BTN_MIDDLE,
                        pressed: true,
                        modifiers: logo,
                        time_us: time,
                    },
                );
                let (mut sx, mut sy) = (0.0f64, 0.0f64);
                let mut worst = 0.0f64;
                for _ in 0..200 {
                    time += 1000;
                    // the hand: (+3.7, −1.3) physical px = that over s in logical px
                    let delta = (3.7 / s, -1.3 / s);
                    ev(
                        &mut c,
                        PhysicalEvent::PointerMotion {
                            delta,
                            delta_unaccel: delta,
                            time_us: time,
                        },
                    );
                    sx += 3.7;
                    sy -= 1.3;
                    let v = c.view(a).unwrap();
                    let content = (
                        (v.edge_x(5100) - e0.0) as f64,
                        (v.edge_y(3050) - e0.1) as f64,
                    );
                    worst = worst
                        .max((content.0 - sx).abs())
                        .max((content.1 - sy).abs());
                }
                assert!(
                    worst <= 1.0,
                    "scale {s}, zoom {zoom}: content strays {worst} px from the hand"
                );
            }
        }
    }

    /// The kept-reference bound (F-LATTICE-1, C12.F14.4 item 1): at k = 2¹⁹ (zoom 2¹⁶, scale 8)
    /// a pan within `|v − R|·k ≤ 2³¹` keeps the reference; one beyond it sets it again on that
    /// axis only (the arithmetic is per axis).
    #[test]
    fn a_pan_beyond_the_reference_bound_sets_it_again_on_that_axis() {
        let zoom = 65536.0;
        let persisted = PersistedCameras::from_entries(vec![(key("A"), pos(0, 0), zoom)]).unwrap();
        let (mut c, mut i, _s, mut t) = Core::new(persisted);
        let big = desc_k(key("A"), (1920, 1080), Transform::Normal, 960, 60_000);
        let (a, _) = c.output_connected(&mut t, big).unwrap();
        let v0 = c.view(a).unwrap();
        assert_eq!(v0.k, (1u64 << 19) as f64);
        // 2000 Canvas px on x: |v − R|·k ≈ 2000·2¹⁹ < 2³¹ = 4096·2¹⁹; a logical px is 2⁻¹⁶ Canvas px
        pan(&mut c, &mut i, (-2000.0 * zoom, 0.0));
        let v1 = c.view(a).unwrap();
        assert_ne!(floor_reference(v1.v).0, v0.r.0);
        assert_eq!(v1.r, v0.r, "within the bound: kept");
        // 3000 more on x (5000 > 4096 from R) and 10 on y: x is set again, y is kept
        pan(&mut c, &mut i, (-3000.0 * zoom, -10.0 * zoom));
        let v2 = c.view(a).unwrap();
        assert_eq!(
            v2.r.0,
            floor_reference(v2.v).0,
            "beyond the bound: set again"
        );
        assert_eq!(v2.r.1, v0.r.1, "the other axis keeps its reference");
        // and the new reference is kept from then on
        pan(&mut c, &mut i, (-50.0 * zoom, 0.0));
        assert_eq!(c.view(a).unwrap().r, v2.r);
    }

    #[test]
    fn super_shift_scroll_and_middle_click_zoom_controls() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let cam = camera_of(&c, a);
        assert_eq!(c.cameras().zoom(cam), Some(1.0));

        let logo_shift = Modifiers {
            logo: true,
            shift: true,
            ctrl: false,
            alt: false,
        };

        // 1. Super+Shift key press updates seat modifiers
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_LEFTMETA,
                    pressed: true,
                    modifiers: logo_shift,
                    mods_changed: true,
                    time_us: 1000,
                },
            )
            .unwrap();

        // 2. Super+Shift+Scroll up (v120: negative) zooms in
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerAxis {
                    axis: crate::input::Axis {
                        source: crate::input::AxisSource::Wheel,
                        amount: (0.0, -15.0),
                        v120: (0, -120),
                    },
                    time_us: 2000,
                },
            )
            .unwrap();
        let zoomed = c.cameras().zoom(cam).unwrap();
        assert!(
            zoomed > 1.0,
            "zoom {zoomed} should be > 1.0 after scroll up"
        );

        // 3. Super+Shift+Middle Click no longer resets the zoom (owner, 2026-10-24)
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerButton {
                    button: BTN_MIDDLE,
                    pressed: true,
                    modifiers: logo_shift,
                    time_us: 3000,
                },
            )
            .unwrap();
        let after = c.cameras().zoom(cam).unwrap();
        assert_eq!(
            after, zoomed,
            "Super+Shift+Middle click leaves the zoom alone"
        );
    }

    #[test]
    fn axis_events_are_consumed_during_an_interaction_before_zoom() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let cam = camera_of(&c, a);
        assert_eq!(c.cameras().zoom(cam), Some(1.0));

        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let logo_shift = Modifiers {
            logo: true,
            shift: true,
            ..Modifiers::default()
        };

        // 1. Super+Middle press starts a CameraPan interaction (M22)
        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerButton {
                button: BTN_MIDDLE,
                pressed: true,
                modifiers: logo,
                time_us: 1000,
            },
        )
        .unwrap();
        assert!(c.input.interaction.active(), "interaction should be active");

        // 2. While interaction is active, Shift is pressed
        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::KeyboardKey {
                key: crate::input::keys::KEY_LEFTSHIFT,
                pressed: true,
                modifiers: logo_shift,
                mods_changed: true,
                time_us: 1100,
            },
        )
        .unwrap();

        // 3. Super+Shift+Scroll during the pan must be consumed (M22) without changing zoom
        let fx = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerAxis {
                    axis: crate::input::Axis {
                        source: crate::input::AxisSource::Wheel,
                        amount: (0.0, -15.0),
                        v120: (0, -120),
                    },
                    time_us: 1200,
                },
            )
            .unwrap();
        assert!(
            fx.is_empty(),
            "axis event during pan should produce no effects"
        );
        assert_eq!(
            c.cameras().zoom(cam),
            Some(1.0),
            "zoom must be untouched during an interaction"
        );

        // 4. Release ends the interaction
        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerButton {
                button: BTN_MIDDLE,
                pressed: false,
                modifiers: logo_shift,
                time_us: 1300,
            },
        )
        .unwrap();
        assert!(!c.input.interaction.active());
    }

    #[test]
    fn rts_pan_mode_updates_pointer_to_track_canvas() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (_a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_pan_mode(crate::navigation::PanMode::Rts);

        let PointerState::Located { p: p0, .. } = c.pointer else {
            panic!("pointer not located");
        };

        // Middle button press with Logo initiates pan
        let logo = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerButton {
                    button: BTN_MIDDLE,
                    pressed: true,
                    modifiers: logo,
                    time_us: 1000,
                },
            )
            .unwrap();

        // Pointer motion during RTS pan: delta (20.0, 10.0)
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (20.0, 10.0),
                    delta_unaccel: (20.0, 10.0),
                    time_us: 2000,
                },
            )
            .unwrap();

        let PointerState::Located { p: p1, .. } = c.pointer else {
            panic!("pointer not located");
        };
        assert_eq!(
            p1.0,
            p0.0 - 20.0,
            "pointer should shift opposite delta to track canvas"
        );
        assert_eq!(
            p1.1,
            p0.1 - 10.0,
            "pointer should shift opposite delta to track canvas"
        );
    }

    #[test]
    fn focus_follows_mouse_hover_focuses_window() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);

        let w = c.toplevel_created();
        let win = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(k).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        c.render_list(a).unwrap();

        assert_eq!(c.focus().target(), None);

        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (1.0, 1.0),
                    delta_unaccel: (1.0, 1.0),
                    time_us: 1000,
                },
            )
            .unwrap();

        assert_eq!(
            c.focus().target(),
            Some(w),
            "focus follows mouse should focus hovered window"
        );
    }

    #[test]
    fn focus_follows_mouse_strict_clears_focus_on_empty_canvas() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        c.set_focus_follows_mouse_strict(true);

        let w = c.toplevel_created();
        let win = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(k).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        c.render_list(a).unwrap();

        // 1. Move over the window at center (961, 541)
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (1.0, 1.0),
                    delta_unaccel: (1.0, 1.0),
                    time_us: 1000,
                },
            )
            .unwrap();
        assert_eq!(c.focus().target(), Some(w));

        // 2. Move away into empty canvas corner (delta: -900, -500 -> logical (61, 41))
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (-900.0, -500.0),
                    delta_unaccel: (-900.0, -500.0),
                    time_us: 2000,
                },
            )
            .unwrap();
        assert_eq!(
            c.focus().target(),
            None,
            "strict focus-follows-mouse must clear focus over empty canvas"
        );
    }

    /// Strict focus: a closed window's focus goes to the window under the pointer or none, never
    /// to the one used before it elsewhere; and Super+Shift+C closes nothing over empty canvas, even
    /// with a focus left on a window there (ThinkPad and desktop, 2026-10-27: "it would close a
    /// random window").
    #[test]
    fn strict_focus_never_hands_a_close_to_a_window_away_from_the_pointer() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        c.set_focus_follows_mouse_strict(true);
        let win = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(k).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        let w = c.toplevel_created();
        c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        let v = c.toplevel_created();
        c.toplevel_committed(v, win(2, 200, 150, false)).unwrap();
        c.toplevel_committed(v, win(2, 200, 150, true)).unwrap();
        let mut fx = Vec::new();
        // v in the top left corner of the view, w in its centre
        c.place_by_user(v, crate::geom::CanvasPoint { x: -950, y: -530 }, &mut fx)
            .unwrap();
        c.render_list(a).unwrap();
        let mut motion = |c: &mut Core, d: (f64, f64), time_us: u64| {
            let _ = c
                .physical_input_vec(
                    &mut i,
                    crate::PhysicalEvent::PointerMotion {
                        delta: d,
                        delta_unaccel: d,
                        time_us,
                    },
                )
                .unwrap();
            c.render_list(a).unwrap();
        };
        // over v (logical ~ (60, 60)), then over w (the centre)
        motion(&mut c, (-900.0, -480.0), 1000);
        assert_eq!(c.focus().target(), Some(v));
        motion(&mut c, (901.0, 481.0), 2000);
        assert_eq!(c.focus().target(), Some(w));

        // w closes under the still pointer: the canvas is empty there, nothing is focused
        let _ = c.toplevel_destroyed(w);
        c.render_list(a).unwrap();
        assert_eq!(c.focus().target(), None, "not v, the window used before");

        // a focus left on v (the canvas moved under a still pointer): Super+Shift+C closes nothing
        let _ = c.focus.set_focus(Some(v));
        assert_eq!(c.close_target(), None);
        // over v it closes v
        motion(&mut c, (-901.0, -481.0), 3000);
        assert_eq!(c.close_target(), Some(v));
    }

    #[test]
    fn zoom_glide_reaches_95_percent_and_keeps_pointer_point() {
        use crate::PhysicalEvent;
        use crate::entry::keys::{KEY_LEFTMETA, KEY_LEFTSHIFT};
        use crate::geom::Q16_ONE;
        use crate::input::{Axis, AxisSource};
        use crate::navigation::ZoomFeel;
        use crate::protocol::EffectBuf;
        use std::time::Duration;

        let period = Duration::from_micros(6_060);
        for (feel, want_ms) in [
            (ZoomFeel::Quick, 100.0),
            (ZoomFeel::Smooth, 165.0),
            (ZoomFeel::Floaty, 300.0),
        ] {
            let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
            let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
            c.set_zoom_feel(feel);
            let cam = camera_of(&c, a);
            let extent = c.output(a).unwrap().logical_extent();
            let p = (1317.3, 311.7);
            let mut fx = EffectBuf::default();
            // Position pointer and prime tick baseline
            c.physical_input(
                &mut i,
                PhysicalEvent::PointerMotion {
                    delta: (p.0 - extent.0 / 2.0, p.1 - extent.1 / 2.0),
                    delta_unaccel: (0.0, 0.0),
                    time_us: 10,
                },
                &mut fx,
            )
            .unwrap();
            c.zoom_tick(a, Duration::ZERO);

            let c0 = c.cameras().center(cam).unwrap();
            let z0 = c.cameras().zoom(cam).unwrap();
            let pt0 = (
                c0.x as f64 / Q16_ONE as f64 + (p.0 - extent.0 / 2.0) / z0,
                c0.y as f64 / Q16_ONE as f64 + (p.1 - extent.1 / 2.0) / z0,
            );

            // Trigger wheel zoom (Super+Shift+Wheel)
            let mods = Modifiers {
                logo: true,
                shift: true,
                ..Default::default()
            };
            c.physical_input(
                &mut i,
                PhysicalEvent::KeyboardKey {
                    key: KEY_LEFTMETA,
                    pressed: true,
                    modifiers: mods,
                    mods_changed: true,
                    time_us: 20,
                },
                &mut fx,
            )
            .unwrap();
            c.physical_input(
                &mut i,
                PhysicalEvent::KeyboardKey {
                    key: KEY_LEFTSHIFT,
                    pressed: true,
                    modifiers: mods,
                    mods_changed: true,
                    time_us: 25,
                },
                &mut fx,
            )
            .unwrap();
            c.physical_input(
                &mut i,
                PhysicalEvent::PointerAxis {
                    axis: Axis {
                        source: AxisSource::Wheel,
                        amount: (0.0, -30.0),
                        v120: (0, -240),
                    },
                    time_us: 30,
                },
                &mut fx,
            )
            .unwrap();
            assert!(c.zoom_gliding());

            let mut now = Duration::ZERO;
            let mut samples = Vec::new();
            let mut max_drift = 0.0_f64;
            for _ in 0..300 {
                now += period;
                let _ = c.zoom_tick(a, now);
                let cp = c.cameras().center(cam).unwrap();
                let z = c.cameras().zoom(cam).unwrap();
                let pt = (
                    cp.x as f64 / Q16_ONE as f64 + (p.0 - extent.0 / 2.0) / z,
                    cp.y as f64 / Q16_ONE as f64 + (p.1 - extent.1 / 2.0) / z,
                );
                max_drift = max_drift.max((pt.0 - pt0.0).hypot(pt.1 - pt0.1));
                samples.push((now.as_secs_f64() * 1000.0, z.ln()));
                if !c.zoom_gliding() {
                    break;
                }
            }
            assert!(!c.zoom_gliding(), "{feel:?} did not settle");
            assert!(max_drift <= 0.05, "{feel:?} drift {max_drift}");
            let zf_ln = samples.last().unwrap().1;
            let total = zf_ln - z0.ln();
            let t95 = samples
                .iter()
                .find(|&&(_, lz)| (lz - z0.ln()) / total >= 0.95)
                .map(|&(ms, _)| ms)
                .expect("95% sample");
            assert!(
                (t95 - want_ms).abs() <= want_ms * 0.10,
                "{feel:?} t95={t95:.2}ms vs {want_ms}ms"
            );
        }
    }

    #[test]
    fn floaty_wheel_momentum_decays_to_rest_and_pan_cancels_glide() {
        use crate::PhysicalEvent;
        use crate::entry::keys::{KEY_LEFTMETA, KEY_LEFTSHIFT};
        use crate::input::{Axis, AxisSource};
        use crate::navigation::ZoomFeel;
        use crate::protocol::EffectBuf;
        use std::time::Duration;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_zoom_feel(ZoomFeel::Floaty);
        let cam = camera_of(&c, a);
        let period = Duration::from_micros(6_060);
        c.zoom_tick(a, Duration::ZERO);

        let mods = Modifiers {
            logo: true,
            shift: true,
            ..Default::default()
        };
        let mut fx = EffectBuf::default();
        for k in [KEY_LEFTMETA, KEY_LEFTSHIFT] {
            c.physical_input(
                &mut i,
                PhysicalEvent::KeyboardKey {
                    key: k,
                    pressed: true,
                    modifiers: mods,
                    mods_changed: true,
                    time_us: 10,
                },
                &mut fx,
            )
            .unwrap();
        }
        c.physical_input(
            &mut i,
            PhysicalEvent::PointerAxis {
                axis: Axis {
                    source: AxisSource::Wheel,
                    amount: (0.0, -30.0),
                    v120: (0, -240),
                },
                time_us: 20,
            },
            &mut fx,
        )
        .unwrap();
        let v0 = c.cameras().glide_velocity(cam).unwrap();
        assert!(v0 > 0.0);
        let mut now = period;
        let _ = c.zoom_tick(a, now);
        let v1 = c.cameras().glide_velocity(cam).unwrap();
        assert!(v1 < v0 && v1 > 0.0);

        // Middle mouse pan cancels active glide cleanly
        c.physical_input(
            &mut i,
            PhysicalEvent::PointerButton {
                button: BTN_MIDDLE,
                pressed: true,
                modifiers: Modifiers {
                    logo: true,
                    ..Default::default()
                },
                time_us: 30,
            },
            &mut fx,
        )
        .unwrap();
        c.physical_input(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (20.0, 10.0),
                delta_unaccel: (20.0, 10.0),
                time_us: 40,
            },
            &mut fx,
        )
        .unwrap();
        assert!(!c.zoom_gliding());
        now += period;
        assert!(c.zoom_tick(a, now).is_empty());
    }

    #[test]
    fn audit_hover_focus_steals_keyboard_focus_while_client_button_held() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: -450, y: -150 },
            &mut fx,
        )
        .unwrap();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 50, y: -150 }, &mut fx)
            .unwrap();
        c.render_list(a).unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (700.0, 540.0),
        };
        c.hover_seen = Some(ws[0]);
        c.set_focus(&mut i, Some(ws[0])).unwrap();

        let press_fx = mouse_button(&mut c, &mut i, true);
        assert!(
            press_fx
                .iter()
                .any(|e| matches!(e, Effect::PointerButton { pressed: true, .. })),
            "expected client to own the pressed button"
        );
        assert!(c.input.ownership.client_owns_any());

        mouse_move(&mut c, &mut i, (400.0, 0.0));
        assert_eq!(
            c.focus().target(),
            Some(ws[0]),
            "keyboard focus should stay with the window holding the pressed mouse button"
        );
    }

    #[test]
    fn audit_hover_focus_does_not_activate_pending_pointer_constraint() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: -450, y: -150 },
            &mut fx,
        )
        .unwrap();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 50, y: -150 }, &mut fx)
            .unwrap();
        c.render_list(a).unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (700.0, 540.0),
        };
        c.hover_seen = Some(ws[0]);
        c.set_focus(&mut i, Some(ws[0])).unwrap();

        let w1_surf = crate::snapshot::SurfaceKey::from_raw(std::num::NonZeroU64::new(2).unwrap());
        let pending_fx = c.request_pointer_constraint(
            w1_surf,
            crate::pointer::ConstraintKind::Lock,
            crate::pointer::ConstraintLifetime::Persistent,
            None,
            None,
        );
        assert!(pending_fx.is_empty());

        let move_fx = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (400.0, 0.0),
                    delta_unaccel: (400.0, 0.0),
                    time_us: 2000,
                },
            )
            .unwrap();
        assert_eq!(c.focus().target(), Some(ws[1]));
        assert!(
            move_fx.iter().any(|e| matches!(
                e,
                Effect::ConstraintActivate { surface, .. } if *surface == w1_surf
            )) && c.constraint().is_some(),
            "hover-focusing ws[1] should reconcile and activate its pending pointer constraint"
        );
    }

    #[test]
    fn coming_back_from_a_clicked_panel_gives_the_window_the_keyboard() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::layer::{Anchors, KeyboardInteractivity, Layer, LayerState};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        let layer_tree = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(std::num::NonZeroU64::new(k).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        let ws = map_windows(&mut c, a, 1);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: -200, y: -540 },
            &mut fx,
        )
        .unwrap();
        c.render_list(a).unwrap();

        let panel = c.layer_created(Some(a)).unwrap();
        let lstate = LayerState {
            layer: Some(Layer::Top),
            anchors: Anchors {
                top: true,
                bottom: false,
                left: true,
                right: true,
            },
            size: (0, 80),
            margins: (0, 0, 0, 0),
            exclusive_zone: 0,
            keyboard: KeyboardInteractivity::OnDemand,
        };
        let lfx = c
            .layer_committed(panel, lstate, layer_tree(99, 0, 0, false))
            .unwrap();
        let lsize = lfx
            .iter()
            .find_map(|e| match e {
                Effect::LayerConfigure { layer, size } if *layer == panel => Some(*size),
                _ => None,
            })
            .expect("layer configure");
        c.layer_committed(panel, lstate, layer_tree(99, lsize.w, lsize.h, true))
            .unwrap();

        c.pointer = PointerState::Located {
            output: a,
            p: (960.0, 150.0),
        };
        mouse_move(&mut c, &mut i, (0.0, 1.0));
        assert_eq!(c.focus().target(), Some(ws[0]));
        assert_eq!(c.hover_seen, Some(ws[0]));

        // Move onto the Top layer strip and click it (OnDemand layer keyboard focus)
        mouse_move(&mut c, &mut i, (0.0, -110.0));
        assert!(c.layer_hit(a, (960.0, 41.0), true).is_some());
        mouse_button(&mut c, &mut i, true);
        mouse_button(&mut c, &mut i, false);
        assert_eq!(c.focus().layer(), Some(panel));

        // Move off the panel back onto ws[0]
        mouse_move(&mut c, &mut i, (0.0, 110.0));
        assert_eq!(
            (c.focus().layer(), c.focus().target()),
            (None, Some(ws[0])),
            "leaving the OnDemand layer back onto ws[0] should clear layer focus and focus ws[0]"
        );
    }

    /// Strict focus (owner, 2026-11-22: "when panels/edge is open keyboard cant interact with the
    /// windows"): an open panel asks for the keyboard on demand, and a click on a window takes it
    /// back from the panel, even when that window was focused before the panel was clicked.
    #[test]
    fn a_click_on_a_window_takes_the_keyboard_from_an_on_demand_panel() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::layer::{Anchors, KeyboardInteractivity, Layer, LayerState};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        let layer_tree = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(std::num::NonZeroU64::new(k).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(false);
        let ws = map_windows(&mut c, a, 1);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: -200, y: -540 },
            &mut fx,
        )
        .unwrap();
        c.render_list(a).unwrap();

        let panel = c.layer_created(Some(a)).unwrap();
        let lstate = LayerState {
            layer: Some(Layer::Top),
            anchors: Anchors {
                top: true,
                bottom: false,
                left: true,
                right: true,
            },
            size: (0, 80),
            margins: (0, 0, 0, 0),
            exclusive_zone: 0,
            keyboard: KeyboardInteractivity::OnDemand,
        };
        let lfx = c
            .layer_committed(panel, lstate, layer_tree(99, 0, 0, false))
            .unwrap();
        let lsize = lfx
            .iter()
            .find_map(|e| match e {
                Effect::LayerConfigure { layer, size } if *layer == panel => Some(*size),
                _ => None,
            })
            .expect("layer configure");
        c.layer_committed(panel, lstate, layer_tree(99, lsize.w, lsize.h, true))
            .unwrap();

        c.pointer = PointerState::Located {
            output: a,
            p: (960.0, 150.0),
        };
        mouse_button(&mut c, &mut i, true);
        mouse_button(&mut c, &mut i, false);
        assert_eq!(c.focus().target(), Some(ws[0]));

        // the panel is clicked (a slider in it): it takes the keyboard
        mouse_move(&mut c, &mut i, (0.0, -110.0));
        assert!(c.layer_hit(a, (960.0, 41.0), true).is_some());
        mouse_button(&mut c, &mut i, true);
        mouse_button(&mut c, &mut i, false);
        assert_eq!(c.focus().layer(), Some(panel));

        // strict focus: going back over the window keeps the keyboard on the panel
        mouse_move(&mut c, &mut i, (0.0, 110.0));
        assert_eq!(c.focus().layer(), Some(panel));

        let fx = mouse_button(&mut c, &mut i, true);
        mouse_button(&mut c, &mut i, false);
        assert_eq!(
            (c.focus().layer(), c.focus().target()),
            (None, Some(ws[0])),
            "a click on the window takes the keyboard from the on-demand panel"
        );
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::KeyboardFocus(Some(w)) if *w == ws[0])),
            "the adapter is told to give the window the keyboard: {fx:?}"
        );
    }

    #[test]
    fn an_input_layer_over_the_window_blocks_its_pointer_constraint() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::layer::{Anchors, KeyboardInteractivity, Layer, LayerState};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        let layer_tree = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(std::num::NonZeroU64::new(k).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 1);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: -200, y: -150 },
            &mut fx,
        )
        .unwrap();
        c.render_list(a).unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();

        let overlay = c.layer_created(Some(a)).unwrap();
        let lstate = LayerState {
            layer: Some(Layer::Overlay),
            anchors: Anchors {
                top: true,
                bottom: true,
                left: true,
                right: true,
            },
            size: (0, 0),
            margins: (0, 0, 0, 0),
            exclusive_zone: -1,
            keyboard: KeyboardInteractivity::None,
        };
        let lfx = c
            .layer_committed(overlay, lstate, layer_tree(99, 0, 0, false))
            .unwrap();
        let lsize = lfx
            .iter()
            .find_map(|e| match e {
                Effect::LayerConfigure { layer, size } if *layer == overlay => Some(*size),
                _ => None,
            })
            .expect("layer configure");
        c.layer_committed(overlay, lstate, layer_tree(99, lsize.w, lsize.h, true))
            .unwrap();

        c.pointer = PointerState::Located {
            output: a,
            p: (960.0, 540.0),
        };
        let routed = c
            .route_pointer()
            .and_then(|(tgt, _)| tgt)
            .map(|t| t.surface);
        let overlay_surf = SurfaceKey::from_raw(std::num::NonZeroU64::new(99).unwrap());
        assert_eq!(routed, Some(overlay_surf));

        let w0_surf = SurfaceKey::from_raw(std::num::NonZeroU64::new(1).unwrap());
        let cfx = c.request_pointer_constraint(
            w0_surf,
            crate::pointer::ConstraintKind::Lock,
            crate::pointer::ConstraintLifetime::Persistent,
            None,
            None,
        );
        assert!(
            cfx.is_empty() && c.constraint().is_none(),
            "an Overlay layer covering the pointer must block constraint activation on the window underneath"
        );
    }

    #[test]
    fn audit_zoom_glide_after_focus_next_leaves_hover_seen_stale_so_nudge_steals_focus() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        c.set_focus_cycle_navigate(true);
        c.set_zoom_feel(crate::navigation::ZoomFeel::Floaty);
        let ws = map_windows(&mut c, a, 3);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: 1800, y: -150 },
            &mut fx,
        )
        .unwrap();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 800, y: -150 }, &mut fx)
            .unwrap();
        c.place_by_user(ws[2], crate::geom::CanvasPoint { x: -10, y: -150 }, &mut fx)
            .unwrap();
        c.render_list(a).unwrap();

        c.set_focus(&mut i, Some(ws[1])).unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (150.0, 540.0),
        };
        c.hover_seen = None;
        let _ = c.zoom_tick(a, std::time::Duration::ZERO);

        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(ws[1]));
        let mut now = std::time::Duration::from_millis(16);
        for _ in 0..240 {
            if !c.zoom_gliding() {
                break;
            }
            let _ = c.zoom_tick(a, now);
            now += std::time::Duration::from_millis(16);
        }
        assert!(!c.zoom_gliding());
        c.render_list(a).unwrap();
        assert_eq!(c.hovered_window(), Some(ws[2]));
        assert_eq!(c.focus().target(), Some(ws[1]));

        // Pointer did not cross into ws[2] — the glide brought ws[2] underneath it.
        mouse_move(&mut c, &mut i, (1.0, 0.0));
        assert_eq!(
            c.focus().target(),
            Some(ws[1]),
            "a 1px nudge inside ws[2] after a camera glide must not steal focus from ws[1]"
        );
    }

    #[test]
    fn audit_camera_pan_leaves_hover_seen_stale_so_nudge_steals_focus() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: -400, y: -150 },
            &mut fx,
        )
        .unwrap();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 800, y: -150 }, &mut fx)
            .unwrap();
        c.render_list(a).unwrap();

        c.set_focus(&mut i, Some(ws[0])).unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (960.0, 540.0),
        };
        c.hover_seen = None;

        // Pan camera right by 1000 px (in Grab mode, moving pointer left pans camera right)
        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerButton {
                button: crate::input::bindings::BTN_MIDDLE,
                pressed: true,
                modifiers: Modifiers {
                    logo: true,
                    ..Default::default()
                },
                time_us: 1000,
            },
        )
        .unwrap();
        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerMotion {
                delta: (-1000.0, 0.0),
                delta_unaccel: (-1000.0, 0.0),
                time_us: 2000,
            },
        )
        .unwrap();
        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerButton {
                button: crate::input::bindings::BTN_MIDDLE,
                pressed: false,
                modifiers: Modifiers {
                    logo: true,
                    ..Default::default()
                },
                time_us: 3000,
            },
        )
        .unwrap();
        c.render_list(a).unwrap();
        assert_eq!(c.hovered_window(), Some(ws[1]));
        assert_eq!(c.focus().target(), Some(ws[0]));

        mouse_move(&mut c, &mut i, (1.0, 0.0));
        assert_eq!(
            c.focus().target(),
            Some(ws[0]),
            "a 1px nudge inside ws[1] after panning must not steal focus from ws[0]"
        );
    }

    #[test]
    fn tablet_tap_focuses_and_raises_window_and_route_point_follows_camera() {
        use crate::{NavigateTo, PhysicalEvent, ShellCommand, ShellStream};
        let (mut c, mut i, mut s, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(ws[0], crate::geom::CanvasPoint { x: 100, y: 200 }, &mut fx)
            .unwrap();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 600, y: 200 }, &mut fx)
            .unwrap();
        c.set_focus(&mut i, Some(ws[1])).unwrap();
        assert_eq!(c.focus().target(), Some(ws[1]));

        // Pan camera to (300, 350) at zoom 2.0: output (960, 540) maps to canvas (300, 350),
        // which is inside ws[0] at local (200, 150).
        c.shell_command(
            &mut s,
            ShellCommand::Navigate {
                output: a,
                to: NavigateTo::Canvas(300.0, 350.0),
                zoom: Some(2.0),
                stream: ShellStream::Atomic,
            },
        )
        .unwrap();

        // Output logical point (990, 560) is (+30, +20) from view centre -> (+15, +10) canvas ->
        // canvas (315, 360), local (215, 160) on ws[0].
        let (target, loc) = c.route_point(a, (990.0, 560.0)).expect("routed");
        let target = target.expect("hit ws[0]");
        assert_eq!(target.origin, crate::geom::CanvasPoint { x: 100, y: 200 });
        assert_eq!(
            (
                (loc.x - target.origin.x * crate::geom::Q16_ONE) / crate::geom::Q16_ONE,
                (loc.y - target.origin.y * crate::geom::Q16_ONE) / crate::geom::Q16_ONE,
            ),
            (215, 160)
        );

        let tap_fx = c
            .physical_input_vec(
                &mut i,
                PhysicalEvent::TabletTap {
                    output: a,
                    p: (990.0, 560.0),
                    time_us: 1_000,
                },
            )
            .unwrap();
        assert_eq!(c.focus().target(), Some(ws[0]));
        assert_eq!(c.stage().top_mapped(), Some(ws[0]));
        assert!(tap_fx.contains(&Effect::KeyboardFocus(Some(ws[0]))));
    }

    #[test]
    fn tablet_tap_on_empty_canvas_keeps_focus() {
        use crate::PhysicalEvent;
        let (mut c, mut i, _s, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 1);
        let mut fx = Vec::new();
        c.place_by_user(ws[0], crate::geom::CanvasPoint { x: 0, y: 0 }, &mut fx)
            .unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        assert_eq!(c.focus().target(), Some(ws[0]));

        // Outside the 400x300 window on empty canvas: does not change focus.
        let tap_fx = c
            .physical_input_vec(
                &mut i,
                PhysicalEvent::TabletTap {
                    output: a,
                    p: (1500.0, 900.0),
                    time_us: 1_000,
                },
            )
            .unwrap();
        assert_eq!(c.focus().target(), Some(ws[0]));
        assert!(!tap_fx.iter().any(|e| matches!(e, Effect::KeyboardFocus(_))));
    }
}
