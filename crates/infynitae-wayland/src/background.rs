//! Apps out of sight yield the CPU; a fullscreen game gets game mode.
//!
//! Background priority (`BACKGROUND_PRIORITY`, on by default): an app none of whose windows any
//! screen has shown for [`HIDE_AFTER`] runs at nice [`BACKGROUND_NICE`], every thread of it and
//! of the processes it started (a browser's tabs), and back at its own nice as soon as one of its
//! windows is in sight or it has no window left. Nice only matters when the CPU is busy: an app
//! out of sight still runs, it just goes after the ones you look at. Never lowered: an app with
//! a window being shared (captured in the last [`CAPTURE_HOLD`]), the game, and X11 apps (one
//! xwayland-satellite serves them all). A thread is lowered only when its process may raise it
//! back (`RLIMIT_NICE`, which the installer sets so apps return to nice 0 and never above it);
//! without that limit nothing is lowered.
//!
//! Game mode (`GAME_MODE`, on by default): a fullscreen window the GPU shows directly (direct
//! scanout, [`WaylandState::frame_scanned_out`]) is a game. Its app is registered with Feral's
//! gamemode, when installed (`launch.sh game-mode`), until the window is no longer fullscreen.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use infynitae_core::output::OutputId;
use infynitae_core::stage::ElementId;
use smithay::reexports::rustix::process::{Pid, getpid, getpriority_process, setpriority_process};

use crate::WaylandState;

/// How long an app's windows are all out of sight before it is lowered.
pub const HIDE_AFTER: Duration = Duration::from_secs(15);

/// The nice value of an app out of sight.
pub const BACKGROUND_NICE: i32 = 10;

/// A shared window counts as in sight until this long after its last captured frame.
pub const CAPTURE_HOLD: Duration = Duration::from_secs(5);

/// The executable whose clients are X11 apps, all through one process.
const SATELLITE: &str = "xwayland-satellite";

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
    /// The apps lowered now: each thread's nice before.
    lowered: HashMap<u32, Lowered>,
    /// The last captured frame of each window shared alone.
    captured: HashMap<ElementId, Instant>,
    /// The game: its window and app.
    game: Option<(ElementId, u32)>,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            enabled: true,
            game_mode: true,
            pids: HashMap::new(),
            hidden_since: HashMap::new(),
            lowered: HashMap::new(),
            captured: HashMap::new(),
            game: None,
        }
    }
}

/// A lowered app: the nice of each thread it lowered, by thread id.
#[derive(Default)]
struct Lowered {
    threads: HashMap<i32, i32>,
}

/// What [`Background::plan`] decided.
#[derive(Debug, Default, PartialEq, Eq)]
struct Plan {
    lower: Vec<u32>,
    restore: Vec<u32>,
}

impl Background {
    /// The decision, from each windowed app and whether it is in sight now (a window shown,
    /// shared or the game's): the apps to lower and to restore. Pure but for its timers.
    fn plan(&mut self, now: Instant, apps: &BTreeMap<u32, bool>) -> Plan {
        let mut plan = Plan::default();
        self.hidden_since
            .retain(|pid, _| apps.get(pid) == Some(&false));
        for (&pid, &in_sight) in apps {
            if in_sight || !self.enabled {
                if self.lowered.contains_key(&pid) {
                    plan.restore.push(pid);
                }
                continue;
            }
            let since = *self.hidden_since.entry(pid).or_insert(now);
            if now.duration_since(since) >= HIDE_AFTER && !self.lowered.contains_key(&pid) {
                plan.lower.push(pid);
            }
        }
        // an app whose last window closed (one that lives on in the tray)
        plan.restore.extend(
            self.lowered
                .keys()
                .filter(|pid| !apps.contains_key(pid))
                .copied(),
        );
        plan.restore.sort_unstable();
        plan
    }

    /// How long until an app out of sight is due to be lowered, or a share's hold ends (the
    /// backend wakes then); `None` when nothing waits.
    pub fn due(&self, now: Instant) -> Option<Duration> {
        if !self.enabled {
            return None;
        }
        let lowering = self
            .hidden_since
            .iter()
            .filter(|(pid, _)| !self.lowered.contains_key(pid))
            .map(|(_, since)| *since + HIDE_AFTER);
        let holds = self.captured.values().map(|t| *t + CAPTURE_HOLD);
        lowering
            .chain(holds)
            .min()
            .map(|t| t.saturating_duration_since(now))
    }
}

