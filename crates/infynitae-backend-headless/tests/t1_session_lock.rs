//! Session lock, `ext-session-lock-v1` (2026-10-24; infynitae-wayland `lock`). Keys are driven
//! like the console backend drives them: `lock_key` first, Core only for keys the lock did not
//! take, and Core's missed releases after an unlock. While locked, no binding fires and only the
//! lock surface gets keys; after the unlock the keyboard is the focused window's again and no key
//! is stuck; a second lock is refused while the first lives.

mod common;

use common::Probe;
use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_DELETE, KEY_LEFTMETA};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::{
    self, ExtSessionLockSurfaceV1,
};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::{
    self, ExtSessionLockV1,
};

impl Dispatch<ExtSessionLockManagerV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ExtSessionLockManagerV1,
        _: <ExtSessionLockManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// The lock's number (1, 2, …) tells the probe's log which lock an event is for.
impl Dispatch<ExtSessionLockV1, u32> for Probe {
    fn event(
        st: &mut Self,
        _: &ExtSessionLockV1,
        ev: ext_session_lock_v1::Event,
        n: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            ext_session_lock_v1::Event::Locked => st.log.push(format!("lock {n} locked")),
            ext_session_lock_v1::Event::Finished => st.log.push(format!("lock {n} finished")),
            _ => {}
        }
    }
}

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

/// A key the way the console backend passes it: the lock first, Core otherwise.
fn key(r: &mut Rig, key: u32, pressed: bool) {
    r.t += 1000;
    if !r.ls.state.lock_key(key, pressed, r.t) {
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
    }
    r.ls.pump();
}

/// Super+Delete: asks the focused window to close when unlocked.
fn super_delete(r: &mut Rig) {
    key(r, KEY_LEFTMETA, true);
    key(r, KEY_DELETE, true);
    key(r, KEY_DELETE, false);
    key(r, KEY_LEFTMETA, false);
}

/// After an unlock, as the console backend does each loop: Core's missed releases.
fn unlock_releases(r: &mut Rig) {
    let keys = r.ls.state.take_unlock_releases().expect("an unlock happened");
    for k in keys {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, k, false, r.t).expect("release");
    }
    r.ls.state.finish_unlock();
    r.ls.pump();
}

/// The probe locks the session (lock number `n`) and gives the output a lock surface with a
/// buffer of the configured size; the backend's locked frame confirms the lock.
fn lock(r: &mut Rig, n: u32) -> (ExtSessionLockV1, WlSurface) {
    let (name, _, _) = r
        .ls
        .probe
        .globals
        .iter()
        .find(|(_, i, _)| i == "ext_session_lock_manager_v1")
        .cloned()
        .expect("ext_session_lock_manager_v1 is offered");
    let qh = r.ls.qh.clone();
    let manager: ExtSessionLockManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
    let lock = manager.lock(&qh, n);
    let surface = r.ls.probe.compositor.as_ref().expect("compositor").create_surface(&qh, ());
    let output = r.ls.probe.outputs[0].1.clone();
    lock.get_lock_surface(&surface, &output, &qh, ());
    r.ls.pump();
    let size = r
        .ls
        .probe
        .log
        .iter()
        .rev()
        .find_map(|l| l.strip_prefix("lock surface ").map(str::to_owned));
    if let Some((w, h)) = size.as_deref().and_then(|s| s.split_once('x')) {
        let buffer = r.ls.probe.buffer(&qh, w.parse().unwrap(), h.parse().unwrap());
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        r.ls.pump();
    }
    r.ls.state.lock_frame_drawn();
    r.ls.pump();
    (lock, surface)
}

fn focused_w() -> (Rig, usize) {
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let fx = r.ls.state.core_mut().set_focus(&mut r.ls.input, Some(w)).unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.ls.state.enable_session_lock();
    r.ls.pump();
    let idx = r.ls.probe.surfaces.iter().position(|s| s.label == "W").unwrap();
    (r, idx)
}

#[test]
fn while_locked_keys_reach_only_the_lock_surface_and_no_binding_fires() {
    let (mut r, idx) = focused_w();
    // Super+L as a user locks: Super is down (Core holds it) when the lock starts
    key(&mut r, KEY_LEFTMETA, true);
    let (lock, _surface) = lock(&mut r, 1);
    assert!(r.ls.probe.log.contains(&"lock 1 locked".to_string()), "{:?}", r.ls.probe.log);
    assert!(r.ls.state.is_locked());
    assert_eq!(r.ls.probe.log.iter().filter(|l| l.starts_with("lock surface ")).count(), 1);
    key(&mut r, KEY_LEFTMETA, false);

    let before = r.ls.probe.key_events.len();
    super_delete(&mut r);
    assert!(r.ls.probe.close_requests.is_empty(), "no binding fires while locked");
    let active = r.ls.probe.kb.active.clone().unwrap_or_default();
    assert!(active.starts_with('#'), "the lock surface has the keyboard, not {active}");
    assert!(
        r.ls.probe.key_events[before..].contains(&(KEY_DELETE, true)),
        "the lock surface gets the keys"
    );

    lock.unlock_and_destroy();
    r.ls.pump();
    unlock_releases(&mut r);
    assert!(!r.ls.state.is_locked());
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("W"), "the keyboard returns to the window");
    assert!(!r.ls.state.core().is_key_down(KEY_LEFTMETA), "Super is not stuck in Core");

    // bindings work again: Super+Delete asks W to close
    super_delete(&mut r);
    assert_eq!(r.ls.probe.close_requests, [idx]);
    assert!(r.ls.probe.kb.violations.is_empty(), "{:?}", r.ls.probe.kb.violations);
}

#[test]
fn a_second_lock_is_refused_while_the_first_lives() {
    let (mut r, _) = focused_w();
    let (first, _s1) = lock(&mut r, 1);
    // a second lock client: refused (it stops at `finished`, making no lock surface)
    let (name, _, _) = r
        .ls
        .probe
        .globals
        .iter()
        .find(|(_, i, _)| i == "ext_session_lock_manager_v1")
        .cloned()
        .unwrap();
    let qh = r.ls.qh.clone();
    let manager: ExtSessionLockManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
    let _second = manager.lock(&qh, 2);
    r.ls.pump();
    assert!(r.ls.probe.log.contains(&"lock 2 finished".to_string()), "{:?}", r.ls.probe.log);
    assert!(r.ls.state.is_locked());
    first.unlock_and_destroy();
    r.ls.pump();
    assert!(!r.ls.state.is_locked());
}
