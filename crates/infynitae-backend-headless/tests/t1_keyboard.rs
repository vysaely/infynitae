//! INC-4 T1: Keyboard ownership (K1–K9), ActiveElementFocus, and key bindings on the production path
//! (TST-07 / x10, OB-15).
//!
//! Evaluates the production Core, Wayland adapter, and headless backend with a recording client probe.
//! Invariants verified after every step:
//! - KV-1: No wl_keyboard protocol violation (enter/leave/key/modifiers MUST / MUST NOT rules).
//! - KV-2: If client has focus: client logical keys == physically held client-owned keys.
//! - KV-3: If client has focus: client (depressed, latched, locked) == seat keyboard XKB modifier state.
//! - KV-4: At scenario end (no physical key held): client holds no keys and no depressed/latched modifiers.

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use common::{Lockstep, output};
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_A, KEY_B, KEY_ENTER, KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT};
use infynitae_core::input::ownership::Owner;
use infynitae_core::stage::ElementId;

struct S {
    ls: Lockstep,
    time_us: u64,
    _w1: usize,
    w2: usize,
    e1: ElementId,
    e2: ElementId,
    held: BTreeSet<u32>,
}

impl S {
    fn new(k3_enforced: bool) -> Self {
        let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
        ls.state.set_k3_enforced(k3_enforced);
        let qh = ls.qh.clone();
        let w1 = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
        let w2 = ls.probe.toplevel(&qh, "W2", (400, 300), 0);
        ls.pump();
        let elements: Vec<ElementId> = ls.state.elements().collect();
        assert_eq!(elements.len(), 2);
        let e1 = elements[0];
        let e2 = elements[1];
        let mut s = Self {
            ls,
            time_us: 10_000,
            _w1: w1,
            w2,
            e1,
            e2,
            held: BTreeSet::new(),
        };
        s.set_focus(Some(e1));
        s
    }

    fn advance(&mut self) {
        self.ls.pump();
        let t = self.ls.now + Duration::from_micros(1000);
        self.ls.run_until(t);
    }

    fn key(&mut self, k: u32, pressed: bool) {
        self.time_us += 1000;
        if pressed {
            self.held.insert(k);
        } else {
            self.held.remove(&k);
        }
        keyboard_key(
            &mut self.ls.state,
            &mut self.ls.input,
            k,
            pressed,
            self.time_us,
        )
        .expect("keyboard_key");
        self.advance();
        self.check(&format!("key {k} {}", if pressed { "down" } else { "up" }));
    }

    fn set_focus(&mut self, target: Option<ElementId>) {
        self.ls.state.set_focus(target);
        self.advance();
        self.check(&format!("set_focus {:?}", target));
    }

    fn check(&self, step: &str) {
        let kb = self.ls.state.keyboard_handle();
        let seat_mods = kb.modifier_state().serialized;
        let c = &self.ls.probe.kb;
        assert!(
            c.violations.is_empty(),
            "KV-1 protocol violations at {step}: {:?}",
            c.violations
        );
        let client_owned_held: BTreeSet<u32> = self
            .held
            .iter()
            .copied()
            .filter(|&k| self.ls.state.core().key_ownership().owner(k) == Some(Owner::Client))
            .collect();
        if c.active.is_some() {
            assert_eq!(
                c.keys, client_owned_held,
                "KV-2 client keys != client-owned held keys at {step}"
            );
            assert_eq!(
                (c.depressed, c.latched, c.locked),
                (seat_mods.depressed, seat_mods.latched, seat_mods.locked),
                "KV-3 client modifiers != seat modifiers at {step}"
            );
        }
    }

    fn assert_end(&self) {
        let c = &self.ls.probe.kb;
        assert!(
            self.held.is_empty(),
            "KV-4 end state: physical keys still held: {:?}",
            self.held
        );
        assert!(
            c.keys.is_empty(),
            "KV-4 end state: client keys still held: {:?}",
            c.keys
        );
        assert_eq!(
            c.depressed, 0,
            "KV-4 end state: client depressed mods != 0: {:#x}",
            c.depressed
        );
        assert_eq!(
            c.latched, 0,
            "KV-4 end state: client latched mods != 0: {:#x}",
            c.latched
        );
    }
}

