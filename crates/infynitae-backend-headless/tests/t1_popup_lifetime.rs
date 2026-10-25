//! Production path tests for popup lifetime versus input routing (A1) and versus the parent's
//! lifecycle (B1), with a recording client.
//!
//! A popup's death has three separate consequences: (1) lifetime — the client's popup is gone;
//! (2) input — the popup grab chain must be reconciled with Core's ActiveElementFocus
//! (INV-INPUT-8/10); (3) scene — the dead surface stops being hit-testable (R2-01). None of them
//! is a commit of the parent (M1). Every expectation below is a literal derived from the test's
//! own sequence (labels, counts, wire lines); production state is only ever compared with it.

mod common;

use std::time::Duration;

use common::input::{BTN_LEFT, BTN_MIDDLE, none};
use common::{Lockstep, PopupSpec, output};
use infynitae_backend_headless::{keyboard_key, pointer_button, pointer_motion, set_focus};
use infynitae_core::input::keys::{KEY_A, KEY_B, KEY_LEFTMETA};
use infynitae_core::stage::{ElementId, Lifecycle};

const KEY_K: u32 = 37;

struct H {
    ls: Lockstep,
    t: u64,
    /// probe index and element of each toplevel, in creation order
    w: Vec<usize>,
    e: Vec<ElementId>,
}

