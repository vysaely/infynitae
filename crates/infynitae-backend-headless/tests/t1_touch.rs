//! Touch (`wl_touch`, Core's `touch`) with a real client: a finger on a window reaches it at its
//! surface-local position; the window keeps that finger as it moves, even past the window's
//! edge, until it lifts; a finger on empty canvas reaches no client; a tap focuses the window;
//! a cancel gives up the fingers; fingers on empty canvas drag it and pinch-zoom it; a second
//! finger takes the first from a window (cancelled there) and the two move the canvas; a
//! two-finger double tap recenters like Super+Space. The seat offers touch only once a
//! touchscreen is there.

mod common;

use common::Probe;
use common::input::*;
use infynitae_core::PhysicalEvent;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_touch::{self, WlTouch};
use wayland_client::{Connection, Dispatch, QueueHandle};

impl Dispatch<WlTouch, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &WlTouch,
        ev: wl_touch::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let label = |st: &Probe, s: &wayland_client::protocol::wl_surface::WlSurface| {
            st.surfaces
                .iter()
                .find(|p| p.wl == *s)
                .map_or("?".to_string(), |p| p.label.to_string())
        };
        let line = match ev {
            wl_touch::Event::Down {
                surface, id, x, y, ..
            } => {
                format!("down {id} {} {x:.1},{y:.1}", label(st, &surface))
            }
            wl_touch::Event::Motion { id, x, y, .. } => format!("motion {id} {x:.1},{y:.1}"),
            wl_touch::Event::Up { id, .. } => format!("up {id}"),
            wl_touch::Event::Frame => "frame".into(),
            wl_touch::Event::Cancel => "cancel".into(),
            _ => return,
        };
        st.log.push(line);
    }
}

/// W mapped and a touchscreen: the client's wl_touch.
fn rig() -> (Rig, WlTouch) {
    let mut r = Rig::x8();
    r.ls.state.enable_touch();
    r.ls.pump();
    let qh = r.ls.qh.clone();
    let seat: WlSeat = r.ls.probe.seat.clone().expect("a seat");
    let touch = seat.get_touch(&qh, ());
    r.ls.pump();
    r.ls.probe.log.clear();
    (r, touch)
}

fn touch(r: &mut Rig, ev: PhysicalEvent) {
    let port = &mut r.ls.input;
    r.ls.state
        .with_effects(|core, fx| core.physical_input(port, ev, fx))
        .expect("touch");
    r.ls.pump();
}

fn down(r: &mut Rig, slot: u32, p: (f64, f64)) {
    r.t += 1000;
    let output = r.ls.output();
    let time_us = r.t;
    touch(
        r,
        PhysicalEvent::TouchDown {
            slot,
            output,
            p,
            time_us,
        },
    );
    touch(r, PhysicalEvent::TouchFrame);
}

fn motion(r: &mut Rig, slot: u32, p: (f64, f64)) {
    r.t += 1000;
    let time_us = r.t;
    touch(r, PhysicalEvent::TouchMotion { slot, p, time_us });
    touch(r, PhysicalEvent::TouchFrame);
}

fn up(r: &mut Rig, slot: u32) {
    r.t += 1000;
    let time_us = r.t;
    touch(r, PhysicalEvent::TouchUp { slot, time_us });
    touch(r, PhysicalEvent::TouchFrame);
}

/// A point on W (output logical; found by hit-testing the screen) and where in W's surface it is.
fn on_w(r: &mut Rig) -> ((f64, f64), (f64, f64)) {
    let output = r.ls.output();
    let q = infynitae_core::geom::Q16_ONE as f64;
    for y in (0..1080).step_by(40) {
        for x in (0..1920).step_by(40) {
            let p = (f64::from(x), f64::from(y));
            let pt = infynitae_core::geom::OutputLogicalPoint { x: p.0, y: p.1 };
            if let Some(hit) = r.ls.state.core_mut().hit_test(output, pt) {
                // well inside, so a small motion stays on it
                let p = (p.0 + 20.0, p.1 + 20.0);
                let pt = infynitae_core::geom::OutputLogicalPoint { x: p.0, y: p.1 };
                let hit = r.ls.state.core_mut().hit_test(output, pt).unwrap_or(hit);
                return (p, (hit.local.x as f64 / q, hit.local.y as f64 / q));
            }
        }
    }
    panic!("W is on the screen");
}

