//! Touch (`wl_touch`, Core's `touch`) with a real client: a finger on a window reaches it at its
//! surface-local position; the window keeps that finger as it moves, even past the window's
//! edge, until it lifts; a finger on empty canvas reaches no client; a tap focuses the window;
//! a cancel gives up the fingers; fingers on empty canvas drag it and pinch-zoom it; a second
//! finger takes the first from a window (cancelled there) and the two move the canvas; a
//! two-finger double tap recenters like Super+Space. The seat offers touch only once a
//! touchscreen is there.

mod common;

use common::Probe;
use common::input::*;
use infynitae_core::PhysicalEvent;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_touch::{self, WlTouch};
use wayland_client::{Connection, Dispatch, QueueHandle};

impl Dispatch<WlTouch, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &WlTouch,
        ev: wl_touch::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let label = |st: &Probe, s: &wayland_client::protocol::wl_surface::WlSurface| {
            st.surfaces
                .iter()
                .find(|p| p.wl == *s)
                .map_or("?".to_string(), |p| p.label.to_string())
        };
        let line = match ev {
            wl_touch::Event::Down {
                surface, id, x, y, ..
            } => {
                format!("down {id} {} {x:.1},{y:.1}", label(st, &surface))
            }
            wl_touch::Event::Motion { id, x, y, .. } => format!("motion {id} {x:.1},{y:.1}"),
            wl_touch::Event::Up { id, .. } => format!("up {id}"),
            wl_touch::Event::Frame => "frame".into(),
            wl_touch::Event::Cancel => "cancel".into(),
            _ => return,
        };
        st.log.push(line);
    }
}

/// W mapped and a touchscreen: the client's wl_touch.
fn rig() -> (Rig, WlTouch) {
    let mut r = Rig::x8();
    r.ls.state.enable_touch();
    r.ls.pump();
    let qh = r.ls.qh.clone();
    let seat: WlSeat = r.ls.probe.seat.clone().expect("a seat");
    let touch = seat.get_touch(&qh, ());
    r.ls.pump();
    r.ls.probe.log.clear();
    (r, touch)
}

fn touch(r: &mut Rig, ev: PhysicalEvent) {
    let port = &mut r.ls.input;
    r.ls.state
        .with_effects(|core, fx| core.physical_input(port, ev, fx))
        .expect("touch");
    r.ls.pump();
}

fn down(r: &mut Rig, slot: u32, p: (f64, f64)) {
    r.t += 1000;
    let output = r.ls.output();
    let time_us = r.t;
    touch(
        r,
        PhysicalEvent::TouchDown {
            slot,
            output,
            p,
            time_us,
        },
    );
    touch(r, PhysicalEvent::TouchFrame);
}

fn motion(r: &mut Rig, slot: u32, p: (f64, f64)) {
    r.t += 1000;
    let time_us = r.t;
    touch(r, PhysicalEvent::TouchMotion { slot, p, time_us });
    touch(r, PhysicalEvent::TouchFrame);
}

fn up(r: &mut Rig, slot: u32) {
    r.t += 1000;
    let time_us = r.t;
    touch(r, PhysicalEvent::TouchUp { slot, time_us });
    touch(r, PhysicalEvent::TouchFrame);
}

/// A point on W (output logical; found by hit-testing the screen) and where in W's surface it is.
fn on_w(r: &mut Rig) -> ((f64, f64), (f64, f64)) {
    let output = r.ls.output();
    let q = infynitae_core::geom::Q16_ONE as f64;
    for y in (0..1080).step_by(40) {
        for x in (0..1920).step_by(40) {
            let p = (f64::from(x), f64::from(y));
            let pt = infynitae_core::geom::OutputLogicalPoint { x: p.0, y: p.1 };
            if let Some(hit) = r.ls.state.core_mut().hit_test(output, pt) {
                // well inside, so a small motion stays on it
                let p = (p.0 + 20.0, p.1 + 20.0);
                let pt = infynitae_core::geom::OutputLogicalPoint { x: p.0, y: p.1 };
                let hit = r.ls.state.core_mut().hit_test(output, pt).unwrap_or(hit);
                return (p, (hit.local.x as f64 / q, hit.local.y as f64 / q));
            }
        }
    }
    panic!("W is on the screen");
}