impl H {
    /// `n` toplevels "W1".."Wn"; the client does NOT destroy popups on `popup_done` unless asked.
    fn new(n: usize) -> H {
        let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
        let qh = ls.qh.clone();
        let labels = ["W1", "W2", "W3"];
        let mut w = Vec::new();
        for l in labels.iter().take(n) {
            w.push(ls.probe.toplevel(&qh, l, (400, 300), 0));
        }
        ls.pump();
        let e: Vec<ElementId> = ls.state.elements().collect();
        assert_eq!(e.len(), n);
        let mut h = H {
            ls,
            t: 10_000,
            w,
            e,
        };
        h.adv();
        h
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
    fn button(&mut self, b: u32, p: bool) {
        self.t += 1000;
        pointer_button(&mut self.ls.state, &mut self.ls.input, b, p, none(), self.t)
            .expect("button");
        self.adv();
    }
    fn focus(&mut self, t: Option<ElementId>) {
        set_focus(&mut self.ls.state, &mut self.ls.input, t).expect("focus");
        self.adv();
    }
    fn label(&self, idx: usize) -> &'static str {
        self.ls.probe.surfaces[idx].label
    }
    /// Moves the pointer to `local` of `element`'s window geometry, wherever the pointer is now,
    /// and checks with the production hit-test that it arrived there.
    fn point_at(&mut self, element: ElementId, local: (f64, f64)) {
        let d = common::delta_to(&self.ls, element, local);
        self.motion(d);
        common::assert_pointer_at(&mut self.ls, element, local, "point_at");
    }
    /// Pointer onto the topmost window at its local (50, 50); a click focuses it. Returns the
    /// window's probe index, element and the press serial (the cause of a menu). The target is
    /// explicit and verified: the pointer is put on that window from wherever it is, the client
    /// must receive the press, and the window must hold the focus afterwards. (The earlier form
    /// was a relative motion: repeated in a loop it walked the pointer off the window.)
    fn click_focus(&mut self) -> (usize, ElementId, u32) {
        let top = *self
            .ls
            .state
            .core()
            .stage()
            .mapped_elements()
            .last()
            .expect("a mapped window");
        self.point_at(top, (50.0, 50.0));
        let before = self.ls.probe.ptr.len();
        self.button(BTN_LEFT, true);
        let s = self.ls.probe.last_serial;
        self.button(BTN_LEFT, false);
        let presses = self.ls.probe.ptr[before..]
            .iter()
            .filter(|l| l.starts_with("button") && l.ends_with("pressed"))
            .count();
        assert_eq!(presses, 1, "the client did not receive the focusing click");
        assert_eq!(
            self.ls.state.core().focus().target(),
            Some(top),
            "click-to-focus"
        );
        let i = self.e.iter().position(|&x| x == top).expect("element");
        (self.w[i], top, s)
    }
    /// A grabbing popup away from the pointer (no stale-hit interference), parent-relative `at`.
    fn popup(&mut self, label: &'static str, parent: usize, serial: u32, at: (i32, i32)) -> usize {
        let qh = self.ls.qh.clone();
        let p = self.ls.probe.popup_with_grab_serial(
            &qh,
            label,
            parent,
            PopupSpec {
                content: (40, 20),
                shadow: 0,
                anchor_rect: (at.0, at.1, 1, 1),
                slide: false,
            },
            serial,
        );
        self.adv();
        p
    }
    /// A menu chain of `depth` levels on `parent`, each level grabbed with a distinct serial (a
    /// later user event inside the previous level). Returns the popups outermost first.
    fn chain(&mut self, parent: usize, first_serial: u32, depth: usize) -> Vec<usize> {
        let labels = ["P1", "P2", "P3"];
        let mut ps = vec![self.popup(labels[0], parent, first_serial, (300, 250))];
        for d in 1..depth {
            self.key(KEY_A, true);
            self.key(KEY_A, false);
            let s = self.ls.probe.last_serial;
            ps.push(self.popup(labels[d], ps[d - 1], s, (2, 2)));
        }
        ps
    }
    fn destroy(&mut self, p: usize) {
        self.ls.probe.destroy_popup(p);
        self.adv();
    }
    fn root_surface(
        &self,
        id: ElementId,
    ) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        self.ls
            .state
            .window(id)
            .and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()))
    }
    fn enters_since(&self, mark: usize) -> Vec<String> {
        self.ls.probe.kb_log[mark..]
            .iter()
            .filter(|l| l.starts_with("wl_keyboard.enter"))
            .cloned()
            .collect()
    }
    /// The A1 oracle. `want_core`: Core's ActiveElementFocus; `want_client`: the label the client
    /// must have entered; then one probe key must reach exactly that surface legally.
    fn assert_routed(
        &mut self,
        ctx: &str,
        want_core: Option<ElementId>,
        want_client: Option<&str>,
    ) {
        assert_eq!(
            self.ls.state.core().focus().target(),
            want_core,
            "[{ctx}] Core focus"
        );
        let smithay = self.ls.state.keyboard_current_focus();
        match want_core {
            Some(id) if want_client.is_some_and(|c| c.starts_with('W')) => assert!(
                smithay == self.root_surface(id),
                "[{ctx}] Smithay keyboard focus is not the root of Core's focus"
            ),
            None => assert!(
                smithay.is_none(),
                "[{ctx}] Smithay focus while Core is unfocused"
            ),
            _ => {}
        }
        if want_client.is_some_and(|c| c.starts_with('W')) || want_client.is_none() {
            assert!(
                !self.ls.state.keyboard_is_grabbed(),
                "[{ctx}] stale keyboard grab"
            );
            assert!(
                !self.ls.state.is_pointer_grabbed(),
                "[{ctx}] stale pointer grab"
            );
        }
        assert_eq!(
            self.ls.probe.kb.active.as_deref(),
            want_client,
            "[{ctx}] client focus"
        );
        let v0 = self.ls.probe.kb.violations.len();
        let n = self.ls.probe.key_events.len();
        self.key(KEY_K, true);
        assert_eq!(
            self.ls.probe.kb.active.as_deref(),
            want_client,
            "[{ctx}] the key moved the client's focus (lazy refocus)"
        );
        self.key(KEY_K, false);
        let got = self.ls.probe.key_events.len() - n;
        let want = if want_client.is_some() { 2 } else { 0 };
        assert_eq!(got, want, "[{ctx}] probe key delivery");
        assert!(
            self.ls.probe.kb.violations[v0..].is_empty(),
            "[{ctx}] wl_keyboard violations: {:?}",
            &self.ls.probe.kb.violations[v0..]
        );
    }
}

// ================================================================ A1: client-driven close

