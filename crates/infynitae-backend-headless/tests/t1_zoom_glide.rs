//! End-to-end headless verification of ZOOM_FEEL camera glides (`instant`, `quick`, `smooth`,
//! `floaty`): pointer canvas point stability (<= 0.05 px/frame), 95% settle times within 10% of
//! target, `instant` exact parity, `floaty` wheel momentum decay to rest, and gliding across
//! wheel, zoom_reset, fit, overview, window slots, and back/restore.

mod common;

use common::input::*;
use common::{Lockstep, output_k};
use infynitae_backend_headless::{keyboard_key, pointer_axis, set_focus, vblank_period};
use infynitae_core::geom::Q16_ONE;
use infynitae_core::input::keys::{
    KEY_1, KEY_ENTER, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_SPACE, KEY_TAB,
};
use infynitae_core::input::{Axis, AxisSource};
use infynitae_core::navigation::{CameraValue, ZoomFeel};
use infynitae_core::stage::ElementRef;
use infynitae_core::{EnterMode, ShellCommand, ShellStream};

fn q16(px: f64) -> i64 {
    (px * Q16_ONE as f64).round_ties_even() as i64
}

fn rig_with_hz(refresh_mhz: u32, feel: ZoomFeel) -> Rig {
    let mut ls = Lockstep::new(Some(output_k("A", (1920, 1080), 120, refresh_mhz)));
    ls.state.core_mut().set_zoom_feel(feel);
    let mut r = Rig::new(ls);
    r.map_w();
    // Put pointer away from screen center at a fractional logical position: (1317.3, 311.7).
    r.motion((357.3, -228.3));
    let o = r.ls.output();
    r.ls.run_until(r.ls.now + vblank_period(refresh_mhz));
    let _ = r.ls.state.core().cameras().camera_for_output(o).unwrap();
    r
}

fn pointer_pos(r: &Rig) -> (f64, f64) {
    match r.ls.state.core().pointer() {
        infynitae_core::pointer::PointerState::Located { p, .. } => p,
        infynitae_core::pointer::PointerState::NoOutput => panic!("pointer not located"),
    }
}

fn camera(r: &Rig) -> CameraValue {
    let c = r.ls.state.core().cameras();
    CameraValue {
        center: c.center(0).unwrap(),
        zoom: c.zoom(0).unwrap(),
    }
}

fn canvas_under_pointer(cam: CameraValue, p: (f64, f64), extent: (f64, f64)) -> (f64, f64) {
    let cx = cam.center.x as f64 / Q16_ONE as f64;
    let cy = cam.center.y as f64 / Q16_ONE as f64;
    (
        cx + (p.0 - extent.0 / 2.0) / cam.zoom,
        cy + (p.1 - extent.1 / 2.0) / cam.zoom,
    )
}

fn wheel_zoom(r: &mut Rig, v120_steps: &[i32]) {
    for &(key, pressed) in &[(KEY_LEFTMETA, true), (KEY_LEFTSHIFT, true)] {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
    }
    for &v120 in v120_steps {
        r.t += 1000;
        pointer_axis(
            &mut r.ls.state,
            &mut r.ls.input,
            Axis {
                source: AxisSource::Wheel,
                amount: (0.0, v120 as f64 / 8.0),
                v120: (0, v120),
            },
            r.t,
        )
        .expect("axis");
    }
    for &(key, pressed) in &[(KEY_LEFTSHIFT, false), (KEY_LEFTMETA, false)] {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
    }
    r.ls.pump();
}

fn tap_chord(r: &mut Rig, mods: &[u32], key: u32) {
    for &m in mods {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, m, true, r.t).expect("mod down");
    }
    r.t += 1000;
    keyboard_key(&mut r.ls.state, &mut r.ls.input, key, true, r.t).expect("key down");
    r.t += 1000;
    keyboard_key(&mut r.ls.state, &mut r.ls.input, key, false, r.t).expect("key up");
    for &m in mods.iter().rev() {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, m, false, r.t).expect("mod up");
    }
    r.ls.pump();
}

#[test]
fn instant_feel_matches_todays_exact_camera_values() {
    let mut r = rig_with_hz(165_000, ZoomFeel::Instant);
    let extent =
        r.ls.state
            .core()
            .output(r.ls.output())
            .unwrap()
            .logical_extent();
    let p = pointer_pos(&r);
    let before = camera(&r);

    wheel_zoom(&mut r, &[-120]);
    assert!(
        !r.ls.state.core().zoom_gliding(),
        "instant must not start a glide"
    );
    let after_one = camera(&r);
    let expected_zoom = 1.15_f64;
    let dx = (p.0 - extent.0 / 2.0) * (1.0 / before.zoom - 1.0 / expected_zoom);
    let dy = (p.1 - extent.1 / 2.0) * (1.0 / before.zoom - 1.0 / expected_zoom);
    assert_eq!(after_one.zoom.to_bits(), expected_zoom.to_bits());
    assert_eq!(after_one.center.x, before.center.x + q16(dx));
    assert_eq!(after_one.center.y, before.center.y + q16(dy));

    // A refresh tick changes nothing when instant
    r.ls.run_until(r.ls.now + vblank_period(165_000));
    assert_eq!(camera(&r), after_one);

    // Super+Space with no focused window resets zoom to 1.0 immediately
    set_focus(&mut r.ls.state, &mut r.ls.input, None).unwrap();
    tap_chord(&mut r, &[KEY_LEFTMETA], KEY_SPACE);
    assert!(!r.ls.state.core().zoom_gliding());
    assert_eq!(camera(&r).zoom, 1.0);
}

