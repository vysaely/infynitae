//! Vertical slice on the production path. A client maps one element (root, subsurfaces,
//! one popup); the adapter extracts the snapshot; Core places it and builds the render list;
//! the adapter's `convert` builds Smithay render elements with `DummyRenderer`; Core hit-tests in
//! physical pixels. Exit tests TST-04 (x6), TST-09 (xsnap + X-15), TST-10 (xhit),
//! TST-18 (d5hit, zoom-1 configurations).

mod common;

use common::oracle::{self, Camera};
use common::{Lockstep, build_xhit_tree, output};
use infynitae_backend_headless::render_frame;
use infynitae_core::geom::{OutputLogicalPoint, Q16_ONE};
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

/// Input-region eligibility of an independently unprojected Canvas point `q` (Q16): the oracle
/// side of xhit (INC1-F06), half-open on the clamped surface-local point.
fn eligible(e: &FrameEntry, q: (i64, i64)) -> bool {
    let lx = (q.0 - e.origin.x * Q16_ONE).clamp(0, e.size.w as i64 * Q16_ONE - 1);
    let ly = (q.1 - e.origin.y * Q16_ONE).clamp(0, e.size.h as i64 * Q16_ONE - 1);
    oracle::region_contains(&e.input, lx.div_euclid(Q16_ONE), ly.div_euclid(Q16_ONE))
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
        // the oracle derives c_eff and q itself from the camera state and output facts
        let core = ls.state.core();
        let camera = core.cameras().camera_for_output(o).expect("bound camera");
        let centre = core.cameras().center(camera).unwrap();
        let cam = Camera {
            centre: (centre.x, centre.y),
            scale_120,
            view_size: (1920, 1080),
        };
        let view = core.view(o).unwrap();
        assert_eq!((view.c_eff.x, view.c_eff.y), oracle::c_eff(&cam));
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
                let q = oracle::unproject(&cam, (p.x, p.y));
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

/// Subsurface unmap on the production path (INC1-F07): a real client unmaps subsurface S1 (null buffer).
/// S1 and its nested subsurface S1a leave the frame (Smithay presents neither, skipping children
/// of a surface without a view), the pixel under S1 hits the root, and remapping S1 restores both.
#[test]
fn unmapped_subsurface_leaves_the_frame_and_returns() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    build_xhit_tree(&mut ls, (400, 300), true);
    let o = ls.output();
    let idx = |ls: &Lockstep, label: &str| {
        ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == label)
            .unwrap()
    };
    let mut renderer = DummyRenderer;
    let first: Vec<_> = render_frame(&mut ls.state, &mut renderer, o)
        .unwrap()
        .into_iter()
        .map(|(e, _)| e.surface)
        .collect();
    // the key of a client surface, through the registry entries of the presented frame
    let key = |ls: &Lockstep, label: &str| {
        use wayland_client::Proxy;
        let client_id = ls.probe.surfaces[idx(ls, label)].wl.id().protocol_id();
        *first
            .iter()
            .find(|k| {
                let s = ls.state.registry().surface(**k).unwrap();
                smithay::reexports::wayland_server::Resource::id(s).protocol_id() == client_id
            })
            .unwrap()
    };
    let (s1, s1a, w) = (key(&ls, "S1"), key(&ls, "S1a"), key(&ls, "W"));
    let keys = |ls: &mut Lockstep, r: &mut DummyRenderer| -> Vec<_> {
        render_frame(&mut ls.state, r, o)
            .unwrap()
            .into_iter()
            .map(|(e, _)| e.surface)
            .collect()
    };
    let before = keys(&mut ls, &mut renderer);
    assert!(before.contains(&s1) && before.contains(&s1a));
    // a pixel inside S1 but outside S1a, S1-local (60, 40). W (400x300) is centred: anchor
    // (−200, −150); S1's origin is root (anchor − 20) + (37,11)
    let view = ls.state.core().view(o).unwrap();
    let px = (
        view.edge_x(-200 - 20 + 37 + 60) as f64 + 0.5,
        view.edge_y(-150 - 20 + 11 + 40) as f64 + 0.5,
    );
    let p = OutputLogicalPoint { x: px.0, y: px.1 };
    assert_eq!(
        ls.state.core_mut().hit_test(o, p).map(|h| h.surface),
        Some(s1)
    );

    let i = idx(&ls, "S1");
    ls.probe.unmap(i);
    ls.pump();
    let after = keys(&mut ls, &mut renderer);
    assert!(!after.contains(&s1), "unmapped S1 is still in the frame");
    assert!(
        !after.contains(&s1a),
        "S1a under an unmapped parent is still in the frame"
    );
    assert_eq!(after.len(), before.len() - 2);
    assert_eq!(
        ls.state.core_mut().hit_test(o, p).map(|h| h.surface),
        Some(w)
    );
    // Smithay agrees: its own Window path presents neither surface
    let id = ls.state.elements().next().unwrap();
    let window = ls.state.window(id).unwrap().clone();
    let smithay: Vec<
        smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement<DummyRenderer>,
    > = smithay::backend::renderer::element::AsRenderElements::render_elements(
        &window,
        &mut renderer,
        (0, 0).into(),
        Scale::from(1.0),
        1.0,
    );
    assert_eq!(
        smithay.len(),
        after.len(),
        "Smithay presents the same number of surfaces"
    );

    let qh = ls.qh.clone();
    ls.probe.draw(&qh, i);
    ls.pump();
    let again = keys(&mut ls, &mut renderer);
    assert!(again.contains(&s1) && again.contains(&s1a));
    assert_eq!(
        ls.state.core_mut().hit_test(o, p).map(|h| h.surface),
        Some(s1)
    );
}
