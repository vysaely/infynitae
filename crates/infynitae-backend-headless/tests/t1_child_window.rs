//! A child window (`xdg_toplevel.set_parent`: a dialog, an app's second window) opens centred over
//! its parent (owner, 2026-10-24), whatever the placement policy and its size, and may cover it
//! (owner, 2026-11-03); a window without a parent is placed by the policy (cluster tiling beside
//! the others).

mod common;

use common::input::*;
use infynitae_core::stage::ElementId;

/// The centre of an element's window geometry in Canvas px.
fn centre(r: &Rig, id: ElementId) -> (i64, i64) {
    let core = r.ls.state.core();
    let a = core.stage().anchor(id).unwrap();
    let g = core.stage().tree(id).unwrap().geometry;
    (
        a.x + i64::from(g.loc.x) + i64::from(g.size.w) / 2,
        a.y + i64::from(g.loc.y) + i64::from(g.size.h) / 2,
    )
}

/// Maps a 300×200 window, a child of the probe's window `parent` when given.
fn open(r: &mut Rig, label: &'static str, parent: Option<usize>) -> ElementId {
    open_sized(r, label, parent, (300, 200))
}

fn open_sized(
    r: &mut Rig,
    label: &'static str,
    parent: Option<usize>,
    size: (i32, i32),
) -> ElementId {
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, label, size, 0);
    if let Some(p) = parent {
        let parent_tl = r.ls.probe.surfaces[p].toplevel.clone().unwrap();
        r.ls.probe.surfaces[idx]
            .toplevel
            .as_ref()
            .unwrap()
            .set_parent(Some(&parent_tl));
    }
    r.ls.pump();
    r.frame();
    *r.ls.state.core().stage().mapped_elements().last().unwrap()
}

#[test]
fn a_child_window_opens_centred_over_its_parent() {
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();

    let d = open(&mut r, "D", Some(w_idx));
    assert_eq!(r.ls.state.core().toplevel_parent(d), Some(w));
    assert_eq!(
        centre(&r, d),
        centre(&r, w),
        "the dialog is centred over its parent"
    );

    // control: without a parent, cluster tiling puts it beside the others
    let x = open(&mut r, "X", None);
    assert_eq!(r.ls.state.core().toplevel_parent(x), None);
    assert_ne!(
        centre(&r, x),
        centre(&r, w),
        "a window without a parent is placed by the policy"
    );
}

/// A dialog larger than its parent (GTK's save dialog, 1096x903 over a 640x517 editor) stays over
/// its parent, not beside it where the screen ends.
#[test]
fn a_child_larger_than_its_parent_opens_centred_over_it() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let ws = r.ls.state.core().stage().tree(w).unwrap().geometry.size;
    let d = open_sized(&mut r, "D", Some(w_idx), (ws.w * 2, ws.h * 2));
    assert_eq!(r.ls.state.core().toplevel_parent(d), Some(w));
    let (cd, cw) = (centre(&r, d), centre(&r, w));
    assert!(
        (cd.0 - cw.0).abs() < i64::from(ws.w) && (cd.1 - cw.1).abs() < i64::from(ws.h),
        "the dialog {cd:?} is over its parent {cw:?}"
    );
}

/// A child nearly its parent's size (Discord's pop-out, 1913×1043 over 1920×1080) takes its
/// parent's place like any other child.
#[test]
fn a_child_nearly_its_parents_size_takes_its_place() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let ws = r.ls.state.core().stage().tree(w).unwrap().geometry.size;
    let p = open_sized(&mut r, "P", Some(w_idx), (ws.w - 7, ws.h - 7));
    assert_eq!(r.ls.state.core().toplevel_parent(p), Some(w));
    let (cp, cw) = (centre(&r, p), centre(&r, w));
    assert!(
        (cp.0 - cw.0).abs() <= 4 && (cp.1 - cw.1).abs() <= 4,
        "the pop-out {cp:?} is centred over its parent {cw:?}"
    );
}

/// A big child of a small window (Discord's main window, a child of its small updater) never covers
/// another app's window beside the parent: it opens at the nearest spot free of them (owner,
/// 2026-11-07).
#[test]
fn a_big_child_does_not_cover_another_apps_window() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let ws = r.ls.state.core().stage().tree(w).unwrap().geometry.size;
    let s = open(&mut r, "S", None); // another app's window, placed beside W
    let d = open_sized(&mut r, "D", Some(w_idx), (ws.w * 3, ws.h * 3));
    assert_eq!(r.ls.state.core().toplevel_parent(d), Some(w));
    let rect = |id: ElementId| {
        let core = r.ls.state.core();
        let a = core.stage().anchor(id).unwrap();
        let g = core.stage().tree(id).unwrap().geometry;
        let (x, y) = (a.x + i64::from(g.loc.x), a.y + i64::from(g.loc.y));
        (x, y, x + i64::from(g.size.w), y + i64::from(g.size.h))
    };
    let (rs, rd) = (rect(s), rect(d));
    assert!(
        rd.2 <= rs.0 || rs.2 <= rd.0 || rd.3 <= rs.1 || rs.3 <= rd.1,
        "the child {rd:?} covers the other window {rs:?}"
    );
}
