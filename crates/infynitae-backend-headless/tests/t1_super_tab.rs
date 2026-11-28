//! Super+Tab through the real key path, as Alt+Tab on Windows with the canvas as the preview
//! (owner, 2026-11-28): a tap goes to the window used before; held, each Tab goes one further in
//! the order of use, focusing the window and centring the camera on it, Shift+Tab one back; the
//! order of use changes only when Super is let go, and Escape goes back to where it started. A
//! window that asks for attention is the first step. The client sees each change as a keyboard
//! enter.

mod common;

use common::input::*;
use infynitae_backend_headless::{keyboard_key, set_focus};
use infynitae_core::input::keys::{KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_TAB};
use infynitae_core::stage::{ElementId, ElementRef};
use infynitae_core::{ShellCommand, ShellStream};

/// Maps a 300×200 window and returns its probe index and element.
fn open(r: &mut Rig, label: &'static str) -> (usize, ElementId) {
    let qh = r.ls.qh.clone();
    let i = r.ls.probe.toplevel(&qh, label, (300, 200), 0);
    r.ls.pump();
    r.frame();
    (
        i,
        *r.ls.state.core().stage().mapped_elements().last().unwrap(),
    )
}

fn key(r: &mut Rig, key: u32, pressed: bool) {
    r.t += 1000;
    keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
    r.ls.pump();
}

/// Tab pressed and let go, Super held.
fn tab(r: &mut Rig) {
    key(r, KEY_TAB, true);
    key(r, KEY_TAB, false);
    r.frame();
}

/// Super down, Tab, Super up.
fn tap(r: &mut Rig) {
    key(r, KEY_LEFTMETA, true);
    tab(r);
    key(r, KEY_LEFTMETA, false);
    r.frame();
}

fn active(r: &Rig) -> Option<&str> {
    r.ls.probe.kb.active.as_deref()
}

fn history(r: &Rig) -> Vec<ElementId> {
    r.ls.state.core().focus().history().to_vec()
}

/// W, X, Y mapped and used in that order, the camera following the cycle (the shipped
/// FOCUS_CYCLE_NAVIGATE).
fn three() -> (Rig, [(usize, ElementId); 3]) {
    let mut r = Rig::x8();
    r.ls.state.core_mut().set_focus_cycle_navigate(true);
    let w = (
        0,
        *r.ls.state.core().stage().mapped_elements().last().unwrap(),
    );
    let x = open(&mut r, "X");
    let y = open(&mut r, "Y");
    for (_, e) in [w, x, y] {
        set_focus(&mut r.ls.state, &mut r.ls.input, Some(e)).unwrap();
        r.ls.pump();
    }
    assert_eq!(active(&r), Some("Y"));
    (r, [w, x, y])
}

/// Taps flip between the last two; held, three Tabs visit X, W and Y again, the order of use
/// unchanged until Super is let go; then the chosen one is first and the others keep their order.
#[test]
fn super_tab_held_steps_in_the_order_of_use_and_commits_on_release() {
    let (mut r, [(_, w), (_, x), (_, y)]) = three();

    tap(&mut r);
    assert_eq!(active(&r), Some("X"), "a tap: the window used before");
    tap(&mut r);
    assert_eq!(active(&r), Some("Y"), "Tab, Tab returns");
    assert_eq!(history(&r), [y, x, w]);

    key(&mut r, KEY_LEFTMETA, true);
    for expected in ["X", "W", "Y", "X", "W"] {
        tab(&mut r);
        assert_eq!(active(&r), Some(expected));
        assert_eq!(
            history(&r),
            [y, x, w],
            "the order changed while Super was held"
        );
    }
    key(&mut r, KEY_LEFTMETA, false);
    r.frame();
    assert_eq!(r.ls.state.core().focus().target(), Some(w));
    assert_eq!(history(&r), [w, y, x], "W first, Y and X in their order");
    assert!(
        !r.ls.probe.key_events.iter().any(|(k, _)| *k == KEY_TAB),
        "Super+Tab leaked to a client"
    );
}