#[test]
fn a_finger_on_a_window_reaches_it_and_stays_with_it_until_it_lifts() {
    let (mut r, _touch) = rig();
    let (p, local) = on_w(&mut r);
    down(&mut r, 0, p);
    assert!(
        r.ls.probe
            .log
            .contains(&format!("down 0 W {:.1},{:.1}", local.0, local.1)),
        "{:?}",
        r.ls.probe.log
    );
    assert!(r.ls.probe.log.contains(&"frame".to_string()));

    // moving 3000 px right: far past W's edge, still W's finger, in W's coordinates
    let zoom = r.ls.state.core().view(r.ls.output()).unwrap().zoom;
    r.ls.probe.log.clear();
    motion(&mut r, 0, (p.0 + 30.0, p.1 + 20.0));
    let want = (local.0 + 30.0 / zoom, local.1 + 20.0 / zoom);
    assert!(
        r.ls.probe
            .log
            .contains(&format!("motion 0 {:.1},{:.1}", want.0, want.1)),
        "zoom {zoom}: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    motion(&mut r, 0, (p.0 + 1500.0, p.1));
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("motion 0 ")),
        "outside W: still W's: {:?}",
        r.ls.probe.log
    );

    up(&mut r, 0);
    assert!(
        r.ls.probe.log.contains(&"up 0".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    // lifted: a motion in that slot goes nowhere
    r.ls.probe.log.clear();
    motion(&mut r, 0, p);
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("motion")),
        "{:?}",
        r.ls.probe.log
    );
}

#[test]
fn a_finger_on_empty_canvas_reaches_no_client() {
    let (mut r, _touch) = rig();
    down(&mut r, 1, (1800.0, 1000.0));
    motion(&mut r, 1, (1700.0, 900.0));
    up(&mut r, 1);
    assert!(
        !r.ls
            .probe
            .log
            .iter()
            .any(|l| l.starts_with("down") || l.starts_with("motion") || l.starts_with("up")),
        "{:?}",
        r.ls.probe.log
    );
}

#[test]
fn a_tap_focuses_the_window_and_a_cancel_gives_up_the_fingers() {
    let (mut r, _touch) = rig();
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, None)
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    assert_ne!(r.ls.probe.kb.active.as_deref(), Some("W"));
    let (p, _) = on_w(&mut r);
    down(&mut r, 0, p);
    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some("W"),
        "a tap is an explicit action: W has the keyboard"
    );
    touch(&mut r, PhysicalEvent::TouchCancel);
    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "{:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    motion(&mut r, 0, p);
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("motion")),
        "cancelled: nothing more: {:?}",
        r.ls.probe.log
    );
}

/// The camera of the rig's output: its centre (Canvas px) and zoom.
fn camera(r: &Rig) -> ((f64, f64), f64) {
    let core = r.ls.state.core();
    let cam = core
        .cameras()
        .camera_for_output(r.ls.output())
        .expect("a camera");
    let c = core.cameras().center(cam).expect("a centre");
    let q = infynitae_core::geom::Q16_ONE as f64;
    (
        (c.x as f64 / q, c.y as f64 / q),
        core.cameras().zoom(cam).expect("a zoom"),
    )
}

/// One finger on empty canvas drags it: the camera moves against the finger, the zoom stays, and
/// the lift ends the drag (a later finger elsewhere moves nothing until it moves itself).
#[test]
fn a_finger_on_empty_canvas_drags_the_canvas() {
    let (mut r, _touch) = rig();
    let ((x0, y0), z0) = camera(&r);
    down(&mut r, 1, (1800.0, 1000.0));
    motion(&mut r, 1, (1700.0, 900.0));
    let ((x, y), z) = camera(&r);
    assert_eq!(z, z0);
    assert!(
        (x - (x0 + 100.0 / z0)).abs() < 0.01 && (y - (y0 + 100.0 / z0)).abs() < 0.01,
        "the canvas followed the finger: {x0},{y0} -> {x},{y}"
    );
    up(&mut r, 1);
    down(&mut r, 2, (100.0, 100.0));
    assert_eq!(camera(&r), ((x, y), z), "a new finger alone moves nothing");
}

/// Two fingers on empty canvas pinch: twice their spread is twice the zoom, and the Canvas point
/// between them stays between them.
#[test]
fn two_fingers_on_empty_canvas_pinch_zoom_around_their_midpoint() {
    let (mut r, _touch) = rig();
    let ((x0, y0), z0) = camera(&r);
    let (mid, extent) = ((1700.0, 950.0), (1920.0, 1080.0));
    let under = |((x, y), z): ((f64, f64), f64)| {
        (
            x + (mid.0 - extent.0 / 2.0) / z,
            y + (mid.1 - extent.1 / 2.0) / z,
        )
    };
    let before = under(((x0, y0), z0));
    down(&mut r, 1, (1650.0, 950.0));
    down(&mut r, 2, (1750.0, 950.0));
    motion(&mut r, 1, (1600.0, 950.0));
    motion(&mut r, 2, (1800.0, 950.0));
    let after = camera(&r);
    assert!(
        (after.1 - 2.0 * z0).abs() < 1e-9,
        "zoom {z0} -> {}",
        after.1
    );
    let now = under(after);
    assert!(
        (now.0 - before.0).abs() < 0.01 && (now.1 - before.1).abs() < 0.01,
        "{before:?} -> {now:?}"
    );
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("down")),
        "no client saw the fingers: {:?}",
        r.ls.probe.log
    );
}

