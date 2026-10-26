//! Key bindings and physical-key evaluation (C4.4, M12).
//!
//! Evaluated during Smithay's two-phase `input_intercept` dispatch:
//! - K1: press ownership decided at press;
//! - K2: release follows the press owner;
//! - K3: keys that change XKB modifier or lock state are never compositor-owned (observe mode).

use crate::input::bindings::Modifiers;

pub const KEY_ESC: u32 = 1;
pub const KEY_ENTER: u32 = 28;
pub const KEY_LEFTCTRL: u32 = 29;
pub const KEY_LEFTALT: u32 = 56;
pub const KEY_F: u32 = 33;
pub const KEY_A: u32 = 30;
pub const KEY_LEFTSHIFT: u32 = 42;
pub const KEY_B: u32 = 48;
pub const KEY_TAB: u32 = 15;
pub const KEY_DELETE: u32 = 111;
pub const KEY_SPACE: u32 = 57;
pub const KEY_LEFTMETA: u32 = 125;
pub const KEY_1: u32 = 2;
pub const KEY_2: u32 = 3;
pub const KEY_3: u32 = 4;
pub const KEY_4: u32 = 5;
pub const KEY_5: u32 = 6;
pub const KEY_6: u32 = 7;
pub const KEY_7: u32 = 8;
pub const KEY_8: u32 = 9;
pub const KEY_9: u32 = 10;
pub const KEY_Q: u32 = 16;
pub const KEY_W: u32 = 17;
pub const KEY_E: u32 = 18;
pub const KEY_I: u32 = 23;
pub const KEY_P: u32 = 25;
pub const KEY_T: u32 = 20;
pub const KEY_C: u32 = 46;
pub const KEY_S: u32 = 31;
pub const KEY_R: u32 = 19;
pub const KEY_BACKSPACE: u32 = 14;
pub const KEY_SYSRQ: u32 = 99;
pub const KEY_L: u32 = 38;
pub const KEY_V: u32 = 47;
pub const KEY_MUTE: u32 = 113;
pub const KEY_VOLUMEDOWN: u32 = 114;
pub const KEY_VOLUMEUP: u32 = 115;
pub const KEY_PLAYPAUSE: u32 = 164;
pub const KEY_NEXTSONG: u32 = 163;
pub const KEY_PREVIOUSSONG: u32 = 165;
pub const KEY_BRIGHTNESSDOWN: u32 = 224;
pub const KEY_BRIGHTNESSUP: u32 = 225;

