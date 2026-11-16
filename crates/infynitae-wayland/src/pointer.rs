//! The seat pointer: Core's pointer effects applied to Smithay's `PointerHandle`, in order.
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
use smithay::input::touch::{
    DownEvent as TouchDownEvent, MotionEvent as TouchMotionEvent, UpEvent as TouchUpEvent,
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
    pub(crate) fn focus(&self, target: Option<PointerTarget>) -> Focus {
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
                self.pointer_motions += 1;
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
                if pressed {
                    self.osk_pressed(false);
                }
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
            Effect::GestureSwipeBegin { fingers, time_ms } => {
                let event = GestureSwipeBeginEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                    fingers,
                };
                ptr.gesture_swipe_begin(self, &event);
            }
            Effect::GestureSwipeUpdate { delta, time_ms } => {
                let event = GestureSwipeUpdateEvent {
                    time: time_ms,
                    delta: convert::global_delta(delta),
                };
                ptr.gesture_swipe_update(self, &event);
            }
            Effect::GestureSwipeEnd { cancelled, time_ms } => {
                let event = GestureSwipeEndEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                    cancelled,
                };
                ptr.gesture_swipe_end(self, &event);
            }
            Effect::GesturePinchBegin { fingers, time_ms } => {
                let event = GesturePinchBeginEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                    fingers,
                };
                ptr.gesture_pinch_begin(self, &event);
            }
            Effect::GesturePinchUpdate {
                delta,
                scale,
                rotation,
                time_ms,
            } => {
                let event = GesturePinchUpdateEvent {
                    time: time_ms,
                    delta: convert::global_delta(delta),
                    scale,
                    rotation,
                };
                ptr.gesture_pinch_update(self, &event);
            }
            Effect::GesturePinchEnd { cancelled, time_ms } => {
                let event = GesturePinchEndEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                    cancelled,
                };
                ptr.gesture_pinch_end(self, &event);
            }
            Effect::GestureHoldBegin { fingers, time_ms } => {
                let event = GestureHoldBeginEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                    fingers,
                };
                ptr.gesture_hold_begin(self, &event);
            }
            Effect::GestureHoldEnd { cancelled, time_ms } => {
                let event = GestureHoldEndEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                    cancelled,
                };
                ptr.gesture_hold_end(self, &event);
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
        let mut surfaces = Vec::new();
        if let Some(c) = self.core.constraint() {
            surfaces.push(c.surface);
        }
        for c in self.core.pending_constraints() {
            if !surfaces.contains(&c.surface) {
                surfaces.push(c.surface);
            }
        }
        if surfaces.is_empty() {
            return;
        }
        let ptr = self.pointer_handle();
        for surface in surfaces {
            let Some(s) = self.registry.surface(surface) else {
                continue;
            };
            if !with_pointer_constraint(s, &ptr, |c| c.is_some()) {
                let _ = self.core.destroy_pointer_constraint(surface);
            }
        }
    }

    /// Drag-and-drop is not offered (2026-10-16). Smithay accepts a client's
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

// ---- touch ----------------------------------------------------------------------------------
// `wl_touch`: the seat's touch, offered only while a touchscreen is there (`enable_touch`; some
// apps change their look for a touch seat). Core decides which surface each finger is on
// (`infynitae_core::touch`); this applies its effects to Smithay's touch handle, which keeps
// each finger on the surface it landed on. It is input delivery, so it lives here (C4.2).

impl WaylandState {
    /// The seat gets touch (a touchscreen was found). Idempotent.
    pub fn enable_touch(&mut self) {
        if self.seat.get_touch().is_none() {
            self.seat.add_touch();
        }
    }

    /// The seat's touch goes (the last touchscreen was removed): fingers down are given up.
    pub fn disable_touch(&mut self) {
        let fx = self.core.clear_touches();
        self.apply(&fx);
        self.cancel_touches();
        if self.seat.get_touch().is_some() {
            self.seat.remove_touch();
        }
    }

    /// Every finger down is given up (`wl_touch.cancel`). Smithay 0.7 sends a cancel only to a
    /// finger with an event since its last frame, which leaves out a finger that is held
    /// still: each one is sent its last location again first (a motion to where it is), so the
    /// cancel reaches every client holding a finger.
    pub fn cancel_touches(&mut self) {
        if let Some(l) = self.session_lock.as_mut() {
            l.fingers.clear();
        }
        let Some(touch) = self.seat.get_touch() else {
            self.touch_last.clear();
            self.lock_touch_last.clear();
            return;
        };
        for (slot, location) in std::mem::take(&mut self.touch_last) {
            let event = TouchMotionEvent {
                slot: Some(slot).into(),
                location: convert::canvas_to_global(location),
                time: 0,
            };
            touch.motion(self, None, &event);
        }
        for (slot, p) in std::mem::take(&mut self.lock_touch_last) {
            let event = TouchMotionEvent {
                slot: Some(slot).into(),
                location: p.into(),
                time: 0,
            };
            touch.motion(self, None, &event);
        }
        touch.cancel(self);
    }

