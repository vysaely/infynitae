//! Tearing for games: a fullscreen window's frames shown the moment they are ready instead of at
//! the screen's next refresh (an async page flip). The lowest latency, at the cost of a visible
//! tear line.
//!
//! A window tears while it is fullscreen and shown directly (direct scanout) and either its app
//! asked (`wp_tearing_control_v1` hint `async`: Vulkan and OpenGL games with vsync off, through
//! Mesa) or the user turned it on for that window (Super+Shift+O or Super+Shift+T: X11 games, which cannot ask).
//! `TEARING="off"` turns it off everywhere. Never while the session is locked.

use infynitae_core::output::OutputId;
use infynitae_core::stage::ElementId;
use smithay::reexports::wayland_protocols::wp::tearing_control::v1::server::{
    wp_tearing_control_manager_v1::{self, WpTearingControlManagerV1},
    wp_tearing_control_v1::{self, PresentationHint, WpTearingControlV1},
};
use smithay::reexports::wayland_server::backend::GlobalId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

use crate::WaylandState;

pub struct Tearing {
    _global: GlobalId,
    /// `TEARING` (on by default; "off" never tears).
    pub(crate) enabled: bool,
    /// The surfaces with a tearing control object, and whether their hint is `async`.
    hints: Vec<(WlSurface, bool)>,
    /// The windows the user turned tearing on for (Super+Shift+O).
    forced: Vec<ElementId>,
}

impl Tearing {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            _global: dh.create_global::<WaylandState, WpTearingControlManagerV1, ()>(1, ()),
            enabled: true,
            hints: Vec::new(),
            forced: Vec::new(),
        }
    }
}

impl WaylandState {
    /// Whether `output` tears now: its fullscreen window asked for it or was given it, tearing
    /// is on and the session unlocked. The backend flips async only for a frame scanned out.
    pub fn tears(&self, output: OutputId) -> bool {
        let t = &self.tearing;
        if !t.enabled || self.is_locked() {
            return false;
        }
        let Some(window) = self.core.presentation(output) else {
            return false;
        };
        t.forced.contains(&window)
            || self
                .window_surface(window)
                .is_some_and(|s| t.hints.iter().any(|(h, on)| *on && *h == s))
    }

    /// Super+Shift+O (or Super+Shift+T): tearing on or off for the focused window (it tears while fullscreen), with
    /// a notification saying which.
    pub fn toggle_tearing_focused(&mut self) {
        let Some(focused) = self.core.focus().target() else {
            return;
        };
        let on = self.toggle_tearing(focused);
        println!(
            "[infynitae] Tearing {} for window {focused:?}",
            if on { "on" } else { "off" }
        );
        self.spawn_action(if on {
            "notify-tearing-on"
        } else {
            "notify-tearing-off"
        });
    }

    /// Tearing on or off for `window`; whether it is on now.
    pub fn toggle_tearing(&mut self, window: ElementId) -> bool {
        let mapped = self.core.stage().mapped_elements();
        let forced = &mut self.tearing.forced;
        forced.retain(|e| mapped.contains(e));
        if let Some(i) = forced.iter().position(|e| *e == window) {
            forced.remove(i);
            false
        } else {
            forced.push(window);
            true
        }
    }

    /// `TEARING`: whether windows may tear at all.
    pub fn set_tearing_enabled(&mut self, enabled: bool) {
        self.tearing.enabled = enabled;
    }
}

impl GlobalDispatch<WpTearingControlManagerV1, ()> for WaylandState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<WpTearingControlManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpTearingControlManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &WpTearingControlManagerV1,
        request: wp_tearing_control_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wp_tearing_control_manager_v1::Request::GetTearingControl { id, surface } = request {
            let hints = &mut state.tearing.hints;
            hints.retain(|(s, _)| s.is_alive());
            if hints.iter().any(|(s, _)| *s == surface) {
                resource.post_error(
                    wp_tearing_control_manager_v1::Error::TearingControlExists,
                    "the surface already has a tearing control object",
                );
                return;
            }
            hints.push((surface.clone(), false));
            data_init.init(id, surface);
        }
    }
}

impl Dispatch<WpTearingControlV1, WlSurface> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &WpTearingControlV1,
        request: wp_tearing_control_v1::Request,
        surface: &WlSurface,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_tearing_control_v1::Request::SetPresentationHint { hint } => {
                let on = matches!(hint, WEnum::Value(PresentationHint::Async));
                if let Some(entry) = state.tearing.hints.iter_mut().find(|(s, _)| s == surface) {
                    entry.1 = on;
                }
            }
            // destroying the object goes back to vsync
            wp_tearing_control_v1::Request::Destroy => {
                state.tearing.hints.retain(|(s, _)| s != surface);
            }
            _ => {}
        }
    }
}