/// An action triggered by a matched compositor key binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    FocusNext,
    /// Super+1..9: focus, raise, and recenter camera on mapped window at slot 0..8 (creation order)
    FocusSlot(usize),
    /// Super+Shift+1..9: focus, raise, and frame/recenter camera on cluster at slot 0..8 (creation order)
    FocusClusterSlot(usize),
    /// Super+W: toggles the Dynamic Island start launcher
    ToggleLauncher,
    /// Super+Q: launches terminal
    SpawnTerminal,
    /// Super+E: launches file manager
    SpawnFileManager,
    /// Super+Shift+S or PrintScreen: capture fullscreen screenshot
    SpawnScreenshot,
    /// Super+Shift+R: capture region screenshot
    SpawnRegionScreenshot,
    /// Super+Shift+Tab: toggles canvas overview radar
    ToggleOverview,
    /// Super+Shift+BackSpace: reload shell UI and scripts
    ReloadShell,
    /// Super+L: lock screen
    SpawnLockScreen,
    /// Super+V: clipboard history
    ClipboardHistory,
    /// Super+A: the control menu (Wi-Fi, Bluetooth, sound output, power, notifications)
    ControlMenu,
    /// Super+Shift+P: toggle privacy mode on focused window (hidden/transparent from screenshots & screenshare)
    TogglePrivacy,
    /// Super+Shift+I: toggle idle sleep inhibition (stay awake mode: prevents DPMS screen sleep and auto-lock)
    ToggleIdleInhibit,
    /// Super+Shift+T: the focused window tears while fullscreen (its frames shown at once), or no longer
    ToggleTearing,
    /// Super+Alt+C: save instant 20-minute replay clip
    SaveReplay,
    /// Super+Alt+R: toggle continuous screen recording
    ToggleRecording,
    /// Hardware Volume Up
    VolumeUp,
    /// Hardware Volume Down
    VolumeDown,
    /// Hardware Volume Mute
    VolumeMute,
    /// Hardware Brightness Up
    BrightnessUp,
    /// Hardware Brightness Down
    BrightnessDown,
    /// Hardware Media Play / Pause
    MediaPlayPause,
    /// Hardware Media Next Song
    MediaNext,
    /// Hardware Media Previous Song
    MediaPrev,

    Cancel,
    /// F-FIT-1 on the pointer's output, for the element under the pointer.
    FitViewport,
    /// Restore (M8, OBJ-30) on the pointer's output.
    ViewportRestore,
    /// Super+Enter (owner decision 2026-10-21): fit the element under the pointer, or, when the
    /// camera already shows exactly that fit, restore the previous camera. Stateless: decided by
    /// comparing the camera with the fit it would make now (no fit mode, INV-CAMERA-5).
    FitToggle,
    /// Super+Shift+Enter (owner decision 2026-10-21): the focused window enters or leaves native
    /// fullscreen on the pointer's output, through the same path as its own request.
    ToggleFullscreen,
    /// Super+Shift+C (owner, 2026-10-21; Shift+Delete until 2026-10-25): the focused window's
    /// client is ended now — its connection is cut and its process killed — for an app that does
    /// not answer a close. The adapter falls back to a close request for an X11 window (all of
    /// them share one client).
    ForceCloseFocused,
    /// Asks the focused window to close (`xdg_toplevel.close`); the client decides. The owner's
    /// binding Super+C (Super+Delete until 2026-10-25; interaction intent of 2026-10-05; decision
    /// record 2026-10-21).
    CloseFocused,
    /// Super+Space or Super+Shift+Space (owner interaction intent of 2026-10-05, restated 2026-10-24):
    /// the pointer's output's camera is centred on the focused window and zoom is reverted to
    /// default 1.0 scale (or, if no window is focused, camera zoom is reset to 1.0).
    /// No history entry, as a shell `Navigate`.
    Recenter,
    /// Super+Shift+Space (owner, 2026-10-24): centre the camera on the focused window and keep
    /// the zoom.
    RecenterKeepZoom,
    /// A user command from the config (`EXEC_<NAME>="CHORD:command"`), by its index in the
    /// adapter's command list.
    Exec(u16),
    Noop,
}

/// A key chord from the config: an evdev key with exactly these modifiers held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: u32,
    pub mods: Modifiers,
}

/// The user's bindings from the config, checked before the built-in ones: chords and what they
/// do (`None`: nothing, a built-in chord freed), and the actions given a chord of their own,
/// whose built-in chords no longer trigger them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserBindings {
    pub chords: Vec<(Chord, Option<KeyAction>)>,
    pub moved: Vec<KeyAction>,
}

/// Evdev codes by key name, for config chords.
const KEY_NAMES: &[(&str, u32)] = &[
    ("ESC", 1),
    ("ESCAPE", 1),
    ("1", 2),
    ("2", 3),
    ("3", 4),
    ("4", 5),
    ("5", 6),
    ("6", 7),
    ("7", 8),
    ("8", 9),
    ("9", 10),
    ("0", 11),
    ("MINUS", 12),
    ("EQUAL", 13),
    ("BACKSPACE", 14),
    ("TAB", 15),
    ("Q", 16),
    ("W", 17),
    ("E", 18),
    ("R", 19),
    ("T", 20),
    ("Y", 21),
    ("U", 22),
    ("I", 23),
    ("O", 24),
    ("P", 25),
    ("BRACKETLEFT", 26),
    ("BRACKETRIGHT", 27),
    ("RETURN", 28),
    ("ENTER", 28),
    ("A", 30),
    ("S", 31),
    ("D", 32),
    ("F", 33),
    ("G", 34),
    ("H", 35),
    ("J", 36),
    ("K", 37),
    ("L", 38),
    ("SEMICOLON", 39),
    ("APOSTROPHE", 40),
    ("GRAVE", 41),
    ("BACKSLASH", 43),
    ("Z", 44),
    ("X", 45),
    ("C", 46),
    ("V", 47),
    ("B", 48),
    ("N", 49),
    ("M", 50),
    ("COMMA", 51),
    ("PERIOD", 52),
    ("DOT", 52),
    ("SLASH", 53),
    ("SPACE", 57),
    ("F1", 59),
    ("F2", 60),
    ("F3", 61),
    ("F4", 62),
    ("F5", 63),
    ("F6", 64),
    ("F7", 65),
    ("F8", 66),
    ("F9", 67),
    ("F10", 68),
    ("F11", 87),
    ("F12", 88),
    ("PRINT", 99),
    ("HOME", 102),
    ("UP", 103),
    ("PAGEUP", 104),
    ("LEFT", 105),
    ("RIGHT", 106),
    ("END", 107),
    ("DOWN", 108),
    ("PAGEDOWN", 109),
    ("INSERT", 110),
    ("DELETE", 111),
];

