//! Super+Tab through the real key path (owner, 2026-10-31): the recently used windows, most recent
//! first. A tap flips between the last two; holding Super, each Tab steps one further back, and
//! letting Super go chooses the window reached. A window that asks for attention (it opened
//! without the focus) comes first, and the next tap goes back. The client sees each change as a
//! keyboard enter.

mod common;

use common::input::*;
use infynitae_backend_headless::{keyboard_key, set_focus};
use infynitae_core::input::keys::{KEY_LEFTMETA, KEY_TAB};
use infynitae_core::stage::ElementId;

/// Maps a 300×200 window and returns it.
fn open(r: &mut Rig, label: &'static str) -> ElementId {
    let qh = r.ls.qh.clone();
    r.ls.probe.toplevel(&qh, label, (300, 200), 0);
    r.ls.pump();
    r.frame();
    *r.ls.state.core().stage().mapped_elements().last().unwrap()
}

fn key(r: &mut Rig, key: u32, pressed: bool) {
    r.t += 1000;
    keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
    r.ls.pump();
}

/// Super down, Tab, Super up.
fn tap(r: &mut Rig) {
    key(r, KEY_LEFTMETA, true);
    key(r, KEY_TAB, true);
    key(r, KEY_TAB, false);
    key(r, KEY_LEFTMETA, false);
    r.frame();
}

fn active(r: &Rig) -> Option<&str> {
    r.ls.probe.kb.active.as_deref()
}

/// W, X, Y used in that order: taps flip Y and X; held, Tab steps X then W; a window asking for
/// attention comes first, then the flip goes back.
#[test]
fn super_tab_flips_steps_back_while_held_and_puts_attention_first() {
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let x = open(&mut r, "X");
    let y = open(&mut r, "Y");
    for e in [w, x, y] {
        set_focus(&mut r.ls.state, &mut r.ls.input, Some(e)).unwrap();
        r.ls.pump();
    }
    assert_eq!(active(&r), Some("Y"));

    // two taps: Y -> X -> Y
    tap(&mut r);
    assert_eq!(active(&r), Some("X"));
    tap(&mut r);
    assert_eq!(active(&r), Some("Y"), "Tab, Tab returns");

    // Super held: X, then W; Super let go on W
    key(&mut r, KEY_LEFTMETA, true);
    key(&mut r, KEY_TAB, true);
    key(&mut r, KEY_TAB, false);
    assert_eq!(active(&r), Some("X"));
    key(&mut r, KEY_TAB, true);
    key(&mut r, KEY_TAB, false);
    assert_eq!(
        active(&r),
        Some("W"),
        "one further back while Super is held"
    );
    key(&mut r, KEY_LEFTMETA, false);
    r.frame();
    assert_eq!(r.ls.state.core().focus().target(), Some(w));
    // W is the most recent now: a tap goes back to Y, where the step-through began
    tap(&mut r);
    assert_eq!(active(&r), Some("Y"));

    // X opened without the focus (the adapter's FOCUS_ON_OPEN rules): Super+Tab goes to it first
    r.ls.state.core_mut().opened_without_focus(x);
    assert_eq!(r.ls.state.core().focus().attention(), [x]);
    tap(&mut r);
    assert_eq!(active(&r), Some("X"), "attention first");
    assert!(
        r.ls.state.core().focus().attention().is_empty(),
        "its mark is cleared"
    );
    tap(&mut r);
    assert_eq!(active(&r), Some("Y"), "then back to where the user was");
    // the Tab keys never reached a client
    assert!(
        !r.ls.probe.key_events.iter().any(|(k, _)| *k == KEY_TAB),
        "Super+Tab leaked to a client"
    );
}
