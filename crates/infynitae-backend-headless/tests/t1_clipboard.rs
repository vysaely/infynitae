//! Production path tests for `wl_data_device_manager` and the primary selection with two clients (2026-10-16).
//!
//! Policy under test: the clipboard is offered only to the client that holds the keyboard focus
//! (which follows Core's ActiveElementFocus), and only that client may set it (Smithay refuses
//! `set_selection` from any other client). The compositor stores no contents: bytes go from the
//! source client to the receiving client through a pipe. A selection dies with its source.

mod common;

use std::io::Read;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use common::{Lockstep, Probe, output};
use infynitae_backend_headless::set_focus;
use infynitae_core::stage::ElementId;
use infynitae_wayland::ClientState;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_data_device_manager;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, EventQueue, QueueHandle};

/// The second client.
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
            .expect("insert the second client");
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

    /// Exchanges messages between the compositor and both clients until quiescent. The second
    /// client's connection may be gone (a test kills it): then only the first is driven.
    fn drive(&mut self, ls: &mut Lockstep) {
        let mut idle = 0;
        for _ in 0..10_000 {
            ls.pump();
            let mut handled = 0;
            if self.conn.flush().is_ok() {
                if let Some(guard) = self.conn.prepare_read() {
                    match guard.read() {
                        Ok(_) => {}
                        Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => {}
                    }
                }
                handled = self.queue.dispatch_pending(&mut self.probe).unwrap_or(0);
                let _ = self.conn.flush();
            }
            let served = ls
                .display
                .dispatch_clients(&mut ls.state)
                .expect("dispatch");
            ls.state.clear_orphaned_selections();
            ls.display.flush_clients().expect("flush");
            if handled == 0 && served == 0 {
                idle += 1;
                if idle >= 3 {
                    return;
                }
            } else {
                idle = 0;
            }
        }
        panic!("the exchange did not settle");
    }
}

struct S {
    ls: Lockstep,
    o: Other,
    a: ElementId,
    b: ElementId,
}

const MIMES: [&str; 2] = ["text/plain;charset=utf-8", "text/plain"];

/// Client A (the lockstep probe) maps "A", client B maps "B"; both bind the clipboard; A has the
/// keyboard focus.
fn scene() -> S {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let wa = ls.probe.toplevel(&qh, "A", (400, 300), 0);
    ls.pump();
    ls.probe.draw(&qh, wa);
    ls.pump();
    let registry = ls.registry().clone();
    ls.probe.bind_clipboard(&registry, &qh);
    ls.pump();
    let mut o = Other::connect(&mut ls);
    o.drive(&mut ls);
    o.probe.bind_basic(&o.registry, &o.qh);
    o.drive(&mut ls);
    o.probe.bind_clipboard(&o.registry, &o.qh);
    let wb = o.probe.toplevel(&o.qh, "B", (300, 200), 0);
    o.drive(&mut ls);
    o.probe.draw(&o.qh, wb);
    o.drive(&mut ls);
    let ids: Vec<_> = ls.state.elements().collect();
    let (a, b) = (ids[0], ids[1]);
    let mut s = S { ls, o, a, b };
    s.focus(a);
    s
}

impl S {
    fn focus(&mut self, e: ElementId) {
        set_focus(&mut self.ls.state, &mut self.ls.input, Some(e)).unwrap();
        self.o.drive(&mut self.ls);
    }

    fn a_copies(&mut self, bytes: &[u8]) {
        let qh = self.ls.qh.clone();
        let serial = self.ls.probe.last_serial;
        self.ls.probe.copy(&qh, &MIMES, bytes, serial);
        self.o.drive(&mut self.ls);
    }

    /// B reads the current selection as `mime`, driving both clients until the pipe closes.
    fn b_pastes(&mut self, mime: &str) -> Vec<u8> {
        let mut reader = self.o.probe.paste(mime);
        self.o.drive(&mut self.ls);
        let mut out = Vec::new();
        reader.read_to_end(&mut out).expect("read");
        out
    }
}

