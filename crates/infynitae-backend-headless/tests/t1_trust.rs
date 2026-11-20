//! Who may use the privileged globals (owner decision 2026-10-31; infynitae-wayland `trust`,
//! docs/SECURITY.md). A sandboxed client (security-context-v1) sees none of them unless its app
//! id is on the capability's list; a native client is allowed by the default `native` word or by
//! its real executable, and an emptied list refuses it. A refused bind ends only that client.

mod common;

use std::collections::BTreeSet;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use common::{Lockstep, output};
use infynitae_wayland::ClientState;
use infynitae_wayland::cursor::TestTempDir;
use infynitae_wayland::trust::Identity;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};

/// Every privileged global, by interface name.
const PRIVILEGED: [&str; 16] = [
    "zwlr_screencopy_manager_v1",
    "ext_image_copy_capture_manager_v1",
    "ext_output_image_capture_source_manager_v1",
    "ext_foreign_toplevel_image_capture_source_manager_v1",
    "zwlr_data_control_manager_v1",
    "ext_data_control_manager_v1",
    "zwp_virtual_keyboard_manager_v1",
    "zwlr_virtual_pointer_manager_v1",
    "zwp_input_method_manager_v2",
    "zwlr_foreign_toplevel_manager_v1",
    "ext_foreign_toplevel_list_v1",
    "ext_session_lock_manager_v1",
    "zwlr_gamma_control_manager_v1",
    "zwlr_output_power_manager_v1",
    "zwlr_output_manager_v1",
    "zwlr_layer_shell_v1",
];

const SECURITY_CONTEXT: &str = "wp_security_context_manager_v1";

#[derive(Default)]
struct Seen {
    globals: Vec<(u32, String)>,
}

impl Seen {
    fn names(&self) -> BTreeSet<&str> {
        self.globals.iter().map(|(_, n)| n.as_str()).collect()
    }
}

impl Dispatch<WlRegistry, ()> for Seen {
    fn event(
        st: &mut Self,
        _: &WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = ev
        {
            st.globals.push((name, interface));
        }
    }
}

struct Peer {
    conn: Connection,
    queue: EventQueue<Seen>,
    seen: Seen,
    registry: WlRegistry,
    qh: QueueHandle<Seen>,
}

impl Peer {
    fn connect(ls: &mut Lockstep, state: ClientState) -> Peer {
        let (server_end, client_end) = UnixStream::pair().expect("socketpair");
        client_end.set_nonblocking(true).expect("non-blocking");
        ls.display
            .handle()
            .insert_client(server_end, Arc::new(state))
            .expect("insert the client");
        let conn = Connection::from_socket(client_end).expect("connection");
        let queue = conn.new_event_queue();
        let qh = queue.handle();
        let registry = conn.display().get_registry(&qh, ());
        let mut p = Peer {
            conn,
            queue,
            seen: Seen::default(),
            registry,
            qh,
        };
        p.drive(ls);
        p
    }

    /// Exchanges messages until quiet; whether the connection is still alive.
    fn drive(&mut self, ls: &mut Lockstep) -> bool {
        let mut alive = true;
        for _ in 0..50 {
            ls.pump();
            if self.conn.flush().is_err() {
                alive = false;
            }
            if let Some(guard) = self.conn.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => alive = false,
                }
            }
            if self.queue.dispatch_pending(&mut self.seen).is_err() {
                alive = false;
            }
            ls.display
                .dispatch_clients(&mut ls.state)
                .expect("the compositor dispatches");
            ls.display.flush_clients().expect("flush");
        }
        alive && self.conn.protocol_error().is_none()
    }
}

fn scene() -> Lockstep {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 60)));
    ls.state.enable_session_lock();
    ls.pump();
    ls
}

fn sandboxed(app: &str) -> ClientState {
    ClientState::with_identity(Identity::Sandboxed {
        engine: Some("flatpak".into()),
        app_id: Some(app.into()),
    })
}

fn native_me() -> ClientState {
    ClientState::with_identity(Identity::of_pid(std::process::id() as i32))
}

fn write_config(home: &std::path::Path, text: &str) {
    let dir = home.join(".config/infynitae");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config"), text).unwrap();
}

