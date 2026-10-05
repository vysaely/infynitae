//! Infynitae Native Smithay adapter (FOUNDATION_ARCHITECTURE C1.2).
//!
//! INC-2 state: compositor, subcompositor, shm, xdg-shell, `wl_output` + xdg-output and
//! presentation-time globals; the commit hook extracts the surface-tree snapshot and hands it to
//! Core; Core's effects (initial configures, output globals, enter/leave) are applied in order;
//! render elements are built from Core's render list and drawn through per-output damage trackers
//! sized `phys_O` (`convert`); observed frames complete on Core's frame clock. No seat, input
//! routing or scale policy yet (INC-3 onwards).
//!
//! Every Smithay handler receives `&mut` protocol state. That state holds the Core but never an
//! authority port or the output-lifecycle token: those types are `!Send` and the state is asserted
//! `Send` below, so storing one here is a compile error (C7.4, E0277). tools/verify-architecture
//! also rejects any mention of the authority entry points in this crate's sources.

#![deny(unsafe_code)]

pub mod convert;
mod handlers;
mod outputs;
pub mod registry;
mod snapshot;

use infynitae_core::Core;
use infynitae_core::stage::ElementId;
use smithay::desktop::{PopupManager, Window};
use smithay::input::SeatState;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::presentation::PresentationState;
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
    /// Required by Smithay's xdg-shell delegate (popup grabs name a seat). No seat and no
    /// `wl_seat` global exist until input intake (INC-3, INC-4).
    seats: SeatState<WaylandState>,
    registry: SurfaceRegistry,
    elements: Vec<ElementHandle>,
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
    /// `zxdg_output_manager_v1`, `wp_presentation`). Output globals follow Core's effects.
    pub fn new(display: &DisplayHandle, core: Core) -> Self {
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
            seats: SeatState::new(),
            registry: SurfaceRegistry::default(),
            elements: Vec::new(),
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
}

/// INV-ARCH-2: `unsafe` is denied here (lint `unsafe_code`).
#[cfg(feature = "probe_unsafe_block")]
pub fn probe_unsafe_block() {
    unsafe {}
}
