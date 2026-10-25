//! Production path tests for `xdg_activation_v1` (B10.7, INV-INPUT-9, W-SM-8, PROV-10).
//!
//! The probe client maps W and V; W has the keyboard focus. A token is valid only if the client
//! created it with a serial of the seat no older than the keyboard's last focus `enter` (W-SM-8:
//! Smithay alone accepts every token). A valid token activates V: the keyboard focus moves to V
//! and no camera moves. A token without a serial, or with a serial from before the last focus
//! change, activates nothing.

mod common;

use std::os::unix::net::UnixStream;
use std::sync::Arc;

use common::{Lockstep, Probe, delta_to, output};
use infynitae_backend_headless::{pointer_motion, set_focus};
use infynitae_core::stage::ElementId;
use infynitae_wayland::ClientState;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, EventQueue, QueueHandle};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;

struct S {
    ls: Lockstep,
    w: usize,
    v: usize,
    ew: ElementId,
    ev: ElementId,
}

fn scene() -> S {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (400, 300), 0);
    ls.pump();
    ls.probe.draw(&qh, w);
    ls.pump();
    let v = ls.probe.toplevel(&qh, "V", (300, 200), 0);
    ls.pump();
    ls.probe.draw(&qh, v);
    ls.pump();
    let ids: Vec<_> = ls.state.elements().collect();
    let (ew, ev) = (ids[0], ids[1]);
    set_focus(&mut ls.state, &mut ls.input, Some(ew)).unwrap();
    ls.pump();
    assert_eq!(ls.probe.kb.active.as_deref(), Some("W"));
    S { ls, w, v, ew, ev }
}

impl S {
    /// A token created by the client, with `serial` (or none) and W as the requesting surface.
    fn token(&mut self, serial: Option<u32>) -> String {
        let activation = self.ls.activation();
        let qh = self.ls.qh.clone();
        let token = activation.get_activation_token(&qh, ());
        if let Some(serial) = serial {
            token.set_serial(serial, self.ls.probe.seat.as_ref().unwrap());
        }
        token.set_surface(&self.ls.probe.surfaces[self.w].wl);
        token.commit();
        self.ls.pump();
        self.ls.probe.activation_tokens.pop().expect("a token")
    }

    fn activate(&mut self, token: &str, idx: usize) {
        let activation = self.ls.activation();
        activation.activate(token.to_string(), &self.ls.probe.surfaces[idx].wl);
        self.ls.pump();
    }

    fn camera(&self) -> String {
        let c = self.ls.state.core().cameras();
        format!(
            "{:?}",
            (0..c.len())
                .map(|i| (c.center(i), c.zoom(i)))
                .collect::<Vec<_>>()
        )
    }
}

/// A token created with the serial of the focus `enter` W received activates V: V gets the
/// keyboard focus (with its configure), and no camera moves (INV-INPUT-9).
#[test]
fn a_token_from_the_focused_client_activates_a_window() {
    let mut s = scene();
    let camera = s.camera();
    let serial = s.ls.probe.last_serial;
    let token = s.token(Some(serial));
    s.activate(&token, s.v);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ev));
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("V"));
    assert_eq!(s.camera(), camera, "activation never moves a camera");
    // a token is used once
    set_focus(&mut s.ls.state, &mut s.ls.input, Some(s.ew)).unwrap();
    s.ls.pump();
    s.activate(&token, s.v);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ew), "spent");
}

/// W-SM-8: a token without a serial is not validated; activating with it moves no focus and
/// asks for attention.
#[test]
fn a_token_without_a_serial_activates_nothing() {
    let mut s = scene();
    let token = s.token(None);
    s.activate(&token, s.v);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ew));
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W"));
    assert_eq!(s.ls.state.core().focus().attention(), [s.ev]);
}

