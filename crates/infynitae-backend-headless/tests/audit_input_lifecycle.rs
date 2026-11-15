//! Reproducers for bugs found in the input-lifecycle audit (`AUDIT_INPUT_LIFECYCLE`).
//! Every test here is marked `#[ignore = "AUDIT_INPUT_LIFECYCLE: ..."]` and asserts the expected
//! behaviour; against unpatched code, every test fails to prove its finding.

mod common;

use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Duration;

use common::input::*;
use common::{Lockstep, PopupSpec, Probe, delta_to, hit_under_pointer, output};
use infynitae_backend_headless::{
    keyboard_key, pointer_button, pointer_motion, set_focus,
};
use infynitae_core::PhysicalEvent;
use infynitae_core::input::bindings::{BTN_LEFT, BTN_MIDDLE};
use infynitae_core::input::keys::{KEY_K, KEY_LEFTMETA, KEY_TAB};
use infynitae_wayland::ClientState;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::protocol::wl_touch::{self, WlTouch};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::{
    self, ExtSessionLockSurfaceV1,
};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::{
    self, ExtSessionLockV1,
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::Layer,
    zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity},
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

impl Dispatch<WlTouch, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &WlTouch,
        ev: wl_touch::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let label = |st: &Probe, s: &WlSurface| {
            st.surfaces
                .iter()
                .find(|p| p.wl == *s)
                .map_or("?".to_string(), |p| p.label.to_string())
        };
        let line = match ev {
            wl_touch::Event::Down {
                surface, id, x, y, ..
            } => format!("down {id} {} {x:.1},{y:.1}", label(st, &surface)),
            wl_touch::Event::Motion { id, x, y, .. } => format!("motion {id} {x:.1},{y:.1}"),
            wl_touch::Event::Up { id, .. } => format!("up {id}"),
            wl_touch::Event::Frame => "frame".into(),
            wl_touch::Event::Cancel => "cancel".into(),
            _ => return,
        };
        st.log.push(line);
    }
}

struct Other {
    conn: Connection,
    queue: EventQueue<Probe>,
    probe: Probe,
    qh: QueueHandle<Probe>,
    registry: WlRegistry,
}

impl Other {
    fn connect(ls: &mut Lockstep) -> Other {
        let (server_end, client_end) = UnixStream::pair().expect("socketpair");
        client_end.set_nonblocking(true).expect("non-blocking");
        ls.display
            .handle()
            .insert_client(server_end, Arc::new(ClientState::default()))
            .expect("insert second client");
        let conn = Connection::from_socket(client_end).expect("connection");
        let queue = conn.new_event_queue();
        let qh = queue.handle();
        let registry = conn.display().get_registry(&qh, ());
        Other {
            conn,
            queue,
            probe: Probe::default(),
            qh,
            registry,
        }
    }

    fn drive(&mut self, ls: &mut Lockstep) {
        let mut idle = 0;
        for _ in 0..10_000 {
            ls.pump();
            self.conn.flush().expect("flush");
            if let Some(guard) = self.conn.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("second client read: {e}"),
                }
            }
            let handled = self
                .queue
                .dispatch_pending(&mut self.probe)
                .expect("second client dispatch");
            self.conn.flush().expect("flush");
            let served = ls
                .display
                .dispatch_clients(&mut ls.state)
                .expect("server dispatch");
            ls.display.flush_clients().expect("server flush");
            if handled == 0 && served == 0 {
                idle += 1;
                if idle >= 3 {
                    return;
                }
            } else {
                idle = 0;
            }
        }
        panic!("exchange did not settle");
    }
}

fn send_key_lock_aware(r: &mut Rig, key: u32, pressed: bool) {
    r.t += 1000;
    if !r.ls.state.lock_key(key, pressed, r.t) {
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
    }
    r.ls.pump();
}

