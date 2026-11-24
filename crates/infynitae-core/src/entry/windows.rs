//! Window size and place changes the user or the app asks for: fit, maximize, fullscreen, app drags.

use super::*;

impl Core {
    /// M8 `FitViewport(O, W)` from the input binding: O is the pointer's output, W the element
    /// under the pointer (its window geometry snapshotted now). Refused — no camera mutation,
    /// nothing recorded or evicted, no pointer reconciliation — when no output holds the pointer,
    /// O presents a protocol-fullscreen element (C12.F17 Q1 B), no mapped element is under the
    /// pointer, or its geometry is degenerate (F-FIT-1). Otherwise one proof, one mutation (the
    /// prior value retained, OBJ-30), and the camera-write consequences.
    pub(super) fn fit_viewport(&mut self, proof: authority::Authorized, out: &mut Vec<Effect>) {
        let PointerState::Located { output, p } = self.pointer else {
            return;
        };
        if self.presentation(output).is_some() && !self.is_presentation_suppressed(output) {
            return;
        }
        let pt = crate::geom::OutputLogicalPoint { x: p.0, y: p.1 };
        // a panel or launcher above the Canvas covers what is under the pointer (L3)
        if self.layer_hit(output, p, true).is_some() {
            return;
        }
        let Some(hit) = self.hit_test(output, pt) else {
            return;
        };
        let element = hit.element;
        let _ = self.fit_on(proof, output, element, out);
    }

    /// Super+Enter: with the same guards as the fit binding, the element under the pointer is fitted,
    /// unless the camera already holds exactly that fit (and there is a value to go back to): then
    /// the previous camera is restored. When enter_mode is Maximize, it maximizes the window to
    /// natively fill the usable viewport (toggleable).
    pub(super) fn fit_toggle(&mut self, proof: authority::Authorized, out: &mut Vec<Effect>) {
        let PointerState::Located { output, p } = self.pointer else {
            return;
        };
        if self.presentation(output).is_some() && !self.is_presentation_suppressed(output) {
            return;
        }
        let pt = crate::geom::OutputLogicalPoint { x: p.0, y: p.1 };
        if self.layer_hit(output, p, true).is_some() {
            return;
        }
        if self.enter_mode == crate::EnterMode::Maximize {
            let hit = self.hit_test(output, pt);
            let element = match hit.map(|h| h.element).or_else(|| self.focus.target()) {
                Some(e) => e,
                None => return,
            };
            self.maximize_toggle(Some(proof), output, element, true, out);
            return;
        }
        let Some(hit) = self.hit_test(output, pt) else {
            return;
        };
        let element = hit.element;
        let at_fit = self
            .fit_plan(output, element)
            .ok()
            .is_some_and(|(camera, target)| {
                self.cameras.effective_value(camera).is_some_and(|v| {
                    v.center == target.center && v.zoom.to_bits() == target.zoom.to_bits()
                }) && self.cameras.history(camera).is_some_and(|h| !h.is_empty())
            });
        if at_fit {
            self.viewport_restore(proof, out);
        } else {
            let _ = self.fit_on(proof, output, element, out);
        }
    }

    /// Viewport fill toggle (EnterMode::Maximize): maximizes the window to fill
    /// the usable screen area, or restores it.
    /// Without `proof` (an app's own button) no camera is written: apps never move it, so at
    /// another zoom than 1 the window stays as it is and the app is only answered.
    /// `into_free`: the window takes the screen's size where it covers no other window, out of
    /// view if need be (owner, 2026-11-10); otherwise it fills the usable area where the view is
    /// (a window opening maximized, which `maximize_opened` then moves to free canvas).
    pub(super) fn maximize_toggle(
        &mut self,
        proof: Option<authority::Authorized>,
        output: OutputId,
        element: crate::stage::ElementId,
        into_free: bool,
        out: &mut Vec<Effect>,
    ) {
        if self.stage.mapped(element).is_none() {
            return;
        }
        if let Some((orig_anchor, orig_size)) = self.stage.saved_floating(element) {
            // moved while maximized (a title bar drag keeps the maximize): it comes back at its
            // size before, centred where it is now, not at the place it left (owner, 2026-11-06)
            let now = self.stage.anchor(element);
            let put = self
                .maximized_at
                .iter()
                .find(|(e, _)| *e == element)
                .map(|(_, a)| *a);
            let current = self.stage.tree(element).map(|t| t.geometry.size);
            let moved = matches!((now, put), (Some(n), Some(p)) if n != p);
            let back_to = match (moved, now, current) {
                (true, Some(n), Some(cur)) => crate::geom::CanvasPoint {
                    x: n.x + i64::from(cur.w) / 2 - i64::from(orig_size.w) / 2,
                    y: n.y + i64::from(cur.h) / 2 - i64::from(orig_size.h) / 2,
                },
                _ => orig_anchor,
            };
            self.stage.set_saved_floating(element, None);
            self.set_maximized(element, None);
            let _ = self.place_by_shell(element, back_to, out);
            out.push(Effect::ToplevelConfigureSize {
                element,
                size: orig_size,
            });
            self.invalidate_frame(output);
            out.extend(self.visibility_pass());
        } else {
            let Some(usable) = self.usable_area(output) else {
                return;
            };
            let (Some(orig_size), Some(anchor)) = (
                self.stage.tree(element).map(|t| t.geometry.size),
                self.stage.anchor(element),
            ) else {
                return;
            };

            // If camera zoom is not 1.0, reset camera zoom to 1.0
            if let Some(camera) = self.cameras.camera_for_output(output) {
                let zoom = self.cameras.zoom(camera).unwrap_or(1.0);
                if (zoom - 1.0).abs() > 0.001 {
                    let Some(proof) = proof else {
                        return;
                    };
                    if let Some(center) = self.cameras.center(camera) {
                        self.cameras.set_zoom_and_center(proof, camera, 1.0, center);
                        self.camera_written(output, camera, out);
                    }
                }
            }

            if self.current_frame(output).is_none() {
                let _ = self.render_list(output);
            }
            let Some(frame) = self.current_frame(output) else {
                return;
            };
            let view = frame.view;

            let top_left_logical = crate::geom::OutputLogicalPoint {
                x: usable.0,
                y: usable.1,
            };
            let Some(tl) = view.unproject(top_left_logical).round_to_point() else {
                return;
            };

            let canvas_w = usable.2.round().max(1.0) as i32;
            let canvas_h = usable.3.round().max(1.0) as i32;
            let mut tl = tl;
            if into_free {
                // its own dialogs do not count: they sit over it
                let others: Vec<_> = self
                    .stage
                    .mapped_elements()
                    .into_iter()
                    .filter(|&e| {
                        e != element
                            && self.toplevel_parent(e) != Some(element)
                            && !self.stage.is_minimized(e) // hidden: nothing to cover
                    })
                    .filter_map(|e| {
                        let (a, t) = (self.stage.anchor(e)?, self.stage.tree(e)?);
                        let (w, h) = (i64::from(t.geometry.size.w), i64::from(t.geometry.size.h));
                        (w > 0 && h > 0).then_some((a.x, a.y, a.x + w, a.y + h))
                    })
                    .collect();
                // the screen's size, where it covers no other window: the view's top left when
                // that is free, else the nearest free spot on the canvas, out of view if need be
                // (owner, 2026-11-10: "it's supposed to go out of the viewport so I could Super+Space
                // to it and it would be fullscreen"); the camera stays where it is
                let size = (i64::from(canvas_w), i64::from(canvas_h));
                if crate::protocol::overlaps_any(&others, (tl.x, tl.y, size.0, size.1)) {
                    let (x, y) = self
                        .free_spot_near(&others, size, (tl.x, tl.y))
                        .unwrap_or_else(|| crate::protocol::beside_all(&others, (tl.x, tl.y)));
                    tl = crate::geom::CanvasPoint { x, y };
                }
            }

            self.stage
                .set_saved_floating(element, Some((anchor, orig_size)));
            // it follows the output's changes (INV-OUTPUT-2, amended 2026-10-31)
            self.set_maximized(element, Some(output));
            self.maximized_at.push((element, tl));
            let _ = self.place_by_shell(element, tl, out);
            // no other window moves (owner, 2026-11-06), and none is covered (2026-11-09)
            out.push(Effect::ToplevelConfigureSize {
                element,
                size: crate::geom::LocalSize {
                    w: canvas_w,
                    h: canvas_h,
                },
            });
            self.invalidate_frame(output);
            out.extend(self.visibility_pass());
        }
    }

