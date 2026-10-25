//! `zwp_keyboard_shortcuts_inhibit_manager_v1`: an app (a VM, a remote desktop, a game) takes the
//! compositor's shortcuts while it has keyboard focus. Smithay's implementation keeps `Rc` state
//! and would make `WaylandState` lose `Send`, so the protocol is served here.
//!
//! An inhibitor is active only while its surface has keyboard focus; Core then lets every key
//! through except Super+L (`Core::set_shortcuts_inhibited`).

use smithay::reexports::wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::server::{
    zwp_keyboard_shortcuts_inhibit_manager_v1::{self, ZwpKeyboardShortcutsInhibitManagerV1},
    zwp_keyboard_shortcuts_inhibitor_v1::{self, ZwpKeyboardShortcutsInhibitorV1},
};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
    backend::{ClientId, GlobalId},
};

use crate::WaylandState;

/// One inhibitor: the surface it is for, and whether it was last told `active`.
#[derive(Debug)]
pub struct Inhibitor {
    resource: ZwpKeyboardShortcutsInhibitorV1,
    surface: WlSurface,
    active: bool,
}

/// The inhibitors of every client, held by `WaylandState`.
pub struct ShortcutsInhibitState {
    #[expect(
        dead_code,
        reason = "owns the zwp_keyboard_shortcuts_inhibit_manager_v1 global for the state's lifetime"
    )]
    global: GlobalId,
    inhibitors: Vec<Inhibitor>,
}

impl ShortcutsInhibitState {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            global: dh
                .create_global::<WaylandState, ZwpKeyboardShortcutsInhibitManagerV1, ()>(1, ()),
            inhibitors: Vec::new(),
        }
    }
}

impl WaylandState {
    /// Activates the inhibitor of the focused surface and no other, and tells Core whether one
    /// is active. Run whenever the keyboard focus or the set of inhibitors changes.
    pub(crate) fn update_shortcuts_inhibit(&mut self, focused: Option<&WlSurface>) {
        let mut any = false;
        for i in &mut self.shortcuts_inhibit.inhibitors {
            let on = focused == Some(&i.surface);
            if on != i.active {
                if on {
                    i.resource.active();
                } else {
                    i.resource.inactive();
                }
                i.active = on;
            }
            any |= on;
        }
        self.core.set_shortcuts_inhibited(any);
    }

    fn refresh_shortcuts_inhibit(&mut self) {
        let focused = self.seat.get_keyboard().and_then(|k| k.current_focus());
        self.update_shortcuts_inhibit(focused.as_ref());
    }
}

impl GlobalDispatch<ZwpKeyboardShortcutsInhibitManagerV1, ()> for WaylandState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwpKeyboardShortcutsInhibitManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        manager: &ZwpKeyboardShortcutsInhibitManagerV1,
        request: zwp_keyboard_shortcuts_inhibit_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwp_keyboard_shortcuts_inhibit_manager_v1::Request::InhibitShortcuts {
            id,
            surface,
            seat: _,
        } = request
        {
            // one seat: a second inhibitor for the same surface is the protocol's error
            if state
                .shortcuts_inhibit
                .inhibitors
                .iter()
                .any(|i| i.surface == surface)
            {
                manager.post_error(
                    zwp_keyboard_shortcuts_inhibit_manager_v1::Error::AlreadyInhibited,
                    "the surface already has a shortcuts inhibitor",
                );
                return;
            }
            let resource = data_init.init(id, ());
            state.shortcuts_inhibit.inhibitors.push(Inhibitor {
                resource,
                surface,
                active: false,
            });
            state.refresh_shortcuts_inhibit();
        }
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for WaylandState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ZwpKeyboardShortcutsInhibitorV1,
        _request: zwp_keyboard_shortcuts_inhibitor_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        // `destroy` is the only request; the inhibitor leaves in `destroyed`
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        resource: &ZwpKeyboardShortcutsInhibitorV1,
        _data: &(),
    ) {
        state
            .shortcuts_inhibit
            .inhibitors
            .retain(|i| i.resource.id() != resource.id());
        state.refresh_shortcuts_inhibit();
    }
}
