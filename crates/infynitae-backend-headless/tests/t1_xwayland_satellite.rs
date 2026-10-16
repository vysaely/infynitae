//! INC-12 T1 (TST-14 c6-sat on the production path; OB-33; INV-XWAYLAND-1; M19), EXPERIMENTAL:
//! the real xwayland-satellite supervised by the production session code, against the production
//! Core, adapter and headless backend.
//!
//! Isolation: the test re-runs itself under `unshare -Urnm` with a private tmpfs on `/tmp` and on
//! the journal's socket directory, and refuses to run unless `/tmp` is that private tmpfs. The X
//! display, its lock file and both its sockets (the abstract one lives in the private network
//! namespace) never touch the host's, so the desktop's own Xwayland is never seen or disturbed;
//! satellite cannot reach the host's journal and logs to stderr. `/dev/dri` is masked too, so
//! Xwayland finds no GPU render node and renders in software: the test touches no GPU device.
//!
//! Ignored by default. Needs `INFYNITAE_XWAYLAND_SATELLITE` (the satellite binary), `unshare`,
//! `mount` and `xmessage`; run with `--ignored`. The satellite (0.8.3) needs `wp_viewporter`
//! from the compositor, which only the INC-6 line offers: before INC-6 the satellite exits at
//! start and the test fails at its first step.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use infynitae_backend_headless::{
    VirtualOutput, connect, keyboard_key, pointer_button, pointer_motion, set_focus, vblank,
};
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::Transform;
use infynitae_core::{Core, InputPort};
use infynitae_wayland::WaylandState;
use infynitae_wayland::session::{Session, SessionOptions};
use infynitae_wayland::xwayland::{RestartPolicy, SatelliteState, XDisplay, XwaylandSupervisor};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::rustix::process::{Pid, Signal, kill_process};
use smithay::reexports::wayland_server::Display;

const INNER: &str = "INFYNITAE_XTEST_INNER";

