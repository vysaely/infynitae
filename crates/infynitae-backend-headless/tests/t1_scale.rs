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

    // Verify ordering on one ordered wire log: both hints precede the initial
    // xdg_surface.configure. A missing line fails the test (it used to make it vacuous: the
    // configure was looked up in a log that never records it).
    let at = |line: &str| {
        ls.probe
            .scale_log
            .iter()
            .position(|l| l == line)
            .unwrap_or_else(|| panic!("`{line}` not on the wire: {:?}", ls.probe.scale_log))
    };
    let configure = at("configure W");
    assert!(
        at("preferred_scale W 180") < configure,
        "preferred_scale must precede the initial configure (M15): {:?}",
        ls.probe.scale_log
    );
    assert!(
        at("preferred_buffer_scale W 2") < configure,
        "preferred_buffer_scale must precede the initial configure (M15): {:?}",
        ls.probe.scale_log
    );
}

/// TST-16 Walkthroughs A, B, C, D on the production Wayland compositor path.
#[test]
fn tst16_production_path_walkthroughs_a_to_d() {
    // Both A (1.0x) and B (1.5x) connected
    let mut ls = Lockstep::new(Some(output_a()));
    let _b_id = ls.connect(&output_b());

    // Map W, centred on A's camera (F-PLACE-1)
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

    // Pan camera B away by 5000 px: W (centred on the Canvas origin) leaves output B's view!
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
    let fx = ls
        .state
        .core_mut()
        .set_renderer_device_max(16384)
        .expect("valid D_max");
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

// ---------------------------------------------------------------------------------------------
// INC-6 hardening (independent audit findings C1, C2; device ceiling at several D_max)
// ---------------------------------------------------------------------------------------------

fn output_scaled(key: &str, scale_120: u32) -> VirtualOutput {
    VirtualOutput {
        key: VirtualOutput::key(key),
        mode: (1920, 1080),
        transform: infynitae_core::output::Transform::Normal,
        scale_120,
        refresh_mhz: 60_000,
    }
}

/// A subsurface of `parent` with its role only; `fractional` binds `wp_fractional_scale_v1`
/// before the surface's first commit, as toolkits do.
fn join_subsurface(
    ls: &mut Lockstep,
    label: &'static str,
    parent: usize,
    fractional: bool,
) -> usize {
    let qh = ls.qh.clone();
    let s = ls
        .probe
        .subsurface_uncommitted(&qh, label, parent, (10, 10), (64, 48), false);
    if fractional {
        ls.probe.get_fractional_scale(&qh, s);
    }
    ls.pump();
    s
}

/// Gives a joined subsurface its buffer; the parent's commit applies position and stacking.
fn map_subsurface(ls: &mut Lockstep, s: usize, parent: usize) {
    let qh = ls.qh.clone();
    ls.probe.draw(&qh, s);
    ls.probe.surfaces[parent].wl.commit();
    ls.pump();
}

/// C1 on the wire: an output scale below 1 is a rung and is sent as `round(h · 120)`, not
/// raised to 120.
#[test]
fn c1_output_scale_below_one_reaches_the_client() {
    let mut ls = Lockstep::new(Some(output_scaled("C", 90))); // 0.75x
    let w = ls
        .probe
        .toplevel_with_fractional_scale(&ls.qh, "W", (800, 600), 0);
    ls.pump();
    assert_eq!(ls.probe.preferred_scales_of("W"), vec![90]);
    ls.probe.draw(&ls.qh, w);
    ls.pump();
    assert_eq!(ls.probe.preferred_scales_of("W"), vec![90]);
    // b(0.75) = 1, the value a surface has before any event: nothing to send
    assert_eq!(ls.probe.preferred_buffer_scales_of("W"), Vec::<i32>::new());
}

/// C2: every surface of the element's tree follows the element's hint — at the hotplug of a
/// differently scaled output, when the element leaves that output's view, and at an output
/// scale change — and a surface that left the tree, lost its `wp_fractional_scale_v1` or was
/// destroyed causes neither a stale event nor a protocol error.
#[test]
fn c2_subsurface_tree_follows_scale_changes() {
    let mut r = Rig::new(Lockstep::new(Some(output_a())));
    let a_id = r.ls.output();
    let w =
        r.ls.probe
            .toplevel_with_fractional_scale(&r.ls.qh, "W", (800, 600), 0);
    r.ls.pump();
    r.ls.probe.draw(&r.ls.qh, w);
    r.ls.pump();

    let s1 = join_subsurface(&mut r.ls, "S1", w, true);
    let s1a = join_subsurface(&mut r.ls, "S1a", s1, true);
    let s2 = join_subsurface(&mut r.ls, "S2", w, false); // integer-scale only
    let s3 = join_subsurface(&mut r.ls, "S3", w, true); // will leave the tree
    let s4 = join_subsurface(&mut r.ls, "S4", w, true); // will lose its fractional-scale object
    for (s, parent) in [(s1, w), (s1a, s1), (s2, w), (s3, w), (s4, w)] {
        map_subsurface(&mut r.ls, s, parent);
    }
    r.frame();

    // initial hints: the element's current value, once per fractional-scale object
    for label in ["W", "S1", "S1a", "S3", "S4"] {
        assert_eq!(r.ls.probe.preferred_scales_of(label), vec![120], "{label}");
    }
    for label in ["W", "S1", "S1a", "S2", "S3", "S4"] {
        assert!(
            r.ls.probe.preferred_buffer_scales_of(label).is_empty(),
            "{label}: integer hint 1 is the protocol default"
        );
    }

    // a 1.5x output starts observing W: the whole tree renegotiates, once
    let b_id = r.ls.connect(&output_b());
    for label in ["W", "S1", "S1a", "S3", "S4"] {
        assert_eq!(
            r.ls.probe.preferred_scales_of(label),
            vec![120, 180],
            "{label} follows the element to 1.5"
        );
    }
    for label in ["W", "S1", "S1a", "S2", "S3", "S4"] {
        assert_eq!(
            r.ls.probe.preferred_buffer_scales_of(label),
            vec![2],
            "{label}"
        );
    }

    // W leaves the 1.5x output's view (its camera pans away): settles to 1.0 on release
    r.motion((1000.0, 0.0));
    r.button(BTN_MIDDLE, true, logo());
    r.motion((5000.0, 0.0));
    r.ls.pump();
    assert_eq!(
        r.ls.probe.preferred_scales_of("S1"),
        vec![120, 180],
        "INV-SCALE-3: no decrease reaches a subsurface during the epoch either"
    );
    r.button(BTN_MIDDLE, false, logo());
    r.ls.pump();
    r.frame();
    for label in ["W", "S1", "S1a", "S3", "S4"] {
        assert_eq!(
            r.ls.probe.preferred_scales_of(label),
            vec![120, 180, 120],
            "{label} settles with the element"
        );
    }
    for label in ["W", "S1", "S1a", "S2", "S3", "S4"] {
        assert_eq!(
            r.ls.probe.preferred_buffer_scales_of(label),
            vec![2, 1],
            "{label}"
        );
    }

    // lifecycle: S1 unmaps (stays in the tree), S2 is destroyed, S3 gives up its role (leaves
    // the tree, surface and wp_fractional_scale_v1 alive), S4 destroys its wp_fractional_scale_v1
    r.ls.probe.unmap(s1);
    {
        let s = &mut r.ls.probe.surfaces[s2];
        s.subsurface.take().expect("S2 role").destroy();
        s.wl.destroy();
    }
    r.ls.probe.surfaces[s3]
        .subsurface
        .take()
        .expect("S3 role")
        .destroy();
    r.ls.probe.destroy_fractional_scale(s4);
    r.ls.probe.surfaces[w].wl.commit();
    r.ls.pump();

    // the remaining output changes scale 1.0 -> 2.0
    r.ls.reconfigure(a_id, &output_scaled("A", 240));
    r.frame();
    for label in ["W", "S1", "S1a"] {
        assert_eq!(
            r.ls.probe.preferred_scales_of(label),
            vec![120, 180, 120, 240],
            "{label}: in the tree (S1 unmapped, S1a under it)"
        );
        assert_eq!(
            r.ls.probe.preferred_buffer_scales_of(label),
            vec![2, 1, 2],
            "{label}"
        );
    }
    assert_eq!(
        r.ls.probe.preferred_scales_of("S3"),
        vec![120, 180, 120],
        "S3 left the tree: no hint of an element it no longer belongs to"
    );
    assert_eq!(r.ls.probe.preferred_buffer_scales_of("S3"), vec![2, 1]);
    assert_eq!(
        r.ls.probe.preferred_scales_of("S4"),
        vec![120, 180, 120],
        "S4 has no wp_fractional_scale_v1 any more"
    );
    assert_eq!(
        r.ls.probe.preferred_buffer_scales_of("S4"),
        vec![2, 1, 2],
        "S4 is still in the tree: its wl_surface hint follows"
    );
    assert_eq!(r.ls.probe.preferred_buffer_scales_of("S2"), vec![2, 1]);

    // S1 remaps at the scale it was told while unmapped: nothing further to say
    map_subsurface(&mut r.ls, s1, w);
    r.frame();
    assert_eq!(
        r.ls.probe.preferred_scales_of("S1"),
        vec![120, 180, 120, 240]
    );
    assert!(
        r.ls.conn.protocol_error().is_none(),
        "no protocol error across the subsurface lifecycle"
    );
    let _ = b_id;
}

/// C2: a surface that joins a tree after the element's hint was last sent (new subsurface,
/// popup, late `wp_fractional_scale_v1`) is told the current hint of ITS element — before its
/// first commit and before a popup's initial configure — and never another element's.
#[test]
fn c2_late_surfaces_learn_their_own_elements_hint() {
    let mut ls = Lockstep::new(Some(output_b())); // 1.5x
    let fx = ls
        .state
        .core_mut()
        .set_renderer_device_max(4096)
        .expect("valid D_max");
    ls.state.apply(&fx);
    let qh = ls.qh.clone();

    let w = ls
        .probe
        .toplevel_with_fractional_scale(&qh, "W", (800, 600), 0);
    let x = ls
        .probe
        .toplevel_with_fractional_scale(&qh, "X", (300, 200), 0);
    ls.pump();
    ls.probe.draw(&qh, w);
    ls.probe.draw(&qh, x);
    ls.pump();
    // X stretches to a logical side of 5000 > D_max: H_max(X) = 4096 / 5000 = 0.8192 (P7)
    let vp = ls.probe.get_viewport(&qh, x).expect("wp_viewporter");
    vp.set_destination(5000, 100);
    ls.probe.surfaces[x].wl.commit();
    ls.pump();
    assert_eq!(ls.probe.preferred_scales_of("W"), vec![180]);
    assert_eq!(ls.probe.preferred_buffer_scales_of("W"), vec![2]);
    assert_eq!(ls.probe.preferred_scales_of("X"), vec![180, 98]);
    assert_eq!(ls.probe.preferred_buffer_scales_of("X"), vec![2, 1]);

    // integer-scale subsurface of W: told at creation, before any commit of its own
    let ws = join_subsurface(&mut ls, "WS", w, false);
    assert_eq!(ls.probe.preferred_buffer_scales_of("WS"), vec![2]);
    // fractional-scale subsurfaces: each gets its own element's hint
    join_subsurface(&mut ls, "WF", w, true);
    assert_eq!(ls.probe.preferred_scales_of("WF"), vec![180]);
    assert_eq!(ls.probe.preferred_buffer_scales_of("WF"), vec![2]);
    join_subsurface(&mut ls, "XF", x, true);
    assert_eq!(
        ls.probe.preferred_scales_of("XF"),
        vec![98],
        "X's hint, not W's"
    );
    assert!(ls.probe.preferred_buffer_scales_of("XF").is_empty());

    // a subtree built before it belongs to any element: Y under a role-less X hears nothing;
    // when X becomes a subsurface of W, both are told W's hint
    let x_sub = ls.probe.bare_surface(&qh, "SX", (64, 48));
    let y = join_subsurface(&mut ls, "SY", x_sub, true);
    assert!(ls.probe.preferred_scales_of("SY").is_empty());
    assert!(ls.probe.preferred_buffer_scales_of("SY").is_empty());
    ls.probe.make_subsurface(&qh, x_sub, w, (20, 20), false);
    ls.pump();
    assert_eq!(ls.probe.preferred_scales_of("SY"), vec![180]);
    assert_eq!(ls.probe.preferred_buffer_scales_of("SY"), vec![2]);
    assert_eq!(ls.probe.preferred_buffer_scales_of("SX"), vec![2]);
    let _ = y;

    // a wp_fractional_scale_v1 bound long after the surface joined and committed
    map_subsurface(&mut ls, ws, w);
    ls.probe.get_fractional_scale(&qh, ws);
    ls.pump();
    assert_eq!(ls.probe.preferred_scales_of("WS"), vec![180]);

    // popup of W: the integer hint precedes its initial configure
    let p = ls.probe.popup(
        &qh,
        "P",
        w,
        common::PopupSpec {
            content: (120, 80),
            shadow: 0,
            anchor_rect: (10, 10, 20, 20),
            slide: false,
        },
    );
    ls.probe.get_fractional_scale(&qh, p);
    ls.pump();
    let at = |line: &str| {
        ls.probe
            .scale_log
            .iter()
            .position(|l| l == line)
            .unwrap_or_else(|| panic!("`{line}` not on the wire: {:?}", ls.probe.scale_log))
    };
    assert!(
        at("preferred_buffer_scale P 2") < at("configure P"),
        "a popup learns the scale before its initial configure"
    );
    assert_eq!(ls.probe.preferred_scales_of("P"), vec![180]);

    // nothing leaked across elements, and nobody was told twice
    for (label, want) in [("W", vec![180]), ("X", vec![180, 98]), ("WF", vec![180])] {
        assert_eq!(ls.probe.preferred_scales_of(label), want, "{label}");
    }
    assert!(ls.conn.protocol_error().is_none());
}

/// C2: popup trees are part of the element's tree (the snapshot Core receives contains them):
/// a popup, a popup's subsurface and a nested popup follow a later hint change.
#[test]
fn c2_popup_trees_follow_scale_changes() {
    let mut ls = Lockstep::new(Some(output_b())); // 1.5x
    let b_id = ls.output();
    let qh = ls.qh.clone();
    let w = ls
        .probe
        .toplevel_with_fractional_scale(&qh, "W", (800, 600), 0);
    ls.pump();
    ls.probe.draw(&qh, w);
    ls.pump();
    let spec = |at: i32| common::PopupSpec {
        content: (120, 80),
        shadow: 0,
        anchor_rect: (at, at, 20, 20),
        slide: false,
    };
    let p = ls.probe.popup(&qh, "P", w, spec(10));
    ls.probe.get_fractional_scale(&qh, p);
    ls.pump();
    assert!(
        ls.probe.surfaces[p].configured,
        "P drew on its configure: a popup's parent must be mapped"
    );
    let ps = join_subsurface(&mut ls, "PS", p, true);
    map_subsurface(&mut ls, ps, p);
    let p2 = ls.probe.popup(&qh, "P2", p, spec(5));
    ls.probe.get_fractional_scale(&qh, p2);
    ls.pump();
    let tree = ["W", "P", "PS", "P2"];
    for label in tree {
        assert_eq!(ls.probe.preferred_scales_of(label), vec![180], "{label}");
        assert_eq!(ls.probe.preferred_buffer_scales_of(label), vec![2]);
    }

    ls.reconfigure(b_id, &output_scaled("B", 360)); // 1.5x -> 3.0x
    for label in tree {
        assert_eq!(
            ls.probe.preferred_scales_of(label),
            vec![180, 360],
            "{label} follows the element"
        );
        assert_eq!(ls.probe.preferred_buffer_scales_of(label), vec![2, 3]);
    }
    assert!(ls.conn.protocol_error().is_none());
}

/// E1R-N3 on a `D_max` change: `b(h)` depends on `H_dev(E)`, so a lower device limit can lower
/// the integer hint while h itself still fits (no P7). The integer-scale buffer must fit at once.
#[test]
fn integer_hint_follows_a_device_ceiling_change() {
    let mut ls = Lockstep::new(Some(output_scaled("Q", 300))); // 2.5x
    let qh = ls.qh.clone();
    let w = ls
        .probe
        .toplevel_with_fractional_scale(&qh, "W", (800, 600), 0);
    ls.pump();
    ls.probe.draw(&qh, w);
    ls.pump();
    let vp = ls.probe.get_viewport(&qh, w).expect("wp_viewporter");
    vp.set_destination(5000, 600);
    ls.probe.surfaces[w].wl.commit();
    ls.pump();
    assert_eq!(ls.probe.preferred_scales_of("W"), vec![300]);
    assert_eq!(
        ls.probe.preferred_buffer_scales_of("W"),
        vec![3],
        "5000 × 3 = 15000 fits 16384"
    );

    // H_dev = 13000 / 5000 = 2.6 >= h = 2.5: h holds, but ceil(h) = 3 no longer fits
    let fx = ls
        .state
        .core_mut()
        .set_renderer_device_max(13000)
        .expect("valid D_max");
    ls.state.apply(&fx);
    ls.pump();
    assert_eq!(
        ls.probe.preferred_scales_of("W"),
        vec![300],
        "h is unchanged"
    );
    assert_eq!(
        ls.probe.preferred_buffer_scales_of("W"),
        vec![3, 2],
        "5000 × 2 = 10000 fits 13000"
    );
}

/// INV-SCALE-2 / P7 at several device ceilings: when a surface grows so that `h > H_max(E)`,
/// the hint drops at once to `H_max(E) = D_max / s_max(E)`, exactly once, and the integer hint
/// `b(h)` keeps an integer-scale buffer within `D_max` (E1R-N3). Values are F-SCALE-3's
/// `round(h · 120)`.
///
/// The last non-client stimulus is an INCREASE (a 3.0x output starts observing W), so the
/// client's growth is a client-caused re-evaluation against that direction: SC-E alone would
/// hold h (INV-SCALE-4 d). Only P7 — correctness, not damping — may lower it.
#[test]
fn p7_forced_decrease_at_device_ceilings() {
    // (D_max, logical side, preferred_scale after P7, b(h))
    let cases: [(u32, i32, u32, i32); 4] = [
        (4096, 3000, 163, 1),  // H_max = ⌊163.84⌋/120 (H-1, B)
        (8192, 3000, 327, 2),  // H_max = ⌊327.68⌋/120
        (16384, 8000, 245, 2), // H_max = ⌊245.76⌋/120
        (16384, 3000, 360, 3), // H_dev = 5.46 > H_cfg: no decrease
    ];
    for (d_max, side, want_120, want_b) in cases {
        let mut ls = Lockstep::new(Some(output_a())); // 1.0x
        let fx = ls
            .state
            .core_mut()
            .set_renderer_device_max(d_max)
            .expect("valid D_max");
        ls.state.apply(&fx);
        let qh = ls.qh.clone();
        let w = ls
            .probe
            .toplevel_with_fractional_scale(&qh, "W", (800, 600), 0);
        ls.pump();
        ls.probe.draw(&qh, w);
        ls.pump();
        ls.connect(&output_scaled("H", 360)); // 3.0x = H_cfg: the stimulus, upwards
        assert_eq!(
            ls.probe.preferred_scales_of("W"),
            vec![120, 360],
            "D_max {d_max}"
        );
        assert_eq!(ls.probe.preferred_buffer_scales_of("W"), vec![3]);

        let vp = ls.probe.get_viewport(&qh, w).expect("wp_viewporter");
        vp.set_destination(side, 600);
        ls.probe.surfaces[w].wl.commit();
        ls.pump();

        let element = ls.state.elements().next().expect("element");
        let h = ls.state.core().scale_state(element).expect("OBJ-16").h;
        let h_max = ((120 * d_max as u64 / side as u64) as f64 / 120.0).min(3.0);
        assert!(
            (h - h_max).abs() < 1e-9,
            "D_max {d_max}: h {h} = H_max {h_max}"
        );
        let mut want_scales = vec![120, 360];
        let mut want_buffers = vec![3];
        if want_120 != 360 {
            want_scales.push(want_120);
            want_buffers.push(want_b);
        }
        assert_eq!(
            ls.probe.preferred_scales_of("W"),
            want_scales,
            "D_max {d_max}, side {side}: one forced decrease"
        );
        assert_eq!(ls.probe.preferred_buffer_scales_of("W"), want_buffers);
        let b = *ls
            .probe
            .preferred_buffer_scales_of("W")
            .last()
            .expect("an integer hint was sent");
        assert!(
            side as i64 * b as i64 <= d_max as i64,
            "integer-scale buffer {side} × {b} fits D_max {d_max}"
        );
    }
}

/// M7 `Open | h > H_max(E) → Open{down}`: P7 applies immediately inside an open
/// ScaleInteractionEpoch, where every ordinary decrease is suppressed, and the epoch's
/// settlement does not undo it.
#[test]
fn p7_applies_inside_an_open_epoch() {
    let mut r = Rig::new(Lockstep::new(Some(output_scaled("H", 360)))); // 3.0x, D_max 16384
    let qh = r.ls.qh.clone();
    let w =
        r.ls.probe
            .toplevel_with_fractional_scale(&qh, "W", (800, 600), 0);
    r.ls.pump();
    r.ls.probe.draw(&qh, w);
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.probe.preferred_scales_of("W"), vec![360]);

    r.button(BTN_MIDDLE, true, logo());
    r.motion((8.0, 0.0)); // W stays in view: ρ is unchanged
    r.ls.pump();
    let element = r.ls.state.elements().next().expect("element");
    assert!(
        r.ls.state
            .core()
            .scale_state(element)
            .expect("OBJ-16")
            .epoch
            .open,
        "the CameraPan epoch is open"
    );

    let vp = r.ls.probe.get_viewport(&qh, w).expect("wp_viewporter");
    vp.set_destination(8000, 600); // H_max = ⌊120·16384/8000⌋/120 = 245/120 (H-1, B)
    r.ls.probe.surfaces[w].wl.commit();
    r.ls.pump();
    assert_eq!(
        r.ls.probe.preferred_scales_of("W"),
        vec![360, 245],
        "forced decrease while the pan is still held"
    );
    assert_eq!(r.ls.probe.preferred_buffer_scales_of("W"), vec![3, 2]);
    assert_eq!(
        r.ls.state.core().interaction().initiator(),
        Some(BTN_MIDDLE)
    );

    r.button(BTN_MIDDLE, false, logo());
    r.ls.pump();
    r.frame();
    assert_eq!(
        r.ls.probe.preferred_scales_of("W"),
        vec![360, 245],
        "settlement keeps the device-bound hint"
    );
}

