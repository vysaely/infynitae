//! E1R / E-F2-02: pointer-constraint lifecycle across batches, observed at the client.
//!
//! Falsifying sequence (Phase E): Batch A — the lock must end while the delivered pointer focus is
//! retained, so W-SM-4a performs an explicit `deactivate()` (`unlocked`); a PERSISTENT constraint stays
//! registered in Smithay. Batch B — later the delivered focus leaves the surface; Smithay 0.7.0
//! `PointerTarget<D> for WlSurface::leave` (wayland/seat/pointer.rs:251-255) calls `deactivate()`
//! unconditionally, sending `unlocked` again with no `locked` in between.
//!
//! Validity criterion (INV-INPUT-5 "Violation: lost/duplicate lock events"; M13 "exactly one
//! `unlocked`"): per constraint object, the client-visible events form the protocol state machine
//!   inactive --locked/confined--> active --unlocked/unconfined--> inactive (oneshot: defunct)
//! i.e. an activation only from inactive, a deactivation only from active, nothing after a oneshot
//! deactivation. Wire order (enter/leave/locked/unlocked) is printed for every case.
//!
//! The same source is built twice:
//!   - experiments/b8-harness            : smithay =0.7.0 as published (sealed dependency)
//!   - experiments/e1r-smithay-p1        : smithay =0.7.0 + one-line `[patch]` (leave() deactivates only
//!                                         an ACTIVE constraint), the OB-11 candidate
//! Output: docs/archive/evidence/E1R/xlock-batch-{stock,p1}-output.txt
use b8_harness::{
    input::{Intake, PanBinding, PanFocusMode},
    lockstep::Lockstep,
    server::*,
};
use smithay::{
    input::pointer::MotionEvent,
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::pointer_constraints::with_pointer_constraint,
};

