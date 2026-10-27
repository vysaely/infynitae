//! An app's own notification window (Steam's toasts: X11 windows titled
//! `notificationtoasts_<n>_desktop`) opens in the screen's bottom right corner, 12 px in from
//! its edges, where the camera is; a second stacks above the first; neither takes the focus
//! (owner, 2026-10-27: they showed up in random places on the canvas).

mod common;

use common::input::*;
use infynitae_core::geom::{CanvasPos, Q16_ONE};
use infynitae_core::stage::ElementId;

fn toast(r: &mut Rig, label: &'static str, n: u32) -> ElementId {
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, label, (283, 70), 0);
    let tl = r.ls.probe.surfaces[idx].toplevel.clone().unwrap();
    tl.set_app_id("steam".to_string());
    tl.set_title(format!("notificationtoasts_{n}_desktop"));
    r.ls.pump();
    r.frame();
    *r.ls.state.core().stage().mapped_elements().last().unwrap()
}

/// Where an element's window geometry starts on the first output, in logical px.
fn on_screen(r: &Rig, id: ElementId) -> (i64, i64) {
    let core = r.ls.state.core();
    let a = core.stage().anchor(id).unwrap();
    let view = core.view(r.ls.output()).unwrap();
    let p = view.project(CanvasPos {
        x: a.x * Q16_ONE,
        y: a.y * Q16_ONE,
    });
    (p.x.round() as i64, p.y.round() as i64)
}

#[test]
fn a_notification_opens_in_the_bottom_right_corner_and_takes_no_focus() {
    let mut r = Rig::x8();
    let focused = r.ls.state.core().focus().target();

    let a = toast(&mut r, "T1", 45);
    assert!(r.ls.state.core().is_notification(a));
    assert_eq!(
        on_screen(&r, a),
        (1920 - 12 - 283, 1080 - 12 - 70),
        "bottom right, 12 px in"
    );

    // a second, while the first is still shown: right above it, the gap between
    let b = toast(&mut r, "T2", 46);
    assert_eq!(
        on_screen(&r, b),
        (1920 - 12 - 283, 1080 - 12 - 70 - 12 - 70)
    );

    assert_eq!(
        r.ls.state.core().focus().target(),
        focused,
        "the notifications took no focus"
    );
}

/// A window of the same app with another title is an ordinary window, placed by the policy.
#[test]
fn another_steam_window_is_an_ordinary_one() {
    let mut r = Rig::x8();
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, "S", (283, 70), 0);
    let tl = r.ls.probe.surfaces[idx].toplevel.clone().unwrap();
    tl.set_app_id("steam".to_string());
    tl.set_title("Steam".to_string());
    r.ls.pump();
    r.frame();
    let s = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    assert!(!r.ls.state.core().is_notification(s));
    assert_ne!(on_screen(&r, s), (1920 - 12 - 283, 1080 - 12 - 70));
}

/// The title can come after the window appeared (Steam's toasts through xwayland-satellite): the
/// window is moved to the corner as soon as it turns out to be a notification (2026-10-27: they
/// still showed up in random places).
#[test]
fn a_notification_whose_title_comes_late_still_goes_to_the_corner() {
    let mut r = Rig::x8();
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, "T", (283, 70), 0);
    let tl = r.ls.probe.surfaces[idx].toplevel.clone().unwrap();
    tl.set_app_id("steam".to_string());
    r.ls.pump();
    r.frame();
    let t = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    assert!(!r.ls.state.core().is_notification(t), "not yet: no title");

    tl.set_title("notificationtoasts_52_desktop".to_string());
    r.ls.pump();
    r.frame();
    assert!(r.ls.state.core().is_notification(t));
    assert_eq!(on_screen(&r, t), (1920 - 12 - 283, 1080 - 12 - 70));
}
