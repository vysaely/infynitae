//! C12.F15 evidence (not compiled, not part of the test suite): render/hit sweep.
//! Production View::new / entries_for / ProjectionFrame::select versus real Smithay
//! from_surface + Element::geometry, at fit-like zooms (k ∉ n/120).
mod common;
use common::{Lockstep, output};
use infynitae_core::geom::{CanvasPoint, CanvasPos, OutputLogicalPoint, Q16_ONE};
use infynitae_core::projection::{DELTA, View, ViewParams};
use infynitae_core::render::{FrameEntry, ProjectionFrame, entries_for};
use infynitae_wayland::convert;
use smithay::backend::renderer::element::Element;
use smithay::backend::renderer::test::DummyRenderer;
use smithay::utils::{Physical, Rectangle, Scale};

type R4 = (i64, i64, i64, i64);
fn rnd(t: f64) -> i64 { t.round() as i64 }
fn bits(k: f64, i: i64) -> f64 { f64::from_bits((k.to_bits() as i64 + i) as u64) }

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 >> 12; self.0 ^= self.0 << 25; self.0 ^= self.0 >> 27; self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) }
    fn f(&mut self) -> f64 { (self.next() >> 11) as f64 / (1u64 << 53) as f64 }
    fn r(&mut self, lo: i64, hi: i64) -> i64 { lo + (self.next() % (hi - lo + 1) as u64) as i64 }
}

/// Core's rect and the presented rect for one axis, by the arithmetic of F-LATTICE-1 and of
/// Smithay's geometry (validated against the real elements below).
fn core_x(d: i64, w: i64, k: f64, c: i64) -> (i64, i64) { (rnd(d as f64 * k + DELTA) - c, rnd((d + w) as f64 * k + DELTA) - c) }
fn smithay_x(d: i64, w: i64, k: f64, c: i64) -> (i64, i64) {
    let l = (d as f64 * k + DELTA) - c as f64;
    (rnd(l), rnd(w as f64 * k + l))
}

/// `view` with its x lattice reference moved by `m` lattice periods (a kept reference, as
/// F-LATTICE-1 as amended computes C from it; the kept reference itself is not implemented).
fn keep(mut v: View, m: i64) -> View {
    v.r.0 += 120 * m;
    let rel = (v.v.x - v.r.0 * Q16_ONE) as f64 / Q16_ONE as f64;
    v.c.0 = rnd(rel * v.k + DELTA);
    v
}

fn presented(ls: &Lockstep, r: &mut DummyRenderer, entries: &[FrameEntry], k: f64) -> Vec<R4> {
    convert::render_elements(r, ls.state.registry(), entries).unwrap().iter().map(|(_, el)| {
        let g: Rectangle<i32, Physical> = el.geometry(Scale::from(k));
        (g.loc.x as i64, g.loc.y as i64, (g.loc.x + g.size.w) as i64, (g.loc.y + g.size.h) as i64)
    }).collect()
}