    /// An app asks to move its window, or to resize it from `edges` (`xdg_toplevel.move`,
    /// `.resize`; `_NET_WM_MOVERESIZE` through the bridge). Granted only while the user holds a
    /// press the app owns with the pointer on that window: an app can never move itself by its
    /// own decision. The press stays the app's, so the release reaches it and ends the drag.
    pub fn client_drag(
        &mut self,
        id: crate::stage::ElementId,
        edges: Option<crate::input::interaction::ResizeEdges>,
    ) {
        let PointerState::Located { output, p } = self.pointer else {
            return;
        };
        let held = self
            .input
            .ownership
            .pressed()
            .iter()
            .find(|(_, o)| *o == Owner::Client)
            .map(|(b, _)| *b);
        let Some(button) = held else {
            return;
        };
        if self.input.interaction.active() || self.locked() || self.fullscreen_of(id).is_some() {
            return;
        }
        let pt = crate::geom::OutputLogicalPoint { x: p.0, y: p.1 };
        let Some(hit) = self.hit_test(output, pt).filter(|h| h.element == id) else {
            return;
        };
        let (Some(anchor), Some(size)) = (
            self.stage.anchor(id),
            self.stage.tree(id).map(|t| t.geometry.size),
        ) else {
            return;
        };
        let press = crate::geom::CanvasPos {
            x: hit.origin.x * crate::geom::Q16_ONE + hit.local.x,
            y: hit.origin.y * crate::geom::Q16_ONE + hit.local.y,
        };
        self.input.cluster_drag.clear();
        self.input.interaction = match edges {
            None => Interaction::ElementMove {
                element: id,
                initiator: button,
                grab_offset: crate::geom::CanvasPos {
                    x: press.x - anchor.x * crate::geom::Q16_ONE,
                    y: press.y - anchor.y * crate::geom::Q16_ONE,
                },
                pan: None,
                is_cluster: false,
            },
            Some(e) => {
                let start_size = self
                    .begin_resize(id, e.left, e.top)
                    .unwrap_or((size.w, size.h));
                Interaction::ElementResize {
                    element: id,
                    initiator: button,
                    left: e.left,
                    top: e.top,
                    axes: (e.left || e.right, e.top || e.bottom),
                    start_size,
                    press,
                    is_cluster: false,
                }
            }
        };
    }

    /// An app's maximize button (`xdg_toplevel.set_maximized`, `.unset_maximized`): the same as
    /// Super+Enter maximizing, only for the focused window. Always answers with a configure.
    pub fn client_maximize(&mut self, id: crate::stage::ElementId, on: bool) -> Vec<Effect> {
        let mut out = Vec::new();
        let state = self.stage.state(id);
        let answerable = matches!(
            state,
            Some(crate::stage::Lifecycle::AwaitingFirstBuffer | crate::stage::Lifecycle::Mapped)
        );
        // not shown yet: the request is kept for when it maps (owner, 2026-11-06: Firefox,
        // saved maximized, reopened at a 240 x 292 size it had kept for its unmaximized state)
        if self.stage.mapped(id).is_none() {
            self.maximize_on_map.retain(|&e| e != id);
            if on {
                self.maximize_on_map.push(id);
            }
        }
        if !answerable {
            return out; // nothing is answered before the first configure or while unmapped
        }
        let is_on = self.maximized.iter().any(|(e, _)| *e == id);
        // an unmaximize while the user holds a press on the window is an app's "drag to restore"
        // (GTK, Firefox, Chromium ask it as a title bar drag starts): the window stays maximized
        // and only moves (owner, 2026-11-06: "moving an app I maximized resets the maximize").
        // Super+Enter still restores it.
        let press_held = self
            .input
            .ownership
            .pressed()
            .iter()
            .any(|(_, o)| *o == Owner::Client);
        let pointer = self.pointer;
        let dragging_it = !on
            && is_on
            && press_held
            && match pointer {
                PointerState::Located { output, p } => self
                    .hit_test(output, crate::geom::OutputLogicalPoint { x: p.0, y: p.1 })
                    .is_some_and(|h| h.element == id),
                PointerState::NoOutput => false,
            };
        let output = self
            .pointer_output()
            .or_else(|| self.outputs.first().map(|o| o.id));
        if let Some(output) = output
            && !dragging_it
            && on != is_on
            && self.focus.target() == Some(id)
            && self.fullscreen_of(id).is_none()
            && !self.input.interaction.active()
            && self.stage.mapped(id).is_some()
        {
            self.maximize_toggle(None, output, id, true, &mut out);
        }
        // the toggle's own configure answers it; exactly one in every case
        if !out
            .iter()
            .any(|e| matches!(e, Effect::ToplevelConfigureSize { element, .. } if *element == id))
        {
            out.push(Effect::ToplevelConfigure(id));
        }
        out
    }

