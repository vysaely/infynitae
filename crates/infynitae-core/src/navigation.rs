//! The only module that may mutate cameras (INV-CAMERA-1, C1.5).
//!
//! INC-2: cameras are created, bound and unbound only through the output-lifecycle entries
//! (M5): binding precedence remembered-and-unbound → persisted (no collision) → default, 1:1
//! (INV-CAMERA-3). INC-3: direct-manipulation pan (F-PAN-1) from physical input. INC-10: the
//! one-shot viewport fit (F-FIT-1) and restore, with the bounded navigation history (OBJ-30,
//! INV-CAMERA-5 as amended by C12.F16). Jump arrives later.

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
    /// OBJ-30: the values before the fits of the current binding; emptied at unbinding (C12.F16).
    history: NavigationHistory,
}

/// A camera's value, as stored: the centre in Q16 and the zoom. An OBJ-30 entry is a copy of one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraValue {
    pub center: CanvasPos,
    pub zoom: f64,
}

/// OBJ-30's capacity: entries per bound camera, i.e. per output (C12.F16).
pub const HISTORY_CAPACITY: usize = 256;

/// OBJ-30 (C12.F16): a bounded stack of exact prior camera values — a fixed ring of
/// [`HISTORY_CAPACITY`] entries, reserved at most once per binding at its first fit and never
/// grown; at capacity a push overwrites the oldest entry in place. An empty history holds no
/// storage. Written only through [`CameraSet`]'s fit, restore and unbinding.
#[derive(Debug, Default)]
pub struct NavigationHistory {
    ring: Option<Box<[CameraValue; HISTORY_CAPACITY]>>,
    /// Index of the oldest entry.
    head: usize,
    len: usize,
}

impl NavigationHistory {
    /// Retains `v` as the newest entry; when full, the oldest is evicted first.
    fn push(&mut self, v: CameraValue) {
        let ring = self
            .ring
            .get_or_insert_with(|| Box::new([v; HISTORY_CAPACITY]));
        if self.len == HISTORY_CAPACITY {
            ring[self.head] = v;
            self.head = (self.head + 1) % HISTORY_CAPACITY;
        } else {
            ring[(self.head + self.len) % HISTORY_CAPACITY] = v;
            self.len += 1;
        }
    }

    /// Removes and returns the newest entry.
    fn pop(&mut self) -> Option<CameraValue> {
        let ring = self.ring.as_ref()?;
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(ring[(self.head + self.len) % HISTORY_CAPACITY])
    }

    /// The binding ended: every entry and the storage go.
    fn clear(&mut self) {
        *self = NavigationHistory::default();
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The entries the storage can hold: [`HISTORY_CAPACITY`] once reserved, else 0.
    pub fn reserved(&self) -> usize {
        if self.ring.is_some() {
            HISTORY_CAPACITY
        } else {
            0
        }
    }

    /// The entries, oldest first.
    pub fn values(&self) -> impl Iterator<Item = CameraValue> + '_ {
        (0..self.len).filter_map(move |i| {
            self.ring
                .as_ref()
                .map(|r| r[(self.head + i) % HISTORY_CAPACITY])
        })
    }
}

/// F-FIT-1 (C12.F15) for a view of `view_size` physical px at output scale `scale` and a window
/// geometry `g = (x, y, w, h)` in Canvas px: `None` (refused) when `g.w ≤ 0` or `g.h ≤ 0`;
/// otherwise the centre of g in Q16 (exact: g is integral), each component clamped into
/// INV-COORD-1, and `zoom = clamp(min(view_w / g.w, view_h / g.h) / scale, 2⁻²⁰, 2¹⁶)`. Pure.
/// F-FIT-1 within the output's usable area (L4, layer C12.F19 prepared): the window fits the
/// usable area `(x, y, w, h)` (output-logical px; `extent` the output's logical size) and its
/// centre is shown at the usable area's centre. With nothing reserved (`usable` = the whole
/// extent) this is exactly [`fit_target`].
pub fn fit_target_in(
    view_size: (u32, u32),
    extent: (f64, f64),
    usable: (f64, f64, f64, f64),
    scale: f64,
    g: (i64, i64, i64, i64),
) -> Option<CameraValue> {
    if usable == (0.0, 0.0, extent.0, extent.1) {
        return fit_target(view_size, scale, g);
    }
    // the usable area in physical px, as the view is
    let size = (
        (usable.2 * scale).round().max(1.0) as u32,
        (usable.3 * scale).round().max(1.0) as u32,
    );
    let mut t = fit_target(size, scale, g)?;
    // the usable centre's offset from the view centre, in Canvas px at the fitted zoom
    let d = (
        (usable.0 + usable.2 / 2.0 - extent.0 / 2.0) / t.zoom,
        (usable.1 + usable.3 / 2.0 - extent.1 / 2.0) / t.zoom,
    );
    let limit = CANVAS_LIMIT_PX * Q16_ONE;
    t.center = CanvasPos {
        x: t.center.x.saturating_sub(q16(d.0)).clamp(-limit, limit),
        y: t.center.y.saturating_sub(q16(d.1)).clamp(-limit, limit),
    };
    Some(t)
}

