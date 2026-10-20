//! No absolute motion to a locked surface on paths other than a camera action (C12.F17 Q2, owner
//! decision 2026-10-19: INV-INPUT-5 widened). zwp_pointer_constraints_v1, `lock_pointer`: "While
//! the lock of this object is active, the wl_pointer objects of the associated seat will not emit
//! any wl_pointer.motion events."
//!
//! - OB-55 (audit Q-B2): `output_reconfigured`. x8, W focused and locked at W-local (630, 520);
//!   output A reconfigured from 1920 × 1080 to 2560 × 1440 at scale 1 (the lock point stays
//!   observable, so the lock is kept): no motion, the physical pointer at the derived lock point;
//!   under a kept confinement and without a constraint the one true-position event.
//! - A presentation's start and end (`presentation_membership`, found by the independent review
//!   of 2026-10-20): W, locked, enters protocol fullscreen on A and leaves it; the lock is kept
//!   throughout and no motion reaches W; without a lock each change sends the one event (M10).

mod common;

use common::input::*;
use common::output;
use infynitae_backend_headless::set_focus;
use infynitae_core::pointer::{ConstraintKind, PointerState};
use smithay::wayland::pointer_constraints::with_pointer_constraint;

fn pointer(r: &Rig) -> (f64, f64) {
    match r.ls.state.core().pointer() {
        PointerState::Located { p, .. } => p,
        other => panic!("pointer {other:?}"),
    }
}

fn smithay_active(r: &Rig) -> bool {
    let w = r.ls.state.elements().next().expect("W");
    let root =
        r.ls.state
            .window(w)
            .unwrap()
            .toplevel()
            .unwrap()
            .wl_surface()
            .clone();
    let ptr = r.ls.state.seat().get_pointer().expect("seat pointer");
    with_pointer_constraint(&root, &ptr, |c| c.is_some_and(|c| c.is_active()))
}

fn locked_rig(oneshot: bool) -> Rig {
    let mut r = Rig::x8();
    let w = r.ls.state.elements().next().expect("W");
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    r.ls.pump();
    r.frame();
    let qh = r.ls.qh.clone();
    r.ls.probe.lock_pointer(&qh, 0, oneshot);
    r.ls.pump();
    r.frame();
    assert!(r.ls.state.core().constraint().is_some(), "locked");
    r
}

#[test]
fn a_reconfiguration_sends_no_motion_to_a_kept_lock() {
    for oneshot in [false, true] {
        let mut r = locked_rig(oneshot);
        let o = r.ls.output();
        let m = r.mark();
        r.ls.reconfigure(o, &output((2560, 1440), 120));
        r.frame();
        assert!(
            r.ls.state.core().constraint().is_some(),
            "oneshot {oneshot}: the lock point is still observable, the lock is kept"
        );
        let wire = r.since(m);
        assert!(
            !wire.iter().any(|l| l.starts_with("motion ")),
            "oneshot {oneshot}: wl_pointer.motion reached a locked surface: {wire:?}"
        );
        // F-LOCK-1 (OB-55's pass criterion): the physical pointer follows the derived lock point,
        // W-local (630, 520) under the new view: the camera keeps centre (0, 0) at zoom 1, so the
        // root surface origin (−420, −320) is at (1280 − 420, 720 − 320) on the 2560 × 1440 view
        let (p, want) = (pointer(&r), (1280.0 + 210.0, 720.0 + 200.0));
        let eps = 1.0 / 65536.0;
        assert!(
            (p.0 - want.0).abs() <= eps && (p.1 - want.1).abs() <= eps,
            "oneshot {oneshot}: pointer {p:?}, lock point {want:?}"
        );
    }
}

