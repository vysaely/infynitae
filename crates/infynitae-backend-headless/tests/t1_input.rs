//! INC-3 T1: pointer intake, ownership, CameraPan, routing and the zero-output transition on the
//! production path (core + adapter + headless backend + a real client over a socketpair).
//!
//! TST-05 (x8) is ported for the non-constraint cases (C1, C6, C7, C8, C9; C3–C5 are locks and
//! confinement, INC-5; C2 is the rejected LeaveFocus mode, not a production path). The oracle is
//! the client's own wire log (`probe.ptr`, wl_fixed values as received) and hand-derived literals:
//!
//! Output A 1920x1080 at scale 1, camera centre (0,0): v = (−960, −540) lies on the 120 lattice,
//! so c_eff = (−960, −540) and q = p + (−960, −540). W: content 800x600 with a 20 px shadow,
//! centred in the view (F-PLACE-1, C12.F1): anchor (−400, −300), so its root surface origin is
//! Canvas (−420, −320), W-local = q + (420, 320), and the startup pointer (960, 540) is W-local
//! (420, 320). Motions (200,200) then (10,0) put the pointer at p = (1170, 740), W-local
//! (630, 520). (x8 itself had W's origin at the camera centre and the pointer at (230, 220);
//! every W-local value here is x8's plus (400, 300).)
//! The pan: 30 × (7.3, −2.1) ⇒ C = (0,0) − Q16(219, −63) = (−219, 63) (x8). New view origin
//! (−1179, −477): R = (−1200, −480), C = (21, 3), c_eff = (−1179, −477) ⇒ q = (−9, 263) ⇒
//! W-local (411, 583) — x8's "motion (11,283)"; the next genuine (1,0) gives (412, 583).

mod common;

use common::input::*;
use common::{Lockstep, output_k};
use infynitae_backend_headless::{VirtualOutput, pointer_axis, pointer_button, pointer_motion};
use infynitae_core::geom::CanvasPos;
use infynitae_core::input::interaction::Interaction;
use infynitae_core::input::ownership::Owner;
use infynitae_core::input::{Axis, AxisSource};
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::pointer::PointerState;
use infynitae_core::{Core, EntryError, PhysicalEvent};

/// The setup itself: enter at the first routed motion, relative motion of the physical delta,
/// one frame per physical event.
#[test]
fn routed_motion_enters_then_moves_with_relative_deltas() {
    let r = Rig::x8();
    assert_eq!(
        r.since(0),
        vec![
            "enter W 620,520",
            "relative 200,200 unaccel 180,180",
            "frame",
            "motion 630,520",
            "relative 10,0 unaccel 9,0",
            "frame",
        ]
    );
    assert_eq!(
        r.ls.state.core().pointer(),
        PointerState::Located {
            output: r.ls.output(),
            p: (1170.0, 740.0)
        }
    );
}

/// TST-05 C1 (KeepFocus, free pointer): nothing reaches the client during the pan; the camera is
/// start − Q16(Σ); the anchor is unchanged; the release delivers the true position once; then
/// genuine motion continues from it with only its own relative delta.
#[test]
fn x8_c1_keepfocus_pan_then_true_position() {
    let mut r = Rig::x8();
    let anchor = r.anchor();
    let configures = r.ls.probe.configures.len();
    let m0 = r.mark();
    r.button(BTN_MIDDLE, true, logo());
    for d in x8_deltas(30) {
        r.motion(d);
    }
    assert_nothing_delivered(&r.since(m0), "C1 during the pan");
    assert!(matches!(
        r.ls.state.core().interaction(),
        Interaction::CameraPan { .. }
    ));
    let m1 = r.mark();
    r.button(BTN_MIDDLE, false, logo());
    assert_eq!(r.camera(), pos(-219, 63), "camera = start − Q16(Σ)");
    assert_eq!(r.anchor(), anchor, "INV-CAMERA-2: Stage anchor changed");
    assert_eq!(
        r.ls.probe.configures.len(),
        configures,
        "INV-CAMERA-4: configure during pan"
    );
    assert_eq!(r.since(m1), vec!["motion 411,583", "frame"]);
    let m2 = r.mark();
    r.motion((1.0, 0.0));
    assert_eq!(wl_fixed(0.9), 0.8984375);
    assert_eq!(
        r.since(m2),
        vec![
            "motion 412,583",
            "relative 1,0 unaccel 0.8984375,0",
            "frame"
        ]
    );
    assert_eq!(r.ls.state.core().interaction(), Interaction::Idle);
    assert!(r.client_buttons_down().is_empty());
    assert!(!r.smithay_grabbed());
}

