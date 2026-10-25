//! INC-3 adversarial attacks A–N on the production pointer path (attack O, wrong coordinate
//! domains, is enforced at compile time: probes p18/p19 in tools/verify-architecture).
//!
//! Every expected value is hand-derived from the canonical formulas in the test's comment, and
//! observed on the client's wire (`probe.ptr`): nothing is computed with the production
//! projection, hit-test or pointer code. Shorthand: on a 1920x1080 output at scale 1 with camera
//! centre (cx, cy) on the 120 lattice, c_eff = (cx − 960, cy − 540) and q = c_eff + p.
//!
//! Placement (F-PLACE-1, C12.F1): a window is centred in the view of the first output at its
//! first map, so its window-geometry origin (the anchor) is the camera centre minus half its
//! size, rounded half to even. `map_w`'s W (800x600 content, 20 px shadow) has anchor
//! c − (400, 300): its 840x640 root spans Canvas [cx − 420, cx + 420) × [cy − 320, cy + 320) and
//! the startup pointer, at the camera centre, is at W-local (420, 320).

mod common;

use common::input::*;
use common::{Lockstep, output_k};
use infynitae_core::geom::CanvasPos;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::OutputId;
use infynitae_core::pointer::PointerState;

const EPS: f64 = 1.0 / 256.0;

/// Enter, leave and absolute motion lines only (relative motion and frames dropped).
fn positional(r: &Rig, from: usize) -> Vec<String> {
    r.ls.probe.ptr[from..]
        .iter()
        .filter(|l| l.starts_with("enter") || l.starts_with("leave") || l.starts_with("motion"))
        .cloned()
        .collect()
}

fn located(r: &Rig) -> (OutputId, (f64, f64)) {
    match r.ls.state.core().pointer() {
        PointerState::Located { output, p } => (output, p),
        PointerState::NoOutput => panic!("no output"),
    }
}

fn persisted(entries: &[(&str, i64, i64)]) -> PersistedCameras {
    PersistedCameras::from_entries(
        entries
            .iter()
            .map(|(k, x, y)| {
                (
                    infynitae_backend_headless::VirtualOutput::key(k),
                    CanvasPos {
                        x: x << 16,
                        y: y << 16,
                    },
                    1.0,
                )
            })
            .collect(),
    )
    .unwrap()
}

fn plain(r: &mut Rig, label: &'static str, content: (i32, i32)) -> usize {
    let qh = r.ls.qh.clone();
    let i = r.ls.probe.toplevel(&qh, label, content, 0);
    r.ls.pump();
    r.frame();
    i
}

/// A — physical-pixel boundary at scale 1: W's root spans Canvas x [−420, 420), i.e. p.x ∈
/// [540, 1380). First pixel (p.x = 540) is inside, one wl_fixed step left is outside; last pixel
/// (p.x = 1380 − 2⁻⁸) is inside, p.x = 1380 is outside. Same on y at the top edge: the root
/// spans Canvas y [−320, 320), p.y ∈ [220, 860). On the row p.y = 540 the W-local y is 320.
#[test]
fn a_physical_pixel_boundary() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    r.map_w();
    let m = r.mark();
    r.motion((-420.0, 0.0)); // p (540, 540): first pixel, W-local x 0
    r.motion((-EPS, 0.0)); // pixel 539: outside
    r.motion((EPS, 0.0)); // back on the first pixel
    r.motion((840.0 - EPS, 0.0)); // p.x 1380 − 2⁻⁸: last pixel
    r.motion((EPS, 0.0)); // p.x 1380: outside
    r.motion((-EPS - 820.0, -320.0)); // p (560 − 2⁻⁸, 220): q (−400 − 2⁻⁸, −320), the top row
    r.motion((0.0, -EPS)); // row 219: outside
    assert_eq!(
        positional(&r, m),
        vec![
            "enter W 0,320",
            "leave W",
            "enter W 0,320",
            "motion 839.99609375,320",
            "leave W",
            "enter W 19.99609375,0",
            "leave W",
        ]
    );
}

