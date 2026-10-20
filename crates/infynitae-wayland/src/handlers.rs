//! Smithay handler implementations: protocol events become Core calls, Core's effects become
//! protocol actions. Handlers never decide placement, order or camera state.

use infynitae_core::protocol::ToplevelRequest;
use smithay::backend::renderer::utils::{RendererSurfaceStateUserData, on_commit_buffer_handler};
use smithay::desktop::{
    PopupKeyboardGrab, PopupKind, PopupManager, PopupUngrabStrategy, Window,
    find_popup_root_surface,
};
use smithay::input::Seat;
use smithay::input::{SeatHandler, SeatState};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::utils::{SERIAL_COUNTER, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    CompositorClientState, CompositorHandler, CompositorState, add_destruction_hook, get_parent,
    is_sync_subsurface, with_states,
};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::pointer_constraints::{PointerConstraintsHandler, with_pointer_constraint};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
    set_data_device_focus,
};
use smithay::wayland::selection::primary_selection::{
    PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    XdgToplevelSurfaceData,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::xdg_activation::{
    XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
};
use smithay::{
    delegate_compositor, delegate_data_device, delegate_fractional_scale, delegate_output,
    delegate_pointer_constraints, delegate_presentation, delegate_primary_selection,
    delegate_relative_pointer, delegate_seat, delegate_shm, delegate_viewporter,
    delegate_xdg_activation, delegate_xdg_shell,
};

use crate::{ClientState, ElementHandle, WaylandState, convert, snapshot};

impl WaylandState {
    /// Hands a client's xdg_toplevel request to Core and applies what Core decided. A request on
    /// a toplevel that is not an element is dropped.
    fn toplevel_request(&mut self, surface: &ToplevelSurface, request: ToplevelRequest) {
        let Some(i) = self.element_index_by_root(surface.wl_surface()) else {
            return;
        };
        if let Ok((_, effects)) = self.core.toplevel_request(self.elements[i].id, request) {
            self.apply(&effects);
        }
    }

    pub(crate) fn element_index_by_root(&self, root: &WlSurface) -> Option<usize> {
        self.elements
            .iter()
            .position(|e| e.window.toplevel().is_some_and(|t| t.wl_surface() == root))
    }

    /// The toplevel root surface of any surface in an element's trees.
    pub(crate) fn toplevel_root(&self, surface: &WlSurface) -> Option<WlSurface> {
        let mut root = surface.clone();
        while let Some(p) = get_parent(&root) {
            root = p;
        }
        match self.popups.find_popup(&root) {
            Some(popup) => find_popup_root_surface(&popup).ok(),
            None => Some(root),
        }
    }

    /// A surface that joins an element's tree after the hint was last sent — a new subsurface
    /// or a new popup — is told the element's current hint, with the subsurfaces already below
    /// it. Later changes reach it with the rest of the tree (`Effect::PreferredScale`). The
    /// value is Core's (OBJ-16); only the routing is decided here.
    fn send_current_preferred_scale(&self, surface: &WlSurface) {
        if let Some((scale_120, integer_scale)) = self
            .toplevel_root(surface)
            .and_then(|root| self.element_index_by_root(&root))
            .and_then(|i| self.core.preferred_scale_of(self.elements[i].id))
        {
            convert::send_preferred_scale_to_tree(surface, scale_120, integer_scale);
        }
    }

    /// Input consequence of a popup's death (A1, INV-INPUT-8/10). The popup lifetime is owned by
    /// the client (Smithay's chain is its record); ActiveElementFocus by Core; this keeps the
    /// Smithay keyboard route consistent with both, explicitly — never through Smithay's lazy
    /// refocus inside key delivery, which announces the key being pressed in `enter` and then
    /// sends it again (`pressed` for a key already logically down).
    /// - A grabbing popup is still live: keyboard focus moves to the topmost live one now.
    /// - None is live (the chain ended), exactly once: release the popup keyboard grab and the
    ///   popup pointer grab by their install serials (INV-INPUT-10 steps 2-3; step 1 is moot, the
    ///   popups are dead), then route the keyboard to Core's ActiveElementFocus root, or to
    ///   nothing when Core is unfocused (step 4).
    fn reconcile_popup_chain(&mut self) {
        let Some(g) = self.popup_grab.clone() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let kb = self.keyboard_handle();
        if !g.has_ended() {
            let top = g.current_grab();
            if top.is_some() && kb.current_focus() != top {
                kb.set_focus(self, top, serial);
            }
            return;
        }
        self.popup_grab = None;
        if let Some(ks) = self.popup_keyboard_grab_serial
            && kb.has_grab(ks)
        {
            self.keyboard_unset_grab();
        }
        self.popup_keyboard_grab_serial = None;
        if let Some(ps) = self.popup_pointer_grab_serial
            && self.pointer_has_grab(ps)
        {
            self.pointer_unset_grab(serial, 0);
        }
        self.popup_pointer_grab_serial = None;
        let target = self
            .core
            .focus()
            .target()
            .and_then(|id| self.window(id))
            .and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()));
        kb.set_focus(self, target, serial);
    }

    fn restore_preempted_constraint(&mut self) {
        let target = self
            .core
            .focus()
            .target()
            .and_then(|id| self.window(id))
            .and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()));
        if let Some(target) = &target {
            let target_key = self.registry.key(target);
            if self
                .core
                .constraint()
                .is_some_and(|c| Some(c.surface) == target_key)
            {
                let ptr = self.pointer_handle();
                with_pointer_constraint(target, &ptr, |c| {
                    if let Some(c) = c
                        && !c.is_active()
                    {
                        c.activate();
                    }
                });
            }
        }
    }

    /// The element's ROOT surface committed: re-extracts the snapshot and drives M1 (map, unmap,
    /// initial configure). Only `commit` of the root surface itself may call this.
    fn element_root_committed(&mut self, index: usize) {
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
        // The state this commit applies is the last one the client acknowledged: Smithay's
        // post-commit hook has made it current before this handler runs (P9b).
        let acked_fullscreen = window.toplevel().is_some_and(|t| {
            t.current_state()
                .states
                .contains(xdg_toplevel::State::Fullscreen)
        });
        let _ = self.with_effects(|core, fx| {
            core.toplevel_committed_acked_into(id, tree, acked_fullscreen, fx)
        });
    }

    /// The element's surface tree changed without a commit of its root (a child surface
    /// committed, or a popup died): re-extracts the snapshot for hit-testing and rendering only.
    /// Never a lifecycle transition, never a configure (B1).
    fn element_tree_changed(&mut self, index: usize) {
        let ElementHandle { id, window } = &self.elements[index];
        let (id, window) = (*id, window.clone());
        window.on_commit();
        let Some(tree) = snapshot::extract(&window, &mut self.registry) else {
            return;
        };
        if let Ok(effects) = self.core.toplevel_tree_changed(id, tree) {
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
        // The positioner works in its parent's window geometry. For a popup of a popup (OB-1)
        // Core's target, which is relative to the toplevel, is moved by the positions of the
        // popups in between, each relative to its own parent.
        let mut parent = surface.get_parent_surface();
        let mut offset = (0, 0);
        while let Some(PopupKind::Xdg(p)) = parent.as_ref().and_then(|s| self.popups.find_popup(s))
        {
            let loc = p.with_pending_state(|s| (s.geometry.loc.x, s.geometry.loc.y));
            offset = (offset.0 + loc.0, offset.1 + loc.1);
            parent = p.get_parent_surface();
        }
        let target = parent
            .and_then(|root| self.element_index_by_root(&root))
            .and_then(|i| {
                self.core
                    .popup_constraint_target(self.elements[i].id, causing_output)
            })
            .map(|mut t| {
                t.loc.x -= offset.0;
                t.loc.y -= offset.1;
                t
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
        add_destruction_hook(surface, |state: &mut Self, s| {
            if let Some(key) = state.registry.key(s) {
                let fx = state.core.destroy_pointer_constraint(key);
                state.apply(&fx);
            }
            state.registry.remove(s);
        });
    }

    fn new_subsurface(&mut self, surface: &WlSurface, _parent: &WlSurface) {
        self.send_current_preferred_scale(surface);
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
        // W-SM-6 / OB-18: a buffer on a popup that has not acknowledged a configure is a protocol
        // error, and that includes a buffer in its very first commit, before any configure was
        // sent. Checked first, so that an offending initial commit is not answered with a
        // configure as if it were a valid one. M3: a valid initial commit gets its configure.
        if let Some(PopupKind::Xdg(p)) = self.popups.find_popup(surface) {
            if has_buffer && !p.ensure_configured() {
                return;
            }
            if !p.is_initial_configure_sent() {
                let _ = p.send_configure();
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
        // a layer surface (L1), or a surface of its tree
        if let Some(i) = self.layer_index_by_root(surface).or_else(|| {
            self.toplevel_root(surface)
                .and_then(|root| self.layer_index_by_root(&root))
        }) {
            self.layer_committed(i);
            return;
        }
        // Only the root surface's own commit drives M1; a child commit (popup, desynchronised
        // subsurface) only changes the element's surface tree.
        if let Some(i) = self.element_index_by_root(surface) {
            self.element_root_committed(i);
        } else if let Some(i) = self
            .toplevel_root(surface)
            .and_then(|root| self.element_index_by_root(&root))
        {
            self.element_tree_changed(i);
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

impl WaylandState {
    /// `xdg_toplevel.set_title` / `set_app_id` (INC-11): Core keeps both as the shell's display
    /// metadata. Both are read from the toplevel's role state, which Smithay has just updated.
    fn toplevel_metadata_changed(&mut self, surface: &ToplevelSurface) {
        let Some(id) = self
            .elements
            .iter()
            .find(|e| e.window.toplevel() == Some(surface))
            .map(|e| e.id)
        else {
            return;
        };
        let (title, app_id) = with_states(surface.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .map(|d| {
                    let d = d.lock().unwrap();
                    (d.title.clone(), d.app_id.clone())
                })
                .unwrap_or_default()
        });
        let _ = self
            .core
            .toplevel_metadata(id, title.as_deref(), app_id.as_deref());
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

    fn title_changed(&mut self, surface: ToplevelSurface) {
        self.toplevel_metadata_changed(&surface);
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        self.toplevel_metadata_changed(&surface);
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

    // M2 (INV-PROTO-1, INV-PRES-1): every xdg_toplevel request is Core's to decide. Each handler
    // below overrides Smithay's default, two of which answer by themselves (W-SM-13: the default
    // maximize and fullscreen handlers send a configure, even before the initial one).

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        self.toplevel_request(&surface, ToplevelRequest::SetMaximized);
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        self.toplevel_request(&surface, ToplevelRequest::UnsetMaximized);
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, output: Option<WlOutput>) {
        // the output the client named, if it is one of Core's; which output is used is Core's
        // choice (C5.2)
        let output = output.and_then(|o| self.output_id_of(&o));
        self.toplevel_request(&surface, ToplevelRequest::SetFullscreen { output });
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        self.toplevel_request(&surface, ToplevelRequest::UnsetFullscreen);
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        self.toplevel_request(&surface, ToplevelRequest::SetMinimized);
    }

    fn move_request(&mut self, surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        self.toplevel_request(&surface, ToplevelRequest::Move);
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        _seat: wl_seat::WlSeat,
        _serial: Serial,
        _edges: xdg_toplevel::ResizeEdge,
    ) {
        self.toplevel_request(&surface, ToplevelRequest::Resize);
    }

    fn show_window_menu(
        &mut self,
        surface: ToplevelSurface,
        _seat: wl_seat::WlSeat,
        _serial: Serial,
        _location: convert::WindowMenuLocation,
    ) {
        self.toplevel_request(&surface, ToplevelRequest::ShowWindowMenu);
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        self.popups_created += 1;
        snapshot::set_popup_created(surface.wl_surface(), self.popups_created);
        self.place_popup(&surface, positioner, None);
        let wl_surface = surface.wl_surface().clone();
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
        // before the popup's initial configure, which waits for its initial commit
        self.send_current_preferred_scale(&wl_surface);
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
                self.popup_pointer_grab_serial = Some(serial);
            }
            self.popup_grab = Some(grab);
        }
    }

    /// A popup's lifetime ended (client `xdg_popup.destroy`; Smithay has already marked it dead).
    /// Three separate consequences, none of which is a commit of the parent (B1):
    /// 1. lifetime: Smithay's grab chain forgets the dead popup (`cleanup`);
    /// 2. input: the popup grab chain is reconciled (A1, `reconcile_popup_chain`);
    /// 3. scene: the element's surface tree changes, so the dead surface stops being hit-testable
    ///    (R2-01) — through the tree-change path, never the root-commit (M1) path.
    fn popup_destroyed(&mut self, surface: PopupSurface) {
        let kind = PopupKind::Xdg(surface.clone());
        let root = find_popup_root_surface(&kind)
            .ok()
            .or_else(|| self.toplevel_root(surface.wl_surface()));
        self.popups.cleanup();
        self.reconcile_popup_chain();
        self.restore_preempted_constraint();
        match root.and_then(|root| self.element_index_by_root(&root)) {
            Some(i) => self.element_tree_changed(i),
            // the root could not be resolved (already gone): refresh every mapped tree; the
            // tree-change path cannot advance any element's lifecycle
            None => {
                for i in 0..self.elements.len() {
                    self.element_tree_changed(i);
                }
            }
        }
    }
}

impl SeatHandler for WaylandState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seats
    }

    /// The keyboard focus moved (it follows Core's ActiveElementFocus, INV-INPUT-8): only the
    /// focused client is offered the clipboard and the primary selection, and only it may set
    /// them (Smithay refuses `set_selection` from a client without keyboard focus). No focus, no
    /// offers: an unfocused client never sees the contents.
    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        use smithay::reexports::wayland_server::Resource;
        let client = focused.and_then(|surface| surface.client());
        set_data_device_focus(&self.dh, seat, client.clone());
        set_primary_focus(&self.dh, seat, client);
    }
}

impl SelectionHandler for WaylandState {
    // the compositor offers no selection of its own
    type SelectionUserData = ();

    /// A client set (or cleared) a selection: Smithay accepted it only from the client with the
    /// keyboard focus, so that client owns it.
    fn new_selection(
        &mut self,
        ty: smithay::wayland::selection::SelectionTarget,
        source: Option<smithay::wayland::selection::SelectionSource>,
        _seat: Seat<Self>,
    ) {
        use smithay::reexports::wayland_server::Resource;
        let owner = source.and_then(|_| {
            self.keyboard_handle()
                .current_focus()
                .and_then(|surface| surface.client())
                .map(|client| client.id())
        });
        let slot = match ty {
            smithay::wayland::selection::SelectionTarget::Clipboard => 0,
            smithay::wayland::selection::SelectionTarget::Primary => 1,
        };
        self.selection_owners[slot] = owner;
    }
}

impl WaylandState {
    /// Clears a selection whose owner disconnected, telling the focused client (`selection` with
    /// no offer). Called after each client dispatch.
    pub fn clear_orphaned_selections(&mut self) {
        let gone =
            |state: &Self,
             owner: &Option<smithay::reexports::wayland_server::backend::ClientId>| {
                owner.as_ref().is_some_and(|id| {
                    state
                        .dh
                        .backend_handle()
                        .get_client_data(id.clone())
                        .is_err()
                })
            };
        if gone(self, &self.selection_owners[0]) {
            self.selection_owners[0] = None;
            smithay::wayland::selection::data_device::clear_data_device_selection(
                &self.dh, &self.seat,
            );
        }
        if gone(self, &self.selection_owners[1]) {
            self.selection_owners[1] = None;
            smithay::wayland::selection::primary_selection::clear_primary_selection(
                &self.dh, &self.seat,
            );
        }
    }
}

impl DataDeviceHandler for WaylandState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device
    }
}