#[test]
fn a_sandboxed_app_sees_no_privileged_global() {
    let mut ls = scene();
    let p = Peer::connect(&mut ls, sandboxed("org.example.Untrusted"));
    let names = p.seen.names();
    assert!(
        names.contains("wl_compositor"),
        "ordinary globals stay: {names:?}"
    );
    assert!(names.contains("xdg_wm_base"));
    for g in PRIVILEGED {
        assert!(!names.contains(g), "a sandboxed app saw {g}");
    }
    assert!(
        !names.contains(SECURITY_CONTEXT),
        "a sandbox cannot nest another"
    );
}

#[test]
fn a_whitelisted_sandboxed_app_gets_only_its_capabilities() {
    let mut ls = scene();
    let home = TestTempDir::new("t1_trust_whitelist");
    write_config(
        home.path(),
        "ALLOW_CAPTURE=\"native com.obsproject.Studio\"\nALLOW_CLIPBOARD=\"native org.example.Clip\"\n",
    );
    ls.state.set_config_home(home.path().to_path_buf());
    ls.state.reload_config();
    let p = Peer::connect(&mut ls, sandboxed("com.obsproject.Studio"));
    let names = p.seen.names();
    for g in &PRIVILEGED[..4] {
        assert!(
            names.contains(g),
            "the whitelisted app lacks {g}: {names:?}"
        );
    }
    for g in &PRIVILEGED[4..] {
        assert!(!names.contains(g), "capture only, yet it saw {g}");
    }
}

#[test]
fn a_native_app_has_the_defaults_and_the_lock_only_when_listed() {
    let mut ls = scene();
    let p = Peer::connect(&mut ls, native_me());
    let names = p.seen.names();
    for g in PRIVILEGED {
        if g == "ext_session_lock_manager_v1" {
            assert!(!names.contains(g), "the lock is Infynitae's own by default");
        } else {
            assert!(names.contains(g), "a native app lacks {g} by default");
        }
    }
    assert!(names.contains(SECURITY_CONTEXT));

    // by its real executable; an emptied list refuses it
    let exe = std::fs::read_link("/proc/self/exe").unwrap();
    let home = TestTempDir::new("t1_trust_native");
    write_config(
        home.path(),
        &format!(
            "ALLOW_LOCK=\"{}\"\nALLOW_CAPTURE=\"\"\nALLOW_INPUT=\"wtype\"\n",
            exe.display()
        ),
    );
    ls.state.set_config_home(home.path().to_path_buf());
    ls.state.reload_config();
    let p = Peer::connect(&mut ls, native_me());
    let names = p.seen.names();
    assert!(names.contains("ext_session_lock_manager_v1"));
    assert!(!names.contains("zwlr_screencopy_manager_v1"));
    assert!(!names.contains("zwlr_virtual_pointer_manager_v1"));
    assert!(
        names.contains("zwlr_data_control_manager_v1"),
        "untouched lists keep native"
    );
}

#[test]
fn a_refused_bind_ends_only_that_client() {
    let mut ls = scene();
    // the global's name, as a trusted client sees it
    let trusted = Peer::connect(&mut ls, native_me());
    let (name, _) = trusted
        .seen
        .globals
        .iter()
        .find(|(_, n)| n == "zwlr_virtual_pointer_manager_v1")
        .cloned()
        .expect("a native app sees the virtual pointer");
    let mut p = Peer::connect(&mut ls, sandboxed("org.example.Untrusted"));
    let _: wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 =
        p.registry.bind(name, 2, &p.qh, ());
    assert!(!p.drive(&mut ls), "the bind is refused");
    // the compositor and the other clients go on
    let again = Peer::connect(&mut ls, native_me());
    assert!(again.seen.names().contains("wl_compositor"));
}

