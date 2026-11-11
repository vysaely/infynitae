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
use crate::log_line;

pub struct Tearing {
    _global: GlobalId,
    /// `TEARING`: on (a window that asks, or after Super+Shift+O), "always" (every window that has the
    /// screen to itself, without asking: the default), "off" never.
    pub(crate) enabled: bool,
    pub(crate) always: bool,
    /// The surfaces with a tearing control object, and their current and pending hints.
    hints: Vec<TearingHint>,
    /// The windows the user turned tearing on for (Super+Shift+O).
    forced: Vec<ElementId>,
    /// The windows the user turned tearing off for although their app asks (Super+Shift+O on a
    /// game that tears by itself; owner, 2026-11-05: the key seemed to do nothing in CS2).
    refused: Vec<ElementId>,
}

#[derive(Debug)]
struct TearingHint {
    surface: WlSurface,
    /// Currently committed presentation hint: `true` for `async`, `false` for `vsync`.
    current: bool,
    /// Pending presentation hint: applied on the surface's next `commit`.
    pending: bool,
    /// the control object was destroyed: back to vsync on the next commit, then the entry goes
    destroyed: bool,
}

impl Tearing {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            _global: dh.create_global::<WaylandState, WpTearingControlManagerV1, ()>(1, ()),
            enabled: true,
            always: true,
            hints: Vec::new(),
            forced: Vec::new(),
            refused: Vec::new(),
        }
    }

    /// Applies the pending presentation hint for `surface` on `wl_surface.commit`.
    pub(crate) fn commit(&mut self, surface: &WlSurface) {
        if let Some(entry) = self.hints.iter_mut().find(|h| &h.surface == surface) {
            entry.current = entry.pending;
        }
        self.hints
            .retain(|h| !(h.destroyed && &h.surface == surface));
    }
}

impl WaylandState {
    /// Applies the pending presentation hint for `surface` on commit.
    pub(crate) fn tearing_commit(&mut self, surface: &WlSurface) {
        self.tearing.commit(surface);
    }

    /// Cleans up tearing hints when `surface` is destroyed.
    pub(crate) fn tearing_surface_destroyed(&mut self, surface: &WlSurface) {
        self.tearing.hints.retain(|h| &h.surface != surface);
    }

    /// Whether `output` tears now: its fullscreen window asked for it or was given it, tearing
    /// is on and the session unlocked. The backend flips async only for a frame scanned out.
    pub fn tears(&self, output: OutputId) -> bool {
        let t = &self.tearing;
        if !t.enabled || self.is_locked() {
            return false;
        }
        let Some(window) = self.exclusive_window(output) else {
            return false;
        };
        if t.refused.contains(&window) {
            return false;
        }
        t.always
            || t.forced.contains(&window)
            || self
                .window_surface(window)
                .is_some_and(|s| t.hints.iter().any(|h| h.current && h.surface == s))
    }

    /// Super+Shift+O (or Super+Shift+T): tearing on or off for the focused window (it tears while fullscreen), with
    /// a notification saying which.
    pub fn toggle_tearing_focused(&mut self) {
        let Some(focused) = self.core.focus().target() else {
            return;
        };
        let on = self.toggle_tearing(focused);
        log_line!(
            "[infynitae] Tearing {} for window {focused:?}",
            if on { "on" } else { "off" }
        );
        self.spawn_action(if on {
            "notify-tearing-on"
        } else {
            "notify-tearing-off"
        });
    }

    /// Tearing on or off for `window`; whether it is on now. Off holds even when its app asks.
    pub fn toggle_tearing(&mut self, window: ElementId) -> bool {
        let mapped = self.core.stage().mapped_elements();
        let asks = self.window_surface(window).is_some_and(|s| {
            self.tearing
                .hints
                .iter()
                .any(|h| h.current && h.surface == s)
        });
        let t = &mut self.tearing;
        t.forced.retain(|e| mapped.contains(e));
        t.refused.retain(|e| mapped.contains(e));
        let was_on =
            !t.refused.contains(&window) && (t.always || asks || t.forced.contains(&window));
        t.forced.retain(|e| *e != window);
        t.refused.retain(|e| *e != window);
        if was_on {
            t.refused.push(window);
        } else {
            t.forced.push(window);
        }
        !was_on
    }

    /// `TEARING`: whether windows may tear at all.
    pub fn set_tearing_enabled(&mut self, enabled: bool) {
        self.tearing.enabled = enabled;
    }

    /// `TEARING="always"`: every window that has the screen to itself tears, asked or not.
    pub fn set_tearing_always(&mut self, always: bool) {
        self.tearing.always = always;
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
            hints.retain(|h| h.surface.is_alive());
            // a destroyed object waiting for its commit does not count
            hints.retain(|h| !(h.destroyed && h.surface == surface));
            if hints.iter().any(|h| h.surface == surface) {
                resource.post_error(
                    wp_tearing_control_manager_v1::Error::TearingControlExists,
                    "the surface already has a tearing control object",
                );
                return;
            }
            hints.push(TearingHint {
                surface: surface.clone(),
                current: false,
                pending: false,
                destroyed: false,
            });
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
                if let Some(entry) = state
                    .tearing
                    .hints
                    .iter_mut()
                    .find(|h| &h.surface == surface)
                {
                    entry.pending = on;
                }
            }
            // destroying the object goes back to vsync on the next commit (the protocol)
            wp_tearing_control_v1::Request::Destroy => {
                if let Some(entry) = state
                    .tearing
                    .hints
                    .iter_mut()
                    .find(|h| &h.surface == surface)
                {
                    entry.pending = false;
                    entry.destroyed = true;
                }
            }
            _ => {}
        }
    }
}
