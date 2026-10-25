//! Super+left drag moves a window; Super+right drag resizes it from the nearest corner
//! (owner, 2026-10-22). Both bring the window to the front and focus it; no pointer motion reaches
//! a client during the drag.

mod common;

use common::Lockstep;
use common::input::*;
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::stage::ElementId;

const LOGO: Modifiers = Modifiers {
    logo: true,
    ctrl: false,
    alt: false,
    shift: false,
};

/// xdg_toplevel state `resizing`.
const RESIZING: u32 = 3;

fn element(r: &Rig) -> ElementId {
    *r.ls.state.core().stage().mapped_elements().last().unwrap()
}

#[test]
fn super_left_drag_moves_the_window_with_the_pointer() {
    let mut r = Rig::x8();
    let w = element(&r);
    let before = r.ls.state.core().stage().anchor(w).unwrap();
    r.button(BTN_LEFT, true, LOGO);
    r.ls.probe.ptr.clear();
    r.motion((100.0, 50.0));
    r.motion((20.0, -10.0));
    assert!(
        !r.ls.probe.ptr.iter().any(|l| l.starts_with("motion")),
        "no motion reaches the client during the drag: {:?}",
        r.ls.probe.ptr
    );
    let after = r.ls.state.core().stage().anchor(w).unwrap();
    assert_eq!((after.x - before.x, after.y - before.y), (120, 40));
    r.button(BTN_LEFT, false, LOGO);
    assert!(!r.ls.state.core().interaction().active());
    assert_eq!(r.ls.state.core().focus().target(), Some(w));
    // a plain left click is the client's again
    r.ls.probe.ptr.clear();
    r.button(BTN_LEFT, true, Modifiers::default());
    r.button(BTN_LEFT, false, Modifiers::default());
    assert!(r.ls.probe.ptr.iter().any(|l| l.starts_with("button")));
    assert_eq!(r.ls.state.core().stage().anchor(w), Some(after));
}

/// A 400 × 300 window without decorations, centred on the view (960, 540): geometry from
/// (760, 390). The pointer goes to (860, 460), in its top-left quadrant, so the drag moves the
/// left and top sides and the bottom-right corner stays. Dragging (−50, −30) asks for 450 × 330;
/// when the client draws that size the anchor moves by (−50, −30).
#[test]
fn super_right_drag_resizes_from_the_nearest_corner() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, "R", (400, 300), 0);
    r.ls.pump();
    r.frame();
    let w = element(&r);
    let start = r.ls.state.core().stage().anchor(w).unwrap();
    r.motion((-100.0, -80.0));
    r.button(BTN_RIGHT, true, LOGO);
    r.ls.probe.toplevel_configures.clear();
    r.motion((-50.0, -30.0));
    let asked =
        r.ls.probe
            .toplevel_configures
            .last()
            .cloned()
            .expect("a configure with the new size");
    assert_eq!(asked.1, (450, 330));
    assert!(asked.2.contains(&RESIZING), "{:?}", asked.2);
    // one configure at a time: a further motion before the client drew waits
    let sent = r.ls.probe.toplevel_configures.len();
    r.motion((-10.0, 0.0));
    assert_eq!(
        r.ls.probe.toplevel_configures.len(),
        sent,
        "no configure while one is unanswered"
    );
    // the client draws the size it was asked for
    r.ls.probe.surfaces[idx].size = (450, 330);
    r.ls.probe.draw(&qh, idx);
    r.ls.pump();
    r.frame();
    let a = r.ls.state.core().stage().anchor(w).unwrap();
    assert_eq!(
        (a.x - start.x, a.y - start.y),
        (-50, -30),
        "the bottom-right corner stayed"
    );
    let next = r.ls.probe.toplevel_configures.last().cloned().unwrap();
    assert_eq!(
        next.1,
        (460, 330),
        "after the commit, the size asked for meanwhile"
    );
    r.button(BTN_RIGHT, false, LOGO);
    let last = r.ls.probe.toplevel_configures.last().cloned().unwrap();
    assert_eq!(last.1, (460, 330), "the last size, once more");
    assert!(
        !last.2.contains(&RESIZING),
        "without resizing: {:?}",
        last.2
    );
    assert!(!r.ls.state.core().interaction().active());
}

/// A drag on the empty Canvas, or with another modifier, is no window drag.
#[test]
fn super_drag_needs_a_window_and_exactly_super() {
    let mut r = Rig::x8();
    let w = element(&r);
    let before = r.ls.state.core().stage().anchor(w);
    // Ctrl held on the seat (Rig::button holds only Super itself)
    let key = |r: &mut Rig, k, pressed| {
        r.t += 1000;
        infynitae_backend_headless::keyboard_key(&mut r.ls.state, &mut r.ls.input, k, pressed, r.t)
            .expect("key");
        r.ls.pump();
    };
    key(&mut r, infynitae_core::input::keys::KEY_LEFTCTRL, true);
    r.button(BTN_LEFT, true, LOGO);
    r.motion((100.0, 0.0));
    r.button(BTN_LEFT, false, LOGO);
    key(&mut r, infynitae_core::input::keys::KEY_LEFTCTRL, false);
    assert_eq!(
        r.ls.state.core().stage().anchor(w),
        before,
        "Super+Ctrl is not a move"
    );
    // off the window: the pointer far to the right
    r.motion((600.0, 0.0));
    r.button(BTN_LEFT, true, LOGO);
    assert!(!r.ls.state.core().interaction().active());
    r.button(BTN_LEFT, false, LOGO);
}

