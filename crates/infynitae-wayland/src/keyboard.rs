//! Adapter keyboard intake and ownership dispatch (C4.4, M12).
//!
//! Two-phase dispatch using Smithay's `input_intercept` and `input_forward`:
//! - K1: press ownership decided at press;
//! - K2: release follows press owner;
//! - K3: keys that change XKB modifier/lock state are never compositor-owned (observe mode).
//!
//! Ordered preemption (INV-INPUT-10, M3, M11):
//! ActiveElementFocus transitions cleanly tear down popup grabs in one atomic batch:
//! 1. ungrab(All) -> popup_done topmost first;
//! 2. keyboard.unset_grab;
//! 3. pointer.unset_grab;
//! 4. keyboard.set_focus(target).

use infynitae_core::input::keys::KeyAction;
use infynitae_core::input::ownership::Owner;
use smithay::backend::input::KeyState;
use smithay::input::keyboard::{KeyboardHandle, Keycode};
use smithay::utils::SERIAL_COUNTER;

use crate::{WaylandState, convert};

/// Preemption strategies for popup grabs under focus changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PreemptionStrategy {
    /// Canonical ORDERED preemption (INV-INPUT-10): ungrab -> kb.unset -> ptr.unset -> set_focus.
    #[default]
    Ordered,
    /// NEG-1 negative control: naive set_focus without ungrabbing popup.
    Naive,
    /// NEG-2 negative control: pointer unset before keyboard unset (W-SM-15 violation).
    PointerFirst,
}

impl WaylandState {
    /// The seat's keyboard handle.
    pub fn keyboard_handle(&self) -> KeyboardHandle<WaylandState> {
        self.seat.get_keyboard().expect("the seat has a keyboard")
    }

    /// Dispatches one physical key event through Smithay's seat keyboard (K1–K3).
    pub fn keyboard_key(&mut self, key: u32, pressed: bool, time_us: u64) {
        let kb = self.keyboard_handle();
        let code = Keycode::new(key + 8);
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        let (matched, mods_changed) = kb.input_intercept(self, code, state, |state, mods, _sym| {
            if !pressed {
                return None;
            }
            let modifiers = convert::modifiers_from_smithay(mods);
            state.core.resolve_key_binding(key, modifiers)
        });
        let time_ms = (time_us / 1000) as u32;
        if pressed {
            match matched {
                Some(action) if !(mods_changed && self.k3_enforced) => {
                    // K1: compositor-owned press is consumed
                    self.core.key_ownership_mut().press(key, Owner::Compositor);
                    self.apply_key_action(action);
                }
                Some(action) => {
                    // K3: observe mode for modifier-changing key: client-owned
                    self.core.key_ownership_mut().press(key, Owner::Client);
                    self.apply_key_action(action);
                    kb.input_forward(
                        self,
                        code,
                        state,
                        SERIAL_COUNTER.next_serial(),
                        time_ms,
                        mods_changed,
                    );
                }
                None => {
                    // Client-owned press: forwarded
                    self.core.key_ownership_mut().press(key, Owner::Client);
                    kb.input_forward(
                        self,
                        code,
                        state,
                        SERIAL_COUNTER.next_serial(),
                        time_ms,
                        mods_changed,
                    );
                }
            }
        } else {
            // K2: release follows the owner of the press
            if self.core.key_ownership_mut().release(key) == Some(Owner::Compositor) {
                // Compositor-owned release is consumed
            } else {
                // Client-owned release is forwarded to the current focus
                kb.input_forward(
                    self,
                    code,
                    state,
                    SERIAL_COUNTER.next_serial(),
                    time_ms,
                    mods_changed,
                );
            }
        }
    }

    fn apply_key_action(&mut self, action: KeyAction) {
        match action {
            KeyAction::FocusNext => {
                let fx = self.core.focus_next();
                self.apply(&fx);
            }
            KeyAction::Cancel => {
                // Compositor interaction cancellation (K6)
            }
            KeyAction::Noop => {}
        }
    }

    /// Sets whether K3 observe mode is enforced (default: true). False is the NEG control.
    pub fn set_k3_enforced(&mut self, enforced: bool) {
        self.k3_enforced = enforced;
    }

    /// Sets whether nested popup grabs preserve the installed pointer grab (W-SM-14).
    pub fn set_nested_grab_keep_pointer(&mut self, keep: bool) {
        self.nested_grab_keep_pointer = keep;
    }

    /// Sets the preemption strategy under focus changes (default: Ordered).
    pub fn set_preemption_strategy(&mut self, strategy: PreemptionStrategy) {
        self.ordered_preemption_strategy = strategy;
    }

    /// Sets keyboard focus directly via Core (ActiveElementFocus) and applies resulting effects.
    pub fn set_focus(&mut self, target: Option<infynitae_core::stage::ElementId>) {
        let fx = self.core.set_focus(target);
        self.apply(&fx);
    }
}
