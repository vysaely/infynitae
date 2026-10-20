//! TST-18 (d5hit), zoomed configurations — the INC-10 production parity TST-18 names
//! ("INC-1 (zoom-1 configurations), INC-10 (zoomed configurations)") and part of OB-14's
//! zoom ≠ 1 hit-test coverage ("0 mismatches vs presented").
//!
//! The D5 tree (root, nested subsurface, below-subsurface reached through an input-region hole,
//! popup and popup subsurface; `build_xhit_tree`) at its D5 anchor (1100, 420) on output B
//! (3840 × 2160, scale 1.5), under cameras at zoom ≠ 1, k = zoom · 1.5 ∉ n/120, including ED-1's
//! k = 1080/651. For every pixel over the tree:
//! - render: each surface's presented rect (the adapter's `CanonicalRect` geometry, what the
//!   damage tracker draws) equals the oracle's F-LATTICE-1 rect;
//! - hit: the production selection (`ProjectionFrame::select`) equals the front-most presented
//!   surface whose input region contains the oracle's unprojected point (F-HIT-1).
//!
//! The oracle below is written from the living formula text (F-VIEW-1, F-LATTICE-1 with the
//! per-frame reference of a freshly defined view, F-UNPROJECT-1, F-HIT-1) and calls no production
//! projection, unprojection or region code. TST-19's negative control is ported with it: at every
//! zoom the superseded canvas-exact rule (D-B2-I9) selects another surface on some pixels. What it
//! does not establish: GPU rendering; kept
//! references (the canonical-rect sweep covers those); multi-output spanning.

mod common;

use common::{Lockstep, build_xhit_tree, oracle, output};
use infynitae_core::geom::{CanvasPoint, CanvasPos, OutputLogicalPoint, Q16_ONE};
use infynitae_core::projection::{View, ViewParams};
use infynitae_core::render::{FrameEntry, ProjectionFrame, entries_for};
use infynitae_wayland::convert;
use smithay::backend::renderer::element::Element;
use smithay::backend::renderer::test::DummyRenderer;
use smithay::utils::{Physical, Rectangle, Scale};

const Q: f64 = Q16_ONE as f64;
const DELTA: f64 = 1.0 / (1u64 << 20) as f64;
/// The far-corner sample inside a pixel: 1 − 2⁻⁸ of the way across.
const FAR: f64 = 1.0 - 1.0 / 256.0;

/// Q16 rounding, ties to even (R1, R5, R7).
fn q16(px: f64) -> i64 {
    (px * Q).round_ties_even() as i64
}

/// The oracle's view on one axis, from the formula text: `v = Q16(centre − n / (2k))` (F-VIEW-1);
/// `R = ⌊v / 120⌋ · 120`; `C = round((v − R)·k + δ)`; `c_eff = R + Q16(C / k)` (F-LATTICE-1).
#[derive(Clone, Copy, Debug)]
struct Axis {
    r: i64,
    c: i64,
    c_eff: i64,
}

fn axis(centre_q: i64, extent: u32, k: f64) -> Axis {
    let v = q16(centre_q as f64 / Q - extent as f64 / (2.0 * k));
    let r = v.div_euclid(120 * Q16_ONE) * 120;
    let c = (((v - r * Q16_ONE) as f64 / Q) * k + DELTA).round() as i64;
    Axis {
        r,
        c,
        c_eff: r * Q16_ONE + q16(c as f64 / k),
    }
}

/// `edge(x) = round((x − R)·k + δ) − C` for an integral Canvas edge x.
fn edge(x: i64, a: Axis, k: f64) -> i64 {
    (((x - a.r) as f64) * k + DELTA).round() as i64 - a.c
}

fn presented(el: &impl Element, k: f64) -> (i64, i64, i64, i64) {
    let g: Rectangle<i32, Physical> = el.geometry(Scale::from(k));
    (
        g.loc.x as i64,
        g.loc.y as i64,
        (g.loc.x + g.size.w) as i64,
        (g.loc.y + g.size.h) as i64,
    )
}

