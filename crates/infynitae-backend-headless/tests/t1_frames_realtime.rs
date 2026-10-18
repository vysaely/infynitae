//! INC-8 exit test TST-11 (T1 part), OB-16 (CR-7): a real-time headless harness with a
//! callback-blocked client.
//!
//! The production Core, adapter and headless backend run on the test thread in real time: a
//! 60 Hz virtual vblank on the presentation clock, the off-screen fallback evaluated every loop
//! iteration, client requests dispatched as they arrive. The client runs in its own thread with
//! its own connection and behaves like a Mesa WSI FIFO swapchain: after committing a frame it
//! blocks on a *private* event queue until that frame's callback completes, and only then
//! processes its main queue, where input waits. Pure withholding blocks it for the whole
//! off-screen period (EB-9: vkcube pings unanswered for 2–4 s); the kick must unblock it before
//! the key is delivered.
//!
//! Pass criterion (OB-16): the client's first response to a key it receives while focused and
//! off-screen comes within one frame (16.7 ms at 60 Hz) of the key's injection. The negative
//! control (kick disabled) waits for the fallback instead (seeded in verify-test-efficacy).

use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use infynitae_backend_headless::{
    VirtualOutput, connect, keyboard_key, pointer_button, pointer_motion, set_focus, vblank,
    vblank_period,
};
use infynitae_core::Core;
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::input::keys::{KEY_A, KEY_LEFTMETA};
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::{OutputId, Transform};
use infynitae_wayland::{ClientState, WaylandState, presentation_clock_now};
use smithay::reexports::wayland_server::Display;
use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_callback::{self, WlCallback},
    wl_compositor::WlCompositor,
    wl_keyboard::{self, WlKeyboard},
    wl_registry::{self, WlRegistry},
    wl_seat::WlSeat,
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::XdgToplevel,
    xdg_wm_base::{self, XdgWmBase},
};

const BTN_MIDDLE: u32 = 0x112;
const FRAME: Duration = Duration::from_nanos(16_666_667);

/// What the client thread reports.
#[derive(Debug)]
enum Report {
    Mapped,
    /// Committed a frame and is now blocked on its callback.
    Blocked,
    /// When its blocked frame's callback completed and when it handled the key.
    Responded {
        done: Instant,
        key: Instant,
    },
}

// ---- the client ------------------------------------------------------------------------------

#[derive(Default)]
struct Client {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    wm_base: Option<XdgWmBase>,
    seat: Option<WlSeat>,
    keyboard: Option<WlKeyboard>,
    configured: Option<u32>,
    key: Option<Instant>,
}

/// The private queue's state: the blocked frame's callback.
#[derive(Default)]
struct Fifo {
    done: Option<Instant>,
}

