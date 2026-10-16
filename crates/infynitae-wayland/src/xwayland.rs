//! M19 — the xwayland-satellite supervisor (OBJ-32; INV-XWAYLAND-1; ADR-10; OB-33). INC-12,
//! EXPERIMENTAL.
//!
//! X11 support is xwayland-satellite running as an ordinary Wayland client of the session. The
//! compositor owns the X display: [`XDisplay`] reserves a display number, holds its lock file and
//! its two listening sockets (the abstract one and the filesystem one) for the whole session,
//! and every satellite the [`XwaylandSupervisor`] starts is handed those sockets with
//! `-listenfd`. A satellite that exits takes Xwayland and its X clients with it; their Wayland
//! surfaces die with its client connection (client death, INV-STAGE-4). The next satellite
//! serves the same display, so a client started with the old `DISPLAY` connects (OB-33). An X
//! client that connects while no satellite runs waits in the listening socket's backlog.
//!
//! Nothing here reaches Core and nothing takes an authority port: there is no X11 path to Stage
//! or cameras, and no window manager of our own.
//!
//! In `-listenfd` mode xwayland-satellite logs to syslog when it can reach it.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::ops::RangeInclusive;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{SocketAddr, UnixListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use smithay::reexports::rustix;

/// An X display this process owns: the lock file `<root>/.X<n>-lock` with this process's pid,
/// the abstract socket `@<root>/.X11-unix/X<n>` and the socket file `<root>/.X11-unix/X<n>`.
/// `root` is `/tmp` for a real session (where X clients look); tests use another directory, so
/// they never touch a real display. Dropping it removes the socket file and the lock file it
/// created, and nothing else.
#[derive(Debug)]
pub struct XDisplay {
    number: u32,
    lock_path: PathBuf,
    socket_path: PathBuf,
    abstract_listener: UnixListener,
    path_listener: UnixListener,
}

impl XDisplay {
    /// Reserves the first display number in `numbers` that is free under `root`: its lock file
    /// can be created (never an existing one, stale or not) and both sockets can be bound (never
    /// over an existing file). A number in use by anyone else is skipped, never taken over.
    pub fn reserve(root: &Path, numbers: RangeInclusive<u32>) -> io::Result<XDisplay> {
        let dir = root.join(".X11-unix");
        if !dir.is_dir() {
            fs::DirBuilder::new().mode(0o1777).create(&dir)?;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o1777))?;
        }
        for number in numbers {
            if let Some(display) = Self::try_reserve(root, &dir, number)? {
                return Ok(display);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "no free X display number in the range",
        ))
    }

    fn try_reserve(root: &Path, dir: &Path, number: u32) -> io::Result<Option<XDisplay>> {
        let lock_path = root.join(format!(".X{number}-lock"));
        let mut lock = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o444)
            .open(&lock_path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(None),
            Err(e) => return Err(e),
        };
        let release = |lock_path: &Path| {
            let _ = fs::remove_file(lock_path);
        };
        // the X convention: the owner's pid, right-aligned in ten columns
        if let Err(e) = writeln!(lock, "{:>10}", std::process::id()).and_then(|()| lock.flush()) {
            release(&lock_path);
            return Err(e);
        }
        let socket_path = dir.join(format!("X{number}"));
        if fs::symlink_metadata(&socket_path).is_ok() {
            release(&lock_path);
            return Ok(None);
        }
        let name = socket_path.as_os_str().as_encoded_bytes();
        let abstract_listener = match SocketAddr::from_abstract_name(name)
            .and_then(|addr| UnixListener::bind_addr(&addr))
        {
            Ok(l) => l,
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                release(&lock_path);
                return Ok(None);
            }
            Err(e) => {
                release(&lock_path);
                return Err(e);
            }
        };
        let path_listener = match UnixListener::bind(&socket_path) {
            Ok(l) => l,
            Err(e) => {
                release(&lock_path);
                return match e.kind() {
                    io::ErrorKind::AddrInUse => Ok(None),
                    _ => Err(e),
                };
            }
        };
        Ok(Some(XDisplay {
            number,
            lock_path,
            socket_path,
            abstract_listener,
            path_listener,
        }))
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    /// The `DISPLAY` value of this display, `:<n>`.
    pub fn name(&self) -> String {
        format!(":{}", self.number)
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    /// Copies of the two listening sockets that a child inherits (no close-on-exec). They are
    /// meant to live only across one spawn.
    fn inheritable(&self) -> io::Result<[OwnedFd; 2]> {
        let dup = |l: &UnixListener| rustix::io::dup(l.as_fd()).map_err(io::Error::from);
        Ok([dup(&self.abstract_listener)?, dup(&self.path_listener)?])
    }
}