/// Depth 1/2/3: the client closes its menu chain itself in protocol order (topmost first); the
/// keyboard is back on the window with exactly ONE enter, before any key, and the next key is
/// legal.
#[test]
fn a1_client_closes_chain_of_depth_1_2_3() {
    for depth in 1..=3 {
        let mut h = H::new(2);
        let (wp, top, s) = h.click_focus();
        let ps = h.chain(wp, s, depth);
        assert_eq!(
            h.ls.probe.kb.active.as_deref(),
            Some(h.label(*ps.last().unwrap()))
        );
        let mark = h.ls.probe.kb_log.len();
        for &p in ps.iter().rev() {
            h.destroy(p);
        }
        let root = h.label(wp);
        // xdg_popup.grab: when the topmost grabbing popup dies the grab returns to its parent
        // popup — each surviving level is entered exactly once, outermost last, then the window
        // exactly once; no lazy or duplicate re-entry
        let mut want: Vec<String> = ps[..depth - 1]
            .iter()
            .rev()
            .map(|&p| format!("wl_keyboard.enter {} keys={{}}", h.label(p)))
            .collect();
        want.push(format!("wl_keyboard.enter {root} keys={{}}"));
        assert_eq!(
            h.enters_since(mark),
            want,
            "depth {depth}: re-entries during the close"
        );
        h.assert_routed(&format!("depth {depth}"), Some(top), Some(root));
    }
}

/// The inner popup dies, the outer one lives: the keyboard moves to the outer popup at once
/// (not lazily inside the next key), the grab stays.
#[test]
fn a1_inner_dies_outer_kept() {
    let mut h = H::new(2);
    let (wp, top, s) = h.click_focus();
    let ps = h.chain(wp, s, 2);
    h.destroy(ps[1]);
    assert!(h.ls.state.keyboard_is_grabbed(), "outer popup still grabs");
    h.assert_routed("inner only", Some(top), Some("P1"));
    // the outer one dies later: back on the window
    h.destroy(ps[0]);
    h.assert_routed("outer later", Some(top), Some(h.label(wp)));
}

/// A client destroys the MIDDLE of three (a protocol violation Smithay 0.7.0 does not report):
/// routing stays on the topmost live popup; the remaining chain then closes cleanly.
#[test]
fn a1_middle_destroyed_tolerated() {
    let mut h = H::new(2);
    let (wp, top, s) = h.click_focus();
    let ps = h.chain(wp, s, 3);
    h.destroy(ps[1]);
    h.assert_routed("middle", Some(top), Some("P3"));
    h.destroy(ps[2]);
    h.destroy(ps[0]);
    h.assert_routed("rest", Some(top), Some(h.label(wp)));
}

/// The whole chain dies in one client flush (no dispatch in between).
#[test]
fn a1_rapid_chain_destroy() {
    let mut h = H::new(2);
    let (wp, top, s) = h.click_focus();
    let ps = h.chain(wp, s, 3);
    for &p in ps.iter().rev() {
        h.ls.probe.destroy_popup(p);
    }
    h.adv();
    h.assert_routed("rapid", Some(top), Some(h.label(wp)));
}

/// Focus moves to another window while the chain is being closed (preemption dismisses the
/// rest); the remaining destruction must not pull the keyboard back.
#[test]
fn a1_focus_other_root_during_destruction() {
    let mut h = H::new(2);
    let (wp, top, s) = h.click_focus();
    let other = if top == h.e[0] { h.e[1] } else { h.e[0] };
    let other_label = h.label(h.w[h.e.iter().position(|&x| x == other).unwrap()]);
    let ps = h.chain(wp, s, 3);
    h.destroy(ps[2]);
    h.focus(Some(other));
    h.destroy(ps[1]);
    h.destroy(ps[0]);
    h.assert_routed("focus moved", Some(other), Some(other_label));
}

