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
pub const KEY_S: u32 = 31;
pub const KEY_R: u32 = 19;
pub const KEY_BACKSPACE: u32 = 14;
pub const KEY_SYSRQ: u32 = 99;
pub const KEY_L: u32 = 38;
pub const KEY_MUTE: u32 = 113;
pub const KEY_VOLUMEDOWN: u32 = 114;
pub const KEY_VOLUMEUP: u32 = 115;
pub const KEY_PLAYPAUSE: u32 = 164;
pub const KEY_NEXTSONG: u32 = 163;
pub const KEY_PREVIOUSSONG: u32 = 165;
pub const KEY_BRIGHTNESSDOWN: u32 = 224;
pub const KEY_BRIGHTNESSUP: u32 = 225;
pub const KEY_KPENTER: u32 = 96;
pub const KEY_D: u32 = 32;
pub const KEY_T: u32 = 20;

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
    /// F-FIT-1 on the pointer's output, for the element under the pointer (INC-10).
    FitViewport,
    /// Restore (M8, OBJ-30) on the pointer's output (INC-10).
    ViewportRestore,
    /// Super+Enter (owner decision 2026-10-21): fit the element under the pointer, or, when the
    /// camera already shows exactly that fit, restore the previous camera. Stateless: decided by
    /// comparing the camera with the fit it would make now (no fit mode, INV-CAMERA-5).
    FitToggle,
    /// Super+Shift+Enter (owner decision 2026-10-21): the focused window enters or leaves native
    /// fullscreen on the pointer's output, through the same path as its own request.
    ToggleFullscreen,
    /// Super+Shift+Delete (owner, 2026-10-21): the focused window's client is ended now — its
    /// connection is cut and its process killed — for an app that does not answer a close. The
    /// adapter falls back to a close request for an X11 window (all of them share one client).
    ForceCloseFocused,
    /// Asks the focused window to close (`xdg_toplevel.close`); the client decides. The owner's
    /// binding Super+Delete (interaction intent of 2026-10-05; decision record 2026-10-21).
    CloseFocused,
    /// Super+Space (owner interaction intent of 2026-10-05, restated 2026-10-21): the pointer's
    /// output's camera is centred on the focused window, its zoom unchanged — "center it", not
    /// fit it. No history entry, as a shell `Navigate`.
    Recenter,
    Noop,
}

/// A compositor key binding specification.
#[derive(Clone, Copy, Debug)]
pub struct KeyBinding {
    pub evdev: u32,
    pub needs_logo: bool,
    pub action: KeyAction,
}

/// Canonical key bindings for Foundation 01: none beyond the exact-modifier bindings resolved in
/// [`resolve_key_binding`]. FocusNext (an x10 harness fixture on Logo+Enter, INC4-F04) moved to
/// Super+Tab when the owner gave Super+Enter to fit; Super+Shift+Tab is reserved for the overview
/// (owner decisions 2026-10-21).
pub const CANONICAL_BINDINGS: [KeyBinding; 0] = [];

/// Pure binding filter: resolves an evdev keycode and modifier snapshot against bindings.
/// Plain Escape only cancels when an interaction is actively in progress (F04, K6).
///
/// INC-10 navigation bindings (owner decision 4 of the INC-10 exit review; exact matching per the
/// M1 brief; proposed for C12.F17, not adopted): Logo+F fits, Logo+Shift+F restores. The modifiers
/// match exactly: Ctrl or Alt held as well resolves neither,
/// and fit requires Shift absent, so Logo+Shift+F never also fits. Neither resolves while an
/// interaction owns the camera or the pointer: a pan computes the camera from its own start, so a
/// fit inside it would be overwritten.
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
    if (key == KEY_ENTER || key == KEY_KPENTER) && mods.logo && !mods.ctrl && !mods.alt && !interaction_active {
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
    // Logo+Delete closes the focused window, Logo+Shift+Delete ends its app; the modifiers match
    // exactly, as for fit
    if key == KEY_DELETE && mods.logo && !mods.ctrl && !mods.alt && !interaction_active {
        if mods.shift {
            return Some(KeyAction::ForceCloseFocused);
        }
        return Some(KeyAction::CloseFocused);
    }
    // Super+Space centres the focused window; exactly Logo, never during an interaction
    if key == KEY_SPACE
        && mods.logo
        && !mods.shift
        && !mods.ctrl
        && !mods.alt
        && !interaction_active
    {
        return Some(KeyAction::Recenter);
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
    // Super+W or Super+D: toggles the Dynamic Island launcher; exactly Logo, never during interaction
    if (key == KEY_W || key == KEY_D) && mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::ToggleLauncher);
    }
    // Super+Q or Super+T or Ctrl+Alt+T: launches terminal; never during interaction
    if (key == KEY_Q || key == KEY_T) && mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::SpawnTerminal);
    }
    if key == KEY_T && mods.ctrl && mods.alt && !mods.logo && !mods.shift && !interaction_active {
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
    if key == KEY_BACKSPACE && mods.logo && mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::ReloadShell);
    }
    // Super+L: lock screen; Logo, never during interaction
    if key == KEY_L && mods.logo && !mods.shift && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(KeyAction::SpawnLockScreen);
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

    /// The owner's Super+Space: exactly Logo held, never during an interaction.
    #[test]
    fn logo_space_recenters_with_exact_modifiers() {
        let m = |logo, ctrl, alt, shift| Modifiers {
            logo,
            ctrl,
            alt,
            shift,
        };
        assert_eq!(
            resolve_key_binding(KEY_SPACE, m(true, false, false, false), false),
            Some(KeyAction::Recenter)
        );
        for mods in [
            m(false, false, false, false),
            m(true, false, false, true),
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
    }

    /// The owner's Super+Delete: exactly Logo held, never during an interaction.
    #[test]
    fn logo_delete_closes_with_exact_modifiers() {
        let m = |logo, ctrl, alt, shift| Modifiers {
            logo,
            ctrl,
            alt,
            shift,
        };
        assert_eq!(
            resolve_key_binding(KEY_DELETE, m(true, false, false, false), false),
            Some(KeyAction::CloseFocused)
        );
        assert_eq!(
            resolve_key_binding(KEY_DELETE, m(true, false, false, true), false),
            Some(KeyAction::ForceCloseFocused),
            "Super+Shift+Delete ends the app"
        );
        for mods in [
            m(false, false, false, false),
            m(true, true, false, false),
            m(true, false, true, false),
            m(true, true, false, true),
        ] {
            assert_eq!(
                resolve_key_binding(KEY_DELETE, mods, false),
                None,
                "{mods:?}"
            );
        }
        assert_eq!(
            resolve_key_binding(KEY_DELETE, m(true, false, false, false), true),
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
        assert_eq!(resolve_key_binding(KEY_A, logo, false), None);
    }

    /// INC-10: Logo+F fits, Logo+Shift+F restores (fit requires Shift absent); plain F and any F
    /// during an interaction are not compositor bindings.
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
            resolve_key_binding(KEY_Q, Modifiers { shift: true, ..logo }, false),
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
        // During interaction: None
        assert_eq!(resolve_key_binding(KEY_VOLUMEUP, none, true), None);
        assert_eq!(resolve_key_binding(KEY_L, logo, true), None);
    }
}