// Drag and drop: Smithay's defaults (a drag starts only from a client's implicit pointer grab).
// Not validated against Core's pointer ownership yet (clipboard slice; DnD needs its own review).
/// Drag-and-drop is not offered: a drag a client starts is refused (`refuse_client_drags`).
impl ClientDndGrabHandler for WaylandState {
    fn started(
        &mut self,
        _source: Option<smithay::reexports::wayland_server::protocol::wl_data_source::WlDataSource>,
        _icon: Option<WlSurface>,
        _seat: Seat<Self>,
    ) {
        self.drag_started = true;
    }
}
impl ServerDndGrabHandler for WaylandState {}

impl PrimarySelectionHandler for WaylandState {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection
    }
}

delegate_data_device!(WaylandState);
delegate_primary_selection!(WaylandState);

/// Smithay sends the current state and enters for already-entered surfaces on bind.
impl OutputHandler for WaylandState {}

delegate_compositor!(WaylandState);
delegate_fractional_scale!(WaylandState);
delegate_output!(WaylandState);
delegate_pointer_constraints!(WaylandState);
delegate_presentation!(WaylandState);
delegate_relative_pointer!(WaylandState);
delegate_seat!(WaylandState);
delegate_shm!(WaylandState);
delegate_viewporter!(WaylandState);
delegate_xdg_shell!(WaylandState);
delegate_xdg_activation!(WaylandState);

