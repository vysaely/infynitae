//! Tests: the scale hints of elements around presentations, holds and their release (INV-SCALE-*).

use super::*;
use crate::navigation::PersistedCameras;
use crate::output::{OutputDescriptor, Transform};
use crate::protocol::tests::*;
use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, tests::key};
use crate::{InputPort, OutputLifecycleToken};

/// The `(scale_120, integer)` hints the effects send to `id`.
fn hints(fx: &[Effect], id: ElementId) -> Vec<(u32, u32)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::PreferredScale {
                element,
                scale_120,
                integer_scale,
            } if *element == id => Some((*scale_120, *integer_scale)),
            _ => None,
        })
        .collect()
}

/// A persisted camera for output B centred at `b_centre_x` px with `b_zoom` (the
/// persisted-camera path, INV-CAMERA-1).
fn camera_b(b_centre_x: i64, b_zoom: f64) -> PersistedCameras {
    PersistedCameras::from_entries(vec![(
        crate::output::tests::key("B"),
        crate::geom::CanvasPos {
            x: b_centre_x << 16,
            y: 0,
        },
        b_zoom,
    )])
    .unwrap()
}

/// Output A (scale 1) at the Canvas origin, then output B (scale 2) with the camera of
/// [`camera_b`]. Windows are placed on A (the lowest-id output).
fn two_scales(c: &mut Core, t: &mut OutputLifecycleToken) -> (OutputId, OutputId) {
    let a = out(c, t, "A", (1920, 1080), 120);
    let b = out(c, t, "B", (1920, 1080), 240);
    (a, b)
}

/// INV-SCALE-1 / C14.4: fullscreen enter and exit are non-client stimuli, so the hint follows
/// the presenting output when W goes fullscreen on an output that did not observe it, and
/// returns when the presentation ends.
#[test]
fn fullscreen_enter_and_exit_move_the_hint_to_the_presenting_output() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    let w = map_win(&mut c, 1, 400, 300);
    assert_eq!(
        c.preferred_scale_of(w),
        Some((120, 1)),
        "windowed on A only"
    );
    focus(&mut c, &mut i, w);
    let (d, fx) = c
        .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(d, ToplevelDecision::Granted);
    assert_eq!(c.presentation(b), Some(w));
    assert_eq!(hints(&fx, w), vec![(240, 2)], "B presents W: rho = 2");
    let (_, fx) = c
        .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
        .unwrap();
    assert_eq!(c.presentation(b), None);
    assert_eq!(
        hints(&fx, w),
        vec![(120, 1)],
        "the presentation ended: rho = 1"
    );
}

/// INV-SCALE-1 "the protocol-fullscreen presentation contributes scale_O (zoom 1)": B observes
/// W windowed through its camera at zoom 0.5 (rho 2 × 0.5 = 1) and presents it at zoom 1
/// (rho 2), with no membership row added or removed by the presentation.
#[test]
fn a_presenting_output_contributes_its_scale_at_zoom_one() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 0.5));
    let (a, b) = two_scales(&mut c, &mut t);
    let w = map_win(&mut c, 1, 400, 300);
    assert!(c.membership().observers(w).contains(&a));
    assert!(
        c.membership().observers(w).contains(&b),
        "B's camera sees W"
    );
    assert_eq!(c.compute_rho(w), Some(1.0), "max(1 × 1, 2 × 0.5)");
    assert_eq!(c.preferred_scale_of(w), Some((120, 1)));
    focus(&mut c, &mut i, w);
    let (_, fx) = c
        .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(c.compute_rho(w), Some(2.0), "B presents W at zoom 1");
    assert_eq!(hints(&fx, w), vec![(240, 2)]);
}

/// A grant made before the first map (C12.F6), as C12.F12 adopts: the initial
/// hint, still before the initial configure (M15), is predicted from the presentation the
/// grant starts (B, scale 2, zoom 1) and the other outputs' cameras (A sees the rect: 1), not
/// from B's camera (zoom 0.5); the map then finds it right and sends no second hint.
#[test]
fn a_pre_map_grant_takes_the_presenting_outputs_scale_when_it_maps() {
    let (mut c, _i, _, mut t) = Core::new(camera_b(100_000, 0.5));
    let (_a, b) = two_scales(&mut c, &mut t);
    let w = c.toplevel_created();
    let (d, _) = c
        .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(d, ToplevelDecision::Recorded);
    let fx = c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
    assert_eq!(
        fx[..2],
        [
            Effect::PreferredScale {
                element: w,
                scale_120: 240,
                integer_scale: 2
            },
            Effect::InitialConfigure(w)
        ],
        "the initial hint is B's, before the one initial configure: {fx:?}"
    );
    assert_eq!(c.toplevel_configure_state(w).fullscreen, Some(b));
    assert_eq!(c.presentation(b), None, "not presented before the map");
    let fx = c.toplevel_committed(w, win(1, 960, 540, true)).unwrap();
    assert_eq!(c.presentation(b), Some(w));
    assert!(hints(&fx, w).is_empty(), "already right: {fx:?}");
    assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
}

/// OB-13 on the integration, with C12.F11: a grant's configure asks for the
/// output's size, and the hint that reaches the client before it (item 3) already fits that
/// size within D_max. W is seen by a scale-3 output P; D_max 8192; granted fullscreen on a
/// 3840-wide scale-1 output O. Without C12.F11: hint 3, a 11,520-px first buffer.
#[test]
fn a_grant_never_asks_for_a_buffer_over_the_device_limit() {
    let persisted = PersistedCameras::from_entries(vec![(
        crate::output::tests::key("O"),
        crate::geom::CanvasPos {
            x: 100_000 << 16,
            y: 0,
        },
        1.0,
    )])
    .unwrap();
    let (mut c, mut i, _, mut t) = Core::new(persisted);
    out(&mut c, &mut t, "P", (1920, 1080), 360);
    let o = out(&mut c, &mut t, "O", (3840, 2160), 120);
    c.set_renderer_device_max(8192).unwrap();
    let w = map_win(&mut c, 1, 400, 300);
    assert_eq!(c.preferred_scale_of(w), Some((360, 3)));
    focus(&mut c, &mut i, w);
    let (_, fx) = c
        .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(o) })
        .unwrap();
    let size = c
        .toplevel_configure_state(w)
        .size
        .expect("a fullscreen size");
    let (s120, b) = c.preferred_scale_of(w).unwrap();
    let side = size.w.max(size.h) as u64;
    assert!(side * u64::from(b) <= 8192, "integer: {side} x {b}");
    assert!(
        side * u64::from(s120) <= 8192 * 120,
        "fractional: {side} x {s120}/120"
    );
    let hint = fx
        .iter()
        .position(|e| matches!(e, Effect::PreferredScale { element, .. } if *element == w))
        .expect("the hint is lowered in the grant's batch");
    let configure = fx
        .iter()
        .position(|e| matches!(e, Effect::ToplevelConfigure(e) if *e == w))
        .unwrap();
    assert!(hint < configure, "{fx:?}");
}

