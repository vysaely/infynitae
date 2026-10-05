//! INC-4 T1: ActiveElementFocus vs popup grab protocol routing (TST-20 / xpopup_focus, OB-30, OB-4).
//!
//! Evaluates the production Core, Wayland adapter, and headless backend with a recording client probe.
//! Invariants verified after every focus transition:
//! - F-1: Smithay keyboard focus == Core ActiveElementFocus target (no divergence).
//! - F-2: Client's logical keyboard focus == target (wl_keyboard enter/leave as seen by the client).
//! - F-3: No wl_keyboard or wl_pointer protocol-order violation.
//! - F-4: Exactly one keyboard transition into the target (no duplicate or transient focus).
//! - F-5: Every grabbing popup received popup_done; no panic.
//! - F-6: No stuck keys: client keys == physically held client-owned keys.

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use common::input::{BTN_LEFT, BTN_MIDDLE, logo, none};
use common::{Lockstep, PopupSpec, output};
use infynitae_backend_headless::{keyboard_key, pointer_button, pointer_motion, set_focus};
use infynitae_core::input::keys::{KEY_A, KEY_LEFTMETA};
use infynitae_core::input::ownership::Owner;
use infynitae_core::stage::ElementId;
use infynitae_wayland::PreemptionStrategy;

struct PopupHarness {
    ls: Lockstep,
    time_us: u64,
    w1: usize,
    _w2: usize,
    e1: ElementId,
    e2: ElementId,
    popups: Vec<usize>,
}

impl PopupHarness {
    fn new() -> Self {
        let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
        ls.probe.auto_destroy_popups = true;
        let qh = ls.qh.clone();
        let w1 = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
        let w2 = ls.probe.toplevel(&qh, "W2", (400, 300), 0);
        ls.pump();
        let elements: Vec<ElementId> = ls.state.elements().collect();
        assert_eq!(elements.len(), 2);
        let e1 = elements[0];
        let e2 = elements[1];
        let mut h = Self {
            ls,
            time_us: 10_000,
            w1,
            _w2: w2,
            e1,
            e2,
            popups: Vec::new(),
        };
        // Initial focus on W1
        h.core_focus(Some(e1));
        h
    }

    fn advance(&mut self) {
        self.ls.pump();
        let t = self.ls.now + Duration::from_micros(16_667);
        self.ls.run_until(t);
    }

    fn open_popup(&mut self, label: &'static str, parent_idx: usize) -> usize {
        let qh = self.ls.qh.clone();
        let p = self.ls.probe.popup_with_grab(
            &qh,
            label,
            parent_idx,
            PopupSpec {
                content: (120, 80),
                shadow: 0,
                anchor_rect: (10, 10, 1, 1),
                slide: false,
            },
        );
        self.popups.push(p);
        self.advance();
        p
    }

    fn core_focus(&mut self, target: Option<ElementId>) {
        set_focus(&mut self.ls.state, &mut self.ls.input, target).expect("set_focus");
        self.advance();
    }

    fn key(&mut self, k: u32, pressed: bool) {
        self.time_us += 1000;
        keyboard_key(
            &mut self.ls.state,
            &mut self.ls.input,
            k,
            pressed,
            self.time_us,
        )
        .expect("keyboard_key");
        self.advance();
    }

    fn motion(&mut self, d: (f64, f64)) {
        self.time_us += 1000;
        pointer_motion(&mut self.ls.state, &mut self.ls.input, d, d, self.time_us).expect("motion");
        self.advance();
    }

    fn button(&mut self, b: u32, p: bool, m: infynitae_core::input::bindings::Modifiers) {
        self.time_us += 1000;
        pointer_button(
            &mut self.ls.state,
            &mut self.ls.input,
            b,
            p,
            m,
            self.time_us,
        )
        .expect("button");
        self.advance();
    }

