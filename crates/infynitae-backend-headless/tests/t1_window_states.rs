//! A window's size the compositor chose — Super+Enter's maximize (`SUPER_ENTER_ACTION="maximize"`),
//! a Super+right drag, the room offered to a window that opened too large, the size a fullscreen
//! window returns to — is kept through focus changes and follows the screen (owner, 2026-10-31:
//! "maximizing is lost on window switch", and windows must fit every resolution and rotation).
//!
//! Every window is told it is tiled on all four sides (SQUARE_CORNERS), and apps told so take a
//! configure without a size (0x0) as "back to your own size": GTK 3 (Firefox, Thunar, Mousepad)
//! returns to the size it opened at. So every configure after the compositor asked for a size
//! carries that size (INV-PROTO-1, amended 2026-10-31) until the app draws another size of its
//! own. The oracle is the client's own record of the configures it received.

mod common;

use common::input::*;
use common::{Lockstep, delta_to, output, output_k};
use infynitae_backend_headless::{VirtualOutput, keyboard_key, set_focus};
use infynitae_core::geom::LocalSize;
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::input::keys::{KEY_ENTER, KEY_LEFTMETA};
use infynitae_core::output::Transform;
use infynitae_core::stage::ElementId;
use infynitae_core::{EnterMode, PlacementPolicy};

const LOGO: Modifiers = Modifiers {
    logo: true,
    ctrl: false,
    alt: false,
    shift: false,
};

/// xdg_toplevel state `fullscreen`.
const FULLSCREEN: u32 = 2;

/// The owner's setup: windows beside one another, Super+Enter maximizes.
fn rig(o: VirtualOutput) -> Rig {
    let mut r = Rig::new(Lockstep::new(Some(o)));
    let core = r.ls.state.core_mut();
    core.set_placement_policy(PlacementPolicy::ClusterTiling);
    core.set_enter_mode(EnterMode::Maximize);
    r
}

/// Maps a toplevel of `size`; returns its probe index and element.
fn map(r: &mut Rig, label: &'static str, size: (i32, i32)) -> (usize, ElementId) {
    let qh = r.ls.qh.clone();
    let before = r.ls.state.core().stage().mapped_elements();
    let idx = r.ls.probe.toplevel(&qh, label, size, 0);
    r.ls.pump();
    r.frame();
    r.frame();
    let id = *r
        .ls
        .state
        .core()
        .stage()
        .mapped_elements()
        .iter()
        .find(|e| !before.contains(e))
        .expect("the new window is mapped");
    (idx, id)
}

/// The client draws a frame at `size` (as an app does after the configure that asked for it).
fn draw(r: &mut Rig, idx: usize, size: (i32, i32)) {
    let qh = r.ls.qh.clone();
    r.ls.probe.surfaces[idx].size = size;
    r.ls.probe.draw(&qh, idx);
    r.ls.pump();
    r.frame();
}

fn focus(r: &mut Rig, id: ElementId) {
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(id)).expect("focus");
    r.ls.pump();
    r.frame();
}

/// Holds `mods`, taps `key`, releases `mods`.
fn chord(r: &mut Rig, mods: &[u32], key: u32) {
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    for &k in mods {
        press(k, true);
    }
    press(key, true);
    press(key, false);
    for &k in mods.iter().rev() {
        press(k, false);
    }
    r.frame();
}

/// The pointer over `id`, then Super+Enter: the window is maximized (or restored).
fn super_enter_on(r: &mut Rig, id: ElementId) {
    let d = delta_to(&r.ls, id, (40.0, 40.0));
    r.motion(d);
    chord(r, &[KEY_LEFTMETA], KEY_ENTER);
}

/// The sizes the toplevel `idx` was configured with since configure number `from`.
fn sizes_since(r: &Rig, idx: usize, from: usize) -> Vec<(i32, i32)> {
    r.ls.probe.toplevel_configures[from..]
        .iter()
        .filter(|(i, _, _)| *i == idx)
        .map(|(_, s, _)| *s)
        .collect()
}

fn last_size(r: &Rig, idx: usize) -> Option<(i32, i32)> {
    sizes_since(r, idx, 0).last().copied()
}

/// The usable area of the first output, rounded as Super+Enter rounds it.
fn usable(r: &Rig) -> (i32, i32) {
    let core = r.ls.state.core();
    let (_, _, w, h) = core.usable_area(r.ls.output()).expect("usable area");
    (w.round() as i32, h.round() as i32)
}

