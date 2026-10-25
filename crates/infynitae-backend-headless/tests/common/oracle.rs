//! Independent test oracle. Written from the canonical formula text
//! (F-VIEW-1, F-LATTICE-1, F-UNPROJECT-1, input-region semantics) in exact integer arithmetic.
//! It calls no production projection, unprojection or region code: a defect there cannot also
//! move the expected value.

use infynitae_core::snapshot::InputRegion;

const Q: i128 = 1 << 16;

/// Parameters a view is computed from, at zoom 1.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// Camera centre, Q16.
    pub centre: (i64, i64),
    pub scale_120: u32,
    pub view_size: (u32, u32),
}

/// ⌊n/d⌋ for d > 0.
fn floor_div(n: i128, d: i128) -> i128 {
    n.div_euclid(d)
}

/// Round half to even of n/d, d > 0.
fn round_half_even(n: i128, d: i128) -> i128 {
    let f = floor_div(n, d);
    let twice_rem = 2 * (n - f * d);
    match twice_rem.cmp(&d) {
        std::cmp::Ordering::Less => f,
        std::cmp::Ordering::Greater => f + 1,
        std::cmp::Ordering::Equal => f + (f & 1),
    }
}

/// Round half away from zero of n/d, d > 0.
fn round_half_away(n: i128, d: i128) -> i128 {
    if n >= 0 {
        floor_div(2 * n + d, 2 * d)
    } else {
        -floor_div(-2 * n + d, 2 * d)
    }
}

/// `c_eff` on one axis: v = Q16(centre − n/(2k)) (ties even); R = ⌊v/120⌋·120;
/// C = round((v − R)·k + 2⁻²⁰) (half away); c_eff = R + Q16(C/k) (ties even); k = s/120.
fn c_eff_axis(centre_q: i64, extent: u32, s: u32) -> i128 {
    let (s, n) = (s as i128, extent as i128);
    // v_q = round_half_even(centre_q − n·Q·120/(2s))
    let v_q = round_half_even(centre_q as i128 * 2 * s - n * Q * 120, 2 * s);
    let r_px = floor_div(v_q, 120 * Q) * 120;
    let rel_q = v_q - r_px * Q;
    // (rel_q/Q)·(s/120) + 2⁻²⁰ over the common denominator 2²⁰·120
    let den = (1i128 << 20) * 120;
    let c = round_half_away(rel_q * s * 16 + 120, den);
    r_px * Q + round_half_even(c * 120 * Q, s)
}

pub fn c_eff(cam: &Camera) -> (i64, i64) {
    (
        c_eff_axis(cam.centre.0, cam.view_size.0, cam.scale_120) as i64,
        c_eff_axis(cam.centre.1, cam.view_size.1, cam.scale_120) as i64,
    )
}

/// F-UNPROJECT-1 at zoom 1: q = c_eff + Q16(p) (ties even). `p · 2¹⁶` is exact in f64.
pub fn unproject(cam: &Camera, p: (f64, f64)) -> (i64, i64) {
    let (cx, cy) = c_eff(cam);
    let q16 = |v: f64| (v * 65536.0).round_ties_even() as i64;
    (cx + q16(p.0), cy + q16(p.1))
}

/// Input-region membership of an integral surface-local cell: requests applied in order, each
/// add/subtract overwriting the cell's membership; half-open rectangles; unset = whole surface.
pub fn region_contains(region: &InputRegion, x: i64, y: i64) -> bool {
    match region {
        InputRegion::Infinite => true,
        InputRegion::Ops(ops) => {
            let mut inside = false;
            for op in ops.iter() {
                let r = op.rect;
                let (x0, y0) = (r.loc.x as i64, r.loc.y as i64);
                let (x1, y1) = (x0 + r.size.w as i64, y0 + r.size.h as i64);
                if x0 <= x && x < x1 && y0 <= y && y < y1 {
                    inside = op.add;
                }
            }
            inside
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_helpers() {
        assert_eq!(round_half_even(5, 2), 2);
        assert_eq!(round_half_even(7, 2), 4);
        assert_eq!(round_half_even(-5, 2), -2);
        assert_eq!(round_half_away(5, 2), 3);
        assert_eq!(round_half_away(-5, 2), -3);
        assert_eq!(round_half_away(-4, 3), -1);
    }
}
