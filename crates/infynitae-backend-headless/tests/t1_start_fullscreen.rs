//! A window that starts fullscreen, or as large as the screen, takes the screen it opens on with
//! the keys, and keeps off the other windows on the Canvas (owner, 2026-11-19: "fullscreen apps
//! are still appearing on random places under or over other windows", right as a game starts:
//! Roblox through Mocktail, Steam games through Proton or SDL). Such a game is opened by a
//! launcher long before its window shows, so nothing traced it to the user: it opened without the
//! focus, its fullscreen request a frame later was declined for want of it, and it stayed a window
//! wherever the free space was.

mod common;

use std::time::{Duration, Instant};

use common::input::*;
use infynitae_backend_headless::set_focus;
use infynitae_core::stage::ElementId;

const BTN_LEFT: u32 = 0x110;

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

/// W (800x600 in the middle of the 1920x1080 screen) mapped and focused, the cluster policy on.
fn rig() -> (Rig, ElementId) {
    let mut r = Rig::x8();
    r.ls.state
        .core_mut()
        .set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).expect("focus");
    r.frame();
    (r, w)
}

/// A toplevel of `size` shown with its first commit; its index and element.
fn open(r: &mut Rig, label: &'static str, size: (i32, i32)) -> (usize, ElementId) {
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, label, size, 0);
    r.ls.pump();
    r.frame();
    let id = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    (idx, id)
}

/// A toplevel that asks for fullscreen before its initial commit (how xwayland-satellite shows
/// an X11 game that starts fullscreen), drawn at `size` when its configure comes.
fn open_fullscreen(r: &mut Rig, label: &'static str, size: (i32, i32)) -> ElementId {
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel_uncommitted(&qh, label, size);
    r.ls.probe.surfaces[idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    r.ls.probe.surfaces[idx].wl.commit();
    for _ in 0..4 {
        r.ls.pump();
        r.frame();
    }
    let id = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    assert!(
        r.ls.state.core().fullscreen_of(id).is_some(),
        "the setup: it opened fullscreen"
    );
    id
}

fn ask_fullscreen(r: &mut Rig, idx: usize) {
    r.ls.probe.surfaces[idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    r.ls.pump();
    r.frame();
}

/// A window that asks for fullscreen a frame after it shows, while the window before it still
/// has the focus, gets the screen and the keys (until 2026-11-19 it was declined and stayed a
/// window where it had opened, without the focus).
#[test]
fn a_window_asking_for_fullscreen_as_it_opens_gets_the_screen_and_the_keys() {
    let (mut r, w) = rig();
    let (g, gid) = open(&mut r, "G", (800, 600));
    assert_eq!(r.ls.state.core().focus().target(), Some(w), "the setup");
    ask_fullscreen(&mut r, g);
    let core = r.ls.state.core();
    let screen = r.ls.output();
    assert_eq!(core.fullscreen_of(gid).map(|f| f.output), Some(screen));
    assert_eq!(core.presentation(screen), Some(gid), "it has the screen");
    assert_eq!(core.focus().target(), Some(gid), "it has the keys");
}

/// The window opening last asks only as it opens: once the user clicked, an unfocused window's
/// request is declined again (INV-PRES-1).
#[test]
fn after_a_click_an_unfocused_window_is_declined_again() {
    let (mut r, w) = rig();
    let (g, gid) = open(&mut r, "G", (800, 600));
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    assert_eq!(r.ls.state.core().focus().target(), Some(w), "the setup");
    ask_fullscreen(&mut r, g);
    let core = r.ls.state.core();
    assert!(core.fullscreen_of(gid).is_none());
    assert_eq!(core.focus().target(), Some(w));
}

/// A window that maps fullscreen keeps off the window in view at its place on the Canvas, where
/// it shows when it steps aside (until 2026-11-19 its corner sat at the view's centre, over W).
#[test]
fn a_window_mapping_fullscreen_keeps_off_the_window_in_view() {
    let (mut r, w) = rig();
    let g = open_fullscreen(&mut r, "G", (1920, 1080));
    let (rw, rg) = (rect(&r, w), rect(&r, g));
    assert!(apart(rw, rg), "the fullscreen window {rg:?} covers {rw:?}");
}

/// A window that opens fullscreen or as large as the screen counts as one the user opened
/// (FOCUS_ON_OPEN and TELEPORT_ON_OPEN "user"): it takes the focus and the camera goes to it,
/// whatever started it. An ordinary window an app opened does not.
#[test]
fn a_window_taking_the_screen_counts_as_opened_by_the_user() {
    let (mut r, _) = rig();
    let long_ago = Instant::now() - Duration::from_secs(60);
    let (_, small) = open(&mut r, "S", (800, 600));
    assert!(
        !r.ls.state.opened_by_user(small, long_ago),
        "the control: an ordinary window"
    );
    let (_, big) = open(&mut r, "B", (1920, 1080));
    assert!(
        r.ls.state.opened_by_user(big, long_ago),
        "a window as large as the screen"
    );
    let fs = open_fullscreen(&mut r, "F", (1920, 1080));
    assert!(
        r.ls.state.opened_by_user(fs, long_ago),
        "a window that opened fullscreen"
    );
}

/// A window exactly as large as the screen is not offered the smaller room (the bounds less the
/// 12 px gap): a game that obeyed would no longer fill the screen. One larger still is.
#[test]
fn a_window_as_large_as_the_screen_is_not_offered_the_room() {
    let (mut r, _) = rig();
    let (b, _) = open(&mut r, "B", (1920, 1080));
    let sized: Vec<_> =
        r.ls.probe
            .toplevel_configures
            .iter()
            .filter(|(i, size, _)| *i == b && *size != (0, 0))
            .map(|(_, size, _)| *size)
            .collect();
    assert_eq!(sized, Vec::<(i32, i32)>::new(), "offered a size");
    let (l, _) = open(&mut r, "L", (2000, 1200));
    assert!(
        r.ls.probe
            .toplevel_configures
            .iter()
            .any(|(i, size, _)| *i == l && *size != (0, 0)),
        "the control: a larger window is offered the room"
    );
}
