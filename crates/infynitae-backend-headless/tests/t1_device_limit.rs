//! OB-13: the device ceiling Core enforces is the one the backend's
//! renderer reports, through the renderer-limit interface and by no other route.
//!
//! The renderer here is the headless fixture: it has no device and reports the limit it was
//! given. These tests show the path and the enforcement. They show nothing about what a real
//! GPU reports; that is OB-52.
//!
//! Expected values are derived by hand from F-SCALE-2 as amended by C12.F2: for a logical side
//! `s`, the fractional hint is at most `⌊120·D_max/s⌋/120` (and at most `H_cfg` = 3), and the
//! integer hint `b` satisfies `s·b ≤ D_max` whenever `b = 1` does.

mod common;

use common::{Lockstep, output};
use infynitae_backend_headless::{
    DeviceLimit, HEADLESS_REFERENCE_DEVICE_LIMIT, HeadlessCompositorOptions, HeadlessRenderer,
    run_headless,
};
use infynitae_core::Core;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::scale::ScaleConfigError;
use infynitae_wayland::limits::{DeviceLimitError, LimitQueryError};

const SIDE: i32 = 3000;

/// A 3.0x output showing W, whose longest logical side is 3000 (a viewport destination).
fn wide_window_on_a_3x_output() -> Lockstep {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 360)));
    let qh = ls.qh.clone();
    let w = ls
        .probe
        .toplevel_with_fractional_scale(&qh, "W", (800, 600), 0);
    ls.pump();
    ls.probe.draw(&qh, w);
    ls.pump();
    let vp = ls.probe.get_viewport(&qh, w).expect("wp_viewporter");
    vp.set_destination(SIDE, 600);
    ls.probe.surfaces[w].wl.commit();
    ls.pump();
    ls
}

fn last_hints(ls: &Lockstep) -> (u32, i32) {
    (
        *ls.probe.preferred_scales_of("W").last().expect("a hint"),
        *ls.probe
            .preferred_buffer_scales_of("W")
            .last()
            .expect("an integer hint"),
    )
}

/// The pass criterion of OB-13 for the hints in force: neither asks for a buffer over `d_max`.
fn assert_fits(ls: &Lockstep, d_max: u32) {
    let (s120, b) = last_hints(ls);
    let fractional = (SIDE as u64 * s120 as u64).div_ceil(120);
    assert!(
        fractional <= d_max as u64,
        "fractional hint {s120}/120 asks for {fractional} px on a {d_max} px device"
    );
    assert!(
        SIDE as u64 * b as u64 <= d_max as u64,
        "integer hint {b} asks for {} px on a {d_max} px device",
        SIDE as u64 * b as u64
    );
}

/// The harness starts as the run loop does: the ceiling is the fixture renderer's default.
#[test]
fn the_ceiling_at_startup_is_what_the_renderer_reports() {
    let ls = wide_window_on_a_3x_output();
    assert_eq!(HEADLESS_REFERENCE_DEVICE_LIMIT, 16384, "EC-16");
    assert_eq!(ls.state.core().scale_config().d_max, 16384);
    // ⌊120·16384/3000⌋ = 655 > 360: the output's scale stands; 3000·3 = 9000 ≤ 16384
    assert_eq!(last_hints(&ls), (360, 3));
    assert_fits(&ls, 16384);
}

/// A change of the device's limit takes effect when it is reported, not before, and then at
/// once: the hints drop to what fits the new device.
#[test]
fn a_changed_limit_is_enforced_when_reported() {
    let mut ls = wide_window_on_a_3x_output();
    let mut renderer = HeadlessRenderer::default();

    // (reported limit, ⌊120·D/3000⌋ capped at 360, largest b with 3000·b ≤ D capped at 3)
    for (d_max, want_120, want_b) in [(8192u32, 327u32, 2i32), (4096, 163, 1)] {
        renderer.set_device_limit(DeviceLimit::MaxBufferSide(d_max));
        ls.pump();
        assert_ne!(
            ls.state.core().scale_config().d_max,
            d_max,
            "Core does not learn a limit nobody reported"
        );
        assert_eq!(ls.state.report_renderer_limits(&renderer), Ok(d_max));
        ls.pump();
        assert_eq!(ls.state.core().scale_config().d_max, d_max);
        assert_eq!(last_hints(&ls), (want_120, want_b), "D_max {d_max}");
        assert_fits(&ls, d_max);
    }
    assert_eq!(ls.probe.preferred_scales_of("W"), vec![360, 327, 163]);

    // a larger device: the ceiling rises; whatever the hints become, they fit
    renderer.set_device_limit(DeviceLimit::MaxBufferSide(16384));
    assert_eq!(ls.state.report_renderer_limits(&renderer), Ok(16384));
    ls.pump();
    assert_eq!(ls.state.core().scale_config().d_max, 16384);
    assert_fits(&ls, 16384);
}

