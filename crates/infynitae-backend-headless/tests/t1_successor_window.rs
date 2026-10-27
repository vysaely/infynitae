//! The next window of an app whose window just closed takes its place (owner, 2026-10-25: Steam
//! opens an updater, then a login window, then its main window, each closing before the next):
//! only when no other window of the app is open; another app, or a second window beside an open
//! one, is placed by the policy as before. A place another window has taken since is not covered.

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

/// A terminal closes; a browser opens alone where it was (the view's centre); the next terminal
/// (Super+Q) is placed beside the browser, not on top of it (ThinkPad, 2026-10-27).
#[test]
fn a_place_another_window_has_taken_since_is_not_covered() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let (term, t) = open(&mut r, "T", "kitty", (300, 200));
    let place = centre(&r, t);
    r.ls.probe.destroy_toplevel(term);
    r.ls.pump();
    r.frame();

    let (_, browser) = open(&mut r, "B", "firefox", (320, 220));
    let (bx, by) = centre(&r, browser);
    assert!(
        (bx - place.0).abs() < 50 && (by - place.1).abs() < 50,
        "alone, the browser is centred too: on the terminal's place"
    );

    let (_, next) = open(&mut r, "N", "kitty", (300, 200));
    let core = r.ls.state.core();
    let rect = |id: ElementId| {
        let a = core.stage().anchor(id).unwrap();
        let g = core.stage().tree(id).unwrap().geometry;
        let (x, y) = (a.x + i64::from(g.loc.x), a.y + i64::from(g.loc.y));
        (x, y, x + i64::from(g.size.w), y + i64::from(g.size.h))
    };
    let (b, n) = (rect(browser), rect(next));
    assert!(
        n.2 <= b.0 || b.2 <= n.0 || n.3 <= b.1 || b.3 <= n.1,
        "the terminal {n:?} does not cover the browser {b:?}"
    );
}