    /// A touchscreen event, before Core (`PhysicalEvent::Touch*`, in the touched screen's logical
    /// px). Returns `true` when the lock took it (Core must not see it): where it goes is the
    /// lock's decision ([`WaylandState::lock_touch_route`]), delivered here.
    pub fn lock_touch(&mut self, ev: &infynitae_core::PhysicalEvent) -> bool {
        use crate::lock::LockTouchRoute;
        use infynitae_core::PhysicalEvent as P;
        let route = self.lock_touch_route(ev);
        let ms = |t: u64| (t / 1000) as u32;
        let touch = self.seat.get_touch();
        match (route, *ev) {
            (LockTouchRoute::Core, _) => return false,
            (LockTouchRoute::Dropped, _) => {}
            (
                LockTouchRoute::Down(surface),
                P::TouchDown {
                    slot, p, time_us, ..
                },
            ) => {
                if let Some(touch) = touch {
                    self.lock_touch_last.retain(|(s, _)| *s != slot);
                    self.lock_touch_last.push((slot, p));
                    // a lock surface covers its screen from its origin: the screen point is
                    // the surface point
                    let event = TouchDownEvent {
                        slot: Some(slot).into(),
                        location: p.into(),
                        serial: SERIAL_COUNTER.next_serial(),
                        time: ms(time_us),
                    };
                    touch.down(self, Some((surface, (0.0, 0.0).into())), &event);
                }
            }
            (LockTouchRoute::Lock, P::TouchMotion { slot, p, time_us }) => {
                if let Some(touch) = touch {
                    if let Some(last) = self.lock_touch_last.iter_mut().find(|(s, _)| *s == slot) {
                        last.1 = p;
                    }
                    let event = TouchMotionEvent {
                        slot: Some(slot).into(),
                        location: p.into(),
                        time: ms(time_us),
                    };
                    touch.motion(self, None, &event);
                }
            }
            (LockTouchRoute::Lock, P::TouchUp { slot, time_us }) => {
                if let Some(touch) = touch {
                    self.lock_touch_last.retain(|(s, _)| *s != slot);
                    let event = TouchUpEvent {
                        slot: Some(slot).into(),
                        serial: SERIAL_COUNTER.next_serial(),
                        time: ms(time_us),
                    };
                    touch.up(self, &event);
                }
            }
            (LockTouchRoute::Lock, P::TouchFrame) => {
                if let Some(touch) = touch {
                    touch.frame(self);
                }
            }
            (LockTouchRoute::Lock, P::TouchCancel) => {
                self.cancel_touches();
            }
            _ => {}
        }
        true
    }

    /// Applies one touch effect. Returns `false` for an effect that is not a touch effect.
    pub(crate) fn apply_touch(&mut self, effect: &Effect) -> bool {
        let touch = self.seat.get_touch();
        match *effect {
            Effect::TouchDown {
                slot,
                target,
                location,
                time_ms,
            } => {
                let Some(touch) = touch else { return true };
                let focus = self.focus(Some(target));
                self.touch_last.retain(|(s, _)| *s != slot);
                self.touch_last.push((slot, location));
                let event = TouchDownEvent {
                    slot: Some(slot).into(),
                    location: convert::canvas_to_global(location),
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                };
                touch.down(self, focus, &event);
                self.osk_pressed(true);
            }
            Effect::TouchMotion {
                slot,
                location,
                time_ms,
            } => {
                let Some(touch) = touch else { return true };
                // Smithay delivers it to the surface the finger landed on
                if let Some(last) = self.touch_last.iter_mut().find(|(s, _)| *s == slot) {
                    last.1 = location;
                }
                let event = TouchMotionEvent {
                    slot: Some(slot).into(),
                    location: convert::canvas_to_global(location),
                    time: time_ms,
                };
                touch.motion(self, None, &event);
            }
            Effect::TouchUp { slot, time_ms } => {
                let Some(touch) = touch else { return true };
                self.touch_last.retain(|(s, _)| *s != slot);
                let event = TouchUpEvent {
                    slot: Some(slot).into(),
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                };
                touch.up(self, &event);
            }
            Effect::TouchFrame => {
                if let Some(touch) = touch {
                    touch.frame(self);
                }
            }
            Effect::TouchCancel => self.cancel_touches(),
            _ => return false,
        }
        true
    }
}

