//! Cluster grid (owner, 2026-11-28; `infynitae msg arrange grid`, `BIND_CLUSTER_LAYOUT`, no
//! default key): the focused window's cluster in n x n slots, n = ceil(sqrt(k)), with real
//! clients. Every window keeps its size (no configure asks for another), the focused window takes
//! the first slot and the others follow in reading order; a column is as wide as its widest
//! window, a row as high as its tallest, 12 px apart, each window centred in its slot.

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

/// `k` windows of mixed sizes, all opened centred in the view (the policy here), one over the
/// other; the one at `k / 2` focused and the grid run.
fn grid_of(k: usize) {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    let wins: Vec<(ElementId, usize)> = (0..k).map(|i| open(&mut r, LABELS[i], SIZES[i])).collect();
    let focused = wins[k / 2].0;
    let element = ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: focused,
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

    // the slots by the owner's rule, from the windows' places before: the focused window in
    // the first, the others in reading order
    let lead = k / 2;
    let mut order: Vec<usize> = (0..k).filter(|&i| i != lead).collect();
    order.sort_by_key(|&i| (before[i].0, before[i].1));
    order.insert(0, lead);
    let n = (1..).find(|n| n * n >= k).unwrap();
    let (mut col_w, mut row_h) = (vec![0; n], vec![0; n]);
    for (slot, &i) in order.iter().enumerate() {
        col_w[slot % n] = col_w[slot % n].max(before[i].2);
        row_h[slot / n] = row_h[slot / n].max(before[i].3);
    }
    let x0 = before.iter().map(|b| b.0).min().unwrap();
    let y0 = before.iter().map(|b| b.1).min().unwrap();
    for (slot, &i) in order.iter().enumerate() {
        let (col, row) = (slot % n, slot / n);
        let sx = x0 + col_w[..col].iter().map(|w| w + 12).sum::<i64>();
        let sy = y0 + row_h[..row].iter().map(|h| h + 12).sum::<i64>();
        let want = (
            sx + (col_w[col] - before[i].2) / 2,
            sy + (row_h[row] - before[i].3) / 2,
        );
        assert_eq!(
            (after[i].0, after[i].1),
            want,
            "{k}: {} in slot {slot}",
            LABELS[i]
        );
        // centred: the slot's row and column line up the windows' middles
        let mid = (after[i].0 + after[i].2 / 2, after[i].1 + after[i].3 / 2);
        assert_eq!(mid, (sx + col_w[col] / 2, sy + row_h[row] / 2));
    }
    // the slots used: n columns in the first row, ceil(k / n) rows; the rest stay empty
    let mut tops: Vec<i64> = order.iter().map(|&i| after[i].1 + after[i].3 / 2).collect();
    tops.dedup();
    assert_eq!(tops.len(), k.div_ceil(n), "{k}: rows used");
}

#[test]
fn grid_of_one_window_keeps_it_in_place() {
    grid_of(1);
}

#[test]
fn grid_of_four_windows_is_two_by_two() {
    grid_of(4);
}

/// 3 x 3 with four slots empty: a row of three, a row of two, the third row unused.
#[test]
fn grid_of_five_windows_is_three_by_three_with_four_empty() {
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
