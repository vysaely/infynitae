//! X11 apps hear chosen keys while a Wayland window has focus (XWAYLAND_KEYS), as KWin's "legacy
//! X11 app support" does: push-to-talk in Discord or TeamSpeak on X11, a game's or OBS's global
//! hotkeys.
//!
//! Every X11 window is a surface of xwayland-satellite's one connection. A key is sent to that
//! connection's keyboards without focus; the satellite passes it to Xwayland, which queues it
//! whatever its focus. X focus is None while no X11 window has Wayland focus, so the key reaches
//! only what listens globally (a key grab on the root window, XInput2 raw events, XRecord) and
//! is never typed into a background X11 window.
//!
//! Wayland keeps apps from reading what is typed into other apps; X11 had no such wall. So the
//! default passes no key that types a character, and never one while the screen is locked.

use std::collections::HashSet;

use infynitae_core::input::bindings::Modifiers;
use smithay::reexports::wayland_server::protocol::wl_keyboard::KeyState;
use smithay::reexports::wayland_server::{Client, Resource};

use crate::WaylandState;

/// Which keys X11 apps hear while another window has focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// None.
    Off,
    /// Keys that type nothing: modifiers alone, F1–F24, media and system keys.
    #[default]
    Modifiers,
    /// Those, and any key while Ctrl, Alt or Super is held.
    Shortcuts,
    /// Every key (an X11 app can then read what is typed anywhere).
    All,
}

impl Mode {
    /// XWAYLAND_KEYS's value; anything else is the default.
    pub fn parse(val: &str) -> Mode {
        match val.to_lowercase().as_str() {
            "off" | "none" | "false" | "0" | "no" => Mode::Off,
            "shortcuts" => Mode::Shortcuts,
            "all" => Mode::All,
            _ => Mode::Modifiers,
        }
    }

    /// Whether X11 apps hear `key` (evdev) pressed with `mods` held.
    pub fn passes(self, key: u32, mods: Modifiers) -> bool {
        match self {
            Mode::Off => false,
            Mode::Modifiers => types_nothing(key),
            Mode::Shortcuts => types_nothing(key) || mods.ctrl || mods.alt || mods.logo,
            Mode::All => true,
        }
    }
}

/// A key that types no character (evdev codes): the modifiers, function, media and system keys.
fn types_nothing(key: u32) -> bool {
    matches!(
        key,
        29 | 42 | 54 | 56 | 97 | 100 | 125 | 126 // Ctrl, Shift, Alt, Super (left and right)
        | 59..=68 | 87 | 88 // F1–F12
        | 183..=194 // F13–F24
        | 70 | 99 | 110 | 119 | 127 // Scroll Lock, Print, Insert, Pause, Menu
        | 113..=116 // mute, volume down, volume up, power
        | 163..=166 // next, play/pause, previous, stop
        | 200 | 201 // play, pause
    )
}

/// What the session knows for XWAYLAND_KEYS.
#[derive(Debug, Default)]
pub(crate) struct X11Keys {
    pub(crate) mode: Mode,
    /// xwayland-satellite's connection, once it made a surface.
    pub(crate) client: Option<Client>,
    /// Keys whose press went to X11 this way: their release goes there too, whatever the mode
    /// or the focus says by then, so no key stays held in X11.
    pressed: HashSet<u32>,
}

impl WaylandState {
    /// Notes `client` as xwayland-satellite's (its surfaces are X11 windows).
    pub(crate) fn note_x11_client(&mut self, client: &Client) {
        if self.x11_keys.client.as_ref() != Some(client) {
            self.x11_keys.client = Some(client.clone());
        }
    }

    /// `key` was pressed or released for the focused window: X11 apps hear it too when
    /// XWAYLAND_KEYS passes it, the focus is no X11 window and the screen is unlocked.
    pub(crate) fn forward_key_to_x11(
        &mut self,
        key: u32,
        pressed: bool,
        mods: Modifiers,
        time: u32,
    ) {
        let Some(client) = self.x11_keys.client.clone() else {
            return;
        };
        let kb = self.keyboard_handle();
        let focus_is_x11 = kb
            .current_focus()
            .and_then(|f| f.client())
            .is_some_and(|c| c == client);
        let send = if pressed {
            !focus_is_x11 && !self.is_locked() && self.x11_keys.mode.passes(key, mods)
        } else {
            // the X11 focus had it already (the satellite's own keyboard got this release)
            self.x11_keys.pressed.remove(&key) && !focus_is_x11
        };
        if !send {
            return;
        }
        if pressed {
            self.x11_keys.pressed.insert(key);
        }
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        for keyboard in kb.client_keyboards(&client) {
            keyboard.key(serial.into(), time, key, state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        logo: false,
        ctrl: false,
        alt: false,
        shift: false,
    };

    #[test]
    fn the_default_passes_no_character() {
        let m = Mode::default();
        assert_eq!(m, Mode::Modifiers);
        assert!(m.passes(54, NONE), "Right Shift alone (push-to-talk)");
        assert!(m.passes(97, NONE), "Right Ctrl alone");
        assert!(m.passes(67, NONE), "F9");
        assert!(m.passes(185, NONE), "F15");
        assert!(!m.passes(30, NONE), "a");
        assert!(!m.passes(57, NONE), "space");
        let ctrl = Modifiers { ctrl: true, ..NONE };
        assert!(!m.passes(30, ctrl), "Ctrl+A only in shortcuts");
        let shift = Modifiers {
            shift: true,
            ..NONE
        };
        assert!(!m.passes(30, shift), "a capital letter is a character");
    }

    #[test]
    fn modes() {
        let ctrl = Modifiers { ctrl: true, ..NONE };
        let shift = Modifiers {
            shift: true,
            ..NONE
        };
        assert!(Mode::parse("shortcuts").passes(50, ctrl), "Ctrl+M");
        assert!(
            !Mode::parse("shortcuts").passes(50, shift),
            "Shift+M types M"
        );
        assert!(Mode::parse("ALL").passes(30, NONE));
        assert!(!Mode::parse("off").passes(54, NONE));
        assert_eq!(Mode::parse("bogus"), Mode::Modifiers);
    }
}
