//! INC-2 T1: the E1R EE-1 transform matrix on the production adapter (OB-45, tracker / membership
//! / damage part; the hit-test part is INC-3).
//!
//! The production tracker is `infynitae_wayland::convert::damage_tracker` built from a real
//! connected output's model and view. It drives Smithay's real `OutputDamageTracker::render_output`
//! into the recording renderer fixture (GLES viewport/scissor semantics). Expected view extents are
//! hand-derived per transform class; the sealed tracker size (`view_O`, transformed) is the negative
//! control and must fail on every rotated case.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::probe_renderer::{CLEAR, Fb, ProbeRenderer, ProbeTex};
use common::{Lockstep, output_k};
use infynitae_backend_headless::VirtualOutput;
use infynitae_core::geom::CanvasPos;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::Transform;
use infynitae_wayland::convert;
use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::test::{DummyFramebuffer, DummyRenderer};
use smithay::utils::{Physical, Rectangle, Scale, Size};

const CASES: [(Transform, u32); 10] = [
    (Transform::Normal, 120),
    (Transform::R90, 120),
    (Transform::R180, 120),
    (Transform::R270, 120),
    (Transform::R90, 150),
    (Transform::R270, 150),
    (Transform::R90, 180),
    (Transform::R270, 180),
    (Transform::Flipped90, 150),
    (Transform::Flipped, 120),
];

/// Hand-derived view extent of a 1920x1080 panel: quarter turns exchange width and height.
fn expected_view(t: Transform) -> (i32, i32) {
    match t {
        Transform::R90 | Transform::R270 | Transform::Flipped90 | Transform::Flipped270 => {
            (1080, 1920)
        }
        _ => (1920, 1080),
    }
}

fn rotated(o: Transform, scale_120: u32) -> VirtualOutput {
    let mut v = output_k("A", (1920, 1080), scale_120, 60_000);
    v.transform = o;
    v
}

fn solid(r: Rectangle<i32, Physical>, id: u32) -> SolidColorRenderElement {
    SolidColorRenderElement::new(
        Id::new(),
        r,
        0usize,
        Color32F::new(id as f32, 0.0, 0.0, 1.0),
        Kind::Unspecified,
    )
}

/// One full frame through `tracker` (age 0 ⇒ full damage): framebuffer, damage, render() size.
fn frame(
    tracker: &mut OutputDamageTracker,
    elems: &[SolidColorRenderElement],
) -> (Fb, Vec<Rectangle<i32, Physical>>, Size<i32, Physical>) {
    let fbcell = Rc::new(RefCell::new(Fb::new(1920, 1080)));
    let mut r = ProbeRenderer { fb: fbcell.clone() };
    let mut target = ProbeTex;
    let res = tracker
        .render_output(
            &mut r,
            &mut target,
            0,
            elems,
            Color32F::new(CLEAR as f32, 0.0, 0.0, 1.0),
        )
        .expect("render_output");
    let damage = res.damage.cloned().unwrap_or_default();
    drop(r);
    let fb = Rc::try_unwrap(fbcell).ok().unwrap().into_inner();
    let vs = fb.render_calls[0].0;
    (fb, damage, vs)
}

