//! E1R / E-F2-05: Outputs = ∅, observed at the client.
//!
//! Sequence: one output → focused interaction (W has pointer focus; optional constraint / held
//! client button) → LAST output removed → stable zero-output state (physical input arrives) → a new
//! output is connected → reconciliation.
//!
//! E1R rule under test (INV-OUTPUT-1 / OBJ-20 / M4, C12.E1R): when the survivor set is empty the
//! pointer enters `NoOutput` — no nearest-output selection, no fabricated output or coordinate. In the
//! SAME batch the adapter (1) ends a client-owned implicit grab (`PointerHandle::unset_grab`), then
//! (2) clears pointer focus with one `motion(focus = None)` at Smithay's unchanged stored location
//! (not delivered to anyone), so the client receives `wl_pointer.leave` and Smithay ends ACTIVE
//! constraints through `leave()` (one `unlocked`/`unconfined`). While `NoOutput`, physical motion and
//! buttons are consumed by core and delivered nowhere (no relative motion either). On connect, the
//! startup placement rule (centre of the new output) produces one ordinary routed motion.
//!
//! Negative controls:
//!   N1 sealed F-RELOCATE-1 on an empty survivor set (nearest-output selection) — panics;
//!   N2 zero-output transition WITHOUT `unset_grab` under a held client button — the implicit grab
//!      keeps focus: the client retains pointer focus on a surface whose output no longer exists.
//!
//! The harness cannot destroy a wl_output global; the seat-visible part of the transition (the only
//! part that reaches wl_pointer / constraints) is executed exactly as core prescribes. wl_surface.leave
//! for the removed output is covered by the membership rule (F-MEMBER-1), not by this binary.
//! Output: docs/foundation/evidence/E1R/xzero-{stock,p1}-output.txt
use b8_harness::{
    input::{Intake, PanBinding, PanFocusMode},
    lockstep::Lockstep,
    server::*,
};
use smithay::{
    input::{
        pointer::{
            AxisFrame, ButtonEvent, Focus, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent, GesturePinchEndEvent,
            GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
            MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
        },
        SeatHandler,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::pointer_constraints::with_pointer_constraint,
};

/// E1R: the adapter's representation of `PointerState::NoOutput` at the seat: a compositor grab that
/// delivers nothing. Installed with `Focus::Clear` (Smithay clears focus through its internal motion
/// path: `leave` + active-constraint end, no client motion), removed before the first placement.
struct ZeroOutputGrab {
    start: GrabStartData<Server>,
}
type F = Option<(<Server as SeatHandler>::PointerFocus, Point<f64, Logical>)>;
type H<'a> = PointerInnerHandle<'a, Server>;
impl PointerGrab<Server> for ZeroOutputGrab {
    fn motion(&mut self, _: &mut Server, _: &mut H<'_>, _: F, _: &MotionEvent) {}
    fn relative_motion(&mut self, _: &mut Server, _: &mut H<'_>, _: F, _: &RelativeMotionEvent) {}
    fn button(&mut self, _: &mut Server, _: &mut H<'_>, _: &ButtonEvent) {}
    fn axis(&mut self, _: &mut Server, _: &mut H<'_>, _: AxisFrame) {}
    fn frame(&mut self, _: &mut Server, _: &mut H<'_>) {}
    fn gesture_swipe_begin(&mut self, _: &mut Server, _: &mut H<'_>, _: &GestureSwipeBeginEvent) {}
    fn gesture_swipe_update(&mut self, _: &mut Server, _: &mut H<'_>, _: &GestureSwipeUpdateEvent) {}
    fn gesture_swipe_end(&mut self, _: &mut Server, _: &mut H<'_>, _: &GestureSwipeEndEvent) {}
    fn gesture_pinch_begin(&mut self, _: &mut Server, _: &mut H<'_>, _: &GesturePinchBeginEvent) {}
    fn gesture_pinch_update(&mut self, _: &mut Server, _: &mut H<'_>, _: &GesturePinchUpdateEvent) {}
    fn gesture_pinch_end(&mut self, _: &mut Server, _: &mut H<'_>, _: &GesturePinchEndEvent) {}
    fn gesture_hold_begin(&mut self, _: &mut Server, _: &mut H<'_>, _: &GestureHoldBeginEvent) {}
    fn gesture_hold_end(&mut self, _: &mut Server, _: &mut H<'_>, _: &GestureHoldEndEvent) {}
    fn start_data(&self) -> &GrabStartData<Server> {
        &self.start
    }
    fn unset(&mut self, _: &mut Server) {}
}
impl std::fmt::Debug for ZeroOutputGrab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ZeroOutputGrab")
    }
}
#[allow(dead_code)]
fn _focus_type_check(_: Option<WlSurface>) {}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    /// E1R rule: set_grab(ZeroOutputGrab, Focus::Clear); motion(None); unset before placement
    E1r,
    /// variant: unset_grab (restores focus with a motion), then motion(None)
    UnsetGrab,
    /// N2: no grab handling at all
    Nothing,
}

const BTN_LEFT: u32 = 0x110;

#[derive(Clone, Copy, PartialEq, Debug)]
enum C {
    None,
    PersistentLock,
    OneshotLock,
    Confine,
}

/// Sealed F-RELOCATE-1 (C12.R1): "choose the surviving output whose Arrangement rect is nearest to a".
fn sealed_relocate(survivors: &[((f64, f64), (f64, f64))], a: (f64, f64)) -> (usize, (f64, f64)) {
    let (i, (org, size)) = survivors
        .iter()
        .enumerate()
        .min_by(|x, y| {
            let d = |r: &((f64, f64), (f64, f64))| {
                let cx = a.0.clamp(r.0 .0, r.0 .0 + r.1 .0);
                let cy = a.1.clamp(r.0 .1, r.0 .1 + r.1 .1);
                (cx - a.0).powi(2) + (cy - a.1).powi(2)
            };
            d(x.1).partial_cmp(&d(y.1)).unwrap()
        })
        .expect("F-RELOCATE-1: nearest surviving output"); // sealed formula has no ∅ branch
    (i, (a.0 - org.0, a.1 - org.1).min_pair(*size))
}
trait MinPair {
    fn min_pair(self, s: (f64, f64)) -> (f64, f64);
}
impl MinPair for (f64, f64) {
    fn min_pair(self, s: (f64, f64)) -> (f64, f64) {
        (self.0.min(s.0), self.1.min(s.1))
    }
}

fn run(c: C, held_button: bool, mode: Mode) -> bool {
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    let qh = ls.qh.clone();
    ls.probe.bind_input(&ls.globals, &qh);
    ls.server.pending_anchors.push_back((600, 300)); // W: 600..1400 x 300..900 — covers the centre (960,540)
    let w = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, false);
    ls.pump();
    ls.run_until(20_000);
    let step = |ls: &mut Lockstep| {
        ls.pump();
        let t = ls.server.now_us + 500;
        ls.run_until(t);
    };
    ls.server.activation_allowed = false; // activation decided by core (this experiment)
    let mut ix = Intake::new(PanBinding { button: 0x112, needs_modifier: true }, PanFocusMode::KeepFocus, 0, (699.0, 400.0));
    ix.motion(&mut ls.server, (1.0, 0.0), (1.0, 0.0)); // focus W
    step(&mut ls);
    match c {
        C::None => {}
        C::PersistentLock => ls.probe.lock_pointer(&qh, w, false),
        C::OneshotLock => ls.probe.lock_pointer(&qh, w, true),
        C::Confine => ls.probe.confine_pointer(&qh, w),
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
    if c != C::None {
        activate(&mut ls);
        step(&mut ls);
    }
    if held_button {
        ix.button(&mut ls.server, BTN_LEFT, true);
        step(&mut ls);
    }
    // ---- LAST output removed (one batch)
    let t_remove = ls.server.now_us;
    let ptr = ls.server.seat.get_pointer().unwrap();
    let loc = ptr.current_location(); // Smithay's stored value, unchanged and delivered to no one
    match mode {
        Mode::E1r => {
            let start = GrabStartData { focus: None, button: 0, location: loc };
            ptr.set_grab(&mut ls.server, ZeroOutputGrab { start }, SERIAL_COUNTER.next_serial(), Focus::Clear);
        }
        Mode::UnsetGrab => {
            if ptr.is_grabbed() {
                ptr.unset_grab(&mut ls.server, SERIAL_COUNTER.next_serial(), 30);
            }
        }
        Mode::Nothing => {}
    }
    ptr.motion(&mut ls.server, None, &MotionEvent { location: loc, serial: SERIAL_COUNTER.next_serial(), time: 31 });
    ptr.frame(&mut ls.server);
    step(&mut ls);
    let focus_after_removal = ptr.current_focus().is_some();
    // ---- stable zero-output state: physical input is consumed by core, delivered nowhere
    let t_zero = ls.server.now_us;
    // (core: PointerState::NoOutput ⇒ no wl_pointer.motion / relative_motion / button is produced)
    ls.server.now_us += 0;
    step(&mut ls);
    if held_button {
        ix.client_buttons.remove(&BTN_LEFT); // the physical release arrives in the zero-output state: consumed
    }
    let t_reconnect = ls.server.now_us;
    let leaked_in_zero = ls.probe.log.iter().filter(|(t, s)| *t >= t_zero && *t < t_reconnect && s.contains("wl_pointer")).count();
    // ---- new output connected: leave NoOutput (remove the zero-output grab first; its restore motion
    // carries pending focus None, i.e. nothing is sent), then startup placement = centre, one routed motion
    if mode == Mode::E1r {
        ptr.unset_grab(&mut ls.server, SERIAL_COUNTER.next_serial(), 40);
    }
    ix.p = (960.0, 540.0);
    ix.deliver_position(&mut ls.server);
    step(&mut ls);
    // reconciliation (INV-INPUT-5): all conditions hold again for a surviving constraint
    if matches!(c, C::PersistentLock | C::Confine) {
        activate(&mut ls);
        step(&mut ls);
    }
    let wire: Vec<String> = ls
        .probe
        .log
        .iter()
        .filter(|(t, s)| *t >= t0 && (s.contains("wl_pointer") || s.contains("_pointer.")) && !s.contains("relative"))
        .map(|(t, s)| {
            let phase = if *t < t_remove {
                "focused   "
            } else if *t < t_zero {
                "removal   "
            } else if *t < t_reconnect {
                "zero-out  "
            } else {
                "reconnect "
            };
            format!("t={:>7.3}ms [{phase}] {s}", *t as f64 / 1e3)
        })
        .collect();
    // validity
    let ev: Vec<&str> = ls.probe.log.iter().filter(|(t, _)| *t >= t0).map(|(_, s)| s.as_str()).collect();
    let mut active = false;
    let mut defunct = false;
    let mut viol: Vec<String> = vec![];
    for e in &ev {
        let act = e.ends_with("_pointer.locked") || e.ends_with("_pointer.confined");
        let de = e.ends_with("_pointer.unlocked") || e.ends_with("_pointer.unconfined");
        if (act || de) && defunct {
            viol.push(format!("{e} after oneshot end"));
        } else if act {
            if active {
                viol.push(format!("{e} while active"));
            }
            active = true;
        } else if de {
            if !active {
                viol.push(format!("{e} while not active"));
            }
            active = false;
            defunct |= c == C::OneshotLock;
        } else if active && matches!(c, C::PersistentLock | C::OneshotLock) && e.starts_with("wl_pointer.motion") {
            viol.push(format!("{e} delivered to a LOCKED surface (INV-INPUT-5)"));
        }
    }
    let removal: Vec<&str> = ls.probe.log.iter().filter(|(t, _)| *t >= t_remove && *t < t_zero).map(|(_, s)| s.as_str()).collect();
    let leave_at_removal = removal.iter().any(|s| s.contains("wl_pointer.leave W"));
    let end_at_removal = removal.iter().filter(|s| s.ends_with("unlocked") || s.ends_with("unconfined")).count();
    let want_end = if c == C::None { 0 } else { 1 };
    let enter_after = ls.probe.log.iter().any(|(t, s)| *t >= t_reconnect && s.contains("wl_pointer.enter W"));
    let ok = !focus_after_removal
        && leave_at_removal
        && end_at_removal == want_end
        && leaked_in_zero == 0
        && enter_after
        && viol.is_empty()
        && ls.probe.ptr_violations.is_empty();
    println!("=== constraint={c:?} held_button={held_button} removal mode={mode:?}");
    for l in &wire {
        println!("    {l}");
    }
    for v in viol.iter().chain(ls.probe.ptr_violations.iter()) {
        println!("    VIOLATION: {v}");
    }
    println!(
        "    focus cleared at removal={} | wl_pointer.leave at removal={leave_at_removal} | constraint end events at removal={end_at_removal} (want {want_end}) | events in zero-output state={leaked_in_zero} (want 0) | enter after reconnect={enter_after}",
        !focus_after_removal
    );
    println!("    -> {}", if ok { "VALID" } else { "INVALID" });
    ok
}

fn main() {
    println!("xzero — E-F2-05 zero-output transition; build: {}; smithay =0.7.0{}; lockstep; HEADLESS\n", env!("CARGO_PKG_NAME"), if env!("CARGO_PKG_NAME") == "e1r-smithay-p1" { " + P1" } else { " (published)" });
    // N1: sealed F-RELOCATE-1 on ∅
    let r = std::panic::catch_unwind(|| sealed_relocate(&[], (100.0, 100.0)));
    println!("N1 sealed F-RELOCATE-1 with survivors = ∅: {}", if r.is_err() { "PANICS (negative control reproduces E-F2-05)" } else { "returned (UNEXPECTED)" });
    let one = sealed_relocate(&[((1920.0, 0.0), (1920.0, 1080.0))], (100.0, 100.0));
    println!("   control: sealed F-RELOCATE-1 with one survivor -> {:?}\n", one);
    let mut all = r.is_err();
    for (c, held) in [
        (C::None, false),
        (C::PersistentLock, false),
        (C::OneshotLock, false),
        (C::Confine, false),
        (C::None, true),
        (C::PersistentLock, true),
    ] {
        all &= run(c, held, Mode::E1r);
        println!();
    }
    println!("N2: zero-output transition WITHOUT grab handling under a held client button (negative control):");
    let n2 = run(C::None, true, Mode::Nothing);
    println!("N2 -> {}\n", if n2 { "VALID (UNEXPECTED)" } else { "INVALID as predicted: the implicit grab keeps focus on a surface of a vanished output" });
    all &= !n2;
    println!("N3: public unset_grab instead of the zero-output grab, held button + active lock (negative control):");
    let n3 = run(C::PersistentLock, true, Mode::UnsetGrab);
    println!("N3 -> {}\n", if n3 { "VALID (UNEXPECTED)" } else { "INVALID as predicted: unset_grab's focus restore delivers an absolute motion to the locked surface" });
    all &= !n3;
    println!("RESULT: {}", if all { "PASS (E1R rule valid in all cases; negative controls fail)" } else { "FAIL" });
}