/// W-SM-8: whether an activation token passed the provenance check when it was created.
struct TokenProvenance {
    validated: bool,
}

impl WaylandState {
    fn token_validated(&self, data: &XdgActivationTokenData) -> bool {
        use smithay::reexports::wayland_server::Resource;
        let Some((serial, seat)) = &data.serial else {
            return false;
        };
        let ours = smithay::input::Seat::<WaylandState>::from_resource(seat)
            .is_some_and(|s| s == self.seat);
        let keyboard = self.keyboard_handle();
        let fresh = keyboard
            .last_enter()
            .is_some_and(|last| serial.is_no_older_than(&last));
        let focused_client = keyboard
            .current_focus()
            .and_then(|surface| surface.client())
            .map(|client| client.id());
        ours && fresh && focused_client.is_some() && focused_client == data.client_id
    }
}

/// How long an activation token validates a request after it was created (provisional; B10.7).
const ACTIVATION_TOKEN_LIFETIME: std::time::Duration = std::time::Duration::from_secs(10);
/// How long an unused token is kept: a request with an expired token still asks for attention,
/// so tokens outlive their validity; but not for ever (INC-12 review r2).
const ACTIVATION_TOKEN_RETENTION: std::time::Duration = std::time::Duration::from_secs(300);
/// At most this many unused tokens are kept (the oldest go first): a client may create tokens
/// without end, and Smithay removes one only when it is used (INC-12 review r2).
pub const ACTIVATION_TOKENS_KEPT: usize = 256;