/// Copy in A, focus B, paste in B: B is offered A's MIME types and receives A's bytes.
#[test]
fn a_copy_in_one_client_pastes_in_another() {
    let mut s = scene();
    s.a_copies(b"hello infynitae");
    assert!(
        s.o.probe.clipboard.selection.is_none(),
        "B unfocused: no offer"
    );
    s.focus(s.b);
    let (_, mimes) =
        s.o.probe
            .clipboard
            .selection
            .clone()
            .expect("offer at focus");
    assert_eq!(mimes, MIMES);
    assert_eq!(s.b_pastes("text/plain"), b"hello infynitae");
    assert!(
        s.ls.probe
            .clipboard
            .log
            .contains(&"send text/plain".to_string()),
        "{:?}",
        s.ls.probe.clipboard.log
    );
}

/// An unfocused client is offered nothing and cannot set the selection: B's copy while A holds
/// the focus is ignored (A's source is not cancelled; A keeps the selection).
#[test]
fn an_unfocused_client_neither_sees_nor_sets_the_clipboard() {
    let mut s = scene();
    s.a_copies(b"from A");
    let qh = s.o.qh.clone();
    let serial = s.o.probe.last_serial;
    s.o.probe.copy(&qh, &["text/plain"], b"from B", serial);
    s.o.drive(&mut s.ls);
    assert!(s.o.probe.clipboard.selection.is_none());
    assert!(
        !s.ls.probe.clipboard.log.contains(&"cancelled".to_string()),
        "{:?}",
        s.ls.probe.clipboard.log
    );
    s.focus(s.b);
    assert_eq!(s.b_pastes("text/plain"), b"from A", "A's selection stands");
}

/// Ownership changes: when the focused B copies, A's source is cancelled and A, refocused, is
/// offered B's selection.
#[test]
fn a_new_copy_takes_the_selection_over() {
    let mut s = scene();
    s.a_copies(b"first");
    s.focus(s.b);
    let qh = s.o.qh.clone();
    let serial = s.o.probe.last_serial;
    s.o.probe.copy(&qh, &["text/plain"], b"second", serial);
    s.o.drive(&mut s.ls);
    assert!(
        s.ls.probe.clipboard.log.contains(&"cancelled".to_string()),
        "{:?}",
        s.ls.probe.clipboard.log
    );
    s.focus(s.a);
    let (_, mimes) = s.ls.probe.clipboard.selection.clone().expect("B's offer");
    assert_eq!(mimes, ["text/plain"]);
}

/// A selection dies with its owner: B copies, A is focused and offered it; B's client quits:
/// A is told the selection is gone (`selection none`). (Known limitation: a source destroyed
/// while its client keeps running stays offered until the next selection; a paste then reads
/// nothing. Smithay 0.7 gives the adapter no way to see that source die.)
#[test]
fn a_quitting_owner_clears_the_selection() {
    let mut s = scene();
    s.focus(s.b);
    let qh = s.o.qh.clone();
    let serial = s.o.probe.last_serial;
    s.o.probe.copy(&qh, &["text/plain"], b"short-lived", serial);
    s.o.drive(&mut s.ls);
    s.focus(s.a);
    assert!(s.ls.probe.clipboard.selection.is_some());
    let Other { conn, queue, .. } = std::mem::replace(&mut s.o, Other::connect(&mut s.ls));
    drop(queue);
    drop(conn);
    s.o.drive(&mut s.ls);
    assert!(
        s.ls.probe.clipboard.selection.is_none(),
        "{:?}",
        s.ls.probe.clipboard.log
    );
    assert_eq!(
        s.ls.probe.clipboard.log.last().map(String::as_str),
        Some("selection none")
    );
}

/// The source client dies during a transfer: the receiver's pipe ends (EOF, no hang), the
/// selection is gone, and the clipboard works again with a new source.
#[test]
fn the_source_client_dying_during_a_transfer_ends_it_cleanly() {
    let mut s = scene();
    // B is the source this time, A the receiver
    s.focus(s.b);
    let qh = s.o.qh.clone();
    let serial = s.o.probe.last_serial;
    s.o.probe.clipboard.source_bytes = Vec::new();
    s.o.probe.copy(&qh, &["text/plain"], b"never sent", serial);
    s.o.drive(&mut s.ls);
    s.focus(s.a);
    let mut reader = s.ls.probe.paste("text/plain");
    // B dies before it serves the request
    let Other { conn, queue, .. } = std::mem::replace(&mut s.o, Other::connect(&mut s.ls));
    drop(queue);
    drop(conn);
    s.o.drive(&mut s.ls);
    let mut out = Vec::new();
    reader.read_to_end(&mut out).expect("EOF, not a hang");
    assert!(out.is_empty());
    assert!(
        s.ls.probe.clipboard.selection.is_none(),
        "{:?}",
        s.ls.probe.clipboard.log
    );
    s.a_copies(b"again");
    assert!(s.ls.state.core().stage().z_order().contains(&s.a));
}