/// The root unmaps while its chain is half closed: Core's successor gets the keyboard.
#[test]
fn a1_root_unmaps_during_destruction() {
    let mut h = H::new(2);
    let (wp, top, s) = h.click_focus();
    let other = if top == h.e[0] { h.e[1] } else { h.e[0] };
    h.focus(Some(other));
    h.focus(Some(top));
    let other_label = h.label(h.w[h.e.iter().position(|&x| x == other).unwrap()]);
    let ps = h.chain(wp, s, 2);
    h.destroy(ps[1]);
    h.ls.probe.unmap(wp);
    h.adv();
    h.destroy(ps[0]);
    h.assert_routed("root unmapped", Some(other), Some(other_label));
}

/// The root is destroyed while its chain is half closed.
#[test]
fn a1_root_destroyed_during_destruction() {
    let mut h = H::new(2);
    let (wp, top, s) = h.click_focus();
    let other = if top == h.e[0] { h.e[1] } else { h.e[0] };
    h.focus(Some(other));
    h.focus(Some(top));
    let other_label = h.label(h.w[h.e.iter().position(|&x| x == other).unwrap()]);
    let ps = h.chain(wp, s, 2);
    h.destroy(ps[1]);
    h.destroy(ps[0]);
    h.ls.probe.destroy_toplevel(wp);
    h.adv();
    h.assert_routed("root destroyed", Some(other), Some(other_label));
}

/// Zero outputs while the chain closes, then reconnect: keyboard routing is output-independent
/// (K8) and no grab is resurrected.
#[test]
fn a1_zero_outputs_during_destruction_then_reconnect() {
    let mut h = H::new(2);
    let (wp, top, s) = h.click_focus();
    let ps = h.chain(wp, s, 3);
    let o = h.ls.output();
    h.ls.disconnect(o);
    h.adv();
    for &p in ps.iter().rev() {
        h.destroy(p);
    }
    assert!(
        !h.ls.state.keyboard_is_grabbed(),
        "keyboard grab survived the close"
    );
    assert_eq!(h.ls.probe.kb.active.as_deref(), Some(h.label(wp)));
    h.ls.connect(&output((1920, 1080), 120));
    h.adv();
    h.assert_routed("reconnected", Some(top), Some(h.label(wp)));
}

/// CameraPan dismisses the chain; the client closes it during the pan: no pointer event during
/// the pan, the window owns the keyboard, no grab after the first post-pan motion.
#[test]
fn a1_camerapan_overlapping_destruction() {
    let mut h = H::new(2);
    let (wp, top, s) = h.click_focus();
    let ps = h.chain(wp, s, 3);
    h.key(KEY_LEFTMETA, true);
    let m = h.ls.probe.ptr.len();
    h.button(BTN_MIDDLE, true);
    for &p in ps.iter().rev() {
        h.destroy(p);
    }
    h.motion((5.0, 0.0));
    let during: Vec<String> = h.ls.probe.ptr[m..].to_vec();
    h.button(BTN_MIDDLE, false);
    h.key(KEY_LEFTMETA, false);
    assert!(
        during.iter().all(|l| l == "frame"),
        "pointer events during the pan: {during:?}"
    );
    h.motion((1.0, 0.0));
    h.assert_routed("pan overlap", Some(top), Some(h.label(wp)));
}

// ================================================================ W-SM-15 / F-4 exact wire

