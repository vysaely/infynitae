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

/// Super+Shift+K on a window keeps it on top and tells the session to remember that for its app
/// (`launch.sh remember-window KEEP_ON_TOP <app id> on`, owner 2026-11-11); again undoes both.
#[test]
fn super_shift_k_is_remembered_for_the_windows_app() {
    use infynitae_backend_headless::keyboard_key;
    use infynitae_core::input::keys::{KEY_K, KEY_LEFTMETA, KEY_LEFTSHIFT};
    let mut r = Rig::x8();
    // the rig's window under the pointer, named by its app id
    r.ls.probe.surfaces[0]
        .toplevel
        .as_ref()
        .unwrap()
        .set_app_id("MPV".to_owned());
    r.ls.probe.surfaces[0].wl.commit();
    for _ in 0..4 {
        r.ls.pump();
        r.frame();
    }
    let chord = |r: &mut Rig| {
        for (k, down) in [
            (KEY_LEFTMETA, true),
            (KEY_LEFTSHIFT, true),
            (KEY_K, true),
            (KEY_K, false),
            (KEY_LEFTSHIFT, false),
            (KEY_LEFTMETA, false),
        ] {
            r.t += 1000;
            keyboard_key(&mut r.ls.state, &mut r.ls.input, k, down, r.t).expect("key");
            r.ls.pump();
        }
    };
    // a click on it gives it the focus
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    let id =
        r.ls.state
            .core()
            .focus()
            .target()
            .expect("the click focused it");
    assert!(!r.ls.state.core().stage().is_above(id));
    chord(&mut r);
    assert!(
        r.ls.state.core().stage().is_above(id),
        "Super+Shift+K kept it on top"
    );
    assert!(
        r.ls.wait_for_launcher_action(
            "remember-window KEEP_ON_TOP mpv on",
            std::time::Duration::from_secs(5)
        ),
        "not remembered: {:?}",
        r.ls.recorded_launcher_actions()
    );
    chord(&mut r);
    assert!(!r.ls.state.core().stage().is_above(id));
    assert!(
        r.ls.wait_for_launcher_action(
            "remember-window KEEP_ON_TOP mpv off",
            std::time::Duration::from_secs(5)
        ),
        "not forgotten: {:?}",
        r.ls.recorded_launcher_actions()
    );
}

/// Super+Shift+P marks a window private and remembers it for its app (`PRIVACY_APPS`); again undoes both,
/// even though the window is now private through its app as well.
#[test]
fn super_shift_p_is_remembered_for_the_windows_app() {
    use infynitae_backend_headless::keyboard_key;
    use infynitae_core::input::keys::{KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_P};
    let mut r = Rig::x8();
    r.ls.probe.surfaces[0]
        .toplevel
        .as_ref()
        .unwrap()
        .set_app_id("KeePassXC".to_owned());
    r.ls.probe.surfaces[0].wl.commit();
    for _ in 0..4 {
        r.ls.pump();
        r.frame();
    }
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    let id =
        r.ls.state
            .core()
            .focus()
            .target()
            .expect("the click focused it");
    let chord = |r: &mut Rig| {
        for (k, down) in [
            (KEY_LEFTMETA, true),
            (KEY_LEFTSHIFT, true),
            (KEY_P, true),
            (KEY_P, false),
            (KEY_LEFTSHIFT, false),
            (KEY_LEFTMETA, false),
        ] {
            r.t += 1000;
            keyboard_key(&mut r.ls.state, &mut r.ls.input, k, down, r.t).expect("key");
            r.ls.pump();
        }
    };
    assert!(!r.ls.state.is_element_private(id));
    chord(&mut r);
    assert!(r.ls.state.is_element_private(id));
    assert!(r.ls.wait_for_launcher_action(
        "remember-window PRIVACY_APPS keepassxc on",
        std::time::Duration::from_secs(5)
    ));
    chord(&mut r);
    assert!(
        !r.ls.state.is_element_private(id),
        "private through its app still"
    );
    assert!(r.ls.wait_for_launcher_action(
        "remember-window PRIVACY_APPS keepassxc off",
        std::time::Duration::from_secs(5)
    ));
}
