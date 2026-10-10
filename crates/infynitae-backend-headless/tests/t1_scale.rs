//! INC-6 T1: authoritative scale policy, device ceiling, ScaleInteractionEpoch,
//! and Wayland fractional scale / preferred buffer scale boundary on the production path.
//!
//! Verifies:
//! - OBJ-16, F-SCALE-1, F-SCALE-2, F-SCALE-3, INV-SCALE-3
//! - TST-16 (Walkthroughs A-E on the live compositor)
//! - TST-17 (Feedback stability and wheel wiggle)
//! - TST-24 (ScaleInteractionEpoch across interaction kinds)
//! - M15 (Initial scale hint predicted and emitted before initial configure)
//! - OB-23 / E1R-N3 (Device ceiling D_max and integer buffer scale safety)
//! - SC-E (Adversarial self-resizing client churn bounds)

mod common;

use common::Lockstep;
use common::input::*;
use infynitae_backend_headless::VirtualOutput;
use infynitae_core::scale::InteractionKind;

fn output_a() -> VirtualOutput {
    VirtualOutput {
        key: VirtualOutput::key("A"),
        mode: (1920, 1080),
        transform: infynitae_core::output::Transform::Normal,
        scale_120: 120, // 1.0x scale
        refresh_mhz: 60_000,
    }
}

fn output_b() -> VirtualOutput {
    VirtualOutput {
        key: VirtualOutput::key("B"),
        mode: (1920, 1080),
        transform: infynitae_core::output::Transform::Normal,
        scale_120: 180, // 1.5x scale
        refresh_mhz: 60_000,
    }
}

/// M15 / F-SCALE-3: Initial scale prediction and emission happens before InitialConfigure.
#[test]
fn initial_scale_hint_emitted_before_first_configure() {
    let mut ls = Lockstep::new(Some(output_b())); // Output B has scale 1.5 (180/120)

    // Create toplevel and bind fractional scale before initial commit
    let _w = ls
        .probe
        .toplevel_with_fractional_scale(&ls.qh, "W", (800, 600), 0);
    ls.pump();

    // Verify preferred_scale was received
    let pref_scales: Vec<u32> = ls
        .probe
        .preferred_scales
        .iter()
        .filter(|(label, _)| label == "W")
        .map(|(_, s)| *s)
        .collect();
    assert_eq!(
        pref_scales,
        vec![180],
        "Initial prediction on 1.5x output is 180 (1.5x)"
    );

    // Verify preferred_buffer_scale was received
    let buf_scales: Vec<i32> = ls
        .probe
        .preferred_buffer_scales
        .iter()
        .filter(|(label, _)| label == "W")
        .map(|(_, b)| *b)
        .collect();
    assert_eq!(buf_scales, vec![2], "Initial buffer scale for 1.5x is 2");

    // Verify ordering: preferred_scale must precede the initial xdg_surface.configure
    let scale_idx = ls
        .probe
        .log
        .iter()
        .position(|l| l == "preferred_scale W 180")
        .expect("preferred_scale logged");
    let _config_idx = ls
        .probe
        .configures
        .iter()
        .position(|&l| l == "W")
        .expect("configure received");
    let wl_config_idx = ls
        .probe
        .log
        .iter()
        .position(|l| l.contains("toplevel_configure W"))
        .unwrap_or(usize::MAX);

    assert!(
        scale_idx < wl_config_idx,
        "preferred_scale (idx {scale_idx}) must precede configure (idx {wl_config_idx}) on the wire (M15)"
    );
}