impl Drop for XDisplay {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.socket_path).is_ok_and(|m| m.file_type().is_socket()) {
            let _ = fs::remove_file(&self.socket_path);
        }
        // only a lock that still names this process
        let ours = fs::read_to_string(&self.lock_path)
            .is_ok_and(|s| s.trim().parse::<u32>().ok() == Some(std::process::id()));
        if ours {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

/// The display numbers a session tries; `:0` is left to the host's own X server.
pub const SESSION_DISPLAY_NUMBERS: RangeInclusive<u32> = 1..=63;

/// For a run loop: the X display a session owns when it runs `satellite` — reserved under
/// `/tmp`, where X clients look — or `None` without one.
pub fn reserve_for_session(satellite: Option<&Path>) -> io::Result<Option<XDisplay>> {
    satellite
        .map(|_| XDisplay::reserve(Path::new("/tmp"), SESSION_DISPLAY_NUMBERS))
        .transpose()
}

/// For a run loop: the supervisor of `satellite` on `display`, as a client of the session's
/// socket `wayland_display`, started.
pub fn start_for_session(
    display: XDisplay,
    satellite: &Path,
    wayland_display: &str,
) -> io::Result<XwaylandSupervisor> {
    let mut s = XwaylandSupervisor::new(
        display,
        vec![satellite.as_os_str().to_os_string()],
        wayland_display,
        RestartPolicy::default(),
    );
    s.start()?;
    println!(
        "[infynitae] xwayland-satellite started on {} (pid {:?})",
        s.display.name(),
        s.pid()
    );
    Ok(s)
}

/// M19's states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SatelliteState {
    /// No satellite has been started.
    Disabled,
    Running,
    /// The satellite exited; the next one starts when the backoff has elapsed.
    Crashed,
    /// It exited too often: X11 stays off for the rest of the session (INV-FAIL-1).
    Degraded,
}

/// When an exited satellite is started again (M19 "backoff elapsed", "repeated crashes").
/// Provisional values, not budgets: no measurement chose them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestartPolicy {
    /// Time from an exit to the next start.
    pub backoff: Duration,
    /// This many exits within `window` end in Degraded.
    pub max_exits: usize,
    pub window: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy {
            backoff: Duration::from_secs(1),
            max_exits: 5,
            window: Duration::from_secs(60),
        }
    }
}

/// The supervisor of OBJ-32: starts xwayland-satellite as a client of the session's Wayland
/// socket on the display it owns, restarts it on the same display after an exit, and gives up
/// after repeated exits.
#[derive(Debug)]
pub struct XwaylandSupervisor {
    display: XDisplay,
    /// The satellite's program and any leading arguments; `:<n> -listenfd <a> -listenfd <b>`
    /// are appended.
    command: Vec<OsString>,
    wayland_display: String,
    policy: RestartPolicy,
    state: SatelliteState,
    child: Option<Child>,
    exits: VecDeque<Instant>,
    restart_at: Option<Instant>,
    last_exit: Option<ExitStatus>,
    starts: u32,
}

