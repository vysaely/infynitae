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
        let f = checks(&mut convert::damage_tracker(&model, &view), v);
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
/// 1080-wide view. W occupies Canvas x [0, 400). With B's camera centre x = 1000 the view is
/// [460, 1540): no enter (an unswapped 1920-wide reading would show W). At x = 900 it is
/// [360, 1440): W enters.
#[test]
fn membership_uses_the_transformed_view() {
    for (cx, visible) in [(1000i64, false), (900, true)] {
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