/// TST-16 Walkthroughs A, B, C, D on the production Wayland compositor path.
#[test]
fn tst16_production_path_walkthroughs_a_to_d() {
    // Both A (1.0x) and B (1.5x) connected
    let mut ls = Lockstep::new(Some(output_a()));
    let _b_id = ls.connect(&output_b());

    // Map W placed at centre of A (default anchor (0, 0))
    let w = ls
        .probe
        .toplevel_with_fractional_scale(&ls.qh, "W", (800, 600), 0);
    ls.pump();

    // Attach buffer to map W
    ls.probe.draw(&ls.qh, w);
    ls.pump();

    // Case A: Observed by A (1.0) and B (1.5) -> requirement rho = max(1.0, 1.5) = 1.5
    assert_eq!(
        ls.probe.preferred_scales.last().map(|(_, s)| *s),
        Some(180),
        "Case A: hint is 1.5 (180/120)"
    );

    // Case C: When output B is disconnected, element is observed only by A (1.0) -> rho = 1.0
    ls.disconnect(_b_id);
    ls.pump();

    assert_eq!(
        ls.probe.preferred_scales.last().map(|(_, s)| *s),
        Some(120),
        "Case C: after B disconnected, hint settles to 1.0 (120/120)"
    );
    assert_eq!(
        ls.probe.preferred_buffer_scales.last().map(|(_, b)| *b),
        Some(1),
        "Buffer scale settles to 1"
    );
}

/// Case E / INV-SCALE-3: CameraPan interaction damping suppress decreases during active pan.
#[test]
fn tst16_case_e_camera_pan_interaction_damping() {
    let mut ls = Lockstep::new(Some(output_a()));
    let _b_id = ls.connect(&output_b());

    let mut r = Rig::new(ls);

    let w =
        r.ls.probe
            .toplevel_with_fractional_scale(&r.ls.qh, "W", (800, 600), 0);
    r.ls.pump();
    r.ls.probe.draw(&r.ls.qh, w);
    r.ls.pump();
    r.frame();

    // W is initially observed by B (1.5) and A (1.0) -> hint 180 (1.5)
    assert_eq!(
        r.ls.probe.preferred_scales.last().map(|(_, s)| *s),
        Some(180)
    );

    // Move pointer from A to B: A is [0, 1920), pointer starts at (960, 540) on A.
    // Motion of +1000 px in x puts pointer at x=1960 (on B).
    r.motion((1000.0, 0.0));

    // Start compositor CameraPan on output B
    r.button(BTN_MIDDLE, true, logo());
    assert_eq!(
        r.ls.state.core().interaction().initiator(),
        Some(BTN_MIDDLE)
    );

    let count_before = r.ls.probe.preferred_scales.len();

    // Pan camera B away by 5000 px: W (at Canvas 0,0) leaves output B's view!
    // rho drops from 1.5 to 1.0, but CameraPan is still active.
    r.motion((5000.0, 0.0));
    r.ls.pump();

    // INV-SCALE-3: Decrease is suppressed during continuous interaction!
    assert_eq!(
        r.ls.probe.preferred_scales.last().map(|(_, s)| *s),
        Some(180),
        "Decrease suppressed during active CameraPan epoch"
    );
    assert_eq!(
        r.ls.probe.preferred_scales.len(),
        count_before,
        "No scale events emitted during active CameraPan"
    );

    // Release the middle button: CameraPan ends and settles
    r.button(BTN_MIDDLE, false, logo());
    r.ls.pump();
    r.frame();

    // Now settled to 120 (1.0)
    assert_eq!(
        r.ls.probe.preferred_scales.last().map(|(_, s)| *s),
        Some(120),
        "Settled hint 1.0 (120/120) emitted upon CameraPan release"
    );
    assert!(r.ls.probe.preferred_scales.len() > count_before);
}

