//! Port-carrying entry points (C1.5, INV-CAMERA-1, INV-OUTPUT-2). The only module in which an
//! `Authorized` proof can be minted.
//!
//! Each entry point first checks that its port or token belongs to this Core. INC-3 state:
//! * the three output-lifecycle entries implement M4/M5 for any number of outputs (startup and
//!   hotplug alike, reconfiguration, disconnection, the zero-output state) and the pointer's M10
//!   transitions;
//! * physical pointer input (C4.2: ownership, M22 CameraPan, routing, KeepFocus);
//! * the shell vocabulary has no variants yet (INC-11).

use crate::input::bindings;
use crate::input::interaction::Interaction;
use crate::input::keys::KeyAction;
use crate::input::ownership::Owner;
use crate::output::{self, OutputDescriptor, OutputId, OutputModel};
use crate::pointer::{self, PointerState};
use crate::protocol::Effect;
use crate::{
    Core, EntryError, InputPort, NavigateTo, OutputLifecycleToken, PhysicalEvent, ShellCommand,
    ShellError, ShellPort, ShellStream,
};

pub mod authority {
    //! Every camera mutation consumes a fresh proof (INV-CAMERA-1): minted only in `entry`,
    //! not `Clone`, `!Send`, consumed by value.

    pub struct Authorized {
        _not_send: crate::NotSend,
    }

    impl Authorized {
        pub(in crate::entry) fn mint() -> Self {
            Authorized {
                _not_send: std::marker::PhantomData,
            }
        }
    }
}

/// A Canvas position in px from the shell, quantised once (Q16 ties-even); `None` when a value is
/// not finite or lies beyond the stored Canvas (±2³⁶ px).
fn shell_canvas_pos((x, y): (f64, f64)) -> Option<crate::geom::CanvasPos> {
    let limit = crate::geom::CANVAS_LIMIT_PX as f64;
    let ok = |v: f64| v.is_finite() && v.abs() <= limit;
    (ok(x) && ok(y)).then(|| crate::geom::CanvasPos {
        x: crate::geom::q16(x),
        y: crate::geom::q16(y),
    })
}

impl Core {
    fn check(&self, core_id: u64) -> Result<(), EntryError> {
        if core_id == self.id {
            Ok(())
        } else {
            Err(EntryError::ForeignPort)
        }
    }

    /// C4.2 pointer intake. P0 validate; P1 ownership continuation (releases follow the press
    /// owner, motion follows the interaction); P2 binding resolution before any client dispatch;
    /// P3/P4 routing through the ProjectionFrame hit test.
    ///
    /// The effects are appended, in order, to the caller's reusable
    /// [`EffectBuf`](crate::protocol::EffectBuf) (C14.3: "C1's `Vec<Effect>` return type becomes a
    /// caller-provided buffer"; pointer intake and routing, camera pan and keyboard intake write
    /// into it, no `Vec` per event). On an error nothing is appended.
    pub fn physical_input(
        &mut self,
        port: &mut InputPort,
        ev: PhysicalEvent,
        buf: &mut crate::protocol::EffectBuf,
    ) -> Result<(), EntryError> {
        let out = &mut buf.fx;
        self.check(port.core_id)?;
        match ev {
            PhysicalEvent::PointerMotion {
                delta,
                delta_unaccel,
                time_us,
            } => {
                let finite = [delta.0, delta.1, delta_unaccel.0, delta_unaccel.1]
                    .iter()
                    .all(|v| v.is_finite());
                if !finite {
                    return Err(EntryError::InvalidEvent);
                }
                self.input.clock.accepted(time_us);
                // a motion owned by a pan mutates a camera: its proof is minted here, per event
                let proof = self
                    .input
                    .interaction
                    .panning()
                    .is_some()
                    .then(authority::Authorized::mint);
                self.pointer_motion(proof, delta, delta_unaccel, time_us, out);
                Ok(())
            }
            PhysicalEvent::PointerButton {
                button,
                pressed,
                modifiers,
                time_us,
            } => {
                self.input.clock.accepted(time_us);
                self.input.modifiers = modifiers;
                let pan = pressed && bindings::is_pan_binding(button, modifiers);
                let reset_zoom = pressed
                    && button == crate::input::bindings::BTN_MIDDLE
                    && modifiers.logo
                    && modifiers.shift
                    && !modifiers.ctrl
                    && !modifiers.alt;
                let proof = (reset_zoom || pan).then(authority::Authorized::mint);
                if pressed {
                    self.button_press(button, modifiers, ms(time_us), proof, out);
                } else {
                    self.button_release(button, ms(time_us), out);
                }
                Ok(())
            }
            PhysicalEvent::PointerAxis { axis, time_us } => {
                let finite = [axis.amount.0, axis.amount.1].iter().all(|v| v.is_finite());
                if !finite {
                    return Err(EntryError::InvalidEvent);
                }
                self.input.clock.accepted(time_us);
                let mods = self.input.modifiers;
                if mods.logo && mods.shift && !mods.ctrl && !mods.alt {
                    let proof = authority::Authorized::mint();
                    self.zoom_axis(proof, axis, out);
                    return Ok(());
                }
                // consumed during an interaction and in NoOutput (M10, M22)
                if self.input.interaction.active() || self.pointer == PointerState::NoOutput {
                    return Ok(());
                }
                out.extend([
                    Effect::PointerAxis {
                        axis,
                        time_ms: ms(time_us),
                    },
                    Effect::PointerFrame,
                ]);
                Ok(())
            }
            PhysicalEvent::KeyboardKey {
                key,
                pressed,
                modifiers,
                mods_changed,
                time_us,
            } => {
                self.input.clock.accepted(time_us);
                self.input.modifiers = modifiers;
                if pressed {
                    // a press bound to fit or restore mutates a camera: its proof is minted here
                    let navigation = matches!(
                        self.resolve_key_binding(key, modifiers, self.input.interaction.active()),
                        Some(
                            KeyAction::FitViewport
                                | KeyAction::ViewportRestore
                                | KeyAction::FitToggle
                                | KeyAction::Recenter
                                | KeyAction::FocusSlot(_)
                                | KeyAction::FocusClusterSlot(_)
                                | KeyAction::FocusNext
                                | KeyAction::ToggleOverview
                        )
                    )
                    .then(authority::Authorized::mint);
                    self.keyboard_press(key, modifiers, mods_changed, time_us, navigation, out);
                } else {
                    self.keyboard_release(key, mods_changed, time_us, out);
                }
                Ok(())
            }
            PhysicalEvent::RecenterElement { element } => {
                let proof = authority::Authorized::mint();
                self.recenter_on_element(proof, element, out);
                Ok(())
            }
        }
    }

    /// [`Core::physical_input`] into a new `Vec`: for tests and one-off callers. It allocates;
    /// the adapter uses [`Core::physical_input`] with its reusable buffer.
    pub fn physical_input_vec(
        &mut self,
        port: &mut InputPort,
        ev: PhysicalEvent,
    ) -> Result<Vec<Effect>, EntryError> {
        let mut out = crate::protocol::EffectBuf::new();
        self.physical_input(port, ev, &mut out)?;
        Ok(out.into_vec())
    }

    /// K1/K3 keyboard press intake in Core (INV-INPUT-1, OBJ-25).
    fn keyboard_press(
        &mut self,
        key: u32,
        modifiers: crate::input::bindings::Modifiers,
        mods_changed: bool,
        time_us: u64,
        navigation: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        // M12 Down->press repeat: if already held by Compositor, stay compositor-owned and consume (K1)
        if self.input.key_ownership.owner(key) == Some(Owner::Compositor) {
            out.push(Effect::KeyboardIntercept { key });
            return;
        }
        // Repeated press of already logically-down client key must not emit duplicate pressed (R03, K1/M12)
        if self.input.key_ownership.owner(key) == Some(Owner::Client) {
            return;
        }

        let interaction_active = self.input.interaction.active();
        let matched = self.resolve_key_binding(key, modifiers, interaction_active);

        // K3: keys that change XKB modifier or lock state are never compositor-owned (observe mode)
        if mods_changed {
            self.input.key_ownership.press(key, Owner::Client);
            if let Some(action) = matched {
                self.apply_key_action(action, navigation, out);
            }
            out.push(Effect::KeyboardForward {
                key,
                pressed: true,
                mods_changed: true,
                time_us,
            });
        } else if let Some(action) = matched {
            // K1: compositor-owned press is consumed
            self.input.key_ownership.press(key, Owner::Compositor);
            self.apply_key_action(action, navigation, out);
            out.push(Effect::KeyboardIntercept { key });
        } else {
            // Client-owned press: forwarded
            self.input.key_ownership.press(key, Owner::Client);
            out.push(Effect::KeyboardForward {
                key,
                pressed: true,
                mods_changed: false,
                time_us,
            });
        }
    }

    /// K2 keyboard release intake in Core (INV-INPUT-1, OBJ-25).
    fn keyboard_release(
        &mut self,
        key: u32,
        mods_changed: bool,
        time_us: u64,
        out: &mut Vec<Effect>,
    ) {
        // Release follows recorded owner. Orphan release (None) is dropped and never forwarded.
        match self.input.key_ownership.release(key) {
            Some(Owner::Client) => out.push(Effect::KeyboardForward {
                key,
                pressed: false,
                mods_changed,
                time_us,
            }),
            Some(Owner::Compositor) => out.push(Effect::KeyboardIntercept { key }),
            None => {}
        }
    }