/// ORDERED preemption (INV-INPUT-10) on the client wire, single and nested: the popups are
/// dismissed topmost first, the keyboard leaves the popup once and enters the new window EXACTLY
/// once — no transient re-entry of the old root (W-SM-15, the pointer-first teardown symptom).
#[test]
fn ordered_preemption_exact_keyboard_wire() {
    for depth in [1usize, 2] {
        let mut h = H::new(2);
        let (wp, top, s) = h.click_focus();
        let other = if top == h.e[0] { h.e[1] } else { h.e[0] };
        let other_label = h.label(h.w[h.e.iter().position(|&x| x == other).unwrap()]);
        let ps = h.chain(wp, s, depth);
        let topmost = h.label(*ps.last().unwrap());
        let mark = h.ls.probe.kb_log.len();
        h.focus(Some(other));
        let wire: Vec<String> = h.ls.probe.kb_log[mark..]
            .iter()
            .filter(|l| !l.starts_with("toplevel_configure"))
            .cloned()
            .collect();
        let mut want: Vec<String> = ps
            .iter()
            .rev()
            .map(|&p| format!("popup_done {}", h.label(p)))
            .collect();
        want.push(format!("wl_keyboard.leave {topmost}"));
        want.push(format!("wl_keyboard.enter {other_label} keys={{}}"));
        want.push("wl_keyboard.modifiers dep=0x0 lat=0x0 lock=0x0".to_string());
        assert_eq!(wire, want, "depth {depth}: preemption keyboard wire");
        assert!(!h.ls.state.keyboard_is_grabbed() && !h.ls.state.is_pointer_grabbed());
    }
}

// ================================================================ B1: lifetime is not a commit

fn configures(h: &H, label: &str) -> usize {
    h.ls.probe
        .configures
        .iter()
        .filter(|l| **l == label)
        .count()
}

/// Mapped parent: a destroyed popup changes the hit-test tree (its area hits the parent again)
/// but no lifecycle transition and no configure of the parent.
#[test]
fn b1_mapped_parent_popup_destroyed_is_not_a_parent_commit() {
    let mut h = H::new(1);
    let (wp, top, s) = h.click_focus();
    let qh = h.ls.qh.clone();
    // a popup right under the pointer (W-local 50,50 is inside its [10,130) × [10,90))
    let p = h.ls.probe.popup_with_grab_serial(
        &qh,
        "P",
        wp,
        PopupSpec {
            content: (120, 80),
            shadow: 0,
            anchor_rect: (10, 10, 1, 1),
            slide: false,
        },
        s,
    );
    h.adv();
    let c0 = configures(&h, h.label(wp));
    h.destroy(p);
    assert_eq!(
        h.ls.state.core().stage().state(top),
        Some(Lifecycle::Mapped)
    );
    assert_eq!(
        configures(&h, h.label(wp)),
        c0,
        "parent configured by a popup's death"
    );
    // R2-01: the former popup area hits the parent
    let m = h.ls.probe.ptr.len();
    h.motion((1.0, 1.0));
    h.button(BTN_LEFT, true);
    h.button(BTN_LEFT, false);
    let wire = &h.ls.probe.ptr[m..];
    assert_eq!(
        wire.iter().filter(|l| l.starts_with("button")).count(),
        2,
        "former popup area swallowed the click: {wire:?}"
    );
}

/// Unmap with a menu open (the client destroys the dismissed popup), then re-show the protocol
/// way: initial commit without a buffer must be answered by EXACTLY ONE configure; the parent
/// stays Unmapped until then.
#[test]
fn b1_unmapped_parent_reshow_gets_exactly_one_configure() {
    for with_popup in [false, true] {
        let mut h = H::new(2);
        h.ls.probe.auto_destroy_popups = true;
        let (wp, top, s) = h.click_focus();
        if with_popup {
            h.popup("P", wp, s, (300, 250));
        }
        h.ls.probe.unmap(wp);
        h.adv();
        assert_eq!(
            h.ls.state.core().stage().state(top),
            Some(Lifecycle::Unmapped),
            "with_popup={with_popup}: parent left Unmapped without a commit"
        );
        let c0 = configures(&h, h.label(wp));
        h.ls.probe.surfaces[wp].wl.commit();
        h.adv();
        assert_eq!(
            configures(&h, h.label(wp)) - c0,
            1,
            "with_popup={with_popup}: configures answering the re-initial commit"
        );
        assert_eq!(
            h.ls.state.core().stage().state(top),
            Some(Lifecycle::AwaitingFirstBuffer)
        );
    }
}