const BTN_LEFT: u32 = 0x110;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Lock { oneshot: bool },
    Confine,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum BatchB {
    /// pointer moves onto another toplevel W2
    ExitToW2,
    /// pointer moves onto empty Canvas
    ExitToEmpty,
    /// the client destroys the constrained surface
    DestroySurface,
    /// interaction output removed: core relocates the pointer; one motion with focus None
    OutputRemoved,
    /// the lock point becomes observable again: reactivation, then exit to W2
    ReactivateThenExit,
    /// client-owned button held on W, pointer moves over W2 (implicit grab keeps focus), release
    HeldButtonThenExit,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum BatchA {
    /// W-SM-4a explicit deactivate while the delivered focus is retained (sealed rule)
    ExplicitDeactivate,
    /// constraint never activated (policy declined: e.g. pointer outside the region)
    NeverActivated,
    /// public-API alternative: end the lock only through a focus change (leave), then the pointer
    /// returns over W without reactivation (lock point still not observable)
    LeaveInsteadThenReturn,
}

struct Case {
    name: &'static str,
    kind: Kind,
    a: BatchA,
    b: BatchB,
}

fn validate(events: &[String], oneshot: bool) -> Vec<String> {
    let mut active = false;
    let mut defunct = false;
    let mut v = vec![];
    for e in events {
        let act = e.ends_with(".locked") || e.ends_with(".confined");
        let deact = e.ends_with(".unlocked") || e.ends_with(".unconfined");
        if defunct {
            v.push(format!("{e} after oneshot deactivation"));
            continue;
        }
        if act {
            if active {
                v.push(format!("{e} while already active"));
            }
            active = true;
        } else if deact {
            if !active {
                v.push(format!("{e} while not active (duplicate / unpaired)"));
            }
            active = false;
            if oneshot {
                defunct = true;
            }
        }
    }
    v
}

fn run(c: &Case) -> bool {
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    let qh = ls.qh.clone();
    ls.probe.bind_input(&ls.globals, &qh);
    ls.server.pending_anchors.push_back((100, 100)); // W: 100..900 x 100..700
    let w = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, false);
    ls.pump();
    ls.run_until(20_000);
    ls.server.pending_anchors.push_back((1000, 100)); // W2: 1000..1800 x 100..700
    let _w2 = ls.probe.create_toplevel(&qh, "W2", (800, 600), 20, false);
    ls.pump();
    ls.run_until(40_000);
    let step = |ls: &mut Lockstep| {
        ls.pump();
        let t = ls.server.now_us + 500;
        ls.run_until(t);
    };
    // Activation is decided by the experiment (core policy), never implicitly by the harness.
    ls.server.activation_allowed = false;
    let mut ix = Intake::new(PanBinding { button: 0x112, needs_modifier: true }, PanFocusMode::KeepFocus, 0, (299.0, 300.0));
    ix.motion(&mut ls.server, (1.0, 0.0), (1.0, 0.0)); // pointer over W: focus W
    step(&mut ls);
    let oneshot = matches!(c.kind, Kind::Lock { oneshot: true });
    match c.kind {
        Kind::Lock { oneshot } => ls.probe.lock_pointer(&qh, w, oneshot),
        Kind::Confine => ls.probe.confine_pointer(&qh, w),
    }
    step(&mut ls);
    let root = ls.server.elements[0].window.toplevel().unwrap().wl_surface().clone();
    let activate = |ls: &mut Lockstep| {
        let ptr = ls.server.seat.get_pointer().unwrap();
        with_pointer_constraint(&root, &ptr, |k| {
            if let Some(k) = k {
                if !k.is_active() {
                    k.activate();
                }
            }
        });
    };
    let t0 = ls.server.now_us;
    // ---- establish (core policy: all INV-INPUT-5 conditions hold)
    if c.a != BatchA::NeverActivated {
        activate(&mut ls);
        step(&mut ls);
    }
    // ---- Batch A
    match c.a {
        BatchA::ExplicitDeactivate => {
            // lock point panned off the interaction output; KeepFocus retains delivered focus (INV-INPUT-3);
            // batch-end reconciliation: focus does not change -> explicit deactivate (W-SM-4a)
            let ptr = ls.server.seat.get_pointer().unwrap();
            with_pointer_constraint(&root, &ptr, |k| {
                if let Some(k) = k {
                    k.deactivate();
                }
            });
            step(&mut ls);
        }
        BatchA::NeverActivated => {}
        BatchA::LeaveInsteadThenReturn => {
            let ptr = ls.server.seat.get_pointer().unwrap();
            let s = SERIAL_COUNTER.next_serial();
            ptr.motion(&mut ls.server, None, &MotionEvent { location: Point::<f64, Logical>::from((299.0, 300.0)), serial: s, time: 5 });
            ptr.frame(&mut ls.server);
            step(&mut ls);
            ix.motion(&mut ls.server, (1.0, 0.0), (1.0, 0.0)); // next motion: focus returns to W, no reactivation
            step(&mut ls);
        }
    }
    // ---- Batch B (a later batch)
    match c.b {
        BatchB::ExitToW2 => {
            ix.motion(&mut ls.server, (900.0, 0.0), (900.0, 0.0));
            step(&mut ls);
        }
        BatchB::ExitToEmpty => {
            ix.motion(&mut ls.server, (0.0, 600.0), (0.0, 600.0));
            step(&mut ls);
        }
        BatchB::DestroySurface => {
            ls.probe.destroy_toplevel(w);
            step(&mut ls);
            ix.motion(&mut ls.server, (1.0, 0.0), (1.0, 0.0));
            step(&mut ls);
        }
        BatchB::OutputRemoved => {
            let ptr = ls.server.seat.get_pointer().unwrap();
            let s = SERIAL_COUNTER.next_serial();
            ptr.motion(&mut ls.server, None, &MotionEvent { location: Point::<f64, Logical>::from((1900.0, 1000.0)), serial: s, time: 10 });
            ptr.frame(&mut ls.server);
            step(&mut ls);
        }
        BatchB::ReactivateThenExit => {
            activate(&mut ls); // lock point observable again, all conditions hold
            step(&mut ls);
            // the lock must end AND focus changes in the same batch: leave() deactivates (W-SM-4a)
            ls.server.activation_allowed = false;
            let ptr = ls.server.seat.get_pointer().unwrap();
            let s = SERIAL_COUNTER.next_serial();
            ptr.motion(&mut ls.server, None, &MotionEvent { location: Point::<f64, Logical>::from((950.0, 900.0)), serial: s, time: 12 });
            ptr.frame(&mut ls.server);
            step(&mut ls);
        }
        BatchB::HeldButtonThenExit => {
            ix.button(&mut ls.server, BTN_LEFT, true);
            step(&mut ls);
            ix.motion(&mut ls.server, (900.0, 0.0), (900.0, 0.0)); // over W2, implicit grab keeps W
            step(&mut ls);
            ix.button(&mut ls.server, BTN_LEFT, false); // grab ends: focus re-evaluated
            step(&mut ls);
            ix.motion(&mut ls.server, (1.0, 0.0), (1.0, 0.0));
            step(&mut ls);
        }
    }
    let wire: Vec<String> = ls
        .probe
        .log
        .iter()
        .filter(|(t, s)| *t >= t0 && (s.contains("wl_pointer.enter") || s.contains("wl_pointer.leave") || s.contains("_pointer.")))
        .filter(|(_, s)| !s.contains("relative"))
        .map(|(t, s)| format!("t={:>7.3}ms {s}", *t as f64 / 1e3))
        .collect();
    let cevents: Vec<String> = ls
        .probe
        .log
        .iter()
        .filter(|(t, s)| *t >= t0 && (s.starts_with("locked_pointer.") || s.starts_with("confined_pointer.")))
        .map(|(_, s)| s.clone())
        .collect();
    let viol = validate(&cevents, oneshot);
    let ok = viol.is_empty() && ls.probe.ptr_violations.is_empty();
    println!("=== {} [{:?}; A={:?}; B={:?}]", c.name, c.kind, c.a, c.b);
    for l in &wire {
        println!("    {l}");
    }
    for v in &viol {
        println!("    VIOLATION: {v}");
    }
    for v in &ls.probe.ptr_violations {
        println!("    POINTER-ORDER VIOLATION: {v}");
    }
    println!("    -> {}", if ok { "VALID" } else { "INVALID" });
    ok
}

