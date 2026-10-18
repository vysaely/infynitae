//! INC-10 T1 (independent exit review, finding L5; proposed C12.F17): which key presses fit or restore, and
//! on which camera, on the production path with a real client and the virtual keyboard.
//!
//! - Logo+F fits the mapped element under the pointer; Logo+Shift+F restores the newest entry.
//! - The modifiers match exactly: Logo+Ctrl+F, Logo+Alt+F (and their Shift forms) are not
//!   compositor bindings, so F reaches the focused client (M12) and nothing changes.
//! - No element under the pointer: Logo+F is a no-op — no camera or history change, no pointer
//!   event — and F is still the compositor's (a resolved binding is consumed, M12).
//! - During a protocol-fullscreen presentation on the pointer's output: FOUNDATION_ARCHITECTURE
//!   §C5 ("fullscreen + viewport-fit on the same output", accepted at Phase C checkpoint 2) says
//!   the presentation takes precedence for rendering and "a fit applied meanwhile changes camera
//!   A's state, which is shown when the presentation ends". This test pins that ratified
//!   behaviour: what the output shows does not change, camera A and its history do. Whether that
//!   should instead be a no-op is an open owner decision (C12.F17 package, Q1); this test does not decide it.

mod common;

use common::input::*;
use common::{Lockstep, output};
use infynitae_backend_headless::{keyboard_key, set_focus};
use infynitae_core::input::keys::{KEY_F, KEY_LEFTMETA, KEY_LEFTSHIFT};
use infynitae_core::navigation::CameraValue;

const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTALT: u32 = 56;

/// Holds `mods` (in order), taps F, releases `mods` (in reverse).
fn chord(r: &mut Rig, mods: &[u32]) {
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    for &k in mods {
        press(k, true);
    }
    press(KEY_F, true);
    press(KEY_F, false);
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

fn history_len(r: &Rig) -> usize {
    r.ls.state.core().cameras().history(0).unwrap().len()
}

fn f_presses(r: &Rig) -> usize {
    r.ls.probe
        .key_events
        .iter()
        .filter(|e| **e == (KEY_F, true))
        .count()
}

fn focus_w(r: &mut Rig) {
    let w = r.ls.state.elements().next().expect("W");
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    r.ls.pump();
    r.frame();
}

/// Logo+Ctrl+F, Logo+Alt+F, Logo+Ctrl+Shift+F and Logo+Alt+Shift+F resolve to no binding: F is
/// forwarded to the focused client, and camera, history and the pointer are untouched — also with
/// a history entry present, so a stray restore would show. Plain Logo+F and Logo+Shift+F still
/// work afterwards.
#[test]
fn extra_ctrl_or_alt_modifiers_trigger_neither_binding() {
    let mut r = Rig::x8();
    focus_w(&mut r);
    chord(&mut r, &[KEY_LEFTMETA]);
    assert_eq!(history_len(&r), 1, "Logo+F fits");
    let fitted = camera(&r);
    for mods in [
        &[KEY_LEFTMETA, KEY_LEFTCTRL][..],
        &[KEY_LEFTMETA, KEY_LEFTALT],
        &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_LEFTSHIFT],
        &[KEY_LEFTMETA, KEY_LEFTALT, KEY_LEFTSHIFT],
        &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_LEFTALT],
    ] {
        let f = f_presses(&r);
        let m = r.mark();
        chord(&mut r, mods);
        assert!(same(camera(&r), fitted), "{mods:?}: the camera changed");
        assert_eq!(history_len(&r), 1, "{mods:?}: the history changed");
        assert!(
            r.since(m).iter().all(|l| !l.starts_with("motion")),
            "{mods:?}: {:?}",
            r.since(m)
        );
        assert_eq!(
            f_presses(&r),
            f + 1,
            "{mods:?}: F not forwarded to the client"
        );
    }
    let f = f_presses(&r);
    chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    assert_eq!(history_len(&r), 0, "Logo+Shift+F restores");
    assert_eq!(f_presses(&r), f, "a binding's F reached the client");
}

