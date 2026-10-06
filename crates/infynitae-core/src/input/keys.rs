//! Key bindings and physical-key evaluation (C4.4, M12).
//!
//! Evaluated during Smithay's two-phase `input_intercept` dispatch:
//! - K1: press ownership decided at press;
//! - K2: release follows the press owner;
//! - K3: keys that change XKB modifier or lock state are never compositor-owned (observe mode).

use crate::input::bindings::Modifiers;

pub const KEY_ESC: u32 = 1;
pub const KEY_ENTER: u32 = 28;
pub const KEY_A: u32 = 30;
pub const KEY_LEFTSHIFT: u32 = 42;
pub const KEY_B: u32 = 48;
pub const KEY_LEFTMETA: u32 = 125;

/// An action triggered by a matched compositor key binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    FocusNext,
    Cancel,
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
}
