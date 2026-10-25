//! Production path tests for cancelling a camera pan with Escape (2026-10-18; M22: interaction |
//! cancel | Normal | no revert; owned buttons stay owned). A cancel is an interaction termination, so it
//! has the consequences of the initiator's release, on the production path with a real client:
//! - INV-SCALE-3: "The epoch ends at interaction termination": W's CameraPan epoch settles;
//! - INV-INPUT-5: reconciliation runs at the end of a compositor interaction: a lock the client
//!   asked for during the pan (deferred) is activated;
//! - M10 "Kept | interaction ends | ≤ 1 true-position event per batch": one motion, as at a
//!   release;
//! - "no revert": the camera keeps the panned value; the initiator's later release is consumed.
//!
//! Each test compares the cancel with the plain release on the same setup, so the oracle is the
//! release path the existing pan tests already establish.

mod common;

use common::input::*;
use infynitae_backend_headless::{keyboard_key, set_focus};
use infynitae_core::input::bindings::BTN_MIDDLE;
use infynitae_core::input::keys::KEY_ESC;
use infynitae_core::pointer::ConstraintKind;

/// Escape pressed and released on the virtual keyboard.
fn escape(r: &mut Rig) {
    for pressed in [true, false] {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, KEY_ESC, pressed, r.t).expect("key");
        r.ls.pump();
    }
    r.frame();
}

fn epoch_open(r: &Rig) -> bool {
    let w = r.ls.state.elements().next().expect("W");
    r.ls.state
        .core()
        .scale_state(w)
        .expect("W has a scale state")
        .epoch
        .open
}

/// x8, a pan begun (Logo + middle button) and moved: W's CameraPan epoch is open.
fn panning() -> Rig {
    let mut r = Rig::x8();
    r.button(BTN_MIDDLE, true, logo());
    for _ in 0..3 {
        r.motion((7.0, 3.0));
    }
    assert!(
        r.ls.state.core().interaction().active(),
        "the pan owns the pointer"
    );
    assert!(epoch_open(&r), "the pan opened W's epoch");
    r
}

#[test]
fn an_escape_cancel_settles_the_pan_epoch_like_a_release() {
    // reference: the release settles it
    let mut released = panning();
    released.button(BTN_MIDDLE, false, logo());
    assert!(!epoch_open(&released), "the release settles the epoch");

    let mut r = panning();
    let camera = r.camera();
    let m = r.mark();
    escape(&mut r);
    assert!(
        !r.ls.state.core().interaction().active(),
        "Escape cancels the pan"
    );
    assert!(
        !epoch_open(&r),
        "the cancel is an interaction termination (INV-SCALE-3)"
    );
    assert_eq!(r.camera(), camera, "no revert");
    let after_cancel = r
        .since(m)
        .iter()
        .filter(|l| l.starts_with("motion "))
        .count();
    assert_eq!(
        after_cancel,
        1,
        "one true-position event (M10): {:?}",
        r.since(m)
    );

    // the initiator stays compositor-owned: its release is consumed and changes nothing
    let m = r.mark();
    r.button(BTN_MIDDLE, false, logo());
    assert_nothing_delivered(&r.since(m), "the initiator's release after a cancel");
    assert_eq!(r.camera(), camera);
}

#[test]
fn an_escape_cancel_runs_the_reconciliation_the_pan_deferred() {
    let start = || {
        let mut r = Rig::x8();
        let w = r.ls.state.elements().next().expect("W");
        set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
        r.ls.pump();
        r.frame();
        r.button(BTN_MIDDLE, true, logo());
        r.motion((2.0, 1.0));
        let qh = r.ls.qh.clone();
        r.ls.probe.lock_pointer(&qh, 0, false);
        r.ls.pump();
        r.frame();
        assert_eq!(
            r.ls.state.core().constraint(),
            None,
            "deferred while the pan owns the pointer (INV-INPUT-5)"
        );
        r
    };

    // reference: the release activates it
    let mut released = start();
    released.button(BTN_MIDDLE, false, logo());
    assert_eq!(
        released.ls.state.core().constraint().map(|c| c.kind),
        Some(ConstraintKind::Lock),
        "the release reconciles"
    );

    let mut r = start();
    escape(&mut r);
    assert_eq!(
        r.ls.state.core().constraint().map(|c| c.kind),
        Some(ConstraintKind::Lock),
        "the cancel ends the interaction, so the deferred lock is reconciled"
    );
    r.button(BTN_MIDDLE, false, logo());
    assert_eq!(
        r.ls.state.core().constraint().map(|c| c.kind),
        Some(ConstraintKind::Lock)
    );
}
