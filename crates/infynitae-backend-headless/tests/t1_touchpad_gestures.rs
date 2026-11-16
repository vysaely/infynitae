//! Touchpad gestures (`zwp_pointer_gestures_v1` and compositor gestures) over the full
//! headless Wayland path:
//! - 3-finger swipe pans the camera by the finger delta without reaching the client;
//! - 2-finger pinch reaches the hovered client as `zwp_pointer_gesture_pinch_v1` events;
//! - 4-finger pinch zooms the canvas around the cursor without reaching the client;
//! - 4-finger swipe up toggles the overview without reaching the client;
//! - hold gestures reach the hovered client as `zwp_pointer_gesture_hold_v1` events;
//! - `TOUCHPAD_GESTURES=false` sends 3-finger swipes (and all other gestures) to the client.

mod common;

use common::Probe;
use common::input::*;
use infynitae_backend_headless::{
    gesture_hold_begin, gesture_hold_end, gesture_pinch_begin, gesture_pinch_end,
    gesture_pinch_update, gesture_swipe_begin, gesture_swipe_end, gesture_swipe_update,
};
use infynitae_core::geom::{CanvasPos, Q16_ONE};
use infynitae_core::input::interaction::Interaction;
use infynitae_core::{ShellCommand, ShellStream};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::pointer_gestures::zv1::client::{
    zwp_pointer_gesture_hold_v1::{self, ZwpPointerGestureHoldV1},
    zwp_pointer_gesture_pinch_v1::{self, ZwpPointerGesturePinchV1},
    zwp_pointer_gesture_swipe_v1::{self, ZwpPointerGestureSwipeV1},
    zwp_pointer_gestures_v1::ZwpPointerGesturesV1,
};

delegate_noop!(Probe: ignore ZwpPointerGesturesV1);

impl Dispatch<ZwpPointerGestureSwipeV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpPointerGestureSwipeV1,
        ev: zwp_pointer_gesture_swipe_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let line = match ev {
            zwp_pointer_gesture_swipe_v1::Event::Begin {
                surface, fingers, ..
            } => format!("swipe_begin {} {fingers}", surface_label(st, &surface)),
            zwp_pointer_gesture_swipe_v1::Event::Update { dx, dy, .. } => {
                format!("swipe_update {dx},{dy}")
            }
            zwp_pointer_gesture_swipe_v1::Event::End { cancelled, .. } => {
                format!("swipe_end {cancelled}")
            }
            _ => return,
        };
        st.log.push(line);
    }
}

impl Dispatch<ZwpPointerGesturePinchV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpPointerGesturePinchV1,
        ev: zwp_pointer_gesture_pinch_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let line = match ev {
            zwp_pointer_gesture_pinch_v1::Event::Begin {
                surface, fingers, ..
            } => format!("pinch_begin {} {fingers}", surface_label(st, &surface)),
            zwp_pointer_gesture_pinch_v1::Event::Update {
                dx,
                dy,
                scale,
                rotation,
                ..
            } => format!("pinch_update {dx},{dy} {scale} {rotation}"),
            zwp_pointer_gesture_pinch_v1::Event::End { cancelled, .. } => {
                format!("pinch_end {cancelled}")
            }
            _ => return,
        };
        st.log.push(line);
    }
}

impl Dispatch<ZwpPointerGestureHoldV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpPointerGestureHoldV1,
        ev: zwp_pointer_gesture_hold_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let line = match ev {
            zwp_pointer_gesture_hold_v1::Event::Begin {
                surface, fingers, ..
            } => format!("hold_begin {} {fingers}", surface_label(st, &surface)),
            zwp_pointer_gesture_hold_v1::Event::End { cancelled, .. } => {
                format!("hold_end {cancelled}")
            }
            _ => return,
        };
        st.log.push(line);
    }
}

