//! INC-2 T1: multi-output lifecycle, membership, frame clock and presentation on the production
//! path (core + adapter + headless backend + a real probe client over a socketpair).
//!
//! Oracle: the client's ordered wire log and hand-derived expectations from the canonical rules
//! (M4, M5, INV-OUTPUT-1, F-MEMBER-1, INV-FRAME-2, INV-FRAME-5). No expected value is computed by
//! the production code under test.

mod common;

use std::time::Duration;

use common::{Lockstep, PopupSpec, output_k};
use infynitae_backend_headless::VirtualOutput;
use infynitae_core::EntryError;
use infynitae_core::geom::CanvasPos;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::pointer::PointerState;

const MS: Duration = Duration::from_millis(1);

fn pos(x: i64, y: i64) -> CanvasPos {
    CanvasPos {
        x: x << 16,
        y: y << 16,
    }
}

fn persisted(entries: &[(&str, (i64, i64))]) -> PersistedCameras {
    PersistedCameras::from_entries(
        entries
            .iter()
            .map(|(c, (x, y))| (VirtualOutput::key(c), pos(*x, *y), 1.0))
            .collect(),
    )
    .unwrap()
}

/// The global name the client saw for the most recently announced wl_output.
fn last_global(ls: &Lockstep) -> u32 {
    ls.probe
        .log
        .iter()
        .rev()
        .find_map(|l| {
            l.strip_prefix("global wl_output ")
                .map(|n| n.parse().unwrap())
        })
        .expect("an output global")
}

fn index_of(ls: &Lockstep, line: &str) -> Option<usize> {
    ls.probe.log.iter().position(|l| l == line)
}

fn count(ls: &Lockstep, line: &str) -> usize {
    ls.probe.log.iter().filter(|l| *l == line).count()
}

/// Maps a frame-less toplevel W with content `content` and no shadow.
fn map_w(ls: &mut Lockstep, content: (i32, i32)) -> usize {
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", content, 0);
    ls.pump();
    w
}

/// M4/M5/INV-OUTPUT-1 on the wire: globals appear with the model's state, enter follows
/// membership, leave precedes the global's removal, reconnect re-binds the remembered camera with a
/// new global, a stale id is rejected.
#[test]
fn lifecycle_on_the_wire() {
    let mut ls = Lockstep::with(
        persisted(&[("B", (5000, 0))]),
        &[output_k("A", (1920, 1080), 120, 60_000)],
    );
    let a = ls.output();
    let na = last_global(&ls);
    assert!(index_of(&ls, &format!("{na}: mode 1920x1080@60000")).is_some());
    assert!(index_of(&ls, &format!("{na}: xdg position 0,0")).is_some());
    map_w(&mut ls, (400, 300));
    assert_eq!(count(&ls, &format!("enter W {na}")), 1);

    // hotplug B, arranged right of A, camera seeded from the persisted table: W is not on B
    let b = ls.connect(&output_k("B", (1920, 1080), 120, 60_000));
    let nb = last_global(&ls);
    assert_ne!(na, nb);
    assert!(index_of(&ls, &format!("{nb}: xdg position 1920,0")).is_some());
    assert_eq!(count(&ls, &format!("enter W {nb}")), 0);
    let core = ls.state.core();
    let cb = core.cameras().camera_for_output(b).unwrap();
    assert_eq!(core.cameras().center(cb), Some(pos(5000, 0)));

    // unplug A: leave before global_remove; camera kept unbound; pointer relocated to B
    let ca = ls.state.core().cameras().camera_for_output(a).unwrap();
    ls.disconnect(a);
    let leave = index_of(&ls, &format!("leave W {na}")).expect("leave on removal");
    let removed = index_of(&ls, &format!("global_remove {na}")).expect("global removed");
    assert!(
        leave < removed,
        "leave must precede the global's removal (INV-OUTPUT-1)"
    );
    let core = ls.state.core();
    assert_eq!(core.cameras().bound_output(ca), None);
    assert_eq!(core.cameras().center(ca), Some(pos(0, 0)));
    assert_eq!(
        core.pointer(),
        PointerState::Located {
            output: b,
            p: (0.0, 540.0)
        }
    );

    // a stale id is rejected and changes nothing on the wire
    let before = ls.probe.log.len();
    let err = infynitae_backend_headless::disconnect(&mut ls.state, &mut ls.token, a);
    assert_eq!(err, Err(EntryError::UnknownOutput));
    ls.pump();
    assert_eq!(ls.probe.log.len(), before);

    // reconnect A: a new OutputId and global, the same camera, W enters again
    let a2 = ls.connect(&output_k("A", (1920, 1080), 120, 60_000));
    let na2 = last_global(&ls);
    assert_ne!(a2, a);
    // a new global is announced after the removal (wayland-server may reuse the numeric name)
    let announced = ls
        .probe
        .log
        .iter()
        .rposition(|l| *l == format!("global wl_output {na2}"))
        .unwrap();
    assert!(
        announced > removed,
        "the reconnected output is a new global"
    );
    assert_eq!(ls.state.core().cameras().camera_for_output(a2), Some(ca));
    assert!(
        ls.probe.log[announced..]
            .iter()
            .any(|l| *l == format!("enter W {na2}")),
        "W enters the reconnected output"
    );
    assert_eq!(
        ls.state.core().cameras().len(),
        2,
        "no camera created for a remembered key"
    );
}