/// K-1: Ordinary client-owned press and release.
#[test]
fn k1_ordinary_client_owned_press_release() {
    let mut s = S::new(true);
    s.key(KEY_A, true);
    s.key(KEY_A, false);
    s.assert_end();
}

/// K-2: Modifier held while another client key is pressed.
#[test]
fn k2_modifier_held_while_another_client_key_pressed() {
    let mut s = S::new(true);
    s.key(KEY_LEFTMETA, true);
    s.key(KEY_A, true);
    s.key(KEY_A, false);
    s.key(KEY_LEFTMETA, false);
    s.assert_end();
}

/// K-3: Compositor binding (Super+B) while modifier held: consumed at press, release consumed.
#[test]
fn k3_compositor_binding_super_b_consumed() {
    let mut s = S::new(true);
    s.key(KEY_LEFTMETA, true);
    let events_before = s.ls.probe.key_events.len();
    s.key(KEY_B, true);
    s.key(KEY_B, false);
    // Client never received B press or release
    assert_eq!(s.ls.probe.key_events.len(), events_before);
    s.key(KEY_LEFTMETA, false);
    s.assert_end();
}

/// K-4: Focus transition (binding Super+Enter = FocusNext) while modifier held.
#[test]
fn k4_focus_transition_binding_super_enter_while_modifier_held() {
    let mut s = S::new(true);
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W1"));
    s.key(KEY_LEFTMETA, true);
    // Super+Enter matches FocusNext binding
    s.key(KEY_ENTER, true);
    // Focus automatically transitioned to W2 via apply_key_action(FocusNext)
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W2"));
    s.key(KEY_ENTER, false);
    s.key(KEY_LEFTMETA, false);
    s.assert_end();
}

/// K-5: Focus transition while a client key is held (forwarded via enter(keys) to new focus).
#[test]
fn k5_focus_transition_while_client_key_held() {
    let mut s = S::new(true);
    s.key(KEY_A, true);
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W1"));
    assert!(s.ls.probe.kb.keys.contains(&KEY_A));

    // Core focus command to W2
    s.set_focus(Some(s.e2));
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W2"));
    assert!(s.ls.probe.kb.keys.contains(&KEY_A), "A was entered into W2");

    // Release A goes to W2
    s.key(KEY_A, false);
    assert!(s.ls.probe.kb.keys.is_empty());
    s.assert_end();
}

/// K-6: Focused client destroyed while Shift+A are held -> successor gets focus.
#[test]
fn k6_focused_client_destroyed_while_shift_a_held() {
    let mut s = S::new(true);
    s.set_focus(Some(s.e2));
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W2"));

    s.key(KEY_LEFTSHIFT, true);
    s.key(KEY_A, true);
    assert_eq!(s.ls.probe.kb.keys.len(), 2);

    // Client tears down W2
    s.ls.probe.destroy_toplevel(s.w2);
    s.advance();

    // Successor W1 should now have focus with Shift and A in keys
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W1"));
    assert!(s.ls.probe.kb.keys.contains(&KEY_A));
    assert!(s.ls.probe.kb.keys.contains(&KEY_LEFTSHIFT));

    // Release keys to successor
    s.key(KEY_A, false);
    s.key(KEY_LEFTSHIFT, false);
    s.assert_end();
}

/// K-7: Interaction cancelled while compositor-owned keys are held; focus changes meanwhile.
#[test]
fn k7_interaction_cancelled_while_compositor_keys_held() {
    let mut s = S::new(true);
    s.key(KEY_LEFTMETA, true);
    s.key(KEY_B, true); // compositor-owned (Noop)
    s.key(KEY_ESC, true); // compositor-owned (Cancel)
    s.key(KEY_ENTER, true); // FocusNext
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W2"));
    s.key(KEY_ENTER, false);
    s.key(KEY_B, false);
    s.key(KEY_ESC, false);
    s.key(KEY_LEFTMETA, false);
    s.assert_end();
}