/// C1–C4 of EE-1 for one tracker; returns the number of failing checks.
fn checks(tracker: &mut OutputDamageTracker, v: (i32, i32)) -> usize {
    let elems = vec![
        solid(Rectangle::new((0, 0).into(), (v.0, 1).into()), 10),
        solid(Rectangle::new((0, v.1 - 1).into(), (v.0, 1).into()), 11),
        solid(Rectangle::new((0, 1).into(), (1, v.1 - 2).into()), 12),
        solid(Rectangle::new((v.0 - 1, 1).into(), (1, v.1 - 2).into()), 13),
        solid(Rectangle::new((0, 0).into(), v.into()), 2),
    ];
    let (fb, damage, vs) = frame(tracker, &elems);
    let mut fails = 0;
    // C1 viewport == panel
    fails += usize::from(vs != Size::from((1920, 1080)));
    // C2 the view covers the panel exactly
    let blank = fb.px.iter().filter(|&&p| p == 0 || p == CLEAR).count();
    fails += usize::from(blank != 0 || fb.lost != 0);
    // C3 the four markers land on the four panel edges at full length
    let mut edges = vec![];
    let mut marker_ok = true;
    for (id, len) in [(10, v.0), (11, v.0), (12, v.1 - 2), (13, v.1 - 2)] {
        let pts: Vec<(i32, i32)> = (0..fb.h)
            .flat_map(|y| (0..fb.w).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.px[(y * fb.w + x) as usize] == id)
            .collect();
        let edge = if !pts.is_empty() && pts.iter().all(|p| p.1 == 0) {
            Some("top")
        } else if !pts.is_empty() && pts.iter().all(|p| p.1 == fb.h - 1) {
            Some("bottom")
        } else if !pts.is_empty() && pts.iter().all(|p| p.0 == 0) {
            Some("left")
        } else if !pts.is_empty() && pts.iter().all(|p| p.0 == fb.w - 1) {
            Some("right")
        } else {
            None
        };
        marker_ok &= edge.is_some() && pts.len() as i32 == len;
        edges.extend(edge);
    }
    edges.sort();
    edges.dedup();
    fails += usize::from(!(marker_ok && edges.len() == 4));
    // C4 full-frame damage == view extent
    let (mut x0, mut y0, mut x1, mut y1, mut area) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN, 0i64);
    for d in &damage {
        (x0, y0) = (x0.min(d.loc.x), y0.min(d.loc.y));
        (x1, y1) = (x1.max(d.loc.x + d.size.w), y1.max(d.loc.y + d.size.h));
        area += d.size.w as i64 * d.size.h as i64;
    }
    fails += usize::from((x0, y0, x1, y1) != (0, 0, v.0, v.1) || area != v.0 as i64 * v.1 as i64);
    fails
}

/// EE-1 C1–C4 with the production tracker: 0 failures in all 10 cases. Negative control: the sealed
/// tracker size fails every rotated case and none of the unrotated ones.
#[test]
fn ee1_tracker_matrix_on_the_production_adapter() {
    let (mut prod_fails, mut sealed_rot_failed, mut rot_cases, mut sealed_unrot_fails) =
        (0, 0, 0, 0);
    for (t, s) in CASES {
        let ls = Lockstep::new(Some(rotated(t, s)));
        let o = ls.output();
        let (model, view) = (
            ls.state.core().output(o).unwrap().clone(),
            ls.state.core().view(o).unwrap(),
        );
        let v = expected_view(t);
        let f = checks(
            &mut convert::damage_tracker(&model, &view, Default::default()),
            v,
        );
        assert_eq!(
            f, 0,
            "production tracker fails {f} checks for {t:?} at {s}/120"
        );
        prod_fails += f;
        let k = s as f64 / 120.0;
        let swap = v != (1920, 1080);
        let mut sealed = OutputDamageTracker::new(v, Scale::from(k), convert_transform(t));
        let sf = checks(&mut sealed, v);
        if swap {
            rot_cases += 1;
            sealed_rot_failed += usize::from(sf > 0);
        } else {
            sealed_unrot_fails += sf;
        }
    }
    assert_eq!(prod_fails, 0);
    assert_eq!(
        sealed_rot_failed, rot_cases,
        "negative control: the sealed tracker must fail every rotated case"
    );
    assert!(rot_cases == 7);
    assert_eq!(
        sealed_unrot_fails, 0,
        "the sealed tracker is correct without a swap"
    );
}

/// The adapter's own Smithay transform mapping, re-derived here from the wl_output enum.
fn convert_transform(t: Transform) -> smithay::utils::Transform {
    use smithay::utils::Transform as S;
    match t {
        Transform::Normal => S::Normal,
        Transform::R90 => S::_90,
        Transform::R180 => S::_180,
        Transform::R270 => S::_270,
        Transform::Flipped => S::Flipped,
        Transform::Flipped90 => S::Flipped90,
        Transform::Flipped180 => S::Flipped180,
        Transform::Flipped270 => S::Flipped270,
    }
}

