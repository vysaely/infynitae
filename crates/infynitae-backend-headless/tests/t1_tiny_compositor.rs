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
    activated: bool,
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
        app: &mut Self,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Configure { states, .. } = event {
            let raw_states: Vec<u32> = states
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_ne_bytes(*c))
                .collect();
            // 4 is State::Activated
            app.activated = raw_states.contains(&4);
        }
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
            device_limit: Default::default(),
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
        activated: false,
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

#[test]
fn test_tiny_compositor_child_process_reaping() {
    let socket_name = format!("infynitae-child-test-{}", std::process::id());
    let socket_name_clone = socket_name.clone();

    // Spawn a short-lived child process (sleep 0.05) through options.exec
    let compositor_handle = thread::spawn(move || {
        let (core, input_port, _shell_port, output_token) = Core::new(PersistedCameras::default());
        let options = HeadlessCompositorOptions {
            socket_name: Some(socket_name_clone),
            exec: Some("sleep 0.05".to_string()),
            width: 1280,
            height: 800,
            refresh_mhz: 60_000,
            duration: Some(Duration::from_millis(600)),
            max_frames: None,
            device_limit: Default::default(),
        };
        run_headless(core, input_port, output_token, options).expect("compositor exited cleanly");
    });

    compositor_handle.join().expect("compositor thread join");
}

