//! The capture indicator: a red mark in the top right corner of every screen while a client
//! captures the screen or a window (screen sharing, a remote desktop, a screenshot), so a capture
//! is never silent. A client counts from each frame it asks for until `HOLD` after its last one.
//!
//! The replay buffer's own helper (`infynitae-record-stream`) captures all the time and is left
//! out: the mark would never go away. It is known by the socket it connected to, the recorder's
//! own that the compositor opened (`session::recorder_socket_path`), never by an executable's
//! name (M2, owner 2026-10-31).
//! The mark is drawn on the screen only, never into a capture (`output_elements_screencopy`).
//!
//! On a screen showing a fullscreen window the mark steps aside (owner decision 2026-10-26):
//! anything over the window would keep a game from going to the screen directly (tearing, game
//! mode). A notification says the screen is being captured instead, once per capture.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use smithay::backend::renderer::element::Id;
use smithay::reexports::wayland_server::{Client, backend::ClientId};

use crate::WaylandState;

/// How long the mark stays after a client's last frame: a share at 1 frame per second keeps it.
pub const HOLD: Duration = Duration::from_secs(3);

pub struct CaptureIndicator {
    /// Whether the mark is drawn at all (`CAPTURE_INDICATOR`, on by default).
    pub(crate) enabled: bool,
    seen: Seen<ClientId>,
    /// The dot's stable element id (an unchanged mark is no damage).
    pub(crate) ids: [Id; 1],
    /// This capture was announced by a notification (the mark was hidden by a fullscreen window).
    noticed: bool,
}

impl Default for CaptureIndicator {
    fn default() -> Self {
        Self {
            enabled: true,
            seen: Seen::default(),
            ids: [Id::new()],
            noticed: false,
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

impl WaylandState {
    /// A client asked for a capture frame: the mark shows (unless the client is left out).
    pub(crate) fn note_capture(&mut self, client: &Client) {
        self.capture_indicator
            .seen
            .note(client.id(), Instant::now(), || {
                client
                    .get_data::<crate::ClientState>()
                    .is_some_and(|s| s.is_recorder())
            });
    }

    /// The capture indicator (the backends ask it when to draw and when to wake).
    pub fn capture_indicator(&mut self) -> &mut CaptureIndicator {
        &mut self.capture_indicator
    }

    /// Whether the mark steps aside on `output`: it shows a fullscreen window (unlocked).
    pub fn capture_mark_hidden(&self, output: infynitae_core::output::OutputId) -> bool {
        !self.is_locked()
            && self.core.presentation(output).is_some()
            && !self.core.is_presentation_suppressed(output)
    }

    /// Whether a capture whose mark a fullscreen window hides is to be announced now: once per
    /// capture ([`announce_hidden_capture`](Self::announce_hidden_capture) does it).
    pub fn capture_notice(&mut self, now: Instant) -> bool {
        if !self.capture_indicator.active(now) {
            self.capture_indicator.noticed = false;
            return false;
        }
        let hidden = self
            .core
            .outputs()
            .iter()
            .any(|o| self.capture_mark_hidden(o.id));
        if !hidden || self.capture_indicator.noticed {
            return false;
        }
        self.capture_indicator.noticed = true;
        true
    }

    /// The notification that the screen is being captured, its mark hidden by a fullscreen
    /// window (`launch.sh notify-capture-hidden`).
    pub fn announce_hidden_capture(&mut self) {
        println!("[infynitae] A capture runs under a fullscreen window: announced");
        self.spawn_action("notify-capture-hidden");
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

    /// Only a client of the recorder's own socket is left out; any other client shows the mark,
    /// whatever its executable is called (M2).
    #[test]
    fn only_the_recorder_socket_is_left_out() {
        use smithay::reexports::wayland_server::Display;
        use std::sync::Arc;
        let (core, _input, _shell, _token) =
            infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
        let display: Display<WaylandState> = Display::new().unwrap();
        let mut state = WaylandState::new(&display.handle(), core);
        let insert = |data: crate::ClientState| {
            let (server, client) = std::os::unix::net::UnixStream::pair().unwrap();
            let c = display
                .handle()
                .insert_client(server, Arc::new(data))
                .unwrap();
            (c, client)
        };
        let (recorder, _r) = insert(crate::ClientState::recorder());
        let (named, _n) = insert(crate::ClientState::with_identity(
            crate::trust::Identity::Native {
                exe: None,
                path: Some("/tmp/infynitae-record-stream".into()),
            },
        ));
        let now = Instant::now();
        state.note_capture(&recorder);
        assert!(
            !state.capture_indicator.active(now),
            "the recorder is left out"
        );
        state.note_capture(&named);
        assert!(
            state.capture_indicator.active(now),
            "a name exempts nothing"
        );
    }
}
