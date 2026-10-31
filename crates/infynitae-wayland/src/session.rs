//! The part of a running session that is the same contract in every backend's loop: the
//! listening socket and its clients, client dispatch, the client process started with the
//! session, the run limits, and the timer that paces frames for a backend without a display
//! clock of its own.
//!
//! A backend keeps what differs: its event sources, its input, its renderer and target, what a
//! frame is, and (for a backend driven by real page flips) when a frame happens. Nothing here
//! takes an authority port.

use std::cell::Cell;
use std::error::Error;
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::signals::{self, Registration, SIGCHLD, SIGINT, SIGTERM};
use infynitae_core::scale::SessionTime;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
use smithay::reexports::rustix::io::Errno;
use smithay::reexports::rustix::process::{Pid, PidfdFlags, Signal, kill_process, pidfd_open};
use smithay::reexports::wayland_server::Display;
use smithay::wayland::socket::ListeningSocketSource;

use crate::{ClientState, WaylandState};

/// How long a child gets between SIGTERM and SIGKILL at shutdown. A browser saves its session
/// and its databases on SIGTERM; 500 ms cut Firefox off mid-write (owner, 2026-10-25: its
/// cookies were lost). The wait ends as soon as everything has exited.
const CHILD_GRACE: Duration = Duration::from_secs(5);

/// Frame pacing for a backend whose frames are not driven by a display: ticks fall on a fixed
/// cadence, and only while something asked for one.
///
/// A tick is due at `phase + n · period`. The deadline of the next tick is computed from the
/// deadline of the last one, never from when the last frame finished, so the time a frame takes
/// does not lengthen the period. A tick that is late is not made up for: the cadence continues
/// from the last deadline that has passed. While nothing has requested a frame there is no
/// deadline and the loop has nothing to wake up for.
#[derive(Debug)]
pub struct FrameTimer {
    period: Duration,
    /// The deadline of the last tick taken.
    last: Option<Instant>,
    /// The deadline of the requested tick.
    next: Option<Instant>,
}

impl FrameTimer {
    /// A timer for a refresh rate in mHz.
    pub fn new(refresh_mhz: u32) -> FrameTimer {
        FrameTimer {
            period: Duration::from_nanos(1_000_000_000_000 / u64::from(refresh_mhz.max(1))),
            last: None,
            next: None,
        }
    }

    pub fn period(&self) -> Duration {
        self.period
    }

    /// Something needs a frame: a tick becomes due at the next cadence point. A request while
    /// one is already pending changes nothing.
    pub fn request(&mut self, now: Instant) {
        if self.next.is_some() {
            return;
        }
        self.next = Some(match self.last {
            Some(last) if last + self.period > now => last + self.period,
            // first frame, or idle for longer than a period: due now, and the cadence restarts
            _ => now,
        });
    }

    /// Time until the pending tick; `None` while none is pending.
    pub fn until_due(&self, now: Instant) -> Option<Duration> {
        self.next.map(|t| t.saturating_duration_since(now))
    }

    /// Takes the pending tick if its deadline has passed.
    pub fn take_due(&mut self, now: Instant) -> bool {
        match self.next {
            Some(deadline) if now >= deadline => {
                // the cadence point this tick stands for: the last one that has passed
                let missed = (now - deadline).as_nanos() / self.period.as_nanos().max(1);
                self.last = Some(deadline + self.period * missed as u32);
                self.next = None;
                true
            }
            _ => false,
        }
    }
}

/// What a session is started with.
#[derive(Clone, Debug, Default)]
pub struct SessionOptions {
    /// Wayland socket name; `None` picks the first free `wayland-N`.
    pub socket_name: Option<String>,
    /// A command started once the socket listens, with `WAYLAND_DISPLAY` set to it.
    pub exec: Option<String>,
    /// Refresh rate of the frame timer in mHz.
    pub refresh_mhz: u32,
    /// Leave the loop after this long.
    pub duration: Option<Duration>,
    /// Leave the loop after this many frames.
    pub max_frames: Option<u64>,
    /// The X display this session's clients get as `DISPLAY` (the supervisor's, M19). `None`:
    /// they get no `DISPLAY` at all, so an X11 client started here never reaches an X server
    /// outside the session (INV-XWAYLAND-1).
    pub x_display: Option<String>,
    /// The Xauthority file of that display, given to the clients as `XAUTHORITY`; without it
    /// they get none (never the compositor's own).
    pub x_authority: Option<std::path::PathBuf>,
}

/// A listening Wayland session.
pub struct Session {
    display: Display<WaylandState>,
    socket_name: String,
    children: Vec<Child>,
    timer: FrameTimer,
    started: Instant,
    frames: u64,
    duration: Option<Duration>,
    max_frames: Option<u64>,
    /// The display became readable since the last dispatch: a request arrived or a client
    /// went away.
    display_ready: Rc<Cell<bool>>,
    /// A deadline of Core's own on the session clock (a scale-settlement deadline); `None`
    /// when Core has nothing pending.
    wake: Option<Instant>,
    /// When the off-screen fallback timer fires (M16); `None` while no unobserved element has a
    /// pending callback, and then it is not armed (INV-IDLE-1).
    /// A held-back frame's start (LATENCY).
    render_at: Option<Instant>,
    fallback: Option<Instant>,
    /// When an idle notification timer fires; `None` while no notification is pending (INV-IDLE-1).
    idle: Option<Instant>,
    /// When the cursor animation redraw timer fires; `None` while no animation frame change is pending.
    cursor_anim: Option<Instant>,
    /// SIGTERM or SIGINT arrived.
    terminated: Rc<Cell<bool>>,
    /// The handlers writing to the self-pipe; removed (the signals then ignored) on drop.
    signal_ids: Vec<Registration>,
    /// SIGCHLD's write end until the session starts listening for it (`dispatch`).
    child_notify: Option<UnixStream>,
    x_display: Option<String>,
    x_authority: Option<std::path::PathBuf>,
    /// Serves a sandbox's listener that the state collected (`trust`).
    serve_sandbox: ServeSandbox,
    /// The recorder's socket file, removed with the session.
    _recorder_socket: Option<SocketFile>,
}

