//! Coordinate domains (INV-COORD-1, INV-COORD-4). Each domain is its own type; there is no
//! arithmetic between domains except through the named conversions in `projection` and `snapshot`.

/// Q16 fixed point: 2⁻¹⁶ px per unit.
pub const Q16_ONE: i64 = 1 << 16;

/// Bound on stored Canvas values in px (INV-COORD-1).
pub const CANVAS_LIMIT_PX: i64 = 1 << 36;

// ---- The representable projection domain (F-VIEW-1 as amended by C12.F14, OB-24 D1). Every
// state Core accepts lies inside it, and for every such state the projection's intermediates are
// bounded (see evidence/phase-f/INC-9/ZOOM_CONTRACT.md for the derivation):
//   |v_O| ≤ 2³⁷ px (F-VIEW-1's exact range), |R·2¹⁶| ≤ 2⁵³, |(x − R)·k| ≤ 2⁵⁷, C ≤ 2²⁷,
//   unprojected positions ≤ 2⁵⁴ Q16 units; nothing reaches i64's 2⁶³.

/// Largest output mode side in physical px (2¹⁵ = 32,768; real modes stop at 16,384).
pub const MODE_MAX: u32 = 1 << 15;
/// Output scale range in 1/120 units: 0.25 … 8.
pub const SCALE_120_MIN: u32 = 30;
pub const SCALE_120_MAX: u32 = 960;
/// Smallest camera zoom, 2⁻²⁰: at it, the largest view at the smallest scale still has its
/// origin within F-VIEW-1's exact range, `MODE_MAX / (2 · 2⁻²⁰ · 0.25) = 2³⁶` px from the centre.
pub const ZOOM_MIN: f64 = 1.0 / (1u64 << 20) as f64;
/// Largest camera zoom, 2¹⁶: one Q16 Canvas quantum (2⁻¹⁶ px) is one logical px, so the pointer
/// can still address every logical pixel (F-UNPROJECT-1).
pub const ZOOM_MAX: f64 = (1u64 << 16) as f64;

/// Whether `zoom` is a camera zoom Core accepts: finite and within [`ZOOM_MIN`, `ZOOM_MAX`].
pub fn zoom_in_domain(zoom: f64) -> bool {
    zoom.is_finite() && (ZOOM_MIN..=ZOOM_MAX).contains(&zoom)
}

/// Q16 quantisation of a px value, round-ties-even (R1, R5, R7).
pub(crate) fn q16(px: f64) -> i64 {
    (px * Q16_ONE as f64).round_ties_even() as i64
}

/// A Canvas position in Q16 fixed point (ADR-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasPos {
    pub x: i64,
    pub y: i64,
}

impl CanvasPos {
    pub fn from_point(p: CanvasPoint) -> Self {
        CanvasPos {
            x: p.x * Q16_ONE,
            y: p.y * Q16_ONE,
        }
    }

    /// Integral Canvas point by R3 (round half to even). Fails outside ±2³⁶ px.
    pub fn round_to_point(self) -> Option<CanvasPoint> {
        let r = |v: i64| (v as f64 / Q16_ONE as f64).round_ties_even() as i64;
        CanvasPoint::new(r(self.x), r(self.y))
    }
}

/// An integral Canvas point in px (anchors and surface origins, INV-COORD-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasPoint {
    pub x: i64,
    pub y: i64,
}

impl CanvasPoint {
    /// A stored Canvas point; `None` outside ±2³⁶ px (INV-COORD-1).
    pub fn new(x: i64, y: i64) -> Option<Self> {
        let ok = |v: i64| (-CANVAS_LIMIT_PX..=CANVAS_LIMIT_PX).contains(&v);
        (ok(x) && ok(y)).then_some(CanvasPoint { x, y })
    }
}

/// An integral Canvas rectangle, half-open: `[x0, x1) × [y0, y1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasRect {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

/// A logical point relative to a surface or to a root surface (protocol integers).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalPoint {
    pub x: i32,
    pub y: i32,
}

/// A logical surface size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalSize {
    pub w: i32,
    pub h: i32,
}

/// A logical rectangle in surface-local or root-surface-local coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalRect {
    pub loc: LocalPoint,
    pub size: LocalSize,
}

impl LocalRect {
    /// Half-open containment of an integral cell; empty for non-positive sizes.
    pub fn contains(&self, x: i64, y: i64) -> bool {
        let (x0, y0) = (self.loc.x as i64, self.loc.y as i64);
        x >= x0 && y >= y0 && x < x0 + self.size.w as i64 && y < y0 + self.size.h as i64
    }
}

/// A surface-local position in Q16 (delivered hit coordinates, F-HIT-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalPos {
    pub x: i64,
    pub y: i64,
}

/// A point in an output's logical space (pointer intake only, C3.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputLogicalPoint {
    pub x: f64,
    pub y: f64,
}

/// Physical edges in an output's view space, half-open: `[x0, x1) × [y0, y1)` (F-LATTICE-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalRect {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl PhysicalRect {
    pub fn contains(&self, x: i64, y: i64) -> bool {
        x >= self.x0 && y >= self.y0 && x < self.x1 && y < self.y1
    }
}

/// The f64 render location handed to Smithay (F-RENDER-1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderLocation {
    pub x: f64,
    pub y: f64,
}