/// Parses a config chord such as `SUPER+SHIFT+Q` (modifiers SUPER/LOGO/WIN, CTRL/CONTROL, ALT,
/// SHIFT; any case; the key last). `None` for an unknown key or modifier, or no key. Pure.
pub fn parse_chord(text: &str) -> Option<Chord> {
    let mut mods = Modifiers::default();
    let mut key = None;
    for part in text.split('+').map(|p| p.trim().to_ascii_uppercase()) {
        match part.as_str() {
            "SUPER" | "LOGO" | "WIN" | "MOD4" => mods.logo = true,
            "CTRL" | "CONTROL" => mods.ctrl = true,
            "ALT" => mods.alt = true,
            "SHIFT" => mods.shift = true,
            name => {
                if key.is_some() {
                    return None;
                }
                key = Some(KEY_NAMES.iter().find(|(n, _)| *n == name)?.1);
            }
        }
    }
    Some(Chord { key: key?, mods })
}

/// The actions a config can bind, by the name after `BIND_` (`BIND_TERMINAL="SUPER+RETURN"`).
pub const ACTION_NAMES: &[(&str, KeyAction)] = &[
    ("FOCUS_NEXT", KeyAction::FocusNext),
    ("OVERVIEW", KeyAction::ToggleOverview),
    ("LAUNCHER", KeyAction::ToggleLauncher),
    ("TERMINAL", KeyAction::SpawnTerminal),
    ("FILE_MANAGER", KeyAction::SpawnFileManager),
    ("SCREENSHOT", KeyAction::SpawnScreenshot),
    ("REGION_SCREENSHOT", KeyAction::SpawnRegionScreenshot),
    ("RELOAD", KeyAction::ReloadShell),
    ("LOCK", KeyAction::SpawnLockScreen),
    ("CLIPBOARD", KeyAction::ClipboardHistory),
    ("CONTROL", KeyAction::ControlMenu),
    ("PRIVACY", KeyAction::TogglePrivacy),
    ("IDLE_INHIBIT", KeyAction::ToggleIdleInhibit),
    ("TEARING", KeyAction::ToggleTearing),
    ("SAVE_REPLAY", KeyAction::SaveReplay),
    ("RECORD", KeyAction::ToggleRecording),
    ("FIT", KeyAction::FitToggle),
    ("FULLSCREEN", KeyAction::ToggleFullscreen),
    ("CLOSE", KeyAction::CloseFocused),
    ("FORCE_CLOSE", KeyAction::ForceCloseFocused),
    ("RECENTER", KeyAction::Recenter),
    ("RECENTER_KEEP_ZOOM", KeyAction::RecenterKeepZoom),
];

/// [`resolve_key_binding`] with the user's bindings first: a chord of theirs wins (also over a
/// built-in one), and an action they moved no longer answers its built-in chord. Escape's cancel
/// is never rebound, and no user binding resolves while an interaction is active. Pure; nothing
/// allocated.
pub fn resolve_key_binding_with(
    user: &UserBindings,
    key: u32,
    mods: Modifiers,
    interaction_active: bool,
) -> Option<KeyAction> {
    if key != KEY_ESC
        && !interaction_active
        && let Some((_, action)) = user
            .chords
            .iter()
            .find(|(c, _)| c.key == key && c.mods == mods)
    {
        return *action;
    }
    resolve_key_binding(key, mods, interaction_active).filter(|a| !user.moved.contains(a))
}

/// A compositor key binding specification.
#[derive(Clone, Copy, Debug)]
pub struct KeyBinding {
    pub evdev: u32,
    pub needs_logo: bool,
    pub action: KeyAction,
}

/// Canonical key bindings: none beyond the exact-modifier bindings resolved in
/// [`resolve_key_binding`]. FocusNext is on Super+Tab; Super+Shift+Tab is reserved for the overview
/// (owner decisions 2026-10-21).
pub const CANONICAL_BINDINGS: [KeyBinding; 0] = [];

