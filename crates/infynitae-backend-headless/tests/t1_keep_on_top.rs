//! Picture-in-picture is kept on top (owner, 2026-11-07): above every window opened or raised
//! after it, in the view's corner, and in the same place on the screen when the view moves.

mod common;

use common::input::*;
use infynitae_core::geom::Q16_ONE;
use infynitae_core::stage::ElementId;
use infynitae_core::{NavigateTo, ShellCommand, ShellStream};

fn open(r: &mut Rig, label: &'static str, size: (i32, i32), title: Option<&str>) -> ElementId {
    let before = r.ls.state.core().stage().mapped_elements();
    let qh = r.ls.qh.clone();
    let i = r.ls.probe.toplevel_uncommitted(&qh, label, size);
    if let Some(t) = title {
        r.ls.probe.surfaces[i]
            .toplevel
            .as_ref()
            .unwrap()
            .set_title(t.to_owned());
    }
    r.ls.probe.surfaces[i].wl.commit();
    for _ in 0..4 {
        r.ls.pump();
        r.frame();
    }
    // the new one (the last in the z-order may be one kept on top)
    r.ls.state
        .core()
        .stage()
        .mapped_elements()
        .into_iter()
        .find(|e| !before.contains(e))
        .unwrap()
}

fn top(r: &Rig) -> ElementId {
    r.ls.state.core().stage().top_mapped().unwrap()
}

/// Where the window is on the screen: its anchor less the camera's centre, at zoom 1.
fn on_screen(r: &Rig, id: ElementId) -> (i64, i64) {
    let core = r.ls.state.core();
    let a = core.stage().anchor(id).unwrap();
    let c = core.cameras().center(0).unwrap();
    (a.x - c.x / Q16_ONE, a.y - c.y / Q16_ONE)
}

#[test]
fn picture_in_picture_stays_above_windows_opened_after_it() {
    let mut r = Rig::x8();
    let pip = open(&mut r, "P", (320, 180), Some("Picture-in-Picture"));
    assert!(r.ls.state.core().stage().is_above(pip), "not kept on top");
    assert_eq!(top(&r), pip);
    let later = open(&mut r, "L", (400, 300), None);
    assert_ne!(later, pip);
    assert_eq!(top(&r), pip, "a window opened after it went over it");
}

#[test]
fn picture_in_picture_keeps_its_place_on_the_screen_when_the_view_moves() {
    let mut r = Rig::x8();
    let pip = open(&mut r, "P", (320, 180), Some("Picture-in-Picture"));
    let other = *r.ls.state.core().stage().mapped_elements().first().unwrap();
    let (was, other_was) = (on_screen(&r, pip), on_screen(&r, other));
    let o = r.ls.output();
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::Navigate {
                    output: o,
                    to: NavigateTo::Delta(300.0, -100.0),
                    zoom: None,
                    stream: ShellStream::Atomic,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();
    assert_ne!(on_screen(&r, other), other_was, "the view did not move");
    let now = on_screen(&r, pip);
    assert!(
        (now.0 - was.0).abs() <= 1 && (now.1 - was.1).abs() <= 1,
        "picture-in-picture moved on the screen: {was:?} -> {now:?}"
    );
}

#[test]
fn an_ordinary_window_is_not_kept_on_top() {
    let mut r = Rig::x8();
    let a = open(&mut r, "A", (320, 180), Some("Some video"));
    assert!(!r.ls.state.core().stage().is_above(a));
    let b = open(&mut r, "B", (320, 180), None);
    assert_eq!(top(&r), b);
}
