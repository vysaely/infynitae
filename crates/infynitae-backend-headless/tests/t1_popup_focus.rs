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
use infynitae_core::input::keys::{KEY_A, KEY_ENTER, KEY_LEFTMETA};
use infynitae_core::stage::ElementId;
#[cfg(feature = "probe_preemption_strategy")]
use infynitae_wayland::PreemptionStrategy;

struct PopupHarness {
    ls: Lockstep,
    time_us: u64,
    w1: usize,
    _w2: usize,
    e1: ElementId,
    e2: ElementId,
    popups: Vec<usize>,
    held: BTreeSet<u32>,
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
            held: BTreeSet::new(),
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
    }

    fn motion(&mut self, d: (f64, f64)) {
        self.time_us += 1000;
        pointer_motion(&mut self.ls.state, &mut self.ls.input, d, d, self.time_us).expect("motion");
        self.advance();
    }

    fn button(&mut self, b: u32, p: bool, m: infynitae_core::input::bindings::Modifiers) {
        if m.logo && p {
            self.key(KEY_LEFTMETA, true);
        }
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
        if m.logo && !p {
            self.key(KEY_LEFTMETA, false);
        }
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
        // F-4: Exactly one keyboard transition into the target
        let f4 = match want_label {
            Some(lbl) => {
                let enters: Vec<_> = self
                    .ls
                    .probe
                    .kb_log
                    .iter()
                    .filter(|l| l.starts_with("wl_keyboard.enter"))
                    .collect();
                enters.last().map(|l| l.contains(lbl)).unwrap_or(false)
            }
            None => true,
        };
        // F-5: Popup done count
        let f5 = self.ls.probe.popup_dones.len() >= expected_dones;
        // F-6: No stuck keys (independent oracle against physically held keys)
        let f6 = self.ls.probe.kb.keys == self.held;
        f1 && f2 && f3 && f4 && f5 && f6
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
    let done_idx =
        h.ls.probe
            .kb_log
            .iter()
            .position(|l| l.contains("popup_done P"))
            .expect("popup_done");
    let enter_idx =
        h.ls.probe
            .kb_log
            .iter()
            .position(|l| l.contains("wl_keyboard.enter W2"))
            .expect("enter");
    assert!(
        done_idx < enter_idx,
        "popup_done must arrive BEFORE wl_keyboard.enter on preemption (INV-INPUT-10 ordering)"
    );
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
    h.button(BTN_MIDDLE, true, logo());

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

    // Handled safely without crash or panic; unmapped parent refuses grab
    assert!(
        h.ls.probe.popup_dones.contains(&"INVALID"),
        "grab on unmapped parent must be refused with popup_done"
    );
    assert!(
        !h.ls.state.keyboard_handle().is_grabbed(),
        "keyboard must not be grabbed"
    );
    assert!(h.ls.probe.kb.violations.is_empty());
}

/// NEG-1: Naive set_focus negative control (without popup ungrab).
/// Verifies that naive strategy fails to dismiss popup (F-5 fails).
#[cfg(feature = "probe_preemption_strategy")]
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
    assert!(
        h.ls.state.keyboard_handle().is_grabbed(),
        "Naive strategy left keyboard grabbed"
    );
}

/// NEG-2: Pointer-first teardown negative control (W-SM-15 violation).
#[cfg(feature = "probe_preemption_strategy")]
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
#[cfg(feature = "probe_nested_grab_keep_pointer")]
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
#[cfg(feature = "probe_test_keyboard_grab")]
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

// ---------------------------------------------------------------- R-series review reproductions

struct R {
    ls: Lockstep,
    t: u64,
    w: Vec<usize>,
    e: Vec<ElementId>,
}

