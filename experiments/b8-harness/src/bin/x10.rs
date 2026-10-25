//! X-10: keyboard / modifier ownership across bindings, focus transitions, destruction and
//! cancellation, observed at the client with a wl_keyboard protocol checker.
//! Output: docs/archive/evidence/phase-c/x10-keyboard-output.txt
//!
//! Invariants checked after EVERY step:
//!  KV-1 no wl_keyboard protocol violation (enter/leave/key/modifiers MUST / MUST NOT rules)
//!  KV-2 if the client has keyboard focus: client logical keys == physically held client-owned keys
//!  KV-3 if the client has keyboard focus: client (depressed, latched, locked) == Smithay seat XKB
//!  KV-4 at scenario end (no physical key held): client holds no key and no depressed/latched modifier

use b8_harness::{
    keys::{Action, KeyBinding, KeyIntake},
    lockstep::Lockstep,
    server::*,
};
use smithay::utils::SERIAL_COUNTER;

const KEY_ESC: u32 = 1;
const KEY_ENTER: u32 = 28;
const KEY_A: u32 = 30;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_B: u32 = 48;
const KEY_LEFTMETA: u32 = 125;

fn name(k: u32) -> &'static str {
    match k {
        KEY_ESC => "Esc",
        KEY_ENTER => "Enter",
        KEY_A => "A",
        KEY_LEFTSHIFT => "Shift",
        KEY_B => "B",
        KEY_LEFTMETA => "Super",
        _ => "?",
    }
}

struct S {
    ls: Lockstep,
    ix: KeyIntake,
    surfaces: Vec<usize>, // probe surface idx per element
    focus: Option<usize>, // element index with keyboard focus (core focus model stand-in)
    ok: bool,
    t_step: u64,
}

fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding { evdev: KEY_ENTER, needs_logo: true, action: Action::FocusNext },
        KeyBinding { evdev: KEY_B, needs_logo: true, action: Action::Noop },
        KeyBinding { evdev: KEY_ESC, needs_logo: false, action: Action::Cancel },
        // a binding on a MODIFIER key itself (K3 subject)
        KeyBinding { evdev: KEY_LEFTSHIFT, needs_logo: false, action: Action::Noop },
    ]
}

fn setup(k3: bool) -> S {
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 60_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    let qh = ls.qh.clone();
    ls.probe.bind_input(&ls.globals, &qh);
    ls.server.pending_anchors.push_back((100, 100));
    ls.server.pending_anchors.push_back((1000, 100));
    let w1 = ls.probe.create_toplevel(&qh, "W1", (400, 300), 0, false);
    let w2 = ls.probe.create_toplevel(&qh, "W2", (400, 300), 0, false);
    ls.pump();
    ls.run_until(10_000);
    let mut s = S { ls, ix: KeyIntake::new(bindings(), k3), surfaces: vec![w1, w2], focus: None, ok: true, t_step: 0 };
    s.set_focus(Some(0), "initial focus W1");
    s
}