impl WaylandState {
    /// Unused activation tokens kept (for tests).
    pub fn activation_tokens_kept(&mut self) -> usize {
        let mut n = 0;
        self.activation.retain_tokens(|_, _| {
            n += 1;
            true
        });
        n
    }

    /// Bounds the tokens kept: older than the retention period, or beyond the newest
    /// `ACTIVATION_TOKENS_KEPT - 1` (room for the one being created), they are dropped.
    fn prune_activation_tokens(&mut self) {
        let mut stamps = Vec::new();
        self.activation.retain_tokens(|_, d| {
            let keep = d.timestamp.elapsed() <= ACTIVATION_TOKEN_RETENTION;
            if keep {
                stamps.push(d.timestamp);
            }
            keep
        });
        if stamps.len() >= ACTIVATION_TOKENS_KEPT {
            stamps.sort_unstable();
            let cutoff = stamps[stamps.len() - (ACTIVATION_TOKENS_KEPT - 1)];
            self.activation.retain_tokens(|_, d| d.timestamp >= cutoff);
        }
    }
}

impl XdgActivationHandler for WaylandState {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.activation
    }

    /// W-SM-8 (INV-INPUT-9): Smithay accepts every token by default. Here every token is kept
    /// (an unvalidated request still asks for attention, `ActivationPolicy`), and its provenance
    /// is recorded when it is created: it is **validated** only if the client that created it
    /// holds the keyboard focus, and its serial is one of this seat's, no older than the
    /// keyboard's last focus `enter`. A client the user merely points at receives fresh pointer
    /// serials, but not the focus: its tokens are not validated (focus-stealing prevention).
    fn token_created(&mut self, _token: XdgActivationToken, data: XdgActivationTokenData) -> bool {
        self.prune_activation_tokens();
        let validated = self.token_validated(&data);
        data.user_data
            .insert_if_missing(|| TokenProvenance { validated });
        true
    }

    /// B10.7: a validated, unexpired token activates the element whose toplevel `surface` is;
    /// Core decides between focus and attention (INV-INPUT-9). A token is used once.
    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        self.activation.remove_token(&token);
        let Some(i) = self.element_index_by_root(&surface) else {
            return;
        };
        let id = self.elements[i].id;
        // an expired token validates nothing: the request only asks for attention
        let validated = token_data.timestamp.elapsed() <= ACTIVATION_TOKEN_LIFETIME
            && token_data
                .user_data
                .get::<TokenProvenance>()
                .is_some_and(|p| p.validated);
        if let Ok(effects) = self.core.activation_request(id, validated) {
            self.apply(&effects);
        }
    }
}

