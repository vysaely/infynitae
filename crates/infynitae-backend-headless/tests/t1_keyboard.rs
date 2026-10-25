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

/// The motion that takes the startup pointer to local (50, 50) of a 400x300 window. The pointer
/// starts at the view centre, and a window is centred there (F-PLACE-1): its local (200, 150).
const TO_LOCAL_50_50: (f64, f64) = (-150.0, -100.0);

use std::collections::BTreeSet;
use std::time::Duration;

use common::input::{BTN_MIDDLE, logo, none};
use common::{Lockstep, output};
use infynitae_backend_headless::{keyboard_key, pointer_button, pointer_motion, set_focus};
use infynitae_core::input::keys::{KEY_A, KEY_B, KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_TAB};
use infynitae_core::stage::ElementId;

struct S {
    ls: Lockstep,
    time_us: u64,
    _w1: usize,
    w2: usize,
    e1: ElementId,
    e2: ElementId,
    held: BTreeSet<u32>,
    client_held: BTreeSet<u32>,
    /// Reference model (KV-2): a CameraPan is in progress according to the test's own sequence
    /// (Super held at the MMB press, until the MMB release or an Escape cancel) — never read
    /// back from Core.
    pan: bool,
}

impl S {
    fn new(_k3_enforced: bool) -> Self {
        let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
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
            client_held: BTreeSet::new(),
            pan: false,
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
            let already_compositor = self.held.contains(&k) && !self.client_held.contains(&k);
            self.held.insert(k);
            let super_down = self.held.contains(&KEY_LEFTMETA);
            let is_compositor = already_compositor
                || (k == KEY_ESC && self.pan)
                || (super_down && !self.held.contains(&KEY_LEFTSHIFT) && k == KEY_TAB);
            if !is_compositor {
                self.client_held.insert(k);
            }
            if k == KEY_ESC && self.pan && !already_compositor {
                self.pan = false; // K6: Escape cancels the interaction
            }
        } else {
            self.held.remove(&k);
            self.client_held.remove(&k);
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

    /// The middle button; the model starts a pan on a press with Super held (the CameraPan chord).
    fn mmb(&mut self, pressed: bool) {
        self.time_us += 1000;
        if pressed {
            self.pan = self.held.contains(&KEY_LEFTMETA);
        } else {
            self.pan = false;
        }
        pointer_button(
            &mut self.ls.state,
            &mut self.ls.input,
            BTN_MIDDLE,
            pressed,
            none(),
            self.time_us,
        )
        .expect("middle button");
        self.advance();
    }

    fn motion(&mut self, d: (f64, f64)) {
        self.time_us += 1000;
        pointer_motion(&mut self.ls.state, &mut self.ls.input, d, d, self.time_us).expect("motion");
        self.advance();
    }

    fn set_focus(&mut self, target: Option<ElementId>) {
        set_focus(&mut self.ls.state, &mut self.ls.input, target).expect("set_focus");
        self.advance();
        self.check(&format!("set_focus {:?}", target));
    }

    fn check(&self, step: &str) {
        let seat_mods = self.ls.state.keyboard_modifier_state().serialized;
        let c = &self.ls.probe.kb;
        assert!(
            c.violations.is_empty(),
            "KV-1 protocol violations at {step}: {:?}",
            c.violations
        );
        if c.active.is_some() {
            assert_eq!(
                c.keys, self.client_held,
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
    assert!(
        s.ls.probe.kb.keys.contains(&KEY_A),
        "client did not receive press"
    );
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

/// K-3: Compositor binding (Super+Tab) while modifier held: consumed at press, release consumed.
#[test]
fn k3_compositor_binding_super_enter_consumed() {
    let mut s = S::new(true);
    s.key(KEY_LEFTMETA, true);
    let events_before = s.ls.probe.key_events.len();
    s.key(KEY_TAB, true);
    s.key(KEY_TAB, false);
    // Client never received the bound key's press or release
    assert_eq!(s.ls.probe.key_events.len(), events_before);
    s.key(KEY_LEFTMETA, false);
    s.assert_end();
}

/// K-4: Focus transition (binding Super+Tab = FocusNext) while modifier held.
#[test]
fn k4_focus_transition_binding_super_enter_while_modifier_held() {
    let mut s = S::new(true);
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W1"));
    s.key(KEY_LEFTMETA, true);
    // Super+Tab matches FocusNext binding
    s.key(KEY_TAB, true);
    // Focus automatically transitioned to W2 via apply_key_action(FocusNext)
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W2"));
    s.key(KEY_TAB, false);
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
    s.key(KEY_ESC, true); // compositor-owned (Cancel)
    s.key(KEY_TAB, true); // FocusNext
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W2"));
    s.key(KEY_TAB, false);
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

const ACTIVATED: u32 = 4;

fn last_activated(ls: &Lockstep, idx: usize) -> Option<bool> {
    ls.probe
        .toplevel_states
        .iter()
        .rev()
        .find(|(i, _)| *i == idx)
        .map(|(_, s)| s.contains(&ACTIVATED))
}

/// ATK-01 (product policy / gaming, F04): Escape with no interaction active is a client key.
#[test]
fn plain_escape_reaches_the_focused_client() {
    let mut s = S::new(true);
    let n = s.ls.probe.key_events.len();
    s.key(KEY_ESC, true);
    s.key(KEY_ESC, false);
    let got: Vec<_> = s.ls.probe.key_events[n..].to_vec();
    assert_eq!(
        got,
        vec![(KEY_ESC, true), (KEY_ESC, false)],
        "Escape swallowed by compositor; kb_log={:?}",
        s.ls.probe.kb_log
    );
    s.assert_end();
}

/// Plain Escape cancels an active interaction (F04, K6).
#[test]
fn plain_escape_cancels_active_interaction() {
    let mut s = S::new(true);
    s.key(KEY_LEFTMETA, true);
    s.mmb(true);
    assert!(
        s.ls.state.core().interaction().active(),
        "CameraPan should be active"
    );
    s.key(KEY_ESC, true);
    assert!(
        !s.ls.state.core().interaction().active(),
        "interaction cancelled by plain Escape"
    );
    assert!(
        !s.ls.probe.kb.keys.contains(&KEY_ESC),
        "Escape was consumed by compositor to cancel interaction"
    );
    s.key(KEY_ESC, false);
    s.mmb(false);
    s.key(KEY_LEFTMETA, false);
    s.assert_end();
}

/// ATK-02 (K1/K2, F03): a repeated press of a compositor-owned key must not leak.
#[test]
fn repeat_press_of_compositor_owned_key_does_not_leak() {
    let mut s = S::new(true);
    s.key(KEY_LEFTMETA, true);
    s.key(KEY_TAB, true); // Super+Tab: compositor-owned
    s.key(KEY_LEFTMETA, false);
    s.key(KEY_TAB, true); // repeat / second press
    s.key(KEY_TAB, false);
    let enter_events: Vec<_> =
        s.ls.probe
            .key_events
            .iter()
            .filter(|(k, _)| *k == KEY_TAB)
            .collect();
    assert!(
        enter_events.is_empty(),
        "compositor-owned Enter leaked to client: {enter_events:?}; viol={:?}",
        s.ls.probe.kb.violations
    );
    assert!(
        s.ls.probe.kb.keys.is_empty(),
        "stuck keys {:?}",
        s.ls.probe.kb.keys
    );
}

/// ATK-03 (K2): release follows the press owner even when modifier state changed in between.
#[test]
fn release_follows_press_owner_after_modifier_change() {
    let mut s = S::new(true);
    s.key(KEY_B, true); // client-owned (no Super)
    s.key(KEY_LEFTMETA, true);
    s.key(KEY_B, false); // release follows client owner
    s.key(KEY_LEFTMETA, false);
    assert!(
        s.ls.probe.kb.violations.is_empty(),
        "{:?}",
        s.ls.probe.kb.violations
    );
    assert!(
        s.ls.probe.kb.keys.is_empty(),
        "client left holding {:?}",
        s.ls.probe.kb.keys
    );
}

/// ATK-04 (K2/protocol must-not, F03): a release with no recorded press must not reach a client.
#[test]
fn release_without_press_is_not_forwarded() {
    let mut s = S::new(true);
    s.key(KEY_A, false);
    assert!(
        s.ls.probe.kb.violations.is_empty(),
        "protocol violation: {:?}",
        s.ls.probe.kb.violations
    );
}

/// ATK-05 (OBJ-22 successor/live, F07): focus_next must never select an unmapped element.
#[test]
fn focus_next_skips_unmapped_elements() {
    let mut s = S::new(true);
    let w2 = s.w2;
    s.ls.probe.unmap(w2);
    s.advance();
    s.key(KEY_LEFTMETA, true);
    s.key(KEY_TAB, true);
    s.key(KEY_TAB, false);
    s.key(KEY_LEFTMETA, false);
    assert_ne!(
        s.ls.state.core().focus().target(),
        Some(s.e2),
        "focus_next focused UNMAPPED W2; client kb={:?}",
        s.ls.probe.kb.active
    );
    assert_ne!(s.ls.probe.kb.active.as_deref(), Some("W2"));
}

/// ATK-06 (K7/M11 successor, F07): target loss must not hand focus to an unmapped element.
#[test]
fn successor_is_never_unmapped() {
    let mut s = S::new(true);
    let (w1, w2) = (s._w1, s.w2);
    s.ls.probe.unmap(w2);
    s.advance();
    s.ls.probe.destroy_toplevel(w1);
    s.advance();
    assert_ne!(
        s.ls.state.core().focus().target(),
        Some(s.e2),
        "successor is UNMAPPED W2"
    );
}

/// ATK-07 (canonical successor = MRU live element, F11): an element never focused is not MRU.
#[test]
fn successor_without_history_is_not_fabricated() {
    let mut s = S::new(true);
    let w1 = s._w1;
    s.ls.probe.destroy_toplevel(w1);
    s.advance();
    assert_eq!(
        s.ls.state.core().focus().target(),
        None,
        "successor fabricated from z-order (never-focused W2)"
    );
}

/// ATK-08 (INV-PROTO-1): `activated` holds exactly on the focused element, through every transition.
#[test]
fn activated_tracks_active_element_focus() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w1 = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
    let w2 = ls.probe.toplevel(&qh, "W2", (400, 300), 0);
    let w3 = ls.probe.toplevel(&qh, "W3", (400, 300), 0);
    ls.pump();
    let elements: Vec<ElementId> = ls.state.elements().collect();
    assert_eq!(elements.len(), 3);
    let (e1, e2, e3) = (elements[0], elements[1], elements[2]);

    set_focus(&mut ls.state, &mut ls.input, Some(e1)).expect("focus e1");
    ls.pump();
    ls.run_until(ls.now + Duration::from_micros(1000));
    assert_eq!(last_activated(&ls, w1), Some(true));

    set_focus(&mut ls.state, &mut ls.input, Some(e2)).expect("focus e2");
    ls.pump();
    ls.run_until(ls.now + Duration::from_micros(1000));
    assert_eq!(last_activated(&ls, w1), Some(false));
    assert_eq!(last_activated(&ls, w2), Some(true));
    assert!(!last_activated(&ls, w3).unwrap_or(false));

    set_focus(&mut ls.state, &mut ls.input, None).expect("focus none");
    ls.pump();
    ls.run_until(ls.now + Duration::from_micros(1000));
    assert_eq!(last_activated(&ls, w2), Some(false));

    set_focus(&mut ls.state, &mut ls.input, Some(e3)).expect("focus e3");
    set_focus(&mut ls.state, &mut ls.input, Some(e2)).expect("focus e2");
    ls.pump();
    ls.run_until(ls.now + Duration::from_micros(1000));
    ls.probe.destroy_toplevel(w2);
    ls.pump();
    ls.run_until(ls.now + Duration::from_micros(1000));
    assert_eq!(ls.state.core().focus().target(), Some(e3));
    assert_eq!(
        last_activated(&ls, w3),
        Some(true),
        "successor not activated"
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
    let expected_rate = std::env::var("KEYBOARD_REPEAT_RATE")
        .ok()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(25);
    let expected_delay = std::env::var("KEYBOARD_REPEAT_DELAY")
        .ok()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(200);
    assert_eq!(
        s.ls.probe.repeat_info,
        Some((expected_rate, expected_delay))
    );

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

/// ATK-16 (M1 unmap target loss, F07): unmapping the focused element causes target loss.
#[test]
fn unmapped_focused_element_loses_focus() {
    let mut s = S::new(true);
    let w1 = s._w1;
    s.ls.probe.unmap(w1);
    s.advance();
    assert_ne!(
        s.ls.state.core().focus().target(),
        Some(s.e1),
        "unmapped focused element retained focus"
    );
}

/// OBJ-25 / M12: backend keyboard intake requires a valid InputPort belonging to this Core.
#[test]
fn keyboard_key_rejects_foreign_port() {
    let mut ls1 = Lockstep::new(Some(output((1920, 1080), 120)));
    let mut ls2 = Lockstep::new(Some(output((1920, 1080), 120)));
    let res =
        infynitae_backend_headless::keyboard_key(&mut ls1.state, &mut ls2.input, KEY_A, true, 1000);
    assert!(
        res.is_err(),
        "foreign input port was accepted by keyboard_key"
    );
}

/// R01: a key event submitted with a FOREIGN port must fail closed — including the seat XKB
/// state (OBJ-27). Otherwise a rejected Shift becomes a modifier announced to clients on `enter`.
#[test]
fn r01_foreign_port_key_leaves_no_xkb_trace() {
    let mut ls1 = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls1.qh.clone();
    let _w1 = ls1.probe.toplevel(&qh, "W1", (400, 300), 0);
    let _w2 = ls1.probe.toplevel(&qh, "W2", (400, 300), 0);
    ls1.pump();
    let elements: Vec<ElementId> = ls1.state.elements().collect();
    set_focus(&mut ls1.state, &mut ls1.input, Some(elements[0])).expect("focus");
    let mut other = Lockstep::new(Some(output((1920, 1080), 120)));
    let before = ls1.state.keyboard_modifier_state().serialized.depressed;
    let res = keyboard_key(&mut ls1.state, &mut other.input, KEY_LEFTSHIFT, true, 1);
    assert!(res.is_err(), "foreign port accepted");
    let after = ls1.state.keyboard_modifier_state().serialized.depressed;
    set_focus(&mut ls1.state, &mut ls1.input, Some(elements[1])).expect("focus");
    let t = ls1.now + Duration::from_micros(16_667);
    ls1.run_until(t);
    assert_eq!(
        after, before,
        "REJECTED event mutated seat XKB state (no fail-closed)"
    );
    assert_eq!(
        ls1.probe.kb.depressed, 0,
        "client told a modifier is held that no accepted event pressed"
    );
}

/// R02: portless sync_seat_key requires a verified InputAuthToken from Core.
#[test]
fn r02_portless_sync_seat_key_requires_token() {
    let ls1 = Lockstep::new(Some(output((1920, 1080), 120)));
    let ls2 = Lockstep::new(Some(output((1920, 1080), 120)));
    let err = ls1.state.core().verify_input_port(&ls2.input, KEY_A, true);
    assert_eq!(err.unwrap_err(), infynitae_core::EntryError::ForeignPort);
}

/// R03 (K1/M12 Down(Client) -> press): a repeated press of a CLIENT-owned key must not produce a
/// second `pressed` for a logically-down key (protocol must-not).
#[test]
fn r03_repeated_client_press_is_not_duplicated() {
    let mut s = S::new(true);
    s.set_focus(Some(s.e1));
    s.key(KEY_A, true);
    s.key(KEY_A, true);
    s.key(KEY_A, false);
    assert!(
        s.ls.probe.kb.violations.is_empty(),
        "protocol violation: {:?}",
        s.ls.probe.kb.violations
    );
}

/// R04: the backend snapshot must not be a second modifier authority: `logo` in the snapshot
/// with no Super held on the seat must NOT acquire a CameraPan (OBJ-27 single authority).
#[test]
fn r04_snapshot_logo_without_seat_super_does_not_pan() {
    let mut s = S::new(true);
    s.set_focus(Some(s.e1));
    s.motion(TO_LOCAL_50_50);
    // (where the pointer is does not matter here: a camera pan needs no window under it)
    pointer_button(
        &mut s.ls.state,
        &mut s.ls.input,
        BTN_MIDDLE,
        true,
        logo(),
        1000,
    )
    .expect("button");
    let pan = s.ls.state.core().interaction().active();
    pointer_button(
        &mut s.ls.state,
        &mut s.ls.input,
        BTN_MIDDLE,
        false,
        logo(),
        2000,
    )
    .expect("button");
    assert!(
        !pan,
        "CameraPan acquired from a backend snapshot while the seat has no Super held"
    );
}

/// R05: temporal sequences — the pan chord sees the seat state at the button's event boundary.
#[test]
fn r05_temporal_modifier_boundaries() {
    // (a) Super down, MMB down, motion, MMB up, Super up: pan; client sees no button
    let mut s1 = S::new(true);
    s1.set_focus(Some(s1.e1));
    s1.motion(TO_LOCAL_50_50);
    // (where the pointer is does not matter here: a camera pan needs no window under it)
    s1.key(KEY_LEFTMETA, true);
    let m = s1.ls.probe.ptr.len();
    pointer_button(
        &mut s1.ls.state,
        &mut s1.ls.input,
        BTN_MIDDLE,
        true,
        none(),
        1000,
    )
    .expect("button");
    let a_pan = s1.ls.state.core().interaction().active();
    s1.motion((30.0, 0.0));
    pointer_button(
        &mut s1.ls.state,
        &mut s1.ls.input,
        BTN_MIDDLE,
        false,
        none(),
        2000,
    )
    .expect("button");
    s1.key(KEY_LEFTMETA, false);
    let a_buttons: Vec<String> = s1.ls.probe.ptr[m..]
        .iter()
        .filter(|l| l.starts_with("button"))
        .cloned()
        .collect();

    // (b) MMB down before Super: client-owned, no pan
    let mut s2 = S::new(true);
    s2.set_focus(Some(s2.e1));
    s2.motion(TO_LOCAL_50_50);
    // (where the pointer is does not matter here: a camera pan needs no window under it)
    pointer_button(
        &mut s2.ls.state,
        &mut s2.ls.input,
        BTN_MIDDLE,
        true,
        none(),
        1000,
    )
    .expect("button");
    s2.key(KEY_LEFTMETA, true);
    let b_pan = s2.ls.state.core().interaction().active();
    pointer_button(
        &mut s2.ls.state,
        &mut s2.ls.input,
        BTN_MIDDLE,
        false,
        none(),
        2000,
    )
    .expect("button");
    s2.key(KEY_LEFTMETA, false);

    // (c) Super released immediately before MMB: no pan (no one-event lag)
    let mut s3 = S::new(true);
    s3.set_focus(Some(s3.e1));
    s3.motion(TO_LOCAL_50_50);
    // (where the pointer is does not matter here: a camera pan needs no window under it)
    s3.key(KEY_LEFTMETA, true);
    s3.key(KEY_LEFTMETA, false);
    pointer_button(
        &mut s3.ls.state,
        &mut s3.ls.input,
        BTN_MIDDLE,
        true,
        none(),
        1000,
    )
    .expect("button");
    let c_pan = s3.ls.state.core().interaction().active();
    pointer_button(
        &mut s3.ls.state,
        &mut s3.ls.input,
        BTN_MIDDLE,
        false,
        none(),
        2000,
    )
    .expect("button");

    assert!(a_pan && a_buttons.is_empty() && !b_pan && !c_pan);
}

/// R06: MRU sequence; arbitrary z-order fallback gone; no auto-focus on remap/create.
#[test]
fn r06_mru_history_sequence() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let wa = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
    let wb = ls.probe.toplevel(&qh, "W2", (400, 300), 0);
    let wc = ls.probe.toplevel(&qh, "W3", (400, 300), 0);
    ls.pump();
    let e: Vec<ElementId> = ls.state.elements().collect();
    assert_eq!(e.len(), 3);
    let (a, b, c) = (e[0], e[1], e[2]);

    set_focus(&mut ls.state, &mut ls.input, Some(a)).unwrap();
    set_focus(&mut ls.state, &mut ls.input, Some(b)).unwrap();
    set_focus(&mut ls.state, &mut ls.input, Some(c)).unwrap();

    ls.probe.destroy_toplevel(wc);
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    assert_eq!(
        ls.state.core().focus().target(),
        Some(b),
        "destroy C -> B (MRU)"
    );
    assert_eq!(ls.probe.kb.active.as_deref(), Some("W2"));

    ls.probe.unmap(wb);
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    assert_eq!(ls.state.core().focus().target(), Some(a), "unmap B -> A");
    assert_eq!(ls.probe.kb.active.as_deref(), Some("W1"));

    ls.probe.destroy_toplevel(wb);
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    assert_eq!(
        ls.state.core().focus().target(),
        Some(a),
        "destroying unfocused unmapped B keeps A"
    );

    ls.probe.destroy_toplevel(wa);
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    assert_eq!(
        ls.state.core().focus().target(),
        None,
        "no MRU successor -> Unfocused"
    );
    assert_eq!(ls.probe.kb.active, None);

    let qh = ls.qh.clone();
    ls.probe.toplevel(&qh, "D", (200, 200), 0);
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    assert_eq!(
        ls.state.core().focus().target(),
        None,
        "creating D does not take focus"
    );
    assert!(ls.probe.kb.violations.is_empty());
}

/// R07: focus cannot be set on an unmapped element through the port-gated entry.
#[test]
fn r07_set_focus_on_unmapped_is_refused() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let _w1 = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
    let w2 = ls.probe.toplevel(&qh, "W2", (400, 300), 0);
    ls.pump();
    let e: Vec<ElementId> = ls.state.elements().collect();
    ls.probe.unmap(w2);
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    let res = set_focus(&mut ls.state, &mut ls.input, Some(e[1]));
    assert!(res.is_err(), "unmapped element accepted as focus target");
}

/// R08: K6 / M22 cancel observable on the wire and on the camera: after Esc cancels a pan,
/// further motion moves no camera, and the compositor-owned MMB release is still consumed.
#[test]
fn r08_k6_cancel_is_externally_observable() {
    let mut s = S::new(true);
    s.set_focus(Some(s.e1));
    s.motion(TO_LOCAL_50_50);
    // (where the pointer is does not matter here: a camera pan needs no window under it)
    s.key(KEY_LEFTMETA, true);
    s.mmb(true);
    s.key(KEY_LEFTMETA, false);
    let cam0 = s.ls.state.core().cameras().center(0).unwrap();
    s.key(KEY_ESC, true);
    s.key(KEY_ESC, false);
    let m = s.ls.probe.ptr.len();
    s.motion((40.0, 0.0));
    let cam1 = s.ls.state.core().cameras().center(0).unwrap();
    s.mmb(false);
    let after: Vec<String> = s.ls.probe.ptr[m..].to_vec();
    assert_eq!(cam0, cam1, "camera kept panning after Esc cancel");
    assert!(
        !after.iter().any(|l| l.starts_with("button")),
        "owned MMB release leaked after cancel"
    );
    assert!(
        !s.ls.probe.key_events.iter().any(|(k, _)| *k == KEY_ESC),
        "Esc leaked while cancelling"
    );
}

/// N17 detection: set_focus must reject an InputPort that does not belong to its Core.
#[test]
fn set_focus_rejects_foreign_port() {
    let mut ls1 = Lockstep::new(Some(output((1920, 1080), 120)));
    let mut ls2 = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls1.qh.clone();
    let _w = ls1.probe.toplevel(&qh, "W", (400, 300), 0);
    ls1.pump();
    let e = ls1.state.elements().next().unwrap();
    let res = set_focus(&mut ls1.state, &mut ls2.input, Some(e));
    assert!(res.is_err(), "set_focus must reject foreign InputPort");
}

/// R2-04 duplicate key and modifier press sequences: seat XKB and client state remain truthful.
#[test]
fn t04_duplicate_presses() {
    const KEY_LEFTCTRL: u32 = 29;
    const KEY_LEFTALT: u32 = 56;
    let seqs = vec![
        (
            "dup",
            vec![
                ("key", KEY_A, true),
                ("key", KEY_A, true),
                ("key", KEY_A, false),
            ],
        ),
        (
            "dup x2",
            vec![
                ("key", KEY_A, true),
                ("key", KEY_A, true),
                ("key", KEY_A, true),
                ("key", KEY_A, false),
            ],
        ),
        (
            "dup after modifier",
            vec![
                ("key", KEY_A, true),
                ("key", KEY_LEFTSHIFT, true),
                ("key", KEY_A, true),
                ("key", KEY_A, false),
                ("key", KEY_LEFTSHIFT, false),
            ],
        ),
        (
            "dup after focus",
            vec![
                ("key", KEY_A, true),
                ("focus2", 0, false),
                ("key", KEY_A, true),
                ("key", KEY_A, false),
            ],
        ),
        (
            "dup modifier Shift",
            vec![
                ("key", KEY_LEFTSHIFT, true),
                ("key", KEY_LEFTSHIFT, true),
                ("key", KEY_LEFTSHIFT, false),
            ],
        ),
        (
            "dup modifier Super",
            vec![
                ("key", KEY_LEFTMETA, true),
                ("key", KEY_LEFTMETA, true),
                ("key", KEY_LEFTMETA, false),
            ],
        ),
        (
            "dup modifier Ctrl",
            vec![
                ("key", KEY_LEFTCTRL, true),
                ("key", KEY_LEFTCTRL, true),
                ("key", KEY_LEFTCTRL, false),
            ],
        ),
        (
            "dup modifier Alt",
            vec![
                ("key", KEY_LEFTALT, true),
                ("key", KEY_LEFTALT, true),
                ("key", KEY_LEFTALT, false),
            ],
        ),
    ];
    for (name, steps) in seqs {
        let mut s = S::new(true);
        s.set_focus(Some(s.e1));
        for &(op, k, p) in &steps {
            match op {
                "key" => s.key(k, p),
                "focus2" => s.set_focus(Some(s.e2)),
                _ => unreachable!(),
            }
        }
        let mods = s.ls.state.keyboard_modifier_state();
        let dep = mods.serialized.depressed;
        let c_keys = &s.ls.probe.kb.keys;
        let c_dep = s.ls.probe.kb.depressed;
        let viols = &s.ls.probe.kb.violations;
        assert!(
            viols.is_empty() && dep == 0 && c_keys.is_empty() && c_dep == 0,
            "T04 {name}: viols={viols:?} seat_dep={dep:#x} client_keys={c_keys:?} client_dep={c_dep:#x}"
        );
    }
}

/// R2-02 token capability binding: a token minted by ANOTHER Core must not authorize this seat.
#[test]
fn t02b_token_is_bound_to_its_core() {
    let mut s1 = S::new(true);
    s1.set_focus(Some(s1.e1));
    let s2 = S::new(true);
    let foreign_token = s2
        .ls
        .state
        .core()
        .verify_input_port(&s2.ls.input, KEY_LEFTSHIFT, true)
        .expect("foreign token");
    let before = s1.ls.state.keyboard_modifier_state().serialized.depressed;
    let ok = s1.ls.state.sync_seat_key(foreign_token);
    let after = s1.ls.state.keyboard_modifier_state().serialized.depressed;
    assert!(!ok.1, "foreign token must be rejected by sync_seat_key");
    assert_eq!(
        after, before,
        "foreign token must not mutate seat modifier state"
    );
}

/// R2-02 token lifetime and consume semantics: an InputAuthToken is consumed by sync_seat_key.
#[test]
fn t02c_token_consumed_and_cannot_be_reused() {
    let mut s = S::new(true);
    s.set_focus(Some(s.e1));
    let token =
        s.ls.state
            .core()
            .verify_input_port(&s.ls.input, KEY_LEFTSHIFT, true)
            .expect("token");
    let ok = s.ls.state.sync_seat_key(token);
    assert!(ok.1, "valid token authorizes seat key sync");
}

/// R2-06 unconfigured keys are delivered to client and never swallowed by compositor bindings.
#[test]
fn t11_unconfigured_keys_are_never_swallowed() {
    const KEY_K: u32 = 37;
    let mut out = Vec::new();
    for (name, combo) in [
        ("plain Esc", vec![KEY_ESC]),
        ("idle Super+Esc", vec![KEY_LEFTMETA, KEY_ESC]),
        ("idle Super+B", vec![KEY_LEFTMETA, KEY_B]),
        ("Super+K (unconfigured)", vec![KEY_LEFTMETA, KEY_K]),
        ("Shift alone", vec![KEY_LEFTSHIFT]),
    ] {
        let mut s = S::new(true);
        s.set_focus(Some(s.e1));
        let n = s.ls.probe.key_events.len();
        for &k in &combo {
            s.key(k, true);
        }
        for &k in combo.iter().rev() {
            s.key(k, false);
        }
        let last = *combo.last().unwrap();
        let delivered = s.ls.probe.key_events[n..]
            .iter()
            .filter(|(k, _)| *k == last)
            .count();
        out.push((name, delivered));
    }
    // configured action: Super+Tab is consumed (FocusNext)
    let mut s = S::new(true);
    s.set_focus(Some(s.e1));
    s.key(KEY_LEFTMETA, true);
    s.key(KEY_TAB, true);
    s.key(KEY_TAB, false);
    s.key(KEY_LEFTMETA, false);
    let enter =
        s.ls.probe
            .key_events
            .iter()
            .filter(|(k, _)| *k == KEY_TAB)
            .count();
    assert_eq!(
        enter, 0,
        "configured Super+Tab must be swallowed by compositor"
    );
    assert!(
        out.iter().all(|(_, d)| *d == 2),
        "unconfigured keys not delivered: {out:?}"
    );
}

/// Negative control: proves that genuinely incorrect or spurious keyboard leave events
/// are reliably detected as protocol violations by the probe harness.
#[test]
fn negative_control_invalid_leave_event_detected() {
    use wayland_client::protocol::wl_keyboard;
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w1 = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
    let w2 = ls.probe.toplevel(&qh, "W2", (400, 300), 0);
    ls.pump();

    let s1 = ls.probe.surfaces[w1].wl.clone();
    let s2 = ls.probe.surfaces[w2].wl.clone();
    let kb = ls.probe.keyboard.as_ref().unwrap().clone();
    let conn = ls.conn.clone();

    // 1. Initially unfocused (active=None, destroyed_active empty):
    // Spurious leave targeting W1 must be flagged as a protocol violation.
    <common::Probe as wayland_client::Dispatch<wl_keyboard::WlKeyboard, ()>>::event(
        &mut ls.probe,
        &kb,
        wl_keyboard::Event::Leave {
            serial: 1,
            surface: s1,
        },
        &(),
        &conn,
        &qh,
    );
    assert_eq!(
        ls.probe.kb.violations,
        vec!["leave(W1) but active=None".to_string()]
    );
    ls.probe.kb.violations.clear();

    // 2. Focused on W1: spurious leave targeting W2 must be flagged as a violation.
    ls.probe.kb.active = Some("W1".to_string());
    <common::Probe as wayland_client::Dispatch<wl_keyboard::WlKeyboard, ()>>::event(
        &mut ls.probe,
        &kb,
        wl_keyboard::Event::Leave {
            serial: 2,
            surface: s2,
        },
        &(),
        &conn,
        &qh,
    );
    assert_eq!(
        ls.probe.kb.violations,
        vec!["leave(W2) but active=Some(\"W1\")".to_string()]
    );
    ls.probe.kb.violations.clear();

    // 3. Spurious dummy/null leave (#0) while focused on W1 must be flagged as a violation.
    // (A null leave is only legal in-flight for an active surface that was destroyed).
    let dummy_s = ls
        .probe
        .compositor
        .as_ref()
        .unwrap()
        .create_surface(&qh, ());
    dummy_s.destroy();
    <common::Probe as wayland_client::Dispatch<wl_keyboard::WlKeyboard, ()>>::event(
        &mut ls.probe,
        &kb,
        wl_keyboard::Event::Leave {
            serial: 3,
            surface: dummy_s,
        },
        &(),
        &conn,
        &qh,
    );
    assert!(
        !ls.probe.kb.violations.is_empty(),
        "spurious destroyed leave while focused must be flagged as violation"
    );
}
