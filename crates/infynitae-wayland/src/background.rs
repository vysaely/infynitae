//! Apps out of sight yield the CPU; a fullscreen game gets game mode.
//!
//! Background priority (`BACKGROUND_PRIORITY`, on by default): an app none of whose windows any
//! screen has shown for [`HIDE_AFTER`] runs at nice [`BACKGROUND_NICE`], every thread of it and
//! of the processes it started (a browser's tabs), and back at its own nice as soon as one of its
//! windows is in sight or it has no window left. Nice only matters when the CPU is busy: an app
//! out of sight still runs, it just goes after the ones you look at. Never lowered: an app that
//! interacts with the person, as Core decides (`Core::app_in_use`: a key or click into it in the
//! last five minutes, its app id in `BACKGROUND_KEEP`, a window being shared (captured in the
//! last [`CAPTURE_HOLD`]), the game, a sound stream of it playing or recording
//! ([`crate::streams`])), and X11 apps (one xwayland-satellite serves them all). A thread is lowered only when its process may raise it
//! back (`RLIMIT_NICE`, which the installer sets so apps return to nice 0 and never above it);
//! without that limit nothing is lowered. A thread of its own walks /proc and sets the nice
//! values, in the order the main loop decided them: an app of hundreds of threads is hundreds of
//! calls (2026-11-23, 25 game clients: 11 to 14 ms a turn on the main loop).
//!
//! Game mode (`GAME_MODE`, on by default): a fullscreen window with a frame on screen, shown
//! directly or composited ([`WaylandState::fullscreen_frame_shown`]), is a game. Its app is registered with Feral's
//! gamemode, when installed (`launch.sh game-mode`), until the window has been off screen for
//! [`GAME_MODE_HOLD`] continuously.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use infynitae_core::AppFacts;
use infynitae_core::output::OutputId;
use infynitae_core::stage::ElementId;
use smithay::reexports::rustix::process::{Pid, getpid, getpriority_process, setpriority_process};

use crate::WaylandState;
use crate::log_line;
use crate::streams::{Running, Streams};

/// How long an app's windows are all out of sight before it is lowered.
pub const HIDE_AFTER: Duration = Duration::from_secs(15);

/// The nice value of an app out of sight.
pub const BACKGROUND_NICE: i32 = 10;

/// A shared window counts as in sight until this long after its last captured frame.
pub const CAPTURE_HOLD: Duration = Duration::from_secs(5);

/// How long a game must be off screen before game mode ends.
pub const GAME_MODE_HOLD: Duration = Duration::from_secs(5);

/// The executable whose clients are X11 apps, all through one process.
const SATELLITE: &str = "xwayland-satellite";

/// How often apps out of sight are looked at.
const SCAN_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// The background priority and game mode state.
pub struct Background {
    /// `BACKGROUND_PRIORITY`.
    pub(crate) enabled: bool,
    /// `GAME_MODE`.
    pub(crate) game_mode: bool,
    /// Each window's app (`None`: an X11 app, the compositor itself, or unknown).
    pids: HashMap<ElementId, Option<u32>>,
    /// When each windowed app's windows were last all out of sight, while they are.
    hidden_since: HashMap<u32, Instant>,
    /// The apps lowered now, or queued to be.
    lowered: HashSet<u32>,
    /// The thread that lowers and restores, started with the first app lowered.
    worker: Option<(Sender<Job>, JoinHandle<()>)>,
    /// The last captured frame of each window shared alone.
    captured: HashMap<ElementId, Instant>,
    /// The game: its window and app.
    pub(crate) game: Option<(ElementId, u32)>,
    /// The sound streams running (an app playing or recording keeps its priority).
    streams: Streams,
    /// When the game was last seen off screen, while game mode is on.
    pub(crate) game_off_since: Option<Instant>,
    /// The last scan of the apps out of sight (`background_update`).
    pub(crate) scanned_at: Option<Instant>,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            enabled: true,
            game_mode: true,
            scanned_at: None,
            pids: HashMap::new(),
            hidden_since: HashMap::new(),
            lowered: HashSet::new(),
            worker: None,
            captured: HashMap::new(),
            game: None,
            game_off_since: None,
            streams: Streams::default(),
        }
    }
}