/// C12.F11: the saved size offered at fullscreen exit is a size Core asks for. X (3000 px
/// wide, seen by A at scale 1 and B at scale 2, D_max 4096) is presented on B and redraws at
/// B's 1920 px, so its hint rises to 2. The unset offers 3000 px again: the hint in force
/// when that configure is sent must bound it (3000 x 2 would be 6000 px), and stays bounded
/// until X redraws at another size.
#[test]
fn the_size_offered_at_fullscreen_exit_fits_the_device_limit() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let (a, b) = two_scales(&mut c, &mut t);
    c.set_renderer_device_max(4096).unwrap();
    let x = map_win(&mut c, 9, 3000, 200);
    let fits = |c: &Core, side: u64| {
        let (s120, b) = c.preferred_scale_of(x).unwrap();
        side * u64::from(b) <= 4096 && side * u64::from(s120) <= 4096 * 120
    };
    assert!(fits(&c, 3000));
    focus(&mut c, &mut i, x);
    c.toplevel_request(x, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    c.toplevel_committed(x, win(9, 1920, 1080, true)).unwrap();
    // a non-client stimulus (A's refresh changes) re-evaluates X at its fullscreen size
    c.output_reconfigured(
        &mut t,
        a,
        OutputDescriptor {
            key: crate::output::tests::key("A"),
            mode: (1920, 1080),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 75_000,
        },
    )
    .unwrap();
    assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "presented on B");
    focus(&mut c, &mut i, x);
    let (_, fx) = c
        .toplevel_request(x, ToplevelRequest::UnsetFullscreen)
        .unwrap();
    let configure = fx
        .iter()
        .position(|e| {
            matches!(e, Effect::ToplevelConfigureSize { element, size }
                if *element == x && (size.w, size.h) == (3000, 200))
        })
        .expect("the saved size is offered");
    assert!(fits(&c, 3000), "{:?}", c.preferred_scale_of(x));
    let hint = fx
        .iter()
        .position(|e| matches!(e, Effect::PreferredScale { element, .. } if *element == x))
        .expect("lowered in the unset's batch");
    assert!(hint < configure, "{fx:?}");
    // a commit at the fullscreen size (the client has not redrawn yet) keeps the bound
    c.toplevel_committed(x, win(9, 1920, 1080, true)).unwrap();
    assert!(fits(&c, 3000), "{:?}", c.preferred_scale_of(x));
}

/// C12.F11 + C12.F12 at the initial commit: a pre-map grant's initial hint is
/// already bounded by the configured size, so the first buffer fits D_max. P (scale 3) sees the
/// placed rect, so the prediction is 3; O, the granted output, is 3840 logical px wide:
/// unbounded, the first buffer would be 11,520 px for D_max 8192.
#[test]
fn a_pre_map_grant_initial_hint_fits_the_device_limit() {
    let persisted = PersistedCameras::from_entries(vec![(
        crate::output::tests::key("O"),
        crate::geom::CanvasPos {
            x: 100_000 << 16,
            y: 0,
        },
        1.0,
    )])
    .unwrap();
    let (mut c, _i, _, mut t) = Core::new(persisted);
    out(&mut c, &mut t, "P", (1920, 1080), 360);
    let o = out(&mut c, &mut t, "O", (3840, 2160), 120);
    c.set_renderer_device_max(8192).unwrap();
    let w = c.toplevel_created();
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(o) })
        .unwrap();
    c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
    let size = c
        .toplevel_configure_state(w)
        .size
        .expect("a fullscreen size");
    let (s120, b) = c.preferred_scale_of(w).unwrap();
    let side = size.w.max(size.h) as u64;
    assert!(side * u64::from(b) <= 8192, "integer: {side} x {b}");
    assert!(
        side * u64::from(s120) <= 8192 * 120,
        "fractional: {side} x {s120}/120"
    );
}

/// A presentation hides every other element on its output (OBJ-19). X, seen by A (scale 1)
/// and B (scale 2), loses B when W's presentation starts there, whether the grant is made to
/// a mapped W or before W's first map. The change comes from W's client: X's hint holds (it
/// is not lowered by another client's presentation), while W, the subject, takes B's scale.
#[test]
fn a_clients_presentation_holds_the_hints_of_the_elements_it_hides() {
    for premap in [false, true] {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 300, 200);
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "A and B see X");
        let w = c.toplevel_created();
        let fx = if premap {
            c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap();
            c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
            c.toplevel_committed(w, win(1, 960, 540, true)).unwrap()
        } else {
            c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
            c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
            focus(&mut c, &mut i, w);
            c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap()
                .1
        };
        assert_eq!(c.presentation(b), Some(w), "premap {premap}");
        assert_eq!(c.membership().observers(x), vec![a], "premap {premap}");
        assert_eq!(c.compute_rho(x), Some(1.0));
        assert!(hints(&fx, x).is_empty(), "premap {premap}: {fx:?}");
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "held");
        assert_eq!(c.preferred_scale_of(w), Some((240, 2)), "premap {premap}");
    }
}

/// A presentation the system ends (its output removed) is a non-client change for everyone
/// (a full pass, INV-OUTPUT-1): unlike a client's, it may lower an unrelated element's hint.
#[test]
fn a_presentation_the_system_ends_moves_unrelated_hints() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let (a, b) = two_scales(&mut c, &mut t);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "held while hidden");
    let fx = c.output_disconnected(&mut t, b).unwrap();
    assert_eq!(c.membership().observers(x), vec![a]);
    assert_eq!(hints(&fx, x), vec![(120, 1)], "{fx:?}");
}

