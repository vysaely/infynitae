//! Whatever places it, a new window never covers another app's window (owner, 2026-11-07: "this
//! should cover any app at compositor level"): the rule sits after every placement path.

mod common;

use common::input::*;
use infynitae_core::stage::ElementId;

fn rect(r: &Rig, id: ElementId) -> (i64, i64, i64, i64) {
    let core = r.ls.state.core();
    let a = core.stage().anchor(id).unwrap();
    let g = core.stage().tree(id).unwrap().geometry;
    (
        a.x,
        a.y,
        a.x + i64::from(g.size.w),
        a.y + i64::from(g.size.h),
    )
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

/// A big child of a small window centred over it would reach over the window beside: under the
/// cluster policy it opens free of it (the rule after every path, here the child's).
#[test]
fn a_child_bigger_than_its_parent_keeps_off_the_neighbour() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let ws = r.ls.state.core().stage().tree(w).unwrap().geometry.size;
    let s = open(&mut r, "S", (300, 200));
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let qh = r.ls.qh.clone();
    let idx =
        r.ls.probe
            .toplevel(&qh, "D", (ws.w * 3 / 2, ws.h * 3 / 2), 0);
    let parent = r.ls.probe.surfaces[w_idx].toplevel.clone().unwrap();
    r.ls.probe.surfaces[idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_parent(Some(&parent));
    r.ls.pump();
    r.frame();
    let d = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let (rs, rd) = (rect(&r, s), rect(&r, d));
    assert!(apart(rs, rd), "the child {rd:?} covers {rs:?}");
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

/// A window that asks to be maximized before it is shown (Discord reopening maximized) fills a
/// screen's room in free space, not the screen in view over the window there (owner, 2026-11-07).
#[test]
fn a_window_opening_maximized_does_not_cover_the_window_in_view() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let qh = r.ls.qh.clone();
    let x = r.ls.probe.toplevel_uncommitted(&qh, "X", (240, 292));
    r.ls.probe.surfaces[x]
        .toplevel
        .as_ref()
        .unwrap()
        .set_maximized();
    r.ls.probe.surfaces[x].wl.commit();
    for _ in 0..6 {
        r.ls.pump();
        r.frame();
    }
    let id = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    assert_ne!(id, w);
    let (rw, rx) = (rect(&r, w), rect(&r, id));
    assert!(rx.2 - rx.0 >= 1000, "it was maximized: {rx:?}");
    assert!(apart(rw, rx), "the maximized window {rx:?} covers {rw:?}");
}
