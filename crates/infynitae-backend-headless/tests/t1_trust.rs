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