/// A lowered app: the nice of each thread it lowered, by thread id.
#[derive(Default)]
struct Lowered {
    threads: HashMap<i32, i32>,
    /// Threads already at [`BACKGROUND_NICE`] or lower when the tree was lowered (an app the user
    /// started with `nice`): left alone then, and never raised by the restore.
    kept: std::collections::HashSet<i32>,
}

/// The worker's work: an app to lower or restore, and the apps with windows its tree stops at.
enum Job {
    Lower(u32, Vec<u32>),
    Restore(u32, Vec<u32>),
}

/// The worker: does each job in turn, keeping each lowered app's threads' nice before.
fn work(jobs: std::sync::mpsc::Receiver<Job>) {
    let mut lowered: HashMap<u32, Lowered> = HashMap::new();
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Lower(pid, windowed) => {
                lowered.insert(pid, lower(pid, &windowed));
            }
            Job::Restore(pid, windowed) => {
                if let Some(l) = lowered.remove(&pid) {
                    restore(pid, &l, &windowed);
                }
            }
        }
    }
}

impl Drop for Background {
    /// Waits for the restores queued (at shutdown): an app left at nice 10 stays there.
    fn drop(&mut self) {
        if let Some((jobs, thread)) = self.worker.take() {
            drop(jobs);
            if thread.join().is_err() {
                log_line!("[infynitae] The background priority thread panicked");
            }
        }
    }
}

/// What [`Background::plan`] decided.
#[derive(Debug, Default, PartialEq, Eq)]
struct Plan {
    lower: Vec<u32>,
    restore: Vec<u32>,
}

impl Background {
    /// The decision, from each windowed app and whether it is in use now
    /// (`Core::app_in_use`): the apps to lower and to restore. Pure but for its timers.
    fn plan(&mut self, now: Instant, apps: &BTreeMap<u32, bool>) -> Plan {
        let mut plan = Plan::default();
        self.hidden_since
            .retain(|pid, _| apps.get(pid) == Some(&false));
        for (&pid, &in_use) in apps {
            if in_use || !self.enabled {
                if self.lowered.contains(&pid) {
                    plan.restore.push(pid);
                }
                continue;
            }
            let since = *self.hidden_since.entry(pid).or_insert(now);
            if now.duration_since(since) >= HIDE_AFTER && !self.lowered.contains(&pid) {
                plan.lower.push(pid);
            }
        }
        // an app whose last window closed (one that lives on in the tray)
        plan.restore.extend(
            self.lowered
                .iter()
                .filter(|pid| !apps.contains_key(pid))
                .copied(),
        );
        plan.restore.sort_unstable();
        plan
    }

    /// Lowers `pid`'s tree, down to (not into) the apps in `windowed`, on the worker.
    fn lower_app(&mut self, pid: u32, windowed: &[u32]) {
        if self.send(Job::Lower(pid, windowed.to_vec())) {
            self.lowered.insert(pid);
        }
    }

    /// Raises `pid`'s tree back, if it was lowered, on the worker.
    fn restore_app(&mut self, pid: u32, windowed: &[u32]) {
        if self.lowered.remove(&pid) {
            self.send(Job::Restore(pid, windowed.to_vec()));
        }
    }

    /// Queues a job, starting the worker first if needed; false if it could not be queued.
    fn send(&mut self, job: Job) -> bool {
        if self.worker.is_none() {
            let (jobs, queue) = channel();
            match std::thread::Builder::new()
                .name("infynitae-background".into())
                .spawn(move || work(queue))
            {
                Ok(thread) => self.worker = Some((jobs, thread)),
                Err(e) => {
                    log_line!(
                        "[infynitae] Background priority: no thread ({e}); apps keep their priority"
                    );
                    return false;
                }
            }
        }
        let Some((jobs, _)) = &self.worker else {
            return false;
        };
        if jobs.send(job).is_ok() {
            return true;
        }
        // the worker panicked: a new one next time
        self.worker = None;
        log_line!("[infynitae] Background priority: the thread ended; apps keep their priority");
        false
    }

