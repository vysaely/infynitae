//! The next window of an app whose window just closed takes its place (owner, 2026-10-25: Steam
//! opens an updater, then a login window, then its main window, each closing before the next):
//! only when no other window of the app is open; another app, or a second window beside an open
//! one, is placed by the policy as before.

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

/// Maps a window of `app` with a `size` content, returning (probe index, element).
fn open(r: &mut Rig, label: &'static str, app: &str, size: (i32, i32)) -> (usize, ElementId) {
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, label, size, 0);
    r.ls.probe.surfaces[idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_app_id(app.to_string());
    r.ls.pump();
    r.frame();
    (
        idx,
        *r.ls.state.core().stage().mapped_elements().last().unwrap(),
    )
}

#[test]
fn an_apps_next_window_takes_the_place_of_the_one_that_closed() {
    let mut r = Rig::x8();
    // Cluster tiling: a window without a remembered position is placed beside existing windows.
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    // the updater, placed by cluster tiling beside W
    let (updater, u) = open(&mut r, "U", "steam", (300, 200));
    let place = centre(&r, u);
    r.ls.probe.destroy_toplevel(updater);
    r.ls.pump();
    r.frame();

    // the login window, of another size: centred where the updater was
    let (_, login) = open(&mut r, "L", "steam", (400, 300));
    assert_eq!(
        centre(&r, login),
        place,
        "it takes the closed window's place"
    );

    // a second steam window while the login is open: placed by the policy, not on top of it
    let (_, main) = open(&mut r, "M", "steam", (500, 400));
    assert_ne!(centre(&r, main), place);

    // another app never takes steam's place
    let (_, other) = open(&mut r, "O", "firefox", (300, 200));
    assert_ne!(centre(&r, other), place);
}