/// Nothing to do at bind time: every surface of an element's tree already holds the element's
/// hint (it was told when it joined, and with every change since), and Smithay sends the held
/// value to a `wp_fractional_scale_v1` when it is created.
impl smithay::wayland::fractional_scale::FractionalScaleHandler for WaylandState {}

impl PointerConstraintsHandler for WaylandState {
    fn new_constraint(
        &mut self,
        surface: &WlSurface,
        pointer: &smithay::input::pointer::PointerHandle<Self>,
    ) {
        let key = self.registry.key_for(surface);
        let info = with_pointer_constraint(surface, pointer, |c| {
            c.map(|c| {
                let lifetime = infynitae_core::pointer::ConstraintLifetime::Persistent;
                let region = c.region().and_then(convert::convert_region);
                match &*c {
                    smithay::wayland::pointer_constraints::PointerConstraint::Locked(locked) => {
                        let hint = locked.cursor_position_hint().map(|p| {
                            infynitae_core::geom::LocalPoint {
                                x: p.x as i32,
                                y: p.y as i32,
                            }
                        });
                        (
                            infynitae_core::pointer::ConstraintKind::Lock,
                            lifetime,
                            region,
                            hint,
                        )
                    }
                    smithay::wayland::pointer_constraints::PointerConstraint::Confined(_) => (
                        infynitae_core::pointer::ConstraintKind::Confine,
                        lifetime,
                        region,
                        None,
                    ),
                }
            })
        });

        if let Some((kind, lifetime, region, hint)) = info {
            let fx = self
                .core
                .request_pointer_constraint(key, kind, lifetime, region, hint);
            self.apply(&fx);
        }
    }

    fn cursor_position_hint(
        &mut self,
        surface: &WlSurface,
        _pointer: &smithay::input::pointer::PointerHandle<Self>,
        location: convert::GlobalPoint,
    ) {
        if let Some(key) = self.registry.key(surface) {
            let hint = convert::hint_to_local_point(location);
            self.core.set_cursor_position_hint(key, hint);
            let fx = self.core.reconcile_constraints();
            self.apply(&fx);
        }
    }
}
