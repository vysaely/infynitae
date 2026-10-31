//! `zwlr_foreign_toplevel_manager_v1`: the window list task bars use (waybar's taskbar, rofi's
//! window mode), with the requests to activate, close, maximize and fullscreen a window.
//!
//! A window is listed while it is mapped (the same set as `ext_foreign_toplevel_list_v1`). Its
//! title, app id and state are compared with what each handle was last told whenever Core's
//! effects are applied, and only a change is sent.
//!
//! Activating goes through the same path as `xdg_activation` (`Core::activation_request`): it
//! focuses the window and moves the camera only when the request is the user's, that is when the
//! bar asking holds the pointer or the keyboard focus (the user just clicked it); otherwise the
//! window only asks for attention. A program in the background cannot move the focus.

use std::collections::HashMap;

use infynitae_core::protocol::ToplevelRequest;
use infynitae_core::stage::ElementId;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::{ClientId, GlobalId};
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;
use wayland_protocols_wlr::foreign_toplevel::v1::server::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

use crate::WaylandState;

/// What a handle was last told about its window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Told {
    title: String,
    app_id: String,
    states: Vec<u32>,
}

/// The managers bound by clients and the handles of every listed window.
pub struct WlrToplevelsState {
    #[expect(
        dead_code,
        reason = "owns the zwlr_foreign_toplevel_manager_v1 global for the state's lifetime"
    )]
    global: GlobalId,
    managers: Vec<ZwlrForeignToplevelManagerV1>,
    handles: HashMap<ElementId, (Vec<ZwlrForeignToplevelHandleV1>, Told)>,
}

impl WlrToplevelsState {
    pub fn new(dh: &DisplayHandle, gate: crate::trust::Gate) -> Self {
        Self {
            global: dh
                .create_global::<WaylandState, ZwlrForeignToplevelManagerV1, crate::trust::Gate>(
                    3, gate,
                ),
            managers: Vec::new(),
            handles: HashMap::new(),
        }
    }
}

impl WaylandState {
    /// The window's title, app id and protocol states now.
    fn wlr_toplevel_now(&self, id: ElementId) -> Told {
        let mut told = Told::default();
        let Some(toplevel) = self.window(id).and_then(|w| w.toplevel()) else {
            return told;
        };
        smithay::wayland::compositor::with_states(toplevel.wl_surface(), |states| {
            if let Some(d) = states.data_map.get::<XdgToplevelSurfaceData>() {
                let d = d.lock().unwrap();
                told.title = d.title.clone().unwrap_or_default();
                told.app_id = d.app_id.clone().unwrap_or_default();
            }
        });
        let current = toplevel.current_state().states;
        use zwlr_foreign_toplevel_handle_v1::State;
        if current.contains(xdg_toplevel::State::Maximized) {
            told.states.push(State::Maximized as u32);
        }
        if self.core.focus().target() == Some(id) {
            told.states.push(State::Activated as u32);
        }
        if current.contains(xdg_toplevel::State::Fullscreen) {
            told.states.push(State::Fullscreen as u32);
        }
        told
    }

    /// Creates the handle of `id` for one manager and tells it everything.
    fn wlr_toplevel_announce(
        &self,
        manager: &ZwlrForeignToplevelManagerV1,
        id: ElementId,
        now: &Told,
    ) -> Option<ZwlrForeignToplevelHandleV1> {
        let client = manager.client()?;
        let handle = client
            .create_resource::<ZwlrForeignToplevelHandleV1, ElementId, WaylandState>(
                &self.dh,
                manager.version(),
                id,
            )
            .ok()?;
        manager.toplevel(&handle);
        handle.title(now.title.clone());
        handle.app_id(now.app_id.clone());
        // a canvas window is on no screen in particular: it is on every screen the bar is on.
        // The protocol's event, sent as an `Event` (the name `output_enter` is kept for Smithay's
        // `Window::output_enter`, which the architecture rejects: OB-26)
        for h in &self.outputs {
            for output in h.output.client_outputs(&client) {
                handle
                    .send_event(zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { output })
                    .ok();
            }
        }
        handle.state(states_bytes(&now.states));
        handle.done();
        Some(handle)
    }