#[test]
fn test_tiny_compositor_graceful_child_termination() {
    let socket_name = format!("infynitae-graceful-child-test-{}", std::process::id());
    let socket_name_clone = socket_name.clone();

    // Spawn a long-running child process (sleep 60) that exits immediately upon SIGTERM
    let start = std::time::Instant::now();
    let compositor_handle = thread::spawn(move || {
        let (core, input_port, _shell_port, output_token) = Core::new(PersistedCameras::default());
        let options = HeadlessCompositorOptions {
            socket_name: Some(socket_name_clone),
            exec: Some("sleep 60".to_string()),
            width: 1280,
            height: 800,
            refresh_mhz: 60_000,
            duration: Some(Duration::from_millis(150)),
            max_frames: None,
            device_limit: Default::default(),
        };
        run_headless(core, input_port, output_token, options).expect("compositor exited cleanly");
    });

    compositor_handle.join().expect("compositor thread join");
    let elapsed = start.elapsed();
    // Must terminate within bounded time (far less than 60s)
    assert!(
        elapsed < Duration::from_secs(2),
        "graceful child termination took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_tiny_compositor_client_event_loop_wakeup() {
    let socket_name = format!("infynitae-wakeup-test-{}", std::process::id());
    let socket_name_clone = socket_name.clone();

    // Run compositor with very low refresh rate: 1 Hz (1000ms frame interval!)
    let compositor_handle = thread::spawn(move || {
        let (core, input_port, _shell_port, output_token) = Core::new(PersistedCameras::default());
        let options = HeadlessCompositorOptions {
            socket_name: Some(socket_name_clone),
            exec: None,
            width: 1280,
            height: 800,
            refresh_mhz: 1_000, // 1 Hz = 1000ms frame interval
            duration: Some(Duration::from_secs(3)),
            max_frames: None,
            device_limit: Default::default(),
        };
        run_headless(core, input_port, output_token, options).expect("compositor exited cleanly");
    });

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
    assert!(socket_path.exists());

    let stream = std::os::unix::net::UnixStream::connect(&socket_path).unwrap();
    let conn = Connection::from_socket(stream).unwrap();
    let mut event_queue: EventQueue<ClientApp> = conn.new_event_queue();
    let qh = event_queue.handle();

    let mut app = ClientApp {
        compositor: None,
        shm: None,
        wm_base: None,
        configured: false,
        activated: false,
        frame_done: false,
    };

    let _registry = conn.display().get_registry(&qh, ());
    event_queue.roundtrip(&mut app).unwrap();

    let compositor = app.compositor.as_ref().unwrap();
    let surface = compositor.create_surface(&qh, ());
    let wm_base = app.wm_base.as_ref().unwrap();
    let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg_surface.get_toplevel(&qh, ());
    surface.commit();

    // Measure time to receive initial configure
    let start = std::time::Instant::now();
    event_queue.roundtrip(&mut app).unwrap();
    let elapsed = start.elapsed();

    assert!(app.configured);
    // Because display_fd is registered with Calloop, the client commit woke the compositor
    // immediately without waiting for the 1000ms vblank period!
    assert!(
        elapsed < Duration::from_millis(350),
        "Client commit must wake event loop immediately (took {:?}), not wait for 1000ms timer",
        elapsed
    );

    drop(surface);
    drop(xdg_surface);
    drop(toplevel);
    drop(conn);
    compositor_handle.join().unwrap();
}

#[test]
fn test_tiny_compositor_initial_focus_and_activation() {
    let socket_name = format!("infynitae-focus-test-{}", std::process::id());
    let socket_name_clone = socket_name.clone();

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
            device_limit: Default::default(),
        };
        run_headless(core, input_port, output_token, options).expect("compositor exited cleanly");
    });

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
    assert!(socket_path.exists());

    // Connect Client 1
    let stream1 = std::os::unix::net::UnixStream::connect(&socket_path).unwrap();
    let conn1 = Connection::from_socket(stream1).unwrap();
    let mut eq1: EventQueue<ClientApp> = conn1.new_event_queue();
    let qh1 = eq1.handle();

    let mut app1 = ClientApp {
        compositor: None,
        shm: None,
        wm_base: None,
        configured: false,
        activated: false,
        frame_done: false,
    };
    let _ = conn1.display().get_registry(&qh1, ());
    eq1.roundtrip(&mut app1).unwrap();

    let s1 = app1.compositor.as_ref().unwrap().create_surface(&qh1, ());
    let xs1 = app1
        .wm_base
        .as_ref()
        .unwrap()
        .get_xdg_surface(&s1, &qh1, ());
    let tl1 = xs1.get_toplevel(&qh1, ());
    s1.commit();

    // Roundtrip to receive initial configure
    eq1.roundtrip(&mut app1).unwrap();
    assert!(app1.configured);

    let shm1 = app1.shm.as_ref().unwrap();
    let temp_path1 = std::env::temp_dir().join(format!("infynitae-shm-c1-{}", std::process::id()));
    let temp_file1 = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path1)
        .unwrap();
    let _ = std::fs::remove_file(&temp_path1);
    temp_file1.set_len(400).unwrap();
    let pool1 = shm1.create_pool(temp_file1.as_fd(), 400, &qh1, ());
    let buffer1 = pool1.create_buffer(0, 10, 10, 40, wl_shm::Format::Argb8888, &qh1, ());
    s1.attach(Some(&buffer1), 0, 0);
    s1.damage(0, 0, 10, 10);
    s1.commit();

    // Poll until app1 is configured and activated
    for _ in 0..50 {
        eq1.roundtrip(&mut app1).ok();
        if app1.activated {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        app1.activated,
        "First mapped toplevel must receive initial focus activation grant"
    );

    // Connect Client 2
    let stream2 = std::os::unix::net::UnixStream::connect(&socket_path).unwrap();
    let conn2 = Connection::from_socket(stream2).unwrap();
    let mut eq2: EventQueue<ClientApp> = conn2.new_event_queue();
    let qh2 = eq2.handle();

    let mut app2 = ClientApp {
        compositor: None,
        shm: None,
        wm_base: None,
        configured: false,
        activated: false,
        frame_done: false,
    };
    let _ = conn2.display().get_registry(&qh2, ());
    eq2.roundtrip(&mut app2).unwrap();

    let s2 = app2.compositor.as_ref().unwrap().create_surface(&qh2, ());
    let xs2 = app2
        .wm_base
        .as_ref()
        .unwrap()
        .get_xdg_surface(&s2, &qh2, ());
    let tl2 = xs2.get_toplevel(&qh2, ());
    s2.commit();

    // Roundtrip to receive initial configure
    eq2.roundtrip(&mut app2).unwrap();
    assert!(app2.configured);

    let shm2 = app2.shm.as_ref().unwrap();
    let temp_path2 = std::env::temp_dir().join(format!("infynitae-shm-c2-{}", std::process::id()));
    let temp_file2 = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path2)
        .unwrap();
    let _ = std::fs::remove_file(&temp_path2);
    temp_file2.set_len(400).unwrap();
    let pool2 = shm2.create_pool(temp_file2.as_fd(), 400, &qh2, ());
    let buffer2 = pool2.create_buffer(0, 10, 10, 40, wl_shm::Format::Argb8888, &qh2, ());
    s2.attach(Some(&buffer2), 0, 0);
    s2.damage(0, 0, 10, 10);
    s2.commit();

    for _ in 0..20 {
        eq2.roundtrip(&mut app2).ok();
        thread::sleep(Duration::from_millis(20));
    }

    assert!(
        !app2.activated,
        "Second mapped toplevel must NOT steal activation from existing focused window"
    );

    drop(s1);
    drop(xs1);
    drop(tl1);
    drop(conn1);
    drop(s2);
    drop(xs2);
    drop(tl2);
    drop(conn2);
    compositor_handle.join().unwrap();
}