/// Maps `start_locked()` / `lock_now()` while an app holds an active `xdg_popup` grab, then
/// connects the lock client and types the first keystroke. Because `start_locked()` sets
/// `l.locked = true` before `SessionLockHandler::lock` runs (`if l.locked { confirmation.lock(); return; }`),
/// `popup_grab` is never ungrabbed, `forwarded_pressed_keys` is never cleared, and `lock_key` calls
/// `kb.set_focus(lock_surface)` before `keyboard_unset_grab()`, so `PopupKeyboardGrab::set_focus`
/// rejects the focus change and the first keystroke after locking hits the app's popup.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: start_locked/lock_now skips popup cleanup and lock_key routes first key to popup"]
fn lock_now_with_popup_grab_routes_first_key_to_popup_and_leaves_held_key() {
    let mut r = Rig::x8();
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    let qh = r.ls.qh.clone();
    let _p = r.ls.probe.popup_with_grab(
        &qh,
        "P",
        0,
        PopupSpec {
            content: (200, 150),
            shadow: 0,
            anchor_rect: (20, 20, 100, 40),
            slide: false,
        },
    );
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("P"));

    // Simulate a modifier held when lock_now / start_locked triggers.
    send_key_lock_aware(&mut r, KEY_LEFTMETA, true);
    let kb = r.ls.state.seat().get_keyboard().unwrap();
    assert!(
        kb.pressed_keys()
            .iter()
            .any(|c| c.raw() == KEY_LEFTMETA + 8)
    );

    // `lock_now()` calls `start_locked()` then spawns the locker; `start_locked()` exercises the
    // exact same compositor lock state without spawning an external locker process.
    r.ls.state.start_locked();
    r.ls.pump();

    // Lock client connects on its own Wayland connection and maps its lock surface
    // (goes through `SessionLockHandler::lock` while `l.locked == true`).
    let mut locker = Other::connect(&mut r.ls);
    locker.drive(&mut r.ls);
    locker.probe.bind_basic(&locker.registry, &locker.qh);
    locker.drive(&mut r.ls);
    let (mgr_name, _, _) = locker
        .probe
        .globals
        .iter()
        .find(|(_, i, _)| i == "ext_session_lock_manager_v1")
        .cloned()
        .expect("ext_session_lock_manager_v1");
    let manager: ExtSessionLockManagerV1 = locker.registry.bind(mgr_name, 1, &locker.qh, ());
    let lock = manager.lock(&locker.qh, 1);
    let surface = locker
        .probe
        .compositor
        .as_ref()
        .expect("compositor")
        .create_surface(&locker.qh, ());
    let output = locker.probe.outputs[0].1.clone();
    let _lock_surface = lock.get_lock_surface(&surface, &output, &locker.qh, ());
    locker.drive(&mut r.ls);
    let size = locker
        .probe
        .log
        .iter()
        .rev()
        .find_map(|l| l.strip_prefix("lock surface ").map(str::to_owned))
        .expect("lock surface configure");
    let (w, h) = size.split_once('x').unwrap();
    let buffer = locker
        .probe
        .buffer(&locker.qh, w.parse().unwrap(), h.parse().unwrap());
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    locker.drive(&mut r.ls);
    r.ls.state.lock_frame_drawn();
    locker.drive(&mut r.ls);
    assert!(r.ls.state.is_locked());

    // Releasing Super while locked must forget it from Smithay's forwarded pressed keys too,
    // and popup grab must already have been dismissed.
    r.t += 1000;
    if !r.ls.state.lock_key(KEY_LEFTMETA, false, r.t) {
        keyboard_key(&mut r.ls.state, &mut r.ls.input, KEY_LEFTMETA, false, r.t).expect("key");
    }
    locker.drive(&mut r.ls);
    let before = r.ls.probe.key_events.len();
    locker.probe.key_events.clear();

    // First password character typed while locked: must go ONLY to the lock surface, never `P`.
    for pressed in [true, false] {
        r.t += 1000;
        if !r.ls.state.lock_key(KEY_K, pressed, r.t) {
            keyboard_key(&mut r.ls.state, &mut r.ls.input, KEY_K, pressed, r.t).expect("key");
        }
        locker.drive(&mut r.ls);
    }

    let app_events = &r.ls.probe.key_events[before..];
    let lock_events = &locker.probe.key_events;
    let dismissed = r.ls.probe.popup_dones.contains(&"P");
    assert!(
        dismissed && app_events.is_empty() && *lock_events == [(KEY_K, true), (KEY_K, false)],
        "lock_now/start_locked routed first key to popup P: dismissed={dismissed}, app_events={app_events:?}, lock_events={lock_events:?}, app_kb={:?}, lock_violations={:?}",
        r.ls.probe.kb.active,
        locker.probe.kb.violations
    );
}

