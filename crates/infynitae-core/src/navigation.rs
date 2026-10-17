//! The only module that may mutate cameras (INV-CAMERA-1, C1.5).
//!
//! INC-2: cameras are created, bound and unbound only through the output-lifecycle entries
//! (M5): binding precedence remembered-and-unbound → persisted (no collision) → default, 1:1
//! (INV-CAMERA-3). INC-3: direct-manipulation pan (F-PAN-1) from physical input. Jump and fit
//! arrive later.

use crate::authority::Authorized;
use crate::geom::{CANVAS_LIMIT_PX, CanvasPoint, CanvasPos, Q16_ONE, q16};
use crate::output::{OutputId, OutputKey};

#[derive(Debug)]
struct Camera {
    center: CanvasPos,
    zoom: f64,
    /// The `OutputKey` the camera is remembered under (cameras survive outputs).
    key: OutputKey,
    /// 1:1 binding (INV-CAMERA-3): a camera is bound to at most one output.
    bound: Option<OutputId>,
}

/// OBJ-09: persisted camera state, a passive read-only lookup given to `Core::new` and consulted
/// only inside `output_connected` (INV-OUTPUT-2). There is no restore API.
#[derive(Debug, Default)]
pub struct PersistedCameras {
    entries: Vec<(OutputKey, CanvasPos, f64)>,
}

/// A persisted entry Core refuses (the file is validated all-or-nothing, INV-FAIL-1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersistedError {
    /// Zoom outside the representable domain ([`crate::geom::zoom_in_domain`]).
    Zoom { entry: usize },
    /// Centre outside ±2³⁶ px (INV-COORD-1).
    Centre { entry: usize },
    /// Two entries for the same key.
    DuplicateKey { entry: usize },
}

impl PersistedCameras {
    /// Builds the lookup from validated persistence entries `(key, centre, zoom)`; any invalid
    /// entry rejects the whole table.
    pub fn from_entries(entries: Vec<(OutputKey, CanvasPos, f64)>) -> Result<Self, PersistedError> {
        for (i, (key, centre, zoom)) in entries.iter().enumerate() {
            if !crate::geom::zoom_in_domain(*zoom) {
                return Err(PersistedError::Zoom { entry: i });
            }
            if CanvasPoint::new(centre.x >> 16, centre.y >> 16).is_none() {
                return Err(PersistedError::Centre { entry: i });
            }
            if entries[..i].iter().any(|(k, _, _)| k == key) {
                return Err(PersistedError::DuplicateKey { entry: i });
            }
        }
        Ok(PersistedCameras { entries })
    }

    fn lookup(&self, key: &OutputKey) -> Option<(CanvasPos, f64)> {
        self.entries
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, c, z)| (*c, *z))
    }
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

    pub fn key(&self, camera: usize) -> Option<&OutputKey> {
        self.cameras.get(camera).map(|c| &c.key)
    }

    pub fn bound_output(&self, camera: usize) -> Option<OutputId> {
        self.cameras.get(camera).and_then(|c| c.bound)
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

    /// M5 on `output_connected`: re-binds the lowest-indexed **unbound** camera remembered for the
    /// key (value unchanged); otherwise creates a camera seeded from the persisted lookup unless a
    /// camera with that key already exists this session (a concurrent key collision), else at the
    /// internal default placement (Canvas origin, zoom 1; Level D). Returns the camera index.
    pub(crate) fn bind_on_connect(
        &mut self,
        _proof: Authorized,
        key: &OutputKey,
        output: OutputId,
        persisted: &PersistedCameras,
    ) -> usize {
        #[cfg(feature = "probe_sealed_key_steal")]
        // NEGATIVE CONTROL (runtime 2i, OB-49): the sealed 2R rule re-binds a remembered camera
        // even while it is bound, stealing it from its output.
        let free = self.cameras.iter().position(|c| &c.key == key);
        #[cfg(not(feature = "probe_sealed_key_steal"))]
        let free = self
            .cameras
            .iter()
            .position(|c| &c.key == key && c.bound.is_none());
        if let Some(i) = free {
            self.cameras[i].bound = Some(output);
            return i;
        }
        let collision = self.cameras.iter().any(|c| &c.key == key);
        let seeded = if collision {
            None
        } else {
            persisted.lookup(key)
        };
        let (center, zoom) = seeded.unwrap_or((CanvasPos { x: 0, y: 0 }, 1.0));
        // every camera zoom is in the representable domain: a persisted one was validated, the
        // default is 1 (F-VIEW-1, C12.F14). A future zoom writer must validate too.
        debug_assert!(
            crate::geom::zoom_in_domain(zoom),
            "zoom {zoom} outside the domain"
        );
        self.cameras.push(Camera {
            center,
            zoom,
            key: key.clone(),
            bound: Some(output),
        });
        self.cameras.len() - 1
    }

    /// M5 on `output_disconnected`: the camera is unbound and kept with its value.
    pub(crate) fn unbind_on_disconnect(
        &mut self,
        _proof: Authorized,
        output: OutputId,
    ) -> Option<usize> {
        let i = self.camera_for_output(output)?;
        self.cameras[i].bound = None;
        Some(i)
    }

    /// F-PAN-1 update of a pan gesture: the camera becomes `start − Q16(sum)`, quantised once from
    /// the f64 sum of the whole gesture (R2, INV-COORD-3). Consumes a proof (INV-CAMERA-1).
    pub(crate) fn pan(
        &mut self,
        proof: Authorized,
        camera: usize,
        start: CanvasPos,
        sum: (f64, f64),
    ) {
        self.set_center(proof, camera, pan_target(start, sum));
    }

    /// Mutation is private to this module and consumes a proof.
    fn set_center(&mut self, _proof: Authorized, camera: usize, center: CanvasPos) {
        if let Some(c) = self.cameras.get_mut(camera) {
            c.center = center;
        }
    }
}

