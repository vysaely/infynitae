//! The next window of an app whose window just closed takes its place (owner, 2026-10-25: Steam
//! opens an updater, then a login window, then its main window, each closing before the next):
//! only when no other window of the app is open; another app, or a second window beside an open
//! one, is placed by the policy as before. A place another window has taken since is not covered.
//! Reopening an app after any key press, button press or touch is the user's act, and opens near
//! the cursor (owner, 2026-11-03).

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
    let (_, login) = open(&mut r, "L", "steam", (400, 200));
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

/// The next window takes the old place moved just enough to show whole (the test VM, 2026-11-01:
/// an editor reopened 263 px past the left edge after the camera moved).
#[test]
fn the_next_window_is_kept_on_screen_at_the_old_place() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let (small, _) = open(&mut r, "U", "steam", (300, 200));
    r.ls.probe.destroy_toplevel(small);
    r.ls.pump();
    r.frame();
    let (_, big) = open(&mut r, "B", "steam", (1200, 700));
    let core = r.ls.state.core();
    let a = core.stage().anchor(big).unwrap();
    let g = core.stage().tree(big).unwrap().geometry;
    let (x, y) = (a.x + i64::from(g.loc.x), a.y + i64::from(g.loc.y));
    // the camera is at the origin of a 1920x1080 screen
    assert!(
        x >= -960 && y >= -540 && x + i64::from(g.size.w) <= 960 && y + i64::from(g.size.h) <= 540,
        "the whole window shows: {x},{y} {:?}",
        g.size
    );
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

/// Closes a window away from the cursor, gives the input, reopens: at the cursor, not the old place.
fn reopened_after(input: impl FnOnce(&mut Rig)) -> ((i64, i64), (i64, i64)) {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let (first, f) = open(&mut r, "A", "mousepad", (300, 200));
    let place = centre(&r, f);
    r.ls.probe.destroy_toplevel(first);
    r.ls.pump();
    r.frame();
    r.motion((-500.0, -250.0));
    input(&mut r);
    let (_, next) = open(&mut r, "B", "mousepad", (300, 200));
    (place, centre(&r, next))
}

/// With no press, the app's own next window keeps the place even though the cursor has moved away
/// (a splash screen turning into the game).
#[test]
fn moving_the_cursor_alone_keeps_the_place() {
    let (place, at) = reopened_after(|_| {});
    assert_eq!(at, place);
}

#[test]
fn a_click_before_reopening_opens_it_near_the_cursor() {
    let (place, at) = reopened_after(|r| {
        r.button(BTN_LEFT, true, none());
        r.button(BTN_LEFT, false, none());
    });
    assert!(
        (at.0 - place.0).abs() > 100 || (at.1 - place.1).abs() > 100,
        "away from {place:?}, at the cursor: {at:?}"
    );
}

#[test]
fn a_key_press_before_reopening_opens_it_near_the_cursor() {
    let (place, at) = reopened_after(|r| {
        r.t += 1000;
        let key = infynitae_core::input::keys::KEY_LEFTMETA;
        let (state, input) = (&mut r.ls.state, &mut r.ls.input);
        infynitae_backend_headless::keyboard_key(state, input, key, true, r.t).expect("press");
        r.t += 1000;
        let (state, input) = (&mut r.ls.state, &mut r.ls.input);
        infynitae_backend_headless::keyboard_key(state, input, key, false, r.t).expect("release");
    });
    assert!(
        (at.0 - place.0).abs() > 100 || (at.1 - place.1).abs() > 100,
        "away from {place:?}, at the cursor: {at:?}"
    );
}

/// Owner, 2026-11-07: Discord's small updater closes and its main window, much bigger, opened
/// centred on the updater's place over the windows beside it. A successor that would reach over
/// a neighbour goes to the nearest free spot instead; it never covers another window.
#[test]
fn a_bigger_next_window_does_not_cover_the_neighbours() {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let (_, browser) = open(&mut r, "B", "firefox", (500, 400));
    // the updater, placed beside the browser
    let (updater, _) = open(&mut r, "U", "discord", (300, 300));
    r.ls.probe.destroy_toplevel(updater);
    r.ls.pump();
    r.frame();
    let (_, main) = open(&mut r, "M", "discord", (1000, 700));
    let core = r.ls.state.core();
    let rect = |id: ElementId| {
        let a = core.stage().anchor(id).unwrap();
        let g = core.stage().tree(id).unwrap().geometry;
        let (x, y) = (a.x + i64::from(g.loc.x), a.y + i64::from(g.loc.y));
        (x, y, x + i64::from(g.size.w), y + i64::from(g.size.h))
    };
    let (b, m) = (rect(browser), rect(main));
    assert!(
        m.2 <= b.0 || b.2 <= m.0 || m.3 <= b.1 || b.3 <= m.1,
        "Discord {m:?} does not cover the browser {b:?}"
    );
}