/// A second finger next to one on W: W's touch is cancelled, and the two pinch-zoom the canvas
/// (owner: "pinch to zoom even if the fingers are on an app"); W sees nothing more of them.
#[test]
fn a_second_finger_on_a_window_takes_both_to_the_canvas() {
    let (mut r, _touch) = rig();
    let (_, z0) = camera(&r);
    let (p, _) = on_w(&mut r);
    down(&mut r, 0, p);
    assert!(r.ls.probe.log.iter().any(|l| l.starts_with("down 0 W")));
    down(&mut r, 1, (p.0 + 100.0, p.1));
    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "W's touch is cancelled: {:?}",
        r.ls.probe.log
    );
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("down 1")),
        "the second finger is no touch for W: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    motion(&mut r, 0, (p.0 - 50.0, p.1));
    motion(&mut r, 1, (p.0 + 150.0, p.1));
    assert!(
        (camera(&r).1 - 2.0 * z0).abs() < 1e-9,
        "twice the spread, twice the zoom: {z0} -> {}",
        camera(&r).1
    );
    up(&mut r, 0);
    up(&mut r, 1);
    assert!(
        !r.ls
            .probe
            .log
            .iter()
            .any(|l| l.starts_with("motion") || l.starts_with("up")),
        "W saw nothing more: {:?}",
        r.ls.probe.log
    );
}

/// Two quick two-finger taps on W: W is focused and centred at zoom 1.0, as Super+Space does.
#[test]
fn a_two_finger_double_tap_on_a_window_recenters_it() {
    let (mut r, _touch) = rig();
    // zoomed out first (a pinch on empty canvas), and W not focused
    down(&mut r, 1, (1600.0, 950.0));
    down(&mut r, 2, (1800.0, 950.0));
    motion(&mut r, 1, (1650.0, 950.0));
    motion(&mut r, 2, (1750.0, 950.0));
    up(&mut r, 1);
    up(&mut r, 2);
    assert!(camera(&r).1 < 0.99, "zoomed out: {}", camera(&r).1);
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, None)
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    let (p, _) = on_w(&mut r);
    for _ in 0..2 {
        down(&mut r, 3, p);
        down(&mut r, 4, (p.0 + 10.0, p.1));
        up(&mut r, 3);
        up(&mut r, 4);
    }
    assert_eq!(camera(&r).1, 1.0, "zoom 1.0");
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("W"), "W focused");
}

/// The window the rig's fingers act on.
fn element(r: &Rig) -> infynitae_core::stage::ElementId {
    *r.ls.state.core().stage().mapped_elements().last().unwrap()
}

/// Three fingers on W, moved together: W follows them by their motion / zoom (the canvas, which
/// two fingers would pan, stays), W is focused, and W saw none of the fingers past the first.
#[test]
fn three_fingers_on_a_window_move_it() {
    let (mut r, _touch) = rig();
    let w = element(&r);
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, None)
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    let (p, _) = on_w(&mut r);
    let fingers = [
        (0, (p.0, p.1)),
        (1, (p.0 + 20.0, p.1)),
        (2, (p.0 + 10.0, p.1 + 15.0)),
    ];
    for (s, at) in fingers {
        down(&mut r, s, at);
    }
    let (c0, z) = camera(&r);
    let a0 = r.ls.state.core().stage().anchor(w).unwrap();
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("W"), "W focused");
    r.ls.probe.log.clear();
    for step in 1..=4 {
        for (s, at) in fingers {
            motion(
                &mut r,
                s,
                (at.0 + 30.0 * f64::from(step), at.1 + 10.0 * f64::from(step)),
            );
        }
    }
    let a = r.ls.state.core().stage().anchor(w).unwrap();
    let want = ((120.0 / z).round() as i64, (40.0 / z).round() as i64);
    assert!(
        ((a.x - a0.x) - want.0).abs() <= 1 && ((a.y - a0.y) - want.1).abs() <= 1,
        "W moved {:?}, want {want:?} (zoom {z})",
        (a.x - a0.x, a.y - a0.y)
    );
    assert_eq!(camera(&r), (c0, z), "the canvas stayed");
    for (s, _) in fingers {
        up(&mut r, s);
    }
    assert!(
        !r.ls
            .probe
            .log
            .iter()
            .any(|l| l.starts_with("motion") || l.starts_with("up") || l.starts_with("down")),
        "W saw nothing of the gesture: {:?}",
        r.ls.probe.log
    );
    // over: one finger on W is W's again
    let (p, _) = on_w(&mut r);
    down(&mut r, 5, p);
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 5 W")),
        "{:?}",
        r.ls.probe.log
    );
}

/// xdg_toplevel state `resizing`.
const RESIZING: u32 = 3;

