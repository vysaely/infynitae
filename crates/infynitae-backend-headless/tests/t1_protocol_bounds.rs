//! INV-PROTO-1 as amended 2026-10-27 (owner: windows opened in the ThinkPad's portrait mode kept
//! their landscape width), on the production path: a toplevel is told the room it has,
//! `xdg_toplevel.configure_bounds` of the first screen's usable area at its current rotation and
//! scale, less the 12 px tiling gap on each side. It comes with the initial configure and again with
//! a configure after that room changed, never as `(0, 0)`. The oracle is the client's own record of
//! the events it received.

mod common;

use std::time::Duration;

use common::{Lockstep, output, output_k};
use infynitae_backend_headless::set_focus;

fn adv(ls: &mut Lockstep) {
    ls.pump();
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
}

/// Every `configure_bounds` the toplevel `idx` has received, in order.
fn bounds_of(ls: &Lockstep, idx: usize) -> Vec<(i32, i32)> {
    ls.probe
        .configure_bounds
        .iter()
        .filter(|(i, _)| *i == idx)
        .map(|(_, b)| *b)
        .collect()
}

/// The bounds a client sees: the first screen's room with its initial configure, nothing more for
/// focus or a maximize request, the new room after the screen changed size and scale, and its own
/// again for a second window and after an unmap and re-show.
#[test]
fn configure_bounds_is_the_screens_room_and_follows_it() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (400, 300), 0);
    adv(&mut ls);
    let first = (1920 - 24, 1080 - 24);
    assert_eq!(bounds_of(&ls, w), vec![first], "with the initial configure");
    let id = ls.state.elements().next().expect("element");

    set_focus(&mut ls.state, &mut ls.input, Some(id)).expect("focus");
    adv(&mut ls);
    ls.probe.surfaces[w]
        .toplevel
        .as_ref()
        .expect("toplevel")
        .set_maximized();
    adv(&mut ls);
    assert_eq!(bounds_of(&ls, w), vec![first], "the room did not change");

    // 2560 x 1440 at scale 1.5: 1706.67 x 960 logical px
    let o = ls.output();
    ls.reconfigure(o, &output_k("A", (2560, 1440), 180, 60_000));
    adv(&mut ls);
    // a configure after the change carries the new room
    set_focus(&mut ls.state, &mut ls.input, None).expect("unfocus");
    adv(&mut ls);
    let seen = bounds_of(&ls, w);
    assert_eq!(seen.last(), Some(&(1682, 936)), "the new room: {seen:?}");
    assert!(seen.iter().all(|b| *b != (0, 0)), "never (0, 0): {seen:?}");

    // a second toplevel is told the room with its own initial configure
    let x = ls.probe.toplevel(&qh, "X", (300, 200), 0);
    adv(&mut ls);
    assert_eq!(bounds_of(&ls, x), vec![(1682, 936)]);
}

/// The ThinkPad's case: its 1366 x 768 panel turned to portrait. The room is the rotated one,
/// 768 wide, so an app opening at its own size fits the screen.
#[test]
fn a_portrait_screen_gives_portrait_room() {
    let mut portrait = output((1366, 768), 120);
    portrait.transform = infynitae_core::output::Transform::R90;
    let mut ls = Lockstep::new(Some(portrait));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (400, 300), 0);
    adv(&mut ls);
    assert_eq!(bounds_of(&ls, w), vec![(768 - 24, 1366 - 24)]);
}

/// The sizes the toplevel `idx` was configured with, in order ((0, 0): its own choice).
fn sizes_of(ls: &Lockstep, idx: usize) -> Vec<(i32, i32)> {
    ls.probe
        .toplevel_configures
        .iter()
        .filter(|(i, _, _)| *i == idx)
        .map(|(_, s, _)| *s)
        .collect()
}

/// Discord's case: Electron ignores `configure_bounds` and opens at the size it saved, larger than
/// the screen. The window is offered the room once it maps; one that fits keeps its own size, and a
/// window too large on one side only keeps the other.
#[test]
fn a_window_opening_larger_than_its_room_is_offered_the_room() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let room = (1920 - 24, 1080 - 24);

    let big = ls.probe.toplevel(&qh, "Discord", (1944, 1124), 0);
    adv(&mut ls);
    adv(&mut ls);
    let sizes = sizes_of(&ls, big);
    assert_eq!(
        sizes.first(),
        Some(&(0, 0)),
        "its own size first: {sizes:?}"
    );
    assert_eq!(sizes.last(), Some(&room), "then the room: {sizes:?}");

    let wide = ls.probe.toplevel(&qh, "Wide", (2400, 600), 0);
    adv(&mut ls);
    adv(&mut ls);
    assert_eq!(sizes_of(&ls, wide).last(), Some(&(room.0, 600)));

    let fits = ls.probe.toplevel(&qh, "Fits", (800, 600), 0);
    adv(&mut ls);
    adv(&mut ls);
    assert_eq!(
        sizes_of(&ls, fits),
        vec![(0, 0)],
        "a window that fits is left alone"
    );
}

