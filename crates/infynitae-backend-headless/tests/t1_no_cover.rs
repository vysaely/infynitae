//! Whatever places it, a new window never covers another app's window (owner, 2026-11-07: "this
//! should cover any app at compositor level"): the rule sits after every placement path.

mod common;

use common::input::*;
use infynitae_core::stage::ElementId;

fn rect(r: &Rig, id: ElementId) -> (i64, i64, i64, i64) {
    let core = r.ls.state.core();
    let a = core.stage().anchor(id).unwrap();
    let g = core.stage().tree(id).unwrap().geometry;
    (a.x, a.y, a.x + i64::from(g.size.w), a.y + i64::from(g.size.h))
}

fn apart(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.2 <= b.0 || b.2 <= a.0 || a.3 <= b.1 || b.3 <= a.1
}

fn open(r: &mut Rig, label: &'static str, size: (i32, i32)) -> ElementId {
    let qh = r.ls.qh.clone();
    r.ls.probe.toplevel(&qh, label, size, 0);
    r.ls.pump();
    r.frame();
    *r.ls.state.core().stage().mapped_elements().last().unwrap()
}

/// The `center` policy centres a new window in the view, where W already is: it opens beside W.
#[test]
fn a_centred_window_does_not_cover_the_one_already_there() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::CentredInView);
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let n = open(&mut r, "N", (400, 300));
    let (rw, rn) = (rect(&r, w), rect(&r, n));
    assert!(apart(rw, rn), "the new window {rn:?} covers {rw:?}");
}

/// Under the default cluster policy, a stream of new windows never overlaps.
#[test]
fn many_new_windows_never_overlap() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let labels = ["A", "B", "C", "D", "E", "F"];
    for (i, l) in labels.iter().enumerate() {
        open(&mut r, l, (300 + 90 * i as i32, 200 + 60 * i as i32));
    }
    let all = r.ls.state.core().stage().mapped_elements();
    for (i, &a) in all.iter().enumerate() {
        for &b in &all[i + 1..] {
            let (ra, rb) = (rect(&r, a), rect(&r, b));
            assert!(apart(ra, rb), "{ra:?} and {rb:?} overlap");
        }
    }
}
