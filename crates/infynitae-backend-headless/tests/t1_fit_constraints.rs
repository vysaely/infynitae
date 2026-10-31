//! Pointer reconciliation under finding M1: pointer reconciliation after a viewport fit and a
//! restore under each pointer-constraint state on the production path (core + adapter + headless
//! backend + a real client), driven by the virtual keyboard (Super+Enter).
//!
//! The camera action moves W under a pointer that stays where it is on the output. M10's
//! view-change rule then sends exactly one true-position event when the pointer is on O and no
//! interaction owns it, with these constraint rules (INV-INPUT-5):
//! - no constraint: one motion at the new surface-local position;
//! - a kept confinement: the same one motion, and the confinement stays active (Core and Smithay);
//! - a confinement whose region no longer holds the pointer: it ends (`unconfined` once), then the
//!   one motion;
//! - a kept lock: no absolute motion; the physical pointer follows the derived lock point
//!   (F-LOCK-1) under the new view;
//! - during a pan (an interaction owns the pointer): the bindings do not resolve, nothing changes.
//!
//! The oracle is hand-derived from the x8 setup (W: 800 × 600 content, 20 px shadow, window
//! geometry centred at Canvas (0, 0), so its root surface origin is (−420, −320); the pointer at
//! W-local (630, 520), output A 1920 × 1080 at scale 1): Super+Enter sets the camera to centre (0, 0)
//! at zoom 1.8, so the pointer's surface-local position becomes ((p − 960) / 1.8 + 420,
//! (p − 540) / 1.8 + 320); the restore brings back zoom 1 and (630, 520).
//!
//! Not covered here: a lock that ends because its point leaves the output. That path is unchanged
//! by the M1 fix, and on it the adapter reports every constraint to Core as persistent, so Core
//! re-activates a oneshot lock Smithay has ended (OB-54).

mod common;

use common::input::*;
use infynitae_backend_headless::{keyboard_key, set_focus};
use infynitae_core::input::keys::{KEY_ENTER, KEY_LEFTMETA, KEY_LEFTSHIFT};
use infynitae_core::pointer::{ConstraintKind, PointerState};
use smithay::wayland::pointer_constraints::with_pointer_constraint;