impl XwaylandSupervisor {
    pub fn new(
        display: XDisplay,
        command: Vec<OsString>,
        wayland_display: &str,
        policy: RestartPolicy,
    ) -> XwaylandSupervisor {
        XwaylandSupervisor {
            display,
            command,
            wayland_display: wayland_display.to_string(),
            policy,
            state: SatelliteState::Disabled,
            child: None,
            exits: VecDeque::new(),
            restart_at: None,
            last_exit: None,
            starts: 0,
        }
    }

    pub fn state(&self) -> SatelliteState {
        self.state
    }

    pub fn display(&self) -> &XDisplay {
        &self.display
    }

    /// The `DISPLAY` X clients of this session get, `None` once X11 is off (Degraded).
    pub fn display_name(&self) -> Option<String> {
        (self.state != SatelliteState::Degraded).then(|| self.display.name())
    }

    /// The running satellite's pid.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// How the last satellite ended.
    pub fn last_exit(&self) -> Option<ExitStatus> {
        self.last_exit
    }

    /// How many satellites have been started.
    pub fn starts(&self) -> u32 {
        self.starts
    }

    /// M19 Disabled → Running (startup). Does nothing in any other state.
    pub fn start(&mut self) -> io::Result<()> {
        if self.state != SatelliteState::Disabled {
            return Ok(());
        }
        self.spawn()
    }

    fn spawn(&mut self) -> io::Result<()> {
        let Some((program, leading)) = self.command.split_first() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no satellite program",
            ));
        };
        let fds = self.display.inheritable()?;
        let mut command = Command::new(program);
        command
            .args(leading)
            .arg(self.display.name())
            .args(["-listenfd", &fds[0].as_raw_fd().to_string()])
            .args(["-listenfd", &fds[1].as_raw_fd().to_string()])
            .env("WAYLAND_DISPLAY", &self.wayland_display)
            // its X server is the one it starts; it is not a systemd service of ours
            .env_remove("DISPLAY")
            .env_remove("NOTIFY_SOCKET");
        let child = command.spawn();
        // the copies were for the child only
        drop(fds);
        let child = child?;
        self.child = Some(child);
        self.state = SatelliteState::Running;
        self.restart_at = None;
        self.starts += 1;
        Ok(())
    }

    /// Reaps an exited satellite and starts the next one when its backoff has elapsed. Returns
    /// how long the caller may sleep before calling again: `Some` only while a restart is
    /// pending (an exit itself is noticed by the caller's wake-up on the child, or at its next
    /// call).
    pub fn poll(&mut self, now: Instant) -> Option<Duration> {
        if self.state == SatelliteState::Running
            && let Some(child) = &mut self.child
            && let Ok(Some(status)) = child.try_wait()
        {
            self.child = None;
            self.last_exit = Some(status);
            self.exits.push_back(now);
            while self
                .exits
                .front()
                .is_some_and(|t| now.saturating_duration_since(*t) > self.policy.window)
            {
                self.exits.pop_front();
            }
            if self.exits.len() >= self.policy.max_exits {
                eprintln!(
                    "[infynitae] xwayland-satellite exited {} times within {:?}; X11 is off for \
                     this session",
                    self.exits.len(),
                    self.policy.window
                );
                self.state = SatelliteState::Degraded;
                self.restart_at = None;
            } else {
                eprintln!(
                    "[infynitae] xwayland-satellite exited ({status}); restarting on {} in {:?}",
                    self.display.name(),
                    self.policy.backoff
                );
                self.state = SatelliteState::Crashed;
                self.restart_at = Some(now + self.policy.backoff);
            }
        }
        if self.state == SatelliteState::Crashed
            && let Some(at) = self.restart_at
        {
            if now >= at {
                if let Err(e) = self.spawn() {
                    eprintln!("[infynitae] failed to restart xwayland-satellite: {e}");
                    self.state = SatelliteState::Degraded;
                    self.restart_at = None;
                }
            } else {
                return Some(at - now);
            }
        }
        None
    }

    /// Ends the running satellite (SIGTERM, a bounded wait, SIGKILL) and reaps it.
    pub fn shutdown(&mut self, grace: Duration) {
        if let Some(child) = self.child.take() {
            let mut children = vec![child];
            crate::session::terminate_children(&mut children, grace);
        }
        self.restart_at = None;
    }
}

