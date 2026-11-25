//! The desktop journal (infynitae-wayland `journal`): each source the compositor sees writes its
//! event once, naming the program by its real executable and its pid. A test client here is
//! known by this test binary's own executable, as a native app is known by its own.

mod common;

use std::os::unix::net::UnixStream;
use std::sync::Arc;

use common::{Lockstep, output};
use infynitae_wayland::ClientState;
use infynitae_wayland::cursor::TestTempDir;
use infynitae_wayland::trust::Identity;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1::ExtDataControlDeviceV1;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_manager_v2::ZwpInputMethodManagerV2;
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_v2::ZwpInputMethodV2;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1::ZwlrDataControlDeviceV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

#[derive(Default)]
struct Seen {
    globals: Vec<(u32, String)>,
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

wayland_client::delegate_noop!(Seen: ignore WlSeat);
wayland_client::delegate_noop!(Seen: ignore ZwlrVirtualPointerManagerV1);
wayland_client::delegate_noop!(Seen: ignore ZwlrVirtualPointerV1);
wayland_client::delegate_noop!(Seen: ignore ZwpVirtualKeyboardManagerV1);
wayland_client::delegate_noop!(Seen: ignore ZwpVirtualKeyboardV1);
wayland_client::delegate_noop!(Seen: ignore ZwlrDataControlManagerV1);
wayland_client::delegate_noop!(Seen: ignore ZwlrDataControlDeviceV1);
wayland_client::delegate_noop!(Seen: ignore ExtDataControlManagerV1);
wayland_client::delegate_noop!(Seen: ignore ExtDataControlDeviceV1);
wayland_client::delegate_noop!(Seen: ignore ZwpInputMethodManagerV2);
wayland_client::delegate_noop!(Seen: ignore ZwpInputMethodV2);

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

    fn drive(&mut self, ls: &mut Lockstep) {
        for _ in 0..20 {
            ls.pump();
            self.conn.flush().expect("flush");
            if let Some(guard) = self.conn.prepare_read() {
                let _ = guard.read();
            }
            self.queue
                .dispatch_pending(&mut self.seen)
                .expect("dispatch");
            ls.display
                .dispatch_clients(&mut ls.state)
                .expect("the compositor dispatches");
            ls.display.flush_clients().expect("flush");
        }
        assert!(self.conn.protocol_error().is_none(), "the client lives");
    }

    fn bind<I>(&self, interface: &str) -> I
    where
        I: wayland_client::Proxy + 'static,
        Seen: Dispatch<I, ()>,
    {
        let (name, _) = self
            .seen
            .globals
            .iter()
            .find(|(_, i)| i == interface)
            .unwrap_or_else(|| panic!("{interface} is offered"));
        self.registry.bind(*name, 1, &self.qh, ())
    }
}

/// This test binary as a native client: its own executable and pid.
fn me() -> ClientState {
    let state = ClientState::with_identity(Identity::of_pid(std::process::id() as i32));
    state.set_pid(std::process::id() as i32);
    state
}

fn my_program() -> String {
    std::fs::read_link("/proc/self/exe")
        .unwrap()
        .display()
        .to_string()
}

/// A scene whose journal is written to its own folder.
fn scene(dir: &TestTempDir) -> Lockstep {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 60)));
    ls.state.open_journal(dir.path()).expect("open the journal");
    ls.pump();
    ls
}

/// The journal's lines for `event`, each split into its six fields.
fn lines(ls: &Lockstep, dir: &TestTempDir, event: &str) -> Vec<Vec<String>> {
    ls.state.journal().flush();
    let text = std::fs::read_to_string(infynitae_wayland::journal::current_file(dir.path()))
        .unwrap_or_default();
    text.lines()
        .map(|l| l.split('\t').map(str::to_owned).collect::<Vec<_>>())
        .filter(|f| f.get(1).map(String::as_str) == Some(event))
        .collect()
}

/// The one line for `event`, by this test binary.
fn written_once_by_me(ls: &Lockstep, dir: &TestTempDir, event: &str) -> Vec<String> {
    let found = lines(ls, dir, event);
    assert_eq!(found.len(), 1, "{event}: {found:?}");
    let f = found.into_iter().next().unwrap();
    assert_eq!(f.len(), 6, "{f:?}");
    assert_eq!(f[2], my_program(), "the program by its executable");
    assert_eq!(f[3], std::process::id().to_string(), "its pid");
    f
}

