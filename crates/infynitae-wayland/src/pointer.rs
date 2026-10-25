//! The seat pointer (INC-3): Core's pointer effects applied to Smithay's `PointerHandle`, in order.
//!
//! This is the only module that drives Smithay pointer delivery (source rule
//! `smithay-pointer-only-in-adapter-pointer`). The adapter decides nothing here: routing, ownership,
//! KeepFocus and the zero-output transition are Core's (C4.2, M10). Smithay's global pointer space
//! is Canvas (INV-COORD-5): focus origins and locations come from Core as Canvas values and pass
//! through `convert` unchanged.

use infynitae_core::input::{Axis, AxisSource};
use infynitae_core::protocol::{Effect, PointerTarget};
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
    GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
    GestureSwipeUpdateEvent, GrabStartData, MotionEvent, PointerGrab, PointerHandle,
    PointerInnerHandle, RelativeMotionEvent,
};
use smithay::input::{Seat, SeatHandler};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::pointer_constraints::with_pointer_constraint;

use crate::{WaylandState, convert};

type Focus = Option<(
    <WaylandState as SeatHandler>::PointerFocus,
    convert::GlobalPoint,
)>;
type Inner<'a> = PointerInnerHandle<'a, WaylandState>;

/// What the focused client last received. Core sends the true position again after changes that
/// may move what lies under the pointer (M10), among them every commit of a fullscreen window;
/// when focus, origin and position are all unchanged the motion would tell the client nothing,
/// and a player that hides the cursor after a still moment (a fullscreen video) would take it
/// for movement 60 times a second. Such a motion, and a frame with no event before it, are not
/// sent.
#[derive(Default)]
pub(crate) struct Sent {
    motion: Option<(Focus, convert::GlobalPoint)>,
    frame_pending: bool,
}

/// The adapter's form of `PointerState::NoOutput` at the seat (EE-3, E1R): a compositor grab that
/// delivers nothing. Installed with `Focus::Clear` (one `wl_pointer.leave` through Smithay's
/// internal path, no client motion); removed before the first placement after a connect.
struct ZeroOutputGrab {
    start: GrabStartData<WaylandState>,
}

impl std::fmt::Debug for ZeroOutputGrab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ZeroOutputGrab")
    }
}