/// Mapping an `Exclusive` layer surface (e.g., launcher / control menu) emits
/// `Effect::LayerKeyboardFocus`, which calls `kb.set_focus(Some(layer))` without tearing down
/// `popup_grab`. `PopupKeyboardGrab::set_focus` drops the focus change, leaving keyboard input
/// stuck on the popup behind the layer panel.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: Effect::LayerKeyboardFocus skips popup grab teardown"]
fn layer_keyboard_focus_does_not_dismiss_popup_grab() {
    let mut r = Rig::x8();
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    let qh = r.ls.qh.clone();
    let _p = r.ls.probe.popup_with_grab(
        &qh,
        "P",
        0,
        PopupSpec {
            content: (200, 150),
            shadow: 0,
            anchor_rect: (20, 20, 100, 40),
            slide: false,
        },
    );
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("P"));

    r.ls.bind_layer_shell();
    let l = r.ls.probe.layer_surface(
        &qh,
        "L",
        Layer::Overlay,
        Anchor::Top | Anchor::Left,
        (300, 120),
        0,
        KeyboardInteractivity::Exclusive,
    );
    r.ls.pump();
    r.ls.probe.draw(&qh, l);
    r.ls.pump();
    r.frame();

    let before = r.ls.probe.kb_log.len();
    r.t += 1000;
    keyboard_key(&mut r.ls.state, &mut r.ls.input, KEY_K, true, r.t).expect("press");
    r.t += 1000;
    keyboard_key(&mut r.ls.state, &mut r.ls.input, KEY_K, false, r.t).expect("release");
    r.ls.pump();

    let dismissed = r.ls.probe.popup_dones.contains(&"P");
    assert!(
        dismissed && r.ls.probe.kb.active.as_deref() == Some("L"),
        "Exclusive layer L did not dismiss popup P or take keyboard: dismissed={dismissed}, kb_active={:?}, key_events_after={:?}, kb_log={:?}",
        r.ls.probe.kb.active,
        r.ls.probe.key_events,
        &r.ls.probe.kb_log[before..]
    );
}

/// Pressing `Super`, triggering `Super+Tab` (focus switch via `K3`), and releasing `Super`
/// sends `wl_keyboard.modifiers(Mod4)` to client A and then `wl_keyboard.leave(A)` with NO
/// `wl_keyboard.modifiers(0)` and NO `wl_keyboard.key(125, released)`. Client A's last wire
/// modifier state remains `Mod4` (`0x40`).
#[test]
fn super_tab_leaves_super_depressed_on_leaving_client_wire() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "A", (400, 300), 0);
    ls.pump();
    ls.probe.draw(&qh, w);
    ls.pump();
    let ew = *ls.state.core().stage().mapped_elements().last().unwrap();

    let mut b = Other::connect(&mut ls);
    b.drive(&mut ls);
    b.probe.bind_basic(&b.registry, &b.qh);
    b.drive(&mut ls);
    let _bi = b.probe.toplevel(&b.qh, "B", (400, 300), 0);
    b.drive(&mut ls);
    let eb = ls.state.elements().into_iter().find(|id| *id != ew).unwrap();

    set_focus(&mut ls.state, &mut ls.input, Some(ew)).unwrap();
    b.drive(&mut ls);
    assert_eq!(ls.probe.kb.active.as_deref(), Some("A"));

    ls.probe.kb_log.clear();
    let mut t = 10_000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, t).unwrap();
    b.drive(&mut ls);
    assert!(
        ls.probe
            .kb_log
            .iter()
            .any(|l| l.starts_with("wl_keyboard.modifiers dep=0x40")),
        "expected Mod4 depressed on client A: {:?}",
        ls.probe.kb_log
    );

    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_TAB, true, t).unwrap();
    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_TAB, false, t).unwrap();
    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, t).unwrap();
    b.drive(&mut ls);

    let last_mod_idx = ls
        .probe
        .kb_log
        .iter()
        .rposition(|l| l.starts_with("wl_keyboard.modifiers "));
    let saw_mod_clear = last_mod_idx
        .is_some_and(|i| ls.probe.kb_log[i].starts_with("wl_keyboard.modifiers dep=0x0 "));
    let saw_super_release = ls
        .probe
        .kb_log
        .iter()
        .any(|l| l == "wl_keyboard.key 125 released");
    assert!(
        saw_mod_clear || saw_super_release,
        "client A saw Mod4 press + leave with no modifier clear and no Super release: {:?}",
        ls.probe.kb_log
    );
    assert_eq!(eb, ls.state.core().focus().target().unwrap());
}

