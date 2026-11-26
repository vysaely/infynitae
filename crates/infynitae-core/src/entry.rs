//! Port-carrying entry points (C1.5, INV-CAMERA-1, INV-OUTPUT-2). The only module in which an
//! `Authorized` proof can be minted.
//!
//! Each entry point first checks that its port or token belongs to this Core:
//! * the three output-lifecycle entries implement M4/M5 for any number of outputs (startup and
//!   hotplug alike, reconfiguration, disconnection, the zero-output state) and the pointer's M10
//!   transitions;
//! * physical input intake handles pointer motion, buttons, axis scrolling, and keyboard events
//!   (C4.2: ownership, M22 CameraPan, routing, KeepFocus);
//! * shell command execution validates and executes semantic commands (C7.2, INV-SHELL-1).

use crate::input::bindings;
use crate::input::interaction::Interaction;
use crate::input::keys::{self, KeyAction, KeyCapture};
use crate::input::ownership::Owner;
use crate::input::{GestureKind, TouchpadGesture};
use crate::output::{self, OutputDescriptor, OutputId, OutputModel};
use crate::pointer::{self, PointerState};
use crate::protocol::Effect;
use crate::{
    Core, EntryError, InputPort, NavigateTo, OutputLifecycleToken, PhysicalEvent, ShellCommand,
    ShellError, ShellPort, ShellStream,
};