/// Pure binding filter: resolves an evdev keycode and modifier snapshot against bindings.
/// Plain Escape only cancels when an interaction is actively in progress (F04, K6).
///
/// Modifiers must match exactly: extra modifiers like Ctrl or Alt prevent matching.
/// Bindings do not resolve while an interaction owns the camera or pointer (except Escape),
/// preventing interference with active gestures.
pub fn resolve_key_binding(
    key: u32,
    mods: Modifiers,
    interaction_active: bool,
) -> Option<KeyAction> {
    if key == KEY_ESC {
        return if interaction_active {
            Some(KeyAction::Cancel)
        } else {
            None
        };
    }
    // Logo+Enter fits or restores (a toggle), Logo+Shift+Enter toggles fullscreen; Ctrl or Alt held
    // as well resolves neither; not during an interaction (the owner's bindings of 2026-10-21)
    if key == KEY_ENTER && mods.logo && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(match mods.shift {
            true => KeyAction::ToggleFullscreen,
            false => KeyAction::FitToggle,
        });
    }
    // Super+Tab cycles the focus; Super+Shift+Tab toggles canvas overview radar
    if key == KEY_TAB && mods.logo && !mods.ctrl && !mods.alt && !interaction_active {
        if mods.shift {
            return Some(KeyAction::ToggleOverview);
        }
        return Some(KeyAction::FocusNext);
    }
    // Logo+C closes the focused window, Logo+Shift+C ends its app (owner, 2026-10-25: Delete
    // took the hand off the mouse); the modifiers match
    // exactly, as for fit
    if key == KEY_C && mods.logo && !mods.ctrl && !mods.alt && !interaction_active {
        if mods.shift {
            return Some(KeyAction::ForceCloseFocused);
        }
        return Some(KeyAction::CloseFocused);
    }
    // Super+Space centres the focused window and reverts zoom to 1.0; Super+Shift+Space centres
    // it and keeps the zoom; never during an interaction
    if key == KEY_SPACE && mods.logo && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(match mods.shift {
            true => KeyAction::RecenterKeepZoom,
            false => KeyAction::Recenter,
        });
    }
    // Super+1..9 focuses, raises and recenters window slot 1..9 (0-indexed); exactly Logo, never during interaction
    if mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        let slot = match key {
            KEY_1 => Some(0),
            KEY_2 => Some(1),
            KEY_3 => Some(2),
            KEY_4 => Some(3),
            KEY_5 => Some(4),
            KEY_6 => Some(5),
            KEY_7 => Some(6),
            KEY_8 => Some(7),
            KEY_9 => Some(8),
            _ => None,
        };
        if let Some(s) = slot {
            return Some(KeyAction::FocusSlot(s));
        }
    }
    // Super+Shift+1..9 focuses, raises and frames/recenters cluster slot 1..9 (0-indexed); Logo+Shift, never during interaction
    if mods.logo && mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        let slot = match key {
            KEY_1 => Some(0),
            KEY_2 => Some(1),
            KEY_3 => Some(2),
            KEY_4 => Some(3),
            KEY_5 => Some(4),
            KEY_6 => Some(5),
            KEY_7 => Some(6),
            KEY_8 => Some(7),
            KEY_9 => Some(8),
            _ => None,
        };
        if let Some(s) = slot {
            return Some(KeyAction::FocusClusterSlot(s));
        }
    }
    // Super+W: toggles the Dynamic Island launcher; exactly Logo, never during interaction
    if key == KEY_W && mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::ToggleLauncher);
    }
    // Super+Q: launches terminal; exactly Logo, never during interaction
    if key == KEY_Q && mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::SpawnTerminal);
    }
    // Super+E: launches file manager; exactly Logo, never during interaction
    if key == KEY_E && mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::SpawnFileManager);
    }
    // Super+Shift+S or PrintScreen (KEY_SYSRQ): screenshot capture; never during interaction
    if (key == KEY_S && mods.logo && mods.shift && !mods.ctrl && !mods.alt && !interaction_active)
        || (key == KEY_SYSRQ && !interaction_active)
    {
        return Some(KeyAction::SpawnScreenshot);
    }
    // Super+Shift+R: region screenshot; Logo+Shift, never during interaction
    if key == KEY_R && mods.logo && mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::SpawnRegionScreenshot);
    }
    // Super+Shift+BackSpace: reload shell UI and scripts; Logo+Shift, never during interaction
    if key == KEY_BACKSPACE
        && mods.logo
        && mods.shift
        && !mods.ctrl
        && !mods.alt
        && !interaction_active
    {
        return Some(KeyAction::ReloadShell);
    }
    // Super+L: lock screen; Logo, never during interaction
    if key == KEY_L && mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::SpawnLockScreen);
    }
    // Super+V: clipboard history; Logo, never during interaction
    if key == KEY_V && mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::ClipboardHistory);
    }
    // Super+A: the control menu; Logo, never during interaction
    if key == KEY_A && mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::ControlMenu);
    }
    // Super+Shift+P: toggle privacy mode; Logo+Shift, never during interaction
    if key == KEY_P && mods.logo && mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::TogglePrivacy);
    }
    // Super+Shift+I: toggle idle sleep inhibition (stay awake mode); Logo+Shift, never during interaction
    if key == KEY_I && mods.logo && mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::ToggleIdleInhibit);
    }
    // Super+Shift+T: the focused window tears while fullscreen; Logo+Shift, never during interaction
    if key == KEY_T && mods.logo && mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::ToggleTearing);
    }
    // Super+Alt+C: save instant 20-minute replay clip; Logo+Alt, never during interaction
    if key == KEY_C && mods.logo && mods.alt && !mods.ctrl && !mods.shift && !interaction_active {
        return Some(KeyAction::SaveReplay);
    }
    // Super+Alt+R: toggle continuous screen recording; Logo+Alt, never during interaction
    if key == KEY_R && mods.logo && mods.alt && !mods.ctrl && !mods.shift && !interaction_active {
        return Some(KeyAction::ToggleRecording);
    }
    // Hardware media and volume keys: triggered directly on keypress, never during interaction
    if !interaction_active {
        match key {
            KEY_VOLUMEUP => return Some(KeyAction::VolumeUp),
            KEY_VOLUMEDOWN => return Some(KeyAction::VolumeDown),
            KEY_MUTE => return Some(KeyAction::VolumeMute),
            KEY_BRIGHTNESSUP => return Some(KeyAction::BrightnessUp),
            KEY_BRIGHTNESSDOWN => return Some(KeyAction::BrightnessDown),
            KEY_PLAYPAUSE => return Some(KeyAction::MediaPlayPause),
            KEY_NEXTSONG => return Some(KeyAction::MediaNext),
            KEY_PREVIOUSSONG => return Some(KeyAction::MediaPrev),
            _ => {}
        }
    }

    for b in &CANONICAL_BINDINGS {
        if b.evdev == key && (!b.needs_logo || mods.logo) {
            return Some(b.action);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(logo: bool, ctrl: bool, alt: bool, shift: bool) -> Modifiers {
        Modifiers {
            logo,
            ctrl,
            alt,
            shift,
        }
    }

    /// Config chords: modifiers in any order and case, the key last; unknown names and two keys
    /// are refused.
    #[test]
    fn chords_parse_from_the_config() {
        assert_eq!(
            parse_chord("SUPER+SHIFT+Q"),
            Some(Chord {
                key: KEY_Q,
                mods: mods(true, false, false, true)
            })
        );
        assert_eq!(
            parse_chord("shift + super + return"),
            Some(Chord {
                key: KEY_ENTER,
                mods: mods(true, false, false, true)
            })
        );
        assert_eq!(
            parse_chord("F5"),
            Some(Chord {
                key: 63,
                mods: Modifiers::default()
            })
        );
        assert_eq!(parse_chord("SUPER+HYPER+Q"), None);
        assert_eq!(parse_chord("SUPER+Q+W"), None);
        assert_eq!(parse_chord("SUPER+SHIFT"), None);
    }

    /// A user chord wins over a built-in one; a moved action stops answering its built-in
    /// chord; Escape's cancel and interactions are untouched; other bindings stay.
    #[test]
    fn user_bindings_come_first_and_free_moved_defaults() {
        let logo = mods(true, false, false, false);
        let user = UserBindings {
            chords: vec![
                (
                    Chord {
                        key: KEY_ENTER,
                        mods: mods(true, false, false, true),
                    },
                    Some(KeyAction::SpawnTerminal),
                ),
                (
                    Chord {
                        key: KEY_W,
                        mods: logo,
                    },
                    None,
                ),
                (
                    Chord {
                        key: KEY_B,
                        mods: logo,
                    },
                    Some(KeyAction::Exec(0)),
                ),
            ],
            moved: vec![KeyAction::SpawnTerminal],
        };
        let r = |key, m, active| resolve_key_binding_with(&user, key, m, active);
        // Super+Shift+Enter was fullscreen; the user gave it to the terminal
        assert_eq!(
            r(KEY_ENTER, mods(true, false, false, true), false),
            Some(KeyAction::SpawnTerminal)
        );
        // the terminal's built-in Super+Q is free now
        assert_eq!(r(KEY_Q, logo, false), None);
        // Super+W freed; Super+B runs command 0
        assert_eq!(r(KEY_W, logo, false), None);
        assert_eq!(r(KEY_B, logo, false), Some(KeyAction::Exec(0)));
        // exact modifiers: Super+Ctrl+B is not Super+B
        assert_eq!(r(KEY_B, mods(true, true, false, false), false), None);
        // untouched bindings and Escape's cancel
        assert_eq!(r(KEY_L, logo, false), Some(KeyAction::SpawnLockScreen));
        assert_eq!(
            r(KEY_ESC, Modifiers::default(), true),
            Some(KeyAction::Cancel)
        );
        // no user binding during an interaction
        assert_eq!(r(KEY_B, logo, true), None);
    }

    /// The owner's Super+Space: exactly Logo held, never during an interaction.
    #[test]
    fn logo_space_recenters_and_reverts_scale() {
        let m = |logo, ctrl, alt, shift| Modifiers {
            logo,
            ctrl,
            alt,
            shift,
        };
        // Super+Space recenters and reverts the scale; Super+Shift+Space recenters, zoom kept
        assert_eq!(
            resolve_key_binding(KEY_SPACE, m(true, false, false, false), false),
            Some(KeyAction::Recenter)
        );
        assert_eq!(
            resolve_key_binding(KEY_SPACE, m(true, false, false, true), false),
            Some(KeyAction::RecenterKeepZoom)
        );
        for mods in [
            m(false, false, false, false),
            m(true, true, false, false),
            m(true, false, true, false),
        ] {
            assert_eq!(
                resolve_key_binding(KEY_SPACE, mods, false),
                None,
                "{mods:?}"
            );
        }
        assert_eq!(
            resolve_key_binding(KEY_SPACE, m(true, false, false, false), true),
            None,
            "not during an interaction"
        );
        assert_eq!(
            resolve_key_binding(KEY_SPACE, m(true, false, false, true), true),
            None,
            "not during an interaction"
        );
    }

    /// The owner's Super+C: exactly Logo held, never during an interaction; Super+Alt+C stays the
    /// replay clip, and Delete closes nothing.
    #[test]
    fn logo_c_closes_with_exact_modifiers() {
        let m = |logo, ctrl, alt, shift| Modifiers {
            logo,
            ctrl,
            alt,
            shift,
        };
        assert_eq!(
            resolve_key_binding(KEY_C, m(true, false, false, false), false),
            Some(KeyAction::CloseFocused)
        );
        assert_eq!(
            resolve_key_binding(KEY_C, m(true, false, false, true), false),
            Some(KeyAction::ForceCloseFocused),
            "Super+Shift+C ends the app"
        );
        assert_eq!(
            resolve_key_binding(KEY_C, m(true, false, true, false), false),
            Some(KeyAction::SaveReplay),
            "Super+Alt+C saves a clip"
        );
        for mods in [m(true, false, false, false), m(true, false, false, true)] {
            assert_eq!(
                resolve_key_binding(KEY_DELETE, mods, false),
                None,
                "Delete is free: {mods:?}"
            );
        }
        for mods in [
            m(false, false, false, false),
            m(true, true, false, false),
            m(true, true, false, true),
        ] {
            assert_eq!(resolve_key_binding(KEY_C, mods, false), None, "{mods:?}");
        }
        assert_eq!(
            resolve_key_binding(KEY_C, m(true, false, false, false), true),
            None,
            "not while an interaction owns the pointer"
        );
    }

    #[test]
    fn binding_resolution_honours_logo_modifier() {
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let none = Modifiers::default();

        let logo_shift = Modifiers {
            shift: true,
            ..logo
        };
        assert_eq!(
            resolve_key_binding(KEY_TAB, logo, false),
            Some(KeyAction::FocusNext)
        );
        assert_eq!(
            resolve_key_binding(KEY_TAB, logo_shift, false),
            Some(KeyAction::ToggleOverview)
        );
        assert_eq!(
            resolve_key_binding(KEY_S, logo_shift, false),
            Some(KeyAction::SpawnScreenshot)
        );
        assert_eq!(
            resolve_key_binding(KEY_R, logo_shift, false),
            Some(KeyAction::SpawnRegionScreenshot)
        );
        assert_eq!(
            resolve_key_binding(KEY_BACKSPACE, logo_shift, false),
            Some(KeyAction::ReloadShell)
        );
        assert_eq!(
            resolve_key_binding(KEY_SYSRQ, none, false),
            Some(KeyAction::SpawnScreenshot)
        );
        assert_eq!(resolve_key_binding(KEY_TAB, none, false), None);
        assert_eq!(resolve_key_binding(KEY_B, logo, false), None);
        assert_eq!(resolve_key_binding(KEY_B, none, false), None);
        assert_eq!(resolve_key_binding(KEY_ESC, none, false), None);
        assert_eq!(
            resolve_key_binding(KEY_ESC, none, true),
            Some(KeyAction::Cancel)
        );
        assert_eq!(resolve_key_binding(KEY_ESC, logo, false), None);
        assert_eq!(
            resolve_key_binding(KEY_ESC, logo, true),
            Some(KeyAction::Cancel)
        );
        assert_eq!(resolve_key_binding(KEY_LEFTSHIFT, none, false), None);
        assert_eq!(resolve_key_binding(KEY_B, logo, false), None);
    }

    #[test]
    fn super_a_control_menu_binding() {
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        assert_eq!(
            resolve_key_binding(KEY_A, logo, false),
            Some(KeyAction::ControlMenu)
        );
        let logo_shift = Modifiers {
            shift: true,
            ..logo
        };
        assert_eq!(resolve_key_binding(KEY_A, logo_shift, false), None);
        assert_eq!(
            resolve_key_binding(KEY_A, logo, true),
            None,
            "not during an interaction"
        );
        assert!(ACTION_NAMES.contains(&("CONTROL", KeyAction::ControlMenu)));
    }

    /// Logo+Enter fits or restores (FitToggle), Logo+Shift+Enter toggles fullscreen;
    /// plain Enter or keys during an interaction are not compositor bindings.
    #[test]
    fn enter_bindings_resolve_the_fit_toggle_and_fullscreen() {
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let logo_shift = Modifiers {
            shift: true,
            ..logo
        };
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(
            resolve_key_binding(KEY_ENTER, logo, false),
            Some(KeyAction::FitToggle)
        );
        assert_eq!(
            resolve_key_binding(KEY_ENTER, logo_shift, false),
            Some(KeyAction::ToggleFullscreen)
        );
        assert_eq!(
            resolve_key_binding(KEY_ENTER, Modifiers::default(), false),
            None
        );
        assert_eq!(resolve_key_binding(KEY_ENTER, shift, false), None);
        assert_eq!(resolve_key_binding(KEY_ENTER, logo, true), None);
        assert_eq!(resolve_key_binding(KEY_ENTER, logo_shift, true), None);
        // Super+F was removed by the owner (2026-10-21): no duplicate of Super+Enter
        assert_eq!(resolve_key_binding(KEY_F, logo, false), None);
        assert_eq!(resolve_key_binding(KEY_F, logo_shift, false), None);
    }

    #[test]
    fn enter_bindings_reject_extra_modifiers() {
        for ctrl in [false, true] {
            for alt in [false, true] {
                for shift in [false, true] {
                    let mods = Modifiers {
                        logo: true,
                        ctrl,
                        alt,
                        shift,
                    };
                    let want = match (ctrl || alt, shift) {
                        (true, _) => None,
                        (false, false) => Some(KeyAction::FitToggle),
                        (false, true) => Some(KeyAction::ToggleFullscreen),
                    };
                    assert_eq!(
                        resolve_key_binding(KEY_ENTER, mods, false),
                        want,
                        "{mods:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn super_1_to_9_slot_bindings() {
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let keys = [
            (KEY_1, 0),
            (KEY_2, 1),
            (KEY_3, 2),
            (KEY_4, 3),
            (KEY_5, 4),
            (KEY_6, 5),
            (KEY_7, 6),
            (KEY_8, 7),
            (KEY_9, 8),
        ];
        for (k, slot) in keys {
            assert_eq!(
                resolve_key_binding(k, logo, false),
                Some(KeyAction::FocusSlot(slot))
            );
            // With Shift: FocusClusterSlot
            assert_eq!(
                resolve_key_binding(
                    k,
                    Modifiers {
                        shift: true,
                        ..logo
                    },
                    false
                ),
                Some(KeyAction::FocusClusterSlot(slot))
            );
            // With Ctrl or Alt: None
            assert_eq!(
                resolve_key_binding(k, Modifiers { ctrl: true, ..logo }, false),
                None
            );
            assert_eq!(
                resolve_key_binding(
                    k,
                    Modifiers {
                        shift: true,
                        ctrl: true,
                        ..logo
                    },
                    false
                ),
                None
            );
            // During interaction: None
            assert_eq!(resolve_key_binding(k, logo, true), None);
            assert_eq!(
                resolve_key_binding(
                    k,
                    Modifiers {
                        shift: true,
                        ..logo
                    },
                    true
                ),
                None
            );
        }
    }

    #[test]
    fn super_q_and_super_e_app_bindings() {
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        assert_eq!(
            resolve_key_binding(KEY_Q, logo, false),
            Some(KeyAction::SpawnTerminal)
        );
        assert_eq!(
            resolve_key_binding(KEY_E, logo, false),
            Some(KeyAction::SpawnFileManager)
        );
        assert_eq!(
            resolve_key_binding(
                KEY_Q,
                Modifiers {
                    shift: true,
                    ..logo
                },
                false
            ),
            None
        );
        assert_eq!(
            resolve_key_binding(KEY_E, Modifiers { ctrl: true, ..logo }, false),
            None
        );
        assert_eq!(resolve_key_binding(KEY_Q, logo, true), None);
        assert_eq!(resolve_key_binding(KEY_E, logo, true), None);
    }

    #[test]
    fn media_keys_and_lock_screen_bindings() {
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let none = Modifiers::default();
        assert_eq!(
            resolve_key_binding(KEY_L, logo, false),
            Some(KeyAction::SpawnLockScreen)
        );
        let logo_shift = Modifiers {
            logo: true,
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(
            resolve_key_binding(KEY_P, logo_shift, false),
            Some(KeyAction::TogglePrivacy)
        );
        assert_eq!(
            resolve_key_binding(KEY_T, logo_shift, false),
            Some(KeyAction::ToggleTearing)
        );
        assert_eq!(
            resolve_key_binding(KEY_VOLUMEUP, none, false),
            Some(KeyAction::VolumeUp)
        );
        assert_eq!(
            resolve_key_binding(KEY_VOLUMEDOWN, none, false),
            Some(KeyAction::VolumeDown)
        );
        assert_eq!(
            resolve_key_binding(KEY_MUTE, none, false),
            Some(KeyAction::VolumeMute)
        );
        assert_eq!(
            resolve_key_binding(KEY_BRIGHTNESSUP, none, false),
            Some(KeyAction::BrightnessUp)
        );
        assert_eq!(
            resolve_key_binding(KEY_BRIGHTNESSDOWN, none, false),
            Some(KeyAction::BrightnessDown)
        );
        assert_eq!(
            resolve_key_binding(KEY_PLAYPAUSE, none, false),
            Some(KeyAction::MediaPlayPause)
        );
        assert_eq!(
            resolve_key_binding(KEY_NEXTSONG, none, false),
            Some(KeyAction::MediaNext)
        );
        assert_eq!(
            resolve_key_binding(KEY_PREVIOUSSONG, none, false),
            Some(KeyAction::MediaPrev)
        );
        let logo_alt = Modifiers {
            logo: true,
            ctrl: false,
            alt: true,
            shift: false,
        };
        assert_eq!(
            resolve_key_binding(KEY_C, logo_alt, false),
            Some(KeyAction::SaveReplay)
        );
        assert_eq!(
            resolve_key_binding(KEY_R, logo_alt, false),
            Some(KeyAction::ToggleRecording)
        );
        // During interaction: None
        assert_eq!(resolve_key_binding(KEY_VOLUMEUP, none, true), None);
        assert_eq!(resolve_key_binding(KEY_L, logo, true), None);
        assert_eq!(resolve_key_binding(KEY_C, logo_alt, true), None);
        assert_eq!(resolve_key_binding(KEY_R, logo_alt, true), None);
    }

    #[test]
    fn super_v_clipboard_history_binding() {
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        assert_eq!(
            resolve_key_binding(KEY_V, logo, false),
            Some(KeyAction::ClipboardHistory)
        );
        // Super+Shift+V does not resolve to it
        assert_eq!(
            resolve_key_binding(
                KEY_V,
                Modifiers {
                    shift: true,
                    ..logo
                },
                false
            ),
            None
        );
        // Ctrl+V does not resolve to it
        assert_eq!(
            resolve_key_binding(
                KEY_V,
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                false
            ),
            None
        );
        // Alt+V does not resolve to it
        assert_eq!(
            resolve_key_binding(
                KEY_V,
                Modifiers {
                    alt: true,
                    ..Modifiers::default()
                },
                false
            ),
            None
        );
        // Plain V does not resolve to it
        assert_eq!(
            resolve_key_binding(KEY_V, Modifiers::default(), false),
            None
        );
        // During interaction: None
        assert_eq!(resolve_key_binding(KEY_V, logo, true), None);
        assert!(ACTION_NAMES.contains(&("CLIPBOARD", KeyAction::ClipboardHistory)));
    }
}