/// The x8 pan with no frame produced between the pan motions and the release: reconciliation
/// still reads the panned camera (the pan dropped the output's ProjectionFrame, so the hit test
/// cannot read the frame shown before the pan).
#[test]
fn release_before_the_next_frame_uses_the_panned_camera() {
    let mut r = Rig::x8();
    r.button(BTN_MIDDLE, true, logo());
    for d in x8_deltas(30) {
        r.t += 1000;
        pointer_motion(&mut r.ls.state, &mut r.ls.input, d, (0.0, 0.0), r.t).unwrap();
    }
    r.ls.pump();
    let m = r.mark();
    r.t += 1000;
    pointer_button(
        &mut r.ls.state,
        &mut r.ls.input,
        BTN_MIDDLE,
        false,
        logo(),
        r.t,
    )
    .unwrap();
    r.ls.pump();
    assert_eq!(r.since(m), vec!["motion 411,583", "frame"]);
}

/// TST-05 C6: the client holds BTN_LEFT, then the binding is pressed: no acquisition (INV-INPUT-2);
/// the binding press is the client's, so it and its release are delivered; no stuck button.
#[test]
fn x8_c6_no_acquisition_while_the_client_owns_a_button() {
    let mut r = Rig::x8();
    let m0 = r.mark();
    r.button(BTN_LEFT, true, none());
    r.button(BTN_MIDDLE, true, logo());
    assert_eq!(r.ls.state.core().interaction(), Interaction::Idle);
    assert_eq!(
        r.ls.state.core().buttons().owner(BTN_MIDDLE),
        Some(Owner::Client)
    );
    r.motion((4.0, 0.0));
    r.button(BTN_MIDDLE, false, logo());
    r.button(BTN_LEFT, false, none());
    let relative = format!("relative 4,0 unaccel {},0", wl_fixed(0.9 * 4.0));
    assert_eq!(
        r.since(m0),
        vec![
            "button 272 pressed",
            "frame",
            "button 274 pressed",
            "frame",
            "motion 634,520",
            relative.as_str(),
            "frame",
            "button 274 released",
            "frame",
            "button 272 released",
            "frame",
        ]
    );
    assert_eq!(r.camera(), pos(0, 0), "a pan was acquired");
    assert!(r.client_buttons_down().is_empty());
    assert!(!r.smithay_grabbed(), "implicit grab stuck");
}

/// TST-05 C7: a button pressed during the pan is compositor-owned through its release, even after
/// the pan ends; the client sees neither edge.
#[test]
fn x8_c7_presses_during_the_pan_stay_compositor_owned() {
    let mut r = Rig::x8();
    let m0 = r.mark();
    r.button(BTN_MIDDLE, true, logo());
    r.motion((3.0, 0.0));
    r.button(BTN_LEFT, true, none());
    assert_eq!(
        r.ls.state.core().buttons().owner(BTN_LEFT),
        Some(Owner::Compositor)
    );
    r.button(BTN_MIDDLE, false, logo());
    r.motion((1.0, 0.0));
    r.button(BTN_LEFT, false, none());
    let lines = r.since(m0);
    assert!(
        !lines.iter().any(|l| l.starts_with("button")),
        "client saw a compositor-owned button: {lines:?}"
    );
    // camera −3 ⇒ W-local x 630 − 3 = 627 at reconciliation, 628 after the genuine motion
    assert_eq!(
        lines,
        vec![
            "motion 627,520",
            "frame",
            "motion 628,520",
            "relative 1,0 unaccel 0.8984375,0",
            "frame"
        ]
    );
    assert!(r.ls.state.core().buttons().pressed().is_empty());
    assert!(!r.smithay_grabbed());
}

