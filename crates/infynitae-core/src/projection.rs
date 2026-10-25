//! Per-output projection (OBJ-12): view origin (F-VIEW-1), reference-relative lattice and physical
//! edges (F-LATTICE-1), render location (F-RENDER-1), unprojection (F-UNPROJECT-1) and projection
//! (F-PROJECT-1). Pure functions of their parameters; nothing here is written back (INV-STAGE-6).

use crate::geom::{
    CanvasPoint, CanvasPos, LocalSize, OutputLogicalPoint, PhysicalRect, Q16_ONE, RenderLocation,
    q16,
};

/// δ = 2⁻²⁰ (F-LATTICE-1).
pub const DELTA: f64 = 1.0 / (1u64 << 20) as f64;
/// Lattice reference period in Canvas px (F-LATTICE-1).
pub const LATTICE: i64 = 120;

/// What a View is computed from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewParams {
    /// Camera centre (Q16).
    pub centre: CanvasPos,
    pub zoom: f64,
    /// Output scale.
    pub scale: f64,
    /// `view_O`: the physical mode after the output transform.
    pub view_size: (u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionError {
    /// zoom, scale or view size outside the representable domain (F-VIEW-1, C12.F14).
    InvalidParams,
}

/// View(O): `v_O`, `k_O`, `R_O`, `C_O`, `c_eff,O` and the view extent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// View origin, Q16 (R7).
    pub v: CanvasPos,
    pub zoom: f64,
    pub scale: f64,
    /// `k_O = zoom · scale`.
    pub k: f64,
    /// Lattice reference in Canvas px, a multiple of 120.
    pub r: (i64, i64),
    /// The camera on the physical grid (integer).
    pub c: (i64, i64),
    /// Snapped camera, Q16 (R5).
    pub c_eff: CanvasPos,
    /// `view_O` in physical px.
    pub size: (u32, u32),
    /// `OutputPresentation(O) = Exclusive(W)` (OBJ-19, C5.2): when set, this is a presentation
    /// view and only W's surface tree is shown through it. `None` for a camera's view.
    pub only: Option<crate::stage::ElementId>,
}

/// The kept-reference bound of F-LATTICE-1 (C12.F14): a reference is kept only while
/// `|v_O − R_O|·k_O ≤ 2³¹` on its axis.
pub const KEPT_REFERENCE_BOUND: f64 = (1u64 << 31) as f64;

impl View {
    /// The view with its lattice reference set from its own origin, `R = floor(v / 120)·120`
    /// (F-LATTICE-1: O's view (re)defined).
    pub fn new(p: ViewParams) -> Result<View, ProjectionError> {
        View::with_kept_reference(p, None)
    }

    /// The view with the output's kept lattice reference `kept` (F-LATTICE-1 as amended by
    /// C12.F14): on each axis it is kept while `|v − R|·k ≤ 2³¹`, else set from the view origin
    /// again. `None` sets it from the origin on both axes. The caller decides whether the view
    /// was (re)defined (a connect, a (re-)binding, a change of `k_O` or `view_O`); this decides
    /// only the bound. The axes are independent: the arithmetic is per axis.
    pub fn with_kept_reference(
        p: ViewParams,
        kept: Option<(i64, i64)>,
    ) -> Result<View, ProjectionError> {
        // Only the representable domain is accepted (F-VIEW-1, C12.F14): Core's entry
        // points never produce anything else, so this refuses only a caller that bypassed them.
        let scale_ok = p.scale.is_finite()
            && (f64::from(crate::geom::SCALE_120_MIN) / 120.0
                ..=f64::from(crate::geom::SCALE_120_MAX) / 120.0)
                .contains(&p.scale);
        let side_ok = |s: u32| (1..=crate::geom::MODE_MAX).contains(&s);
        if !crate::geom::zoom_in_domain(p.zoom)
            || !scale_ok
            || !side_ok(p.view_size.0)
            || !side_ok(p.view_size.1)
        {
            return Err(ProjectionError::InvalidParams);
        }
        let k = p.zoom * p.scale;
        // F-VIEW-1: v_O = Q16(centre − view_O / (2·k_O)), evaluated in f64, ties to even (R7)
        let v_axis = |centre_q: i64, extent: u32| {
            let centre_px = centre_q as f64 / Q16_ONE as f64;
            q16(centre_px - extent as f64 / (2.0 * k))
        };
        let v = CanvasPos {
            x: v_axis(p.centre.x, p.view_size.0),
            y: v_axis(p.centre.y, p.view_size.1),
        };
        // F-LATTICE-1: R = floor(v / 120)·120, unless a kept reference is within the bound
        let reference = |v_q: i64, kept: Option<i64>| match kept {
            Some(r) if keeps(v_q, r, k) => r,
            _ => v_q.div_euclid(LATTICE * Q16_ONE) * LATTICE,
        };
        let (rx, ry) = (
            reference(v.x, kept.map(|r| r.0)),
            reference(v.y, kept.map(|r| r.1)),
        );
        let (cx, ex) = camera_on_grid(v.x, rx, k);
        let (cy, ey) = camera_on_grid(v.y, ry, k);
        Ok(View {
            v,
            zoom: p.zoom,
            scale: p.scale,
            k,
            r: (rx, ry),
            c: (cx, cy),
            c_eff: CanvasPos { x: ex, y: ey },
            size: p.view_size,
            only: None,
        })
    }

    /// `edge(x) = round((x − R)·k + δ) − C` for an integral Canvas edge (R4). Evaluated in f64 and
    /// returned as i64: no i32 conversion happens here (cull first).
    pub fn edge_x(&self, x: i64) -> i64 {
        edge(x, self.r.0, self.c.0, self.k)
    }

    pub fn edge_y(&self, y: i64) -> i64 {
        edge(y, self.r.1, self.c.1, self.k)
    }

    /// A surface's physical rect; origin and size are never rounded separately.
    pub fn physical_rect(&self, origin: CanvasPoint, size: LocalSize) -> PhysicalRect {
        PhysicalRect {
            x0: self.edge_x(origin.x),
            y0: self.edge_y(origin.y),
            x1: self.edge_x(origin.x + size.w as i64),
            y1: self.edge_y(origin.y + size.h as i64),
        }
    }

    /// Whether a physical rect intersects `[0, view_O)`.
    pub fn intersects_view(&self, r: &PhysicalRect) -> bool {
        r.x0 < r.x1
            && r.y0 < r.y1
            && r.x1 > 0
            && r.y1 > 0
            && r.x0 < self.size.0 as i64
            && r.y0 < self.size.1 as i64
    }

