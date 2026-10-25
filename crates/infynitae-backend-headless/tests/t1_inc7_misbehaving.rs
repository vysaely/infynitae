//! Production path tests for misbehaving clients (OB-18). Pass criterion of the
//! obligation: "protocol error to the client; compositor survives".
//!
//! Each test makes one request the protocol forbids, checks that the client is disconnected with
//! the protocol error the specification names, and then that the compositor is whole: the
//! offender's elements are gone from Core, no grab is left on the seat, and a second client that
//! was connected all along still maps a window, is focused and receives its configure.
//!
//! Two clients: the offender is the harness's probe; the bystander is a second probe on its own
//! connection.

mod common;

use std::os::unix::net::UnixStream;
use std::sync::Arc;

use common::{Lockstep, PopupSpec, Probe, Surf, output};
use infynitae_backend_headless::set_focus;
use infynitae_core::stage::Lifecycle;
use infynitae_wayland::ClientState;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, DispatchError, EventQueue, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_positioner::{self, XdgPositioner};

/// The error the pinned Smithay raises for a buffer on an unconfigured xdg_surface:
/// `xdg_surface.error.not_constructed`. (xdg-shell names `unconfigured_buffer`, code 3, for this;
/// the client is disconnected either way, which is what OB-18 asks for.)
const NOT_CONSTRUCTED: u32 = 1;
/// The error the pinned Smithay raises for an acknowledgement of an unknown serial:
/// `xdg_wm_base.error.invalid_surface_state`. (xdg-shell names `xdg_surface.invalid_serial`.)
const INVALID_SURFACE_STATE: u32 = 4;

/// A second client of the same compositor.
struct Bystander {
    conn: Connection,
    queue: EventQueue<Probe>,
    probe: Probe,
    qh: QueueHandle<Probe>,
    registry: WlRegistry,
}

struct M {
    ls: Lockstep,
    by: Bystander,
}

/// What one side of a connection saw when the exchange stopped.
#[derive(Debug, PartialEq, Eq)]
struct Killed {
    interface: String,
    code: u32,
}

impl M {
    fn new() -> M {
        let ls = Lockstep::new(Some(output((640, 480), 120)));
        let (server_end, client_end) = UnixStream::pair().expect("socketpair");
        client_end.set_nonblocking(true).expect("non-blocking");
        ls.display
            .handle()
            .insert_client(server_end, Arc::new(ClientState::default()))
            .expect("insert bystander");
        let conn = Connection::from_socket(client_end).expect("bystander connection");
        let queue = conn.new_event_queue();
        let qh = queue.handle();
        let registry = conn.display().get_registry(&qh, ());
        let mut m = M {
            ls,
            by: Bystander {
                conn,
                queue,
                probe: Probe::default(),
                qh,
                registry,
            },
        };
        assert_eq!(m.drive(), None);
        m.by.probe.bind_basic(&m.by.registry, &m.by.qh);
        assert_eq!(m.drive(), None);
        m
    }