/// TST-05 C8: the modifier released before the initiator does not end or leak the pan;
/// ownership follows the button.
#[test]
fn x8_c8_modifier_released_first() {
    let mut r = Rig::x8();
    let m0 = r.mark();
    r.button(BTN_MIDDLE, true, logo());
    for d in x8_deltas(5) {
        r.motion(d);
    }
    // the modifier goes up: the following events carry no modifier
    r.motion((7.3, -2.1));
    assert_nothing_delivered(&r.since(m0), "C8 during the pan");
    r.button(BTN_MIDDLE, false, none());
    // Σ = 6 × (7.3, −2.1) = (43.8, −12.6): Q16 ties-even of the f64 sums
    let sx = (0..6).fold(0.0f64, |a, _| a + 7.3);
    let sy = (0..6).fold(0.0f64, |a, _| a - 2.1);
    let q = |v: f64| (v * 65536.0).round_ties_even() as i64;
    assert_eq!(
        r.camera(),
        CanvasPos {
            x: -q(sx),
            y: -q(sy)
        }
    );
    assert_eq!(r.ls.state.core().interaction(), Interaction::Idle);
    assert!(r.ls.state.core().buttons().pressed().is_empty());
    // a later middle press without the modifier is not the binding: it is the client's
    let m1 = r.mark();
    r.button(BTN_MIDDLE, true, none());
    r.button(BTN_MIDDLE, false, none());
    assert_eq!(
        r.since(m1),
        vec![
            "button 274 pressed",
            "frame",
            "button 274 released",
            "frame"
        ]
    );
}

/// TST-05 C9: the focused surface is destroyed mid-pan: the pan survives and terminates cleanly;
/// reconciliation finds nothing under the pointer and clears the focus. Smithay's `leave` of the
/// dead focus sends no `leave` the client can observe (its surface argument no longer exists)
/// and then `frame` (wl_pointer v≥5, smithay `seat/pointer.rs` `leave`): the only wire event is
/// that lone frame. No motion, enter or button reaches anything.
#[test]
fn x8_c9_focused_surface_destroyed_mid_pan() {
    let mut r = Rig::x8();
    r.button(BTN_MIDDLE, true, logo());
    r.motion((5.0, 0.0));
    r.ls.probe.destroy_toplevel(0);
    r.ls.pump();
    r.motion((5.0, 0.0));
    let m0 = r.mark();
    r.button(BTN_MIDDLE, false, logo());
    r.motion((1.0, 0.0));
    assert_eq!(r.ls.state.core().interaction(), Interaction::Idle);
    assert!(r.ls.state.core().stage().z_order().is_empty());
    assert_eq!(r.since(m0), vec!["frame"], "events after destruction");
    let ptr = r.ls.state.seat().get_pointer().expect("pointer");
    assert!(ptr.current_focus().is_none());
}

/// INV-CAMERA-4: a pan that moves W out of the view and back causes leave/enter only — zero
/// configures and zero pointer events during the gesture (client-observed).
#[test]
fn pan_membership_changes_without_configures() {
    let mut r = Rig::x8();
    let configures = r.ls.probe.configures.len();
    let log0 = r.ls.probe.log.len();
    let m0 = r.mark();
    let n = r.ls.probe.outputs[0].0;
    r.button(BTN_MIDDLE, true, logo());
    // camera x +2000: view [1040, 2960) ∌ W [−420, 420)
    for _ in 0..20 {
        r.motion((-100.0, 0.0));
    }
    assert_eq!(r.camera(), pos(2000, 0));
    let left = r.ls.probe.log[log0..].to_vec();
    assert_eq!(left, vec![format!("leave W {n}")]);
    for _ in 0..20 {
        r.motion((100.0, 0.0));
    }
    assert_eq!(
        r.ls.probe.log[log0..].to_vec(),
        vec![format!("leave W {n}"), format!("enter W {n}")]
    );
    assert_nothing_delivered(&r.since(m0), "membership pan");
    r.button(BTN_MIDDLE, false, logo());
    assert_eq!(
        r.ls.probe.configures.len(),
        configures,
        "configure during pan"
    );
    // the camera is back where it started, so the release's true position is where the client
    // already has the pointer: nothing to tell it (the adapter drops a motion that repeats the last)
    assert_eq!(r.since(m0), Vec::<String>::new());
}

