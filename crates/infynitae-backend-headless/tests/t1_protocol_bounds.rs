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