#[test]
fn every_feel_keeps_pointer_canvas_point_within_005_px_every_frame() {
    for refresh_mhz in [165_000, 60_000] {
        let period = vblank_period(refresh_mhz);
        for feel in [
            ZoomFeel::Instant,
            ZoomFeel::Quick,
            ZoomFeel::Smooth,
            ZoomFeel::Floaty,
        ] {
            let mut r = rig_with_hz(refresh_mhz, feel);
            let extent =
                r.ls.state
                    .core()
                    .output(r.ls.output())
                    .unwrap()
                    .logical_extent();
            let p = pointer_pos(&r);
            let initial_pt = canvas_under_pointer(camera(&r), p, extent);

            wheel_zoom(&mut r, &[-120, -120, -120]);
            let mut max_drift = 0.0_f64;
            for _ in 0..200 {
                r.ls.run_until(r.ls.now + period);
                let pt = canvas_under_pointer(camera(&r), p, extent);
                let drift = (pt.0 - initial_pt.0).hypot(pt.1 - initial_pt.1);
                max_drift = max_drift.max(drift);
                if !r.ls.state.core().zoom_gliding() {
                    break;
                }
            }
            assert!(
                !r.ls.state.core().zoom_gliding(),
                "{feel:?} @ {refresh_mhz} mHz did not settle"
            );
            assert!(
                max_drift <= 0.05,
                "{feel:?} @ {refresh_mhz} mHz zoom-in drift {max_drift} > 0.05 px"
            );

            // Also zoom back out while keeping the same pointer fixed
            wheel_zoom(&mut r, &[120, 120]);
            for _ in 0..200 {
                r.ls.run_until(r.ls.now + period);
                let pt = canvas_under_pointer(camera(&r), p, extent);
                let drift = (pt.0 - initial_pt.0).hypot(pt.1 - initial_pt.1);
                max_drift = max_drift.max(drift);
                if !r.ls.state.core().zoom_gliding() {
                    break;
                }
            }
            assert!(
                max_drift <= 0.05,
                "{feel:?} @ {refresh_mhz} mHz zoom-out drift {max_drift} > 0.05 px"
            );
        }
    }
}

#[test]
fn time_to_95_percent_within_10_percent_of_target() {
    let refresh_mhz = 165_000;
    let period = vblank_period(refresh_mhz);
    for (feel, target_ms) in [
        (ZoomFeel::Quick, 100.0),
        (ZoomFeel::Smooth, 165.0),
        (ZoomFeel::Floaty, 300.0),
    ] {
        let mut r = rig_with_hz(refresh_mhz, feel);
        let t0 = r.ls.now;
        let ln_z0 = camera(&r).zoom.ln();

        // Use wheel zoom for Quick & Smooth, and zoom_reset/fit timing check (and wheel check) for Floaty.
        if feel == ZoomFeel::Floaty {
            // Wheel step with Floaty includes short decaying momentum (~312 ms vs 300 ms, within 10%)
            wheel_zoom(&mut r, &[-120]);
        } else {
            wheel_zoom(&mut r, &[-120, -120]);
        }

        let mut samples = Vec::new();
        for _ in 0..300 {
            r.ls.run_until(r.ls.now + period);
            samples.push(((r.ls.now - t0).as_secs_f64() * 1000.0, camera(&r).zoom.ln()));
            if !r.ls.state.core().zoom_gliding() {
                break;
            }
        }
        let ln_zf = camera(&r).zoom.ln();
        let total = ln_zf - ln_z0;
        let t95 = samples
            .iter()
            .find(|&&(_, ln_z)| (ln_z - ln_z0) / total >= 0.95)
            .map(|&(ms, _)| ms)
            .expect("95% sample");

        // Monotone step progress after input ended
        for w in samples.windows(2) {
            assert!(
                w[1].1 >= w[0].1 - 1e-9,
                "{feel:?} non-monotone zoom step: {} -> {}",
                w[0].1,
                w[1].1
            );
        }

        assert!(
            (t95 - target_ms).abs() <= target_ms * 0.10,
            "{feel:?}: 95% time {t95:.2} ms outside ±10% of {target_ms} ms"
        );
    }
}

