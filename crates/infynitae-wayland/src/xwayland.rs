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
//! Access control: Xwayland is started with `-auth` and a private MIT-MAGIC-COOKIE-1
//! ([`XAuthority`], mode 0600 in `XDG_RUNTIME_DIR`), so only clients that hold the cookie — those
//! the session starts get `XAUTHORITY` — can connect. Both sockets stay bound: the abstract name,
//! which libxcb tries first, cannot be taken by another process to impersonate the display.
//!
//! In `-listenfd` mode xwayland-satellite logs to syslog when it can reach it.

use std::cell::Cell;
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
use std::rc::Rc;
use std::time::{Duration, Instant};

use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
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
    authority: Option<XAuthority>,
}

/// A private Xauthority file with one MIT-MAGIC-COOKIE-1 for one display (16 bytes from
/// `/dev/urandom`; family "wild", so it matches whatever host name a client uses). Mode 0600;
/// removed when dropped.
#[derive(Debug)]
pub struct XAuthority {
    path: PathBuf,
}

impl XAuthority {
    /// Writes the file `<dir>/infynitae-xauth-<pid>-<number>` (created exclusively, 0600).
    pub fn create(dir: &Path, number: u32) -> io::Result<XAuthority> {
        let mut cookie = [0u8; 16];
        std::io::Read::read_exact(&mut File::open("/dev/urandom")?, &mut cookie)?;
        let path = dir.join(format!("infynitae-xauth-{}-{number}", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let field = |out: &mut Vec<u8>, bytes: &[u8]| {
            out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            out.extend_from_slice(bytes);
        };
        // Xau entry: family (FamilyWild 0xffff), address, display number, auth name, auth data
        let mut entry = 0xffffu16.to_be_bytes().to_vec();
        field(&mut entry, b"");
        field(&mut entry, number.to_string().as_bytes());
        field(&mut entry, b"MIT-MAGIC-COOKIE-1");
        field(&mut entry, &cookie);
        if number != 0 {
            entry.extend_from_slice(&0xffffu16.to_be_bytes());
            field(&mut entry, b"");
            field(&mut entry, b"0");
            field(&mut entry, b"MIT-MAGIC-COOKIE-1");
            field(&mut entry, &cookie);
        }
        let written = file.write_all(&entry).and_then(|()| file.flush());
        let auth = XAuthority { path };
        written.map(|()| auth)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for XAuthority {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// A lock this user left behind: owned by our uid, naming a process that no longer exists.
fn stale_and_ours(lock_path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = fs::symlink_metadata(lock_path) else {
        return false;
    };
    let pid = fs::read_to_string(lock_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    meta.uid() == rustix::process::getuid().as_raw()
        && pid.is_some_and(|pid| pid > 0 && !Path::new(&format!("/proc/{pid}")).exists())
}

/// Removes what a session of ours that died without cleaning up (SIGKILL, a crash) left in the
/// runtime directory `dir`: its X cookies `infynitae-xauth-<pid>-<n>` and its satellites'
/// runtime directories `infynitae-satellite-<pid>-<n>`. Only entries owned by our uid whose pid
/// names no running process, and only these two name patterns (INC-12 review r2).
pub fn reclaim_stale_session_files(dir: &Path) {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let uid = rustix::process::getuid().as_raw();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(rest) = name
            .strip_prefix("infynitae-xauth-")
            .or_else(|| name.strip_prefix("infynitae-satellite-"))
        else {
            continue;
        };
        let Some((pid, n)) = rest.split_once('-') else {
            continue;
        };
        let (Ok(pid), Ok(_)) = (pid.parse::<u32>(), n.parse::<u32>()) else {
            continue;
        };
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if pid == 0
            || pid == std::process::id()
            || meta.uid() != uid
            || Path::new(&format!("/proc/{pid}")).exists()
        {
            continue;
        }
        let _ = if meta.is_dir() {
            fs::remove_dir_all(entry.path())
        } else {
            fs::remove_file(entry.path())
        };
    }
}

impl XDisplay {
    /// Reserves the first display number in `numbers` that is free under `root`: its lock file
    /// can be created and both sockets can be bound (never over an existing file). A number in
    /// use by anyone else is skipped, never taken over. One exception: a lock this user left
    /// behind (owned by our uid, naming a dead process, e.g. after a crash) is reclaimed with its
    /// socket file if that is ours too, so crashes do not use up display numbers.
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
        if stale_and_ours(&lock_path) {
            use std::os::unix::fs::MetadataExt;
            let socket = dir.join(format!("X{number}"));
            if fs::symlink_metadata(&socket).is_ok_and(|m| {
                m.file_type().is_socket() && m.uid() == rustix::process::getuid().as_raw()
            }) {
                let _ = fs::remove_file(&socket);
            }
            let _ = fs::remove_file(&lock_path);
        }
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
            authority: None,
        }))
    }

    /// Requires X clients of this display to authenticate: a private Xauthority file is written
    /// in `dir` (normally `XDG_RUNTIME_DIR`) and given to every satellite with `-auth`.
    pub fn with_authority(mut self, dir: &Path) -> io::Result<XDisplay> {
        self.authority = Some(XAuthority::create(dir, self.number)?);
        Ok(self)
    }

    /// The Xauthority file clients of this display need as `XAUTHORITY`.
    pub fn authority_path(&self) -> Option<&Path> {
        self.authority.as_ref().map(XAuthority::path)
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
        let socket_0 = Path::new("/tmp/.X11-unix/X0");
        if socket_0.is_symlink()
            && fs::read_link(socket_0).is_ok_and(|dest| dest == self.socket_path)
        {
            let _ = fs::remove_file(socket_0);
        }
    }
}

/// The display numbers a session tries; `:0` is left to the host's own X server.
pub const SESSION_DISPLAY_NUMBERS: RangeInclusive<u32> = 1..=63;

/// For a run loop: the X display a session owns when it runs `satellite` — reserved under
/// `/tmp`, where X clients look — or `None` without one.
pub fn reserve_for_session(satellite: Option<&Path>) -> io::Result<Option<XDisplay>> {
    if satellite.is_none() {
        return Ok(None);
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "XDG_RUNTIME_DIR is not set: no private place for the X cookie",
        )
    })?;
    let runtime_path = Path::new(&runtime);
    reclaim_stale_session_files(runtime_path);
    let display = XDisplay::reserve(Path::new("/tmp"), SESSION_DISPLAY_NUMBERS)?
        .with_authority(runtime_path)?;

    // Maintain stable symlinks so any tool or launcher resolves active credentials
    if let Some(auth_path) = display.authority_path() {
        let stable_symlink = runtime_path.join("infynitae-xauth");
        let _ = fs::remove_file(&stable_symlink);
        let _ = std::os::unix::fs::symlink(auth_path, &stable_symlink);

        // ~/.Xauthority for X11 tools that ignore XAUTHORITY: linked only when absent or already a
        // symlink (ours); a real file belongs to another desktop and is never replaced
        if let Ok(home) = std::env::var("HOME") {
            let xauth_home = PathBuf::from(home).join(".Xauthority");
            let replaceable =
                fs::symlink_metadata(&xauth_home).map_or(true, |m| m.file_type().is_symlink());
            if replaceable {
                let _ = fs::remove_file(&xauth_home);
                let _ = std::os::unix::fs::symlink(auth_path, &xauth_home);
            }
        }
    }

    // :0 is the host's: earlier builds pointed an X0 symlink at their display; one left behind
    // (a crash skips Drop) would block a real X server, so it goes
    let socket_0 = Path::new("/tmp/.X11-unix/X0");
    if socket_0.is_symlink()
        && fs::read_link(socket_0).is_ok_and(|dest| dest.starts_with("/tmp/.X11-unix/X"))
    {
        let _ = fs::remove_file(socket_0);
    }

    Ok(Some(display))
}