    /// Exchanges messages between the compositor and both clients until quiescent. Unlike
    /// `Lockstep::pump` it survives the offender's death and reports the protocol error it died
    /// of. The bystander must never fail.
    fn drive(&mut self) -> Option<Killed> {
        let mut killed = None;
        let mut idle = 0;
        for _ in 0..10_000 {
            let served = self
                .ls
                .display
                .dispatch_clients(&mut self.ls.state)
                .expect("server dispatch");
            let _ = self.ls.display.flush_clients();
            let mut handled = 0;
            if killed.is_none() {
                let _ = self.ls.conn.flush();
                if let Some(guard) = self.ls.conn.prepare_read() {
                    let _ = guard.read();
                }
                match self.ls.queue.dispatch_pending(&mut self.ls.probe) {
                    Ok(n) => handled += n,
                    Err(DispatchError::Backend(WaylandError::Protocol(_))) => {}
                    Err(e) => panic!("offender: {e}"),
                }
                let _ = self.ls.conn.flush();
                killed = self.ls.conn.protocol_error().map(|e| Killed {
                    interface: e.object_interface.clone(),
                    code: e.code,
                });
            }
            self.by.conn.flush().expect("bystander flush");
            if let Some(guard) = self.by.conn.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("bystander read: {e}"),
                }
            }
            handled += self
                .by
                .queue
                .dispatch_pending(&mut self.by.probe)
                .expect("bystander dispatch");
            self.by.conn.flush().expect("bystander flush");
            if served == 0 && handled == 0 {
                idle += 1;
                if idle >= 3 {
                    return killed;
                }
            } else {
                idle = 0;
            }
        }
        panic!("the exchange did not settle");
    }

    /// The offender maps a focused 200x100 window "W" (legally).
    fn offender_window(&mut self) -> usize {
        let qh = self.ls.qh.clone();
        let w = self.ls.probe.toplevel(&qh, "W", (200, 100), 0);
        assert_eq!(self.drive(), None);
        let id = self.ls.state.elements().last().expect("element");
        assert_eq!(
            self.ls.state.core().stage().state(id),
            Some(Lifecycle::Mapped)
        );
        set_focus(&mut self.ls.state, &mut self.ls.input, Some(id)).expect("focus");
        assert_eq!(self.drive(), None);
        w
    }

    /// An xdg_surface of the offender with no role yet.
    fn offender_xdg_surface(&mut self, label: &'static str, size: (i32, i32)) -> usize {
        let qh = self.ls.qh.clone();
        let p = &mut self.ls.probe;
        let idx = p.surfaces.len();
        let wl = p
            .compositor
            .as_ref()
            .expect("wl_compositor")
            .create_surface(&qh, ());
        let xdg = p
            .wm_base
            .as_ref()
            .expect("xdg_wm_base")
            .get_xdg_surface(&wl, &qh, idx);
        p.surfaces.push(Surf {
            label,
            wl,
            xdg: Some(xdg),
            toplevel: None,
            popup: None,
            subsurface: None,
            size,
            configured: false,
            popup_configure: None,
            frame_driven: false,
        });
        idx
    }

    fn positioner(&self) -> XdgPositioner {
        let pos: XdgPositioner = self
            .ls
            .probe
            .wm_base
            .as_ref()
            .expect("xdg_wm_base")
            .create_positioner(&self.ls.qh, ());
        pos.set_size(40, 20);
        pos.set_anchor_rect(10, 10, 1, 1);
        pos.set_anchor(xdg_positioner::Anchor::TopLeft);
        pos.set_gravity(xdg_positioner::Gravity::BottomRight);
        pos
    }

    /// The compositor is whole after the offender's death: nothing of the offender is left in
    /// Core or on the seat, and the bystander maps a window, gets its configure and is focused.
    fn assert_survived(&mut self) {
        self.drive();
        let core = self.ls.state.core();
        assert_eq!(core.stage().z_order(), &[], "an element of the offender");
        assert_eq!(self.ls.state.elements().count(), 0);
        assert!(core.fullscreen().is_empty());
        assert_eq!(core.focus().target(), None);
        assert!(
            !self.ls.state.keyboard_is_grabbed(),
            "a keyboard grab is left"
        );
        assert!(
            !self.ls.state.is_pointer_grabbed(),
            "a pointer grab is left"
        );
        self.assert_bystander_works();
    }

    /// The second client maps a window, gets its configure and is focused.
    fn assert_bystander_works(&mut self) {
        let qh = self.by.qh.clone();
        let b = self.by.probe.toplevel(&qh, "B", (120, 80), 0);
        self.drive();
        assert!(self.by.probe.surfaces[b].configured, "no configure for B");
        let id = self
            .ls
            .state
            .elements()
            .last()
            .expect("the bystander's element");
        assert_eq!(
            self.ls.state.core().stage().state(id),
            Some(Lifecycle::Mapped)
        );
        set_focus(&mut self.ls.state, &mut self.ls.input, Some(id)).expect("focus");
        self.drive();
        assert_eq!(self.by.probe.kb.active.as_deref(), Some("B"));
        assert!(
            self.by.probe.kb.violations.is_empty(),
            "{:?}",
            self.by.probe.kb.violations
        );
    }
}

fn killed(interface: &str, code: u32) -> Option<Killed> {
    Some(Killed {
        interface: interface.to_string(),
        code,
    })
}

/// W-SM-6, toplevel: a buffer on the initial commit, before any configure.
#[test]
fn a_toplevel_buffer_on_the_initial_commit() {
    let mut m = M::new();
    let qh = m.ls.qh.clone();
    let w = m.ls.probe.toplevel_uncommitted(&qh, "W", (200, 100));
    m.ls.probe.draw(&qh, w);
    assert_eq!(m.drive(), killed("xdg_surface", NOT_CONSTRUCTED));
    m.assert_survived();
}