#[test]
fn a_finger_on_a_window_reaches_it_and_stays_with_it_until_it_lifts() {
    let (mut r, _touch) = rig();
    let (p, local) = on_w(&mut r);
    down(&mut r, 0, p);
    assert!(
        r.ls.probe
            .log
            .contains(&format!("down 0 W {:.1},{:.1}", local.0, local.1)),
        "{:?}",
        r.ls.probe.log
    );
    assert!(r.ls.probe.log.contains(&"frame".to_string()));

    // moving 3000 px right: far past W's edge, still W's finger, in W's coordinates
    let zoom = r.ls.state.core().view(r.ls.output()).unwrap().zoom;
    r.ls.probe.log.clear();
    motion(&mut r, 0, (p.0 + 30.0, p.1 + 20.0));
    let want = (local.0 + 30.0 / zoom, local.1 + 20.0 / zoom);
    assert!(
        r.ls.probe
            .log
            .contains(&format!("motion 0 {:.1},{:.1}", want.0, want.1)),
        "zoom {zoom}: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    motion(&mut r, 0, (p.0 + 1500.0, p.1));
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("motion 0 ")),
        "outside W: still W's: {:?}",
        r.ls.probe.log
    );

    up(&mut r, 0);
    assert!(
        r.ls.probe.log.contains(&"up 0".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    // lifted: a motion in that slot goes nowhere
    r.ls.probe.log.clear();
    motion(&mut r, 0, p);
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("motion")),
        "{:?}",
        r.ls.probe.log
    );
}

#[test]
fn a_finger_on_empty_canvas_reaches_no_client() {
    let (mut r, _touch) = rig();
    down(&mut r, 1, (1800.0, 1000.0));
    motion(&mut r, 1, (1700.0, 900.0));
    up(&mut r, 1);
    assert!(
        !r.ls
            .probe
            .log
            .iter()
            .any(|l| l.starts_with("down") || l.starts_with("motion") || l.starts_with("up")),
        "{:?}",
        r.ls.probe.log
    );
}

#[test]
fn a_tap_focuses_the_window_and_a_cancel_gives_up_the_fingers() {
    let (mut r, _touch) = rig();
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, None)
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    assert_ne!(r.ls.probe.kb.active.as_deref(), Some("W"));
    let (p, _) = on_w(&mut r);
    down(&mut r, 0, p);
    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some("W"),
        "a tap is an explicit action: W has the keyboard"
    );
    touch(&mut r, PhysicalEvent::TouchCancel);
    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    motion(&mut r, 0, p);
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("motion")),
        "cancelled: nothing more: {:?}",
        r.ls.probe.log
    );
}

/// The camera of the rig's output: its centre (Canvas px) and zoom.
fn camera(r: &Rig) -> ((f64, f64), f64) {
    let core = r.ls.state.core();
    let cam = core
        .cameras()
        .camera_for_output(r.ls.output())
        .expect("a camera");
    let c = core.cameras().center(cam).expect("a centre");
    let q = infynitae_core::geom::Q16_ONE as f64;
    (
        (c.x as f64 / q, c.y as f64 / q),
        core.cameras().zoom(cam).expect("a zoom"),
    )
}

/// One finger on empty canvas drags it: the camera moves against the finger, the zoom stays, and
/// the lift ends the drag (a later finger elsewhere moves nothing until it moves itself).
#[test]
fn a_finger_on_empty_canvas_drags_the_canvas() {
    let (mut r, _touch) = rig();
    let ((x0, y0), z0) = camera(&r);
    down(&mut r, 1, (1800.0, 1000.0));
    motion(&mut r, 1, (1700.0, 900.0));
    let ((x, y), z) = camera(&r);
    assert_eq!(z, z0);
    assert!(
        (x - (x0 + 100.0 / z0)).abs() < 0.01 && (y - (y0 + 100.0 / z0)).abs() < 0.01,
        "the canvas followed the finger: {x0},{y0} -> {x},{y}"
    );
    up(&mut r, 1);
    down(&mut r, 2, (100.0, 100.0));
    assert_eq!(camera(&r), ((x, y), z), "a new finger alone moves nothing");
}

