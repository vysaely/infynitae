//! Chord capture for the edge's Keys card (`capture-chord`): while a capture waits, a bound chord
//! is captured instead of acting (Super+Shift+I does not toggle stay awake), and a bare Escape
//! cancels it. Moved out of the Wayland crate, which may not name the input authority.

mod common;

use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{
    KEY_ESC, KEY_I, KEY_LEFTMETA, KEY_LEFTSHIFT, KeyCapture, format_chord,
};

fn key(r: &mut Rig, k: u32, pressed: bool) {
    r.t += 1000;
    keyboard_key(&mut r.ls.state, &mut r.ls.input, k, pressed, r.t).expect("key");
    r.ls.pump();
}

#[test]
fn key_capture_intercepts_a_bound_chord_and_escape_cancels() {
    let mut r = Rig::x8();
    key(&mut r, KEY_LEFTMETA, true);
    key(&mut r, KEY_LEFTSHIFT, true);

    // Super+Shift+I is bound to stay awake; while a capture waits it is captured instead
    assert!(r.ls.state.core_mut().begin_key_capture());
    key(&mut r, KEY_I, true);
    assert!(!r.ls.state.is_idle_inhibited());
    let Some(KeyCapture::Captured(chord)) = r.ls.state.core_mut().take_key_capture() else {
        panic!("expected a captured chord");
    };
    assert_eq!(format_chord(chord).as_deref(), Some("SUPER+SHIFT+I"));
    key(&mut r, KEY_I, false);
    key(&mut r, KEY_LEFTSHIFT, false);
    key(&mut r, KEY_LEFTMETA, false);

    // a bare Escape cancels without leaving the capture on
    assert!(r.ls.state.core_mut().begin_key_capture());
    key(&mut r, KEY_ESC, true);
    assert_eq!(
        r.ls.state.core_mut().take_key_capture(),
        Some(KeyCapture::Cancelled)
    );
    key(&mut r, KEY_ESC, false);
    assert!(!r.ls.state.core().is_capturing_key());
}