// ---------------------------------------------------------------- presentation clock (F2)

use wayland_protocols::wp::presentation_time::client::{
    wp_presentation::{self, WpPresentation},
    wp_presentation_feedback::{self, WpPresentationFeedback},
};

#[derive(Default)]
struct PresentationClient {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    wm_base: Option<XdgWmBase>,
    presentation: Option<WpPresentation>,
    configured: bool,
    clock_id: Option<u32>,
    /// (seconds, nanoseconds, refresh in ns) of each `presented` event.
    presented: Vec<(u64, u32, u32)>,
}

impl Dispatch<WlRegistry, ()> for PresentationClient {
    fn event(
        app: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => app.compositor = Some(registry.bind(name, 4, qh, ())),
                "wl_shm" => app.shm = Some(registry.bind(name, 1, qh, ())),
                "xdg_wm_base" => app.wm_base = Some(registry.bind(name, 1, qh, ())),
                "wp_presentation" => app.presentation = Some(registry.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(PresentationClient: ignore WlCompositor);
delegate_noop!(PresentationClient: ignore WlSurface);
delegate_noop!(PresentationClient: ignore WlShm);
delegate_noop!(PresentationClient: ignore WlShmPool);
delegate_noop!(PresentationClient: ignore WlBuffer);
delegate_noop!(PresentationClient: ignore XdgToplevel);

impl Dispatch<XdgWmBase, ()> for PresentationClient {
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

impl Dispatch<XdgSurface, ()> for PresentationClient {
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

impl Dispatch<WpPresentation, ()> for PresentationClient {
    fn event(
        app: &mut Self,
        _: &WpPresentation,
        event: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_presentation::Event::ClockId { clk_id } = event {
            app.clock_id = Some(clk_id);
        }
    }
}

impl Dispatch<WpPresentationFeedback, ()> for PresentationClient {
    fn event(
        app: &mut Self,
        _: &WpPresentationFeedback,
        event: wp_presentation_feedback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_presentation_feedback::Event::Presented {
            tv_sec_hi,
            tv_sec_lo,
            tv_nsec,
            refresh,
            ..
        } = event
        {
            app.presented.push((
                (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo),
                tv_nsec,
                refresh,
            ));
        }
    }
}

/// The client's own reading of CLOCK_MONOTONIC.
fn monotonic_now() -> Duration {
    use smithay::reexports::rustix::time::{ClockId, clock_gettime};
    let t = clock_gettime(ClockId::Monotonic);
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

/// `wp_presentation` announces a clock, and `presented` timestamps are instants on that clock
/// (presentation-time: "the clock ... used for the timestamps"). A client that reads the
/// announced clock before the commit and after the event must find the timestamp in between:
/// a timestamp on any other time base (for example seconds since the compositor started) is
/// outside that window unless the machine booted within the last few seconds.
#[test]
fn presentation_timestamps_are_on_the_announced_clock() {
    let socket_name = format!("infynitae-presentation-test-{}", std::process::id());
    let options = HeadlessCompositorOptions {
        socket_name: Some(socket_name.clone()),
        exec: None,
        width: 1280,
        height: 800,
        refresh_mhz: 60_000,
        duration: Some(Duration::from_secs(3)),
        max_frames: None,
        device_limit: Default::default(),
    };
    let compositor = thread::spawn(move || {
        let (core, input_port, _shell_port, output_token) = Core::new(PersistedCameras::default());
        run_headless(core, input_port, output_token, options).expect("compositor exited cleanly");
    });
    let socket_path =
        std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR is set"))
            .join(&socket_name);
    for _ in 0..100 {
        if socket_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let stream = std::os::unix::net::UnixStream::connect(&socket_path).expect("connect");
    let conn = Connection::from_socket(stream).expect("client connection");
    let mut queue: EventQueue<PresentationClient> = conn.new_event_queue();
    let qh = queue.handle();
    let mut app = PresentationClient::default();
    let _registry = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut app).expect("globals");
    queue.roundtrip(&mut app).expect("clock id");
    assert_eq!(app.clock_id, Some(1), "CLOCK_MONOTONIC is announced");

    let surface = app.compositor.as_ref().unwrap().create_surface(&qh, ());
    let xdg_surface = app
        .wm_base
        .as_ref()
        .unwrap()
        .get_xdg_surface(&surface, &qh, ());
    let _toplevel = xdg_surface.get_toplevel(&qh, ());
    surface.commit();
    queue.roundtrip(&mut app).expect("configure");
    assert!(app.configured);

    let file = std::fs::File::from(
        smithay::reexports::rustix::fs::memfd_create(
            "infynitae-presentation-test",
            smithay::reexports::rustix::fs::MemfdFlags::CLOEXEC,
        )
        .expect("memfd"),
    );
    file.set_len(64 * 64 * 4).expect("size");
    let pool = app
        .shm
        .as_ref()
        .unwrap()
        .create_pool(file.as_fd(), 64 * 64 * 4, &qh, ());
    let buffer = pool.create_buffer(0, 64, 64, 256, wl_shm::Format::Argb8888, &qh, ());

    let presentation = app
        .presentation
        .clone()
        .expect("wp_presentation is advertised");
    for _ in 0..3 {
        let count = app.presented.len();
        let before = monotonic_now();
        presentation.feedback(&surface, &qh, ());
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, 64, 64);
        surface.commit();
        for _ in 0..200 {
            queue.roundtrip(&mut app).expect("roundtrip");
            if app.presented.len() > count {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let after = monotonic_now();
        assert_eq!(
            app.presented.len(),
            count + 1,
            "one presented event per feedback"
        );
        let (sec, nsec, refresh) = app.presented[count];
        let at = Duration::new(sec, nsec);
        assert!(
            before <= at && at <= after,
            "presented at {at:?} on the announced clock, outside the client's window \
             [{before:?}, {after:?}]"
        );
        assert_eq!(
            refresh, 16_666_666,
            "refresh of a 60 Hz output in nanoseconds"
        );
    }
    drop(conn);
    compositor.join().expect("compositor thread");
}

/// The command started with the session is a child of the session: it is running while the
/// session runs and gone (not running, not a zombie) when `run_headless` returns. The command
/// is a shell that records its own pid and then sleeps.
#[test]
fn the_session_command_does_not_outlive_the_session() {
    let dir = std::env::temp_dir().join(format!("infynitae-child-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let script = dir.join("child.sh");
    let pid_file = dir.join("pid");
    std::fs::write(
        &script,
        // stdio detached and a short sleep: if the session fails to end it, the leftover
        // neither holds the test runner's output pipe nor lingers
        format!(
            "#!/bin/sh\necho $$ > {}\nexec sleep 30 </dev/null >/dev/null 2>&1\n",
            pid_file.display()
        ),
    )
    .expect("script");
    let options = HeadlessCompositorOptions {
        socket_name: Some(format!("infynitae-own-child-test-{}", std::process::id())),
        exec: Some(format!("sh {}", script.display())),
        width: 1280,
        height: 800,
        refresh_mhz: 60_000,
        duration: Some(Duration::from_millis(700)),
        max_frames: None,
        device_limit: Default::default(),
    };
    let compositor = thread::spawn(move || {
        let (core, input_port, _shell_port, output_token) = Core::new(PersistedCameras::default());
        run_headless(core, input_port, output_token, options).expect("compositor exited cleanly");
    });
    let mut pid = None;
    for _ in 0..60 {
        if let Ok(text) = std::fs::read_to_string(&pid_file)
            && let Ok(p) = text.trim().parse::<u32>()
        {
            pid = Some(p);
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let pid = pid.expect("the session command started");
    let proc_dir = std::path::PathBuf::from(format!("/proc/{pid}"));
    assert!(proc_dir.exists(), "the command runs while the session runs");
    compositor.join().expect("compositor thread");
    assert!(
        !proc_dir.exists(),
        "process {pid} outlived the session (running or zombie)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