/// Two fingers on empty canvas pinch: twice their spread is twice the zoom, and the Canvas point
/// between them stays between them.
#[test]
fn two_fingers_on_empty_canvas_pinch_zoom_around_their_midpoint() {
    let (mut r, _touch) = rig();
    let ((x0, y0), z0) = camera(&r);
    let (mid, extent) = ((1700.0, 950.0), (1920.0, 1080.0));
    let under = |((x, y), z): ((f64, f64), f64)| {
        (
            x + (mid.0 - extent.0 / 2.0) / z,
            y + (mid.1 - extent.1 / 2.0) / z,
        )
    };
    let before = under(((x0, y0), z0));
    down(&mut r, 1, (1650.0, 950.0));
    down(&mut r, 2, (1750.0, 950.0));
    motion(&mut r, 1, (1600.0, 950.0));
    motion(&mut r, 2, (1800.0, 950.0));
    let after = camera(&r);
    assert!(
        (after.1 - 2.0 * z0).abs() < 1e-9,
        "zoom {z0} -> {}",
        after.1
    );
    let now = under(after);
    assert!(
        (now.0 - before.0).abs() < 0.01 && (now.1 - before.1).abs() < 0.01,
        "{before:?} -> {now:?}"
    );
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("down")),
        "no client saw the fingers: {:?}",
        r.ls.probe.log
    );
}

/// A second finger next to one on W: W's touch is cancelled, and the two pinch-zoom the canvas
/// (owner: "pinch to zoom even if the fingers are on an app"); W sees nothing more of them.
#[test]
fn a_second_finger_on_a_window_takes_both_to_the_canvas() {
    let (mut r, _touch) = rig();
    let (_, z0) = camera(&r);
    let (p, _) = on_w(&mut r);
    down(&mut r, 0, p);
    assert!(r.ls.probe.log.iter().any(|l| l.starts_with("down 0 W")));
    down(&mut r, 1, (p.0 + 100.0, p.1));
    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "W's touch is cancelled: {:?}",
        r.ls.probe.log
    );
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("down 1")),
        "the second finger is no touch for W: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    motion(&mut r, 0, (p.0 - 50.0, p.1));
    motion(&mut r, 1, (p.0 + 150.0, p.1));
    assert!(
        (camera(&r).1 - 2.0 * z0).abs() < 1e-9,
        "twice the spread, twice the zoom: {z0} -> {}",
        camera(&r).1
    );
    up(&mut r, 0);
    up(&mut r, 1);
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("motion") || l.starts_with("up")),
        "W saw nothing more: {:?}",
        r.ls.probe.log
    );
}

/// Two quick two-finger taps on W: W is focused and centred at zoom 1.0, as Super+Space does.
#[test]
fn a_two_finger_double_tap_on_a_window_recenters_it() {
    let (mut r, _touch) = rig();
    // zoomed out first (a pinch on empty canvas), and W not focused
    down(&mut r, 1, (1600.0, 950.0));
    down(&mut r, 2, (1800.0, 950.0));
    motion(&mut r, 1, (1650.0, 950.0));
    motion(&mut r, 2, (1750.0, 950.0));
    up(&mut r, 1);
    up(&mut r, 2);
    assert!(camera(&r).1 < 0.99, "zoomed out: {}", camera(&r).1);
    let fx = r.ls.state.core_mut().set_focus(&mut r.ls.input, None).unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    let (p, _) = on_w(&mut r);
    for _ in 0..2 {
        down(&mut r, 3, p);
        down(&mut r, 4, (p.0 + 10.0, p.1));
        up(&mut r, 3);
        up(&mut r, 4);
    }
    assert_eq!(camera(&r).1, 1.0, "zoom 1.0");
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("W"), "W focused");
}
