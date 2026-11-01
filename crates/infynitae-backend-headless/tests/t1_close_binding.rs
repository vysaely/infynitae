//! The owner's Super+Shift+C (Super+C until 2026-11-01; interaction intent of 2026-10-05): the
//! focused window is asked to close (`xdg_toplevel.close`) and decides itself; nothing closes
//! without a focused window, and the key does not reach the client.

mod common;

use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_C, KEY_LEFTCTRL, KEY_LEFTMETA, KEY_LEFTSHIFT};

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

/// Super+Ctrl+Shift+C ends the focused window's client: its connection is cut and its window is
/// destroyed, with no polite close first. The probe client lives in this process, which is never
/// signalled (only processes other than the compositor's are killed; `ownership` tests the kill).
#[test]
fn super_ctrl_shift_c_ends_the_focused_windows_client() {
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
        (KEY_LEFTCTRL, true),
        (KEY_LEFTSHIFT, true),
        (KEY_C, true),
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
