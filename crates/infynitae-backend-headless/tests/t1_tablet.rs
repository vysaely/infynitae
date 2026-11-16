//! Graphics tablet and stylus (`zwp_tablet_manager_v2`) with real Wayland clients:
//! - a client binding `zwp_tablet_manager_v2` receives `tablet_added`, `tool_added`, `proximity_in`,
//!   `down`, `motion`, `pressure`, `tilt`, `button`, `up`, `proximity_out` and `frame` at the
//!   surface-local coordinates through the output's camera (panned and zoomed), with no duplicate
//!   `wl_pointer` events;
//! - a tap with the pen focuses and raises the window under it;
//! - a client that did not bind `zwp_tablet_manager_v2` receives `wl_pointer` motion and left click;
//! - pad buttons go only through `zwp_tablet_pad_v2` to the focused client;
//! - locking the session cancels any active pen contact and blocks tablet and pad events.

mod common;

use std::os::unix::net::UnixStream;
use std::sync::Arc;

use common::input::*;
use common::{Lockstep, Probe};
use infynitae_backend_headless::set_focus;
use infynitae_core::stage::{ElementId, ElementRef};
use infynitae_core::{NavigateTo, ShellCommand, ShellStream};
use infynitae_wayland::{ClientState, TabletAxes, TabletEvent, TabletPadDescriptor};
use smithay::backend::input::{TabletToolCapabilities, TabletToolDescriptor, TabletToolType};
use smithay::wayland::tablet_manager::TabletDescriptor;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::{
    self, ExtSessionLockSurfaceV1,
};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::ExtSessionLockV1;
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
    zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
    zwp_tablet_v2::{self, ZwpTabletV2},
};

delegate_noop!(Probe: ignore ZwpTabletManagerV2);
delegate_noop!(Probe: ignore ExtSessionLockManagerV1);
delegate_noop!(Probe: ignore ExtSessionLockV1);

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

impl Dispatch<ZwpTabletSeatV2, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpTabletSeatV2,
        ev: zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwp_tablet_seat_v2::Event::TabletAdded { .. } => st.log.push("tablet_added".into()),
            zwp_tablet_seat_v2::Event::ToolAdded { .. } => st.log.push("tool_added".into()),
            zwp_tablet_seat_v2::Event::PadAdded { .. } => st.log.push("pad_added".into()),
            _ => {}
        }
    }

    wayland_client::event_created_child!(Probe, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<ZwpTabletV2, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpTabletV2,
        ev: zwp_tablet_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_tablet_v2::Event::Name { name } = ev {
            st.log.push(format!("tablet_name {name}"));
        }
    }
}

impl Dispatch<ZwpTabletToolV2, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpTabletToolV2,
        ev: zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let label = |st: &Probe, s: &wayland_client::protocol::wl_surface::WlSurface| {
            st.surfaces
                .iter()
                .find(|p| p.wl == *s)
                .map_or("?", |p| p.label)
        };
        let line = match ev {
            zwp_tablet_tool_v2::Event::ProximityIn { surface, .. } => {
                format!("proximity_in {}", label(st, &surface))
            }
            zwp_tablet_tool_v2::Event::ProximityOut => "proximity_out".into(),
            zwp_tablet_tool_v2::Event::Down { .. } => "down".into(),
            zwp_tablet_tool_v2::Event::Up => "up".into(),
            zwp_tablet_tool_v2::Event::Motion { x, y } => format!("motion {x:.1},{y:.1}"),
            zwp_tablet_tool_v2::Event::Pressure { pressure } => format!("pressure {pressure}"),
            zwp_tablet_tool_v2::Event::Tilt { tilt_x, tilt_y } => {
                format!("tilt {tilt_x:.1},{tilt_y:.1}")
            }
            zwp_tablet_tool_v2::Event::Button { button, state, .. } => {
                format!("tool_button {button} {state:?}")
            }
            zwp_tablet_tool_v2::Event::Frame { .. } => "frame".into(),
            _ => return,
        };
        st.log.push(line);
    }
}