/// Client A owns a `BTN_LEFT` press (`ClickGrab`) and calls `wl_data_device.start_drag`.
/// `refuse_client_drags` replaces `DnDGrab` with `PanDismissedPopupGrab`, whose next motion
/// unsets the grab to `DefaultGrab` while `BTN_LEFT` is still pressed. Moving onto client B and
/// releasing delivers an unmatched `button 272 released` to client B.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: refuse_client_drags drops ClickGrab leaving unmatched release on other client"]
fn refuse_client_drags_drops_click_grab_and_delivers_unmatched_release_to_second_client() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let reg_a = ls.registry().clone();
    let qh_a = ls.qh.clone();
    ls.probe.bind_clipboard(&reg_a, &qh_a);
    let w = ls.probe.toplevel(&qh_a, "A", (700, 500), 0);
    ls.pump();
    ls.probe.draw(&qh_a, w);
    ls.pump();
    let ew = *ls.state.core().stage().mapped_elements().last().unwrap();

    let mut b = Other::connect(&mut ls);
    b.drive(&mut ls);
    b.probe.bind_basic(&b.registry, &b.qh);
    let ptr_b = b.probe.seat.as_ref().unwrap().get_pointer(&b.qh, ());
    b.probe.pointer = Some(ptr_b);
    b.drive(&mut ls);
    let _bi = b.probe.toplevel(&b.qh, "B", (200, 200), 0);
    b.drive(&mut ls);
    let eb = ls.state.elements().into_iter().find(|id| *id != ew).unwrap();
    let t = ls.now + Duration::from_millis(20);
    ls.run_until(t);
    b.drive(&mut ls);

    // Move B right by (+500, 0) using Super+Left so A and B never overlap even when A is clicked.
    let d0 = delta_to(&ls, eb, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, d0, d0, 1).unwrap();
    b.drive(&mut ls);
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, 2).unwrap();
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, logo(), 3).unwrap();
    pointer_motion(&mut ls.state, &mut ls.input, (500.0, 0.0), (450.0, 0.0), 4).unwrap();
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, false, logo(), 5).unwrap();
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, 6).unwrap();
    b.drive(&mut ls);
    let t = ls.now + Duration::from_millis(20);
    ls.run_until(t);
    b.drive(&mut ls);

    // Confirm A and B are now disjoint and hit independently.
    let db = delta_to(&ls, eb, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, db, db, 7).unwrap();
    b.drive(&mut ls);
    assert_eq!(hit_under_pointer(&mut ls).map(|(e, _)| e), Some(eb));

    let da = delta_to(&ls, ew, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, da, da, 8).unwrap();
    b.drive(&mut ls);
    assert_eq!(hit_under_pointer(&mut ls).map(|(e, _)| e), Some(ew));

    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, none(), 9).unwrap();
    b.drive(&mut ls);
    let serial = ls.probe.last_serial;

    // A starts a drag -> refuse_client_drags replaces DnDGrab with PanDismissedPopupGrab.
    let source = ls
        .probe
        .data_device_manager
        .as_ref()
        .unwrap()
        .create_data_source(&qh_a, ());
    source.offer("text/plain".into());
    let device = ls.probe.clipboard.device.clone().unwrap();
    device.start_drag(Some(&source), &ls.probe.surfaces[w].wl, None, serial);
    b.drive(&mut ls);
    assert!(
        ls.probe.clipboard.log.contains(&"cancelled".to_string()),
        "drag should have been refused: {:?}",
        ls.probe.clipboard.log
    );

    b.probe.ptr.clear();
    let db = delta_to(&ls, eb, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, db, db, 10).unwrap();
    b.drive(&mut ls);
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, false, none(), 11).unwrap();
    b.drive(&mut ls);

    assert!(
        !b.probe
            .ptr
            .iter()
            .any(|l| l == "button 272 released"),
        "client B received unmatched button 272 released without pressed: B.ptr={:?}",
        b.probe.ptr
    );
}

