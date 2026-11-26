//! Which apps keep full CPU priority while none of their windows is in sight (the adapter's
//! background priority, `BACKGROUND_PRIORITY`). An app that interacts with the person keeps it
//! (owner, 2026-11-22: "an app that interacts with us should have priority"): a window of it on a
//! screen, a key or click into one in the last [`USED_HOLD_US`], its app id in `BACKGROUND_KEEP`,
//! or what only the adapter can see of its processes ([`AppFacts`]). Core decides; the adapter
//! keeps the timers and sets the nice values.

use crate::Core;
use crate::stage::ElementId;

/// A key press or click into a window keeps its app at full priority this long after it, in
/// microseconds of the input clock (`InputClock`): that clock moves only as input arrives, so
/// while nobody touches the PC the five minutes do not run out.
pub const USED_HOLD_US: u64 = 5 * 60 * 1_000_000;

/// What the adapter knows of one app's processes (the process with the windows and those it
/// started) that Core cannot see.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppFacts {
    /// A window of it was captured lately (shared on its own).
    pub captured: bool,
    /// It is the game (game mode).
    pub game: bool,
    /// One of its processes has a sound stream playing.
    pub playing: bool,
    /// One of its processes is recording (a microphone stream running).
    pub recording: bool,
}

impl Core {
    /// Whether the app with `windows` keeps full priority now.
    pub fn app_in_use(&self, windows: &[ElementId], facts: AppFacts) -> bool {
        let now = self.input.clock.now_us();
        facts.captured
            || facts.game
            || facts.playing
            || facts.recording
            || windows.iter().any(|&w| {
                self.membership.is_observed(w)
                    || self.background_kept(w) == Some(true)
                    || self
                        .last_used
                        .get(&w)
                        .is_some_and(|&t| now.saturating_sub(t) < USED_HOLD_US)
            })
    }

    /// The app ids kept at full priority however long they are out of sight (`BACKGROUND_KEEP`,
    /// compared ignoring case).
    pub fn set_background_keep(&mut self, app_ids: &[&str]) {
        self.background_keep = app_ids
            .iter()
            .map(|n| n.trim().to_lowercase())
            .filter(|n| !n.is_empty())
            .collect();
    }

    /// Whether the window's app is in `BACKGROUND_KEEP`; `None` for a window without an app id
    /// (nothing to keep it by).
    pub fn background_kept(&self, id: ElementId) -> Option<bool> {
        let app = self
            .stage
            .meta(id)?
            .app_id
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty())?;
        Some(
            self.background_keep
                .iter()
                .any(|k| app.eq_ignore_ascii_case(k)),
        )
    }

    /// Adds `id`'s app to `BACKGROUND_KEEP` or takes it out, until the config is read again
    /// (the adapter writes it there); false for a window without an app id.
    pub(super) fn keep_in_background(&mut self, id: ElementId, on: bool) -> bool {
        let Some(app) = self
            .stage
            .meta(id)
            .and_then(|m| m.app_id.as_deref())
            .map(|a| a.trim().to_lowercase())
            .filter(|a| !a.is_empty())
        else {
            return false;
        };
        self.background_keep.retain(|k| *k != app);
        if on {
            self.background_keep.push(app);
        }
        true
    }

    /// A key press or click went into `id`.
    pub(super) fn note_used(&mut self, id: ElementId) {
        self.last_used.insert(id, self.input.clock.now_us());
    }
}

#[cfg(test)]
mod tests {
    use super::{AppFacts, USED_HOLD_US};
    use crate::Core;
    use crate::entry::tests::*;
    use crate::input::keys::KEY_A;
    use crate::navigation::PersistedCameras;

    fn key_at(c: &mut Core, i: &mut crate::InputPort, time_us: u64) {
        for pressed in [true, false] {
            c.physical_input_vec(
                i,
                crate::PhysicalEvent::KeyboardKey {
                    key: KEY_A,
                    pressed,
                    modifiers: Default::default(),
                    mods_changed: false,
                    time_us,
                },
            )
            .unwrap();
        }
    }

    /// A window off every screen: its app is in use only by a rule below.
    fn hidden_window(c: &mut Core, t: &mut crate::OutputLifecycleToken) -> crate::stage::ElementId {
        let (a, _) = c.output_connected(t, desc("A")).unwrap();
        let w = map_windows(c, a, 1)[0];
        let mut fx = Vec::new();
        c.place_by_user(
            w,
            crate::geom::CanvasPoint {
                x: 50_000,
                y: 50_000,
            },
            &mut fx,
        )
        .unwrap();
        c.render_list(a).unwrap();
        w
    }