pub fn fit_target(
    view_size: (u32, u32),
    scale: f64,
    g: (i64, i64, i64, i64),
) -> Option<CameraValue> {
    let (x, y, w, h) = g;
    if w <= 0 || h <= 0 {
        return None;
    }
    let ratio = (view_size.0 as f64 / w as f64).min(view_size.1 as f64 / h as f64);
    let zoom = (ratio / scale).clamp(crate::geom::ZOOM_MIN, crate::geom::ZOOM_MAX);
    let limit = CANVAS_LIMIT_PX * Q16_ONE;
    // Q16 of x + w/2 is (2x + w)·2¹⁵, exactly; i128 for |x| ≤ 2³⁶ and any i64 w
    let centre = |o: i64, extent: i64| {
        ((2 * o as i128 + extent as i128) * (Q16_ONE as i128 / 2))
            .clamp(-(limit as i128), limit as i128) as i64
    };
    Some(CameraValue {
        center: CanvasPos {
            x: centre(x, w),
            y: centre(y, h),
        },
        zoom,
    })
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
            history: NavigationHistory::default(),
        });
        self.cameras.len() - 1
    }

    /// M5 on `output_disconnected`: the camera is unbound and kept with its value; its navigation
    /// history ends with the binding, under the same proof (C12.F16), so no entry reaches another
    /// output or a later connection.
    pub(crate) fn unbind_on_disconnect(
        &mut self,
        _proof: Authorized,
        output: OutputId,
    ) -> Option<usize> {
        let i = self.camera_for_output(output)?;
        self.cameras[i].bound = None;
        self.cameras[i].history.clear();
        Some(i)
    }

    /// OBJ-30 of a camera: read access (public, like every camera reader).
    pub fn history(&self, camera: usize) -> Option<&NavigationHistory> {
        self.cameras.get(camera).map(|c| &c.history)
    }

    /// F-FIT-1's one mutation: the camera's exact value is retained as the newest history entry
    /// (the oldest evicted when 256 are held), then the camera becomes `target`. Consumes a proof.
    pub(crate) fn fit(&mut self, _proof: Authorized, camera: usize, target: CameraValue) {
        if let Some(c) = self.cameras.get_mut(camera) {
            debug_assert!(crate::geom::zoom_in_domain(target.zoom));
            c.history.push(CameraValue {
                center: c.center,
                zoom: c.zoom,
            });
            c.center = target.center;
            c.zoom = target.zoom;
        }
    }

    /// Restore (M8): the newest entry is removed and applied verbatim — no recomputation,
    /// re-quantisation or clamp; `false` (refused, nothing changed) when the history is empty.
    /// Consumes a proof.
    pub(crate) fn restore(&mut self, _proof: Authorized, camera: usize) -> bool {
        let Some(c) = self.cameras.get_mut(camera) else {
            return false;
        };
        let Some(v) = c.history.pop() else {
            return false;
        };
        c.center = v.center;
        c.zoom = v.zoom;
        true
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

    /// A shell `Navigate` (C7.2, INC-11): the camera's centre becomes `center`, its zoom unchanged;
    /// no history entry (OBJ-30 records fits). Consumes a proof (INV-CAMERA-1).
    pub(crate) fn navigate(&mut self, proof: Authorized, camera: usize, center: CanvasPos) {
        self.set_center(proof, camera, center);
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

    // ---- INC-10: F-FIT-1 (C12.F15), the pure computation; the history on the production path is
    // tested through Core's entries (lib.rs tests)

    /// The lower clamp: at scale 1 on a 1920 × 1080 view, a window 1920·2²⁰ px wide fits at exactly
    /// 2⁻²⁰; one pixel wider is clamped to it; one narrower is not clamped.
    #[test]
    fn fit_zoom_clamps_at_the_lower_bound() {
        let w = 1920i64 << 20;
        let at = fit_target((1920, 1080), 1.0, (0, 0, w, 1)).unwrap().zoom;
        assert_eq!(at, crate::geom::ZOOM_MIN);
        let below = fit_target((1920, 1080), 1.0, (0, 0, w + 1, 1))
            .unwrap()
            .zoom;
        assert_eq!(below, crate::geom::ZOOM_MIN, "clamped");
        assert!(
            1920.0 / ((w + 1) as f64) < crate::geom::ZOOM_MIN,
            "the raw zoom was below"
        );
        let above = fit_target((1920, 1080), 1.0, (0, 0, w - 1, 1))
            .unwrap()
            .zoom;
        assert!(above > crate::geom::ZOOM_MIN);
        assert_eq!(above, 1920.0 / (w - 1) as f64);
    }

    /// The upper clamp is reachable (C12.F15.6 erratum 2): a 1 px window on a 2¹⁵ px view at scale
    /// 0.25 gives z_raw = 2¹⁵ / 1 / 0.25 = 2¹⁷, clamped to 2¹⁶.
    #[test]
    fn fit_zoom_clamps_at_the_upper_bound() {
        let side = crate::geom::MODE_MAX;
        let z = fit_target((side, side), 0.25, (5, 7, 1, 1)).unwrap().zoom;
        assert_eq!(z, crate::geom::ZOOM_MAX);
        assert_eq!(f64::from(side) / 0.25, (1u64 << 17) as f64);
        // and at scale 8 the same window fits unclamped
        assert_eq!(
            fit_target((side, side), 8.0, (5, 7, 1, 1)).unwrap().zoom,
            4096.0
        );
    }

    /// Units (C12.F15.2): `scale_O` is divided out once. At scale 2 a 1920 × 1080 physical view is
    /// 960 × 540 Canvas px at zoom 1, so a 960 × 540 window fits at zoom 1, k = 2, and covers the
    /// view: its physical rect is the whole view within ±1 px (here exactly).
    #[test]
    fn a_window_that_fits_the_view_at_scale_2_fits_at_zoom_1() {
        let t = fit_target((1920, 1080), 2.0, (-480, -270, 960, 540)).unwrap();
        assert_eq!(t.zoom, 1.0);
        assert_eq!(t.center, CanvasPos { x: 0, y: 0 });
        let view = crate::projection::View::new(crate::projection::ViewParams {
            centre: t.center,
            zoom: t.zoom,
            scale: 2.0,
            view_size: (1920, 1080),
        })
        .unwrap();
        let r = view.physical_rect(
            crate::geom::CanvasPoint { x: -480, y: -270 },
            crate::geom::LocalSize { w: 960, h: 540 },
        );
        for (got, want) in [(r.x0, 0), (r.y0, 0), (r.x1, 1920), (r.y1, 1080)] {
            assert!((got - want).abs() <= 1, "{r:?}");
        }
    }

    /// Refusal: a zero or negative window side has no fit (F-FIT-1).
    #[test]
    fn a_degenerate_window_has_no_fit() {
        for g in [(0, 0, 0, 10), (0, 0, 10, 0), (0, 0, -5, 10), (0, 0, 10, -1)] {
            assert_eq!(fit_target((1920, 1080), 1.0, g), None, "{g:?}");
        }
    }

    /// The centre is g's centre in Q16, exact for odd sides, and clamped into INV-COORD-1 at both
    /// ends (±2³⁶ px); every fitted camera is accepted by `View::new` (F-VIEW-1's domain).
    #[test]
    fn the_fit_centre_is_exact_and_clamped_into_the_canvas() {
        let t = fit_target((1920, 1080), 1.0, (-3, 4, 7, 9)).unwrap();
        assert_eq!(
            t.center,
            CanvasPos {
                x: (1 << 16) / 2,
                y: (17 << 16) / 2
            }
        );
        let limit = CANVAS_LIMIT_PX << 16;
        let lim = CANVAS_LIMIT_PX;
        let hi = fit_target((1920, 1080), 1.0, (lim - 10, lim - 10, 100, 100)).unwrap();
        assert_eq!(hi.center, CanvasPos { x: limit, y: limit });
        let lo = fit_target((1920, 1080), 1.0, (-lim - 50, -lim - 50, 20, 20)).unwrap();
        assert_eq!(
            lo.center,
            CanvasPos {
                x: -limit,
                y: -limit
            }
        );
        for (view, scale, g) in [
            ((1920u32, 1080u32), 1.0, (lim - 10, lim - 10, 100, 100)),
            ((1920, 1080), 1.0, (-lim - 50, -lim - 50, 20, 20)),
            ((1, 1), 8.0, (0, 0, 1i64 << 36, 1i64 << 36)),
            ((crate::geom::MODE_MAX, 1), 0.25, (0, 0, 1, 1)),
            ((1920, 1080), 1.5, (-123, 77, 801, 603)),
        ] {
            let t = fit_target(view, scale, g).unwrap();
            assert!(
                crate::projection::View::new(crate::projection::ViewParams {
                    centre: t.center,
                    zoom: t.zoom,
                    scale,
                    view_size: view,
                })
                .is_ok(),
                "{view:?} {scale} {g:?} → {t:?}"
            );
        }
    }

    /// OBJ-30's ring itself: push beyond capacity overwrites the oldest in place; pop returns
    /// newest first; clear releases the storage.
    #[test]
    fn the_ring_keeps_the_newest_256_values() {
        let v = |i: i64| CameraValue {
            center: CanvasPos { x: i, y: -i },
            zoom: 1.0 + i as f64,
        };
        let mut h = NavigationHistory::default();
        assert_eq!((h.len(), h.reserved()), (0, 0));
        assert_eq!(h.pop(), None);
        for i in 0..300 {
            h.push(v(i));
        }
        assert_eq!(
            (h.len(), h.reserved()),
            (HISTORY_CAPACITY, HISTORY_CAPACITY)
        );
        assert!(h.values().eq((44..300).map(v)));
        for i in (44..300).rev() {
            assert_eq!(h.pop(), Some(v(i)));
        }
        assert_eq!(h.pop(), None);
        h.push(v(7));
        h.clear();
        assert_eq!((h.len(), h.reserved()), (0, 0));
    }
}
