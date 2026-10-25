//! X-8: camera-pan input ownership, observed at the client.
//! Output: docs/archive/evidence/phase-b/x8-pan-input-output.txt
//!
//! Each case checks invariants against the probe client's own event log:
//!  INV-1  while CameraPan owns the pointer: 0 wl_pointer.motion, 0 relative_motion, 0 wl_pointer.button
//!  INV-2  every button press the client saw has a matching release (no stuck buttons)
//!  INV-3  Stage anchor unchanged by the pan
//!  INV-4  first genuine motion after termination produces relative_motion equal to that delta only
//!  INV-5  no relative_motion is synthesized at termination (no replay, no compensation)

use b8_harness::{
    input::{Intake, PanBinding, PanFocusMode},
    lockstep::Lockstep,
    server::*,
};

const MS: u64 = 1000;
const BTN_LEFT: u32 = 0x110;
const BTN_MIDDLE: u32 = 0x112;

struct Case {
    ls: Lockstep,
    ix: Intake,
    w: usize,
}

fn setup(mode: PanFocusMode) -> Case {
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::MaxGesture, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    let qh = ls.qh.clone();
    ls.probe.bind_input(&ls.globals, &qh);
    ls.server.pending_anchors.push_back((100, 100)); // geometry 100..900 x 100..700; root surface origin (80,80)
    let w = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, false);
    ls.pump();
    ls.run_until(20 * MS);
    // The pan binding is configuration, not architecture: here middle button + a modifier.
    let ix = Intake::new(PanBinding { button: BTN_MIDDLE, needs_modifier: true }, mode, 0, (299.0, 300.0));
    let mut c = Case { ls, ix, w };
    c.motion((1.0, 0.0)); // establish pointer focus at (300,300)
    c
}