/// Three fingers on a 400 × 300 window spread 100 px wider and 60 px taller: it is asked for that
/// much more (at zoom 1), `resizing`, one configure at a time; the last lift sends the last size
/// without `resizing`, and the window's top-left corner stays.
#[test]
fn three_fingers_spread_on_a_window_resize_it() {
    let mut r = Rig::new(common::Lockstep::new(Some(a_1080())));
    r.ls.state.enable_touch();
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, "R", (400, 300), 0);
    r.ls.pump();
    r.frame();
    let w = element(&r);
    let start = r.ls.state.core().stage().anchor(w).unwrap();
    let z = camera(&r).1;
    // the window is centred on the view (960, 540)
    let fingers = [
        (0, (900.0, 500.0)),
        (1, (1000.0, 500.0)),
        (2, (950.0, 560.0)),
    ];
    for (s, at) in fingers {
        down(&mut r, s, at);
    }
    r.ls.probe.toplevel_configures.clear();
    // fingers 0 and 1 spread 50 px each sideways, finger 2 goes 60 px down
    motion(&mut r, 0, (850.0, 500.0));
    motion(&mut r, 1, (1050.0, 500.0));
    motion(&mut r, 2, (950.0, 620.0));
    let asked =
        r.ls.probe
            .toplevel_configures
            .first()
            .cloned()
            .expect("a configure");
    assert!(asked.2.contains(&RESIZING), "{:?}", asked.2);
    // the client draws what it was last asked for
    let last = r.ls.probe.toplevel_configures.last().cloned().unwrap();
    r.ls.probe.surfaces[idx].size = last.1;
    r.ls.probe.draw(&qh, idx);
    r.ls.pump();
    r.frame();
    let want = (
        400 + (100.0 / z).round() as i32,
        300 + (60.0 / z).round() as i32,
    );
    let now = r.ls.probe.toplevel_configures.last().cloned().unwrap();
    assert_eq!(now.1, want, "the spread's size (zoom {z})");
    for (s, _) in fingers {
        up(&mut r, s);
    }
    let fin = r.ls.probe.toplevel_configures.last().cloned().unwrap();
    assert_eq!(fin.1, want, "the last size, once more");
    assert!(!fin.2.contains(&RESIZING), "without resizing: {:?}", fin.2);
    r.ls.probe.surfaces[idx].size = want;
    r.ls.probe.draw(&qh, idx);
    r.ls.pump();
    r.frame();
    assert_eq!(
        r.ls.state.core().stage().anchor(w),
        Some(start),
        "the top-left corner stayed"
    );
}

/// A portrait 800×1280 panel rotated Right Side Up (`Transform::R270`) at 1.5× scale (`180/120`):
/// the view is 1280×800, and a touch in physical panel coordinates mapped through
/// `panel_point_to_view` lands at the exact surface-local coordinates on the window.
#[test]
fn a_touch_on_a_rotated_panel_maps_through_the_output_transform() {
    use infynitae_core::output::Transform;
    let mut out = common::output((800, 1280), 180);
    out.transform = Transform::R270;
    let mut r = Rig::new(common::Lockstep::new(Some(out)));
    r.ls.state.enable_touch();
    r.ls.pump();
    let qh = r.ls.qh.clone();
    let seat: WlSeat = r.ls.probe.seat.clone().expect("a seat");
    let _touch = seat.get_touch(&qh, ());
    r.ls.pump();
    r.map_w();

    let output = r.ls.output();
    let model = r.ls.state.core().output(output).unwrap().clone();
    assert_eq!(model.view_size(), (1280, 800));

    // Pick an asymmetric physical point on the 800×1280 panel that maps inside W on the 1280×800
    // view: for R270, panel (px, py) maps to view (py, 800 - px).
    let panel_px = (300.0, 600.0);
    let view_px =
        infynitae_wayland::convert::panel_point_to_view(panel_px, (800, 1280), model.transform);
    assert_eq!(view_px, (600.0, 500.0));

    let logical = (view_px.0 / model.scale, view_px.1 / model.scale);
    let pt = infynitae_core::geom::OutputLogicalPoint {
        x: logical.0,
        y: logical.1,
    };
    let hit =
        r.ls.state
            .core_mut()
            .hit_test(output, pt)
            .expect("center of rotated view hits W");
    let q = infynitae_core::geom::Q16_ONE as f64;
    let local = (hit.local.x as f64 / q, hit.local.y as f64 / q);

    r.ls.probe.log.clear();
    down(&mut r, 0, logical);
    assert!(
        r.ls.probe
            .log
            .contains(&format!("down 0 W {:.1},{:.1}", local.0, local.1)),
        "{:?}",
        r.ls.probe.log
    );
    up(&mut r, 0);
}