/// A child surface's commit or death never runs its parent's lifecycle: a toplevel that has not
/// made its initial commit is not configured (nor placed) because of its popup.
#[test]
fn b1_uncommitted_parent_untouched_by_popup_commit_or_death() {
    let mut h = H::new(1);
    let (_, _, s) = h.click_focus();
    let qh = h.ls.qh.clone();
    let x = h.ls.probe.toplevel_uncommitted(&qh, "X", (200, 200));
    h.adv();
    let p = h.ls.probe.popup_with_grab_serial(
        &qh,
        "PX",
        x,
        PopupSpec {
            content: (40, 30),
            shadow: 0,
            anchor_rect: (1, 1, 1, 1),
            slide: false,
        },
        s,
    );
    h.adv();
    assert_eq!(configures(&h, "X"), 0, "X configured by its popup's commit");
    h.destroy(p);
    assert_eq!(configures(&h, "X"), 0, "X configured by its popup's death");
    let xid = h.ls.state.elements().last().unwrap();
    assert_eq!(
        h.ls.state.core().stage().state(xid),
        Some(Lifecycle::AwaitingInitialCommit)
    );
}

/// Pending (uncommitted) parent state is not promoted by a popup's death: the window geometry the
/// client set but did not commit must not move the surface; its own commit does.
#[test]
fn b1_pending_parent_state_not_promoted() {
    let mut h = H::new(1);
    let (wp, _, s) = h.click_focus();
    let p = h.popup("P", wp, s, (300, 250));
    let probe = |h: &mut H| {
        let m = h.ls.probe.ptr.len();
        h.motion((1.0, 0.0));
        h.motion((-1.0, 0.0));
        h.ls.probe.ptr[m..]
            .iter()
            .rev()
            .find(|l| l.starts_with("motion"))
            .cloned()
    };
    let before = probe(&mut h);
    // pending geometry offset (10,10), not committed
    h.ls.probe.surfaces[wp]
        .xdg
        .as_ref()
        .expect("xdg")
        .set_window_geometry(10, 10, 380, 280);
    h.destroy(p);
    let after_destroy = probe(&mut h);
    assert_eq!(
        after_destroy, before,
        "pending parent geometry promoted by a popup's death"
    );
    h.ls.probe.surfaces[wp].wl.commit();
    h.adv();
    let after_commit = probe(&mut h);
    assert_ne!(
        after_commit, before,
        "control: the parent's own commit applies the geometry"
    );
}

/// Nested: the inner popup's death updates the hit-test tree (its area falls through to the
/// outer popup / window) without touching the parent's lifecycle; zero outputs + reconnect and
/// repeated create/destroy leave no hole and no extra configure.
#[test]
fn b1_nested_zero_output_and_repeated_destroy() {
    let mut h = H::new(1);
    h.ls.probe.auto_destroy_popups = true;
    let (wp, top, s) = h.click_focus();
    let c0 = configures(&h, h.label(wp));
    let ps = h.chain(wp, s, 2);
    h.destroy(ps[1]);
    h.destroy(ps[0]);
    let o = h.ls.output();
    h.ls.disconnect(o);
    h.adv();
    let qh = h.ls.qh.clone();
    let p = h.ls.probe.popup_with_grab_serial(
        &qh,
        "PZ",
        wp,
        PopupSpec {
            content: (120, 80),
            shadow: 0,
            anchor_rect: (10, 10, 1, 1),
            slide: false,
        },
        h.ls.probe.last_serial,
    );
    h.adv();
    h.destroy(p);
    h.ls.connect(&output((1920, 1080), 120));
    h.adv();
    let mut delivered = 0;
    for _ in 0..20 {
        let s = h.ls.probe.last_serial;
        let p = h.popup("PR", wp, s, (10, 10));
        h.destroy(p);
        let m = h.ls.probe.ptr.len();
        h.button(BTN_LEFT, true);
        h.button(BTN_LEFT, false);
        delivered += h.ls.probe.ptr[m..]
            .iter()
            .filter(|l| l.starts_with("button"))
            .count();
    }
    assert_eq!(delivered, 40, "clicks lost to dead popup areas");
    assert_eq!(
        configures(&h, h.label(wp)),
        c0,
        "parent configured by popup churn"
    );
    assert_eq!(
        h.ls.state.core().stage().state(top),
        Some(Lifecycle::Mapped)
    );
    assert!(
        h.ls.probe.kb.violations.is_empty(),
        "{:?}",
        h.ls.probe.kb.violations
    );
}

