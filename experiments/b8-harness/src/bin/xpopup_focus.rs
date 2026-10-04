//! Phase D 1R (PD-F2-02): desktop focus (core ActiveElementFocus) vs Smithay popup-grab protocol
//! routing. A grabbing xdg_popup is open on W1; core then changes ActiveElementFocus. Three adapter
//! strategies are compared:
//!   NAIVE         keyboard.set_focus(target) while the grab is active (the sealed C4.5 path)
//!   POINTER-FIRST grab.ungrab(All); pointer.unset_grab; keyboard.unset_grab; keyboard.set_focus(target)
//!   ORDERED       grab.ungrab(All); keyboard.unset_grab; pointer.unset_grab; keyboard.set_focus(target)
//! Checked after every scenario:
//!   F-1 Smithay keyboard focus == core ActiveElementFocus target (no divergence)
//!   F-2 the client's logical keyboard focus == target (wl_keyboard enter/leave as seen by the client)
//!   F-3 no wl_keyboard / wl_pointer protocol-order violation (duplicate enter, leave without enter)
//!   F-4 exactly one keyboard transition into the target (no transient focus on another surface)
//!   F-5 every grabbing popup received popup_done; no panic (W-SM-7 / Smithay asserts)
//!   F-6 no stuck key: client keys == physically held client-owned keys
//! Output: docs/foundation/evidence/phase-d/xpopup-focus-output.txt
use b8_harness::{keys::KeyIntake, lockstep::Lockstep, server::*};
use smithay::{desktop::PopupUngrabStrategy, reexports::wayland_server::protocol::wl_surface::WlSurface, utils::SERIAL_COUNTER};

const KEY_A: u32 = 30;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Strategy {
    Naive,
    PointerFirst,
    Ordered,
}

struct S {
    ls: Lockstep,
    ix: KeyIntake,
    w: Vec<usize>,
    popups: Vec<usize>,
    /// core ActiveElementFocus (element index)
    focus: Option<usize>,
    /// scenario-specific extra check (e.g. nested-grab routing)
    extra_ok: bool,
}

fn setup() -> S {
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 60_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = false;
    ls.server.popup_grabs_enabled = true;
    ls.probe.auto_destroy_popups = true;
    let qh = ls.qh.clone();
    ls.probe.bind_input(&ls.globals, &qh);
    ls.server.pending_anchors.push_back((100, 100));
    ls.server.pending_anchors.push_back((1000, 100));
    let w1 = ls.probe.create_toplevel(&qh, "W1", (400, 300), 0, false);
    let w2 = ls.probe.create_toplevel(&qh, "W2", (400, 300), 0, false);
    ls.pump();
    ls.run_until(10_000);
    let mut s = S { ls, ix: KeyIntake::new(vec![], true), w: vec![w1, w2], popups: vec![], focus: None, extra_ok: true };
    s.core_focus_plain(Some(0));
    s
}

impl S {
    fn advance(&mut self) {
        self.ls.pump();
        let t = self.ls.server.now_us + 1000;
        self.ls.run_until(t);
    }
    fn root(&self, e: usize) -> Option<WlSurface> {
        self.ls.server.elements.get(e).and_then(|el| el.window.toplevel().map(|t| t.wl_surface().clone()))
    }
    fn core_focus_plain(&mut self, e: Option<usize>) {
        let surf = e.and_then(|e| self.root(e));
        let kb = self.ls.server.seat.get_keyboard().unwrap();
        kb.set_focus(&mut self.ls.server, surf, SERIAL_COUNTER.next_serial());
        self.focus = e;
        self.advance();
    }
    fn open_popup(&mut self, label: &str, parent: usize) {
        let qh = self.ls.qh.clone();
        let p = self.ls.probe.create_popup_ex(&qh, label, parent, (120, 80), 0, (10, 10, 1, 1), true);
        self.popups.push(p);
        self.advance();
    }
    /// The adapter step under test: core changed ActiveElementFocus to `target`.
    fn core_focus(&mut self, target: Option<usize>, strat: Strategy) {
        let srv = &mut self.ls.server;
        let surf = target.and_then(|e| srv.elements.get(e)).and_then(|el| el.window.toplevel().map(|t| t.wl_surface().clone()));
        let kb = srv.seat.get_keyboard().unwrap();
        let ptr = srv.seat.get_pointer().unwrap();
        let serial = SERIAL_COUNTER.next_serial();
        if strat != Strategy::Naive {
            if let Some(mut g) = srv.popup_grab.take() {
                g.ungrab(PopupUngrabStrategy::All); // popup_done, topmost first
            }
            match strat {
                Strategy::PointerFirst => {
                    ptr.unset_grab(srv, serial, 0);
                    kb.unset_grab(srv);
                }
                _ => {
                    kb.unset_grab(srv);
                    ptr.unset_grab(srv, serial, 0);
                }
            }
        }
        kb.set_focus(srv, surf, serial);
        self.focus = target;
        self.advance();
    }
    fn label(&self, e: Option<usize>) -> Option<String> {
        e.map(|e| self.ls.probe.surfaces[self.w[e]].label.clone())
    }
}