/// B — fractional scale 1.5 (k = 1.5): camera (0,0) ⇒ v = (−640, −360), R = (−720, −360),
/// C = (120, 0), c_eff = (−640, −360). W (301x201, no shadow) is centred in the 1280x720
/// logical view: anchor = (−150.5, −100.5) → (−150, −100) by R3 (ties to even), so it spans
/// Canvas x [−150, 151) and the row q.y = 0 is W-local y 100. Its right edge projects to
/// round((151 + 720)·1.5 + 2⁻²⁰) − 120 = 1307 − 120 = 1187, so its last pixel column is
/// 1186 = ⌊p·1.5⌋ for p ∈ [790.667, 791.333). p = 790.75 ⇒ q = 150.75: local 300.75.
/// p = 791.25 is still pixel 1186 (inside, ≤ ½ px rendering bound), q = 151.25 lies past the
/// surface and is clamped to 301 − 2⁻¹⁶ (F-HIT-1), which the client receives as wl_fixed
/// 300.99609375. p = 791.5 is pixel 1187: outside.
#[test]
fn b_fractional_scale_edge_and_clamp() {
    let mut r = Rig::new(Lockstep::new(Some(output_k(
        "A",
        (1920, 1080),
        180,
        60_000,
    ))));
    plain(&mut r, "W", (301, 201));
    let m = r.mark();
    // startup p = (640, 360) ⇒ q = (0, 0): W-local (150, 100)
    r.motion((150.75, 0.0));
    r.motion((0.5, 0.0));
    r.motion((0.25, 0.0));
    assert_eq!(
        positional(&r, m),
        vec!["enter W 300.75,100", "motion 300.99609375,100", "leave W"]
    );
}

/// C — negative Canvas: pan the camera to (−5000, −3000) (Σ = (5000, 3000)), then map W there
/// (anchor = camera centre − (400, 300) = (−5400, −3300)). c_eff = (−5960, −3540):
/// R = (−6000, −3600), C = (40, 60). The pointer (960, 540) is q = (−5000, −3000), W-local
/// (420, 320); +(1, 1) enters at (421, 321).
#[test]
fn c_negative_canvas_coordinates() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    r.button(BTN_MIDDLE, true, logo());
    r.motion((5000.0, 3000.0));
    r.button(BTN_MIDDLE, false, logo());
    assert_eq!(r.camera(), pos(-5000, -3000));
    r.map_w();
    let id = r.ls.state.core().stage().z_order()[0];
    assert_eq!(
        r.ls.state.core().stage().anchor(id),
        infynitae_core::geom::CanvasPoint::new(-5400, -3300)
    );
    let m = r.mark();
    r.motion((1.0, 1.0));
    assert_eq!(positional(&r, m), vec!["enter W 421,321"]);
}

/// D — near the ±2³⁶ bound: A's persisted camera (2³⁶ − 2000, −(2³⁶ − 2000)). x: v =
/// 68719473776, R = 68719473720, C = 56; y: v = −68719475276, R = −68719475280, C = 4; c_eff = v.
/// W is centred on the camera, so the pointer is W-local (420, 320) and +(1,1) enters at
/// (421, 321). A pan of Σx = −5000 saturates the camera at x = 2³⁶ exactly (INV-COORD-1); at
/// release q.x = 2³⁶ + 1, beyond W's right edge (2³⁶ − 2000 + 420): the client gets a leave.
#[test]
fn d_large_canvas_coordinates_near_the_bound() {
    let big = (1i64 << 36) - 2000;
    let ls = Lockstep::with(persisted(&[("A", big, -big)]), &[a_1080()]);
    let mut r = Rig::new(ls);
    r.map_w();
    let m = r.mark();
    r.motion((1.0, 1.0));
    assert_eq!(positional(&r, m), vec!["enter W 421,321"]);
    r.button(BTN_MIDDLE, true, logo());
    r.motion((-5000.0, 0.0));
    assert_eq!(r.camera().x, (1i64 << 36) << 16, "camera not saturated");
    let m = r.mark();
    r.button(BTN_MIDDLE, false, logo());
    assert_eq!(positional(&r, m), vec!["leave W"]);
}

/// E — output transition: B (right of A) has camera centre (1000, 0) ⇒ c_eff (40, −540). The
/// pointer goes to A's p.x = 1919 (q.x = 959, past W's right edge at 420: no surface), then +2
/// crosses to B's p = (1, 540) ⇒ q = (41, 0) ⇒ W-local (41 + 420, 0 + 320) = (461, 320): the
/// client sees B's camera, not A's.
#[test]
fn e_output_transition_uses_the_new_outputs_camera() {
    let ls = Lockstep::with(
        persisted(&[("B", 1000, 0)]),
        &[a_1080(), output_k("B", (1920, 1080), 120, 60_000)],
    );
    let mut r = Rig::new(ls);
    r.map_w();
    let b = r.ls.state.core().outputs()[1].id;
    let m = r.mark();
    r.motion((959.0, 0.0));
    assert!(positional(&r, m).is_empty());
    r.motion((2.0, 0.0));
    assert_eq!(located(&r), (b, (1.0, 540.0)));
    assert_eq!(positional(&r, m), vec!["enter W 461,320"]);
}