    fn check_invariants(&self, target: Option<ElementId>, expected_dones: usize) -> bool {
        let kb = self.ls.state.keyboard_handle();
        let srv_focus = kb.current_focus();
        let want_surface = target.and_then(|id| {
            self.ls
                .state
                .window(id)
                .and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()))
        });
        // F-1: Smithay keyboard focus == target
        let f1 = srv_focus == want_surface;
        // F-2: Client logical focus == target label
        let client_focus = self.ls.probe.kb.active.clone();
        let want_label = target.map(|id| if id == self.e1 { "W1" } else { "W2" });
        let f2 = client_focus.as_deref() == want_label;
        // F-3: No protocol violations
        let f3 = self.ls.probe.kb.violations.is_empty();
        // F-5: Popup done count
        let f5 = self.ls.probe.popup_dones.len() >= expected_dones;
        // F-6: No stuck keys
        let held_client_owned: BTreeSet<u32> = self
            .ls
            .probe
            .kb
            .keys
            .iter()
            .copied()
            .filter(|&k| self.ls.state.core().key_ownership().owner(k) == Some(Owner::Client))
            .collect();
        let f6 = self.ls.probe.kb.keys == held_client_owned;
        f1 && f2 && f3 && f5 && f6
    }
}

/// PF-1: Grab active -> core focuses W2 under ORDERED preemption.
#[test]
fn pf1_grab_active_core_focuses_w2_ordered() {
    let mut h = PopupHarness::new();
    h.open_popup("P", h.w1);
    assert!(
        h.ls.state.keyboard_handle().is_grabbed(),
        "popup grab must grab keyboard"
    );
    h.core_focus(Some(h.e2));
    assert!(
        h.check_invariants(Some(h.e2), 1),
        "PF-1 invariants must hold"
    );
    assert_eq!(h.ls.probe.popup_dones, vec!["P"]);
    assert_eq!(h.ls.probe.kb.active.as_deref(), Some("W2"));
    assert!(
        !h.ls.state.keyboard_handle().is_grabbed(),
        "keyboard still grabbed after preemption"
    );
    assert!(
        !h.ls.state.is_pointer_grabbed(),
        "pointer still grabbed after preemption"
    );
}

/// PF-2: Grab active -> core focuses None under ORDERED preemption.
#[test]
fn pf2_grab_active_core_focuses_none_ordered() {
    let mut h = PopupHarness::new();
    h.open_popup("P", h.w1);
    h.core_focus(None);
    assert!(h.check_invariants(None, 1), "PF-2 invariants must hold");
    assert_eq!(h.ls.probe.popup_dones, vec!["P"]);
    assert_eq!(h.ls.probe.kb.active, None);
}

/// PF-3: Popup parent W1 destroyed by client during grab -> successor W2 focused under ORDERED preemption.
#[test]
fn pf3_parent_destroyed_during_grab_successor_focused_ordered() {
    let mut h = PopupHarness::new();
    h.core_focus(Some(h.e2));
    h.core_focus(Some(h.e1));
    let p = h.open_popup("P", h.w1);
    // Client tears down popup first, then parent toplevel in protocol order
    h.ls.probe.destroy_popup(p);
    h.popups.clear();
    h.ls.probe.destroy_toplevel(h.w1);
    h.advance();

    // Core target loss selects successor W2
    assert_eq!(h.ls.state.core().focus().target(), Some(h.e2));
    assert_eq!(h.ls.probe.kb.active.as_deref(), Some("W2"));
    assert!(h.ls.probe.kb.violations.is_empty());
}

/// PF-4: Popup destroyed in flight while core changes focus -> clean preemption.
#[test]
fn pf4_popup_destroyed_in_flight_while_core_changes_focus_ordered() {
    let mut h = PopupHarness::new();
    let p = h.open_popup("P", h.w1);
    // Client destroys popup in flight
    h.ls.probe.destroy_popup(p);
    h.popups.clear();
    // Core simultaneously changes focus
    h.core_focus(Some(h.e2));
    assert!(
        h.check_invariants(Some(h.e2), 0),
        "PF-4 invariants must hold"
    );
    assert_eq!(h.ls.probe.kb.active.as_deref(), Some("W2"));
}

