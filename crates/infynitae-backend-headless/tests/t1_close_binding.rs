//! The owner's Super+Delete (interaction intent of 2026-10-05; decision record 2026-10-21): the
//! focused window is asked to close (`xdg_toplevel.close`) and decides itself; nothing closes
//! without a focused window, and the key does not reach the client.

mod common;

use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_DELETE, KEY_LEFTMETA, KEY_LEFTSHIFT};

/// Holds `mods`, taps Delete, releases `mods`.
fn chord(r: &mut Rig, mods: &[u32]) {
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    for &k in mods {
        press(k, true);
    }
    press(KEY_DELETE, true);
    press(KEY_DELETE, false);
    for &k in mods.iter().rev() {
        press(k, false);
    }
    r.frame();
}

#[test]
fn super_delete_asks_the_focused_window_to_close() {
    let mut r = Rig::x8();
    // no focused window yet: nothing is asked to close
    chord(&mut r, &[KEY_LEFTMETA]);
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
    chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT]);
    assert!(
        r.ls.probe.close_requests.is_empty(),
        "Super+Shift+Delete is not bound"
    );
    // the unbound chord reached the client (Delete is an ordinary key there); the bound one
    // must not
    let before = r.ls.probe.key_events.len();
    chord(&mut r, &[KEY_LEFTMETA]);
    assert_eq!(r.ls.probe.close_requests, [idx]);
    assert!(
        r.ls.state.core().stage().state(w) == Some(infynitae_core::stage::Lifecycle::Mapped),
        "a request: the window stays until its client closes it"
    );
    assert!(
        !r.ls.probe.key_events[before..]
            .iter()
            .any(|(k, _)| *k == KEY_DELETE),
        "the bound key never reaches the client"
    );
}
