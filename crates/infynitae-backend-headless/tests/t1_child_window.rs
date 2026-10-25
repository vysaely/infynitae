//! A child window (`xdg_toplevel.set_parent`: a dialog, an app's second window) opens centred over
//! its parent (owner, 2026-10-24), whatever the placement policy; a window without a parent is
//! placed by the policy (cluster tiling beside the others).

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
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, label, (300, 200), 0);
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