    /// How long until an app out of sight is due to be lowered, a share's hold ends, or game
    /// mode ends (the backend wakes then); `None` when nothing waits.
    pub fn due(&self, now: Instant) -> Option<Duration> {
        let game_hold = if self.game_mode {
            self.game_off_since.map(|since| since + GAME_MODE_HOLD)
        } else {
            None
        };
        if !self.enabled {
            return game_hold.map(|t| t.saturating_duration_since(now));
        }
        let lowering = self
            .hidden_since
            .iter()
            .filter(|(pid, _)| !self.lowered.contains(pid))
            .map(|(_, since)| *since + HIDE_AFTER);
        let holds = self.captured.values().map(|t| *t + CAPTURE_HOLD);
        lowering
            .chain(holds)
            .chain(game_hold)
            .min()
            .map(|t| t.saturating_duration_since(now))
    }

    /// Tracks whether the game is still exclusive on a screen; returns the process id if game
    /// mode just ended (off screen for [`GAME_MODE_HOLD`] continuously). Pure but for its timer.
    pub fn update_game(&mut self, now: Instant, exclusive: bool) -> Option<u32> {
        let (_window, pid) = self.game?;
        if exclusive && self.game_mode {
            self.game_off_since = None;
            return None;
        }
        let since = *self.game_off_since.get_or_insert(now);
        if now.duration_since(since) >= GAME_MODE_HOLD || !self.game_mode {
            self.game = None;
            self.game_off_since = None;
            Some(pid)
        } else {
            None
        }
    }
}

impl WaylandState {
    /// The background priority and game mode state.
    pub fn background(&self) -> &Background {
        &self.background
    }

    /// A frame of `output` was shown. The fullscreen window on it is the game (owner, 2026-11-01:
    /// a game that was not scanned out and asked for nothing never got game mode, and needs the
    /// CPU most; a fullscreen video gets it too, which costs little).
    pub fn fullscreen_frame_shown(&mut self, output: OutputId) {
        if !self.background.game_mode {
            return;
        }
        // fitting alone isn't a game: a maximized editor is not; a fitted window that tears is.
        // With TEARING="always" that is every window filling the screen (a maximised browser
        // too): the owner wants the desktop as quick as a game (2026-11-12, "i want the desktop
        // to be responsive too"); only holding notifications back asks for a real game
        let fullscreen = self
            .core
            .presentation(output)
            .filter(|_| !self.core.is_presentation_suppressed(output));
        let fitted = self.fitted_window(output).filter(|_| self.tears(output));
        let Some(window) = fullscreen.or(fitted) else {
            return;
        };
        let Some(pid) = self.client_pid(window) else {
            return;
        };
        if let Some((cur_win, cur_pid)) = self.background.game {
            if cur_win == window {
                self.background.game_off_since = None;
                return;
            }
            log_line!("[infynitae] Game mode off for process {cur_pid}");
            self.gamemode(false, cur_pid, false);
        }
        // notifications wait (GAME_QUIET) for a fullscreen window or one that plays like a game,
        // never for a maximised browser
        let quiet = fullscreen == Some(window) || self.plays_like_a_game(window);
        log_line!(
            "[infynitae] Game mode on for process {pid}{}",
            if quiet { ", notifications held" } else { "" }
        );
        self.background.game = Some((window, pid));
        self.background.game_off_since = None;
        self.gamemode(true, pid, quiet);
    }