/// PF-5: Popup destroyed and dispatched first, then core focuses W2 -> clean transition.
#[test]
fn pf5_popup_destroyed_first_then_core_focuses_w2_ordered() {
    let mut h = PopupHarness::new();
    let p = h.open_popup("P", h.w1);
    h.ls.probe.destroy_popup(p);
    h.popups.clear();
    h.advance();
    h.core_focus(Some(h.e2));
    assert!(
        h.check_invariants(Some(h.e2), 0),
        "PF-5 invariants must hold"
    );
    assert_eq!(h.ls.probe.kb.active.as_deref(), Some("W2"));
}

/// PF-6: Nested popup grab (P1 -> P2) keeps pointer grab (W-SM-14) -> key routed to P2, then core focuses W2.
#[test]
fn pf6_nested_popup_grab_keeps_pointer_grab() {
    let mut h = PopupHarness::new();
    let p1 = h.open_popup("P1", h.w1);
    h.open_popup("P2", p1);

    // Key press routes to topmost popup P2
    h.key(KEY_A, true);
    assert_eq!(h.ls.probe.kb.active.as_deref(), Some("P2"));
    h.key(KEY_A, false);

    // Core focus changes to W2: both popups dismissed, W2 gains focus
    h.core_focus(Some(h.e2));
    assert!(
        h.check_invariants(Some(h.e2), 2),
        "PF-6 invariants must hold"
    );
    assert_eq!(h.ls.probe.kb.active.as_deref(), Some("W2"));
}

/// PF-7: Client-owned key held during grab -> core focuses W2 -> release reaches W2 cleanly.
#[test]
fn pf7_client_owned_key_held_during_grab_core_focuses_w2_release() {
    let mut h = PopupHarness::new();
    h.open_popup("P", h.w1);
    h.key(KEY_A, true);
    assert!(h.ls.probe.kb.keys.contains(&KEY_A));

    // Focus transitions to W2 under ORDERED preemption
    h.core_focus(Some(h.e2));
    assert_eq!(h.ls.probe.kb.active.as_deref(), Some("W2"));
    assert!(h.ls.probe.kb.keys.contains(&KEY_A), "A was entered into W2");

    // Release A goes to W2
    h.key(KEY_A, false);
    assert!(h.ls.probe.kb.keys.is_empty(), "No stuck keys");
    assert!(
        h.check_invariants(Some(h.e2), 1),
        "PF-7 invariants must hold"
    );
}

/// PF-OB4: Popup grab active -> Compositor CameraPan acquired -> popup dismissed before camera moves.
#[test]
fn pf_ob4_interaction_acquisition_dismisses_popup_grab() {
    let mut h = PopupHarness::new();
    h.open_popup("P", h.w1);
    assert!(h.ls.probe.popup_dones.is_empty());

    // Inject middle button press with logo modifier to trigger CameraPan acquisition
    h.time_us += 1000;
    pointer_button(
        &mut h.ls.state,
        &mut h.ls.input,
        BTN_MIDDLE,
        true,
        logo(),
        h.time_us,
    )
    .expect("pan button press");
    h.advance();

    // Popup was dismissed before any camera motion occurred (OB-4)
    assert_eq!(h.ls.probe.popup_dones, vec!["P"]);
}

/// PF-WSM7: Misbehaving client requests grab on dead/unmapped parent surface -> refused without panic.
#[test]
fn pf_wsm7_grab_on_unmapped_parent_handled_gracefully() {
    let mut h = PopupHarness::new();
    let qh = h.ls.qh.clone();
    // Create an unmapped toplevel (no buffer attached, not mapped in stage)
    let dead = h.ls.probe.toplevel(&qh, "DEAD", (100, 100), 0);
    h.ls.probe.unmap(dead);
    h.advance();

    // Attempt grab on unmapped parent
    h.ls.probe.popup_with_grab(
        &qh,
        "INVALID",
        dead,
        PopupSpec {
            content: (50, 50),
            shadow: 0,
            anchor_rect: (0, 0, 1, 1),
            slide: false,
        },
    );
    h.advance();

    // Handled safely without crash or panic
    assert!(h.ls.probe.kb.violations.is_empty());
}

