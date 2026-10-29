//! FOCUS_ON_OPEN = "priority" (owner, 2026-10-29): a new window takes the focus unless the focus
//! is busy. A window the user opened always takes it; with Do Not Disturb on no other window
//! does; a fullscreen window keeps it against every window but its own child windows; otherwise
//! the new window takes it (`WaylandState::open_focus_held`).

mod common;

use common::input::*;
use infynitae_backend_headless::set_focus;
use infynitae_core::stage::ElementId;

/// Maps a 300×200 window, a child of the probe's window `parent` when given.
fn open(r: &mut Rig, label: &'static str, parent: Option<usize>) -> ElementId {
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, label, (300, 200), 0);
    if let Some(p) = parent {
        let parent_tl = r.ls.probe.surfaces[p].toplevel.clone().unwrap();
        r.ls.probe.surfaces[idx]
            .toplevel
            .as_ref()
            .unwrap()
            .set_parent(Some(&parent_tl));
    }
    r.ls.pump();
    r.frame();
    *r.ls.state.core().stage().mapped_elements().last().unwrap()
}

fn idx_of(r: &Rig, label: &str) -> usize {
    r.ls.probe
        .surfaces
        .iter()
        .position(|s| s.label == label)
        .unwrap()
}

fn focus(r: &mut Rig, e: ElementId) {
    set_focus(&mut r.ls.state, &mut r.ls.input, Some(e)).unwrap();
    r.ls.pump();
}

/// A flag file of this test's own: the real session's Do Not Disturb never leaks in.
fn dnd_flag(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("infynitae-test-dnd-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn a_fullscreen_window_keeps_the_focus_against_all_but_its_own_dialogs() {
    let mut r = Rig::x8();
    r.ls.state.dnd_flag = Some(dnd_flag("fullscreen"));
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let w_idx = idx_of(&r, "W");
    let x = open(&mut r, "X", None);
    let d = open(&mut r, "D", Some(w_idx));

    // windowed: any new window takes the focus
    focus(&mut r, w);
    assert!(
        !r.ls.state.open_focus_held(x, false),
        "windowed: X takes it"
    );

    // W fullscreen and focused: another app's window is held, W's own dialog is not, and a
    // window the user opened always takes it
    r.ls.probe.surfaces[w_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    r.ls.pump();
    r.frame();
    focus(&mut r, w);
    assert!(
        r.ls.state.core().fullscreen_of(w).is_some(),
        "W is fullscreen"
    );
    assert!(
        r.ls.state.open_focus_held(x, false),
        "fullscreen: X is held"
    );
    assert!(
        !r.ls.state.open_focus_held(d, false),
        "fullscreen: W's own dialog takes it"
    );
    assert!(
        !r.ls.state.open_focus_held(x, true),
        "a window the user opened takes it"
    );

    // W back to a window: X takes it again
    r.ls.probe.surfaces[w_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .unset_fullscreen();
    r.ls.pump();
    r.frame();
    focus(&mut r, w);
    assert!(
        r.ls.state.core().fullscreen_of(w).is_none(),
        "W is windowed"
    );
    assert!(!r.ls.state.open_focus_held(x, false), "windowed again");
}

#[test]
fn do_not_disturb_holds_every_window_the_user_did_not_open() {
    let mut r = Rig::x8();
    let flag = dnd_flag("switch");
    r.ls.state.dnd_flag = Some(flag.clone());
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let w_idx = idx_of(&r, "W");
    let x = open(&mut r, "X", None);
    let d = open(&mut r, "D", Some(w_idx));
    focus(&mut r, w);

    assert!(!r.ls.state.do_not_disturb());
    assert!(!r.ls.state.open_focus_held(x, false), "off: X takes it");

    std::fs::write(&flag, b"").unwrap();
    assert!(r.ls.state.do_not_disturb());
    assert!(r.ls.state.open_focus_held(x, false), "on: X is held");
    assert!(r.ls.state.open_focus_held(d, false), "on: a dialog too");
    assert!(
        !r.ls.state.open_focus_held(x, true),
        "on: a window the user opened still takes it"
    );

    std::fs::remove_file(&flag).unwrap();
    assert!(!r.ls.state.open_focus_held(x, false), "off again");

    // no runtime dir: never on
    r.ls.state.dnd_flag = None;
    assert!(!r.ls.state.do_not_disturb());
}