#[test]
fn output_removed_disables_before_removal() {
    let mut ls = Lockstep::new(Some(output_k("A", (1920, 1080), 120, 60_000)));
    let a = ls.output();

    // Disconnecting the output disables it and queues removal with a 5-second delay
    ls.disconnect(a);
    assert_eq!(ls.state.disabled_outputs_count(), 1);

    // Pruning before deadline keeps it queued
    ls.state.prune_disabled_outputs();
    assert_eq!(ls.state.disabled_outputs_count(), 1);

    // Advancing deadline to past allows prune_disabled_outputs to remove it
    ls.state.expire_disabled_outputs();
    ls.state.prune_disabled_outputs();
    assert_eq!(ls.state.disabled_outputs_count(), 0);
}

/// M4 reconfiguration on the wire: wl_output/xdg_output state follows the new facts with the same
/// OutputId and camera; membership is recomputed (enter on growth, leave on shrink).
#[test]
fn reconfiguration_on_the_wire() {
    // A shows W (400x300) centred, Canvas x [−200, 200); B's camera at (1500,150): its 1920-wide
    // view [540, 2460) misses it
    let mut ls = Lockstep::with(
        persisted(&[("B", (1500, 150))]),
        &[
            output_k("A", (1920, 1080), 120, 60_000),
            output_k("B", (1920, 1080), 120, 60_000),
        ],
    );
    let b = ls.state.core().outputs()[1].id;
    let nb = last_global(&ls);
    map_w(&mut ls, (400, 300));
    assert_eq!(count(&ls, &format!("enter W {nb}")), 0);
    let cb = ls.state.core().cameras().camera_for_output(b).unwrap();

    // 3840x2160 at scale 1: view [−420, 3420) now covers W → enter after the new mode
    ls.reconfigure(b, &output_k("B", (3840, 2160), 120, 60_000));
    let mode = index_of(&ls, &format!("{nb}: mode 3840x2160@60000")).expect("new mode");
    let enter = index_of(&ls, &format!("enter W {nb}")).expect("enter on growth");
    assert!(mode < enter);
    // scale 2: logical 1920 wide → view [540, 2460) again → leave; xdg size follows
    ls.reconfigure(b, &output_k("B", (3840, 2160), 240, 60_000));
    assert!(index_of(&ls, &format!("{nb}: scale 2")).is_some());
    assert!(index_of(&ls, &format!("{nb}: xdg size 1920,1080")).is_some());
    assert_eq!(count(&ls, &format!("leave W {nb}")), 1);
    // refresh change
    ls.reconfigure(b, &output_k("B", (3840, 2160), 240, 144_000));
    assert!(index_of(&ls, &format!("{nb}: mode 3840x2160@144000")).is_some());
    // same OutputId, same global, same camera with the same value; nothing allocated
    let core = ls.state.core();
    assert_eq!(core.outputs()[1].id, b);
    assert_eq!(core.cameras().camera_for_output(b), Some(cb));
    assert_eq!(core.cameras().center(cb), Some(pos(1500, 150)));
    assert_eq!(core.cameras().len(), 2);
    assert_eq!(count(&ls, "global_remove 0"), 0);
    assert_eq!(
        ls.probe
            .log
            .iter()
            .filter(|l| l.starts_with("global wl_output"))
            .count(),
        2
    );
    // a changed OutputKey is an identity change: rejected, nothing sent
    let before = ls.probe.log.len();
    let err = infynitae_backend_headless::reconfigure(
        &mut ls.state,
        &mut ls.token,
        b,
        &output_k("C", (800, 600), 120, 60_000),
    );
    assert_eq!(err, Err(EntryError::IdentityChanged));
    ls.pump();
    assert_eq!(ls.probe.log.len(), before);
}

