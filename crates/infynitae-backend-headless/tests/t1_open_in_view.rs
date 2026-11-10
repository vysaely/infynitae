//! New windows open where the user sees them (owner, 2026-11-09): a window the camera does not
//! come to (an app opened it, it is a child, or TELEPORT_ON_OPEN is off) and that the placement
//! put mostly outside the view is brought into it; one mostly in view stays beside the others.

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

/// The usable view `(x0, y0, x1, y1)` of the first output in Canvas pixels (no layers here).
fn view(r: &Rig) -> (i64, i64, i64, i64) {
    let core = r.ls.state.core();
    let o = &core.outputs()[0];
    let camera = core.cameras().camera_for_output(o.id).unwrap();
    let c = core.cameras().center(camera).unwrap();
    let z = core.cameras().zoom(camera).unwrap();
    let (w, h) = o.logical_extent();
    let (cx, cy) = (c.x as f64 / 65536.0, c.y as f64 / 65536.0);
    let (x0, y0) = (cx - w / 2.0 / z, cy - h / 2.0 / z);
    (
        x0.ceil() as i64,
        y0.ceil() as i64,
        (x0 + w / z).floor() as i64,
        (y0 + h / z).floor() as i64,
    )
}

/// The part of `r` inside `v`, as a share of `r`.
fn seen(r: (i64, i64, i64, i64), v: (i64, i64, i64, i64)) -> f64 {
    let w = (r.2.min(v.2) - r.0.max(v.0)).max(0);
    let h = (r.3.min(v.3) - r.1.max(v.1)).max(0);
    (w * h) as f64 / ((r.2 - r.0) * (r.3 - r.1)) as f64
}

fn open(r: &mut Rig, label: &'static str, size: (i32, i32)) -> ElementId {
    let qh = r.ls.qh.clone();
    r.ls.probe.toplevel(&qh, label, size, 0);
    r.ls.pump();
    r.frame();
    *r.ls.state.core().stage().mapped_elements().last().unwrap()
}

fn bring(r: &mut Rig, id: ElementId) {
    infynitae_backend_headless::bring_into_view(&mut r.ls.state, &mut r.ls.input, id)
        .expect("bring into view");
    r.frame();
}

fn cluster() -> Rig {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    r
}

/// The bug seen in the VM: with the view full of windows, a folder picker (a toplevel with no
/// parent, opened by an app) went to a free spot far outside the view, where nobody saw it.
#[test]
fn a_window_placed_outside_a_full_view_is_brought_into_it() {
    let mut r = cluster();
    for l in ["A", "B", "C", "D"] {
        open(&mut r, l, (900, 480));
    }
    let picker = open(&mut r, "Picker", (700, 500));
    let v = view(&r);
    assert!(
        seen(rect(&r, picker), v) < 0.6,
        "the setup: the placement put the picker {:?} mostly outside the view {v:?}",
        rect(&r, picker)
    );
    bring(&mut r, picker);
    let p = rect(&r, picker);
    assert!(
        p.0 >= v.0 && p.1 >= v.1 && p.2 <= v.2 && p.3 <= v.3,
        "the picker {p:?} is outside the view {v:?}"
    );
}

/// A second window beside a centred first one reaches a little past the screen's edge: it stays
/// there, beside the first, rather than being pulled over it.
#[test]
fn a_window_mostly_in_view_stays_beside_the_others() {
    let mut r = cluster();
    let b = open(&mut r, "B", (800, 600));
    let before = rect(&r, b);
    assert!(
        seen(before, view(&r)) >= 0.6,
        "the setup: B {before:?} is mostly in view"
    );
    bring(&mut r, b);
    assert_eq!(rect(&r, b), before, "B stayed where it was");
}

/// A window larger than the view is brought to the view's top left corner (its title bar and
/// controls on the screen).
#[test]
fn a_window_larger_than_the_view_shows_its_top_left() {
    let mut r = cluster();
    let big = open(&mut r, "Big", (3000, 2000));
    bring(&mut r, big);
    let (v, b) = (view(&r), rect(&r, big));
    assert!(
        (0..=32).contains(&(b.0 - v.0)) && (0..=32).contains(&(b.1 - v.1)),
        "the big window {b:?}, the view {v:?}"
    );
}