#[test]
fn touch_on_rotated_and_scaled_second_output() {
    use infynitae_core::output::Transform;

    let o1 = common::output_k("DP-1", (1920, 1080), 120, 60_000);
    let mut o2 = common::output_k("HDMI-A-1", (1920, 1080), 180, 60_000); // 1.5x scale
    o2.transform = Transform::R90; // logical extent: 1080/1.5 = 720 x 1920/1.5 = 1280
    let mut ls = common::Lockstep::with(Default::default(), &[o1, o2]);
    let qh = ls.qh.clone();
    // Place W on the second output by locating the pointer on it first, or hit-test on output 2.
    let out2 = ls.state.core().outputs()[1].id;
    let w_idx = ls.probe.toplevel(&qh, "W2", (400, 300), 0);
    ls.pump();
    ls.probe.draw(&qh, w_idx);
    ls.pump();
    // Move W2 onto output 2's view center.
    let w_id = *ls.state.core().stage().mapped_elements().last().unwrap();
    let cam2 = ls.state.core().cameras().camera_for_output(out2).unwrap();
    let c2 = ls.state.core().cameras().center(cam2).unwrap();
    let q = infynitae_core::geom::Q16_ONE;
    let target_anchor = ((c2.x / q) as f64 - 200.0, (c2.y / q) as f64 - 150.0);
    let eref = infynitae_core::stage::ElementRef {
        instance: ls.state.core().stage().instance(),
        element: w_id,
    };
    let fx = ls
        .state
        .core_mut()
        .shell_command(
            &mut ls.shell,
            infynitae_core::ShellCommand::MoveElement {
                element: eref,
                to: target_anchor,
                stream: infynitae_core::ShellStream::Atomic,
            },
        )
        .unwrap();
    ls.state.apply(&fx);
    ls.pump();
    let mut r = Rig::new(ls);
    r.frame();

    r.ls.state.enable_touch();
    r.ls.pump();
    let seat: WlSeat = r.ls.probe.seat.clone().expect("seat");
    let _touch = seat.get_touch(&qh, ());
    r.ls.pump();
    r.ls.probe.log.clear();

    // Touch at logical center of rotated/scaled output 2: (360, 640).
    r.t += 1000;
    let t1 = r.t;
    touch(
        &mut r,
        PhysicalEvent::TouchDown {
            slot: 0,
            output: out2,
            p: (360.0, 640.0),
            time_us: t1,
        },
    );
    touch(&mut r, PhysicalEvent::TouchFrame);
    assert!(
        r.ls.probe.log.iter().any(|l| l == "down 0 W2 200.0,150.0"),
        "expected touch on W2 at 200.0,150.0: {:?}",
        r.ls.probe.log
    );
    assert!(r.ls.probe.log.contains(&"frame".to_string()));

    r.ls.probe.log.clear();
    r.t += 1000;
    let t2 = r.t;
    touch(
        &mut r,
        PhysicalEvent::TouchMotion {
            slot: 0,
            p: (390.0, 660.0),
            time_us: t2,
        },
    );
    touch(&mut r, PhysicalEvent::TouchFrame);
    assert!(
        r.ls.probe.log.iter().any(|l| l == "motion 0 230.0,170.0"),
        "expected motion on W2 at 230.0,170.0: {:?}",
        r.ls.probe.log
    );
    assert!(r.ls.probe.log.contains(&"frame".to_string()));

    r.ls.probe.log.clear();
    r.t += 1000;
    let t3 = r.t;
    touch(
        &mut r,
        PhysicalEvent::TouchUp {
            slot: 0,
            time_us: t3,
        },
    );
    touch(&mut r, PhysicalEvent::TouchFrame);
    assert!(r.ls.probe.log.contains(&"up 0".to_string()));
    assert!(r.ls.probe.log.contains(&"frame".to_string()));
}