impl Dispatch<ZwpTabletPadV2, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpTabletPadV2,
        ev: zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let label = |st: &Probe, s: &wayland_client::protocol::wl_surface::WlSurface| {
            st.surfaces
                .iter()
                .find(|p| p.wl == *s)
                .map_or("?", |p| p.label)
        };
        let line = match ev {
            zwp_tablet_pad_v2::Event::Enter { surface, .. } => {
                format!("pad_enter {}", label(st, &surface))
            }
            zwp_tablet_pad_v2::Event::Leave { surface, .. } => {
                format!("pad_leave {}", label(st, &surface))
            }
            zwp_tablet_pad_v2::Event::Button { button, state, .. } => {
                format!("pad_button {button} {state:?}")
            }
            _ => return,
        };
        st.log.push(line);
    }

    wayland_client::event_created_child!(Probe, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn sample_tablet() -> TabletDescriptor {
    TabletDescriptor {
        name: "Test Pen Tablet".into(),
        usb_id: Some((0x056a, 0x0374)),
        syspath: Some("/sys/devices/virtual/tablet0".into()),
    }
}

fn sample_tool() -> TabletToolDescriptor {
    TabletToolDescriptor {
        tool_type: TabletToolType::Pen,
        hardware_serial: 0x1234_5678,
        hardware_id_wacom: 0x0802,
        capabilities: TabletToolCapabilities::PRESSURE | TabletToolCapabilities::TILT,
    }
}

fn submit_tablet(r: &mut Rig, ev: TabletEvent) {
    let phys = r.ls.state.tablet_event(ev);
    let port = &mut r.ls.input;
    for p in phys {
        r.ls.state
            .with_effects(|core, fx| core.physical_input(port, p, fx))
            .expect("tablet core event");
    }
    r.ls.pump();
}

fn bind_tablet_seat(r: &mut Rig) -> ZwpTabletSeatV2 {
    let qh = r.ls.qh.clone();
    let (name, version) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "zwp_tablet_manager_v2")
            .map(|(n, _, v)| (*n, *v))
            .expect("zwp_tablet_manager_v2 global advertised");
    let mgr: ZwpTabletManagerV2 = r.ls.registry().bind(name, version.min(1), &qh, ());
    let seat: WlSeat = r.ls.probe.seat.clone().expect("seat");
    let tseat = mgr.get_tablet_seat(&seat, &qh, ());
    r.ls.pump();
    tseat
}

fn shell(r: &mut Rig, cmd: ShellCommand) {
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(&mut r.ls.shell, cmd)
            .expect("shell_command");
    r.ls.state.apply(&fx);
}

fn move_and_zoom(r: &mut Rig, el: ElementId, at: (f64, f64), center: (f64, f64), zoom: f64) {
    let output = r.ls.output();
    let eref = ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: el,
    };
    shell(
        r,
        ShellCommand::MoveElement {
            element: eref,
            to: at,
            stream: ShellStream::Atomic,
        },
    );
    shell(
        r,
        ShellCommand::Navigate {
            output,
            to: NavigateTo::Canvas(center.0, center.1),
            zoom: Some(zoom),
            stream: ShellStream::Atomic,
        },
    );
    r.ls.pump();
    r.frame();
}

struct Other {
    conn: Connection,
    queue: EventQueue<Probe>,
    probe: Probe,
    qh: QueueHandle<Probe>,
    registry: wayland_client::protocol::wl_registry::WlRegistry,
}

impl Other {
    fn connect(ls: &mut Lockstep) -> Self {
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
        panic!("second client exchange did not settle");
    }
}

