//! The renderer's device capability and the one path by which it reaches Core (OB-13).
//!
//! Core enforces the device ceiling `D_max` (F-SCALE-2, P7); it does not know where the number
//! comes from. A backend's renderer reports it through [`RendererLimits`], and
//! [`WaylandState::report_renderer_limits`] is the only place outside Core that hands a ceiling
//! to Core (tools/verify-architecture, rule `device-limit-only-through-renderer-limits`). A
//! backend therefore cannot pass a bare number: it has to name a renderer that reports one.
//!
//! Which renderers really ask a device is a separate matter. The headless renderer is a
//! fixture that reports a documented, configurable limit. Asking a real GPU, and asking again
//! when the renderer is recreated, is OB-52 (INC-13) and is not done by anything here.

use infynitae_core::scale::ScaleConfigError;

use crate::WaylandState;

/// What a renderer reports about the device it draws with.
pub trait RendererLimits {
    /// The largest side, in device pixels, of a buffer the renderer can take (`D_max`).
    fn max_buffer_side(&self) -> Result<u32, LimitQueryError>;
}

/// The renderer could not say what its limit is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitQueryError(pub String);

/// Why a renderer's limit did not become Core's ceiling. In both cases Core keeps the ceiling
/// it had and no client is told anything.
#[derive(Clone, Debug, PartialEq)]
pub enum DeviceLimitError {
    /// The renderer's query failed.
    Query(LimitQueryError),
    /// The renderer reported a limit Core refuses (0: no buffer fits such a device).
    Refused(ScaleConfigError),
}

impl std::fmt::Display for DeviceLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceLimitError::Query(LimitQueryError(why)) => {
                write!(f, "the renderer did not report its device limit: {why}")
            }
            DeviceLimitError::Refused(e) => {
                write!(f, "the renderer's device limit was refused: {e:?}")
            }
        }
    }
}

impl std::error::Error for DeviceLimitError {}

impl WaylandState {
    /// Asks `renderer` for its device limit, hands it to Core and applies what Core decides
    /// (lowered scale hints for elements that no longer fit, P7). Returns the limit now in
    /// force. Called at startup and whenever the renderer's device may have changed.
    pub fn report_renderer_limits(
        &mut self,
        renderer: &impl RendererLimits,
    ) -> Result<u32, DeviceLimitError> {
        let d_max = renderer
            .max_buffer_side()
            .map_err(DeviceLimitError::Query)?;
        let fx = self
            .core
            .set_renderer_device_max(d_max)
            .map_err(DeviceLimitError::Refused)?;
        self.apply(&fx);
        Ok(d_max)
    }
}