// ---- tablet (`zwp_tablet_manager_v2`: tools, tablets, pads) ---------------------------------
// Tablet tool and pad delivery lives here alongside pointer and touch delivery (C4.2, source
// rule `smithay-pointer-only-in-adapter-pointer`). A surface whose client bound
// `zwp_tablet_seat_v2` for this seat receives `zwp_tablet_tool_v2` events (proximity_in, down,
// up, motion, pressure, tilt, distance, slider, rotation, wheel, button, frame) at its
// surface-local coordinates through the output's camera; a client that has not bound it gets
// pointer motion and left click instead, never both.

const BTN_LEFT: u32 = 0x110;

/// Extracts the button code from a backend's tablet tool button event inside `pointer` (C4.2:
/// source rule `smithay-pointer-only-in-adapter-pointer` reserves `.button()` for this module).
pub fn tablet_tool_button<B: smithay::backend::input::InputBackend>(
    event: &B::TabletToolButtonEvent,
) -> u32 {
    use smithay::backend::input::TabletToolButtonEvent;
    event.button()
}

/// Extra axes reported with a tablet tool event (`zwp_tablet_tool_v2`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TabletAxes {
    pub pressure: Option<f64>,
    pub distance: Option<f64>,
    pub tilt: Option<(f64, f64)>,
    pub rotation: Option<f64>,
    pub slider: Option<f64>,
    pub wheel: Option<(f64, i32)>,
}

impl TabletAxes {
    fn any(&self) -> bool {
        self.pressure.is_some()
            || self.distance.is_some()
            || self.tilt.is_some()
            || self.rotation.is_some()
            || self.slider.is_some()
            || self.wheel.is_some()
    }
}

/// One event from a tablet stylus or tool, mapped to `output`'s logical coordinates `p`.
#[derive(Clone, Debug, PartialEq)]
pub enum TabletEvent {
    ProximityIn {
        tablet: smithay::wayland::tablet_manager::TabletDescriptor,
        tool: smithay::backend::input::TabletToolDescriptor,
        output: infynitae_core::output::OutputId,
        p: (f64, f64),
        axes: TabletAxes,
        time_us: u64,
    },
    ProximityOut {
        tool: smithay::backend::input::TabletToolDescriptor,
        time_us: u64,
    },
    Axis {
        tablet: smithay::wayland::tablet_manager::TabletDescriptor,
        tool: smithay::backend::input::TabletToolDescriptor,
        output: infynitae_core::output::OutputId,
        p: (f64, f64),
        axes: TabletAxes,
        time_us: u64,
    },
    Tip {
        tablet: smithay::wayland::tablet_manager::TabletDescriptor,
        tool: smithay::backend::input::TabletToolDescriptor,
        output: infynitae_core::output::OutputId,
        p: (f64, f64),
        down: bool,
        axes: TabletAxes,
        time_us: u64,
    },
    Button {
        tool: smithay::backend::input::TabletToolDescriptor,
        button: u32,
        pressed: bool,
        time_us: u64,
    },
}

/// Description of a tablet pad device (`zwp_tablet_pad_v2`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabletPadDescriptor {
    pub sysname: String,
    pub path: Option<String>,
    pub buttons: u32,
    pub modes: u32,
    pub tablet: smithay::wayland::tablet_manager::TabletDescriptor,
}

pub(crate) enum TabletCursor<'a> {
    Inactive,
    OnOutput {
        p: (f64, f64),
        status: Option<&'a smithay::input::pointer::CursorImageStatus>,
    },
    OnOtherOutput,
}

#[derive(Clone, Copy, Debug)]
enum TabletGrab {
    /// Tip down on a client that bound `tablet-v2`: keeps that surface until tip up.
    Tablet {
        output: infynitae_core::output::OutputId,
        target: PointerTarget,
        layer_offset: Option<(f64, f64)>,
    },
    /// Tip down on a client without `tablet-v2` (or empty canvas): drives `BTN_LEFT` on the pointer.
    Pointer,
}

struct PadEntry {
    desc: TabletPadDescriptor,
    max_server_id: u32,
    instances: Vec<(
        smithay::reexports::wayland_server::Weak<
            smithay::reexports::wayland_protocols::wp::tablet::zv2::server::zwp_tablet_pad_v2::ZwpTabletPadV2,
        >,
        smithay::reexports::wayland_server::Weak<
            smithay::reexports::wayland_protocols::wp::tablet::zv2::server::zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        >,
    )>,
}

#[derive(Default)]
pub(crate) struct TabletState {
    seats: Vec<
        smithay::reexports::wayland_server::Weak<
            smithay::reexports::wayland_protocols::wp::tablet::zv2::server::zwp_tablet_seat_v2::ZwpTabletSeatV2,
        >,
    >,
    grab: Option<TabletGrab>,
    focused_tool: Option<(smithay::backend::input::TabletToolDescriptor, WlSurface)>,
    last_loc: Option<convert::GlobalPoint>,
    cursor_at: Option<(infynitae_core::output::OutputId, (f64, f64))>,
    tool_cursor: Option<smithay::input::pointer::CursorImageStatus>,
    pads: Vec<PadEntry>,
    pad_focus: Option<WlSurface>,
}