/// Pressing a button inside an active `PopupPointerGrab` bypasses `DefaultGrab` so no `ClickGrab`
/// is installed. Ending the popup grab while the button stays held (`Super+Tab` ->
/// `pointer_unset_grab`) returns Smithay to `DefaultGrab`; moving onto client B and releasing
/// sends an unmatched `button 272 released` to client B.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: ending PopupPointerGrab mid-press returns to DefaultGrab without ClickGrab"]
fn popup_pointer_grab_ended_mid_press_delivers_unmatched_release_to_second_client() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh_a = ls.qh.clone();
    let w = ls.probe.toplevel(&qh_a, "A", (700, 500), 0);
    ls.pump();
    ls.probe.draw(&qh_a, w);
    ls.pump();
    let ew = *ls.state.core().stage().mapped_elements().last().unwrap();

    let mut b = Other::connect(&mut ls);
    b.drive(&mut ls);
    b.probe.bind_basic(&b.registry, &b.qh);
    let ptr_b = b.probe.seat.as_ref().unwrap().get_pointer(&b.qh, ());
    b.probe.pointer = Some(ptr_b);
    b.drive(&mut ls);
    let _bi = b.probe.toplevel(&b.qh, "B", (200, 200), 0);
    b.drive(&mut ls);
    let eb = ls.state.elements().into_iter().find(|id| *id != ew).unwrap();
    let t = ls.now + Duration::from_millis(20);
    ls.run_until(t);
    b.drive(&mut ls);

    set_focus(&mut ls.state, &mut ls.input, Some(ew)).unwrap();
    b.drive(&mut ls);

    // Click A at (40, 40) outside centered B to obtain a recent pointer serial, then open popup
    // over A's exposed top-left region (20, 20, 20, 20) -> popup covers (20..140, 20..140).
    let da = delta_to(&ls, ew, (40.0, 40.0));
    pointer_motion(&mut ls.state, &mut ls.input, da, da, 1).unwrap();
    b.drive(&mut ls);
    assert_eq!(hit_under_pointer(&mut ls).map(|(e, _)| e), Some(ew));
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, none(), 2).unwrap();
    b.drive(&mut ls);
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, false, none(), 3).unwrap();
    b.drive(&mut ls);

    let _p = ls.probe.popup_with_grab(
        &qh_a,
        "P",
        w,
        PopupSpec {
            content: (120, 120),
            shadow: 0,
            anchor_rect: (20, 20, 20, 20),
            slide: false,
        },
    );
    b.drive(&mut ls);
    let t = ls.now + Duration::from_millis(20);
    ls.run_until(t);
    b.drive(&mut ls);

    // Hover inside popup P (at A-local (60, 60)) and press BTN_LEFT while PopupPointerGrab is active.
    let dp = delta_to(&ls, ew, (60.0, 60.0));
    pointer_motion(&mut ls.state, &mut ls.input, dp, dp, 4).unwrap();
    b.drive(&mut ls);
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, none(), 5).unwrap();
    b.drive(&mut ls);
    assert!(
        ls.probe.ptr.iter().any(|l| l == "button 272 pressed"),
        "expected popup client A to get pressed: {:?}",
        ls.probe.ptr
    );

    // Super+Tab switches focus to B while BTN_LEFT is still down -> unsets PopupPointerGrab to DefaultGrab.
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, 6).unwrap();
    keyboard_key(&mut ls.state, &mut ls.input, KEY_TAB, true, 7).unwrap();
    keyboard_key(&mut ls.state, &mut ls.input, KEY_TAB, false, 8).unwrap();
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, 9).unwrap();
    b.drive(&mut ls);

    b.probe.ptr.clear();
    let db = delta_to(&ls, eb, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, db, db, 10).unwrap();
    b.drive(&mut ls);
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, false, none(), 11).unwrap();
    b.drive(&mut ls);

    assert!(
        !b.probe
            .ptr
            .iter()
            .any(|l| l == "button 272 released"),
        "client B received unmatched button 272 released without pressed: B.ptr={:?}",
        b.probe.ptr
    );
}

/// With no mapped toplevel (`core.focus().target() == None`), a layer surface's `layer_popup`
/// holds `popup_grab`. Starting `CameraPan` (`Super+MMB`) checks `if let Some(target) = self.focus.target()`
/// in `entry.rs:2639` and emits no `Effect::DismissPopupGrab`, so the layer popup is never dismissed.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: CameraPan omits DismissPopupGrab when no window is focused"]
fn camera_pan_with_layer_popup_and_no_window_focus_does_not_dismiss_popup_grab() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    ls.bind_layer_shell();
    let qh = ls.qh.clone();
    let l = ls.probe.layer_surface(
        &qh,
        "L",
        Layer::Overlay,
        Anchor::Top | Anchor::Left,
        (300, 200),
        0,
        KeyboardInteractivity::OnDemand,
    );
    ls.pump();
    ls.probe.draw(&qh, l);
    ls.pump();
    let t = ls.now + Duration::from_millis(20);
    ls.run_until(t);

    assert_eq!(ls.state.core().focus().target(), None);

    // Move pointer from view center (960, 540) to layer surface L at (60, 60).
    pointer_motion(&mut ls.state, &mut ls.input, (-900.0, -480.0), (-810.0, -432.0), 1).unwrap();
    ls.pump();
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, none(), 2).unwrap();
    ls.pump();
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, false, none(), 3).unwrap();
    ls.pump();
    let serial = ls.probe.last_serial;

    let _lp = ls.probe.layer_popup(
        &qh,
        "LP",
        l,
        PopupSpec {
            content: (160, 120),
            shadow: 0,
            anchor_rect: (20, 20, 40, 20),
            slide: false,
        },
        Some(serial),
    );
    ls.pump();
    assert!(ls.state.is_pointer_grabbed(), "layer popup grab should be active");

    // Start CameraPan (Super + MMB).
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, 4).unwrap();
    pointer_button(&mut ls.state, &mut ls.input, BTN_MIDDLE, true, logo(), 5).unwrap();
    pointer_motion(&mut ls.state, &mut ls.input, (50.0, 0.0), (45.0, 0.0), 6).unwrap();
    pointer_button(&mut ls.state, &mut ls.input, BTN_MIDDLE, false, logo(), 7).unwrap();
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, 8).unwrap();
    ls.pump();

    assert!(
        ls.probe.popup_dones.contains(&"LP"),
        "expected CameraPan to dismiss layer popup LP even with focus.target()==None; popup_dones={:?}",
        ls.probe.popup_dones
    );
}