/// The oracle's input-region test of the clamped surface-local cell of `q` (F-HIT-1).
fn eligible(e: &FrameEntry, q: (i64, i64)) -> bool {
    let lx = (q.0 - e.origin.x * Q16_ONE).clamp(0, e.size.w as i64 * Q16_ONE - 1);
    let ly = (q.1 - e.origin.y * Q16_ONE).clamp(0, e.size.h as i64 * Q16_ONE - 1);
    oracle::region_contains(&e.input, lx.div_euclid(Q16_ONE), ly.div_euclid(Q16_ONE))
}

/// One camera's result: pixels checked, render mismatches, hit mismatches, delivered-coordinate
/// mismatches, and the largest overrun (Canvas px) of an unclamped hit point past its surface.
struct Parity {
    n: u64,
    render_bad: u64,
    hit_bad: u64,
    local_bad: u64,
    overrun: f64,
    /// Pixels where the superseded canvas-exact rule (D-B2-I9) selects another surface than the
    /// presented one: TST-19's negative control at zoom ≠ 1.
    canvas_exact_differs: u64,
}

fn parity(ls: &Lockstep, centre: (f64, f64), zoom: f64) -> Parity {
    let (scale, size) = (1.5, (3840u32, 2160u32));
    let k = zoom * scale;
    let centre_q = (q16(centre.0), q16(centre.1));
    let view = View::new(ViewParams {
        centre: CanvasPos {
            x: centre_q.0,
            y: centre_q.1,
        },
        zoom,
        scale,
        view_size: size,
    })
    .expect("view in the domain");
    let (ax, ay) = (axis(centre_q.0, size.0, k), axis(centre_q.1, size.1, k));
    let id = ls.state.elements().next().expect("element");
    let tree = ls.state.core().stage().tree(id).expect("tree");
    let entries = entries_for(&view, id, CanvasPoint { x: 1100, y: 420 }, tree);
    let mut renderer = DummyRenderer;
    let elements =
        convert::render_elements(&mut renderer, ls.state.registry(), &entries, view.k).unwrap();
    assert_eq!(elements.len(), entries.len());
    let mut render_bad = 0;
    for (e, el) in &elements {
        let want = (
            edge(e.origin.x, ax, k),
            edge(e.origin.y, ay, k),
            edge(e.origin.x + e.size.w as i64, ax, k),
            edge(e.origin.y + e.size.h as i64, ay, k),
        );
        if presented(el, view.k) != want {
            render_bad += 1;
        }
    }
    let frame = ProjectionFrame { view, entries };
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for (_, el) in &elements {
        let (a, b, c, d) = presented(el, view.k);
        (x0, y0, x1, y1) = (x0.min(a - 3), y0.min(b - 3), x1.max(c + 3), y1.max(d + 3));
    }
    let (mut n, mut hit_bad, mut local_bad, mut overrun) = (0, 0, 0, 0.0f64);
    let mut canvas_exact_differs = 0;
    for j in y0.max(0)..y1.min(size.1 as i64) {
        for (i, f) in (x0.max(0)..x1.min(size.0 as i64)).flat_map(|i| [(i, 0.5), (i, FAR)]) {
            n += 1;
            // a point of physical pixel (i, j), u = p · scale (F-HIT-1): its centre, and a point
            // just inside its far corner, where a covered pixel reaches past the surface's
            // logical edge and the delivered coordinates need the clamp
            let p = OutputLogicalPoint {
                x: (i as f64 + f) / scale,
                y: (j as f64 + f) / scale,
            };
            let q = (ax.c_eff + q16(p.x / zoom), ay.c_eff + q16(p.y / zoom));
            let want = elements
                .iter()
                .find(|(e, el)| {
                    let (a, b, c, d) = presented(el, view.k);
                    i >= a && i < c && j >= b && j < d && eligible(e, q)
                })
                .map(|(e, _)| e.surface);
            let got = frame.select(p);
            if got.map(|h| h.surface) != want {
                hit_bad += 1;
            }
            // the superseded rule: the first surface whose Canvas rect contains q exactly (no
            // pixel coverage) and whose input region contains it
            let canvas_exact = elements
                .iter()
                .find(|(e, _)| {
                    let (lx, ly) = (q.0 - e.origin.x * Q16_ONE, q.1 - e.origin.y * Q16_ONE);
                    lx >= 0
                        && ly >= 0
                        && lx < e.size.w as i64 * Q16_ONE
                        && ly < e.size.h as i64 * Q16_ONE
                        && eligible(e, q)
                })
                .map(|(e, _)| e.surface);
            if canvas_exact != want {
                canvas_exact_differs += 1;
            }
            // OB-14 sub-pixel clamp: delivered = q − origin clamped into [0, w) × [0, h); the
            // unclamped point lies past the surface by less than half a physical pixel
            if let Some(h) = got
                && let Some((e, _)) = elements.iter().find(|(e, _)| e.surface == h.surface)
            {
                let (lx, ly) = (q.0 - e.origin.x * Q16_ONE, q.1 - e.origin.y * Q16_ONE);
                let (w, hh) = (e.size.w as i64 * Q16_ONE, e.size.h as i64 * Q16_ONE);
                if (h.local.x, h.local.y) != (lx.clamp(0, w - 1), ly.clamp(0, hh - 1)) {
                    local_bad += 1;
                }
                let past = |v: i64, extent: i64| (-v).max(v - (extent - 1)).max(0) as f64 / Q;
                overrun = overrun.max(past(lx, w)).max(past(ly, hh));
            }
        }
    }
    Parity {
        n,
        render_bad,
        hit_bad,
        local_bad,
        overrun,
        canvas_exact_differs,
    }
}