struct TabletDelivery<'a> {
    tablet: &'a smithay::wayland::tablet_manager::TabletHandle,
    tool_handle: &'a smithay::wayland::tablet_manager::TabletToolHandle,
    tool: smithay::backend::input::TabletToolDescriptor,
    output: infynitae_core::output::OutputId,
    p: (f64, f64),
    surface: WlSurface,
    origin: convert::GlobalPoint,
    location: infynitae_core::geom::CanvasPos,
    axes: &'a TabletAxes,
    force_motion: bool,
    time_ms: u32,
}

fn queue_tablet_axes(tool: &smithay::wayland::tablet_manager::TabletToolHandle, axes: &TabletAxes) {
    if let Some(pressure) = axes.pressure {
        tool.pressure(pressure);
    }
    if let Some(distance) = axes.distance {
        tool.distance(distance);
    }
    if let Some(tilt) = axes.tilt {
        tool.tilt(tilt);
    }
    if let Some(rotation) = axes.rotation {
        tool.rotation(rotation);
    }
    if let Some(slider) = axes.slider {
        tool.slider_position(slider);
    }
    if let Some((degrees, clicks)) = axes.wheel {
        tool.wheel(degrees, clicks);
    }
}

impl WaylandState {
    /// Registers a graphics tablet on the seat (`zwp_tablet_v2`).
    pub fn add_tablet(&mut self, desc: &smithay::wayland::tablet_manager::TabletDescriptor) {
        use smithay::wayland::tablet_manager::TabletSeatTrait;
        self.seat.tablet_seat().add_tablet::<Self>(&self.dh, desc);
    }

    /// Removes a graphics tablet from the seat.
    pub fn remove_tablet(&mut self, desc: &smithay::wayland::tablet_manager::TabletDescriptor) {
        use smithay::wayland::tablet_manager::TabletSeatTrait;
        self.seat.tablet_seat().remove_tablet(desc);
    }

    /// Registers a tablet pad (`zwp_tablet_pad_v2`) and announces it to bound tablet seats.
    pub fn add_tablet_pad(&mut self, desc: TabletPadDescriptor) {
        use smithay::wayland::tablet_manager::TabletSeatTrait;
        self.seat
            .tablet_seat()
            .add_tablet::<Self>(&self.dh, &desc.tablet);
        self.remove_tablet_pad(&desc.sysname);
        let mut entry = PadEntry {
            desc,
            max_server_id: 0xFF00_0000,
            instances: Vec::new(),
        };
        let seats: Vec<_> = self
            .tablet
            .seats
            .iter()
            .filter_map(|s| s.upgrade().ok())
            .collect();
        for seat in seats {
            if let Ok(client) = self.dh.get_client(seat.id()) {
                Self::announce_pad(&self.dh, &client, &seat, &mut entry);
            }
        }
        self.tablet.pads.push(entry);
        if let Some(surface) = self
            .tablet
            .pad_focus
            .clone()
            .or_else(|| self.keyboard_handle().current_focus())
        {
            self.tablet.pad_focus = Some(surface.clone());
            self.send_pad_enter(&surface);
        }
    }

    /// Removes a tablet pad by its device sysname.
    pub fn remove_tablet_pad(&mut self, sysname: &str) {
        self.tablet.pads.retain(|p| {
            if p.desc.sysname == sysname {
                for (pad, _) in &p.instances {
                    if let Ok(wl_pad) = pad.upgrade() {
                        wl_pad.removed();
                    }
                }
                false
            } else {
                true
            }
        });
    }

    /// Forwards a tablet pad button press or release through `zwp_tablet_pad_v2.button` to the
    /// focused client, and nothing else.
    pub fn tablet_pad_button(&mut self, sysname: &str, button: u32, pressed: bool, time_us: u64) {
        use smithay::reexports::wayland_protocols::wp::tablet::zv2::server::zwp_tablet_pad_v2;
        if self.is_locked() {
            return;
        }
        let target = self
            .keyboard_handle()
            .current_focus()
            .or_else(|| self.tablet.pad_focus.clone())
            .or_else(|| self.tablet.focused_tool.as_ref().map(|(_, s)| s.clone()));
        let Some(surface) = target else {
            return;
        };
        if self.tablet.pad_focus.as_ref() != Some(&surface) {
            self.update_tablet_pad_focus(Some(&surface));
        }
        let time_ms = (time_us / 1000) as u32;
        let state = if pressed {
            zwp_tablet_pad_v2::ButtonState::Pressed
        } else {
            zwp_tablet_pad_v2::ButtonState::Released
        };
        for pad in &self.tablet.pads {
            if !sysname.is_empty() && pad.desc.sysname != sysname {
                continue;
            }
            for (pad_weak, _) in &pad.instances {
                if pad_weak.id().same_client_as(&surface.id())
                    && let Ok(wl_pad) = pad_weak.upgrade()
                {
                    wl_pad.button(time_ms, button, state);
                }
            }
        }
    }