#[test]
fn finger_down_when_window_unmaps_or_closes_cancels_touch() {
    let mut r = Rig::new(common::Lockstep::new(Some(a_1080())));
    r.ls.state.enable_touch();
    r.ls.pump();
    let qh = r.ls.qh.clone();
    let seat: WlSeat = r.ls.probe.seat.clone().expect("seat");
    let _touch = seat.get_touch(&qh, ());
    let w1 = r.ls.probe.toplevel(&qh, "W1", (400, 300), 0);
    let w2 = r.ls.probe.toplevel(&qh, "W2", (400, 300), 0);
    r.ls.pump();
    r.ls.probe.draw(&qh, w1);
    r.ls.probe.draw(&qh, w2);
    r.ls.pump();
    r.frame();

    // Place W1 left of center and W2 right of center so both are visible and disjoint.
    let out = r.ls.output();
    let cam = r.ls.state.core().cameras().camera_for_output(out).unwrap();
    let center = r.ls.state.core().cameras().center(cam).unwrap();
    let q = infynitae_core::geom::Q16_ONE as f64;
    let cx = center.x as f64 / q;
    let cy = center.y as f64 / q;
    let elems: Vec<_> = r.ls.state.core().stage().mapped_elements().to_vec();
    for (id, pos) in [
        (elems[0], (cx - 500.0, cy - 150.0)),
        (elems[1], (cx + 100.0, cy - 150.0)),
    ] {
        let eref = infynitae_core::stage::ElementRef {
            instance: r.ls.state.core().stage().instance(),
            element: id,
        };
        let fx =
            r.ls.state
                .core_mut()
                .shell_command(
                    &mut r.ls.shell,
                    infynitae_core::ShellCommand::MoveElement {
                        element: eref,
                        to: pos,
                        stream: infynitae_core::ShellStream::Atomic,
                    },
                )
                .unwrap();
        r.ls.state.apply(&fx);
    }
    r.ls.pump();
    r.frame();

    // Find logical point on W1 and W2.
    let find_on = |r: &mut Rig, target_id| -> (f64, f64) {
        let out = r.ls.output();
        for y in (0..1080).step_by(40) {
            for x in (0..1920).step_by(40) {
                let pt = infynitae_core::geom::OutputLogicalPoint {
                    x: f64::from(x) + 10.0,
                    y: f64::from(y) + 10.0,
                };
                if r.ls
                    .state
                    .core_mut()
                    .hit_test(out, pt)
                    .is_some_and(|h| h.element == target_id)
                {
                    return (pt.x, pt.y);
                }
            }
        }
        panic!("target not on screen");
    };
    let p1 = find_on(&mut r, elems[0]);
    let p2 = find_on(&mut r, elems[1]);

    // 1. Finger 0 down on W1, then W1 unmaps -> W1 gets cancel and slot 1 on W2 goes to W2.
    r.ls.probe.log.clear();
    down(&mut r, 0, p1);
    assert!(r.ls.probe.log.iter().any(|l| l.starts_with("down 0 W1")));
    r.ls.probe.log.clear();
    r.ls.probe.unmap(w1);
    r.ls.pump();
    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "unmapping W1 should cancel touch on W1: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    down(&mut r, 1, p2);
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 1 W2")),
        "new finger on W2 after W1 unmapped should reach W2: {:?}",
        r.ls.probe.log
    );

    // 2. Finger 1 is now down on W2, destroy W2 -> touch cleared and a new finger on W3 works.
    r.ls.probe.destroy_toplevel(w2);
    r.ls.pump();
    let w3 = r.ls.probe.toplevel(&qh, "W3", (400, 300), 0);
    r.ls.pump();
    r.ls.probe.draw(&qh, w3);
    r.ls.pump();
    r.frame();
    let w3_id = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    r.ls.probe.log.clear();
    let p3 = find_on(&mut r, w3_id);
    down(&mut r, 2, p3);
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 2 W3")),
        "new finger on W3 after W2 destroyed should reach W3: {:?}",
        r.ls.probe.log
    );
}

#[test]
fn finger_down_when_panel_opens_or_closes_cancels_touch() {
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
        Anchor, KeyboardInteractivity,
    };

    let (mut r, _touch) = rig();
    r.ls.bind_layer_shell();
    r.ls.pump();
    let qh = r.ls.qh.clone();
    let (p, _) = on_w(&mut r);

    // Finger 0 down on window W.
    down(&mut r, 0, p);
    assert!(r.ls.probe.log.iter().any(|l| l.starts_with("down 0 W")));
    r.ls.probe.log.clear();

    // An exclusive top-layer panel opens.
    let panel = r.ls.probe.layer_surface(
        &qh,
        "PANEL",
        Layer::Top,
        Anchor::Top | Anchor::Left | Anchor::Right,
        (0, 200),
        0,
        KeyboardInteractivity::Exclusive,
    );
    r.ls.pump();
    r.ls.probe.draw(&qh, panel);
    r.ls.pump();

    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "opening exclusive panel should cancel W's active touch: {:?}",
        r.ls.probe.log
    );

    // Finger 1 taps on the panel -> reaches PANEL (not absorbed by W's stale touch).
    r.ls.probe.log.clear();
    down(&mut r, 1, (100.0, 50.0));
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 1 PANEL")),
        "tap on open panel should reach PANEL: {:?}",
        r.ls.probe.log
    );

    // Panel unmaps while finger 1 is down on it -> PANEL gets cancel, next tap reaches W.
    r.ls.probe.log.clear();
    r.ls.probe.unmap(panel);
    r.ls.pump();
    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "unmapping panel while finger is down should cancel touch: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    down(&mut r, 2, p);
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 2 W")),
        "tap on W after panel closed should reach W: {:?}",
        r.ls.probe.log
    );
}

#[test]
fn finger_down_when_output_rotates_cancels_panel_touch() {
    use infynitae_core::output::Transform;
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
        Anchor, KeyboardInteractivity,
    };

    let (mut r, _touch) = rig();
    r.ls.bind_layer_shell();
    r.ls.pump();
    let qh = r.ls.qh.clone();

    let panel = r.ls.probe.layer_surface(
        &qh,
        "PANEL",
        Layer::Top,
        Anchor::Bottom | Anchor::Left | Anchor::Right,
        (0, 120),
        0,
        KeyboardInteractivity::None,
    );
    r.ls.pump();
    r.ls.probe.draw(&qh, panel);
    r.ls.pump();

    r.ls.probe.log.clear();
    down(&mut r, 0, (200.0, 1000.0));
    assert!(r.ls.probe.log.iter().any(|l| l.starts_with("down 0 PANEL")));

    // Autorotate turns the output to 90 degrees while finger 0 is down on the panel.
    r.ls.probe.log.clear();
    let out = r.ls.output();
    let mut rotated = a_1080();
    rotated.transform = Transform::R90;
    r.ls.reconfigure(out, &rotated);
    r.ls.pump();

    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "rotating output while finger is down on panel should cancel touch: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    motion(&mut r, 0, (220.0, 1000.0));
    up(&mut r, 0);
    assert!(
        !r.ls
            .probe
            .log
            .iter()
            .any(|l| l.starts_with("motion") || l.starts_with("up")),
        "cancelled finger should send no motion/up after rotation: {:?}",
        r.ls.probe.log
    );
}

