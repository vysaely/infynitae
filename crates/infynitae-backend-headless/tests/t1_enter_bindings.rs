//! The owner's Enter bindings (decision 2026-10-21): Super+Enter fits the window under the pointer
//! and, pressed again on that very fit, restores the camera before it (a stateless toggle: no fit
//! mode, INV-CAMERA-5); Super+Shift+Enter toggles the focused window's native fullscreen.

mod common;

use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_ENTER, KEY_LEFTMETA, KEY_LEFTSHIFT};
use infynitae_core::navigation::CameraValue;
use infynitae_core::{NavigateTo, ShellCommand, ShellStream};

/// Holds `mods`, taps Enter, releases `mods`.
fn chord(r: &mut Rig, mods: &[u32]) {
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    for &k in mods {
        press(k, true);
    }
    press(KEY_ENTER, true);
    press(KEY_ENTER, false);
    for &k in mods.iter().rev() {
        press(k, false);
    }
    r.frame();
}

fn camera(r: &Rig) -> CameraValue {
    let c = r.ls.state.core().cameras();
    CameraValue {
        center: c.center(0).unwrap(),
        zoom: c.zoom(0).unwrap(),
    }
}

fn same(a: CameraValue, b: CameraValue) -> bool {
    a.center == b.center && a.zoom.to_bits() == b.zoom.to_bits()
}

fn history(r: &Rig) -> usize {
    r.ls.state.core().cameras().history(0).unwrap().len()
}

#[test]
fn super_enter_fits_then_restores_then_fits_again() {
    let mut r = Rig::x8();
    let before = camera(&r);
    chord(&mut r, &[KEY_LEFTMETA]);
    let fitted = camera(&r);
    assert!(
        !same(fitted, before),
        "the window under the pointer was fitted"
    );
    assert_eq!(history(&r), 1);
    chord(&mut r, &[KEY_LEFTMETA]);
    assert!(
        same(camera(&r), before),
        "pressed on the fit: the camera before it"
    );
    assert_eq!(history(&r), 0);
    chord(&mut r, &[KEY_LEFTMETA]);
    assert!(same(camera(&r), fitted), "and again: the same fit");
    assert_eq!(history(&r), 1);
}

#[test]
fn super_enter_after_the_camera_moved_fits_again_instead_of_restoring() {
    let mut r = Rig::x8();
    chord(&mut r, &[KEY_LEFTMETA]);
    let fitted = camera(&r);
    // the camera moves a little (a shell navigation): it no longer holds the fit
    let o = r.ls.output();
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::Navigate {
                    output: o,
                    to: NavigateTo::Delta(3.0, 0.0),
                    stream: ShellStream::Atomic,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();
    assert!(!same(camera(&r), fitted));
    chord(&mut r, &[KEY_LEFTMETA]);
    assert!(same(camera(&r), fitted), "a fresh fit, not a restore");
    assert_eq!(history(&r), 2);
}

#[test]
fn super_shift_enter_toggles_the_focused_windows_fullscreen() {
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    // no focused window: nothing
    chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    assert!(r.ls.state.core().fullscreen_of(w).is_none());
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, Some(w))
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    let o = r.ls.output();
    assert_eq!(
        r.ls.state.core().fullscreen_of(w).map(|f| f.output),
        Some(o),
        "fullscreen on the pointer's output"
    );
    assert!(
        r.ls.probe
            .toplevel_states
            .iter()
            .any(|(_, s)| s.contains(&2)),
        "the client was configured fullscreen"
    );
    chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    assert!(r.ls.state.core().fullscreen_of(w).is_none(), "and back");
}