/// Runtime 2i (OB-49) on the T1 path: two connected outputs presenting the same OutputKey get
/// distinct cameras and distinct wl_output names; the duplicate is not seeded from persistence;
/// reconnects re-bind the lowest unbound camera. Under `probe_sealed_key_steal` this test must
/// fail with "1:1 violated".
#[test]
fn same_key_outputs_keep_one_to_one() {
    let a_out = output_k("A", (1920, 1080), 120, 60_000);
    let mut ls = Lockstep::with(
        persisted(&[("A", (77, 88))]),
        &[a_out.clone(), a_out.clone()],
    );
    let (o1, o2) = (
        ls.state.core().outputs()[0].id,
        ls.state.core().outputs()[1].id,
    );
    let core = ls.state.core();
    let (c1, c2) = (
        core.cameras().camera_for_output(o1),
        core.cameras().camera_for_output(o2),
    );
    assert!(
        c1.is_some() && c2.is_some() && c1 != c2,
        "1:1 violated: {c1:?} {c2:?}"
    );
    assert_eq!(core.cameras().center(c1.unwrap()), Some(pos(77, 88)));
    assert_eq!(
        core.cameras().center(c2.unwrap()),
        Some(pos(0, 0)),
        "duplicate seeded from persistence"
    );
    let names: Vec<_> = ls
        .probe
        .log
        .iter()
        .filter(|l| l.contains(": name "))
        .cloned()
        .collect();
    assert_eq!(names.len(), 2);
    assert_ne!(
        names[0].split(": name ").nth(1),
        names[1].split(": name ").nth(1),
        "wl_output names must be unique"
    );
    // each output's membership is its own: W (centred on the first camera's centre) enters both
    map_w(&mut ls, (400, 300));
    assert_eq!(
        ls.probe
            .log
            .iter()
            .filter(|l| l.starts_with("enter W "))
            .count(),
        2
    );
    ls.disconnect(o1);
    ls.disconnect(o2);
    let r1 = ls.connect(&a_out);
    let r2 = ls.connect(&a_out);
    let core = ls.state.core();
    assert_eq!(
        core.cameras().camera_for_output(r1),
        c1,
        "reconnect takes the lowest unbound camera"
    );
    assert_eq!(core.cameras().camera_for_output(r2), c2);
    assert_eq!(core.cameras().len(), 2);
}

