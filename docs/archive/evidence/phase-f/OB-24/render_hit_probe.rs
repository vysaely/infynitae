//! C12.F14 correction (2026-10-17), evidence for C12.F14.4 item 3 — NOT part of the test suite.
//! Smithay 0.7.0's presented rect of a surface (the production `convert::render_elements` →
//! `WaylandSurfaceRenderElement::from_surface` at F-RENDER-1's f64 location L, then
//! `Element::geometry(Scale(k))` = `[round(L), round(fl(w·k) + L))`) compared with Core's
//! F-LATTICE-1 rect `[round((x − R)·k + δ) − C, round((x + w − R)·k + δ) − C)`, for k swept by
//! single ulps around the right edge's rounding point (k ∉ n/120), with the per-frame reference
//! (|v − R| < 120 px) and with a reference kept at |v − R|·k ≈ ±2³¹ (L built from Core's formula;
//! the kept reference itself is not implemented). DummyRenderer: Smithay element geometry, not
//! GPU pixels. It also checks that a model of Smithay's arithmetic equals the real geometry.
//! Run: copy to crates/infynitae-backend-headless/tests/ and
//! `cargo test -p infynitae-backend-headless --test render_hit_probe -- --nocapture`.
//! Output (this tree): render_hit_probe.out.
mod common;
use common::{Lockstep, output};
use infynitae_core::geom::{CanvasPoint, CanvasPos};
use infynitae_core::projection::{DELTA, View, ViewParams};
use infynitae_core::render::entries_for;
use infynitae_wayland::convert;
use smithay::backend::renderer::element::Element;
use smithay::backend::renderer::test::DummyRenderer;
use smithay::utils::{Physical, Rectangle, Scale};

#[test]
fn scratch_smithay_vs_core_right_edge() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let _w = ls.probe.toplevel(&qh, "W", (803, 601), 17);
    ls.pump();
    let id = ls.state.elements().next().unwrap();
    let tree = ls.state.core().stage().tree(id).unwrap().clone();
    let view = View::new(ViewParams { centre: CanvasPos { x: 0, y: 0 }, zoom: 1.0, scale: 1.0, view_size: (1920, 1080) }).unwrap();
    let base = entries_for(&view, id, CanvasPoint { x: 100, y: 100 }, &tree);
    let root = base.iter().find(|e| e.size.w > 100).unwrap().clone();
    let w = root.size.w as i64;
    let mut renderer = DummyRenderer;
    let rnd = |t: f64| t.round() as i64;
    for (label, dv) in [("per-frame |v-R|<120", 60.0f64), ("kept +2^31", 2f64.powi(31)), ("kept -2^31", -2f64.powi(31))] {
        let (mut tot, mut bad, mut model_bad) = (0, 0, 0);
        let mut ex = None;
        for j in 1..60 {
            let k0 = 1.0 + j as f64 * 0.0137;
            let d = (dv / k0) as i64 + 5;
            let h = (((d + w) as f64) * k0).floor() + 0.5;
            let kc = (h - DELTA) / (d + w) as f64;
            for i in -200i64..=200 {
                let k = f64::from_bits((kc.to_bits() as i64 + i) as u64);
                let c = rnd((d - 5) as f64 * k + DELTA);
                let l = (d as f64 * k + DELTA) - c as f64; // F-RENDER-1, as View::render_location
                let core = (rnd(d as f64 * k + DELTA) - c, rnd((d + w) as f64 * k + DELTA) - c);
                let model = (rnd(l), rnd(w as f64 * k + l));
                let mut e = root.clone();
                e.location.x = l;
                let els = convert::render_elements(&mut renderer, ls.state.registry(), &[e]).unwrap();
                let g: Rectangle<i32, Physical> = els[0].1.geometry(Scale::from(k));
                let real = (g.loc.x as i64, (g.loc.x + g.size.w) as i64);
                assert_eq!(real, model, "the model of Smithay's arithmetic");
                tot += 1;
                model_bad += usize::from(model != core);
                if real != core {
                    bad += 1;
                    ex.get_or_insert((d, w, format!("{:#x}", k.to_bits()), c, core, real));
                }
            }
        }
        println!("RENDERHIT {label}: w {w}: real Smithay rect != Core rect in {bad}/{tot} (model {model_bad}); e.g. {ex:?}");
    }
}
