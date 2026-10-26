//! Touch while locked (`lock_touch`), with a real client, as the console backend calls it before
//! Core: a finger reaches the lock surface of the screen it touched at that point (the lock
//! screen's on-screen keyboard), and no window sees it; a finger held on a window when the lock
//! starts is given up; a finger that landed on the lock surface stays with it past the unlock.
//! Unlocked, touches go to Core as before.

mod common;

use common::Probe;
use common::input::*;
use infynitae_core::PhysicalEvent;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_touch::{self, WlTouch};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::{
    self, ExtSessionLockSurfaceV1,
};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::ExtSessionLockV1;

wayland_client::delegate_noop!(Probe: ignore ExtSessionLockManagerV1);
wayland_client::delegate_noop!(Probe: ignore ExtSessionLockV1);

impl Dispatch<ExtSessionLockSurfaceV1, ()> for Probe {
    fn event(
        st: &mut Self,
        s: &ExtSessionLockSurfaceV1,
        ev: ext_session_lock_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = ev
        {
            s.ack_configure(serial);
            st.log.push(format!("lock surface {width}x{height}"));
        }
    }
}

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
                .map_or("lock".to_string(), |p| p.label.to_string())
        };
        let line = match ev {
            wl_touch::Event::Down {
                surface, id, x, y, ..
            } => format!("down {id} {} {x:.1},{y:.1}", label(st, &surface)),
            wl_touch::Event::Motion { id, x, y, .. } => format!("motion {id} {x:.1},{y:.1}"),
            wl_touch::Event::Up { id, .. } => format!("up {id}"),
            wl_touch::Event::Cancel => "cancel".into(),
            _ => return,
        };
        st.log.push(line);
    }
}

/// W mapped, a touchscreen, the lock offered.
fn rig() -> (Rig, WlTouch) {
    let mut r = Rig::x8();
    r.ls.state.enable_touch();
    r.ls.state.enable_session_lock();
    r.ls.pump();
    let qh = r.ls.qh.clone();
    let seat: WlSeat = r.ls.probe.seat.clone().expect("a seat");
    let touch = seat.get_touch(&qh, ());
    r.ls.pump();
    (r, touch)
}

/// The session locked, the lock surface drawn.
fn lock(r: &mut Rig) -> ExtSessionLockV1 {
    let (name, _, _) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "ext_session_lock_manager_v1")
            .cloned()
            .expect("the lock is offered");
    let qh = r.ls.qh.clone();
    let manager: ExtSessionLockManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
    let lock = manager.lock(&qh, ());
    let surface =
        r.ls.probe
            .compositor
            .as_ref()
            .expect("compositor")
            .create_surface(&qh, ());
    let output = r.ls.probe.outputs[0].1.clone();
    lock.get_lock_surface(&surface, &output, &qh, ());
    r.ls.pump();
    let size =
        r.ls.probe
            .log
            .iter()
            .rev()
            .find_map(|l| l.strip_prefix("lock surface ").map(str::to_owned))
            .expect("configured");
    let (w, h) = size.split_once('x').unwrap();
    let buffer =
        r.ls.probe
            .buffer(&qh, w.parse().unwrap(), h.parse().unwrap());
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    r.ls.pump();
    r.ls.state.lock_frame_drawn();
    r.ls.pump();
    assert!(r.ls.state.is_locked());
    lock
}

/// As the console backend does: the lock first, else Core.
fn touch(r: &mut Rig, ev: PhysicalEvent) {
    if !r.ls.state.lock_touch(&ev) {
        let port = &mut r.ls.input;
        r.ls.state
            .with_effects(|core, fx| core.physical_input(port, ev, fx))
            .expect("touch");
    }
    if !r.ls.state.lock_touch(&PhysicalEvent::TouchFrame) {
        let port = &mut r.ls.input;
        let _ =
            r.ls.state
                .with_effects(|core, fx| core.physical_input(port, PhysicalEvent::TouchFrame, fx));
    }
    r.ls.pump();
}

fn down(r: &mut Rig, slot: u32, p: (f64, f64)) {
    r.t += 1000;
    let (output, time_us) = (r.ls.output(), r.t);
    touch(
        r,
        PhysicalEvent::TouchDown {
            slot,
            output,
            p,
            time_us,
        },
    );
}

fn up(r: &mut Rig, slot: u32) {
    r.t += 1000;
    let time_us = r.t;
    touch(r, PhysicalEvent::TouchUp { slot, time_us });
}

/// A point on W (output logical).
fn on_w(r: &mut Rig) -> (f64, f64) {
    let output = r.ls.output();
    for y in (0..1080).step_by(40) {
        for x in (0..1920).step_by(40) {
            let p = (f64::from(x) + 20.0, f64::from(y) + 20.0);
            let pt = infynitae_core::geom::OutputLogicalPoint { x: p.0, y: p.1 };
            if r.ls.state.core_mut().hit_test(output, pt).is_some() {
                return p;
            }
        }
    }
    panic!("W is on the screen");
}

#[test]
fn locked_a_finger_reaches_the_lock_surface_and_no_window() {
    let (mut r, _touch) = rig();
    let p = on_w(&mut r);
    let _lock = lock(&mut r);
    r.ls.probe.log.clear();
    down(&mut r, 1, p);
    up(&mut r, 1);
    let log = &r.ls.probe.log;
    let expected = format!("down 1 lock {:.1},{:.1}", p.0, p.1);
    assert!(log.contains(&expected), "{expected}: {log:?}");
    assert!(
        !log.iter().any(|l| l.contains(" W ")),
        "W saw nothing: {log:?}"
    );
    assert!(log.contains(&"up 1".to_string()), "{log:?}");
}

#[test]
fn a_finger_on_a_window_is_given_up_at_the_lock() {
    let (mut r, _touch) = rig();
    let p = on_w(&mut r);
    down(&mut r, 1, p);
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 1 W")),
        "{:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    let _lock = lock(&mut r);
    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "the lock gives the window's finger up: {:?}",
        r.ls.probe.log
    );
    // the finger moving on, or lifting, while locked reaches no window
    r.ls.probe.log.clear();
    up(&mut r, 1);
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("up")),
        "{:?}",
        r.ls.probe.log
    );
}

#[test]
fn a_lock_finger_stays_with_the_lock_past_the_unlock_then_touch_is_core_s() {
    let (mut r, _touch) = rig();
    let p = on_w(&mut r);
    let lock = lock(&mut r);
    down(&mut r, 1, (10.0, 10.0));
    lock.unlock_and_destroy();
    r.ls.pump();
    assert!(!r.ls.state.is_locked());
    r.ls.probe.log.clear();
    up(&mut r, 1);
    assert!(
        r.ls.probe.log.contains(&"up 1".to_string()),
        "the lift ends the lock surface's finger: {:?}",
        r.ls.probe.log
    );
    down(&mut r, 2, p);
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 2 W")),
        "unlocked, touch is Core's again: {:?}",
        r.ls.probe.log
    );
}