#[test]
fn finger_down_when_fullscreen_starts_cancels_touch() {
    let mut r = Rig::new(common::Lockstep::new(Some(a_1080())));
    let qh = r.ls.qh.clone();
    let w = r.ls.probe.toplevel(&qh, "W", (800, 600), 0);
    r.ls.pump();
    r.ls.probe.draw(&qh, w);
    r.ls.pump();
    r.frame();
    r.ls.state.enable_touch();
    r.ls.pump();
    let seat: WlSeat = r.ls.probe.seat.clone().expect("seat");
    let _touch = seat.get_touch(&qh, ());
    r.ls.pump();
    let (p, _) = on_w(&mut r);

    down(&mut r, 0, p);
    assert!(r.ls.probe.log.iter().any(|l| l.starts_with("down 0 W")));

    r.ls.probe.log.clear();
    // Ask W to go fullscreen via its xdg_toplevel, ack configure, and commit.
    r.ls.probe.surfaces[w]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    r.ls.pump();
    let last = r.ls.probe.toplevel_configures.last().cloned().unwrap();
    r.ls.probe.surfaces[w].size = last.1;
    r.ls.probe.draw(&qh, w);
    r.ls.pump();
    r.frame();

    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "entering fullscreen while finger is down should cancel touch: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    down(&mut r, 1, (500.0, 500.0));
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 1 W")),
        "new finger on fullscreen W should reach W: {:?}",
        r.ls.probe.log
    );
}

#[test]
fn stationary_finger_cancel_reaches_client() {
    let (mut r, _touch) = rig();
    let (p, _) = on_w(&mut r);

    down(&mut r, 0, p);
    assert!(r.ls.probe.log.iter().any(|l| l.starts_with("down 0 W")));
    assert!(r.ls.probe.log.contains(&"frame".to_string()));

    r.ls.probe.log.clear();
    touch(&mut r, PhysicalEvent::TouchCancel);
    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "TouchCancel on stationary finger must reach client: {:?}",
        r.ls.probe.log
    );
}

#[test]
fn edge_swipe_from_screen_edges_opens_hover_strip_and_cancels_touch() {
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
        Anchor, KeyboardInteractivity,
    };

    let (mut r, _touch) = rig();
    r.ls.bind_layer_shell();
    r.ls.pump();
    let qh = r.ls.qh.clone();

    let strip_top = r.ls.probe.layer_surface(
        &qh,
        "STRIP_TOP",
        Layer::Overlay,
        Anchor::Top,
        (400, 2),
        0,
        KeyboardInteractivity::None,
    );
    let strip_bot = r.ls.probe.layer_surface(
        &qh,
        "STRIP_BOT",
        Layer::Overlay,
        Anchor::Bottom,
        (400, 2),
        0,
        KeyboardInteractivity::None,
    );
    let strip_left = r.ls.probe.layer_surface(
        &qh,
        "STRIP_LEFT",
        Layer::Overlay,
        Anchor::Left,
        (2, 400),
        0,
        KeyboardInteractivity::None,
    );
    let strip_right = r.ls.probe.layer_surface(
        &qh,
        "STRIP_RIGHT",
        Layer::Overlay,
        Anchor::Right,
        (2, 400),
        0,
        KeyboardInteractivity::None,
    );
    r.ls.pump();
    for s in [strip_top, strip_bot, strip_left, strip_right] {
        r.ls.probe.draw(&qh, s);
    }
    r.ls.pump();

    for (start, step, strip_label) in [
        ((960.0, 10.0), (960.0, 48.0), "STRIP_TOP"),
        ((960.0, 1070.0), (960.0, 1032.0), "STRIP_BOT"),
        ((10.0, 540.0), (48.0, 540.0), "STRIP_LEFT"),
        ((1910.0, 540.0), (1872.0, 540.0), "STRIP_RIGHT"),
    ] {
        // Put pointer in the middle before each swipe.
        let _ = infynitae_backend_headless::pointer_motion(
            &mut r.ls.state,
            &mut r.ls.input,
            (0.0, 0.0),
            (0.0, 0.0),
            r.t + 1,
        );
        r.t += 1000;
        r.ls.probe.log.clear();
        r.ls.probe.ptr.clear();

        down(&mut r, 0, start);
        motion(&mut r, 0, step);
        up(&mut r, 0);

        assert!(
            r.ls.probe
                .ptr
                .iter()
                .any(|l| l.starts_with(&format!("enter {strip_label} "))),
            "inward swipe from {start:?} -> {step:?} should move pointer onto {strip_label}: ptr={:?} log={:?}",
            r.ls.probe.ptr,
            r.ls.probe.log
        );
    }

    // Swiping in while a window is under the finger cancels the window's touch too.
    let (p_top_on_w, _) = on_w(&mut r);
    // W covers 560..1360 x 240..840 in Rig::x8, so resize/reposition or map a near-top strip test:
    // Tap at (960, 250) isn't near edge, so move pointer out and map a near-edge window.
    let w_top = r.ls.probe.toplevel(&qh, "WTOP", (1800, 1070), 0);
    r.ls.pump();
    r.ls.probe.draw(&qh, w_top);
    r.ls.pump();
    r.frame();
    let _ = p_top_on_w;
    r.ls.probe.log.clear();
    down(&mut r, 0, (960.0, 12.0));
    assert!(
        r.ls.probe.log.iter().any(|l| l.starts_with("down 0 WTOP")),
        "initial finger near edge starts on WTOP: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.clear();
    r.ls.probe.ptr.clear();
    motion(&mut r, 0, (960.0, 50.0));
    up(&mut r, 0);
    assert!(
        r.ls.probe.log.contains(&"cancel".to_string()),
        "inward edge swipe should cancel WTOP's touch: {:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe
            .ptr
            .iter()
            .any(|l| l.starts_with("enter STRIP_TOP ")),
        "inward edge swipe over WTOP should enter STRIP_TOP: {:?}",
        r.ls.probe.ptr
    );
}