fn run(title: &str, f: impl FnOnce(&mut S) -> u64) -> bool {
    println!("=== {title}");
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut s = setup();
        let t0 = f(&mut s);
        let srv_focus = s.ls.server.seat.get_keyboard().unwrap().current_focus();
        let want = s.focus.and_then(|e| s.root(e));
        let f1 = srv_focus == want;
        let client_focus = s.ls.probe.kb.active.clone();
        let f2 = client_focus == s.label(s.focus);
        let f3 = s.ls.probe.kb.violations.is_empty() && s.ls.probe.ptr_violations.is_empty();
        let kb_lines: Vec<String> = s.ls.probe.log.iter().filter(|(t, l)| *t >= t0 && (l.contains("wl_keyboard.enter") || l.contains("wl_keyboard.leave"))).map(|(_, l)| l.clone()).collect();
        let enters = kb_lines.iter().filter(|l| l.contains("enter")).count();
        let f4 = enters == usize::from(s.focus.is_some());
        let dones = s.ls.probe.log.iter().filter(|(t, l)| *t >= t0 && l.contains("popup_done")).count();
        let f5 = dones == s.popups.len() || s.popups.is_empty();
        let held: std::collections::BTreeSet<u32> = s.ix.held.difference(&s.ix.owned).copied().collect();
        let f6 = s.ls.probe.kb.active.is_none() || s.ls.probe.kb.keys == held;
        println!("  -- client events before the focus change (popup open / grab):");
        for (t, l) in s.ls.probe.log.iter().filter(|(t, l)| *t < t0 && *t >= 10_000 && (l.contains("keyboard") || l.contains("popup") || l.contains("pointer."))) {
            println!("       t={:>6.1}ms {l}", *t as f64 / 1e3);
        }
        for (t, l) in s.ls.server.log.iter().filter(|(_, l)| l.contains("grab")) {
            println!("       t={:>6.1}ms server: {l}", *t as f64 / 1e3);
        }
        println!("  -- client events since the focus change:");
        for (t, l) in s.ls.probe.log.iter().filter(|(t, l)| *t >= t0 && (l.contains("keyboard") || l.contains("popup") || l.contains("pointer.") || l.contains("client-side"))) {
            println!("       t={:>6.1}ms {l}", *t as f64 / 1e3);
        }
        for v in s.ls.probe.kb.violations.iter().chain(s.ls.probe.ptr_violations.iter()) {
            println!("       VIOLATION {v}");
        }
        println!(
            "  core ActiveElementFocus={:?} | Smithay keyboard focus={} | client logical focus={:?}",
            s.label(s.focus),
            srv_focus.as_ref().map(|x| { let id: u32 = surface_id(x).parse().unwrap(); s.ls.probe.surfaces.iter().find(|q| wayland_client::Proxy::id(&q.wl).protocol_id() == id).map(|q| q.label.clone()).unwrap_or(format!("#{id}")) }).unwrap_or("none".into()),
            client_focus
        );
        println!("  F-1 core==Smithay {f1} | F-2 client==core {f2} | F-3 no protocol-order violation {f3} | F-4 single transition (enters={enters}) {f4} | F-5 popup_done {dones}/{} {f5} | F-6 no stuck key {f6}", s.popups.len());
        if !s.extra_ok {
            println!("  scenario-specific check FAILED (see observation)");
        }
        f1 && f2 && f3 && f4 && f5 && f6 && s.extra_ok
    }));
    let pass = match r {
        Ok(p) => p,
        Err(_) => {
            println!("  PANIC (Smithay assertion or harness)");
            false
        }
    };
    println!("  RESULT {title}: {}\n", if pass { "PASS" } else { "FAIL" });
    pass
}