    /// `proof`: minted by `physical_input` for a press bound to fit or restore (INV-CAMERA-1).
    /// The effects are appended to `out` (the caller's reusable buffer, C14.3).
    fn apply_key_action(
        &mut self,
        action: KeyAction,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        let fx = match action {
            crate::input::keys::KeyAction::FocusNext => {
                let fx = self.focus_next();
                out.extend(fx);
                if let PointerState::Located { output, .. } = self.pointer {
                    if let Some(pres) = self.presentation(output) {
                        if let Some(target) = self.focus.target() {
                            if pres == target {
                                self.unsuppress_presentation(output);
                            } else {
                                self.suppress_presentation(output);
                            }
                        }
                    }
                }
                if self.focus_cycle_navigate {
                    if let Some(p) = proof {
                        self.recenter(p, out);
                    }
                }
                return;
            }
            // M22 cancel: no revert, owned buttons stay owned; the interaction terminates as at
            // its initiator's release (INV-SCALE-3, INV-INPUT-5, M10)
            crate::input::keys::KeyAction::Cancel => {
                if self.input.interaction.active() {
                    self.end_interaction()
                } else {
                    Vec::new()
                }
            }
            KeyAction::FitViewport => {
                if let Some(p) = proof {
                    self.fit_viewport(p, out);
                }
                return;
            }
            KeyAction::ViewportRestore => {
                if let Some(p) = proof {
                    self.viewport_restore(p, out);
                }
                return;
            }
            KeyAction::FitToggle => {
                if let Some(p) = proof {
                    self.fit_toggle(p, out);
                }
                return;
            }
            KeyAction::Recenter => {
                if let Some(p) = proof {
                    self.recenter(p, out);
                }
                return;
            }
            KeyAction::FocusSlot(slot) => {
                self.focus_slot(slot, proof, out);
                return;
            }
            KeyAction::FocusClusterSlot(slot) => {
                self.focus_cluster_slot(slot, proof, out);
                return;
            }
            KeyAction::ToggleFullscreen => self.toggle_fullscreen(),
            KeyAction::ForceCloseFocused => self
                .focus
                .target()
                .map(Effect::ForceClose)
                .into_iter()
                .collect(),
            // a request, not a destruction: the client closes (or asks the user first)
            KeyAction::CloseFocused => self
                .focus
                .target()
                .map(Effect::CloseRequest)
                .into_iter()
                .collect(),
            KeyAction::ToggleLauncher => vec![Effect::ToggleLauncher],
            KeyAction::SpawnTerminal => vec![Effect::SpawnTerminal],
            KeyAction::SpawnFileManager => vec![Effect::SpawnFileManager],
            KeyAction::SpawnScreenshot => vec![Effect::SpawnScreenshot],
            KeyAction::SpawnRegionScreenshot => vec![Effect::SpawnRegionScreenshot],
            KeyAction::ToggleOverview => {
                if let Some(p) = proof {
                    self.overview_toggle(p, out);
                } else {
                    out.push(Effect::ToggleOverview);
                }
                return;
            }
            KeyAction::ReloadShell => vec![Effect::ReloadShell],
            KeyAction::SpawnLockScreen => vec![Effect::SpawnLockScreen],
            KeyAction::TogglePrivacy => vec![Effect::TogglePrivacy],
            KeyAction::ToggleIdleInhibit => vec![Effect::ToggleIdleInhibit],
            KeyAction::SaveReplay => vec![Effect::SaveReplay],
            KeyAction::ToggleRecording => vec![Effect::ToggleRecording],
            KeyAction::VolumeUp => vec![Effect::VolumeUp],
            KeyAction::VolumeDown => vec![Effect::VolumeDown],
            KeyAction::VolumeMute => vec![Effect::VolumeMute],
            KeyAction::BrightnessUp => vec![Effect::BrightnessUp],
            KeyAction::BrightnessDown => vec![Effect::BrightnessDown],
            KeyAction::MediaPlayPause => vec![Effect::MediaPlayPause],
            KeyAction::MediaNext => vec![Effect::MediaNext],
            KeyAction::MediaPrev => vec![Effect::MediaPrev],
            KeyAction::Exec(i) => vec![Effect::Exec(i)],
            crate::input::keys::KeyAction::Noop => Vec::new(),
        };
        out.extend(fx);
    }