/// INV-SCALE-2: P7 does not wait for an observer. W is mapped but in no output's view (ρ is
/// undefined, the hint is held); growing past the device bound still lowers the hint at once —
/// by a commit of the root, and by a commit of a subsurface.
#[test]
fn p7_applies_to_an_unobserved_element() {
    for grow_root in [true, false] {
        let mut r = Rig::new(Lockstep::new(Some(output_scaled("H", 360)))); // 3.0x
        let qh = r.ls.qh.clone();
        let w =
            r.ls.probe
                .toplevel_with_fractional_scale(&qh, "W", (800, 600), 0);
        r.ls.pump();
        r.ls.probe.draw(&qh, w);
        r.ls.pump();
        let s = join_subsurface(&mut r.ls, "S", w, true);
        map_subsurface(&mut r.ls, s, w);
        r.frame();

        // pan W out of the only view
        r.button(BTN_MIDDLE, true, logo());
        r.motion((5000.0, 0.0));
        r.button(BTN_MIDDLE, false, logo());
        r.ls.pump();
        r.frame();
        let element = r.ls.state.elements().next().expect("element");
        assert_eq!(
            r.ls.state.core().compute_rho(element),
            None,
            "W is unobserved"
        );
        assert_eq!(r.ls.probe.preferred_scales_of("W"), vec![360], "held");

        let grown = if grow_root { w } else { s };
        let vp = r.ls.probe.get_viewport(&qh, grown).expect("wp_viewporter");
        vp.set_destination(8000, 48); // H_max = ⌊120·16384/8000⌋/120 = 245/120 (H-1, B)
        r.ls.probe.surfaces[grown].wl.commit();
        r.ls.pump();
        for label in ["W", "S"] {
            assert_eq!(
                r.ls.probe.preferred_scales_of(label),
                vec![360, 245],
                "{label}, grown by the {}",
                if grow_root { "root" } else { "subsurface" }
            );
            assert_eq!(r.ls.probe.preferred_buffer_scales_of(label), vec![3, 2]);
        }
    }
}

