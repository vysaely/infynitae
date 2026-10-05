//! Smithay handler implementations: protocol events become Core calls, Core's effects become
//! protocol actions. Handlers never decide placement, order or camera state.

use smithay::backend::renderer::utils::{RendererSurfaceStateUserData, on_commit_buffer_handler};
use smithay::desktop::{PopupKind, Window, find_popup_root_surface};
use smithay::input::{SeatHandler, SeatState};
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::utils::Serial;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    CompositorClientState, CompositorHandler, CompositorState, add_destruction_hook, get_parent,
    is_sync_subsurface, with_states,
};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::{
    delegate_compositor, delegate_output, delegate_presentation, delegate_relative_pointer,
    delegate_seat, delegate_shm, delegate_xdg_shell,
};

use crate::{ClientState, ElementHandle, WaylandState, convert, snapshot};

impl WaylandState {
    fn element_index_by_root(&self, root: &WlSurface) -> Option<usize> {
        self.elements
            .iter()
            .position(|e| e.window.toplevel().is_some_and(|t| t.wl_surface() == root))
    }

    /// The toplevel root surface of any surface in an element's trees.
    fn toplevel_root(&self, surface: &WlSurface) -> Option<WlSurface> {
        let mut root = surface.clone();
        while let Some(p) = get_parent(&root) {
            root = p;
        }
        match self.popups.find_popup(&root) {
            Some(popup) => find_popup_root_surface(&popup).ok(),
            None => Some(root),
        }
    }

    /// Re-extracts the element's snapshot and applies Core's decision (M1).
    fn element_committed(&mut self, index: usize) {
        let ElementHandle { id, window } = &self.elements[index];
        let (id, window) = (*id, window.clone());
        window.on_commit();
        let Some(tree) = snapshot::extract(&window, &mut self.registry) else {
            return;
        };
        // A refused snapshot or transition changes nothing in Core; the protocol side (for
        // example `ensure_configured` below) reports client errors itself.
        if let Ok(effects) = self.core.toplevel_committed(id, tree) {
            self.apply(&effects);
        }
    }

    /// M3: popup position = `get_unconstrained_geometry(target)` against the anchor output's
    /// viewport (R8), or the positioner's own geometry when Core has no target.
    fn place_popup(&self, surface: &PopupSurface, positioner: PositionerState) {
        let target = surface
            .get_parent_surface()
            .and_then(|parent| self.element_index_by_root(&parent))
            .and_then(|i| self.core.popup_constraint_target(self.elements[i].id));
        let geometry = match target {
            Some(t) => positioner.get_unconstrained_geometry(convert::parent_geometry_rect(t)),
            None => positioner.get_geometry(),
        };
        surface.with_pending_state(|s| {
            s.geometry = geometry;
            s.positioner = positioner;
        });
    }
}

impl CompositorHandler for WaylandState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<ClientState>()
            .expect("clients are inserted with ClientState")
            .compositor
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        add_destruction_hook(surface, |state: &mut Self, s| state.registry.remove(s));
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.popups.commit(surface);

        let has_buffer = with_states(surface, |states| {
            states
                .data_map
                .get::<RendererSurfaceStateUserData>()
                .is_some_and(|d| {
                    d.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .buffer()
                        .is_some()
                })
        });
        // M3: a popup's initial configure on its initial commit; W-SM-6: a buffer before the
        // popup acked a configure is a protocol error
        if let Some(PopupKind::Xdg(p)) = self.popups.find_popup(surface) {
            if !p.is_initial_configure_sent() {
                let _ = p.send_configure();
            } else if has_buffer && !p.ensure_configured() {
                return;
            }
        }
        // W-SM-6: a buffer on a toplevel that has not acked a configure is a protocol error
        if let Some(t) = self
            .element_index_by_root(surface)
            .and_then(|i| self.elements[i].window.toplevel())
            && has_buffer
            && !t.ensure_configured()
        {
            return;
        }
        // a synchronised subsurface's state applies with its parent's commit
        if is_sync_subsurface(surface) {
            return;
        }
        if let Some(i) = self
            .toplevel_root(surface)
            .and_then(|root| self.element_index_by_root(&root))
        {
            self.element_committed(i);
        }
    }
}

impl BufferHandler for WaylandState {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for WaylandState {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}

impl XdgShellHandler for WaylandState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let id = self.core.toplevel_created();
        self.elements.push(ElementHandle {
            id,
            window: Window::new_wayland_window(surface),
        });
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let gone: Vec<_> = self
            .elements
            .iter()
            .filter(|e| e.window.toplevel() == Some(&surface))
            .map(|e| e.id)
            .collect();
        self.elements
            .retain(|e| e.window.toplevel() != Some(&surface));
        for id in gone {
            self.core.toplevel_destroyed(id);
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        self.place_popup(&surface, positioner);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        self.place_popup(&surface, positioner);
        surface.send_repositioned(token);
    }

    /// Popup grabs are INC-4 (keyboard focus, popup routing): not honoured before it, so no
    /// popup grab exists when an interaction is acquired (OB-4 acquisition part).
    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}
}

impl SeatHandler for WaylandState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seats
    }
}

/// Smithay sends the current state and enters for already-entered surfaces on bind.
impl OutputHandler for WaylandState {}

delegate_compositor!(WaylandState);
delegate_output!(WaylandState);
delegate_presentation!(WaylandState);
delegate_relative_pointer!(WaylandState);
delegate_seat!(WaylandState);
delegate_shm!(WaylandState);
delegate_xdg_shell!(WaylandState);
