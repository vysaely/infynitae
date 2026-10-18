//! OB-55 (audit Q-B2): `output_reconfigured` must not send absolute motion to a locked surface.
//! zwp_pointer_constraints_v1, `lock_pointer`: "While the lock of this object is active, the
//! wl_pointer objects of the associated seat will not emit any wl_pointer.motion events." M10's
//! reconfiguration row allows "at most one" true-position event; with a kept lock that is none,
//! as after a fit or restore (C12.F17 K4, proposed). Whether INV-INPUT-5's text is widened to say
//! so is the owner's Q2; the protocol rule holds either way.
//!
//! Path: x8, W focused and locked at W-local (630, 520); output A reconfigured from 1920 × 1080 to
//! 2560 × 1440 at scale 1 (the lock point stays observable, so the lock is kept).

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