/// While a window `W` exists (`focus.target() == Some(W)`) and an `Exclusive` layer `L` holds
/// keyboard focus, `CameraPan` (`Super+MMB`) emits `Effect::DismissPopupGrab(W)`.
/// `outputs.rs:152-156` unconditionally calls `kb.set_focus(self, self.window_surface(W), serial)`,
/// stealing keyboard focus away from exclusive layer `L` back to `W`.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: Effect::DismissPopupGrab steals keyboard focus from Exclusive layer"]
fn dismiss_popup_grab_on_camera_pan_steals_keyboard_from_exclusive_layer() {
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    r.ls.pump();
    assert_eq!(r.ls.state.core().focus().target(), Some(w));

    r.ls.bind_layer_shell();
    let qh = r.ls.qh.clone();
    let l = r.ls.probe.layer_surface(
        &qh,
        "L",
        Layer::Overlay,
        Anchor::Top | Anchor::Left,
        (300, 120),
        0,
        KeyboardInteractivity::Exclusive,
    );
    r.ls.pump();
    r.ls.probe.draw(&qh, l);
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("L"));
    assert!(r.ls.state.core().focus().layer().is_some());

    // Pan the camera (Super+MMB down then up).
    let cam0 = r.camera();
    r.button(BTN_MIDDLE, true, logo());
    r.motion((50.0, 0.0));
    r.button(BTN_MIDDLE, false, logo());
    assert_ne!(r.camera(), cam0, "CameraPan must have moved camera");

    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some("L"),
        "CameraPan's DismissPopupGrab stole keyboard focus from exclusive layer L to {:?}; kb_log={:?}",
        r.ls.probe.kb.active,
        r.ls.probe.kb_log
    );
}

/// `WaylandState::disable_touch` (called on touchscreen removal in backend-drm `DeviceRemoved`)
/// cancels Smithay's `wl_touch` but never injects `PhysicalEvent::TouchCancel` into Core.
/// An active canvas touch slot remains in `Core::touches`, so a single finger tap on a window `W`
/// after touchscreen reconnect is absorbed as a second finger of the stale canvas gesture.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: disable_touch never sends TouchCancel to Core leaving stale touch slots"]
fn disable_touch_leaves_core_touch_state_active() {
    let mut r = Rig::x8();
    r.ls.state.enable_touch();
    r.ls.pump();
    let qh = r.ls.qh.clone();
    let seat: WlSeat = r.ls.probe.seat.clone().expect("seat");
    let _touch = seat.get_touch(&qh, ());
    r.ls.pump();

    let output = r.ls.output();
    // Touch slot 0 lands on empty canvas (e.g., view top-left (10, 10), outside centered W).
    {
        let port = &mut r.ls.input;
        r.ls.state
            .with_effects(|core, fx| {
                core.physical_input(
                    port,
                    PhysicalEvent::TouchDown {
                        slot: 0,
                        output,
                        p: (10.0, 10.0),
                        time_us: 1000,
                    },
                    fx,
                )?;
                core.physical_input(port, PhysicalEvent::TouchFrame, fx)
            })
            .unwrap();
    }
    r.ls.pump();

    // Touchscreen unplug -> disable_touch(), then plug back in -> enable_touch().
    r.ls.state.disable_touch();
    r.ls.pump();
    r.ls.state.enable_touch();
    r.ls.pump();
    r.ls.probe.log.clear();

    // New single finger slot 1 taps on center of window W (output logical (960, 540)).
    {
        let port = &mut r.ls.input;
        r.ls.state
            .with_effects(|core, fx| {
                core.physical_input(
                    port,
                    PhysicalEvent::TouchDown {
                        slot: 1,
                        output,
                        p: (960.0, 540.0),
                        time_us: 2000,
                    },
                    fx,
                )?;
                core.physical_input(port, PhysicalEvent::TouchFrame, fx)
            })
            .unwrap();
    }
    r.ls.pump();

    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 1 W ")),
        "expected tap on W after reconnect to reach W, but stale Core canvas touch absorbed it: log={:?}",
        r.ls.probe.log
    );
}