/// A socket file removed when dropped.
struct SocketFile(std::path::PathBuf);

impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Where the replay recorder connects: `$XDG_RUNTIME_DIR/infynitae/<socket>.recorder`, in the
/// compositor's own 0700 directory (a sandbox is not given it). `live/record/infynitae-recorder`
/// points `WAYLAND_DISPLAY` at it.
pub fn recorder_socket_path(socket_name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| {
        std::path::PathBuf::from(d)
            .join("infynitae")
            .join(format!("{socket_name}.recorder"))
    })
}

/// Opens the recorder's socket and serves it: its clients are marked as the recorder.
fn open_recorder_socket<D: 'static>(
    socket_name: &str,
    display: &Display<WaylandState>,
    handle: &LoopHandle<'static, D>,
) -> std::io::Result<Option<SocketFile>> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let Some(path) = recorder_socket_path(socket_name) else {
        return Ok(None);
    };
    let dir = path.parent().expect("the path has a directory");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    // a stale file of an earlier session of this socket name
    let _ = std::fs::remove_file(&path);
    let listener = std::os::unix::net::UnixListener::bind(&path)?;
    let file = SocketFile(path.clone());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let mut dh = display.handle();
    handle
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            move |_, listener, _| {
                while let Ok((stream, _)) = listener.accept() {
                    let state = ClientState::recorder();
                    match dh.insert_client(stream, Arc::new(state)) {
                        Ok(client) => {
                            if let (Ok(c), Some(data)) = (
                                client.get_credentials(&dh),
                                client.get_data::<ClientState>(),
                            ) {
                                data.identify(crate::trust::Identity::of_pid(c.pid));
                            }
                        }
                        Err(e) => eprintln!("[infynitae] Failed to insert the recorder: {e}"),
                    }
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(Some(file))
}

type ServeSandbox = Box<
    dyn Fn(
        smithay::wayland::security_context::SecurityContextListenerSource,
        smithay::wayland::security_context::SecurityContext,
    ),
>;

impl Session {
    /// Opens the listening socket and registers it and the display with the event loop, then
    /// starts the session's command if there is one. The socket and its lock file are removed
    /// when the event loop that owns their source is dropped.
    pub fn new<D: 'static>(
        mut display: Display<WaylandState>,
        handle: &LoopHandle<'static, D>,
        options: &SessionOptions,
    ) -> Result<Session, Box<dyn Error>> {
        // SIGTERM and SIGINT, first: from here on they only end the loop (the caller then shuts
        // down as for a run limit), so no signal can leave a socket behind half-way through
        // start-up. A handler writes a byte to a self-pipe the loop watches; no signal is
        // blocked, so the session's children inherit no blocked mask and the SIGTERM sent to
        // them at shutdown reaches them (`session_children_do_not_inherit_blocked_signals`).
        // Further signals only write again; once the session is dropped they are ignored, so a
        // second signal never interrupts the teardown (`repeated_signals_shut_down_once`).
        let terminated = Rc::new(Cell::new(false));
        let flag = terminated.clone();
        let (mut wake, notify) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        let mut signal_ids = vec![
            signals::register(SIGTERM, notify.try_clone()?)?,
            signals::register(SIGINT, notify)?,
        ];
        let wake_fd = wake.try_clone()?;
        handle
            .insert_source(
                Generic::new(wake_fd, Interest::READ, Mode::Level),
                move |_, _, _| {
                    let mut drain = [0u8; 64];
                    while matches!(wake.read(&mut drain), Ok(n) if n > 0) {}
                    flag.set(true);
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| format!("Failed to register signal source: {e}"))?;
        // Every process the session starts, and everything those start, belongs to the session
        // and ends with it (`ownership`): orphaned descendants are re-parented to the compositor,
        // which reaps them when SIGCHLD says one exited.
        if let Err(e) = crate::ownership::become_subreaper() {
            eprintln!(
                "[infynitae] Not a child subreaper ({e}): orphans of session processes escape the session"
            );
        }
        // SIGCHLD reaps the session's exits and the orphans it adopted. Only a session that starts
        // processes listens for it: an idle session that started nothing has nothing to wake for
        // (INV-IDLE-1), whatever else exits in this process. With an -e command that is now; else
        // from the first process the compositor starts itself (a shortcut), at a dispatch.
        let (mut child_wake, child_notify) = UnixStream::pair()?;
        child_wake.set_nonblocking(true)?;
        let child_fd = child_wake.try_clone()?;
        handle
            .insert_source(
                Generic::new(child_fd, Interest::READ, Mode::Level),
                move |_, _, _| {
                    let mut drain = [0u8; 64];
                    while matches!(child_wake.read(&mut drain), Ok(n) if n > 0) {}
                    crate::ownership::reap_adopted();
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| format!("Failed to register child-exit source: {e}"))?;
        let mut child_notify = Some(child_notify);
        if options.exec.is_some()
            && let Some(notify) = child_notify.take()
        {
            signal_ids.push(signals::register(SIGCHLD, notify)?);
        }
        let socket = match options.socket_name.as_deref() {
            Some(name) => ListeningSocketSource::with_name(name)?,
            None => ListeningSocketSource::new_auto()?,
        };
        let socket_name = socket.socket_name().to_string_lossy().into_owned();
        let mut clients = display.handle();
        handle
            .insert_source(socket, move |stream, _, _| {
                match clients.insert_client(stream, Arc::new(ClientState::default())) {
                    // a native client is known by its executable, read before its first request
                    // (`trust`)
                    Ok(client) => {
                        let identity = match client.get_credentials(&clients) {
                            Ok(c) => crate::trust::Identity::of_pid(c.pid),
                            Err(_) => crate::trust::Identity::Native {
                                exe: None,
                                path: None,
                            },
                        };
                        if let Some(data) = client.get_data::<ClientState>() {
                            data.identify(identity);
                        }
                    }
                    Err(e) => eprintln!("[infynitae] Failed to insert client: {e}"),
                }
            })
            .map_err(|e| format!("Failed to register listening socket: {e}"))?;
        // the replay recorder's own socket (M2): only what connects there is exempt from the
        // capture mark, never a client known by its executable's name
        let recorder_socket = match open_recorder_socket(&socket_name, &display, handle) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "[infynitae] No recorder socket ({e}): the replay buffer shows the capture mark"
                );
                None
            }
        };
        // a sandbox's listener (security-context-v1): every client through it is sandboxed. The
        // session holds the loop handle for this (the loop must not: a source holding its own
        // loop's handle would keep the loop, and the socket, alive)
        let listeners = handle.clone();
        let sandboxed_clients = display.handle();
        let serve_sandbox: ServeSandbox = Box::new(move |source, context| {
            let mut dh = sandboxed_clients.clone();
            let identity = crate::trust::sandboxed(&context);
            println!("[infynitae] Sandbox listener for {}", identity.describe());
            if let Err(e) = listeners.insert_source(source, move |stream, _, _| {
                let state = ClientState::with_identity(identity.clone());
                if let Err(e) = dh.insert_client(stream, Arc::new(state)) {
                    eprintln!("[infynitae] Failed to insert sandboxed client: {e}");
                }
            }) {
                eprintln!("[infynitae] Failed to serve a sandbox listener: {e}");
            }
        });
        // a readable display wakes the loop; the requests are dispatched by `dispatch`
        let display_ready = Rc::new(Cell::new(false));
        let ready = display_ready.clone();
        let display_fd = display.backend().poll_fd().try_clone_to_owned()?;
        handle
            .insert_source(
                Generic::new(display_fd, Interest::READ, Mode::Level),
                move |_, _, _| {
                    ready.set(true);
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| format!("Failed to register display event source: {e}"))?;
        println!("[infynitae] Listening on Wayland socket: {socket_name}");
        println!("[infynitae] To connect clients, set WAYLAND_DISPLAY={socket_name}");

        let mut session = Session {
            display,
            socket_name,
            children: Vec::new(),
            timer: FrameTimer::new(options.refresh_mhz),
            started: Instant::now(),
            frames: 0,
            duration: options.duration,
            max_frames: options.max_frames,
            display_ready,
            wake: None,
            fallback: None,
            render_at: None,
            idle: None,
            cursor_anim: None,
            terminated,
            signal_ids,
            child_notify,
            x_display: options.x_display.clone(),
            x_authority: options.x_authority.clone(),
            serve_sandbox,
            _recorder_socket: recorder_socket,
        };
        if let Some(cmd) = &options.exec {
            session.spawn(handle, cmd);
        }
        // the first frame
        session.request_frame();
        Ok(session)
    }

    pub fn socket_name(&self) -> &str {
        &self.socket_name
    }

    /// Starts a client of this session. Its exit wakes the loop, so it is reaped when it happens
    /// and not at the next unrelated event.
    fn spawn<D: 'static>(&mut self, handle: &LoopHandle<'_, D>, cmd: &str) {
        println!("[infynitae] Spawning initial client: {cmd}");
        let Some(mut command) = client_command(
            cmd,
            &self.socket_name,
            self.x_display.as_deref(),
            self.x_authority.as_deref(),
        ) else {
            return;
        };
        match crate::ownership::spawn_owned(&mut command) {
            Ok(child) => {
                println!("[infynitae] Client spawned with PID {}", child.id());
                let exited = Pid::from_raw(child.id() as i32)
                    .and_then(|pid| pidfd_open(pid, PidfdFlags::empty()).ok());
                if let Some(fd) = exited {
                    let _ = handle
                        .insert_source(Generic::new(fd, Interest::READ, Mode::Level), |_, _, _| {
                            Ok(PostAction::Remove)
                        });
                }
                self.children.push(child);
            }
            Err(e) => eprintln!("[infynitae] Failed to spawn {cmd}: {e}"),
        }
    }

    /// Milliseconds since the session started, on a monotonic clock.
    pub fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// The session clock: the one clock Core's scale-epoch deadlines are expressed on (M7). The
    /// session is its owner; nothing else mints a [`SessionTime`] in a running compositor. It is
    /// not the time an input event carries and not the presentation clock.
    pub fn now(&self) -> SessionTime {
        SessionTime::from_millis(self.now_ms())
    }

    /// Arms, moves or cancels the wake-up for a deadline of Core's. The loop's own sleep is
    /// bounded by it, so the deadline wakes the loop with no client request and no frame
    /// pending; `None` cancels it, and an idle session then has nothing to wake for. Calling it
    /// every turn with Core's current deadline is the whole protocol: there is no timer object
    /// to keep in step, so a deadline that no longer exists cannot fire.
    pub fn wake_at(&mut self, deadline: Option<SessionTime>) {
        self.wake = deadline.map(|at| self.started + Duration::from_millis(at.millis()));
    }

    /// How long the loop may sleep: until the pending frame, the duration limit, Core's deadline
    /// or the off-screen fallback (M16), whichever is first; `None` when none exists, and the loop
    /// then waits for events only.
    pub fn timeout(&self) -> Option<Duration> {
        let now = Instant::now();
        let limit = self
            .duration
            .map(|d| (self.started + d).saturating_duration_since(now));
        let wake = self.wake.map(|at| at.saturating_duration_since(now));
        let fallback = self.fallback.map(|t| t.saturating_duration_since(now));
        let idle = self.idle.map(|t| t.saturating_duration_since(now));
        let cursor_anim = self.cursor_anim.map(|t| t.saturating_duration_since(now));
        let render_at = self.render_at.map(|t| t.saturating_duration_since(now));
        [
            self.timer.until_due(now),
            limit,
            wake,
            fallback,
            idle,
            cursor_anim,
            render_at,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Arms the off-screen fallback timer for `deadline`, given on the presentation clock whose
    /// current time is `now` (Core's [`crate::WaylandState::frame_fallback`]), or disarms it.
    pub fn set_fallback_deadline(&mut self, deadline: Option<Duration>, now: Duration) {
        self.fallback = deadline.map(|d| Instant::now() + d.saturating_sub(now));
    }

    /// Arms the wake-up for a frame held back until `deadline` (LATENCY, on the presentation
    /// clock whose current time is `now`), or disarms it.
    pub fn set_render_deadline(&mut self, deadline: Option<Duration>, now: Duration) {
        self.render_at = deadline.map(|d| Instant::now() + d.saturating_sub(now));
    }

    /// Arms the idle notification timer for `deadline`, given on the presentation clock whose
    /// current time is `now` ([`crate::WaylandState::idle_notify_check`]), or disarms it.
    pub fn set_idle_deadline(&mut self, deadline: Option<Duration>, now: Duration) {
        self.idle = deadline.map(|d| Instant::now() + d.saturating_sub(now));
    }

    /// Arms the cursor animation redraw timer for `due`, given as duration until next frame change
    /// ([`crate::WaylandState::cursor_animation_due`]), or disarms it.
    pub fn set_cursor_animation_due(&mut self, due: Option<Duration>) {
        self.cursor_anim = due.map(|d| Instant::now() + d);
    }

    /// After the loop woke: reaps exited children, dispatches the clients' requests and flushes
    /// the replies. A request may have changed what is shown or asked for a frame callback, and
    /// a client that went away (which dispatches no request) took its surfaces with it, so any
    /// activity on the display requests a frame.
    pub fn dispatch(&mut self, state: &mut WaylandState) {
        self.children.retain_mut(|child| match child.try_wait() {
            Ok(Some(status)) => {
                println!(
                    "[infynitae] Child process {} exited with status {status}",
                    child.id()
                );
                crate::ownership::forget_spawned(child.id());
                false
            }
            Ok(None) => true,
            Err(e) => {
                eprintln!("[infynitae] Failed to wait on child {}: {e}", child.id());
                crate::ownership::forget_spawned(child.id());
                false
            }
        });
        state.launches.poll();
        if self.cursor_anim.is_some_and(|t| Instant::now() >= t) {
            self.cursor_anim = None;
            self.request_frame();
        }
        if state.spawned_children
            && let Some(notify) = self.child_notify.take()
        {
            match signals::register(SIGCHLD, notify) {
                Ok(r) => self.signal_ids.push(r),
                Err(e) => eprintln!("[infynitae] Not reaping child processes: {e}"),
            }
            // anything that exited before
            crate::ownership::reap_adopted();
        }
        let ready = self.display_ready.take();
        match self.display.dispatch_clients(state) {
            Ok(0) if !ready => {}
            Ok(_) => self.request_frame(),
            Err(e) => {
                eprintln!("[infynitae] Client dispatch error: {e}");
                self.request_frame();
            }
        }
        for (source, context) in state.security_listeners.drain(..) {
            (self.serve_sandbox)(source, context);
        }
        state.settle_unlock();
        state.clear_orphaned_selections();
        state.refuse_client_drags();
        state.prune_disabled_outputs();
        self.flush();
    }

    /// Something other than a client request changed what is shown (input, an output change, a
    /// redraw asked for by the host).
    pub fn request_frame(&mut self) {
        self.timer.request(Instant::now());
    }

    /// Whether the frame timer's tick is due; taking it ends the request. Under a frame limit
    /// the next frame is requested at once: the limit asks for that many frames, whether or not
    /// a client is active.
    pub fn frame_due(&mut self) -> bool {
        let now = Instant::now();
        if self.cursor_anim.is_some_and(|t| now >= t) {
            self.cursor_anim = None;
            self.timer.request(now);
        }
        let due = self.timer.take_due(now);
        if due && self.max_frames.is_some() {
            self.timer.request(now);
        }
        due
    }

    /// Counts a frame towards the frame limit.
    pub fn count_frame(&mut self) {
        self.frames += 1;
    }

    pub fn flush(&mut self) {
        let _ = self.display.flush_clients();
    }

    /// Whether a run limit has been reached or a termination signal arrived.
    pub fn finished(&self) -> bool {
        self.terminated.get()
            || self.duration.is_some_and(|d| self.started.elapsed() >= d)
            || self.max_frames.is_some_and(|n| self.frames >= n)
    }

    /// Ends the session's processes — its children and all their descendants, including
    /// orphans it adopted (`ownership`): SIGTERM, a bounded wait, then SIGKILL; every one is
    /// reaped. Nothing outside the compositor's own process subtree is signalled.
    pub fn shutdown(&mut self) {
        let t = Instant::now();
        let report =
            crate::ownership::end_session_tree(&mut self.children, CHILD_GRACE, &mut || {});
        self.report_teardown(t, &report);
    }

    /// [`Session::shutdown`] that keeps dispatching the clients while they quit: an app that
    /// waits for the compositor's reply as it closes would otherwise sit out the whole grace
    /// period and be killed.
    pub fn shutdown_with(&mut self, state: &mut WaylandState) {
        let display = &mut self.display;
        let mut tick = || {
            let _ = display.dispatch_clients(state);
            let _ = display.flush_clients();
        };
        let t = Instant::now();
        let report = crate::ownership::end_session_tree(&mut self.children, CHILD_GRACE, &mut tick);
        self.report_teardown(t, &report);
    }

    fn report_teardown(&mut self, t: Instant, report: &crate::ownership::Teardown) {
        if report.terminated > 0 {
            println!(
                "[infynitae] Session processes ended in {:?}: {} terminated, {} killed, {} left",
                t.elapsed(),
                report.terminated,
                report.killed,
                report.survivors
            );
        }
        self.flush();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A session dropped without `shutdown` (an error return after `Session::new`, e.g. the
        // nested backend's window or renderer failing, or a panic) still ends the processes it
        // started; after `shutdown` none is left and nothing happens here.
        if !self.children.is_empty() {
            if std::thread::panicking() {
                // already unwinding: no registry lock (a poisoned one would abort the process);
                // the direct children are killed and reaped
                for child in &mut self.children {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            } else {
                // the same teardown as a normal end (one code path, one seeded control)
                self.shutdown();
            }
        }
        // the notifications end; the handlers stay and do nothing (signals module doc)
        self.signal_ids.clear();
    }
}

/// The command of a client the session starts: `cmd` split on whitespace, with the session's
/// Wayland socket, and its X display and cookie or no `DISPLAY` / `XAUTHORITY` at all
/// (INV-XWAYLAND-1: never the compositor's own). Nothing of the compositor's own launch that
/// would misdirect the client is passed on: a `WAYLAND_SOCKET` fd, an activation token or
/// startup id meant for the compositor itself, or host desktop session variables
/// (`KDE_FULL_SESSION`, etc.). `None` for an empty command.
fn client_command(
    cmd: &str,
    socket_name: &str,
    x_display: Option<&str>,
    x_authority: Option<&std::path::Path>,
) -> Option<Command> {
    let mut parts = cmd.split_whitespace();
    let mut command = Command::new(parts.next()?);
    command
        .args(parts)
        .env("WAYLAND_DISPLAY", socket_name)
        .env_remove("WAYLAND_SOCKET")
        .env_remove("XDG_ACTIVATION_TOKEN")
        .env_remove("DESKTOP_STARTUP_ID")
        .env_remove("KDE_FULL_SESSION")
        .env_remove("KDE_SESSION_VERSION")
        .env_remove("KDE_SESSION_UID")
        .env_remove("KDE_APPLICATIONS_AS_SCOPE")
        .env_remove("XDG_MENU_PREFIX");
    crate::virtual_input::apply_keymap_env(&mut command);
    match x_display {
        Some(x) => command.env("DISPLAY", x),
        None => command.env_remove("DISPLAY"),
    };
    match x_authority {
        Some(a) => command.env("XAUTHORITY", a),
        None => command.env_remove("XAUTHORITY"),
    };
    Some(command)
}

/// Terminates and reaps child processes: those already gone are reaped, the rest get SIGTERM,
/// then up to `grace_period` to exit, then SIGKILL. No child is left running or as a zombie.
/// Only the given children are signalled.
pub fn terminate_children(children: &mut Vec<Child>, grace_period: Duration) {
    let reap = |children: &mut Vec<Child>| {
        children.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
    };
    reap(children);
    for child in children.iter() {
        let pid_raw = child.id() as i32;
        if let Some(pid) = Pid::from_raw(pid_raw) {
            match kill_process(pid, Signal::TERM) {
                // ESRCH: it exited in between
                Ok(()) | Err(Errno::SRCH) => {}
                Err(e) => eprintln!("[infynitae] Failed to send SIGTERM to child {pid_raw}: {e}"),
            }
        }
    }
    let start = Instant::now();
    while !children.is_empty() && start.elapsed() < grace_period {
        std::thread::sleep(Duration::from_millis(10));
        reap(children);
    }
    for child in children.iter_mut() {
        println!(
            "[infynitae] Child {} did not exit within grace period, escalating to SIGKILL",
            child.id()
        );
        let _ = child.kill();
        let _ = child.wait();
    }
    children.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// The deadlines of `count` ticks for a client that asks for the next frame as soon as it
    /// gets one, where producing each frame takes `work`.
    fn animate(refresh_mhz: u32, work: Duration, count: usize) -> Vec<Duration> {
        let mut timer = FrameTimer::new(refresh_mhz);
        let t0 = Instant::now();
        let mut now = t0;
        let mut ticks = Vec::new();
        timer.request(now);
        while ticks.len() < count {
            now += timer.until_due(now).expect("a frame is pending");
            assert!(timer.take_due(now));
            ticks.push(now - t0);
            now += work;
            timer.request(now);
        }
        ticks
    }

    /// The period does not grow by the time a frame takes: a continuously animating client gets
    /// tick n at exactly n periods, whatever the frame costs (below one period). Measuring the
    /// period from the end of the previous frame instead gives 58 Hz for 60 and 134 Hz for 165
    /// with these frame times.
    #[test]
    fn cadence_does_not_drift_with_frame_time() {
        for (refresh_mhz, period_ns) in [(60_000u32, 16_666_666u64), (165_000, 6_060_606)] {
            for work_us in [0u64, 500, 1400, 5000] {
                let ticks = animate(refresh_mhz, Duration::from_micros(work_us), 400);
                for (n, t) in ticks.iter().enumerate() {
                    assert_eq!(
                        t.as_nanos() as u64,
                        n as u64 * period_ns,
                        "{refresh_mhz} mHz with {work_us} us frames: tick {n}"
                    );
                }
            }
        }
    }

    /// Nothing requested: no deadline, so the loop has no timer to wake for. One request gives
    /// one tick; a second request while it is pending does not move it.
    #[test]
    fn idle_has_no_deadline_and_a_request_gives_one_tick() {
        let mut timer = FrameTimer::new(60_000);
        let t0 = Instant::now();
        assert_eq!(timer.until_due(t0), None);
        assert!(!timer.take_due(t0 + 100 * MS));
        timer.request(t0);
        assert!(timer.take_due(t0));
        assert_eq!(timer.until_due(t0 + MS), None, "the tick ended the request");
        timer.request(t0 + MS);
        let pending = timer.until_due(t0 + MS);
        assert_eq!(pending, Some(timer.period() - MS));
        timer.request(t0 + 5 * MS);
        assert_eq!(
            timer.until_due(t0 + MS),
            pending,
            "a second request moved the deadline"
        );
        assert!(!timer.take_due(t0 + 16 * MS), "taken before its deadline");
        assert!(timer.take_due(t0 + 17 * MS));
        assert!(!timer.take_due(t0 + 40 * MS), "one request gave two ticks");
        // a pending tick whose deadline has passed is not pushed back by further requests
        // (a client committing in a storm must not starve the frame)
        let mut timer = FrameTimer::new(60_000);
        timer.request(t0);
        timer.request(t0 + 5 * MS);
        assert_eq!(timer.until_due(t0 + 5 * MS), Some(Duration::ZERO));
        assert!(timer.take_due(t0 + MS));
    }

    /// A late tick is taken once, not once per missed period, and the cadence keeps its phase;
    /// after an idle stretch longer than a period a request is due at once.
    #[test]
    fn a_stall_is_not_made_up_for() {
        let mut timer = FrameTimer::new(100_000);
        let p = timer.period();
        let t0 = Instant::now();
        timer.request(t0);
        assert!(timer.take_due(t0));
        timer.request(t0);
        // the loop stalls for 3.5 periods
        let late = t0 + p * 7 / 2;
        assert!(timer.take_due(late));
        assert!(!timer.take_due(late), "a burst after a stall");
        timer.request(late);
        assert_eq!(
            timer.until_due(late),
            Some(p / 2),
            "the next tick is on the original cadence (t0 + 4 periods)"
        );
        assert!(timer.take_due(t0 + p * 4));
        // idle for many periods, then a request: due immediately
        let later = t0 + p * 50 + MS;
        timer.request(later);
        assert_eq!(timer.until_due(later), Some(Duration::ZERO));
    }

    use smithay::reexports::calloop::EventLoop;

    /// A session with no client and no child: after its first frame it has no deadline, so its
    /// loop sleeps until an event; with a duration limit the only deadline is that limit.
    #[test]
    fn an_idle_session_has_nothing_to_wake_for() {
        for duration in [None, Some(Duration::from_secs(30))] {
            let (core, _input, _shell, _outputs) =
                infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
            let display: Display<WaylandState> = Display::new().unwrap();
            let mut state = WaylandState::new(&display.handle(), core);
            let mut event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
            let options = SessionOptions {
                socket_name: Some(format!("infynitae-idle-test-{}", std::process::id())),
                refresh_mhz: 60_000,
                duration,
                ..SessionOptions::default()
            };
            let mut session = Session::new(display, &event_loop.handle(), &options).unwrap();
            assert_eq!(
                session.timeout().map(|t| t <= Duration::from_millis(17)),
                Some(true),
                "the first frame is pending"
            );
            event_loop.dispatch(Some(Duration::ZERO), &mut ()).unwrap();
            session.dispatch(&mut state);
            assert!(session.frame_due());
            for _ in 0..3 {
                event_loop.dispatch(Some(Duration::ZERO), &mut ()).unwrap();
                session.dispatch(&mut state);
                assert!(!session.frame_due(), "a frame nobody asked for");
            }
            match duration {
                None => assert_eq!(session.timeout(), None),
                Some(_) => assert!(session.timeout().unwrap() > Duration::from_secs(29)),
            }
            assert!(!session.finished());
            drop(event_loop);
        }
    }

    /// M16 / INV-IDLE-1: the off-screen fallback deadline bounds the loop's sleep while it is
    /// armed and not at all once disarmed; a deadline already passed wakes the loop at once.
    #[test]
    fn the_fallback_deadline_bounds_the_sleep_only_while_armed() {
        let (core, _input, _shell, _outputs) =
            infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
        let display: Display<WaylandState> = Display::new().unwrap();
        let mut state = WaylandState::new(&display.handle(), core);
        let mut event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
        let options = SessionOptions {
            socket_name: Some(format!("infynitae-fallback-test-{}", std::process::id())),
            refresh_mhz: 60_000,
            ..SessionOptions::default()
        };
        let mut session = Session::new(display, &event_loop.handle(), &options).unwrap();
        event_loop.dispatch(Some(Duration::ZERO), &mut ()).unwrap();
        session.dispatch(&mut state);
        assert!(session.frame_due());
        assert_eq!(session.timeout(), None, "idle");
        let now = Duration::from_secs(100);
        session.set_fallback_deadline(Some(now + 500 * MS), now);
        let t = session.timeout().expect("armed");
        assert!(t <= 500 * MS && t > 450 * MS, "{t:?}");
        session.set_fallback_deadline(Some(now - MS), now);
        assert_eq!(session.timeout(), Some(Duration::ZERO), "overdue");
        session.set_fallback_deadline(None, now);
        assert_eq!(session.timeout(), None, "disarmed");
        drop(event_loop);
    }

    #[test]
    fn the_cursor_animation_due_bounds_the_sleep_and_requests_a_frame() {
        let (core, _input, _shell, _outputs) =
            infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
        let display: Display<WaylandState> = Display::new().unwrap();
        let mut state = WaylandState::new(&display.handle(), core);
        let mut event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
        let options = SessionOptions {
            socket_name: Some(format!("infynitae-anim-test-{}", std::process::id())),
            refresh_mhz: 60_000,
            ..SessionOptions::default()
        };
        let mut session = Session::new(display, &event_loop.handle(), &options).unwrap();
        event_loop.dispatch(Some(Duration::ZERO), &mut ()).unwrap();
        session.dispatch(&mut state);
        assert!(session.frame_due());
        assert_eq!(session.timeout(), None, "idle");

        // Arm cursor animation in 100ms
        session.set_cursor_animation_due(Some(100 * MS));
        let t = session.timeout().expect("armed cursor anim");
        assert!(t <= 100 * MS && t > 50 * MS, "{t:?}");

        // Disarm
        session.set_cursor_animation_due(None);
        assert_eq!(session.timeout(), None, "disarmed cursor anim");

        // Overdue cursor animation requests a frame
        session.set_cursor_animation_due(Some(Duration::ZERO));
        assert_eq!(session.timeout(), Some(Duration::ZERO));
        event_loop.dispatch(session.timeout(), &mut ()).unwrap();
        session.dispatch(&mut state);
        std::thread::sleep(Duration::from_millis(20));
        assert!(session.frame_due(), "overdue animation triggers frame_due");

        drop(event_loop);
    }

    /// A session dropped without `shutdown` (an early error return in a backend after
    /// `Session::new`) still ends the client it started: none outlives the session (2026-10-20).
    #[test]
    fn a_session_dropped_without_shutdown_ends_its_client() {
        let display: Display<WaylandState> = Display::new().unwrap();
        let event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
        let options = SessionOptions {
            socket_name: Some(format!("infynitae-dropexec-test-{}", std::process::id())),
            exec: Some("sleep 30".into()),
            refresh_mhz: 60_000,
            ..SessionOptions::default()
        };
        let session = Session::new(display, &event_loop.handle(), &options).unwrap();
        let pids: Vec<u32> = session.children.iter().map(|c| c.id()).collect();
        assert_eq!(pids.len(), 1, "the client was started");
        let t = Instant::now();
        drop(session);
        for pid in pids {
            let state = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
            if state.is_some() {
                // a failing run must not leave its client behind
                if let Some(p) = Pid::from_raw(pid as i32) {
                    let _ = kill_process(p, Signal::KILL);
                }
            }
            assert!(
                state.is_none(),
                "client {pid} outlived the session: {state:?}"
            );
        }
        assert!(t.elapsed() < CHILD_GRACE + Duration::from_secs(1));
        drop(event_loop);
    }

    fn session(
        name: &str,
        max_frames: Option<u64>,
    ) -> (Session, WaylandState, EventLoop<'static, ()>) {
        let (core, _input, _shell, _outputs) =
            infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
        let display: Display<WaylandState> = Display::new().unwrap();
        let state = WaylandState::new(&display.handle(), core);
        let event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
        let options = SessionOptions {
            socket_name: Some(format!("infynitae-{name}-test-{}", std::process::id())),
            refresh_mhz: 1_000_000,
            max_frames,
            ..SessionOptions::default()
        };
        let session = Session::new(display, &event_loop.handle(), &options).unwrap();
        (session, state, event_loop)
    }

    /// A deadline of Core's (a scale-settlement deadline, INV-SCALE-3) wakes an otherwise idle
    /// session by itself: no client request, no frame. It is not a frame; it can be moved in
    /// both directions; a deadline already past gives a zero sleep, not a negative one; and when
    /// it is cancelled the idle session again has nothing to wake for (INV-IDLE-1).
    #[test]
    fn a_core_deadline_wakes_an_idle_session_and_only_while_armed() {
        let (mut session, mut state, mut event_loop) = session("wake", None);
        while session.timeout().is_some() {
            turn(&mut session, &mut state, &mut event_loop);
        }
        assert_eq!(session.timeout(), None, "idle: nothing to wake for");

        let at = session.now_ms() + 60;
        session.wake_at(Some(SessionTime::from_millis(at)));
        let sleep = session
            .timeout()
            .expect("an armed deadline bounds the sleep");
        assert!(
            sleep <= Duration::from_millis(60),
            "sleeps past the deadline: {sleep:?}"
        );
        assert!(
            sleep >= Duration::from_millis(20),
            "the deadline is 60 ms away: {sleep:?}"
        );
        // the loop itself sleeps until the deadline and returns with no event to dispatch
        let before = Instant::now();
        event_loop.dispatch(session.timeout(), &mut ()).unwrap();
        session.dispatch(&mut state);
        assert!(session.now_ms() >= at, "woke before the deadline");
        assert!(
            before.elapsed() < Duration::from_secs(2),
            "did not wake at the deadline"
        );
        assert!(!session.frame_due(), "a settlement deadline is not a frame");
        assert_eq!(
            session.timeout(),
            Some(Duration::ZERO),
            "a due deadline: no sleep"
        );

        session.wake_at(Some(SessionTime::from_millis(at + 10_000)));
        assert!(
            session.timeout().unwrap() > Duration::from_secs(9),
            "moved later"
        );
        session.wake_at(Some(SessionTime::from_millis(at + 100)));
        assert!(
            session.timeout().unwrap() <= Duration::from_millis(100),
            "moved earlier"
        );
        session.wake_at(None);
        assert_eq!(session.timeout(), None, "cancelled: idle again");
        drop(event_loop);
    }

    /// One loop turn; whether a frame was taken.
    fn turn(
        session: &mut Session,
        state: &mut WaylandState,
        event_loop: &mut EventLoop<()>,
    ) -> bool {
        event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut ())
            .unwrap();
        session.dispatch(state);
        session.frame_due()
    }

    /// A client that goes away without a request (killed, or its connection dropped) dispatches
    /// nothing, yet what is shown changed: the session must still produce a frame.
    #[test]
    fn a_vanished_client_gets_a_frame() {
        let (mut session, mut state, mut event_loop) = session("vanish", None);
        let path = std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap())
            .join(session.socket_name());
        let client = std::os::unix::net::UnixStream::connect(path).unwrap();
        // accept the client and let the session go idle
        for _ in 0..5 {
            turn(&mut session, &mut state, &mut event_loop);
        }
        assert_eq!(session.timeout(), None, "idle with a silent client");
        drop(client);
        let framed = (0..20).any(|_| turn(&mut session, &mut state, &mut event_loop));
        assert!(framed, "no frame after the client vanished");
        assert_eq!(session.timeout(), None, "idle again afterwards");
    }

    /// INV-XWAYLAND-1: a client the session starts gets the session's X display, or no
    /// `DISPLAY` at all, never the one the compositor itself was started with (this test's own
    /// environment may well have one).
    #[test]
    fn a_session_client_gets_the_sessions_x_display_or_none() {
        for (n, x_display) in [None, Some(":42".to_string())].into_iter().enumerate() {
            let dir = std::env::temp_dir().join(format!(
                "infynitae-session-display-{}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let out = dir.join("out");
            let script = dir.join("client.sh");
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\necho \"${{DISPLAY-unset}}\" > {}.tmp && mv {0}.tmp {0}\n",
                    out.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
            let (core, _input, _shell, _outputs) =
                infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
            let display: Display<WaylandState> = Display::new().unwrap();
            let _state = WaylandState::new(&display.handle(), core);
            let event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
            let options = SessionOptions {
                socket_name: Some(format!("infynitae-display-test-{}-{n}", std::process::id())),
                exec: Some(script.display().to_string()),
                refresh_mhz: 60_000,
                x_display: x_display.clone(),
                ..SessionOptions::default()
            };
            let mut session = Session::new(display, &event_loop.handle(), &options).unwrap();
            let end = Instant::now() + Duration::from_secs(10);
            while !out.exists() {
                assert!(Instant::now() < end, "the client never ran");
                std::thread::sleep(MS);
            }
            let got = std::fs::read_to_string(&out).unwrap();
            assert_eq!(
                got.trim(),
                x_display.as_deref().unwrap_or("unset"),
                "x_display {x_display:?}"
            );
            session.shutdown();
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    /// INV-XWAYLAND-1: the explicit environment of a session client, whatever the environment
    /// this test runs in: the session's socket, and its X display or `DISPLAY` removed.
    #[test]
    fn a_client_command_sets_or_removes_display() {
        let env = |x, a: Option<&std::path::Path>| {
            let c = client_command("prog a b", "wl-7", x, a).unwrap();
            let mut e: Vec<(String, Option<String>)> = c
                .get_envs()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.map(|v| v.to_string_lossy().into_owned()),
                    )
                })
                .collect();
            // the seat's keymap names are set once per process, by whichever test makes a
            // keyboard first: present or not depending on the test order
            e.retain(|(k, _)| !k.contains("XKB_"));
            e.sort();
            (c.get_program().to_owned(), c.get_args().count(), e)
        };
        let removed = |k: &str| (k.to_string(), None);
        let (program, args, e) = env(None, None);
        assert_eq!((program.to_str(), args), (Some("prog"), 2));
        assert_eq!(
            e,
            [
                removed("DESKTOP_STARTUP_ID"),
                removed("DISPLAY"),
                removed("KDE_APPLICATIONS_AS_SCOPE"),
                removed("KDE_FULL_SESSION"),
                removed("KDE_SESSION_UID"),
                removed("KDE_SESSION_VERSION"),
                ("WAYLAND_DISPLAY".into(), Some("wl-7".into())),
                removed("WAYLAND_SOCKET"),
                removed("XAUTHORITY"),
                removed("XDG_ACTIVATION_TOKEN"),
                removed("XDG_MENU_PREFIX"),
            ]
        );
        let (_, _, e) = env(Some(":3"), Some(std::path::Path::new("/run/x/cookie")));
        assert!(e.contains(&("DISPLAY".into(), Some(":3".into()))), "{e:?}");
        assert!(
            e.contains(&("XAUTHORITY".into(), Some("/run/x/cookie".into()))),
            "{e:?}"
        );
        assert!(client_command("   ", "wl-7", None, None).is_none());
    }

    /// A frame limit ends a session that has no client: frames keep coming until it is reached.
    #[test]
    fn a_frame_limit_is_reached_without_clients() {
        let (mut session, mut state, mut event_loop) = session("limit", Some(5));
        let mut frames = 0;
        for _ in 0..200 {
            if session.finished() {
                break;
            }
            assert!(
                session.timeout().is_some(),
                "nothing to wake for before the limit"
            );
            if turn(&mut session, &mut state, &mut event_loop) {
                session.count_frame();
                frames += 1;
            }
        }
        assert!(session.finished());
        assert_eq!(frames, 5);
    }

    /// A child of this test, registered as owned (session process ownership): a session another
    /// test shuts down in this process leaves it alone.
    fn spawn(cmd: &str, args: &[&str]) -> Child {
        crate::ownership::spawn_owned(Command::new(cmd).args(args)).unwrap()
    }

    /// Whether the process still exists in any state, a zombie included.
    fn exists(pid: u32) -> bool {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }

    #[test]
    fn an_exited_child_is_reaped() {
        let child = spawn("true", &[]);
        let pid = child.id();
        let mut children = vec![child];
        std::thread::sleep(Duration::from_millis(50));
        terminate_children(&mut children, Duration::from_millis(500));
        assert!(children.is_empty());
        assert!(!exists(pid), "left as a zombie");
    }

    #[test]
    fn an_adopted_orphan_is_reaped_without_exec_command() {
        let (mut session, mut state, mut event_loop) = session("orphan", None);
        // as a shortcut does: the compositor started a process of its own
        state.spawned_children = true;
        let mut child = crate::ownership::spawn_owned(
            Command::new("sh")
                .arg("-c")
                .arg("sh -c 'sleep 0.1 & echo $!'")
                .stdout(std::process::Stdio::piped()),
        )
        .unwrap();
        let mut out = String::new();
        std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut out).unwrap();
        child.wait().unwrap();
        crate::ownership::forget_spawned(child.id());
        let orphan_pid: u32 = out.trim().parse().unwrap();
        assert!(exists(orphan_pid));

        let start = Instant::now();
        while exists(orphan_pid) && start.elapsed() < Duration::from_secs(5) {
            turn(&mut session, &mut state, &mut event_loop);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!exists(orphan_pid), "orphan {orphan_pid} remained a zombie");
    }

    #[test]
    fn a_running_child_exits_on_sigterm_within_the_grace_period() {
        let child = spawn("sleep", &["10"]);
        let pid = child.id();
        let mut children = vec![child];
        let start = Instant::now();
        terminate_children(&mut children, Duration::from_millis(1000));
        assert!(children.is_empty());
        assert!(start.elapsed() < Duration::from_millis(500));
        assert!(!exists(pid), "still running or a zombie");
        terminate_children(&mut children, Duration::from_millis(500));
    }

    #[test]
    fn a_child_ignoring_sigterm_is_killed_after_the_grace_period() {
        let child = spawn("sh", &["-c", "trap '' TERM; while true; do sleep 1; done"]);
        let pid = child.id();
        let mut children = vec![child];
        std::thread::sleep(Duration::from_millis(50));
        let start = Instant::now();
        terminate_children(&mut children, Duration::from_millis(100));
        let elapsed = start.elapsed();
        assert!(children.is_empty());
        assert!(
            elapsed >= Duration::from_millis(80),
            "killed after {elapsed:?}"
        );
        assert!(elapsed < Duration::from_millis(500), "took {elapsed:?}");
        assert!(!exists(pid), "still running or a zombie");
    }
}
