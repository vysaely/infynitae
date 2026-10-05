//! INC-1 T1: the vertical slice on the production path. A real client maps one element (root,
//! subsurfaces, one popup); the adapter extracts the snapshot; Core places it and builds the
//! render list; the adapter's `convert` builds Smithay render elements with `DummyRenderer`; Core
//! hit-tests in physical pixels. Exit tests TST-04 (x6), TST-09 (xsnap + X-15), TST-10 (xhit),
//! TST-18 (d5hit, zoom-1 configurations).

mod common;

use common::{Lockstep, build_xhit_tree, output};
use infynitae_backend_headless::render_frame;
use infynitae_core::geom::{CanvasPos, OutputLogicalPoint, Q16_ONE};
use infynitae_core::render::FrameEntry;
use infynitae_core::stage::Lifecycle;
use smithay::backend::renderer::element::Element;
use smithay::backend::renderer::test::DummyRenderer;
use smithay::utils::{Physical, Rectangle, Scale};

/// Smithay's presented rect of a render element at scale k, as half-open edges.
fn presented(el: &impl Element, k: f64) -> (i64, i64, i64, i64) {
    let g: Rectangle<i32, Physical> = el.geometry(Scale::from(k));
    (
        g.loc.x as i64,
        g.loc.y as i64,
        (g.loc.x + g.size.w) as i64,
        (g.loc.y + g.size.h) as i64,
    )
}

fn edges(e: &FrameEntry) -> (i64, i64, i64, i64) {
    (e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1)
}

/// Input-region eligibility of a Canvas point for an entry: the oracle side of xhit (half-open on
/// the clamped surface-local point).
fn eligible(e: &FrameEntry, q: CanvasPos) -> bool {
    let lx = (q.x - e.origin.x * Q16_ONE).clamp(0, e.size.w as i64 * Q16_ONE - 1);
    let ly = (q.y - e.origin.y * Q16_ONE).clamp(0, e.size.h as i64 * Q16_ONE - 1);
    e.input
        .contains(lx.div_euclid(Q16_ONE), ly.div_euclid(Q16_ONE))
}

/// The whole slice with the real camera: every pixel over the tree, Core's hit-test against the
/// surface Smithay presents there (DummyRenderer element geometry), at a fractional output scale.
#[test]
fn vertical_slice_hit_test_equals_presented_surface() {
    for scale_120 in [120, 150, 180, 210] {
        let mut ls = Lockstep::new(Some(output((1920, 1080), scale_120)));
        build_xhit_tree(&mut ls, (400, 300), true);
        let id = ls.state.elements().next().expect("element");
        assert_eq!(ls.state.core().stage().state(id), Some(Lifecycle::Mapped));
        let o = ls.output();
        let k = scale_120 as f64 / 120.0;

        let mut renderer = DummyRenderer;
        let frame = render_frame(&mut ls.state, &mut renderer, o).expect("frame");
        assert_eq!(frame.len(), 6, "W, S1, S1a, S2, P, PS are all presented");
        for (entry, el) in &frame {
            assert_eq!(
                edges(entry),
                presented(el, k),
                "core edges != presented at k={k}"
            );
        }
        let view = ls.state.core().view(o).unwrap();
        let (x0, y0, x1, y1) =
            frame
                .iter()
                .fold((i64::MAX, i64::MAX, i64::MIN, i64::MIN), |a, (e, _)| {
                    (
                        a.0.min(e.rect.x0 - 3),
                        a.1.min(e.rect.y0 - 3),
                        a.2.max(e.rect.x1 + 3),
                        a.3.max(e.rect.y1 + 3),
                    )
                });
        let mut checked = 0;
        for j in y0.max(0)..y1.min(1080) {
            for i in x0.max(0)..x1.min(1920) {
                let p = OutputLogicalPoint {
                    x: (i as f64 + 0.5) / k,
                    y: (j as f64 + 0.5) / k,
                };
                let q = view.unproject(p);
                let oracle = frame
                    .iter()
                    .find(|(e, el)| {
                        let (a, b, c, d) = presented(el, k);
                        i >= a && i < c && j >= b && j < d && eligible(e, q)
                    })
                    .map(|(e, _)| e.surface);
                let ours = ls.state.core_mut().hit_test(o, p).map(|h| h.surface);
                assert_eq!(ours, oracle, "hit != presented at pixel ({i},{j}), k={k}");
                checked += 1;
            }
        }
        assert!(checked > 100_000, "covered {checked} pixels");
    }
}

/// Destroying the toplevel purges the element everywhere (M1, INV-STAGE-4).
#[test]
fn destroyed_element_leaves_the_render_list() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    build_xhit_tree(&mut ls, (400, 300), false);
    let o = ls.output();
    let id = ls.state.elements().next().unwrap();
    let mut renderer = DummyRenderer;
    assert!(
        !render_frame(&mut ls.state, &mut renderer, o)
            .unwrap()
            .is_empty()
    );
    let w = ls
        .probe
        .surfaces
        .iter()
        .position(|s| s.label == "W")
        .unwrap();
    ls.probe.destroy_toplevel(w);
    ls.pump();
    assert_eq!(ls.state.core().stage().state(id), None);
    assert!(
        render_frame(&mut ls.state, &mut renderer, o)
            .unwrap()
            .is_empty()
    );
}