#[test]
fn super_shift_left_drag_moves_cluster() {
    let mut r = Rig::x8();
    let w = element(&r);
    let before = r.ls.state.core().stage().anchor(w).unwrap();
    let key = |r: &mut Rig, k, pressed| {
        r.t += 1000;
        infynitae_backend_headless::keyboard_key(&mut r.ls.state, &mut r.ls.input, k, pressed, r.t)
            .expect("key");
        r.ls.pump();
    };
    key(&mut r, infynitae_core::input::keys::KEY_LEFTSHIFT, true);
    let shift_logo = Modifiers {
        shift: true,
        ..LOGO
    };
    r.button(BTN_LEFT, true, shift_logo);
    r.motion((100.0, 50.0));
    let after = r.ls.state.core().stage().anchor(w).unwrap();
    assert_eq!((after.x - before.x, after.y - before.y), (100, 50));
    r.button(BTN_LEFT, false, shift_logo);
    key(&mut r, infynitae_core::input::keys::KEY_LEFTSHIFT, false);
    assert!(!r.ls.state.core().interaction().active());
}

/// The window is destroyed mid-drag (move and resize alike): the drag is orphaned, motion is
/// consumed until the initiator is released, then the pointer is the clients' again.
#[test]
fn a_window_destroyed_mid_drag_orphans_the_drag() {
    for button in [BTN_LEFT, BTN_RIGHT] {
        let mut r = Rig::x8();
        r.button(button, true, LOGO);
        r.motion((30.0, 20.0));
        assert!(r.ls.state.core().interaction().active());
        r.ls.probe.destroy_toplevel(0);
        r.ls.pump();
        r.frame();
        assert!(r.ls.state.core().stage().mapped_elements().is_empty());
        r.motion((40.0, 10.0));
        assert!(
            r.ls.state.core().interaction().active(),
            "orphaned until the release"
        );
        r.button(button, false, LOGO);
        assert!(!r.ls.state.core().interaction().active());
    }
}

/// Super+middle during a Super+left window move (owner, 2026-10-22): the camera pans, the pointer
/// stays, and the window keeps its grabbed point under the pointer, traveling across the canvas
/// by the camera delta. Releasing middle ends the pan while the window remains held.
#[test]
fn super_middle_during_a_move_pans_and_carries_the_window() {
    let mut r = Rig::x8();
    let w = element(&r);
    let a0 = r.ls.state.core().stage().anchor(w).unwrap();
    r.button(BTN_LEFT, true, LOGO);
    r.button(BTN_MIDDLE, true, LOGO);
    let c0 = r.camera();
    r.ls.probe.ptr.clear();
    r.motion((-300.0, 120.0));
    r.motion((-50.0, 0.0));
    let c1 = r.camera();
    assert_ne!(c1, c0, "the camera panned");
    let a1 = r.ls.state.core().stage().anchor(w).unwrap();
    assert_eq!(
        ((a1.x - a0.x) << 16, (a1.y - a0.y) << 16),
        (c1.x - c0.x, c1.y - c0.y),
        "the window moved with the camera: same place on screen"
    );
    assert!(
        !r.ls.probe.ptr.iter().any(|l| l.starts_with("motion")),
        "nothing reaches a client: {:?}",
        r.ls.probe.ptr
    );
    // middle up: the pan ends, the move goes on
    r.button(BTN_MIDDLE, false, LOGO);
    assert!(r.ls.state.core().interaction().active());
    r.motion((10.0, 5.0));
    assert_eq!(r.camera(), c1, "no pan after middle is released");
    let a2 = r.ls.state.core().stage().anchor(w).unwrap();
    assert_eq!(
        (a2.x - a1.x, a2.y - a1.y),
        (10, 5),
        "the window follows the pointer again"
    );
    r.button(BTN_LEFT, false, LOGO);
    assert!(!r.ls.state.core().interaction().active());
    assert_eq!(r.ls.state.core().stage().anchor(w), Some(a2));
}

/// A fullscreen window (Super+Shift+Enter) is moved too (owner, 2026-10-25): Super+left takes it
/// out of fullscreen at its size before it and drags it with the pointer.
#[test]
fn super_left_drag_takes_a_fullscreen_window_out_and_moves_it() {
    use infynitae_backend_headless::keyboard_key;
    use infynitae_core::input::keys::{KEY_ENTER, KEY_LEFTMETA, KEY_LEFTSHIFT};
    let mut r = Rig::x8();
    let w = element(&r);
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, Some(w))
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    for (key, pressed) in [
        (KEY_LEFTMETA, true),
        (KEY_LEFTSHIFT, true),
        (KEY_ENTER, true),
        (KEY_ENTER, false),
        (KEY_LEFTSHIFT, false),
        (KEY_LEFTMETA, false),
    ] {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    }
    r.frame();
    assert!(
        r.ls.state.core().fullscreen_of(w).is_some(),
        "precondition: fullscreen"
    );

    r.button(BTN_LEFT, true, LOGO);
    assert!(
        r.ls.state.core().fullscreen_of(w).is_none(),
        "out of fullscreen"
    );
    assert!(r.ls.state.core().interaction().active(), "and moving");
    // the window's windowed position applies with the drag's first motion; from then on it
    // follows the pointer exactly
    r.motion((10.0, 10.0));
    let before = r.ls.state.core().stage().anchor(w).unwrap();
    r.motion((100.0, 50.0));
    let after = r.ls.state.core().stage().anchor(w).unwrap();
    assert_eq!((after.x - before.x, after.y - before.y), (100, 50));
    r.button(BTN_LEFT, false, LOGO);
    assert!(!r.ls.state.core().interaction().active());
    assert!(
        r.ls.state.core().fullscreen_of(w).is_none(),
        "it stays a window"
    );
}