    /// A window filling the screen without asking for fullscreen plays like a game when it asked to
    /// tear or was made to (Super+Shift+T), says it shows a game (`wp_content_type_v1`), holds
    /// the pointer (locked or confined), or is an X11 window (they come through one
    /// xwayland-satellite; a screen-filling X11 window is a game far more often than not).
    /// Checked once per new game window, never per frame.
    pub(crate) fn plays_like_a_game(&self, window: ElementId) -> bool {
        use smithay::reexports::wayland_protocols::wp::content_type::v1::server::wp_content_type_v1::Type;
        let Some(surface) = self.window_surface(window) else {
            return false;
        };
        let asked = self.tearing_asked(window, &surface);
        let game_content = self.content_type(window) == Type::Game;
        let key = self.registry.key(&surface);
        let holds_pointer = key.is_some()
            && self
                .core
                .constraint()
                .is_some_and(|c| Some(c.surface) == key);
        asked || game_content || holds_pointer || self.client_pid(window).is_some_and(is_satellite)
    }

    /// Registers the game with gamemode or ends it (`launch.sh game-mode`); never for this
    /// process (a client in the compositor's own process, as in the test suite).
    /// `quiet`: notifications wait until it ends (`GAME_QUIET`).
    fn gamemode(&mut self, on: bool, pid: u32, quiet: bool) {
        if pid != std::process::id() {
            let state = if on { "on" } else { "off" };
            let quiet = if quiet { " quiet" } else { "" };
            self.spawn_action_with("game-mode", &format!("{state} {pid}{quiet}"));
        }
    }

    /// The game, if game mode is on: its window and app.
    pub fn game(&self) -> Option<(ElementId, u32)> {
        self.background.game
    }

    /// A window shared alone drew a frame: its app counts as in sight for [`CAPTURE_HOLD`].
    pub(crate) fn note_window_capture(&mut self, window: ElementId) {
        self.background.captured.insert(window, Instant::now());
    }

    /// Lowers and restores apps by what the screens show now, and ends game mode when the game
    /// has been off screen for [`GAME_MODE_HOLD`] continuously. Cheap: the backends call it on
    /// every turn of their loop.
    pub fn background_update(&mut self, now: Instant) {
        if let Some((window, _pid)) = self.background.game {
            let still = self.background.game_mode
                && self
                    .core
                    .outputs()
                    .iter()
                    .any(|o| self.exclusive_window(o.id) == Some(window));
            if let Some(ended_pid) = self.background.update_game(now, still) {
                log_line!("[infynitae] Game mode off for process {ended_pid}");
                self.gamemode(false, ended_pid, false);
            }
        }
        let bg = &self.background;
        if !bg.enabled && bg.lowered.is_empty() && bg.hidden_since.is_empty() {
            return;
        }
        // its waits are seconds long: ten scans a second are plenty, not one per wake (about
        // 2,000 a second in a game, each listing and walking every window)
        if bg
            .scanned_at
            .is_some_and(|t| now.duration_since(t) < SCAN_EVERY)
        {
            return;
        }
        self.background.scanned_at = Some(now);

        let mapped = self.core.stage().mapped_elements();
        self.background.pids.retain(|e, _| mapped.contains(e));
        self.background
            .captured
            .retain(|e, t| mapped.contains(e) && now.duration_since(*t) < CAPTURE_HOLD);
        let game = self.background.game.map(|(_, pid)| pid);
        let own = getpid().as_raw_nonzero().get() as u32;
        // each windowed app: its windows, and whether one of them is being shared
        let mut windows: BTreeMap<u32, (Vec<ElementId>, bool)> = BTreeMap::new();
        for e in mapped {
            let pid = match self.background.pids.get(&e) {
                Some(pid) => *pid,
                None => {
                    let pid = self
                        .client_pid(e)
                        .filter(|p| *p != own && !is_satellite(*p));
                    self.background.pids.insert(e, pid);
                    pid
                }
            };
            let Some(pid) = pid else { continue };
            let app = windows.entry(pid).or_default();
            app.0.push(e);
            app.1 |= self.background.captured.contains_key(&e);
        }
        let mut apps: BTreeMap<u32, bool> = BTreeMap::new();
        let mut streams = None;
        for (&pid, (ws, captured)) in &windows {
            let mut facts = AppFacts {
                captured: *captured,
                game: game == Some(pid),
                ..AppFacts::default()
            };
            let mut in_use = self.core.app_in_use(ws, facts);
            // the sound server is asked only about an app nothing else keeps
            if !in_use && self.background.enabled {
                let running = streams.get_or_insert_with(|| self.background.streams.running(now));
                let windowed = |p| windows.contains_key(&p);
                facts.playing = Running::of(&running.playing, pid, windowed);
                facts.recording = Running::of(&running.recording, pid, windowed);
                in_use = self.core.app_in_use(ws, facts);
            }
            apps.insert(pid, in_use);
        }

        let plan = self.background.plan(now, &apps);
        let windowed: Vec<u32> = apps.keys().copied().collect();
        for pid in plan.restore {
            self.background.restore_app(pid, &windowed);
        }
        for pid in plan.lower {
            self.background.lower_app(pid, &windowed);
        }
    }

