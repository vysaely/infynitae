//! INC-1 T1 parity (FOUNDATION_TEST_PLAN §4): the Phase B/C/D harness tests ported to the
//! production snapshot extraction, core projection/render/hit and adapter `convert`, each with its
//! negative control.
//!
//! Camera state other than the default cannot be reached before INC-3 (navigation). The
//! fractional-camera and large-magnitude configurations therefore feed explicit view parameters
//! and anchors into the production pure functions (`View::new`, `render::entries_for`,
//! `ProjectionFrame::select`, `convert::render_elements`); nothing is written to Core.

mod common;

use common::oracle::{self, Camera};
use common::{Lockstep, PopupSpec, build_xhit_tree, output};
use infynitae_backend_headless::render_frame;
use infynitae_core::geom::{CanvasPoint, CanvasPos, OutputLogicalPoint, Q16_ONE};
use infynitae_core::projection::{View, ViewParams};
use infynitae_core::render::{FrameEntry, ProjectionFrame, entries_for};
use infynitae_core::stage::ElementId;
use infynitae_wayland::convert;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::{AsRenderElements, Element, Id};
use smithay::backend::renderer::test::DummyRenderer;
use smithay::desktop::WindowSurfaceType;
use smithay::utils::{Physical, Point, Rectangle, Scale};

/// (label, root-surface-local origin, size).
type SurfaceTruth = (&'static str, (i64, i64), (i32, i32));

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

/// Oracle eligibility (INC1-F06): the clamped surface-local cell of an independently unprojected
/// Canvas point `q` (Q16), tested with the independent region evaluator.
fn eligible(e: &FrameEntry, q: (i64, i64)) -> bool {
    let lx = (q.0 - e.origin.x * Q16_ONE).clamp(0, e.size.w as i64 * Q16_ONE - 1);
    let ly = (q.1 - e.origin.y * Q16_ONE).clamp(0, e.size.h as i64 * Q16_ONE - 1);
    oracle::region_contains(&e.input, lx.div_euclid(Q16_ONE), ly.div_euclid(Q16_ONE))
}

/// A camera whose view origin is about `v` (F-VIEW-1), at scale `scale_120`/120 and zoom 1.
fn camera_at(v: (f64, f64), scale_120: u32, size: (u32, u32)) -> Camera {
    let k = scale_120 as f64 / 120.0;
    let q16 = |x: f64| (x * Q16_ONE as f64).round_ties_even() as i64;
    Camera {
        centre: (
            q16(v.0 + size.0 as f64 / (2.0 * k)),
            q16(v.1 + size.1 as f64 / (2.0 * k)),
        ),
        scale_120,
        view_size: size,
    }
}

/// The production view of a camera.
fn view_of(cam: &Camera) -> View {
    View::new(ViewParams {
        centre: CanvasPos {
            x: cam.centre.0,
            y: cam.centre.1,
        },
        zoom: 1.0,
        scale: cam.scale_120 as f64 / 120.0,
        view_size: cam.view_size,
    })
    .unwrap()
}

fn element(ls: &Lockstep) -> ElementId {
    ls.state.elements().next().expect("element")
}

fn label_of(ls: &Lockstep, key: infynitae_core::snapshot::SurfaceKey) -> &'static str {
    let surface = ls.state.registry().surface(key).expect("registered");
    ls.probe
        .surfaces
        .iter()
        .find(|s| {
            use wayland_client::Proxy;
            s.wl.id().protocol_id()
                == smithay::reexports::wayland_server::Resource::id(surface).protocol_id()
        })
        .map(|s| s.label)
        .unwrap_or("?")
}

