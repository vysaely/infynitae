//! C12.F17 Q3 (owner decision 2026-10-19, "exactly one") T1, on the production path (core +
//! adapter + headless backend + a real client): the end of a compositor interaction and an output
//! relocation send the batch's one true-position event to a kept confinement, as a fit or restore
//! does (C12.F17 K4, `t1_fit_constraints`); a kept lock still receives no absolute motion
//! (INV-INPUT-5, zwp_pointer_constraints_v1).
//!
//! - Pan release: W focused and constrained (x8 setup), a Logo+middle pan moves the camera under a
//!   pointer that stays where it is on the output; at the release the reconciliation keeps the
//!   constraint, then a kept confinement gets exactly one `motion` at the pointer's new
//!   surface-local position (x8's (630, 520) moved by the pan to (627, 518)), a kept lock none.
//! - Relocation: outputs A and B side by side; camera A is panned so W reaches A's right edge,
//!   camera B (persisted) shows W from B-local x = 0. The pointer on B over W, W confined; B is
//!   unplugged, the pointer is relocated onto A (F-RELOCATE-1) still over W, the confinement is
//!   kept and receives exactly one `motion`.

mod common;

use common::input::*;
use common::{Lockstep, hit_under_pointer, output_k};
use infynitae_backend_headless::set_focus;
use infynitae_core::geom::CanvasPos;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::pointer::{ConstraintKind, PointerState};
use smithay::wayland::pointer_constraints::with_pointer_constraint;

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