/// F-MEMBER-1 in `view_O` space on a rotated output: B (1920x1080 at 90°, scale 1) has a
/// 1080-wide view. W (400x300) is centred on A's camera at the Canvas origin and occupies Canvas
/// x [−200, 200). With B's camera centre x = 800 the view is [260, 1340): no enter (an unswapped
/// 1920-wide reading, [−160, 1760), would show W). At x = 700 it is [160, 1240): W enters.
#[test]
fn membership_uses_the_transformed_view() {
    for (cx, visible) in [(800i64, false), (700, true)] {
        let entries = vec![(
            VirtualOutput::key("B"),
            CanvasPos { x: cx << 16, y: 0 },
            1.0,
        )];
        let mut b = output_k("B", (1920, 1080), 120, 60_000);
        b.transform = Transform::R90;
        let mut ls = Lockstep::with(
            PersistedCameras::from_entries(entries).unwrap(),
            &[output_k("A", (1920, 1080), 120, 60_000), b],
        );
        let nb: u32 = ls
            .probe
            .log
            .iter()
            .rev()
            .find_map(|l| {
                l.strip_prefix("global wl_output ")
                    .map(|n| n.parse().unwrap())
            })
            .unwrap();
        let qh = ls.qh.clone();
        ls.probe.toplevel(&qh, "W", (400, 300), 0);
        ls.pump();
        let entered = ls.probe.log.iter().any(|l| *l == format!("enter W {nb}"));
        assert_eq!(entered, visible, "camera x {cx}");
    }
}

/// Full-frame damage through the production render path (core render list → convert →
/// production tracker) with a real client's surfaces: the first frame (age 0) damages exactly
/// the view extent of each transform class.
#[test]
fn production_render_output_damages_the_view() {
    for (t, s) in CASES {
        let mut ls = Lockstep::new(Some(rotated(t, s)));
        let qh = ls.qh.clone();
        ls.probe.toplevel(&qh, "W", (400, 300), 0);
        ls.pump();
        let o = ls.output();
        let mut renderer = DummyRenderer;
        let mut fb = DummyFramebuffer;
        let damage = convert::render_output(&mut ls.state, &mut renderer, &mut fb, o, 0)
            .expect("render")
            .expect("first frame has damage");
        let v = expected_view(t);
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for d in &damage {
            (x0, y0) = (x0.min(d.loc.x), y0.min(d.loc.y));
            (x1, y1) = (x1.max(d.loc.x + d.size.w), y1.max(d.loc.y + d.size.h));
        }
        assert_eq!((x0, y0, x1, y1), (0, 0, v.0, v.1), "{t:?} at {s}/120");
    }
}

/// The C5 measurement on one output: a client tiles the view with subsurfaces; the production
/// render list (core → `convert::render_elements`) gives each tile's Smithay placement; tiles are
/// redrawn as solid ids at exactly those placements through the production damage tracker. For
/// 200 OutputLogical points p the presented tile is read where a 1×1 cursor at ⌊p·k⌋ lands on
/// the panel (same tracker); the hit tile is `Core::hit_test(O, p)` on the recorded
/// ProjectionFrame. Returns (mismatches, sealed-rule mismatches, hit surface per point).
struct C5 {
    mismatches: usize,
    sealed_mismatches: usize,
    hits: Vec<Option<infynitae_core::snapshot::SurfaceKey>>,
}

/// W's root plus a 12×16 grid of 167×131 tiles around the camera centre, Canvas
/// [−1002, 1002) × [−1048, 1048).
fn tile_view(ls: &mut Lockstep) {
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (40, 30), 0);
    // the window geometry is the root alone (40x30), centred on the camera: the root origin is
    // the camera centre − (20, 15), and the grid is offset by (20, 15) to stay around the centre
    let xdg = ls.probe.surfaces[w].xdg.clone().expect("xdg");
    xdg.set_window_geometry(0, 0, 40, 30);
    ls.pump();
    for iy in 0..16 {
        for ix in 0..12 {
            let off = (20 - 1002 + ix * 167, 15 - 1048 + iy * 131);
            ls.probe.subsurface(&qh, "T", w, off, (167, 131), false);
        }
    }
    ls.pump();
}

