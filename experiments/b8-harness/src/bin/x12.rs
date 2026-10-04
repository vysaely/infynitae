//! X-12: reproducer for W-SM-4b (Smithay 0.7.0 `wayland/seat/pointer.rs:246-256`): the seat's pointer
//! `leave()` deactivates constraints unconditionally, so a ONESHOT lock that was never activated
//! receives `unlocked` and is destroyed when the pointer merely leaves its surface.
//! Spec (zwp_locked_pointer_v1.unlocked): "Notification that the pointer lock of the seat's pointer is
//! no longer active. If this is a oneshot pointer lock ... this object is now defunct".
//! Output: docs/foundation/evidence/phase-b/x12-oneshot-lock-defect-output.txt
use b8_harness::{
    input::{Intake, PanBinding, PanFocusMode},
    lockstep::Lockstep,
    server::*,
};

fn run(oneshot: bool) -> (bool, bool, bool) {
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    let qh = ls.qh.clone();
    ls.probe.bind_input(&ls.globals, &qh);
    ls.server.pending_anchors.push_back((100, 100));
    let w = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, false);
    ls.pump();
    ls.run_until(20_000);
    let mut ix = Intake::new(PanBinding { button: 0x112, needs_modifier: true }, PanFocusMode::KeepFocus, 0, (299.0, 300.0));
    let step = |ls: &mut Lockstep| {
        ls.pump();
        let t = ls.server.now_us + 500;
        ls.run_until(t);
    };
    ix.motion(&mut ls.server, (1.0, 0.0), (1.0, 0.0)); // pointer over W at (300,300): W has pointer focus
    step(&mut ls);
    // policy declines activation (models: pointer outside the lock region / compositor policy)
    ls.server.activation_allowed = false;
    ls.probe.lock_pointer(&qh, w, oneshot);
    step(&mut ls);
    let t_leave = ls.server.now_us;
    ix.motion(&mut ls.server, (1200.0, 500.0), (1200.0, 500.0)); // pointer leaves W
    step(&mut ls);
    // policy now allows activation; pointer returns over W
    ls.server.activation_allowed = true;
    let t_back = ls.server.now_us;
    ix.motion(&mut ls.server, (-1200.0, -500.0), (-1200.0, -500.0));
    step(&mut ls);
    let has = |t0: u64, t1: u64, n: &str| ls.probe.log.iter().any(|(t, s)| *t >= t0 && *t < t1 && s.contains(n));
    let locked_before_leave = has(0, t_leave, "locked_pointer.locked");
    let unlocked_on_leave = has(t_leave, t_back, "locked_pointer.unlocked");
    let locked_after_return = has(t_back, u64::MAX, "locked_pointer.locked");
    println!("-- lifetime={} trace:", if oneshot { "oneshot" } else { "persistent" });
    for (t, s) in &ls.probe.log {
        if s.contains("pointer") {
            println!("    t={:>7.3}ms client {s}", *t as f64 / 1e3);
        }
    }
    (locked_before_leave, unlocked_on_leave, locked_after_return)
}

fn main() {
    println!("X-12 W-SM-4b reproducer — smithay =0.7.0, lockstep");
    let (l0, u, l1) = run(true);
    println!("oneshot:    locked before leave={l0}  unlocked on leave={u}  locked after return={l1}");
    println!("  => DEFECT REPRODUCED: {}", !l0 && u && !l1);
    println!("     (never-active oneshot lock got `unlocked` = defunct, and cannot activate when activation becomes allowed)");
    let (l0, u, l1) = run(false);
    println!("persistent: locked before leave={l0}  unlocked on leave={u}  locked after return={l1}");
    println!("  => spurious `unlocked` for a never-active persistent lock: {} (lock survives and activates later: {l1})", !l0 && u);
}