/// TST-03 SC3(d)/SC1 frame clock: W observed by A (165 Hz) and B (144 Hz) completes on A's vblanks
/// only (highest refresh, INV-FRAME-2): 165 dones in one second.
#[test]
fn clock_is_the_highest_refresh_observer() {
    let mut ls = Lockstep::with(
        persisted(&[("B", (600, 300))]),
        &[
            output_k("A", (1920, 1080), 120, 165_000),
            output_k("B", (3840, 2160), 180, 144_000),
        ],
    );
    let w = map_w(&mut ls, (800, 600));
    let (a, b) = (
        ls.state.core().outputs()[0].id,
        ls.state.core().outputs()[1].id,
    );
    let id = ls.state.elements().next().unwrap();
    assert_eq!(ls.state.core().membership().observers(id), vec![a, b]);
    assert_eq!(ls.state.core().clock_output_of(id), Some(a));
    ls.probe.surfaces[w].frame_driven = true;
    let qh = ls.qh.clone();
    ls.probe.frame_and_draw(&qh, w);
    ls.pump();
    ls.run_until(Duration::from_secs(1));
    let dones = ls
        .probe
        .frames
        .iter()
        .filter(|(t, _)| *t <= Duration::from_secs(1))
        .count();
    // A's period is 1e12/165000 ns = 6 060 606 ns: vblanks at k·period for k = 1..=165
    assert_eq!(dones, 165, "one completion per vblank of the clock output");
}

/// SC3(d2): A shows a 20 px sliver, B shows all of W: the clock is still A (highest refresh).
#[test]
fn clock_does_not_depend_on_visible_area() {
    // B connects first, so W (800x600) is centred on B's camera centre (1400,600):
    // W = [1000,1800) × [300,900). A's camera centre 40 → view [−920, 1000): none of W; A at 60 →
    // [−900, 1020): 20 px.
    let mut ls = Lockstep::with(
        persisted(&[("B", (1400, 600)), ("A", (60, 600))]),
        &[
            output_k("B", (3840, 2160), 180, 144_000),
            output_k("A", (1920, 1080), 120, 165_000),
        ],
    );
    map_w(&mut ls, (800, 600));
    let (b, a) = (
        ls.state.core().outputs()[0].id,
        ls.state.core().outputs()[1].id,
    );
    let id = ls.state.elements().next().unwrap();
    assert_eq!(ls.state.core().membership().observers(id), vec![b, a]);
    assert_eq!(ls.state.core().clock_output_of(id), Some(a));
}

/// SC1/SC3 starve and re-entry, and SC3(e) handover: an unobserved element's callback is withheld
/// (no completion while off-screen, INV-FRAME-3 kick/fallback are INC-8); when it becomes observed
/// again it completes at the first vblank of the new clock output; a callback pending at handover
/// completes at the next vblank of the surviving clock.
#[test]
fn starve_reentry_and_handover() {
    let a_out = output_k("A", (1920, 1080), 120, 165_000);
    let mut ls = Lockstep::with(
        persisted(&[("B", (5000, 0)), ("C", (300, 300))]),
        &[a_out.clone(), output_k("B", (1920, 1080), 120, 60_000)],
    );
    let w = map_w(&mut ls, (800, 600));
    let a = ls.state.core().outputs()[0].id;
    let qh = ls.qh.clone();
    ls.probe.frame_and_draw(&qh, w);
    ls.pump();
    ls.run_until(20 * MS);
    let before = ls.probe.frames.len();
    assert!(before >= 1, "visible: completes");
    // A unplugged: W unobserved (B looks elsewhere); a pending callback stays pending
    ls.probe.frame_and_draw(&qh, w);
    ls.pump();
    ls.disconnect(a);
    let t_off = ls.now;
    ls.run_until(t_off + 500 * MS);
    assert_eq!(
        ls.probe.frames.len(),
        before,
        "callbacks withheld while unobserved"
    );
    // A returns: same camera, W observed; first completion at A's first vblank
    let t_on = ls.now;
    ls.connect(&a_out);
    ls.run_until(t_on + 20 * MS);
    let first = ls
        .probe
        .frames
        .get(before)
        .expect("completion after re-entry")
        .0;
    assert_eq!(
        first,
        t_on + Duration::from_nanos(6_060_606),
        "first vblank of the new clock"
    );
    // handover: C (144 Hz) also shows W; A (clock) unplugged with a callback pending
    let c = ls.connect(&output_k("C", (1920, 1080), 120, 144_000));
    let a3 = ls
        .state
        .core()
        .outputs()
        .iter()
        .find(|o| o.key.connector == "A")
        .unwrap()
        .id;
    ls.probe.frame_and_draw(&qh, w);
    ls.pump();
    let n = ls.probe.frames.len();
    let t_h = ls.now;
    ls.disconnect(a3);
    let id = ls.state.elements().next().unwrap();
    assert_eq!(ls.state.core().clock_output_of(id), Some(c));
    ls.run_until(t_h + 20 * MS);
    let after = ls.probe.frames.get(n).expect("completion after handover").0;
    // C was connected at t_on + 20 ms, period 1e12/144000 ns = 6 944 444 ns
    let c_period = Duration::from_nanos(6_944_444);
    let mut next_c = t_on + 20 * MS + c_period;
    while next_c <= t_h {
        next_c += c_period;
    }
    assert_eq!(
        after, next_c,
        "the pending callback completes at the next vblank of the new clock"
    );
}

