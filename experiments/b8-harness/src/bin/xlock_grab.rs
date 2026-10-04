//! Phase D 1R (PD-F2-02 pointer part / PD-F2-03): D5 epoch 4 reproduced at protocol level.
//! A client holds an ACTIVE pointer lock on W and a client-owned button (Smithay implicit click grab);
//! the interaction output disappears and the pointer is relocated where core's hit-test finds nothing.
//! (The harness cannot destroy an output global; the relocation step — the only part that touches the
//! seat — is executed exactly as core would after removal: one pointer update in one batch.)
//! Strategies for the constraint at relocation:
//!   SEALED   W-SM-4a as sealed: core predicts a focus change from its own hit-test -> no explicit
//!            deactivate; motion(focus=None)
//!   EXPLICIT deactivate explicitly (focus "will not change" because of the grab), then motion(None)
//!   DEFER    1R rule: while a client-owned grab sequence is active the constraint is NOT reconciled and
//!            no absolute motion is delivered (pointer is locked); the physical pointer is relocated in
//!            core only; at the end of the client sequence the reconciliation runs once
//! Checked at every step: core constraint state == Smithay `is_active()`; `unlocked` count over the run
//! is exactly 1; no wl_pointer.motion while locked; no relative motion (no physical delta injected).
//! Output: docs/foundation/evidence/phase-d/xlock-grab-output.txt
use b8_harness::{
    input::{Intake, PanBinding, PanFocusMode},
    lockstep::Lockstep,
    server::*,
};
use smithay::{
    backend::input::ButtonState,
    input::pointer::{ButtonEvent, MotionEvent},
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::pointer_constraints::with_pointer_constraint,
};

const BTN_LEFT: u32 = 0x110;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Strat {
    Sealed,
    Explicit,
    Defer,
}