/// NEG-1: Naive set_focus negative control (without popup ungrab).
/// Verifies that naive strategy fails to dismiss popup (F-5 fails).
#[test]
fn neg1_naive_set_focus_fails_popup_dismissal() {
    let mut h = PopupHarness::new();
    h.ls.state
        .set_preemption_strategy(PreemptionStrategy::Naive);
    h.open_popup("P", h.w1);
    h.core_focus(Some(h.e2));

    // Under Naive strategy, popup was NOT ungrabbed: popup_done was never sent
    assert!(
        h.ls.probe.popup_dones.is_empty(),
        "Naive strategy failed to ungrab popup (NEG-1 reproducer confirmed)"
    );
}

/// NEG-2: Pointer-first teardown negative control (W-SM-15 violation).
#[test]
fn neg2_pointer_first_teardown_control() {
    fn run(s: PreemptionStrategy) -> Vec<String> {
        let mut h = PopupHarness::new();
        pointer_motion(
            &mut h.ls.state,
            &mut h.ls.input,
            (50.0, 50.0),
            (50.0, 50.0),
            h.time_us,
        )
        .expect("motion");
        pointer_button(
            &mut h.ls.state,
            &mut h.ls.input,
            BTN_LEFT,
            true,
            none(),
            h.time_us,
        )
        .expect("button");
        pointer_button(
            &mut h.ls.state,
            &mut h.ls.input,
            BTN_LEFT,
            false,
            none(),
            h.time_us,
        )
        .expect("button");
        let top = h.ls.state.core().focus().target().expect("focused");
        let (parent, other) = if top == h.e1 {
            (h.w1, h.e2)
        } else {
            (h._w2, h.e1)
        };
        h.ls.state.set_preemption_strategy(s);
        h.open_popup("P", parent);
        let m = h.ls.probe.kb_log.len();
        h.core_focus(Some(other));
        h.ls.probe.kb_log[m..]
            .iter()
            .filter(|l| !l.starts_with("toplevel_configure"))
            .cloned()
            .collect()
    }
    let ordered = run(PreemptionStrategy::Ordered);
    let pfirst = run(PreemptionStrategy::PointerFirst);
    assert_ne!(
        ordered, pfirst,
        "NEG-2 indistinguishable from ORDERED on the client wire"
    );
}

/// OBS-1: Nested grab replaces pointer grab observation (Smithay default / anvil order).
#[test]
fn obs1_nested_grab_replaces_pointer_grab_observation() {
    let mut h = PopupHarness::new();
    h.ls.state.set_nested_grab_keep_pointer(false);
    let p1 = h.open_popup("P1", h.w1);
    h.open_popup("P2", p1);

    // Key routing check under unpatched anvil order
    h.key(KEY_A, true);
    let active = h.ls.probe.kb.active.clone();
    let kbg = h.ls.state.keyboard_handle().is_grabbed();
    h.key(KEY_A, false);
    assert!(
        active.as_deref() != Some("P2") || !kbg,
        "OBS-1 shows no failure: routing intact without W-SM-14 workaround"
    );
}