/// TST-06 (x9), INV-FRAME-5: commit #1 presented while observed (sync = the clock output);
/// commits #2 and #3 while unobserved: #2 discarded when #3 supersedes it, #3 stays pending and is
/// presented after re-entry, never discarded.
#[test]
fn x9_presentation_feedback() {
    let a_out = output_k("A", (1920, 1080), 120, 165_000);
    let mut ls = Lockstep::with(
        persisted(&[("B", (5000, 0))]),
        &[a_out.clone(), output_k("B", (1920, 1080), 120, 60_000)],
    );
    let w = map_w(&mut ls, (400, 300));
    let a = ls.state.core().outputs()[0].id;
    let na = ls
        .probe
        .log
        .iter()
        .find_map(|l| l.strip_prefix("global wl_output "))
        .unwrap()
        .to_string();
    let qh = ls.qh.clone();
    let s1 = ls.probe.frame_and_draw(&qh, w);
    ls.pump();
    ls.run_until(10 * MS);
    assert!(index_of(&ls, &format!("presented #{s1}")).is_some());
    assert!(
        index_of(&ls, &format!("sync #{s1} {na}")).is_some(),
        "sync_output = clock output"
    );
    ls.disconnect(a);
    let s2 = ls.probe.frame_and_draw(&qh, w);
    ls.pump();
    let s3 = ls.probe.frame_and_draw(&qh, w);
    ls.pump();
    ls.run_until(ls.now + 100 * MS);
    assert!(
        index_of(&ls, &format!("discarded #{s2}")).is_some(),
        "superseded commit discarded"
    );
    assert!(index_of(&ls, &format!("presented #{s2}")).is_none());
    assert!(
        index_of(&ls, &format!("presented #{s3}")).is_none(),
        "unobserved: still pending"
    );
    assert!(index_of(&ls, &format!("discarded #{s3}")).is_none());
    let t_on = ls.now;
    ls.connect(&a_out);
    ls.run_until(t_on + 10 * MS);
    let na2 = last_global(&ls);
    assert!(
        index_of(&ls, &format!("presented #{s3}")).is_some(),
        "latest commit presented on re-entry"
    );
    assert!(index_of(&ls, &format!("sync #{s3} {na2}")).is_some());
}

