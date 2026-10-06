//! Infynitae Native Smithay adapter (FOUNDATION_ARCHITECTURE C1.2).
//!
//! INC-2 state: compositor, subcompositor, shm, xdg-shell, `wl_output` + xdg-output and
//! presentation-time globals; the commit hook extracts the surface-tree snapshot and hands it to
//! Core; Core's effects (initial configures, output globals, enter/leave) are applied in order;
//! render elements are built from Core's render list and drawn through per-output damage trackers
//! sized `phys_O` (`convert`); observed frames complete on Core's frame clock. INC-3: one seat
//! (`seat0`) with a pointer and the relative-pointer manager; Core's pointer effects are applied in
//! order (`pointer`). No keyboard (INC-4), constraints (INC-5) or scale policy (INC-6) yet.
//!
//! Every Smithay handler receives `&mut` protocol state. That state holds the Core but never an
//! authority port or the output-lifecycle token: those types are `!Send` and the state is asserted
//! `Send` below, so storing one here is a compile error (C7.4, E0277). tools/verify-architecture
//! also rejects any mention of the authority entry points in this crate's sources.

#![deny(unsafe_code)]

pub mod convert;
mod handlers;
mod keyboard;
mod outputs;
mod pointer;
pub mod registry;
mod snapshot;

pub use keyboard::PreemptionStrategy;

use infynitae_core::Core;
use infynitae_core::stage::ElementId;
use smithay::desktop::{PopupManager, Window};
use smithay::input::{Seat, SeatState};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::relative_pointer::RelativePointerManagerState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;

/// `clk_id` announced by `wp_presentation`: CLOCK_MONOTONIC on Linux. Frame times handed to
/// `complete_frames` are on this clock.
const CLOCK_MONOTONIC: u32 = 1;

pub use registry::SurfaceRegistry;

/// Per-client data the composition root attaches when inserting a client.
#[derive(Debug, Default)]
pub struct ClientState {
    compositor: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

/// An element's protocol handle (C2.2: handles stay in the adapter).
struct ElementHandle {
    id: ElementId,
    window: Window,
}

/// Protocol state handed to every Smithay handler.
pub struct WaylandState {
    core: Core,
    dh: DisplayHandle,
    compositor: CompositorState,
    #[expect(
        dead_code,
        reason = "owns the xdg-output manager global for the state's lifetime"
    )]
    output_manager: OutputManagerState,
    #[expect(
        dead_code,
        reason = "owns the wp_presentation global for the state's lifetime"
    )]
    presentation: PresentationState,
    outputs: Vec<outputs::OutputHandle>,
    xdg_shell: XdgShellState,
    shm: ShmState,
    popups: PopupManager,
    seats: SeatState<WaylandState>,
    /// The single seat, with pointer and keyboard (INC-4).
    seat: Seat<WaylandState>,
    pub(crate) popup_grab: Option<smithay::desktop::PopupGrab<WaylandState>>,
    pub(crate) nested_grab_keep_pointer: bool,
    pub(crate) ordered_preemption_strategy: keyboard::PreemptionStrategy,
    #[expect(
        dead_code,
        reason = "owns the zwp_relative_pointer_manager_v1 global for the state's lifetime"
    )]
    relative_pointer: RelativePointerManagerState,
    registry: SurfaceRegistry,
    elements: Vec<ElementHandle>,
    pub(crate) serial_outputs:
        std::collections::VecDeque<(smithay::utils::Serial, infynitae_core::output::OutputId)>,
    pub(crate) popup_pointer_grab_serial: Option<smithay::utils::Serial>,
    /// Install serial of the popup keyboard grab (the latest level of the chain), the identity by
    /// which teardown releases exactly that grab (A1).
    pub(crate) popup_keyboard_grab_serial: Option<smithay::utils::Serial>,
    // probe builds only: the Send assertion must be the one and only error
    #[cfg(feature = "probe_store_port_in_state")]
    leaked_port: infynitae_core::InputPort,
    #[cfg(feature = "probe_store_output_token_in_state")]
    leaked_token: infynitae_core::OutputLifecycleToken,
}

/// C7.4 / EC-15: protocol state must be `Send`.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<WaylandState>();
};