/// TST-09: the production snapshot equals the client's ground truth (origins, sizes, popup and
/// below-subsurface included); X-15: the render list order and positions equal Smithay's own
/// `Window::render_elements`; negative control: `Window::surface_under` disagrees with the
/// presented stacking, and only by W-SM-11 or W-SM-12.
#[test]
fn tst09_xsnap_x15_and_surface_under_control() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    build_xhit_tree(&mut ls, (400, 300), true);
    let id = element(&ls);
    let o = ls.output();
    let core = ls.state.core();
    let anchor = core.stage().anchor(id).unwrap();
    let tree = core.stage().tree(id).unwrap().clone();
    let origins = tree.origins(anchor).unwrap();
    let root = CanvasPoint {
        x: anchor.x - 20,
        y: anchor.y - 20,
    };
    let mut got: Vec<SurfaceTruth> = tree
        .surfaces
        .iter()
        .zip(&origins)
        .map(|(s, o)| {
            (
                label_of(&ls, s.key),
                (o.x - root.x, o.y - root.y),
                (s.size.w, s.size.h),
            )
        })
        .collect();
    // hand-derived ground truth in root-surface-local coordinates, front to back
    let truth = vec![
        ("PS", (115, 65), (30, 30)),
        ("P", (110, 60), (140, 100)),
        ("S1a", (47, 21), (30, 20)),
        ("S1", (37, 11), (101, 53)),
        ("W", (0, 0), (440, 340)),
        ("S2", (-15, 50), (60, 40)),
    ];
    assert_eq!(got, truth, "snapshot != client ground truth");

    // X-15: Smithay's Window path at scale 1, its root placed where core places the root
    let mut renderer = DummyRenderer;
    let frame = render_frame(&mut ls.state, &mut renderer, o).unwrap();
    let view = ls.state.core().view(o).unwrap();
    let window = ls.state.window(id).unwrap().clone();
    let root_px =
        Point::<i32, Physical>::from((view.edge_x(root.x) as i32, view.edge_y(root.y) as i32));
    let smithay: Vec<WaylandSurfaceRenderElement<DummyRenderer>> =
        window.render_elements(&mut renderer, root_px, Scale::from(1.0), 1.0);
    let id_of =
        |e: &FrameEntry| Id::from_wayland_resource(ls.state.registry().surface(e.surface).unwrap());
    let ours: Vec<Id> = frame.iter().map(|(e, _)| id_of(e)).collect();
    let theirs: Vec<Id> = smithay.iter().map(|e| e.id().clone()).collect();
    assert_eq!(
        ours, theirs,
        "X-15: render order differs from Window::render_elements"
    );
    for ((e, _), s) in frame.iter().zip(&smithay) {
        assert_eq!(
            edges(e),
            presented(s, 1.0),
            "X-15: position differs for {}",
            label_of(&ls, e.surface)
        );
    }

    // negative control: Window::surface_under vs the presented surface
    got.clear();
    let (mut disagreements, mut unexplained) = (0, 0);
    let s2 = frame
        .iter()
        .find(|(e, _)| label_of(&ls, e.surface) == "S2")
        .unwrap()
        .0
        .surface;
    let w = frame
        .iter()
        .find(|(e, _)| label_of(&ls, e.surface) == "W")
        .unwrap()
        .0
        .surface;
    for y in -20..380 {
        for x in -40..460 {
            // root-surface-local point; output-logical point at k = 1
            let (lx, ly) = (x as f64 + 0.5, y as f64 + 0.25);
            let p = OutputLogicalPoint {
                x: root_px.x as f64 + lx,
                y: root_px.y as f64 + ly,
            };
            let ours = ls.state.core_mut().hit_test(o, p).map(|h| h.surface);
            let theirs = window
                .surface_under(Point::from((lx, ly)), WindowSurfaceType::ALL)
                .and_then(|(s, _)| ls.state.registry().key(&s));
            if ours == theirs {
                continue;
            }
            disagreements += 1;
            let w_sm_12 = ours == Some(w) && theirs == Some(s2);
            let near_region_edge = [(lx - 20.0).abs(), (lx - 40.0).abs(), (lx - 420.0).abs()]
                .iter()
                .chain(
                    [
                        (ly - 20.0).abs(),
                        (ly - 55.0).abs(),
                        (ly - 85.0).abs(),
                        (ly - 320.0).abs(),
                    ]
                    .iter(),
                )
                .any(|d| *d <= 0.5);
            if !(w_sm_12 || near_region_edge) {
                unexplained += 1;
            }
        }
    }
    assert!(
        disagreements > 0,
        "negative control: surface_under must disagree with the presented stacking"
    );
    assert_eq!(
        unexplained, 0,
        "every surface_under disagreement must be W-SM-11 or W-SM-12"
    );
}