/// W-SM-8: a serial from before the last focus change does not show that the user just acted
/// in the requesting client; the token is not validated: no focus, attention.
#[test]
fn a_token_with_a_serial_older_than_the_last_focus_change_activates_nothing() {
    let mut s = scene();
    let old = s.ls.probe.last_serial;
    set_focus(&mut s.ls.state, &mut s.ls.input, Some(s.ev)).unwrap();
    s.ls.pump();
    set_focus(&mut s.ls.state, &mut s.ls.input, Some(s.ew)).unwrap();
    s.ls.pump();
    assert!(s.ls.probe.last_serial != old, "a newer enter");
    let token = s.token(Some(old));
    s.activate(&token, s.v);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ew));
}

/// A second client: the one that must not steal the focus.
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

    /// Exchanges messages between the compositor and both clients until quiescent.
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
        panic!("the exchange did not settle");
    }
}

/// Focus-stealing prevention (W-SM-8): a client the user merely points at gets
/// fresh pointer serials, newer than the last focus change, but not the keyboard focus. A token
/// it creates with such a serial is not validated: activating its own window asks for attention
/// and leaves the focus where it was.
#[test]
fn a_hovered_client_cannot_take_the_focus() {
    let mut s = scene();
    let mut o = Other::connect(&mut s.ls);
    o.drive(&mut s.ls);
    o.probe.bind_basic(&o.registry, &o.qh);
    let pointer = o.probe.seat.as_ref().unwrap().get_pointer(&o.qh, ());
    o.probe.pointer = Some(pointer);
    o.drive(&mut s.ls);
    let b = o.probe.toplevel(&o.qh, "B", (200, 100), 0);
    o.drive(&mut s.ls);
    o.probe.draw(&o.qh, b);
    o.drive(&mut s.ls);
    let eb = s.ls.state.elements().last().unwrap();
    assert!(eb != s.ew && eb != s.ev);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ew));
    // the pointer is (or moves) onto B: B has received `enter` with a serial newer than W's
    // focus `enter`
    // a frame that shows B: hit testing reads what was shown (INV-HIT-1)
    let t = s.ls.now + std::time::Duration::from_millis(20);
    s.ls.run_until(t);
    o.drive(&mut s.ls);
    let d = delta_to(&s.ls, eb, (20.0, 20.0));
    pointer_motion(&mut s.ls.state, &mut s.ls.input, d, d, 1).unwrap();
    o.drive(&mut s.ls);
    // serials only grow: B's `enter`, after B mapped, is newer than W's focus `enter`
    let serial = o.probe.last_serial;
    assert!(
        o.probe.ptr.iter().any(|l| l.starts_with("enter B")) && serial != 0,
        "B received the pointer: {:?}",
        o.probe.ptr
    );
    // B asks to activate its own window with that serial
    let (n, _) = o.probe.global("xdg_activation_v1").expect("offered");
    let activation: XdgActivationV1 = o.registry.bind(n, 1, &o.qh, ());
    let token = activation.get_activation_token(&o.qh, ());
    token.set_serial(serial, o.probe.seat.as_ref().unwrap());
    token.set_surface(&o.probe.surfaces[b].wl);
    token.commit();
    o.drive(&mut s.ls);
    let t = o.probe.activation_tokens.pop().expect("a token");
    activation.activate(t, &o.probe.surfaces[b].wl);
    o.drive(&mut s.ls);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ew), "no steal");
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W"));
    assert_eq!(
        s.ls.state.core().focus().attention(),
        [eb],
        "attention instead"
    );
}

/// Unused tokens are bounded: a client creating more tokens than the limit leaves at most
/// `ACTIVATION_TOKENS_KEPT` behind, and the newest still works.
#[test]
fn unused_tokens_are_bounded() {
    let mut s = scene();
    let serial = s.ls.probe.last_serial;
    let n = infynitae_wayland::ACTIVATION_TOKENS_KEPT + 100;
    let mut last = String::new();
    for _ in 0..n {
        last = s.token(Some(serial));
    }
    let kept = s.ls.state.activation_tokens_kept();
    assert!(
        kept <= infynitae_wayland::ACTIVATION_TOKENS_KEPT,
        "{kept} tokens kept"
    );
    s.activate(&last, s.v);
    assert_eq!(
        s.ls.probe.kb.active.as_deref(),
        Some("V"),
        "the newest token works"
    );
}