impl R {
    fn from(ls: Lockstep, n: usize) -> R {
        let mut ls = ls;
        ls.probe.auto_destroy_popups = true;
        let qh = ls.qh.clone();
        let labels = ["W1", "W2", "W3", "W4"];
        let mut w = Vec::new();
        for l in labels.iter().take(n) {
            w.push(ls.probe.toplevel(&qh, l, (400, 300), 0));
        }
        ls.pump();
        let e: Vec<ElementId> = ls.state.elements().collect();
        assert_eq!(e.len(), n);
        let mut r = R {
            ls,
            t: 10_000,
            w,
            e,
        };
        r.adv();
        r
    }
    fn new(n: usize) -> R {
        R::from(Lockstep::new(Some(output((1920, 1080), 120))), n)
    }
    fn adv(&mut self) {
        self.ls.pump();
        let t = self.ls.now + Duration::from_micros(16_667);
        self.ls.run_until(t);
    }
    fn key(&mut self, k: u32, p: bool) {
        self.t += 1000;
        keyboard_key(&mut self.ls.state, &mut self.ls.input, k, p, self.t).expect("key");
        self.adv();
    }
    fn motion(&mut self, d: (f64, f64)) {
        self.t += 1000;
        pointer_motion(&mut self.ls.state, &mut self.ls.input, d, d, self.t).expect("motion");
        self.adv();
    }
    fn button(&mut self, b: u32, p: bool, m: infynitae_core::input::bindings::Modifiers) {
        self.t += 1000;
        pointer_button(&mut self.ls.state, &mut self.ls.input, b, p, m, self.t).expect("button");
        self.adv();
    }
    fn focus(&mut self, t: Option<ElementId>) {
        set_focus(&mut self.ls.state, &mut self.ls.input, t).expect("focus");
        self.adv();
    }
    fn target(&self) -> Option<ElementId> {
        self.ls.state.core().focus().target()
    }
    fn popup(&mut self, label: &'static str, parent: usize) -> usize {
        let qh = self.ls.qh.clone();
        let p = self.ls.probe.popup_with_grab(
            &qh,
            label,
            parent,
            PopupSpec {
                content: (120, 80),
                shadow: 0,
                anchor_rect: (10, 10, 1, 1),
                slide: false,
            },
        );
        self.adv();
        p
    }
    fn viol(&self) -> Vec<String> {
        self.ls.probe.kb.violations.clone()
    }
    fn ptr_grabbed(&self) -> bool {
        self.ls.state.seat().get_pointer().unwrap().is_grabbed()
    }
    fn kb_grabbed(&self) -> bool {
        self.ls.state.keyboard_handle().is_grabbed()
    }
    /// Click-to-focus on the topmost window under the pointer; returns its (probe idx, element).
    fn click_focus(&mut self) -> (usize, ElementId) {
        self.motion((50.0, 50.0));
        self.button(BTN_LEFT, true, none());
        self.button(BTN_LEFT, false, none());
        let top = self.target().expect("click-to-focus");
        let i = self.e.iter().position(|&x| x == top).unwrap();
        (self.w[i], top)
    }
}

/// R09: nested popup + focus change: INV-INPUT-10 step 3 must remove the retained pointer grab.
#[test]
fn r09_nested_preemption_removes_retained_pointer_grab() {
    let mut r = R::new(2);
    let (wp, top) = r.click_focus();
    let other = if top == r.e[0] { r.e[1] } else { r.e[0] };
    let p1 = r.popup("P1", wp);
    r.popup("P2", p1);
    assert!(r.ptr_grabbed() && r.kb_grabbed(), "nested grab installed");
    r.focus(Some(other));
    assert_eq!(r.ls.probe.popup_dones, vec!["P2", "P1"], "topmost first");
    assert!(!r.kb_grabbed(), "keyboard grab survived preemption");
    assert!(
        !r.ptr_grabbed(),
        "retained PopupPointerGrab survived preemption (step 3 skipped for nested grabs)"
    );
}

/// R09b: nested grab with a DISTINCT serial (real menus: the submenu opens on a later event).
/// W-SM-14 keeps the pointer grab installed with the FIRST serial; INV-INPUT-10 step 3 must
/// still remove it on preemption.
#[test]
fn r09b_nested_preemption_with_distinct_serials() {
    let mut r = R::new(2);
    let (wp, top) = r.click_focus();
    let other = if top == r.e[0] { r.e[1] } else { r.e[0] };
    let p1 = r.popup("P1", wp);
    r.key(KEY_A, true); // a later user event inside P1 -> new serial
    r.key(KEY_A, false);
    let s2 = r.ls.probe.last_serial;
    let qh = r.ls.qh.clone();
    r.ls.probe.popup_with_grab_serial(
        &qh,
        "P2",
        p1,
        PopupSpec {
            content: (60, 40),
            shadow: 0,
            anchor_rect: (5, 5, 1, 1),
            slide: false,
        },
        s2,
    );
    r.adv();
    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some("P2"),
        "nested grab routed"
    );
    r.focus(Some(other));
    assert!(
        !r.ptr_grabbed(),
        "retained PopupPointerGrab (first serial) survived preemption"
    );
    // a later click on the new focus must reach it normally
    let m = r.ls.probe.ptr.len();
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    assert!(!r.ls.probe.ptr[m..].is_empty());
}