/// TST-04 (x6): Smithay's render edges for the production `from_surface` location equal core's
/// edge(x) for a root and an odd-offset subsurface, over every scale n/120 (120..=360) and three
/// Canvas magnitudes up to 2³⁶. Negative control: an i32 root location (W-SM-5 style) mismatches.
#[test]
fn tst04_x6_render_edges_equal_lattice_edges() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (803, 601), 17);
    ls.pump();
    ls.probe.subsurface(&qh, "S", w, (37, 11), (101, 53), false);
    ls.pump();
    let id = element(&ls);
    let tree = ls.state.core().stage().tree(id).unwrap().clone();
    assert_eq!(tree.surfaces.len(), 2);
    let mut renderer = DummyRenderer;
    let (mut checked, mut bad, mut bad_i32) = (0u64, 0u64, 0u64);
    for n in 120..=360i64 {
        let k = n as f64 / 120.0;
        for base in [0i64, 1_000_003, (1i64 << 36) - 77_777] {
            for dx in (0..1200).step_by(13) {
                let root = (base + dx, base / 3 + dx / 2);
                let anchor = CanvasPoint {
                    x: root.0 + 17,
                    y: root.1 + 17,
                };
                let view = view_of(&camera_at(
                    ((base - 400 + dx % 7) as f64, (base / 3 - 300) as f64),
                    n as u32,
                    (1920, 1080),
                ));
                let entries = entries_for(&view, id, anchor, &tree);
                let elements =
                    convert::render_elements(&mut renderer, ls.state.registry(), &entries).unwrap();
                assert_eq!(elements.len(), entries.len());
                for (e, el) in &elements {
                    checked += 1;
                    if edges(e) != presented(el, k) {
                        bad += 1;
                    }
                }
                // negative control: round the root location to i32 first, add offsets·k
                let root_l = entries
                    .iter()
                    .find(|e| e.origin.x == root.0)
                    .map(|e| e.location);
                if let Some(rl) = root_l {
                    let shifted: Vec<FrameEntry> = entries
                        .iter()
                        .map(|e| {
                            let mut e = e.clone();
                            e.location.x = rl.x.round() + (e.origin.x - root.0) as f64 * k;
                            e.location.y = rl.y.round() + (e.origin.y - root.1) as f64 * k;
                            e
                        })
                        .collect();
                    for (e, el) in
                        convert::render_elements(&mut renderer, ls.state.registry(), &shifted)
                            .unwrap()
                    {
                        if edges(&e) != presented(&el, k) {
                            bad_i32 += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(
        checked > 50_000,
        "checked {checked} surface rects (off-view surfaces are culled)"
    );
    assert_eq!(
        bad, 0,
        "production f64 location: {bad} of {checked} rects differ from edge(x)"
    );
    assert!(
        bad_i32 > 0,
        "negative control: the i32 root location must mismatch"
    );
}

/// Every pixel over the tree under `view`: production selection vs presented surface; returns
/// (pixels, mismatches, canvas-exact-rule mismatches).
fn hit_parity(
    ls: &Lockstep,
    renderer: &mut DummyRenderer,
    cam: Camera,
    anchor: CanvasPoint,
) -> (u64, u64, u64) {
    let view = view_of(&cam);
    // the production lattice agrees with the independent derivation of c_eff
    assert_eq!(
        (view.c_eff.x, view.c_eff.y),
        oracle::c_eff(&cam),
        "c_eff for {cam:?}"
    );
    let id = element(ls);
    let tree = ls.state.core().stage().tree(id).unwrap();
    let entries = entries_for(&view, id, anchor, tree);
    let elements = convert::render_elements(renderer, ls.state.registry(), &entries).unwrap();
    assert_eq!(elements.len(), entries.len());
    let frame = ProjectionFrame { view, entries };
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for (_, el) in &elements {
        let (a, b, c, d) = presented(el, view.k);
        (x0, y0, x1, y1) = (x0.min(a - 3), y0.min(b - 3), x1.max(c + 3), y1.max(d + 3));
    }
    let (mut n, mut bad, mut old) = (0, 0, 0);
    for j in y0.max(0)..y1.min(view.size.1 as i64) {
        for i in x0.max(0)..x1.min(view.size.0 as i64) {
            n += 1;
            let p = OutputLogicalPoint {
                x: (i as f64 + 0.5) / view.scale,
                y: (j as f64 + 0.5) / view.scale,
            };
            let q = oracle::unproject(&cam, (p.x, p.y));
            let oracle = elements
                .iter()
                .find(|(e, el)| {
                    let (a, b, c, d) = presented(el, view.k);
                    i >= a && i < c && j >= b && j < d && eligible(e, q)
                })
                .map(|(e, _)| e.surface);
            if frame.select(p).map(|h| h.surface) != oracle {
                bad += 1;
            }
            // D-B2-I9 canvas-exact rule (superseded): containment in Canvas, no pixel coverage
            let exact = frame
                .entries
                .iter()
                .find(|e| {
                    let lx = q.0 - e.origin.x * Q16_ONE;
                    let ly = q.1 - e.origin.y * Q16_ONE;
                    lx >= 0
                        && ly >= 0
                        && lx < e.size.w as i64 * Q16_ONE
                        && ly < e.size.h as i64 * Q16_ONE
                        && eligible(e, q)
                })
                .map(|e| e.surface);
            if exact != oracle {
                old += 1;
            }
        }
    }
    (n, bad, old)
}

/// TST-10 (xhit): physical-pixel selection equals the presented surface at k ∈ {1, 1.25, 1.5,
/// 1.75} and fractional cameras; negative control: the canvas-exact rule mismatches.
#[test]
fn tst10_xhit_selection_equals_presented() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    build_xhit_tree(&mut ls, (400, 300), true);
    let mut renderer = DummyRenderer;
    let anchor = CanvasPoint { x: 500, y: 400 };
    let mut old_total = 0;
    for scale_120 in [120, 150, 180, 210] {
        for v in [
            (400.0, 300.0),
            (400.3, 300.7),
            (401.71875, 299.55),
            (359.5, 241.25),
        ] {
            let (n, bad, old) = hit_parity(
                &ls,
                &mut renderer,
                camera_at(v, scale_120, (1920, 1080)),
                anchor,
            );
            assert!(n > 100_000);
            assert_eq!(
                bad, 0,
                "scale {scale_120}/120 v={v:?}: {bad} of {n} pixels differ from the presented surface"
            );
            old_total += old;
        }
    }
    assert!(
        old_total > 0,
        "negative control: the canvas-exact rule must mismatch somewhere"
    );
}

/// TST-18 (d5hit), zoom-1 configurations: output B (3840x2160, scale 1.5, zoom 1) with the D5
/// camera, on the D5 tree (800x600 content at anchor (1100,420)).
#[test]
fn tst18_d5hit_zoom1_configurations() {
    let mut ls = Lockstep::new(Some(output((3840, 2160), 180)));
    build_xhit_tree(&mut ls, (800, 600), true);
    let mut renderer = DummyRenderer;
    let cam = Camera {
        centre: (
            (2200.45 * Q16_ONE as f64).round_ties_even() as i64,
            (900.15 * Q16_ONE as f64).round_ties_even() as i64,
        ),
        scale_120: 180,
        view_size: (3840, 2160),
    };
    let (n, bad, _) = hit_parity(&ls, &mut renderer, cam, CanvasPoint { x: 1100, y: 420 });
    assert!(n > 100_000);
    assert_eq!(
        bad, 0,
        "{bad} of {n} pixels differ from the presented surface"
    );
}

/// M3 / R8: a popup that would leave the output viewport is configured inside the anchor
/// output's target (slide), through Smithay's `get_unconstrained_geometry`.
#[test]
fn popup_is_configured_inside_the_output_target() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (400, 300), 0);
    ls.pump();
    // the toplevel (400x300) is centred on the camera: anchor (−200, −150), so the viewport
    // x ∈ [−960, 960) is parent-relative [−760, 1160)
    let p = ls.probe.popup(
        &qh,
        "P",
        w,
        PopupSpec {
            content: (1200, 80),
            shadow: 0,
            anchor_rect: (390, 50, 1, 1),
            slide: true,
        },
    );
    ls.pump();
    let (x, y, width, height) = ls.probe.surfaces[p]
        .popup_configure
        .expect("popup configured");
    assert_eq!((width, height), (1200, 80));
    assert_eq!(x, 1160 - 1200, "slid left to the viewport's right edge");
    assert_eq!(y, 50);
    // control: without slide the positioner's geometry is kept (it overflows the viewport)
    let q = ls.probe.popup(
        &qh,
        "Q",
        w,
        PopupSpec {
            content: (1200, 80),
            shadow: 0,
            anchor_rect: (390, 50, 1, 1),
            slide: false,
        },
    );
    ls.pump();
    assert_eq!(ls.probe.surfaces[q].popup_configure.map(|c| c.0), Some(390));
}

/// OB-1 (multi-level popups), INC-7: a chain three popups deep, with a sibling popup on the root
/// and a subsurface inside the middle popup. At scale 1 and at a fractional scale:
///
/// 1. the production snapshot equals the client's ground truth, hand-derived in root-surface
///    coordinates. W has a 20 px shadow, so its geometry origin is (20, 20). A popup's geometry
///    origin is its parent's geometry origin plus its anchor point (anchor top-left, gravity
///    bottom-right), and its surface origin is that minus its own shadow:
///    P1 (160x120, shadow 10) at (100, 50) of W: geometry (120, 70), surface (110, 60), 180x140;
///    P2 (90x60, shadow 5) at (30, 20) of P1: geometry (150, 90), surface (145, 85), 100x70;
///    P3 (50x40) at (40, 30) of P2: (190, 120);
///    PS, a 20x20 subsurface of P2 at (5, 5): (150, 90);
///    Q (60x50), a second popup of W at (300, 200): (320, 220).
/// 2. the render list is, in order and position, what Smithay's own `Window::render_elements`
///    produces: the deepest popup in front, a popup's subsurface above it, then the root tree;
/// 3. on every pixel around the tree the hit-test returns the front-most presented surface.
#[test]
fn ob1_nested_popup_chain_matches_what_is_presented() {
    for scale_120 in [120u32, 150] {
        let k = scale_120 as f64 / 120.0;
        let mut ls = Lockstep::new(Some(output((1920, 1080), scale_120)));
        let qh = ls.qh.clone();
        let w = ls.probe.toplevel(&qh, "W", (400, 300), 20);
        ls.pump();
        let spec = |content, shadow, at: (i32, i32)| common::PopupSpec {
            content,
            shadow,
            anchor_rect: (at.0, at.1, 1, 1),
            slide: false,
        };
        let p1 = ls
            .probe
            .popup(&qh, "P1", w, spec((160, 120), 10, (100, 50)));
        ls.pump();
        let p2 = ls.probe.popup(&qh, "P2", p1, spec((90, 60), 5, (30, 20)));
        ls.pump();
        ls.probe.popup(&qh, "P3", p2, spec((50, 40), 0, (40, 30)));
        ls.pump();
        ls.probe.subsurface(&qh, "PS", p2, (5, 5), (20, 20), false);
        ls.pump();
        ls.probe.popup(&qh, "Q", w, spec((60, 50), 0, (300, 200)));
        ls.pump();

        let id = element(&ls);
        let o = ls.output();
        let core = ls.state.core();
        let anchor = core.stage().anchor(id).unwrap();
        let tree = core.stage().tree(id).unwrap().clone();
        let origins = tree.origins(anchor).unwrap();
        let root = CanvasPoint {
            x: anchor.x - 20,
            y: anchor.y - 20,
        };
        let got: Vec<SurfaceTruth> = tree
            .surfaces
            .iter()
            .zip(&origins)
            .map(|(s, o)| {
                (
                    label_of(&ls, s.key),
                    (o.x - root.x, o.y - root.y),
                    (s.size.w, s.size.h),
                )
            })
            .collect();
        let truth = vec![
            ("P3", (190, 120), (50, 40)),
            ("PS", (150, 90), (20, 20)),
            ("P2", (145, 85), (100, 70)),
            ("P1", (110, 60), (180, 140)),
            ("Q", (320, 220), (60, 50)),
            ("W", (0, 0), (440, 340)),
        ];
        assert_eq!(
            got, truth,
            "scale {scale_120}: snapshot != client ground truth"
        );

        let mut renderer = DummyRenderer;
        let frame = render_frame(&mut ls.state, &mut renderer, o).unwrap();
        assert_eq!(
            frame.len(),
            6,
            "scale {scale_120}: all six surfaces are presented"
        );
        for (entry, el) in &frame {
            assert_eq!(
                edges(entry),
                presented(el, k),
                "scale {scale_120}: core edges != presented for {}",
                label_of(&ls, entry.surface)
            );
        }
        // order against Smithay's own Window path (and positions too at scale 1, where its
        // placement and the lattice coincide)
        let view = ls.state.core().view(o).unwrap();
        let window = ls.state.window(id).unwrap().clone();
        let root_px =
            Point::<i32, Physical>::from((view.edge_x(root.x) as i32, view.edge_y(root.y) as i32));
        let smithay: Vec<WaylandSurfaceRenderElement<DummyRenderer>> =
            window.render_elements(&mut renderer, root_px, Scale::from(k), 1.0);
        let id_of = |e: &FrameEntry| {
            Id::from_wayland_resource(ls.state.registry().surface(e.surface).unwrap())
        };
        let ours: Vec<Id> = frame.iter().map(|(e, _)| id_of(e)).collect();
        let theirs: Vec<Id> = smithay.iter().map(|e| e.id().clone()).collect();
        assert_eq!(
            ours, theirs,
            "scale {scale_120}: render order differs from Smithay's"
        );
        if scale_120 == 120 {
            for ((e, _), s) in frame.iter().zip(&smithay) {
                assert_eq!(edges(e), presented(s, 1.0), "{}", label_of(&ls, e.surface));
            }
        }

        // hit-test against what is presented, on every pixel of the tree's bounding box + 3
        let rects: Vec<_> = frame
            .iter()
            .map(|(e, el)| (e.surface, presented(el, k)))
            .collect();
        let (x0, y0, x1, y1) =
            rects
                .iter()
                .fold((i64::MAX, i64::MAX, i64::MIN, i64::MIN), |a, (_, r)| {
                    (
                        a.0.min(r.0 - 3),
                        a.1.min(r.1 - 3),
                        a.2.max(r.2 + 3),
                        a.3.max(r.3 + 3),
                    )
                });
        let (mut checked, mut on_popups) = (0, 0);
        for j in y0..y1 {
            for i in x0..x1 {
                let p = OutputLogicalPoint {
                    x: (i as f64 + 0.5) / k,
                    y: (j as f64 + 0.5) / k,
                };
                let want = rects
                    .iter()
                    .find(|(_, (a, b, c, d))| i >= *a && i < *c && j >= *b && j < *d)
                    .map(|(s, _)| *s);
                let got = ls.state.core_mut().hit_test(o, p).map(|h| h.surface);
                assert_eq!(
                    got.map(|s| label_of(&ls, s)),
                    want.map(|s| label_of(&ls, s)),
                    "scale {scale_120}: pixel ({i},{j})"
                );
                checked += 1;
                on_popups += usize::from(want.is_some_and(|s| label_of(&ls, s) != "W"));
            }
        }
        assert!(
            checked > 100_000 && on_popups > 20_000,
            "{checked} / {on_popups}"
        );
    }
}