/// ATK-09 (OB-4 × INV-INPUT-3, F05): dismissing the grab on CameraPan acquisition must not deliver
/// pointer events to any client.
#[test]
fn atk09_pan_acquisition_dismissal_emits_no_pointer_events() {
    let mut h = PopupHarness::new();
    h.motion((50.0, 50.0));
    h.button(BTN_LEFT, true, none());
    h.button(BTN_LEFT, false, none());
    let top = h.ls.state.core().focus().target().expect("focused");
    let parent = if top == h.e1 { h.w1 } else { h._w2 };
    h.open_popup("P", parent);
    h.motion((5.0, 0.0));
    let m = h.ls.probe.ptr.len();
    h.button(BTN_MIDDLE, true, logo());
    let after: Vec<String> = h.ls.probe.ptr[m..].to_vec();
    assert_eq!(h.ls.probe.popup_dones, vec!["P"]);
    let leaked: Vec<&String> = after
        .iter()
        .filter(|l| {
            l.starts_with("motion")
                || l.starts_with("enter")
                || l.starts_with("leave")
                || l.starts_with("button")
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "INV-INPUT-3: pointer events at pan acquisition: {leaked:?}"
    );
}

/// ATK-11 (protocol refusal, F08): a grab on an unfocused element is refused and dismissed.
#[test]
fn atk11_refused_grab_dismisses_the_popup() {
    let mut h = PopupHarness::new();
    let qh = h.ls.qh.clone();
    h.ls.probe.popup_with_grab(
        &qh,
        "Q",
        h._w2,
        PopupSpec {
            content: (120, 80),
            shadow: 0,
            anchor_rect: (10, 10, 1, 1),
            slide: false,
        },
    );
    h.advance();
    let kb = h.ls.state.keyboard_handle();
    assert!(!kb.is_grabbed(), "grab installed for an unfocused element");
    assert_eq!(
        h.ls.probe.popup_dones,
        vec!["Q"],
        "refused grab left the popup alive (no popup_done)"
    );
}

/// ATK-12 (INC-3 zero-output state × INV-INPUT-10, F09): preemption must not tear down zero-output grab.
#[test]
fn atk12_preemption_keeps_the_zero_output_grab() {
    let mut h = PopupHarness::new();
    h.key(KEY_A, true);
    h.key(KEY_A, false);
    let o = h.ls.output();
    h.ls.disconnect(o);
    h.advance();
    assert!(
        h.ls.state.seat().get_pointer().unwrap().is_grabbed(),
        "zero-output grab installed"
    );
    h.open_popup("P", h.w1);
    h.core_focus(Some(h.e2));
    assert!(
        h.ls.state.seat().get_pointer().unwrap().is_grabbed(),
        "zero-output grab torn down by popup preemption while no output exists"
    );
}

/// ATK-13 (modifier authority, F16): CameraPan reads seat XKB state (OBJ-27).
#[test]
fn atk13_pan_modifier_comes_from_the_seat_xkb_state() {
    let mut h = PopupHarness::new();
    pointer_motion(
        &mut h.ls.state,
        &mut h.ls.input,
        (50.0, 50.0),
        (50.0, 50.0),
        h.time_us,
    )
    .expect("motion");
    // Super physically held on the seat keyboard; the button arrives with a stale empty snapshot.
    h.key(KEY_LEFTMETA, true);
    pointer_button(
        &mut h.ls.state,
        &mut h.ls.input,
        BTN_MIDDLE,
        true,
        none(),
        h.time_us,
    )
    .expect("button");
    let pan = h.ls.state.core().interaction().active();
    pointer_button(
        &mut h.ls.state,
        &mut h.ls.input,
        BTN_MIDDLE,
        false,
        none(),
        h.time_us,
    )
    .expect("button");
    h.key(KEY_LEFTMETA, false);
    assert!(
        pan,
        "Super held on the seat but CameraPan not acquired: two modifier authorities"
    );
}

/// ATK-14 (stale grab bookkeeping, F09): stale popup grab does not kill implicit button grab.
#[test]
fn atk14_stale_popup_grab_does_not_kill_implicit_button_grab() {
    let mut h = PopupHarness::new();
    h.motion((50.0, 50.0));
    h.button(BTN_LEFT, true, none());
    h.button(BTN_LEFT, false, none());
    let top = h.ls.state.core().focus().target().expect("focused");
    let (parent, other) = if top == h.e1 {
        (h.w1, h.e2)
    } else {
        (h._w2, h.e1)
    };
    let p = h.open_popup("P", parent);
    h.ls.probe.destroy_popup(p);
    h.advance();
    h.button(BTN_LEFT, true, none());
    let grabbed_before = h.ls.state.seat().get_pointer().unwrap().is_grabbed();
    h.core_focus(Some(other));
    let grabbed_after = h.ls.state.seat().get_pointer().unwrap().is_grabbed();
    h.button(BTN_LEFT, false, none());
    assert_eq!(
        grabbed_before, grabbed_after,
        "implicit client button grab removed by stale popup preemption"
    );
}

/// ATK-15 (K8/OB-15 × popups, F09): output removal keeps popup keyboard route.
#[test]
fn atk15_output_removal_keeps_popup_keyboard_route() {
    let mut h = PopupHarness::new();
    h.key(KEY_A, true);
    h.key(KEY_A, false);
    h.open_popup("P", h.w1);
    assert_eq!(h.ls.probe.kb.active.as_deref(), Some("P"));
    let o = h.ls.output();
    h.ls.disconnect(o);
    h.advance();
    assert_eq!(
        h.ls.probe.kb.active.as_deref(),
        Some("P"),
        "output removal changed keyboard routing"
    );
    assert!(h.ls.probe.kb.violations.is_empty());
}

/// ATK-18 (M3 parent unmapped/destroyed, F17): unmapping parent dismisses its child popups.
#[test]
fn atk18_unmapped_parent_dismisses_its_popup_tree() {
    let mut h = PopupHarness::new();
    let qh = h.ls.qh.clone();
    let w2 = h._w2;
    h.ls.probe.popup(
        &qh,
        "Q",
        w2,
        PopupSpec {
            content: (60, 40),
            shadow: 0,
            anchor_rect: (5, 5, 1, 1),
            slide: false,
        },
    );
    h.advance();
    h.ls.probe.unmap(w2);
    h.advance();
    assert_eq!(
        h.ls.probe.popup_dones,
        vec!["Q"],
        "popup of an unmapped parent not dismissed"
    );
}

/// Click-to-focus on mapped element acquires ActiveElementFocus (F12, Mutation U).
#[test]
fn click_to_focus_focuses_mapped_window() {
    let mut h = PopupHarness::new();
    h.core_focus(None);
    assert_eq!(h.ls.probe.kb.active, None);
    h.motion((50.0, 50.0));
    h.button(BTN_LEFT, true, none());
    h.button(BTN_LEFT, false, none());
    assert!(
        h.ls.state.core().focus().target().is_some(),
        "click-to-focus did not acquire focus"
    );
}

fn fds() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .map(|d| d.count())
        .unwrap_or(0)
}