/// OBJ-29 / M-renderer: a lower `D_max` reported later (renderer recreation) forces the
/// decrease immediately; an invalid one is refused and changes nothing.
#[test]
fn device_ceiling_lowered_at_runtime_and_invalid_value_refused() {
    let mut ls = Lockstep::new(Some(output_scaled("H", 360)));
    let qh = ls.qh.clone();
    let w = ls
        .probe
        .toplevel_with_fractional_scale(&qh, "W", (800, 600), 0);
    ls.pump();
    ls.probe.draw(&qh, w);
    ls.pump();
    let vp = ls.probe.get_viewport(&qh, w).expect("wp_viewporter");
    vp.set_destination(3000, 600);
    ls.probe.surfaces[w].wl.commit();
    ls.pump();
    assert_eq!(
        ls.state.core().scale_config().d_max,
        16384,
        "the default ceiling is the EC-16 fixture"
    );
    assert_eq!(ls.probe.preferred_scales_of("W"), vec![360]);

    for (d_max, want_120, want_b) in [(8192u32, 327u32, 2i32), (4096, 163, 1)] {
        let fx = ls
            .state
            .core_mut()
            .set_renderer_device_max(d_max)
            .expect("valid D_max");
        ls.state.apply(&fx);
        ls.pump();
        assert_eq!(
            ls.probe.preferred_scales_of("W").last(),
            Some(&want_120),
            "D_max {d_max}"
        );
        assert_eq!(
            ls.probe.preferred_buffer_scales_of("W").last(),
            Some(&want_b)
        );
    }
    assert_eq!(ls.probe.preferred_scales_of("W"), vec![360, 327, 163]);

    assert_eq!(
        ls.state.core_mut().set_renderer_device_max(0),
        Err(infynitae_core::scale::ScaleConfigError::DeviceMax)
    );
    assert_eq!(ls.state.core().scale_config().d_max, 4096, "kept");
    ls.pump();
    assert_eq!(ls.probe.preferred_scales_of("W"), vec![360, 327, 163]);
}