    #[test]
    fn a_window_in_sight_keeps_its_app() {
        let (mut c, _i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let w = map_windows(&mut c, a, 1)[0];
        assert!(c.membership.is_observed(w), "mapped in the view");
        assert!(c.app_in_use(&[w], AppFacts::default()));
    }

    #[test]
    fn an_app_out_of_sight_and_untouched_is_not_in_use() {
        let (mut c, _i, _, mut t) = Core::new(PersistedCameras::default());
        let w = hidden_window(&mut c, &mut t);
        assert!(!c.membership.is_observed(w));
        assert!(!c.app_in_use(&[w], AppFacts::default()));
    }

    /// Each fact the adapter gives keeps the app on its own.
    #[test]
    fn sound_recording_sharing_and_the_game_keep_an_app() {
        let (mut c, _i, _, mut t) = Core::new(PersistedCameras::default());
        let w = hidden_window(&mut c, &mut t);
        let none = AppFacts::default();
        for facts in [
            AppFacts {
                playing: true,
                ..none
            },
            AppFacts {
                recording: true,
                ..none
            },
            AppFacts {
                captured: true,
                ..none
            },
            AppFacts { game: true, ..none },
        ] {
            assert!(c.app_in_use(&[w], facts), "{facts:?}");
        }
    }

    /// A key into the focused window keeps its app for five minutes of input, then not; a key
    /// the compositor takes (a shortcut) is not a use.
    #[test]
    fn a_key_into_a_window_keeps_its_app_for_five_minutes() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let w = hidden_window(&mut c, &mut t);
        c.set_focus(&mut i, Some(w)).unwrap();
        key_at(&mut c, &mut i, 1_000_000);
        assert!(c.app_in_use(&[w], AppFacts::default()), "just typed into");

        // other input moves the clock on: 4 min 59 s later still, 5 min later no more
        let motion = |c: &mut Core, i: &mut crate::InputPort, time_us: u64| {
            c.physical_input_vec(
                i,
                crate::PhysicalEvent::PointerMotion {
                    delta: (1.0, 0.0),
                    delta_unaccel: (1.0, 0.0),
                    time_us,
                },
            )
            .unwrap();
        };
        motion(&mut c, &mut i, 1_000_000 + USED_HOLD_US - 1_000_000);
        assert!(c.app_in_use(&[w], AppFacts::default()), "4 min 59 s after");
        motion(&mut c, &mut i, 1_000_000 + USED_HOLD_US);
        assert!(!c.app_in_use(&[w], AppFacts::default()), "5 min after");

        // Super+Tab is the compositor's: no use of the window it leaves
        super_tab(&mut c, &mut i);
        assert!(!c.app_in_use(&[w], AppFacts::default()), "a shortcut");
    }

    #[test]
    fn a_click_into_a_window_keeps_its_app() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let w = map_windows(&mut c, a, 1)[0];
        c.pointer = crate::pointer::PointerState::Located {
            output: a,
            p: (960.0, 540.0),
        };
        for pressed in [true, false] {
            c.physical_input_vec(
                &mut i,
                crate::PhysicalEvent::PointerButton {
                    button: 0x110,
                    pressed,
                    modifiers: Default::default(),
                    time_us: 2_000_000,
                },
            )
            .unwrap();
        }
        // the window then leaves the view
        let mut fx = Vec::new();
        c.place_by_user(
            w,
            crate::geom::CanvasPoint {
                x: 50_000,
                y: 50_000,
            },
            &mut fx,
        )
        .unwrap();
        c.render_list(a).unwrap();
        assert!(!c.membership.is_observed(w));
        assert!(c.app_in_use(&[w], AppFacts::default()));
    }

    #[test]
    fn an_app_in_background_keep_is_kept_by_its_app_id() {
        let (mut c, _i, _, mut t) = Core::new(PersistedCameras::default());
        let w = hidden_window(&mut c, &mut t);
        assert_eq!(c.background_kept(w), None, "no app id yet");
        c.set_background_keep(&["Blender", " org.kde.kdenlive "]);
        assert!(!c.app_in_use(&[w], AppFacts::default()));
        c.toplevel_metadata(w, Some("Render"), Some("org.kde.Kdenlive"))
            .unwrap();
        assert_eq!(c.background_kept(w), Some(true));
        assert!(c.app_in_use(&[w], AppFacts::default()));
        c.set_background_keep(&[]);
        assert_eq!(c.background_kept(w), Some(false));
        assert!(!c.app_in_use(&[w], AppFacts::default()));
    }

    /// The window menu's switch keeps the app at once and asks the adapter to write it down; a
    /// window without an app id has nothing to keep it by.
    #[test]
    fn the_window_menu_switch_keeps_the_app_and_says_so() {
        use crate::protocol::Effect;
        use crate::stage::ElementRef;
        let (mut c, _i, mut s, mut t) = Core::new(PersistedCameras::default());
        let w = hidden_window(&mut c, &mut t);
        let element = ElementRef {
            instance: c.stage().instance(),
            element: w,
        };
        let keep = |c: &mut Core, s: &mut crate::ShellPort, on| {
            c.shell_command(s, crate::ShellCommand::BackgroundKeep { element, on })
                .unwrap()
        };
        assert_eq!(keep(&mut c, &mut s, true), vec![]);
        c.toplevel_metadata(w, None, Some("Blender")).unwrap();
        assert_eq!(
            keep(&mut c, &mut s, true),
            vec![Effect::BackgroundKept {
                element: w,
                on: true
            }]
        );
        assert!(c.app_in_use(&[w], AppFacts::default()));
        assert_eq!(keep(&mut c, &mut s, true).len(), 1, "once on the list");
        assert_eq!(c.background_keep, ["blender"]);
        assert_eq!(
            keep(&mut c, &mut s, false),
            vec![Effect::BackgroundKept {
                element: w,
                on: false
            }]
        );
        assert!(!c.app_in_use(&[w], AppFacts::default()));
    }

    #[test]
    fn a_closed_window_is_forgotten() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let w = hidden_window(&mut c, &mut t);
        c.set_focus(&mut i, Some(w)).unwrap();
        key_at(&mut c, &mut i, 1_000_000);
        assert_eq!(c.last_used.len(), 1);
        c.toplevel_destroyed(w);
        assert!(c.last_used.is_empty());
    }
}
