//! Production path integration tests: Pointer constraints, batch reconciliation, confinement,
//! zero-output transitions, and output removal under interaction.
//!
//! Covers obligations:
//! - OB-31: xlock_grab port (TST-21) — persistent and oneshot locks with DEFER strategy;
//! - OB-47: Constraint-lifecycle regression (EE-2 / E1..E11);
//! - OB-50: Zero-output constraint lifecycle (EE-3);
//! - OB-51: W-SM-16 popup preemption under lock;
//! - OB-20: TST-05 x8 C3 (pan with lock) and C4 (pan with confinement);
//! - OB-3:  Output removal mid-interaction with active constraint.

mod common;

use common::input::*;
use common::{Lockstep, output_k};
use infynitae_backend_headless::VirtualOutput;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::pointer::{ConstraintKind, PointerState};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface as ServerSurface;
use smithay::wayland::pointer_constraints::with_pointer_constraint;

fn helper_server_surface(r: &Rig, idx: usize) -> ServerSurface {
    let elem_id = r.ls.state.elements().nth(idx).expect("element");
    r.ls.state
        .window(elem_id)
        .expect("window")
        .toplevel()
        .expect("toplevel")
        .wl_surface()
        .clone()
}

fn helper_smithay_active(r: &Rig, idx: usize) -> bool {
    let root = helper_server_surface(r, idx);
    let ptr = r.ls.state.seat().get_pointer().expect("seat pointer");
    with_pointer_constraint(&root, &ptr, |c| c.map(|c| c.is_active()).unwrap_or(false))
}

fn focus_element(r: &mut Rig, idx: usize) {
    let elem_id = r.ls.state.elements().nth(idx).expect("element");
    infynitae_backend_headless::set_focus(&mut r.ls.state, &mut r.ls.input, Some(elem_id)).unwrap();
    r.ls.pump();
    r.frame();
}

// ==============================================================================
// OB-31: Port of xlock_grab (TST-21) — Lock + Client Implicit Grab + Output Loss
// ==============================================================================

#[test]
fn ob31_xlock_grab_persistent_defer() {
    run_xlock_grab_test(false);
}

#[test]
fn ob31_xlock_grab_oneshot_defer() {
    run_xlock_grab_test(true);
}

fn run_xlock_grab_test(oneshot: bool) {
    // Setup two outputs: A (interaction output) and B (survivor)
    let entries = vec![
        (VirtualOutput::key("A"), pos(0, 0), 1.0),
        (VirtualOutput::key("B"), pos(2000, 0), 1.0),
    ];
    let ls = Lockstep::with(
        PersistedCameras::from_entries(entries).unwrap(),
        &[a_1080(), output_k("B", (1920, 1080), 120, 60_000)],
    );
    let mut r = Rig::new(ls);
    r.map_w();
    // Move pointer onto W: pointer focus W
    r.motion((200.0, 200.0));
    r.motion((10.0, 0.0));
    focus_element(&mut r, 0);
    let qh = r.ls.qh.clone();

    // Lock pointer requested by client
    r.ls.probe.lock_pointer(&qh, 0, oneshot);
    r.ls.pump();
    r.frame();

    assert!(
        helper_smithay_active(&r, 0),
        "Smithay constraint must be active after lock request"
    );
    assert!(
        r.ls.state.core().constraint().is_some(),
        "Core constraint must be active"
    );
    assert_eq!(
        r.ls.state.core().constraint().unwrap().kind,
        ConstraintKind::Lock
    );

    // Client presses BTN_LEFT: client-owned implicit click grab on W
    r.button(BTN_LEFT, true, none());
    assert!(
        helper_smithay_active(&r, 0),
        "Lock remains active while client-owned button is held"
    );

    let m_disconnect = r.mark();
    let a_id = r.ls.output();

    // Remove interaction output A while client button is held
    r.ls.disconnect(a_id);
    r.ls.pump();
    r.frame();

    // DEFER strategy (INV-INPUT-5): while client grab is active, reconciliation is strictly DEFERRED.
    // Pointer is relocated in Core to B, but Core constraint state and Smithay remain active.
    assert!(
        helper_smithay_active(&r, 0),
        "Smithay lock must remain active during deferred client grab"
    );
    assert!(
        r.ls.state.core().constraint().is_some(),
        "Core lock must remain active during deferred client grab"
    );

    // No absolute motion delivered to W while locked
    let intermediate_events = r.since(m_disconnect);
    assert!(
        !intermediate_events.iter().any(|s| s.starts_with("motion")),
        "Zero absolute motion must reach client while locked during output removal"
    );

    // Client releases BTN_LEFT: implicit grab ends
    r.button(BTN_LEFT, false, none());
    r.ls.pump();
    r.frame();

    // End of client sequence -> batch-end reconciliation evaluates settled state.
    // Lock point on W is at Canvas (-20, -20) which is NOT observable through output B (at 2000, 0).
    // Therefore, constraint ends cleanly with exactly ONE unlocked event.
    let post_events = r.since(m_disconnect);
    println!("post_events: {post_events:#?}");
    let unlocked_count = post_events.iter().filter(|s| **s == "unlocked").count();
    assert_eq!(
        unlocked_count, 1,
        "Exactly one 'unlocked' event must be emitted at reconciliation"
    );

    assert!(
        !helper_smithay_active(&r, 0),
        "Smithay lock must be inactive after reconciliation"
    );
    assert!(
        r.ls.state.core().constraint().is_none(),
        "Core lock must be inactive after reconciliation"
    );
}

