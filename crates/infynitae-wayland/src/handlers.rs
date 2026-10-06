//! Smithay handler implementations: protocol events become Core calls, Core's effects become
//! protocol actions. Handlers never decide placement, order or camera state.

use smithay::backend::renderer::utils::{RendererSurfaceStateUserData, on_commit_buffer_handler};
use smithay::desktop::{
    PopupKeyboardGrab, PopupKind, PopupManager, PopupUngrabStrategy, Window,
    find_popup_root_surface,
};
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
        let was_unmapped = tree.root().is_none() || tree.root().is_some_and(|r| !r.mapped);
        if was_unmapped && let Some(toplevel) = window.toplevel() {
            let root_surface = toplevel.wl_surface();
            let popups: Vec<_> = PopupManager::popups_for_surface(root_surface)
                .map(|(p, _)| p)
                .collect();
            for p in popups {
                let _ = PopupManager::dismiss_popup(root_surface, &p);
            }
        }
        // A refused snapshot or transition changes nothing in Core; the protocol side (for
        // example `ensure_configured` below) reports client errors itself.
        if let Ok(effects) = self.core.toplevel_committed(id, tree) {
            self.apply(&effects);
        }
    }

    /// M3: popup position = `get_unconstrained_geometry(target)` against the anchor output's
    /// viewport (R8), or the positioner's own geometry when Core has no target.
    fn place_popup(
        &self,
        surface: &PopupSurface,
        positioner: PositionerState,
        serial: Option<Serial>,
    ) {
        let causing_output = serial
            .and_then(|s| self.serial_output(s))
            .or_else(|| self.core.pointer_output());
        let target = surface
            .get_parent_surface()
            .and_then(|parent| self.element_index_by_root(&parent))
            .and_then(|i| {
                self.core
                    .popup_constraint_target(self.elements[i].id, causing_output)
            });
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
        let root_surface = surface.wl_surface();
        let popups: Vec<_> = PopupManager::popups_for_surface(root_surface)
            .map(|(p, _)| p)
            .collect();
        for p in popups {
            let _ = PopupManager::dismiss_popup(root_surface, &p);
        }
        let gone: Vec<_> = self
            .elements
            .iter()
            .filter(|e| e.window.toplevel() == Some(&surface))
            .map(|e| e.id)
            .collect();
        self.elements
            .retain(|e| e.window.toplevel() != Some(&surface));
        for id in gone {
            let effects = self.core.toplevel_destroyed(id);
            self.apply(&effects);
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        self.place_popup(&surface, positioner, None);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        self.place_popup(&surface, positioner, None);
        surface.send_repositioned(token);
    }

    /// Popup grabs (INC-4, M3, INV-INPUT-10).
    /// Precondition W-SM-7: root surface must exist and be an element root, else grab_popup asserts.
    /// Focus precondition: root must match the element with ActiveElementFocus.
    fn grab(&mut self, surface: PopupSurface, _seat: wl_seat::WlSeat, serial: Serial) {
        let kind = PopupKind::Xdg(surface.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
            surface.send_popup_done();
            return;
        };
        let Some(elem_idx) = self.element_index_by_root(&root) else {
            surface.send_popup_done();
            return;
        };
        let elem_id = self.elements[elem_idx].id;
        if self.core.focus().target() != Some(elem_id) {
            surface.send_popup_done();
            return;
        }
        let seat = self.seat.clone();
        if let Ok(mut grab) = self.popups.grab_popup(root, kind, &seat, serial) {
            let kb = seat.get_keyboard().unwrap();
            if kb.is_grabbed()
                && !(kb.has_grab(serial) || kb.has_grab(grab.previous_serial().unwrap_or(serial)))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                surface.send_popup_done();
                return;
            }
            let pos = surface.with_pending_state(|s| s.positioner);
            self.place_popup(&surface, pos, Some(serial));
            if surface.is_initial_configure_sent() {
                let _ = surface.send_configure();
            }
            kb.set_focus(self, grab.current_grab(), serial);
            self.set_keyboard_grab(&kb, PopupKeyboardGrab::new(&grab), serial);
            let nested = grab.previous_serial().is_some();
            let ptr_grabbed = self.is_pointer_grabbed();
            if nested && self.nested_grab_keep_pointer && ptr_grabbed {
                // W-SM-14: Keep existing pointer grab for nested popup
            } else if !(ptr_grabbed
                && !(self.pointer_has_grab(serial)
                    || self.pointer_has_grab(grab.previous_serial().unwrap_or(serial))))
            {
                self.set_popup_pointer_grab(&grab, serial);
            }
            self.popup_grab = Some(grab);
        }
    }
}

impl SeatHandler for WaylandState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        if self.popup_preemption_pending {
            self.popup_preemption_ordering_violated = true;
        }
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
