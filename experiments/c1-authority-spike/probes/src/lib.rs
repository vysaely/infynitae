#![allow(dead_code, unused_imports, unused_variables)]
use spike_core::{geom::CanvasPos, navigation::OutputKey, Core, InputPort, OutputDescriptor, OutputLifecycleToken, PhysicalEvent};
use spike_wayland::Handlers;

#[cfg(feature = "p1_mutate_through_read_accessor")]
pub fn activation(h: &mut Handlers) {
    h.core.cameras().set_center(0, CanvasPos { x: 1, y: 1 }); // expected E0624 (private method)
}

#[cfg(feature = "p2_forge_input_port")]
pub fn activation(_h: &mut Handlers) -> InputPort {
    InputPort { core_id: 1 } // expected E0451 (private field)
}

#[cfg(feature = "p3_physical_input_without_port")]
pub fn activation(h: &mut Handlers) {
    let _ = h.core.physical_input(PhysicalEvent::PanDelta { camera: 0, dx: 5, dy: 0 }); // expected E0061 (missing argument)
}

#[cfg(feature = "p4_call_navigation_internals")]
pub fn activation(h: &mut Handlers) {
    let _ = spike_core::navigation::Authority::Shell; // expected E0603 (private item)
}

#[cfg(feature = "p5_control_compiles")]
pub fn activation(h: &Handlers) -> CanvasPos {
    h.core.cameras().center(0) // positive control: read access must compile
}

fn desc() -> OutputDescriptor {
    OutputDescriptor { key: OutputKey(1), mode: (1920, 1080), scale_120: 120, refresh_mhz: 60_000 }
}

#[cfg(feature = "p6_output_connected_without_token")]
pub fn activation(h: &mut Handlers) {
    let _ = h.core.output_connected(desc()); // expected E0061 (missing token argument)
}

#[cfg(feature = "p7_forge_output_token")]
pub fn activation(_h: &mut Handlers) -> OutputLifecycleToken {
    OutputLifecycleToken { core_id: 1 } // expected: cannot construct (private fields)
}

#[cfg(feature = "p8_descriptor_with_coordinates")]
pub fn activation(_h: &mut Handlers) -> OutputDescriptor {
    // a caller trying to smuggle camera placement through the descriptor: expected E0560 (no such field)
    OutputDescriptor { key: OutputKey(1), mode: (1920, 1080), scale_120: 120, refresh_mhz: 60_000, center: CanvasPos { x: 5, y: 5 } }
}

#[cfg(feature = "p9_public_restore_api")]
pub fn activation(h: &mut Handlers) {
    h.core.restore_persisted_state(); // expected E0599 (no such method: no public restore API)
}
