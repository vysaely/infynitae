//! Key bindings and physical-key evaluation (C4.4, M12).
//!
//! Evaluated during Smithay's two-phase `input_intercept` dispatch:
//! - K1: press ownership decided at press;
//! - K2: release follows the press owner;
//! - K3: keys that change XKB modifier or lock state are never compositor-owned (observe mode).

use crate::input::bindings::Modifiers;

pub const KEY_ESC: u32 = 1;
pub const KEY_ENTER: u32 = 28;
pub const KEY_F: u32 = 33;
pub const KEY_A: u32 = 30;
pub const KEY_LEFTSHIFT: u32 = 42;
pub const KEY_B: u32 = 48;
pub const KEY_LEFTMETA: u32 = 125;

/// An action triggered by a matched compositor key binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    FocusNext,
    Cancel,
    /// F-FIT-1 on the pointer's output, for the element under the pointer (INC-10).
    FitViewport,
    /// Restore (M8, OBJ-30) on the pointer's output (INC-10).
    ViewportRestore,
    Noop,
}

/// A compositor key binding specification.
#[derive(Clone, Copy, Debug)]
pub struct KeyBinding {
    pub evdev: u32,
    pub needs_logo: bool,
    pub action: KeyAction,
}

/// Canonical key bindings for Foundation 01.
pub const CANONICAL_BINDINGS: [KeyBinding; 1] = [KeyBinding {
    evdev: KEY_ENTER,
    needs_logo: true,
    action: KeyAction::FocusNext,
}];

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
    if key == KEY_F && mods.logo && !mods.ctrl && !mods.alt && !interaction_active {
        return Some(if mods.shift {
            KeyAction::ViewportRestore
        } else {
            KeyAction::FitViewport
        });
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

    #[test]
    fn binding_resolution_honours_logo_modifier() {
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let none = Modifiers::default();

        assert_eq!(
            resolve_key_binding(KEY_ENTER, logo, false),
            Some(KeyAction::FocusNext)
        );
        assert_eq!(resolve_key_binding(KEY_ENTER, none, false), None);
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
    fn navigation_bindings_resolve_fit_and_restore() {
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
            resolve_key_binding(KEY_F, logo, false),
            Some(KeyAction::FitViewport)
        );
        assert_eq!(
            resolve_key_binding(KEY_F, logo_shift, false),
            Some(KeyAction::ViewportRestore)
        );
        assert_eq!(
            resolve_key_binding(KEY_F, Modifiers::default(), false),
            None
        );
        assert_eq!(resolve_key_binding(KEY_F, shift, false), None);
        assert_eq!(resolve_key_binding(KEY_F, logo, true), None);
        assert_eq!(resolve_key_binding(KEY_F, logo_shift, true), None);
    }

    /// INC-10 review L5: the navigation bindings match their modifiers exactly — Logo+Ctrl+F, Logo+Alt+F and
    /// their Shift forms resolve to nothing (the key is then the client's, M12).
    #[test]
    fn navigation_bindings_reject_extra_modifiers() {
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
                        (false, false) => Some(KeyAction::FitViewport),
                        (false, true) => Some(KeyAction::ViewportRestore),
                    };
                    assert_eq!(resolve_key_binding(KEY_F, mods, false), want, "{mods:?}");
                }
            }
        }
    }
}