/// TST-03 X5 (EB-H1): popup membership follows the popup's true geometry (toplevel shadow 20,
/// popup shadow 10). Negative control: Smithay's `Window::output_enter`, driven exactly as `Space`
/// drives it, enters the popup on an output that shows none of it.
#[test]
fn x5_popup_membership_and_output_enter_control() {
    // A's camera centre (1400,600) places W (800x600, centred) at anchor (1000,300), X5's
    // position; root origin (980,280); popup geometry origin (1100,350), surface origin
    // (1090,340), surface 220x120 → true Canvas x range [1090, 1310).
    for (case, b_centre_x, popup_visible) in [
        ("B right edge at 1085: popup invisible", 1085 - 960, false),
        ("B left edge at 1305: popup visible 5 px", 1305 + 960, true),
        ("B right edge at 1095: popup visible 5 px", 1095 - 960, true),
    ] {
        let mut ls = Lockstep::with(
            persisted(&[("A", (1400, 600)), ("B", (b_centre_x, 300))]),
            &[
                output_k("A", (1920, 1080), 120, 165_000),
                output_k("B", (1920, 1080), 120, 60_000),
            ],
        );
        let nb = last_global(&ls);
        let qh = ls.qh.clone();
        let w = ls.probe.toplevel(&qh, "W", (800, 600), 20);
        ls.pump();
        ls.probe.popup(
            &qh,
            "P",
            w,
            PopupSpec {
                content: (200, 100),
                shadow: 10,
                anchor_rect: (100, 50, 1, 1),
                slide: false,
            },
        );
        ls.pump();
        assert_eq!(
            count(&ls, &format!("enter P {nb}")),
            usize::from(popup_visible),
            "{case}"
        );

        // negative control (invisible case only): Space's overlap for B, handed to output_enter
        if !popup_visible {
            use smithay::desktop::space::SpaceElement;
            let b = ls.state.core().outputs()[1].id;
            let id = ls.state.elements().next().unwrap();
            let window = ls.state.window(id).unwrap().clone();
            let output = ls.state.smithay_output(b).unwrap().clone();
            // Space: element location = geometry origin (1000,300); bbox in space coordinates =
            // bbox_with_popups + location − geometry.loc; overlap relative to the bbox origin
            let geo = window.geometry();
            let mut bbox = window.bbox_with_popups();
            bbox.loc += smithay::utils::Point::from((1000, 300)) - geo.loc;
            let out_geo = smithay::utils::Rectangle::new(
                (b_centre_x as i32 - 960, 300 - 540).into(),
                (1920, 1080).into(),
            );
            let mut overlap = out_geo.intersection(bbox).expect("W overlaps B");
            overlap.loc -= bbox.loc;
            window.output_enter(&output, overlap);
            ls.pump();
            assert_eq!(
                count(&ls, &format!("enter P {nb}")),
                1,
                "{case}: Window::output_enter must enter the invisible popup (EB-H1)"
            );
        }
    }
}