/// K-8: Binding on a MODIFIER key (Shift) with K3: observe mode; focus change while held.
#[test]
fn k8_binding_on_modifier_key_with_k3_observe_mode() {
    let mut s = S::new(true);
    s.key(KEY_LEFTSHIFT, true);
    // K3 observe mode: client received Shift press
    assert!(s.ls.probe.kb.keys.contains(&KEY_LEFTSHIFT));
    assert_ne!(s.ls.probe.kb.depressed, 0);

    // Focus change to W2
    s.set_focus(Some(s.e2));
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W2"));
    assert!(s.ls.probe.kb.keys.contains(&KEY_LEFTSHIFT));
    assert_ne!(s.ls.probe.kb.depressed, 0);

    s.key(KEY_LEFTSHIFT, false);
    s.assert_end();
}

/// K-9: Keyboard focus lost (no target) while keys are held, then regained.
#[test]
fn k9_keyboard_focus_lost_and_regained() {
    let mut s = S::new(true);
    s.key(KEY_LEFTSHIFT, true);
    s.key(KEY_A, true);

    // Focus loss (e.g. unmapped, no successor)
    s.set_focus(None);
    assert_eq!(s.ls.probe.kb.active, None);

    // Physical release of A while focus is None: dropped
    s.key(KEY_A, false);

    // Focus regained on W1
    s.set_focus(Some(s.e1));
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W1"));
    // Only Shift is currently held
    assert_eq!(
        s.ls.probe.kb.keys,
        [KEY_LEFTSHIFT].into_iter().collect::<BTreeSet<_>>()
    );

    s.key(KEY_LEFTSHIFT, false);
    s.assert_end();
}

/// NEGATIVE CONTROL: K-8 with K3 DISABLED.
/// Modifier binding is intercepted and consumed, causing stuck modifier / KV-3 failure.
#[test]
fn neg_k3_without_k3_stuck_modifier_control() {
    let mut s = S::new(false);
    s.time_us += 1000;
    s.held.insert(KEY_LEFTSHIFT);
    keyboard_key(
        &mut s.ls.state,
        &mut s.ls.input,
        KEY_LEFTSHIFT,
        true,
        s.time_us,
    )
    .expect("keyboard_key");
    s.advance();

    // With K3 disabled, the seat's XKB state registered the modifier:
    let seat_depressed =
        s.ls.state
            .keyboard_handle()
            .modifier_state()
            .serialized
            .depressed;
    assert_ne!(
        seat_depressed, 0,
        "Seat XKB registered Shift modifier press"
    );

    // But because the compositor consumed the press, the client never received it:
    assert_eq!(
        s.ls.probe.kb.depressed, 0,
        "Client was starved of modifier update: KV-3 violation"
    );
    assert_ne!(
        s.ls.probe.kb.depressed, seat_depressed,
        "Negative control verifies modifier divergence when K3 is disabled"
    );
}

/// K8 (OB-15): Output removal under held keys: keyboard focus is output-independent.
#[test]
fn k8_ob15_output_removal_under_held_keys() {
    let mut s = S::new(true);
    s.key(KEY_A, true);
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W1"));
    assert!(s.ls.probe.kb.keys.contains(&KEY_A));

    // Remove output
    let out_id = s.ls.output();
    s.ls.disconnect(out_id);
    s.advance();

    // Keyboard focus and held keys remain undisturbed
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W1"));
    assert!(s.ls.probe.kb.keys.contains(&KEY_A));

    s.key(KEY_A, false);
    s.assert_end();
}

/// K9 (OB-15): Key repeat probe: compositor sends 0 repeat events, announces rate/delay.
#[test]
fn k9_ob15_key_repeat_probe() {
    let mut s = S::new(true);
    // Protocol announced repeat rate and delay
    assert_eq!(s.ls.probe.repeat_info, Some((25, 200)));

    let keys_before = s.ls.probe.key_events.len();
    s.key(KEY_A, true);
    assert_eq!(s.ls.probe.key_events.len(), keys_before + 1);

    // Advance time by 500 ms (past the 200 ms delay and multiple 25 Hz intervals)
    let t = s.ls.now + Duration::from_millis(500);
    s.ls.run_until(t);

    // Compositor synthesized zero repeats
    assert_eq!(
        s.ls.probe.key_events.len(),
        keys_before + 1,
        "No repeat events synthesized by compositor"
    );

    s.key(KEY_A, false);
    s.assert_end();
}