    /// Brings every handle up to date: new windows announced, closed ones closed, changes sent.
    pub(crate) fn sync_wlr_toplevels(&mut self) {
        if self.wlr_toplevels.managers.is_empty() && self.wlr_toplevels.handles.is_empty() {
            return;
        }
        // in the order the windows were opened (ids only grow): the bar lists them so
        let mut listed: Vec<ElementId> = self.foreign_toplevels.keys().copied().collect();
        listed.sort();
        let gone: Vec<ElementId> = self
            .wlr_toplevels
            .handles
            .keys()
            .filter(|id| !listed.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some((handles, _)) = self.wlr_toplevels.handles.remove(&id) {
                for h in handles {
                    h.closed();
                }
            }
        }
        for id in listed {
            let now = self.wlr_toplevel_now(id);
            match self.wlr_toplevels.handles.get_mut(&id) {
                Some((handles, told)) => {
                    if *told == now {
                        continue;
                    }
                    handles.retain(|h| h.is_alive());
                    for h in handles.iter() {
                        if told.title != now.title {
                            h.title(now.title.clone());
                        }
                        if told.app_id != now.app_id {
                            h.app_id(now.app_id.clone());
                        }
                        if told.states != now.states {
                            h.state(states_bytes(&now.states));
                        }
                        h.done();
                    }
                    *told = now;
                }
                None => {
                    let managers = self.wlr_toplevels.managers.clone();
                    let handles = managers
                        .iter()
                        .filter_map(|m| self.wlr_toplevel_announce(m, id, &now))
                        .collect();
                    self.wlr_toplevels.handles.insert(id, (handles, now));
                }
            }
        }
    }

    /// `activate`: the user's when the asking client holds the pointer or the keyboard focus.
    fn wlr_toplevel_activate(&mut self, client: &Client, id: ElementId) {
        let holds =
            |s: Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>| {
                s.and_then(|s| s.client())
                    .is_some_and(|c| c.id() == client.id())
            };
        let pointer = self.pointer_handle().current_focus();
        let keyboard = self.seat.get_keyboard().and_then(|k| k.current_focus());
        let validated = holds(pointer) || holds(keyboard);
        if let Ok(effects) = self.core.activation_request(id, validated) {
            self.apply(&effects);
        }
    }

    fn wlr_toplevel_request(&mut self, id: ElementId, request: ToplevelRequest) {
        if let Some(t) = self.window(id).and_then(|w| w.toplevel()).cloned() {
            self.toplevel_request(&t, request);
        }
    }
}

/// The `state` array: each state a native-endian u32.
fn states_bytes(states: &[u32]) -> Vec<u8> {
    states.iter().flat_map(|s| s.to_ne_bytes()).collect()
}

impl GlobalDispatch<ZwlrForeignToplevelManagerV1, crate::trust::Gate> for WaylandState {
    fn can_view(client: Client, gate: &crate::trust::Gate) -> bool {
        gate.allows(&client)
    }
    fn bind(
        state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrForeignToplevelManagerV1>,
        _global_data: &crate::trust::Gate,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, ());
        let mut ids: Vec<ElementId> = state.wlr_toplevels.handles.keys().copied().collect();
        ids.sort();
        for id in ids {
            let now = state.wlr_toplevel_now(id);
            if let Some(h) = state.wlr_toplevel_announce(&manager, id, &now)
                && let Some((handles, _)) = state.wlr_toplevels.handles.get_mut(&id)
            {
                handles.push(h);
            }
        }
        state.wlr_toplevels.managers.push(manager);
        state.sync_wlr_toplevels();
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        manager: &ZwlrForeignToplevelManagerV1,
        request: zwlr_foreign_toplevel_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Request::Stop = request {
            state.wlr_toplevels.managers.retain(|m| m != manager);
            manager.finished();
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        manager: &ZwlrForeignToplevelManagerV1,
        _data: &(),
    ) {
        state.wlr_toplevels.managers.retain(|m| m != manager);
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ElementId> for WaylandState {
    fn request(
        state: &mut Self,
        client: &Client,
        _handle: &ZwlrForeignToplevelHandleV1,
        request: zwlr_foreign_toplevel_handle_v1::Request,
        id: &ElementId,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Request;
        let id = *id;
        // a handle outlives its window until the client destroys it: requests then do nothing
        if !state.foreign_toplevels.contains_key(&id) {
            return;
        }
        match request {
            Request::Activate { .. } => state.wlr_toplevel_activate(client, id),
            Request::Close => {
                if let Some(t) = state.window(id).and_then(|w| w.toplevel()) {
                    t.send_close();
                }
            }
            Request::SetMaximized => state.wlr_toplevel_request(id, ToplevelRequest::SetMaximized),
            Request::UnsetMaximized => {
                state.wlr_toplevel_request(id, ToplevelRequest::UnsetMaximized)
            }
            Request::SetFullscreen { output } => {
                let output = output.and_then(|o| state.output_id_from_wl(&o));
                state.wlr_toplevel_request(id, ToplevelRequest::SetFullscreen { output });
            }
            Request::UnsetFullscreen => {
                state.wlr_toplevel_request(id, ToplevelRequest::UnsetFullscreen)
            }
            // no minimizing on the canvas; set_rectangle is a hint for minimize animations
            _ => {}
        }
        state.sync_wlr_toplevels();
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        handle: &ZwlrForeignToplevelHandleV1,
        id: &ElementId,
    ) {
        if let Some((handles, _)) = state.wlr_toplevels.handles.get_mut(id) {
            handles.retain(|h| h != handle);
        }
    }
}