impl Drop for XwaylandSupervisor {
    fn drop(&mut self) {
        self.shutdown(Duration::from_millis(500));
    }
}

/// The lock file's content for tests: the pid it names.
pub fn lock_owner(path: &Path) -> Option<u32> {
    let mut s = String::new();
    std::io::Read::read_to_string(&mut File::open(path).ok()?, &mut s).ok()?;
    s.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private root for one test: never `/tmp` itself, so no real display is touched (the
    /// abstract socket name is derived from it too).
    fn root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("infynitae-xwayland-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sh(script: &str) -> Vec<OsString> {
        ["sh", "-c", script, "satellite"]
            .iter()
            .map(OsString::from)
            .collect()
    }

    const FAST: RestartPolicy = RestartPolicy {
        backoff: Duration::from_millis(50),
        max_exits: 3,
        window: Duration::from_secs(60),
    };

    /// Waits (bounded) until the satellite has exited and `poll` noticed it.
    fn until_exited(s: &mut XwaylandSupervisor, at: Instant) {
        let end = Instant::now() + Duration::from_secs(10);
        while s.state() == SatelliteState::Running && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
            s.poll(at);
        }
    }

    #[test]
    fn a_display_owns_its_lock_and_sockets_and_removes_only_them() {
        let root = root("own");
        let d = XDisplay::reserve(&root, 0..=3).unwrap();
        assert_eq!(d.name(), ":0");
        assert_eq!(lock_owner(d.lock_path()), Some(std::process::id()));
        let meta = fs::symlink_metadata(d.socket_path()).unwrap();
        assert!(meta.file_type().is_socket());
        let (lock, socket) = (d.lock_path().to_path_buf(), d.socket_path().to_path_buf());
        drop(d);
        assert!(!lock.exists() && !socket.exists());
        assert!(root.join(".X11-unix").is_dir(), "the directory stays");
        fs::remove_dir_all(&root).unwrap();
    }

    /// A number with a lock file (anyone's, stale or not) or with a file where its socket
    /// would be is skipped, and the files are left as they were.
    #[test]
    fn a_taken_number_is_skipped_and_left_alone() {
        let root = root("taken");
        fs::write(root.join(".X0-lock"), "     99999\n").unwrap();
        fs::create_dir_all(root.join(".X11-unix")).unwrap();
        fs::write(root.join(".X11-unix/X1"), "").unwrap();
        let d = XDisplay::reserve(&root, 0..=3).unwrap();
        assert_eq!(d.number(), 2);
        assert_eq!(
            fs::read_to_string(root.join(".X0-lock")).unwrap(),
            "     99999\n"
        );
        assert!(
            !root.join(".X1-lock").exists(),
            "released when its socket was taken"
        );
        assert!(root.join(".X11-unix/X1").exists());
        let e = XDisplay::reserve(&root, 2..=2).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AddrInUse, "held by d");
        drop(d);
        fs::remove_dir_all(&root).unwrap();
    }

    /// The satellite gets the display, both listening sockets (open, by the numbers given) and
    /// the session's Wayland socket, and no `DISPLAY` of its own.
    #[test]
    fn the_satellite_is_given_the_display_the_sockets_and_the_session() {
        let root = root("args");
        let d = XDisplay::reserve(&root, 0..=3).unwrap();
        let script = r#"sock() { [ "$(readlink /proc/self/fd/$1 | cut -c1-7)" = "socket:" ]; }
            [ "$1" = ":0" ] && [ "$2" = -listenfd ] && [ "$4" = -listenfd ] && sock "$3" \
            && sock "$5" && [ "$WAYLAND_DISPLAY" = wl-test ] && [ -z "${DISPLAY+set}" ] && exit 0
            exit 9"#;
        let mut s = XwaylandSupervisor::new(d, sh(script), "wl-test", FAST);
        assert_eq!(s.state(), SatelliteState::Disabled);
        s.start().unwrap();
        assert_eq!(s.state(), SatelliteState::Running);
        until_exited(&mut s, Instant::now());
        assert_eq!(s.last_exit().and_then(|e| e.code()), Some(0));
        drop(s);
        fs::remove_dir_all(&root).unwrap();
    }

    /// M19: an exit leads to Crashed, the backoff to Running on the same display, repeated exits
    /// to Degraded, after which there is no display to give clients.
    #[test]
    fn an_exited_satellite_restarts_on_the_same_display_until_it_exits_too_often() {
        let root = root("restart");
        let d = XDisplay::reserve(&root, 5..=9).unwrap();
        let mut s = XwaylandSupervisor::new(d, sh("exit 3"), "wl-test", FAST);
        s.start().unwrap();
        let t0 = Instant::now();
        until_exited(&mut s, t0);
        assert_eq!(s.state(), SatelliteState::Crashed);
        assert_eq!(s.last_exit().and_then(|e| e.code()), Some(3));
        assert_eq!(s.poll(t0), Some(FAST.backoff), "nothing before the backoff");
        assert_eq!(s.state(), SatelliteState::Crashed);
        assert_eq!(s.poll(t0 + FAST.backoff), None);
        assert_eq!(s.state(), SatelliteState::Running, "restarted");
        assert_eq!(s.starts(), 2);
        assert_eq!(s.display_name().as_deref(), Some(":5"), "the same display");
        until_exited(&mut s, t0 + FAST.backoff);
        s.poll(t0 + 2 * FAST.backoff);
        assert_eq!(s.starts(), 3);
        until_exited(&mut s, t0 + 2 * FAST.backoff);
        assert_eq!(
            s.state(),
            SatelliteState::Degraded,
            "three exits within the window"
        );
        assert_eq!(s.display_name(), None);
        assert_eq!(s.poll(t0 + Duration::from_secs(5)), None);
        assert_eq!(s.starts(), 3, "never again");
        drop(s);
        fs::remove_dir_all(&root).unwrap();
    }

    /// Exits spread wider than the window never add up to Degraded.
    #[test]
    fn exits_outside_the_window_do_not_count() {
        let root = root("window");
        let d = XDisplay::reserve(&root, 0..=3).unwrap();
        let mut s = XwaylandSupervisor::new(d, sh("exit 1"), "wl-test", FAST);
        s.start().unwrap();
        let mut now = Instant::now();
        for _ in 0..6 {
            until_exited(&mut s, now);
            assert_eq!(s.state(), SatelliteState::Crashed);
            now += FAST.window + FAST.backoff;
            s.poll(now);
            assert_eq!(s.state(), SatelliteState::Running);
        }
        drop(s);
        fs::remove_dir_all(&root).unwrap();
    }

    /// Shutdown ends a running satellite and reaps it; the display's files go with the
    /// supervisor.
    #[test]
    fn shutdown_ends_the_satellite_and_the_display_files_go() {
        let root = root("shutdown");
        let d = XDisplay::reserve(&root, 0..=3).unwrap();
        let (lock, socket) = (d.lock_path().to_path_buf(), d.socket_path().to_path_buf());
        let mut s = XwaylandSupervisor::new(d, sh("exec sleep 30"), "wl-test", FAST);
        s.start().unwrap();
        let pid = s.pid().unwrap();
        s.shutdown(Duration::from_millis(500));
        assert_eq!(s.pid(), None);
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "reaped, not a zombie"
        );
        drop(s);
        assert!(!lock.exists() && !socket.exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