#[test]
fn tst18_d5hit_zoomed_configurations() {
    let mut ls = Lockstep::new(Some(output((3840, 2160), 180)));
    build_xhit_tree(&mut ls, (800, 600), true);
    // the D5 camera's fractional centre over the tree, and zooms whose k = 1.5·zoom is not on
    // the 1/120 lattice: ED-1's k = 1080/651, the INC-10 fit and perf zooms, a deep zoom-out and
    // a magnification that crops the tree at the view's edges
    let centre = (1500.45, 720.15);
    let zooms = [1080.0 / 651.0 / 1.5, 0.73, 1.797, 2.391, 0.2537, 3.713];
    let mut total = 0;
    let mut clamped = 0.0f64;
    for zoom in zooms {
        let k: f64 = zoom * 1.5;
        assert!(
            (k * 120.0 - (k * 120.0).round()).abs() > 1e-9,
            "k = {k} must lie off the 1/120 lattice"
        );
        let Parity {
            n,
            render_bad,
            hit_bad,
            local_bad,
            overrun,
            canvas_exact_differs,
        } = parity(&ls, centre, zoom);
        // TST-19's negative control (d5_model: the canvas-exact rule disagrees along W's edges
        // on A0 at ED-1's zoom): the superseded rule must disagree with the presented selection
        // at every zoom, or the comparison above could not tell the rules apart
        assert!(
            canvas_exact_differs > 0,
            "zoom {zoom}: the canvas-exact rule agrees everywhere"
        );
        assert_eq!(
            local_bad, 0,
            "zoom {zoom}: delivered coordinates not q − origin, clamped"
        );
        // ½ physical px over k in Canvas px, plus the Q16 terms (R1 and the R5 snap)
        let bound = 0.5 / k + 3.0 / Q;
        assert!(
            overrun <= bound,
            "zoom {zoom}: a hit point {overrun} Canvas px past its surface (bound {bound})"
        );
        assert!(n > 10_000, "zoom {zoom}: only {n} pixels checked");
        assert_eq!(
            render_bad, 0,
            "zoom {zoom}: presented rect ≠ F-LATTICE-1 rect"
        );
        assert_eq!(
            hit_bad, 0,
            "zoom {zoom}: {hit_bad} of {n} pixels select another surface than presented"
        );
        clamped = clamped.max(overrun);
        total += n;
    }
    assert!(total > 1_000_000, "{total} pixels in all");
    assert!(
        clamped > 0.0,
        "some hit must need the clamp, or it is not exercised"
    );
}