    /// Restores every lowered app (background priority turned off, or shutting down: dropping
    /// [`Background`] waits for the restores).
    pub fn background_restore_all(&mut self) {
        let windowed: Vec<u32> = self.background.lowered.iter().copied().collect();
        for &pid in &windowed {
            self.background.restore_app(pid, &windowed);
        }
        self.background.hidden_since.clear();
    }
}

/// Whether the process is xwayland-satellite (every X11 app's client).
fn is_satellite(pid: u32) -> bool {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n == SATELLITE))
        .unwrap_or(false)
}

/// `pid` and the processes it started, down to (not into) another app with windows.
fn tree(pid: u32, windowed: &[u32]) -> Vec<i32> {
    let mut out = vec![pid as i32];
    let mut i = 0;
    while i < out.len() {
        for child in crate::ownership::children_of(out[i]) {
            if !windowed.contains(&(child as u32)) && !out.contains(&child) {
                out.push(child);
            }
        }
        i += 1;
    }
    out
}

/// The threads of a process.
fn threads(pid: i32) -> Vec<i32> {
    std::fs::read_dir(format!("/proc/{pid}/task"))
        .map(|tasks| {
            tasks
                .flatten()
                .filter_map(|t| t.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The lowest nice a process may set itself back to (`RLIMIT_NICE`: 20 - its limit).
fn lowest_nice(pid: i32) -> i32 {
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).unwrap_or_default();
    let soft = limits
        .lines()
        .find_map(|l| l.strip_prefix("Max nice priority"))
        .and_then(|rest| rest.split_whitespace().next().map(str::to_owned));
    match soft.as_deref() {
        Some("unlimited") => -20,
        Some(n) => 20 - n.parse::<i32>().unwrap_or(0).clamp(0, 40),
        None => 20,
    }
}

/// Lowers `pid`'s tree to [`BACKGROUND_NICE`]: the threads below it that may be raised back.
fn lower(pid: u32, windowed: &[u32]) -> Lowered {
    let mut lowered = Lowered::default();
    for process in tree(pid, windowed) {
        let floor = lowest_nice(process);
        for tid in threads(process) {
            let Some(id) = Pid::from_raw(tid) else {
                continue;
            };
            let Ok(nice) = getpriority_process(Some(id)) else {
                continue;
            };
            if nice >= BACKGROUND_NICE {
                lowered.kept.insert(tid);
            } else if nice >= floor && setpriority_process(Some(id), BACKGROUND_NICE).is_ok() {
                lowered.threads.insert(tid, nice);
            }
        }
    }
    lowered
}

/// Raises `pid`'s tree back: each thread still at [`BACKGROUND_NICE`] to its nice before, a
/// thread started since to the nice its app had.
fn restore(pid: u32, lowered: &Lowered, windowed: &[u32]) {
    // a thread started since takes its app's own level back: the root's before, or its own if the
    // root was left alone
    let root = pid as i32;
    let default = match lowered.threads.get(&root) {
        Some(&n) => n,
        None if lowered.kept.contains(&root) => BACKGROUND_NICE,
        None => 0,
    };
    for process in tree(pid, windowed) {
        for tid in threads(process) {
            let Some(id) = Pid::from_raw(tid) else {
                continue;
            };
            if !lowered.kept.contains(&tid) && getpriority_process(Some(id)) == Ok(BACKGROUND_NICE)
            {
                let before = lowered.threads.get(&tid).copied().unwrap_or(default);
                let _ = setpriority_process(Some(id), before);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps(list: &[(u32, bool)]) -> BTreeMap<u32, bool> {
        list.iter().copied().collect()
    }

    #[test]
    fn an_app_out_of_sight_is_lowered_after_the_delay_and_restored_in_sight() {
        let mut bg = Background::default();
        let t0 = Instant::now();
        assert_eq!(
            bg.plan(t0, &apps(&[(10, false), (11, true)])),
            Plan::default()
        );
        assert_eq!(bg.due(t0), Some(HIDE_AFTER));
        let later = t0 + HIDE_AFTER;
        assert_eq!(
            bg.plan(later, &apps(&[(10, false), (11, true)])).lower,
            vec![10]
        );
        bg.lowered.insert(10);
        // lowered once, not again
        assert_eq!(bg.plan(later, &apps(&[(10, false)])), Plan::default());
        assert_eq!(bg.due(later), None);
        assert_eq!(bg.plan(later, &apps(&[(10, true)])).restore, vec![10]);
    }

    #[test]
    fn a_glance_restarts_the_delay() {
        let mut bg = Background::default();
        let t0 = Instant::now();
        bg.plan(t0, &apps(&[(10, false)]));
        bg.plan(t0 + HIDE_AFTER / 2, &apps(&[(10, true)]));
        let t1 = t0 + HIDE_AFTER;
        assert_eq!(bg.plan(t1, &apps(&[(10, false)])), Plan::default());
        assert_eq!(
            bg.plan(t1 + HIDE_AFTER, &apps(&[(10, false)])).lower,
            vec![10]
        );
    }

    #[test]
    fn an_app_without_windows_and_a_disabled_setting_restore() {
        let mut bg = Background::default();
        let t0 = Instant::now();
        bg.lowered.insert(10);
        bg.lowered.insert(11);
        assert_eq!(bg.plan(t0, &apps(&[(11, false)])).restore, vec![10]);
        bg.lowered.remove(&10);
        bg.enabled = false;
        let plan = bg.plan(t0 + HIDE_AFTER * 2, &apps(&[(11, false), (12, false)]));
        assert_eq!(
            plan,
            Plan {
                lower: vec![],
                restore: vec![11]
            }
        );
        assert_eq!(bg.due(t0), None);
    }

    fn test_element() -> ElementId {
        let (mut core, ..) =
            infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
        core.toplevel_created()
    }

    #[test]
    fn game_mode_hysteresis_stays_on_briefly_and_ends_after_hold() {
        let mut bg = Background::default();
        let elem = test_element();
        let pid = 42;
        bg.game = Some((elem, pid));
        let t0 = Instant::now();

        // on, off for 3 s then back: stays on
        assert_eq!(bg.update_game(t0, false), None);
        assert_eq!(bg.game, Some((elem, pid)));
        assert_eq!(bg.update_game(t0 + Duration::from_secs(3), true), None);
        assert_eq!(bg.game, Some((elem, pid)));
        assert_eq!(bg.game_off_since, None);

        // off for 6 s: off once
        let t1 = t0 + Duration::from_secs(4);
        assert_eq!(bg.update_game(t1, false), None);
        assert_eq!(bg.game, Some((elem, pid)));
        assert_eq!(
            bg.update_game(t1 + Duration::from_secs(6), false),
            Some(pid)
        );
        assert_eq!(bg.game, None);
        assert_eq!(bg.game_off_since, None);
        // off once: subsequent updates return None
        assert_eq!(bg.update_game(t1 + Duration::from_secs(7), false), None);
    }

    #[test]
    fn game_mode_hold_wakes_the_loop_then_clears() {
        let mut bg = Background::default();
        let elem = test_element();
        let pid = 42;
        bg.game = Some((elem, pid));
        let t0 = Instant::now();
        assert_eq!(bg.due(t0), None);

        // Off screen: due in 5 s
        bg.update_game(t0, false);
        assert_eq!(bg.due(t0), Some(GAME_MODE_HOLD));
        assert_eq!(
            bg.due(t0 + Duration::from_secs(2)),
            Some(Duration::from_secs(3))
        );

        // Back on screen: due cleared
        bg.update_game(t0 + Duration::from_secs(2), true);
        assert_eq!(bg.due(t0 + Duration::from_secs(2)), None);

        // Off for 6 s: off once, due cleared
        let t1 = t0 + Duration::from_secs(3);
        bg.update_game(t1, false);
        assert_eq!(
            bg.update_game(t1 + Duration::from_secs(6), false),
            Some(pid)
        );
        assert_eq!(bg.due(t1 + Duration::from_secs(6)), None);
    }

    /// An app the user started with `nice` (already at or below the background level) is left
    /// alone when lowered and is not raised by the restore, even where the nice limit would allow
    /// it (2026-10-27: with gamemode's limit, restore raised such a tree to nice 0).
    #[test]
    fn an_app_started_with_nice_is_never_raised() {
        // exactly at the background level: the case a restore could take for its own lowering
        let own = getpriority_process(None).unwrap();
        if own > BACKGROUND_NICE {
            return; // this process runs lower already: no child can be placed at the level
        }
        let by = (BACKGROUND_NICE - own).to_string();
        let mut child = crate::ownership::spawn_owned(std::process::Command::new("nice").args([
            "-n",
            &by,
            "sh",
            "-c",
            "sleep 30 & wait",
        ]))
        .unwrap();
        let pid = child.id();
        let started = Instant::now();
        while tree(pid, &[]).len() < 2 && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let tids: Vec<i32> = tree(pid, &[]).into_iter().flat_map(threads).collect();
        let nice = |tid: i32| getpriority_process(Pid::from_raw(tid)).unwrap();
        let before: Vec<i32> = tids.iter().map(|t| nice(*t)).collect();
        assert!(
            before.iter().all(|n| *n == BACKGROUND_NICE),
            "started at the level: {before:?}"
        );
        let lowered = lower(pid, &[]);
        assert!(lowered.threads.is_empty(), "nothing to lower");
        restore(pid, &lowered, &[]);
        let after: Vec<i32> = tids.iter().map(|t| nice(*t)).collect();
        assert_eq!(after, before, "left where the user put it");
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn a_real_process_tree_is_lowered_and_restored_only_where_the_limit_allows() {
        // a shell with a child: both are lowered and come back (or, under a nice limit that
        // forbids raising back, neither is touched)
        // owned by this test: a session shut down by another test in this process leaves it alone
        let mut child = crate::ownership::spawn_owned(
            std::process::Command::new("sh").args(["-c", "sleep 30 & wait"]),
        )
        .unwrap();
        let pid = child.id();
        let started = Instant::now();
        while tree(pid, &[]).len() < 2 && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let tids: Vec<i32> = tree(pid, &[]).into_iter().flat_map(threads).collect();
        assert!(tids.len() >= 2, "the shell and its sleep: {tids:?}");
        let nice = |tid: i32| getpriority_process(Pid::from_raw(tid)).unwrap();
        let before: Vec<i32> = tids.iter().map(|t| nice(*t)).collect();
        let may_restore = lowest_nice(pid as i32) <= before[0];
        let lowered = lower(pid, &[]);
        for (tid, b) in tids.iter().zip(&before) {
            let want = if may_restore {
                BACKGROUND_NICE.max(*b)
            } else {
                *b
            };
            assert_eq!(nice(*tid), want, "thread {tid} lowered");
        }
        restore(pid, &lowered, &[]);
        for (tid, b) in tids.iter().zip(&before) {
            assert_eq!(nice(*tid), *b, "thread {tid} restored");
        }
        // another app's subtree is left alone
        assert_eq!(tree(pid, &[pid + 1_000_000]).len(), tids.len());
        let _ = child.kill();
        let _ = child.wait();
        crate::ownership::forget_spawned(pid);
    }

    /// Lowering and restoring an app of 300 processes keeps the main loop's turn under a
    /// millisecond (2026-11-23: 25 game clients of about 100 threads each cost turns of 11 to
    /// 14 ms, "Slow turn 14.2 ms: background priority 13.8 ms").
    #[test]
    fn a_large_tree_costs_the_turn_under_a_millisecond() {
        const BUDGET: Duration = Duration::from_millis(1);
        let mut child = crate::ownership::spawn_owned(
            std::process::Command::new("sh")
                .args(["-c", "for i in $(seq 300); do sleep 30 & done; wait"]),
        )
        .unwrap();
        let pid = child.id();
        let started = Instant::now();
        while tree(pid, &[]).len() < 301 && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let processes = tree(pid, &[]);
        let tids: Vec<i32> = processes.iter().copied().flat_map(threads).collect();
        let nice = |tid: i32| getpriority_process(Pid::from_raw(tid)).ok();
        let before: Vec<Option<i32>> = tids.iter().map(|t| nice(*t)).collect();
        let may_restore = before[0].is_some_and(|n| lowest_nice(pid as i32) <= n);
        let low: Vec<Option<i32>> = before
            .iter()
            .map(|b| {
                b.map(|b| {
                    if may_restore {
                        BACKGROUND_NICE.max(b)
                    } else {
                        b
                    }
                })
            })
            .collect();
        // the work may finish after the call returns: wait for it, up to 10 s
        let settled = |want: &[Option<i32>]| {
            let started = Instant::now();
            loop {
                let now: Vec<Option<i32>> = tids.iter().map(|t| nice(*t)).collect();
                if now == want || started.elapsed() > Duration::from_secs(10) {
                    return now == want;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        };

        let mut bg = Background::default();
        // the worker starts once per session: started here, so the turn timed is a turn's work
        // (2026-11-25: on a busy CI runner the thread's start alone took about 2 ms)
        bg.lower_app(u32::MAX, &[]);
        bg.restore_app(u32::MAX, &[]);
        let started = Instant::now();
        bg.lower_app(pid, &[]);
        let lowering = started.elapsed();
        let lowered = settled(&low);
        let started = Instant::now();
        bg.restore_app(pid, &[]);
        let restoring = started.elapsed();
        let restored = settled(&before);

        for p in processes.iter().rev() {
            if let Some(p) = Pid::from_raw(*p) {
                let _ = smithay::reexports::rustix::process::kill_process(
                    p,
                    smithay::reexports::rustix::process::Signal::KILL,
                );
            }
        }
        let _ = child.wait();
        crate::ownership::forget_spawned(pid);
        assert_eq!(processes.len(), 301, "the shell and its 300 sleeps");
        assert!(
            lowered && restored,
            "lowered {lowered}, restored {restored}"
        );
        assert!(
            lowering < BUDGET && restoring < BUDGET,
            "lowering took {lowering:?}, restoring {restoring:?}"
        );
    }

    #[test]
    fn the_nice_floor_follows_the_limit() {
        let own = getpid().as_raw_nonzero().get();
        let floor = lowest_nice(own);
        assert!((-20..=20).contains(&floor));
        assert_eq!(
            lowest_nice(i32::MAX),
            20,
            "an unknown process is never lowered"
        );
    }
}