#[test]
fn edge_swipe_ignores_parallel_drag_and_open_panel() {
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
        Anchor, KeyboardInteractivity,
    };

    let (mut r, _touch) = rig();
    r.ls.bind_layer_shell();
    r.ls.pump();
    let qh = r.ls.qh.clone();

    let strip_top = r.ls.probe.layer_surface(
        &qh,
        "STRIP_TOP",
        Layer::Overlay,
        Anchor::Top,
        (400, 2),
        0,
        KeyboardInteractivity::None,
    );
    let open_bot = r.ls.probe.layer_surface(
        &qh,
        "OPEN_BOT",
        Layer::Top,
        Anchor::Bottom | Anchor::Left | Anchor::Right,
        (0, 200),
        0,
        KeyboardInteractivity::None,
    );
    r.ls.pump();
    r.ls.probe.draw(&qh, strip_top);
    r.ls.probe.draw(&qh, open_bot);
    r.ls.pump();

    // 1. Parallel drag along top edge does not open STRIP_TOP.
    r.ls.probe.log.clear();
    r.ls.probe.ptr.clear();
    down(&mut r, 0, (960.0, 10.0));
    motion(&mut r, 0, (1010.0, 10.0));
    up(&mut r, 0);
    assert!(
        !r.ls
            .probe
            .ptr
            .iter()
            .any(|l| l.starts_with("enter STRIP_TOP")),
        "parallel drag along edge must not trigger edge swipe: {:?}",
        r.ls.probe.ptr
    );

    // 2. Touch starting inside an open panel/OSK near the bottom edge stays with that layer surface.
    r.ls.probe.log.clear();
    r.ls.probe.ptr.clear();
    down(&mut r, 0, (500.0, 1068.0));
    motion(&mut r, 0, (500.0, 1020.0));
    up(&mut r, 0);
    assert!(
        r.ls.probe
            .log
            .iter()
            .any(|l| l.starts_with("down 0 OPEN_BOT"))
            && r.ls.probe.log.iter().any(|l| l.starts_with("motion 0"))
            && !r.ls.probe.log.contains(&"cancel".to_string()),
        "finger on an open bottom layer/OSK must not trigger edge swipe cancel: {:?}",
        r.ls.probe.log
    );

    // 3. TOUCH_EDGE_SWIPE="false" disables edge swipe and removing the line reverts to true.
    let home = infynitae_wayland::cursor::TestTempDir::new("touch-edge-swipe");
    let cfg_dir = home.path().join(".config/infynitae");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("config"), "TOUCH_EDGE_SWIPE=\"false\"\n").unwrap();
    r.ls.state.set_config_home(home.path().to_path_buf());
    r.ls.state.reload_config();
    assert!(!r.ls.state.core().touch_edge_swipe());

    r.ls.probe.log.clear();
    r.ls.probe.ptr.clear();
    down(&mut r, 0, (960.0, 10.0));
    motion(&mut r, 0, (960.0, 50.0));
    up(&mut r, 0);
    assert!(
        !r.ls
            .probe
            .ptr
            .iter()
            .any(|l| l.starts_with("enter STRIP_TOP")),
        "TOUCH_EDGE_SWIPE=\"false\" must disable edge swipe: {:?}",
        r.ls.probe.ptr
    );

    std::fs::write(cfg_dir.join("config"), "").unwrap();
    r.ls.state.reload_config();
    assert!(
        r.ls.state.core().touch_edge_swipe(),
        "removing TOUCH_EDGE_SWIPE must revert to default true"
    );
}