fn two_disjoint_windows() -> (
    Lockstep,
    usize,
    infynitae_core::stage::ElementId,
    Other,
    infynitae_core::stage::ElementId,
) {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh_a = ls.qh.clone();
    let w = ls.probe.toplevel(&qh_a, "A", (700, 500), 0);
    ls.pump();
    ls.probe.draw(&qh_a, w);
    ls.pump();
    let ew = *ls.state.core().stage().mapped_elements().last().unwrap();

    let mut b = Other::connect(&mut ls);
    b.drive(&mut ls);
    b.probe.bind_basic(&b.registry, &b.qh);
    let ptr_b = b.probe.seat.as_ref().unwrap().get_pointer(&b.qh, ());
    b.probe.pointer = Some(ptr_b);
    b.drive(&mut ls);
    let _bi = b.probe.toplevel(&b.qh, "B", (200, 200), 0);
    b.drive(&mut ls);
    let eb = ls.state.elements().into_iter().find(|id| *id != ew).unwrap();
    let t = ls.now + Duration::from_millis(20);
    ls.run_until(t);
    b.drive(&mut ls);

    // Move B right by (+500, 0) via Super+Left so A and B never overlap.
    let d0 = delta_to(&ls, eb, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, d0, d0, 1).unwrap();
    b.drive(&mut ls);
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, 2).unwrap();
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, logo(), 3).unwrap();
    pointer_motion(&mut ls.state, &mut ls.input, (500.0, 0.0), (450.0, 0.0), 4).unwrap();
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, false, logo(), 5).unwrap();
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, 6).unwrap();
    b.drive(&mut ls);
    let t = ls.now + Duration::from_millis(20);
    ls.run_until(t);
    b.drive(&mut ls);

    let db = delta_to(&ls, eb, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, db, db, 7).unwrap();
    b.drive(&mut ls);
    assert_eq!(hit_under_pointer(&mut ls).map(|(e, _)| e), Some(eb));

    let da = delta_to(&ls, ew, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, da, da, 8).unwrap();
    b.drive(&mut ls);
    assert_eq!(hit_under_pointer(&mut ls).map(|(e, _)| e), Some(ew));

    (ls, w, ew, b, eb)
}

/// Client A owns a `BTN_LEFT` press (`ClickGrab`) and destroys its surface mid-press.
/// Core still holds `Owner::Client`, while Smithay's `ClickGrab` drops back to `DefaultGrab`
/// because `!focus.alive()`. Moving onto client B and releasing `BTN_LEFT` delivers an
/// unmatched `button 272 released` to client B.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: ClickGrab lost on surface destroy delivers unmatched release to second client"]
fn click_grab_lost_on_surface_destroy_delivers_unmatched_release_to_second_client() {
    let (mut ls, w, _ew, mut b, eb) = two_disjoint_windows();

    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, none(), 9).unwrap();
    b.drive(&mut ls);

    ls.probe.destroy_toplevel(w);
    b.drive(&mut ls);
    let t = ls.now + Duration::from_millis(20);
    ls.run_until(t);
    b.drive(&mut ls);

    b.probe.ptr.clear();
    let db = delta_to(&ls, eb, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, db, db, 10).unwrap();
    b.drive(&mut ls);
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, false, none(), 11).unwrap();
    b.drive(&mut ls);

    assert!(
        !b.probe.ptr.iter().any(|l| l == "button 272 released"),
        "client B received unmatched button 272 released without pressed: B.ptr={:?}",
        b.probe.ptr
    );
}