/// W-SM-6, toplevel: a buffer after the initial configure was sent and before it was
/// acknowledged.
#[test]
fn a_toplevel_buffer_before_the_acknowledgement() {
    let mut m = M::new();
    let qh = m.ls.qh.clone();
    m.ls.probe.hold_acks = true;
    let w = m.ls.probe.toplevel(&qh, "W", (200, 100), 0);
    assert_eq!(m.drive(), None);
    assert_eq!(m.ls.probe.held_acks.len(), 1, "the initial configure");
    m.ls.probe.draw(&qh, w);
    assert_eq!(m.drive(), killed("xdg_surface", NOT_CONSTRUCTED));
    m.assert_survived();
}

/// W-SM-6, popup: a buffer on the popup's initial commit, before any configure.
#[test]
fn a_popup_buffer_on_the_initial_commit() {
    let mut m = M::new();
    let w = m.offender_window();
    let p = m.offender_xdg_surface("P", (40, 20));
    let pos = m.positioner();
    let qh = m.ls.qh.clone();
    let parent = m.ls.probe.surfaces[w].xdg.clone().expect("parent");
    let xdg = m.ls.probe.surfaces[p].xdg.clone().expect("xdg");
    m.ls.probe.surfaces[p].popup = Some(xdg.get_popup(Some(&parent), &pos, &qh, p));
    m.ls.probe.draw(&qh, p);
    assert_eq!(m.drive(), killed("xdg_surface", NOT_CONSTRUCTED));
    m.assert_survived();
}

/// W-SM-6, popup: a buffer after the popup's configure was sent and before it was acknowledged.
#[test]
fn a_popup_buffer_before_the_acknowledgement() {
    let mut m = M::new();
    let w = m.offender_window();
    let qh = m.ls.qh.clone();
    m.ls.probe.hold_acks = true;
    let p = m.ls.probe.popup(
        &qh,
        "P",
        w,
        PopupSpec {
            content: (40, 20),
            shadow: 0,
            anchor_rect: (10, 10, 1, 1),
            slide: false,
        },
    );
    assert_eq!(m.drive(), None);
    assert_eq!(m.ls.probe.held_acks.len(), 1, "the popup's configure");
    m.ls.probe.draw(&qh, p);
    assert_eq!(m.drive(), killed("xdg_surface", NOT_CONSTRUCTED));
    m.assert_survived();
}

/// An acknowledgement of a serial the compositor never sent, on a fullscreen window: the client
/// dies of it and the presentation it held ends with it.
#[test]
fn an_acknowledgement_of_a_serial_never_sent() {
    let mut m = M::new();
    let w = m.offender_window();
    m.ls.probe.surfaces[w]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    assert_eq!(m.drive(), None);
    assert_eq!(m.ls.state.core().fullscreen().len(), 1);
    m.ls.probe.surfaces[w]
        .xdg
        .as_ref()
        .unwrap()
        .ack_configure(0x7fff_fff0);
    assert_eq!(m.drive(), killed("xdg_wm_base", INVALID_SURFACE_STATE));
    m.assert_survived();
}

/// W-SM-7: a grabbing popup whose parent xdg_surface has no role, so the grab has no root that
/// is an element. The pinned Smithay raises no protocol error for the parent (xdg-shell names
/// `invalid_popup_parent`); the adapter must not reach Smithay's `grab_popup` assertion. It
/// dismisses the popup instead. The compositor survives, no grab is installed, and the
/// offender's legal window is untouched.
#[test]
fn a_grabbing_popup_of_a_surface_without_a_role() {
    let mut m = M::new();
    m.offender_window();
    let parent = m.offender_xdg_surface("R", (10, 10));
    let p = m.offender_xdg_surface("P", (40, 20));
    let pos = m.positioner();
    let qh = m.ls.qh.clone();
    let parent_xdg = m.ls.probe.surfaces[parent].xdg.clone().expect("parent");
    let xdg = m.ls.probe.surfaces[p].xdg.clone().expect("xdg");
    let popup = xdg.get_popup(Some(&parent_xdg), &pos, &qh, p);
    popup.grab(
        m.ls.probe.seat.as_ref().expect("seat"),
        m.ls.probe.last_serial,
    );
    m.ls.probe.surfaces[p].popup = Some(popup);
    m.ls.probe.surfaces[p].wl.commit();
    assert_eq!(m.drive(), None, "the pinned Smithay raises no error here");
    assert_eq!(m.ls.probe.popup_dones, vec!["P"], "the popup is dismissed");
    assert!(!m.ls.state.keyboard_is_grabbed(), "a keyboard grab is left");
    assert!(!m.ls.state.is_pointer_grabbed(), "a pointer grab is left");
    assert_eq!(m.ls.state.elements().count(), 1, "the offender's window");
    assert_eq!(m.ls.probe.kb.active.as_deref(), Some("W"));
    m.assert_bystander_works();
}
