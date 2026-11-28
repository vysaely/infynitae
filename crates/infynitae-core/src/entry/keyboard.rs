//! Keyboard intake and the actions bound to keys.

use super::*;

impl Core {
    /// K1/K3 keyboard press intake in Core (INV-INPUT-1, OBJ-25).
    pub(super) fn keyboard_press(
        &mut self,
        key: u32,
        modifiers: crate::input::bindings::Modifiers,
        mods_changed: bool,
        time_us: u64,
        navigation: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        // M12 Down->press repeat: if already held by Compositor, stay compositor-owned and consume (K1)
        if self.input.key_ownership.owner(key) == Some(Owner::Compositor) {
            out.push(Effect::KeyboardIntercept { key });
            return;
        }
        // Repeated press of already logically-down client key must not emit duplicate pressed (R03, K1/M12)
        if self.input.key_ownership.owner(key) == Some(Owner::Client) {
            return;
        }

        if self.is_capturing_key() && !keys::is_modifier_key(key) && !mods_changed {
            if key == keys::KEY_ESC && modifiers == bindings::Modifiers::default() {
                self.key_capture = Some(KeyCapture::Cancelled);
            } else if keys::key_name(key).is_some() {
                self.key_capture = Some(KeyCapture::Captured(keys::Chord {
                    key,
                    mods: modifiers,
                }));
            }
            self.input.key_ownership.press(key, Owner::Compositor);
            out.push(Effect::KeyboardIntercept { key });
            return;
        }

        let interaction_active = self.input.interaction.active();
        let matched = (!self.is_capturing_key())
            .then(|| self.resolve_key_binding(key, modifiers, interaction_active))
            .flatten()
            .filter(|a| {
                !self.shortcuts_inhibited
                    || matches!(
                        a,
                        KeyAction::SpawnLockScreen
                            | KeyAction::ForceCloseFocused
                            | KeyAction::FocusNext
                            | KeyAction::ToggleWindowList
                            | KeyAction::ToggleLauncher
                            | KeyAction::ControlMenu
                            | KeyAction::ToggleOverview
                            | KeyAction::SpawnSystemMonitor
                    )
            });

        // K3: keys that change XKB modifier or lock state are never compositor-owned (observe mode)
        if mods_changed {
            self.input.key_ownership.press(key, Owner::Client);
            if let Some(action) = matched {
                self.apply_key_action(action, navigation, out);
            }
            out.push(Effect::KeyboardForward {
                key,
                pressed: true,
                mods_changed: true,
                time_us,
            });
        } else if let Some(action) = matched {
            // K1: compositor-owned press is consumed
            self.input.key_ownership.press(key, Owner::Compositor);
            self.apply_key_action(action, navigation, out);
            out.push(Effect::KeyboardIntercept { key });
        } else {
            // Client-owned press: forwarded
            self.input.key_ownership.press(key, Owner::Client);
            out.push(Effect::KeyboardForward {
                key,
                pressed: true,
                mods_changed: false,
                time_us,
            });
        }
    }

    /// K2 keyboard release intake in Core (INV-INPUT-1, OBJ-25).
    pub(super) fn keyboard_release(
        &mut self,
        key: u32,
        mods_changed: bool,
        time_us: u64,
        out: &mut Vec<Effect>,
    ) {
        // Release follows recorded owner. Orphan release (None) is dropped and never forwarded.
        match self.input.key_ownership.release(key) {
            Some(Owner::Client) => out.push(Effect::KeyboardForward {
                key,
                pressed: false,
                mods_changed,
                time_us,
            }),
            Some(Owner::Compositor) => out.push(Effect::KeyboardIntercept { key }),
            None => {}
        }
    }