// ==============================================================================
// OB-20: TST-05 x8 C3 (Pan with Lock) & C4 (Pan with Confinement)
// ==============================================================================

#[test]
fn ob20_x8_c3_pan_with_lock() {
    let mut r = Rig::x8();
    focus_element(&mut r, 0);
    let qh = r.ls.qh.clone();

    // Lock pointer on W
    r.ls.probe.lock_pointer(&qh, 0, false);
    r.ls.pump();
    r.frame();

    assert!(helper_smithay_active(&r, 0));
    assert!(r.ls.state.core().constraint().is_some());

    let m_pan = r.mark();

    // Start compositor pan: Logo + BTN_MIDDLE
    r.button(BTN_MIDDLE, true, logo());
    assert_eq!(
        r.ls.state.core().interaction().initiator(),
        Some(BTN_MIDDLE)
    );

    // Pan moves camera across several steps
    for _ in 0..10 {
        r.motion((7.3, -2.1));
    }

    // Zero client motion/relative/button while pan owns the pointer (INV-INPUT-3 KeepFocus, X-8 C3)
    let pan_events = r.since(m_pan);
    assert!(
        pan_events.is_empty(),
        "Zero client events during pan (motion=0 relative=0 button=0): {pan_events:?}"
    );

    // Release middle button to end pan
    r.button(BTN_MIDDLE, false, logo());
    r.ls.pump();
    r.frame();

    // Lock point is still observable on interaction output: lock is kept!
    assert!(helper_smithay_active(&r, 0), "Lock must be kept after pan");
    assert!(r.ls.state.core().constraint().is_some());

    // First genuine motion after pan termination reports relative motion only, exactly the delta (INV-4)
    let m_post = r.mark();
    r.motion((1.0, 0.0));
    let post_events = r.since(m_post);
    assert!(
        !post_events.iter().any(|s| s.starts_with("motion")),
        "Zero absolute motion events while locked after pan"
    );
    assert!(
        post_events.iter().any(|s| s.starts_with("relative")),
        "Genuine relative motion must be delivered after locked pan"
    );
}

#[test]
fn ob20_x8_c4_pan_with_confinement() {
    let mut r = Rig::x8();
    focus_element(&mut r, 0);
    let qh = r.ls.qh.clone();

    // Confine pointer on W
    r.ls.probe.confine_pointer(&qh, 0);
    r.ls.pump();
    r.frame();

    assert!(helper_smithay_active(&r, 0));
    assert_eq!(
        r.ls.state.core().constraint().unwrap().kind,
        ConstraintKind::Confine
    );

    // Pan during confinement
    r.button(BTN_MIDDLE, true, logo());
    for _ in 0..5 {
        r.motion((5.0, 5.0));
    }
    r.button(BTN_MIDDLE, false, logo());
    r.ls.pump();
    r.frame();

    // Confinement maintained if pointer remains within region
    assert!(helper_smithay_active(&r, 0));
}

// ==============================================================================
// OB-50: Port of xzero (EE-3) — Zero-Output Constraint Lifecycle
// ==============================================================================

#[test]
fn ob50_zero_output_persistent_lock() {
    run_xzero_lifecycle_test(ConstraintKind::Lock, false, false);
}

#[test]
fn ob50_zero_output_oneshot_lock() {
    run_xzero_lifecycle_test(ConstraintKind::Lock, true, false);
}

#[test]
fn ob50_zero_output_confinement() {
    run_xzero_lifecycle_test(ConstraintKind::Confine, false, false);
}

#[test]
fn ob50_zero_output_with_held_button() {
    run_xzero_lifecycle_test(ConstraintKind::Lock, false, true);
}