/// M9 "settlement": W's fullscreen exit during a camera drag settles W's epoch at once, so
/// the decrease a drag suppresses happens in that batch; X, another client's window, keeps
/// its epoch and its hint.
#[test]
fn a_subjects_fullscreen_exit_during_a_drag_settles_its_epoch() {
    use crate::PhysicalEvent;
    use crate::input::bindings::Modifiers;
    let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
    let button = |pressed| PhysicalEvent::PointerButton {
        button: 0x112,
        pressed,
        modifiers: Modifiers {
            logo: true,
            ..Modifiers::default()
        },
        time_us: 0,
    };
    c.physical_input_vec(&mut i, button(true)).unwrap();
    c.physical_input_vec(
        &mut i,
        PhysicalEvent::PointerMotion {
            delta: (-3.0, 0.0),
            delta_unaccel: (-3.0, 0.0),
            time_us: 0,
        },
    )
    .unwrap();
    assert!(
        c.scale_state(w).unwrap().epoch.open,
        "the drag opened W's epoch"
    );
    assert!(c.scale_state(x).unwrap().epoch.open);
    let (_, fx) = c
        .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
        .unwrap();
    assert_eq!(hints(&fx, w), vec![(120, 1)], "settled now: {fx:?}");
    assert!(!c.scale_state(w).unwrap().epoch.open);
    assert!(c.scale_state(x).unwrap().epoch.open, "X's epoch is not W's");
    assert!(hints(&fx, x).is_empty());
    c.physical_input_vec(&mut i, button(false)).unwrap();
}

/// C12.F13: cross-client churn (2026-10-15). A client that never holds focus
/// cycles create, pre-map grant, map and destroy; another client focused toggles fullscreen.
/// X, seen by A and B, never changes its hint; the presenting clients' own windows may.
#[test]
fn a_client_cycling_fullscreen_does_not_churn_another_clients_hint() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    let x = map_win(&mut c, 9, 300, 200);
    let held = c.preferred_scale_of(x);
    let mut changes = 0;
    for k in 0..20u64 {
        let w = c.toplevel_created();
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        c.toplevel_committed(w, win(100 + k, 0, 0, false)).unwrap();
        let fx = c
            .toplevel_committed(w, win(100 + k, 960, 540, true))
            .unwrap();
        assert_eq!(c.presentation(b), Some(w));
        changes += hints(&fx, x).len();
        changes += hints(&c.toplevel_destroyed(w), x).len();
    }
    let v = map_win(&mut c, 50, 400, 300);
    for _ in 0..20 {
        focus(&mut c, &mut i, v);
        let (_, fx) = c
            .toplevel_request(v, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        changes += hints(&fx, x).len();
        let (_, fx) = c
            .toplevel_request(v, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        changes += hints(&fx, x).len();
    }
    assert_eq!(changes, 0);
    assert_eq!(c.preferred_scale_of(x), held);
}

/// C12.F13 probe A: X, seen by A (scale 1) and B (scale 2), commits
/// between another client's fullscreen toggles on B. Hidden, X is held; its own commits do
/// not lower it (the hold lasts until a non-client stimulus): no change at all.
#[test]
fn a_held_window_committing_between_toggles_keeps_its_hint() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    let mut changes = 0;
    for _ in 0..5 {
        focus(&mut c, &mut i, w);
        let (_, fx) = c
            .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        changes += hints(&fx, x).len();
        changes += hints(&c.toplevel_committed(x, win(9, 300, 200, true)).unwrap(), x).len();
        let (_, fx) = c
            .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        changes += hints(&fx, x).len();
        changes += hints(&c.toplevel_committed(x, win(9, 300, 200, true)).unwrap(), x).len();
    }
    assert_eq!(changes, 0);
    assert_eq!(c.preferred_scale_of(x), Some((240, 2)));
}

/// C12.F13 probe C: X never commits; W1 toggles fullscreen on B while a
/// second client cycles create / pre-map grant on C / map / destroy. No change of X's hint.
#[test]
fn a_passive_window_under_two_presentations_keeps_its_hint() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    let o3 = out(&mut c, &mut t, "C", (1920, 1080), 180);
    let x = map_win(&mut c, 9, 300, 200);
    let w1 = map_win(&mut c, 1, 400, 300);
    let mut changes = 0;
    for k in 0..5u64 {
        focus(&mut c, &mut i, w1);
        let (_, fx) = c
            .toplevel_request(w1, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        changes += hints(&fx, x).len();
        let v = c.toplevel_created();
        c.toplevel_request(v, ToplevelRequest::SetFullscreen { output: Some(o3) })
            .unwrap();
        c.toplevel_committed(v, win(100 + k, 0, 0, false)).unwrap();
        changes += hints(
            &c.toplevel_committed(v, win(100 + k, 960, 540, true))
                .unwrap(),
            x,
        )
        .len();
        changes += hints(&c.toplevel_destroyed(v), x).len();
        focus(&mut c, &mut i, w1);
        let (_, fx) = c
            .toplevel_request(w1, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        changes += hints(&fx, x).len();
    }
    assert_eq!(changes, 0);
}

/// C12.F13 probe B: X is held at B's scale while W is presented on B; a
/// system change (A reconfigured) is a non-client stimulus and lowers X; when W's client ends
/// the presentation (unset, unmap, destroy), X is revealed on B and is raised again (a rise
/// is never refused).
#[test]
fn a_revealed_window_is_raised_after_a_system_change_lowered_it() {
    for end in ["unset", "unmap", "destroy"] {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "{end}: held");
        c.output_reconfigured(
            &mut t,
            a,
            OutputDescriptor {
                key: crate::output::tests::key("A"),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120: 120,
                refresh_mhz: 75_000,
            },
        )
        .unwrap();
        assert_eq!(
            c.preferred_scale_of(x),
            Some((120, 1)),
            "{end}: lowered by the system"
        );
        let fx = match end {
            "unset" => {
                c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                    .unwrap()
                    .1
            }
            "unmap" => c.toplevel_committed(w, win(1, 400, 300, false)).unwrap(),
            _ => c.toplevel_destroyed(w),
        };
        assert_eq!(
            hints(&fx, x),
            vec![(240, 2)],
            "{end}: revealed on B: {fx:?}"
        );
    }
}

