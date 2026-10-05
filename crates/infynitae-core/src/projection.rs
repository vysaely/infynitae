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