impl Dispatch<WlRegistry, ()> for Client {
    fn event(
        st: &mut Self,
        r: &WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ev
        {
            match interface.as_str() {
                "wl_compositor" => st.compositor = Some(r.bind(name, version.min(6), qh, ())),
                "wl_shm" => st.shm = Some(r.bind(name, 1, qh, ())),
                "xdg_wm_base" => st.wm_base = Some(r.bind(name, version.min(6), qh, ())),
                "wl_seat" => st.seat = Some(r.bind(name, version.min(7), qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<XdgWmBase, ()> for Client {
    fn event(
        _: &mut Self,
        b: &XdgWmBase,
        ev: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = ev {
            b.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for Client {
    fn event(
        st: &mut Self,
        _: &XdgSurface,
        ev: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = ev {
            st.configured = Some(serial);
        }
    }
}

impl Dispatch<WlKeyboard, ()> for Client {
    fn event(
        st: &mut Self,
        _: &WlKeyboard,
        ev: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // only the test's key: Super, held for the pan, is client-visible (K3)
        if let wl_keyboard::Event::Key { key, .. } = ev
            && key == KEY_A
        {
            st.key.get_or_insert_with(Instant::now);
        }
    }
}

impl Dispatch<WlCallback, ()> for Fifo {
    fn event(
        st: &mut Self,
        _: &WlCallback,
        ev: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = ev {
            st.done.get_or_insert_with(Instant::now);
        }
    }
}

delegate_noop!(Client: ignore WlCompositor);
delegate_noop!(Client: ignore WlShm);
delegate_noop!(Client: ignore WlShmPool);
delegate_noop!(Client: ignore WlBuffer);
delegate_noop!(Client: ignore WlSurface);
delegate_noop!(Client: ignore WlSeat);
delegate_noop!(Client: ignore XdgToplevel);

fn buffer(c: &Client, qh: &QueueHandle<Client>, w: i32, h: i32) -> WlBuffer {
    let len = (w * h * 4) as usize;
    let fd = smithay::reexports::rustix::fs::memfd_create(
        "infynitae-t1-frames-realtime",
        smithay::reexports::rustix::fs::MemfdFlags::CLOEXEC,
    )
    .expect("memfd");
    let file = std::fs::File::from(fd);
    file.set_len(len as u64).expect("size");
    let pool = c
        .shm
        .as_ref()
        .expect("wl_shm")
        .create_pool(file.as_fd(), len as i32, qh, ());
    pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, qh, ())
}

/// A FIFO-presenting client: maps a 400x300 window, then on `go` commits a frame and blocks on
/// its callback (private queue) before it looks at input.
fn client(socket: UnixStream, report: Sender<Report>, go: Receiver<()>) {
    let conn = Connection::from_socket(socket).expect("connection");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let mut c = Client::default();
    conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut c).expect("globals");
    c.keyboard = Some(c.seat.as_ref().expect("seat").get_keyboard(&qh, ()));
    let surface = c
        .compositor
        .as_ref()
        .expect("compositor")
        .create_surface(&qh, ());
    let xdg = c
        .wm_base
        .as_ref()
        .expect("xdg_wm_base")
        .get_xdg_surface(&surface, &qh, ());
    let _toplevel = xdg.get_toplevel(&qh, ());
    surface.commit();
    while c.configured.is_none() {
        queue.blocking_dispatch(&mut c).expect("configure");
    }
    xdg.ack_configure(c.configured.take().unwrap());
    surface.attach(Some(&buffer(&c, &qh, 400, 300)), 0, 0);
    surface.commit();
    conn.flush().expect("flush");
    report.send(Report::Mapped).unwrap();

    go.recv().unwrap();
    queue.roundtrip(&mut c).expect("settle");
    // the FIFO present: the frame's callback is on a private queue, and the client waits on it
    let mut fifo_queue = conn.new_event_queue();
    let fqh = fifo_queue.handle();
    let mut fifo = Fifo::default();
    surface.frame(&fqh, ());
    surface.attach(Some(&buffer(&c, &qh, 400, 300)), 0, 0);
    surface.damage_buffer(0, 0, 400, 300);
    surface.commit();
    conn.flush().expect("flush");
    report.send(Report::Blocked).unwrap();
    while fifo.done.is_none() {
        fifo_queue
            .blocking_dispatch(&mut fifo)
            .expect("blocked present");
    }
    // unblocked: the main loop runs and sees its input
    while c.key.is_none() {
        queue.blocking_dispatch(&mut c).expect("input");
    }
    report
        .send(Report::Responded {
            done: fifo.done.unwrap(),
            key: c.key.unwrap(),
        })
        .unwrap();
    // stay connected until the compositor side has read the report
    let _ = go.recv();
}

// ---- the compositor --------------------------------------------------------------------------

struct Compositor {
    display: Display<WaylandState>,
    state: WaylandState,
    output: OutputId,
    next_vblank: Instant,
}

impl Compositor {
    /// One loop iteration: client requests, the vblank if due, the fallback evaluation.
    fn iterate(&mut self) {
        self.display
            .dispatch_clients(&mut self.state)
            .expect("dispatch");
        if Instant::now() >= self.next_vblank {
            vblank(&mut self.state, self.output, presentation_clock_now());
            self.next_vblank += vblank_period(60_000);
        }
        let _ = self.state.frame_fallback(presentation_clock_now());
        self.display.flush_clients().expect("flush");
        thread::sleep(Duration::from_micros(200));
    }

    /// Iterates until `rx` yields a report or `limit` passes.
    fn until<T>(&mut self, rx: &Receiver<T>, limit: Duration) -> Option<T> {
        let end = Instant::now() + limit;
        while Instant::now() < end {
            self.iterate();
            if let Ok(r) = rx.try_recv() {
                return Some(r);
            }
        }
        None
    }

    fn run_for(&mut self, d: Duration) {
        let end = Instant::now() + d;
        while Instant::now() < end {
            self.iterate();
        }
    }
}

#[test]
fn a_key_unblocks_a_fifo_client_off_screen_within_one_frame() {
    let (core, mut input, _shell, mut token) = Core::new(PersistedCameras::default());
    let display: Display<WaylandState> = Display::new().expect("display");
    let mut state = WaylandState::new(&display.handle(), core);
    let (server_end, client_end) = UnixStream::pair().expect("socketpair");
    display
        .handle()
        .insert_client(server_end, Arc::new(ClientState::default()))
        .expect("client");
    let output = connect(
        &mut state,
        &mut token,
        &VirtualOutput {
            key: VirtualOutput::key("A"),
            mode: (1920, 1080),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        },
    )
    .expect("output");
    let mut comp = Compositor {
        display,
        state,
        output,
        next_vblank: Instant::now(),
    };

    let (report_tx, report) = channel();
    let (go, go_rx) = channel();
    let client = thread::spawn(move || client(client_end, report_tx, go_rx));

    let r = comp.until(&report, Duration::from_secs(10));
    assert!(matches!(r, Some(Report::Mapped)), "{r:?}");
    comp.run_for(3 * FRAME);
    let element = comp.state.elements().next().expect("the window");
    assert!(comp.state.core().membership().is_observed(element));

    // focus it, then pan the camera 3000 px away (Super + middle button)
    let mut t = 1_000u64;
    let mut tick = || {
        t += 1_000;
        t
    };
    set_focus(&mut comp.state, &mut input, Some(element)).unwrap();
    let logo = Modifiers {
        logo: true,
        ..Modifiers::default()
    };
    keyboard_key(&mut comp.state, &mut input, KEY_LEFTMETA, true, tick()).unwrap();
    pointer_button(&mut comp.state, &mut input, BTN_MIDDLE, true, logo, tick()).unwrap();
    for _ in 0..10 {
        pointer_motion(
            &mut comp.state,
            &mut input,
            (300.0, 0.0),
            (0.0, 0.0),
            tick(),
        )
        .unwrap();
    }
    pointer_button(&mut comp.state, &mut input, BTN_MIDDLE, false, logo, tick()).unwrap();
    keyboard_key(&mut comp.state, &mut input, KEY_LEFTMETA, false, tick()).unwrap();
    comp.run_for(3 * FRAME);
    assert!(
        !comp.state.core().membership().is_observed(element),
        "the window is off-screen"
    );

    go.send(()).unwrap();
    let r = comp.until(&report, Duration::from_secs(10));
    assert!(matches!(r, Some(Report::Blocked)), "{r:?}");
    // blocked for a while, well inside the fallback period
    comp.run_for(Duration::from_millis(300));
    assert!(comp.state.core().frame_pending(element), "still withheld");

    let key_at = Instant::now();
    keyboard_key(&mut comp.state, &mut input, KEY_A, true, tick()).unwrap();
    let r = comp.until(&report, Duration::from_secs(5));
    let Some(Report::Responded { done, key }) = r else {
        panic!("no response: {r:?}");
    };
    assert!(done >= key_at, "unblocked by the kick, not before the key");
    assert!(key >= key_at, "the key was handled after it was injected");
    let latency = key - key_at;
    eprintln!("first response {} us after the key", latency.as_micros());
    assert!(
        latency < FRAME,
        "first response {latency:?} after the key: more than one frame (OB-16)"
    );
    keyboard_key(&mut comp.state, &mut input, KEY_A, false, tick()).unwrap();
    go.send(()).unwrap();
    comp.run_for(FRAME);
    client.join().expect("client thread");
}