/// Client A owns a `BTN_LEFT` press (`ClickGrab`) and unmaps its surface (`attach(None)` + `commit`)
/// mid-press. Smithay's `ClickGrab` checks only `focus.alive()`, not mappedness/visibility, so it
/// keeps routing motion to unmapped A while preventing B underneath the pointer from receiving
/// `wl_pointer.enter`.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: ClickGrab kept while client surface is unmapped"]
fn click_grab_kept_while_client_surface_unmapped() {
    let (mut ls, w, _ew, mut b, eb) = two_disjoint_windows();

    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, none(), 9).unwrap();
    b.drive(&mut ls);

    ls.probe.unmap(w);
    b.drive(&mut ls);
    let t = ls.now + Duration::from_millis(20);
    ls.run_until(t);
    b.drive(&mut ls);

    ls.probe.ptr.clear();
    b.probe.ptr.clear();
    let db = delta_to(&ls, eb, (100.0, 100.0));
    pointer_motion(&mut ls.state, &mut ls.input, db, db, 10).unwrap();
    b.drive(&mut ls);

    assert!(
        b.probe.ptr.iter().any(|l| l.starts_with("enter B ")),
        "expected visible B under pointer to receive enter after A unmapped, but unmapped A kept ClickGrab: A.ptr={:?}, B.ptr={:?}",
        ls.probe.ptr,
        b.probe.ptr
    );
}

/// `SessionLockHandler::lock` cancels Smithay's `wl_touch` state without injecting
/// `PhysicalEvent::TouchCancel` into Core. An active canvas touch slot remains in `Core::touches`
/// across lock and unlock, absorbing a subsequent single-finger tap on window `W`.
#[test]
#[ignore = "AUDIT_INPUT_LIFECYCLE: SessionLockHandler::lock does not send TouchCancel to Core"]
fn session_lock_leaves_core_touch_state_active_across_unlock() {
    let mut r = Rig::x8();
    r.ls.state.enable_touch();
    r.ls.state.enable_session_lock();
    r.ls.pump();
    let qh = r.ls.qh.clone();
    let seat: WlSeat = r.ls.probe.seat.clone().expect("seat");
    let _touch = seat.get_touch(&qh, ());
    r.ls.pump();

    let output = r.ls.output();
    // Touch slot 0 lands on empty canvas before locking.
    {
        let port = &mut r.ls.input;
        r.ls.state
            .with_effects(|core, fx| {
                core.physical_input(
                    port,
                    PhysicalEvent::TouchDown {
                        slot: 0,
                        output,
                        p: (10.0, 10.0),
                        time_us: 1000,
                    },
                    fx,
                )?;
                core.physical_input(port, PhysicalEvent::TouchFrame, fx)
            })
            .unwrap();
    }
    r.ls.pump();

    // Lock the session (triggers SessionLockHandler::lock) and immediately unlock.
    let (name, _, _) = r
        .ls
        .probe
        .globals
        .iter()
        .find(|(_, i, _)| i == "ext_session_lock_manager_v1")
        .cloned()
        .expect("ext_session_lock_manager_v1");
    let manager: ExtSessionLockManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
    let lock = manager.lock(&qh, 1);
    let lock_surface = r
        .ls
        .probe
        .compositor
        .as_ref()
        .expect("compositor")
        .create_surface(&qh, ());
    let wl_out = r.ls.probe.outputs[0].1.clone();
    lock.get_lock_surface(&lock_surface, &wl_out, &qh, ());
    r.ls.pump();
    let size = r
        .ls
        .probe
        .log
        .iter()
        .rev()
        .find_map(|l| l.strip_prefix("lock surface ").map(str::to_owned));
    if let Some((lw, lh)) = size.as_deref().and_then(|s| s.split_once('x')) {
        let buffer = r
            .ls
            .probe
            .buffer(&qh, lw.parse().unwrap(), lh.parse().unwrap());
        lock_surface.attach(Some(&buffer), 0, 0);
        lock_surface.commit();
        r.ls.pump();
    }
    r.ls.state.lock_frame_drawn();
    r.ls.pump();
    assert!(r.ls.state.is_locked());

    lock.unlock_and_destroy();
    r.ls.pump();
    let releases = r
        .ls
        .state
        .take_unlock_releases()
        .expect("an unlock happened");
    for k in releases {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, k, false, r.t).expect("release");
    }
    r.ls.state.finish_unlock();
    r.ls.pump();
    assert!(!r.ls.state.is_locked());

    r.ls.probe.log.clear();
    // After unlock, single finger slot 1 taps on center of window W.
    {
        let port = &mut r.ls.input;
        r.ls.state
            .with_effects(|core, fx| {
                core.physical_input(
                    port,
                    PhysicalEvent::TouchDown {
                        slot: 1,
                        output,
                        p: (960.0, 540.0),
                        time_us: 2000,
                    },
                    fx,
                )?;
                core.physical_input(port, PhysicalEvent::TouchFrame, fx)
            })
            .unwrap();
    }
    r.ls.pump();

    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 1 W ")),
        "expected tap on W after unlock to reach W, but stale Core canvas touch from before lock absorbed it: log={:?}",
        r.ls.probe.log
    );
}