impl PointerGrab<WaylandState> for ZeroOutputGrab {
    fn motion(&mut self, _: &mut WaylandState, _: &mut Inner<'_>, _: Focus, _: &MotionEvent) {}
    fn relative_motion(
        &mut self,
        _: &mut WaylandState,
        _: &mut Inner<'_>,
        _: Focus,
        _: &RelativeMotionEvent,
    ) {
    }
    fn button(&mut self, _: &mut WaylandState, _: &mut Inner<'_>, _: &ButtonEvent) {}
    fn axis(&mut self, _: &mut WaylandState, _: &mut Inner<'_>, _: AxisFrame) {}
    fn frame(&mut self, _: &mut WaylandState, _: &mut Inner<'_>) {}
    fn gesture_swipe_begin(
        &mut self,
        _: &mut WaylandState,
        _: &mut Inner<'_>,
        _: &GestureSwipeBeginEvent,
    ) {
    }
    fn gesture_swipe_update(
        &mut self,
        _: &mut WaylandState,
        _: &mut Inner<'_>,
        _: &GestureSwipeUpdateEvent,
    ) {
    }
    fn gesture_swipe_end(
        &mut self,
        _: &mut WaylandState,
        _: &mut Inner<'_>,
        _: &GestureSwipeEndEvent,
    ) {
    }
    fn gesture_pinch_begin(
        &mut self,
        _: &mut WaylandState,
        _: &mut Inner<'_>,
        _: &GesturePinchBeginEvent,
    ) {
    }
    fn gesture_pinch_update(
        &mut self,
        _: &mut WaylandState,
        _: &mut Inner<'_>,
        _: &GesturePinchUpdateEvent,
    ) {
    }
    fn gesture_pinch_end(
        &mut self,
        _: &mut WaylandState,
        _: &mut Inner<'_>,
        _: &GesturePinchEndEvent,
    ) {
    }
    fn gesture_hold_begin(
        &mut self,
        _: &mut WaylandState,
        _: &mut Inner<'_>,
        _: &GestureHoldBeginEvent,
    ) {
    }
    fn gesture_hold_end(
        &mut self,
        _: &mut WaylandState,
        _: &mut Inner<'_>,
        _: &GestureHoldEndEvent,
    ) {
    }
    fn start_data(&self) -> &GrabStartData<WaylandState> {
        &self.start
    }
    fn unset(&mut self, _: &mut WaylandState) {}
}

/// A pointer grab installed when an active popup grab is dismissed by CameraPan acquisition.
/// While active during the pan, all pointer motion is consumed by the camera pan interaction.
/// At pan end, upon receiving the reconciled true-position motion or button event,
/// it unsets the grab with `restore_focus: false` (preventing Smithay synthetic motion)
/// and forwards the event to the client.
struct PanDismissedPopupGrab {
    start: GrabStartData<WaylandState>,
}

impl std::fmt::Debug for PanDismissedPopupGrab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PanDismissedPopupGrab")
    }
}

impl PointerGrab<WaylandState> for PanDismissedPopupGrab {
    fn motion(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        focus: Focus,
        event: &MotionEvent,
    ) {
        handle.unset_grab(self, data, event.serial, event.time, false);
        handle.motion(data, focus, event);
    }

    fn relative_motion(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        focus: Focus,
        event: &RelativeMotionEvent,
    ) {
        handle.unset_grab(self, data, SERIAL_COUNTER.next_serial(), 0, false);
        handle.relative_motion(data, focus, event);
    }

    fn button(&mut self, data: &mut WaylandState, handle: &mut Inner<'_>, event: &ButtonEvent) {
        handle.unset_grab(self, data, event.serial, event.time, false);
        handle.button(data, event);
    }

    fn axis(&mut self, data: &mut WaylandState, handle: &mut Inner<'_>, details: AxisFrame) {
        handle.unset_grab(self, data, SERIAL_COUNTER.next_serial(), 0, false);
        handle.axis(data, details);
    }

    fn frame(&mut self, data: &mut WaylandState, handle: &mut Inner<'_>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.unset_grab(self, data, event.serial, event.time, false);
        handle.gesture_swipe_begin(data, event);
    }
    fn gesture_swipe_update(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event);
    }
    fn gesture_swipe_end(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event);
    }
    fn gesture_pinch_begin(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.unset_grab(self, data, event.serial, event.time, false);
        handle.gesture_pinch_begin(data, event);
    }
    fn gesture_pinch_update(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event);
    }
    fn gesture_pinch_end(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event);
    }
    fn gesture_hold_begin(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.unset_grab(self, data, event.serial, event.time, false);
        handle.gesture_hold_begin(data, event);
    }
    fn gesture_hold_end(
        &mut self,
        data: &mut WaylandState,
        handle: &mut Inner<'_>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event);
    }

    fn start_data(&self) -> &GrabStartData<WaylandState> {
        &self.start
    }

    fn unset(&mut self, _: &mut WaylandState) {}
}

impl WaylandState {
    /// The seat's pointer, created with the seat.
    pub(crate) fn pointer_handle(&self) -> PointerHandle<WaylandState> {
        self.seat.get_pointer().expect("the seat has a pointer")
    }

    /// The Smithay focus of a Core target: the live surface and its Canvas origin. A target whose
    /// surface is no longer registered resolves to no focus (fail closed).
    fn focus(&self, target: Option<PointerTarget>) -> Focus {
        let t = target?;
        let surface: WlSurface = self.registry.surface(t.surface)?.clone();
        Some((surface, convert::canvas_point_to_global(t.origin)))
    }

