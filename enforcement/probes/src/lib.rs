//! Protocol-side negative compile probes (OB-19), ported from experiments/c1-authority-spike and
//! written against the production crates. A Smithay handler holds `&mut Core` through the protocol
//! state but no authority port; each probe is a function with exactly that access.
//!
//! The expected diagnostic of every probe is asserted by tools/verify-architecture.

#![deny(unsafe_code)]
#![allow(dead_code, unused_imports, unused_variables)]

use infynitae_core::geom::CanvasPos;
use infynitae_core::output::{OutputDescriptor, OutputId};
use infynitae_core::{Core, InputPort, OutputLifecycleToken, PhysicalEvent};

/// p1: mutate a camera through the read accessor. Expected E0624 (`set_center` is private).
#[cfg(feature = "p1_mutate_through_read_accessor")]
pub fn handler(core: &mut Core, proof: infynitae_core::authority::Authorized) {
    core.cameras()
        .set_center(proof, 0, CanvasPos { x: 1, y: 1 });
}

/// p2: forge an input port. Expected: cannot construct `InputPort` (private fields).
#[cfg(feature = "p2_forge_input_port")]
pub fn handler(_core: &mut Core) -> InputPort {
    InputPort { core_id: 1 }
}

/// p3: submit physical input without a port. Expected E0061 (missing argument).
#[cfg(feature = "p3_physical_input_without_port")]
pub fn handler(core: &mut Core, ev: PhysicalEvent) {
    let _ = core.physical_input(ev);
}

/// p4: reach the navigation internals. Expected E0603 (`Authority` is private).
#[cfg(feature = "p4_call_navigation_internals")]
pub fn handler(_core: &mut Core) {
    let _ = infynitae_core::navigation::Authority::Shell;
}

/// p5: positive control. Read access must compile.
#[cfg(feature = "p5_control_compiles")]
pub fn handler(core: &Core) -> Option<CanvasPos> {
    core.cameras().center(0)
}

/// p6: connect an output without the lifecycle token. Expected E0061.
#[cfg(feature = "p6_output_connected_without_token")]
pub fn handler(core: &mut Core, desc: OutputDescriptor) {
    let _ = core.output_connected(desc);
}

/// p7: forge the lifecycle token. Expected: cannot construct `OutputLifecycleToken` (private fields).
#[cfg(feature = "p7_forge_output_token")]
pub fn handler(_core: &mut Core) -> OutputLifecycleToken {
    OutputLifecycleToken { core_id: 1 }
}

/// p8: smuggle a camera position through the descriptor. Expected E0560 (no such field).
#[cfg(feature = "p8_descriptor_with_coordinates")]
pub fn handler(_core: &mut Core, base: OutputDescriptor) -> OutputDescriptor {
    OutputDescriptor {
        center: CanvasPos { x: 5, y: 5 },
        ..base
    }
}

/// p9: a public restore API. Expected E0599 (no such method).
#[cfg(feature = "p9_public_restore_api")]
pub fn handler(core: &mut Core) {
    core.restore_persisted_state();
}

/// p10: reconfigure an output without the token. Expected E0061.
#[cfg(feature = "p10_reconfigure_without_token")]
pub fn handler(core: &mut Core, output: OutputId, desc: OutputDescriptor) {
    let _ = core.output_reconfigured(output, desc);
}

/// p11: reconfigure with the input port instead of the token. Expected E0308.
#[cfg(feature = "p11_reconfigure_with_input_port")]
pub fn handler(core: &mut Core, port: &mut InputPort, output: OutputId, desc: OutputDescriptor) {
    let _ = core.output_reconfigured(port, output, desc);
}

/// p12: smuggle a zoom through a reconfiguration descriptor. Expected E0560.
#[cfg(feature = "p12_reconfigure_descriptor_with_zoom")]
pub fn handler(_core: &mut Core, base: OutputDescriptor) -> OutputDescriptor {
    OutputDescriptor {
        zoom_milli: 2000,
        ..base
    }
}

/// p13: pass a camera position alongside the descriptor. Expected E0061.
#[cfg(feature = "p13_reconfigure_with_camera_argument")]
pub fn handler(
    core: &mut Core,
    token: &mut OutputLifecycleToken,
    output: OutputId,
    desc: OutputDescriptor,
) {
    let _ = core.output_reconfigured(token, output, desc, CanvasPos { x: 5, y: 5 });
}

fn require_clone<T: Clone>() {}

/// p14: duplicate the lifecycle token. Expected E0277 (`Clone` not implemented).
#[cfg(feature = "p14_clone_output_token")]
pub fn handler() {
    require_clone::<OutputLifecycleToken>();
}

/// p15: duplicate the input port. Expected E0277.
#[cfg(feature = "p15_clone_input_port")]
pub fn handler() {
    require_clone::<InputPort>();
}

/// p16: duplicate the shell port. Expected E0277.
#[cfg(feature = "p16_clone_shell_port")]
pub fn handler() {
    require_clone::<infynitae_core::ShellPort>();
}

/// p17: duplicate an authority proof. Expected E0277.
#[cfg(feature = "p17_clone_authorized")]
pub fn handler() {
    require_clone::<infynitae_core::authority::Authorized>();
}

/// p18 (INC-3, attack O): hit-test with a Canvas position where an OutputLogical point is required.
/// Expected E0308 (coordinate domains are distinct types, INV-COORD-4).
#[cfg(feature = "p18_hit_test_with_canvas_position")]
pub fn handler(core: &mut Core, o: OutputId) {
    let _ = core.hit_test(o, CanvasPos { x: 0, y: 0 });
}

/// p19 (INC-3, attack O): a Canvas value submitted as a device delta. Expected E0308.
#[cfg(feature = "p19_canvas_value_as_device_delta")]
pub fn handler(core: &mut Core, port: &mut InputPort) {
    let _ = core.physical_input(
        port,
        PhysicalEvent::PointerMotion {
            delta: CanvasPos { x: 1, y: 1 },
            delta_unaccel: (1.0, 1.0),
            time_us: 0,
        },
    );
}

/// p20 (INC-3): pan a camera from outside core. Expected E0624 (`pan` is crate-private and
/// consumes a proof only `entry` can mint).
#[cfg(feature = "p20_pan_from_outside_core")]
pub fn handler(core: &mut Core, proof: infynitae_core::authority::Authorized) {
    core.cameras()
        .pan(proof, 0, CanvasPos { x: 0, y: 0 }, (1.0, 0.0));
}

/// p21 (INC-3): place the pointer from outside core (a second pointer writer). Expected E0603
/// (`locate` is crate-private; OBJ-20 has one writer).
#[cfg(feature = "p21_place_pointer_from_outside_core")]
pub fn handler(_core: &mut Core) {
    let _ = infynitae_core::pointer::locate;
}