/// Shift+Tab while Super is held steps back; outside a cycle Super+Shift+Tab stays the overview.
#[test]
fn shift_tab_steps_back_while_super_is_held() {
    let (mut r, [(_, w), (_, x), (_, y)]) = three();

    key(&mut r, KEY_LEFTMETA, true);
    tab(&mut r);
    tab(&mut r);
    assert_eq!(active(&r), Some("W"));
    key(&mut r, KEY_LEFTSHIFT, true);
    tab(&mut r);
    assert_eq!(active(&r), Some("X"), "one back");
    tab(&mut r);
    assert_eq!(active(&r), Some("Y"), "back to the start");
    tab(&mut r);
    assert_eq!(active(&r), Some("W"), "round to the last");
    key(&mut r, KEY_LEFTSHIFT, false);
    key(&mut r, KEY_LEFTMETA, false);
    r.frame();
    assert_eq!(history(&r), [w, y, x]);
}

/// Escape while Super is held: the focus and the camera go back to where they were before the
/// first Tab, and the order of use is as it was.
#[test]
fn escape_goes_back_to_the_focus_and_camera_before_the_first_tab() {
    let (mut r, [(_, w), (_, x), (_, y)]) = three();
    // W far off, so the camera must move to show it
    let element = ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: w,
    };
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::MoveElement {
                    element,
                    to: (5000.0, 0.0),
                    stream: ShellStream::Atomic,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();
    let before = r.ls.state.core().cameras().effective_value(0).unwrap();

    key(&mut r, KEY_LEFTMETA, true);
    tab(&mut r);
    tab(&mut r);
    assert_eq!(active(&r), Some("W"));
    assert_ne!(
        r.ls.state.core().cameras().effective_value(0).unwrap(),
        before,
        "the camera did not move to W"
    );
    key(&mut r, KEY_ESC, true);
    key(&mut r, KEY_ESC, false);
    r.frame();
    assert_eq!(active(&r), Some("Y"));
    assert_eq!(
        r.ls.state.core().cameras().effective_value(0).unwrap(),
        before,
        "the camera is not going back"
    );
    key(&mut r, KEY_LEFTMETA, false);
    r.frame();
    assert_eq!(r.ls.state.core().focus().target(), Some(y));
    assert_eq!(history(&r), [y, x, w]);
    assert!(
        !r.ls.probe.key_events.iter().any(|(k, _)| *k == KEY_ESC),
        "the cycle's Escape reached a client"
    );
}

/// A minimized window comes after the others; stepping onto it leaves it hidden until Super is
/// let go, and then it is back and first in the order of use.
#[test]
fn a_minimized_window_comes_last_and_is_restored_on_release() {
    let (mut r, [(_, w), (_, x), (_, y)]) = three();
    let (zi, z) = open(&mut r, "Z");
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(z)).unwrap();
    r.ls.pump();
    r.ls.probe.surfaces[zi]
        .toplevel
        .as_ref()
        .unwrap()
        .set_minimized();
    for _ in 0..3 {
        r.ls.pump();
        r.frame();
    }
    assert!(r.ls.state.core().stage().is_minimized(z));
    assert_eq!(r.ls.state.core().focus().target(), Some(y));

    key(&mut r, KEY_LEFTMETA, true);
    for expected in [x, w, z] {
        tab(&mut r);
        assert_eq!(r.ls.state.core().focus().target(), Some(expected));
    }
    assert!(
        r.ls.state.core().stage().is_minimized(z),
        "restored before Super was let go"
    );
    key(&mut r, KEY_LEFTMETA, false);
    r.frame();
    let core = r.ls.state.core();
    assert!(!core.stage().is_minimized(z), "not restored on release");
    assert_eq!(core.stage().top_mapped(), Some(z));
    assert_eq!(core.focus().target(), Some(z));
    assert_eq!(core.focus().history()[0], z);
}

/// A window asking for attention (it opened without the focus) is the first step; the next tap
/// goes back to where the user was.
#[test]
fn attention_is_the_first_step() {
    let (mut r, [(_, w), _, _]) = three();
    // used: Y, X, W; W asks, so the first step is W, not X
    r.ls.state.core_mut().opened_without_focus(w);
    assert_eq!(r.ls.state.core().focus().attention(), [w]);
    tap(&mut r);
    assert_eq!(active(&r), Some("W"), "attention first");
    assert!(
        r.ls.state.core().focus().attention().is_empty(),
        "its mark is cleared"
    );
    tap(&mut r);
    assert_eq!(active(&r), Some("Y"), "then back to where the user was");
}
