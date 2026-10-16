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

use infynitae_backend_headless::{VirtualOutput, connect, vblank};
use infynitae_core::Core;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::Transform;
use infynitae_wayland::WaylandState;
use infynitae_wayland::session::{Session, SessionOptions};
use infynitae_wayland::xwayland::{RestartPolicy, SatelliteState, XDisplay, XwaylandSupervisor};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::rustix::process::{Pid, Signal, kill_process};
use smithay::reexports::wayland_server::Display;

const NAME: &str = "a_satellite_restarts_on_the_same_display_and_its_clients_die_with_it";
const INNER: &str = "INFYNITAE_XTEST_INNER";

/// The outer run: the same test binary, this test only, inside private namespaces.
fn rerun_isolated() {
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
            NAME,
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

#[test]
#[ignore = "needs xwayland-satellite, unshare, mount and xmessage; see the module doc"]
fn a_satellite_restarts_on_the_same_display_and_its_clients_die_with_it() {
    if std::env::var_os(INNER).is_none() {
        return rerun_isolated();
    }
    assert_isolated();
    let satellite = PathBuf::from(
        std::env::var_os("INFYNITAE_XWAYLAND_SATELLITE").expect("INFYNITAE_XWAYLAND_SATELLITE"),
    );

    let (core, _input, _shell, mut token) = Core::new(PersistedCameras::default());
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
    let x = XwaylandSupervisor::new(
        x_display,
        vec![satellite.into_os_string()],
        session.socket_name(),
        policy,
    );
    let mut run = Run {
        state,
        session,
        event_loop,
        output,
        x,
    };
    run.x.start().unwrap();
    assert_eq!(run.x.state(), SatelliteState::Running);

    // an X11 client becomes an ordinary element, placed like any other (INV-XWAYLAND-1)
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