/// C12.F13 probe F: a hold ends when it is no longer needed. X reaches
/// into B (scale 2) by its own size; another client toggles fullscreen on B (set, unset);
/// then X shrinks itself off B: its own geometry lowers its hint, as without the toggle.
#[test]
fn a_hold_ends_when_the_presentation_that_caused_it_ends() {
    for toggle in [false, true] {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(1000, 1.0));
        let (a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 1200, 200);
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "X reaches into B");
        if toggle {
            let w = map_win(&mut c, 1, 400, 300);
            focus(&mut c, &mut i, w);
            c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap();
            focus(&mut c, &mut i, w);
            c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                .unwrap();
            assert!(!c.scale_state(x).unwrap().presentation_hold, "no hold left");
        }
        let fx = c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
        assert_eq!(c.membership().observers(x), vec![a], "toggle {toggle}");
        assert_eq!(hints(&fx, x), vec![(120, 1)], "toggle {toggle}: {fx:?}");
    }
}

/// Persisted cameras at the given x centres (px), zoom 1, for the outputs named.
fn cameras_at(at: &[(&str, i64)]) -> PersistedCameras {
    PersistedCameras::from_entries(
        at.iter()
            .map(|(name, x)| {
                (
                    crate::output::tests::key(name),
                    crate::geom::CanvasPos { x: x << 16, y: 0 },
                    1.0,
                )
            })
            .collect(),
    )
    .unwrap()
}

/// C12.F13: the element's own evaluation ends a hold its requirement no longer
/// needs. X (300 px, seen by A and B) is held while W is presented on B; X grows onto D
/// (scale 2): its requirement is back at its hint, the hold ends; X shrinks off D: its own
/// geometry lowers it.
#[test]
fn a_held_windows_own_growth_ends_its_hold() {
    let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("D", 1500)]));
    let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
    let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
    let d = out(&mut c, &mut t, "D", (1920, 1080), 240);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert!(c.scale_state(x).unwrap().presentation_hold, "held");
    c.toplevel_committed(x, win(9, 2400, 200, true)).unwrap();
    assert!(c.membership().observers(x).contains(&d));
    assert!(
        !c.scale_state(x).unwrap().presentation_hold,
        "needed no more"
    );
    let fx = c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
    assert_eq!(c.membership().observers(x), vec![a]);
    assert_eq!(hints(&fx, x), vec![(120, 1)], "{fx:?}");
}

/// C12.F13: a subject's own fullscreen ends its hold. X (seen by A, E and B) is
/// held while W is presented on B; X goes fullscreen on A and back (a subject both times);
/// then X shrinks off A, onto E alone (scale 0.75): its own geometry lowers it.
#[test]
fn a_held_windows_own_fullscreen_ends_its_hold() {
    let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("A", 1500), ("B", 1500)]));
    // E first: X is placed on E (the lowest-id output), origin-aligned (wider than E's view),
    // and A and B see its right part
    let e = out(&mut c, &mut t, "E", (1920, 1080), 90);
    let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
    let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
    let x = map_win(&mut c, 9, 3000, 200);
    assert_eq!(c.membership().observers(x), vec![e, a, b]);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert!(c.scale_state(x).unwrap().presentation_hold, "held");
    focus(&mut c, &mut i, x);
    c.toplevel_request(x, ToplevelRequest::SetFullscreen { output: Some(a) })
        .unwrap();
    focus(&mut c, &mut i, x);
    c.toplevel_request(x, ToplevelRequest::UnsetFullscreen)
        .unwrap();
    assert!(
        !c.scale_state(x).unwrap().presentation_hold,
        "its own fullscreen ended it"
    );
    let fx = c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
    assert_eq!(c.membership().observers(x), vec![e], "off A, on E alone");
    let (s120, _) = hints(&fx, x).pop().expect("lowered by its own geometry");
    assert_eq!(s120, 90, "{fx:?}");
}

/// C12.F13: P7 lowers a held window (the device bound is a correctness rule).
#[test]
fn p7_lowers_a_held_window() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    c.set_renderer_device_max(4096).unwrap();
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "held");
    assert!(c.scale_state(x).unwrap().presentation_hold);
    // X grows to 3000 px: 3000 x 2 > 4096
    let fx = c.toplevel_committed(x, win(9, 3000, 200, true)).unwrap();
    let (s120, _) = hints(&fx, x).pop().expect("P7 lowers it");
    assert!(u64::from(s120) * 3000 <= 4096 * 120, "{s120}");
}

/// C12.F13: a camera action releases the hold of the elements it touches only. The pointer
/// is on A (scale 1, the first output); X is seen by D (scale 1) and B (scale 2), not by A
/// (A's camera was panned away first), and is held at 2 while W is presented on B. A drag of
/// A far from X keeps the hold through the drag and its settling release; a drag that
/// brings X into A's view touches it: the hold ends and X takes its requirement (1).
#[test]
fn a_camera_drag_releases_only_the_holds_it_touches() {
    use crate::PhysicalEvent;
    use crate::input::bindings::Modifiers;
    let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("D", 0)]));
    let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
    let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
    let d = out(&mut c, &mut t, "D", (1920, 1080), 120);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    let button = |pressed| PhysicalEvent::PointerButton {
        button: 0x112,
        pressed,
        modifiers: Modifiers {
            logo: true,
            ..Modifiers::default()
        },
        time_us: 0,
    };
    let motion = |dx: f64| PhysicalEvent::PointerMotion {
        delta: (dx, 0.0),
        delta_unaccel: (dx, 0.0),
        time_us: 0,
    };
    let drag = |c: &mut Core, i: &mut InputPort, steps: &[f64]| {
        let mut fx = c.physical_input_vec(i, button(true)).unwrap();
        for dx in steps {
            fx.extend(c.physical_input_vec(i, motion(*dx)).unwrap());
        }
        fx.extend(c.physical_input_vec(i, button(false)).unwrap());
        fx
    };
    assert_eq!(c.pointer_output(), Some(a));
    drag(&mut c, &mut i, &[-2500.0]);
    assert_eq!(c.membership().observers(x), vec![b, d], "A panned away");
    assert_eq!(c.preferred_scale_of(x), Some((240, 2)));
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert!(c.scale_state(x).unwrap().presentation_hold, "held");
    let fx = drag(&mut c, &mut i, &[-500.0, -500.0]);
    assert!(hints(&fx, x).is_empty(), "a drag elsewhere: {fx:?}");
    assert!(c.scale_state(x).unwrap().presentation_hold, "kept");
    assert_eq!(c.preferred_scale_of(x), Some((240, 2)));
    let fx = drag(&mut c, &mut i, &[1750.0, 1750.0]);
    assert_eq!(c.membership().observers(x), vec![a, d], "A sees X again");
    assert!(!c.scale_state(x).unwrap().presentation_hold, "released");
    assert_eq!(hints(&fx, x), vec![(120, 1)], "{fx:?}");
}