/// F1 — the interaction output disappears while the pointer has a target, with two survivors:
/// A | B | C, default cameras except C (1000, 0). The pointer on B at (1000, 540) is q = (40, 0),
/// W-local (460, 320). Removing B relocates a = (2920, 540) to the nearest survivor: C at
/// distance 920 (A's rect ends at 1920 − 2⁻⁸, distance 1000 + 2⁻⁸; not first-in-order),
/// p = (0, 540), q = (40, 0): W-local (460, 320) — W is the focus before and after, so the true
/// position arrives as one motion.
/// F2 — near an edge, over no surface: the pointer on B at (1919.5, 540) (q.x = 959.5, past W's
/// right edge at 420) relocates to C (distance 0.5) at (0, 540): the client enters W at
/// (460, 320).
#[test]
fn f_output_removal_relocates_to_the_nearest_survivor() {
    let outs = [
        a_1080(),
        output_k("B", (1920, 1080), 120, 60_000),
        output_k("C", (1920, 1080), 120, 60_000),
    ];
    for (to_b, want) in [
        // the enter carries the position: a motion repeating it is not sent
        (1960.0, vec!["enter W 460,320"]),
        (2879.5, vec!["enter W 460,320"]),
    ] {
        let mut r = Rig::new(Lockstep::with(persisted(&[("C", 1000, 0)]), &outs));
        r.map_w();
        let (b, c) = {
            let o = r.ls.state.core().outputs();
            (o[1].id, o[2].id)
        };
        let m = r.mark();
        r.motion((to_b, 0.0));
        assert_eq!(located(&r).0, b);
        r.ls.disconnect(b);
        assert_eq!(
            located(&r),
            (c, (0.0, 540.0)),
            "relocated to the wrong survivor"
        );
        assert_eq!(
            positional(&r, m),
            want,
            "from B at x {}",
            960.0 + to_b - 1920.0
        );
    }
}

/// H — the first output returns without fabricated history: A panned to camera (300, 0), the
/// pointer moved, A unplugged (NoOutput), A reconnected: the remembered camera is re-bound
/// unchanged (M5) and the pointer is placed by the startup rule at (960, 540), not at its old
/// position. c_eff = (−660, −540) ⇒ q = (300, 0) ⇒ W-local (300 + 420, 0 + 320) = (720, 320).
#[test]
fn h_first_output_returns_without_history() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    r.map_w();
    r.button(BTN_MIDDLE, true, logo());
    r.motion((-300.0, 0.0));
    r.button(BTN_MIDDLE, false, logo());
    r.motion((-500.0, -400.0));
    let a = r.ls.output();
    r.ls.disconnect(a);
    let m = r.mark();
    let a2 = r.ls.connect(&a_1080());
    r.frame();
    assert_eq!(located(&r), (a2, (960.0, 540.0)));
    assert_eq!(r.camera(), pos(300, 0));
    assert_eq!(positional(&r, m), vec!["enter W 720,320"]);
}

/// I/J/K — stacking, input-region holes, unmapped surfaces. W1 (400x300) then W2 (200x100), both
/// without shadow and centred on the camera: W1 spans Canvas [−200,200)x[−150,150) and W2, on
/// top, [−100,100)x[−50,50). The startup pointer is q = (0, 0). W1-local = q + (200, 150),
/// W2-local = q + (100, 50).
/// J: W2's input region excludes its local [0,50)x[0,50), Canvas [−100,−50)x[−50,0); K: W2
/// unmapped.
#[test]
fn i_overlapping_surfaces_route_to_the_presented_one() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    plain(&mut r, "W1", (400, 300));
    plain(&mut r, "W2", (200, 100));
    let m = r.mark();
    r.motion((10.0, 10.0)); // q (10, 10): W2 on top, W2-local (110, 60)
    r.motion((140.0, 0.0)); // q (150, 10): past W2's right edge, only W1: W1-local (350, 160)
    assert_eq!(
        positional(&r, m),
        vec!["enter W2 110,60", "leave W2", "enter W1 350,160"]
    );
}

#[test]
fn j_input_region_holes_fall_through() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    plain(&mut r, "W1", (400, 300));
    let w2 = plain(&mut r, "W2", (200, 100));
    let qh = r.ls.qh.clone();
    r.ls.probe.input_region(
        &qh,
        w2,
        &[(true, (0, 0, 200, 100)), (false, (0, 0, 50, 50))],
    );
    r.ls.pump();
    r.frame();
    let m = r.mark();
    r.motion((-90.0, -40.0)); // q (−90, −40): W2-local (10, 10), its hole: W1 below at (110, 110)
    r.motion((50.0, 0.0)); // q (−40, −40): W2-local (60, 10), W2's region
    r.motion((-11.0, 0.0)); // q (−51, −40): W2-local (49, 10), the hole again: W1 at (149, 110)
    assert_eq!(
        positional(&r, m),
        vec![
            "enter W1 110,110",
            "leave W1",
            "enter W2 60,10",
            "leave W2",
            "enter W1 149,110"
        ]
    );
}