    /// Whether `id` asked to be maximized before it was shown (`client_maximize`); asked once,
    /// when it maps.
    pub fn take_maximize_on_map(&mut self, id: crate::stage::ElementId) -> bool {
        let had = self.maximize_on_map.contains(&id);
        self.maximize_on_map.retain(|&e| e != id);
        had
    }

    /// A window that has just opened fills the screen under the pointer, as Super+Enter's
    /// maximize does (it asked before it was shown); nothing for a fullscreen window or one
    /// already maximized.
    pub fn maximize_opened(&mut self, id: crate::stage::ElementId) -> Vec<Effect> {
        let mut out = Vec::new();
        let output = self
            .pointer_output()
            .or_else(|| self.outputs.first().map(|o| o.id));
        if let Some(output) = output
            && self.fullscreen_of(id).is_none()
            && !self.maximized.iter().any(|(e, _)| *e == id)
        {
            self.maximize_toggle(None, output, id, false, &mut out);
            // a maximize the user asks for covers what the screen shows; one a new window asked
            // for (Discord reopening maximized) covers no other app's window: it fills a screen's
            // room in the nearest free space instead (owner, 2026-11-07)
            let put = self
                .maximized_at
                .iter()
                .find(|(e, _)| *e == id)
                .map(|(_, a)| *a);
            if let (Some(tl), Some(usable)) = (put, self.usable_area(output)) {
                let size = (
                    usable.2.round().max(1.0) as i64,
                    usable.3.round().max(1.0) as i64,
                );
                if let Some(free) = self.free_place_for(id, size, tl)
                    && self.place_by_shell(id, free, &mut out).is_ok()
                {
                    for (e, a) in &mut self.maximized_at {
                        if *e == id {
                            *a = free;
                        }
                    }
                    self.invalidate_frame(output);
                    out.extend(self.visibility_pass());
                }
            }
        }
        out
    }

    /// A new window the rules let in over a fullscreen window (the user opened it, or it is a
    /// dialog of an app): it is shown and takes the focus. `set_focus` alone would decline
    /// (A-8), leaving it hidden behind the fullscreen window.
    pub fn show_opened(
        &mut self,
        port: &mut InputPort,
        id: crate::stage::ElementId,
    ) -> Result<Vec<Effect>, EntryError> {
        self.check_port(port)?;
        if self.stage.state(id) != Some(crate::stage::Lifecycle::Mapped) {
            return Err(EntryError::InvalidEvent);
        }
        let had_constraint = self.constraint.is_some();
        let mut out = Vec::new();
        self.show_chosen(id, &mut out);
        out.extend(self.focus.set_focus_at(self.input.clock.now_us(), Some(id)));
        out.extend(self.reconcile_constraints());
        if had_constraint && self.constraint.is_none() {
            self.true_position_into(&mut out);
        }
        Ok(out)
    }

    /// The user chose `id`: its own fullscreen comes back, another one on the pointer's screen,
    /// or one that would hide it, steps aside.
    ///
    /// Screen by screen, not only the pointer's (B2, B6): a screen whose fullscreen window is
    /// `id` presents it again; a screen presenting another window steps aside when its view
    /// covers `id`, or when it is the pointer's screen and no other screen shows `id` (the
    /// camera is about to bring it there). Other screens keep their fullscreen window.
    pub(super) fn show_chosen(&mut self, id: crate::stage::ElementId, out: &mut Vec<Effect>) {
        let pointer_on = self.pointer_output();
        let shown_elsewhere = self.membership.observing_outputs(id).next().is_some();
        let covers: Vec<_> = self
            .outputs
            .iter()
            .map(|o| (o.id, self.view_covers(o.id, id)))
            .collect();
        for (o, covered) in covers {
            match self.presentation(o) {
                Some(p) if p == id => self.unsuppress_presentation(o, out),
                Some(_) if covered || (pointer_on == Some(o) && !shown_elsewhere) => {
                    self.suppress_presentation(o, out)
                }
                _ => {}
            }
        }
    }

    /// `id` leaves fullscreen: back to its size before it (when known), at its position before
    /// it, or at `anchor` (a Super+drag out of fullscreen).
    pub(super) fn leave_fullscreen(
        &mut self,
        id: crate::stage::ElementId,
        anchor: Option<crate::geom::CanvasPoint>,
    ) -> Vec<Effect> {
        let mut fx = Vec::new();
        if !self.left_fullscreen.contains(&id) {
            self.left_fullscreen.push(id);
        }
        let request = crate::protocol::ToplevelRequest::UnsetFullscreen;
        if let Ok((_, t_fx)) = self.toplevel_request(id, request) {
            fx.extend(t_fx);
        }
        if let Some(a) = anchor {
            // dragged out: the drag places it, where the user lets go
            self.returning.retain(|e| *e != id);
            let _ = self.place_by_shell(id, a, &mut fx);
        }
        if let Some(o) = self.pointer_output() {
            self.invalidate_frame(o);
        }
        fx.extend(self.visibility_pass());
        fx
    }

    /// Super+Shift+Enter: the focused window's fullscreen toggles on the pointer's output, as if it
    /// had asked (`set_fullscreen` / `unset_fullscreen`: the same path, M9, INV-PRES-1). Nothing
    /// on the Canvas moves: the window is shown over the whole screen, not at its place, and no
    /// neighbour makes room for it (owner, 2026-10-31; INV-PRES-1 "fullscreen never touches
    /// cameras or anchors").
    pub(super) fn toggle_fullscreen(&mut self) -> Vec<Effect> {
        let Some(id) = self.focus.target() else {
            return Vec::new();
        };
        let was_fullscreen = self.fullscreen_of(id).is_some();
        if was_fullscreen {
            return self.leave_fullscreen(id, None);
        }
        self.left_fullscreen.retain(|e| *e != id); // the user asks for it again
        let mut fx = Vec::new();
        let output = self
            .pointer_output()
            .or_else(|| self.outputs.first().map(|o| o.id));
        let request = crate::protocol::ToplevelRequest::SetFullscreen { output };
        if let Ok((_, t_fx)) = self.toplevel_request(id, request) {
            fx.extend(t_fx);
        }
        if let Some(o) = output {
            self.invalidate_frame(o);
        }
        fx.extend(self.visibility_pass());
        fx
    }

    /// F-FIT-1 on `element` for `output`'s camera, shared by the binding and the shell: refused
    /// (nothing changed) when the element is not mapped, has no window geometry, or the fit is
    /// degenerate; otherwise one mutation (the prior value retained, OBJ-30) and the camera-write
    /// consequences. The callers' guards (presentation, pointer) come first.
    pub(super) fn fit_on(
        &mut self,
        proof: authority::Authorized,
        output: OutputId,
        element: crate::stage::ElementId,
        out: &mut Vec<Effect>,
    ) -> Result<(), crate::ShellError> {
        let (camera, target) = self.fit_plan(output, element)?;
        self.cameras
            .glide_fit(proof, camera, target, self.zoom_feel);
        self.camera_written(output, camera, out);
        Ok(())
    }