fn run_xzero_lifecycle_test(kind: ConstraintKind, oneshot: bool, held_button: bool) {
    let mut r = Rig::x8();
    focus_element(&mut r, 0);
    let qh = r.ls.qh.clone();

    match kind {
        ConstraintKind::Lock => r.ls.probe.lock_pointer(&qh, 0, oneshot),
        ConstraintKind::Confine => r.ls.probe.confine_pointer(&qh, 0),
    }
    r.ls.pump();
    r.frame();

    assert!(helper_smithay_active(&r, 0));

    if held_button {
        r.button(BTN_LEFT, true, none());
    }

    let o = r.ls.output();
    let m_disconnect = r.mark();

    // LAST output removed -> transition to NoOutput
    r.ls.disconnect(o);
    r.ls.pump();
    r.frame();

    assert_eq!(r.ls.state.core().pointer(), PointerState::NoOutput);
    assert!(
        !helper_smithay_active(&r, 0),
        "Constraint must be deactivated on last output removal"
    );

    let removal_events = r.since(m_disconnect);
    let end_count = removal_events
        .iter()
        .filter(|s| **s == "unlocked" || **s == "unconfined")
        .count();
    assert_eq!(
        end_count, 1,
        "Exactly ONE deactivation event emitted at zero-output transition"
    );
    assert!(
        removal_events.iter().any(|s| s.starts_with("leave")),
        "Leave event must be emitted at zero-output transition"
    );

    // Stable zero-output state: physical motions and clicks are consumed, nothing leaked
    let m_zero = r.mark();
    r.motion((10.0, 10.0));
    if held_button {
        r.button(BTN_LEFT, false, none());
    }
    assert!(
        r.since(m_zero).is_empty(),
        "Zero-output state must consume all physical input"
    );

    // Connect new output: startup placement at centre
    let m_reconnect = r.mark();
    r.ls.connect(&a_1080());
    r.ls.pump();
    r.frame();

    let reconnect_events = r.since(m_reconnect);
    assert!(
        reconnect_events.iter().any(|s| s.starts_with("enter")),
        "Pointer enters on reconnect"
    );

    if oneshot {
        // Oneshot constraint was destroyed on deactivation: must NOT reactivate
        assert!(
            !helper_smithay_active(&r, 0),
            "Oneshot constraint must remain defunct after reconnect"
        );
    } else {
        // Persistent constraint reconciles against new settled state
        assert!(
            helper_smithay_active(&r, 0),
            "Persistent constraint reactivates on reconnect when surface is focused"
        );
    }
}

// ==============================================================================
// OB-51: W-SM-16 Popup Preemption under Active Lock
// ==============================================================================

#[test]
fn ob51_popup_preemption_under_lock() {
    let mut r = Rig::x8();
    focus_element(&mut r, 0);
    let qh = r.ls.qh.clone();

    // Lock pointer on W
    r.ls.probe.lock_pointer(&qh, 0, false);
    r.ls.pump();
    r.frame();

    assert!(helper_smithay_active(&r, 0));

    // Open grabbing popup
    let m_popup = r.mark();
    let spec = common::PopupSpec {
        content: (200, 150),
        shadow: 0,
        anchor_rect: (100, 100, 50, 50),
        slide: false,
    };
    let _popup_idx = r.ls.probe.popup_with_grab(&qh, "P", 0, spec);
    r.ls.pump();
    r.frame();
    r.ls.pump();
    r.frame();

    // Popup grab is active
    assert!(r.ls.state.is_pointer_grabbed());

    // Motions during popup grab route to popup, not W
    r.motion((2.0, 2.0));

    let popup_events = r.since(m_popup);
    println!("popup_events: {popup_events:#?}");
    let motion_to_w = popup_events
        .iter()
        .any(|s| s.starts_with("motion") && !s.contains("P"));
    assert!(
        !motion_to_w,
        "No absolute motion delivered to locked root surface W during popup grab"
    );

    // Destroy popup
    r.ls.probe.destroy_popup(1);
    r.ls.pump();
    r.frame();
    r.ls.pump();
    r.frame();

    assert!(!r.ls.state.is_pointer_grabbed());

    // Lock on W is preserved or cleanly restored without synthetic motion
    assert!(
        helper_smithay_active(&r, 0),
        "Pointer lock on W survives popup teardown"
    );
    assert!(r.ls.state.core().constraint().is_some());
}

// ==============================================================================
// OB-47: Port of xlock_batch (EE-2) — Lifecycle Matrix
// ==============================================================================