mod focus;
mod gestures;
mod keyboard;
mod outputs;
mod pointer_input;
pub(crate) mod priority;
mod shell;
mod touch;
mod windows;

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
                // the hand is on the mouse again: keyed window actions follow the pointer's rule
                self.keyed_focus = None;
                self.key_opened = false;
                self.pointer_motion(proof, delta, delta_unaccel, time_us, out);
                self.physical_input_activity = true;
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
                    self.closed_windows.clear();
                    self.opening = None;
                    self.button_press(button, modifiers, ms(time_us), proof, out);
                } else {
                    self.button_release(button, ms(time_us), out);
                }
                self.physical_input_activity = true;
                Ok(())
            }
            PhysicalEvent::PointerAxis { axis, time_us } => {
                let finite = [axis.amount.0, axis.amount.1].iter().all(|v| v.is_finite());
                if !finite {
                    return Err(EntryError::InvalidEvent);
                }
                self.input.clock.accepted(time_us);
                self.physical_input_activity = true;
                // consumed during an interaction and in NoOutput (M10, M22)
                if self.input.interaction.active() || self.pointer == PointerState::NoOutput {
                    return Ok(());
                }
                let mods = self.input.modifiers;
                if mods.logo && mods.shift && !mods.ctrl && !mods.alt {
                    let proof = authority::Authorized::mint();
                    self.zoom_axis(proof, axis, out);
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
                self.focus.settle_hover(time_us);
                // a held Super frees the cursor from a window, but not from a game in fullscreen
                // in front: there is nowhere to move it to, and a stray press threw the aim
                // (owner, 2026-11-05). Super+Tab out still frees it, the game stepping aside.
                let in_fullscreen = self.pointer_output().is_some_and(|o| {
                    self.presentation(o).is_some() && !self.is_presentation_suppressed(o)
                });
                out.extend(self.suspend_constraints(
                    modifiers.logo && self.free_cursor_on_super && !in_fullscreen,
                ));
                if pressed {
                    self.closed_windows.clear();
                    self.opening = None;
                    // a press bound to fit or restore mutates a camera: its proof is minted here
                    let navigation = (!self.is_capturing_key()
                        && matches!(
                            self.resolve_key_binding(
                                key,
                                modifiers,
                                self.input.interaction.active()
                            ),
                            Some(
                                KeyAction::FitViewport
                                    | KeyAction::ViewportRestore
                                    | KeyAction::FitToggle
                                    | KeyAction::Recenter
                                    | KeyAction::RecenterKeepZoom
                                    | KeyAction::FocusSlot(_)
                                    | KeyAction::FocusClusterSlot(_)
                                    | KeyAction::ArrangeCluster
                                    | KeyAction::ArrangeClusterByApp
                                    | KeyAction::FocusNext
                                    | KeyAction::FocusDirection(_)
                                    | KeyAction::ToggleOverview
                            )
                        ))
                    .then(authority::Authorized::mint);
                    self.keyboard_press(key, modifiers, mods_changed, time_us, navigation, out);
                    // a key the window got, not a modifier on its way to a shortcut
                    if self.input.key_ownership.owner(key) == Some(Owner::Client)
                        && !keys::is_modifier_key(key)
                        && self.focus.layer().is_none()
                        && let Some(w) = self.focus.target()
                    {
                        self.note_used(w);
                    }
                } else {
                    self.keyboard_release(key, mods_changed, time_us, out);
                }
                self.physical_input_activity = true;
                Ok(())
            }
            PhysicalEvent::RecenterElement { element } => {
                let proof = authority::Authorized::mint();
                self.recenter_on_element(proof, element, false, false, out);
                Ok(())
            }
            PhysicalEvent::BringIntoView { element } => {
                self.bring_into_view_now(element);
                Ok(())
            }
            PhysicalEvent::TouchDown {
                slot,
                output,
                p,
                time_us,
            } => {
                if !(p.0.is_finite() && p.1.is_finite()) {
                    return Err(EntryError::InvalidEvent);
                }
                self.input.clock.accepted(time_us);
                self.closed_windows.clear();
                self.opening = None;
                self.arm_edge_swipe(slot, output, p);
                self.touch_down(slot, output, p, ms(time_us), out);
                self.touch_resize_settle(out);
                self.physical_input_activity = true;
                Ok(())
            }
            PhysicalEvent::TouchMotion { slot, p, time_us } => {
                if !(p.0.is_finite() && p.1.is_finite()) {
                    return Err(EntryError::InvalidEvent);
                }
                self.input.clock.accepted(time_us);
                if self.step_edge_swipe(slot, p, out) {
                    self.physical_input_activity = true;
                    return Ok(());
                }
                if self.touches.on_window(slot) {
                    self.window_gesture_step(slot, p, out);
                    self.physical_input_activity = true;
                    return Ok(());
                }
                // a finger on the canvas moves the camera (INV-CAMERA-1)
                let proof = self
                    .touches
                    .on_canvas(slot)
                    .then(authority::Authorized::mint);
                self.touch_motion(proof, slot, p, ms(time_us), out);
                self.physical_input_activity = true;
                Ok(())
            }
            PhysicalEvent::TouchUp { slot, time_us } => {
                self.input.clock.accepted(time_us);
                if self.touches.edge_swipe.is_some_and(|e| e.slot == slot) {
                    self.touches.edge_swipe = None;
                }
                let crate::touch::Lift {
                    on_surface,
                    double_tap,
                } = self.touches.lift(slot, Some(ms(time_us)));
                if on_surface {
                    out.push(Effect::TouchUp {
                        slot,
                        time_ms: ms(time_us),
                    });
                }
                // a two-finger double tap recenters like Super+Space (INV-CAMERA-1)
                if let Some((output, at)) = double_tap {
                    self.touch_recenter(authority::Authorized::mint(), output, at, out);
                }
                self.touch_resize_settle(out);
                self.physical_input_activity = true;
                Ok(())
            }
            PhysicalEvent::TouchFrame => {
                out.push(Effect::TouchFrame);
                Ok(())
            }
            PhysicalEvent::TouchCancel => {
                if self.touches.clear() {
                    out.push(Effect::TouchCancel);
                }
                self.touch_resize_settle(out);
                Ok(())
            }
            PhysicalEvent::TabletTap { output, p, time_us } => {
                if !(p.0.is_finite() && p.1.is_finite()) {
                    return Err(EntryError::InvalidEvent);
                }
                self.input.clock.accepted(time_us);
                self.closed_windows.clear();
                self.opening = None;
                if self.output(output).is_some() && !self.input.interaction.active() {
                    self.tap_focus(output, p, out);
                    out.extend(self.reconcile_constraints());
                }
                self.physical_input_activity = true;
                Ok(())
            }
            PhysicalEvent::JumpToWindow { element } => {
                self.jump_to_window(element, Some(authority::Authorized::mint()), out);
                Ok(())
            }
            PhysicalEvent::GestureSwipeBegin { fingers, time_us } => {
                self.input.clock.accepted(time_us);
                self.physical_input_activity = true;
                let proof = self.touchpad_gestures.then(authority::Authorized::mint);
                self.gesture_swipe_begin(fingers, ms(time_us), proof, out);
                Ok(())
            }
            PhysicalEvent::GestureSwipeUpdate { delta, time_us } => {
                if !(delta.0.is_finite() && delta.1.is_finite()) {
                    return Err(EntryError::InvalidEvent);
                }
                self.input.clock.accepted(time_us);
                self.physical_input_activity = true;
                let proof = matches!(
                    self.input.gesture,
                    Some(TouchpadGesture::Pan | TouchpadGesture::FourFingerSwipe { .. })
                )
                .then(authority::Authorized::mint);
                self.gesture_swipe_update(delta, ms(time_us), proof, out);
                Ok(())
            }
            PhysicalEvent::GestureSwipeEnd { cancelled, time_us } => {
                self.input.clock.accepted(time_us);
                self.physical_input_activity = true;
                self.gesture_swipe_end(cancelled, ms(time_us), out);
                Ok(())
            }
            PhysicalEvent::GesturePinchBegin { fingers, time_us } => {
                self.input.clock.accepted(time_us);
                self.physical_input_activity = true;
                self.gesture_pinch_begin(fingers, ms(time_us), out);
                Ok(())
            }
            PhysicalEvent::GesturePinchUpdate {
                delta,
                scale,
                rotation,
                time_us,
            } => {
                let finite = [delta.0, delta.1, scale, rotation]
                    .iter()
                    .all(|v| v.is_finite());
                if !finite {
                    return Err(EntryError::InvalidEvent);
                }
                self.input.clock.accepted(time_us);
                self.physical_input_activity = true;
                let proof = matches!(self.input.gesture, Some(TouchpadGesture::Pinch { .. }))
                    .then(authority::Authorized::mint);
                self.gesture_pinch_update(delta, scale, rotation, ms(time_us), proof, out);
                Ok(())
            }
            PhysicalEvent::GesturePinchEnd { cancelled, time_us } => {
                self.input.clock.accepted(time_us);
                self.physical_input_activity = true;
                self.gesture_pinch_end(cancelled, ms(time_us), out);
                Ok(())
            }
            PhysicalEvent::GestureHoldBegin { fingers, time_us } => {
                self.input.clock.accepted(time_us);
                self.physical_input_activity = true;
                self.gesture_hold_begin(fingers, ms(time_us), out);
                Ok(())
            }
            PhysicalEvent::GestureHoldEnd { cancelled, time_us } => {
                self.input.clock.accepted(time_us);
                self.physical_input_activity = true;
                self.gesture_hold_end(cancelled, ms(time_us), out);
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

    /// Advances any active camera zoom glide on `output` to `now`, emitting consequences when the
    /// camera changes. Called once per output refresh by backends.
    pub fn zoom_tick(&mut self, output: OutputId, now: std::time::Duration) -> Vec<Effect> {
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return Vec::new();
        };
        let refresh_mhz = self
            .output(output)
            .map_or(60_000, |m| m.refresh_mhz)
            .max(1_000);
        let frame_period = std::time::Duration::from_secs_f64(1000.0 / f64::from(refresh_mhz));
        let mut out = Vec::new();
        if self.cameras.tick_glide(camera, now, frame_period) {
            self.camera_written(output, camera, &mut out);
        }
        out
    }

    /// After a fit or restore of `camera` on `output`: the lattice reference follows (a zoom change
    /// sets it again, F-LATTICE-1), the frame is stale, membership and the scale policy are
    /// evaluated once for the camera (an atomic camera action: a one-step epoch, INV-SCALE-3), and
    /// the pointer is reconciled by M10's view-change rule — one true-position event when it is on
    /// O and no interaction owns it (the bindings never resolve during one), none otherwise. A lock
    /// kept by the reconciliation gets none (no absolute motion reaches a locked surface,
    /// INV-INPUT-5); a kept confinement gets the event like no constraint, so the confined client
    /// sees where the moved surface now lies under the pointer.
    pub(crate) fn camera_written(
        &mut self,
        output: OutputId,
        camera: usize,
        out: &mut Vec<Effect>,
    ) {
        self.update_lattice(output);
        self.invalidate_frame(output);
        self.on_top_follow(camera, out);
        self.visibility_pass_camera(camera, out);
        let on_it = matches!(self.pointer, PointerState::Located { output: o, .. } if o == output);
        if on_it {
            // what is under the pointer now is not a crossing: the camera moved, not the hand. Left
            // stale, the next 1 px nudge moved the focus to it (AUDIT_FOCUS_HOVER finding 5)
            self.hover_seen = self.hovered_window();
        }
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
}

/// `wl_pointer` timestamps: milliseconds with wrap-around.
fn ms(time_us: u64) -> u32 {
    crate::input::wire_ms(time_us)
}

#[cfg(test)]
mod tests {
    //! E1R output-lifecycle tests. Expected values are hand-derived literals, not values computed by
    //! the code under test.

    use crate::Core;
    use crate::geom::CanvasPos;
    use crate::input::bindings::Modifiers;
    use crate::output::tests::key;
    use crate::output::{OutputDescriptor, OutputId, OutputKey, Transform};
    use crate::pointer::PointerState;
    use crate::protocol::Effect;

    pub(super) fn desc_k(
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

    pub(super) fn desc(connector: &str) -> OutputDescriptor {
        desc_k(key(connector), (1920, 1080), Transform::Normal, 120, 60_000)
    }

    pub(super) fn pos(x: i64, y: i64) -> CanvasPos {
        CanvasPos {
            x: x << 16,
            y: y << 16,
        }
    }

    pub(super) fn camera_of(c: &Core, o: OutputId) -> usize {
        c.cameras().camera_for_output(o).expect("bound")
    }

    // ---- The output's lattice reference R_O (F-LATTICE-1 as amended by C12.F14)

    /// Maps `n` 400x300 windows on `c` (output `a` connected) and returns them; `press` below
    /// is a key press or release with the modifiers the seat would report after it.
    pub(super) fn map_windows(c: &mut Core, a: OutputId, n: u64) -> Vec<crate::stage::ElementId> {
        let ws = (1..=n).map(|k| map_window(c, k)).collect();
        c.render_list(a).unwrap();
        ws
    }

    /// Maps one 400x300 window whose surface key is `k`.
    pub(super) fn map_window(c: &mut Core, k: u64) -> crate::stage::ElementId {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;
        let win = |k: u64, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w: 400, h: 300 },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(k).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w: 400, h: 300 }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        let w = c.toplevel_created();
        c.toplevel_committed(w, win(k, false)).unwrap();
        c.toplevel_committed(w, win(k, true)).unwrap();
        w
    }

    pub(super) fn press(
        c: &mut Core,
        i: &mut crate::InputPort,
        key: u32,
        pressed: bool,
        logo: bool,
    ) {
        let modifiers = Modifiers {
            logo,
            ..Modifiers::default()
        };
        let _ = c
            .physical_input_vec(
                i,
                crate::PhysicalEvent::KeyboardKey {
                    key,
                    pressed,
                    modifiers,
                    mods_changed: key == crate::input::keys::KEY_LEFTMETA,
                    time_us: 1000,
                },
            )
            .unwrap();
    }

    /// Super+Tab tapped (Super down, Tab, Super up), through the physical key path.
    pub(super) fn super_tab(c: &mut Core, i: &mut crate::InputPort) {
        use crate::input::keys::{KEY_LEFTMETA, KEY_TAB};
        press(c, i, KEY_LEFTMETA, true, true);
        press(c, i, KEY_TAB, true, true);
        press(c, i, KEY_TAB, false, true);
        press(c, i, KEY_LEFTMETA, false, false);
    }

    pub(super) fn press_overview(c: &mut Core, i: &mut crate::InputPort) {
        use crate::input::keys::{KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_TAB};
        let mods = |logo, shift| crate::input::bindings::Modifiers {
            logo,
            shift,
            ..Default::default()
        };
        for (key, pressed, logo, shift, changed) in [
            (KEY_LEFTMETA, true, true, false, true),
            (KEY_LEFTSHIFT, true, true, true, true),
            (KEY_TAB, true, true, true, false),
            (KEY_TAB, false, true, true, false),
            (KEY_LEFTSHIFT, false, true, false, true),
            (KEY_LEFTMETA, false, false, false, true),
        ] {
            c.physical_input_vec(
                i,
                crate::PhysicalEvent::KeyboardKey {
                    key,
                    pressed,
                    modifiers: mods(logo, shift),
                    mods_changed: changed,
                    time_us: 1000,
                },
            )
            .unwrap();
        }
    }

    /// A fullscreen window, focused, with another window on the canvas and the pointer on screen.
    pub(super) fn fullscreen_rig(
        c: &mut Core,
        i: &mut crate::InputPort,
        t: &mut crate::OutputLifecycleToken,
    ) -> (OutputId, Vec<crate::stage::ElementId>) {
        let (a, _) = c.output_connected(t, desc("A")).unwrap();
        let ws = map_windows(c, a, 2);
        c.set_focus(i, Some(ws[0])).unwrap();
        c.toplevel_request(
            ws[0],
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
        )
        .unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (1.0, 1.0),
        };
        (a, ws)
    }

    /// Two windows, the first focused and under the pointer in the middle of the screen.
    pub(super) fn window_under_pointer(
        c: &mut Core,
        i: &mut crate::InputPort,
        t: &mut crate::OutputLifecycleToken,
    ) -> Vec<crate::stage::ElementId> {
        let (a, _) = c.output_connected(t, desc("A")).unwrap();
        let ws = map_windows(c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[1],
            crate::geom::CanvasPoint { x: 5000, y: 5000 },
            &mut fx,
        )
        .unwrap();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: -100, y: -100 },
            &mut fx,
        )
        .unwrap();
        c.set_focus(i, Some(ws[0])).unwrap();
        let (w, h) = c.output(a).unwrap().logical_extent();
        c.pointer = PointerState::Located {
            output: a,
            p: (w / 2.0, h / 2.0),
        };
        c.hover_seen = c.hovered_window();
        assert_eq!(c.hover_seen, Some(ws[0]));
        ws
    }

    pub(super) fn mouse_button(
        c: &mut Core,
        i: &mut crate::InputPort,
        pressed: bool,
    ) -> Vec<Effect> {
        c.physical_input_vec(
            i,
            crate::PhysicalEvent::PointerButton {
                button: crate::input::bindings::BTN_LEFT,
                pressed,
                modifiers: Default::default(),
                time_us: 1000,
            },
        )
        .unwrap()
    }

    pub(super) fn mouse_move(c: &mut Core, i: &mut crate::InputPort, d: (f64, f64)) {
        c.physical_input_vec(
            i,
            crate::PhysicalEvent::PointerMotion {
                delta: d,
                delta_unaccel: d,
                time_us: 2000,
            },
        )
        .unwrap();
    }

    /// Keyboard only with strict focus (owner, 2026-11-22: "the last used device has the priority
    /// over the window"): three windows apart, the pointer still over empty canvas in the top left
    /// corner, ws[0] focused.
    pub(super) fn keyboard_only_strict(
        c: &mut Core,
        i: &mut crate::InputPort,
        t: &mut crate::OutputLifecycleToken,
    ) -> (OutputId, Vec<crate::stage::ElementId>) {
        let (a, _) = c.output_connected(t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        c.set_focus_follows_mouse_strict(true);
        let ws = map_windows(c, a, 3);
        let mut fx = Vec::new();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 6000, y: 0 }, &mut fx)
            .unwrap();
        c.place_by_user(ws[2], crate::geom::CanvasPoint { x: -6000, y: 0 }, &mut fx)
            .unwrap();
        c.place_by_user(ws[0], crate::geom::CanvasPoint { x: 0, y: 0 }, &mut fx)
            .unwrap();
        c.set_focus(i, Some(ws[0])).unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (10.0, 10.0),
        };
        c.render_list(a).unwrap();
        c.hover_seen = c.hovered_window();
        assert_eq!(c.hover_seen, None, "the pointer is over empty canvas");
        (a, ws)
    }

    /// Super+`key` (with Shift if `shift`), through the physical key path; the frame is drawn
    /// after it, as the next output frame would.
    pub(super) fn super_chord(
        c: &mut Core,
        i: &mut crate::InputPort,
        a: OutputId,
        key: u32,
        shift: bool,
    ) -> Vec<Effect> {
        use crate::input::keys::{KEY_LEFTMETA, KEY_LEFTSHIFT};
        let mut ev = |c: &mut Core, key: u32, pressed: bool, logo: bool, shift: bool| {
            c.physical_input_vec(
                i,
                crate::PhysicalEvent::KeyboardKey {
                    key,
                    pressed,
                    modifiers: Modifiers {
                        logo,
                        shift,
                        ..Modifiers::default()
                    },
                    mods_changed: key == KEY_LEFTMETA || key == KEY_LEFTSHIFT,
                    time_us: 1000,
                },
            )
            .unwrap()
        };
        ev(c, KEY_LEFTMETA, true, true, false);
        if shift {
            ev(c, KEY_LEFTSHIFT, true, true, true);
        }
        let fx = ev(c, key, true, true, shift);
        ev(c, key, false, true, shift);
        if shift {
            ev(c, KEY_LEFTSHIFT, false, true, false);
        }
        ev(c, KEY_LEFTMETA, false, false, false);
        c.render_list(a).unwrap();
        fx
    }
}