/// With no element under the pointer, Logo+F changes nothing (camera, history, wire) at an empty
/// history, with an entry, and at capacity; F is consumed as a resolved binding.
#[test]
fn logo_f_with_no_element_under_the_pointer_is_a_no_op() {
    let mut r = Rig::x8();
    focus_w(&mut r);
    // fill the history to 0, 1 and 256 entries with fits on W, then move off W
    for fill in [0usize, 1, 256] {
        while history_len(&r) < fill {
            chord(&mut r, &[KEY_LEFTMETA]);
        }
        while history_len(&r) > fill {
            chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
        }
        let back = r.ls.state.core().pointer();
        // off W: x8's pointer is at (1170, 740); W spans at most the view, so the far corner of
        // the output is empty Canvas at every camera reached here
        r.motion((1919.0 - 1170.0, 1079.0 - 740.0));
        r.motion((1.0, 1.0)); // clamped to the output
        let before = camera(&r);
        let f = f_presses(&r);
        let m = r.mark();
        chord(&mut r, &[KEY_LEFTMETA]);
        assert!(same(camera(&r), before), "{fill}: the camera changed");
        assert_eq!(history_len(&r), fill, "{fill}: the history changed");
        assert!(r.since(m).is_empty(), "{fill}: {:?}", r.since(m));
        assert_eq!(f_presses(&r), f, "{fill}: F reached the client");
        // back onto W for the next round
        let infynitae_core::pointer::PointerState::Located { p: (x0, y0), .. } = back else {
            panic!("pointer");
        };
        let infynitae_core::pointer::PointerState::Located { p: (x1, y1), .. } =
            r.ls.state.core().pointer()
        else {
            panic!("pointer");
        };
        r.motion((x0 - x1, y0 - y1));
    }
}

/// During a pan, Logo+F and Logo+Shift+F (Logo already held by the pan) resolve to nothing: the
/// camera follows the pan alone and the history is untouched.
#[test]
fn fit_and_restore_are_inert_during_a_pan() {
    let mut r = Rig::x8();
    focus_w(&mut r);
    chord(&mut r, &[KEY_LEFTMETA]);
    assert_eq!(history_len(&r), 1);
    r.button(BTN_MIDDLE, true, logo());
    r.motion((5.0, 3.0));
    let panned = camera(&r);
    for mods in [&[][..], &[KEY_LEFTSHIFT]] {
        chord(&mut r, mods);
        assert!(same(camera(&r), panned), "{mods:?}: the camera changed");
        assert_eq!(history_len(&r), 1, "{mods:?}: the history changed");
    }
    r.button(BTN_MIDDLE, false, logo());
}

/// The ratified §C5 behaviour during a presentation (see the module doc): Logo+F with the
/// presented W under the pointer fits camera A to W — history +1 — while the output keeps showing
/// the presentation view unchanged; Logo+Shift+F then restores camera A, again unseen; a second
/// fit is shown only once the presentation ends.
#[test]
fn a_fit_during_a_presentation_changes_the_hidden_camera_as_ratified() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.map_w();
    r.motion((200.0, 200.0));
    r.motion((10.0, 0.0));
    focus_w(&mut r);
    let o = r.ls.output();
    let w = r.ls.state.elements().next().unwrap();
    r.ls.probe.surfaces[0]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.state.core().presentation(o), Some(w), "W presented");
    let shown = r.ls.state.core_mut().render_list(o).unwrap().clone();
    let before = camera(&r);

    chord(&mut r, &[KEY_LEFTMETA]);
    assert_eq!(history_len(&r), 1, "the fit is recorded");
    assert_eq!(camera(&r).zoom, 1.8, "camera A is fitted to W");
    let now = r.ls.state.core_mut().render_list(o).unwrap().clone();
    assert_eq!(now.view.k, shown.view.k, "the presentation view changed");
    assert_eq!(now.entries, shown.entries, "the presented frame changed");

    chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    assert!(same(camera(&r), before), "restore applies to camera A");
    assert_eq!(history_len(&r), 0);
    let now = r.ls.state.core_mut().render_list(o).unwrap().clone();
    assert_eq!(now.entries, shown.entries, "the presented frame changed");

    chord(&mut r, &[KEY_LEFTMETA]);
    let fitted = camera(&r);
    r.ls.probe.surfaces[0]
        .toplevel
        .as_ref()
        .unwrap()
        .unset_fullscreen();
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.state.core().presentation(o), None);
    let after = r.ls.state.core_mut().render_list(o).unwrap().clone();
    assert!(same(camera(&r), fitted));
    assert_eq!(
        after.view.k, 1.8,
        "camera A's fitted view is shown when the presentation ends"
    );
}