#[test]
fn a_virtual_pointer_and_keyboard_are_written_once_each() {
    let dir = TestTempDir::new("t1_journal_virtual");
    let mut ls = scene(&dir);
    let mut p = Peer::connect(&mut ls, me());
    let seat: WlSeat = p.bind("wl_seat");
    let pointers: ZwlrVirtualPointerManagerV1 = p.bind("zwlr_virtual_pointer_manager_v1");
    let _pointer = pointers.create_virtual_pointer(Some(&seat), &p.qh, ());
    let keyboards: ZwpVirtualKeyboardManagerV1 = p.bind("zwp_virtual_keyboard_manager_v1");
    let _keyboard = keyboards.create_virtual_keyboard(&seat, &p.qh, ());
    p.drive(&mut ls);
    let f = written_once_by_me(&ls, &dir, "virtual-pointer");
    assert_eq!(f[5], "allowed");
    written_once_by_me(&ls, &dir, "virtual-keyboard");
}

#[test]
fn a_clipboard_reader_is_written_once_per_device() {
    let dir = TestTempDir::new("t1_journal_clipboard");
    let mut ls = scene(&dir);
    let mut p = Peer::connect(&mut ls, me());
    let seat: WlSeat = p.bind("wl_seat");
    let wlr: ZwlrDataControlManagerV1 = p.bind("zwlr_data_control_manager_v1");
    let _wlr_device = wlr.get_data_device(&seat, &p.qh, ());
    p.drive(&mut ls);
    let f = written_once_by_me(&ls, &dir, "clipboard-reader");
    assert!(f[4].starts_with("wlr-data-control"), "{f:?}");
    let ext: ExtDataControlManagerV1 = p.bind("ext_data_control_manager_v1");
    let _ext_device = ext.get_data_device(&seat, &p.qh, ());
    p.drive(&mut ls);
    let found = lines(&ls, &dir, "clipboard-reader");
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[1][4].starts_with("ext-data-control"), "{found:?}");
}

#[test]
fn an_input_method_is_written_once() {
    let dir = TestTempDir::new("t1_journal_input_method");
    let mut ls = scene(&dir);
    let mut p = Peer::connect(&mut ls, me());
    let seat: WlSeat = p.bind("wl_seat");
    let manager: ZwpInputMethodManagerV2 = p.bind("zwp_input_method_manager_v2");
    let _im = manager.get_input_method(&seat, &p.qh, ());
    p.drive(&mut ls);
    written_once_by_me(&ls, &dir, "input-method");
}

/// A refused global is hidden, so the client is asked about each time it lists the globals: the
/// journal says it once per program and capability.
#[test]
fn a_refused_global_is_written_once_per_program_and_capability() {
    let dir = TestTempDir::new("t1_journal_refused");
    let mut ls = scene(&dir);
    let sandboxed = || {
        ClientState::with_identity(Identity::Sandboxed {
            engine: Some("flatpak".into()),
            app_id: Some("org.example.Untrusted".into()),
        })
    };
    let _a = Peer::connect(&mut ls, sandboxed());
    let _b = Peer::connect(&mut ls, sandboxed());
    let found = lines(&ls, &dir, "global-refused");
    // the session lock is not offered in this scene: seven capabilities
    let mut keys: Vec<&str> = found
        .iter()
        .map(|f| {
            assert_eq!(f[2], "org.example.Untrusted (sandboxed by flatpak)");
            assert_eq!(f[5], "refused");
            f[4].split(": not on ")
                .nth(1)
                .and_then(|r| r.split(',').next())
                .unwrap()
        })
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "ALLOW_CAPTURE",
            "ALLOW_CLIPBOARD",
            "ALLOW_DISPLAY",
            "ALLOW_INPUT",
            "ALLOW_INPUT_METHOD",
            "ALLOW_LAYERS",
            "ALLOW_WINDOWS"
        ],
        "{found:?}"
    );
    // the same app again writes nothing new, and an allowed native app nothing at all
    let _c = Peer::connect(&mut ls, me());
    assert_eq!(lines(&ls, &dir, "global-refused").len(), 7);
}