/// F-PAN-1: `C = C_start − Q16(Σ)`, Q16 ties-even, saturated at the stored-Canvas bound ±2³⁶ px
/// (INV-COORD-1). Pure.
pub fn pan_target(start: CanvasPos, sum: (f64, f64)) -> CanvasPos {
    let limit = CANVAS_LIMIT_PX * Q16_ONE;
    let axis = |c: i64, s: f64| c.saturating_sub(q16(s)).clamp(-limit, limit);
    CanvasPos {
        x: axis(start.x, sum.0),
        y: axis(start.y, sum.1),
    }
}

/// The trusted source of a navigation request. Not visible outside core. The physical-input pan
/// is identified by its mint site; the distinction is first needed by shell navigation commands.
#[expect(
    dead_code,
    reason = "authority shape; first use arrives with INC-11 (shell navigation commands)"
)]
pub(crate) enum Authority {
    PhysicalInput,
    Shell,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::tests::key;

    fn pos(x: i64, y: i64) -> CanvasPos {
        CanvasPos {
            x: x << 16,
            y: y << 16,
        }
    }

    /// F-PAN-1 oracle: hand-derived Q16 values (ties-even at the Q16 half step) and saturation.
    #[test]
    fn pan_target_quantises_the_sum_once() {
        let s = pos(100, -50);
        assert_eq!(pan_target(s, (219.0, -63.0)), pos(-119, 13));
        // 0.5 / 65536 px is a tie: rounds to the even Q16 value 0
        let half = 0.5 / 65536.0;
        assert_eq!(pan_target(s, (half, 0.0)), s);
        // 1.5 / 65536 px rounds to 2 units
        let r = pan_target(s, (3.0 * half, 0.0));
        assert_eq!(r.x, s.x - 2);
        let far = pan_target(s, (-1e30, 1e30));
        assert_eq!((far.x, far.y), ((1 << 36) << 16, -((1 << 36) << 16)));
    }

    #[test]
    fn persisted_table_is_validated_all_or_nothing() {
        assert!(PersistedCameras::from_entries(vec![(key("A"), pos(1, 2), 1.5)]).is_ok());
        assert_eq!(
            PersistedCameras::from_entries(vec![
                (key("A"), pos(1, 2), 1.0),
                (key("B"), pos(0, 0), 0.0)
            ])
            .err(),
            Some(PersistedError::Zoom { entry: 1 })
        );
        assert_eq!(
            PersistedCameras::from_entries(vec![(key("A"), pos((1 << 36) + 1, 0), 1.0)]).err(),
            Some(PersistedError::Centre { entry: 0 })
        );
        assert_eq!(
            PersistedCameras::from_entries(vec![
                (key("A"), pos(0, 0), 1.0),
                (key("A"), pos(5, 5), 1.0)
            ])
            .err(),
            Some(PersistedError::DuplicateKey { entry: 1 })
        );
    }
}