    /// `proof`: minted by `physical_input` for a press bound to fit or restore (INV-CAMERA-1).
    /// The effects are appended to `out` (the caller's reusable buffer, C14.3).
    pub(super) fn apply_key_action(
        &mut self,
        action: KeyAction,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        let fx = match action {
            crate::input::keys::KeyAction::FocusNext => {
                let (fx, attention) = self.focus_next();
                out.extend(fx);
                if let Some(target) = self.focus.target() {
                    self.show_chosen(target, out);
                }
                // a window that asked for attention is brought into view as a focus jump does
                // (Super+1..9), whatever FOCUS_CYCLE_NAVIGATE says (owner, 2026-10-31)
                if (self.focus_cycle_navigate || attention)
                    && let Some(p) = proof
                {
                    self.recenter(p, false, true, out);
                }
                // what is under the pointer now is not a crossing: the camera moved, not the hand
                self.hover_seen = self.hovered_window();
                self.keyed_focus = self.focus.target();
                return;
            }
            // M22 cancel: no revert, owned buttons stay owned; the interaction terminates as at
            // its initiator's release (INV-SCALE-3, INV-INPUT-5, M10)
            crate::input::keys::KeyAction::Cancel => {
                if self.input.interaction.active() {
                    self.end_interaction()
                } else {
                    Vec::new()
                }
            }
            KeyAction::FitViewport => {
                if let Some(p) = proof {
                    self.fit_viewport(p, out);
                }
                return;
            }
            KeyAction::ViewportRestore => {
                if let Some(p) = proof {
                    self.viewport_restore(p, out);
                }
                return;
            }
            KeyAction::FitToggle => {
                if let Some(p) = proof {
                    self.fit_toggle(p, out);
                }
                return;
            }
            // Super+Space / Super+Shift+Space: the window under the pointer is focused and raised
            // first, so the recenter is on it (owner, 2026-11-15); off a window, the focused one
            KeyAction::Recenter | KeyAction::RecenterKeepZoom => {
                if let Some(p) = proof {
                    if let Some(element) = self.hovered_window() {
                        self.raise_and_focus(element, out);
                    }
                    self.recenter(p, action == KeyAction::RecenterKeepZoom, false, out);
                    // what is under the pointer now is not a crossing: the camera moved, not the hand
                    self.hover_seen = self.hovered_window();
                }
                return;
            }
            KeyAction::FocusDirection(direction) => {
                self.focus_direction(direction, proof, out);
                return;
            }
            KeyAction::FocusSlot(slot) => {
                self.focus_slot(slot, proof, out);
                return;
            }
            KeyAction::FocusClusterSlot(slot) => {
                self.focus_cluster_slot(slot, proof, out);
                return;
            }
            KeyAction::ArrangeCluster => {
                if let Some(id) = self.focus.target() {
                    self.arrange_cluster(id, crate::arrange::ClusterLayout::Grid, proof, out);
                }
                return;
            }
            KeyAction::ArrangeClusterByApp => {
                if let Some(id) = self.focus.target() {
                    self.arrange_cluster(id, crate::arrange::ClusterLayout::ByApp, proof, out);
                }
                return;
            }
            KeyAction::ToggleFullscreen => self.toggle_fullscreen(),
            KeyAction::ToggleKeepOnTop => {
                if let Some(id) = self.focus.target() {
                    let on = !self.stage.is_above(id);
                    self.set_on_top(id, on);
                    if self.stage.is_above(id) == on {
                        out.push(Effect::KeptOnTop { element: id, on });
                    }
                }
                return;
            }
            KeyAction::WindowMenu => {
                if let Some(id) = self
                    .hovered_window()
                    .filter(|_| self.keyed_choice().is_none())
                {
                    self.raise_and_focus(id, out);
                    if let PointerState::Located { p, .. } = self.pointer {
                        self.window_menu = Some((id, p));
                    }
                } else if let Some(id) = self.focus.target() {
                    self.show_window_menu(id, crate::geom::LocalPoint { x: 0, y: 0 });
                }
                return;
            }
            KeyAction::ForceCloseFocused => self
                .close_target()
                .map(Effect::ForceClose)
                .into_iter()
                .collect(),
            // a request, not a destruction: the client closes (or asks the user first)
            KeyAction::CloseFocused => self
                .close_target()
                .map(Effect::CloseRequest)
                .into_iter()
                .collect(),
            KeyAction::ToggleWindowList => vec![Effect::ToggleWindowList],
            KeyAction::ToggleLauncher => {
                self.key_opened = true;
                vec![Effect::ToggleLauncher]
            }
            KeyAction::SpawnTerminal => {
                self.key_opened = true;
                vec![Effect::SpawnTerminal]
            }
            KeyAction::SpawnFileManager => {
                self.key_opened = true;
                vec![Effect::SpawnFileManager]
            }
            KeyAction::SpawnSystemMonitor => {
                self.key_opened = true;
                vec![Effect::SpawnSystemMonitor]
            }
            KeyAction::SpawnScreenshot => vec![Effect::SpawnScreenshot],
            KeyAction::SpawnRegionScreenshot => vec![Effect::SpawnRegionScreenshot],
            KeyAction::ToggleOverview => {
                if let Some(p) = proof {
                    self.overview_toggle(p, out);
                } else {
                    out.push(Effect::ToggleOverview);
                }
                return;
            }
            KeyAction::ReloadShell => vec![Effect::ReloadShell],
            KeyAction::SpawnLockScreen => vec![Effect::SpawnLockScreen],
            KeyAction::ClipboardHistory => vec![Effect::SpawnClipboardHistory],
            KeyAction::ControlMenu => vec![Effect::SpawnControlMenu],
            KeyAction::ToggleShortcuts => vec![Effect::ToggleShortcuts],
            KeyAction::TogglePrivacy => vec![Effect::TogglePrivacy],
            KeyAction::ToggleIdleInhibit => vec![Effect::ToggleIdleInhibit],
            KeyAction::ToggleTearing => vec![Effect::ToggleTearing],
            KeyAction::SaveReplay => vec![Effect::SaveReplay],
            KeyAction::ToggleRecording => vec![Effect::ToggleRecording],
            KeyAction::VolumeUp => vec![Effect::VolumeUp],
            KeyAction::VolumeDown => vec![Effect::VolumeDown],
            KeyAction::VolumeMute => vec![Effect::VolumeMute],
            KeyAction::BrightnessUp => vec![Effect::BrightnessUp],
            KeyAction::BrightnessDown => vec![Effect::BrightnessDown],
            KeyAction::MediaPlayPause => vec![Effect::MediaPlayPause],
            KeyAction::MediaNext => vec![Effect::MediaNext],
            KeyAction::MediaPrev => vec![Effect::MediaPrev],
            KeyAction::Exec(i) => vec![Effect::Exec(i)],
            crate::input::keys::KeyAction::Noop => Vec::new(),
        };
        out.extend(fx);
    }
}

