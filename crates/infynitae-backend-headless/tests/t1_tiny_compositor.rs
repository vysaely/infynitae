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
        };
        run_headless(core, input_port, output_token, options).expect("compositor exited cleanly");
    });

    compositor_handle.join().expect("compositor thread join");
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