    /// Applies one pointer effect. Returns `false` for an effect that is not a pointer effect.
    pub(crate) fn apply_pointer(&mut self, effect: &Effect) -> bool {
        let ptr = self.pointer_handle();
        match *effect {
            Effect::PointerMotion {
                target,
                location,
                time_ms,
            } => {
                let focus = self.focus(target);
                let location = convert::canvas_to_global(location);
                // a grab may deliver the motion elsewhere or not at all: always passed on, and
                // what the client got is then unknown
                let grabbed = ptr.is_grabbed();
                let unchanged = !grabbed
                    && ptr.current_location() == location
                    && ptr.current_focus() == focus.as_ref().map(|(s, _)| s.clone())
                    && self.pointer_sent.motion.as_ref() == Some(&(focus.clone(), location));
                if unchanged {
                    return true;
                }
                let event = MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                };
                self.pointer_sent.motion = (!grabbed).then(|| (focus.clone(), location));
                self.pointer_sent.frame_pending = true;
                ptr.motion(self, focus, &event);
            }
            Effect::PointerRelative {
                target,
                delta,
                delta_unaccel,
                time_us,
            } => {
                let focus = self.focus(target);
                let event = RelativeMotionEvent {
                    delta: convert::global_delta(delta),
                    delta_unaccel: convert::global_delta(delta_unaccel),
                    utime: time_us,
                };
                self.pointer_sent.frame_pending = true;
                ptr.relative_motion(self, focus, &event);
            }
            Effect::PointerButton {
                button,
                pressed,
                time_ms,
            } => {
                let state = if pressed {
                    smithay::backend::input::ButtonState::Pressed
                } else {
                    smithay::backend::input::ButtonState::Released
                };
                let serial = SERIAL_COUNTER.next_serial();
                if let Some(out) = self.core.pointer_output() {
                    self.record_serial_output(serial, out);
                }
                let event = ButtonEvent {
                    serial,
                    time: time_ms,
                    button,
                    state,
                };
                self.pointer_sent.frame_pending = true;
                ptr.button(self, &event);
            }
            Effect::PointerAxis { axis, time_ms } => {
                self.pointer_sent.frame_pending = true;
                ptr.axis(self, axis_frame(axis, time_ms));
            }
            Effect::PointerFrame => {
                if std::mem::take(&mut self.pointer_sent.frame_pending) {
                    ptr.frame(self);
                }
            }
            Effect::PointerZeroOutputBegin => {
                let popup_target = self.popup_grab.as_ref().and_then(|g| {
                    if !g.has_ended() {
                        g.current_grab()
                    } else {
                        None
                    }
                });
                let location = ptr.current_location();
                let start = GrabStartData {
                    focus: None,
                    button: 0,
                    location,
                };
                ptr.set_grab(
                    self,
                    ZeroOutputGrab { start },
                    SERIAL_COUNTER.next_serial(),
                    smithay::input::pointer::Focus::Clear,
                );
                // pending focus None, so removing the grab later restores nothing
                let event = MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time: 0,
                };
                ptr.motion(self, None, &event);
                ptr.frame(self);
                self.pointer_sent = Sent::default();
                if let Some(target) = popup_target
                    && !self.is_locked()
                {
                    let kb = self.keyboard_handle();
                    kb.set_focus(self, Some(target), SERIAL_COUNTER.next_serial());
                }
            }
            Effect::PointerZeroOutputEnd => {
                ptr.unset_grab(self, SERIAL_COUNTER.next_serial(), 0);
                self.pointer_sent = Sent::default();
                self.popups.cleanup();
                let grab_opt = self.popup_grab.clone();
                let is_alive = grab_opt.as_ref().is_some_and(|g| {
                    if g.has_ended() {
                        return false;
                    }
                    if let Some(top) = g.current_grab() {
                        top.is_alive() && self.popups.find_popup(&top).is_some()
                    } else {
                        false
                    }
                });
                if is_alive && let Some(g) = grab_opt {
                    let serial = g.serial();
                    self.set_popup_pointer_grab(&g, serial);
                    let kb = self.keyboard_handle();
                    let grab = smithay::desktop::PopupKeyboardGrab::new(&g);
                    self.set_keyboard_grab(&kb, grab, serial);
                } else {
                    self.popup_grab = None;
                    self.popup_pointer_grab_serial = None;
                }
            }
            Effect::ConstraintActivate { surface, .. } => {
                let ptr = self.pointer_handle();
                if let Some(s) = self.registry.surface(surface) {
                    with_pointer_constraint(s, &ptr, |c| {
                        if let Some(c) = c
                            && !c.is_active()
                        {
                            c.activate();
                        }
                    });
                }
            }
            Effect::ConstraintDeactivate { surface, .. } => {
                let ptr = self.pointer_handle();
                if let Some(s) = self.registry.surface(surface) {
                    with_pointer_constraint(s, &ptr, |c| {
                        if let Some(c) = c
                            && c.is_active()
                        {
                            c.deactivate();
                        }
                    });
                    let still_present = with_pointer_constraint(s, &ptr, |c| c.is_some());
                    if !still_present {
                        let _ = self.core.destroy_pointer_constraint(surface);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// OB-54: Smithay 0.7.0 keeps a constraint's lifetime private, so Core holds every
    /// constraint as persistent. A oneshot constraint that Smithay ended through `leave()` (the
    /// delivered focus changed in the batch that ended it, W-SM-4a) is destroyed by Smithay; at
    /// the end of the batch Core drops it too, so it is never activated again and Core and
    /// Smithay keep the same constraint state (INV-INPUT-5). Nothing reaches the client: the
    /// constraint no longer exists, and Core's deactivate effect for it is discarded.
    pub(crate) fn drop_constraints_smithay_destroyed(&mut self) {
        let core = &self.core;
        let Some(surface) = core
            .constraint()
            .or(core.pending_constraint())
            .map(|c| c.surface)
        else {
            return;
        };
        let Some(s) = self.registry.surface(surface) else {
            return;
        };
        let ptr = self.pointer_handle();
        if !with_pointer_constraint(s, &ptr, |c| c.is_some()) {
            let _ = self.core.destroy_pointer_constraint(surface);
        }
    }

    /// Drag-and-drop is not offered (clipboard slice, 2026-10-16). Smithay accepts a client's
    /// `start_drag` on the implicit grab of a pressed button and installs its DnD grab before any
    /// handler can refuse it; that grab would take pointer delivery away from Core's routing. It
    /// is replaced at once by the grab that ends at the next pointer event and forwards it (as
    /// after a pan-dismissed popup grab): Smithay drops the drag, the source gets `cancelled`, no
    /// client is offered anything, and delivery follows Core's effects again. Run after every
    /// client dispatch.
    pub fn refuse_client_drags(&mut self) {
        if !std::mem::take(&mut self.drag_started) {
            return;
        }
        let ptr = self.pointer_handle();
        let Some(serial) = ptr.with_grab(|serial, _| serial) else {
            return;
        };
        let start = GrabStartData {
            focus: None,
            button: 0,
            location: ptr.current_location(),
        };
        ptr.set_grab(
            self,
            PanDismissedPopupGrab { start },
            serial,
            smithay::input::pointer::Focus::Keep,
        );
    }

    pub(crate) fn set_pan_dismissed_pointer_grab(&mut self, serial: smithay::utils::Serial) {
        self.popup_pointer_grab_serial = None;
        let ptr = self.pointer_handle();
        let location = ptr.current_location();
        let start = GrabStartData {
            focus: None,
            button: 0,
            location,
        };
        ptr.set_grab(
            self,
            PanDismissedPopupGrab { start },
            serial,
            smithay::input::pointer::Focus::Keep,
        );
    }

    /// The seat (tests read Smithay's pointer state through it; nothing writes through it).
    pub fn seat(&self) -> &Seat<WaylandState> {
        &self.seat
    }

    pub(crate) fn set_popup_pointer_grab(
        &mut self,
        grab: &smithay::desktop::PopupGrab<WaylandState>,
        serial: smithay::utils::Serial,
    ) {
        self.popup_pointer_grab_serial = Some(serial);
        let ptr = self.pointer_handle();
        ptr.set_grab(
            self,
            smithay::desktop::PopupPointerGrab::new(grab),
            serial,
            smithay::input::pointer::Focus::Keep,
        );
    }

    pub fn is_pointer_grabbed(&self) -> bool {
        let ptr = self.pointer_handle();
        ptr.is_grabbed()
    }

    pub(crate) fn pointer_has_grab(&self, serial: smithay::utils::Serial) -> bool {
        let ptr = self.pointer_handle();
        ptr.has_grab(serial)
    }

    pub(crate) fn pointer_unset_grab(&mut self, serial: smithay::utils::Serial, time: u32) {
        self.popup_pointer_grab_serial = None;
        let ptr = self.pointer_handle();
        ptr.unset_grab(self, serial, time);
        self.pointer_sent = Sent::default();
    }

    pub(crate) fn keyboard_unset_grab(&mut self) {
        self.popup_keyboard_grab_serial = None;
        let kb = self.keyboard_handle();
        kb.unset_grab(self);
    }

    pub(crate) fn set_keyboard_grab(
        &mut self,
        kb: &smithay::input::keyboard::KeyboardHandle<WaylandState>,
        grab: smithay::desktop::PopupKeyboardGrab<WaylandState>,
        serial: smithay::utils::Serial,
    ) {
        self.popup_keyboard_grab_serial = Some(serial);
        kb.set_grab(self, grab, serial);
    }

    #[doc(hidden)]
    #[cfg(feature = "probe_test_keyboard_grab")]
    pub fn test_set_keyboard_grab(&mut self, serial: smithay::utils::Serial) {
        struct ForeignGrab;
        impl smithay::input::keyboard::KeyboardGrab<WaylandState> for ForeignGrab {
            fn input(
                &mut self,
                _data: &mut WaylandState,
                _handle: &mut smithay::input::keyboard::KeyboardInnerHandle<'_, WaylandState>,
                _keycode: smithay::backend::input::Keycode,
                _state: smithay::backend::input::KeyState,
                _modifiers: Option<smithay::input::keyboard::ModifiersState>,
                _serial: smithay::utils::Serial,
                _time: u32,
            ) {
            }
            fn set_focus(
                &mut self,
                _data: &mut WaylandState,
                _handle: &mut smithay::input::keyboard::KeyboardInnerHandle<'_, WaylandState>,
                _focus: Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>,
                _serial: smithay::utils::Serial,
            ) {
            }
            fn unset(&mut self, _data: &mut WaylandState) {}
            fn start_data(&self) -> &smithay::input::keyboard::GrabStartData<WaylandState> {
                static DATA: std::sync::OnceLock<
                    smithay::input::keyboard::GrabStartData<WaylandState>,
                > = std::sync::OnceLock::new();
                DATA.get_or_init(|| smithay::input::keyboard::GrabStartData { focus: None })
            }
        }
        let kb = self.keyboard_handle();
        kb.set_grab(self, ForeignGrab, serial);
    }
}

fn axis_frame(axis: Axis, time_ms: u32) -> AxisFrame {
    let source = match axis.source {
        AxisSource::Wheel => smithay::backend::input::AxisSource::Wheel,
        AxisSource::Continuous => smithay::backend::input::AxisSource::Continuous,
    };
    let mut frame = AxisFrame::new(time_ms).source(source);
    for (dir, amount, v120) in [
        (
            smithay::backend::input::Axis::Horizontal,
            axis.amount.0,
            axis.v120.0,
        ),
        (
            smithay::backend::input::Axis::Vertical,
            axis.amount.1,
            axis.v120.1,
        ),
    ] {
        if amount != 0.0 {
            frame = frame.value(dir, amount);
        }
        if v120 != 0 {
            frame = frame.v120(dir, v120);
        }
    }
    frame
}
