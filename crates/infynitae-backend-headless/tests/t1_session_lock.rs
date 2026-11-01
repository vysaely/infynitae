//! Session lock, `ext-session-lock-v1` (2026-10-24; infynitae-wayland `lock`). Keys are driven
//! like the console backend drives them: `lock_key` first, Core only for keys the lock did not
//! take, and Core's missed releases after an unlock. While locked, no binding fires and only the
//! lock surface gets keys; after the unlock the keyboard is the focused window's again and no key
//! is stuck; a second lock is refused while the first lives.

mod common;

use common::Probe;
use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_C, KEY_LEFTMETA, KEY_LEFTSHIFT};
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

/// Super+Shift+C: asks the focused window to close when unlocked.
fn super_delete(r: &mut Rig) {
    key(r, KEY_LEFTMETA, true);
    key(r, KEY_LEFTSHIFT, true);
    key(r, KEY_C, true);
    key(r, KEY_C, false);
    key(r, KEY_LEFTSHIFT, false);
    key(r, KEY_LEFTMETA, false);
}

/// After an unlock, as the console backend does each loop: Core's missed releases.
fn unlock_releases(r: &mut Rig) {
    let keys =
        r.ls.state
            .take_unlock_releases()
            .expect("an unlock happened");
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
    let (name, _, _) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "ext_session_lock_manager_v1")
            .cloned()
            .expect("ext_session_lock_manager_v1 is offered");
    let qh = r.ls.qh.clone();
    let manager: ExtSessionLockManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
    let lock = manager.lock(&qh, n);
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
            .find_map(|l| l.strip_prefix("lock surface ").map(str::to_owned));
    if let Some((w, h)) = size.as_deref().and_then(|s| s.split_once('x')) {
        let buffer =
            r.ls.probe
                .buffer(&qh, w.parse().unwrap(), h.parse().unwrap());
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
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, Some(w))
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.ls.state.enable_session_lock();
    r.ls.pump();
    let idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    (r, idx)
}