fn surface_label(st: &Probe, s: &wayland_client::protocol::wl_surface::WlSurface) -> String {
    st.surfaces
        .iter()
        .find(|p| p.wl == *s)
        .map_or("?".to_string(), |p| p.label.to_string())
}

struct GestureRig {
    r: Rig,
    _manager: ZwpPointerGesturesV1,
    _swipe: ZwpPointerGestureSwipeV1,
    _pinch: ZwpPointerGesturePinchV1,
    _hold: ZwpPointerGestureHoldV1,
}

fn rig() -> GestureRig {
    let mut r = Rig::x8();
    let (name, version) =
        r.ls.probe
            .global("zwp_pointer_gestures_v1")
            .expect("zwp_pointer_gestures_v1 global must be advertised");
    let qh = r.ls.qh.clone();
    let reg = r.ls.registry().clone();
    let manager: ZwpPointerGesturesV1 = reg.bind(name, version.min(3), &qh, ());
    let ptr = r.ls.probe.pointer.as_ref().expect("pointer bound");
    let swipe = manager.get_swipe_gesture(ptr, &qh, ());
    let pinch = manager.get_pinch_gesture(ptr, &qh, ());
    let hold = manager.get_hold_gesture(ptr, &qh, ());
    r.ls.pump();
    r.ls.probe.log.clear();
    GestureRig {
        r,
        _manager: manager,
        _swipe: swipe,
        _pinch: pinch,
        _hold: hold,
    }
}

fn gesture_lines(log: &[String]) -> Vec<&str> {
    log.iter()
        .map(String::as_str)
        .filter(|l| l.starts_with("swipe_") || l.starts_with("pinch_") || l.starts_with("hold_"))
        .collect()
}

fn pos(x: i64, y: i64) -> CanvasPos {
    CanvasPos {
        x: x * Q16_ONE,
        y: y * Q16_ONE,
    }
}

#[test]
fn three_finger_swipe_moves_camera_by_finger_delta() {
    let mut g = rig();
    let r = &mut g.r;
    assert_eq!(r.camera(), pos(0, 0));

    r.t += 1000;
    gesture_swipe_begin(&mut r.ls.state, &mut r.ls.input, 3, r.t).unwrap();
    r.ls.pump();
    assert!(matches!(
        r.ls.state.core().interaction(),
        Interaction::CameraPan { .. }
    ));

    r.t += 1000;
    gesture_swipe_update(&mut r.ls.state, &mut r.ls.input, (45.0, -20.0), r.t).unwrap();
    r.t += 1000;
    gesture_swipe_update(&mut r.ls.state, &mut r.ls.input, (15.0, 5.0), r.t).unwrap();
    r.ls.pump();
    assert_eq!(r.camera(), pos(-60, 15));

    r.t += 1000;
    gesture_swipe_end(&mut r.ls.state, &mut r.ls.input, false, r.t).unwrap();
    r.ls.pump();
    assert_eq!(r.ls.state.core().interaction(), Interaction::Idle);
    assert!(
        gesture_lines(&r.ls.probe.log).is_empty(),
        "3-finger swipe must not reach client: {:?}",
        r.ls.probe.log
    );
}

#[test]
fn two_finger_pinch_and_hold_reach_client() {
    let mut g = rig();
    let r = &mut g.r;

    r.t += 1000;
    gesture_pinch_begin(&mut r.ls.state, &mut r.ls.input, 2, r.t).unwrap();
    r.t += 1000;
    gesture_pinch_update(
        &mut r.ls.state,
        &mut r.ls.input,
        (3.0, -4.0),
        1.5,
        12.0,
        r.t,
    )
    .unwrap();
    r.t += 1000;
    gesture_pinch_end(&mut r.ls.state, &mut r.ls.input, false, r.t).unwrap();
    r.ls.pump();

    assert_eq!(
        r.ls.probe.log,
        vec!["pinch_begin W 2", "pinch_update 3,-4 1.5 12", "pinch_end 0",]
    );
    assert_eq!(r.ls.state.core().cameras().zoom(0), Some(1.0));

    r.ls.probe.log.clear();
    r.t += 1000;
    gesture_hold_begin(&mut r.ls.state, &mut r.ls.input, 2, r.t).unwrap();
    r.t += 1000;
    gesture_hold_end(&mut r.ls.state, &mut r.ls.input, true, r.t).unwrap();
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["hold_begin W 2", "hold_end 1"]);
}

