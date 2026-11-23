//! Cluster layouts (2026-11-23; `infynitae msg arrange`, no default key): the focused window's
//! cluster in a spiral around it, then in tiles, then in a line, with real clients: the windows
//! move, sizes are asked by configure for tiles and the line, and the spiral keeps every size and
//! the focused window's place.

mod common;

use common::Lockstep;
use common::input::*;
use infynitae_core::ShellCommand;
use infynitae_core::arrange::ClusterLayout;
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

fn arrange(r: &mut Rig, layout: ClusterLayout) {
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::ArrangeCluster {
                    element: None,
                    layout,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
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
fn arrange_spiral_tiles_and_line() {
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

    // spiral: A stays, the others around it at their own sizes, 12 px apart at least
    let from = r.ls.probe.toplevel_configures.len();
    arrange(&mut r, ClusterLayout::Spiral);
    assert_eq!(rect(&r, a), (-250, -200, 500, 400));
    let rects: Vec<_> = [a, b, c, d].iter().map(|&e| rect(&r, e)).collect();
    for (k, x) in rects.iter().enumerate() {
        for y in &rects[k + 1..] {
            let apart = x.0 + i64::from(x.2) + 12 <= y.0
                || y.0 + i64::from(y.2) + 12 <= x.0
                || x.1 + i64::from(x.3) + 12 <= y.1
                || y.1 + i64::from(y.3) + 12 <= x.1;
            assert!(apart, "{x:?} {y:?}");
        }
    }
    for i in [ia, ib, ic, id] {
        let asked = asked_since(&r, from, i);
        assert!(
            asked.is_none_or(|s| s == (0, 0)),
            "spiral asked {i} for {asked:?}"
        );
    }

    // tiles: the usable view (1896 x 1056) in quarters of 942 x 522, each asked by configure
    let from = r.ls.probe.toplevel_configures.len();
    arrange(&mut r, ClusterLayout::Tiles);
    for i in [ia, ib, ic, id] {
        assert_eq!(asked_since(&r, from, i), Some((942, 522)));
    }

    // the clients draw their tiles' size, as apps do (the cluster stays one: 12 px apart)
    let qh = r.ls.qh.clone();
    for i in [ia, ib, ic, id] {
        r.ls.probe.surfaces[i].size = (942, 522);
        r.ls.probe.draw(&qh, i);
    }
    r.ls.pump();
    r.frame();
    for e in [a, b, c, d] {
        let (_, _, w, h) = rect(&r, e);
        assert_eq!((w, h), (942, 522));
    }

    // line: each asked for the usable height and the width of its shape at that height
    let before: Vec<_> = [a, b, c, d].iter().map(|&e| rect(&r, e)).collect();
    let from = r.ls.probe.toplevel_configures.len();
    arrange(&mut r, ClusterLayout::Line);
    for (k, i) in [ia, ib, ic, id].into_iter().enumerate() {
        let (_, _, w, h) = before[k];
        let want = ((i64::from(w) * 1056 / i64::from(h)).max(64) as i32, 1056);
        assert_eq!(asked_since(&r, from, i), Some(want), "{i}");
    }
}