fn main() {
    println!("xlock_batch — E-F2-02 multi-batch constraint lifecycle; build: {}", env!("CARGO_PKG_NAME"));
    println!("smithay =0.7.0{}; lockstep; HEADLESS\n", if env!("CARGO_PKG_NAME") == "e1r-smithay-p1" { " + [patch] P1 (leave() deactivates only an active constraint)" } else { " (published, unpatched)" });
    use BatchA::*;
    use BatchB::*;
    let cases = [
        Case { name: "E1 persistent lock: explicit deactivate, later exit to W2", kind: Kind::Lock { oneshot: false }, a: ExplicitDeactivate, b: ExitToW2 },
        Case { name: "E2 persistent lock: explicit deactivate, later exit to empty", kind: Kind::Lock { oneshot: false }, a: ExplicitDeactivate, b: ExitToEmpty },
        Case { name: "E3 oneshot lock: explicit deactivate, later exit", kind: Kind::Lock { oneshot: true }, a: ExplicitDeactivate, b: ExitToW2 },
        Case { name: "E4 persistent lock: explicit deactivate, surface destroyed", kind: Kind::Lock { oneshot: false }, a: ExplicitDeactivate, b: DestroySurface },
        Case { name: "E5 persistent lock: explicit deactivate, output removed", kind: Kind::Lock { oneshot: false }, a: ExplicitDeactivate, b: OutputRemoved },
        Case { name: "E6 persistent lock: explicit deactivate, reactivation, exit", kind: Kind::Lock { oneshot: false }, a: ExplicitDeactivate, b: ReactivateThenExit },
        Case { name: "E7 persistent lock: explicit deactivate, held button, exit", kind: Kind::Lock { oneshot: false }, a: ExplicitDeactivate, b: HeldButtonThenExit },
        Case { name: "E8 persistent confinement: explicit deactivate, later exit", kind: Kind::Confine, a: ExplicitDeactivate, b: ExitToW2 },
        Case { name: "E9 persistent lock never activated, exit (W-SM-4b persistent)", kind: Kind::Lock { oneshot: false }, a: NeverActivated, b: ExitToW2 },
        Case { name: "E10 oneshot lock never activated, exit (W-SM-4b / X-12)", kind: Kind::Lock { oneshot: true }, a: NeverActivated, b: ExitToW2 },
        Case { name: "E11 public-API alternative: end by leave, return, exit", kind: Kind::Lock { oneshot: false }, a: LeaveInsteadThenReturn, b: ExitToW2 },
    ];
    let mut valid = 0;
    let mut names = vec![];
    for c in &cases {
        if run(c) {
            valid += 1;
        } else {
            names.push(c.name.split(' ').next().unwrap());
        }
        println!();
    }
    println!("SUMMARY: {valid}/{} cases VALID; INVALID: {:?}", cases.len(), names);
}
