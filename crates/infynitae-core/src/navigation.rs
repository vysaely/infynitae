//! The only module that may mutate cameras (INV-CAMERA-1, C1.5).
//!
//! INC-0: shape only. No camera is ever created, and nothing calls the mutator; camera creation
//! and 1:1 output binding arrive with INC-2, navigation with INC-3.

use crate::authority::Authorized;
use crate::geom::CanvasPos;

pub struct CameraSet {
    center: Vec<CanvasPos>,
}

impl CameraSet {
    pub(crate) fn empty() -> Self {
        CameraSet { center: Vec::new() }
    }

    /// Read access is public.
    pub fn center(&self, camera: usize) -> Option<CanvasPos> {
        self.center.get(camera).copied()
    }

    /// Mutation is private to this module and consumes a proof.
    #[expect(
        dead_code,
        reason = "INC-0 authority shape; first caller arrives with INC-2/INC-3"
    )]
    fn set_center(&mut self, _proof: Authorized, camera: usize, center: CanvasPos) {
        if let Some(c) = self.center.get_mut(camera) {
            *c = center;
        }
    }
}

/// The trusted source of a navigation request. Not visible outside core.
#[expect(
    dead_code,
    reason = "INC-0 authority shape; first use arrives with INC-3"
)]
pub(crate) enum Authority {
    PhysicalInput,
    Shell,
}
