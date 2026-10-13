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
    /// zoom or scale not finite and positive, or an empty view.
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

impl View {
    pub fn new(p: ViewParams) -> Result<View, ProjectionError> {
        let ok = |x: f64| x.is_finite() && x > 0.0;
        if !ok(p.zoom) || !ok(p.scale) || p.view_size.0 == 0 || p.view_size.1 == 0 {
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
        // F-LATTICE-1: R = floor(v / 120)·120; C = round((v − R)·k + δ); c_eff = R + Q16(C / k)
        let lattice = |v_q: i64| {
            let r = v_q.div_euclid(LATTICE * Q16_ONE) * LATTICE;
            let rel = (v_q - r * Q16_ONE) as f64 / Q16_ONE as f64; // exact: 0 ≤ rel < 120
            let c = (rel * k + DELTA).round() as i64;
            let c_eff = r * Q16_ONE + q16(c as f64 / k);
            (r, c, c_eff)
        };
        let (rx, cx, ex) = lattice(v.x);
        let (ry, cy, ey) = lattice(v.y);
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

fn edge(x: i64, r: i64, c: i64, k: f64) -> i64 {
    ((x - r) as f64 * k + DELTA).round() as i64 - c
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
        // an exact tie: centre 0, extent 1, k = 2¹⁶ → v = −2⁻¹⁷ px = −0.5 Q16 units; ties-even
        // gives 0 where round-half-away would give −1
        let t = View::new(ViewParams {
            centre: CanvasPos { x: 0, y: 0 },
            zoom: 1.0,
            scale: 65536.0,
            view_size: (1, 1),
        })
        .unwrap();
        assert_eq!(t.v.x, 0, "R7 rounds ties to even");
    }

    /// (case, camera centre Q16, scale_120, view size, output-logical p, expected c_eff Q16,
    /// expected q Q16). The expected values are hand-derived from the canonical formula text with
    /// exact rational arithmetic (round-half-even Q16, round-half-away C), outside this crate; see
    /// docs/foundation/evidence/phase-f/INC-1/corrective/unproject_reference.py (INC1-F06).
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
}