/// The VM's case (2026-10-31): a terminal is open in the middle of the screen when a window opens
/// too large. Fitted to the room, it has no place beside the terminal that is on the screen: it
/// takes the middle of the view, and the terminal is pushed aside on the canvas, uncovered.
#[test]
fn a_fitted_window_takes_the_view_and_pushes_the_others_aside() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let room = (1920 - 24, 1080 - 24);
    ls.probe.toplevel(&qh, "Terminal", (800, 600), 0);
    adv(&mut ls);
    adv(&mut ls);
    let terminal = *ls
        .state
        .core()
        .stage()
        .mapped_elements()
        .first()
        .expect("mapped");
    // the pointer toward the bottom right, as in the VM: the nearest free place is then below the
    // terminal, off the screen
    let d = (500.0, 400.0);
    infynitae_backend_headless::pointer_motion(&mut ls.state, &mut ls.input, d, d, 1).unwrap();
    adv(&mut ls);

    ls.probe.toplevel(&qh, "XTerm", (2404, 1954), 0);
    adv(&mut ls);
    adv(&mut ls);
    let core = ls.state.core();
    let big = *core
        .stage()
        .mapped_elements()
        .iter()
        .find(|e| **e != terminal)
        .expect("the second window");
    let a = core.stage().anchor(big).expect("anchor");
    let c = core.cameras().center(0).expect("camera");
    let (cx, cy) = (c.x >> 16, c.y >> 16);
    let (w, h) = (i64::from(room.0), i64::from(room.1));
    assert!(
        a.x >= cx - 960 && a.x + w <= cx + 960 && a.y >= cy - 540 && a.y + h <= cy + 540,
        "the fitted window {a:?} is on the screen around {cx},{cy}"
    );
    let t = core.stage().anchor(terminal).expect("terminal anchor");
    let apart = t.x + 800 <= a.x || t.x >= a.x + w || t.y + 600 <= a.y || t.y >= a.y + h;
    assert!(
        apart,
        "the terminal {t:?} is pushed off the fitted window {a:?}"
    );
}

/// The owner's case (2026-10-31): a window going fullscreen moved the window beside it, and after
/// the fullscreen ended the two overlapped. A fullscreen window is shown over the whole screen
/// (F-PRESENT-1), not at its place on the canvas: nothing on the canvas makes room for it, and it
/// comes back to its own place beside the other.
#[test]
fn a_fullscreen_window_pushes_no_other_window() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    // the default placement (the owner's: windows beside one another)
    ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let qh = ls.qh.clone();
    let a = ls.probe.toplevel(&qh, "A", (800, 600), 0);
    adv(&mut ls);
    adv(&mut ls);
    ls.probe.toplevel(&qh, "B", (800, 600), 0);
    adv(&mut ls);
    adv(&mut ls);
    let ids = ls.state.core().stage().mapped_elements();
    let (ea, eb) = (ids[0], ids[1]);
    let anchor = |ls: &Lockstep, e| ls.state.core().stage().anchor(e).expect("anchor");
    let (a0, b0) = (anchor(&ls, ea), anchor(&ls, eb));

    // A, focused (only the focused window is granted fullscreen), goes fullscreen and draws at
    // the screen's size, as an app does
    infynitae_backend_headless::set_focus(&mut ls.state, &mut ls.input, Some(ea)).expect("focus");
    adv(&mut ls);
    ls.probe.surfaces[a]
        .toplevel
        .as_ref()
        .expect("toplevel")
        .set_fullscreen(None);
    adv(&mut ls);
    let last = ls
        .probe
        .toplevel_configures
        .iter()
        .rev()
        .find(|(i, _, _)| *i == a)
        .cloned();
    assert!(
        last.as_ref().is_some_and(|(_, _, st)| st.contains(&2)),
        "A was granted fullscreen: {last:?}"
    );
    ls.probe.surfaces[a].size = (1920, 1080);
    ls.probe.draw(&qh, a);
    adv(&mut ls);
    adv(&mut ls);
    assert_eq!(
        anchor(&ls, eb),
        b0,
        "B stays where it was while A is fullscreen"
    );

    // and back: A at its own place, the two apart
    ls.probe.surfaces[a]
        .toplevel
        .as_ref()
        .expect("toplevel")
        .unset_fullscreen();
    adv(&mut ls);
    ls.probe.surfaces[a].size = (800, 600);
    ls.probe.draw(&qh, a);
    adv(&mut ls);
    adv(&mut ls);
    let (a1, b1) = (anchor(&ls, ea), anchor(&ls, eb));
    assert_eq!((a1, b1), (a0, b0), "both where they were before");
    let apart =
        a1.x + 800 <= b1.x || b1.x + 800 <= a1.x || a1.y + 600 <= b1.y || b1.y + 600 <= a1.y;
    assert!(apart, "A {a1:?} and B {b1:?} do not overlap");
}
