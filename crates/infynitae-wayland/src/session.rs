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

use signal_hook::SigId;
use signal_hook::consts::{SIGCHLD, SIGINT, SIGTERM};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
use smithay::reexports::rustix::io::Errno;
use smithay::reexports::rustix::process::{Pid, PidfdFlags, Signal, kill_process, pidfd_open};
use smithay::reexports::wayland_server::Display;
use smithay::wayland::socket::ListeningSocketSource;

use crate::{ClientState, WaylandState};

/// How long a child gets between SIGTERM and SIGKILL at shutdown.
const CHILD_GRACE: Duration = Duration::from_millis(500);

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
    /// SIGTERM or SIGINT arrived.
    terminated: Rc<Cell<bool>>,
    /// The handlers writing to the self-pipe; removed (the signals then ignored) on drop.
    signal_ids: Vec<SigId>,
}

impl Session {
    /// Opens the listening socket and registers it and the display with the event loop, then
    /// starts the session's command if there is one. The socket and its lock file are removed
    /// when the event loop that owns their source is dropped.
    pub fn new<D: 'static>(
        mut display: Display<WaylandState>,
        handle: &LoopHandle<'_, D>,
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
            signal_hook::low_level::pipe::register(SIGTERM, notify.try_clone()?)?,
            signal_hook::low_level::pipe::register(SIGINT, notify)?,
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
        let (mut child_wake, child_notify) = UnixStream::pair()?;
        child_wake.set_nonblocking(true)?;
        signal_ids.push(signal_hook::low_level::pipe::register(
            SIGCHLD,
            child_notify,
        )?);
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
        let socket = match options.socket_name.as_deref() {
            Some(name) => ListeningSocketSource::with_name(name)?,
            None => ListeningSocketSource::new_auto()?,
        };
        let socket_name = socket.socket_name().to_string_lossy().into_owned();
        let mut clients = display.handle();
        handle
            .insert_source(socket, move |stream, _, _| {
                if let Err(e) = clients.insert_client(stream, Arc::new(ClientState::default())) {
                    eprintln!("[infynitae] Failed to insert client: {e}");
                }
            })
            .map_err(|e| format!("Failed to register listening socket: {e}"))?;
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
            terminated,
            signal_ids,
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
        let mut parts = cmd.split_whitespace();
        let Some(bin) = parts.next() else { return };
        let mut command = Command::new(bin);
        command
            .args(parts)
            .env("WAYLAND_DISPLAY", &self.socket_name);
        match command.spawn() {
            Ok(child) => {
                println!("[infynitae] Client spawned with PID {}", child.id());
                crate::ownership::register_spawned(child.id());
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

    /// How long the loop may sleep: until the pending frame or the duration limit, whichever is
    /// first; `None` when neither exists, and the loop then waits for events only.
    pub fn timeout(&self) -> Option<Duration> {
        let now = Instant::now();
        let limit = self
            .duration
            .map(|d| (self.started + d).saturating_duration_since(now));
        match (self.timer.until_due(now), limit) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
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
        let ready = self.display_ready.take();
        match self.display.dispatch_clients(state) {
            Ok(0) if !ready => {}
            Ok(_) => self.request_frame(),
            Err(e) => {
                eprintln!("[infynitae] Client dispatch error: {e}");
                self.request_frame();
            }
        }
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
        let report = crate::ownership::end_session_tree(&mut self.children, CHILD_GRACE);
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
        for id in self.signal_ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
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

    fn spawn(cmd: &str, args: &[&str]) -> Child {
        Command::new(cmd).args(args).spawn().unwrap()
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