/// The outer run: the same test binary, this test only, inside private namespaces.
fn rerun_isolated(name: &str) {
    let exe = std::env::current_exe().expect("test binary");
    let script = "mount -t tmpfs tmpfs /tmp && \
        { [ ! -d /run/systemd/journal ] || mount -t tmpfs tmpfs /run/systemd/journal; } && \
        { [ ! -d /dev/dri ] || mount -t tmpfs tmpfs /dev/dri; } && \
        mkdir -m 700 /tmp/rt && exec \"$0\" \"$@\"";
    let status = Command::new("unshare")
        .args(["-Urnm", "--", "sh", "-c", script])
        .arg(exe)
        .args([
            "--exact",
            name,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(INNER, "1")
        .env("XDG_RUNTIME_DIR", "/tmp/rt")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("NOTIFY_SOCKET")
        .status()
        .expect("unshare");
    assert!(status.success(), "the isolated run failed: {status}");
}

/// `/tmp` is a tmpfs of this mount namespace, not the host's.
fn assert_isolated() {
    let mounts = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
    let tmp_is_tmpfs = mounts.lines().any(|l| {
        let f: Vec<&str> = l.split(' ').collect();
        f.get(4) == Some(&"/tmp") && l.contains(" - tmpfs ")
    });
    assert!(tmp_is_tmpfs, "refusing to run: /tmp is not private");
    assert_eq!(std::env::var("XDG_RUNTIME_DIR").as_deref(), Ok("/tmp/rt"));
    assert!(std::env::var_os("DISPLAY").is_none());
    let dri = std::fs::read_dir("/dev/dri")
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(dri, 0, "refusing to run: a GPU device node is visible");
}

struct Run {
    state: WaylandState,
    input: InputPort,
    session: Session,
    event_loop: EventLoop<'static, ()>,
    output: infynitae_core::output::OutputId,
    x: XwaylandSupervisor,
}

impl Run {
    /// Runs the loop as a backend does until `done` holds, at most `limit`.
    fn until(&mut self, limit: Duration, what: &str, mut done: impl FnMut(&mut Run) -> bool) {
        let end = Instant::now() + limit;
        while !done(self) {
            assert!(Instant::now() < end, "timed out waiting for: {what}");
            let restart = self.x.poll(Instant::now());
            let wait = [
                self.session.timeout(),
                restart,
                Some(Duration::from_millis(10)),
            ]
            .into_iter()
            .flatten()
            .min();
            self.event_loop.dispatch(wait, &mut ()).unwrap();
            self.session.dispatch(&mut self.state);
            if self.session.frame_due() {
                vblank(
                    &mut self.state,
                    self.output,
                    infynitae_wayland::presentation_clock_now(),
                );
                self.session.flush();
            }
        }
    }

    /// The popups in `element`'s tree: (position relative to its window geometry, size).
    fn popups(&self, element: infynitae_core::stage::ElementId) -> Vec<((i32, i32), (i32, i32))> {
        use infynitae_core::snapshot::SurfaceRole;
        self.state
            .core()
            .stage()
            .tree(element)
            .map(|t| {
                t.surfaces
                    .iter()
                    .filter_map(|s| match s.role {
                        SurfaceRole::Popup { position, .. } => {
                            Some(((position.x, position.y), (s.size.w, s.size.h)))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Moves the pointer onto `local` (logical px from `element`'s window-geometry origin).
    fn point_at(&mut self, element: infynitae_core::stage::ElementId, local: (f64, f64)) {
        use infynitae_core::geom::{CanvasPos, Q16_ONE};
        use infynitae_core::pointer::PointerState;
        let core = self.state.core();
        let PointerState::Located { output, p } = core.pointer() else {
            panic!("the pointer has no output");
        };
        let anchor = core.stage().anchor(element).expect("placed");
        let view = core.view(output).expect("a view");
        let q = |a: i64, l: f64| a * Q16_ONE + (l * Q16_ONE as f64).round() as i64;
        let target = view.project(CanvasPos {
            x: q(anchor.x, local.0),
            y: q(anchor.y, local.1),
        });
        let d = (target.x - p.0, target.y - p.1);
        pointer_motion(&mut self.state, &mut self.input, d, d, 1).unwrap();
        self.session.flush();
    }

    fn click(&mut self) {
        for pressed in [true, false] {
            pointer_button(
                &mut self.state,
                &mut self.input,
                0x110,
                pressed,
                Modifiers::default(),
                2,
            )
            .unwrap();
            self.session.flush();
        }
    }

    fn key(&mut self, key: u32) {
        for pressed in [true, false] {
            keyboard_key(&mut self.state, &mut self.input, key, pressed, 3).unwrap();
            self.session.flush();
        }
    }

    /// The GTK3 menu client of OB-9 (`tests/data/x11_menu.py`), on this display, with D-Bus off.
    fn gtk_menu_client(&self) -> Child {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/x11_menu.py");
        Command::new("python3")
            .args(["-I", script, "30000"])
            .env("DISPLAY", self.x.display_name().expect("a display"))
            .env("GDK_BACKEND", "x11")
            .env("DBUS_SESSION_BUS_ADDRESS", "disabled:")
            .env("NO_AT_BRIDGE", "1")
            .env("GTK_A11Y", "none")
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("python3 with GTK3")
    }

    fn mapped(&self) -> usize {
        self.state.core().stage().mapped_elements().len()
    }

    fn x_client(&self, label: &str) -> Child {
        Command::new("xmessage")
            .args(["-name", label, "-timeout", "60", label])
            .env("DISPLAY", self.x.display_name().expect("a display"))
            .env_remove("WAYLAND_DISPLAY")
            .stderr(Stdio::piped())
            .spawn()
            .expect("xmessage")
    }
}

/// An isolated session (production session, Core, adapter, headless backend) with the
/// supervisor armed for the first X11 connection.
fn start_run() -> Run {
    let satellite = PathBuf::from(
        std::env::var_os("INFYNITAE_XWAYLAND_SATELLITE").expect("INFYNITAE_XWAYLAND_SATELLITE"),
    );

    let (core, input, _shell, mut token) = Core::new(PersistedCameras::default());
    let display: Display<WaylandState> = Display::new().unwrap();
    let mut state = WaylandState::new(&display.handle(), core);
    let output = connect(
        &mut state,
        &mut token,
        &VirtualOutput {
            key: VirtualOutput::key("X12"),
            mode: (1280, 800),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        },
    )
    .unwrap();
    let event_loop: EventLoop<'static, ()> = EventLoop::try_new().unwrap();
    let x_display = XDisplay::reserve(std::path::Path::new("/tmp"), 1..=63).unwrap();
    assert_eq!(
        x_display.name(),
        ":1",
        "a private /tmp: the first number is free"
    );
    let session = Session::new(
        display,
        &event_loop.handle(),
        &SessionOptions {
            socket_name: Some("wl-inc12".into()),
            refresh_mhz: 60_000,
            x_display: Some(x_display.name()),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let policy = RestartPolicy {
        backoff: Duration::from_millis(200),
        ..RestartPolicy::default()
    };
    let mut x = XwaylandSupervisor::new(
        x_display,
        vec![satellite.into_os_string()],
        session.socket_name(),
        policy,
    );
    // M19 "first X11 connection": nothing runs until an X11 client connects
    x.arm_on_demand(&event_loop.handle()).unwrap();
    Run {
        state,
        input,
        session,
        event_loop,
        output,
        x,
    }
}

/// A condition that becomes true `ms` milliseconds after its first call.
fn false_after(ms: u64) -> bool {
    thread_local!(static START: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) });
    START.with(|s| {
        let t0 = *s.get().get_or_insert(Instant::now());
        s.set(Some(t0));
        t0.elapsed() >= Duration::from_millis(ms)
    })
}

#[test]
#[ignore = "needs xwayland-satellite, unshare, mount and xmessage; see the module doc"]
fn a_satellite_restarts_on_the_same_display_and_its_clients_die_with_it() {
    if std::env::var_os(INNER).is_none() {
        return rerun_isolated(
            "a_satellite_restarts_on_the_same_display_and_its_clients_die_with_it",
        );
    }
    assert_isolated();
    let mut run = start_run();
    run.until(Duration::from_secs(2), "an idle moment", |_| {
        false_after(200)
    });
    assert_eq!(run.x.state(), SatelliteState::Disabled, "no X11 client yet");
    assert_eq!((run.x.starts(), run.x.pid()), (0, None));

    // an X11 client becomes an ordinary element, placed like any other (INV-XWAYLAND-1); its
    // connection is what starts the satellite
    let mut first = run.x_client("X12FIRST");
    run.until(Duration::from_secs(20), "the X11 window maps", |r| {
        assert_ne!(
            r.x.state(),
            SatelliteState::Degraded,
            "the satellite keeps exiting (last: {:?}); before INC-6 the compositor has no \
             wp_viewporter",
            r.x.last_exit()
        );
        r.mapped() == 1
    });
    assert_eq!(run.x.starts(), 1, "started by the first connection");
    let element = run.state.core().stage().mapped_elements()[0];
    assert!(
        run.state.core().stage().anchor(element).is_some(),
        "placed by Core's placement, not by X11"
    );

    // the satellite dies: its X clients and their elements go with it (client death)
    let pid = run.x.pid().unwrap();
    kill_process(Pid::from_raw(pid as i32).unwrap(), Signal::KILL).unwrap();
    run.until(
        Duration::from_secs(10),
        "the X11 window is destroyed",
        |r| r.mapped() == 0 && r.state.core().stage().z_order().is_empty(),
    );
    let mut status = None;
    run.until(Duration::from_secs(10), "the X11 client exits", |_| {
        status = first.try_wait().unwrap();
        status.is_some()
    });
    assert!(!status.unwrap().success(), "its X server went away");

    // the next satellite serves the same display; a client with the old DISPLAY connects
    run.until(Duration::from_secs(10), "the satellite restarts", |r| {
        r.x.state() == SatelliteState::Running && r.x.starts() == 2
    });
    assert_eq!(
        run.x.display_name().as_deref(),
        Some(":1"),
        "the same display"
    );
    let mut second = run.x_client("X12SECOND");
    run.until(Duration::from_secs(20), "the second X11 window maps", |r| {
        r.mapped() == 1
    });

    let _ = second.kill();
    let _ = second.wait();
    run.x.shutdown(Duration::from_millis(500));
    run.session.shutdown();
}

/// Lines a child printed so far, read without blocking forever: the reader thread hands them over.
fn lines(child: &mut Child) -> std::sync::mpsc::Receiver<String> {
    use std::io::BufRead;
    let out = child.stdout.take().expect("piped stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// OB-9 (T1 part), INC-12: a GTK3 menu of an X11 client, an override-redirect window, reaches
/// Infynitae through xwayland-satellite as an `xdg_popup` of the client's window, at the place
/// the client asked for (the button's bottom centre: (150, 200) of the 300×200 window); it
/// receives input (a click on its first item activates it); and it is dismissed correctly (a
/// second menu, closed with Escape, takes its popup away). D-Bus is disabled for the client;
/// nothing reaches the desktop's session bus.
#[test]
#[ignore = "needs xwayland-satellite, unshare, mount, python3 and GTK3; see the module doc"]
fn an_x11_menu_is_a_popup_at_its_place_takes_input_and_dismisses() {
    if std::env::var_os(INNER).is_none() {
        return rerun_isolated("an_x11_menu_is_a_popup_at_its_place_takes_input_and_dismisses");
    }
    assert_isolated();
    let mut run = start_run();

    for dismiss in [false, true] {
        let mut app = run.gtk_menu_client();
        let out = lines(&mut app);
        let mut seen = Vec::new();
        run.until(Duration::from_secs(30), "the X11 menu pops up", |r| {
            seen.extend(out.try_iter());
            r.mapped() == 1
                && !r
                    .popups(r.state.core().stage().mapped_elements()[0])
                    .is_empty()
        });
        let element = run.state.core().stage().mapped_elements()[0];
        let popups = run.popups(element);
        assert_eq!(popups.len(), 1, "{popups:?}");
        let ((x, y), (w, h)) = popups[0];
        assert_eq!((x, y), (150, 200), "attached at the button's bottom centre");
        assert!(w > 0 && h > 0, "{popups:?}");
        set_focus(&mut run.state, &mut run.input, Some(element)).unwrap();
        run.session.flush();
        if dismiss {
            run.key(1); // Escape
            run.until(Duration::from_secs(10), "the menu is dismissed", |r| {
                seen.extend(out.try_iter());
                r.popups(element).is_empty() && seen.iter().any(|l| l == "DEACTIVATED")
            });
            assert!(!seen.iter().any(|l| l.starts_with("ACTIVATED")), "{seen:?}");
            let _ = app.kill();
        } else {
            // the first item, a few px inside the menu's top-left corner
            run.point_at(element, (f64::from(x) + 12.0, f64::from(y) + 10.0));
            run.click();
            run.until(Duration::from_secs(10), "the item is activated", |_| {
                seen.extend(out.try_iter());
                seen.iter().any(|l| l.starts_with("ACTIVATED"))
            });
            assert!(seen.iter().any(|l| l == "ACTIVATED First"), "{seen:?}");
        }
        let _ = app.wait();
        run.until(Duration::from_secs(10), "the X11 window is gone", |r| {
            r.mapped() == 0
        });
    }
    run.x.shutdown(Duration::from_millis(500));
    run.session.shutdown();
}