#[test]
fn tablet_v2_client_receives_events_with_panned_zoomed_camera_and_no_pointer_input() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    let qh = r.ls.qh.clone();
    // Window B mapped after W so B has focus initially; W is at canvas (100, 200) size 400x300.
    r.ls.probe.toplevel(&qh, "W", (400, 300), 0);
    r.ls.pump();
    let w_el = r.ls.state.elements().next().unwrap();
    r.ls.probe.toplevel(&qh, "B", (200, 200), 0);
    r.ls.pump();
    let b_el = r.ls.state.elements().nth(1).unwrap();
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(b_el)).expect("focus B");
    r.ls.pump();
    assert_eq!(r.ls.state.core().focus().target(), Some(b_el));

    // Place B far away and W at (100, 200); pan camera to (300, 350) at zoom 2.0.
    // On 1920x1080, output logical center (960, 540) maps to canvas (300, 350),
    // so output (990, 560) maps to canvas (315, 360) -> W-local (215.0, 160.0).
    let output = r.ls.output();
    let b_ref = ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: b_el,
    };
    shell(
        &mut r,
        ShellCommand::MoveElement {
            element: b_ref,
            to: (2000.0, 2000.0),
            stream: ShellStream::Atomic,
        },
    );
    move_and_zoom(&mut r, w_el, (100.0, 200.0), (300.0, 350.0), 2.0);

    let _tseat = bind_tablet_seat(&mut r);
    let tablet = sample_tablet();
    let tool = sample_tool();
    r.ls.probe.log.clear();
    r.ls.probe.ptr.clear();

    // Proximity-in at (990, 560) -> W-local (215.0, 160.0).
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::ProximityIn {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (990.0, 560.0),
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    assert!(
        r.ls.probe.log.contains(&"tablet_added".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.log.contains(&"tool_added".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.log.contains(&"proximity_in W".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.log.contains(&"motion 215.0,160.0".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.ptr.is_empty(),
        "tablet-v2 client must not receive pointer events: {:?}",
        r.ls.probe.ptr
    );

    // Tip down focuses and raises W.
    r.ls.probe.log.clear();
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Tip {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (990.0, 560.0),
            down: true,
            axes: TabletAxes {
                pressure: Some(0.5),
                tilt: Some((12.5, -7.5)),
                ..Default::default()
            },
            time_us: t,
        },
    );
    assert!(
        r.ls.probe.log.contains(&"down".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert_eq!(r.ls.state.core().focus().target(), Some(w_el));
    assert_eq!(r.ls.state.core().stage().top_mapped(), Some(w_el));
    assert!(
        r.ls.probe.ptr.is_empty(),
        "no pointer events on tablet tap: {:?}",
        r.ls.probe.ptr
    );

    // Motion by (+40, +20) output logical -> (+20, +10) canvas at zoom 2.0 -> W-local (235.0, 170.0).
    r.ls.probe.log.clear();
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Axis {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (1030.0, 580.0),
            axes: TabletAxes {
                pressure: Some(0.75),
                tilt: Some((15.0, -5.0)),
                ..Default::default()
            },
            time_us: t,
        },
    );
    assert!(
        r.ls.probe.log.contains(&"motion 235.0,170.0".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe
            .log
            .iter()
            .any(|l| l.starts_with("pressure 4915")),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.log.contains(&"tilt 15.0,-5.0".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.log.contains(&"frame".to_string()),
        "{:?}",
        r.ls.probe.log
    );

    // Stylus button press, tip up, proximity out.
    r.ls.probe.log.clear();
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Button {
            tool: tool.clone(),
            button: 0x14b,
            pressed: true,
            time_us: t,
        },
    );
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Tip {
            tablet,
            tool: tool.clone(),
            output,
            p: (1030.0, 580.0),
            down: false,
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    r.t += 1000;
    let t = r.t;
    submit_tablet(&mut r, TabletEvent::ProximityOut { tool, time_us: t });
    assert!(
        r.ls.probe
            .log
            .contains(&"tool_button 331 Value(Pressed)".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.log.contains(&"up".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.log.contains(&"proximity_out".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.ptr.is_empty(),
        "tablet-v2 client never received wl_pointer events: {:?}",
        r.ls.probe.ptr
    );
}

#[test]
fn client_without_tablet_v2_gets_pointer_motion_and_left_click() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    let qh = r.ls.qh.clone();
    r.ls.probe.toplevel(&qh, "W", (400, 300), 0);
    r.ls.pump();
    let w_el = r.ls.state.elements().next().unwrap();
    // Move pointer away onto empty canvas before panning/zooming.
    r.motion((800.0, 400.0));
    move_and_zoom(&mut r, w_el, (100.0, 200.0), (300.0, 350.0), 2.0);
    r.ls.probe.ptr.clear();

    let output = r.ls.output();
    let tablet = sample_tablet();
    let tool = sample_tool();

    // Probe did NOT bind zwp_tablet_manager_v2: pen drives wl_pointer motion + BTN_LEFT.
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::ProximityIn {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (990.0, 560.0),
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    assert!(
        r.ls.probe.ptr.contains(&"enter W 215,160".to_string()),
        "{:?}",
        r.ls.probe.ptr
    );

    r.ls.probe.ptr.clear();
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Tip {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (990.0, 560.0),
            down: true,
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Axis {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (1030.0, 580.0),
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Tip {
            tablet,
            tool,
            output,
            p: (1030.0, 580.0),
            down: false,
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    assert!(
        r.ls.probe.ptr.contains(&"button 272 pressed".to_string()),
        "{:?}",
        r.ls.probe.ptr
    );
    assert!(
        r.ls.probe.ptr.contains(&"motion 235,170".to_string()),
        "{:?}",
        r.ls.probe.ptr
    );
    assert!(
        r.ls.probe.ptr.contains(&"button 272 released".to_string()),
        "{:?}",
        r.ls.probe.ptr
    );
    assert_eq!(r.ls.state.core().focus().target(), Some(w_el));
}

#[test]
fn pen_switches_between_tablet_v2_and_plain_clients_without_double_input() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    let qh = r.ls.qh.clone();
    r.ls.probe.toplevel(&qh, "W", (400, 300), 0);
    r.ls.pump();
    let w_el = r.ls.state.elements().next().unwrap();
    let _tseat = bind_tablet_seat(&mut r);

    let mut plain = Other::connect(&mut r.ls);
    plain.drive(&mut r.ls);
    plain.probe.bind_basic(&plain.registry, &plain.qh);
    let ptr = plain
        .probe
        .seat
        .as_ref()
        .unwrap()
        .get_pointer(&plain.qh, ());
    plain.probe.pointer = Some(ptr);
    plain.drive(&mut r.ls);
    let b_idx = plain.probe.toplevel(&plain.qh, "B", (400, 300), 0);
    plain.drive(&mut r.ls);
    plain.probe.draw(&plain.qh, b_idx);
    plain.drive(&mut r.ls);
    let b_el = r.ls.state.elements().nth(1).unwrap();

    // Place W at (100, 200) and B at (700, 200), camera at (550, 350) zoom 1.0:
    // W covers output x in [510, 910), B covers output x in [1110, 1510).
    let instance = r.ls.state.core().stage().instance();
    let w_ref = ElementRef {
        instance,
        element: w_el,
    };
    let b_ref = ElementRef {
        instance,
        element: b_el,
    };
    let output = r.ls.output();
    shell(
        &mut r,
        ShellCommand::MoveElement {
            element: w_ref,
            to: (100.0, 200.0),
            stream: ShellStream::Atomic,
        },
    );
    shell(
        &mut r,
        ShellCommand::MoveElement {
            element: b_ref,
            to: (700.0, 200.0),
            stream: ShellStream::Atomic,
        },
    );
    shell(
        &mut r,
        ShellCommand::Navigate {
            output,
            to: NavigateTo::Canvas(550.0, 350.0),
            zoom: Some(1.0),
            stream: ShellStream::Atomic,
        },
    );
    r.ls.pump();
    r.frame();
    plain.drive(&mut r.ls);

    r.ls.probe.log.clear();
    r.ls.probe.ptr.clear();
    plain.probe.ptr.clear();

    let tablet = sample_tablet();
    let tool = sample_tool();

    // Hover and tap W (tablet-v2): only W gets tablet events, no pointer events anywhere.
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::ProximityIn {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (610.0, 450.0),
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    plain.drive(&mut r.ls);
    assert!(r.ls.probe.log.contains(&"proximity_in W".to_string()));
    assert!(r.ls.probe.ptr.is_empty());
    assert!(plain.probe.ptr.is_empty());

    // Move onto B (no tablet-v2) and tap: W gets proximity_out, B gets pointer motion + click.
    r.ls.probe.log.clear();
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Axis {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (1210.0, 450.0),
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Tip {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (1210.0, 450.0),
            down: true,
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Tip {
            tablet,
            tool,
            output,
            p: (1210.0, 450.0),
            down: false,
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    plain.drive(&mut r.ls);
    assert!(r.ls.probe.log.contains(&"proximity_out".to_string()));
    assert!(r.ls.probe.ptr.is_empty());
    assert!(
        plain.probe.ptr.contains(&"enter B 100,60".to_string()),
        "{:?}",
        plain.probe.ptr
    );
    assert!(
        plain.probe.ptr.contains(&"button 272 pressed".to_string()),
        "{:?}",
        plain.probe.ptr
    );
    assert!(
        plain.probe.ptr.contains(&"button 272 released".to_string()),
        "{:?}",
        plain.probe.ptr
    );
    assert_eq!(r.ls.state.core().focus().target(), Some(b_el));
}

#[test]
fn pad_buttons_go_only_through_tablet_pad_v2() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    r.map_w();
    let w_el = r.ls.state.elements().next().unwrap();
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w_el)).expect("focus W");
    r.ls.pump();
    let _tseat = bind_tablet_seat(&mut r);
    r.ls.state.add_tablet_pad(TabletPadDescriptor {
        sysname: "pad0".into(),
        path: Some("/dev/input/pad0".into()),
        buttons: 4,
        modes: 2,
        tablet: sample_tablet(),
    });
    r.ls.pump();
    assert!(
        r.ls.probe.log.contains(&"pad_added".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.log.contains(&"pad_enter W".to_string()),
        "{:?}",
        r.ls.probe.log
    );

    r.ls.probe.log.clear();
    r.ls.probe.ptr.clear();
    r.ls.probe.kb_log.clear();
    r.ls.state.tablet_pad_button("pad0", 2, true, 1_000);
    r.ls.state.tablet_pad_button("pad0", 2, false, 2_000);
    r.ls.pump();

    assert!(
        r.ls.probe
            .log
            .contains(&"pad_button 2 Value(Pressed)".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe
            .log
            .contains(&"pad_button 2 Value(Released)".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(r.ls.probe.ptr.is_empty(), "{:?}", r.ls.probe.ptr);
    assert!(r.ls.probe.kb_log.is_empty(), "{:?}", r.ls.probe.kb_log);
}

#[test]
fn lock_cancels_tablet_and_blocks_tablet_events() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    r.map_w();
    r.ls.state.enable_session_lock();
    r.ls.pump();
    let _tseat = bind_tablet_seat(&mut r);
    r.ls.state.add_tablet_pad(TabletPadDescriptor {
        sysname: "pad0".into(),
        path: Some("/dev/input/pad0".into()),
        buttons: 4,
        modes: 1,
        tablet: sample_tablet(),
    });
    let output = r.ls.output();
    let tablet = sample_tablet();
    let tool = sample_tool();

    // Place stylus down on W (window W is centered at (-400, -300), size 840x640 -> (960, 540) is inside W).
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::ProximityIn {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (960.0, 540.0),
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::Tip {
            tablet: tablet.clone(),
            tool: tool.clone(),
            output,
            p: (960.0, 540.0),
            down: true,
            axes: TabletAxes::default(),
            time_us: t,
        },
    );

    // Lock the session as an allowed client.
    let qh = r.ls.qh.clone();
    let (name, _) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "ext_session_lock_manager_v1")
            .map(|(n, _, v)| (*n, *v))
            .expect("lock manager");
    let mgr: ExtSessionLockManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
    r.ls.probe.log.clear();
    let _lock: ExtSessionLockV1 = mgr.lock(&qh, ());
    r.ls.pump();
    assert!(r.ls.state.is_locked());
    assert!(
        r.ls.probe.log.contains(&"up".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.log.contains(&"proximity_out".to_string()),
        "{:?}",
        r.ls.probe.log
    );

    // Subsequent tablet and pad events while locked deliver nothing.
    r.ls.probe.log.clear();
    r.ls.probe.ptr.clear();
    r.t += 1000;
    let t = r.t;
    submit_tablet(
        &mut r,
        TabletEvent::ProximityIn {
            tablet,
            tool,
            output,
            p: (960.0, 540.0),
            axes: TabletAxes::default(),
            time_us: t,
        },
    );
    r.ls.state.tablet_pad_button("pad0", 0, true, t);
    r.ls.pump();
    assert!(r.ls.probe.log.is_empty(), "{:?}", r.ls.probe.log);
    assert!(r.ls.probe.ptr.is_empty(), "{:?}", r.ls.probe.ptr);
}
