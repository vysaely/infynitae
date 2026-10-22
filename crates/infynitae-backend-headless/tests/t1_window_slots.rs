//! Super+1..9 jumps to window slot 1..9 (creation order):
//! focuses the window, brings it to the front, and centers the camera on it.
//! (owner request 2026-10-22, "like on a kde or windows 11", Option A).

mod common;

use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_1, KEY_2, KEY_3, KEY_9, KEY_E, KEY_LEFTMETA, KEY_Q, KEY_W};
use infynitae_core::stage::ElementId;

fn top(r: &Rig) -> Option<ElementId> {
    r.ls.state.core().stage().top_mapped()
}

fn tap_slot(r: &mut Rig, key: u32) {
    for (k, pressed) in [
        (KEY_LEFTMETA, true),
        (key, true),
        (key, false),
        (KEY_LEFTMETA, false),
    ] {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, k, pressed, r.t).expect("key");
        r.ls.pump();
    }
    r.frame();
}

#[test]
fn super_1_to_9_focuses_raises_and_recenters() {
    let mut r = Rig::x8();
    let qh = r.ls.qh.clone();
    let w1 = r.ls.state.core().stage().mapped_in_creation_order()[0];

    // Map a second window W2
    r.ls.probe.toplevel(&qh, "W2", (800, 600), 20);
    r.ls.pump();
    r.frame();

    let mapped = r.ls.state.core().stage().mapped_in_creation_order();
    assert_eq!(mapped.len(), 2);
    let w2 = mapped[1];

    // Precondition: W2 was mapped last, so it is on top
    assert_eq!(top(&r), Some(w2));

    // Move camera away so we can verify recenter
    r.motion((200.0, 200.0));

    // Press Super+1: switches to window slot 1 (W1)
    tap_slot(&mut r, KEY_1);
    assert_eq!(
        r.ls.state.core().focus().target(),
        Some(w1),
        "W1 is focused"
    );
    assert_eq!(top(&r), Some(w1), "W1 is raised to top");

    // Press Super+2: switches to window slot 2 (W2)
    tap_slot(&mut r, KEY_2);
    assert_eq!(
        r.ls.state.core().focus().target(),
        Some(w2),
        "W2 is focused"
    );
    assert_eq!(top(&r), Some(w2), "W2 is raised to top");

    // Press Super+9 (no window in slot 9): does nothing, preserves focus on W2
    tap_slot(&mut r, KEY_9);
    assert_eq!(
        r.ls.state.core().focus().target(),
        Some(w2),
        "W2 stays focused on empty slot"
    );

    // Press Super+1 again: W1 is raised and focused again
    tap_slot(&mut r, KEY_1);
    assert_eq!(
        r.ls.state.core().focus().target(),
        Some(w1),
        "W1 focused again"
    );
    assert_eq!(top(&r), Some(w1), "W1 raised again");
}

#[test]
fn super_1_to_9_handles_three_windows() {
    let mut r = Rig::x8();
    let qh = r.ls.qh.clone();
    let w1 = r.ls.state.core().stage().mapped_in_creation_order()[0];

    r.ls.probe.toplevel(&qh, "W2", (600, 400), 20);
    r.ls.probe.toplevel(&qh, "W3", (500, 300), 20);
    r.ls.pump();
    r.frame();

    let mapped = r.ls.state.core().stage().mapped_in_creation_order();
    assert_eq!(mapped.len(), 3);
    let (w2, w3) = (mapped[1], mapped[2]);

    tap_slot(&mut r, KEY_2);
    assert_eq!(r.ls.state.core().focus().target(), Some(w2));
    assert_eq!(top(&r), Some(w2));

    tap_slot(&mut r, KEY_3);
    assert_eq!(r.ls.state.core().focus().target(), Some(w3));
    assert_eq!(top(&r), Some(w3));

    tap_slot(&mut r, KEY_1);
    assert_eq!(r.ls.state.core().focus().target(), Some(w1));
    assert_eq!(top(&r), Some(w1));
}

#[test]
fn super_w_toggles_launcher() {
    let mods = infynitae_core::input::bindings::Modifiers {
        logo: true,
        ..Default::default()
    };
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_W, mods, false),
        Some(infynitae_core::input::keys::KeyAction::ToggleLauncher),
        "Super+W resolves to ToggleLauncher"
    );
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_W, mods, true),
        None,
        "not during interaction"
    );
}

#[test]
fn super_q_spawns_terminal() {
    let mods = infynitae_core::input::bindings::Modifiers {
        logo: true,
        ..Default::default()
    };
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_Q, mods, false),
        Some(infynitae_core::input::keys::KeyAction::SpawnTerminal),
        "Super+Q resolves to SpawnTerminal"
    );
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_Q, mods, true),
        None,
        "not during interaction"
    );
}

#[test]
fn super_e_spawns_file_manager() {
    let mods = infynitae_core::input::bindings::Modifiers {
        logo: true,
        ..Default::default()
    };
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_E, mods, false),
        Some(infynitae_core::input::keys::KeyAction::SpawnFileManager),
        "Super+E resolves to SpawnFileManager"
    );
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_E, mods, true),
        None,
        "not during interaction"
    );
}

