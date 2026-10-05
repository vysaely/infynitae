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
    #[cfg(feature = "probe_store_output_token_in_state")]
    pub leaked_output: spike_core::OutputLifecycleToken, // NEGATIVE PROBE (PD-F2-18): E0277
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
    /// a client commit that changed the element's logical size
    pub fn on_commit(&mut self, element: u64, size: (i64, i64)) -> Vec<Effect> {
        self.core.protocol_event(ProtocolEvent::Commit { element, size })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spike_core::{geom::CanvasPos, navigation::OutputKey, AuthorityError, OutputDescriptor, PersistedCameras, PhysicalEvent, PointerState, ShellCommand, Transform};

    fn handlers(core: Core) -> Handlers {
        Handlers {
            core,
            #[cfg(feature = "probe_store_port_in_state")]
            leaked: unreachable!(),
            #[cfg(feature = "probe_store_output_token_in_state")]
            leaked_output: unreachable!(),
        }
    }
    fn desc(key: u64) -> OutputDescriptor {
        OutputDescriptor { key: OutputKey(key), mode: (1920, 1080), transform: Transform::Normal, scale_120: 120, refresh_mhz: 60_000 }
    }

    #[test]
    fn activation_never_moves_cameras() {
        let (core, _input, _shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        h.core.output_connected(&mut outs, desc(1)).unwrap();
        h.core.output_connected(&mut outs, desc(2)).unwrap();
        h.on_new_toplevel(7);
        let before = (h.core.cameras().center(0), h.core.cameras().center(1));
        let fx = h.on_activation(7);
        assert_eq!(fx, vec![Effect::AttentionRequested(7)]);
        assert_eq!((h.core.cameras().center(0), h.core.cameras().center(1)), before);
    }

    #[test]
    fn trusted_input_moves_camera_but_not_stage() {
        let (core, mut input, mut shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        h.core.output_connected(&mut outs, desc(1)).unwrap();
        h.on_new_toplevel(1);
        let anchor = h.core.stage().anchor(1);
        h.core.physical_input(&mut input, PhysicalEvent::PanDelta { camera: 0, dx: 10, dy: -4 }).unwrap();
        assert_eq!(h.core.cameras().center(0), CanvasPos { x: -10, y: 4 });
        assert_eq!(h.core.stage().anchor(1), anchor, "camera motion must not mutate Stage (I2)");
        h.core.shell_command(&mut shell, ShellCommand::NavigateTo { camera: 0, to: CanvasPos { x: 3, y: 3 } }).unwrap();
        assert_eq!(h.core.cameras().center(0), CanvasPos { x: 3, y: 3 });
    }

    /// PD-F2-01: viewport-fit is one-shot. After an authorized fit, NO client commit (resize, any size,
    /// any number) moves or zooms the camera; only a fresh authorized action does.
    #[test]
    fn client_resize_after_fit_never_moves_camera() {
        let (core, _input, mut shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        h.core.output_connected(&mut outs, desc(1)).unwrap();
        h.on_new_toplevel(5);
        h.core.shell_command(&mut shell, ShellCommand::FitViewport { camera: 0, element: 5, view: (1920, 1080) }).unwrap();
        let fitted = (h.core.cameras().center(0), h.core.cameras().zoom_milli(0));
        assert_eq!(fitted, (CanvasPos { x: 400, y: 300 }, 1800));
        for i in 1..=200i64 {
            let fx = h.on_commit(5, (100 + 37 * i, 50 + 11 * i)); // adversarial self-resizing client
            assert!(fx.is_empty(), "a client commit produced effects {fx:?}");
            assert_eq!((h.core.cameras().center(0), h.core.cameras().zoom_milli(0)), fitted, "client commit #{i} moved the camera");
        }
        h.core.shell_command(&mut shell, ShellCommand::FitViewport { camera: 0, element: 5, view: (1920, 1080) }).unwrap();
        assert_eq!(h.core.stage().size(5), Some((7500, 2250)));
        assert_eq!(h.core.cameras().center(0), CanvasPos { x: 3750, y: 1125 });
        assert_eq!(h.core.cameras().zoom_milli(0), 256);
    }

    #[test]
    fn fit_of_stale_element_mutates_nothing() {
        let (core, _input, mut shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        h.core.output_connected(&mut outs, desc(1)).unwrap();
        let before = (h.core.cameras().center(0), h.core.cameras().zoom_milli(0));
        assert_eq!(h.core.shell_command(&mut shell, ShellCommand::FitViewport { camera: 0, element: 99, view: (1920, 1080) }).unwrap(), vec![]);
        assert_eq!((h.core.cameras().center(0), h.core.cameras().zoom_milli(0)), before);
    }

    #[test]
    fn ports_are_bound_to_their_core() {
        let (mut a, _ia, _sa, mut oa) = Core::new(PersistedCameras::default());
        let (_b, mut ib, _sb, mut ob) = Core::new(PersistedCameras::default());
        a.output_connected(&mut oa, desc(1)).unwrap();
        // a port / token minted by a different Core::new call grants nothing
        assert_eq!(a.physical_input(&mut ib, PhysicalEvent::PanDelta { camera: 0, dx: 1, dy: 1 }).err(), Some(AuthorityError::ForeignPort));
        assert_eq!(a.output_connected(&mut ob, desc(2)).err(), Some(AuthorityError::ForeignPort));
        assert_eq!(a.cameras().len(), 1, "a foreign token created a camera");
    }

    /// PD-F2-18 (2R): no camera exists before an output is connected through the token; connecting
    /// instantiates and binds a camera at Core-chosen placement; a second output does not touch it.
    #[test]
    fn output_connected_with_token_instantiates_and_binds_camera() {
        let (core, _input, _shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        assert!(h.core.cameras().is_empty(), "cameras must not exist before an output is connected");
        let (o1, fx) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        // E1R (E-F2-05): the first connect also leaves the zero-output pointer state (startup placement)
        assert_eq!(fx, vec![Effect::CameraBound { camera: 0, output: o1 }, Effect::PointerState(PointerState::Located { output: o1, p: (960.0, 540.0) })]);
        assert_eq!(h.core.cameras().camera_for_output(o1), Some(0));
        assert_eq!((h.core.cameras().center(0), h.core.cameras().zoom_milli(0)), (CanvasPos { x: 0, y: 0 }, 1000), "internal default placement");
        let c0 = h.core.cameras().center(0);
        let (o2, _) = h.core.output_connected(&mut outs, desc(11)).unwrap();
        assert_eq!(h.core.cameras().camera_for_output(o2), Some(1));
        assert_eq!(h.core.cameras().center(0), c0, "connecting another output changed an existing camera");
    }

    /// PD-F2-18 (2R): disconnect unbinds and preserves the camera and its value; reconnecting the same
    /// OutputKey re-binds the SAME camera unchanged (no new camera, no reset).
    #[test]
    fn output_disconnected_unbinds_and_preserves_camera() {
        let (core, mut input, _shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        let (o1, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        h.core.physical_input(&mut input, PhysicalEvent::PanDelta { camera: 0, dx: -25, dy: 40 }).unwrap();
        let value = (h.core.cameras().center(0), h.core.cameras().zoom_milli(0));
        let fx = h.core.output_disconnected(&mut outs, o1).unwrap();
        // E1R (E-F2-05): removing the last output enters the zero-output pointer state
        assert_eq!(fx, vec![Effect::CameraUnbound { camera: 0, output: o1 }, Effect::PointerState(PointerState::NoOutput)]);
        assert_eq!(h.core.cameras().len(), 1, "camera destroyed on disconnect");
        assert_eq!(h.core.cameras().bound_output(0), None);
        assert_eq!((h.core.cameras().center(0), h.core.cameras().zoom_milli(0)), value, "camera value changed on disconnect");
        let (o2, fx) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        assert_ne!(o1, o2, "OutputId reused");
        assert_eq!(fx, vec![Effect::CameraBound { camera: 0, output: o2 }, Effect::PointerState(PointerState::Located { output: o2, p: (960.0, 540.0) })]);
        assert_eq!(h.core.cameras().len(), 1, "a second camera was created for a remembered OutputKey");
        assert_eq!((h.core.cameras().center(0), h.core.cameras().zoom_milli(0)), value);
    }

    /// PD-F2-18 (2R): persisted state is a passive lookup consulted only by `output_connected`; the
    /// startup connect and a later runtime hotplug of another persisted output take the same path.
    /// A camera already remembered this session takes precedence over the persisted entry.
    #[test]
    fn persisted_lookup_uses_the_single_authorized_path() {
        let persisted = PersistedCameras::from_entries(vec![
            (OutputKey(10), CanvasPos { x: 5000, y: -700 }, 1500),
            (OutputKey(30), CanvasPos { x: -123, y: 456 }, 750),
        ]);
        let (core, mut input, _shell, mut outs) = Core::new(persisted);
        let mut h = handlers(core);
        assert!(h.core.cameras().is_empty(), "persisted state must not create cameras by itself");
        // "startup": the backend enumerates its outputs through the same entry
        let (o10, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        let (o20, _) = h.core.output_connected(&mut outs, desc(20)).unwrap();
        let c10 = h.core.cameras().camera_for_output(o10).unwrap();
        let c20 = h.core.cameras().camera_for_output(o20).unwrap();
        assert_eq!((h.core.cameras().center(c10), h.core.cameras().zoom_milli(c10)), (CanvasPos { x: 5000, y: -700 }, 1500));
        assert_eq!((h.core.cameras().center(c20), h.core.cameras().zoom_milli(c20)), (CanvasPos { x: 0, y: 0 }, 1000));
        // runtime hotplug of a persisted output: identical path, persisted value applied
        let (o30, _) = h.core.output_connected(&mut outs, desc(30)).unwrap();
        let c30 = h.core.cameras().camera_for_output(o30).unwrap();
        assert_eq!((h.core.cameras().center(c30), h.core.cameras().zoom_milli(c30)), (CanvasPos { x: -123, y: 456 }, 750));
        // the session's own value wins over the persisted table on reconnect
        h.core.physical_input(&mut input, PhysicalEvent::PanDelta { camera: c10, dx: 100, dy: 0 }).unwrap();
        let moved = h.core.cameras().center(c10);
        h.core.output_disconnected(&mut outs, o10).unwrap();
        let (o10b, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        assert_eq!(h.core.cameras().camera_for_output(o10b), Some(c10));
        assert_eq!(h.core.cameras().center(c10), moved, "persisted table overrode the session's camera");
        // protocol activity never creates cameras
        let n = h.core.cameras().len();
        h.on_new_toplevel(1);
        h.on_commit(1, (300, 200));
        h.on_activation(1);
        assert_eq!(h.core.cameras().len(), n);
    }

    // ------------------------------------------------------------------ E1R
    fn rdesc(key: u64, mode: (i32, i32), transform: Transform, scale_120: u32, refresh_mhz: u32) -> OutputDescriptor {
        OutputDescriptor { key: OutputKey(key), mode, transform, scale_120, refresh_mhz }
    }

    /// E1R (E-F2-03): mode, scale, transform and refresh changes keep the OutputId, the camera identity
    /// and the camera value; no Authorized proof is minted; no new OutputId is allocated.
    #[test]
    fn output_reconfigured_keeps_identity_and_camera() {
        let (core, mut input, _shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        let (o, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        h.core.physical_input(&mut input, PhysicalEvent::PanDelta { camera: 0, dx: 17, dy: -3 }).unwrap();
        let value = (h.core.cameras().center(0), h.core.cameras().zoom_milli(0));
        for d in [
            rdesc(10, (2560, 1440), Transform::Normal, 120, 60_000),  // mode
            rdesc(10, (2560, 1440), Transform::Normal, 150, 60_000),  // scale 1.25
            rdesc(10, (2560, 1440), Transform::R90, 150, 60_000),     // transform
            rdesc(10, (2560, 1440), Transform::R90, 150, 144_000),    // refresh
        ] {
            let fx = h.core.output_reconfigured(&mut outs, o, d).unwrap();
            assert_eq!(fx[0], Effect::OutputReconfigured { output: o, camera: Some(0) });
            assert!(fx.iter().all(|e| matches!(e, Effect::OutputReconfigured { .. } | Effect::PointerState(_))), "{fx:?}");
            assert_eq!(h.core.outputs().len(), 1, "reconfiguration allocated or duplicated an output");
            assert_eq!(h.core.outputs()[0], (o, d));
            assert_eq!(h.core.cameras().camera_for_output(o), Some(0));
            assert_eq!(h.core.cameras().len(), 1);
            assert_eq!((h.core.cameras().center(0), h.core.cameras().zoom_milli(0)), value, "reconfiguration moved the camera");
        }
        assert_eq!(h.core.outputs()[0].1.view_phys(), (1440, 2560), "view uses the transformed mode");
        assert_eq!(h.core.outputs()[0].1.mode, (2560, 1440), "descriptor keeps the physical mode");
    }

    /// E1R (E-F2-03): reconfiguration is not reconnect: an unknown id, a foreign token or a changed
    /// OutputKey are rejected and mutate nothing. Reconnect (disconnect + connect) allocates a new
    /// OutputId and re-binds the remembered camera — a separate, separately tested concept.
    #[test]
    fn output_reconfigured_rejects_unknown_foreign_and_identity_change() {
        let (core, _input, _shell, mut outs) = Core::new(PersistedCameras::default());
        let (_other, _i, _s, mut foreign) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        let (o, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        let before = (h.core.outputs().to_vec(), h.core.pointer());
        assert_eq!(h.core.output_reconfigured(&mut outs, o + 99, desc(10)).err(), Some(AuthorityError::UnknownOutput));
        assert_eq!(h.core.output_reconfigured(&mut foreign, o, desc(10)).err(), Some(AuthorityError::ForeignPort));
        assert_eq!(h.core.output_reconfigured(&mut outs, o, desc(11)).err(), Some(AuthorityError::IdentityChanged));
        assert_eq!((h.core.outputs().to_vec(), h.core.pointer()), before, "a rejected reconfiguration mutated state");
        // reconnect: a different concept
        h.core.output_disconnected(&mut outs, o).unwrap();
        let (o2, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        assert_ne!(o, o2);
        assert_eq!(h.core.cameras().camera_for_output(o2), Some(0));
        assert_eq!(h.core.output_reconfigured(&mut outs, o, desc(10)).err(), Some(AuthorityError::UnknownOutput), "stale OutputId accepted");
    }

    /// E1R (E-F2-03, F-RECONF-1): the pointer's OutputLogical p is clamped into the new extent
    /// (half-open, wl_fixed); a p that stays inside is unchanged; no relative motion is synthesized.
    #[test]
    fn output_reconfigured_clamps_pointer_into_new_extent() {
        let (core, _input, _shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        let (o, _) = h.core.output_connected(&mut outs, desc(10)).unwrap(); // p = (960, 540)
        // rotate 90 at scale 1.5: logical extent 720 x 1280 -> x clamps to 720 - 2^-8, y unchanged
        let fx = h.core.output_reconfigured(&mut outs, o, rdesc(10, (1920, 1080), Transform::R90, 180, 60_000)).unwrap();
        let want = PointerState::Located { output: o, p: (720.0 - 1.0 / 256.0, 540.0) };
        assert_eq!(fx, vec![Effect::OutputReconfigured { output: o, camera: Some(0) }, Effect::PointerState(want)]);
        assert_eq!(h.core.pointer(), want);
        // back to normal at scale 1: p inside -> unchanged, no pointer effect
        let fx = h.core.output_reconfigured(&mut outs, o, desc(10)).unwrap();
        assert_eq!(fx, vec![Effect::OutputReconfigured { output: o, camera: Some(0) }]);
        assert_eq!(h.core.pointer(), want);
    }

    /// E1R (E-F2-05): one output -> last output removed -> stable zero-output state -> reconnect.
    /// No panic, no fabricated output, no fabricated coordinate; cameras persist unbound.
    #[test]
    fn zero_output_state_is_stable_and_reconnect_reconciles() {
        let (core, mut input, _shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        assert_eq!(h.core.pointer(), PointerState::NoOutput, "before any output: zero-output state");
        let (o1, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        h.core.physical_input(&mut input, PhysicalEvent::PanDelta { camera: 0, dx: 5, dy: 5 }).unwrap();
        let value = (h.core.cameras().center(0), h.core.cameras().zoom_milli(0));
        h.core.output_disconnected(&mut outs, o1).unwrap();
        assert!(h.core.outputs().is_empty());
        assert_eq!(h.core.pointer(), PointerState::NoOutput, "no fabricated output / coordinate");
        assert_eq!(h.core.cameras().bound_output(0), None);
        assert_eq!((h.core.cameras().center(0), h.core.cameras().zoom_milli(0)), value);
        // the zero-output state is stable: repeated / unknown disconnects are rejected, nothing changes
        assert_eq!(h.core.output_disconnected(&mut outs, o1).err(), Some(AuthorityError::UnknownOutput));
        assert_eq!(h.core.pointer(), PointerState::NoOutput);
        // a different monitor arrives first: placement by the startup rule, its own camera
        let (o2, fx) = h.core.output_connected(&mut outs, rdesc(20, (2560, 1440), Transform::R270, 240, 60_000)).unwrap();
        assert!(fx.contains(&Effect::PointerState(PointerState::Located { output: o2, p: (360.0, 640.0) })), "{fx:?}");
        assert_eq!(h.core.cameras().camera_for_output(o2), Some(1));
        // the original monitor returns: same camera, value unchanged; the located pointer is not moved
        let (o3, fx) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        assert_eq!(fx, vec![Effect::CameraBound { camera: 0, output: o3 }]);
        assert_eq!((h.core.cameras().center(0), h.core.cameras().zoom_milli(0)), value);
    }

    /// E1R (E-F2-05): F-RELOCATE-1 is applied only on a non-empty survivor set; with survivors the
    /// pointer moves to the Arrangement-nearest output (spike: left-to-right arrangement).
    #[test]
    fn relocation_uses_survivors_and_only_survivors() {
        let (core, _input, _shell, mut outs) = Core::new(PersistedCameras::default());
        let mut h = handlers(core);
        let (a, _) = h.core.output_connected(&mut outs, desc(1)).unwrap(); // pointer on A at (960,540)
        let (b, _) = h.core.output_connected(&mut outs, desc(2)).unwrap(); // B arranged at x = 1920
        let fx = h.core.output_disconnected(&mut outs, a).unwrap();
        // a = (960,540) in the Arrangement; B's rect is [1920, 3840) x [0, 1080): nearest point x = 1920
        assert!(fx.contains(&Effect::PointerState(PointerState::Located { output: b, p: (0.0, 540.0) })), "{fx:?}");
        let fx = h.core.output_disconnected(&mut outs, b).unwrap();
        assert!(fx.contains(&Effect::PointerState(PointerState::NoOutput)));
    }

    /// E1R (E-F2-04, E-F3-03): two CONNECTED outputs presenting the same OutputKey (e.g. virtual outputs
    /// without a distinguishing connector) each keep their own camera (1:1); the duplicate does not take
    /// the persisted value; on reconnect the lowest unbound camera with the key is re-bound.
    #[test]
    fn concurrent_output_key_collision_keeps_one_to_one() {
        let persisted = PersistedCameras::from_entries(vec![(OutputKey(10), CanvasPos { x: 77, y: 88 }, 1250)]);
        let (core, _input, _shell, mut outs) = Core::new(persisted);
        let mut h = handlers(core);
        let (a, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        let (b, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        let (ca, cb) = (h.core.cameras().camera_for_output(a), h.core.cameras().camera_for_output(b));
        assert!(ca.is_some() && cb.is_some() && ca != cb, "1:1 violated: a={ca:?} b={cb:?}");
        assert_eq!((h.core.cameras().center(ca.unwrap()), h.core.cameras().zoom_milli(ca.unwrap())), (CanvasPos { x: 77, y: 88 }, 1250));
        assert_eq!((h.core.cameras().center(cb.unwrap()), h.core.cameras().zoom_milli(cb.unwrap())), (CanvasPos { x: 0, y: 0 }, 1000), "duplicate seeded from persistence");
        h.core.output_disconnected(&mut outs, a).unwrap();
        h.core.output_disconnected(&mut outs, b).unwrap();
        let (a2, _) = h.core.output_connected(&mut outs, desc(10)).unwrap();
        assert_eq!(h.core.cameras().camera_for_output(a2), ca, "reconnect must take the lowest unbound camera");
    }
}