/// INC-10 (G-OB24 met, C12.F14.5; replaces the INC-3 gate test `no_pan_at_persistent_zoom`, whose
/// refusal C12.F14 lifted): a pan at a persistent zoom ≠ 1, at k ∉ n/120, moves the camera by
/// F-PAN-1 — `C = start − Q16(Σ dp / z)` at scale 1 — and keeps the output's lattice reference,
/// so no surface's physical width or height changes in any frame (F-LATTICE-1). The per-frame
/// reference of the base changes them (ED-1; seeded control in tools/verify-test-efficacy).
#[test]
fn zoomed_pan_moves_by_f_pan_1_and_keeps_widths() {
    let zoom = 0.73; // k ∉ n/120 at scale 1 (120·k = 87.6: each lattice cell shifts the fractions)
    let entries = vec![(VirtualOutput::key("A"), pos(0, 0), zoom)];
    let ls = Lockstep::with(
        PersistedCameras::from_entries(entries).unwrap(),
        &[a_1080()],
    );
    let mut r = Rig::new(ls);
    r.map_w();
    r.motion((3.0, 0.0));
    let o = r.ls.output();
    let sizes = |r: &mut Rig| -> Vec<(i64, i64)> {
        let frame = r.ls.state.core_mut().render_list(o).expect("frame");
        frame
            .entries
            .iter()
            .map(|e| (e.rect.x1 - e.rect.x0, e.rect.y1 - e.rect.y0))
            .collect()
    };
    let before = sizes(&mut r);
    assert!(!before.is_empty());
    let m0 = r.mark();
    r.button(BTN_MIDDLE, true, logo());
    assert!(matches!(
        r.ls.state.core().interaction(),
        Interaction::CameraPan { .. }
    ));
    let mut sum = (0.0f64, 0.0f64);
    let mut cells = std::collections::BTreeSet::new();
    for i in 0..240 {
        let d = (3.1 + (i % 7) as f64 * 0.37, -1.3 + (i % 5) as f64 * 0.29);
        sum = (sum.0 + d.0, sum.1 + d.1);
        r.motion(d);
        let v = r.ls.state.core().view(o).expect("view").v;
        cells.insert(((v.x >> 16).div_euclid(120), (v.y >> 16).div_euclid(120)));
        assert_eq!(sizes(&mut r), before, "a width changed at pan step {i}");
    }
    r.button(BTN_MIDDLE, false, logo());
    assert_nothing_delivered(&r.since(m0)[..r.since(m0).len() - 2], "zoomed pan");
    // the view origin crossed 120 px lattice cells: a per-frame reference would have moved
    assert!(cells.len() > 5, "{}", cells.len());
    // F-PAN-1, independent oracle: the exact Σ / z, within one Q16 unit (the f64 sum's rounding)
    let want = |s: f64| -(s / zoom * 65536.0);
    let c = r.camera();
    assert!(
        (c.x as f64 - want(sum.0)).abs() <= 1.0 && (c.y as f64 - want(sum.1)).abs() <= 1.0,
        "camera {c:?} vs Σ/z {:?}",
        (want(sum.0), want(sum.1))
    );
}

/// M10 / EE-3 (OB-50 pointer part): the last output removed under a focused pointer with a
/// client-held button: one leave, nothing in the zero-output state (motion, buttons, axis, the
/// held button's release), the startup placement enters after reconnect, and no implicit grab
/// survives (a different button's click after reconnect releases cleanly).
#[test]
fn zero_output_transition_with_a_held_button() {
    let mut r = Rig::x8();
    r.button(BTN_LEFT, true, none());
    let o = r.ls.output();
    let m0 = r.mark();
    r.ls.disconnect(o);
    assert_eq!(r.ls.state.core().pointer(), PointerState::NoOutput);
    // Focus::Clear: Smithay's leave sends wl_pointer.leave then frame (v≥5)
    assert_eq!(r.since(m0), vec!["leave W", "frame"]);
    let m1 = r.mark();
    r.motion((5.0, 5.0));
    r.button(BTN_RIGHT, true, none());
    r.button(BTN_RIGHT, false, none());
    r.button(BTN_LEFT, false, none());
    let axis = Axis {
        source: AxisSource::Wheel,
        amount: (0.0, 15.0),
        v120: (0, 120),
    };
    r.t += 1000;
    pointer_axis(&mut r.ls.state, &mut r.ls.input, axis, r.t).unwrap();
    r.ls.pump();
    assert!(r.since(m1).is_empty(), "zero-output leak {:?}", r.since(m1));
    assert!(r.ls.state.core().buttons().pressed().is_empty());
    let m2 = r.mark();
    r.ls.connect(&a_1080());
    // startup placement (960, 540) ⇒ q = (0, 0) ⇒ W-local (420, 320)
    assert_eq!(r.since(m2), vec!["enter W 420,320", "frame"]);
    assert!(!r.smithay_grabbed(), "zero-output grab not removed");
    r.button(BTN_RIGHT, true, none());
    r.button(BTN_RIGHT, false, none());
    assert!(
        !r.smithay_grabbed(),
        "implicit grab stuck: Smithay still believes BTN_LEFT is pressed"
    );
    assert!(r.client_buttons_down().is_empty());
}