    /// M8 `FitViewport(O, W)` from the input binding: O is the pointer's output, W the element
    /// under the pointer (its window geometry snapshotted now). Refused — no camera mutation,
    /// nothing recorded or evicted, no pointer reconciliation — when no output holds the pointer,
    /// O presents a protocol-fullscreen element (C12.F17 Q1 B), no mapped element is under the
    /// pointer, or its geometry is degenerate (F-FIT-1). Otherwise one proof, one mutation (the
    /// prior value retained, OBJ-30), and the camera-write consequences.
    fn fit_viewport(&mut self, proof: authority::Authorized, out: &mut Vec<Effect>) {
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
    /// natively fill the usable viewport at 100% sharp scale (toggleable).
    fn fit_toggle(&mut self, proof: authority::Authorized, out: &mut Vec<Effect>) {
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
            self.maximize_toggle(proof, output, element, out);
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
                self.cameras.center(camera) == Some(target.center)
                    && self.cameras.zoom(camera).map(f64::to_bits)
                        == Some(target.zoom.to_bits())
                    && self.cameras.history(camera).is_some_and(|h| !h.is_empty())
            });
        if at_fit {
            self.viewport_restore(proof, out);
        } else {
            let _ = self.fit_on(proof, output, element, out);
        }
    }

    /// Native viewport fill toggle (EnterMode::Maximize): maximizes the window to fill
    /// the usable screen area at 100% sharp 1:1 scale without digital blur, or restores it.
    fn maximize_toggle(
        &mut self,
        proof: authority::Authorized,
        output: OutputId,
        element: crate::stage::ElementId,
        out: &mut Vec<Effect>,
    ) {
        if self.stage.mapped(element).is_none() {
            return;
        }
        if let Some((orig_anchor, orig_size)) = self.stage.saved_floating(element) {
            let _ = self.stage.set_saved_floating(element, None);
            let _ = self.place_by_shell(element, orig_anchor, out);
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

            // If camera zoom is not 1.0, reset camera zoom to 1.0 for sharp 1:1 view
            if let Some(camera) = self.cameras.camera_for_output(output) {
                let zoom = self.cameras.zoom(camera).unwrap_or(1.0);
                if (zoom - 1.0).abs() > 0.001 {
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

            let _ = self
                .stage
                .set_saved_floating(element, Some((anchor, orig_size)));
            let _ = self.place_by_shell(element, tl, out);
            let target_rect = (tl.x, tl.y, tl.x + canvas_w as i64, tl.y + canvas_h as i64);
            self.push_overlapping_cluster(element, target_rect, out);
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

    /// Super+Space: the pointer's output's camera is centred on the focused window — the centre of
    /// its window geometry at the centre of the usable area (L4) — with its zoom unchanged and no
    /// history entry (as a shell `Navigate`). Refused, nothing changed, when no output holds the
    /// pointer, that output presents a protocol-fullscreen element (C12.F17 Q1 B), nothing is
    /// focused, or the focused window is not mapped or has a degenerate geometry. Then the
    /// camera-write consequences, as after a fit.
    pub(crate) fn recenter_on_element(
        &mut self,
        proof: authority::Authorized,
        element: crate::stage::ElementId,
        out: &mut Vec<Effect>,
    ) {
        let output = match self.pointer {
            PointerState::Located { output, .. } => Some(output),
            PointerState::NoOutput => self.outputs.first().map(|o| o.id),
        };
        let Some(output) = output else {
            return;
        };
        if self.presentation(output).is_some() && !self.is_presentation_suppressed(output) {
            return;
        }
        if self.presentation(output) == Some(element) && self.is_presentation_suppressed(output) {
            self.unsuppress_presentation(output);
            return;
        }
        if self.stage.mapped(element).is_none() {
            return;
        }
        let (Some(anchor), Some(tree)) = (self.stage.anchor(element), self.stage.tree(element))
        else {
            return;
        };
        let g = (
            anchor.x,
            anchor.y,
            tree.geometry.size.w as i64,
            tree.geometry.size.h as i64,
        );
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return;
        };
        let (Some(extent), Some(usable)) = (
            self.output(output).map(|m| m.logical_extent()),
            self.usable_area(output),
        ) else {
            return;
        };
        let target_zoom = 1.0;
        let Some(center) = crate::navigation::recenter_target(extent, usable, target_zoom, g) else {
            return;
        };
        self.cameras.set_zoom_and_center(proof, camera, target_zoom, center);
        self.camera_written(output, camera, out);
    }

    fn recenter(&mut self, proof: authority::Authorized, out: &mut Vec<Effect>) {
        if let Some(element) = self.focus.target() {
            self.recenter_on_element(proof, element, out);
        } else {
            let output = match self.pointer {
                PointerState::Located { output, .. } => Some(output),
                PointerState::NoOutput => self.outputs.first().map(|o| o.id),
            };
            if let Some(output) = output
                && let Some(camera) = self.cameras.camera_for_output(output)
            {
                self.zoom_reset(proof, output, camera, out);
            }
        }
    }

    /// Super+1..9: focus, raise, and recenter camera on mapped window at slot `slot` (0-indexed creation order).
    fn focus_slot(
        &mut self,
        slot: usize,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        let live = self.focusable_elements_in_creation_order();
        let Some(&element) = live.get(slot) else {
            return;
        };
        if let PointerState::Located { output, .. } = self.pointer {
            if let Some(pres) = self.presentation(output) {
                if pres == element {
                    self.unsuppress_presentation(output);
                } else {
                    self.suppress_presentation(output);
                }
            }
        }
        let raised = self.raise_by_user(element);
        out.extend(self.focus.set_focus(Some(element)));
        out.extend(self.reconcile_constraints());
        if raised
            && matches!(self.pointer, pointer::PointerState::Located { .. })
            && !self.input.interaction.active()
            && !self.locked()
        {
            self.true_position_into(out);
        }
        if let Some(p) = proof {
            self.recenter(p, out);
        }
    }

    /// Super+Shift+1..9: focus, raise primary window, and frame/recenter camera on cluster at slot `slot` (0-indexed creation order).
    fn focus_cluster_slot(
        &mut self,
        slot: usize,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        let clusters = self.clusters();
        let Some(cluster) = clusters.get(slot) else {
            return;
        };
        let Some(&primary) = cluster.first() else {
            return;
        };
        if let PointerState::Located { output, .. } = self.pointer {
            if let Some(pres) = self.presentation(output) {
                if pres == primary {
                    self.unsuppress_presentation(output);
                } else {
                    self.suppress_presentation(output);
                }
            }
        }
        let raised = self.raise_by_user(primary);
        out.extend(self.focus.set_focus(Some(primary)));
        out.extend(self.reconcile_constraints());
        if raised
            && matches!(self.pointer, pointer::PointerState::Located { .. })
            && !self.input.interaction.active()
            && !self.locked()
        {
            self.true_position_into(out);
        }
        if let Some(p) = proof {
            self.frame_cluster(p, cluster, out);
        }
    }

    /// Fits or centers the camera on the cluster's bounding box.
    fn frame_cluster(
        &mut self,
        proof: authority::Authorized,
        cluster: &[crate::stage::ElementId],
        out: &mut Vec<Effect>,
    ) {
        if cluster.is_empty() {
            return;
        }
        let Some(output) = self.pointer_output().or_else(|| self.outputs().first().map(|o| o.id)) else {
            return;
        };
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return;
        };
        let (Some((view_size, scale)), Some(extent), Some(usable)) = (
            self.output(output).map(|m| (m.view_size(), m.scale)),
            self.output(output).map(|m| m.logical_extent()),
            self.usable_area(output),
        ) else {
            return;
        };

        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        for &e in cluster {
            let a = self.stage.anchor(e).unwrap_or(crate::geom::CanvasPoint { x: 0, y: 0 });
            let (w, h) = self
                .stage
                .tree(e)
                .map(|t| (t.geometry.size.w as i64, t.geometry.size.h as i64))
                .unwrap_or((0, 0));
            min_x = min_x.min(a.x);
            min_y = min_y.min(a.y);
            max_x = max_x.max(a.x.saturating_add(w));
            max_y = max_y.max(a.y.saturating_add(h));
        }
        if min_x > max_x || min_y > max_y {
            return;
        }
        let g = (min_x, min_y, (max_x - min_x).max(1), (max_y - min_y).max(1));
        if let Some(mut target) = crate::navigation::fit_target_in(view_size, extent, usable, scale, g) {
            // Cap zoom at 1.0 (never magnify small clusters beyond 100% native scale)
            target.zoom = target.zoom.min(1.0);
            self.cameras.fit(proof, camera, target);
            self.camera_written(output, camera, out);
        }
    }

    /// Super+Shift+Tab: scales the camera out to capture all mapped windows on the canvas.
    /// If the camera is already framing the overview, restores the previous camera zoom and position.
    pub(crate) fn overview_toggle(&mut self, proof: authority::Authorized, out: &mut Vec<Effect>) {
        let Some(output) = self.pointer_output().or_else(|| self.outputs.first().map(|o| o.id)) else {
            return;
        };
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return;
        };
        let (Some((view_size, scale)), Some(extent), Some(usable)) = (
            self.output(output).map(|m| (m.view_size(), m.scale)),
            self.output(output).map(|m| m.logical_extent()),
            self.usable_area(output),
        ) else {
            return;
        };

        let mapped = self.stage.mapped_elements();
        if mapped.is_empty() {
            out.push(Effect::ToggleOverview);
            return;
        }

        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        for &e in &mapped {
            let a = self.stage.anchor(e).unwrap_or(crate::geom::CanvasPoint { x: 0, y: 0 });
            let (w, h) = self
                .stage
                .tree(e)
                .map(|t| (t.geometry.size.w as i64, t.geometry.size.h as i64))
                .unwrap_or((0, 0));
            min_x = min_x.min(a.x);
            min_y = min_y.min(a.y);
            max_x = max_x.max(a.x.saturating_add(w));
            max_y = max_y.max(a.y.saturating_add(h));
        }

        if min_x > max_x || min_y > max_y {
            return;
        }

        // Add 8% comfortable canvas margin (at least 60px)
        let span_w = (max_x - min_x).max(1);
        let span_h = (max_y - min_y).max(1);
        let pad_x = (span_w / 12).max(60);
        let pad_y = (span_h / 12).max(60);

        let g = (
            min_x - pad_x,
            min_y - pad_y,
            span_w + 2 * pad_x,
            span_h + 2 * pad_y,
        );

        let Some(mut target) = crate::navigation::fit_target_in(view_size, extent, usable, scale, g) else {
            return;
        };
        target.zoom = target.zoom.min(1.0);

        let at_overview = self.cameras.center(camera) == Some(target.center)
            && self.cameras.zoom(camera).map(f64::to_bits) == Some(target.zoom.to_bits())
            && self.cameras.history(camera).is_some_and(|h| !h.is_empty());

        if at_overview {
            self.viewport_restore(proof, out);
        } else {
            self.cameras.fit(proof, camera, target);
            self.camera_written(output, camera, out);
        }
    }

    /// Super+Shift+Enter: the focused window's fullscreen toggles on the pointer's output, as if it
    /// had asked (`set_fullscreen` / `unset_fullscreen`: the same path, M9, INV-PRES-1).
    /// Under smart cluster tiling, entering fullscreen pushes any overlapping windows away into
    /// adjacent cluster slots (12px gap) so no window is ever occluded or covered. Exiting fullscreen
    /// restores original saved floating geometry.
    fn toggle_fullscreen(&mut self) -> Vec<Effect> {
        let Some(id) = self.focus.target() else {
            return Vec::new();
        };
        let mut fx = Vec::new();
        let was_fullscreen = self.fullscreen_of(id).is_some();
        if was_fullscreen {
            let request = crate::protocol::ToplevelRequest::UnsetFullscreen;
            if let Ok((_, t_fx)) = self.toplevel_request(id, request) {
                fx.extend(t_fx);
            }
            if let Some((orig_anchor, orig_size)) = self.stage.saved_floating(id) {
                let _ = self.stage.set_saved_floating(id, None);
                let _ = self.place_by_shell(id, orig_anchor, &mut fx);
                fx.push(Effect::ToplevelConfigureSize {
                    element: id,
                    size: orig_size,
                });
            }
            if let Some(o) = self.pointer_output() {
                self.invalidate_frame(o);
            }
            fx.extend(self.visibility_pass());
            fx
        } else {
            let output = self.pointer_output().or_else(|| self.outputs.first().map(|o| o.id));
            let orig_anchor = self.stage.anchor(id);
            let orig_size = self.stage.tree(id).map(|t| t.geometry.size);

            if let Some(out_id) = output
                && let Some(out_model) = self.outputs.iter().find(|o| o.id == out_id)
            {
                let (out_w, out_h) = out_model.logical_extent();
                let canvas_w = out_w.round().max(1.0) as i32;
                let canvas_h = out_h.round().max(1.0) as i32;

                if self.current_frame(out_id).is_none() {
                    let _ = self.render_list(out_id);
                }
                let tl = if let Some(frame) = self.current_frame(out_id)
                    && let Some(p) = frame.view.unproject(crate::geom::OutputLogicalPoint { x: 0.0, y: 0.0 }).round_to_point()
                {
                    p
                } else if let Some(a) = orig_anchor {
                    a
                } else {
                    crate::geom::CanvasPoint { x: 0, y: 0 }
                };

                if self.stage.saved_floating(id).is_none() {
                    if let (Some(a), Some(s)) = (orig_anchor, orig_size) {
                        let _ = self.stage.set_saved_floating(id, Some((a, s)));
                    }
                }

                let _ = self.place_by_shell(id, tl, &mut fx);
                let target_rect = (tl.x, tl.y, tl.x + canvas_w as i64, tl.y + canvas_h as i64);
                self.push_overlapping_cluster(id, target_rect, &mut fx);
            }

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
    }

    /// F-FIT-1 on `element` for `output`'s camera, shared by the binding and the shell: refused
    /// (nothing changed) when the element is not mapped, has no window geometry, or the fit is
    /// degenerate; otherwise one mutation (the prior value retained, OBJ-30) and the camera-write
    /// consequences. The callers' guards (presentation, pointer) come first.
    fn fit_on(
        &mut self,
        proof: authority::Authorized,
        output: OutputId,
        element: crate::stage::ElementId,
        out: &mut Vec<Effect>,
    ) -> Result<(), crate::ShellError> {
        let (camera, target) = self.fit_plan(output, element)?;
        self.cameras.fit(proof, camera, target);
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
    fn viewport_restore(&mut self, proof: authority::Authorized, out: &mut Vec<Effect>) {
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
        if !self.cameras.restore(proof, camera) {
            return;
        }
        self.camera_written(output, camera, out);
    }

    /// After a fit or restore of `camera` on `output`: the lattice reference follows (a zoom change
    /// sets it again, F-LATTICE-1), the frame is stale, membership and the scale policy are
    /// evaluated once for the camera (an atomic camera action: a one-step epoch, INV-SCALE-3), and
    /// the pointer is reconciled by M10's view-change rule — one true-position event when it is on
    /// O and no interaction owns it (the bindings never resolve during one), none otherwise. A lock
    /// kept by the reconciliation gets none (no absolute motion reaches a locked surface,
    /// INV-INPUT-5); a kept confinement gets the event like no constraint, so the confined client
    /// sees where the moved surface now lies under the pointer.
    fn camera_written(&mut self, output: OutputId, camera: usize, out: &mut Vec<Effect>) {
        self.update_lattice(output);
        self.invalidate_frame(output);
        self.visibility_pass_camera(camera, out);
        let on_it = matches!(self.pointer, PointerState::Located { output: o, .. } if o == output);
        if on_it && !self.input.interaction.active() {
            out.extend(self.reconcile_constraints());
            if !self.locked() {
                self.true_position_into(out);
            }
        }
    }

    /// Whether the active constraint is a lock: no absolute motion reaches its surface
    /// (zwp_pointer_constraints_v1; INV-INPUT-5).
    pub(crate) fn locked(&self) -> bool {
        self.constraint
            .is_some_and(|c| c.kind == pointer::ConstraintKind::Lock)
    }

    /// Motion follows the interaction (INV-INPUT-1): a pan moves the camera and nothing reaches a
    /// client (INV-INPUT-3, KeepFocus); an orphaned interaction or NoOutput consumes it; otherwise
    /// the pointer moves in the Arrangement and the event is routed, with its relative motion.
    fn pointer_motion(
        &mut self,
        proof: Option<authority::Authorized>,
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
        out: &mut Vec<Effect>,
    ) {
        match self.input.interaction {
            Interaction::CameraPan { output, camera, .. } => {
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
                }
                if self.focus_follows_mouse {
                    if let PointerState::Located { output: cur_o, p: cur_p } = self.pointer {
                        if self.layer_hit(cur_o, cur_p, true).is_none() {
                            let pt = crate::geom::OutputLogicalPoint {
                                x: cur_p.0,
                                y: cur_p.1,
                            };
                            if let Some(hit) = self.hit_test(cur_o, pt) {
                                if self.focus.target() != Some(hit.element) {
                                    out.extend(self.focus.set_focus(Some(hit.element)));
                                }
                            } else if self.focus_follows_mouse_strict && self.focus.target().is_some() {
                                out.extend(self.focus.set_focus(None));
                            }
                        }
                    }
                }
                self.routed_motion(delta, delta_unaccel, time_us, out);
            }
        }
    }

    /// One motion of a pan (F-PAN-1): the camera moves by the accumulated delta; the pointer stays.
    /// Returns whether the camera was written.
    fn pan_step(
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
        if self.fullscreen_pan && self.presentation(output).is_some() && !self.is_presentation_suppressed(output) {
            self.suppress_presentation(output);
        }
        if (self.pan_mode == crate::navigation::PanMode::Free || self.pan_mode == crate::navigation::PanMode::Rts)
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
            self.pointer = PointerState::Located {
                output,
                p: new_p,
            };
        }
        // F-LATTICE-1: the reference is kept while panning, unless the bound is exceeded
        self.update_lattice(output);
        self.invalidate_frame(output);
        self.visibility_pass_camera(camera, out);
        if self.pan_mode == crate::navigation::PanMode::Free || self.pan_mode == crate::navigation::PanMode::Rts {
            self.true_position_into(out);
        }
        true
    }

    /// One motion of a WindowMove or WindowResize (INC-14): the pointer moves in the Arrangement
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
                    grow(start_size.0, d(q_now.x, press.x), left),
                    grow(start_size.1, d(q_now.y, press.y), top),
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
                        let new_w = ((orig_s.0 as f64) * scale_x).round().clamp(64.0, 32768.0) as i32;
                        let new_h = ((orig_s.1 as f64) * scale_y).round().clamp(64.0, 32768.0) as i32;

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
    fn zoom_axis(
        &mut self,
        proof: authority::Authorized,
        axis: crate::input::Axis,
        out: &mut Vec<Effect>,
    ) {
        let PointerState::Located { output, p } = self.pointer else {
            return;
        };
        if let Some(pres) = self.presentation(output) {
            if !self.is_presentation_suppressed(output) {
                if self.fullscreen_pan {
                    if let Some(camera) = self.cameras.camera_for_output(output) {
                        if let Some((anchor, tree, _)) = self.stage.mapped(pres) {
                            let extent = self.output(output).map(|m| m.logical_extent()).unwrap_or((1920.0, 1080.0));
                            let margin_x = ((extent.0 - f64::from(tree.geometry.size.w)) / 2.0).max(0.0);
                            let margin_y = ((extent.1 - f64::from(tree.geometry.size.h)) / 2.0).max(0.0);
                            let pres_center = crate::geom::CanvasPos {
                                x: crate::geom::q16(anchor.x as f64 - margin_x + extent.0 / 2.0),
                                y: crate::geom::q16(anchor.y as f64 - margin_y + extent.1 / 2.0),
                            };
                            self.cameras.set_zoom_and_center(proof, camera, 1.0, pres_center);
                            self.camera_written(output, camera, out);
                        }
                    }
                    self.suppress_presentation(output);
                    return;
                } else {
                    return;
                }
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
            x: old_center.x.saturating_add(crate::geom::q16(dx)).clamp(-limit, limit),
            y: old_center.y.saturating_add(crate::geom::q16(dy)).clamp(-limit, limit),
        };

        self.cameras.set_zoom_and_center(proof, camera, new_zoom, new_center);
        self.camera_written(output, camera, out);
    }

    /// Super+Shift+Middle Click: reset camera zoom to 1.0 centered around pointer.
    fn zoom_reset(
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
        let dx = (p.0 - extent.0 / 2.0) * (1.0 / old_zoom - 1.0 / new_zoom);
        let dy = (p.1 - extent.1 / 2.0) * (1.0 / old_zoom - 1.0 / new_zoom);
        let limit = crate::geom::CANVAS_LIMIT_PX * crate::geom::Q16_ONE;
        let new_center = crate::geom::CanvasPos {
            x: old_center.x.saturating_add(crate::geom::q16(dx)).clamp(-limit, limit),
            y: old_center.y.saturating_add(crate::geom::q16(dy)).clamp(-limit, limit),
        };

        self.cameras.set_zoom_and_center(proof, camera, new_zoom, new_center);
        self.camera_written(output, camera, out);
    }

    /// Ownership is decided here, at press, before any client dispatch (INV-INPUT-1, C4.2 P2).
    /// The effects are appended to `out` (the caller's reusable buffer, C14.3, INV-ALLOC-1).
    fn button_press(
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
        // Super+Shift+Middle Click: reset camera zoom to default 1.0
        if mods.logo && mods.shift && !mods.ctrl && !mods.alt && button == crate::input::bindings::BTN_MIDDLE {
            if let Some(p) = proof {
                if let PointerState::Located { output, .. } = self.pointer
                    && let Some(camera) = self.cameras.camera_for_output(output)
                {
                    self.zoom_reset(p, output, camera, out);
                }
            }
            self.input.ownership.press(button, Owner::Compositor);
            return;
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
        let camera = self.cameras.camera_for_output(output);
        // G-OB24 is met (C12.F14.5): a pan at any zoom keeps the output's lattice reference
        let acquire = bindings::is_pan_binding(button, mods)
            // INV-INPUT-2: never while the client owns a button
            && !self.input.ownership.client_owns_any();
        // Super+left / Super+right on a window: move or resize it (owner request 2026-10-22, "like
        // Hyprland"); not on a panel above it, not a fullscreen window, never while the client
        // owns a button (INV-INPUT-2)
        if let Some(drag) = bindings::window_drag_binding(button, mods)
            && !self.input.ownership.client_owns_any()
            && self.layer_hit(output, p, true).is_none()
            && let Some(hit) =
                self.hit_test(output, crate::geom::OutputLogicalPoint { x: p.0, y: p.1 })
            && self.fullscreen_of(hit.element).is_none()
            && let (Some(anchor), Some(size)) = (
                self.stage.anchor(hit.element),
                self.stage.tree(hit.element).map(|t| t.geometry.size),
            )
        {
            let element = hit.element;
            self.raise_by_user(element);
            out.extend(self.focus.set_focus(Some(element)));
            out.push(Effect::DismissPopupGrab(element));
            // the pointer's Canvas position (R1): the hit's surface origin plus its local position
            let press = crate::geom::CanvasPos {
                x: hit.origin.x * crate::geom::Q16_ONE + hit.local.x,
                y: hit.origin.y * crate::geom::Q16_ONE + hit.local.y,
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
                        let s = self.stage.tree(e).map(|t| (t.geometry.size.w, t.geometry.size.h))?;
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
        if let (true, Some(camera), Some(start)) =
            (acquire, camera, camera.and_then(|c| self.cameras.center(c)))
        {
            let mut start = start;
            if self.fullscreen_pan && let Some(pres) = self.presentation(output) {
                if !self.is_presentation_suppressed(output) {
                    if let Some((anchor, tree, _)) = self.stage.mapped(pres) {
                        let extent = self.output(output).map(|m| m.logical_extent()).unwrap_or((1920.0, 1080.0));
                        let margin_x = ((extent.0 - f64::from(tree.geometry.size.w)) / 2.0).max(0.0);
                        let margin_y = ((extent.1 - f64::from(tree.geometry.size.h)) / 2.0).max(0.0);
                        let pres_center = crate::geom::CanvasPos {
                            x: crate::geom::q16(anchor.x as f64 - margin_x + extent.0 / 2.0),
                            y: crate::geom::q16(anchor.y as f64 - margin_y + extent.1 / 2.0),
                        };
                        if let Some(p) = proof {
                            self.cameras.set_zoom_and_center(p, camera, 1.0, pres_center);
                        }
                        start = pres_center;
                    }
                    self.suppress_presentation(output);
                }
            }
            // M6 Idle → Panning: the press is consumed; KeepFocus (no event).
            // OB-4: Dismiss active popup grabs before the camera change.
            self.input.interaction = Interaction::CameraPan {
                output,
                camera,
                initiator: button,
                start,
                sum: (0.0, 0.0),
            };
            self.input.ownership.press(button, Owner::Compositor);
            if let Some(target) = self.focus.target() {
                out.push(Effect::DismissPopupGrab(target));
            }
            return;
        }
        self.input.ownership.press(button, Owner::Client);
        // Click-to-focus (INV-INPUT-8, M11): an explicit user action.
        // If the click hits an element, that element gains ActiveElementFocus. A click on a layer
        // surface above the Canvas reaches it instead, and focuses it when it is on-demand
        // interactive (L3, L5); below the Canvas it is reached only where no element is.
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
            out.extend(self.focus.set_focus(Some(hit.element)));
        } else if let Some(h) = on_layer
            && self
                .layer(h.layer)
                .is_some_and(|r| r.state.keyboard == crate::layer::KeyboardInteractivity::OnDemand)
        {
            out.extend(self.focus.layer_take(h.layer, false));
        }
        out.push(Effect::PointerButton {
            button,
            pressed: true,
            time_ms,
        });
        out.push(Effect::PointerFrame);
    }

    /// The end of a compositor interaction, by its initiator's release or by a cancel (M22):
    /// batch-end reconciliation (INV-INPUT-5), exactly one true-position event unless the
    /// reconciliation keeps a lock (M10, C12.F17 Q3: a kept confinement receives it), and the
    /// CameraPan epoch settles (INV-SCALE-3: it ends at interaction termination).
    fn end_interaction(&mut self) -> Vec<Effect> {
        let resized = matches!(
            self.input.interaction,
            Interaction::ElementResize { is_cluster: false, .. }
        );
        self.input.cluster_drag.clear();
        self.input.interaction = Interaction::Idle;
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
    fn button_release(&mut self, button: u32, time_ms: u32, out: &mut Vec<Effect>) {
        match self.input.ownership.release(button) {
            None => {}
            Some(Owner::Compositor) => {
                if self.input.interaction.initiator() == Some(button) {
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

    /// C7.2 / INV-SHELL-1: one semantic command of the trusted shell, validated in full before
    /// anything is written (C7.3: atomic; an error changes nothing) and executed through the same
    /// single-writer paths user input uses. A command that moves a camera consumes a proof minted
    /// here, per command (INV-CAMERA-1). Effects are returned for the adapter to apply.
    pub fn shell_command(
        &mut self,
        port: &mut ShellPort,
        cmd: ShellCommand,
    ) -> Result<Vec<Effect>, ShellError> {
        if port.core_id != self.id {
            return Err(ShellError::ForeignPort);
        }
        let mut out = Vec::new();
        match cmd {
            ShellCommand::Navigate { output, to, stream } => {
                let camera = self.shell_camera(output)?;
                let center = self.navigate_center(camera, to)?;
                self.shell_stream_step(stream);
                self.cameras
                    .navigate(authority::Authorized::mint(), camera, center);
                self.camera_written(output, camera, &mut out);
                self.shell_stream_end(stream, &mut out);
            }
            ShellCommand::ViewportFit { output, element } => {
                let element = self
                    .stage
                    .resolve(element)
                    .map_err(|_| ShellError::StaleElement)?;
                self.shell_camera(output)?;
                self.fit_on(authority::Authorized::mint(), output, element, &mut out)?;
            }
            ShellCommand::ViewportRestore { output } => {
                let camera = self.shell_camera(output)?;
                if self.cameras.history(camera).is_none_or(|h| h.is_empty()) {
                    return Err(ShellError::Refused);
                }
                if self.cameras.restore(authority::Authorized::mint(), camera) {
                    self.camera_written(output, camera, &mut out);
                }
            }
            ShellCommand::Focus { element } => {
                let id = self.shell_mapped(element)?;
                // A-8: focus never moves to an element a presentation hides
                if self.hidden_by_presentation(id) {
                    return Err(ShellError::Presenting);
                }
                out.extend(self.focus.set_focus(Some(id)));
                out.extend(self.reconcile_constraints());
            }
            ShellCommand::AcknowledgeAttention { element } => {
                let id = self
                    .stage
                    .resolve(element)
                    .map_err(|_| ShellError::StaleElement)?;
                self.focus.acknowledge_attention(id);
            }
            ShellCommand::MoveElement {
                element,
                to,
                stream,
            } => {
                let id = self.shell_mapped(element)?;
                if self.fullscreen_of(id).is_some() {
                    return Err(ShellError::Presenting);
                }
                let anchor = shell_canvas_pos(to)
                    .and_then(|p| p.round_to_point())
                    .ok_or(ShellError::InvalidArgument)?;
                self.shell_stream_step(stream);
                self.place_by_shell(id, anchor, &mut out)
                    .map_err(|_| ShellError::StaleElement)?;
                // what lies under the pointer may have changed: M10, as after a camera write
                if matches!(self.pointer, PointerState::Located { .. })
                    && !self.input.interaction.active()
                {
                    out.extend(self.reconcile_constraints());
                    if !self.locked() {
                        self.true_position_into(&mut out);
                    }
                }
                self.shell_stream_end(stream, &mut out);
            }
        }
        Ok(out)
    }

    /// Ends an open shell stream (its connection closed, C7.3): its epoch settles. No camera or
    /// Stage write; nothing happens when no stream is open.
    pub fn end_shell_stream(&mut self, port: &mut ShellPort) -> Result<Vec<Effect>, ShellError> {
        if port.core_id != self.id {
            return Err(ShellError::ForeignPort);
        }
        let mut out = Vec::new();
        self.shell_stream_end(ShellStream::End, &mut out);
        Ok(out)
    }

    /// The camera a shell command may write on `output`: the output is connected and has one, it
    /// presents no protocol-fullscreen element (C12.F17 Q1 B scope 2), and no interaction owns
    /// the pointer (a drag is moving a camera).
    fn shell_camera(&self, output: OutputId) -> Result<usize, ShellError> {
        let camera = self
            .output(output)
            .and_then(|_| self.cameras.camera_for_output(output))
            .ok_or(ShellError::UnknownOutput)?;
        if let Some(pres) = self.presentation(output) {
            if self.focus.target() == Some(pres) {
                return Err(ShellError::Presenting);
            }
        }
        if self.input.interaction.active() {
            return Err(ShellError::Busy);
        }
        Ok(camera)
    }

    /// The element a reference names, mapped.
    fn shell_mapped(
        &self,
        element: crate::stage::ElementRef,
    ) -> Result<crate::stage::ElementId, ShellError> {
        let id = self
            .stage
            .resolve(element)
            .map_err(|_| ShellError::StaleElement)?;
        self.stage
            .mapped(id)
            .map(|_| id)
            .ok_or(ShellError::NotMapped)
    }

    /// Where a `Navigate` puts the camera's centre (its zoom stays): within the stored Canvas,
    /// ±2³⁶ px, or the command is invalid.
    fn navigate_center(
        &self,
        camera: usize,
        to: NavigateTo,
    ) -> Result<crate::geom::CanvasPos, ShellError> {
        use crate::geom::{CANVAS_LIMIT_PX, CanvasPos, Q16_ONE};
        let center = match to {
            NavigateTo::Element(r) => {
                let id = self.shell_mapped(r)?;
                let (Some(a), Some(tree)) = (self.stage.anchor(id), self.stage.tree(id)) else {
                    return Err(ShellError::NotMapped);
                };
                // the centre of the window geometry, exact in Q16 (Q16_ONE is even)
                let size = tree.geometry.size;
                CanvasPos {
                    x: a.x * Q16_ONE + size.w as i64 * (Q16_ONE / 2),
                    y: a.y * Q16_ONE + size.h as i64 * (Q16_ONE / 2),
                }
            }
            NavigateTo::Canvas(x, y) => {
                shell_canvas_pos((x, y)).ok_or(ShellError::InvalidArgument)?
            }
            NavigateTo::Delta(dx, dy) => {
                let d = shell_canvas_pos((dx, dy)).ok_or(ShellError::InvalidArgument)?;
                let c = self
                    .cameras
                    .center(camera)
                    .ok_or(ShellError::UnknownOutput)?;
                CanvasPos {
                    x: c.x.saturating_add(d.x),
                    y: c.y.saturating_add(d.y),
                }
            }
        };
        let limit = CANVAS_LIMIT_PX * Q16_ONE;
        if center.x.abs() > limit || center.y.abs() > limit {
            return Err(ShellError::InvalidArgument);
        }
        Ok(center)
    }

    /// A step of a shell stream opens it (INV-SCALE-3 `ShellStream`).
    fn shell_stream_step(&mut self, stream: ShellStream) {
        if stream != ShellStream::Atomic {
            self.shell_stream = true;
        }
    }

    /// The explicit end-of-stream: the stream's epoch settles (INV-SCALE-3).
    fn shell_stream_end(&mut self, stream: ShellStream, out: &mut Vec<Effect>) {
        if stream == ShellStream::End && self.shell_stream {
            self.shell_stream = false;
            out.extend(self.settle_interaction(crate::scale::InteractionKind::ShellStream));
        }
    }

    /// M4 `Absent → Enabled` (startup and hotplug alike): validates the hardware facts, allocates
    /// a new `OutputId`, binds a camera by the M5 precedence (one proof), places the output in the
    /// Arrangement, leaves `NoOutput` by the startup rule, and runs the visibility pass. Effects:
    /// `OutputAdded` first, then enters.
    pub fn output_connected(
        &mut self,
        token: &mut OutputLifecycleToken,
        desc: OutputDescriptor,
    ) -> Result<(OutputId, Vec<Effect>), EntryError> {
        self.check(token.core_id)?;
        if !output::valid(&desc) {
            return Err(EntryError::InvalidDescriptor);
        }
        let id = self.output_ids.next();
        let right_edge = self
            .outputs
            .iter()
            .map(|o| o.arrangement.0 + o.logical_extent().0)
            .fold(0.0, f64::max);
        let model = OutputModel::from_descriptor(id, &desc, (right_edge, 0.0));
        self.cameras.bind_on_connect(
            authority::Authorized::mint(),
            &desc.key,
            id,
            &self.persisted,
        );
        let was_zero = self.pointer == PointerState::NoOutput;
        if was_zero {
            self.pointer = pointer::startup(&model);
        }
        self.outputs.push(model);
        // F-LATTICE-1: O connected and a camera bound — the reference is set from the view
        self.update_lattice(id);
        let mut fx = vec![Effect::OutputAdded(id)];
        fx.extend(self.visibility_pass());
        if was_zero {
            // M10 NoOutput → Located: the grab goes first, then one routed motion (enter) unless
            // an orphaned interaction still owns the pointer
            fx.push(Effect::PointerZeroOutputEnd);
            if !self.input.interaction.active() {
                fx.extend(self.true_position());
            }
        }
        fx.extend(self.reconcile_constraints());
        fx.extend(self.settle_all_epochs());
        Ok((id, fx))
    }

    /// M4 reconfiguration: new hardware facts for a connected output, same `OutputId`, same
    /// `OutputKey` (else rejected, nothing mutated). Not a camera authority: no proof is minted
    /// and no camera item is called (E1R; checked by tools/verify-architecture). The view,
    /// ProjectionFrame and membership are rebuilt; the pointer is clamped (F-RECONF-1). Effects:
    /// `OutputChanged`, then leaves and enters.
    pub fn output_reconfigured(
        &mut self,
        token: &mut OutputLifecycleToken,
        output: OutputId,
        desc: OutputDescriptor,
    ) -> Result<Vec<Effect>, EntryError> {
        self.check(token.core_id)?;
        let slot = self
            .outputs
            .iter_mut()
            .find(|o| o.id == output)
            .ok_or(EntryError::UnknownOutput)?;
        if slot.key != desc.key {
            return Err(EntryError::IdentityChanged);
        }
        if !output::valid(&desc) {
            return Err(EntryError::InvalidDescriptor);
        }
        *slot = OutputModel::from_descriptor(output, &desc, slot.arrangement);
        let model = slot.clone();
        // F-LATTICE-1: a new k_O or view_O sets the reference again; otherwise it is kept
        self.update_lattice(output);
        self.invalidate_frame(output);
        self.pointer = pointer::reconfigured(self.pointer, &model);
        let mut fx = vec![Effect::OutputChanged(output)];
        // its layer surfaces are arranged in the new extent and told the new scale (L1)
        fx.extend(self.layers_output_reconfigured(output));
        fx.extend(self.fullscreen_output_reconfigured(output));
        fx.extend(self.visibility_pass());
        // M10: at most one true-position event, none while an interaction owns the pointer, and
        // none to a lock the reconciliation keeps (zwp_pointer_constraints_v1: no motion while
        // locked; OB-55)
        fx.extend(self.reconcile_constraints());
        let on_it = matches!(self.pointer, PointerState::Located { output: o, .. } if o == output);
        if on_it && !self.input.interaction.active() && !self.locked() {
            fx.extend(self.true_position());
        }
        fx.extend(self.settle_all_epochs());
        // a presented element is told its new size: the hint for it goes first (as at entry)
        Self::hints_before_configures(&mut fx);
        Ok(fx)
    }

    /// M4 `Enabled → Removed` (INV-OUTPUT-1): the camera is unbound and kept (one proof); the
    /// output's ProjectionFrame is dropped; the pointer is relocated to a surviving output
    /// (F-RELOCATE-1) or, when none survives, enters `NoOutput`. Effects: every leave for the
    /// output, then `OutputRemoved`.
    pub fn output_disconnected(
        &mut self,
        token: &mut OutputLifecycleToken,
        output: OutputId,
    ) -> Result<Vec<Effect>, EntryError> {
        self.check(token.core_id)?;
        let index = self
            .outputs
            .iter()
            .position(|o| o.id == output)
            .ok_or(EntryError::UnknownOutput)?;
        let lost = self.outputs.remove(index);
        self.remove_frame(output);
        self.remove_lattice(output);
        // its layer surfaces are closed before the pointer is relocated (they are not on any
        // output any more) and before the global goes (their leaves first, INV-OUTPUT-1)
        let closed = self.layers_output_removed(output);
        self.cameras
            .unbind_on_disconnect(authority::Authorized::mint(), output);
        // C4.6: a pan on the removed output keeps its ownership and consumes motion until release
        self.input.interaction.orphan_if_on(output);
        let mut pointer_fx = Vec::new();
        if let PointerState::Located { output: o, p } = self.pointer
            && o == output
        {
            self.pointer = match self.outputs.split_first() {
                Some((first, rest)) => pointer::relocate(&lost, p, first, rest),
                None => PointerState::NoOutput,
            };
            if self.pointer == PointerState::NoOutput {
                // M10 → NoOutput: Focus::Clear grab (leave), nothing to the old focus
                self.constraint = None;
                pointer_fx.push(Effect::PointerZeroOutputBegin);
            } else {
                // M10: exactly one true-position event unless an interaction owns the pointer
                // or the reconciliation keeps a lock (C12.F17 Q3: a kept confinement receives it)
                let recon_fx = self.reconcile_constraints();
                pointer_fx.extend(recon_fx);
                if !self.input.interaction.active() && !self.locked() {
                    pointer_fx.extend(self.true_position());
                }
            }
        }
        // INV-OUTPUT-1: "fullscreen presentation dropped with the INV-PRES-1 configure"
        let restored = self.fullscreen_output_removed(output);
        let mut fx = closed;
        fx.extend(self.visibility_pass());
        fx.extend(restored);
        fx.extend(pointer_fx);
        fx.push(Effect::OutputRemoved(output));
        fx.extend(self.settle_all_epochs());
        Ok(fx)
    }
}

/// `wl_pointer` timestamps: milliseconds with wrap-around.
fn ms(time_us: u64) -> u32 {
    crate::input::wire_ms(time_us)
}

#[cfg(test)]
mod tests {
    //! The 2R / E1R output-lifecycle tests (spike EE-4 / ER2-1) ported to the production core.
    //! Expected values are hand-derived literals, not values computed by the code under test.

    use crate::geom::CanvasPos;
    use crate::navigation::PersistedCameras;
    use crate::output::tests::key;
    use crate::output::{OutputDescriptor, OutputId, OutputKey, Transform};
    use crate::input::bindings::{BTN_MIDDLE, Modifiers};
    use crate::pointer::PointerState;
    use crate::protocol::Effect;
    use crate::{Core, EntryError};

    fn desc_k(
        k: OutputKey,
        mode: (u32, u32),
        t: Transform,
        scale_120: u32,
        refresh_mhz: u32,
    ) -> OutputDescriptor {
        OutputDescriptor {
            key: k,
            mode,
            transform: t,
            scale_120,
            refresh_mhz,
        }
    }

    fn desc(connector: &str) -> OutputDescriptor {
        desc_k(key(connector), (1920, 1080), Transform::Normal, 120, 60_000)
    }

    fn pos(x: i64, y: i64) -> CanvasPos {
        CanvasPos {
            x: x << 16,
            y: y << 16,
        }
    }

    fn camera_of(c: &Core, o: OutputId) -> usize {
        c.cameras().camera_for_output(o).expect("bound")
    }

    /// A token minted by another Core grants nothing (C1.5, C12.R2.3).
    #[test]
    fn foreign_tokens_grant_nothing() {
        let (mut a, _, _, mut own) = Core::new(PersistedCameras::default());
        let (_b, _, _, mut foreign) = Core::new(PersistedCameras::default());
        let (o, _) = a.output_connected(&mut own, desc("A")).unwrap();
        assert_eq!(
            a.output_connected(&mut foreign, desc("B")).err(),
            Some(EntryError::ForeignPort)
        );
        assert_eq!(
            a.output_reconfigured(&mut foreign, o, desc("A")).err(),
            Some(EntryError::ForeignPort)
        );
        assert_eq!(
            a.output_disconnected(&mut foreign, o).err(),
            Some(EntryError::ForeignPort)
        );
        assert_eq!(
            (a.cameras().len(), a.outputs().len()),
            (1, 1),
            "a foreign token changed state"
        );
    }

    /// M4/M5: no camera exists before a connect; each connect creates and binds its own camera at
    /// the default placement; connecting another output leaves existing cameras untouched.
    #[test]
    fn connect_creates_and_binds_one_camera_per_output() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        assert!(c.cameras().is_empty());
        let (a, fx) = c.output_connected(&mut t, desc("A")).unwrap();
        // M10 NoOutput → Located: the startup pointer (960, 540) unprojects through
        // c_eff = (−960, −540) to Canvas (0, 0); nothing is mapped, so the focus stays clear
        assert_eq!(
            fx,
            vec![
                Effect::OutputAdded(a),
                Effect::PointerZeroOutputEnd,
                Effect::PointerMotion {
                    target: None,
                    location: pos(0, 0),
                    time_ms: 0
                },
                Effect::PointerFrame
            ]
        );
        assert_eq!(camera_of(&c, a), 0);
        assert_eq!(
            (c.cameras().center(0), c.cameras().zoom(0)),
            (Some(pos(0, 0)), Some(1.0))
        );
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        assert_ne!(a, b);
        assert_eq!(camera_of(&c, b), 1);
        assert_eq!(
            c.cameras().center(0),
            Some(pos(0, 0)),
            "connecting B changed A's camera"
        );
        assert_eq!(c.outputs().len(), 2);
        assert_eq!(
            c.output(b).unwrap().arrangement,
            (1920.0, 0.0),
            "B is arranged right of A"
        );
    }

    /// M5: disconnect unbinds and keeps the camera and its value; reconnecting the same key re-binds
    /// the same camera unchanged with a new OutputId (reconnect is not reconfiguration).
    #[test]
    fn disconnect_keeps_the_camera_and_reconnect_rebinds_it() {
        let persisted =
            PersistedCameras::from_entries(vec![(key("A"), pos(-25, 40), 1.0)]).unwrap();
        let (mut c, _i, _s, mut t) = Core::new(persisted);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(c.cameras().center(0), Some(pos(-25, 40)));
        let fx = c.output_disconnected(&mut t, a).unwrap();
        // the last output: M10 → NoOutput installs the zero-output grab before the removal
        assert_eq!(
            fx,
            vec![Effect::PointerZeroOutputBegin, Effect::OutputRemoved(a)]
        );
        assert_eq!(c.cameras().len(), 1, "camera destroyed on disconnect");
        assert_eq!(c.cameras().bound_output(0), None);
        assert_eq!(c.cameras().center(0), Some(pos(-25, 40)));
        assert_eq!(
            c.output_disconnected(&mut t, a).err(),
            Some(EntryError::UnknownOutput),
            "repeated disconnect"
        );
        let (a2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_ne!(a, a2, "OutputId reused");
        assert_eq!(camera_of(&c, a2), 0, "the remembered camera is re-bound");
        assert_eq!(c.cameras().len(), 1);
        assert_eq!(c.cameras().center(0), Some(pos(-25, 40)));
    }

    /// M20 / OB-24 D2 (C12.F14): the persisted table is accepted or rejected whole. A valid
    /// table seeds its cameras; an empty one and a rejected one — corrupt (a non-finite zoom) or
    /// partially valid (one entry outside the representable domain) — leave every output with
    /// the default camera: none of the table's valid entries is applied.
    #[test]
    fn a_rejected_persisted_table_applies_none_of_its_entries() {
        use crate::geom::ZOOM_MAX;
        let valid = (key("A"), pos(300, -200), 2.0);
        let tables = [
            (vec![valid.clone(), (key("B"), pos(9, 9), 1.5)], true),
            (vec![], true),
            (vec![valid.clone(), (key("B"), pos(9, 9), f64::NAN)], false),
            (
                vec![valid.clone(), (key("B"), pos(9, 9), ZOOM_MAX * 2.0)],
                false,
            ),
            (vec![(key("B"), pos(9, 9), 0.0), valid.clone()], false),
        ];
        for (entries, accepted) in tables {
            let applies_a = accepted && !entries.is_empty();
            let table = PersistedCameras::from_entries(entries);
            assert_eq!(table.is_ok(), accepted);
            // a rejected table is replaced by the empty/default state (M20)
            let (mut c, _i, _s, mut t) = Core::new(table.unwrap_or_default());
            let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
            let cam = camera_of(&c, a);
            if applies_a {
                assert_eq!(c.cameras().center(cam), Some(pos(300, -200)));
                assert_eq!(c.cameras().zoom(cam), Some(2.0));
            } else {
                assert_eq!(c.cameras().center(cam), Some(pos(0, 0)));
                assert_eq!(c.cameras().zoom(cam), Some(1.0));
            }
        }
    }

    /// INV-CAMERA-3 rule order: a camera remembered this session wins over the persisted entry.
    #[test]
    fn remembered_camera_wins_over_the_persisted_entry() {
        let persisted = PersistedCameras::from_entries(vec![
            (key("A"), pos(5000, -700), 1.5),
            (key("C"), pos(-123, 456), 0.75),
        ])
        .unwrap();
        let (mut c, _i, _s, mut t) = Core::new(persisted);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        assert_eq!(
            (
                c.cameras().center(camera_of(&c, a)),
                c.cameras().zoom(camera_of(&c, a))
            ),
            (Some(pos(5000, -700)), Some(1.5))
        );
        assert_eq!(
            (
                c.cameras().center(camera_of(&c, b)),
                c.cameras().zoom(camera_of(&c, b))
            ),
            (Some(pos(0, 0)), Some(1.0))
        );
        let (cc, _) = c.output_connected(&mut t, desc("C")).unwrap();
        assert_eq!(
            c.cameras().center(camera_of(&c, cc)),
            Some(pos(-123, 456)),
            "runtime hotplug takes the same path"
        );
        c.output_disconnected(&mut t, a).unwrap();
        let (a2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(camera_of(&c, a2), 0);
        assert_eq!(c.cameras().len(), 3, "no new camera for a remembered key");
    }

    /// Runtime 2i (OB-49, transferred from OB-19): two CONNECTED outputs presenting the same
    /// OutputKey each keep their own camera (1:1); the duplicate is not seeded from persistence;
    /// on reconnect the lowest unbound camera with the key is re-bound. Under the
    /// `probe_sealed_key_steal` feature (the sealed 2R rule) this test must fail with "1:1 violated".
    #[test]
    fn concurrent_output_key_collision_keeps_one_to_one() {
        let persisted =
            PersistedCameras::from_entries(vec![(key("A"), pos(77, 88), 1.25)]).unwrap();
        let (mut c, _i, _s, mut t) = Core::new(persisted);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (ca, cb) = (
            c.cameras().camera_for_output(a),
            c.cameras().camera_for_output(b),
        );
        assert!(
            ca.is_some() && cb.is_some() && ca != cb,
            "1:1 violated: a={ca:?} b={cb:?}"
        );
        // every connected output has exactly one camera, every camera at most one output
        for o in [a, b] {
            let n = (0..c.cameras().len())
                .filter(|&i| c.cameras().bound_output(i) == Some(o))
                .count();
            assert_eq!(n, 1, "1:1 violated: output {o:?} has {n} cameras");
        }
        assert_eq!(
            (
                c.cameras().center(ca.unwrap()),
                c.cameras().zoom(ca.unwrap())
            ),
            (Some(pos(77, 88)), Some(1.25))
        );
        assert_eq!(
            (
                c.cameras().center(cb.unwrap()),
                c.cameras().zoom(cb.unwrap())
            ),
            (Some(pos(0, 0)), Some(1.0)),
            "duplicate seeded from persistence"
        );
        // the two views differ: no aliasing of camera state
        assert_ne!(c.view(a).unwrap().c_eff, c.view(b).unwrap().c_eff);
        c.output_disconnected(&mut t, a).unwrap();
        c.output_disconnected(&mut t, b).unwrap();
        let (a2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(
            c.cameras().camera_for_output(a2),
            ca,
            "reconnect must take the lowest unbound camera"
        );
        let (b2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(
            c.cameras().camera_for_output(b2),
            cb,
            "then the next unbound camera"
        );
        assert_eq!(c.cameras().len(), 2);
    }

    /// E1R (EE-4): mode, scale, transform and refresh changes keep the OutputId, the camera
    /// identity and value, and allocate nothing.
    #[test]
    fn reconfiguration_keeps_identity_and_camera() {
        let persisted = PersistedCameras::from_entries(vec![(key("A"), pos(17, -3), 1.0)]).unwrap();
        let (mut c, _i, _s, mut t) = Core::new(persisted);
        let (o, _) = c.output_connected(&mut t, desc("A")).unwrap();
        for d in [
            desc_k(key("A"), (2560, 1440), Transform::Normal, 120, 60_000),
            desc_k(key("A"), (2560, 1440), Transform::Normal, 150, 60_000),
            desc_k(key("A"), (2560, 1440), Transform::R90, 150, 60_000),
            desc_k(key("A"), (2560, 1440), Transform::R90, 150, 144_000),
        ] {
            let fx = c.output_reconfigured(&mut t, o, d).unwrap();
            assert_eq!(fx[0], Effect::OutputChanged(o));
            assert_eq!(c.outputs().len(), 1);
            assert_eq!(c.outputs()[0].id, o);
            assert_eq!(camera_of(&c, o), 0);
            assert_eq!(c.cameras().len(), 1);
            assert_eq!(
                (c.cameras().center(0), c.cameras().zoom(0)),
                (Some(pos(17, -3)), Some(1.0))
            );
        }
        let m = c.output(o).unwrap();
        assert_eq!(
            (m.phys, m.view_size(), m.scale, m.refresh_mhz),
            ((2560, 1440), (1440, 2560), 1.25, 144_000)
        );
    }

    /// E1R: unknown and stale ids, foreign tokens, a changed key and an invalid descriptor are
    /// rejected and mutate nothing.
    #[test]
    fn reconfiguration_rejections_mutate_nothing() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (o, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let before = (c.outputs().to_vec(), c.pointer(), c.cameras().len());
        let (_other, _, _, mut foreign) = Core::new(PersistedCameras::default());
        assert_eq!(
            c.output_reconfigured(&mut t, OutputId::for_test(99), desc("A"))
                .err(),
            Some(EntryError::UnknownOutput)
        );
        assert_eq!(
            c.output_reconfigured(&mut foreign, o, desc("A")).err(),
            Some(EntryError::ForeignPort)
        );
        assert_eq!(
            c.output_reconfigured(&mut t, o, desc("B")).err(),
            Some(EntryError::IdentityChanged)
        );
        assert_eq!(
            c.output_reconfigured(
                &mut t,
                o,
                desc_k(key("A"), (0, 0), Transform::Normal, 120, 60_000)
            )
            .err(),
            Some(EntryError::InvalidDescriptor)
        );
        assert_eq!(
            (c.outputs().to_vec(), c.pointer(), c.cameras().len()),
            before
        );
        c.output_disconnected(&mut t, o).unwrap();
        let (o2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_ne!(o, o2);
        assert_eq!(
            c.output_reconfigured(&mut t, o, desc("A")).err(),
            Some(EntryError::UnknownOutput),
            "stale OutputId"
        );
    }

    /// F-RECONF-1: p is clamped into the new extent (half-open, wl_fixed); inside stays.
    #[test]
    fn reconfiguration_clamps_the_pointer() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (o, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: o,
                p: (960.0, 540.0)
            }
        );
        // 90° at scale 1.5: logical 720 x 1280 → x clamps to 720 − 2⁻⁸
        c.output_reconfigured(
            &mut t,
            o,
            desc_k(key("A"), (1920, 1080), Transform::R90, 180, 60_000),
        )
        .unwrap();
        let want = PointerState::Located {
            output: o,
            p: (720.0 - 1.0 / 256.0, 540.0),
        };
        assert_eq!(c.pointer(), want);
        c.output_reconfigured(&mut t, o, desc("A")).unwrap();
        assert_eq!(
            c.pointer(),
            want,
            "a pointer inside the new extent is unchanged"
        );
    }

    /// INV-OUTPUT-1 zero outputs: before any connect and after the last disconnect the pointer is
    /// `NoOutput`; nothing is fabricated; cameras persist unbound; the first connect places the
    /// pointer at the new output's centre; a later connect does not move a located pointer.
    #[test]
    fn zero_output_state_is_stable_and_the_first_connect_leaves_it() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        assert_eq!(c.pointer(), PointerState::NoOutput);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.output_disconnected(&mut t, a).unwrap();
        assert!(c.outputs().is_empty());
        assert_eq!(c.pointer(), PointerState::NoOutput);
        assert_eq!(c.cameras().bound_output(0), None);
        assert_eq!(
            c.output_disconnected(&mut t, a).err(),
            Some(EntryError::UnknownOutput)
        );
        assert_eq!(c.pointer(), PointerState::NoOutput);
        let (b, _) = c
            .output_connected(
                &mut t,
                desc_k(key("B"), (2560, 1440), Transform::R270, 240, 60_000),
            )
            .unwrap();
        // view 1440x2560 at scale 2 → logical 720x1280; centre (360, 640)
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: b,
                p: (360.0, 640.0)
            }
        );
        let (a2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(camera_of(&c, a2), 0);
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: b,
                p: (360.0, 640.0)
            }
        );
    }

    /// F-RELOCATE-1: with survivors the pointer moves to the Arrangement-nearest output; with none
    /// it enters NoOutput (the formula's domain excludes the empty set).
    #[test]
    fn relocation_uses_survivors_only() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap(); // pointer (960,540) on A
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap(); // B at x = 1920
        c.output_disconnected(&mut t, a).unwrap();
        // a = (960,540); B's rect is [1920, 3840) × [0, 1080): nearest point x = 1920 → p = (0, 540)
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: b,
                p: (0.0, 540.0)
            }
        );
        c.output_disconnected(&mut t, b).unwrap();
        assert_eq!(c.pointer(), PointerState::NoOutput);
    }

    /// The nearer of two survivors wins, and only the interaction output's loss relocates.
    /// Arrangement: A [0,1920), B [1920,3840), C [3840,5760), D [5760,7680); pointer (960,540) on A.
    #[test]
    fn relocation_picks_the_nearest_survivor() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        let (_c3, _) = c.output_connected(&mut t, desc("C")).unwrap();
        let (d, _) = c.output_connected(&mut t, desc("D")).unwrap();
        c.output_disconnected(&mut t, d).unwrap();
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: a,
                p: (960.0, 540.0)
            },
            "not the interaction output"
        );
        // survivors B (distance 960) and C (distance 2880): B, at its left edge
        c.output_disconnected(&mut t, a).unwrap();
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: b,
                p: (0.0, 540.0)
            }
        );
    }

    /// A disconnected output's ProjectionFrame does not survive: no hit-test and no frame for a
    /// stale OutputId.
    #[test]
    fn disconnect_drops_the_output_frame() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let id = c.toplevel_created();
        let tree = |mapped| crate::snapshot::SurfaceTree {
            geometry: crate::geom::LocalRect {
                loc: crate::geom::LocalPoint::default(),
                size: crate::geom::LocalSize { w: 100, h: 100 },
            },
            surfaces: vec![crate::snapshot::SnapshotSurface {
                key: crate::snapshot::tests::key(1),
                role: crate::snapshot::SurfaceRole::Root,
                size: crate::geom::LocalSize { w: 100, h: 100 },
                mapped,
                input: crate::snapshot::InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        c.toplevel_committed(id, tree(false)).unwrap();
        c.toplevel_committed(id, tree(true)).unwrap();
        c.render_list(a).unwrap();
        let p = crate::geom::OutputLogicalPoint { x: 970.5, y: 550.5 };
        assert!(c.hit_test(a, p).is_some());
        c.output_disconnected(&mut t, a).unwrap();
        assert_eq!(c.hit_test(a, p), None, "a stale frame survived its output");
        assert_eq!(
            c.render_list(a).err(),
            Some(crate::FrameError::UnknownOutput)
        );
    }

    /// INC2-F07: A ProjectionFrame belongs strictly to the output/view configuration from which it
    /// was built and must not survive output_reconfigured. Before reconfiguration, p_before hits
    /// the frame; reconfiguring mode, transform or scale invalidates the frame so that the stale
    /// frame is not used by a subsequent hit-test.
    #[test]
    fn reconfiguration_drops_the_output_frame() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let id = c.toplevel_created();
        let tree = |mapped| crate::snapshot::SurfaceTree {
            geometry: crate::geom::LocalRect {
                loc: crate::geom::LocalPoint::default(),
                size: crate::geom::LocalSize { w: 100, h: 100 },
            },
            surfaces: vec![crate::snapshot::SnapshotSurface {
                key: crate::snapshot::tests::key(1),
                role: crate::snapshot::SurfaceRole::Root,
                size: crate::geom::LocalSize { w: 100, h: 100 },
                mapped,
                input: crate::snapshot::InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        c.toplevel_committed(id, tree(false)).unwrap();
        c.toplevel_committed(id, tree(true)).unwrap();
        c.render_list(a).unwrap();
        // The 100x100 window is centred (anchor (−50,−50)): at 1920x1080 scale 1.0 it is at
        // OutputLogical [910,1010)x[490,590).
        let p_before = crate::geom::OutputLogicalPoint { x: 970.5, y: 550.5 };
        assert!(c.hit_test(a, p_before).is_some());

        // 1. Mode shrink: 800x600. p_before (970.5, 550.5) is outside the new extent.
        c.output_reconfigured(
            &mut t,
            a,
            desc_k(key("A"), (800, 600), Transform::Normal, 120, 60_000),
        )
        .unwrap();
        assert_eq!(
            c.hit_test(a, p_before),
            None,
            "stale frame survived mode reconfiguration"
        );

        // Populate new frame for 800x600: window is at logical [350,450)x[250,350).
        let p_800 = crate::geom::OutputLogicalPoint { x: 410.5, y: 310.5 };
        assert!(c.hit_test(a, p_800).is_some());

        // 2. Transform change: R90 on 1920x1080 -> view_size (1080, 1920).
        c.output_reconfigured(
            &mut t,
            a,
            desc_k(key("A"), (1920, 1080), Transform::R90, 120, 60_000),
        )
        .unwrap();
        // On R90 the view is 1080x1920 and the window is at [490,590)x[910,1010). p_800 (410.5, 310.5) misses:
        assert_eq!(
            c.hit_test(a, p_800),
            None,
            "stale frame survived transform reconfiguration"
        );

        // 3. Scale change: scale 2.0 (scale_120 = 240) on 1920x1080 Normal -> logical extent 960x540.
        c.output_reconfigured(
            &mut t,
            a,
            desc_k(key("A"), (1920, 1080), Transform::Normal, 240, 60_000),
        )
        .unwrap();
        assert_eq!(
            c.hit_test(a, p_before),
            None,
            "stale frame survived scale reconfiguration"
        );
    }

    /// An invalid descriptor creates nothing and allocates no id.
    #[test]
    fn invalid_descriptor_is_rejected_without_mutation() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        assert_eq!(
            c.output_connected(
                &mut t,
                desc_k(key("A"), (0, 1080), Transform::Normal, 120, 60_000)
            )
            .err(),
            Some(EntryError::InvalidDescriptor)
        );
        assert!(c.cameras().is_empty() && c.outputs().is_empty());
        assert_eq!(c.pointer(), PointerState::NoOutput);
        let (o, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(
            o,
            OutputId::for_test(1),
            "the rejected connect allocated an id"
        );
    }

    // ---- INC-10: the output's lattice reference R_O (F-LATTICE-1 as amended by C12.F14)

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
        assert!(zoomed > 1.0, "zoom {zoomed} should be > 1.0 after scroll up");

        // 3. Super+Shift+Middle Click resets zoom to 1.0
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
        let reset = c.cameras().zoom(cam).unwrap();
        assert_eq!(reset, 1.0, "zoom reset should return to 1.0");
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

    #[test]
    fn focus_cycle_navigate_recenters_camera() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_cycle_navigate(true);

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

        c.set_focus(&mut i, Some(w1)).unwrap();
        assert_eq!(c.focus().target(), Some(w1));

        let logo = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_TAB,
                    pressed: true,
                    modifiers: logo,
                    mods_changed: false,
                    time_us: 1000,
                },
            )
            .unwrap();

        assert_eq!(c.focus().target(), Some(w2), "focus should cycle to w2");
    }

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
        c.toplevel_request(w1, crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) }).unwrap();
        c.toplevel_committed(w1, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(a), Some(w1));
        assert_eq!(c.view(a).unwrap().only, Some(w1), "w1 is presented fullscreen");

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

        assert_eq!(c.focus().target(), Some(w2), "focus moves to w2 without modal lock");
        assert!(c.view(a).unwrap().only.is_none(), "view is now normal camera view showing w2");

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
        assert_eq!(c.view(a).unwrap().only, Some(w1), "w1 is presented fullscreen again");
    }

    #[test]
    fn maximize_pushes_overlapping_windows_into_cluster_slot() {
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
        c.place_by_shell(w1, crate::geom::CanvasPoint { x: 0, y: 0 }, &mut out).unwrap();
        c.place_by_shell(w2, crate::geom::CanvasPoint { x: 812, y: 0 }, &mut out).unwrap();

        // Focus w1
        c.set_focus(&mut i, Some(w1)).unwrap();
        c.set_enter_mode(crate::EnterMode::Maximize);

        // Move pointer to (10, 10) on output A
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerMotion {
                delta: (10.0, 10.0),
                delta_unaccel: (10.0, 10.0),
                time_us: 1000,
            },
        ).unwrap();

        let logo = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };

        // Press Super+Enter on w1
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::KeyboardKey {
                key: crate::input::keys::KEY_ENTER,
                pressed: true,
                modifiers: logo,
                mods_changed: false,
                time_us: 2000,
            },
        ).unwrap();

        // w1 expands to fill viewport (-960..960, -540..540)
        // w2 was at (812, 0) which would be covered by w1's right edge at x=960!
        // push_overlapping_cluster should displace w2 to (960 + 12 = 972, 0)
        let anchor_w1 = c.stage().anchor(w1).unwrap();
        let anchor_w2 = c.stage().anchor(w2).unwrap();
        println!("anchor_w1: {anchor_w1:?}, anchor_w2: {anchor_w2:?}");
        assert_eq!(anchor_w1.x, -960);
        assert_eq!(anchor_w1.y, -540);
        assert_eq!(anchor_w2.x, 972, "w2 should be displaced to x=972 flush with 12px gap from w1's right edge (+960)");
        assert_eq!(anchor_w2.y, 0);
    }

    #[test]
    fn fullscreen_pushes_overlapping_windows_into_cluster_slot() {
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
        c.place_by_shell(w1, crate::geom::CanvasPoint { x: 0, y: 0 }, &mut out).unwrap();
        c.place_by_shell(w2, crate::geom::CanvasPoint { x: 812, y: 0 }, &mut out).unwrap();

        // Focus w1
        c.set_focus(&mut i, Some(w1)).unwrap();

        // Move pointer to (10, 10) on output A
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerMotion {
                delta: (10.0, 10.0),
                delta_unaccel: (10.0, 10.0),
                time_us: 1000,
            },
        ).unwrap();

        let logo_shift = Modifiers {
            logo: true,
            shift: true,
            ctrl: false,
            alt: false,
        };

        // Press Super+Shift+Enter on w1 to toggle fullscreen
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::KeyboardKey {
                key: crate::input::keys::KEY_ENTER,
                pressed: true,
                modifiers: logo_shift,
                mods_changed: false,
                time_us: 2000,
            },
        ).unwrap();

        // w1 expands to fill viewport (-960..960, -540..540)
        // w2 was at (812, 0), so smart cluster tiling pushes w2 to (960 + 12 = 972, 0)
        let anchor_w1 = c.stage().anchor(w1).unwrap();
        let anchor_w2 = c.stage().anchor(w2).unwrap();
        assert_eq!(anchor_w1.x, -960);
        assert_eq!(anchor_w1.y, -540);
        assert_eq!(anchor_w2.x, 972, "w2 must be displaced to x=972 to avoid being covered by fullscreen w1");
        assert_eq!(anchor_w2.y, 0);

        // Release Enter key
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::KeyboardKey {
                key: crate::input::keys::KEY_ENTER,
                pressed: false,
                modifiers: logo_shift,
                mods_changed: false,
                time_us: 2500,
            },
        ).unwrap();

        // Toggle fullscreen off (Super+Shift+Enter again)
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::KeyboardKey {
                key: crate::input::keys::KEY_ENTER,
                pressed: true,
                modifiers: logo_shift,
                mods_changed: false,
                time_us: 3000,
            },
        ).unwrap();

        // w1 restores to original floating anchor (0, 0)
        let restored_w1 = c.stage().anchor(w1).unwrap();
        assert_eq!(restored_w1.x, 0);
        assert_eq!(restored_w1.y, 0);
        // w2 remains at (972, 0)
        assert_eq!(c.stage().anchor(w2).unwrap().x, 972);
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
                size: if mapped { LocalSize { w: 1920, h: 1080 } } else { LocalSize::default() },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        c.toplevel_committed(w1, win(false)).unwrap();
        c.toplevel_committed(w1, win(true)).unwrap();
        c.set_focus(&mut i, Some(w1)).unwrap();
        c.toplevel_request(w1, crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) }).unwrap();
        c.toplevel_committed(w1, win(true)).unwrap();

        assert_eq!(c.presentation(a), Some(w1));
        assert_eq!(c.view(a).unwrap().only, Some(w1), "initially presenting fullscreen");

        let logo = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };

        // Press Super + BTN_MIDDLE
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerButton {
                button: crate::input::bindings::BTN_MIDDLE,
                pressed: true,
                modifiers: logo,
                time_us: 1000,
            },
        ).unwrap();

        // Drag mouse by delta (200.0, 100.0)
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerMotion {
                delta: (200.0, 100.0),
                delta_unaccel: (200.0, 100.0),
                time_us: 2000,
            },
        ).unwrap();

        // View has switched out of exclusive presentation to camera view!
        assert!(c.view(a).unwrap().only.is_none(), "view transitioned to camera view allowing user to pan out of fullscreen");

        // Release BTN_MIDDLE
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerButton {
                button: crate::input::bindings::BTN_MIDDLE,
                pressed: false,
                modifiers: logo,
                time_us: 3000,
            },
        ).unwrap();

        assert!(c.view(a).unwrap().only.is_none(), "still in camera view after pan completes");

        // Pressing Super+1 focuses w1 and returns to fullscreen presentation
        let _ = c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::KeyboardKey {
                key: crate::input::keys::KEY_1,
                pressed: true,
                modifiers: logo,
                mods_changed: false,
                time_us: 4000,
            },
        ).unwrap();

        assert_eq!(c.view(a).unwrap().only, Some(w1), "Super+1 restores exclusive fullscreen presentation");
    }

    #[test]
    fn recenter_element_centers_camera_on_target_window() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();

        let w1 = c.toplevel_created();
        let win = |mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w: 800, h: 600 },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(1).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped { LocalSize { w: 800, h: 600 } } else { LocalSize::default() },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        c.toplevel_committed(w1, win(false)).unwrap();
        c.toplevel_committed(w1, win(true)).unwrap();

        // Place w1 at an offset anchor
        let anchor = crate::geom::CanvasPoint { x: 5000, y: 3000 };
        let mut fx = Vec::new();
        c.place_by_shell(w1, anchor, &mut fx).unwrap();

        // Recenter element w1
        let mut buf = crate::protocol::EffectBuf::new();
        c.recenter_element(&mut i, w1, &mut buf).unwrap();

        // Camera must now be centered on w1's window center
        let camera = c.cameras.camera_for_output(a).unwrap();
        let center = c.cameras.center(camera).unwrap();
        let cx = center.x / crate::geom::Q16_ONE;
        let cy = center.y / crate::geom::Q16_ONE;
        assert_eq!(cx, 5400);
        assert_eq!(cy, 3300);
    }
}