/// R15 / N21 detection: causing (grab-serial) output A != current pointer output B != clock output C.
/// All cameras at the Canvas origin, where the parent W1 (400x300) is centred: it spans Canvas x
/// [−200, 200) and is visible on A, B, C. Popup: 600 wide at parent x = 390 with slide, so
/// unconstrained it spans Canvas x [190, 790). Expected parent-relative x by target, from the
/// view's right edge e: slid to Canvas e − 600, i.e. e − 600 + 200. A (e = 960): fits, 390
/// unchanged; B (e = 400): 0; C (e = 640): 240.
#[test]
fn r15_rule1_uses_the_causing_event_output() {
    use common::input::{BTN_LEFT, none};
    use common::{PopupSpec, output_k};
    use infynitae_backend_headless::{pointer_button, pointer_motion, set_focus};
    use infynitae_core::geom::CanvasPos;
    use infynitae_core::navigation::PersistedCameras;
    use infynitae_core::stage::ElementId;

    let entries = ["A", "B", "C"]
        .iter()
        .map(|k| {
            (
                infynitae_backend_headless::VirtualOutput::key(k),
                CanvasPos { x: 0, y: 0 },
                1.0,
            )
        })
        .collect();
    let mut ls = Lockstep::with(
        PersistedCameras::from_entries(entries).unwrap(),
        &[
            output_k("A", (1920, 1080), 120, 60_000),
            output_k("B", (800, 600), 120, 60_000),
            output_k("C", (1280, 720), 120, 144_000),
        ],
    );
    ls.probe.auto_destroy_popups = true;
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
    ls.pump();
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    let elements: Vec<ElementId> = ls.state.elements().collect();
    assert_eq!(elements.len(), 1);

    pointer_motion(
        &mut ls.state,
        &mut ls.input,
        (50.0, 50.0),
        (50.0, 50.0),
        10_000,
    )
    .expect("motion");
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);

    // Click on A (the causing event)
    pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, none(), 11_000).expect("button");
    ls.pump();
    let serial = ls.probe.last_serial;
    let out_click = ls.state.core().pointer_output();
    pointer_button(
        &mut ls.state,
        &mut ls.input,
        BTN_LEFT,
        false,
        none(),
        12_000,
    )
    .expect("button");
    ls.pump();
    set_focus(&mut ls.state, &mut ls.input, Some(elements[0])).expect("focus");
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);

    // Pointer moves to B
    pointer_motion(
        &mut ls.state,
        &mut ls.input,
        (1000.0, 0.0),
        (1000.0, 0.0),
        13_000,
    )
    .expect("motion");
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    let out_now = ls.state.core().pointer_output();
    assert_ne!(out_click, out_now, "pointer did not change output");

    let p = ls.probe.popup_with_grab_serial(
        &qh,
        "P",
        w,
        PopupSpec {
            content: (600, 50),
            shadow: 0,
            anchor_rect: (390, 10, 1, 1),
            slide: true,
        },
        serial,
    );
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    let cfg = ls.probe.surfaces[p].popup_configure;
    assert_eq!(
        cfg.map(|c| c.0),
        Some(390),
        "popup constrained to the wrong output's viewport"
    );
}

/// M08 verification: causal serial outputs store is strictly capped at 64 entries.
#[test]
fn serial_outputs_storage_bounded_at_capacity() {
    use common::input::{BTN_LEFT, none};
    use infynitae_backend_headless::pointer_button;

    let mut ls = Lockstep::new(Some(common::output((1920, 1080), 120)));
    for i in 0..100 {
        let t = 10_000 + (i as u64) * 1000;
        pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, none(), t).expect("press");
        pointer_button(
            &mut ls.state,
            &mut ls.input,
            BTN_LEFT,
            false,
            none(),
            t + 500,
        )
        .expect("release");
    }
    assert_eq!(
        ls.state.serial_outputs_len(),
        64,
        "serial outputs must be capped at 64 entries (M08)"
    );
}