fn main() {
    println!("xpopup_focus — smithay =0.7.0 PopupManager::grab_popup + PopupKeyboardGrab/PopupPointerGrab; lockstep\n");
    let mut res: Vec<(&str, bool, bool)> = Vec::new(); // (name, pass, expected_pass)
    let p = run("NEG-1 grab active -> core focuses W2, NAIVE set_focus (sealed path)", |s| {
        s.open_popup("P", s.w[0]);
        let t0 = s.ls.server.now_us;
        s.core_focus(Some(1), Strategy::Naive);
        t0
    });
    res.push(("NEG-1 naive", p, false));
    let p = run("NEG-2 grab active -> core focuses W2, POINTER-FIRST unset order", |s| {
        s.open_popup("P", s.w[0]);
        let t0 = s.ls.server.now_us;
        s.core_focus(Some(1), Strategy::PointerFirst);
        t0
    });
    res.push(("NEG-2 pointer-first", p, false));
    let p = run("PF-1 grab active -> core focuses W2, ORDERED", |s| {
        s.open_popup("P", s.w[0]);
        let t0 = s.ls.server.now_us;
        s.core_focus(Some(1), Strategy::Ordered);
        t0
    });
    res.push(("PF-1 focus other element", p, true));
    let p = run("PF-2 grab active -> core focus none, ORDERED", |s| {
        s.open_popup("P", s.w[0]);
        let t0 = s.ls.server.now_us;
        s.core_focus(None, Strategy::Ordered);
        t0
    });
    res.push(("PF-2 focus none", p, true));
    let p = run("PF-3 popup parent W1 destroyed by its client while the grab is active -> successor W2, ORDERED", |s| {
        s.open_popup("P", s.w[0]);
        let t0 = s.ls.server.now_us;
        // client tears down: popup first (protocol order), then its parent toplevel, in one flush
        let pi = s.popups[0];
        s.ls.probe.destroy_popup(pi);
        s.popups.clear();
        s.ls.probe.destroy_toplevel(s.w[0]);
        s.ls.pump();
        // core: target loss -> successor W2 (element index shifts: W1 removed from the Stage)
        s.w.remove(0);
        s.core_focus(Some(0), Strategy::Ordered);
        t0
    });
    res.push(("PF-3 parent destroyed", p, true));
    let p = run("PF-4 popup destroyed by the client in flight while core changes focus (destroy not yet dispatched), ORDERED", |s| {
        s.open_popup("P", s.w[0]);
        let t0 = s.ls.server.now_us;
        let pi = s.popups[0];
        s.ls.probe.destroy_popup(pi); // queued in the client, not yet flushed
        s.popups.clear();
        s.core_focus(Some(1), Strategy::Ordered); // server dismisses a popup the client already destroyed
        t0
    });
    res.push(("PF-4 popup destroyed in flight", p, true));
    let p = run("PF-5 popup destroyed and dispatched first (grab ended), then core focuses W2, ORDERED", |s| {
        s.open_popup("P", s.w[0]);
        let t0 = s.ls.server.now_us;
        let pi = s.popups[0];
        s.ls.probe.destroy_popup(pi);
        s.popups.clear();
        s.advance();
        s.core_focus(Some(1), Strategy::Ordered);
        t0
    });
    res.push(("PF-5 popup destroyed first", p, true));
    for (fix, name, exp) in [(false, "OBS-1 nested, grab REPLACES pointer grab (anvil order)", false), (true, "PF-6 nested, nested grab KEEPS the pointer grab", true)] {
        let p = run(&format!("{name}: P1 -> P2, key routing check, then core focuses W2, ORDERED"), |s| {
            s.ls.server.nested_grab_keep_pointer = fix;
            s.open_popup("P1", s.w[0]);
            let parent = s.popups[0];
            s.open_popup("P2", parent);
            s.ix.key(&mut s.ls.server, KEY_A, true);
            s.advance();
            s.ix.key(&mut s.ls.server, KEY_A, false);
            s.advance();
            let routed = s.ls.probe.kb.active.clone();
            let kbg = s.ls.server.seat.get_keyboard().unwrap().is_grabbed();
            println!("  observation: after the nested grab a key is routed to {routed:?}; keyboard grab still installed: {kbg}");
            s.extra_ok = routed.as_deref() == Some("P2") && kbg;
            let t0 = s.ls.server.now_us;
            s.core_focus(Some(1), Strategy::Ordered);
            t0
        });
        res.push((name, p, exp));
    }
    let p = run("PF-7 client-owned key A held during the grab -> core focuses W2, ORDERED -> release A", |s| {
        s.open_popup("P", s.w[0]);
        s.ix.key(&mut s.ls.server, KEY_A, true); // forwarded to the popup (grab focus)
        s.advance();
        let t0 = s.ls.server.now_us;
        s.core_focus(Some(1), Strategy::Ordered);
        s.ix.key(&mut s.ls.server, KEY_A, false); // must reach W2, where A is logically down via enter(keys)
        s.advance();
        t0
    });
    res.push(("PF-7 held key", p, true));
    println!("SUMMARY");
    let mut ok = true;
    for (n, p, exp) in &res {
        let as_expected = p == exp;
        ok &= as_expected;
        println!("  {n:<32} {} (expected {}) {}", if *p { "PASS" } else { "FAIL" }, if *exp { "PASS" } else { "FAIL" }, if as_expected { "" } else { "<-- UNEXPECTED" });
    }
    println!("\nALL AS EXPECTED: {ok}");
}