#[cfg(test)]
mod tests {
    use crate::Core;
    use crate::entry::tests::*;
    use crate::input::bindings::Modifiers;
    use crate::navigation::PersistedCameras;
    use crate::pointer::PointerState;
    use crate::protocol::Effect;

    /// While the focused window holds a shortcuts inhibitor, a bound chord goes to it as a key;
    /// Super+Shift+L still locks and Super+Shift+Delete still ends it; given back, the chord is the
    /// compositor's again.
    #[test]
    fn inhibited_shortcuts_go_to_the_window_except_lock() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let w = map_windows(&mut c, a, 1)[0];
        c.set_focus(&mut i, Some(w)).unwrap();
        let logo = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let held = std::cell::Cell::new(logo);
        let mut time = 1000;
        let mut press = |c: &mut Core, key, mods_changed| {
            time += 1000;
            let fx = c
                .physical_input_vec(
                    &mut i,
                    crate::PhysicalEvent::KeyboardKey {
                        key,
                        pressed: true,
                        modifiers: held.get(),
                        mods_changed,
                        time_us: time,
                    },
                )
                .unwrap();
            time += 1000;
            let _ = c.physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key,
                    pressed: false,
                    modifiers: held.get(),
                    mods_changed: false,
                    time_us: time,
                },
            );
            fx
        };
        use crate::input::keys::{KEY_ESC, KEY_L, KEY_LEFTMETA, KEY_Q};
        let _ = press(&mut c, KEY_LEFTMETA, true);
        let forwarded = |fx: &[Effect], key| {
            fx.iter().any(
                |e| matches!(e, Effect::KeyboardForward { key: k, pressed: true, .. } if *k == key),
            )
        };

        assert!(
            !forwarded(&press(&mut c, KEY_Q, false), KEY_Q),
            "Super+Q is a shortcut"
        );
        c.set_shortcuts_inhibited(true);
        let fx = press(&mut c, KEY_Q, false);
        assert!(
            forwarded(&fx, KEY_Q),
            "inhibited: Super+Q reaches the window: {fx:?}"
        );
        assert!(
            !fx.contains(&Effect::SpawnTerminal),
            "and starts nothing: {fx:?}"
        );
        held.set(Modifiers {
            shift: true,
            ..logo
        });
        let fx = press(&mut c, KEY_ESC, false);
        assert!(
            fx.contains(&Effect::SpawnSystemMonitor),
            "a stuck fullscreen game can't hide the system monitor: {fx:?}"
        );
        let fx = press(&mut c, KEY_L, false);
        held.set(logo);
        assert!(
            fx.contains(&Effect::SpawnLockScreen),
            "Super+Shift+L still locks: {fx:?}"
        );
        assert!(!forwarded(&fx, KEY_L), "and the window does not get it");
        let fx = press(&mut c, crate::input::keys::KEY_TAB, false);
        assert!(
            !forwarded(&fx, crate::input::keys::KEY_TAB),
            "Super+Tab always leaves the window: {fx:?}"
        );
        for key in [
            crate::input::keys::KEY_W,
            crate::input::keys::KEY_L,
            crate::input::keys::KEY_A,
        ] {
            let fx = press(&mut c, key, false);
            assert!(!forwarded(&fx, key), "the edge stays reachable: {fx:?}");
        }
        held.set(Modifiers {
            shift: true,
            ..logo
        });
        let fx = press(&mut c, crate::input::keys::KEY_DELETE, false);
        held.set(logo);
        assert!(
            fx.contains(&Effect::ForceClose(w)),
            "Super+Shift+Delete ends the window that holds the shortcuts: {fx:?}"
        );
        assert!(!forwarded(&fx, crate::input::keys::KEY_DELETE));
        c.set_shortcuts_inhibited(false);
        assert!(
            !forwarded(&press(&mut c, KEY_Q, false), KEY_Q),
            "given back"
        );
    }

    /// Super+arrow with the pointer left over empty canvas: Super+Shift+C and Super+Shift+Delete
    /// act on the window the keyboard went to. A mouse motion gives the pointer the priority
    /// again: over empty canvas they act on nothing, as before.
    #[test]
    fn a_window_chosen_by_key_is_closed_by_key_with_strict_focus() {
        use crate::input::keys::{KEY_C, KEY_DELETE, KEY_RIGHT};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = keyboard_only_strict(&mut c, &mut i, &mut t);

        super_chord(&mut c, &mut i, a, KEY_RIGHT, false);
        assert_eq!(c.focus().target(), Some(ws[1]));
        assert_eq!(
            c.hovered_window(),
            None,
            "the pointer is still over empty canvas"
        );
        let fx = super_chord(&mut c, &mut i, a, KEY_C, true);
        assert!(fx.contains(&Effect::CloseRequest(ws[1])), "{fx:?}");
        let fx = super_chord(&mut c, &mut i, a, KEY_DELETE, true);
        assert!(fx.contains(&Effect::ForceClose(ws[1])), "{fx:?}");

        mouse_move(&mut c, &mut i, (1.0, 1.0));
        c.render_list(a).unwrap();
        assert_eq!(
            c.focus().target(),
            Some(ws[1]),
            "no crossing: the focus stays"
        );
        let fx = super_chord(&mut c, &mut i, a, KEY_C, true);
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::CloseRequest(_))),
            "after a mouse motion over empty canvas nothing closes: {fx:?}"
        );
        let fx = super_chord(&mut c, &mut i, a, KEY_DELETE, true);
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::ForceClose(_))),
            "{fx:?}"
        );
    }

    /// Super+Tab and Super+1..9 choose by key as Super+arrow does.
    #[test]
    fn super_tab_and_super_slot_choose_the_close_target_by_key() {
        use crate::input::keys::{KEY_1, KEY_C};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = keyboard_only_strict(&mut c, &mut i, &mut t);
        c.set_focus_cycle_navigate(true);
        c.set_focus(&mut i, Some(ws[1])).unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();

        super_tab(&mut c, &mut i);
        c.render_list(a).unwrap();
        assert_eq!(c.focus().target(), Some(ws[1]));
        assert_eq!(c.hovered_window(), None);
        let fx = super_chord(&mut c, &mut i, a, KEY_C, true);
        assert!(
            fx.contains(&Effect::CloseRequest(ws[1])),
            "Super+Tab: {fx:?}"
        );

        super_chord(&mut c, &mut i, a, KEY_1, false);
        assert_eq!(c.focus().target(), Some(ws[0]));
        assert_eq!(c.hovered_window(), None);
        let fx = super_chord(&mut c, &mut i, a, KEY_C, true);
        assert!(fx.contains(&Effect::CloseRequest(ws[0])), "Super+1: {fx:?}");
    }

    /// The window menu by key shows for the window the keyboard chose, not for one the camera
    /// brought under the still pointer; after a mouse motion it is the hovered window again.
    #[test]
    fn the_window_menu_by_key_follows_the_keyboard_choice() {
        use crate::input::keys::KEY_3;
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = keyboard_only_strict(&mut c, &mut i, &mut t);
        // ws[1] just right of ws[2]: centred on ws[2], the camera brings ws[1] under the pointer
        let mut fx = Vec::new();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: -5400, y: 0 }, &mut fx)
            .unwrap();
        let (w, h) = c.output(a).unwrap().logical_extent();
        c.pointer = PointerState::Located {
            output: a,
            p: (w / 2.0 + 500.0, h / 2.0),
        };
        c.render_list(a).unwrap();
        c.hover_seen = c.hovered_window();

        super_chord(&mut c, &mut i, a, KEY_3, false);
        assert_eq!(c.focus().target(), Some(ws[2]));
        assert_eq!(
            c.hovered_window(),
            Some(ws[1]),
            "the camera moved, not the hand"
        );
        c.apply_key_action(crate::input::keys::KeyAction::WindowMenu, None, &mut fx);
        assert_eq!(c.window_menu().map(|(id, _)| id), Some(ws[2]));
        assert_eq!(
            c.focus().target(),
            Some(ws[2]),
            "the focus stays where the keys put it"
        );

        mouse_move(&mut c, &mut i, (1.0, 1.0));
        c.render_list(a).unwrap();
        c.apply_key_action(crate::input::keys::KeyAction::WindowMenu, None, &mut fx);
        assert_eq!(c.window_menu().map(|(id, _)| id), Some(ws[1]));
    }

    /// A window opened after the launcher was opened by key (Super+W, then Enter in it) is chosen
    /// by key: Super+Shift+C closes it with the pointer still over empty canvas.
    #[test]
    fn a_window_opened_from_the_launcher_by_key_is_closed_by_key() {
        use crate::input::keys::{KEY_C, KEY_W};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = keyboard_only_strict(&mut c, &mut i, &mut t);
        let fx = super_chord(&mut c, &mut i, a, KEY_W, false);
        assert!(fx.contains(&Effect::ToggleLauncher), "{fx:?}");
        let opened = map_window(&mut c, 4);
        let mut fx = Vec::new();
        c.place_by_user(opened, crate::geom::CanvasPoint { x: 600, y: 600 }, &mut fx)
            .unwrap();
        c.set_focus(&mut i, Some(opened)).unwrap();
        c.render_list(a).unwrap();
        assert_eq!(c.hovered_window(), None);
        let fx = super_chord(&mut c, &mut i, a, KEY_C, true);
        assert!(fx.contains(&Effect::CloseRequest(opened)), "{fx:?}");
    }

    /// Attention first (owner, 2026-10-31): a window that opened without the focus is where
    /// Super+Tab goes first, its mark cleared and the camera brought to it (even with
    /// FOCUS_CYCLE_NAVIGATE off); the next Super+Tab goes back to the window the user was in.
    #[test]
    fn super_tab_goes_to_the_window_asking_for_attention_first_then_back() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 3);
        let [w1, w2, w3] = [ws[0], ws[1], ws[2]];
        c.set_focus_cycle_navigate(false);
        // w3 far to the right, out of view
        let mut fx = Vec::new();
        c.place_by_user(w3, crate::geom::CanvasPoint { x: 6000, y: 0 }, &mut fx)
            .unwrap();
        for w in [w2, w1] {
            c.set_focus(&mut i, Some(w)).unwrap();
        }
        c.opened_without_focus(w3);
        assert_eq!(c.focus().attention(), &[w3]);
        let cam = camera_of(&c, a);
        let before = c.cameras().center(cam);

        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(w3), "attention first");
        assert!(c.focus().attention().is_empty(), "its mark is cleared");
        let at = c.cameras().center(cam).unwrap();
        assert_ne!(Some(at), before, "the camera went to it");
        assert!(at.x > 5000 << 16, "centred on w3: {at:?}");

        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(w1), "back to where the user was");
        assert_eq!(
            c.cameras().center(cam),
            Some(at),
            "a plain flip keeps the camera (FOCUS_CYCLE_NAVIGATE off)"
        );
        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(w3), "then the flip as usual");

        // an unmapped window is never marked
        let _ = c.toplevel_destroyed(w2);
        c.opened_without_focus(w2);
        assert!(c.focus().attention().is_empty());
    }

    /// Super+Space focuses and raises the window under the pointer and centres on it; off any
    /// window it centres the focused one, which keeps focus (owner, 2026-11-15).
    #[test]
    fn super_space_focuses_the_window_under_the_pointer() {
        use crate::input::keys::{KEY_LEFTMETA, KEY_SPACE};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let ws = window_under_pointer(&mut c, &mut i, &mut t);
        let super_space = |c: &mut Core, i: &mut crate::InputPort| {
            press(c, i, KEY_LEFTMETA, true, true);
            press(c, i, KEY_SPACE, true, true);
            press(c, i, KEY_SPACE, false, true);
            press(c, i, KEY_LEFTMETA, false, false);
        };
        // the far window has focus; the pointer is over ws[0]
        c.set_focus(&mut i, Some(ws[1])).unwrap();
        super_space(&mut c, &mut i);
        assert_eq!(
            c.focus().target(),
            Some(ws[0]),
            "the hovered window takes focus"
        );
        let a = c.outputs.first().unwrap().id;
        let cam = camera_of(&c, a);
        let anchor = c.stage().anchor(ws[0]).unwrap();
        let size = c.stage().tree(ws[0]).unwrap().geometry.size;
        let centre = c.cameras().center(cam).unwrap();
        assert_eq!(
            (centre.x >> 16, centre.y >> 16),
            (
                anchor.x + i64::from(size.w) / 2,
                anchor.y + i64::from(size.h) / 2
            ),
            "the camera is on the hovered window"
        );

        // off every window: the focused window keeps focus
        c.set_focus(&mut i, Some(ws[1])).unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (1.0, 1.0),
        };
        if c.hovered_window().is_none() {
            super_space(&mut c, &mut i);
            assert_eq!(c.focus().target(), Some(ws[1]));
        }
    }

    #[test]
    fn super_slash_and_f1_emit_toggle_shortcuts_unless_inhibited() {
        use crate::input::keys::{KEY_F1, KEY_LEFTMETA, KEY_SLASH};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let w = map_windows(&mut c, a, 1)[0];
        c.set_focus(&mut i, Some(w)).unwrap();

        let logo = Modifiers {
            logo: true,
            ..Default::default()
        };
        let mut time = 1000;
        let mut press_key = |c: &mut Core, key, modifiers, mods_changed| {
            time += 1000;
            let fx = c
                .physical_input_vec(
                    &mut i,
                    crate::PhysicalEvent::KeyboardKey {
                        key,
                        pressed: true,
                        modifiers,
                        mods_changed,
                        time_us: time,
                    },
                )
                .unwrap();
            time += 1000;
            let _ = c.physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key,
                    pressed: false,
                    modifiers,
                    mods_changed: false,
                    time_us: time,
                },
            );
            fx
        };

        let _ = press_key(&mut c, KEY_LEFTMETA, logo, true);
        for key in [KEY_SLASH, KEY_F1] {
            let fx = press_key(&mut c, key, logo, false);
            assert!(
                fx.contains(&Effect::ToggleShortcuts),
                "key {key} emits ToggleShortcuts: {fx:?}"
            );
            assert!(
                fx.contains(&Effect::KeyboardIntercept { key }),
                "key {key} is intercepted: {fx:?}"
            );
        }

        c.set_shortcuts_inhibited(true);
        let fx = press_key(&mut c, KEY_SLASH, logo, false);
        assert!(
            !fx.contains(&Effect::ToggleShortcuts),
            "inhibited app receives Super+/: {fx:?}"
        );
        assert!(fx.iter().any(|e| matches!(
            e,
            Effect::KeyboardForward {
                key: KEY_SLASH,
                pressed: true,
                ..
            }
        )));
    }
}