#[test]
fn review_render_hit() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (803, 601), 0);
    ls.pump();
    // two adjacent subsurfaces (A's right edge is B's left edge), above the root
    ls.probe.subsurface(&qh, "A", w, (100, 100), (101, 53), false);
    ls.probe.subsurface(&qh, "B", w, (201, 100), (77, 53), false);
    ls.pump();
    let id = ls.state.elements().next().unwrap();
    let tree = ls.state.core().stage().tree(id).unwrap().clone();
    let mut renderer = DummyRenderer;
    let anchor = CanvasPoint { x: 1000, y: 500 };

    // ---- 1. real Smithay, production View::new, k swept by ulps at A's right-edge rounding point
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    for (label, scale, m_of) in [
        ("per-frame, scale 1", 1.0f64, None),
        ("per-frame, scale 1.5", 1.5, None),
        ("kept |v-R|k~2^20 (R left)", 1.0, Some(-(1i64 << 20))),
        ("kept |v-R|k~2^31 (R left)", 1.0, Some(-(1i64 << 31))),
        ("kept |v-R|k~2^31 (R right)", 1.0, Some(1i64 << 31)),
    ] {
        let (mut n, mut validated) = (0u64, 0u64);
        let mut by_edge = [0u64; 4];
        let (mut seam, mut ptr_bad, mut ptr_checked) = (0u64, 0u64, 0u64);
        let mut example = None;
        for _ in 0..40 {
            let zoom0 = 0.3 + 3.5 * rng.f(); // a fit-like zoom
            let centre = CanvasPos { x: (anchor.x + 150 + rng.r(-60, 60)) * Q16_ONE + rng.r(0, Q16_ONE - 1), y: (anchor.y + 126) * Q16_ONE };
            let mk = |zoom: f64| {
                let v = View::new(ViewParams { centre, zoom, scale, view_size: (1920, 1080) }).unwrap();
                match m_of { None => v, Some(phys) => keep(v, ((phys as f64 / v.k) as i64) / 120) }
            };
            let v0 = mk(zoom0);
            let a = entries_for(&v0, id, anchor, &tree).into_iter().find(|e| e.size.w == 101).unwrap();
            let d1 = a.origin.x + 101 - v0.r.0;
            let h = (d1 as f64 * v0.k).floor() + 0.5;
            let z_target = (h - DELTA) / d1 as f64 / scale;
            for i in -60..=60 {
                let view = mk(bits(z_target, i));
                let entries = entries_for(&view, id, anchor, &tree);
                let pres = presented(&ls, &mut renderer, &entries, view.k);
                for (e, p) in entries.iter().zip(&pres) {
                    n += 1;
                    let dx = e.origin.x - view.r.0;
                    let (m0, m1) = smithay_x(dx, e.size.w as i64, view.k, view.c.0);
                    validated += u64::from(m0 == p.0 && m1 == p.2);
                    let r = e.rect;
                    for (j, (a_, b_)) in [(r.x0, p.0), (r.y0, p.1), (r.x1, p.2), (r.y1, p.3)].into_iter().enumerate() {
                        if a_ != b_ { by_edge[j] += 1; example.get_or_insert((e.size.w, view.k, (r.x0, r.x1), (p.0, p.2))); }
                    }
                }
                // seam between A and B as presented (Core: A.x1 == B.x0 by construction)
                let ia = entries.iter().position(|e| e.size.w == 101);
                let ib = entries.iter().position(|e| e.size.w == 77);
                if let (Some(ia), Some(ib)) = (ia, ib) {
                    assert_eq!(entries[ia].rect.x1, entries[ib].rect.x0);
                    if pres[ia].2 != pres[ib].0 { seam += 1; }
                    // pointer: every column between the two right-edge values, at A's middle row
                    let (lo, hi) = (entries[ia].rect.x1.min(pres[ia].2), entries[ia].rect.x1.max(pres[ia].2));
                    let row = (entries[ia].rect.y0 + entries[ia].rect.y1) / 2;
                    let frame = ProjectionFrame { view, entries: entries.clone() };
                    for col in (lo - 1)..=(hi) {
                        if col < 0 || col >= 1920 { continue; }
                        let p = OutputLogicalPoint { x: (col as f64 + 0.5) / scale, y: (row as f64 + 0.5) / scale };
                        let core = frame.select(p).map(|h| h.surface);
                        let shown = entries.iter().zip(&pres).find(|(_, r)| col >= r.0 && col < r.2 && row >= r.1 && row < r.3).map(|(e, _)| e.surface);
                        ptr_checked += 1;
                        if core != shown { ptr_bad += 1; }
                    }
                }
            }
        }
        println!("REAL {label}: {n} rects (model==Smithay x: {validated}); mismatched edges x0 {} y0 {} x1 {} y1 {}; A|B seams {seam}; pointer columns wrong {ptr_bad}/{ptr_checked}; e.g. {example:?}", by_edge[0], by_edge[1], by_edge[2], by_edge[3]);
    }

    // ---- 2. arithmetic (validated above): random rates, and the k ∈ n/120 regime
    let mut rng = Rng(7);
    for (label, reach) in [("per-frame", 120.0f64), ("kept to 2^31", (1u64 << 31) as f64)] {
        let (mut n, mut bad0, mut bad1) = (0u64, 0u64, 0u64);
        for _ in 0..20_000_000u64 {
            let k = (rng.f() * 22.0 - 2.0).exp2() * (rng.r(30, 960) as f64 / 120.0); // zoom 2^-2..2^20 /scale mix
            let k = k.min(512.0);
            let vr = (rng.f() * 2.0 - 1.0) * (reach / k).max(1.0);
            let c = rnd(vr * k + DELTA);
            let d = vr as i64 + rng.r(0, ((1920.0 / k) as i64).max(1));
            let w = rng.r(1, 4000);
            let (a, b) = (core_x(d, w, k, c), smithay_x(d, w, k, c));
            n += 1; bad0 += u64::from(a.0 != b.0); bad1 += u64::from(a.1 != b.1);
        }
        println!("RATE generic k, {label}: x0 {bad0}/{n}, x1 {bad1}/{n}");
    }
    let (mut n, mut bad) = (0u64, 0u64);
    for n120 in 30..=960i64 {
        let k = n120 as f64 / 120.0;
        for (base, span) in [(0i64, 3000i64), ((1i64 << 31) / n120 * 120 - 3000, 3000), (-(1i64 << 31) / n120 * 120, 3000)] {
            let c = rnd(base as f64 * k + DELTA);
            for d in base..base + span {
                for w in [1i64, 2, 3, 17, 101, 803, 3001] {
                    n += 1;
                    bad += u64::from(core_x(d, w, k, c) != smithay_x(d, w, k, c));
                }
            }
        }
    }
    println!("EXHAUSTIVE k = n/120 (30..960), per-frame and kept ±2^31 bases: {bad}/{n} rects differ");
}