// ================================================================ long-lived combined stress

fn fds() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .map(|d| d.count())
        .unwrap_or(0)
}

/// 200 cycles in one session: depth-1/2/3 chains closed by the client or dismissed by the
/// compositor (focus change, CameraPan with destruction during the pan), duplicate modifiers,
/// zero outputs + reconnect, >64 provenance events. The end state is asserted explicitly.
#[test]
fn round4_long_lived_combined_stress() {
    let mut h = H::new(2);
    let mut cur = h.ls.output();
    let fd0 = fds();
    for i in 0..200usize {
        let (wp, top, s) = h.click_focus();
        let other = if top == h.e[0] { h.e[1] } else { h.e[0] };
        let dismissed = h.ls.probe.popup_dones.len();
        let ps = h.chain(wp, s, 1 + i % 3);
        // the cycle is only a stress of the grab paths if the chain really holds the grabs
        assert!(
            h.ls.state.keyboard_is_grabbed() && h.ls.state.is_pointer_grabbed(),
            "cycle {i}: the menu chain of depth {} holds no grab",
            1 + i % 3
        );
        assert_eq!(
            h.ls.probe.popup_dones.len(),
            dismissed,
            "cycle {i}: a popup of the chain was dismissed before the cycle acted"
        );
        h.key(KEY_LEFTMETA, true);
        h.key(KEY_LEFTMETA, true); // duplicate modifier
        h.key(KEY_LEFTMETA, false);
        match i % 4 {
            0 => {
                for &p in ps.iter().rev() {
                    h.destroy(p);
                }
            }
            1 => {
                h.focus(Some(other));
                for &p in ps.iter().rev() {
                    h.destroy(p);
                }
            }
            2 => {
                h.key(KEY_LEFTMETA, true);
                h.button(BTN_MIDDLE, true);
                for &p in ps.iter().rev() {
                    h.ls.probe.destroy_popup(p);
                }
                h.motion((2.0, 0.0));
                h.button(BTN_MIDDLE, false);
                h.key(KEY_LEFTMETA, false);
                h.motion((-2.0, 0.0));
            }
            _ => {
                h.ls.disconnect(cur);
                h.adv();
                for &p in ps.iter().rev() {
                    h.destroy(p);
                }
                cur = h.ls.connect(&output((1920, 1080), 120));
                h.adv();
            }
        }
        h.key(KEY_B, true);
        h.key(KEY_B, false);
    }
    let fd1 = fds();
    let core = h.ls.state.core();
    let target = core.focus().target();
    assert!(
        h.ls.probe.kb.violations.is_empty(),
        "violations: {:?}",
        h.ls.probe.kb.violations
    );
    assert_eq!(h.ls.state.keyboard_modifier_state().serialized.depressed, 0);
    assert_eq!(h.ls.probe.kb.depressed, 0);
    assert!(
        core.keys().pressed().is_empty(),
        "key ownership {:?}",
        core.keys().pressed()
    );
    assert!(core.buttons().pressed().is_empty(), "button ownership");
    assert!(!h.ls.state.is_pointer_grabbed(), "pointer grab");
    assert!(!h.ls.state.keyboard_is_grabbed(), "keyboard grab");
    assert!(h.ls.state.serial_outputs_len() <= 64);
    assert!(target.is_some());
    assert!(
        h.ls.state.keyboard_current_focus() == h.root_surface(target.unwrap()),
        "Smithay focus != Core focus root"
    );
    assert!(fd1 <= fd0 + 5, "fds {fd0} -> {fd1}");
    println!(
        "ROUND4-STRESS cycles=200 fds {fd0}->{fd1} serials={}",
        h.ls.state.serial_outputs_len()
    );
}