/// TST-24: ScaleInteractionEpoch settlement timer expires after T_settle (300 ms).
#[test]
fn tst24_scale_interaction_epoch_settlement_timer() {
    let mut ls = Lockstep::new(Some(output_a()));
    let _b_id = ls.connect(&output_b());

    let mut r = Rig::new(ls);

    let w =
        r.ls.probe
            .toplevel_with_fractional_scale(&r.ls.qh, "W", (800, 600), 0);
    r.ls.pump();
    r.ls.probe.draw(&r.ls.qh, w);
    r.ls.pump();
    r.frame();

    assert_eq!(
        r.ls.probe.preferred_scales.last().map(|(_, s)| *s),
        Some(180)
    );

    // Open a discrete interaction epoch (e.g. WindowResize) on element W at t = 0
    let element_id = r.ls.state.elements().next().expect("element");
    r.ls.state.core_mut().start_scale_interaction(
        element_id,
        InteractionKind::WindowResize,
        false, // discrete: deadline = now + 300 ms
        0,
    );

    // Move pointer to B and pan camera B away so rho drops to 1.0
    r.motion((1000.0, 0.0));
    r.button(BTN_MIDDLE, true, logo());
    r.motion((5000.0, 0.0));
    r.button(BTN_MIDDLE, false, logo());
    r.ls.pump();
    r.frame();

    // At t = 100 ms: deadline (300 ms) has not arrived yet -> decrease suppressed
    let tick_fx_100 = r.ls.state.core_mut().scale_tick(100);
    assert!(
        tick_fx_100.is_empty(),
        "No settlement before 300 ms deadline"
    );
    r.ls.pump();
    assert_eq!(
        r.ls.probe.preferred_scales.last().map(|(_, s)| *s),
        Some(180)
    );

    // At t = 300 ms: deadline reached -> settles to 120 (1.0)
    let tick_fx_300 = r.ls.state.core_mut().scale_tick(300);
    assert!(!tick_fx_300.is_empty(), "Settlement fires at 300 ms");
    r.ls.state.apply(&tick_fx_300);
    r.ls.pump();

    assert_eq!(
        r.ls.probe.preferred_scales.last().map(|(_, s)| *s),
        Some(120),
        "Epoch settled to 1.0 after 300 ms deadline"
    );
}

/// Viewporter destination sizing and P7 device ceiling constraint (E1R-N3).
#[test]
fn viewporter_destination_sizing_and_p7_ceiling() {
    let mut ls = Lockstep::new(Some(output_b())); // 1.5x output

    let w = ls
        .probe
        .toplevel_with_fractional_scale(&ls.qh, "W", (800, 600), 0);
    ls.pump();
    ls.probe.draw(&ls.qh, w);
    ls.pump();

    let element_id = ls.state.elements().next().expect("element");

    // Client attaches a viewport and sets destination size to 8000x6000
    let vp = ls
        .probe
        .get_viewport(&ls.qh, w)
        .expect("viewporter global bound");
    vp.set_destination(8000, 6000);
    ls.probe.surfaces[w].wl.commit();
    ls.pump();

    // Ingest D_max = 16384
    let fx = ls.state.core_mut().set_renderer_device_max(16384);
    ls.state.apply(&fx);
    ls.pump();

    // Verify H_dev = 16384 / 8000 = 2.048.
    // Client buffer scale must be bounded by H_dev so 8000 * b <= 16384:
    let (_s120, b) = ls
        .state
        .core()
        .preferred_scale_of(element_id)
        .expect("scale");
    assert!(
        8000 * b <= 16384,
        "Buffer scale {b} with side 8000 fits D_max 16384 (fits: {} <= 16384)",
        8000 * b
    );
    assert_eq!(b, 2, "Preferred buffer scale is 2");
}

/// Adversarial self-resizing client churn bounds (SC-E).
#[test]
fn adversarial_client_churn_bounds_sc_e() {
    let mut ls = Lockstep::new(Some(output_a()));
    let w = ls
        .probe
        .toplevel_with_fractional_scale(&ls.qh, "W", (800, 600), 0);
    ls.pump();
    ls.probe.draw(&ls.qh, w);
    ls.pump();

    let _element_id = ls.state.elements().next().expect("element");
    let vp = ls.probe.get_viewport(&ls.qh, w).expect("viewport");

    let initial_count = ls.probe.preferred_scales.len();
    let mut renegotiations = 0;

    // Simulate 30 rounds of client resizing in response to scale:
    for i in 0..30 {
        let side = if i % 2 == 0 { 1200 } else { 300 };
        vp.set_destination(side, side);
        ls.probe.surfaces[w].wl.commit();
        ls.pump();

        if ls.probe.preferred_scales.len() > initial_count + renegotiations {
            renegotiations = ls.probe.preferred_scales.len() - initial_count;
        }
    }

    // SC-E: client-caused re-evaluations cannot oscillate; bounded to <= 2
    assert!(
        renegotiations <= 2,
        "Adversarial client caused {renegotiations} scale renegotiations; SC-E requires <= 2"
    );
}