/// Limits at and around the point where the window's side stops fitting at scale 1, and the
/// largest value the interface can carry.
#[test]
fn boundary_limits() {
    // (reported limit, fractional hint ·120, integer hint)
    let cases: [(u32, u32, i32); 6] = [
        (u32::MAX, 360, 3), // no device bound below H_cfg
        (9000, 360, 3),     // 120·9000/3000 = 360 exactly; 3000·3 = 9000 exactly
        (8999, 359, 2),     // ⌊359.96⌋; 3000·3 = 9000 > 8999
        (3001, 120, 1),     // ⌊120.04⌋
        (3000, 120, 1),     // the side fits at scale 1 exactly
        (6000, 240, 2),     // 120·6000/3000 = 240 exactly; 3000·2 = 6000 exactly
    ];
    for (d_max, want_120, want_b) in cases {
        let mut ls = wide_window_on_a_3x_output();
        let renderer = HeadlessRenderer::new(DeviceLimit::MaxBufferSide(d_max));
        assert_eq!(ls.state.report_renderer_limits(&renderer), Ok(d_max));
        ls.pump();
        assert_eq!(last_hints(&ls), (want_120, want_b), "D_max {d_max}");
        assert_fits(&ls, d_max);
    }
}

/// A query that fails, and a limit Core refuses, leave the ceiling in force untouched and tell
/// the client nothing.
#[test]
fn a_failed_query_and_a_refused_limit_change_nothing() {
    let mut ls = wide_window_on_a_3x_output();
    let mut renderer = HeadlessRenderer::new(DeviceLimit::MaxBufferSide(8192));
    assert_eq!(ls.state.report_renderer_limits(&renderer), Ok(8192));
    ls.pump();
    let before = (
        ls.probe.preferred_scales_of("W"),
        ls.probe.preferred_buffer_scales_of("W"),
    );
    assert_eq!(before.0, vec![360, 327]);

    renderer.set_device_limit(DeviceLimit::Unavailable("context lost".into()));
    assert_eq!(
        ls.state.report_renderer_limits(&renderer),
        Err(DeviceLimitError::Query(LimitQueryError(
            "context lost".into()
        )))
    );
    renderer.set_device_limit(DeviceLimit::MaxBufferSide(0));
    assert_eq!(
        ls.state.report_renderer_limits(&renderer),
        Err(DeviceLimitError::Refused(ScaleConfigError::DeviceMax))
    );
    ls.pump();
    assert_eq!(ls.state.core().scale_config().d_max, 8192, "kept");
    assert_eq!(
        (
            ls.probe.preferred_scales_of("W"),
            ls.probe.preferred_buffer_scales_of("W"),
        ),
        before,
        "no hint was sent"
    );
    assert_fits(&ls, 8192);
}

fn start_with(limit: DeviceLimit) -> Result<(), Box<dyn std::error::Error>> {
    let (core, input, _shell, token) = Core::new(PersistedCameras::default());
    run_headless(
        core,
        input,
        token,
        HeadlessCompositorOptions {
            // would end the session at once if it ever started
            max_frames: Some(0),
            duration: Some(std::time::Duration::ZERO),
            device_limit: limit,
            ..HeadlessCompositorOptions::default()
        },
    )
}

/// The run loop asks its renderer before it serves anything, with the limit from its options:
/// a session whose renderer cannot name a usable limit does not start.
#[test]
fn the_session_does_not_start_without_a_device_limit() {
    let err = start_with(DeviceLimit::Unavailable("no device".into()))
        .expect_err("a session without a device limit");
    assert_eq!(
        err.downcast_ref::<DeviceLimitError>(),
        Some(&DeviceLimitError::Query(LimitQueryError(
            "no device".into()
        )))
    );
    let err = start_with(DeviceLimit::MaxBufferSide(0)).expect_err("a zero limit");
    assert_eq!(
        err.downcast_ref::<DeviceLimitError>(),
        Some(&DeviceLimitError::Refused(ScaleConfigError::DeviceMax))
    );
}