/// Invalid and stale requests do not disturb the compositor: a receive on an offer whose
/// selection was replaced, and a drag without an implicit pointer grab (refused by Smithay).
#[test]
fn stale_and_invalid_requests_are_harmless() {
    let mut s = scene();
    s.a_copies(b"one");
    s.focus(s.b);
    let (stale, _) = s.o.probe.clipboard.selection.clone().unwrap();
    s.focus(s.a);
    s.a_copies(b"two");
    let (r, w) = std::io::pipe().unwrap();
    stale.receive("text/plain".into(), std::os::fd::AsFd::as_fd(&w));
    drop(w);
    s.o.drive(&mut s.ls);
    drop(r);
    // a drag with a serial that is no pointer grab's
    let device = s.o.probe.clipboard.device.clone().unwrap();
    let source =
        s.o.probe
            .data_device_manager
            .as_ref()
            .unwrap()
            .create_data_source(&s.o.qh, ());
    let surface = s.o.probe.surfaces[0].wl.clone();
    device.start_drag(Some(&source), &surface, None, 12345);
    s.o.drive(&mut s.ls);
    s.focus(s.b);
    assert_eq!(s.b_pastes("text/plain"), b"two");
}

/// The primary selection follows the same rule: copied by the focused A, offered to B when B
/// is focused, read through a pipe; never offered to B while A holds the focus.
#[test]
fn the_primary_selection_follows_the_keyboard_focus() {
    let mut s = scene();
    let registry = s.ls.registry().clone();
    let qh = s.ls.qh.clone();
    s.ls.probe.bind_primary(&registry, &qh);
    s.o.probe.bind_primary(&s.o.registry, &s.o.qh);
    s.o.drive(&mut s.ls);
    let serial = s.ls.probe.last_serial;
    s.ls.probe
        .copy_primary(&qh, "text/plain", b"middle click", serial);
    s.o.drive(&mut s.ls);
    assert!(s.o.probe.primary_selection.is_none(), "B unfocused");
    s.focus(s.b);
    let (_, mimes) =
        s.o.probe
            .primary_selection
            .clone()
            .expect("offered at focus");
    assert_eq!(mimes, ["text/plain"]);
    let mut reader = s.o.probe.paste_primary("text/plain");
    s.o.drive(&mut s.ls);
    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();
    assert_eq!(out, b"middle click");
}

/// Starts a drag of `text/plain` from W on the held left press (the only serial Smithay accepts),
/// with no icon; returns the source.
fn start_drag(
    r: &mut common::input::Rig,
) -> wayland_client::protocol::wl_data_source::WlDataSource {
    use common::input::{BTN_LEFT, none};
    let registry = r.ls.registry().clone();
    let qh = r.ls.qh.clone();
    r.ls.probe.bind_clipboard(&registry, &qh);
    r.ls.pump();
    r.button(BTN_LEFT, true, none());
    assert!(r.ls.probe.ptr.iter().any(|l| l == "button 272 pressed"));
    let serial = r.ls.probe.last_serial;
    let source =
        r.ls.probe
            .data_device_manager
            .as_ref()
            .unwrap()
            .create_data_source(&qh, ());
    source.offer("text/plain".into());
    source.set_actions(
        wl_data_device_manager::DndAction::Copy | wl_data_device_manager::DndAction::Move,
    );
    let device = r.ls.probe.clipboard.device.clone().unwrap();
    let origin = r.ls.probe.surfaces[0].wl.clone();
    device.start_drag(Some(&source), &origin, None, serial);
    r.ls.pump();
    source
}

/// The target takes the drag it was offered: `text/plain`, moved.
fn accept_drag(r: &mut common::input::Rig) {
    let (offer, serial) = r.ls.probe.clipboard.drag.clone().expect("a drag offer");
    offer.accept(serial, Some("text/plain".into()));
    offer.set_actions(
        wl_data_device_manager::DndAction::Move,
        wl_data_device_manager::DndAction::Move,
    );
    r.ls.pump();
}

