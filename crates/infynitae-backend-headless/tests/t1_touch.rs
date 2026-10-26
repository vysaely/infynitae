//! Touch (`wl_touch`, Core's `touch`) with a real client: a finger on a window reaches it at its
//! surface-local position; the window keeps that finger as it moves, even past the window's
//! edge, until it lifts; a finger on empty canvas reaches no client; a tap focuses the window;
//! a cancel gives up the fingers. The seat offers touch only once a touchscreen is there.

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
