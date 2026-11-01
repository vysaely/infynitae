//! keyboard-shortcuts-inhibit-v1 with a real client: an inhibitor is active only while its window
//! has keyboard focus, and while it is, a bound chord reaches the window as keys.

mod common;

use std::time::Duration;

use common::{Lockstep, Probe, output};
use infynitae_backend_headless::{keyboard_key, set_focus};
use infynitae_core::input::keys::{KEY_ENTER, KEY_LEFTMETA, KEY_TAB};
use infynitae_core::stage::ElementId;
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1;
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibitor_v1::{
    self, ZwpKeyboardShortcutsInhibitorV1,
};

wayland_client::delegate_noop!(Probe: ignore ZwpKeyboardShortcutsInhibitManagerV1);

/// `active` / `inactive` go to the probe's log.
impl wayland_client::Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitorV1,
        ev: zwp_keyboard_shortcuts_inhibitor_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        match ev {
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Active => st.log.push("active".into()),
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Inactive => st.log.push("inactive".into()),
            _ => {}
        }
    }
}

struct S {
    ls: Lockstep,
    t: u64,
}

impl S {
    fn pump(&mut self) {
        self.ls.pump();
        let t = self.ls.now + Duration::from_micros(1000);
        self.ls.run_until(t);
    }

    fn key(&mut self, k: u32, pressed: bool) {
        self.t += 1000;
        keyboard_key(&mut self.ls.state, &mut self.ls.input, k, pressed, self.t).expect("key");
        self.pump();
    }

    fn focus(&mut self, e: ElementId) {
        set_focus(&mut self.ls.state, &mut self.ls.input, Some(e)).expect("set_focus");
        self.pump();
    }

    /// Super+`key`: whether the focused client got the key.
    fn chord_reaches_client(&mut self, key: u32) -> bool {
        let before = self.ls.probe.key_events.len();
        self.key(KEY_LEFTMETA, true);
        self.key(key, true);
        self.key(key, false);
        self.key(KEY_LEFTMETA, false);
        self.ls.probe.key_events[before..]
            .iter()
            .any(|(k, pressed)| *k == key && *pressed)
    }
}

#[test]
fn the_focused_window_takes_the_shortcuts_and_gives_them_back() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w1 = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
    ls.probe.toplevel(&qh, "W2", (400, 300), 0);
    ls.pump();
    let e: Vec<ElementId> = ls.state.elements().collect();
    let mut s = S { ls, t: 10_000 };
    s.focus(e[0]);
    assert!(
        !s.chord_reaches_client(KEY_ENTER),
        "Super+Enter is a shortcut"
    );
    s.focus(e[0]);

    let (name, _, _) =
        s.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "zwp_keyboard_shortcuts_inhibit_manager_v1")
            .cloned()
            .expect("the inhibit manager is offered");
    let manager: ZwpKeyboardShortcutsInhibitManagerV1 = s.ls.registry().bind(name, 1, &s.ls.qh, ());
    let seat = s.ls.probe.seat.clone().expect("seat");
    let surface = s.ls.probe.surfaces[w1].wl.clone();
    s.ls.probe.log.clear();
    let inhibitor = manager.inhibit_shortcuts(&surface, &seat, &s.ls.qh, ());
    s.pump();
    assert_eq!(s.ls.probe.log, ["active"], "focused: active at once");
    assert!(
        s.chord_reaches_client(KEY_ENTER),
        "inhibited: the window gets Super+Enter"
    );
    assert_eq!(
        s.ls.state.core().focus().target(),
        Some(e[0]),
        "and the focus did not move"
    );

    // Super+Tab stays yours, so a game cannot trap you in it (owner, 2026-11-01)
    assert!(
        !s.chord_reaches_client(KEY_TAB),
        "inhibited: Super+Tab does not reach the window"
    );
    assert_eq!(
        s.ls.state.core().focus().target(),
        Some(e[1]),
        "and it switches windows"
    );
    s.focus(e[0]);

    s.ls.probe.log.clear();
    s.focus(e[1]);
    assert_eq!(s.ls.probe.log, ["inactive"], "unfocused: inactive");
    assert!(
        !s.chord_reaches_client(KEY_ENTER),
        "another window: Super+Enter is a shortcut"
    );

    s.focus(e[0]);
    s.ls.probe.log.clear();
    inhibitor.destroy();
    s.pump();
    assert!(
        !s.chord_reaches_client(KEY_ENTER),
        "destroyed: the shortcuts are back"
    );
}