/// EE-3 without a held button: leave at removal, enter after reconnect, nothing between.
#[test]
fn zero_output_transition_free_pointer() {
    let mut r = Rig::x8();
    let o = r.ls.output();
    let m0 = r.mark();
    r.ls.disconnect(o);
    // the leave happens at removal, not later
    assert_eq!(r.since(m0), vec!["leave W", "frame"], "no leave at removal");
    r.motion((1.0, 1.0));
    r.ls.connect(&a_1080());
    assert_eq!(
        r.since(m0),
        vec!["leave W", "frame", "enter W 420,320", "frame"]
    );
}

/// C4.6 / OB-3 (interaction orphaning): the pan's output is removed mid-pan; ownership is kept,
/// motion is consumed, and the initiator's release reconciles on the relocated output. B's
/// persisted camera centre (1060, −100) gives c_eff (100, −640); the pointer (1170+3, 740) on A
/// relocates to (0, 740) on B ⇒ q = (100, 100) ⇒ W-local (100 + 420, 100 + 320) = (520, 420). W
/// kept the focus throughout (KeepFocus), so the true position arrives as a motion.
#[test]
fn removing_the_pan_output_orphans_the_pan() {
    let entries = vec![(VirtualOutput::key("B"), pos(1060, -100), 1.0)];
    let ls = Lockstep::with(
        PersistedCameras::from_entries(entries).unwrap(),
        &[a_1080(), output_k("B", (1920, 1080), 120, 60_000)],
    );
    let mut r = Rig::new(ls);
    r.map_w();
    r.motion((200.0, 200.0));
    r.motion((10.0, 0.0));
    let a = r.ls.output();
    let m0 = r.mark();
    r.button(BTN_MIDDLE, true, logo());
    r.motion((3.0, 0.0));
    r.ls.disconnect(a);
    assert_eq!(
        r.ls.state.core().interaction(),
        Interaction::Orphaned {
            initiator: BTN_MIDDLE
        }
    );
    let cam_a = r.camera();
    r.motion((40.0, 0.0));
    assert_eq!(r.camera(), cam_a, "orphaned motion moved a camera");
    assert_nothing_delivered(&r.since(m0), "orphaned pan");
    r.button(BTN_MIDDLE, false, logo());
    assert_eq!(r.ls.state.core().interaction(), Interaction::Idle);
    assert_eq!(r.since(m0), vec!["motion 520,420", "frame"]);
    assert!(r.ls.state.core().buttons().pressed().is_empty());
}

/// M10 reconfiguration: the pointer is clamped (F-RECONF-1) and exactly one true-position event
/// follows. A 800x600: c_eff (−400, −300); p = (1170, 740) clamps on both axes to
/// (800 − 2⁻⁸, 600 − 2⁻⁸) ⇒ q = (400 − 2⁻⁸, 300 − 2⁻⁸) ⇒ W-local (820 − 2⁻⁸, 620 − 2⁻⁸), inside
/// the 840x640 root. (A mode larger than the centred window would leave every clamped position
/// outside it: the window is not moved by the reconfiguration.) During a pan the
/// reconfiguration sends nothing.
#[test]
fn reconfiguration_clamps_and_sends_one_true_position() {
    let mut r = Rig::x8();
    let o = r.ls.output();
    let m0 = r.mark();
    r.ls.reconfigure(o, &output_k("A", (800, 600), 120, 60_000));
    assert_eq!(
        r.since(m0),
        vec!["motion 819.99609375,619.99609375", "frame"]
    );
    r.button(BTN_MIDDLE, true, logo());
    let m1 = r.mark();
    r.ls.reconfigure(o, &a_1080());
    assert!(r.since(m1).is_empty(), "{:?}", r.since(m1));
    r.button(BTN_MIDDLE, false, logo());
    assert_eq!(r.since(m1).len(), 2, "one true position at the end");
}