    /// Conservative cull of a Canvas box `[x0, x1) × [y0, y1)`: `false` only when no surface
    /// inside the box can intersect the view. `edge` is non-decreasing in its argument (k > 0),
    /// so a surface inside the box has `edge(sx1) ≤ edge(x1)` and `edge(sx0) ≥ edge(x0)`; each
    /// test that fails here therefore fails for the surface in [`View::intersects_view`].
    pub fn may_show(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> bool {
        self.edge_x(x1) > 0
            && self.edge_y(y1) > 0
            && self.edge_x(x0) < self.size.0 as i64
            && self.edge_y(y0) < self.size.1 as i64
    }

    /// [`View::may_show`] as four integer thresholds, computed once per view (see [`Cull`]).
    pub fn cull(&self) -> Cull {
        let (w, h) = (self.size.0 as i64, self.size.1 as i64);
        let (r, c, k) = (self.r, self.c, self.k);
        Cull {
            x_min: first_edge_at_least(1, r.0, c.0, k),
            y_min: first_edge_at_least(1, r.1, c.1, k),
            x_max: first_edge_at_least(w, r.0, c.0, k) - 1,
            y_max: first_edge_at_least(h, r.1, c.1, k) - 1,
        }
    }

    /// F-RENDER-1: `L(x) = (x − R)·k + δ − C`.
    pub fn render_location(&self, origin: CanvasPoint) -> RenderLocation {
        RenderLocation {
            x: (origin.x - self.r.0) as f64 * self.k + DELTA - self.c.0 as f64,
            y: (origin.y - self.r.1) as f64 * self.k + DELTA - self.c.1 as f64,
        }
    }

    /// F-UNPROJECT-1: `q = c_eff + Q16(p / zoom)` (R1).
    pub fn unproject(&self, p: OutputLogicalPoint) -> CanvasPos {
        CanvasPos {
            x: self.c_eff.x + q16(p.x / self.zoom),
            y: self.c_eff.y + q16(p.y / self.zoom),
        }
    }

    /// F-PROJECT-1: `p = (q − c_eff)·zoom`.
    pub fn project(&self, q: CanvasPos) -> OutputLogicalPoint {
        OutputLogicalPoint {
            x: (q.x - self.c_eff.x) as f64 / Q16_ONE as f64 * self.zoom,
            y: (q.y - self.c_eff.y) as f64 / Q16_ONE as f64 * self.zoom,
        }
    }

    /// F-HIT-1: the view-space physical pixel under an output-logical point, `⌊p · scale⌋`.
    pub fn pixel(&self, p: OutputLogicalPoint) -> (i64, i64) {
        (
            (p.x * self.scale).floor() as i64,
            (p.y * self.scale).floor() as i64,
        )
    }
}

/// Whether a reference `r` may be kept for view origin `v_q` (Q16) at `k`: `|v − R|·k ≤ 2³¹`.
fn keeps(v_q: i64, r: i64, k: f64) -> bool {
    (grid_value(v_q, r, k) - DELTA).abs() <= KEPT_REFERENCE_BOUND
}

/// The f64 value `fl(fl(rel · k) + δ)` whose rounding is C (F-LATTICE-1), `rel = (v − R)` in px.
fn grid_value(v_q: i64, r: i64, k: f64) -> f64 {
    // The Q16 difference converts to f64 exactly while |v − R| ≤ 2³⁷ px (2⁵³ Q16 units). A
    // reference set from this view gives |v − R| < 120 px. A kept reference can reach
    // |v − R| < 2³⁷ + 120 px: v and the origin R was set from share k and view_O, so they differ
    // by at most the centre's span 2³⁷, plus R's < 120. Past 2³⁷ px the conversion rounds by at
    // most one Q16 unit, and only where the kept-reference bound forces k < 2⁻⁶, which moves the
    // value by at most 2⁻²²; with the product's and the sum's half-ulps the value is within
    // 3·2⁻²² < δ of the exact one (C12.F15.7; `grid_value_precision_at_the_conversion_limit`).
    let rel = (v_q - r * Q16_ONE) as f64 / Q16_ONE as f64;
    rel * k + DELTA
}

/// F-LATTICE-1: the camera on the physical grid, `C = round((v − R)·k + δ)`, and the snapped
/// camera `c_eff = R + Q16(C / k)` (R5), for a view origin `v` (Q16) and a lattice reference `R`.
fn camera_on_grid(v_q: i64, r: i64, k: f64) -> (i64, i64) {
    let c = grid_value(v_q, r, k).round() as i64;
    (c, r * Q16_ONE + q16(c as f64 / k))
}

fn edge(x: i64, r: i64, c: i64, k: f64) -> i64 {
    ((x - r) as f64 * k + DELTA).round() as i64 - c
}

/// [`View::may_show`] without a rounding per box: `edge` is non-decreasing in its argument
/// (k > 0), so `edge(x1) > 0` holds exactly for `x1 >= x_min`, where `x_min` is the smallest
/// Canvas coordinate whose edge is at least 1, and `edge(x0) < view_w` exactly for
/// `x0 <= x_max`, the largest whose edge is below `view_w` (likewise y). Each threshold is found
/// with `edge` itself, so this is the same predicate; a pass computes it once per view and then
/// culls every element with four integer comparisons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cull {
    x_min: i64,
    y_min: i64,
    x_max: i64,
    y_max: i64,
}