    /// The camera and the F-FIT-1 target a fit of `element` on `output` would write; nothing is
    /// changed.
    fn fit_plan(
        &self,
        output: OutputId,
        element: crate::stage::ElementId,
    ) -> Result<(usize, crate::navigation::CameraValue), crate::ShellError> {
        if self.stage.mapped(element).is_none() {
            return Err(crate::ShellError::NotMapped);
        }
        let (Some(anchor), Some(tree)) = (self.stage.anchor(element), self.stage.tree(element))
        else {
            return Err(crate::ShellError::NotMapped);
        };
        let g = (
            anchor.x,
            anchor.y,
            tree.geometry.size.w as i64,
            tree.geometry.size.h as i64,
        );
        // the two facts F-FIT-1 reads, not a clone of the model (its key owns strings)
        let (Some((view_size, scale)), Some(camera)) = (
            self.output(output).map(|m| (m.view_size(), m.scale)),
            self.cameras.camera_for_output(output),
        ) else {
            return Err(crate::ShellError::UnknownOutput);
        };
        let (Some(extent), Some(usable)) = (
            self.output(output).map(|m| m.logical_extent()),
            self.usable_area(output),
        ) else {
            return Err(crate::ShellError::UnknownOutput);
        };
        let Some(target) = crate::navigation::fit_target_in(view_size, extent, usable, scale, g)
        else {
            return Err(crate::ShellError::Refused);
        };
        Ok((camera, target))
    }

    /// M8 `ViewportRestore(O)` from the input binding, O the pointer's output: the newest entry of
    /// O's history is applied and removed; refused with nothing changed when O holds no pointer,
    /// O presents a protocol-fullscreen element (C12.F17 Q1 B), or its history is empty.
    pub(super) fn viewport_restore(&mut self, proof: authority::Authorized, out: &mut Vec<Effect>) {
        let PointerState::Located { output, .. } = self.pointer else {
            return;
        };
        if self.presentation(output).is_some() && !self.is_presentation_suppressed(output) {
            return;
        }
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return;
        };
        if self.cameras.history(camera).is_none_or(|h| h.is_empty()) {
            return;
        }
        if !self.cameras.glide_restore(proof, camera, self.zoom_feel) {
            return;
        }
        self.overview_at.retain(|o| o.0 != output);
        self.camera_written(output, camera, out);
    }
}

#[cfg(test)]
mod tests {
    use crate::Core;
    use crate::entry::tests::*;
    use crate::input::bindings::Modifiers;
    use crate::navigation::PersistedCameras;
    use crate::pointer::PointerState;
    use crate::protocol::Effect;

    #[test]
    fn fullscreen_allows_instant_super_slot_switching_without_modal_lock() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();

        let w1 = c.toplevel_created();
        let w2 = c.toplevel_created();
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
        c.toplevel_committed(w1, win(1, 400, 300, false)).unwrap();
        c.toplevel_committed(w1, win(1, 400, 300, true)).unwrap();
        c.toplevel_committed(w2, win(2, 400, 300, false)).unwrap();
        c.toplevel_committed(w2, win(2, 400, 300, true)).unwrap();
        c.render_list(a).unwrap();

        // Focus w1 and enter fullscreen
        c.set_focus(&mut i, Some(w1)).unwrap();
        c.toplevel_request(
            w1,
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
        )
        .unwrap();
        c.toplevel_committed(w1, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(a), Some(w1));
        assert_eq!(
            c.view(a).unwrap().only,
            Some(w1),
            "w1 is presented fullscreen"
        );