#[test]
fn k_unmapped_surface_receives_nothing() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    let w1 = plain(&mut r, "W1", (400, 300));
    let w2 = plain(&mut r, "W2", (200, 100));
    // S: a 100x50 subsurface of W1 at (250, 0), Canvas [50,150)x[−150,−100), outside W2; Sa: a
    // mapped 30x20 child of S at (10, 10), Canvas [60,90)x[−140,−120). When S is unmapped, Sa
    // keeps its buffer and size but is not shown (its parent is not), so it must not receive
    // input.
    let qh = r.ls.qh.clone();
    let sub =
        r.ls.probe
            .subsurface(&qh, "S", w1, (250, 0), (100, 50), false);
    r.ls.probe
        .subsurface(&qh, "Sa", sub, (10, 10), (30, 20), false);
    r.ls.probe.surfaces[sub].wl.commit();
    r.ls.pump();
    r.frame();
    let m = r.mark();
    r.motion((10.0, 10.0)); // q (10, 10): W2 at (110, 60)
    r.ls.probe.unmap(w2);
    r.ls.pump();
    r.frame();
    r.motion((1.0, 0.0)); // q (11, 10): W2 is unmapped: W1 at (211, 160)
    r.motion((44.0, -155.0)); // q (55, −145): S at (5, 5)
    r.ls.probe.unmap(sub);
    r.ls.pump();
    r.frame();
    r.motion((7.0, 10.0)); // q (62, −135): inside Sa's last rect, but Sa is unshown: W1 (262, 15)
    // The snapshot of an unmapped S has no renderer view, so its offset reads (0, 0) and the
    // unshown Sa would sit at W1-local [10, 40) × [10, 30): probe there too.
    r.motion((-247.0, 0.0)); // q (−185, −135): W1 at (15, 15)
    assert_eq!(
        positional(&r, m),
        vec![
            "enter W2 110,60",
            "leave W2",
            "enter W1 211,160",
            "leave W1",
            "enter S 5,5",
            "leave S",
            "enter W1 262,15",
            "motion 15,15"
        ]
    );
}

/// L — a stale ProjectionFrame cannot route after reconfiguration: A goes from scale 1 to 2.
/// The pointer is first moved to (860, 540), W-local (320, 320). In the 960x540 logical extent
/// x stays 860 and y clamps to 540 − 2⁻⁸. New view: k = 2, v = (−480, −270), R = (−480, −360),
/// C = (0, 180), c_eff = (−480, −270) ⇒ q = (380, 270 − 2⁻⁸) ⇒ W-local (380 + 420,
/// 270 − 2⁻⁸ + 320) = (800, 590 − 2⁻⁸). The old frame (k = 1) would give W-local
/// (320, 320 − 2⁻⁸). (With W centred, the fully clamped corner (960 − 2⁻⁸, 540 − 2⁻⁸) of the
/// earlier version of this test is q.x = 480 − 2⁻⁸, past W's right edge at 420.)
#[test]
fn l_no_stale_frame_after_reconfiguration() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    r.map_w();
    r.motion((-100.0, 0.0));
    let a = r.ls.output();
    let m = r.mark();
    r.ls.reconfigure(a, &output_k("A", (1920, 1080), 240, 60_000));
    assert_eq!(positional(&r, m), vec!["motion 800,589.99609375"]);
}

/// M — a pan leaves the Stage byte-for-byte unchanged: anchors, surface trees, lifecycle states
/// and z-order of every element, through membership changes.
#[test]
fn m_pan_never_writes_the_stage() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    r.map_w();
    plain(&mut r, "V", (100, 100));
    let snapshot = |r: &Rig| {
        let st = r.ls.state.core().stage();
        st.z_order()
            .iter()
            .map(|id| {
                format!(
                    "{id:?} {:?} {:?} {:?}",
                    st.anchor(*id),
                    st.state(*id),
                    st.tree(*id)
                )
            })
            .collect::<Vec<_>>()
    };
    let before = snapshot(&r);
    r.button(BTN_MIDDLE, true, logo());
    for _ in 0..30 {
        r.motion((-97.0, 13.0));
    }
    for _ in 0..30 {
        r.motion((53.0, -71.0));
    }
    r.button(BTN_MIDDLE, false, logo());
    assert_ne!(r.camera(), pos(0, 0));
    assert_eq!(snapshot(&r), before, "INV-CAMERA-2: a pan wrote the Stage");
}