/// OB-55's third case: a kept confinement after `output_reconfigured` gets exactly the one
/// true-position event (M10, C12.F17.6), the confinement stays active in Core and Smithay, no
/// `unconfined`, and the physical pointer stays where F-RECONF-1 leaves it (no warp): (1170, 740),
/// inside the 2560 × 1440 view, so at W-local (1170 − 860, 740 − 400) = (310, 340).
#[test]
fn a_reconfiguration_sends_one_true_position_event_to_a_kept_confinement() {
    let mut r = Rig::x8();
    let w = r.ls.state.elements().next().expect("W");
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    r.ls.pump();
    r.frame();
    let qh = r.ls.qh.clone();
    r.ls.probe.confine_pointer(&qh, 0);
    r.ls.pump();
    r.frame();
    assert_eq!(
        r.ls.state.core().constraint().map(|c| c.kind),
        Some(ConstraintKind::Confine)
    );
    let p0 = pointer(&r);
    assert_eq!(p0, (1170.0, 740.0), "x8: the pointer at W-local (630, 520)");
    let o = r.ls.output();
    let m = r.mark();
    r.ls.reconfigure(o, &output((2560, 1440), 120));
    r.frame();
    let wire: Vec<String> = r.since(m).iter().map(|s| s.to_string()).collect();
    let delivered: Vec<(f64, f64)> = wire
        .iter()
        .filter_map(|l| l.strip_prefix("motion "))
        .map(|xy| {
            let (x, y) = xy.split_once(',').expect("x,y");
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect();
    assert_eq!(delivered.len(), 1, "one true-position event: {wire:?}");
    let want = (310.0, 340.0);
    assert!(
        (delivered[0].0 - want.0).abs() <= 1.0 / 256.0
            && (delivered[0].1 - want.1).abs() <= 1.0 / 256.0,
        "delivered {:?}, expected {want:?}",
        delivered[0]
    );
    assert_eq!(pointer(&r), p0, "the physical pointer moved");
    assert_eq!(
        r.ls.state.core().constraint().map(|c| c.kind),
        Some(ConstraintKind::Confine),
        "confinement lost"
    );
    assert!(smithay_active(&r), "Smithay's confinement ended");
    assert!(!wire.iter().any(|l| l == "unconfined"), "{wire:?}");
}

#[test]
fn a_reconfiguration_still_reconciles_an_unconstrained_pointer() {
    // the counterpart: without a lock the one true-position event is sent (M10)
    let mut r = Rig::x8();
    let o = r.ls.output();
    let m = r.mark();
    r.ls.reconfigure(o, &output((2560, 1440), 120));
    r.frame();
    let motions = r
        .since(m)
        .iter()
        .filter(|l| l.starts_with("motion "))
        .count();
    assert_eq!(motions, 1, "{:?}", r.since(m));
}

/// Sets or unsets fullscreen on W and returns the client's lines since `m`.
fn fullscreen(r: &mut Rig, on: bool) -> Vec<String> {
    let m = r.mark();
    let t = r.ls.probe.surfaces[0].toplevel.as_ref().unwrap();
    if on {
        t.set_fullscreen(None);
    } else {
        t.unset_fullscreen();
    }
    r.ls.pump();
    r.frame();
    r.since(m).iter().map(|s| s.to_string()).collect()
}

fn motions(wire: &[String]) -> usize {
    wire.iter().filter(|l| l.starts_with("motion ")).count()
}

#[test]
fn a_presentation_start_and_end_send_no_motion_to_a_kept_lock() {
    for oneshot in [false, true] {
        let mut r = locked_rig(oneshot);
        let o = r.ls.output();
        let w = r.ls.state.elements().next().unwrap();
        for on in [true, false] {
            let wire = fullscreen(&mut r, on);
            assert_eq!(
                r.ls.state.core().presentation(o),
                on.then_some(w),
                "oneshot {oneshot}: presentation {on}"
            );
            assert!(
                r.ls.state.core().constraint().is_some(),
                "oneshot {oneshot}, presentation {on}: the lock was not kept"
            );
            assert_eq!(
                motions(&wire),
                0,
                "oneshot {oneshot}, presentation {on}: wl_pointer.motion reached a locked surface: {wire:?}"
            );
        }
    }
}

#[test]
fn a_presentation_start_and_end_still_reconcile_an_unconstrained_pointer() {
    let mut r = Rig::x8();
    let w = r.ls.state.elements().next().unwrap();
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    r.ls.pump();
    r.frame();
    for on in [true, false] {
        let wire = fullscreen(&mut r, on);
        assert_eq!(motions(&wire), 1, "presentation {on}: {wire:?}");
    }
}