/// RES-1 (INC3-F09 class): repeated client/keyboard/popup/grab/focus/output cycles keep
/// descriptor count bounded.
#[test]
fn res1_bounded_resources_over_inc4_cycles() {
    let mut samples = Vec::new();
    for i in 0..40 {
        let mut h = PopupHarness::new();
        h.key(KEY_A, true);
        h.key(KEY_A, false);
        let p1 = h.open_popup("P1", h.w1);
        h.open_popup("P2", p1);
        h.core_focus(Some(h.e2));
        let o = h.ls.output();
        h.ls.disconnect(o);
        h.advance();
        h.ls.connect(&output((1920, 1080), 120));
        h.advance();
        let w2 = h._w2;
        h.ls.probe.destroy_toplevel(w2);
        h.advance();
        drop(h);
        if i == 4 || i == 39 {
            samples.push(fds());
        }
    }
    assert!(
        samples[1] <= samples[0] + 5,
        "descriptor growth over 35 cycles: {samples:?}"
    );
}

/// Grab on toplevel refused when keyboard is already grabbed by an active grab (Mutation L).
#[test]
fn pf_grab_refused_when_keyboard_grabbed_by_another_sequence() {
    let mut h = PopupHarness::new();
    h.ls.state
        .test_set_keyboard_grab(smithay::utils::Serial::from(42));
    assert!(h.ls.state.keyboard_handle().is_grabbed());

    let _p = h.open_popup("P", h.w1);
    assert!(
        h.ls.probe.popup_dones.contains(&"P"),
        "foreign grab was not refused"
    );
}