impl Case {
    fn tick(&mut self) {
        self.ls.pump();
        let t = self.ls.server.now_us + 500;
        self.ls.run_until(t);
    }
    fn motion(&mut self, d: (f64, f64)) {
        self.ix.motion(&mut self.ls.server, d, (d.0 * 0.9, d.1 * 0.9));
        self.tick();
    }
    fn button(&mut self, code: u32, pressed: bool) {
        self.ix.button(&mut self.ls.server, code, pressed);
        self.tick();
    }
    fn now(&self) -> u64 {
        self.ls.server.now_us
    }
    fn count(&self, t0: u64, t1: u64, needle: &str) -> usize {
        self.ls.probe.log.iter().filter(|(t, s)| *t >= t0 && *t <= t1 && s.contains(needle)).count()
    }
    fn client_lines(&self, t0: u64) -> Vec<String> {
        self.ls.probe.log.iter().filter(|(t, _)| *t >= t0).map(|(t, s)| format!("    t={:>7.3}ms client {s}", *t as f64 / 1e3)).collect()
    }
    fn intake_lines(&self, t0: u64) -> Vec<String> {
        self.ix.log.iter().filter(|(t, _)| *t >= t0).map(|(t, s)| format!("    t={:>7.3}ms {s}", *t as f64 / 1e3)).collect()
    }
    fn merged(&self, t0: u64) -> Vec<String> {
        let mut v: Vec<(u64, u8, String)> = Vec::new();
        for (t, s) in &self.ix.log {
            if *t >= t0 {
                v.push((*t, 0, format!("    t={:>7.3}ms        {s}", *t as f64 / 1e3)));
            }
        }
        for (t, s) in &self.ls.server.log {
            if *t >= t0 && (s.contains("constraint") || s.contains("visible")) {
                v.push((*t, 1, format!("    t={:>7.3}ms server {s}", *t as f64 / 1e3)));
            }
        }
        for (t, s) in &self.ls.probe.log {
            if *t >= t0 {
                v.push((*t, 2, format!("    t={:>7.3}ms client {s}", *t as f64 / 1e3)));
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        v.into_iter().map(|x| x.2).collect()
    }
    fn button_pairs_ok(&self) -> bool {
        self.ls.probe.buttons_down.is_empty()
    }
    fn anchor(&self) -> Option<(i32, i32)> {
        self.ls.server.elements.first().and_then(|e| e.anchor)
    }
}

/// relative_motion carries wl_fixed (24.8): the expected client-visible value of a delta.
fn fixed(v: f64) -> f64 {
    (v * 256.0).round() / 256.0
}
fn expected_rel(d: (f64, f64)) -> String {
    format!("relative_motion ({:.3},{:.3}) unaccel ({:.3},{:.3})", fixed(d.0), fixed(d.1), fixed(d.0 * 0.9), fixed(d.1 * 0.9))
}

fn check(name: &str, ok: bool, detail: String) -> bool {
    println!("  [{}] {name}: {detail}", if ok { "PASS" } else { "FAIL" });
    ok
}

/// Standard pan: modifier, press, N deltas, release, modifier up. Returns (t_acquire, t_release_done).
fn pan(c: &mut Case, deltas: &[(f64, f64)], release_modifier_first: bool) -> (u64, u64) {
    c.ix.modifier = true;
    let t_acq = c.now();
    c.button(BTN_MIDDLE, true);
    for d in deltas {
        c.motion(*d);
    }
    if release_modifier_first {
        c.ix.modifier = false;
    }
    let t_rel = c.now();
    c.button(BTN_MIDDLE, false);
    c.ix.modifier = false;
    let _ = t_rel;
    (t_acq, c.now())
}

fn standard_pan_checks(c: &Case, t_acq: u64, t_end_minus: u64, anchor_before: Option<(i32, i32)>) -> bool {
    let m = c.count(t_acq, t_end_minus, "wl_pointer.motion");
    let r = c.count(t_acq, t_end_minus, "relative_motion");
    let b = c.count(t_acq, t_end_minus, "wl_pointer.button");
    let mut ok = check("INV-1 no client motion/relative/button while pan owns the pointer", m + r + b == 0, format!("motion={m} relative={r} button={b}"));
    ok &= check("INV-2 no stuck buttons", c.button_pairs_ok(), format!("client believes pressed: {:?}", c.ls.probe.buttons_down));
    ok &= check("INV-3 Stage anchor unchanged", c.anchor() == anchor_before, format!("{:?} -> {:?}", anchor_before, c.anchor()));
    ok
}

fn main() {
    println!("X-8 camera-pan input ownership — b8-harness, smithay 0.7.0, lockstep");
    println!("W: content 800x600 + 20px CSD shadow, anchor (100,100), root surface origin (80,80); output A 1920x1080@1.0; pointer at (300,300).");
    println!("Pan deltas: 30 x (7.3,-2.1) (accel), unaccel = 0.9 x accel. Expected camera after pan: start - round(sum) = (0,0) - (219,-63) = (-219,63).");
    let deltas: Vec<(f64, f64)> = vec![(7.3, -2.1); 30];
    let mut all_ok = true;

    for mode in [PanFocusMode::KeepFocus, PanFocusMode::LeaveFocus] {
        println!("\n=== C1/C2 free pointer, mode {mode:?}: motion before acquisition, pan, termination, first genuine motion");
        let mut c = setup(mode);
        let t0 = c.now();
        c.motion((10.0, 0.0)); // immediately before acquisition
        let a0 = c.anchor();
        let (t_acq, t_end) = pan(&mut c, &deltas, false);
        let t_after = c.now();
        c.motion((1.0, 0.0)); // first genuine motion after termination
        for l in c.merged(t0) {
            println!("{l}");
        }
        // during-pan window ends just before the release's reconciliation
        let last_consumed = c.ix.log.iter().filter(|(_, s)| s.contains("release 274 consumed")).map(|(t, _)| *t).last().unwrap_or(t_end);
        all_ok &= standard_pan_checks(&c, t_acq, last_consumed.saturating_sub(1), a0);
        all_ok &= check("camera = start - round(sum)", c.ls.server.outputs[0].camera == (-219, 63), format!("{:?}", c.ls.server.outputs[0].camera));
        let rel_after: Vec<_> = c.ls.probe.log.iter().filter(|(t, s)| *t >= t_after && s.contains("relative_motion")).map(|(_, s)| s.clone()).collect();
        all_ok &= check("INV-4/5 only the genuine delta is reported relatively after termination", rel_after == vec![expected_rel((1.0, 0.0))], format!("{rel_after:?}"));
        // expected absolute position after reconciliation: canvas = camera + p = (-219+311, 63+300) = (92,363) -> surface-local (12, 283)
        let mot: Vec<_> = c.ls.probe.log.iter().filter(|(t, s)| *t >= last_consumed && (s.contains("wl_pointer.motion") || s.contains("wl_pointer.enter"))).map(|(_, s)| s.clone()).collect();
        // pointer output-local x: 299 +1 +10 = 310; Canvas after pan = camera (-219,63) + (310,300) = (91,363);
        // surface-local w.r.t. root origin (80,80) = (11,283); after the +1 genuine motion = (12,283)
        let want_first = if mode == PanFocusMode::KeepFocus { "wl_pointer.motion (11.000,283.000)" } else { "wl_pointer.enter W (11.000,283.000)" };
        all_ok &= check("reconciliation delivers the TRUE position once, then genuine motion continues from it", mot == vec![want_first.to_string(), "wl_pointer.motion (12.000,283.000)".to_string()], format!("{mot:?}"));
    }

    for (mode, oneshot) in [(PanFocusMode::KeepFocus, false), (PanFocusMode::LeaveFocus, false), (PanFocusMode::KeepFocus, true), (PanFocusMode::LeaveFocus, true)] {
        println!("\n=== C3 locked pointer ({}), mode {mode:?}", if oneshot { "oneshot" } else { "persistent" });
        let mut c = setup(mode);
        let qh = c.ls.qh.clone();
        let t0 = c.now();
        let w = c.w;
        c.ls.probe.lock_pointer(&qh, w, oneshot);
        c.tick();
        c.motion((5.0, 0.0)); // locked: relative only
        let a0 = c.anchor();
        let (t_acq, _) = pan(&mut c, &deltas, false);
        let t_after = c.now();
        c.motion((1.0, 0.0));
        for l in c.merged(t0) {
            println!("{l}");
        }
        let last_consumed = c.ix.log.iter().filter(|(_, s)| s.contains("release 274 consumed")).map(|(t, _)| *t).last().unwrap();
        all_ok &= standard_pan_checks(&c, t_acq, last_consumed.saturating_sub(1), a0);
        let unlocked = c.count(t_acq, u64::MAX, "unlocked");
        println!("  [INFO] locked_pointer.unlocked events caused by the pan interaction: {unlocked}");
        let after: Vec<_> = c.ls.probe.log.iter().filter(|(t, s)| *t >= t_after && (s.contains("relative_motion") || s.contains("wl_pointer.motion"))).map(|(_, s)| s.clone()).collect();
        println!("  [INFO] client pointer events for the first genuine motion (+1,0) after termination: {after:?}");
        let lock_kept = unlocked == 0;
        if mode == PanFocusMode::KeepFocus {
            all_ok &= check("KeepFocus: lock survives the pan (no unlocked)", lock_kept, format!("unlocked={unlocked}"));
            all_ok &= check("INV-4 locked: genuine motion -> relative only, exactly the delta", after == vec![expected_rel((1.0, 0.0))], format!("{after:?}"));
        }
    }

    println!("\n=== C4 locked pointer, pan moves the lock point off the output (KeepFocus)");
    {
        let mut c = setup(PanFocusMode::KeepFocus);
        let qh = c.ls.qh.clone();
        let t0 = c.now();
        let w = c.w;
        c.ls.probe.lock_pointer(&qh, w, false);
        c.tick();
        let far: Vec<(f64, f64)> = vec![(-25.0, 0.0); 100]; // camera +2500 in x
        let a0 = c.anchor();
        let (t_acq, _) = pan(&mut c, &far, false);
        for l in c.merged(t0) {
            if !l.contains("visible") {
                println!("{l}");
            }
        }
        let last_consumed = c.ix.log.iter().filter(|(_, s)| s.contains("release 274 consumed")).map(|(t, _)| *t).last().unwrap();
        all_ok &= standard_pan_checks(&c, t_acq, last_consumed.saturating_sub(1), a0);
        let unl = c.count(last_consumed, u64::MAX, "locked_pointer.unlocked");
        all_ok &= check("W-SM-4 ordering rule: exactly one unlocked when the lock ends at reconciliation", unl == 1, format!("unlocked events={unl}"));
        let rel_end = c.count(last_consumed, u64::MAX, "relative_motion");
        all_ok &= check("INV-5 no relative motion synthesized at termination", rel_end == 0, format!("relative after release={rel_end}"));
    }

    println!("\n=== C5 confined pointer (KeepFocus): small pan (pointer stays over W) and large pan (W leaves the pointer)");
    for (name, ds) in [("small", vec![(2.0, 0.0); 10]), ("large", vec![(-30.0, 0.0); 40])] {
        let mut c = setup(PanFocusMode::KeepFocus);
        let qh = c.ls.qh.clone();
        let t0 = c.now();
        let w = c.w;
        c.ls.probe.confine_pointer(&qh, w);
        c.tick();
        let a0 = c.anchor();
        let (t_acq, _) = pan(&mut c, &ds, false);
        println!("  -- {name} pan");
        for l in c.merged(t0) {
            if !l.contains("visible") {
                println!("{l}");
            }
        }
        let last_consumed = c.ix.log.iter().filter(|(_, s)| s.contains("release 274 consumed")).map(|(t, _)| *t).last().unwrap();
        all_ok &= standard_pan_checks(&c, t_acq, last_consumed.saturating_sub(1), a0);
    }

    println!("\n=== C6 stuck-button prevention: client holds BTN_LEFT, then the pan binding is pressed");
    {
        let mut c = setup(PanFocusMode::KeepFocus);
        let t0 = c.now();
        c.button(BTN_LEFT, true);
        c.ix.modifier = true;
        c.button(BTN_MIDDLE, true); // binding matches, but the client owns a pressed button -> delivered, no acquisition
        c.motion((4.0, 0.0));
        c.button(BTN_MIDDLE, false);
        c.ix.modifier = false;
        c.button(BTN_LEFT, false);
        for l in c.merged(t0) {
            println!("{l}");
        }
        all_ok &= check("INV-2 no stuck buttons", c.button_pairs_ok(), format!("{:?}", c.ls.probe.buttons_down));
        all_ok &= check("no acquisition while a client owns a button", !c.ix.log.iter().any(|(_, s)| s.contains("ACQUIRES")), String::new());
        let _ = c.client_lines(0);
        let _ = c.intake_lines(0);
    }

    println!("\n=== C7 buttons pressed DURING the pan are compositor-owned through their release, even after the pan ends");
    {
        let mut c = setup(PanFocusMode::KeepFocus);
        let t0 = c.now();
        c.ix.modifier = true;
        c.button(BTN_MIDDLE, true);
        c.motion((3.0, 0.0));
        c.button(BTN_LEFT, true); // during pan
        c.button(BTN_MIDDLE, false); // pan ends while BTN_LEFT is still held
        c.ix.modifier = false;
        c.motion((1.0, 0.0));
        c.button(BTN_LEFT, false); // release of a compositor-owned press
        for l in c.merged(t0) {
            println!("{l}");
        }
        let b = c.count(t0, u64::MAX, "wl_pointer.button");
        all_ok &= check("client saw neither press nor release of a compositor-owned button", b == 0, format!("button events={b}"));
        all_ok &= check("INV-2 no stuck buttons", c.button_pairs_ok(), format!("{:?}", c.ls.probe.buttons_down));
    }

    println!("\n=== C8 modifier released before the initiating button: ownership follows the button, not the modifier");
    {
        let mut c = setup(PanFocusMode::KeepFocus);
        let a0 = c.anchor();
        let (t_acq, _) = pan(&mut c, &deltas[..5], true);
        let last_consumed = c.ix.log.iter().filter(|(_, s)| s.contains("release 274 consumed")).map(|(t, _)| *t).last().unwrap();
        all_ok &= standard_pan_checks(&c, t_acq, last_consumed.saturating_sub(1), a0);
    }

    println!("\n=== C9 focused (locked) surface destroyed during the pan");
    {
        let mut c = setup(PanFocusMode::KeepFocus);
        let qh = c.ls.qh.clone();
        let t0 = c.now();
        let w = c.w;
        c.ls.probe.lock_pointer(&qh, w, false);
        c.tick();
        c.ix.modifier = true;
        c.button(BTN_MIDDLE, true);
        c.motion((5.0, 0.0));
        // client destroys its toplevel mid-pan
        if let Some(l) = c.ls.probe.locked.take() {
            l.destroy();
        }
        let s = &c.ls.probe.surfaces[w];
        if let Some(t) = &s.toplevel {
            t.destroy();
        }
        s.xdg.destroy();
        s.wl.destroy();
        c.tick();
        c.motion((5.0, 0.0));
        c.button(BTN_MIDDLE, false);
        c.ix.modifier = false;
        c.motion((1.0, 0.0));
        for l in c.merged(t0) {
            println!("{l}");
        }
        all_ok &= check("pan survives target destruction and terminates cleanly", matches!(c.ix.interaction, b8_harness::input::Interaction::Normal), String::new());
        all_ok &= check("Stage has no element left for the destroyed toplevel", c.ls.server.elements.is_empty(), format!("{} elements", c.ls.server.elements.len()));
    }

    println!("\nNOT REPRESENTED by this harness (no claim made): output removal during a pan; real libinput device semantics (acceleration, pointer clipping at device level); a real game's response to lock/unlock.");
    println!("\nALL CHECKS: {}", if all_ok { "PASS" } else { "SOME FAILED" });
}