/// B2.5 T1: relative motion crosses the Arrangement into B and clamps at the union's edges.
#[test]
fn motion_crosses_the_arrangement_and_clamps_to_the_union() {
    let mut r = Rig::new(Lockstep::with(
        PersistedCameras::default(),
        &[a_1080(), output_k("B", (1280, 720), 120, 60_000)],
    ));
    let (a, b) = {
        let o = r.ls.state.core().outputs();
        (o[0].id, o[1].id)
    };
    r.motion((950.0, 0.0)); // (1910, 540) on A
    r.motion((20.0, 0.0)); // a' = (1930, 540) ⇒ B (10, 540)
    assert_eq!(
        r.ls.state.core().pointer(),
        PointerState::Located {
            output: b,
            p: (10.0, 540.0)
        }
    );
    r.motion((0.0, 500.0)); // B is 720 tall: the union is [0,1920)x[0,1080) ∪ [1920,3200)x[0,720)
    // a' = (1930, 1040) is nearest to A's rect at (1920 − 2⁻⁸, 1040)
    assert_eq!(
        r.ls.state.core().pointer(),
        PointerState::Located {
            output: a,
            p: (1920.0 - 1.0 / 256.0, 1040.0)
        }
    );
    r.motion((1e9, -1e9));
    assert_eq!(
        r.ls.state.core().pointer(),
        PointerState::Located {
            output: b,
            p: (1280.0 - 1.0 / 256.0, 0.0)
        }
    );
}

/// Axis events are routed to the focus on the client path and consumed during a pan.
#[test]
fn axis_is_routed_and_consumed_during_a_pan() {
    let mut r = Rig::x8();
    let wheel = Axis {
        source: AxisSource::Wheel,
        amount: (0.0, 15.0),
        v120: (0, 120),
    };
    let m0 = r.mark();
    r.t += 1000;
    pointer_axis(&mut r.ls.state, &mut r.ls.input, wheel, r.t).unwrap();
    r.ls.pump();
    let routed = r.since(m0).len();
    assert!(
        r.since(m0).iter().any(|l| l.starts_with("axis")),
        "{:?}",
        r.since(m0)
    );
    assert_eq!(r.since(m0).last(), Some(&"frame"));
    r.button(BTN_MIDDLE, true, logo());
    let m1 = r.mark();
    r.t += 1000;
    pointer_axis(&mut r.ls.state, &mut r.ls.input, wheel, r.t).unwrap();
    r.ls.pump();
    assert!(r.since(m1).is_empty(), "axis leaked during the pan");
    assert!(routed >= 2);
}

/// Intake is bound to its Core and rejects non-finite values without changing state.
#[test]
fn intake_rejects_foreign_ports_and_non_finite_values() {
    let mut r = Rig::x8();
    let (_other, mut foreign, _, _) = Core::new(PersistedCameras::default());
    let before = r.ls.state.core().pointer();
    let ev = PhysicalEvent::PointerMotion {
        delta: (1.0, 0.0),
        delta_unaccel: (1.0, 0.0),
        time_us: 1,
    };
    assert_eq!(
        r.ls.state.core_mut().physical_input_vec(&mut foreign, ev),
        Err(EntryError::ForeignPort)
    );
    for bad in [f64::NAN, f64::INFINITY] {
        let ev = PhysicalEvent::PointerMotion {
            delta: (bad, 0.0),
            delta_unaccel: (0.0, 0.0),
            time_us: 1,
        };
        assert_eq!(
            r.ls.state
                .core_mut()
                .physical_input_vec(&mut r.ls.input, ev),
            Err(EntryError::InvalidEvent)
        );
    }
    assert_eq!(r.ls.state.core().pointer(), before);
}