#[test]
fn ob47_e1_persistent_explicit_deactivate_then_exit() {
    let mut r = Rig::x8();
    focus_element(&mut r, 0);
    let qh = r.ls.qh.clone();

    r.ls.probe.lock_pointer(&qh, 0, false);
    r.ls.pump();
    r.frame();
    assert!(helper_smithay_active(&r, 0));

    // Deactivate lock while focus is held by deactivating constraint
    let root = helper_server_surface(&r, 0);
    let surface_key = r.ls.state.registry().key(&root).unwrap();
    let fx =
        r.ls.state
            .core_mut()
            .destroy_pointer_constraint(surface_key);
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();

    assert!(!helper_smithay_active(&r, 0));

    // Move pointer off surface:
    // Under patch P1 (authorized under D-B12-PIN), leave() deactivates only active constraints.
    // Because the constraint was already deactivated, zero duplicate unlocked events are emitted.
    let m = r.mark();
    r.motion((1000.0, 1000.0));
    let exit_events = r.since(m);
    let duplicate_unlocked = exit_events.iter().filter(|s| **s == "unlocked").count();
    assert_eq!(
        duplicate_unlocked, 0,
        "Under authorized patch P1, leave() emits zero duplicate unlocked events (W-SM-4c resolved)"
    );
}

#[test]
fn ob47_e3_oneshot_explicit_deactivate_defunct() {
    let mut r = Rig::x8();
    focus_element(&mut r, 0);
    let qh = r.ls.qh.clone();

    r.ls.probe.lock_pointer(&qh, 0, true);
    r.ls.pump();
    r.frame();
    assert!(helper_smithay_active(&r, 0));

    // Deactivate oneshot
    let root = helper_server_surface(&r, 0);
    let surface_key = r.ls.state.registry().key(&root).unwrap();
    let fx =
        r.ls.state
            .core_mut()
            .destroy_pointer_constraint(surface_key);
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();

    assert!(!helper_smithay_active(&r, 0));

    // Focus returns or moves: oneshot object is defunct and never reactivates
    r.motion((-10.0, 0.0));
    assert!(
        !helper_smithay_active(&r, 0),
        "Oneshot constraint must be defunct after deactivation"
    );
}

// ==============================================================================
// Checkpoint L: 200-cycle Long-Lived Pointer Constraint Stress
// ==============================================================================

#[test]
fn round5_pointer_constraints_stress_200_cycles() {
    let mut r = Rig::x8();
    focus_element(&mut r, 0);
    // every cycle needs the pointer on W, where a constraint can activate: x8's position,
    // (210, 200) from the view centre, which is local (610, 500) of the 800x600 window geometry.
    // The pans below are symmetric, so it is checked again at the end.
    let w = r.ls.state.elements().next().expect("W");
    common::assert_pointer_at(&mut r.ls, w, (610.0, 500.0), "round5 start");
    let qh = r.ls.qh.clone();

    for i in 0..200usize {
        match i % 3 {
            0 => {
                // Persistent lock + pan with lock + unlock
                r.ls.probe.lock_pointer(&qh, 0, false);
                r.ls.pump();
                r.frame();
                assert!(helper_smithay_active(&r, 0));

                r.button(BTN_MIDDLE, true, logo());
                r.motion((10.0, 10.0));
                r.motion((-10.0, -10.0));
                r.button(BTN_MIDDLE, false, logo());
                r.ls.pump();
                r.frame();

                assert!(helper_smithay_active(&r, 0));

                r.ls.probe.destroy_lock();
                r.ls.pump();
                r.frame();
                assert!(!helper_smithay_active(&r, 0));
            }
            1 => {
                // Confine + pan with confinement + destroy
                r.ls.probe.confine_pointer(&qh, 0);
                r.ls.pump();
                r.frame();
                assert!(helper_smithay_active(&r, 0));

                r.button(BTN_MIDDLE, true, logo());
                r.motion((5.0, 5.0));
                r.motion((-5.0, -5.0));
                r.button(BTN_MIDDLE, false, logo());
                r.ls.pump();
                r.frame();

                assert!(helper_smithay_active(&r, 0));

                r.ls.probe.destroy_confinement();
                r.ls.pump();
                r.frame();
                assert!(!helper_smithay_active(&r, 0));
            }
            _ => {
                // Oneshot lock + explicit destroy
                r.ls.probe.lock_pointer(&qh, 0, true);
                r.ls.pump();
                r.frame();
                assert!(helper_smithay_active(&r, 0));

                r.ls.probe.destroy_lock();
                r.ls.pump();
                r.frame();
                assert!(!helper_smithay_active(&r, 0));
            }
        }
    }

    let root = helper_server_surface(&r, 0);
    let surface_key = r.ls.state.registry().key(&root).unwrap();
    let fx =
        r.ls.state
            .core_mut()
            .destroy_pointer_constraint(surface_key);
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();

    assert!(!r.ls.state.is_pointer_grabbed());
    assert!(r.ls.state.core().constraint().is_none());
    assert!(!helper_smithay_active(&r, 0));
    common::assert_pointer_at(&mut r.ls, w, (610.0, 500.0), "round5 end");
    assert_eq!(
        r.camera(),
        pos(0, 0),
        "200 symmetric pans left the camera elsewhere"
    );
}
