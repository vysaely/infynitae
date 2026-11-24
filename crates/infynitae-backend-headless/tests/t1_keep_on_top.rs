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

#[test]
fn show_window_menu_and_window_menu_bind_record_element_and_screen_position() {
    use infynitae_core::input::keys::{KeyAction, UserBindings, parse_chord};

    let mut r = Rig::x8();
    let id = *r.ls.state.core().stage().mapped_elements().first().unwrap();
    let anchor = r.ls.state.core().stage().anchor(id).unwrap();
    // the window is focused, as the click on its title bar that asks for the menu makes it
    let element = infynitae_core::stage::ElementRef::from_raw(
        r.ls.state.core().stage().instance().get(),
        id.get().get(),
    )
    .unwrap();
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(&mut r.ls.shell, ShellCommand::Focus { element })
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();

    // Ask via Wayland xdg_toplevel.show_window_menu at (30, 18) in the window's geometry.
    let seat = r.ls.probe.seat.clone().unwrap();
    r.ls.probe.surfaces[0]
        .toplevel
        .as_ref()
        .unwrap()
        .show_window_menu(&seat, 0, 30, 18);
    r.ls.pump();

    let expected_x = (anchor.x + 30 + 960) as f64;
    let expected_y = (anchor.y + 18 + 540) as f64;
    assert_eq!(
        r.ls.state.core_mut().take_window_menu(),
        Some((id, (expected_x, expected_y)))
    );

    // Configurable WINDOW_MENU bind on Super+right button triggers window_menu at pointer position.
    let chord = parse_chord("SUPER+BTN_RIGHT").expect("SUPER+BTN_RIGHT chord");
    r.ls.state.core_mut().set_key_bindings(UserBindings {
        chords: vec![(chord, Some(KeyAction::WindowMenu))],
        moved: vec![KeyAction::WindowMenu],
    });
    let pointer_p = match r.ls.state.core().pointer() {
        infynitae_core::pointer::PointerState::Located { p, .. } => p,
        _ => panic!("expected located pointer"),
    };
    r.button(BTN_RIGHT, true, logo());
    r.button(BTN_RIGHT, false, logo());
    assert_eq!(
        r.ls.state.core_mut().take_window_menu(),
        Some((id, pointer_p))
    );
}

#[test]
fn keep_on_top_command_toggles_stage_above_follows_camera_pan_and_remembers_for_app() {
    use infynitae_core::stage::ElementRef;

    let mut r = Rig::x8();
    let a = open(&mut r, "A", (320, 180), Some("Video"));
    let idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "A")
            .unwrap();
    r.ls.probe.surfaces[idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_app_id("VLC".to_owned());
    r.ls.probe.surfaces[idx].wl.commit();
    for _ in 0..4 {
        r.ls.pump();
        r.frame();
    }
    let eref =
        ElementRef::from_raw(r.ls.state.core().stage().instance().get(), a.get().get()).unwrap();
    assert!(!r.ls.state.core().stage().is_above(a));

    // Toggle on via ShellCommand::KeepOnTop.
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::KeepOnTop {
                    element: eref,
                    on: None,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx);
    assert!(r.ls.state.core().stage().is_above(a));
    assert!(
        r.ls.wait_for_launcher_action(
            "remember-window KEEP_ON_TOP vlc on",
            std::time::Duration::from_secs(5)
        ),
        "not remembered: {:?}",
        r.ls.recorded_launcher_actions()
    );

    // Stays above windows opened after it, and keeps its screen spot when the camera pans.
    let b = open(&mut r, "B", (400, 300), None);
    assert_eq!(top(&r), a);
    let (was, b_was) = (on_screen(&r, a), on_screen(&r, b));
    let o = r.ls.output();
    let nav_fx =
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
    r.ls.state.apply(&nav_fx);
    r.ls.pump();
    r.frame();
    assert_ne!(on_screen(&r, b), b_was);
    let now = on_screen(&r, a);
    assert!(
        (now.0 - was.0).abs() <= 1 && (now.1 - was.1).abs() <= 1,
        "kept-on-top window moved on the screen: {was:?} -> {now:?}"
    );

    // Toggle off via ShellCommand::KeepOnTop.
    let fx_off =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::KeepOnTop {
                    element: eref,
                    on: None,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx_off);
    assert!(!r.ls.state.core().stage().is_above(a));
    assert!(
        r.ls.wait_for_launcher_action(
            "remember-window KEEP_ON_TOP vlc off",
            std::time::Duration::from_secs(5)
        ),
        "not forgotten: {:?}",
        r.ls.recorded_launcher_actions()
    );
}

/// One right-click that asks for the window menu and opens the app's own menu with a grab
/// (Firefox's tab bar, owner 2026-11-24: the window menu took no click): the window menu wins
/// for that press, the app's grab on the same press is refused, and the pointer is not grabbed.
#[test]
fn the_window_menu_wins_over_a_popup_grab_on_the_same_press() {
    let mut r = Rig::x8();
    let id = *r.ls.state.core().stage().mapped_elements().first().unwrap();
    r.button(BTN_RIGHT, true, none());
    let serial = r.ls.probe.last_serial;
    let seat = r.ls.probe.seat.clone().unwrap();
    r.ls.probe.surfaces[0]
        .toplevel
        .as_ref()
        .unwrap()
        .show_window_menu(&seat, serial, 30, 18);
    r.ls.pump();
    assert_eq!(r.ls.state.core().window_menu().map(|(e, _)| e), Some(id));
    let qh = r.ls.qh.clone();
    r.ls.probe.popup_with_grab_serial(
        &qh,
        "P",
        0,
        common::PopupSpec {
            content: (120, 80),
            shadow: 0,
            anchor_rect: (10, 10, 1, 1),
            slide: false,
        },
        serial,
    );
    r.ls.pump();
    assert!(
        r.ls.probe.kb_log.iter().any(|l| l == "popup_done P"),
        "the app's grab on the same press is refused: {:?}",
        r.ls.probe.kb_log
    );
    assert!(!r.ls.state.is_pointer_grabbed(), "the pointer stays free");
    r.button(BTN_RIGHT, false, none());

    // a later press: the app's own menu works as before
    r.button(BTN_LEFT, true, none());
    let qh = r.ls.qh.clone();
    let later = r.ls.probe.last_serial;
    r.ls.probe.popup_with_grab_serial(
        &qh,
        "Q",
        0,
        common::PopupSpec {
            content: (120, 80),
            shadow: 0,
            anchor_rect: (10, 10, 1, 1),
            slide: false,
        },
        later,
    );
    r.ls.pump();
    assert!(
        !r.ls.probe.kb_log.iter().any(|l| l == "popup_done Q"),
        "{:?}",
        r.ls.probe.kb_log
    );
    r.button(BTN_LEFT, false, none());
}