/// Drag-and-drop within one window (owner, 2026-11-23: Discord's server list could not be
/// reordered; it was refused since 2026-10-16). The drag holds while the button does: each motion
/// reaches the window as a drag motion, the release drops there, and the source learns the drop
/// was performed; afterwards the pointer is the window's again.
#[test]
fn a_drag_within_one_window_holds_until_the_release_and_drops_there() {
    use common::input::{BTN_LEFT, Rig, none};
    let mut r = Rig::x8();
    let _source = start_drag(&mut r);
    assert!(
        !r.ls.probe.clipboard.log.contains(&"cancelled".to_string()),
        "the drag is not cancelled: {:?}",
        r.ls.probe.clipboard.log
    );
    r.motion((10.0, 0.0));
    assert!(
        r.ls.probe
            .clipboard
            .log
            .iter()
            .any(|l| l.starts_with("dnd enter W ")),
        "{:?}",
        r.ls.probe.clipboard.log
    );
    accept_drag(&mut r);
    r.motion((10.0, 0.0));
    r.motion((0.0, 10.0));
    let log = &r.ls.probe.clipboard.log;
    let motions = log.iter().filter(|l| l.starts_with("dnd motion ")).count();
    assert!(motions >= 2, "the drag follows the pointer: {log:?}");
    // no button reaches the window as a pointer event while it drags
    let m = r.mark();
    r.button(BTN_LEFT, false, none());
    assert!(
        !r.since(m).contains(&"button 272 released"),
        "{:?}",
        r.since(m)
    );
    let log = &r.ls.probe.clipboard.log;
    assert!(log.contains(&"dnd drop".to_string()), "{log:?}");
    assert!(log.contains(&"dnd_drop_performed".to_string()), "{log:?}");
    assert!(!log.contains(&"cancelled".to_string()), "{log:?}");
    // the drag is over: the pointer is W's again at the next motion, and a press reaches it
    r.motion((10.0, 0.0));
    let m = r.mark();
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    let after = r.since(m);
    assert!(after.contains(&"button 272 pressed"), "{after:?}");
    assert!(after.contains(&"button 272 released"), "{after:?}");
    assert!(
        r.ls.probe
            .ptr
            .iter()
            .rev()
            .any(|l| l.starts_with("enter W ")),
        "{:?}",
        r.ls.probe.ptr
    );
}

/// A drag goes where Core routes the pointer: from W onto V, opened over W, it leaves W and enters
/// V, and the release drops on V.
#[test]
fn a_drag_moves_from_one_window_onto_another_and_drops_there() {
    use common::input::{BTN_LEFT, Rig, none};
    let mut r = Rig::x8();
    let qh = r.ls.qh.clone();
    // V (200 x 150) opens where the placement puts it; the pointer stays on W
    let vi = r.ls.probe.toplevel(&qh, "V", (200, 150), 0);
    r.ls.pump();
    r.ls.probe.draw(&qh, vi);
    r.ls.pump();
    r.frame();
    let v = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let _source = start_drag(&mut r);
    r.motion((5.0, 0.0));
    assert!(
        r.ls.probe
            .clipboard
            .log
            .iter()
            .any(|l| l.starts_with("dnd enter W ")),
        "{:?}",
        r.ls.probe.clipboard.log
    );
    // onto V, at V-local (100, 75)
    let d = common::delta_to(&r.ls, v, (100.0, 75.0));
    r.motion(d);
    let log = &r.ls.probe.clipboard.log;
    let leave = log.iter().position(|l| l == "dnd leave");
    let enter_v = log.iter().position(|l| l.starts_with("dnd enter V "));
    assert!(
        leave.is_some() && enter_v.is_some() && leave < enter_v,
        "W left, then V entered: {log:?}"
    );
    accept_drag(&mut r);
    r.motion((1.0, 0.0));
    r.button(BTN_LEFT, false, none());
    let log = &r.ls.probe.clipboard.log;
    let enter_v = log
        .iter()
        .rposition(|l| l.starts_with("dnd enter V "))
        .unwrap();
    let drop = log.iter().position(|l| l == "dnd drop");
    assert!(drop.is_some_and(|d| d > enter_v), "dropped on V: {log:?}");
    assert!(log.contains(&"dnd_drop_performed".to_string()), "{log:?}");
}