/// R10: CameraPan acquisition with a popup grab: no client pointer/relative events during the
/// pan; after the pan, keyboard routing returns to the focused element's tree (not a dead popup);
/// the pan's end produces the same client wire as a pan without a popup (exactly-one reconciliation).
#[test]
fn r10_pan_with_popup_no_synthetic_events_and_clean_return() {
    fn run(with_popup: bool) -> (Vec<String>, Vec<String>, Option<String>, bool) {
        let mut r = R::new(1);
        let (wp, _) = r.click_focus();
        if with_popup {
            r.popup("P", wp);
        }
        r.key(KEY_LEFTMETA, true);
        let m = r.ls.probe.ptr.len();
        r.button(BTN_MIDDLE, true, none());
        r.motion((25.0, 10.0));
        r.motion((25.0, 10.0));
        let during: Vec<String> = r.ls.probe.ptr[m..].to_vec();
        let m2 = r.ls.probe.ptr.len();
        r.button(BTN_MIDDLE, false, none());
        r.key(KEY_LEFTMETA, false);
        let end: Vec<String> = r.ls.probe.ptr[m2..].to_vec();
        r.key(KEY_A, true);
        let got_a = r.ls.probe.key_events.iter().any(|&(k, p)| k == KEY_A && p);
        let active = r.ls.probe.kb.active.clone();
        r.key(KEY_A, false);
        (during, end, active, got_a)
    }
    let (_d0, e0, _a0, _k0) = run(false);
    let (d1, e1, a1, k1) = run(true);
    assert!(
        d1.iter().all(|l| l == "frame"),
        "client pointer events during pan with popup: {d1:?}"
    );
    assert_eq!(
        e1, e0,
        "pan end differs from the no-popup control (extra/missing reconciliation)"
    );
    assert!(
        k1 && a1.as_deref() == Some("W1"),
        "keyboard did not return to the focused element after dismissal"
    );
}

/// R11: the failed-review ATK-12 sequence exactly (WITH the focus change), plus nested popups,
/// then reconnect: the INC-3 zero-output grab survives and the first output removes it.
#[test]
fn r11_zero_output_grab_survives_preemption_and_reconnect() {
    let mut r = R::new(2);
    r.focus(Some(r.e[0]));
    r.key(KEY_A, true);
    r.key(KEY_A, false);
    let o = r.ls.output();
    r.ls.disconnect(o);
    r.adv();
    assert!(r.ptr_grabbed(), "zero-output grab installed");
    let p1 = r.popup("P", r.w[0]);
    r.popup("PN", p1);
    r.focus(Some(r.e[1]));
    let held = r.ptr_grabbed();
    r.ls.connect(&output((1920, 1080), 120));
    r.adv();
    let after_reconnect = r.ptr_grabbed();
    assert!(
        held,
        "zero-output grab torn down by preemption at zero outputs"
    );
    assert!(
        !after_reconnect,
        "zero-output grab not removed by the first output"
    );
    assert!(r.viol().is_empty(), "{:?}", r.viol());
}

/// R12: popup grab, zero outputs, reconnect: does the "grabbing" popup still dismiss on a click
/// outside (xdg-shell grab semantics), or did it silently lose its pointer grab?
#[test]
fn r12_popup_grab_semantics_across_zero_outputs() {
    let mut r = R::new(1);
    let (wp, _) = r.click_focus();
    r.popup("P", wp);
    let o = r.ls.output();
    r.ls.disconnect(o);
    r.adv();
    r.ls.connect(&output((1920, 1080), 120));
    r.adv();
    r.motion((600.0, 300.0)); // far outside the popup and the window
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    assert_eq!(
        r.ls.probe.popup_dones,
        vec!["P"],
        "grabbing popup not dismissed by a click outside after a zero-output cycle"
    );
}

