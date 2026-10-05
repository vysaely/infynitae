//! The only module that may mutate cameras (INV-CAMERA-1, C1.5).
//!
//! INC-1: a camera is created and bound only through `output_connected` (one output, default
//! placement, zoom 1). Binding precedence across several outputs, unbinding and navigation
//! (pan, jump, fit) arrive with INC-2 and INC-3.

use crate::authority::Authorized;
use crate::geom::CanvasPos;
use crate::output::OutputId;

#[derive(Debug)]
struct Camera {
    center: CanvasPos,
    zoom: f64,
    /// 1:1 binding (INV-CAMERA-3): a camera is bound to at most one output.
    bound: Option<OutputId>,
}

#[derive(Debug)]
pub struct CameraSet {
    cameras: Vec<Camera>,
}

impl CameraSet {
    pub(crate) fn empty() -> Self {
        CameraSet {
            cameras: Vec::new(),
        }
    }

    /// Read access is public.
    pub fn center(&self, camera: usize) -> Option<CanvasPos> {
        self.cameras.get(camera).map(|c| c.center)
    }

    pub fn zoom(&self, camera: usize) -> Option<f64> {
        self.cameras.get(camera).map(|c| c.zoom)
    }

    pub fn camera_for_output(&self, output: OutputId) -> Option<usize> {
        self.cameras.iter().position(|c| c.bound == Some(output))
    }

    pub fn len(&self) -> usize {
        self.cameras.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cameras.is_empty()
    }

    /// Creates a camera at Core's internal default placement (centre at the Canvas origin, zoom 1;
    /// a Level D policy, INV-CAMERA-3 rule 3) and binds it to `output`. The caller guarantees the
    /// output has no camera; INC-1 admits a single output.
    pub(crate) fn create_bound(&mut self, _proof: Authorized, output: OutputId) -> usize {
        self.cameras.push(Camera {
            center: CanvasPos { x: 0, y: 0 },
            zoom: 1.0,
            bound: Some(output),
        });
        self.cameras.len() - 1
    }

    /// Mutation is private to this module and consumes a proof.
    #[expect(
        dead_code,
        reason = "authority shape; first caller arrives with INC-3 (navigation)"
    )]
    fn set_center(&mut self, _proof: Authorized, camera: usize, center: CanvasPos) {
        if let Some(c) = self.cameras.get_mut(camera) {
            c.center = center;
        }
    }
}

/// The trusted source of a navigation request. Not visible outside core.
#[expect(
    dead_code,
    reason = "authority shape; first use arrives with INC-3 (navigation)"
)]
pub(crate) enum Authority {
    PhysicalInput,
    Shell,
}