impl
    Dispatch<
        wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
        (),
    > for Seen
{
    fn event(
        _: &mut Self,
        _: &wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
        _: <wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[test]
fn the_screen_locker_setting_may_lock() {
    let mut ls = scene();
    let exe = std::fs::read_link("/proc/self/exe").unwrap();
    let home = TestTempDir::new("t1_trust_locker");
    write_config(
        home.path(),
        &format!("SCREEN_LOCKER=\"{} --flag\"\n", exe.display()),
    );
    ls.state.set_config_home(home.path().to_path_buf());
    ls.state.reload_config();
    let p = Peer::connect(&mut ls, native_me());
    assert!(p.seen.names().contains("ext_session_lock_manager_v1"));
    let p = Peer::connect(&mut ls, sandboxed("org.example.Untrusted"));
    assert!(!p.seen.names().contains("ext_session_lock_manager_v1"));
}

/// A client through `infynitae-sandbox-socket` (live/sandbox-socket, the socket of
/// live/windows-run) is sandboxed, with the app id the helper set: a real session with a real
/// socket, the helper as built, and a client connecting to the helper's socket. Ignored by
/// default: needs `INFYNITAE_SANDBOX_SOCKET`, the helper binary (CI's job "Sandbox socket").
#[test]
#[ignore = "needs INFYNITAE_SANDBOX_SOCKET, the built live/sandbox-socket"]
fn a_client_through_the_sandbox_socket_is_sandboxed_with_its_app_id() {
    use infynitae_wayland::session::{Session, SessionOptions};
    use smithay::reexports::calloop::EventLoop;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let helper = std::env::var_os("INFYNITAE_SANDBOX_SOCKET").expect("INFYNITAE_SANDBOX_SOCKET");
    let (core, _input, _shell, _token) =
        infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
    let display =
        smithay::reexports::wayland_server::Display::<infynitae_wayland::WaylandState>::new()
            .unwrap();
    let mut state = infynitae_wayland::WaylandState::new(&display.handle(), core);
    // capture for this app id only: a sandboxed client with it sees capture and nothing else
    // privileged, where a native client would see every list's default
    let home = TestTempDir::new("t1_trust_sandbox_socket");
    write_config(
        home.path(),
        "ALLOW_CAPTURE=\"native infynitae.windows.probe\"\n",
    );
    state.set_config_home(home.path().to_path_buf());
    state.reload_config();
    let mut event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
    let mut session = Session::new(
        display,
        &event_loop.handle(),
        &SessionOptions {
            socket_name: Some(format!("infynitae-sandbox-test-{}", std::process::id())),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let display_name = session.socket_name().to_string();
    let mut pump = |state: &mut infynitae_wayland::WaylandState| {
        event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut ())
            .unwrap();
        session.dispatch(state);
    };

    let socket = home.path().join("probe.socket");
    // the helper's command waits for its input to close, then exits 7: the status passes through
    let mut child = Command::new(&helper)
        .arg(&socket)
        .args(["infynitae.windows.probe", "infynitae-windows", "--"])
        .args(["sh", "-c", "cat; exit 7"])
        .env("WAYLAND_DISPLAY", &display_name)
        .stdin(Stdio::piped())
        .spawn()
        .expect("start the helper");
    let start = Instant::now();
    while !socket.exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "no socket");
        assert!(child.try_wait().unwrap().is_none(), "the helper ended");
        pump(&mut state);
    }

    let stream = UnixStream::connect(&socket).expect("connect through the helper");
    stream.set_nonblocking(true).unwrap();
    let conn = Connection::from_socket(stream).unwrap();
    let mut queue: EventQueue<Seen> = conn.new_event_queue();
    let _registry = conn.display().get_registry(&queue.handle(), ());
    let mut seen = Seen::default();
    while start.elapsed() < Duration::from_secs(10) && !seen.names().contains("wl_compositor") {
        pump(&mut state);
        conn.flush().unwrap();
        if let Some(guard) = conn.prepare_read() {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("the connection failed: {e}"),
            }
        }
        queue.dispatch_pending(&mut seen).unwrap();
    }
    let names = seen.names();
    assert!(names.contains("xdg_wm_base"), "ordinary globals: {names:?}");
    for g in &PRIVILEGED[..4] {
        assert!(names.contains(g), "the listed app id lacks {g}: {names:?}");
    }
    for g in &PRIVILEGED[4..] {
        assert!(!names.contains(g), "a sandboxed client saw {g}");
    }
    assert!(
        !names.contains(SECURITY_CONTEXT),
        "a sandbox cannot nest another"
    );

    drop(child.stdin.take());
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the helper did not end"
        );
        pump(&mut state);
    };
    assert_eq!(status.code(), Some(7), "the command's status");
    assert!(!socket.exists(), "the socket file is removed");
}