/// The X of [`the_size_offered_at_fullscreen_exit_fits_the_device_limit`] up to the unset:
/// 3000 px wide, seen by A (scale 1) and B (scale 2), D_max 4096, presented on B, redrawn at
/// B's 1920 px, then unset (3000 px offered).
fn offered_after_unset(c: &mut Core, i: &mut InputPort, t: &mut OutputLifecycleToken) -> ElementId {
    let (_a, b) = two_scales(c, t);
    c.set_renderer_device_max(4096).unwrap();
    let x = map_win(c, 9, 3000, 200);
    focus(c, i, x);
    c.toplevel_request(x, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    c.toplevel_committed(x, win(9, 1920, 1080, true)).unwrap();
    focus(c, i, x);
    c.toplevel_request(x, ToplevelRequest::UnsetFullscreen)
        .unwrap();
    x
}

/// C12.F11 (adversarial, 2026-10-16): the offer bounds the hint only until the client redraws
/// at another size. X redraws at the offered 3000 px (its own tree bounds it now), then
/// shrinks to 300 px: nothing bounds it any more, and a later non-client stimulus finds the
/// requirement of A and B (2) — the offer did not leave a permanent cap.
#[test]
fn a_spent_restore_offer_no_longer_bounds_the_hint() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let x = offered_after_unset(&mut c, &mut i, &mut t);
    c.toplevel_committed(x, win(9, 3000, 200, true)).unwrap();
    assert!(
        c.restore_offers.is_empty(),
        "redrawn at another size: spent"
    );
    c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
    // A becomes scale 2 (a non-client stimulus): X's requirement is 2, and 300 x 2 fits
    // 4096; a stale 3000 px offer would cap it at 1
    let fx = rescale_a(&mut c, &mut t, 240);
    assert_eq!(hints(&fx, x), vec![(240, 2)], "{fx:?}");
}

/// Output A reconfigured to `scale_120` with the same logical size (1920 x 1080), a
/// non-client stimulus for everything.
fn rescale_a(c: &mut Core, t: &mut OutputLifecycleToken, scale_120: u32) -> Vec<Effect> {
    let a = c
        .outputs()
        .iter()
        .find(|o| o.key == crate::output::tests::key("A"))
        .unwrap()
        .id;
    c.output_reconfigured(
        t,
        a,
        OutputDescriptor {
            key: crate::output::tests::key("A"),
            mode: (1920 * scale_120 / 120, 1080 * scale_120 / 120),
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: 60_000,
        },
    )
    .unwrap()
}

/// C12.F11 (adversarial): a re-grant before the client redrew saves the size still on offer
/// (its last windowed size), not the fullscreen geometry it still has; the second unset offers
/// 3000 px again and the hint still fits it.
#[test]
fn a_re_grant_before_the_redraw_offers_the_windowed_size_again() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let x = offered_after_unset(&mut c, &mut i, &mut t);
    let b = c.outputs().iter().find(|o| o.scale == 2.0).unwrap().id;
    focus(&mut c, &mut i, x);
    c.toplevel_request(x, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    focus(&mut c, &mut i, x);
    let (_, fx) = c
        .toplevel_request(x, ToplevelRequest::UnsetFullscreen)
        .unwrap();
    assert!(
        fx.iter().any(
            |e| matches!(e, Effect::ToplevelConfigureSize { element, size }
            if *element == x && (size.w, size.h) == (3000, 200))
        ),
        "{fx:?}"
    );
    let (s120, n) = c.preferred_scale_of(x).unwrap();
    assert!(3000 * u64::from(n) <= 4096 && 3000 * u64::from(s120) <= 4096 * 120);
}

/// C12.F11 + C12.F10 (adversarial): an unmap spends the offer with the rest of the
/// fullscreen state; the re-shown window (300 px) is bounded by its own tree only.
#[test]
fn an_unmap_spends_the_restore_offer() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let x = offered_after_unset(&mut c, &mut i, &mut t);
    c.toplevel_committed(x, win(9, 0, 0, false)).unwrap();
    c.toplevel_committed(x, win(9, 300, 200, false)).unwrap();
    c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
    assert!(c.restore_offers.is_empty());
    let fx = rescale_a(&mut c, &mut t, 240);
    assert_eq!(
        hints(&fx, x),
        vec![(240, 2)],
        "bounded by its 300 px only: {fx:?}"
    );
}

/// C12.F13 (adversarial): an output connected is a system change and touches every element:
/// a hold ends there even though the new output never observes the held element.
#[test]
fn an_output_connected_releases_every_hold() {
    let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("F", 100_000)]));
    let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
    let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert!(c.scale_state(x).unwrap().presentation_hold, "held");
    let (_, fx) = c
        .output_connected(
            &mut t,
            OutputDescriptor {
                key: crate::output::tests::key("F"),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120: 120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap();
    assert_eq!(c.membership().observers(x), vec![a], "F looks elsewhere");
    assert!(!c.scale_state(x).unwrap().presentation_hold, "released");
    assert_eq!(hints(&fx, x), vec![(120, 1)], "{fx:?}");
}

/// C12.F13: a reported device limit applies P7 at once and touches no hold; nor does it leave
/// the next, unrelated stimulus releasing every hold. X is held at B's scale while W is
/// presented on B; a limit is reported; then a camera drag of A far from X (touching nothing
/// of X): the hold stays. (The sweep's "re-evaluate everything" flag of the report used to be
/// read as "touched everything" by the drag's evaluation.)
#[test]
fn a_device_limit_report_releases_no_hold_now_or_later() {
    use crate::PhysicalEvent;
    use crate::input::bindings::Modifiers;
    let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("D", 0)]));
    let _a = out(&mut c, &mut t, "A", (1920, 1080), 120);
    let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
    let _d = out(&mut c, &mut t, "D", (1920, 1080), 120);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    let button = |pressed| PhysicalEvent::PointerButton {
        button: 0x112,
        pressed,
        modifiers: Modifiers {
            logo: true,
            ..Modifiers::default()
        },
        time_us: 0,
    };
    let motion = |dx: f64| PhysicalEvent::PointerMotion {
        delta: (dx, 0.0),
        delta_unaccel: (dx, 0.0),
        time_us: 0,
    };
    let drag = |c: &mut Core, i: &mut InputPort, dx: f64| {
        c.physical_input_vec(i, button(true)).unwrap();
        c.physical_input_vec(i, motion(dx)).unwrap();
        c.physical_input_vec(i, button(false)).unwrap();
    };
    drag(&mut c, &mut i, -2500.0);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert!(c.scale_state(x).unwrap().presentation_hold, "held");
    let held = c.preferred_scale_of(x);
    let fx = c.set_renderer_device_max(16384).unwrap();
    assert!(hints(&fx, x).is_empty(), "{fx:?}");
    assert!(
        c.scale_state(x).unwrap().presentation_hold,
        "not released by the report"
    );
    drag(&mut c, &mut i, -500.0);
    assert!(
        c.scale_state(x).unwrap().presentation_hold,
        "a drag elsewhere after the report does not release it"
    );
    assert_eq!(c.preferred_scale_of(x), held);
}

