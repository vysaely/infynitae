//! OB-54 (audit Q-B3): a oneshot pointer lock that ends must stay ended
//! (zwp_pointer_constraints_v1: "the constraint is destroyed" after a oneshot deactivation), and
//! Core's constraint state must agree with Smithay's (INV-INPUT-5: "core and Smithay keep the same
//! constraint state").
//!
//! Smithay 0.7.0 keeps a constraint's lifetime private (`PointerConstraint::lifetime` is not
//! `pub`), so the adapter cannot report it and reports every constraint as persistent. A oneshot
//! lock that Smithay ends through `leave()` (the delivered pointer focus changes in the batch that
//! ends it, W-SM-4a) is destroyed by Smithay, while Core keeps it pending and later activates it
//! again: Core then believes in a lock the client no longer has, and withholds absolute motion.
//!
//! Path: x8, W focused, a oneshot lock at W-local (630, 520). A pan moves W 1300 px to the left;
//! at the release the lock point is no longer observable, so the lock ends, and the pointer, which
//! stayed where it was during the pan, now hits no surface (focus leaves W: Smithay's `leave()`).
//! A second pan brings W back under the pointer; its release reconciles again.

mod common;

use common::input::*;
use infynitae_backend_headless::set_focus;
use infynitae_core::input::bindings::BTN_MIDDLE;
use smithay::wayland::pointer_constraints::with_pointer_constraint;

fn smithay_has_constraint(r: &Rig) -> bool {
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
    with_pointer_constraint(&root, &ptr, |c| c.is_some())
}

fn pan(r: &mut Rig, dx: f64) {
    r.button(BTN_MIDDLE, true, logo());
    r.motion((dx, 0.0));
    r.button(BTN_MIDDLE, false, logo());
}

#[test]
fn an_ended_oneshot_lock_is_never_activated_again() {
    let mut r = Rig::x8();
    let w = r.ls.state.elements().next().expect("W");
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    r.ls.pump();
    r.frame();
    let qh = r.ls.qh.clone();
    r.ls.probe.lock_pointer(&qh, 0, true);
    r.ls.pump();
    r.frame();
    assert!(
        r.ls.state.core().constraint().is_some(),
        "the lock is active"
    );
    assert!(smithay_has_constraint(&r));

    let m = r.mark();
    pan(&mut r, -1300.0);
    assert_eq!(
        r.ls.state.core().constraint(),
        None,
        "the lock point left the output: the lock ended"
    );
    assert!(
        r.since(m).contains(&"unlocked"),
        "the client was told: {:?}",
        r.since(m)
    );
    assert!(
        !smithay_has_constraint(&r),
        "a oneshot constraint is destroyed when it ends"
    );

    // W back under the pointer: nothing may activate a lock the client no longer has
    let m = r.mark();
    pan(&mut r, 1300.0);
    assert_eq!(
        r.ls.state.core().constraint(),
        None,
        "Core re-activated a oneshot lock that ended: {:?}",
        r.since(m)
    );
    assert_eq!(
        r.ls.state.core().pending_constraint(),
        None,
        "an ended oneshot lock is not pending"
    );
    let m = r.mark();
    r.motion((3.0, 0.0));
    assert!(
        r.since(m).iter().any(|l| l.starts_with("motion ")),
        "absolute motion reaches the unlocked client: {:?}",
        r.since(m)
    );
}

#[test]
fn an_ended_persistent_lock_activates_again() {
    // the counterpart: a persistent lock that ends stays pending and comes back (protocol)
    let mut r = Rig::x8();
    let w = r.ls.state.elements().next().expect("W");
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    r.ls.pump();
    r.frame();
    let qh = r.ls.qh.clone();
    r.ls.probe.lock_pointer(&qh, 0, false);
    r.ls.pump();
    r.frame();
    pan(&mut r, -1300.0);
    assert_eq!(r.ls.state.core().constraint(), None);
    assert!(
        smithay_has_constraint(&r),
        "a persistent constraint survives"
    );
    pan(&mut r, 1300.0);
    assert!(
        r.ls.state.core().constraint().is_some(),
        "the persistent lock is active again"
    );
    assert!(smithay_has_constraint(&r));
}
