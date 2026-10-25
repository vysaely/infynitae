//! A user action on a window brings it to the front (C4 focus model: click raises and gives
//! keyboard focus; owner, 2026-10-22).

mod common;

use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_LEFTMETA, KEY_TAB};
use infynitae_core::stage::{ElementId, ElementRef};
use infynitae_core::{ShellCommand, ShellStream};

/// W (from `Rig::x8`, under the pointer) and a second window W2 mapped after it, on top of it.
fn two() -> (Rig, ElementId, ElementId) {
    let mut r = Rig::x8();
    let qh = r.ls.qh.clone();
    r.ls.probe.toplevel(&qh, "W2", (800, 600), 20);
    r.ls.pump();
    r.frame();
    let m = r.ls.state.core().stage().mapped_elements();
    assert_eq!(m.len(), 2);
    (r, m[0], m[1])
}

fn top(r: &Rig) -> Option<ElementId> {
    r.ls.state.core().stage().top_mapped()
}

#[test]
fn super_tab_brings_the_focused_window_to_the_front() {
    let (mut r, w, w2) = two();
    assert_eq!(
        top(&r),
        Some(w2),
        "precondition: the later window is on top"
    );
    let tap = |r: &mut Rig| {
        for (key, pressed) in [
            (KEY_LEFTMETA, true),
            (KEY_TAB, true),
            (KEY_TAB, false),
            (KEY_LEFTMETA, false),
        ] {
            r.t += 1000;
            keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
            r.ls.pump();
        }
        r.frame();
    };
    tap(&mut r);
    let focused = r.ls.state.core().focus().target();
    assert_eq!(top(&r), focused, "the focused window is in front");
    tap(&mut r);
    let again = r.ls.state.core().focus().target();
    assert_ne!(again, focused, "the next window");
    assert_eq!(top(&r), again, "and it is in front now");
    assert!([Some(w), Some(w2)].contains(&again));
}

#[test]
fn a_click_brings_the_window_to_the_front() {
    let (mut r, w, w2) = two();
    // W2 goes far left: W alone lies under the pointer, still below W2 in the z-order
    let element = ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: w2,
    };
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::MoveElement {
                    element,
                    to: (-5000.0, 0.0),
                    stream: ShellStream::Atomic,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();
    assert_eq!(top(&r), Some(w2), "precondition: W is below");
    r.button(BTN_LEFT, true, Default::default());
    r.button(BTN_LEFT, false, Default::default());
    assert_eq!(r.ls.state.core().focus().target(), Some(w));
    assert_eq!(top(&r), Some(w), "the clicked window is in front");
}