/// C12.F13 as adopted (P-3): the re-evaluation a scale-configuration change causes releases
/// no hold, now or later. As `a_device_limit_report_releases_no_hold_now_or_later`: X is held
/// at B's scale (2) while W is presented on B; a configuration change that does not lower it
/// (a higher `H_cfg`) emits nothing for X, and neither it nor the full pass it schedules (run
/// by a later drag elsewhere) releases the hold. When P7 does lower a held element, it lowers
/// it to the target of its requirement, after which the hold ends by the rule "requirement
/// back at or above its hint" (`a_configuration_change_applies_p7_at_once_and_releases_no_hold`).
#[test]
fn a_configuration_change_releases_no_hold_later() {
    use crate::PhysicalEvent;
    use crate::input::bindings::Modifiers;
    let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("D", 0)]));
    let _a = out(&mut c, &mut t, "A", (1920, 1080), 120);
    let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
    let _d = out(&mut c, &mut t, "D", (1920, 1080), 120);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    let button = |pressed| PhysicalEvent::PointerButton {
        button: 0x112,
        pressed,
        modifiers: Modifiers {
            logo: true,
            ..Modifiers::default()
        },
        time_us: 0,
    };
    let motion = |dx: f64| PhysicalEvent::PointerMotion {
        delta: (dx, 0.0),
        delta_unaccel: (dx, 0.0),
        time_us: 0,
    };
    let drag = |c: &mut Core, i: &mut InputPort, dx: f64| {
        c.physical_input_vec(i, button(true)).unwrap();
        c.physical_input_vec(i, motion(dx)).unwrap();
        c.physical_input_vec(i, button(false)).unwrap();
    };
    drag(&mut c, &mut i, -2500.0);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert!(c.scale_state(x).unwrap().presentation_hold, "held");
    let config = crate::scale::ScalePolicyConfig {
        h_cfg: c.scale_config().h_cfg * 2.0,
        ..*c.scale_config()
    };
    let fx = c.set_scale_config(config).unwrap();
    assert!(hints(&fx, x).is_empty(), "{fx:?}");
    assert!(
        c.scale_state(x).unwrap().presentation_hold,
        "not released by the change"
    );
    let held = c.preferred_scale_of(x);
    drag(&mut c, &mut i, -500.0);
    assert!(
        c.scale_state(x).unwrap().presentation_hold,
        "a drag elsewhere after the change does not release it"
    );
    assert_eq!(c.preferred_scale_of(x), held);
}

/// C12.F13 as adopted (package rev. 2, P-3): a change of the scale configuration applies P7
/// at once and releases no hold. X (300 px, A and B: hint 2) is held while W is presented on
/// B; H_cfg is lowered to 1.5: X is lowered in that call, to a hint the new ceiling allows,
/// and stays held.
#[test]
fn a_configuration_change_applies_p7_at_once_and_releases_no_hold() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert!(c.scale_state(x).unwrap().presentation_hold, "held");
    let config = crate::scale::ScalePolicyConfig {
        h_cfg: 1.5,
        ..*c.scale_config()
    };
    let fx = c.set_scale_config(config).unwrap();
    let (s120, _) = hints(&fx, x).pop().expect("P7 at once");
    assert!(s120 <= 180, "{s120}");
    assert!(c.scale_state(x).unwrap().presentation_hold, "still held");
}

/// C12.F13 (adversarial): a hold survives no change of identity. X is held while W is
/// presented on B; X is destroyed; a new window Y mapped at the same place is not held and
/// takes its requirement (A and B's, 2, once W's presentation ends).
#[test]
fn a_hold_dies_with_its_element() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let (a, b) = two_scales(&mut c, &mut t);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert!(c.scale_state(x).unwrap().presentation_hold);
    c.toplevel_destroyed(x);
    assert!(c.scale_state(x).is_none());
    let y = map_win(&mut c, 10, 300, 200);
    assert_eq!(c.membership().observers(y), vec![a], "hidden on B");
    assert!(!c.scale_state(y).unwrap().presentation_hold);
    assert_eq!(c.preferred_scale_of(y), Some((120, 1)));
}

/// The end of a presentation, however its client ends it (unset, unmap, destroy), reveals
/// the elements it hid. A revealed element whose requirement rises still gets the higher
/// hint in that batch (a hold never leaves it under-resolved): X mapped while W was presented
/// on B, so its hint is A's; revealed, it takes B's.
#[test]
fn the_end_of_a_presentation_raises_the_hint_of_a_revealed_element() {
    for end in ["unset", "unmap", "destroy"] {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (a, b) = two_scales(&mut c, &mut t);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        let x = map_win(&mut c, 9, 300, 200);
        assert_eq!(c.membership().observers(x), vec![a], "{end}: hidden on B");
        assert_eq!(c.preferred_scale_of(x), Some((120, 1)), "{end}");
        let fx = match end {
            "unset" => {
                c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                    .unwrap()
                    .1
            }
            "unmap" => c.toplevel_committed(w, win(1, 400, 300, false)).unwrap(),
            _ => c.toplevel_destroyed(w),
        };
        assert_eq!(c.presentation(b), None, "{end}");
        assert_eq!(c.membership().observers(x), vec![a, b], "{end}");
        assert_eq!(hints(&fx, x), vec![(240, 2)], "{end}: {fx:?}");
    }
}