impl WaylandState {
    /// Creates the globals (`wl_compositor` v6 and `wl_subcompositor`, `wl_shm`, `xdg_wm_base`,
    /// `zxdg_output_manager_v1`, `wp_presentation`, `wl_seat` with pointer and keyboard,
    /// `zwp_relative_pointer_manager_v1`). Output globals follow Core's effects.
    pub fn new(display: &DisplayHandle, core: Core) -> Self {
        let mut seats = SeatState::new();
        let mut seat = seats.new_wl_seat(display, "seat0");
        seat.add_pointer();
        seat.add_keyboard(Default::default(), 200, 25)
            .expect("the seat has a keyboard");
        WaylandState {
            core,
            dh: display.clone(),
            compositor: CompositorState::new_v6::<Self>(display),
            output_manager: OutputManagerState::new_with_xdg_output::<Self>(display),
            presentation: PresentationState::new::<Self>(display, CLOCK_MONOTONIC),
            outputs: Vec::new(),
            xdg_shell: XdgShellState::new::<Self>(display),
            shm: ShmState::new::<Self>(display, []),
            popups: PopupManager::default(),
            seats,
            seat,
            popup_grab: None,
            popup_pointer_grab_serial: None,
            popup_keyboard_grab_serial: None,
            nested_grab_keep_pointer: true,
            ordered_preemption_strategy: keyboard::PreemptionStrategy::Ordered,
            relative_pointer: RelativePointerManagerState::new::<Self>(display),
            registry: SurfaceRegistry::default(),
            elements: Vec::new(),
            serial_outputs: std::collections::VecDeque::new(),
            // probe builds only: the Send assertion must be the one and only error
            #[cfg(feature = "probe_store_port_in_state")]
            leaked_port: unreachable!(),
            #[cfg(feature = "probe_store_output_token_in_state")]
            leaked_token: unreachable!(),
        }
    }

    pub fn core(&self) -> &Core {
        &self.core
    }

    /// The composition root and backends reach Core through the state, as handlers do.
    pub fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    pub fn registry(&self) -> &SurfaceRegistry {
        &self.registry
    }

    /// The protocol handle of an element (render oracles and later increments).
    pub fn window(&self, id: ElementId) -> Option<&Window> {
        self.elements.iter().find(|e| e.id == id).map(|e| &e.window)
    }

    /// Elements in creation order.
    pub fn elements(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.elements.iter().map(|e| e.id)
    }

    /// Returns the current modifier state from Smithay's XKB seat state (OBJ-27).
    pub fn seat_modifiers(&self) -> infynitae_core::input::bindings::Modifiers {
        self.seat
            .get_keyboard()
            .map(|kb| convert::modifiers_from_smithay(&kb.modifier_state()))
            .unwrap_or_default()
    }

    /// Returns whether the seat's keyboard currently has an active grab.
    pub fn keyboard_is_grabbed(&self) -> bool {
        self.keyboard_handle().is_grabbed()
    }

    /// Returns the currently focused surface of the seat's keyboard, if any.
    pub fn keyboard_current_focus(
        &self,
    ) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        self.keyboard_handle().current_focus()
    }

    /// Returns Smithay's raw XKB modifier state for testing and observation.
    pub fn keyboard_modifier_state(&self) -> smithay::input::keyboard::ModifiersState {
        self.keyboard_handle().modifier_state()
    }

    /// Updates Smithay's XKB state for a physical key and returns the updated modifiers
    /// and whether modifier state changed, without forwarding the key to any client.
    /// Requires an InputAuthToken capability proof minted by Core for an authenticated InputPort (INC4R-02).
    /// Consumes the token by value; rejects foreign tokens minted by another Core with zero state mutation (R2-02).
    pub fn sync_seat_key(
        &mut self,
        token: infynitae_core::InputAuthToken,
        key: u32,
        pressed: bool,
    ) -> (infynitae_core::input::bindings::Modifiers, bool) {
        if token.core_id() != self.core.id() {
            return (self.seat_modifiers(), false);
        }
        use smithay::backend::input::KeyState;
        use smithay::input::keyboard::Keycode;

        let kb = self.keyboard_handle();
        let code = Keycode::new(key + 8);
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        let (_, mods_changed) =
            kb.input_intercept(self, code, state, |_state, _mods, _sym| None::<()>);
        let modifiers = convert::modifiers_from_smithay(&kb.modifier_state());
        (modifiers, mods_changed)
    }

    /// Returns the number of interaction serials currently held in the provenance ring buffer.
    pub fn serial_outputs_len(&self) -> usize {
        self.serial_outputs.len()
    }

    /// Records the causal output on which a user interaction serial occurred (D-B6 Rule 1 provenance).
    pub(crate) fn record_serial_output(
        &mut self,
        serial: smithay::utils::Serial,
        output: infynitae_core::output::OutputId,
    ) {
        if self.serial_outputs.len() >= 64 {
            self.serial_outputs.pop_front();
        }
        self.serial_outputs.push_back((serial, output));
    }

    /// Retrieves the causal output mapped to a serial, if known.
    pub(crate) fn serial_output(
        &self,
        serial: smithay::utils::Serial,
    ) -> Option<infynitae_core::output::OutputId> {
        self.serial_outputs
            .iter()
            .rev()
            .find(|(s, _)| *s == serial)
            .map(|(_, o)| *o)
    }
}

/// INV-ARCH-2: `unsafe` is denied here (lint `unsafe_code`).
#[cfg(feature = "probe_unsafe_block")]
pub fn probe_unsafe_block() {
    unsafe {}
}
