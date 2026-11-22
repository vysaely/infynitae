//! The owner's Super+Shift+C (Super+C until 2026-11-01; interaction intent of 2026-10-05): the
//! focused window is asked to close (`xdg_toplevel.close`) and decides itself; nothing closes
//! without a focused window, and the key does not reach the client.

mod common;

use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_C, KEY_DELETE, KEY_LEFTMETA, KEY_LEFTSHIFT};
use std::time::Duration;

/// Holds `mods`, taps C, and releases `mods`.
fn chord(r: &mut Rig, mods: &[u32]) {
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    for &k in mods {
        press(k, true);
    }
    press(KEY_C, true);
    press(KEY_C, false);
    for &k in mods.iter().rev() {
        press(k, false);
    }
    r.frame();
}

#[test]
fn super_shift_c_asks_the_focused_window_to_close() {
    let mut r = Rig::x8();
    // no focused window yet: nothing is asked to close
    chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    assert!(r.ls.probe.close_requests.is_empty());
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, Some(w))
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    let idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let before = r.ls.probe.key_events.len();
    chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    assert_eq!(r.ls.probe.close_requests, [idx]);
    assert!(
        r.ls.state.core().stage().state(w) == Some(infynitae_core::stage::Lifecycle::Mapped),
        "a request: the window stays until its client closes it"
    );
    assert!(
        !r.ls.probe.key_events[before..]
            .iter()
            .any(|(k, _)| *k == KEY_C),
        "the bound key never reaches the client"
    );
}

/// Super+Shift+Delete ends the focused window's client: its connection is cut and its window is
/// destroyed, with no polite close first. The probe client lives in this process, which is never
/// signalled (only processes other than the compositor's are killed; `ownership` tests the kill).
#[test]
fn super_shift_delete_ends_the_focused_windows_client() {
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
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
        (KEY_DELETE, true),
    ] {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
    }
    // a killed connection is cleaned up by the server when the client's socket stirs: a real app's
    // closes as its process dies; this in-process probe sends one more request instead
    let idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    r.ls.probe.surfaces[idx].wl.commit();
    let _ = r.ls.conn.flush();
    r.ls.display
        .dispatch_clients(&mut r.ls.state)
        .expect("server dispatch");
    assert!(r.ls.probe.close_requests.is_empty(), "not a polite close");
    assert!(
        r.ls.state.core().stage().state(w).is_none(),
        "the window went with its client"
    );
}

/// Focuses W, names it "Editor", freezes its client or not, and asks it to close (Super+Shift+C).
/// Returns W's element and the probe's index of it.
fn ask_w_to_close(r: &mut Rig, frozen: bool) -> (infynitae_core::stage::ElementId, usize) {
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, Some(w))
            .unwrap();
    r.ls.state.apply(&fx);
    let idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let surf = &r.ls.probe.surfaces[idx];
    surf.toplevel.as_ref().unwrap().set_title("Editor".into());
    surf.wl.commit();
    r.ls.pump();
    r.ls.probe.frozen = frozen;
    chord(r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    assert_eq!(r.ls.probe.close_requests, [idx]);
    assert_eq!(r.ls.probe.pings, 1, "one ping goes with the request");
    (w, idx)
}

fn asked(r: &Rig, w: infynitae_core::stage::ElementId, within: Duration) -> bool {
    let instance = r.ls.state.core().stage().instance().get();
    r.ls.wait_for_launcher_action(
        &format!("not-responding {instance} {} Editor", w.get()),
        within,
    )
}

/// Owner, 2026-11-22: a window asked to close that stays and does not answer the ping is asked
/// about ("Editor is not responding", End or Wait) once, after the wait and not before.
#[test]
fn a_frozen_window_asked_to_close_is_asked_about_once() {
    let mut r = Rig::x8();
    let start = r.ls.now;
    let (w, _) = ask_w_to_close(&mut r, true);
    r.ls.run_until(start + Duration::from_millis(4500));
    assert!(
        !asked(&r, w, Duration::from_millis(300)),
        "not before the wait: {:?}",
        r.ls.recorded_launcher_actions()
    );
    r.ls.run_until(start + Duration::from_millis(5200));
    assert!(
        asked(&r, w, Duration::from_secs(5)),
        "after it: {:?}",
        r.ls.recorded_launcher_actions()
    );
    r.ls.run_until(start + Duration::from_secs(12));
    std::thread::sleep(Duration::from_millis(200));
    let times =
        r.ls.recorded_launcher_actions()
            .iter()
            .filter(|a| a.starts_with("not-responding"))
            .count();
    assert_eq!(times, 1, "asked once");
    assert_eq!(
        r.ls.probe.pings, 1,
        "and pinged once: nothing polls the app"
    );
}

/// A client that answers the ping is left to close in its own time (it may be asking to save).
#[test]
fn a_window_that_answers_is_not_asked_about() {
    let mut r = Rig::x8();
    let start = r.ls.now;
    let (w, _) = ask_w_to_close(&mut r, false);
    r.ls.run_until(start + Duration::from_secs(6));
    assert!(
        !asked(&r, w, Duration::from_millis(300)),
        "{:?}",
        r.ls.recorded_launcher_actions()
    );
}

/// A frozen client whose window goes away anyway (it closed before it froze) is not asked about.
#[test]
fn a_window_that_went_is_not_asked_about() {
    let mut r = Rig::x8();
    let start = r.ls.now;
    let (w, idx) = ask_w_to_close(&mut r, true);
    r.ls.probe.destroy_toplevel(idx);
    r.ls.pump();
    assert!(r.ls.state.core().stage().state(w).is_none());
    r.ls.run_until(start + Duration::from_secs(6));
    assert!(
        !asked(&r, w, Duration::from_millis(300)),
        "{:?}",
        r.ls.recorded_launcher_actions()
    );
}