impl S {
    fn advance(&mut self) {
        self.ls.pump();
        let t = self.ls.server.now_us + 1000;
        self.ls.run_until(t);
    }
    fn set_focus(&mut self, el: Option<usize>, why: &str) {
        let surf = el.and_then(|e| self.ls.server.elements.get(e)).and_then(|e| e.window.toplevel().map(|t| t.wl_surface().clone()));
        let kb = self.ls.server.seat.get_keyboard().unwrap();
        kb.set_focus(&mut self.ls.server, surf, SERIAL_COUNTER.next_serial());
        self.focus = el;
        let n = self.ls.server.now_us;
        self.ix.log.push((n, format!("focus model: keyboard focus -> {:?} ({why})", el.map(|e| format!("W{}", e + 1)))));
        self.advance();
        self.check(&format!("focus -> {:?}", el.map(|e| format!("W{}", e + 1))));
    }
    fn key(&mut self, k: u32, pressed: bool) {
        let a = self.ix.key(&mut self.ls.server, k, pressed);
        self.advance();
        self.check(&format!("{} {}", name(k), if pressed { "down" } else { "up" }));
        match a {
            Some(Action::FocusNext) => {
                let n = self.ls.server.elements.len();
                let next = self.focus.map(|f| (f + 1) % n).unwrap_or(0);
                self.set_focus(Some(next), "binding FocusNext");
            }
            Some(Action::Cancel) => {
                let n = self.ls.server.now_us;
                self.ix.log.push((n, "interaction: CANCELLED (compositor-owned keys stay owned; no key events synthesized)".into()));
            }
            _ => {}
        }
    }
    fn check(&mut self, step: &str) {
        let kb = self.ls.server.seat.get_keyboard().unwrap();
        let seat_mods = kb.modifier_state().serialized;
        let seat_pressed: std::collections::BTreeSet<u32> = kb.pressed_keys().iter().map(|k| k.raw() - 8).collect();
        let client_owned_held: std::collections::BTreeSet<u32> = self.ix.held.difference(&self.ix.owned).copied().collect();
        let c = &self.ls.probe.kb;
        let mut fails: Vec<String> = Vec::new();
        if !c.violations.is_empty() {
            fails.push(format!("KV-1 {:?}", c.violations));
        }
        if c.active.is_some() {
            if c.keys != client_owned_held {
                fails.push(format!("KV-2 client keys {:?} != client-owned held {:?}", c.keys, client_owned_held));
            }
            if (c.depressed, c.latched, c.locked) != (seat_mods.depressed, seat_mods.latched, seat_mods.locked) {
                fails.push(format!(
                    "KV-3 client mods dep={:#x} lat={:#x} lock={:#x} != seat dep={:#x} lat={:#x} lock={:#x}",
                    c.depressed, c.latched, c.locked, seat_mods.depressed, seat_mods.latched, seat_mods.locked
                ));
            }
        }
        println!(
            "  {:<34} | physical {:<22} owned {:<10} | seat XKB pressed {:<22} mods dep={:#04x} | client focus {:<5} keys {:<16} dep={:#04x} | {}",
            step,
            format!("{:?}", self.ix.held.iter().map(|k| name(*k)).collect::<Vec<_>>()),
            format!("{:?}", self.ix.owned.iter().map(|k| name(*k)).collect::<Vec<_>>()),
            format!("{:?}", seat_pressed.iter().map(|k| name(*k)).collect::<Vec<_>>()),
            seat_mods.depressed,
            c.active.clone().unwrap_or("-".into()),
            format!("{:?}", c.keys.iter().map(|k| name(*k)).collect::<Vec<_>>()),
            c.depressed,
            if fails.is_empty() { "ok".to_string() } else { format!("FAIL {}", fails.join("; ")) }
        );
        if !fails.is_empty() {
            self.ok = false;
        }
    }
    fn end(&mut self, title: &str) -> bool {
        let c = &self.ls.probe.kb;
        let stuck = !self.ix.held.is_empty() || !c.keys.is_empty() || c.depressed != 0 || c.latched != 0;
        let kv4 = !stuck || !self.ix.held.is_empty();
        let pass = self.ok && kv4 && self.ix.held.is_empty();
        println!("  -- protocol events seen by the client:");
        let t0 = self.t_step;
        for (t, s) in self.ls.probe.log.iter().filter(|(t, s)| *t >= t0 && (s.contains("keyboard") || s.contains("VIOLATION"))) {
            println!("       t={:>6.1}ms {s}", *t as f64 / 1e3);
        }
        println!("  -- intake/focus decisions:");
        for (t, s) in self.ix.log.iter().filter(|(t, _)| *t >= t0) {
            println!("       t={:>6.1}ms {s}", *t as f64 / 1e3);
        }
        println!(
            "  RESULT {title}: {}  (KV-4 end state: client keys {:?} dep={:#x} lat={:#x})\n",
            if pass { "PASS" } else { "FAIL" },
            c.keys,
            c.depressed,
            c.latched
        );
        pass
    }
}

fn scenario(title: &str, k3: bool, f: impl FnOnce(&mut S)) -> bool {
    println!("=== {title}");
    let mut s = setup(k3);
    s.t_step = 0;
    f(&mut s);
    s.end(title)
}

