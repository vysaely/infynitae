//! Super+1..9 jumps to window slot 1..9 in creation order (owner, 2026-10-22):
//! focuses the window, brings it to the front, and centres the camera on it.

mod common;

use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{
    KEY_1, KEY_2, KEY_3, KEY_9, KEY_D, KEY_E, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_Q, KEY_S, KEY_SYSRQ,
    KEY_TAB, KEY_W,
};
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

fn tap_cluster_slot(r: &mut Rig, key: u32) {
    for (k, pressed) in [
        (KEY_LEFTMETA, true),
        (KEY_LEFTSHIFT, true),
        (key, true),
        (key, false),
        (KEY_LEFTSHIFT, false),
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
fn super_d_toggles_window_list() {
    let mods = infynitae_core::input::bindings::Modifiers {
        logo: true,
        ..Default::default()
    };
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_D, mods, false),
        Some(infynitae_core::input::keys::KeyAction::ToggleWindowList),
        "Super+D resolves to ToggleWindowList"
    );
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_D, mods, true),
        None,
        "not during interaction"
    );
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

#[test]
fn super_shift_s_spawns_screenshot() {
    let mods = infynitae_core::input::bindings::Modifiers {
        logo: true,
        shift: true,
        ..Default::default()
    };
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_S, mods, false),
        Some(infynitae_core::input::keys::KeyAction::SpawnScreenshot),
        "Super+Shift+S resolves to SpawnScreenshot"
    );
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_S, mods, true),
        None,
        "not during interaction"
    );
}

#[test]
fn printscreen_spawns_screenshot() {
    let mods = infynitae_core::input::bindings::Modifiers::default();
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_SYSRQ, mods, false),
        Some(infynitae_core::input::keys::KeyAction::SpawnScreenshot),
        "PrintScreen resolves to SpawnScreenshot"
    );
}

#[test]
fn super_shift_1_to_9_resolves_cluster_slots() {
    let mods = infynitae_core::input::bindings::Modifiers {
        logo: true,
        shift: true,
        ..Default::default()
    };
    for slot in 0..9 {
        let key = (slot as u32) + KEY_1;
        assert_eq!(
            infynitae_core::input::keys::resolve_key_binding(key, mods, false),
            Some(infynitae_core::input::keys::KeyAction::FocusClusterSlot(
                slot
            )),
            "Super+Shift+{key} resolves to FocusClusterSlot({slot})"
        );
        assert_eq!(
            infynitae_core::input::keys::resolve_key_binding(key, mods, true),
            None,
            "not during interaction"
        );
    }
}

#[test]
fn super_shift_1_to_9_focuses_and_frames_clusters() {
    let mut r = Rig::x8();
    let qh = r.ls.qh.clone();
    let w1 = r.ls.state.core().stage().mapped_in_creation_order()[0];

    // Map a second window W2
    r.ls.probe.toplevel(&qh, "W2", (800, 600), 20);
    r.ls.pump();
    r.frame();

    let mapped = r.ls.state.core().stage().mapped_in_creation_order();
    assert_eq!(mapped.len(), 2);
    let _w2 = mapped[1];

    let clusters = r.ls.state.core().clusters();
    assert_eq!(
        clusters.len(),
        1,
        "both windows near each other form 1 cluster"
    );
    assert_eq!(clusters[0].len(), 2, "cluster 1 has 2 windows");

    // Press Super+Shift+1 -> focuses Cluster 1 (W1 primary)
    tap_cluster_slot(&mut r, KEY_1);
    assert_eq!(
        r.ls.state.core().focus().target(),
        Some(w1),
        "Cluster 1 focuses W1"
    );
}

#[test]
fn super_shift_tab_toggles_overview() {
    let mods = infynitae_core::input::bindings::Modifiers {
        logo: true,
        shift: true,
        ..Default::default()
    };
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(KEY_TAB, mods, false),
        Some(infynitae_core::input::keys::KeyAction::ToggleOverview),
        "Super+Shift+Tab resolves to ToggleOverview"
    );
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(
            infynitae_core::input::keys::KEY_R,
            mods,
            false
        ),
        Some(infynitae_core::input::keys::KeyAction::SpawnRegionScreenshot),
        "Super+Shift+R resolves to SpawnRegionScreenshot"
    );
    assert_eq!(
        infynitae_core::input::keys::resolve_key_binding(
            infynitae_core::input::keys::KEY_BACKSPACE,
            mods,
            false
        ),
        Some(infynitae_core::input::keys::KeyAction::ReloadShell),
        "Super+Shift+BackSpace resolves to ReloadShell"
    );
}