/// The owner's case: A is maximized and drawn at the screen's size; the focus goes to B and back
/// to A (a click, Super+1..9, Super+Tab all move the focus the same way). Each of those changes
/// sends A a configure (`activated` changes), and every one of them keeps the maximized size.
/// Before the fix they carried 0x0, and an app told it is tiled went back to its own size.
#[test]
fn a_maximized_window_keeps_its_size_through_focus_changes() {
    let mut r = rig(a_1080());
    let (a, ea) = map(&mut r, "A", (800, 600));
    let (_, eb) = map(&mut r, "B", (800, 600));
    focus(&mut r, ea);
    super_enter_on(&mut r, ea);
    let max = usable(&r);
    assert_eq!(max, (1920, 1080));
    assert_eq!(last_size(&r, a), Some(max), "Super+Enter asks for the view");
    draw(&mut r, a, max);

    let from = r.ls.probe.toplevel_configures.len();
    focus(&mut r, eb);
    focus(&mut r, ea);
    focus(&mut r, eb);
    let seen = sizes_since(&r, a, from);
    assert!(seen.len() >= 3, "each focus change configures A: {seen:?}");
    assert!(
        seen.iter().all(|s| *s == max),
        "every configure keeps the maximized size: {seen:?}"
    );
    // and Super+Enter still knows it is maximized: pressed again it restores 800x600
    focus(&mut r, ea);
    super_enter_on(&mut r, ea);
    assert_eq!(last_size(&r, a), Some((800, 600)), "restored");
    draw(&mut r, a, (800, 600));
    let from = r.ls.probe.toplevel_configures.len();
    focus(&mut r, eb);
    assert_eq!(
        sizes_since(&r, a, from),
        vec![(800, 600)],
        "the restored size is kept too"
    );
}

/// A Super+right drag sizes the window; the size stays through a focus change.
#[test]
fn a_resized_window_keeps_its_size_through_focus_changes() {
    let mut r = rig(a_1080());
    let (a, ea) = map(&mut r, "A", (400, 300));
    let (_, eb) = map(&mut r, "B", (400, 300));
    focus(&mut r, ea);
    // the pointer near A's bottom-right corner: the drag moves the right and bottom sides
    let d = delta_to(&r.ls, ea, (390.0, 290.0));
    r.motion(d);
    r.button(BTN_RIGHT, true, LOGO);
    r.motion((100.0, 50.0));
    draw(&mut r, a, (500, 350));
    r.button(BTN_RIGHT, false, LOGO);
    draw(&mut r, a, (500, 350));
    assert_eq!(last_size(&r, a), Some((500, 350)));
    let from = r.ls.probe.toplevel_configures.len();
    focus(&mut r, eb);
    focus(&mut r, ea);
    let seen = sizes_since(&r, a, from);
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|s| *s == (500, 350)), "{seen:?}");
}

/// A window that opened larger than its room was offered the room; the room stays its size.
#[test]
fn a_window_fitted_to_the_room_keeps_it_through_focus_changes() {
    let mut r = rig(a_1080());
    let (b, eb) = map(&mut r, "B", (800, 600));
    let (a, ea) = map(&mut r, "Discord", (1944, 1124));
    let room = (1920 - 24, 1080 - 24);
    assert_eq!(last_size(&r, a), Some(room), "offered the room");
    draw(&mut r, a, room);
    let from = r.ls.probe.toplevel_configures.len();
    focus(&mut r, eb);
    focus(&mut r, ea);
    let seen = sizes_since(&r, a, from);
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|s| *s == room), "{seen:?}");
    // a window whose size nobody chose is still asked for none
    assert!(
        sizes_since(&r, b, 0).iter().all(|s| *s == (0, 0)),
        "{:?}",
        sizes_since(&r, b, 0)
    );
}

