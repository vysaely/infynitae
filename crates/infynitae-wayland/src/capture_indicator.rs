//! The capture indicator: a red mark in the top right corner of every screen while a client
//! captures the screen or a window (screen sharing, a remote desktop, a screenshot), so a capture
//! is never silent. A client counts from each frame it asks for until `HOLD` after its last one.
//!
//! The replay buffer's own helper (`infynitae-record-stream`) captures all the time and is left
//! out: the mark would never go away. It is known by its executable's name, read when a client
//! starts capturing.
//! The mark is drawn on the screen only, never into a capture (`output_elements_screencopy`).

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use smithay::backend::renderer::element::Id;
use smithay::reexports::wayland_server::{Client, backend::ClientId};

use crate::WaylandState;

/// How long the mark stays after a client's last frame: a share at 1 frame per second keeps it.
pub const HOLD: Duration = Duration::from_secs(3);

/// The executable left out: the replay buffer's helper.
const RECORDER: &str = "infynitae-record-stream";

pub struct CaptureIndicator {
    /// Whether the mark is drawn at all (`CAPTURE_INDICATOR`, on by default).
    pub(crate) enabled: bool,
    seen: Seen<ClientId>,
    /// The dot's stable element id (an unchanged mark is no damage).
    pub(crate) ids: [Id; 1],
}

impl Default for CaptureIndicator {
    fn default() -> Self {
        Self {
            enabled: true,
            seen: Seen::default(),
            ids: [Id::new()],
        }
    }
}

impl CaptureIndicator {
    /// Whether the mark is drawn at `now`.
    pub fn active(&self, now: Instant) -> bool {
        self.enabled && self.seen.active(now)
    }

    /// How long until the drawn mark goes away by itself (the backend wakes then).
    pub fn due(&self, now: Instant) -> Option<Duration> {
        self.seen.due(now)
    }

    /// Records whether the mark is drawn now; true when that changed (the screens need a frame).
    pub fn update(&mut self, now: Instant) -> bool {
        self.seen.update(now, self.enabled)
    }
}

/// The capturing clients, by any key (a `ClientId`; a number in the tests).
struct Seen<K> {
    /// Each client's last frame request, and whether it is left out. A client quiet for `HOLD`
    /// is forgotten (a disconnected one too).
    last: HashMap<K, (bool, Instant)>,
    /// Whether the mark was drawn after the last update.
    shown: bool,
}

impl<K> Default for Seen<K> {
    fn default() -> Self {
        Self {
            last: HashMap::new(),
            shown: false,
        }
    }
}

impl<K: Hash + Eq> Seen<K> {
    /// `client` asked for a frame at `now`; `exempt` is asked only when the client is new.
    fn note(&mut self, client: K, now: Instant, exempt: impl FnOnce() -> bool) {
        self.last.entry(client).or_insert_with(|| (exempt(), now)).1 = now;
    }

    /// The times of the clients that show the mark.
    fn shown_by(&self) -> impl Iterator<Item = Instant> + '_ {
        self.last
            .values()
            .filter(|(exempt, _)| !exempt)
            .map(|(_, t)| *t)
    }

    fn active(&self, now: Instant) -> bool {
        self.shown_by()
            .any(|t| now.saturating_duration_since(t) < HOLD)
    }

    fn due(&self, now: Instant) -> Option<Duration> {
        if !self.shown {
            return None;
        }
        self.shown_by()
            .map(|t| HOLD.saturating_sub(now.saturating_duration_since(t)))
            .max()
    }

    fn update(&mut self, now: Instant, enabled: bool) -> bool {
        let active = enabled && self.active(now);
        self.last
            .retain(|_, (_, t)| now.saturating_duration_since(*t) < HOLD);
        std::mem::replace(&mut self.shown, active) != active
    }
}

/// Whether the process is the replay buffer's helper, by its executable's file name.
fn is_recorder(pid: i32) -> bool {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n == RECORDER))
        .unwrap_or(false)
}

impl WaylandState {
    /// A client asked for a capture frame: the mark shows (unless the client is left out).
    pub(crate) fn note_capture(&mut self, client: &Client) {
        let dh = self.dh.clone();
        self.capture_indicator
            .seen
            .note(client.id(), Instant::now(), || {
                client
                    .get_credentials(&dh)
                    .is_ok_and(|c| is_recorder(c.pid))
            });
    }

    /// The capture indicator (the backends ask it when to draw and when to wake).
    pub fn capture_indicator(&mut self) -> &mut CaptureIndicator {
        &mut self.capture_indicator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn a_capture_shows_the_mark_until_hold_after_its_last_frame() {
        let t0 = Instant::now();
        let mut s = Seen::default();
        assert!(!s.update(t0, true), "nothing captured: nothing changes");
        s.note(1, t0, || false);
        assert!(s.update(t0, true), "shown");
        assert_eq!(s.due(t0), Some(HOLD));
        s.note(1, at(t0, 1000), || panic!("asked once per client"));
        assert!(!s.update(at(t0, 3500), true), "kept by the second frame");
        assert_eq!(s.due(at(t0, 3500)), Some(Duration::from_millis(500)));
        assert!(
            s.update(at(t0, 4000), true),
            "gone HOLD after the last frame"
        );
        assert_eq!(s.due(at(t0, 4000)), None);
        assert!(s.last.is_empty(), "the quiet client is forgotten");
    }

    #[test]
    fn the_recorder_never_shows_it_and_is_asked_about_once_while_it_captures() {
        let t0 = Instant::now();
        let mut s = Seen::default();
        s.note(7, t0, || true);
        s.note(7, at(t0, 10), || panic!("asked once"));
        assert!(!s.update(at(t0, 10), true));
        s.note(7, at(t0, 2000), || panic!("still known while it captures"));
        assert!(!s.update(at(t0, 2000), true));
        // another client does
        s.note(8, at(t0, 2000), || false);
        assert!(s.update(at(t0, 2000), true));
    }

    #[test]
    fn the_longest_waiting_client_sets_when_it_goes() {
        let t0 = Instant::now();
        let mut s = Seen::default();
        s.note(1, t0, || false);
        s.note(2, at(t0, 2000), || false);
        s.update(at(t0, 2000), true);
        assert_eq!(s.due(at(t0, 2000)), Some(HOLD));
    }

    #[test]
    fn turned_off_it_is_never_shown() {
        let t0 = Instant::now();
        let mut s = Seen::default();
        s.note(1, t0, || false);
        assert!(!s.update(t0, false));
        assert_eq!(s.due(t0), None);
    }

    #[test]
    fn only_the_helper_by_name_is_left_out() {
        assert!(!is_recorder(std::process::id() as i32), "the test binary");
        assert!(!is_recorder(-1), "no such process");
    }
}
