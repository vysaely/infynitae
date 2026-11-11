//! `zwp_idle_inhibit_manager_v1`: a video player, a browser playing video, a game, keeps the
//! screens on and the session unlocked while it shows. An inhibitor counts while its surface is
//! part of a mapped window and the session is not locked: locking lets the screens go dark as
//! always, whatever an app asks.
//!
//! Apps that ask over D-Bus instead (org.freedesktop.ScreenSaver: X11 apps, VLC, Steam) and
//! `systemd-inhibit --what=idle` reach the backend through logind (`LogindInhibitWatcher`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use smithay::reexports::calloop::channel::{Channel, Sender, channel};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::idle_inhibit::{IdleInhibitHandler, IdleInhibitManagerState};

use crate::WaylandState;

pub struct IdleInhibit {
    #[expect(
        dead_code,
        reason = "owns the zwp_idle_inhibit_manager_v1 global for the state's lifetime"
    )]
    manager: IdleInhibitManagerState,
    /// The surfaces with an inhibitor, as clients made them (dead ones dropped when read).
    surfaces: Vec<WlSurface>,
}

impl IdleInhibit {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            manager: IdleInhibitManagerState::new::<WaylandState>(dh),
            surfaces: Vec::new(),
        }
    }
}

impl IdleInhibitHandler for WaylandState {
    fn inhibit(&mut self, surface: WlSurface) {
        self.idle_inhibit.surfaces.retain(|s| s.is_alive());
        if !self.idle_inhibit.surfaces.contains(&surface) {
            self.idle_inhibit.surfaces.push(surface);
        }
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibit.surfaces.retain(|s| *s != surface);
    }
}

impl WaylandState {
    /// Whether an app's inhibitor keeps the screens on now: its surface alive, in a mapped
    /// window, and the session unlocked.
    pub fn app_inhibits_idle(&self) -> bool {
        !self.is_locked()
            && self.idle_inhibit.surfaces.iter().any(|s| {
                s.is_alive()
                    && self
                        .registry
                        .key(s)
                        .is_some_and(|k| self.core.stage().element_containing_surface(k).is_some())
            })
    }
}

smithay::delegate_idle_inhibit!(WaylandState);

/// How often logind may be asked about idle inhibitors.
pub const LOGIND_QUERY_INTERVAL: Duration = Duration::from_secs(45);

/// Asynchronously queries logind for idle inhibitors off the main loop.
pub struct LogindInhibitWatcher {
    sender: Sender<bool>,
    in_flight: Arc<AtomicBool>,
    last_known: bool,
    last_queried: Option<Instant>,
}

impl LogindInhibitWatcher {
    /// Creates a new watcher and its calloop channel.
    pub fn new() -> (Self, Channel<bool>) {
        let (sender, channel) = channel();
        let watcher = Self {
            sender,
            in_flight: Arc::new(AtomicBool::new(false)),
            last_known: false,
            last_queried: None,
        };
        (watcher, channel)
    }

    /// The last known answer from logind. Pure, runs no process on the calling thread.
    pub fn is_inhibited(&self) -> bool {
        self.last_known
    }

    /// Records an answer delivered through the channel.
    pub fn update(&mut self, inhibited: bool) {
        self.last_known = inhibited;
    }

    /// Starts a query in a background thread if none is in flight and [`LOGIND_QUERY_INTERVAL`]
    /// has passed since the last query.
    pub fn query_async(&mut self, now: Instant) {
        if self
            .last_queried
            .is_some_and(|t| now.duration_since(t) < LOGIND_QUERY_INTERVAL)
        {
            return;
        }
        self.force_query();
        self.last_queried = Some(now);
    }

    /// Starts a query in a background thread if none is in flight, without checking interval.
    pub fn force_query(&self) {
        if self
            .in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let sender = self.sender.clone();
            let in_flight = self.in_flight.clone();
            std::thread::spawn(move || {
                let inhibited = logind_idle_inhibited();
                in_flight.store(false, Ordering::SeqCst);
                let _ = sender.send(inhibited);
            });
        }
    }
}

/// Whether logind holds an idle inhibitor (`BlockInhibited` names "idle"): an app that asked over
/// D-Bus (org.freedesktop.ScreenSaver, through infynitae-portal), or `systemd-inhibit
/// --what=idle`. Asked through busctl with a timeout; asked in a background thread off the main
/// loop.
pub fn logind_idle_inhibited() -> bool {
    std::process::Command::new("busctl")
        .args([
            "--timeout=1",
            "get-property",
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "BlockInhibited",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| block_inhibited_names_idle(&String::from_utf8_lossy(&o.stdout)))
}

/// busctl's `s "sleep:idle:handle-lid-switch"`: whether "idle" is one of the names. Pure.
fn block_inhibited_names_idle(reply: &str) -> bool {
    reply
        .trim()
        .trim_start_matches('s')
        .trim()
        .trim_matches('"')
        .split(':')
        .any(|w| w == "idle")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_found_among_the_inhibited_names() {
        assert!(block_inhibited_names_idle("s \"sleep:idle\"\n"));
        assert!(block_inhibited_names_idle("s \"idle\""));
        assert!(!block_inhibited_names_idle("s \"sleep:handle-lid-switch\""));
        assert!(!block_inhibited_names_idle("s \"\""));
        assert!(
            !block_inhibited_names_idle("s \"idler\""),
            "the whole name only"
        );
    }

    #[test]
    fn idle_inhibit_decision_runs_no_process_on_calling_thread() {
        let (mut watcher, _channel) = LogindInhibitWatcher::new();
        // The decision path is a pure in-memory read: no Command::new, no child process
        let t0 = Instant::now();
        assert!(!watcher.is_inhibited());
        watcher.update(true);
        assert!(watcher.is_inhibited());
        watcher.update(false);
        assert!(!watcher.is_inhibited());
        assert!(
            t0.elapsed() < Duration::from_millis(5),
            "in-memory check takes microseconds"
        );
    }

    #[test]
    fn logind_inhibit_query_runs_in_background_and_delivers_to_channel() {
        let (watcher, rx) = LogindInhibitWatcher::new();
        assert!(!watcher.is_inhibited());
        watcher.force_query();
        let started = Instant::now();
        let mut received = None;
        while started.elapsed() < Duration::from_secs(3) {
            if let Ok(val) = rx.try_recv() {
                received = Some(val);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(received.is_some(), "received result from background thread");
    }
}