/// For a run loop: the supervisor of `satellite` on `display`, as a client of the session's
/// socket `wayland_display`, armed to start at the first X11 connection (M19), so a session in
/// which nothing uses X11 runs no X server.
pub fn start_for_session<D: 'static>(
    display: XDisplay,
    satellite: &Path,
    wayland_display: &str,
    handle: &LoopHandle<'_, D>,
) -> io::Result<XwaylandSupervisor> {
    let mut s = XwaylandSupervisor::new(
        display,
        vec![satellite.as_os_str().to_os_string()],
        wayland_display,
        RestartPolicy::default(),
    );
    s.arm_on_demand(handle)?;
    println!(
        "[infynitae] X11 display {} ready; xwayland-satellite starts at the first X11 connection",
        s.display.name()
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
    /// Set when an X11 client connected while no satellite had started (M19 "first X11
    /// connection"); the next `poll` starts it.
    wanted: Rc<Cell<bool>>,
    /// The running satellite's own runtime directory (see `spawn`), removed once it is reaped.
    private_runtime: Option<PathBuf>,
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
            wanted: Rc::new(Cell::new(false)),
            private_runtime: None,
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

    /// M19 Disabled → Running at the first X11 connection: both listening sockets are watched
    /// (a copy each) until one has a client waiting in its backlog, then the watch ends and the
    /// next [`XwaylandSupervisor::poll`] starts the satellite, which accepts that client. Nothing
    /// is accepted here and nothing wakes the loop while no X11 client connects.
    pub fn arm_on_demand<D: 'static>(&mut self, handle: &LoopHandle<'_, D>) -> io::Result<()> {
        for fd in self.display.inheritable()? {
            // the copies are only watched, never inherited
            rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
            let wanted = self.wanted.clone();
            handle
                .insert_source(
                    Generic::new(fd, Interest::READ, Mode::Level),
                    move |_, _, _| {
                        wanted.set(true);
                        Ok(PostAction::Remove)
                    },
                )
                .map_err(|e| io::Error::other(e.to_string()))?;
        }
        Ok(())
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
            // its X server is the one it starts; it is not a systemd service of ours; nothing of
            // the compositor's own launch environment applies to it
            .env_remove("DISPLAY")
            .env_remove("XAUTHORITY")
            .env_remove("WAYLAND_SOCKET")
            .env_remove("XDG_ACTIVATION_TOKEN")
            .env_remove("DESKTOP_STARTUP_ID")
            .env_remove("NOTIFY_SOCKET");
        if let Some(auth) = self.display.authority_path() {
            command.arg("-auth").arg(auth);
        }
        // The satellite binds a Wayland socket of its own for Xwayland (`xwls-<n>`) in its
        // XDG_RUNTIME_DIR and removes it only when it exits by itself; it is ended by a signal
        // (or crashes). It gets a private directory beside the session's X cookie, removed after
        // it is reaped, so no stop or restart leaves a socket in the session's runtime directory.
        // The session's socket is then named by its absolute path.
        self.release_runtime();
        if let Some(dir) = self.display.authority_path().and_then(Path::parent) {
            let private = dir.join(format!(
                "infynitae-satellite-{}-{}",
                std::process::id(),
                self.display.number()
            ));
            fs::DirBuilder::new().mode(0o700).create(&private)?;
            command
                .env("XDG_RUNTIME_DIR", &private)
                .env("WAYLAND_DISPLAY", dir.join(&self.wayland_display));
            self.private_runtime = Some(private);
        }
        // an owned child (R2-4): the session's orphan reaper leaves its exit status to `poll`
        let child = crate::ownership::spawn_owned(&mut command);
        // the copies were for the child only
        drop(fds);
        let child = match child {
            Ok(child) => child,
            Err(e) => {
                self.release_runtime();
                return Err(e);
            }
        };
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
        if self.state == SatelliteState::Disabled
            && self.wanted.take()
            && let Err(e) = self.spawn()
        {
            eprintln!("[infynitae] failed to start xwayland-satellite: {e}");
            self.state = SatelliteState::Degraded;
        }
        if self.state == SatelliteState::Running
            && let Some(child) = &mut self.child
            && let Ok(Some(status)) = child.try_wait()
        {
            crate::ownership::forget_spawned(child.id());
            self.child = None;
            self.release_runtime();
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
            let pid = child.id();
            let mut children = vec![child];
            crate::session::terminate_children(&mut children, grace);
            crate::ownership::forget_spawned(pid);
        }
        self.release_runtime();
        self.restart_at = None;
    }

    /// Removes the reaped satellite's private runtime directory (only that directory).
    fn release_runtime(&mut self) {
        if let Some(dir) = self.private_runtime.take() {
            let _ = fs::remove_dir_all(dir);
        }
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
        // a lock naming a live process (init): not ours to take, stale or not
        fs::write(root.join(".X0-lock"), "         1\n").unwrap();
        fs::create_dir_all(root.join(".X11-unix")).unwrap();
        fs::write(root.join(".X11-unix/X1"), "").unwrap();
        let d = XDisplay::reserve(&root, 0..=3).unwrap();
        assert_eq!(d.number(), 2);
        assert_eq!(
            fs::read_to_string(root.join(".X0-lock")).unwrap(),
            "         1\n"
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

    /// The satellite's own Wayland socket (`xwls-<n>`, which it removes only when it exits by
    /// itself) lives in a private runtime directory beside the X cookie, and the session's socket
    /// is named by its absolute path. A satellite ended by a signal — at shutdown or after a
    /// crash and before its restart — leaves nothing behind.
    #[test]
    fn the_satellite_runtime_directory_goes_with_the_satellite() {
        let root = root("runtime");
        let d = XDisplay::reserve(&root, 0..=3)
            .unwrap()
            .with_authority(&root)
            .unwrap();
        let report = root.join("seen");
        let script = format!(
            r#": > "$XDG_RUNTIME_DIR/xwls-1"; echo "$XDG_RUNTIME_DIR $WAYLAND_DISPLAY" > {}
            [ -e {}/crash ] && exit 3; exec sleep 30"#,
            report.display(),
            root.display()
        );
        let private = root.join(format!("infynitae-satellite-{}-0", std::process::id()));
        // a crash, then the restart, then a shutdown by signal
        fs::write(root.join("crash"), "").unwrap();
        let mut s = XwaylandSupervisor::new(d, sh(&script), "wl-test", FAST);
        s.start().unwrap();
        until_exited(&mut s, Instant::now());
        assert_eq!(s.state(), SatelliteState::Crashed);
        assert!(
            !private.exists(),
            "removed when the crashed satellite was reaped"
        );
        assert_eq!(
            fs::read_to_string(&report).unwrap().trim(),
            format!("{} {}", private.display(), root.join("wl-test").display())
        );
        fs::remove_file(root.join("crash")).unwrap();
        s.poll(Instant::now() + Duration::from_secs(1));
        assert_eq!(s.state(), SatelliteState::Running);
        let end = Instant::now() + Duration::from_secs(5);
        while !private.join("xwls-1").exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            private.join("xwls-1").exists(),
            "the restarted satellite's socket"
        );
        s.shutdown(Duration::from_millis(500));
        assert!(!private.exists(), "removed at shutdown");
        drop(s);
        fs::remove_dir_all(&root).unwrap();
    }

    /// Review r2: a dead session's cookie and satellite directory are reclaimed; a live
    /// process's entries, other names, and malformed names are left alone.
    #[test]
    fn stale_session_files_of_dead_processes_are_reclaimed() {
        let root = root("stale-files");
        // a pid that does not exist: above the kernel's pid_max ceiling
        let dead = 4_194_305u32;
        let me = std::process::id();
        fs::write(root.join(format!("infynitae-xauth-{dead}-3")), "x").unwrap();
        fs::create_dir_all(root.join(format!("infynitae-satellite-{dead}-3"))).unwrap();
        fs::write(
            root.join(format!("infynitae-satellite-{dead}-3/xwls-1")),
            "",
        )
        .unwrap();
        fs::write(root.join(format!("infynitae-xauth-{me}-4")), "x").unwrap();
        fs::write(root.join("infynitae-xauth-1-5"), "x").unwrap(); // init is alive
        fs::write(root.join(format!("other-{dead}-3")), "x").unwrap();
        fs::write(root.join(format!("infynitae-xauth-{dead}")), "x").unwrap();
        reclaim_stale_session_files(&root);
        let mut left: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        let mut want = vec![
            format!("infynitae-xauth-{me}-4"),
            "infynitae-xauth-1-5".to_string(),
            format!("other-{dead}-3"),
            format!("infynitae-xauth-{dead}"),
        ];
        want.sort();
        assert_eq!(left, want);
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

    /// M19 "first X11 connection": once armed, nothing runs until an X11 client connects; its
    /// connection (waiting in the backlog) starts the satellite at the next poll; later
    /// connections start nothing more.
    #[test]
    fn the_first_x11_connection_starts_the_satellite() {
        use smithay::reexports::calloop::EventLoop;
        let root = root("demand");
        let d = XDisplay::reserve(&root, 0..=3).unwrap();
        let path = d.socket_path().to_path_buf();
        let mut s = XwaylandSupervisor::new(d, sh("exec sleep 30"), "wl-test", FAST);
        let mut event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
        s.arm_on_demand(&event_loop.handle()).unwrap();
        event_loop
            .dispatch(Some(Duration::from_millis(30)), &mut ())
            .unwrap();
        assert_eq!(s.poll(Instant::now()), None);
        assert_eq!((s.state(), s.starts()), (SatelliteState::Disabled, 0));
        let _first = std::os::unix::net::UnixStream::connect(&path).unwrap();
        event_loop
            .dispatch(Some(Duration::from_secs(5)), &mut ())
            .unwrap();
        s.poll(Instant::now());
        assert_eq!((s.state(), s.starts()), (SatelliteState::Running, 1));
        let _second = std::os::unix::net::UnixStream::connect(&path).unwrap();
        event_loop
            .dispatch(Some(Duration::from_millis(30)), &mut ())
            .unwrap();
        s.poll(Instant::now());
        assert_eq!(s.starts(), 1, "started once");
        s.shutdown(Duration::from_millis(500));
        drop(s);
        fs::remove_dir_all(&root).unwrap();
    }

    /// A lock this user left behind (its process is gone, e.g. after a crash) is reclaimed with
    /// its socket file, so crashes do not use display numbers up.
    #[test]
    fn a_stale_lock_of_ours_is_reclaimed() {
        let root = root("stale");
        let mut child = Command::new("true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        fs::create_dir_all(root.join(".X11-unix")).unwrap();
        fs::write(root.join(".X0-lock"), format!("{dead:>10}\n")).unwrap();
        let stale_socket = UnixListener::bind(root.join(".X11-unix/X0")).unwrap();
        drop(stale_socket);
        let d = XDisplay::reserve(&root, 0..=3).unwrap();
        assert_eq!(d.number(), 0, "reclaimed");
        assert_eq!(lock_owner(d.lock_path()), Some(std::process::id()));
        drop(d);
        fs::remove_dir_all(&root).unwrap();
    }

    /// The Xauthority file: private (0600), one MIT-MAGIC-COOKIE-1 entry for the display with a
    /// 16-byte cookie, a different cookie each time, removed with its display; the satellite gets
    /// it with `-auth`.
    #[test]
    fn the_display_has_a_private_cookie_the_satellite_is_given() {
        let root = root("auth");
        let d = XDisplay::reserve(&root, 0..=3)
            .unwrap()
            .with_authority(&root)
            .unwrap();
        let path = d.authority_path().unwrap().to_path_buf();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let bytes = fs::read(&path).unwrap();
        // family wild, empty address, "0", the auth name, 16 bytes of data
        let mut want = vec![0xff, 0xff, 0, 0, 0, 1, b'0', 0, 18];
        want.extend_from_slice(b"MIT-MAGIC-COOKIE-1");
        want.extend_from_slice(&[0, 16]);
        assert_eq!(&bytes[..want.len()], &want[..]);
        assert_eq!(bytes.len(), want.len() + 16);
        let other = XAuthority::create(&root, 9).unwrap();
        assert_ne!(
            fs::read(other.path()).unwrap()[want.len()..],
            bytes[want.len()..]
        );
        drop(other);
        let script = r#"[ "$6" = -auth ] && [ -f "$7" ] && exit 0; exit 9"#;
        let mut s = XwaylandSupervisor::new(d, sh(script), "wl-test", FAST);
        s.start().unwrap();
        until_exited(&mut s, Instant::now());
        assert_eq!(
            s.last_exit().and_then(|e| e.code()),
            Some(0),
            "-auth <file>"
        );
        drop(s);
        assert!(!path.exists(), "removed with the display");
        fs::remove_dir_all(&root).unwrap();
    }
}