#[test]
fn while_locked_keys_reach_only_the_lock_surface_and_no_binding_fires() {
    let (mut r, idx) = focused_w();
    // Super+Shift+L as a user locks: Super is down (Core holds it) when the lock starts
    key(&mut r, KEY_LEFTMETA, true);
    let (lock, _surface) = lock(&mut r, 1);
    assert!(
        r.ls.probe.log.contains(&"lock 1 locked".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(r.ls.state.is_locked());
    assert_eq!(
        r.ls.probe
            .log
            .iter()
            .filter(|l| l.starts_with("lock surface "))
            .count(),
        1
    );
    key(&mut r, KEY_LEFTMETA, false);

    let before = r.ls.probe.key_events.len();
    super_delete(&mut r);
    assert!(
        r.ls.probe.close_requests.is_empty(),
        "no binding fires while locked"
    );
    let active = r.ls.probe.kb.active.clone().unwrap_or_default();
    assert!(
        active.starts_with('#'),
        "the lock surface has the keyboard, not {active}"
    );
    assert!(
        r.ls.probe.key_events[before..].contains(&(KEY_C, true)),
        "the lock surface gets the keys"
    );

    lock.unlock_and_destroy();
    r.ls.pump();
    unlock_releases(&mut r);
    assert!(!r.ls.state.is_locked());
    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some("W"),
        "the keyboard returns to the window"
    );
    assert!(
        !r.ls.state.core().is_key_down(KEY_LEFTMETA),
        "Super is not stuck in Core"
    );

    // bindings work again: Super+Shift+C asks W to close
    super_delete(&mut r);
    assert_eq!(r.ls.probe.close_requests, [idx]);
    assert!(
        r.ls.probe.kb.violations.is_empty(),
        "{:?}",
        r.ls.probe.kb.violations
    );
}

#[test]
fn a_second_lock_is_refused_while_the_first_lives() {
    let (mut r, _) = focused_w();
    let (first, _s1) = lock(&mut r, 1);
    // a second lock client: refused (it stops at `finished`, making no lock surface)
    let (name, _, _) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "ext_session_lock_manager_v1")
            .cloned()
            .unwrap();
    let qh = r.ls.qh.clone();
    let manager: ExtSessionLockManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
    let _second = manager.lock(&qh, 2);
    r.ls.pump();
    assert!(
        r.ls.probe.log.contains(&"lock 2 finished".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(r.ls.state.is_locked());
    first.unlock_and_destroy();
    r.ls.pump();
    assert!(!r.ls.state.is_locked());
}

/// A second client, not the lock holder: it binds by the probe's global names and ignores events.
struct Intruder {
    conn: Connection,
    queue: wayland_client::EventQueue<Nobody>,
    registry: wayland_client::protocol::wl_registry::WlRegistry,
}

/// The intruder's event handler: every event is ignored.
struct Nobody;

wayland_client::delegate_noop!(Nobody: ignore wayland_client::protocol::wl_registry::WlRegistry);
wayland_client::delegate_noop!(Nobody: ignore ExtSessionLockManagerV1);
wayland_client::delegate_noop!(Nobody: ignore ExtSessionLockV1);
wayland_client::delegate_noop!(Nobody: ignore ExtSessionLockSurfaceV1);
wayland_client::delegate_noop!(Nobody: ignore wayland_client::protocol::wl_compositor::WlCompositor);
wayland_client::delegate_noop!(Nobody: ignore WlSurface);
wayland_client::delegate_noop!(Nobody: ignore wayland_client::protocol::wl_output::WlOutput);

impl Intruder {
    fn connect(r: &mut Rig) -> Intruder {
        let (server_end, client_end) = std::os::unix::net::UnixStream::pair().unwrap();
        client_end.set_nonblocking(true).unwrap();
        r.ls.display
            .handle()
            .insert_client(
                server_end,
                std::sync::Arc::new(infynitae_wayland::ClientState::default()),
            )
            .unwrap();
        let conn = Connection::from_socket(client_end).unwrap();
        let queue = conn.new_event_queue();
        let registry = conn.display().get_registry(&queue.handle(), ());
        Intruder {
            conn,
            queue,
            registry,
        }
    }

    fn bind<I: wayland_client::Proxy + 'static>(&self, r: &Rig, interface: &str, version: u32) -> I
    where
        Nobody: Dispatch<I, ()>,
    {
        let (name, _, _) =
            r.ls.probe
                .globals
                .iter()
                .find(|(_, i, _)| i == interface)
                .cloned()
                .unwrap();
        self.registry.bind(name, version, &self.queue.handle(), ())
    }

    /// Sends what it asked for and lets the server handle it.
    fn send(&mut self, r: &mut Rig) {
        let _ = self.conn.flush();
        r.ls.pump();
        if let Some(guard) = self.conn.prepare_read() {
            let _ = guard.read();
        }
        let _ = self.queue.dispatch_pending(&mut Nobody);
    }
}

#[test]
fn another_client_cannot_unlock_through_a_refused_lock() {
    let (mut r, _) = focused_w();
    let (first, _s1) = lock(&mut r, 1);
    let mut x = Intruder::connect(&mut r);
    let manager: ExtSessionLockManagerV1 = x.bind(&r, "ext_session_lock_manager_v1", 1);
    let refused = manager.lock(&x.queue.handle(), ());
    x.send(&mut r);
    // Smithay posts invalid_unlock on the refused lock but still calls the handler's unlock
    refused.unlock_and_destroy();
    x.send(&mut r);
    assert!(
        r.ls.state.is_locked(),
        "a refused lock unlocked the session"
    );
    assert!(r.ls.state.take_unlock_releases().is_none());
    // the holder still unlocks
    first.unlock_and_destroy();
    r.ls.pump();
    assert!(!r.ls.state.is_locked());
}

#[test]
fn another_client_cannot_replace_the_lock_surface() {
    let (mut r, _) = focused_w();
    let (_first, _s1) = lock(&mut r, 1);
    let output_id = r.ls.output();
    let holder_surface =
        r.ls.state
            .lock_surface(output_id)
            .expect("the holder's lock surface");
    let mut x = Intruder::connect(&mut r);
    let manager: ExtSessionLockManagerV1 = x.bind(&r, "ext_session_lock_manager_v1", 1);
    let compositor: wayland_client::protocol::wl_compositor::WlCompositor =
        x.bind(&r, "wl_compositor", 4);
    let output: wayland_client::protocol::wl_output::WlOutput = x.bind(&r, "wl_output", 4);
    let refused = manager.lock(&x.queue.handle(), ());
    let surface = compositor.create_surface(&x.queue.handle(), ());
    refused.get_lock_surface(&surface, &output, &x.queue.handle(), ());
    x.send(&mut r);
    assert_eq!(
        r.ls.state.lock_surface(output_id),
        Some(holder_surface),
        "the lock surface was replaced"
    );
}

/// `--locked`: the session is locked before any lock client exists. No binding fires and no
/// window gets a key; the lock screen, when it comes, takes the lock at once and is the only one
/// that can unlock.
#[test]
fn a_session_started_locked_stays_locked_until_a_lock_client_unlocks() {
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, Some(w))
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    let idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    r.ls.state.start_locked();
    r.ls.pump();
    assert!(r.ls.state.is_locked());

    let before = r.ls.probe.key_events.len();
    super_delete(&mut r);
    assert!(
        r.ls.probe.close_requests.is_empty(),
        "no binding fires before the lock screen"
    );
    assert_eq!(
        r.ls.probe.key_events.len(),
        before,
        "no window gets a key: {:?}",
        &r.ls.probe.key_events[before..]
    );
    assert_ne!(r.ls.probe.kb.active.as_deref(), Some("W"));

    let (lock, _surface) = lock(&mut r, 1);
    assert!(
        r.ls.probe.log.contains(&"lock 1 locked".to_string()),
        "the lock screen takes the lock: {:?}",
        r.ls.probe.log
    );
    key(&mut r, KEY_C, true);
    key(&mut r, KEY_C, false);
    let active = r.ls.probe.kb.active.clone().unwrap_or_default();
    assert!(
        active.starts_with('#'),
        "the lock surface has the keyboard, not {active}"
    );

    lock.unlock_and_destroy();
    r.ls.pump();
    unlock_releases(&mut r);
    assert!(!r.ls.state.is_locked(), "unlocked by the lock screen");
    super_delete(&mut r);
    assert_eq!(r.ls.probe.close_requests, [idx], "bindings work again");
}

/// M4 (owner, 2026-10-31): when the lock holder's client dies while locked, the session stays
/// locked and the compositor starts the locker itself, again every few seconds until one holds
/// the lock; a living holder is never replaced.
#[test]
fn a_dead_locker_is_started_again_by_the_compositor() {
    let (mut r, _) = focused_w();
    let mut x = Intruder::connect(&mut r);
    let manager: ExtSessionLockManagerV1 = x.bind(&r, "ext_session_lock_manager_v1", 1);
    let _held = manager.lock(&x.queue.handle(), ());
    x.send(&mut r);
    assert!(r.ls.state.is_locked());
    let t0 = std::time::Instant::now();
    assert!(!r.ls.state.restart_dead_locker(t0), "the holder lives");
    drop(x);
    r.ls.pump();
    assert!(r.ls.state.is_locked(), "its death unlocks nothing");
    assert!(r.ls.state.restart_dead_locker(t0));
    assert!(
        !r.ls
            .state
            .restart_dead_locker(t0 + std::time::Duration::from_secs(1))
    );
    assert!(
        r.ls.state
            .restart_dead_locker(t0 + std::time::Duration::from_secs(6))
    );
    assert_eq!(r.ls.state.locker_restarts(), 2);
}
