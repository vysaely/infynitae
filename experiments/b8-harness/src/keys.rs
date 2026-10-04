//! X-10: keyboard ownership model under test (C4 rules K1–K3), on Smithay 0.7.0's real seat
//! keyboard (one XKB state per seat).
//!
//!  K1  ownership is decided at PRESS with `input_intercept` (updates XKB, runs the binding
//!      filter), then either consumed (compositor-owned) or `input_forward`ed (client-owned).
//!  K2  a RELEASE follows its press's owner (consumed, or forwarded to the CURRENT focus).
//!  K3  a key whose press changes XKB modifier/lock state is never compositor-owned: if a binding
//!      matches such a key it fires in observe mode and the key is still forwarded.
//!
//! `k3_enforced = false` is a NEGATIVE CONTROL that intercepts modifier bindings to show the
//! failure K3 prevents. Nothing here synthesizes key events.

use std::collections::BTreeSet;

use smithay::{
    backend::input::KeyState,
    input::keyboard::Keycode,
    utils::SERIAL_COUNTER,
};

use crate::server::Server;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    FocusNext,
    Cancel,
    Noop,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyBinding {
    pub evdev: u32,
    pub needs_logo: bool,
    pub action: Action,
}

pub struct KeyIntake {
    pub bindings: Vec<KeyBinding>,
    pub k3_enforced: bool,
    /// physically held keys (ground truth, evdev codes)
    pub held: BTreeSet<u32>,
    /// keys whose press was consumed by the compositor
    pub owned: BTreeSet<u32>,
    pub log: Vec<(u64, String)>,
    time: u32,
}

impl KeyIntake {
    pub fn new(bindings: Vec<KeyBinding>, k3_enforced: bool) -> Self {
        KeyIntake { bindings, k3_enforced, held: BTreeSet::new(), owned: BTreeSet::new(), log: Vec::new(), time: 0 }
    }

    /// One physical key event. Returns the action of a matched binding, if any.
    pub fn key(&mut self, srv: &mut Server, evdev: u32, pressed: bool) -> Option<Action> {
        self.time += 1;
        let kb = srv.seat.get_keyboard().expect("keyboard");
        let code = Keycode::new(evdev + 8);
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        if pressed {
            self.held.insert(evdev);
        } else {
            self.held.remove(&evdev);
        }
        let bindings = self.bindings.clone();
        let (matched, mods_changed) = kb.input_intercept(srv, code, state, |_, mods, _sym| {
            if !pressed {
                return None;
            }
            bindings.iter().find(|b| b.evdev == evdev && (!b.needs_logo || mods.logo)).map(|b| b.action)
        });
        let now = srv.now_us;
        if pressed {
            match matched {
                Some(a) if !(mods_changed && self.k3_enforced) => {
                    // K1: compositor-owned
                    self.owned.insert(evdev);
                    self.log.push((now, format!("intake: press {evdev} -> binding {a:?}: CONSUMED (compositor-owned)")));
                    return Some(a);
                }
                Some(a) => {
                    // K3: modifier-changing key -> observe mode, still forwarded
                    self.log.push((now, format!("intake: press {evdev} -> binding {a:?} on a MODIFIER key: observe mode, FORWARDED (K3)")));
                    kb.input_forward(srv, code, state, SERIAL_COUNTER.next_serial(), self.time, mods_changed);
                    return Some(a);
                }
                None => {
                    kb.input_forward(srv, code, state, SERIAL_COUNTER.next_serial(), self.time, mods_changed);
                    return None;
                }
            }
        }
        // K2: release follows the owner of the press
        if self.owned.remove(&evdev) {
            self.log.push((now, format!("intake: release {evdev}: CONSUMED (compositor-owned press)")));
        } else {
            kb.input_forward(srv, code, state, SERIAL_COUNTER.next_serial(), self.time, mods_changed);
        }
        None
    }
}
