//! Infynitae Native headless backend: virtual outputs and the test renderer, so tests exercise
//! the production core and adapter (FOUNDATION_ARCHITECTURE C1.1, NB-12).
//!
//! INC-1 state: one virtual output, connected through the output-lifecycle token, and frame
//! production with Smithay's `DummyRenderer`. No virtual vblank, hotplug or input injection yet
//! (INC-2, INC-3).

#![deny(unsafe_code)]

use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::render::FrameEntry;
use infynitae_core::{EntryError, FrameError, OutputLifecycleToken};
use infynitae_wayland::{WaylandState, convert};
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::test::DummyRenderer;

/// A virtual output: hardware facts only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualOutput {
    /// Physical mode before the transform.
    pub mode: (u32, u32),
    pub transform: Transform,
    pub scale_120: u32,
    pub refresh_mhz: u32,
}

impl VirtualOutput {
    pub fn descriptor(&self) -> OutputDescriptor {
        OutputDescriptor {
            key: OutputKey::opaque(),
            mode: self.mode,
            transform: self.transform,
            scale_120: self.scale_120,
            refresh_mhz: self.refresh_mhz,
        }
    }
}

/// Output discovery: the backend's output source holds the token and connects through it.
pub fn connect(
    state: &mut WaylandState,
    token: &mut OutputLifecycleToken,
    output: &VirtualOutput,
) -> Result<OutputId, EntryError> {
    state
        .core_mut()
        .output_connected(token, output.descriptor())
}

#[derive(Debug)]
pub enum RenderError {
    Frame(FrameError),
    Renderer(<DummyRenderer as smithay::backend::renderer::RendererSuper>::Error),
}

/// One frame of an output: Core's render list (recorded as its ProjectionFrame) turned into
/// Smithay render elements with the test renderer, front to back.
pub fn render_frame(
    state: &mut WaylandState,
    renderer: &mut DummyRenderer,
    output: OutputId,
) -> Result<Vec<(FrameEntry, WaylandSurfaceRenderElement<DummyRenderer>)>, RenderError> {
    let entries = state
        .core_mut()
        .render_list(output)
        .map_err(RenderError::Frame)?
        .entries
        .clone();
    convert::render_elements(renderer, state.registry(), &entries).map_err(RenderError::Renderer)
}