impl Cull {
    /// Whether a surface inside the Canvas box `[x0, x1) × [y0, y1)` can intersect the view:
    /// equal to [`View::may_show`] for every Canvas coordinate (INV-COORD-1 range).
    pub fn admits(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> bool {
        x1 >= self.x_min && y1 >= self.y_min && x0 <= self.x_max && y0 <= self.y_max
    }
}

/// The search range of the thresholds: beyond every Canvas coordinate (±2^36, INV-COORD-1), so a
/// threshold clamped to it decides every coordinate the same way `edge` does.
const CULL_LIMIT: i64 = 1 << 40;

/// The smallest `x` in `[-CULL_LIMIT, CULL_LIMIT]` with `edge(x) >= target`, `CULL_LIMIT` if
/// none is. From the inverse of `edge`, then corrected by stepping with `edge` itself (a few
/// steps: the estimate is off by the rounding of a few floating-point operations). If the
/// estimate is not usable (a zoom so small or large that it is not finite, or still off after
/// `STEPS` steps), a binary search over the whole range, which `edge` being non-decreasing allows:
/// at most 42 evaluations, whatever the view.
fn first_edge_at_least(target: i64, r: i64, c: i64, k: f64) -> i64 {
    const STEPS: u32 = 16;
    let at_least = |x: i64| edge(x, r, c, k) >= target;
    let estimate = r as f64 + ((target + c) as f64 - 0.5 - DELTA) / k;
    if estimate.is_finite() {
        let mut x = (estimate.floor() as i64).clamp(-CULL_LIMIT, CULL_LIMIT);
        for _ in 0..STEPS {
            if x > -CULL_LIMIT && at_least(x - 1) {
                x -= 1;
            } else if x < CULL_LIMIT && !at_least(x) {
                x += 1;
            } else {
                return x;
            }
        }
    }
    // invariant: the answer is in (lo, hi]
    if at_least(-CULL_LIMIT) {
        return -CULL_LIMIT;
    }
    let (mut lo, mut hi) = (-CULL_LIMIT, CULL_LIMIT);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if at_least(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::geom::CANVAS_LIMIT_PX;

    /// Deterministic xorshift64* for property tests (no dependencies in core).
    pub(crate) struct Rng(pub u64);

    impl Rng {
        pub(crate) fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        pub(crate) fn range(&mut self, lo: i64, hi: i64) -> i64 {
            lo + (self.next() % (hi - lo + 1) as u64) as i64
        }
    }

    pub(crate) fn view(cx: f64, cy: f64, scale: f64, size: (u32, u32)) -> View {
        View::new(ViewParams {
            centre: CanvasPos {
                x: q16(cx),
                y: q16(cy),
            },
            zoom: 1.0,
            scale,
            view_size: size,
        })
        .unwrap()
    }

    /// The cull thresholds when the inverse estimate is useless: a subnormal or an infinite scale
    /// factor makes it non-finite, so the bounded binary search decides; it finds the same
    /// threshold a linear scan would (edge is non-decreasing), in at most 42 evaluations. Whole
    /// views at the extreme zooms `View::new` accepts agree with `may_show`.
    #[test]
    fn cull_thresholds_at_extreme_scale_factors() {
        for k in [1e-310, f64::MIN_POSITIVE, 1e300] {
            for target in [1, 1920] {
                let x = first_edge_at_least(target, 0, 0, k);
                assert!(
                    x == CULL_LIMIT || edge(x, 0, 0, k) >= target,
                    "k {k} target {target}"
                );
                assert!(
                    x == -CULL_LIMIT || edge(x - 1, 0, 0, k) < target,
                    "k {k} target {target}: not the smallest"
                );
            }
        }
        // C12.F14 (D1): the representable domain ends at zoom 2^16 (1e6 is refused by
        // View::new); its extremes of k are 2^16 · 8 = 2^19 and 2^-20 · 0.25 = 2^-22
        use crate::geom::{ZOOM_MAX, ZOOM_MIN};
        for (zoom, scale) in [
            (1e-6, 1.0),
            (1e-3, 1.0),
            (1e3, 1.0),
            (ZOOM_MAX, 1.0),
            (ZOOM_MAX, 8.0),
            (ZOOM_MIN, 0.25),
        ] {
            let v = View::new(ViewParams {
                centre: CanvasPos { x: 0, y: 0 },
                zoom,
                scale,
                view_size: (1920, 1080),
            })
            .unwrap();
            let cull = v.cull();
            for (x0, y0, x1, y1) in [
                (-10, -10, 10, 10),
                (-(1 << 36), -(1 << 36), 1 << 36, 1 << 36),
                (5, 5, 6, 6),
                (-(1 << 36), 0, -(1 << 35), 1),
            ] {
                assert_eq!(
                    cull.admits(x0, y0, x1, y1),
                    v.may_show(x0, y0, x1, y1),
                    "zoom {zoom} scale {scale}"
                );
            }
        }
    }

    /// `Cull::admits` equals `View::may_show` on random views (zoom, scale, centre, size) and
    /// boxes concentrated around the view edges, where a threshold off by one would show.
    #[test]
    fn cull_thresholds_equal_may_show() {
        let mut rng = Rng(0x5eed_c011_u64.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let zooms = [0.001, 0.07, 0.25, 0.5, 1.0, 1.25, 2.0, 3.7, 8.0];
        let scales = [0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
        let mut checked = 0;
        for _ in 0..400 {
            let zoom = zooms[rng.range(0, zooms.len() as i64 - 1) as usize];
            let scale = scales[rng.range(0, scales.len() as i64 - 1) as usize];
            let v = View::new(ViewParams {
                centre: CanvasPos {
                    x: rng.range(-(1 << 30), 1 << 30) * Q16_ONE / 7,
                    y: rng.range(-(1 << 30), 1 << 30) * Q16_ONE / 11,
                },
                zoom,
                scale,
                view_size: (rng.range(1, 5000) as u32, rng.range(1, 3000) as u32),
            })
            .unwrap();
            let cull = v.cull();
            // Canvas coordinates near each view edge, and anywhere
            let near = |rng: &mut Rng, phys: i64, along_x: bool| {
                let canvas = if along_x {
                    v.r.0 + ((phys + v.c.0) as f64 / v.k) as i64
                } else {
                    v.r.1 + ((phys + v.c.1) as f64 / v.k) as i64
                };
                canvas + rng.range(-3, 3)
            };
            for _ in 0..200 {
                let pick = |rng: &mut Rng, along_x: bool, size: i64| match rng.range(0, 2) {
                    0 => near(rng, 0, along_x),
                    1 => near(rng, size, along_x),
                    _ => rng.range(-(1 << 36), 1 << 36),
                };
                let (w, h) = (v.size.0 as i64, v.size.1 as i64);
                let (a, b) = (pick(&mut rng, true, w), pick(&mut rng, true, w));
                let (c, d) = (pick(&mut rng, false, h), pick(&mut rng, false, h));
                let (x0, x1, y0, y1) = (a.min(b), a.max(b), c.min(d), c.max(d));
                assert_eq!(
                    cull.admits(x0, y0, x1, y1),
                    v.may_show(x0, y0, x1, y1),
                    "view {v:?} box ({x0}, {y0}, {x1}, {y1})"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 80_000);
    }

    #[test]
    fn view_origin_is_centre_minus_half_view_ties_even() {
        let v = view(0.0, 0.0, 1.0, (1920, 1080));
        assert_eq!(
            v.v,
            CanvasPos {
                x: -960 * Q16_ONE,
                y: -540 * Q16_ONE
            }
        );
        // an exact tie: centre 0, extent 1, k = 2¹⁶ (zoom 2¹⁶, the domain's maximum, at scale 1)
        // → v = −2⁻¹⁷ px = −0.5 Q16 units; ties-even
        // gives 0 where round-half-away would give −1
        let t = View::new(ViewParams {
            centre: CanvasPos { x: 0, y: 0 },
            zoom: 65536.0,
            scale: 1.0,
            view_size: (1, 1),
        })
        .unwrap();
        assert_eq!(t.v.x, 0, "R7 rounds ties to even");
    }

    /// (case, camera centre Q16, scale_120, view size, output-logical p, expected c_eff Q16,
    /// expected q Q16). The expected values are hand-derived from the canonical formula text with
    /// exact rational arithmetic (round-half-even Q16, round-half-away C), outside this crate; see
    /// docs/archive/evidence/phase-f/INC-1/corrective/unproject_reference.py (INC1-F06).
    #[allow(clippy::type_complexity)]
    const UNPROJECT_REFERENCE: &[(
        &str,
        (i64, i64),
        u32,
        (u32, u32),
        (f64, f64),
        (i64, i64),
        (i64, i64),
    )] = &[
        (
            "origin camera k=1",
            (0, 0),
            120,
            (1920, 1080),
            (0.0, 0.0),
            (-62914560, -35389440),
            (-62914560, -35389440),
        ),
        (
            "origin camera k=1",
            (0, 0),
            120,
            (1920, 1080),
            (100.25, 50.5),
            (-62914560, -35389440),
            (-56344576, -32079872),
        ),
        (
            "origin camera k=1",
            (0, 0),
            120,
            (1920, 1080),
            (1919.5, 1079.5),
            (-62914560, -35389440),
            (62881792, 35356672),
        ),
        (
            "negative fractional camera k=1.25",
            (-327712768, -196624384),
            150,
            (2560, 1440),
            (0.0, 0.0),
            (-394841293, -234356736),
            (-394841293, -234356736),
        ),
        (
            "negative fractional camera k=1.25",
            (-327712768, -196624384),
            150,
            (2560, 1440),
            (0.4, 0.4),
            (-394841293, -234356736),
            (-394815079, -234330522),
        ),
        (
            "negative fractional camera k=1.25",
            (-327712768, -196624384),
            150,
            (2560, 1440),
            (2047.6, 1151.2),
            (-394841293, -234356736),
            (-260649779, -158911693),
        ),
        (
            "d5 camera k=1.5",
            (144208691, 58992230),
            180,
            (3840, 2160),
            (0.3333333333333333, 0.3333333333333333),
            (60336811, 11796480),
            (60358656, 11818325),
        ),
        (
            "d5 camera k=1.5",
            (144208691, 58992230),
            180,
            (3840, 2160),
            (1.0, 1.6666666666666667),
            (60336811, 11796480),
            (60402347, 11905707),
        ),
        (
            "d5 camera k=1.5",
            (144208691, 58992230),
            180,
            (3840, 2160),
            (2559.6666666666665, 1439.6666666666667),
            (60336811, 11796480),
            (228087126, 106146475),
        ),
        (
            "d5 camera k=1.5",
            (144208691, 58992230),
            180,
            (3840, 2160),
            (1279.6666666666667, 720.3333333333334),
            (60336811, 11796480),
            (144201046, 59004245),
        ),
        (
            "k=1.75",
            (8090812, -5170790),
            210,
            (1920, 1080),
            (0.2857142857142857, 0.2857142857142857),
            (-27862162, -25390519),
            (-27843437, -25371794),
        ),
        (
            "k=1.75",
            (8090812, -5170790),
            210,
            (1920, 1080),
            (4.285714285714286, 7.714285714285714),
            (-27862162, -25390519),
            (-27581293, -24884956),
        ),
        (
            "k=1.75",
            (8090812, -5170790),
            210,
            (1920, 1080),
            (1096.857142857143, 616.8571428571429),
            (-27862162, -25390519),
            (44021468, 15035831),
        ),
        (
            "awkward k=132/120",
            (-1133773, 2732851),
            132,
            (1366, 768),
            (0.45454545454545453, 0.45454545454545453),
            (-41823884, -20137425),
            (-41794095, -20107636),
        ),
        (
            "awkward k=132/120",
            (-1133773, 2732851),
            132,
            (1366, 768),
            (621.3636363636363, 349.5454545454545),
            (-41823884, -20137425),
            (-1102197, 2770386),
        ),
        (
            "awkward k=132/120",
            (-1133773, 2732851),
            132,
            (1366, 768),
            (1241.3636363636363, 697.7272727272726),
            (-41823884, -20137425),
            (39530123, 25588830),
        ),
        (
            "awkward k=160/120",
            (-1133773, 2732851),
            160,
            (1366, 768),
            (0.375, 0.375),
            (-34701312, -16121856),
            (-34676736, -16097280),
        ),
        (
            "awkward k=160/120",
            (-1133773, 2732851),
            160,
            (1366, 768),
            (512.625, 288.375),
            (-34701312, -16121856),
            (-1105920, 2777088),
        ),
        (
            "awkward k=160/120",
            (-1133773, 2732851),
            160,
            (1366, 768),
            (1024.125, 575.625),
            (-34701312, -16121856),
            (32415744, 21602304),
        ),
        (
            "awkward k=200/120",
            (-1133773, 2732851),
            200,
            (1366, 768),
            (0.3, 0.3),
            (-27996979, -12346982),
            (-27977318, -12327321),
        ),
        (
            "awkward k=200/120",
            (-1133773, 2732851),
            200,
            (1366, 768),
            (410.09999999999997, 230.7),
            (-27996979, -12346982),
            (-1120665, 2772173),
        ),
        (
            "awkward k=200/120",
            (-1133773, 2732851),
            200,
            (1366, 768),
            (819.3, 460.5),
            (-27996979, -12346982),
            (25696666, 17832346),
        ),
        (
            "awkward k=270/120",
            (-1133773, 2732851),
            270,
            (1366, 768),
            (0.2222222222222222, 0.2222222222222222),
            (-21029774, -8446862),
            (-21015210, -8432298),
        ),
        (
            "awkward k=270/120",
            (-1133773, 2732851),
            270,
            (1366, 768),
            (303.77777777777777, 170.88888888888889),
            (-21029774, -8446862),
            (-1121394, 2752512),
        ),
        (
            "awkward k=270/120",
            (-1133773, 2732851),
            270,
            (1366, 768),
            (606.8888888888889, 341.1111111111111),
            (-21029774, -8446862),
            (18743296, 13908196),
        ),
        (
            "pixel boundaries k=120/120",
            (19660800, -13107200),
            120,
            (1920, 1080),
            (1.0, 1.0),
            (-43253760, -48496640),
            (-43188224, -48431104),
        ),
        (
            "pixel boundaries k=120/120",
            (19660800, -13107200),
            120,
            (1920, 1080),
            (0.9999999999999999, 1.0000000000000002),
            (-43253760, -48496640),
            (-43188224, -48431104),
        ),
        (
            "pixel boundaries k=120/120",
            (19660800, -13107200),
            120,
            (1920, 1080),
            (0.9999999, 1.0000001),
            (-43253760, -48496640),
            (-43188224, -48431104),
        ),
        (
            "pixel boundaries k=120/120",
            (19660800, -13107200),
            120,
            (1920, 1080),
            (37.0, 37.0),
            (-43253760, -48496640),
            (-40828928, -46071808),
        ),
        (
            "pixel boundaries k=120/120",
            (19660800, -13107200),
            120,
            (1920, 1080),
            (36.99999999999999, 37.00000000000001),
            (-43253760, -48496640),
            (-40828928, -46071808),
        ),
        (
            "pixel boundaries k=120/120",
            (19660800, -13107200),
            120,
            (1920, 1080),
            (36.9999999, 37.0000001),
            (-43253760, -48496640),
            (-40828928, -46071808),
        ),
        (
            "pixel boundaries k=120/120",
            (19660800, -13107200),
            120,
            (1920, 1080),
            (600.0, 600.0),
            (-43253760, -48496640),
            (-3932160, -9175040),
        ),
        (
            "pixel boundaries k=120/120",
            (19660800, -13107200),
            120,
            (1920, 1080),
            (599.9999999999999, 600.0000000000001),
            (-43253760, -48496640),
            (-3932160, -9175040),
        ),
        (
            "pixel boundaries k=120/120",
            (19660800, -13107200),
            120,
            (1920, 1080),
            (599.9999999, 600.0000001),
            (-43253760, -48496640),
            (-3932160, -9175040),
        ),
        (
            "pixel boundaries k=150/120",
            (19660800, -13107200),
            150,
            (1920, 1080),
            (0.8, 0.8),
            (-30670848, -41418752),
            (-30618419, -41366323),
        ),
        (
            "pixel boundaries k=150/120",
            (19660800, -13107200),
            150,
            (1920, 1080),
            (0.7999999999999999, 0.8000000000000002),
            (-30670848, -41418752),
            (-30618419, -41366323),
        ),
        (
            "pixel boundaries k=150/120",
            (19660800, -13107200),
            150,
            (1920, 1080),
            (0.7999999000000001, 0.8000001),
            (-30670848, -41418752),
            (-30618419, -41366323),
        ),
        (
            "pixel boundaries k=150/120",
            (19660800, -13107200),
            150,
            (1920, 1080),
            (29.6, 29.6),
            (-30670848, -41418752),
            (-28730982, -39478886),
        ),
        (
            "pixel boundaries k=150/120",
            (19660800, -13107200),
            150,
            (1920, 1080),
            (29.599999999999998, 29.600000000000005),
            (-30670848, -41418752),
            (-28730982, -39478886),
        ),
        (
            "pixel boundaries k=150/120",
            (19660800, -13107200),
            150,
            (1920, 1080),
            (29.5999999, 29.600000100000003),
            (-30670848, -41418752),
            (-28730982, -39478886),
        ),
        (
            "pixel boundaries k=150/120",
            (19660800, -13107200),
            150,
            (1920, 1080),
            (480.0, 480.0),
            (-30670848, -41418752),
            (786432, -9961472),
        ),
        (
            "pixel boundaries k=150/120",
            (19660800, -13107200),
            150,
            (1920, 1080),
            (479.99999999999994, 480.00000000000006),
            (-30670848, -41418752),
            (786432, -9961472),
        ),
        (
            "pixel boundaries k=150/120",
            (19660800, -13107200),
            150,
            (1920, 1080),
            (479.9999999, 480.0000001),
            (-30670848, -41418752),
            (786432, -9961472),
        ),
        (
            "pixel boundaries k=180/120",
            (19660800, -13107200),
            180,
            (1920, 1080),
            (0.6666666666666666, 0.6666666666666666),
            (-22282240, -36700160),
            (-22238549, -36656469),
        ),
        (
            "pixel boundaries k=180/120",
            (19660800, -13107200),
            180,
            (1920, 1080),
            (0.6666666666666665, 0.6666666666666667),
            (-22282240, -36700160),
            (-22238549, -36656469),
        ),
        (
            "pixel boundaries k=180/120",
            (19660800, -13107200),
            180,
            (1920, 1080),
            (0.6666665666666667, 0.6666667666666666),
            (-22282240, -36700160),
            (-22238549, -36656469),
        ),
        (
            "pixel boundaries k=180/120",
            (19660800, -13107200),
            180,
            (1920, 1080),
            (24.666666666666668, 24.666666666666668),
            (-22282240, -36700160),
            (-20665685, -35083605),
        ),
        (
            "pixel boundaries k=180/120",
            (19660800, -13107200),
            180,
            (1920, 1080),
            (24.666666666666664, 24.66666666666667),
            (-22282240, -36700160),
            (-20665685, -35083605),
        ),
        (
            "pixel boundaries k=180/120",
            (19660800, -13107200),
            180,
            (1920, 1080),
            (24.666666566666667, 24.66666676666667),
            (-22282240, -36700160),
            (-20665685, -35083605),
        ),
        (
            "pixel boundaries k=180/120",
            (19660800, -13107200),
            180,
            (1920, 1080),
            (400.0, 400.0),
            (-22282240, -36700160),
            (3932160, -10485760),
        ),
        (
            "pixel boundaries k=180/120",
            (19660800, -13107200),
            180,
            (1920, 1080),
            (399.99999999999994, 400.00000000000006),
            (-22282240, -36700160),
            (3932160, -10485760),
        ),
        (
            "pixel boundaries k=180/120",
            (19660800, -13107200),
            180,
            (1920, 1080),
            (399.9999999, 400.0000001),
            (-22282240, -36700160),
            (3932160, -10485760),
        ),
        (
            "near +2^36 k=1.75",
            (4503598972010496, 1501199657336832),
            210,
            (3840, 2160),
            (0.0, 0.0),
            (4503598900108142, 1501199616891758),
            (4503598900108142, 1501199616891758),
        ),
        (
            "near +2^36 k=1.75",
            (4503598972010496, 1501199657336832),
            210,
            (3840, 2160),
            (2193.7, 1233.1),
            (4503598900108142, 1501199616891758),
            (4503599043874465, 1501199697704200),
        ),
        (
            "near -2^36 k=1.25",
            (-4503598972010496, -1501199657336832),
            150,
            (2560, 1440),
            (0.0, 0.0),
            (-4503599039119360, -1501199695085568),
            (-4503599039119360, -1501199695085568),
        ),
        (
            "near -2^36 k=1.25",
            (-4503598972010496, -1501199657336832),
            150,
            (2560, 1440),
            (2047.9, 1151.9),
            (-4503599039119360, -1501199695085568),
            (-4503598904908186, -1501199619594650),
        ),
    ];

    /// INC1-F06: production unprojection (and c_eff, and projection) against the independent
    /// reference table: positive and negative cameras, fractional cameras, every INC-1 scale and
    /// awkward ones, exact pixel boundaries and their neighbours, magnitudes near ±2³⁶.
    #[test]
    fn unproject_matches_the_independent_reference() {
        for &(case, centre, scale_120, size, p, c_eff, q) in UNPROJECT_REFERENCE {
            let v = View::new(ViewParams {
                centre: CanvasPos {
                    x: centre.0,
                    y: centre.1,
                },
                zoom: 1.0,
                scale: scale_120 as f64 / 120.0,
                view_size: size,
            })
            .unwrap();
            assert_eq!((v.c_eff.x, v.c_eff.y), c_eff, "{case}: c_eff at p={p:?}");
            let got = v.unproject(OutputLogicalPoint { x: p.0, y: p.1 });
            assert_eq!((got.x, got.y), q, "{case}: unproject({p:?})");
            let back = v.project(CanvasPos { x: q.0, y: q.1 });
            let want = (
                (q.0 - c_eff.0) as f64 / Q16_ONE as f64,
                (q.1 - c_eff.1) as f64 / Q16_ONE as f64,
            );
            assert_eq!((back.x, back.y), want, "{case}: project(q)");
        }
        assert_eq!(UNPROJECT_REFERENCE.len(), 56);
    }

    #[test]
    fn lattice_reference_floors_negative_origins() {
        let v = view(0.0, 0.0, 1.0, (1920, 1080)); // v = (−960, −540)
        assert_eq!(v.r, (-960, -600));
        assert_eq!(v.r.0 % LATTICE, 0);
        assert!(v.v.x - v.r.0 * Q16_ONE >= 0 && v.v.y - v.r.1 * Q16_ONE >= 0);
        assert_eq!(v.c, (0, 60));
        assert_eq!(
            v.c_eff,
            CanvasPos {
                x: -960 * Q16_ONE,
                y: -540 * Q16_ONE
            }
        );
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        let p = |zoom, scale, size| ViewParams {
            centre: CanvasPos { x: 0, y: 0 },
            zoom,
            scale,
            view_size: size,
        };
        assert!(View::new(p(0.0, 1.0, (1, 1))).is_err());
        assert!(View::new(p(1.0, f64::NAN, (1, 1))).is_err());
        assert!(View::new(p(1.0, -1.0, (1, 1))).is_err());
        assert!(View::new(p(1.0, 1.0, (0, 1))).is_err());
    }

    /// OB-6 / CR-6 (C3.7): over random cameras, k ∈ n/120, zoom 1, magnitudes up to 2³⁶:
    /// edge(x) = round(L) for the surface origin and its far edge, and unprojecting a pixel centre
    /// through c_eff then re-projecting lands in the same pixel.
    #[test]
    fn ob6_edge_equals_round_l_and_reprojection_lands_in_the_same_pixel() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut checked = 0;
        for _ in 0..4000 {
            let n = rng.range(120, 360);
            let scale = n as f64 / 120.0;
            let mag = [0, 1_000_003, CANVAS_LIMIT_PX - 4096][rng.range(0, 2) as usize];
            let sign = if rng.range(0, 1) == 0 { 1 } else { -1 };
            let centre_q = sign * mag * Q16_ONE + rng.range(-Q16_ONE * 997, Q16_ONE * 997);
            let size = (rng.range(640, 3840) as u32, rng.range(480, 2160) as u32);
            let v = View::new(ViewParams {
                centre: CanvasPos {
                    x: centre_q,
                    y: centre_q / 3,
                },
                zoom: 1.0,
                scale,
                view_size: size,
            })
            .unwrap();
            // a surface origin near the view, and its width
            let ox = v.c_eff.x.div_euclid(Q16_ONE) + rng.range(-500, 2500);
            let w = rng.range(0, 1200);
            let l = v.render_location(CanvasPoint { x: ox, y: 0 });
            assert_eq!(
                v.edge_x(ox),
                l.x.round() as i64,
                "edge(x0) != round(L) at k={scale}"
            );
            let far = (l.x + w as f64 * v.k).round() as i64;
            assert_eq!(
                v.edge_x(ox + w),
                far,
                "edge(x1) != round(L + w·k) at k={scale}"
            );
            // re-projection of a pixel centre
            let (px, py) = (
                rng.range(0, size.0 as i64 - 1),
                rng.range(0, size.1 as i64 - 1),
            );
            let p = OutputLogicalPoint {
                x: (px as f64 + 0.5) / v.scale,
                y: (py as f64 + 0.5) / v.scale,
            };
            let q = v.unproject(p);
            assert_eq!(
                v.pixel(v.project(q)),
                (px, py),
                "re-projection left the pixel at k={scale}"
            );
            checked += 1;
        }
        assert_eq!(checked, 4000);
    }

    /// Far surfaces are culled on i64 edges; no i32 conversion is needed or attempted.
    #[test]
    fn far_surfaces_cull_without_overflow() {
        let v = view(0.0, 0.0, 3.0, (3840, 2160));
        let r = v.physical_rect(
            CanvasPoint {
                x: CANVAS_LIMIT_PX,
                y: -CANVAS_LIMIT_PX,
            },
            LocalSize { w: 100, h: 100 },
        );
        assert!(r.x0 > i32::MAX as i64);
        assert!(!v.intersects_view(&r));
    }

    #[test]
    fn zero_width_rects_are_not_visible() {
        let v = view(0.0, 0.0, 1.0, (100, 100));
        let r = v.physical_rect(CanvasPoint { x: 0, y: 0 }, LocalSize { w: 0, h: 10 });
        assert!(!v.intersects_view(&r));
    }

    /// F-VIEW-1's representable domain (C12.F14): every corner of the accepted domain — zoom, scale, view
    /// size, camera centre and edge positions at their extremes — and 20,000 random states inside
    /// it evaluate without overflow (`cargo test` builds with the test profile, which keeps
    /// overflow checks on, so an overflow panics here; no release run of this test exists, and a
    /// release build — checks off — is covered only because the same inputs never overflow)
    /// and keep the projection's invariants: the view origin
    /// within F-VIEW-1's exact range |v| ≤ 2³⁷ px, monotone physical edges, the camera centre at
    /// the view centre, unproject ∘ project = identity up to the Q16 quantum. Every
    /// intermediate is monotone in each parameter, so the corners bound the domain.
    #[test]
    fn the_accepted_domain_never_overflows() {
        use crate::geom::{MODE_MAX, ZOOM_MAX, ZOOM_MIN};
        let lim = CANVAS_LIMIT_PX;
        let span = 1i64 << 31;
        let check = |zoom: f64, scale: f64, size: (u32, u32), cx: i64, cy: i64| {
            let p = ViewParams {
                centre: CanvasPos { x: cx, y: cy },
                zoom,
                scale,
                view_size: size,
            };
            let v = View::new(p).unwrap_or_else(|e| panic!("{p:?}: {e:?}"));
            let bound = (1i64 << 37) * Q16_ONE;
            assert!(
                v.v.x.abs() <= bound && v.v.y.abs() <= bound,
                "{p:?}: v {:?}",
                v.v
            );
            let xs = [-(lim + span), -lim, -1, 0, 1, lim, lim + span];
            let ex: Vec<i64> = xs.iter().map(|&x| v.edge_x(x)).collect();
            let ey: Vec<i64> = xs.iter().map(|&y| v.edge_y(y)).collect();
            assert!(ex.windows(2).all(|w| w[0] <= w[1]), "{p:?}: {ex:?}");
            assert!(ey.windows(2).all(|w| w[0] <= w[1]), "{p:?}: {ey:?}");
            for &x in &[-lim, 0, lim] {
                let l = v.render_location(CanvasPoint { x, y: -x });
                assert!(l.x.is_finite() && l.y.is_finite(), "{p:?}");
            }
            let half = (size.0 as f64 / scale / 2.0, size.1 as f64 / scale / 2.0);
            let quantum = zoom / Q16_ONE as f64;
            // the view origin snaps to the physical grid (F-LATTICE-1): the camera error is at
            // most half a physical px, 1/(2·scale) logical px, plus two Q16 roundings (v_O and
            // c_eff), each at most one quantum
            let c = v.project(p.centre);
            let tol = 0.5 / scale + 2.0 * quantum + 1e-6;
            assert!(
                (c.x - half.0).abs() <= tol && (c.y - half.1).abs() <= tol,
                "{p:?}: centre at {c:?}"
            );
            for corner in [(0.0, 0.0), (2.0 * half.0, 2.0 * half.1)] {
                let q = OutputLogicalPoint {
                    x: corner.0,
                    y: corner.1,
                };
                let back = v.project(v.unproject(q));
                assert!(
                    (back.x - q.x).abs() <= quantum + 1e-6
                        && (back.y - q.y).abs() <= quantum + 1e-6,
                    "{p:?}: {q:?} -> {back:?}"
                );
                let _ = v.pixel(q);
            }
        };
        let zooms = [ZOOM_MIN, 1.0, ZOOM_MAX];
        let scales = [0.25, 1.0, 8.0];
        let sizes = [(1, 1), (MODE_MAX, MODE_MAX), (MODE_MAX, 1), (1920, 1080)];
        let centres = [
            -lim * Q16_ONE,
            -lim * Q16_ONE + 1,
            0,
            lim * Q16_ONE - 1,
            lim * Q16_ONE,
        ];
        for &zoom in &zooms {
            for &scale in &scales {
                for &size in &sizes {
                    for &cx in &centres {
                        for &cy in &centres {
                            check(zoom, scale, size, cx, cy);
                        }
                    }
                }
            }
        }
        let mut rng = Rng(0x5EED_0FD0_4A11);
        for _ in 0..20_000 {
            let zoom = (rng.range(-20_000, 16_000) as f64 / 1000.0).exp2();
            let scale = rng.range(30, 960) as f64 / 120.0;
            let size = (
                rng.range(1, MODE_MAX as i64) as u32,
                rng.range(1, MODE_MAX as i64) as u32,
            );
            let c = |rng: &mut Rng| rng.range(-lim * Q16_ONE, lim * Q16_ONE);
            let (cx, cy) = (c(&mut rng), c(&mut rng));
            check(zoom.clamp(ZOOM_MIN, ZOOM_MAX), scale, size, cx, cy);
        }
    }

    /// F-VIEW-1's representable domain (C12.F14): a view outside the domain is refused, not computed (no
    /// saturation, no silent wrap): zoom, scale and view size each just outside a bound.
    #[test]
    fn a_view_outside_the_domain_is_refused() {
        use crate::geom::{MODE_MAX, ZOOM_MAX, ZOOM_MIN};
        let p = |zoom, scale, size| ViewParams {
            centre: CanvasPos { x: 0, y: 0 },
            zoom,
            scale,
            view_size: size,
        };
        for bad in [
            p(ZOOM_MIN / 2.0, 1.0, (1920, 1080)),
            p(ZOOM_MAX * 2.0, 1.0, (1920, 1080)),
            p(f64::INFINITY, 1.0, (1920, 1080)),
            p(f64::NAN, 1.0, (1920, 1080)),
            p(1.0, 0.24, (1920, 1080)),
            p(1.0, 8.01, (1920, 1080)),
            p(1.0, 1.0, (MODE_MAX + 1, 1080)),
            p(1.0, 1.0, (1920, 0)),
        ] {
            assert_eq!(
                View::new(bad),
                Err(ProjectionError::InvalidParams),
                "{bad:?}"
            );
        }
        for good in [
            p(ZOOM_MIN, 0.25, (MODE_MAX, MODE_MAX)),
            p(ZOOM_MAX, 8.0, (1, 1)),
        ] {
            assert!(View::new(good).is_ok(), "{good:?}");
        }
    }

    // ---- C12.F14 (OB-24 D3/D4): the output's lattice reference, kept while panning

    /// `view` with its lattice reference replaced by `r` (a reference kept from an earlier
    /// frame, F-LATTICE-1 as amended by C12.F14); C and c_eff follow from it.
    fn with_reference(mut view: View, r: (i64, i64)) -> View {
        let (cx, ex) = camera_on_grid(view.v.x, r.0, view.k);
        let (cy, ey) = camera_on_grid(view.v.y, r.1, view.k);
        view.r = r;
        view.c = (cx, cy);
        view.c_eff = CanvasPos { x: ex, y: ey };
        view
    }

    /// The exact value of `round(a · 2^-shift · k + δ)` (round half away from zero, as
    /// `f64::round`), for the f64 value k itself: no product or sum is rounded.
    fn exact_round(a: i128, shift: i32, k: f64) -> i64 {
        let bits = k.to_bits();
        assert!(k.is_finite() && k > 0.0 && (bits >> 52) != 0, "normal k");
        let mant = ((bits & ((1 << 52) - 1)) | (1 << 52)) as i128;
        let p = ((bits >> 52) & 0x7ff) as i32 - 1075 - shift; // a·k = a·mant·2^p
        let s = (-p).max(20);
        let num = a * mant * (1i128 << (p + s)) + (1i128 << (s - 20)); // (a·k + δ)·2^s
        let half = 1i128 << (s - 1);
        let r = if num >= 0 {
            (num + half) >> s
        } else {
            -((-num + half) >> s)
        };
        r as i64
    }

    /// The physical edge of Canvas x, evaluated exactly (F-LATTICE-1 with R kept).
    fn exact_edge(view: &View, x: i64) -> i64 {
        let c = exact_round((view.v.x - view.r.0 * Q16_ONE) as i128, 16, view.k);
        exact_round((x - view.r.0) as i128, 0, view.k) - c
    }

    /// C12.F14 (D3/D4): at k ∉ n/120 a reference kept across a pan changes no width and no gap,
    /// in any frame; the sealed per-frame reference (the negative control, ED-1) does.
    #[test]
    fn a_kept_lattice_reference_keeps_widths_and_gaps_during_a_zoomed_pan() {
        let (zoom, scale) = (1.2345, 1.25); // k = 1.543125 ∉ n/120
        let params = |cx: f64| ViewParams {
            centre: CanvasPos { x: q16(cx), y: 0 },
            zoom,
            scale,
            view_size: (1920, 1080),
        };
        // window edges inside and around the view: widths 1…300, gaps 7
        let mut xs = vec![-700i64];
        for w in [1, 2, 3, 17, 119, 120, 121, 240, 300, 77, 5, 64] {
            let last = *xs.last().unwrap();
            xs.extend([last + w, last + w + 7]);
        }
        let widths = |v: &View| -> Vec<i64> {
            let e: Vec<i64> = xs.iter().map(|&x| v.edge_x(x)).collect();
            e.windows(2).map(|w| w[1] - w[0]).collect()
        };
        let first = View::new(params(0.0)).unwrap();
        let kept = first.r;
        let (base_kept, base_sealed) = (widths(&first), widths(&first));
        let (mut changed_kept, mut changed_sealed) = (0, 0);
        for frame in 1..=240 {
            let sealed = View::new(params(frame as f64 * 37.3)).unwrap();
            changed_sealed += usize::from(widths(&sealed) != base_sealed);
            let held = with_reference(sealed, kept);
            changed_kept += usize::from(widths(&held) != base_kept);
            // the kept reference stays far inside the reference bound here
            assert!(((held.v.x >> 16) - held.r.0).abs() as f64 * held.k <= (1u64 << 31) as f64);
        }
        assert_eq!(changed_kept, 0, "a kept reference changed a width or gap");
        assert!(
            changed_sealed > 0,
            "control: the sealed per-frame reference should change a width at k ∉ n/120"
        );
    }

    /// C12.F14 (D4), a sample: with a kept reference within the bound `|v_O − R_O|·k_O ≤ 2³¹`,
    /// sampled visible physical edges equal the exact value of F-LATTICE-1 across the
    /// representable domain (zoom 2⁻²⁰ … 2¹⁶, scale 0.25 … 8, centres up to ±2³⁶ px), on both
    /// sides of the reference, at generic k and at k with exact ties (k = 1.25·2ⁿ:
    /// (x − R)·k = j + ½, where δ decides). A sample, not a guarantee: an edge whose sum lies
    /// within 2⁻²¹ of a rounding point can differ from exact arithmetic, and random states
    /// rarely land there (`kept_reference_rounding_steps_at_the_visible_edge_limit`).
    #[test]
    fn a_kept_reference_within_the_bound_matches_exact_edges_on_sampled_states() {
        use crate::geom::{CANVAS_LIMIT_PX, MODE_MAX, ZOOM_MAX, ZOOM_MIN};
        let bound = (1u64 << 31) as f64;
        let mut rng = Rng(0x0B24_C12F_1400_0001);
        let mut checked = 0u64;
        for case in 0..4000 {
            let (zoom, scale) = if case % 2 == 0 {
                (
                    (rng.range(-20_000, 16_000) as f64 / 1000.0).exp2(),
                    rng.range(30, 960) as f64 / 120.0,
                )
            } else {
                (
                    (rng.range(-20, 16) as f64).exp2(),
                    [1.25, 1.5, 0.75, 2.5][rng.range(0, 3) as usize],
                )
            };
            let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
            let size = (rng.range(1, MODE_MAX as i64) as u32, 1);
            let lim = CANVAS_LIMIT_PX * Q16_ONE;
            let centre = CanvasPos {
                x: rng.range(-lim, lim),
                y: 0,
            };
            let sealed = View::new(ViewParams {
                centre,
                zoom,
                scale,
                view_size: size,
            })
            .unwrap();
            let k = sealed.k;
            // a reference kept from an earlier frame: up to the bound away, on either side,
            // and within the Canvas range a view origin can have (|v| ≤ 2³⁷)
            let v_px = sealed.v.x >> 16;
            let reach = ((bound / k) as i64).min(1 << 37) / LATTICE;
            let r = sealed.r.0 + LATTICE * rng.range(-reach, reach);
            if r.abs() > 1 << 37 || ((v_px - r).abs() + 1) as f64 * k > bound {
                continue;
            }
            let held = with_reference(sealed, (r, sealed.r.1));
            // the visible Canvas span [v, v + view/k], sampled, with its first and last edges
            let span = ((size.0 as f64 / k) as i64).max(1);
            let step = (span / 64).max(1);
            let mut x = v_px - 1;
            while x <= v_px + span + 1 {
                assert_eq!(
                    held.edge_x(x),
                    exact_edge(&held, x),
                    "zoom {zoom} scale {scale} v {v_px} R {r} x {x}"
                );
                checked += 1;
                x += step;
            }
        }
        assert!(checked > 100_000, "{checked} edges checked");
    }

    /// C12.F14 (D4), why the bound is 2³¹ and not the decision package's 2⁵⁰: beyond about
    /// 2³³ the f64 sum `(x − R)·k + δ` loses δ, and at an exact tie (k = 1.25·2ⁿ, x < R) the
    /// edge then rounds away from where F-LATTICE-1 puts it.
    #[test]
    fn beyond_the_bound_a_kept_reference_loses_exact_edges() {
        let mut wrong = 0;
        for n in [-4, 0, 6, 12] {
            let zoom = f64::from(n).exp2();
            let sealed = View::new(ViewParams {
                centre: CanvasPos { x: 0, y: 0 },
                zoom,
                scale: 1.25,
                view_size: (1920, 1),
            })
            .unwrap();
            let k = sealed.k;
            // |v − R|·k ≈ 2³⁵, the reference far to the right of the view (x − R < 0)
            let r = sealed.r.0 + LATTICE * (((1u64 << 35) as f64 / k) as i64 / LATTICE);
            let held = with_reference(sealed, (r, sealed.r.1));
            for x in (sealed.v.x >> 16)..(sealed.v.x >> 16) + 400 {
                wrong += usize::from(held.edge_x(x) != exact_edge(&held, x));
            }
        }
        assert!(wrong > 0, "f64 stayed exact at |v − R|·k ≈ 2³⁵");
    }

    /// C12.F14 (D4), the rounding-step argument for the bound `|v_O − R_O|·k_O ≤ 2³¹`, checked
    /// deterministically at the visible-edge limit for every binary exponent e of k in the
    /// domain (k = 2ᵉ·m, 2⁻²² ≤ k ≤ 2¹⁹). A visible edge has |(x − R)·k| ≤ 2³¹ + 2¹⁵ (+ k for
    /// the edge just outside) < 2³², where one f64 rounding moves a value by at most half an
    /// ulp, 2⁻²². Established, on both sides of R, at the largest |(x − R)·k| each e reaches:
    /// an exact tie (x − R)·k = j + ½ rounds up, as exact F-LATTICE-1 does (δ is not absorbed);
    /// an integral (x − R)·k is its own edge; and around the δ-shifted rounding boundary
    /// (x − R)·k = h − δ, h a half-integer, the product's error is ≤ 2⁻²², the sum's ≤ 2⁻²¹ < δ,
    /// and the sum stays above the exact product. Not established: that an edge equals its
    /// exact-arithmetic value. A sum within 2⁻²¹ of h rounds either way; the sweep must meet
    /// such edges, and two fixed ones are kept below, one inside the bound and one at the
    /// magnitudes of the per-frame reference.
    #[test]
    fn kept_reference_rounding_steps_at_the_visible_edge_limit() {
        use crate::geom::zoom_in_domain;
        const S: i32 = 80; // fixed point 2⁻⁸⁰: every k, product and sum below is exact in it
        let fixed = |v: f64| -> i128 {
            let (b, m) = (v.to_bits(), v.abs());
            if m == 0.0 {
                return 0;
            }
            let mant = ((b & ((1 << 52) - 1)) | (1 << 52)) as i128;
            let p = ((b >> 52) & 0x7ff) as i32 - 1075 + S;
            assert!(p >= 0 && (b >> 52) & 0x7ff != 0, "{v} in 2⁻⁸⁰");
            if v < 0.0 { -(mant << p) } else { mant << p }
        };
        let round_away = |n: i128| -> i64 {
            let half = 1i128 << (S - 1);
            (if n >= 0 {
                (n + half) >> S
            } else {
                -((-n + half) >> S)
            }) as i64
        };
        let (delta, step) = (1i128 << (S - 20), 1i128 << (S - 22)); // δ, one rounding step
        let visible = ((1i64 << 31) + (1 << 15)) as f64;
        // |x − R| for a Canvas edge x (|x| ≤ 2³⁶) and a reference kept from a view origin
        // (|v| ≤ 2³⁷, R at most 120 below it)
        let reach = ((1i64 << 36) + (1 << 37) + LATTICE) as f64;
        // k = zoom · scale for a zoom in the domain at scale 0.25, 1 or 8 (exact quotients)
        let in_domain = |k: f64| [0.25, 1.0, 8.0].iter().any(|&s| zoom_in_domain(k / s));
        let (mut ties, mut boundary, mut disagree) = (0, 0, 0);
        for e in -22..=19i32 {
            let pow = f64::from(e).exp2();
            let limit = visible.min(reach * pow); // the largest |(x − R)·k| this e reaches
            // integral products: k = 2ᵉ, x − R a multiple of 2^max(0, −e)
            let unit = 1i64 << (-e).max(0);
            let d = (limit / pow) as i64 / unit * unit;
            for d in [d, -d] {
                assert!(in_domain(pow));
                assert_eq!(edge(d, 0, 0, pow), (d as f64 * pow) as i64, "e {e} d {d}");
            }
            if e == 19 {
                continue; // k = 2¹⁹ is the only k with this exponent
            }
            // exact ties: (x − R)·k = j + ½ must round up for x > R and x < R alike
            let (k, d) = if e >= 0 {
                let k = pow + 0.5; // odd x − R gives j + ½
                (k, ((limit / k) as i64 - 1) | 1)
            } else {
                let o = ((limit / 1.5) as i64 - 1) | 1; // (o·2⁻ᵉ)·1.5·2ᵉ = 1.5·o
                (1.5 * pow, o << -e)
            };
            assert!(in_domain(k), "e {e}");
            for d in [d, -d] {
                let twice = 2 * d as i128 * fixed(k); // 2·(x − R)·k, exact
                assert_eq!(twice.rem_euclid(2 << S), 1 << S, "e {e} d {d}: a tie");
                let up = ((twice >> S) + 1).div_euclid(2) as i64; // ⌈j + ½⌉
                assert_eq!(edge(d, 0, 0, k), up, "e {e} d {d}: a tie rounds up");
                ties += 1;
            }
            // the δ-shifted boundary at a generic k (full mantissa): k swept by single ulps
            // around (h − δ)/d moves (x − R)·k across h − δ in steps of about 2⁻²¹
            let generic = f64::from_bits((((e + 1023) as u64) << 52) | 0x5_5555_5555_5555);
            let d0 = (limit / generic) as i64;
            for d in [d0, -d0] {
                let h = (d as f64 * generic).floor() + 0.5;
                let k0 = (h - DELTA) / d as f64;
                for i in -64i64..=64 {
                    let k = f64::from_bits((k0.to_bits() as i64 + i) as u64);
                    let exponent = ((k.to_bits() >> 52) & 0x7ff) as i32 - 1023;
                    assert!(in_domain(k) && exponent == e, "e {e} k {k:e}");
                    let exact = d as i128 * fixed(k); // (x − R)·k, exact
                    let product = d as f64 * k;
                    let sum = product + DELTA;
                    assert!(
                        (fixed(product) - exact).abs() <= step,
                        "e {e} d {d} k {k:e}"
                    );
                    assert!(
                        (fixed(sum) - exact - delta).abs() <= 2 * step,
                        "e {e} d {d} k {k:e}"
                    );
                    assert!(fixed(sum) > exact, "e {e} d {d} k {k:e}: δ absorbed");
                    let want = round_away(exact + delta);
                    if edge(d, 0, 0, k) != want {
                        // only within 2⁻²¹ of the half-integer the exact sum rounds at
                        let to_h = (exact + delta - fixed(h)).abs();
                        assert!(
                            to_h <= 2 * step,
                            "e {e} d {d} k {k:e}: off by more than 2⁻²¹"
                        );
                        disagree += 1;
                    }
                    boundary += 1;
                }
            }
        }
        assert_eq!(ties, 2 * 41);
        assert!(boundary >= 2 * 41 * 129, "{boundary}");
        assert!(
            disagree > 0,
            "the sweep never met an edge that f64 rounds the other way"
        );
        // not established, fixed cases: f64 and exact arithmetic disagree inside the bound
        // ((x − R)·k ≈ 2³¹ − 5) and at the per-frame reference's magnitudes ((x − R)·k ≈ 37.5)
        for (d, bits, want) in [
            (2_147_483_643i64, 0x3FF0_0000_000F_FFFEu64, 2_147_483_643i64),
            (37, 0x3FF0_3759_EB3E_4530, 37),
        ] {
            let k = f64::from_bits(bits);
            assert!(in_domain(k));
            assert_eq!(
                round_away(d as i128 * fixed(k) + delta),
                want,
                "exact edge, d {d}"
            );
            assert_eq!(edge(d, 0, 0, k), want + 1, "f64 edge, d {d}");
        }
    }

    /// C12.F15.7, the Q16 precision budget of C_O at the conversion limit, on the production
    /// `grid_value` / `camera_on_grid`: at k = 2⁻²² (zoom 2⁻²⁰, scale 0.25) and a kept reference
    /// with `|v − R|` just above 2³⁷ px on both sides of R, the Q16 difference is odd and above
    /// 2⁵³, so its conversion to f64 rounds; the value whose rounding gives C is still within
    /// 3·2⁻²² of the exact `(v − R)·k + δ`. An exact tie `(v − R)·k = j + ½` rounds up on both
    /// sides of R (δ). Assumptions: binary64, round-to-nearest-even, no FMA.
    #[test]
    fn grid_value_precision_at_the_conversion_limit() {
        let k = (-22f64).exp2();
        assert!(crate::geom::zoom_in_domain(k / 0.25));
        let q53 = 1i64 << 53;
        // (v_q, R): |v| < 2³⁷ px, R a multiple of 120 on the other side, |v − R| ≈ 2³⁷ + 120 px
        for (v_q, r) in [(q53 - 1, -120i64), (-(q53 - 1), 120)] {
            let d_q = v_q - r * Q16_ONE; // (v − R) in Q16, exact in i64
            assert!(d_q.unsigned_abs() > 1 << 53 && d_q % 2 != 0);
            assert_ne!((d_q as f64) as i64, d_q, "the conversion must round here");
            assert!((grid_value(v_q, r, k) - DELTA).abs() <= KEPT_REFERENCE_BOUND);
            // exact (v − R)·k + δ in units of 2⁻³⁸: d_q·2⁻¹⁶·2⁻²² + 2⁻²⁰ = (d_q + 2¹⁸)·2⁻³⁸
            let exact = d_q as i128 + (1i128 << 18);
            let g = grid_value(v_q, r, k);
            let scaled = g * (38f64).exp2();
            assert_eq!(scaled.fract(), 0.0, "g·2³⁸ is an integer (ulp(2¹⁵) = 2⁻³⁷)");
            let err = (scaled as i128 - exact).abs();
            assert!(err <= 3 << 16, "v_q {v_q}: error {err}·2⁻³⁸ > 3·2⁻²²");
            assert!(
                err > 0,
                "v_q {v_q}: the conversion rounded, so the value is not exact"
            );
        }
        // exact ties: (v − R) = ±(2¹⁵ − ½)·2²² px, so (v − R)·k = ±(2¹⁵ − ½)
        let tie = q53 - (1i64 << 37);
        assert_eq!(camera_on_grid(tie, 0, k).0, 1 << 15, "+(j + ½) rounds up");
        assert_eq!(
            camera_on_grid(-tie, 0, k).0,
            -(1 << 15) + 1,
            "−(j + ½) rounds up"
        );
    }
}