fn kind(r: &Rig) -> Option<ConstraintKind> {
    r.ls.state.core().constraint().map(|c| c.kind)
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

fn near(a: (f64, f64), b: (f64, f64), what: &str) {
    // wl_fixed carries 1/256 px; Core's hit local is on the Q16 lattice
    assert!(
        (a.0 - b.0).abs() <= 1.0 / 256.0 + 1e-9 && (a.1 - b.1).abs() <= 1.0 / 256.0 + 1e-9,
        "{what}: delivered {a:?}, expected {b:?}"
    );
}

/// The root surface-local position of the pointer: Core's hit in W's window geometry plus W's
/// 20 px shadow (the x8 W).
fn surface_local(r: &mut Rig) -> (f64, f64) {
    let (_, (x, y)) = hit_under_pointer(&mut r.ls).expect("W under the pointer");
    (x + 20.0, y + 20.0)
}

fn count(lines: &[&str], what: &str) -> usize {
    lines.iter().filter(|l| **l == what).count()
}

fn focus_w(r: &mut Rig) {
    let w = r.ls.state.elements().next().expect("W");
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    r.ls.pump();
    r.frame();
}

/// x8 with W focused and `lock` (else a confinement to the whole surface) active in Core and
/// Smithay; a Logo+middle pan by (3, 2); returns the client's lines from the release on.
fn pan_release_under(lock: bool) -> (Rig, Vec<String>) {
    let mut r = Rig::x8();
    focus_w(&mut r);
    let qh = r.ls.qh.clone();
    if lock {
        r.ls.probe.lock_pointer(&qh, 0, false);
    } else {
        r.ls.probe.confine_pointer(&qh, 0);
    }
    r.ls.pump();
    r.frame();
    let want = if lock {
        ConstraintKind::Lock
    } else {
        ConstraintKind::Confine
    };
    assert_eq!(kind(&r), Some(want), "constraint before the pan");
    assert!(smithay_active(&r), "Smithay before the pan");
    let camera = r.camera();
    r.button(BTN_MIDDLE, true, logo());
    r.motion((3.0, 2.0));
    assert_ne!(r.camera(), camera, "the pan moved the camera");
    let m = r.mark();
    r.button(BTN_MIDDLE, false, logo());
    let lines = r.since(m).iter().map(|s| s.to_string()).collect();
    assert_eq!(kind(&r), Some(want), "constraint lost at the release");
    assert!(
        smithay_active(&r),
        "Smithay's constraint ended at the release"
    );
    (r, lines)
}

/// M10 + C12.F17 Q3: the release reconciles, keeps the confinement, and sends exactly one
/// true-position event at the pointer's new W-local position; no `unconfined`.
#[test]
fn a_pan_release_sends_one_true_position_event_to_a_kept_confinement() {
    let (mut r, lines) = pan_release_under(false);
    let lines: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let delivered = motions(&lines);
    assert_eq!(delivered.len(), 1, "one true-position event: {lines:?}");
    assert_eq!(count(&lines, "unconfined"), 0, "{lines:?}");
    // x8's W-local (630, 520) with the camera panned by (3, 2) at zoom 1
    near(surface_local(&mut r), (627.0, 518.0), "the oracle");
    near(delivered[0], (627.0, 518.0), "pan release");
}

/// INV-INPUT-5: the release keeps the lock and sends it no absolute motion; no `unlocked`.
#[test]
fn a_pan_release_sends_no_absolute_motion_to_a_kept_lock() {
    let (_, lines) = pan_release_under(true);
    let lines: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    assert!(
        motions(&lines).is_empty(),
        "absolute motion to a locked surface: {lines:?}"
    );
    assert_eq!(count(&lines, "unlocked"), 0, "{lines:?}");
}

/// M10 + C12.F17 Q3: B unplugged under a confined pointer over W; the pointer is relocated onto A
/// still over W, the confinement is kept (Core and Smithay, no `unconfined`) and receives exactly
/// one true-position event at the relocated W-local position.
#[test]
fn a_relocation_onto_a_kept_confinement_sends_one_true_position_event() {
    let a = output_k("A", (1920, 1080), 120, 60_000);
    let b = output_k("B", (1920, 1080), 120, 60_000);
    // camera B centred at Canvas (560, 0): W's window geometry, Canvas x [−400, 400), starts at
    // B-local x = 0
    let persisted = PersistedCameras::from_entries(vec![(
        infynitae_backend_headless::VirtualOutput::key("B"),
        CanvasPos { x: 560 << 16, y: 0 },
        1.0,
    )])
    .expect("persisted cameras");
    let mut r = Rig::new(Lockstep::with(persisted, &[a, b]));
    r.map_w();
    let (oa, ob) = {
        let o = r.ls.state.core().outputs();
        (o[0].id, o[1].id)
    };
    assert_eq!(
        r.anchor().map(|p| (p.x, p.y)),
        Some((-400, -300)),
        "W centred on camera A"
    );
    // pan camera A to centre (−660, 0): W's right part, Canvas x [260, 400), at A-local
    // [1880, 2020) — W reaches A's right edge
    r.button(BTN_MIDDLE, true, logo());
    r.motion((660.0, 0.0));
    r.button(BTN_MIDDLE, false, logo());
    assert_eq!(r.camera(), pos(-660, 0));
    let located = |r: &Rig| match r.ls.state.core().pointer() {
        PointerState::Located { output, p } => (output, p),
        PointerState::NoOutput => panic!("no output"),
    };
    let (o, p) = located(&r);
    assert_eq!(o, oa);
    // onto B at B-local x = 50 (Canvas x −350, over W)
    r.motion((1920.0 - p.0 + 50.0, 0.0));
    let (o, p) = located(&r);
    assert_eq!((o, p.0), (ob, 50.0), "the pointer on B");
    focus_w(&mut r);
    let qh = r.ls.qh.clone();
    r.ls.probe.confine_pointer(&qh, 0);
    r.ls.pump();
    r.frame();
    assert_eq!(kind(&r), Some(ConstraintKind::Confine), "confined on B");
    assert!(smithay_active(&r));

    let m = r.mark();
    r.ls.disconnect(ob);
    let lines: Vec<String> = r.since(m).iter().map(|s| s.to_string()).collect();
    let lines: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let (o, _) = located(&r);
    assert_eq!(o, oa, "relocated onto A");
    let local = surface_local(&mut r);
    assert_eq!(kind(&r), Some(ConstraintKind::Confine), "confinement lost");
    assert!(smithay_active(&r), "Smithay's confinement ended");
    assert_eq!(count(&lines, "unconfined"), 0, "{lines:?}");
    let delivered = motions(&lines);
    assert_eq!(delivered.len(), 1, "one true-position event: {lines:?}");
    near(delivered[0], local, "relocation");
}
