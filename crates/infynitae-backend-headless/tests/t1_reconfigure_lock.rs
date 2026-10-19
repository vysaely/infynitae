//! No absolute motion to a locked surface on paths other than a camera action (C12.F17 Q2, owner
//! decision 2026-10-19: INV-INPUT-5 widened). zwp_pointer_constraints_v1, `lock_pointer`: "While
//! the lock of this object is active, the wl_pointer objects of the associated seat will not emit
//! any wl_pointer.motion events."
//!
//! - OB-55 (audit Q-B2): `output_reconfigured`. x8, W focused and locked at W-local (630, 520);
//!   output A reconfigured from 1920 × 1080 to 2560 × 1440 at scale 1 (the lock point stays
//!   observable, so the lock is kept): no motion; without a lock the one true-position event.
//! - A presentation's start and end (`presentation_membership`, found by the independent review
//!   of 2026-10-20): W, locked, enters protocol fullscreen on A and leaves it; the lock is kept
//!   throughout and no motion reaches W; without a lock each change sends the one event (M10).

mod common;

use common::input::*;
use common::output;
use infynitae_backend_headless::set_focus;

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
    }
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
