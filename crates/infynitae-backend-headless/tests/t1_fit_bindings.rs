//! INC-10 T1 (independent exit review, finding L5; proposed C12.F17): which key presses fit or restore, and
//! on which camera, on the production path with a real client and the virtual keyboard.
//!
//! - Logo+F fits the mapped element under the pointer; Logo+Shift+F restores the newest entry.
//! - The modifiers match exactly: Logo+Ctrl+F, Logo+Alt+F (and their Shift forms) are not
//!   compositor bindings, so F reaches the focused client (M12) and nothing changes.
//! - No element under the pointer: Logo+F is a no-op — no camera or history change, no pointer
//!   event — and F is still the compositor's (a resolved binding is consumed, M12).
//! - During a protocol-fullscreen presentation on the pointer's output (C12.F17 Q1, owner decision
//!   2026-10-19: B, scope 2): fit and restore are refused — no camera or history change, no
//!   pointer event — and the binding is still consumed (M12, F is not forwarded). The camera is
//!   the one from before the presentation when it ends.

mod common;

use common::input::*;
use common::{Lockstep, output};
use infynitae_backend_headless::{keyboard_key, set_focus};
use infynitae_core::input::keys::{KEY_ENTER, KEY_LEFTMETA, KEY_LEFTSHIFT};
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

fn history_len(r: &Rig) -> usize {
    r.ls.state.core().cameras().history(0).unwrap().len()
}

fn f_presses(r: &Rig) -> usize {
    r.ls.probe
        .key_events
        .iter()
        .filter(|e| **e == (KEY_ENTER, true))
        .count()
}

/// M8 fit (`true`) of W or restore on the pointer's output, through Core's shell entry.
fn shell(r: &mut Rig, fit: bool) {
    use infynitae_core::ShellCommand;
    let o = r.ls.output();
    let w = r.ls.state.elements().next().expect("W");
    let element = infynitae_core::stage::ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: w,
    };
    let cmd = if fit {
        ShellCommand::ViewportFit { output: o, element }
    } else {
        ShellCommand::ViewportRestore { output: o }
    };
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(&mut r.ls.shell, cmd)
            .expect("the shell command runs");
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();
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
    chord(&mut r, &[KEY_LEFTMETA]);
    assert_eq!(history_len(&r), 0, "Super+Enter at the fit restores");
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
        // the history is filled through Core's shell entry: the key is a toggle (Super+F, which
        // fitted again and again, was removed by the owner, 2026-10-21)
        while history_len(&r) < fill {
            shell(&mut r, true);
        }
        while history_len(&r) > fill {
            shell(&mut r, false);
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
/// camera follows the pan alone and the history is untouched, and F is forwarded to the focused
/// client (M12; C12.F17 Q4, owner decision 2026-10-19: forward, not consume).
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
        let presses = f_presses(&r);
        chord(&mut r, mods);
        assert!(same(camera(&r), panned), "{mods:?}: the camera changed");
        assert_eq!(history_len(&r), 1, "{mods:?}: the history changed");
        assert_eq!(f_presses(&r), presses + 1, "{mods:?}: F was not forwarded");
    }
    r.button(BTN_MIDDLE, false, logo());
}

