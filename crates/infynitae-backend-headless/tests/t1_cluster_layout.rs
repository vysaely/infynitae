//! Cluster grid (owner, 2026-11-28; `infynitae msg arrange grid`, `BIND_CLUSTER_LAYOUT`, no
//! default key): the focused window's cluster n to a row, n = ceil(sqrt(k)), with real clients.
//! Every window keeps its size (no configure asks for another), the focused window comes first
//! and the others follow in reading order. Packed: in a row each window at its own width, 12 px
//! after the one before; rows 12 px apart, as high as their tallest window, each window at its
//! row's top.

mod common;

use common::Lockstep;
use common::input::*;
use infynitae_core::ShellCommand;
use infynitae_core::arrange::ClusterLayout;
use infynitae_core::stage::{ElementId, ElementRef};

/// Ten sizes, every width different (the reading order of windows sharing a centre is then
/// left to right, the widest first), even so that centring rounds nothing.
const SIZES: [(i32, i32); 10] = [
    (640, 480),
    (500, 400),
    (420, 300),
    (360, 240),
    (300, 500),
    (260, 200),
    (220, 340),
    (180, 160),
    (160, 280),
    (120, 100),
];
const LABELS: [&str; 10] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

fn open(r: &mut Rig, label: &'static str, size: (i32, i32)) -> (ElementId, usize) {
    let qh = r.ls.qh.clone();
    let idx = r.ls.probe.toplevel(&qh, label, size, 0);
    r.ls.pump();
    r.frame();
    let id = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    (id, idx)
}

fn rect(r: &Rig, id: ElementId) -> (i64, i64, i64, i64) {
    let core = r.ls.state.core();
    let a = core.stage().anchor(id).unwrap();
    let s = core.stage().tree(id).unwrap().geometry.size;
    (a.x, a.y, i64::from(s.w), i64::from(s.h))
}

fn shell(r: &mut Rig, cmd: ShellCommand) {
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(&mut r.ls.shell, cmd)
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();
}

/// `sizes` opened centred in the view (the policy here), one over the other; the one at `lead`
/// focused and the grid run. Checks no window is resized or asked for another size, and that
/// the windows, in the order the owner's rule gives (the focused one first, the others in
/// reading order), are packed n to a row from the cluster's top-left corner. Returns the
/// labels in that order.
fn grid_of_sizes(sizes: &[(i32, i32)], lead: usize) -> Vec<&'static str> {
    let k = sizes.len();
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    let wins: Vec<(ElementId, usize)> = (0..k).map(|i| open(&mut r, LABELS[i], sizes[i])).collect();
    let element = ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: wins[lead].0,
    };
    shell(&mut r, ShellCommand::Focus { element });
    let before: Vec<_> = wins.iter().map(|&(e, _)| rect(&r, e)).collect();

    let from = r.ls.probe.toplevel_configures.len();
    shell(
        &mut r,
        ShellCommand::ArrangeCluster {
            element: None,
            layout: ClusterLayout::Grid,
        },
    );
    let after: Vec<_> = wins.iter().map(|&(e, _)| rect(&r, e)).collect();

    // no window resized, and none asked for another size
    for (i, (b, a)) in before.iter().zip(&after).enumerate() {
        assert_eq!((a.2, a.3), (b.2, b.3), "{k}: {} kept its size", LABELS[i]);
        let own = (b.2 as i32, b.3 as i32);
        for (_, size, _) in r.ls.probe.toplevel_configures[from..]
            .iter()
            .filter(|(s, ..)| *s == wins[i].1)
        {
            assert!(
                *size == (0, 0) || *size == own,
                "{k}: {} asked for {size:?}",
                LABELS[i]
            );
        }
    }
    // no two overlap, 12 px apart at least
    for (i, x) in after.iter().enumerate() {
        for y in &after[i + 1..] {
            let apart = x.0 + x.2 + 12 <= y.0
                || y.0 + y.2 + 12 <= x.0
                || x.1 + x.3 + 12 <= y.1
                || y.1 + y.3 + 12 <= x.1;
            assert!(apart, "{k}: {x:?} {y:?}");
        }
    }

    // the order by the owner's rule, from the windows' places before: all share a centre, so
    // reading order is left to right, the widest first
    let mut order: Vec<usize> = (0..k).filter(|&i| i != lead).collect();
    order.sort_by_key(|&i| (before[i].0, before[i].1));
    order.insert(0, lead);
    let n = (1..).find(|n| n * n >= k).unwrap();
    let x0 = before.iter().map(|b| b.0).min().unwrap();
    let y0 = before.iter().map(|b| b.1).min().unwrap();
    let mut top = y0;
    for (row, slots) in order.chunks(n).enumerate() {
        // the row's first window at the left edge, each next one 12 px after the one before
        let mut x = x0;
        for &i in slots {
            assert_eq!(
                (after[i].0, after[i].1),
                (x, top),
                "{k}: {} in row {row}, at its top",
                LABELS[i]
            );
            x += after[i].2 + 12;
        }
        top += slots.iter().map(|&i| after[i].3).max().unwrap() + 12;
    }
    order.iter().map(|&i| LABELS[i]).collect()
}

/// `k` of the ten mixed sizes, the one at `k / 2` focused.
fn grid_of(k: usize) {
    grid_of_sizes(&SIZES[..k], k / 2);
}

/// The owner's case (2026-11-28): a small editor beside big terminals and medium windows sat in
/// a slot sized by the big ones, far from its neighbours. Three big terminals, the small editor
/// focused, two medium windows: three to a row, the editor first and 12 px left of the first
/// terminal, at the row's top.
#[test]
fn grid_packs_a_small_window_beside_big_ones() {
    let order = grid_of_sizes(
        &[
            (900, 600),
            (880, 620),
            (860, 580),
            (400, 300),
            (660, 500),
            (640, 480),
        ],
        3,
    );
    assert_eq!(order, ["D", "A", "B", "C", "E", "F"]);
}

#[test]
fn grid_of_one_window_keeps_it_in_place() {
    grid_of(1);
}

#[test]
fn grid_of_four_windows_is_two_by_two() {
    grid_of(4);
}

/// Three to a row: a row of three, a row of two.
#[test]
fn grid_of_five_windows_is_a_row_of_three_and_a_row_of_two() {
    grid_of(5);
}

#[test]
fn grid_of_nine_windows_is_three_by_three() {
    grid_of(9);
}

#[test]
fn grid_of_ten_windows_is_four_by_four() {
    grid_of(10);
}