fn main() {
    println!("X-10 keyboard/modifier ownership — smithay =0.7.0 seat keyboard (default XKB: us/pc105), lockstep");
    println!("bindings (configuration data): Super+Enter=FocusNext, Super+B=Noop, Esc=Cancel, Shift=Noop (binding on a modifier key)\n");
    let mut all = true;

    all &= scenario("K-1 ordinary client-owned press/release", true, |s| {
        s.key(KEY_A, true);
        s.key(KEY_A, false);
    });
    all &= scenario("K-2 modifier held while another client key is pressed", true, |s| {
        s.key(KEY_LEFTMETA, true);
        s.key(KEY_A, true);
        s.key(KEY_A, false);
        s.key(KEY_LEFTMETA, false);
    });
    all &= scenario("K-3 compositor binding (Super+B) while modifier held", true, |s| {
        s.key(KEY_LEFTMETA, true);
        s.key(KEY_B, true);
        s.key(KEY_B, false);
        s.key(KEY_LEFTMETA, false);
    });
    all &= scenario("K-4 focus transition (binding Super+Enter) while a modifier is held", true, |s| {
        s.key(KEY_LEFTMETA, true);
        s.key(KEY_ENTER, true); // FocusNext: W1 -> W2 while Super and Enter are held
        s.key(KEY_ENTER, false);
        s.key(KEY_LEFTMETA, false);
    });
    all &= scenario("K-5 focus transition while a non-modifier client key is held", true, |s| {
        s.key(KEY_A, true);
        s.set_focus(Some(1), "shell focus command"); // no key involved
        s.key(KEY_A, false); // release goes to the NEW focus (A is logically down there via enter)
    });
    all &= scenario("K-6 focused client destroyed while Shift+A are held", true, |s| {
        s.set_focus(Some(1), "focus W2");
        s.key(KEY_LEFTSHIFT, true);
        s.key(KEY_A, true);
        // W2's client role object is destroyed (client destroys its toplevel)
        let w2 = s.surfaces[1];
        s.ls.probe.destroy_toplevel(w2);
        s.advance();
        s.check("W2 destroyed");
        s.set_focus(Some(0), "successor after target loss");
        s.key(KEY_A, false);
        s.key(KEY_LEFTSHIFT, false);
    });
    all &= scenario("K-7 interaction cancelled while compositor-owned keys are held; focus changes meanwhile", true, |s| {
        s.key(KEY_LEFTMETA, true);
        s.key(KEY_B, true); // compositor-owned
        s.key(KEY_ESC, true); // Cancel (compositor-owned)
        s.key(KEY_ENTER, true); // FocusNext while B and Esc are compositor-owned and held
        s.key(KEY_ENTER, false);
        s.key(KEY_B, false);
        s.key(KEY_ESC, false);
        s.key(KEY_LEFTMETA, false);
    });
    all &= scenario("K-8 binding on a MODIFIER key (Shift) with K3: observe mode; focus change while held", true, |s| {
        s.key(KEY_LEFTSHIFT, true);
        s.set_focus(Some(1), "shell focus command");
        s.key(KEY_LEFTSHIFT, false);
    });
    all &= scenario("K-9 keyboard focus lost (no target) while keys are held, then regained", true, |s| {
        s.key(KEY_LEFTSHIFT, true);
        s.key(KEY_A, true);
        s.set_focus(None, "focus loss (e.g. focused element unmapped, no successor)");
        s.key(KEY_A, false);
        s.set_focus(Some(1), "focus regained");
        s.key(KEY_LEFTSHIFT, false);
    });

    println!("--- NEGATIVE CONTROL: the same as K-8 with K3 DISABLED (modifier binding intercepted). Expected to FAIL: this is the hazard K3 exists to prevent.");
    let neg = scenario("NEG K-8 without K3", false, |s| {
        s.key(KEY_LEFTSHIFT, true);
        s.set_focus(Some(1), "shell focus command");
        s.key(KEY_LEFTSHIFT, false);
    });
    println!("NEGATIVE CONTROL outcome: {} (expected FAIL)\n", if neg { "PASS (unexpected)" } else { "FAIL (as expected)" });

    println!("NOT REPRESENTED: output removal (K8: keyboard focus is output-independent; nothing to observe in this harness).");
    println!("ALL K3-ENFORCED SCENARIOS: {}", if all && !neg { "PASS" } else { "SOME FAILED" });
}
