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
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::SERIAL_COUNTER;

use crate::{WaylandState, convert};

type Focus = Option<(
    <WaylandState as SeatHandler>::PointerFocus,
    convert::GlobalPoint,
)>;
type Inner<'a> = PointerInnerHandle<'a, WaylandState>;

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

impl WaylandState {
    /// The seat's pointer, created with the seat.
    fn pointer_handle(&self) -> PointerHandle<WaylandState> {
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
                let event = MotionEvent {
                    location: convert::canvas_to_global(location),
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                };
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
                let event = ButtonEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time: time_ms,
                    button,
                    state,
                };
                ptr.button(self, &event);
            }
            Effect::PointerAxis { axis, time_ms } => ptr.axis(self, axis_frame(axis, time_ms)),
            Effect::PointerFrame => ptr.frame(self),
            Effect::PointerZeroOutputBegin => {
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
            }
            Effect::PointerZeroOutputEnd => {
                ptr.unset_grab(self, SERIAL_COUNTER.next_serial(), 0);
            }
            _ => return false,
        }
        true
    }

    /// The seat (tests read Smithay's pointer state through it; nothing writes through it).
    pub fn seat(&self) -> &Seat<WaylandState> {
        &self.seat
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