/// The subject's own fullscreen enter and exit are a non-client stimulus (C14.4): W's last
/// stimulus lowered it (A's scale fell to 1), and a client response could not raise it
/// (SC-E); its own grant on B raises it, and its own unset lowers it again.
#[test]
fn a_subjects_own_fullscreen_is_a_non_client_stimulus() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
    let a = out(&mut c, &mut t, "A", (1920, 1080), 132);
    let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
    let w = map_win(&mut c, 1, 400, 300);
    c.output_reconfigured(
        &mut t,
        a,
        OutputDescriptor {
            key: crate::output::tests::key("A"),
            mode: (1920, 1080),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        },
    )
    .unwrap();
    assert_eq!(c.preferred_scale_of(w), Some((120, 1)));
    assert_eq!(c.scale_state(w).unwrap().last_stimulus_direction, -1);
    focus(&mut c, &mut i, w);
    let (_, fx) = c
        .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(hints(&fx, w), vec![(240, 2)], "{fx:?}");
    let (_, fx) = c
        .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
        .unwrap();
    assert_eq!(hints(&fx, w), vec![(120, 1)], "{fx:?}");
}

/// Fullscreen entry and exit put every scale hint of the batch before its first configure
/// (as M15 orders the initial hint before the initial configure): a grant, an unset (its
/// configure offers the saved size), and the first map of a pre-map grant that replaces the
/// older presentation on the output (the replaced element's configure).
#[test]
fn fullscreen_batches_put_hints_before_configures() {
    let order = |fx: &[Effect]| {
        let hint = fx
            .iter()
            .rposition(|e| matches!(e, Effect::PreferredScale { .. }))
            .expect("a hint");
        let configure = fx
            .iter()
            .position(|e| {
                matches!(
                    e,
                    Effect::ToplevelConfigure(_) | Effect::ToplevelConfigureSize { .. }
                )
            })
            .expect("a configure");
        assert!(hint < configure, "{fx:?}");
    };
    let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    order(
        &c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap()
            .1,
    );
    order(
        &c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap()
            .1,
    );
    // W presented on B; V, granted B before its first map, replaces it when it maps
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    let v = c.toplevel_created();
    c.toplevel_request(v, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    c.toplevel_committed(v, win(2, 0, 0, false)).unwrap();
    let fx = c.toplevel_committed(v, win(2, 960, 540, true)).unwrap();
    assert_eq!(c.presentation(b), Some(v));
    assert!(!hints(&fx, w).is_empty(), "W left B: {fx:?}");
    order(&fx);
}

/// The scale hints of a run in which a presentation starts or ends during a camera drag
/// (the evaluation of a drag's batches is scoped, `Sweep`), with the scoped or the full
/// reference evaluation.
fn presentation_under_a_drag(reference: bool, start: bool) -> Vec<(u32, u32)> {
    use crate::PhysicalEvent;
    use crate::input::bindings::Modifiers;
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    c.scale_reference = reference;
    let (_a, b) = two_scales(&mut c, &mut t);
    let x = map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    if !start {
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
    }
    let logo = Modifiers {
        logo: true,
        ..Modifiers::default()
    };
    let button = |pressed| PhysicalEvent::PointerButton {
        button: 0x112,
        pressed,
        modifiers: logo,
        time_us: 0,
    };
    let mut seen = Vec::new();
    let mut log = |fx: &[Effect]| {
        seen.extend(hints(fx, x));
        seen.push((0, 0));
        seen.extend(hints(fx, w));
        seen.push((1, 1));
    };
    log(&c.physical_input_vec(&mut i, button(true)).unwrap());
    log(&c
        .physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (-3.0, 0.0),
                delta_unaccel: (-3.0, 0.0),
                time_us: 0,
            },
        )
        .unwrap());
    let request = if start {
        ToplevelRequest::SetFullscreen { output: Some(b) }
    } else {
        ToplevelRequest::UnsetFullscreen
    };
    log(&c.toplevel_request(w, request).unwrap().1);
    log(&c.physical_input_vec(&mut i, button(false)).unwrap());
    log(&c.toplevel_committed(x, win(9, 300, 200, true)).unwrap());
    seen.extend(c.preferred_scale_of(x));
    seen.extend(c.preferred_scale_of(w));
    seen
}

/// A presentation that starts or ends during a camera drag: the drag's scoped evaluation
/// visits the elements it hides or reveals, so the hints equal the full reference.
#[test]
fn a_presentation_change_during_a_camera_drag_matches_the_full_reference() {
    for start in [true, false] {
        assert_eq!(
            presentation_under_a_drag(false, start),
            presentation_under_a_drag(true, start),
            "start {start}"
        );
    }
}

/// INV-SCALE-5 "evaluation once per core batch after all its updates": every batch that
/// starts, ends or re-derives a presentation evaluates the policy exactly once.
#[test]
fn each_presentation_batch_evaluates_the_policy_once() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    map_win(&mut c, 9, 300, 200);
    let w = map_win(&mut c, 1, 400, 300);
    let passes = |c: &Core| c.scale_passes.0 + c.scale_passes.1;
    let once = |c: &mut Core, what: &str, f: &mut dyn FnMut(&mut Core)| {
        let before = passes(c);
        f(c);
        assert_eq!(passes(c), before + 1, "{what}");
    };
    focus(&mut c, &mut i, w);
    let fs = ToplevelRequest::SetFullscreen { output: Some(b) };
    once(&mut c, "grant", &mut |c| {
        c.toplevel_request(w, fs).unwrap();
    });
    once(&mut c, "commit of the presented element", &mut |c| {
        c.toplevel_committed(w, win(1, 960, 540, true)).unwrap();
    });
    once(&mut c, "tree change of the presented element", &mut |c| {
        c.toplevel_tree_changed(w, win(1, 960, 540, true)).unwrap();
    });
    once(&mut c, "unset", &mut |c| {
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
    });
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, fs).unwrap();
    once(&mut c, "unmap of the presented element", &mut |c| {
        c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
    });
    let v = c.toplevel_created();
    c.toplevel_request(v, fs).unwrap();
    c.toplevel_committed(v, win(2, 0, 0, false)).unwrap();
    once(&mut c, "first map of a pre-map grant", &mut |c| {
        c.toplevel_committed(v, win(2, 960, 540, true)).unwrap();
    });
    once(&mut c, "destruction of the presented element", &mut |c| {
        c.toplevel_destroyed(v);
    });
}

