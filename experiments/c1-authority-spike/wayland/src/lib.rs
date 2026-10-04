//! Stand-in for the Smithay adapter. Smithay gives every protocol handler `&mut State`; here that
//! is `&mut Handlers`, which contains the Core but NO authority port.
//!
//! The following must not compile from protocol-handling code:
//!
//! ```compile_fail
//! // E0616/E0624-class: there is no mutable camera accessor, and set_center is private
//! fn activation(h: &mut spike_wayland::Handlers) {
//!     h.core.cameras().set_center(0, spike_core::geom::CanvasPos { x: 1, y: 1 });
//! }
//! ```
//!
//! ```compile_fail
//! // a port cannot be forged: private field
//! let mut p = spike_core::InputPort { core_id: 1 };
//! ```
//!
//! ```compile_fail
//! // physical input requires a port the handler does not have
//! fn activation(h: &mut spike_wayland::Handlers) {
//!     let _ = h.core.physical_input(spike_core::PhysicalEvent::PanDelta { camera: 0, dx: 5, dy: 0 });
//! }
//! ```
//!
//! ```compile_fail
//! // the navigation internals are not reachable
//! fn activation(h: &mut spike_wayland::Handlers) {
//!     spike_core::navigation::pan(todo!(), todo!(), 0, 1, 1);
//! }
//! ```
//!
//! Positive control (must compile): read access is fine.
//! ```
//! fn read(h: &spike_wayland::Handlers) -> spike_core::geom::CanvasPos { h.core.cameras().center(0) }
//! ```

use spike_core::{protocol::ProtocolEvent, Core, Effect};

pub struct Handlers {
    pub core: Core,
    #[cfg(feature = "probe_store_port_in_state")]
    pub leaked: spike_core::InputPort, // NEGATIVE PROBE: must fail the Send assertion below (E0277)
}

/// C-NB-2 hardening: protocol state must be `Send`; authority ports are `!Send`, so a port stored
/// in protocol state is a compile error.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Handlers>();
};

impl Handlers {
    /// xdg_activation_v1.activate after token validation
    pub fn on_activation(&mut self, element: u64) -> Vec<Effect> {
        self.core.protocol_event(ProtocolEvent::ActivationRequest { element })
    }
    pub fn on_new_toplevel(&mut self, element: u64) -> Vec<Effect> {
        self.core.protocol_event(ProtocolEvent::NewToplevel { element })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spike_core::{geom::CanvasPos, AuthorityError, PhysicalEvent, ShellCommand};

    #[test]
    fn activation_never_moves_cameras() {
        let (core, _input, _shell) = Core::new(2);
        let mut h = Handlers { core, #[cfg(feature = "probe_store_port_in_state")] leaked: unreachable!() };
        h.on_new_toplevel(7);
        let before = (h.core.cameras().center(0), h.core.cameras().center(1));
        let fx = h.on_activation(7);
        assert_eq!(fx, vec![Effect::AttentionRequested(7)]);
        assert_eq!((h.core.cameras().center(0), h.core.cameras().center(1)), before);
    }

    #[test]
    fn trusted_input_moves_camera_but_not_stage() {
        let (core, mut input, mut shell) = Core::new(1);
        let mut h = Handlers { core, #[cfg(feature = "probe_store_port_in_state")] leaked: unreachable!() };
        h.on_new_toplevel(1);
        let anchor = h.core.stage().anchor(1);
        h.core.physical_input(&mut input, PhysicalEvent::PanDelta { camera: 0, dx: 10, dy: -4 }).unwrap();
        assert_eq!(h.core.cameras().center(0), CanvasPos { x: -10, y: 4 });
        assert_eq!(h.core.stage().anchor(1), anchor, "camera motion must not mutate Stage (I2)");
        h.core.shell_command(&mut shell, ShellCommand::NavigateTo { camera: 0, to: CanvasPos { x: 3, y: 3 } }).unwrap();
        assert_eq!(h.core.cameras().center(0), CanvasPos { x: 3, y: 3 });
    }

    #[test]
    fn ports_are_bound_to_their_core() {
        let (mut a, _ia, _sa) = Core::new(1);
        let (_b, mut ib, _sb) = Core::new(1);
        // a port minted by a different Core::new call grants nothing
        assert_eq!(a.physical_input(&mut ib, PhysicalEvent::PanDelta { camera: 0, dx: 1, dy: 1 }).err(), Some(AuthorityError::ForeignPort));
    }
}