fn run(strat: Strat, oneshot: bool) -> bool {
    println!("=== strategy {strat:?}, lifetime {}", if oneshot { "oneshot" } else { "persistent" });
    let outs = [OutCfg { name: "B", mode: (1920, 1080), scale: 1.0, refresh_mhz: 144_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    let qh = ls.qh.clone();
    ls.probe.bind_input(&ls.globals, &qh);
    ls.server.pending_anchors.push_back((100, 100));
    let w = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, false);
    ls.pump();
    ls.run_until(20_000);
    let step = |ls: &mut Lockstep| {
        ls.pump();
        let t = ls.server.now_us + 500;
        ls.run_until(t);
    };
    let mut ix = Intake::new(PanBinding { button: 0x112, needs_modifier: true }, PanFocusMode::KeepFocus, 0, (299.0, 300.0));
    ix.motion(&mut ls.server, (1.0, 0.0), (1.0, 0.0)); // pointer over W: pointer focus W
    step(&mut ls);
    ls.probe.lock_pointer(&qh, w, oneshot); // activated on creation (focused)
    step(&mut ls);
    let root = ls.server.elements[0].window.toplevel().unwrap().wl_surface().clone();
    let ptr = ls.server.seat.get_pointer().unwrap();
    let smithay_active = |srv: &Server| with_pointer_constraint(&root, &srv.seat.get_pointer().unwrap(), |c| c.map(|c| c.is_active()).unwrap_or(false));
    let mut core_active = smithay_active(&ls.server);
    let mut ok = true;
    let check = |ls: &Lockstep, core_active: bool, what: &str| {
        let s = smithay_active(&ls.server);
        let agree = s == core_active;
        println!("  {what:<70} core lock active={core_active:<5} Smithay active={s:<5} {}", if agree { "agree" } else { "DIVERGED" });
        agree
    };
    ok &= check(&ls, core_active, "lock established");
    ix.button(&mut ls.server, BTN_LEFT, true); // client-owned press: Smithay implicit click grab on W
    step(&mut ls);
    ok &= check(&ls, core_active, "client-owned BTN_LEFT held (implicit grab)");
    let t0 = ls.server.now_us;

    // ---- interaction output removed; core relocates the pointer to a point where nothing is hit
    let far: Point<f64, Logical> = (1900.0, 1000.0).into();
    let serial = SERIAL_COUNTER.next_serial();
    match strat {
        Strat::Sealed => {
            // core: hit-test at the relocated point finds nothing -> "focus will change" -> leave() deactivates
            core_active = false;
            ptr.motion(&mut ls.server, None, &MotionEvent { location: far, serial, time: 10 });
            ptr.frame(&mut ls.server);
        }
        Strat::Explicit => {
            with_pointer_constraint(&root, &ptr, |c| {
                if let Some(c) = c {
                    c.deactivate();
                }
            });
            core_active = false;
            ptr.motion(&mut ls.server, None, &MotionEvent { location: far, serial, time: 10 });
            ptr.frame(&mut ls.server);
        }
        Strat::Defer => {
            // client-owned sequence active: reconciliation deferred; locked pointer -> no absolute motion;
            // core records the relocated physical position only
        }
    }
    step(&mut ls);
    ok &= check(&ls, core_active, "after relocation (output removed)");

    // ---- the client releases its button: end of the client-owned sequence
    let serial = SERIAL_COUNTER.next_serial();
    ptr.button(&mut ls.server, &ButtonEvent { serial, time: 20, button: BTN_LEFT, state: ButtonState::Released });
    ptr.frame(&mut ls.server);
    step(&mut ls);
    ok &= check(&ls, core_active, "client releases BTN_LEFT (implicit grab ends)");

    // ---- reconciliation at batch end (PD-F2-04 trigger: pointer's output set changed / sequence ended)
    let lock_point_observable = false; // the lock point is not visible through the surviving output
    if strat == Strat::Defer && core_active && !lock_point_observable {
        // the delivered focus WILL change now (no grab): leave() performs the single deactivation
        core_active = false;
    }
    let serial = SERIAL_COUNTER.next_serial();
    ptr.motion(&mut ls.server, None, &MotionEvent { location: far, serial, time: 30 });
    ptr.frame(&mut ls.server);
    step(&mut ls);
    ok &= check(&ls, core_active, "reconciliation (true position, nothing under it)");

    let log: Vec<&String> = ls.probe.log.iter().filter(|(t, s)| *t >= t0 && (s.contains("pointer") || s.contains("relative"))).map(|(_, s)| s).collect();
    for l in &log {
        println!("     client: {l}");
    }
    let unlocked = log.iter().filter(|s| s.contains("unlocked")).count();
    let rel = log.iter().filter(|s| s.contains("relative_motion")).count();
    let motion_while_locked = {
        // any wl_pointer.motion before the first `unlocked`
        let first_unlock = log.iter().position(|s| s.contains("unlocked")).unwrap_or(log.len());
        log[..first_unlock].iter().filter(|s| s.contains("wl_pointer.motion")).count()
    };
    let pass = ok && unlocked == 1 && rel == 0 && motion_while_locked == 0 && ls.probe.ptr_violations.is_empty();
    println!(
        "  unlocked events={unlocked} (want 1) | relative events={rel} (want 0) | motion while locked={motion_while_locked} (want 0) | pointer-order violations={} | core==Smithay at every step={ok}",
        ls.probe.ptr_violations.len()
    );
    println!("  RESULT {strat:?}/{}: {}\n", if oneshot { "oneshot" } else { "persistent" }, if pass { "PASS" } else { "FAIL" });
    pass
}

fn main() {
    println!("xlock_grab — lock + client implicit grab + interaction-output loss; smithay =0.7.0; lockstep\n");
    let mut all = true;
    for (s, one, exp) in [
        (Strat::Sealed, false, false),
        (Strat::Sealed, true, false),
        (Strat::Explicit, false, false),
        (Strat::Explicit, true, true),
        (Strat::Defer, false, true),
        (Strat::Defer, true, true),
    ] {
        let p = run(s, one);
        let as_exp = p == exp;
        all &= as_exp;
        println!("  >> {s:?}/{}: {} (expected {}){}\n", if one { "oneshot" } else { "persistent" }, if p { "PASS" } else { "FAIL" }, if exp { "PASS" } else { "FAIL" }, if as_exp { "" } else { "  <-- UNEXPECTED" });
    }
    println!("ALL AS EXPECTED: {all}");
}