fn c5_measure(ls: &mut Lockstep, t: Transform, s: u32) -> C5 {
    use infynitae_backend_headless::render_frame;
    use infynitae_core::geom::OutputLogicalPoint;
    use smithay::backend::renderer::element::Element;

    let o = ls.output();
    let (model, view) = (
        ls.state.core().output(o).unwrap().clone(),
        ls.state.core().view(o).unwrap(),
    );
    let k = s as f64 / 120.0;
    let rendered = render_frame(&mut ls.state, &mut DummyRenderer, o).expect("render");
    assert!(rendered.len() > 20, "{t:?}: too few visible tiles");
    // tile id per render-list position (front to back), placed where Smithay places it
    let ids: Vec<_> = rendered.iter().map(|(e, _)| e.surface).collect();
    let geo: Vec<Rectangle<i32, Physical>> = rendered
        .iter()
        .map(|(_, el)| el.geometry(Scale::from(k)))
        .collect();
    let solids: Vec<_> = geo
        .iter()
        .enumerate()
        .map(|(i, g)| solid(*g, 100 + i as u32))
        .collect();
    let (scene, _, _) = frame(
        &mut convert::damage_tracker(&model, &view, Default::default()),
        &solids,
    );
    let (vw, vh) = (view.size.0 as f64, view.size.1 as f64);
    let (lw, lh) = (vw / k, vh / k);
    let mut pts = vec![
        (0.0, 0.0),
        (lw - 1e-6, 0.0),
        (0.0, lh - 1e-6),
        (lw - 1e-6, lh - 1e-6),
        (lw / 2.0, lh / 2.0),
    ];
    let mut rng: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (rng >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..195 {
        let (a, b) = (next(), next());
        pts.push((a * lw, b * lh));
    }
    let st = convert_transform(t);
    let vsize = Size::<f64, Physical>::from((vw, vh));
    let tile_at = |u: smithay::utils::Point<f64, Physical>| {
        let (ux, uy) = (u.x.floor() as i32, u.y.floor() as i32);
        geo.iter()
            .position(|g| {
                ux >= g.loc.x && uy >= g.loc.y && ux < g.loc.x + g.size.w && uy < g.loc.y + g.size.h
            })
            .map(|i| 100 + i as u32)
    };
    let mut out = C5 {
        mismatches: 0,
        sealed_mismatches: 0,
        hits: Vec::new(),
    };
    // one cursor tracker: after the first full frame, buffer age 1 repaints only the old and new
    // cursor pixels
    let fbcell = Rc::new(RefCell::new(Fb::new(1920, 1080)));
    let mut cursor_tracker = convert::damage_tracker(&model, &view, Default::default());
    for (n, &(px, py)) in pts.iter().enumerate() {
        let cur = Rectangle::new(
            ((px * k).floor() as i32, (py * k).floor() as i32).into(),
            (1, 1).into(),
        );
        fbcell.borrow_mut().first_hit = None;
        let mut r = ProbeRenderer { fb: fbcell.clone() };
        cursor_tracker
            .render_output(
                &mut r,
                &mut ProbeTex,
                usize::from(n > 0),
                &[solid(cur, 7)],
                Color32F::new(CLEAR as f32, 0.0, 0.0, 1.0),
            )
            .expect("cursor frame");
        drop(r);
        let presented = fbcell
            .borrow()
            .first_hit
            .map(|i| scene.px[i])
            .filter(|id| *id != CLEAR);
        let hit_surface = ls
            .state
            .core_mut()
            .hit_test(o, OutputLogicalPoint { x: px, y: py })
            .map(|h| h.surface);
        let hit = hit_surface
            .and_then(|s| ids.iter().position(|x| *x == s))
            .map(|i| 100 + i as u32);
        out.hits.push(hit_surface);
        out.mismatches += usize::from(presented.is_none() || presented != hit);
        let e = smithay::utils::Point::<f64, Physical>::from((px * k, py * k));
        let sealed_a = tile_at(st.transform_point_in(e, &vsize));
        let sealed_b = tile_at(st.invert().transform_point_in(e, &vsize));
        out.sealed_mismatches += usize::from(sealed_a != presented || sealed_b != presented);
    }
    out
}

/// EE-1 C5 on the production path (OB-45 hit-test part): render/hit agreement on every point of
/// all 10 transform cases. Negative control: the sealed hit rule (the transformed point t(p·k)
/// or t⁻¹(p·k), E1R) disagrees on every non-Normal case and agrees at Normal.
#[test]
fn ee1_c5_render_hit_agreement_on_the_production_path() {
    let (mut sealed_nonnormal_failed, mut nonnormal) = (0, 0);
    for (t, s) in CASES {
        let mut ls = Lockstep::new(Some(rotated(t, s)));
        tile_view(&mut ls);
        let m = c5_measure(&mut ls, t, s);
        assert_eq!(m.mismatches, 0, "{t:?} at {s}/120: render/hit mismatches");
        if t != Transform::Normal {
            nonnormal += 1;
            sealed_nonnormal_failed += usize::from(m.sealed_mismatches > 0);
        } else {
            assert_eq!(
                m.sealed_mismatches, 0,
                "the sealed rule is the identity at Normal"
            );
        }
    }
    assert_eq!(
        sealed_nonnormal_failed, nonnormal,
        "negative control: the sealed hit rule must fail every non-Normal case"
    );
    assert_eq!(nonnormal, 9);
}

/// Adversarial N: a pan moves rendering and hit-testing together. After a pan of
/// Σ = (−37.25, −211.5) (camera (37.25, 211.5), a fractional Canvas camera) the C5 agreement
/// still holds on every point, and the hit at most points changed: both moved, not one.
#[test]
fn n_pan_moves_render_and_hit_together() {
    use common::input::{BTN_MIDDLE, logo};
    use infynitae_backend_headless::{keyboard_key, pointer_button, pointer_motion};
    use infynitae_core::input::keys::KEY_LEFTMETA;
    for (t, s) in [(Transform::Normal, 120), (Transform::R90, 150)] {
        let mut ls = Lockstep::new(Some(rotated(t, s)));
        tile_view(&mut ls);
        let before = c5_measure(&mut ls, t, s);
        assert_eq!(before.mismatches, 0);
        keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, 1).unwrap();
        pointer_button(&mut ls.state, &mut ls.input, BTN_MIDDLE, true, logo(), 2).unwrap();
        pointer_motion(
            &mut ls.state,
            &mut ls.input,
            (-37.25, -211.5),
            (0.0, 0.0),
            3,
        )
        .unwrap();
        pointer_button(&mut ls.state, &mut ls.input, BTN_MIDDLE, false, logo(), 4).unwrap();
        keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, 5).unwrap();
        ls.pump();
        assert_eq!(
            ls.state.core().cameras().center(0),
            Some(CanvasPos {
                x: 37 * 65536 + 16384,
                y: 211 * 65536 + 32768
            })
        );
        let after = c5_measure(&mut ls, t, s);
        assert_eq!(
            after.mismatches, 0,
            "{t:?}: render and hit disagree after a pan"
        );
        let moved = before
            .hits
            .iter()
            .zip(&after.hits)
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            moved > 150,
            "{t:?}: only {moved}/200 hits changed: hit-test did not move"
        );
    }
}

/// INC3-F09 regression: verify bounded descriptor usage across repeated Lockstep
/// construction/destruction cycles. Without the fix (shared shm pool and Lockstep Drop client
/// cleanup), each cycle leaked 193 file descriptors, exhausting standard ulimit (1024) in 5 cycles.
#[test]
fn inc3_f09_bounded_resource_cycles() {
    #[cfg(target_os = "linux")]
    let fd_count = || -> usize {
        std::fs::read_dir("/proc/self/fd")
            .map(|entries| entries.flatten().count())
            .unwrap_or(0)
    };

    #[cfg(target_os = "linux")]
    let before = fd_count();

    for _ in 0..20 {
        let mut ls = Lockstep::new(Some(rotated(Transform::Normal, 120)));
        tile_view(&mut ls);
        drop(ls);
    }

    #[cfg(target_os = "linux")]
    {
        let after = fd_count();
        // Allow up to 5 descriptors of variance for standard test runner background threads.
        // The uncorrected leak would accumulate 20 * 193 = 3860 descriptors.
        assert!(
            after <= before + 5,
            "INC3-F09 regression: file descriptors accumulated across construction/destruction cycles (before: {before}, after: {after})"
        );
    }
}
