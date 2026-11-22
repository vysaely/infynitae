//! Super+G (2026-11-21): the focused window's cluster tidied, then in a grid, then in columns, at
//! each press, with real clients: the windows move, sizes are asked by configure, a client that
//! keeps its size is centred in its cell, and the camera frames the tidied cluster.

mod common;

use common::Lockstep;
use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::ShellCommand;
use infynitae_core::input::keys::{KEY_G, KEY_LEFTMETA};
use infynitae_core::stage::{ElementId, ElementRef};

fn open(r: &mut Rig, label: &'static str, size: (i32, i32)) -> (ElementId, usize) {
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, label, size, 0);
    r.ls.pump();
    r.frame();
    let id = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    (id, idx)
}

fn rect(r: &Rig, id: ElementId) -> (i64, i64, i32, i32) {
    let core = r.ls.state.core();
    let a = core.stage().anchor(id).unwrap();
    let s = core.stage().tree(id).unwrap().geometry.size;
    (a.x, a.y, s.w, s.h)
}

fn super_g(r: &mut Rig) {
    for (key, pressed) in [
        (KEY_LEFTMETA, true),
        (KEY_G, true),
        (KEY_G, false),
        (KEY_LEFTMETA, false),
    ] {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    }
    r.frame();
}

/// The last size each surface was asked for by a configure since `from`.
fn asked_since(r: &Rig, from: usize, idx: usize) -> Option<(i32, i32)> {
    r.ls.probe.toplevel_configures[from..]
        .iter()
        .rev()
        .find(|(i, ..)| *i == idx)
        .map(|(_, size, _)| *size)
}

#[test]
fn super_g_cycles_tidy_grid_and_columns() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    // four windows opened centred in the view (the policy here): one over the other
    let (a, ia) = open(&mut r, "A", (500, 400));
    let (b, ib) = open(&mut r, "B", (400, 300));
    let (c, ic) = open(&mut r, "C", (300, 200));
    let (d, id) = open(&mut r, "D", (200, 150));
    assert_eq!(rect(&r, a), (-250, -200, 500, 400));
    assert_eq!(rect(&r, d), (-100, -75, 200, 150));
    let element = ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: a,
    };
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(&mut r.ls.shell, ShellCommand::Focus { element })
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();

    // tidy: one row from A's corner, 12 px apart, sizes kept; the camera frames the row
    let from = r.ls.probe.toplevel_configures.len();
    super_g(&mut r);
    assert_eq!(rect(&r, a), (-250, -200, 500, 400));
    assert_eq!(rect(&r, b), (262, -200, 400, 300));
    assert_eq!(rect(&r, c), (674, -200, 300, 200));
    assert_eq!(rect(&r, d), (986, -200, 200, 150));
    for i in [ia, ib, ic, id] {
        let asked = asked_since(&r, from, i);
        assert!(
            asked.is_none_or(|s| s == (0, 0)),
            "tidy asked {i} for {asked:?}"
        );
    }
    for _ in 0..240 {
        if !r.ls.state.core().zoom_gliding() {
            break;
        }
        r.frame();
    }
    let camera = r.ls.state.core().cameras();
    let (centre, zoom) = (camera.center(0).unwrap(), camera.zoom(0).unwrap());
    // the row spans x -250..1186, y -200..200: its centre, at zoom 1
    assert_eq!((centre.x >> 16, centre.y >> 16, zoom), (468, 0, 1.0));

    // grid: 2 x 2 cells of 942 x 522; the clients are asked for them and, until they draw it,
    // centred in their cells; B draws the cell's size and fills its cell
    let from = r.ls.probe.toplevel_configures.len();
    super_g(&mut r);
    for i in [ia, ib, ic, id] {
        assert_eq!(asked_since(&r, from, i), Some((942, 522)));
    }
    assert_eq!(rect(&r, a), (-250 + 221, -200 + 61, 500, 400));
    assert_eq!(rect(&r, c), (-250 + 321, 334 + 161, 300, 200));
    let qh = r.ls.qh.clone();
    r.ls.probe.surfaces[ib].size = (942, 522);
    r.ls.probe.draw(&qh, ib);
    r.ls.pump();
    r.frame();
    assert_eq!(rect(&r, b), (704, -200, 942, 522));

    // columns: side by side from the corner the windows have now (A's x, B's y), the usable
    // height, in reading order
    let from = r.ls.probe.toplevel_configures.len();
    super_g(&mut r);
    for i in [ia, ib, ic, id] {
        assert_eq!(asked_since(&r, from, i), Some((465, 1056)));
    }
    let xs: Vec<i64> = [a, b, c, d].iter().map(|&e| rect(&r, e).0).collect();
    assert_eq!(
        xs,
        vec![
            -29 + (465 - 500) / 2,
            448 + (465 - 942) / 2,
            925 + 82,
            1402 + 132
        ]
    );
}