    pub(crate) fn tablet_seat_bound(
        &mut self,
        client: &smithay::reexports::wayland_server::Client,
        seat: &smithay::reexports::wayland_protocols::wp::tablet::zv2::server::zwp_tablet_seat_v2::ZwpTabletSeatV2,
    ) {
        self.tablet.seats.retain(|s| s.upgrade().is_ok());
        self.tablet.seats.push(seat.downgrade());
        for pad in &mut self.tablet.pads {
            Self::announce_pad(&self.dh, client, seat, pad);
        }
        if let Some(surface) = self
            .keyboard_handle()
            .current_focus()
            .filter(|s| s.id().same_client_as(&seat.id()))
        {
            self.tablet.pad_focus = Some(surface.clone());
            self.send_pad_enter(&surface);
        }
    }

    pub(crate) fn tablet_seat_destroyed(
        &mut self,
        seat_id: &smithay::reexports::wayland_server::backend::ObjectId,
    ) {
        self.tablet.seats.retain(|s| &s.id() != seat_id);
    }

    pub(crate) fn client_has_tablet_seat(&self, surface: &WlSurface) -> bool {
        self.tablet
            .seats
            .iter()
            .any(|s| s.id().same_client_as(&surface.id()) && s.upgrade().is_ok())
    }

    pub(crate) fn set_tablet_tool_cursor(
        &mut self,
        image: smithay::input::pointer::CursorImageStatus,
    ) {
        if self.tablet.cursor_at.is_some() {
            self.tablet.tool_cursor = Some(image);
        } else {
            self.set_cursor_status(image);
        }
    }