        let logo = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };

        // Press Super+2 to switch to slot 1 (w2)
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_2,
                    pressed: true,
                    modifiers: logo,
                    mods_changed: false,
                    time_us: 1000,
                },
            )
            .unwrap();

        assert_eq!(
            c.focus().target(),
            Some(w2),
            "focus moves to w2 without modal lock"
        );
        assert!(
            c.view(a).unwrap().only.is_none(),
            "view is now normal camera view showing w2"
        );

        // Press Super+1 to switch back to slot 0 (w1, the fullscreen app)
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_1,
                    pressed: true,
                    modifiers: logo,
                    mods_changed: false,
                    time_us: 2000,
                },
            )
            .unwrap();

        assert_eq!(c.focus().target(), Some(w1), "focus returns to w1");
        assert_eq!(
            c.view(a).unwrap().only,
            Some(w1),
            "w1 is presented fullscreen again"
        );
    }

    #[test]
    fn maximize_moves_only_the_maximized_window() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();

        let w1 = c.toplevel_created();
        let w2 = c.toplevel_created();
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
        // Initial windowed mapping: w1 and w2 side-by-side
        c.toplevel_committed(w1, win(1, 800, 600, false)).unwrap();
        c.toplevel_committed(w1, win(1, 800, 600, true)).unwrap();
        c.toplevel_committed(w2, win(2, 800, 600, false)).unwrap();
        c.toplevel_committed(w2, win(2, 800, 600, true)).unwrap();
        c.render_list(a).unwrap();

        // Place w1 at (0, 0) and w2 at (812, 0)
        let mut out = Vec::new();
        c.place_by_shell(w1, crate::geom::CanvasPoint { x: 0, y: 0 }, &mut out)
            .unwrap();
        c.place_by_shell(w2, crate::geom::CanvasPoint { x: 812, y: 0 }, &mut out)
            .unwrap();

        // Focus w1
        c.set_focus(&mut i, Some(w1)).unwrap();
        c.set_placement_policy(crate::PlacementPolicy::ClusterTiling);
        c.set_enter_mode(crate::EnterMode::Maximize);

        // Move pointer to (10, 10) on output A
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (10.0, 10.0),
                    delta_unaccel: (10.0, 10.0),
                    time_us: 1000,
                },
            )
            .unwrap();

        let logo = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };

        // Press Super+Enter on w1
        let fx = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_ENTER,
                    pressed: true,
                    modifiers: logo,
                    mods_changed: false,
                    time_us: 2000,
                },
            )
            .unwrap();

        // the viewport is (-960..960, -540..540), w2 at (812, 0). Owner, 2026-11-06: only w1
        // moves (w2 used to be pushed to x=972); 2026-11-10: w1 takes the screen's size, 1920 x
        // 1080, where it covers no window, out of view if need be (2026-11-09 it shrank into the
        // free part of the screen: "it's supposed to go out of the viewport")
        let anchor_w1 = c.stage().anchor(w1).unwrap();
        let anchor_w2 = c.stage().anchor(w2).unwrap();
        assert!(
            fx.iter().any(
                |e| matches!(e, Effect::ToplevelConfigureSize { element, size }
                if *element == w1 && (size.w, size.h) == (1920, 1080))
            ),
            "w1 takes the screen's size: {fx:?}"
        );
        let apart = anchor_w1.x + 1920 <= anchor_w2.x
            || anchor_w2.x + 800 <= anchor_w1.x
            || anchor_w1.y + 1080 <= anchor_w2.y
            || anchor_w2.y + 600 <= anchor_w1.y;
        assert!(apart, "w1 at {anchor_w1:?} covers w2 at {anchor_w2:?}");
        assert_eq!(
            (anchor_w2.x, anchor_w2.y),
            (812, 0),
            "w2 stays where it was"
        );
    }

    /// With nothing else in view, a maximize fills the screen where the camera is.
    #[test]
    fn a_maximize_with_nothing_in_view_fills_the_view() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 1);
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        let (w, h) = c.output(a).unwrap().logical_extent();
        c.pointer = PointerState::Located {
            output: a,
            p: (w / 2.0, h / 2.0),
        };
        let fx = c.client_maximize(ws[0], true);
        let (ux, uy, uw, uh) = c.usable_area(a).unwrap();
        assert!(fx.iter().any(
            |e| matches!(e, Effect::ToplevelConfigureSize { element, size }
            if *element == ws[0] && (size.w, size.h) == (uw.round() as i32, uh.round() as i32))
        ));
        let tl = c
            .render_list(a)
            .unwrap()
            .view
            .unproject(crate::geom::OutputLogicalPoint { x: ux, y: uy })
            .round_to_point()
            .unwrap();
        assert_eq!(c.stage().anchor(ws[0]), Some(tl), "at the view's top left");
    }

    #[test]
    /// Owner, 2026-10-31: a fullscreen window is shown over the whole screen, not at its place on
    /// the Canvas: Super+Shift+Enter moves neither it nor its neighbour (it used to put the window
    /// at the view's corner and push the neighbour away).
    fn fullscreen_by_the_key_moves_and_pushes_no_window() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();

        let w1 = c.toplevel_created();
        let w2 = c.toplevel_created();
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

        c.set_placement_policy(crate::PlacementPolicy::ClusterTiling);

        // Initial windowed mapping: w1 and w2 side-by-side
        c.toplevel_committed(w1, win(1, 800, 600, false)).unwrap();
        c.toplevel_committed(w1, win(1, 800, 600, true)).unwrap();
        c.toplevel_committed(w2, win(2, 800, 600, false)).unwrap();
        c.toplevel_committed(w2, win(2, 800, 600, true)).unwrap();
        c.render_list(a).unwrap();

        // Place w1 at (0, 0) and w2 at (812, 0)
        let mut out = Vec::new();
        c.place_by_shell(w1, crate::geom::CanvasPoint { x: 0, y: 0 }, &mut out)
            .unwrap();
        c.place_by_shell(w2, crate::geom::CanvasPoint { x: 812, y: 0 }, &mut out)
            .unwrap();

        // Focus w1
        c.set_focus(&mut i, Some(w1)).unwrap();

        // Move pointer to (10, 10) on output A
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (10.0, 10.0),
                    delta_unaccel: (10.0, 10.0),
                    time_us: 1000,
                },
            )
            .unwrap();

        let logo_shift = Modifiers {
            logo: true,
            shift: true,
            ctrl: false,
            alt: false,
        };

        // Press Super+Shift+Enter on w1 to toggle fullscreen
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_ENTER,
                    pressed: true,
                    modifiers: logo_shift,
                    mods_changed: false,
                    time_us: 2000,
                },
            )
            .unwrap();

        // fullscreen, and nothing on the Canvas moved
        assert!(c.fullscreen_of(w1).is_some());
        let anchor_w1 = c.stage().anchor(w1).unwrap();
        let anchor_w2 = c.stage().anchor(w2).unwrap();
        assert_eq!((anchor_w1.x, anchor_w1.y), (0, 0), "w1 keeps its place");
        assert_eq!((anchor_w2.x, anchor_w2.y), (812, 0), "w2 is not pushed");

        // Release Enter key
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_ENTER,
                    pressed: false,
                    modifiers: logo_shift,
                    mods_changed: false,
                    time_us: 2500,
                },
            )
            .unwrap();

        // Toggle fullscreen off (Super+Shift+Enter again)
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_ENTER,
                    pressed: true,
                    modifiers: logo_shift,
                    mods_changed: false,
                    time_us: 3000,
                },
            )
            .unwrap();

        // back, both where they were
        assert!(c.fullscreen_of(w1).is_none());
        let restored_w1 = c.stage().anchor(w1).unwrap();
        assert_eq!((restored_w1.x, restored_w1.y), (0, 0));
        assert_eq!(c.stage().anchor(w2).unwrap().x, 812);
    }

    #[test]
    fn toggle_fullscreen_under_centred_in_view_does_not_push_overlapping_windows() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();

        let w1 = c.toplevel_created();
        let w2 = c.toplevel_created();

        let win = |key: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(key).unwrap()),
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

        // Default placement is CentredInView
        assert_eq!(c.placement, crate::PlacementPolicy::CentredInView);

        c.toplevel_committed(w1, win(1, 800, 600, false)).unwrap();
        c.toplevel_committed(w1, win(1, 800, 600, true)).unwrap();
        c.toplevel_committed(w2, win(2, 800, 600, false)).unwrap();
        c.toplevel_committed(w2, win(2, 800, 600, true)).unwrap();
        c.render_list(a).unwrap();

        // Place w1 at (0, 0) and overlapping w2 at (400, 300)
        let mut out = Vec::new();
        c.place_by_shell(w1, crate::geom::CanvasPoint { x: 0, y: 0 }, &mut out)
            .unwrap();
        c.place_by_shell(w2, crate::geom::CanvasPoint { x: 400, y: 300 }, &mut out)
            .unwrap();

        // Focus w1
        c.set_focus(&mut i, Some(w1)).unwrap();

        // Move pointer to output A
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (10.0, 10.0),
                    delta_unaccel: (10.0, 10.0),
                    time_us: 1000,
                },
            )
            .unwrap();

        let logo_shift = Modifiers {
            logo: true,
            shift: true,
            ctrl: false,
            alt: false,
        };

        // Press Super+Shift+Enter on w1 to toggle fullscreen
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_ENTER,
                    pressed: true,
                    modifiers: logo_shift,
                    mods_changed: false,
                    time_us: 2000,
                },
            )
            .unwrap();

        // w2 must remain at (400, 300) because placement policy is CentredInView (not ClusterTiling)
        let anchor_w2 = c.stage().anchor(w2).unwrap();
        assert_eq!(
            anchor_w2.x, 400,
            "w2 must not be pushed under CentredInView placement policy"
        );
        assert_eq!(anchor_w2.y, 300);
    }

    #[test]
    fn super_middle_mouse_pans_out_of_fullscreen_presentation() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        c.set_fullscreen_pan(true);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();

        let w1 = c.toplevel_created();
        let win = |mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w: 1920, h: 1080 },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(1).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w: 1920, h: 1080 }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        c.toplevel_committed(w1, win(false)).unwrap();
        c.toplevel_committed(w1, win(true)).unwrap();
        c.set_focus(&mut i, Some(w1)).unwrap();
        c.toplevel_request(
            w1,
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
        )
        .unwrap();
        c.toplevel_committed(w1, win(true)).unwrap();

        assert_eq!(c.presentation(a), Some(w1));
        assert_eq!(
            c.view(a).unwrap().only,
            Some(w1),
            "initially presenting fullscreen"
        );

        let logo = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };

        // Press Super + BTN_MIDDLE
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerButton {
                    button: crate::input::bindings::BTN_MIDDLE,
                    pressed: true,
                    modifiers: logo,
                    time_us: 1000,
                },
            )
            .unwrap();

        // Drag mouse by delta (200.0, 100.0)
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (200.0, 100.0),
                    delta_unaccel: (200.0, 100.0),
                    time_us: 2000,
                },
            )
            .unwrap();

        // View has switched out of exclusive presentation to camera view!
        assert!(
            c.view(a).unwrap().only.is_none(),
            "view transitioned to camera view allowing user to pan out of fullscreen"
        );

        // Release BTN_MIDDLE
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerButton {
                    button: crate::input::bindings::BTN_MIDDLE,
                    pressed: false,
                    modifiers: logo,
                    time_us: 3000,
                },
            )
            .unwrap();

        assert!(
            c.view(a).unwrap().only.is_none(),
            "still in camera view after pan completes"
        );

        // Pressing Super+1 focuses w1 and returns to fullscreen presentation
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_1,
                    pressed: true,
                    modifiers: logo,
                    mods_changed: false,
                    time_us: 4000,
                },
            )
            .unwrap();

        assert_eq!(
            c.view(a).unwrap().only,
            Some(w1),
            "Super+1 restores exclusive fullscreen presentation"
        );
    }

    #[test]
    fn client_side_unset_fullscreen_keeps_the_place_and_allows_subsequent_maximize() {
        use crate::geom::{CanvasPoint, LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();

        let w = c.toplevel_created();
        let win = |key: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(key).unwrap()),
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

        let orig_anchor = CanvasPoint { x: 120, y: 150 };
        let mut out = Vec::new();
        c.place_by_shell(w, orig_anchor, &mut out).unwrap();
        c.set_focus(&mut i, Some(w)).unwrap();
        c.set_enter_mode(crate::EnterMode::Maximize);

        // Move pointer onto output A
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (10.0, 10.0),
                    delta_unaccel: (10.0, 10.0),
                    time_us: 500,
                },
            )
            .unwrap();

        // Super+Shift+Enter toggles fullscreen
        let logo_shift = Modifiers {
            logo: true,
            shift: true,
            ctrl: false,
            alt: false,
        };
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_ENTER,
                    pressed: true,
                    modifiers: logo_shift,
                    mods_changed: false,
                    time_us: 1000,
                },
            )
            .unwrap();
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_ENTER,
                    pressed: false,
                    modifiers: logo_shift,
                    mods_changed: false,
                    time_us: 1500,
                },
            )
            .unwrap();

        assert!(c.fullscreen_of(w).is_some(), "window is fullscreen");
        assert_eq!(
            c.fullscreen_of(w).and_then(|f| f.saved),
            Some(LocalSize { w: 400, h: 300 }),
            "the size it returns to"
        );
        assert_eq!(c.stage().saved_floating(w), None, "it is not maximized");
        assert_eq!(c.stage().anchor(w), Some(orig_anchor), "the anchor stays");

        // Client itself unsets fullscreen via toplevel_request
        let _ = c
            .toplevel_request(w, crate::protocol::ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert!(c.fullscreen_of(w).is_none(), "no longer fullscreen");

        // the window is where it was, and not maximized
        assert_eq!(c.stage().anchor(w), Some(orig_anchor), "the anchor stayed");
        assert_eq!(c.stage().saved_floating(w), None);

        // A following Super+Enter must maximize, NOT restore
        let logo_only = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_ENTER,
                    pressed: true,
                    modifiers: logo_only,
                    mods_changed: false,
                    time_us: 2000,
                },
            )
            .unwrap();

        // If it maximized, saved_floating is now set (saving orig_anchor before maximize)
        assert!(
            c.stage().saved_floating(w).is_some(),
            "Super+Enter maximized and saved floating geometry"
        );
    }

    /// Title bar drag: the window follows the mouse, and the release reaches the app and ends it.
    #[test]
    fn an_app_moves_its_window_while_the_user_holds_a_press_on_it() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let ws = window_under_pointer(&mut c, &mut i, &mut t);
        let before = c.stage().anchor(ws[0]).unwrap();
        mouse_button(&mut c, &mut i, true);
        c.client_drag(ws[0], None);
        assert!(c.input.interaction.active());
        mouse_move(&mut c, &mut i, (70.0, 30.0));
        let after = c.stage().anchor(ws[0]).unwrap();
        assert_eq!((after.x - before.x, after.y - before.y), (70, 30));
        let fx = mouse_button(&mut c, &mut i, false);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::PointerButton { pressed: false, .. })),
            "{fx:?}"
        );
        assert!(!c.input.interaction.active());
        mouse_move(&mut c, &mut i, (70.0, 30.0));
        assert_eq!(c.stage().anchor(ws[0]), Some(after), "the drag ended");
    }

    /// An app cannot move itself by its own decision: no press, or a window not under the pointer.
    #[test]
    fn an_app_cannot_move_a_window_without_the_users_press_on_it() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let ws = window_under_pointer(&mut c, &mut i, &mut t);
        c.client_drag(ws[0], None);
        assert!(!c.input.interaction.active(), "no press");
        mouse_button(&mut c, &mut i, true);
        c.client_drag(ws[1], None);
        assert!(!c.input.interaction.active(), "not the window pressed");
    }

    /// An app's right edge changes the width only.
    #[test]
    fn an_app_edge_resizes_only_that_way() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let ws = window_under_pointer(&mut c, &mut i, &mut t);
        mouse_button(&mut c, &mut i, true);
        let edges = crate::input::interaction::ResizeEdges {
            right: true,
            ..Default::default()
        };
        c.client_drag(ws[0], Some(edges));
        match c.input.interaction {
            crate::input::interaction::Interaction::ElementResize {
                axes, left, top, ..
            } => {
                assert_eq!((axes, left, top), ((true, false), false, false));
            }
            ref other => panic!("{other:?}"),
        }
    }

    /// An app's maximize button: only the focused window, on and off, and the app is told.
    #[test]
    fn an_app_maximizes_and_restores_its_focused_window() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let ws = window_under_pointer(&mut c, &mut i, &mut t);
        let at = c.stage().anchor(ws[0]);
        let fx = c.client_maximize(ws[1], true);
        assert_eq!(
            fx,
            vec![Effect::ToplevelConfigure(ws[1])],
            "not focused: declined"
        );
        assert!(!c.toplevel_configure_state(ws[1]).maximized);
        let _ = c.client_maximize(ws[0], true);
        assert!(c.toplevel_configure_state(ws[0]).maximized);
        let _ = c.client_maximize(ws[0], false);
        assert!(!c.toplevel_configure_state(ws[0]).maximized);
        assert_eq!(c.stage().anchor(ws[0]), at, "back where it was");
    }

    /// Apps never move the camera: zoomed out, an app's maximize is answered and changes nothing.
    #[test]
    fn an_app_maximize_leaves_a_zoomed_camera_alone() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let ws = window_under_pointer(&mut c, &mut i, &mut t);
        press_overview(&mut c, &mut i);
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        let zoom = c.cameras().zoom(0);
        assert!(zoom.is_some_and(|z| z < 1.0));
        let fx = c.client_maximize(ws[0], true);
        assert_eq!(fx, vec![Effect::ToplevelConfigure(ws[0])]);
        assert_eq!(c.cameras().zoom(0), zoom);
        assert!(!c.toplevel_configure_state(ws[0]).maximized);
    }

    /// Maximized, or as large as the view, with nothing in front: the edge's strips step aside.
    #[test]
    fn a_window_fills_the_view_only_when_it_covers_it_with_nothing_in_front() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let ws = window_under_pointer(&mut c, &mut i, &mut t);
        assert!(!c.fills_view(ws[0]), "a small window");
        let _ = c.client_maximize(ws[0], true);
        let mut fx = Vec::new();
        c.raise_by_user(ws[0]);
        assert!(!c.fills_view(ws[0]), "not until it has drawn at that size");
        let output = c.outputs()[0].id;
        let (_, _, w, h) = c.usable_area(output).unwrap();
        let size = crate::geom::LocalSize {
            w: w as i32,
            h: h as i32,
        };
        let tree = crate::snapshot::SurfaceTree {
            geometry: crate::geom::LocalRect {
                loc: crate::geom::LocalPoint::default(),
                size,
            },
            surfaces: vec![crate::snapshot::SnapshotSurface {
                key: crate::snapshot::SurfaceKey::from_raw(std::num::NonZeroU64::new(1).unwrap()),
                role: crate::snapshot::SurfaceRole::Root,
                size,
                mapped: true,
                input: crate::snapshot::InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        c.toplevel_committed(ws[0], tree).unwrap();
        assert!(c.fills_view(ws[0]), "maximized");
        let at = c.stage().anchor(ws[0]).unwrap();
        c.place_by_user(ws[1], at, &mut fx).unwrap();
        c.raise_by_user(ws[1]);
        assert!(!c.fills_view(ws[0]), "another window in front");
        c.raise_by_user(ws[0]);
        assert!(c.fills_view(ws[0]));
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: 9000, y: 9000 },
            &mut fx,
        )
        .unwrap();
        assert!(!c.fills_view(ws[0]), "moved away");
    }

    /// Taken out of fullscreen by the user, a window stays out when it asks again (Wine does at
    /// once, for a window as large as the screen), until the user asks for fullscreen.
    #[test]
    fn a_window_taken_out_of_fullscreen_stays_out_until_the_user_asks() {
        use crate::protocol::{ToplevelDecision, ToplevelRequest};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = fullscreen_rig(&mut c, &mut i, &mut t);
        assert_eq!(c.presentation(a), Some(ws[0]));
        let _ = c.toggle_fullscreen();
        assert_eq!(c.presentation(a), None);
        let ask = ToplevelRequest::SetFullscreen { output: Some(a) };
        let (decision, _) = c.toplevel_request(ws[0], ask).unwrap();
        assert_eq!(decision, ToplevelDecision::Declined);
        assert_eq!(c.presentation(a), None);
        let _ = c.toggle_fullscreen();
        assert_eq!(c.presentation(a), Some(ws[0]), "the user asked");
    }

    /// Pulled out of fullscreen with Super+drag, a window can be dragged again.
    #[test]
    fn a_window_pulled_out_of_fullscreen_can_be_dragged_again() {
        use crate::input::bindings::{BTN_LEFT, Modifiers};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = fullscreen_rig(&mut c, &mut i, &mut t);
        let (w, h) = c.output(a).unwrap().logical_extent();
        c.pointer = PointerState::Located {
            output: a,
            p: (w / 2.0, h / 2.0),
        };
        let logo = Modifiers {
            logo: true,
            ..Default::default()
        };
        let button = |c: &mut Core, i: &mut crate::InputPort, pressed| {
            c.physical_input_vec(
                i,
                crate::PhysicalEvent::PointerButton {
                    button: BTN_LEFT,
                    pressed,
                    modifiers: logo,
                    time_us: 1000,
                },
            )
            .unwrap();
        };
        button(&mut c, &mut i, true);
        mouse_move(&mut c, &mut i, (40.0, 20.0));
        button(&mut c, &mut i, false);
        assert_eq!(c.presentation(a), None);
        let (x0, y0) = {
            let e = c.stage().anchor(ws[0]).unwrap();
            (e.x, e.y)
        };
        // the pointer back over the window
        assert_eq!(c.hovered_window(), Some(ws[0]), "under the pointer");
        button(&mut c, &mut i, true);
        assert!(c.input.interaction.active(), "the second drag starts");
        mouse_move(&mut c, &mut i, (30.0, 10.0));
        button(&mut c, &mut i, false);
        let e = c.stage().anchor(ws[0]).unwrap();
        assert_eq!((e.x - x0, e.y - y0), (30, 10));
    }

    /// Maximize pushes the neighbours aside; restoring puts back those nobody moved since.
    #[test]
    fn resizing_a_maximized_window_ends_the_maximize() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 1);
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        let (w, h) = c.output(a).unwrap().logical_extent();
        c.pointer = PointerState::Located {
            output: a,
            p: (w / 2.0, h / 2.0),
        };
        let _ = c.client_maximize(ws[0], true);
        assert!(c.maximized.iter().any(|(e, _)| *e == ws[0]));
        c.begin_resize(ws[0], false, false);
        assert!(
            !c.maximized.iter().any(|(e, _)| *e == ws[0]),
            "resized: maximized no longer"
        );
        assert_eq!(c.stage().saved_floating(ws[0]), None);
        c.resize = None;
        // the next maximize maximizes again
        let _ = c.client_maximize(ws[0], true);
        assert!(c.maximized.iter().any(|(e, _)| *e == ws[0]));
    }

    #[test]
    fn a_maximized_window_moved_is_restored_where_it_is() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        c.set_placement_policy(crate::PlacementPolicy::ClusterTiling);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 1);
        let mut fx = Vec::new();
        let at = |x, y| crate::geom::CanvasPoint { x, y };
        c.place_by_user(ws[0], at(-200, -150), &mut fx).unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        let (w, h) = c.output(a).unwrap().logical_extent();
        c.pointer = PointerState::Located {
            output: a,
            p: (w / 2.0, h / 2.0),
        };
        let _ = c.client_maximize(ws[0], true);
        // dragged while maximized (the window has not redrawn: still 400 x 300)
        c.place_by_user(ws[0], at(3000, 3000), &mut fx).unwrap();
        let _ = c.client_maximize(ws[0], false);
        assert_eq!(
            c.stage().anchor(ws[0]),
            Some(at(3000, 3000)),
            "where it is, not back at (-200, -150)"
        );
        // not moved: back to its place before
        c.place_by_user(ws[0], at(-200, -150), &mut fx).unwrap();
        let _ = c.client_maximize(ws[0], true);
        let _ = c.client_maximize(ws[0], false);
        assert_eq!(c.stage().anchor(ws[0]), Some(at(-200, -150)));
    }

    #[test]
    fn maximize_and_restore_move_no_other_window() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        c.set_placement_policy(crate::PlacementPolicy::ClusterTiling);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 3);
        let mut fx = Vec::new();
        let at = |x, y| crate::geom::CanvasPoint { x, y };
        c.place_by_user(ws[0], at(-200, -150), &mut fx).unwrap();
        c.place_by_user(ws[1], at(250, -150), &mut fx).unwrap();
        c.place_by_user(ws[2], at(-700, -150), &mut fx).unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        let (w, h) = c.output(a).unwrap().logical_extent();
        c.pointer = PointerState::Located {
            output: a,
            p: (w / 2.0, h / 2.0),
        };
        let _ = c.client_maximize(ws[0], true);
        assert_eq!(c.stage().anchor(ws[1]), Some(at(250, -150)), "stays");
        assert_eq!(c.stage().anchor(ws[2]), Some(at(-700, -150)), "stays");
        let _ = c.client_maximize(ws[0], false);
        assert_eq!(c.stage().anchor(ws[0]), Some(at(-200, -150)), "back");
        assert_eq!(c.stage().anchor(ws[1]), Some(at(250, -150)));
        assert_eq!(c.stage().anchor(ws[2]), Some(at(-700, -150)));
    }

    /// A window the user opens over a fullscreen window is shown, not hidden behind it.
    #[test]
    fn a_window_the_user_opens_comes_out_over_fullscreen() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = fullscreen_rig(&mut c, &mut i, &mut t);
        let fx = c.show_opened(&mut i, ws[1]).unwrap();
        assert!(!fx.is_empty());
        assert!(c.is_presentation_suppressed(a));
        assert_eq!(c.focus().target(), Some(ws[1]));
    }

    /// A game stepped aside that asks for fullscreen again (games do when they get focus, as by
    /// focus following the mouse) stays aside, under the window the user chose (owner,
    /// 2026-11-15: "fullscreen apps still appear above other windows").
    #[test]
    fn a_repeated_fullscreen_request_does_not_bring_a_stepped_aside_game_back_on_top() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = fullscreen_rig(&mut c, &mut i, &mut t);
        c.show_opened(&mut i, ws[1]).unwrap();
        assert!(c.is_presentation_suppressed(a));
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        let fx = c
            .toplevel_request(
                ws[0],
                crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
            )
            .unwrap();
        assert!(!fx.1.is_empty(), "the request is still answered");
        assert!(c.is_presentation_suppressed(a), "the game stays aside");
        assert_eq!(c.presentation(a), Some(ws[0]), "and stays fullscreen");
        assert_eq!(*c.stage.mapped_elements().last().unwrap(), ws[1]);
        // the user going back to it brings it in front, on top
        let mut fx = Vec::new();
        c.unsuppress_presentation(a, &mut fx);
        assert_eq!(*c.stage.mapped_elements().last().unwrap(), ws[0]);
    }

    /// A fullscreen window stepped aside goes behind its neighbours: at the screen's size it
    /// would cover them on the canvas, and they could not be seen or clicked.
    #[test]
    fn a_fullscreen_window_stepped_aside_goes_behind_the_others() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = fullscreen_rig(&mut c, &mut i, &mut t);
        press_overview(&mut c, &mut i);
        assert!(c.is_presentation_suppressed(a));
        assert_eq!(*c.stage.mapped_elements().first().unwrap(), ws[0]);
    }

    /// Back in front, the fullscreen window is on top again (owner, 2026-11-05: games came
    /// back under the windows that had been over them while it stood aside).
    #[test]
    fn a_fullscreen_window_back_in_front_is_on_top_again() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = fullscreen_rig(&mut c, &mut i, &mut t);
        press_overview(&mut c, &mut i);
        assert!(c.is_presentation_suppressed(a));
        assert_eq!(*c.stage.mapped_elements().first().unwrap(), ws[0]);
        let mut fx = Vec::new();
        c.unsuppress_presentation(a, &mut fx);
        assert!(!c.is_presentation_suppressed(a));
        assert_eq!(*c.stage.mapped_elements().last().unwrap(), ws[0]);
    }

    /// Holding Super over a game in fullscreen leaves the cursor with it (owner, 2026-11-05);
    /// with the game stepped aside (the overview), a held Super frees it as over any window.
    #[test]
    fn a_held_super_does_not_free_the_cursor_from_a_fullscreen_game() {
        use crate::input::keys::KEY_LEFTMETA;
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = fullscreen_rig(&mut c, &mut i, &mut t);
        c.set_free_cursor_on_super(true);
        press(&mut c, &mut i, KEY_LEFTMETA, true, true);
        assert!(
            !c.constraints_suspended,
            "fullscreen in front: Super held, cursor kept"
        );
        press(&mut c, &mut i, KEY_LEFTMETA, false, false);
        press_overview(&mut c, &mut i);
        assert!(c.is_presentation_suppressed(a));
        press(&mut c, &mut i, KEY_LEFTMETA, true, true);
        assert!(
            c.constraints_suspended,
            "stepped aside: Super held frees it"
        );
        press(&mut c, &mut i, KEY_LEFTMETA, false, false);
        assert!(!c.constraints_suspended);
    }
}
