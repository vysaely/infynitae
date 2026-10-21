//! Binding resolution (C4.2 P2): decided at press, before any client dispatch, from the pressed
//! button and the modifier snapshot carried by the event. Pure: no state.
//!
//! Core keeps no XKB copy (C4.4). The modifier state is a per-event snapshot the backend supplies
//! (from INC-4 the adapter reads it from Smithay's `KeyboardHandle`).

/// `BTN_MIDDLE` (linux/input-event-codes.h).
pub const BTN_MIDDLE: u32 = 0x112;
/// `BTN_LEFT` and `BTN_RIGHT` (linux/input-event-codes.h).
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;

/// What a Super+button press over a window starts (owner request 2026-10-22, "like Hyprland"):
/// exactly Logo held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowDrag {
    Move,
    Resize,
}

pub fn window_drag_binding(button: u32, mods: Modifiers) -> Option<WindowDrag> {
    if !mods.logo || mods.ctrl || mods.alt || mods.shift {
        return None;
    }
    match button {
        BTN_LEFT => Some(WindowDrag::Move),
        BTN_RIGHT => Some(WindowDrag::Resize),
        _ => None,
    }
}

/// The modifier snapshot of one physical event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub logo: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The camera-pan binding (x8, EB-13): the middle button with Logo held. Not configurable in
/// INC-3.
pub const PAN_BINDING: (u32, Modifiers) = (
    BTN_MIDDLE,
    Modifiers {
        logo: true,
        ctrl: false,
        alt: false,
        shift: false,
    },
);

/// Whether a press names the camera-pan binding: the button matches and every required modifier
/// is held (extra modifiers do not prevent the match).
pub fn is_pan_binding(button: u32, mods: Modifiers) -> bool {
    let (b, need) = PAN_BINDING;
    button == b
        && (!need.logo || mods.logo)
        && (!need.ctrl || mods.ctrl)
        && (!need.alt || mods.alt)
        && (!need.shift || mods.shift)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pan_binding_needs_the_middle_button_and_logo() {
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        assert!(is_pan_binding(0x112, logo));
        assert!(is_pan_binding(
            0x112,
            Modifiers {
                shift: true,
                ..logo
            }
        ));
        assert!(!is_pan_binding(0x112, Modifiers::default()));
        assert!(!is_pan_binding(0x110, logo));
    }
}