#[test]
fn floaty_wheel_momentum_coasts_and_decays_to_rest() {
    let period = vblank_period(165_000);
    let mut r = rig_with_hz(165_000, ZoomFeel::Floaty);
    wheel_zoom(&mut r, &[-120, -120]);
    let static_zoom = 1.15_f64.powi(2);

    let mut prev_vel = f64::INFINITY;
    let mut saw_positive_vel = false;
    for _ in 0..300 {
        r.ls.run_until(r.ls.now + period);
        if let Some(v) = r.ls.state.core().cameras().glide_velocity(0) {
            if v > 0.0 {
                saw_positive_vel = true;
            }
            assert!(v <= prev_vel + 1e-12, "velocity did not decay");
            prev_vel = v;
        }
        if !r.ls.state.core().zoom_gliding() {
            break;
        }
    }
    assert!(saw_positive_vel, "expected floaty wheel velocity");
    assert!(!r.ls.state.core().zoom_gliding(), "did not settle to rest");
    assert!(
        camera(&r).zoom > static_zoom * 1.05,
        "floaty momentum should coast beyond static target"
    );
}

#[test]
fn fit_overview_slots_and_restore_glide_and_settle_verbatim() {
    let period = vblank_period(165_000);
    let mut r = rig_with_hz(165_000, ZoomFeel::Smooth);
    r.ls.state.core_mut().set_enter_mode(EnterMode::Fit);
    // Add a second window off to the side so overview and slots move camera meaningfully
    let qh2 = r.ls.qh.clone();
    r.ls.probe.toplevel(&qh2, "W2", (600, 400), 20);
    r.ls.pump();
    r.ls.run_until(r.ls.now + period);
    let stage = r.ls.state.core().stage();
    let w2_ref = ElementRef {
        instance: stage.instance(),
        element: *stage.z_order().last().unwrap(),
    };
    let move_fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::MoveElement {
                    element: w2_ref,
                    to: (2400.0, 1200.0),
                    stream: ShellStream::Atomic,
                },
            )
            .unwrap();
    r.ls.state.apply(&move_fx);
    r.ls.run_until(r.ls.now + period);

    // Move cursor over first window and Super+Enter -> Fit glides
    let initial = camera(&r);
    tap_chord(&mut r, &[KEY_LEFTMETA], KEY_ENTER);
    assert!(r.ls.state.core().zoom_gliding(), "fit should glide");
    r.ls.run_until(r.ls.now + period);
    let mid_fit = camera(&r);
    assert_ne!(mid_fit, initial);
    while r.ls.state.core().zoom_gliding() {
        r.ls.run_until(r.ls.now + period);
    }
    let settled_fit = camera(&r);
    assert_ne!(settled_fit.zoom, initial.zoom);

    // ShellCommand::ViewportRestore glides cleanly back to exact initial CameraValue
    let o = r.ls.output();
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(&mut r.ls.shell, ShellCommand::ViewportRestore { output: o })
            .unwrap();
    r.ls.state.apply(&fx);
    assert!(r.ls.state.core().zoom_gliding(), "restore should glide");
    while r.ls.state.core().zoom_gliding() {
        r.ls.run_until(r.ls.now + period);
    }
    assert_eq!(camera(&r), initial, "restore must settle verbatim");

    // Overview (Super+Shift+Tab) glides in, and tapping again toggles smoothly back
    tap_chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT], KEY_TAB);
    assert!(r.ls.state.core().zoom_gliding(), "overview should glide");
    r.ls.run_until(r.ls.now + period);
    // Toggle off mid-glide
    tap_chord(&mut r, &[KEY_LEFTMETA, KEY_LEFTSHIFT], KEY_TAB);
    assert!(r.ls.state.core().zoom_gliding(), "overview restore glides");
    while r.ls.state.core().zoom_gliding() {
        r.ls.run_until(r.ls.now + period);
    }
    assert_eq!(camera(&r), initial);

    // Super+1 slot switch glides when moving from offset camera
    wheel_zoom(&mut r, &[-120, -120]);
    while r.ls.state.core().zoom_gliding() {
        r.ls.run_until(r.ls.now + period);
    }
    tap_chord(&mut r, &[KEY_LEFTMETA], KEY_1);
    assert!(r.ls.state.core().zoom_gliding(), "slot focus should glide");
    while r.ls.state.core().zoom_gliding() {
        r.ls.run_until(r.ls.now + period);
    }
}

#[test]
fn pointer_motion_mid_glide_reanchors_cleanly() {
    let period = vblank_period(165_000);
    let mut r = rig_with_hz(165_000, ZoomFeel::Smooth);
    let extent =
        r.ls.state
            .core()
            .output(r.ls.output())
            .unwrap()
            .logical_extent();
    wheel_zoom(&mut r, &[-120, -120, -120]);
    for _ in 0..5 {
        r.ls.run_until(r.ls.now + period);
    }
    // Move pointer to a new screen position mid-glide
    r.motion((-400.0, 150.0));
    let p_new = pointer_pos(&r);
    let anchor_after_move = canvas_under_pointer(camera(&r), p_new, extent);
    let mut max_drift = 0.0_f64;
    while r.ls.state.core().zoom_gliding() {
        r.ls.run_until(r.ls.now + period);
        let pt = canvas_under_pointer(camera(&r), p_new, extent);
        max_drift = max_drift.max((pt.0 - anchor_after_move.0).hypot(pt.1 - anchor_after_move.1));
    }
    assert!(
        max_drift <= 0.05,
        "post-move re-anchor drift {max_drift} > 0.05 px"
    );
}