    pub(crate) fn tablet_cursor(
        &self,
        output: infynitae_core::output::OutputId,
    ) -> TabletCursor<'_> {
        match self.tablet.cursor_at {
            Some((o, p)) if o == output => TabletCursor::OnOutput {
                p,
                status: self.tablet.tool_cursor.as_ref(),
            },
            Some(_) => TabletCursor::OnOtherOutput,
            None => TabletCursor::Inactive,
        }
    }

    fn announce_pad(
        dh: &smithay::reexports::wayland_server::DisplayHandle,
        client: &smithay::reexports::wayland_server::Client,
        seat: &smithay::reexports::wayland_protocols::wp::tablet::zv2::server::zwp_tablet_seat_v2::ZwpTabletSeatV2,
        entry: &mut PadEntry,
    ) {
        use smithay::reexports::wayland_protocols::wp::tablet::zv2::server::{
            zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, zwp_tablet_pad_v2::ZwpTabletPadV2,
        };
        let Ok(wl_pad) = client.create_resource::<ZwpTabletPadV2, _, Self>(dh, seat.version(), ())
        else {
            return;
        };
        entry.max_server_id = entry.max_server_id.max(wl_pad.id().protocol_id());
        seat.pad_added(&wl_pad);
        let Ok(wl_group) =
            client.create_resource::<ZwpTabletPadGroupV2, _, Self>(dh, wl_pad.version(), ())
        else {
            return;
        };
        entry.max_server_id = entry.max_server_id.max(wl_group.id().protocol_id());
        wl_pad.group(&wl_group);
        let btns: Vec<u8> = (0..entry.desc.buttons).flat_map(u32::to_ne_bytes).collect();
        wl_group.buttons(btns);
        if entry.desc.modes > 1 {
            wl_group.modes(entry.desc.modes);
        }
        wl_group.done();
        if let Some(ref path) = entry.desc.path {
            wl_pad.path(path.clone());
        }
        if entry.desc.buttons > 0 {
            wl_pad.buttons(entry.desc.buttons);
        }
        wl_pad.done();
        entry
            .instances
            .push((wl_pad.downgrade(), wl_group.downgrade()));
    }

    pub(crate) fn update_tablet_pad_focus(&mut self, focused: Option<&WlSurface>) {
        if self.tablet.pad_focus.as_ref() == focused {
            return;
        }
        if let Some(old) = self.tablet.pad_focus.take()
            && old.is_alive()
        {
            let serial: u32 = SERIAL_COUNTER.next_serial().into();
            for pad in &self.tablet.pads {
                for (pad_weak, _) in &pad.instances {
                    if pad_weak.id().same_client_as(&old.id())
                        && let Ok(wl_pad) = pad_weak.upgrade()
                    {
                        wl_pad.leave(serial, &old);
                    }
                }
            }
        }
        if let Some(surface) = focused {
            self.tablet.pad_focus = Some(surface.clone());
            self.send_pad_enter(surface);
        }
    }

    fn send_pad_enter(&mut self, surface: &WlSurface) {
        use smithay::reexports::wayland_protocols::wp::tablet::zv2::server::zwp_tablet_v2::ZwpTabletV2;
        let max_id = self
            .tablet
            .pads
            .iter()
            .filter(|p| {
                p.instances
                    .iter()
                    .any(|(pad_weak, _)| pad_weak.id().same_client_as(&surface.id()))
            })
            .map(|p| p.max_server_id)
            .max();
        let Some(max_id) = max_id else {
            return;
        };
        let Ok(client) = self.dh.get_client(surface.id()) else {
            return;
        };
        let upper = max_id.max(0xFF00_1000);
        let Some(wl_tablet) = (0xFF00_0000..=upper).find_map(|id| {
            client
                .object_from_protocol_id::<ZwpTabletV2>(&self.dh, id)
                .ok()
        }) else {
            return;
        };
        let serial: u32 = SERIAL_COUNTER.next_serial().into();
        for pad in &self.tablet.pads {
            for (pad_weak, group_weak) in &pad.instances {
                if pad_weak.id().same_client_as(&surface.id())
                    && let Ok(wl_pad) = pad_weak.upgrade()
                {
                    wl_pad.enter(serial, &wl_tablet, surface);
                    if let Ok(wl_group) = group_weak.upgrade() {
                        wl_group.mode_switch(0, serial, 0);
                    }
                }
            }
        }
    }

    pub(crate) fn cancel_tablet(&mut self) {
        use smithay::wayland::tablet_manager::TabletSeatTrait;
        if let Some((ref tool, _)) = self.tablet.focused_tool.take()
            && let Some(handle) = self.seat.tablet_seat().get_tool(tool)
        {
            handle.proximity_out(0);
        }
        self.tablet.grab = None;
        self.tablet.last_loc = None;
        self.tablet.cursor_at = None;
        self.tablet.tool_cursor = None;
    }

    fn clear_tablet_focus(
        &mut self,
        tool_handle: &smithay::wayland::tablet_manager::TabletToolHandle,
        time_ms: u32,
    ) {
        tool_handle.proximity_out(time_ms);
        self.tablet.focused_tool = None;
        self.tablet.last_loc = None;
        self.tablet.cursor_at = None;
        self.tablet.tool_cursor = None;
    }

    fn pointer_delta_to(
        &self,
        output: infynitae_core::output::OutputId,
        p: (f64, f64),
    ) -> Option<(f64, f64)> {
        let dst = self.core.output(output)?;
        let target = (dst.arrangement.0 + p.0, dst.arrangement.1 + p.1);
        let now = match self.core.pointer() {
            infynitae_core::pointer::PointerState::Located {
                output: cur,
                p: cur_p,
            } => {
                let o = self.core.output(cur)?;
                (o.arrangement.0 + cur_p.0, o.arrangement.1 + cur_p.1)
            }
            infynitae_core::pointer::PointerState::NoOutput => return None,
        };
        Some((target.0 - now.0, target.1 - now.1))
    }

    fn pointer_motion_to(
        &self,
        output: infynitae_core::output::OutputId,
        p: (f64, f64),
        time_us: u64,
        even_if_zero_when_unfocused: bool,
    ) -> Vec<infynitae_core::PhysicalEvent> {
        let Some(delta) = self.pointer_delta_to(output, p) else {
            return Vec::new();
        };
        let moved = delta.0.abs() > 1e-6 || delta.1.abs() > 1e-6;
        if moved || (even_if_zero_when_unfocused && self.pointer_handle().current_focus().is_none())
        {
            vec![infynitae_core::PhysicalEvent::PointerMotion {
                delta,
                delta_unaccel: delta,
                time_us,
            }]
        } else {
            Vec::new()
        }
    }

    fn deliver_tablet_surface(&mut self, d: TabletDelivery<'_>) {
        let loc = convert::canvas_to_global(d.location);
        let same = self
            .tablet
            .focused_tool
            .as_ref()
            .is_some_and(|(tl, s)| *tl == d.tool && *s == d.surface);
        if !same {
            self.tablet.tool_cursor = None;
            d.tool_handle.proximity_in(
                loc,
                (d.surface.clone(), d.origin),
                d.tablet,
                SERIAL_COUNTER.next_serial(),
                d.time_ms,
            );
            self.tablet.focused_tool = Some((d.tool, d.surface.clone()));
            self.tablet.last_loc = Some(loc);
            if d.axes.any() {
                queue_tablet_axes(d.tool_handle, d.axes);
                d.tool_handle.motion(
                    loc,
                    Some((d.surface, d.origin)),
                    d.tablet,
                    SERIAL_COUNTER.next_serial(),
                    d.time_ms,
                );
            }
        } else if d.force_motion || self.tablet.last_loc != Some(loc) || d.axes.any() {
            queue_tablet_axes(d.tool_handle, d.axes);
            d.tool_handle.motion(
                loc,
                Some((d.surface, d.origin)),
                d.tablet,
                SERIAL_COUNTER.next_serial(),
                d.time_ms,
            );
            self.tablet.last_loc = Some(loc);
        }
        self.tablet.cursor_at = Some((d.output, d.p));
    }

    /// Delivers a tablet tool event (`zwp_tablet_tool_v2`) when the surface under the pen bound
    /// `tablet-v2` for this seat, and returns the Core [`PhysicalEvent`](infynitae_core::PhysicalEvent)s
    /// for the backend to submit through its input port (window focus on a pen tap, or pointer
    /// motion and `BTN_LEFT` click when the surface under the pen has not bound `tablet-v2`).
    pub fn tablet_event(&mut self, ev: TabletEvent) -> Vec<infynitae_core::PhysicalEvent> {
        use infynitae_core::PhysicalEvent;
        use smithay::wayland::tablet_manager::TabletSeatTrait;
        if self.is_locked() {
            self.cancel_tablet();
            return Vec::new();
        }
        let ms = |t: u64| (t / 1000) as u32;
        match ev {
            TabletEvent::ProximityIn {
                tablet,
                tool,
                output,
                p,
                axes,
                time_us,
            } => {
                let time_ms = ms(time_us);
                let dh = self.dh.clone();
                let tablet_seat = self.seat.tablet_seat();
                let tablet_handle = tablet_seat.add_tablet::<Self>(&dh, &tablet);
                let tool_handle = tablet_seat.add_tool::<Self>(self, &dh, &tool);
                let Some((target, location)) = self.core.route_point(output, p) else {
                    return Vec::new();
                };
                if let Some((surface, origin)) = self.focus(target)
                    && self.client_has_tablet_seat(&surface)
                {
                    // Force fresh proximity_in if re-entering after an out-of-band state reset.
                    self.tablet.focused_tool = None;
                    self.deliver_tablet_surface(TabletDelivery {
                        tablet: &tablet_handle,
                        tool_handle: &tool_handle,
                        tool,
                        output,
                        p,
                        surface,
                        origin,
                        location,
                        axes: &axes,
                        force_motion: false,
                        time_ms,
                    });
                    Vec::new()
                } else {
                    self.clear_tablet_focus(&tool_handle, time_ms);
                    self.pointer_motion_to(output, p, time_us, false)
                }
            }
            TabletEvent::Axis {
                tablet,
                tool,
                output,
                p,
                axes,
                time_us,
            } => {
                let time_ms = ms(time_us);
                let dh = self.dh.clone();
                let tablet_seat = self.seat.tablet_seat();
                let tablet_handle = tablet_seat.add_tablet::<Self>(&dh, &tablet);
                let tool_handle = tablet_seat.add_tool::<Self>(self, &dh, &tool);
                match self.tablet.grab {
                    Some(TabletGrab::Tablet {
                        output: grab_out,
                        target,
                        layer_offset,
                    }) => {
                        let Some(location) = self.core.unproject_point(grab_out, p, layer_offset)
                        else {
                            return Vec::new();
                        };
                        if let Some((surface, origin)) = self.focus(Some(target)) {
                            self.deliver_tablet_surface(TabletDelivery {
                                tablet: &tablet_handle,
                                tool_handle: &tool_handle,
                                tool,
                                output: grab_out,
                                p,
                                surface,
                                origin,
                                location,
                                axes: &axes,
                                force_motion: true,
                                time_ms,
                            });
                        }
                        Vec::new()
                    }
                    Some(TabletGrab::Pointer) => self.pointer_motion_to(output, p, time_us, false),
                    None => {
                        let Some((target, location)) = self.core.route_point(output, p) else {
                            return Vec::new();
                        };
                        if let Some((surface, origin)) = self.focus(target)
                            && self.client_has_tablet_seat(&surface)
                        {
                            self.deliver_tablet_surface(TabletDelivery {
                                tablet: &tablet_handle,
                                tool_handle: &tool_handle,
                                tool,
                                output,
                                p,
                                surface,
                                origin,
                                location,
                                axes: &axes,
                                force_motion: true,
                                time_ms,
                            });
                            Vec::new()
                        } else {
                            self.clear_tablet_focus(&tool_handle, time_ms);
                            self.pointer_motion_to(output, p, time_us, false)
                        }
                    }
                }
            }
            TabletEvent::Tip {
                tablet,
                tool,
                output,
                p,
                down: true,
                axes,
                time_us,
            } => {
                let time_ms = ms(time_us);
                let dh = self.dh.clone();
                let tablet_seat = self.seat.tablet_seat();
                let tablet_handle = tablet_seat.add_tablet::<Self>(&dh, &tablet);
                let tool_handle = tablet_seat.add_tool::<Self>(self, &dh, &tool);
                let Some((target, location)) = self.core.route_point(output, p) else {
                    return Vec::new();
                };
                if let Some((surface, origin)) = self.focus(target)
                    && self.client_has_tablet_seat(&surface)
                {
                    let t = target.expect("focus comes from target");
                    let layer_offset = self.core.is_layer_surface(t.surface).then(|| {
                        let q = infynitae_core::geom::Q16_ONE as f64;
                        (p.0 - location.x as f64 / q, p.1 - location.y as f64 / q)
                    });
                    self.tablet.grab = Some(TabletGrab::Tablet {
                        output,
                        target: t,
                        layer_offset,
                    });
                    self.deliver_tablet_surface(TabletDelivery {
                        tablet: &tablet_handle,
                        tool_handle: &tool_handle,
                        tool,
                        output,
                        p,
                        surface,
                        origin,
                        location,
                        axes: &axes,
                        force_motion: false,
                        time_ms,
                    });
                    let serial = SERIAL_COUNTER.next_serial();
                    self.record_serial_output(serial, output);
                    tool_handle.tip_down(serial, time_ms);
                    self.osk_pressed(true);
                    vec![PhysicalEvent::TabletTap { output, p, time_us }]
                } else {
                    self.clear_tablet_focus(&tool_handle, time_ms);
                    self.tablet.grab = Some(TabletGrab::Pointer);
                    let mut out = self.pointer_motion_to(output, p, time_us, true);
                    out.push(PhysicalEvent::PointerButton {
                        button: BTN_LEFT,
                        pressed: true,
                        modifiers: self.seat_modifiers(),
                        time_us,
                    });
                    out
                }
            }
            TabletEvent::Tip {
                tablet,
                tool,
                output,
                p,
                down: false,
                axes,
                time_us,
            } => {
                let time_ms = ms(time_us);
                let dh = self.dh.clone();
                let tablet_seat = self.seat.tablet_seat();
                let tablet_handle = tablet_seat.add_tablet::<Self>(&dh, &tablet);
                let tool_handle = tablet_seat.add_tool::<Self>(self, &dh, &tool);
                match self.tablet.grab.take() {
                    Some(TabletGrab::Tablet { .. }) => {
                        tool_handle.tip_up(time_ms);
                        if let Some((target, location)) = self.core.route_point(output, p)
                            && let Some((surface, origin)) = self.focus(target)
                            && self.client_has_tablet_seat(&surface)
                        {
                            self.deliver_tablet_surface(TabletDelivery {
                                tablet: &tablet_handle,
                                tool_handle: &tool_handle,
                                tool,
                                output,
                                p,
                                surface,
                                origin,
                                location,
                                axes: &axes,
                                force_motion: false,
                                time_ms,
                            });
                        } else {
                            self.clear_tablet_focus(&tool_handle, time_ms);
                        }
                        Vec::new()
                    }
                    Some(TabletGrab::Pointer) => {
                        let mut out = self.pointer_motion_to(output, p, time_us, false);
                        out.push(PhysicalEvent::PointerButton {
                            button: BTN_LEFT,
                            pressed: false,
                            modifiers: self.seat_modifiers(),
                            time_us,
                        });
                        out
                    }
                    None => {
                        tool_handle.tip_up(time_ms);
                        Vec::new()
                    }
                }
            }
            TabletEvent::Button {
                tool,
                button,
                pressed,
                time_us,
            } => {
                let time_ms = ms(time_us);
                if let Some(tool_handle) = self.seat.tablet_seat().get_tool(&tool) {
                    let state = if pressed {
                        smithay::backend::input::ButtonState::Pressed
                    } else {
                        smithay::backend::input::ButtonState::Released
                    };
                    tool_handle.button(button, state, SERIAL_COUNTER.next_serial(), time_ms);
                }
                Vec::new()
            }
            TabletEvent::ProximityOut { tool, time_us } => {
                let time_ms = ms(time_us);
                if let Some(tool_handle) = self.seat.tablet_seat().get_tool(&tool) {
                    self.clear_tablet_focus(&tool_handle, time_ms);
                } else {
                    self.tablet.focused_tool = None;
                    self.tablet.last_loc = None;
                    self.tablet.cursor_at = None;
                    self.tablet.tool_cursor = None;
                }
                if matches!(self.tablet.grab.take(), Some(TabletGrab::Pointer)) {
                    vec![PhysicalEvent::PointerButton {
                        button: BTN_LEFT,
                        pressed: false,
                        modifiers: self.seat_modifiers(),
                        time_us,
                    }]
                } else {
                    Vec::new()
                }
            }
        }
    }
}