/// C12.F17 Q1 B: with a fit recorded before the presentation (so a stray restore would show),
/// Logo+F and Logo+Shift+F while O presents W change neither camera A nor its history, deliver no
/// pointer event and do not forward F; the presented frame is unchanged. Once the presentation
/// ends, Logo+Shift+F restores as usual.
#[test]
fn fit_and_restore_are_refused_while_the_output_presents() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.map_w();
    r.motion((200.0, 200.0));
    r.motion((10.0, 0.0));
    focus_w(&mut r);
    let o = r.ls.output();
    let w = r.ls.state.elements().next().unwrap();
    let initial = camera(&r);
    chord(&mut r, &[KEY_LEFTMETA]);
    assert_eq!(history_len(&r), 1, "the fit before the presentation");
    let fitted = camera(&r);
    assert_eq!(fitted.zoom, 1.8);

    r.ls.probe.surfaces[0]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.state.core().presentation(o), Some(w), "W presented");
    let shown = r.ls.state.core_mut().render_list(o).unwrap().clone();
    let presses = f_presses(&r);

    // Super+Shift+Enter is the fullscreen toggle now (it would end this presentation): only the
    // fit toggle is the camera action here
    {
        let mods = &[KEY_LEFTMETA][..];
        let m = r.mark();
        chord(&mut r, mods);
        assert!(same(camera(&r), fitted), "{mods:?}: camera A changed");
        assert_eq!(history_len(&r), 1, "{mods:?}: history changed");
        assert_eq!(f_presses(&r), presses, "{mods:?}: F was forwarded");
        let lines = r.since(m);
        assert!(
            lines.iter().all(|l| !l.starts_with("motion")),
            "{mods:?}: a pointer event: {lines:?}"
        );
        let now = r.ls.state.core_mut().render_list(o).unwrap().clone();
        assert_eq!(
            now.view.k, shown.view.k,
            "{mods:?}: the presentation view changed"
        );
        assert_eq!(
            now.entries, shown.entries,
            "{mods:?}: the presented frame changed"
        );
    }

    r.ls.probe.surfaces[0]
        .toplevel
        .as_ref()
        .unwrap()
        .unset_fullscreen();
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.state.core().presentation(o), None);
    assert!(
        same(camera(&r), fitted),
        "the camera before the presentation is shown"
    );
    chord(&mut r, &[KEY_LEFTMETA]);
    assert!(
        same(camera(&r), initial),
        "the restore applies after the presentation"
    );
    assert_eq!(history_len(&r), 0);
}

/// C12.F17 Q1 B with the §8.1 evidence client: a client that obeys the fullscreen configure (it
/// redraws at the output's 1920 × 1080) is not fitted — Logo+F during the presentation is refused,
/// so after `unset_fullscreen` and the redraw at its saved 800 × 600 camera A is unchanged and W
/// is shown where it was before the presentation (under option A it was framed at its fullscreen
/// extent and then sat at the view's top-left).
#[test]
fn a_fit_during_the_presentation_of_a_resized_client_is_refused() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, "W", (800, 600), 0);
    r.ls.pump();
    r.frame();
    r.motion((10.0, 0.0));
    focus_w(&mut r);
    let o = r.ls.output();
    let w = r.ls.state.elements().next().unwrap();
    let before = camera(&r);
    let rect = |r: &mut Rig| {
        let list = r.ls.state.core_mut().render_list(o).unwrap().clone();
        let e = list
            .entries
            .iter()
            .find(|e| e.element == w)
            .expect("W shown");
        (e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1)
    };
    let placed = rect(&mut r);

    r.ls.probe.surfaces[idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.state.core().presentation(o), Some(w), "W presented");
    r.ls.probe.surfaces[idx].size = (1920, 1080);
    r.ls.probe.draw(&qh, idx);
    r.ls.pump();
    r.frame();
    let g = r.ls.state.core().stage().tree(w).unwrap().geometry;
    assert_eq!((g.size.w, g.size.h), (1920, 1080), "the client obeyed");

    chord(&mut r, &[KEY_LEFTMETA]);
    assert_eq!(history_len(&r), 0, "the fit was refused");
    assert!(same(camera(&r), before), "camera A unchanged");

    r.ls.probe.surfaces[idx]
        .toplevel
        .as_ref()
        .unwrap()
        .unset_fullscreen();
    r.ls.pump();
    r.frame();
    r.ls.probe.surfaces[idx].size = (800, 600);
    r.ls.probe.draw(&qh, idx);
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.state.core().presentation(o), None);
    assert!(same(camera(&r), before));
    assert_eq!(
        rect(&mut r),
        placed,
        "W is shown where it was before the presentation"
    );
}