/// An app that draws another size by itself after it drew the one asked of it (it resized its own
/// window) has chosen its size: the compositor no longer holds it to the old one, and a later
/// configure leaves the size to it again (0x0).
#[test]
fn a_window_that_takes_its_own_size_is_let_go() {
    let mut r = rig(a_1080());
    let (a, ea) = map(&mut r, "A", (800, 600));
    let (_, eb) = map(&mut r, "B", (800, 600));
    focus(&mut r, ea);
    super_enter_on(&mut r, ea);
    let max = usable(&r);
    draw(&mut r, a, max);
    assert!(r.ls.state.core().held_size(ea).is_some());
    // a frame at the old size before the app read the configure does not count: here the
    // configure was acknowledged, so this one does
    draw(&mut r, a, (700, 500));
    assert_eq!(r.ls.state.core().held_size(ea), None, "let go");
    let from = r.ls.probe.toplevel_configures.len();
    focus(&mut r, eb);
    assert_eq!(sizes_since(&r, a, from), vec![(0, 0)]);
}

/// A frame drawn at the old size, under the configure acknowledged before the maximize, does not
/// let the window go: it has not answered the maximize yet.
#[test]
fn a_frame_drawn_before_the_maximize_was_read_keeps_the_hold() {
    let mut r = rig(a_1080());
    let (a, ea) = map(&mut r, "A", (800, 600));
    let (_, eb) = map(&mut r, "B", (800, 600));
    focus(&mut r, ea);
    r.ls.probe.hold_acks = true;
    super_enter_on(&mut r, ea);
    // drawn before the client acknowledged the maximize
    draw(&mut r, a, (800, 600));
    let max = usable(&r);
    assert_eq!(
        r.ls.state.core().held_size(ea),
        Some(LocalSize { w: max.0, h: max.1 })
    );
    r.ls.probe.hold_acks = false;
    let from = r.ls.probe.toplevel_configures.len();
    focus(&mut r, eb);
    assert_eq!(sizes_since(&r, a, from), vec![max]);
}

/// One screen setting: a name, its physical mode, scale (×120) and rotation.
struct Screen {
    name: &'static str,
    mode: (u32, u32),
    scale_120: u32,
    transform: Transform,
}

const fn screen(name: &'static str, mode: (u32, u32), scale_120: u32, r90: bool) -> Screen {
    Screen {
        name,
        mode,
        scale_120,
        transform: if r90 {
            Transform::R90
        } else {
            Transform::Normal
        },
    }
}

const SCREENS: [Screen; 10] = [
    screen("1920x1080", (1920, 1080), 120, false),
    screen("1366x768", (1366, 768), 120, false),
    screen("2560x1440", (2560, 1440), 120, false),
    screen("2560x1440@1.5", (2560, 1440), 180, false),
    screen("2560x1440@2", (2560, 1440), 240, false),
    screen("3840x2160@2", (3840, 2160), 240, false),
    screen("1366x768 portrait", (1366, 768), 120, true),
    screen("1920x1080 portrait", (1920, 1080), 120, true),
    screen("1920x1080@1.5", (1920, 1080), 180, false),
    screen("2560x1440@1.5 portrait", (2560, 1440), 180, true),
];

fn virtual_output(s: &Screen) -> VirtualOutput {
    let mut o = output_k("A", s.mode, s.scale_120, 60_000);
    o.transform = s.transform;
    o
}

/// The screen's logical size, rounded down (the fullscreen size, C5.2).
fn logical_floor(r: &Rig) -> (i32, i32) {
    let core = r.ls.state.core();
    let o = core.output(r.ls.output()).expect("output");
    let (w, h) = o.logical_extent();
    (w.floor() as i32, h.floor() as i32)
}

/// The camera's centre on the first output, in whole Canvas px.
fn camera_centre(r: &Rig) -> (i64, i64) {
    let c = r.ls.state.core().cameras().center(0).expect("camera");
    (c.x >> 16, c.y >> 16)
}