/// Rule-1 provenance store SEMANTICS (B3: F15/F16), literal truth from the test's own sequence.
/// Outputs A (1920x1080, 60 Hz), B (800x600, 60 Hz), C (1280x720, 144 Hz = the parent's frame
/// clock), all cameras centred on the parent. A 600-wide sliding popup at x = 390 lands at
/// x = 360 on A's viewport, -200 on B's, 40 on C's. Sequence: one click on A (s_old), 20 clicks
/// on C, 20 clicks on A (2 records per click: 82 records, capacity 64, FIFO), pointer parked on
/// B. Retained: the last 32 clicks (C #9..#20, A #21..#40). Expected by literal:
/// s_old (evicted) -> pointer output B; C #5 (evicted) -> B; C #15 -> C; A #30 -> A;
/// newest (A #40) -> A; a never-issued serial -> B. A newest-first eviction (F15) or an unknown
/// serial aliasing the latest record (F16) cannot satisfy all six.
#[test]
fn serial_provenance_fifo_eviction_and_fallback() {
    use common::input::{BTN_LEFT, none};
    use common::{PopupSpec, output_k};
    use infynitae_backend_headless::{pointer_button, pointer_motion};
    use infynitae_core::geom::CanvasPos;
    use infynitae_core::navigation::PersistedCameras;

    let entries = ["A", "B", "C"]
        .iter()
        .map(|k| {
            (
                infynitae_backend_headless::VirtualOutput::key(k),
                CanvasPos { x: 0, y: 0 },
                1.0,
            )
        })
        .collect();
    let mut ls = Lockstep::with(
        PersistedCameras::from_entries(entries).unwrap(),
        &[
            output_k("A", (1920, 1080), 120, 60_000),
            output_k("B", (800, 600), 120, 60_000),
            output_k("C", (1280, 720), 120, 144_000),
        ],
    );
    ls.probe.auto_destroy_popups = true;
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
    ls.pump();
    let mut t = 10_000u64;
    let adv = |ls: &mut Lockstep| {
        ls.pump();
        let n = ls.now + Duration::from_micros(16_667);
        ls.run_until(n);
    };
    adv(&mut ls);
    let motion = |ls: &mut Lockstep, t: &mut u64, d: (f64, f64)| {
        *t += 1000;
        pointer_motion(&mut ls.state, &mut ls.input, d, d, *t).expect("motion");
        adv(ls);
    };
    // returns the press serial (the cause a client would hand to xdg_popup.grab)
    let click = |ls: &mut Lockstep, t: &mut u64| -> u32 {
        *t += 1000;
        pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, true, none(), *t).expect("press");
        adv(ls);
        let s = ls.probe.last_serial;
        *t += 1000;
        pointer_button(&mut ls.state, &mut ls.input, BTN_LEFT, false, none(), *t).expect("release");
        adv(ls);
        s
    };
    // q (50, −50): on W1 (Canvas [−200,200)x[−150,150)), output A
    motion(&mut ls, &mut t, (50.0, -50.0));
    let s_old = click(&mut ls, &mut t); // focuses W1
    // onto W1's area on output C (C-local (700, 490) is q (60, 130); W1 spans C-local x 440..840
    // and y 210..510 there): the client receives every press, so each serial is a real causing
    // event
    motion(&mut ls, &mut t, (2410.0, 0.0));
    let c: Vec<u32> = (0..20).map(|_| click(&mut ls, &mut t)).collect();
    motion(&mut ls, &mut t, (-2410.0, 0.0)); // back onto W1 on output A
    let a: Vec<u32> = (0..20).map(|_| click(&mut ls, &mut t)).collect();
    motion(&mut ls, &mut t, (1000.0, 0.0)); // output B (fallback)
    assert_eq!(ls.state.serial_outputs_len(), 64);
    let place = |ls: &mut Lockstep, serial: u32| -> Option<i32> {
        let p = ls.probe.popup_with_grab_serial(
            &qh,
            "P",
            w,
            PopupSpec {
                content: (600, 50),
                shadow: 0,
                anchor_rect: (390, 10, 1, 1),
                slide: true,
            },
            serial,
        );
        adv(ls);
        let x = ls.probe.surfaces[p].popup_configure.map(|c| c.0);
        ls.probe.destroy_popup(p);
        adv(ls);
        x
    };
    let got = [
        ("s_old (evicted)", place(&mut ls, s_old)),
        ("C #5 (evicted)", place(&mut ls, c[4])),
        ("C #15 (retained)", place(&mut ls, c[14])),
        ("A #30 (retained)", place(&mut ls, a[9])),
        ("A #40 (newest)", place(&mut ls, a[19])),
        ("never issued", place(&mut ls, 4_000_000)),
    ];
    // parent-relative x by target, as in `r15_rule1_uses_the_causing_event_output`:
    // B (fallback) 0, C 240, A 390
    let want = [Some(0), Some(0), Some(240), Some(390), Some(390), Some(0)];
    for ((name, g), w) in got.iter().zip(want) {
        assert_eq!(*g, w, "{name}: provenance resolution");
    }
}