/// A child surface's commit (`toplevel_tree_changed`) is a client response (SC-E): after a
/// non-client stimulus lowered W's hint, a subsurface that reaches B (scale 2) does not raise
/// it; a root commit that grows onto B does not either.
#[test]
fn client_responses_do_not_reverse_a_lowered_hint() {
    for by_child in [true, false] {
        let (mut c, _i, _, mut t) = Core::new(camera_b(1500, 1.0));
        let a = out(&mut c, &mut t, "A", (1920, 1080), 132);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
        let w = map_win(&mut c, 1, 400, 300);
        assert_eq!(c.preferred_scale_of(w), Some((132, 2)));
        // the non-client stimulus: A's scale falls to 1
        c.output_reconfigured(
            &mut t,
            a,
            OutputDescriptor {
                key: crate::output::tests::key("A"),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120: 120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap();
        assert_eq!(c.preferred_scale_of(w), Some((120, 1)));
        let fx = if by_child {
            let mut tree = win(1, 400, 300, true);
            tree.surfaces.insert(
                0,
                SnapshotSurface {
                    key: key(2),
                    role: SurfaceRole::Subsurface {
                        parent: 1,
                        offset: LocalPoint { x: 1500, y: 0 },
                    },
                    size: LocalSize { w: 400, h: 300 },
                    mapped: true,
                    input: InputRegion::Infinite,
                    frame_requested: false,
                },
            );
            c.toplevel_tree_changed(w, tree).unwrap()
        } else {
            c.toplevel_committed(w, win(1, 2000, 300, true)).unwrap()
        };
        assert!(
            c.membership().observers(w).contains(&b),
            "by child {by_child}"
        );
        assert_eq!(c.compute_rho(w), Some(2.0));
        assert!(hints(&fx, w).is_empty(), "by child {by_child}: {fx:?}");
        assert_eq!(c.preferred_scale_of(w), Some((120, 1)));
    }
}

/// A commit of a presented element that starts or ends no presentation is a client response
/// (SC-E): it must not release the hint SC-E holds for another element. X's hint was lowered
/// by a non-client stimulus (A's scale falls to 1) and then held at 1 although X grew onto B
/// (rho 2); W, presented on C, commits. Evaluated as a
/// non-client stimulus, every frame of a fullscreen client would release every such hold.
#[test]
fn a_commit_of_the_presented_element_releases_no_held_hint() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(1500, 1.0));
    let a = out(&mut c, &mut t, "A", (1920, 1080), 132);
    let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
    let o = out(&mut c, &mut t, "C", (1920, 1080), 120);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(o) })
        .unwrap();
    assert_eq!(c.presentation(o), Some(w));
    let x = map_win(&mut c, 9, 400, 300);
    c.output_reconfigured(
        &mut t,
        a,
        OutputDescriptor {
            key: crate::output::tests::key("A"),
            mode: (1920, 1080),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        },
    )
    .unwrap();
    c.toplevel_committed(x, win(9, 2000, 300, true)).unwrap();
    assert!(c.membership().observers(x).contains(&b));
    assert_eq!(c.compute_rho(x), Some(2.0));
    assert_eq!(c.preferred_scale_of(x), Some((120, 1)), "held by SC-E");
    for _ in 0..2 {
        let fx = c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert!(hints(&fx, x).is_empty(), "{fx:?}");
    }
    assert_eq!(c.preferred_scale_of(x), Some((120, 1)));
}

/// F-SCALE-3: the initial prediction compares the placed rect with each view in one unit
/// (Q16). A's camera is restored 100,000 px away; B (scale 2) looks at the origin; W is placed
/// on A: its initial hint is A's, and the map does not renegotiate it.
#[test]
fn the_initial_hint_of_a_window_far_from_the_origin_is_its_outputs() {
    let persisted = PersistedCameras::from_entries(vec![(
        crate::output::tests::key("A"),
        crate::geom::CanvasPos {
            x: 100_000 << 16,
            y: 0,
        },
        1.0,
    )])
    .unwrap();
    let (mut c, _i, _, mut t) = Core::new(persisted);
    let (a, b) = two_scales(&mut c, &mut t);
    let w = c.toplevel_created();
    let fx = c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
    assert_eq!(hints(&fx, w), vec![(120, 1)], "{fx:?}");
    let fx = c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
    assert_eq!(
        c.membership().observers(w),
        vec![a],
        "B does not see W ({b:?})"
    );
    assert!(
        hints(&fx, w).is_empty(),
        "no renegotiation at the map: {fx:?}"
    );
}

/// Output changes: B's scale changes while it presents W (the hint follows, at zoom 1), and
/// B's removal ends the presentation (the hint returns to A's scale).
#[test]
fn output_changes_move_the_hint_of_a_presented_element() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
    let (_a, b) = two_scales(&mut c, &mut t);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
    let fx = c
        .output_reconfigured(
            &mut t,
            b,
            OutputDescriptor {
                key: crate::output::tests::key("B"),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120: 180,
                refresh_mhz: 60_000,
            },
        )
        .unwrap();
    assert_eq!(c.presentation(b), Some(w));
    assert_eq!(hints(&fx, w), vec![(180, 2)], "{fx:?}");
    let hint = fx
        .iter()
        .position(|e| matches!(e, Effect::PreferredScale { .. }))
        .unwrap();
    let configure = fx
        .iter()
        .position(|e| matches!(e, Effect::ToplevelConfigure(e) if *e == w))
        .expect("the presented element is told its new size");
    assert!(hint < configure, "the hint before the configure: {fx:?}");
    let fx = c.output_disconnected(&mut t, b).unwrap();
    assert!(c.fullscreen().is_empty());
    assert_eq!(hints(&fx, w), vec![(120, 1)], "{fx:?}");
}

/// Unmap drops the presentation (C12.F10) and sends the unmapped element no hint (an
/// unobserved element holds h, INV-SCALE-5); after a windowed remap on A the hint is never
/// below A's requirement, and the next fullscreen on B raises it again.
#[test]
fn unmap_and_remap_of_a_presented_element() {
    let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
    let (a, b) = two_scales(&mut c, &mut t);
    let w = map_win(&mut c, 1, 400, 300);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
    let fx = c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
    assert_eq!(c.presentation(b), None);
    assert!(hints(&fx, w).is_empty(), "{fx:?}");
    c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
    c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
    assert_eq!(c.membership().observers(w), vec![a]);
    assert!(c.preferred_scale_of(w).unwrap().0 >= 120);
    focus(&mut c, &mut i, w);
    c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
        .unwrap();
    assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
}