impl WaylandState {
    /// The background priority and game mode state.
    pub fn background(&self) -> &Background {
        &self.background
    }

    /// A frame of `output` was shown: `scanout` when it was a client's buffer on the screen as
    /// is (direct scanout). The fullscreen window shown that way is the game.
    pub fn frame_scanned_out(&mut self, output: OutputId, scanout: bool) {
        if !scanout || !self.background.game_mode || self.background.game.is_some() {
            return;
        }
        let Some(window) = self.core.presentation(output) else {
            return;
        };
        let Some(pid) = self.client_pid(window) else {
            return;
        };
        println!("[infynitae] Game mode on for process {pid}");
        self.background.game = Some((window, pid));
        self.gamemode(true, pid);
    }

    /// Registers the game with gamemode or ends it (`launch.sh game-mode`); never for this
    /// process (a client in the compositor's own process, as in the test suite).
    fn gamemode(&mut self, on: bool, pid: u32) {
        if pid != std::process::id() {
            let state = if on { "on" } else { "off" };
            self.spawn_action_with("game-mode", &format!("{state} {pid}"));
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
    /// is no longer fullscreen. Cheap: the backends call it on every turn of their loop.
    pub fn background_update(&mut self, now: Instant) {
        if let Some((window, pid)) = self.background.game {
            let still = self.background.game_mode
                && self
                    .core
                    .outputs()
                    .iter()
                    .any(|o| self.core.presentation(o.id) == Some(window));
            if !still {
                println!("[infynitae] Game mode off for process {pid}");
                self.background.game = None;
                self.gamemode(false, pid);
            }
        }
        let bg = &self.background;
        if !bg.enabled && bg.lowered.is_empty() && bg.hidden_since.is_empty() {
            return;
        }

        let mapped = self.core.stage().mapped_elements();
        self.background.pids.retain(|e, _| mapped.contains(e));
        self.background
            .captured
            .retain(|e, t| mapped.contains(e) && now.duration_since(*t) < CAPTURE_HOLD);
        let game = self.background.game.map(|(_, pid)| pid);
        let own = getpid().as_raw_nonzero().get() as u32;
        let mut apps: BTreeMap<u32, bool> = BTreeMap::new();
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
            let in_sight = self.core.membership().is_observed(e)
                || self.background.captured.contains_key(&e)
                || game == Some(pid);
            *apps.entry(pid).or_insert(false) |= in_sight;
        }

        let plan = self.background.plan(now, &apps);
        let windowed: Vec<u32> = apps.keys().copied().collect();
        for pid in plan.restore {
            if let Some(lowered) = self.background.lowered.remove(&pid) {
                restore(pid, &lowered, &windowed);
            }
        }
        for pid in plan.lower {
            self.background.lowered.insert(pid, lower(pid, &windowed));
        }
    }

    /// Restores every lowered app (background priority turned off, or shutting down).
    pub fn background_restore_all(&mut self) {
        let windowed: Vec<u32> = self.background.lowered.keys().copied().collect();
        for (pid, lowered) in std::mem::take(&mut self.background.lowered) {
            restore(pid, &lowered, &windowed);
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
            if nice < BACKGROUND_NICE
                && nice >= floor
                && setpriority_process(Some(id), BACKGROUND_NICE).is_ok()
            {
                lowered.threads.insert(tid, nice);
            }
        }
    }
    lowered
}

/// Raises `pid`'s tree back: each thread still at [`BACKGROUND_NICE`] to its nice before, a
/// thread started since to the nice its app had.
fn restore(pid: u32, lowered: &Lowered, windowed: &[u32]) {
    let default = lowered.threads.get(&(pid as i32)).copied().unwrap_or(0);
    for process in tree(pid, windowed) {
        for tid in threads(process) {
            let Some(id) = Pid::from_raw(tid) else {
                continue;
            };
            if getpriority_process(Some(id)) == Ok(BACKGROUND_NICE) {
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
        bg.lowered.insert(10, Lowered::default());
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
        bg.lowered.insert(10, Lowered::default());
        bg.lowered.insert(11, Lowered::default());
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
