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
use crate::log_line;

/// How long the mark stays after a client's last frame: a share at 1 frame per second keeps it.
pub const HOLD: Duration = Duration::from_secs(3);

/// Where a capture is shown (`CAPTURE_INDICATOR`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    /// The red dot in the screen's corner (`dot`; `true` from before 2026-11-12).
    Dot,
    /// No dot: the edge says it, in its background apps module, from `$XDG_RUNTIME_DIR/infynitae/capturing`
    /// (the default; owner 2026-11-12: "its better to put it in tray or module"). The dot was one more element
    /// over a fullscreen game, which kept it from going to the screen directly.
    Edge,
    /// Nowhere (`false`).
    Off,
}

impl CaptureMode {
    pub fn parse(v: &str) -> CaptureMode {
        match v.to_ascii_lowercase().as_str() {
            "dot" | "true" | "on" | "yes" | "1" => CaptureMode::Dot,
            "false" | "off" | "no" | "0" | "none" => CaptureMode::Off,
            _ => CaptureMode::Edge,
        }
    }
}

pub struct CaptureIndicator {
    pub(crate) mode: CaptureMode,
    seen: Seen<ClientId>,
    /// Each capturing client's program, by name, for the edge (`capturing`).
    names: HashMap<ClientId, String>,
    /// What `capturing` holds now (nothing written while it is unchanged).
    written: Vec<String>,
    /// Where the edge reads it (`$XDG_RUNTIME_DIR/infynitae/capturing`): set by a real session only, so a
    /// test never writes into the desktop's runtime folder.
    file: Option<std::path::PathBuf>,
    /// The dot's stable element id (an unchanged mark is no damage).
    pub(crate) ids: [Id; 1],
    /// This capture was announced by a notification (the mark was hidden by a fullscreen window).
    noticed: bool,
}

impl Default for CaptureIndicator {
    fn default() -> Self {
        Self {
            mode: CaptureMode::Edge,
            seen: Seen::default(),
            names: HashMap::new(),
            written: Vec::new(),
            file: None,
            ids: [Id::new()],
            noticed: false,
        }
    }
}

impl CaptureIndicator {
    /// `CAPTURE_INDICATOR` set by hand (tests).
    pub fn set_mode(&mut self, mode: CaptureMode) {
        self.mode = mode;
    }

    /// Where the edge reads what captures; a session sets it at its start.
    pub fn set_file(&mut self, file: std::path::PathBuf) {
        let _ = std::fs::remove_file(&file);
        self.file = Some(file);
    }

    /// The programs named as capturing now, for the edge (`capturing`).
    pub fn written(&self) -> &[String] {
        &self.written
    }

    /// Whether the mark is drawn at `now` (only as a dot).
    pub fn active(&self, now: Instant) -> bool {
        self.mode == CaptureMode::Dot && self.seen.active(now)
    }

    /// How long until the drawn mark goes away by itself (the backend wakes then).
    pub fn due(&self, now: Instant) -> Option<Duration> {
        self.seen.due(now)
    }

    /// Records whether a capture is shown now; true when that changed (the screens need a frame for the
    /// dot). Shown by the edge, the capturing programs' names go to `capturing` when they change.
    pub fn update(&mut self, now: Instant) -> bool {
        let changed = self.seen.update(now, self.mode != CaptureMode::Off);
        self.names.retain(|k, _| self.seen.last.contains_key(k));
        let mut now_names: Vec<String> = if self.mode == CaptureMode::Edge {
            self.seen
                .shown_keys(now)
                .filter_map(|k| self.names.get(k).cloned())
                .collect()
        } else {
            Vec::new()
        };
        now_names.sort();
        now_names.dedup();
        if now_names != self.written {
            if let Some(file) = &self.file {
                write_capturing(file, &now_names);
            }
            self.written = now_names;
        }
        changed
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

    /// The clients that show a capture now.
    fn shown_keys(&self, now: Instant) -> impl Iterator<Item = &K> + '_ {
        self.last
            .iter()
            .filter(move |(_, (exempt, t))| !exempt && now.saturating_duration_since(*t) < HOLD)
            .map(|(k, _)| k)
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

/// `capturing`: one capturing program per line while any captures, gone after.
fn write_capturing(path: &std::path::Path, names: &[String]) {
    if names.is_empty() {
        let _ = std::fs::remove_file(path);
    } else {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, names.join("\n") + "\n").is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

impl WaylandState {
    /// A client asked for a capture frame: the mark shows (unless the client is left out).
    pub(crate) fn note_capture(&mut self, client: &Client) {
        if !self.capture_indicator.names.contains_key(&client.id()) {
            let name = client
                .get_data::<crate::ClientState>()
                .and_then(|s| s.identity().map(|i| i.app_name()))
                .unwrap_or_else(|| "An app".to_string());
            self.capture_indicator.names.insert(client.id(), name);
        }
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
        // not for a window that merely fills the screen: a promise of docs/SECURITY.md
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
        log_line!("[infynitae] A capture runs under a fullscreen window: announced");
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
    fn the_settings_words_pick_the_place() {
        assert_eq!(CaptureMode::parse("edge"), CaptureMode::Edge);
        assert_eq!(CaptureMode::parse("dot"), CaptureMode::Dot);
        assert_eq!(
            CaptureMode::parse("true"),
            CaptureMode::Dot,
            "true from before"
        );
        assert_eq!(CaptureMode::parse("false"), CaptureMode::Off);
        assert_eq!(CaptureMode::parse(""), CaptureMode::Edge, "the default");
    }

    /// Shown by the edge: no dot is drawn, the capturing program is named while it captures and forgotten
    /// HOLD after its last frame.
    #[test]
    fn shown_by_the_edge_it_names_the_capturing_program_and_draws_no_dot() {
        let t0 = Instant::now();
        let mut s = Seen::default();
        s.note(1, t0, || false);
        s.note(2, t0, || true);
        assert!(s.update(t0, true));
        let shown: Vec<_> = s.shown_keys(t0).copied().collect();
        assert_eq!(shown, vec![1], "the recorder is left out");
        assert_eq!(s.shown_keys(at(t0, 3000)).count(), 0, "gone HOLD after");
        let ind = CaptureIndicator::default();
        assert_eq!(ind.mode, CaptureMode::Edge);
        assert!(!ind.active(t0), "no dot");
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
        state.capture_indicator.mode = CaptureMode::Dot;
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