/// On every screen of the list, then after a change to every other one while it is open: a
/// maximized window fills the usable area (its configure asks for exactly that size, and it sits
/// in the middle of the view), a fullscreen window is asked for the screen's size, and both keep
/// their sizes through a focus change on the new screen.
#[test]
fn maximized_and_fullscreen_windows_follow_every_screen_change() {
    for from in &SCREENS {
        for to in &SCREENS {
            if from.name == to.name {
                continue;
            }
            let what = format!("{} -> {}", from.name, to.name);
            let mut r = rig(virtual_output(from));
            let (m, em) = map(&mut r, "Maximized", (640, 480));
            let (f, ef) = map(&mut r, "Fullscreen", (640, 480));
            let (_, et) = map(&mut r, "Tiled", (300, 200));

            // the maximized one (first: Super+Enter does nothing over a fullscreen screen)
            focus(&mut r, em);
            super_enter_on(&mut r, em);
            let max = usable(&r);
            assert_eq!(last_size(&r, m), Some(max), "{what}: maximized at first");
            draw(&mut r, m, max);
            let before = r.ls.state.core().stage().anchor(em).expect("anchor");
            let centre = (
                before.x + i64::from(max.0) / 2,
                before.y + i64::from(max.1) / 2,
            );

            // the fullscreen one: granted to the focused window, drawn at the screen's size
            focus(&mut r, ef);
            r.ls.probe.surfaces[f]
                .toplevel
                .as_ref()
                .expect("toplevel")
                .set_fullscreen(None);
            r.ls.pump();
            r.frame();
            let full = logical_floor(&r);
            assert_eq!(last_size(&r, f), Some(full), "{what}: fullscreen at first");
            draw(&mut r, f, full);

            // the screen changes
            let o = r.ls.output();
            r.ls.reconfigure(o, &virtual_output(to));
            r.frame();
            let full = logical_floor(&r);
            let max = usable(&r);
            assert_eq!(last_size(&r, f), Some(full), "{what}: fullscreen follows");
            assert_eq!(last_size(&r, m), Some(max), "{what}: maximized follows");
            draw(&mut r, m, max);
            draw(&mut r, f, full);
            let a = r.ls.state.core().stage().anchor(em).expect("anchor");
            assert_eq!(
                (a.x + i64::from(max.0) / 2, a.y + i64::from(max.1) / 2),
                centre,
                "{what}: it keeps its centre"
            );
            // the camera did not move: the maximized window fills the view it was maximized in
            let (cx, cy) = camera_centre(&r);
            assert!(
                (a.x - (cx - i64::from(max.0) / 2)).abs() <= 2
                    && (a.y - (cy - i64::from(max.1) / 2)).abs() <= 2,
                "{what}: the maximized window {a:?} fills the view around {cx},{cy}"
            );
            // a focus change keeps every size
            let from_n = r.ls.probe.toplevel_configures.len();
            focus(&mut r, et);
            focus(&mut r, em);
            assert!(
                sizes_since(&r, m, from_n).iter().all(|s| *s == max),
                "{what}: maximized through focus"
            );
            assert!(
                sizes_since(&r, f, from_n).iter().all(|s| *s == full),
                "{what}: fullscreen through focus"
            );
            assert!(
                r.ls.state.core().fullscreen_of(ef).is_some(),
                "{what}: still fullscreen"
            );
        }
    }
}

/// A window maximized on a screen that is unplugged fills the screen that is left.
#[test]
fn a_maximized_window_fills_the_screen_left_after_an_unplug() {
    let mut r = rig(output_k("A", (2560, 1440), 120, 60_000));
    let (m, em) = map(&mut r, "Maximized", (640, 480));
    focus(&mut r, em);
    super_enter_on(&mut r, em);
    assert_eq!(last_size(&r, m), Some((2560, 1440)));
    draw(&mut r, m, (2560, 1440));
    let laptop = r.ls.connect(&output_k("B", (1366, 768), 120, 60_000));
    let first = r.ls.output();
    r.ls.disconnect(first);
    r.ls.output = Some(laptop);
    r.frame();
    assert_eq!(last_size(&r, m), Some((1366, 768)));
}

/// The configure a fullscreen window gets carries the `fullscreen` state and the screen's size;
/// the configures above are checked by size, this one by its state too.
#[test]
fn a_fullscreen_window_is_told_so_after_a_rotation() {
    let mut r = rig(a_1080());
    let (f, ef) = map(&mut r, "F", (640, 480));
    focus(&mut r, ef);
    r.ls.probe.surfaces[f]
        .toplevel
        .as_ref()
        .expect("toplevel")
        .set_fullscreen(None);
    r.ls.pump();
    r.frame();
    let o = r.ls.output();
    let mut portrait = output((1920, 1080), 120);
    portrait.transform = Transform::R90;
    r.ls.reconfigure(o, &portrait);
    r.frame();
    let last =
        r.ls.probe
            .toplevel_configures
            .iter()
            .rev()
            .find(|(i, _, _)| *i == f)
            .cloned()
            .expect("a configure");
    assert_eq!(last.1, (1080, 1920));
    assert!(last.2.contains(&FULLSCREEN), "{:?}", last.2);
}
