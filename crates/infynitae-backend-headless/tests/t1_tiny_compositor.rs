//! End-to-end integration test of the runnable tiny compositor.
//!
//! Validates:
//! 1. Starting compositor with a real Wayland listening socket.
//! 2. Connecting a real Wayland client over the socket.
//! 3. Advertising globals (wl_compositor, wl_shm, xdg_wm_base, wl_seat, etc.).
//! 4. Mapping an xdg-toplevel surface with a shm buffer.
//! 5. Frame tick / presentation callback delivery.
//! 6. Clean client disconnect and compositor shutdown.

use std::os::fd::AsFd;
use std::thread;
use std::time::Duration;

use infynitae_backend_headless::{HeadlessCompositorOptions, run_headless};
use infynitae_core::Core;
use infynitae_core::navigation::PersistedCameras;

use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_callback::{self, WlCallback},
    wl_compositor::WlCompositor,
    wl_registry::{self, WlRegistry},
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};

struct ClientApp {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    wm_base: Option<XdgWmBase>,
    configured: bool,
    frame_done: bool,
}

impl Dispatch<WlRegistry, ()> for ClientApp {
    fn event(
        app: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    app.compositor = Some(registry.bind(name, version.min(6), qh, ()));
                }
                "wl_shm" => {
                    app.shm = Some(registry.bind(name, 1, qh, ()));
                }
                "xdg_wm_base" => {
                    app.wm_base = Some(registry.bind(name, version.min(6), qh, ()));
                }
                _ => {}
            }
        }
    }
}

delegate_noop!(ClientApp: ignore WlCompositor);
delegate_noop!(ClientApp: ignore WlSurface);
delegate_noop!(ClientApp: ignore WlShm);
delegate_noop!(ClientApp: ignore WlShmPool);
delegate_noop!(ClientApp: ignore WlBuffer);

impl Dispatch<XdgWmBase, ()> for ClientApp {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for ClientApp {
    fn event(
        app: &mut Self,
        xdg_surface: &XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg_surface.ack_configure(serial);
            app.configured = true;
        }
    }
}

impl Dispatch<XdgToplevel, ()> for ClientApp {
    fn event(
        _: &mut Self,
        _: &XdgToplevel,
        _: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlCallback, ()> for ClientApp {
    fn event(
        app: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            app.frame_done = true;
        }
    }
}

#[test]
fn test_tiny_compositor_end_to_end_client_session() {
    let socket_name = format!("infynitae-test-{}", std::process::id());
    let socket_name_clone = socket_name.clone();

    // Spawn compositor in background thread
    let compositor_handle = thread::spawn(move || {
        let (core, input_port, _shell_port, output_token) = Core::new(PersistedCameras::default());
        let options = HeadlessCompositorOptions {
            socket_name: Some(socket_name_clone),
            exec: None,
            width: 1280,
            height: 800,
            refresh_mhz: 60_000,
            duration: Some(Duration::from_secs(3)),
            max_frames: None,
        };
        run_headless(core, input_port, output_token, options).expect("compositor exited cleanly");
    });

    // Wait briefly for socket to become available
    let socket_path = std::env::var("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
        .join(&socket_name);

    for _ in 0..50 {
        if socket_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        socket_path.exists(),
        "Wayland socket was not created in time"
    );

    // Connect real Wayland client
    let stream = std::os::unix::net::UnixStream::connect(&socket_path)
        .expect("connect to compositor socket");
    let conn = Connection::from_socket(stream).expect("create client connection");
    let mut event_queue: EventQueue<ClientApp> = conn.new_event_queue();
    let qh = event_queue.handle();

    let mut app = ClientApp {
        compositor: None,
        shm: None,
        wm_base: None,
        configured: false,
        frame_done: false,
    };

    let _registry = conn.display().get_registry(&qh, ());
    event_queue.roundtrip(&mut app).expect("roundtrip globals");

    assert!(app.compositor.is_some(), "wl_compositor must be advertised");
    assert!(app.shm.is_some(), "wl_shm must be advertised");
    assert!(app.wm_base.is_some(), "xdg_wm_base must be advertised");

    // Create surface and xdg-toplevel
    let compositor = app.compositor.as_ref().unwrap();
    let surface = compositor.create_surface(&qh, ());
    let wm_base = app.wm_base.as_ref().unwrap();
    let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg_surface.get_toplevel(&qh, ());
    toplevel.set_title("Test Client".to_string());
    surface.commit();

    // Roundtrip to receive initial configure
    event_queue
        .roundtrip(&mut app)
        .expect("roundtrip configure");
    assert!(
        app.configured,
        "xdg_surface must be configured by compositor"
    );

    // Create shm buffer and attach
    let shm = app.shm.as_ref().unwrap();
    let temp_path = std::env::temp_dir().join(format!("infynitae-shm-{}", std::process::id()));
    let temp_file = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path)
        .expect("shm file");
    let _ = std::fs::remove_file(&temp_path);
    let pool_size = 100 * 100 * 4;
    temp_file.set_len(pool_size as u64).expect("size shm file");
    let pool = shm.create_pool(temp_file.as_fd(), pool_size, &qh, ());
    let buffer = pool.create_buffer(0, 100, 100, 400, wl_shm::Format::Argb8888, &qh, ());

    surface.attach(Some(&buffer), 0, 0);
    surface.damage(0, 0, 100, 100);

    // Request frame callback
    let _frame_cb = surface.frame(&qh, ());
    surface.commit();
    conn.flush().expect("client flush");

    // Wait for frame callback done event
    for _ in 0..100 {
        event_queue.roundtrip(&mut app).ok();
        if app.frame_done {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        app.frame_done,
        "frame callback must be completed by compositor vblank"
    );

    // Disconnect client cleanly
    drop(surface);
    drop(xdg_surface);
    drop(toplevel);
    drop(conn);

    // Wait for compositor thread to finish cleanly
    compositor_handle.join().expect("compositor thread join");
}