#[test]
fn four_finger_pinch_zooms_around_cursor_and_swipe_up_opens_overview() {
    let mut g = rig();
    let r = &mut g.r;
    // Rig::x8() placed pointer at (1170, 740): offset (+210, +200) from screen centre (960, 540).
    r.t += 1000;
    gesture_pinch_begin(&mut r.ls.state, &mut r.ls.input, 4, r.t).unwrap();
    r.t += 1000;
    gesture_pinch_update(&mut r.ls.state, &mut r.ls.input, (0.0, 0.0), 2.0, 0.0, r.t).unwrap();
    r.t += 1000;
    gesture_pinch_end(&mut r.ls.state, &mut r.ls.input, false, r.t).unwrap();
    r.ls.pump();

    assert_eq!(r.ls.state.core().cameras().zoom(0), Some(2.0));
    assert_eq!(r.camera(), pos(105, 100));
    assert!(
        gesture_lines(&r.ls.probe.log).is_empty(),
        "4-finger pinch must not reach client: {:?}",
        r.ls.probe.log
    );

    // Add a second window far away so overview zooms out.
    let qh2 = r.ls.qh.clone();
    r.ls.probe.toplevel(&qh2, "W2", (600, 400), 20);
    r.ls.pump();
    let w2 = *r.ls.state.core().stage().z_order().last().unwrap();
    let w2_ref = infynitae_core::stage::ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: w2,
    };
    let move_fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::MoveElement {
                    element: w2_ref,
                    to: (3000.0, 2000.0),
                    stream: ShellStream::Atomic,
                },
            )
            .unwrap();
    r.ls.state.apply(&move_fx);
    r.ls.pump();

    let zoom_before = r.ls.state.core().cameras().zoom(0).unwrap();
    r.t += 1000;
    gesture_swipe_begin(&mut r.ls.state, &mut r.ls.input, 4, r.t).unwrap();
    r.t += 1000;
    gesture_swipe_update(&mut r.ls.state, &mut r.ls.input, (0.0, -50.0), r.t).unwrap();
    r.t += 1000;
    gesture_swipe_end(&mut r.ls.state, &mut r.ls.input, false, r.t).unwrap();
    r.ls.pump();

    let zoom_after = r.ls.state.core().cameras().zoom(0).unwrap();
    assert!(
        zoom_after < zoom_before,
        "4-finger swipe up should zoom out to overview ({zoom_after} vs {zoom_before})"
    );
    assert!(
        gesture_lines(&r.ls.probe.log).is_empty(),
        "4-finger swipe must not reach client: {:?}",
        r.ls.probe.log
    );
}

#[test]
fn touchpad_gestures_false_sends_three_finger_swipe_to_client() {
    let mut g = rig();
    let r = &mut g.r;
    r.ls.state.core_mut().set_touchpad_gestures(false);

    r.t += 1000;
    gesture_swipe_begin(&mut r.ls.state, &mut r.ls.input, 3, r.t).unwrap();
    r.t += 1000;
    gesture_swipe_update(&mut r.ls.state, &mut r.ls.input, (-12.5, 8.0), r.t).unwrap();
    r.t += 1000;
    gesture_swipe_end(&mut r.ls.state, &mut r.ls.input, false, r.t).unwrap();
    r.ls.pump();

    assert_eq!(r.camera(), pos(0, 0));
    assert_eq!(
        r.ls.probe.log,
        vec!["swipe_begin W 3", "swipe_update -12.5,8", "swipe_end 0",]
    );
}