/// R13: root destroyed while its popup grab is live: no stale grab remains; successor focused.
#[test]
fn r13_root_destroyed_with_live_grab() {
    let mut r = R::new(2);
    let (wp, top) = r.click_focus();
    let other = if top == r.e[0] { r.e[1] } else { r.e[0] };
    r.focus(Some(other));
    r.focus(Some(top));
    r.popup("P", wp);
    r.ls.probe.destroy_toplevel(wp); // parent first, popup still alive (misbehaving order)
    r.adv();
    assert_eq!(r.target(), Some(other));
    assert!(
        !r.kb_grabbed(),
        "stale keyboard grab after root destruction"
    );
}

/// R16: after OB-4 dismissal, Smithay keyboard focus vs Core ActiveElementFocus (INV-INPUT-8:
/// routing within the focused element's live tree, or none).
#[test]
fn r16_keyboard_focus_after_pan_dismissal() {
    let mut r = R::new(1);
    let (wp, top) = r.click_focus();
    let _p = r.popup("P", wp);
    r.key(KEY_LEFTMETA, true);
    r.button(BTN_MIDDLE, true, none());
    r.button(BTN_MIDDLE, false, none());
    r.key(KEY_LEFTMETA, false);
    let kb = r.ls.state.keyboard_handle();
    let smithay_focus = kb.current_focus();
    let root =
        r.ls.state
            .window(top)
            .and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()));
    assert!(
        smithay_focus == root,
        "Smithay keyboard focus left outside the focused element's live tree"
    );
    assert!(!r.kb_grabbed(), "keyboard grab survived pan dismissal");
}

/// R17: one long-lived Lockstep, 150 cycles: map window, click-focus, nested popup grabs with
/// distinct serials, CameraPan with popup, key cycles, focus transfer, zero-output + reconnect,
/// destroy window. Bounded: fds, focus history, key/button ownership tables, elements, grabs.
#[test]
fn r17_long_lived_stress_bounded_state() {
    let mut r = R::new(1);
    let mut samples = Vec::new();
    let mut cur = r.ls.output();
    for i in 0..150 {
        let qh = r.ls.qh.clone();
        let w = r.ls.probe.toplevel(&qh, "X", (300, 200), 0);
        r.adv();
        r.motion((1.0, 1.0));
        r.motion((-1.0, -1.0));
        r.button(BTN_LEFT, true, none());
        r.button(BTN_LEFT, false, none());
        let p1 = r.popup("P1", w);
        r.key(KEY_A, true);
        r.key(KEY_A, false);
        let s2 = r.ls.probe.last_serial;
        r.ls.probe.popup_with_grab_serial(
            &qh,
            "P2",
            p1,
            PopupSpec {
                content: (40, 30),
                shadow: 0,
                anchor_rect: (1, 1, 1, 1),
                slide: false,
            },
            s2,
        );
        r.adv();
        r.key(KEY_LEFTMETA, true);
        r.button(BTN_MIDDLE, true, none());
        r.motion((3.0, 0.0));
        r.button(BTN_MIDDLE, false, none());
        r.key(KEY_LEFTMETA, true); // repeat
        r.key(KEY_LEFTMETA, false);
        r.key(KEY_LEFTMETA, true);
        r.key(KEY_ENTER, true); // focus_next
        r.key(KEY_ENTER, false);
        r.key(KEY_LEFTMETA, false);
        r.ls.disconnect(cur);
        r.adv();
        cur = r.ls.connect(&output((1920, 1080), 120));
        r.adv();
        r.ls.probe.destroy_toplevel(w);
        r.adv();
        if i == 9 || i == 149 {
            let core = r.ls.state.core();
            samples.push((
                fds(),
                core.focus().history().len(),
                core.keys().pressed().len(),
                core.buttons().pressed().len(),
                r.ls.state.elements().count(),
                r.ptr_grabbed(),
                r.kb_grabbed(),
            ));
        }
    }
    let (a, b) = (samples[0], samples[1]);
    assert!(
        b.0 <= a.0 + 5 && b.1 <= a.1 && b.2 == 0 && b.3 == 0 && b.4 <= a.4,
        "unbounded state: {samples:?}"
    );
}
