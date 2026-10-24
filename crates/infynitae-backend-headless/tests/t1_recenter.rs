//! The owner's Super+Space and Super+Shift+Space (owner request 2026-10-24): the pointer's
//! output's camera is centred on the focused window and zoom is reverted to default 1.0 scale
//! (or, if no window is focused, camera zoom is reset to 1.0). No history entry.

mod common;

use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::geom::{CanvasPos, Q16_ONE};
use infynitae_core::input::keys::{KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_SPACE, KEY_TAB};
use infynitae_core::{NavigateTo, ShellCommand, ShellStream};

/// Holds `mods`, taps Space, releases `mods`.
fn chord(r: &mut Rig, mods: &[u32]) {
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    for &k in mods {
        press(k, true);
    }
    press(KEY_SPACE, true);
    press(KEY_SPACE, false);
    for &k in mods.iter().rev() {
        press(k, false);
    }
    r.frame();
}

/// The camera is moved away from the window (a shell navigation), the window focused or not.
fn away(focus: bool) -> (Rig, CanvasPos) {
    let mut r = Rig::x8();
    let o = r.ls.output();
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::Navigate {
                    output: o,
                    to: NavigateTo::Delta(300.0, -100.0),
                    stream: ShellStream::Atomic,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();
    let core = r.ls.state.core();
    let w = *core.stage().mapped_elements().last().unwrap();
    let a = core.stage().anchor(w).unwrap();
    let size = core.stage().tree(w).unwrap().geometry.size;
    let centre = CanvasPos {
        x: a.x * Q16_ONE + i64::from(size.w) * (Q16_ONE / 2),
        y: a.y * Q16_ONE + i64::from(size.h) * (Q16_ONE / 2),
    };
    assert_ne!(
        core.cameras().center(0),
        Some(centre),
        "precondition: moved away"
    );
    if focus {
        let fx =
            r.ls.state
                .core_mut()
                .set_focus(&mut r.ls.input, Some(w))
                .unwrap();
        r.ls.state.apply(&fx);
        r.ls.pump();
    }
    (r, centre)
}

#[test]
fn super_space_centres_the_focused_window_and_keeps_the_zoom() {
    let (mut r, centre) = away(true);
    let zoom = r.ls.state.core().cameras().zoom(0).unwrap();
    let history = r.ls.state.core().cameras().history(0).unwrap().len();
    r.ls.probe.ptr.clear();
    chord(&mut r, &[KEY_LEFTMETA]);
    let c = r.ls.state.core().cameras();
    assert_eq!(c.center(0), Some(centre));
    assert_eq!(c.zoom(0).unwrap().to_bits(), zoom.to_bits());
    assert_eq!(c.history(0).unwrap().len(), history, "no history entry");
    assert!(
        r.ls.probe
            .ptr
            .iter()
            .any(|l| l.starts_with("motion") || l.starts_with("enter W")),
        "the window moved under the pointer: one true-position event (M10): {:?}",
        r.ls.probe.ptr
    );
}

#[test]
fn super_shift_space_also_centres_the_focused_window() {
    let (mut r, centre) = away(true);
    let zoom = r.ls.state.core().cameras().zoom(0).unwrap();
    let history = r.ls.state.core().cameras().history(0).unwrap().len();
    chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    let c = r.ls.state.core().cameras();
    assert_eq!(c.center(0), Some(centre), "Super+Shift+Space centres the window");
    assert_eq!(c.zoom(0).unwrap().to_bits(), zoom.to_bits());
    assert_eq!(c.history(0).unwrap().len(), history, "no history entry");
}

#[test]
fn super_space_reverts_overview_zoom_to_default_scale() {
    let (mut r, centre) = away(true);
    // Press Super+Shift+Tab to trigger ToggleOverview
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    press(KEY_LEFTMETA, true);
    press(KEY_LEFTSHIFT, true);
    press(KEY_TAB, true);
    press(KEY_TAB, false);
    press(KEY_LEFTSHIFT, false);
    press(KEY_LEFTMETA, false);
    r.frame();

    // Now pressing Super+Space must recenter on focused window AND revert zoom to 1.0!
    chord(&mut r, &[KEY_LEFTMETA]);
    let c = r.ls.state.core().cameras();
    assert_eq!(c.center(0), Some(centre), "window centred");
    assert_eq!(c.zoom(0), Some(1.0), "zoom reverted to default 1.0 scale");
}

#[test]
fn super_space_without_a_focused_window_at_default_zoom_changes_nothing() {
    let (mut r, _) = away(false);
    let before = r.ls.state.core().cameras().center(0);
    chord(&mut r, &[KEY_LEFTMETA]);
    assert_eq!(
        r.ls.state.core().cameras().center(0),
        before,
        "nothing focused and already at 1.0 zoom"
    );
}