/// Super+Enter on the virtual keyboard. `_restore` names what the press is meant to do: Super+Shift+F
/// was replaced by the owner (2026-10-21), and at the fit the same Super+Enter restores.
fn combo(r: &mut Rig, _restore: bool) {
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    press(KEY_LEFTMETA, true);
    press(KEY_ENTER, true);
    press(KEY_ENTER, false);
    press(KEY_LEFTMETA, false);
    r.frame();
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Setup {
    None,
    Lock,
    Confine,
    /// A confinement to a region around the pointer that no longer holds it after the fit.
    ConfineLost,
}

/// x8 with W focused and the constraint of `setup` active in Core and in Smithay.
fn rig(setup: Setup) -> Rig {
    let mut r = Rig::x8();
    let w = r.ls.state.elements().next().expect("W");
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    r.ls.pump();
    r.frame();
    let qh = r.ls.qh.clone();
    match setup {
        Setup::None => {}
        Setup::Lock => r.ls.probe.lock_pointer(&qh, 0, false),
        Setup::Confine => r.ls.probe.confine_pointer(&qh, 0),
        Setup::ConfineLost => {
            r.ls.probe
                .confine_pointer_with_region(&qh, 0, (600, 500, 60, 40))
        }
    }
    r.ls.pump();
    r.frame();
    let kind = r.ls.state.core().constraint().map(|c| c.kind);
    let want = match setup {
        Setup::None => None,
        Setup::Lock => Some(ConstraintKind::Lock),
        Setup::Confine | Setup::ConfineLost => Some(ConstraintKind::Confine),
    };
    assert_eq!(kind, want, "{setup:?}: constraint before the fit");
    assert_eq!(smithay_active(&r), want.is_some(), "{setup:?}: Smithay");
    r
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

fn pointer(r: &Rig) -> (f64, f64) {
    match r.ls.state.core().pointer() {
        PointerState::Located { p, .. } => p,
        other => panic!("pointer {other:?}"),
    }
}

/// The surface-local positions of the client's motion events in `lines`.
fn motions(lines: &[&str]) -> Vec<(f64, f64)> {
    lines
        .iter()
        .filter_map(|l| l.strip_prefix("motion "))
        .map(|xy| {
            let (x, y) = xy.split_once(',').expect("x,y");
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect()
}

/// W-local position of output point `p` under the camera centre (0, 0) at `zoom` (scale 1).
fn local(p: (f64, f64), zoom: f64) -> (f64, f64) {
    ((p.0 - 960.0) / zoom + 420.0, (p.1 - 540.0) / zoom + 320.0)
}

fn near(a: (f64, f64), b: (f64, f64), what: &str) {
    // wl_fixed carries 1/256 px; Core's hit local is on the Q16 lattice
    assert!(
        (a.0 - b.0).abs() <= 1.0 / 256.0 + 1e-9 && (a.1 - b.1).abs() <= 1.0 / 256.0 + 1e-9,
        "{what}: delivered {a:?}, expected {b:?}"
    );
}

fn count(lines: &[&str], what: &str) -> usize {
    lines.iter().filter(|l| **l == what).count()
}

/// One camera action (`shift`: restore) on `setup` and its checks; `zoom` is the zoom after it.
fn act(r: &mut Rig, setup: Setup, shift: bool, zoom: f64) {
    let what = format!("{setup:?} {}", if shift { "restore" } else { "fit" });
    let p0 = pointer(r);
    let m = r.mark();
    let sent = r.ls.state.pointer_motions();
    combo(r, shift);
    assert_eq!(
        r.ls.state.core().cameras().zoom(0),
        Some(zoom),
        "{what}: zoom"
    );
    let lines = r.since(m);
    let delivered = motions(&lines);
    match setup {
        Setup::None | Setup::Confine => {
            assert_eq!(
                delivered.len(),
                1,
                "{what}: one true-position event: {lines:?}"
            );
            assert_eq!(pointer(r), p0, "{what}: the physical pointer moved");
            near(delivered[0], local(p0, zoom), &what);
            let kind = r.ls.state.core().constraint().map(|c| c.kind);
            if setup == Setup::Confine {
                assert_eq!(
                    kind,
                    Some(ConstraintKind::Confine),
                    "{what}: confinement lost"
                );
                assert!(smithay_active(r), "{what}: Smithay's confinement ended");
                assert_eq!(count(&lines, "unconfined"), 0, "{what}: {lines:?}");
            } else {
                assert_eq!(kind, None, "{what}");
            }
        }
        Setup::Lock => {
            assert!(
                delivered.is_empty(),
                "{what}: absolute motion to a locked surface: {lines:?}"
            );
            // Core's own output, not only the wire: the adapter drops a motion repeating what
            // the client has, and one to a lock's point always does
            assert_eq!(
                r.ls.state.pointer_motions(),
                sent,
                "{what}: Core sent absolute motion to a locked surface"
            );
            assert_eq!(count(&lines, "unlocked"), 0, "{what}: {lines:?}");
            assert_eq!(
                r.ls.state.core().constraint().map(|c| c.kind),
                Some(ConstraintKind::Lock),
                "{what}: lock lost"
            );
            assert!(smithay_active(r), "{what}: Smithay's lock ended");
            // F-LOCK-1: the physical pointer is the lock point W-local (630, 520) under the view
            let want = (960.0 + 210.0 * zoom, 540.0 + 200.0 * zoom);
            // (C_O is on the Q16 grid, C12.F15.7: the projection is within 2⁻¹⁶ px of it)
            let p = pointer(r);
            let eps = 1.0 / 65536.0;
            assert!(
                (p.0 - want.0).abs() <= eps && (p.1 - want.1).abs() <= eps,
                "{what}: pointer {p:?}, lock point {want:?}"
            );
        }
        Setup::ConfineLost => unreachable!(),
    }
}

/// Fit then restore with no constraint, a kept confinement and a kept lock: one true-position
/// event (none under the lock) at the hand-derived position, constraint state preserved.
#[test]
fn fit_and_restore_reconcile_the_pointer_under_each_constraint() {
    for setup in [Setup::None, Setup::Confine, Setup::Lock] {
        let mut r = rig(setup);
        let w_local = if setup == Setup::Lock {
            None
        } else {
            Some(local(pointer(&r), 1.0))
        };
        if let Some(l) = w_local {
            near(l, (630.0, 520.0), "x8 oracle");
        }
        act(&mut r, setup, false, 1.8);
        assert_eq!(r.ls.state.core().cameras().history(0).unwrap().len(), 1);
        act(&mut r, setup, true, 1.0);
        assert_eq!(r.ls.state.core().cameras().history(0).unwrap().len(), 0);
        // the next physical motion is routed as before the fit (no stale state left behind)
        let m = r.mark();
        r.motion((1.0, 0.0));
        let lines = r.since(m);
        let n = motions(&lines).len();
        assert_eq!(
            n,
            usize::from(setup != Setup::Lock),
            "{setup:?}: next motion {lines:?}"
        );
    }
}

/// The constraint transition under finding M1: a confinement whose region no longer holds the
/// pointer after the fit ends with exactly one `unconfined` (INV-INPUT-5: "a pointer outside the
/// region after a view change ends it"; no warp), then exactly one true-position event at the new
/// position; the physical pointer does not move.
#[test]
fn a_fit_that_ends_the_confinement_ends_it_once_then_reconciles_the_pointer() {
    let mut r = rig(Setup::ConfineLost);
    let p0 = pointer(&r);
    let m = r.mark();
    combo(&mut r, false);
    assert_eq!(r.ls.state.core().cameras().zoom(0), Some(1.8));
    let lines = r.since(m);
    assert_eq!(r.ls.state.core().constraint(), None, "still confined");
    assert!(!smithay_active(&r), "Smithay still confined");
    assert_eq!(count(&lines, "unconfined"), 1, "{lines:?}");
    let delivered = motions(&lines);
    assert_eq!(delivered.len(), 1, "{lines:?}");
    let end_at = lines.iter().position(|l| *l == "unconfined").unwrap();
    let motion_at = lines.iter().position(|l| l.starts_with("motion")).unwrap();
    assert!(
        end_at < motion_at,
        "the end comes before the motion: {lines:?}"
    );
    assert_eq!(pointer(&r), p0, "no warp");
    near(delivered[0], local(p0, 1.8), "ConfineLost");
}

/// While a pan owns the pointer the bindings do not resolve: under a confinement, with one fit
/// already recorded (so a stray restore would show), Super+Enter and Super+Shift+Enter during the pan change
/// neither the zoom nor the history and deliver no pointer event; the confinement stays.
#[test]
fn no_fit_reconciliation_while_a_pan_owns_the_pointer() {
    let mut r = rig(Setup::Confine);
    combo(&mut r, false);
    assert_eq!(r.ls.state.core().cameras().history(0).unwrap().len(), 1);
    r.button(BTN_MIDDLE, true, logo());
    r.motion((3.0, 2.0));
    let m = r.mark();
    for keys in [&[KEY_ENTER][..], &[KEY_LEFTSHIFT, KEY_ENTER]] {
        for &(key, pressed) in keys
            .iter()
            .map(|k| (k, true))
            .chain(keys.iter().rev().map(|k| (k, false)))
            .collect::<Vec<_>>()
            .iter()
        {
            r.t += 1000;
            keyboard_key(&mut r.ls.state, &mut r.ls.input, *key, pressed, r.t).expect("key");
            r.ls.pump();
        }
        r.frame();
        assert_eq!(r.ls.state.core().cameras().zoom(0), Some(1.8), "{keys:?}");
        assert_eq!(
            r.ls.state.core().cameras().history(0).unwrap().len(),
            1,
            "{keys:?}"
        );
    }
    let lines = r.since(m);
    assert!(
        lines
            .iter()
            .all(|l| !l.starts_with("motion") && !l.starts_with("relative")),
        "{lines:?}"
    );
    assert_eq!(
        r.ls.state.core().constraint().map(|c| c.kind),
        Some(ConstraintKind::Confine)
    );
    r.button(BTN_MIDDLE, false, logo());
}
