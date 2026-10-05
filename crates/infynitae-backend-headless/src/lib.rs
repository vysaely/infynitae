//! Infynitae Native headless backend: virtual outputs, virtual vblank and the test renderer, so
//! tests exercise the production core and adapter (FOUNDATION_ARCHITECTURE C1.1, NB-12).
//!
//! INC-2 state: any number of virtual outputs connected, reconfigured and disconnected through
//! the output-lifecycle token (startup and hotplug alike); virtual vblank per output driving
//! Core's frame clock; frame production through the adapter's damage trackers. No input
//! injection yet (INC-3).

#![deny(unsafe_code)]

use std::time::Duration;

use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::render::FrameEntry;
use infynitae_core::{EntryError, FrameError, OutputLifecycleToken};
use infynitae_wayland::{WaylandState, convert};
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::test::DummyRenderer;

/// A virtual output: hardware facts only.
#[derive(Clone, Debug, PartialEq)]
pub struct VirtualOutput {
    /// Persistence identity the backend derives (here: a virtual connector).
    pub key: OutputKey,
    /// Physical mode before the transform.
    pub mode: (u32, u32),
    pub transform: Transform,
    pub scale_120: u32,
    pub refresh_mhz: u32,
}

impl VirtualOutput {
    pub fn descriptor(&self) -> OutputDescriptor {
        OutputDescriptor {
            key: self.key.clone(),
            mode: self.mode,
            transform: self.transform,
            scale_120: self.scale_120,
            refresh_mhz: self.refresh_mhz,
        }
    }

    /// The key of a headless virtual connector (`device` "headless", no EDID serial).
    pub fn key(connector: &str) -> OutputKey {
        OutputKey {
            device: "headless".into(),
            connector: connector.into(),
            make: "Infynitae".into(),
            model: "Virtual".into(),
            serial: None,
        }
    }
}

/// Output discovery or hotplug: the backend's output source holds the token and connects through
/// it; the adapter applies Core's effects (global, enters).
pub fn connect(
    state: &mut WaylandState,
    token: &mut OutputLifecycleToken,
    output: &VirtualOutput,
) -> Result<OutputId, EntryError> {
    let (id, effects) = state
        .core_mut()
        .output_connected(token, output.descriptor())?;
    state.apply(&effects);
    Ok(id)
}

/// A mode, transform, scale or refresh change of a connected virtual output.
pub fn reconfigure(
    state: &mut WaylandState,
    token: &mut OutputLifecycleToken,
    id: OutputId,
    output: &VirtualOutput,
) -> Result<(), EntryError> {
    let effects = state
        .core_mut()
        .output_reconfigured(token, id, output.descriptor())?;
    state.apply(&effects);
    Ok(())
}

/// Unplug: leaves first, then the global is destroyed (INV-OUTPUT-1).
pub fn disconnect(
    state: &mut WaylandState,
    token: &mut OutputLifecycleToken,
    id: OutputId,
) -> Result<(), EntryError> {
    let effects = state.core_mut().output_disconnected(token, id)?;
    state.apply(&effects);
    Ok(())
}

/// A virtual vblank of `output` at `now` (monotonic): Core's frame clock decides which observed
/// elements complete, the adapter completes their callbacks and presents their feedback.
pub fn vblank(state: &mut WaylandState, output: OutputId, now: Duration) {
    let elements = state.core().frame_tick(output);
    state.complete_frames(output, &elements, now);
}

/// The virtual vblank period of an output (refresh in mHz).
pub fn vblank_period(refresh_mhz: u32) -> Duration {
    Duration::from_nanos(1_000_000_000_000 / u64::from(refresh_mhz.max(1)))
}

#[derive(Debug)]
pub enum RenderError {
    Frame(FrameError),
    Renderer(<DummyRenderer as smithay::backend::renderer::RendererSuper>::Error),
}

/// Core's render list for an output (recorded as its ProjectionFrame) turned into Smithay render
/// elements with the test renderer, front to back.
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
