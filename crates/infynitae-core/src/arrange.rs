//! Cluster layouts (owner, 2026-11-21): the windows of a cluster (`Core::clusters`, windows
//! within 300 px of each other) arranged once when the person asks (a key, `infynitae msg
//! arrange`), and kept arranged by `CLUSTER_SORT` when they turn it on. These are the only
//! paths on which one window's change moves other windows (docs/DECISIONS.md, 2026-11-06 and
//! 2026-11-21).
//!
//! - tidy: the windows at their own sizes, in reading order, packed in rows as wide as the usable
//!   view at zoom 1, 12 px apart;
//! - grid: equal cells, the columns chosen so that a cell is as square as the view allows;
//! - columns: side by side, each the usable height;
//! - box (owner, 2026-11-23): own sizes, the biggest first, in centred rows that make a block as
//!   close to the view's shape as the sizes allow;
//! - ring (owner, 2026-11-23): the big windows boxed in the middle, the smaller ones on an ellipse
//!   around them, the first at the top, clockwise.
//!
//! The cluster's top-left corner stays where it is. Fullscreen, minimized, kept-on-top and
//! notification windows and children (dialogs) are left out; a child moves with its parent.

use crate::Core;
use crate::protocol::Effect;
use crate::stage::ElementId;

/// The placement gap between windows (F-PLACE-1), and between the room and the usable view's edge.
pub(crate) const GAP: i64 = 12;
/// The smallest cell side a layout asks a window for (the cluster scale's smallest size).
const MIN_SIDE: i64 = 64;

/// One of the layouts, in the order the key cycles them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClusterLayout {
    Tidy,
    Grid,
    Columns,
    Box,
    Ring,
    /// Super+Shift+G (owner, 2026-11-22): every window at its own size, one row per app, rows as
    /// wide as they need (not squeezed into one view: an app with a minimum size such as Discord
    /// keeps it). Not in the Super+G cycle.
    ByApp,
}

impl ClusterLayout {
    /// The layout's name as `infynitae msg arrange` takes it.
    pub fn parse(name: &str) -> Option<ClusterLayout> {
        match name.trim().to_ascii_lowercase().as_str() {
            "tidy" => Some(ClusterLayout::Tidy),
            "grid" => Some(ClusterLayout::Grid),
            "columns" => Some(ClusterLayout::Columns),
            "box" => Some(ClusterLayout::Box),
            "ring" | "circle" => Some(ClusterLayout::Ring),
            "app" | "by-app" => Some(ClusterLayout::ByApp),
            _ => None,
        }
    }

    fn next(self) -> ClusterLayout {
        match self {
            ClusterLayout::Tidy => ClusterLayout::Grid,
            ClusterLayout::Grid => ClusterLayout::Columns,
            ClusterLayout::Columns => ClusterLayout::Box,
            ClusterLayout::Box => ClusterLayout::Ring,
            ClusterLayout::Ring | ClusterLayout::ByApp => ClusterLayout::Tidy,
        }
    }

    /// A layout that keeps every window's size: the one `CLUSTER_SORT` "tidy" keeps a cluster in.
    fn keeps_sizes(self) -> bool {
        matches!(
            self,
            ClusterLayout::Tidy | ClusterLayout::Box | ClusterLayout::Ring
        )
    }
}

/// `CLUSTER_SORT`: what is kept arranged without a key. Both off by default: no window moves
/// another unless the person asks (owner, 2026-11-06).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClusterSort {
    /// "tidy": a cluster is tidied again after one of its windows closes or is minimized.
    pub tidy: bool,
    /// "app": a new window opens beside its app's newest window in the first screen's view.
    pub app: bool,
}

impl ClusterSort {
    /// The config's words, any of "tidy" and "app" separated by spaces or commas; "off", an empty
    /// value or other words turn nothing on.
    pub fn parse(value: &str) -> ClusterSort {
        let mut sort = ClusterSort::default();
        for word in value.split([' ', ',', '\t']) {
            match word.to_ascii_lowercase().as_str() {
                "tidy" => sort.tidy = true,
                "app" => sort.app = true,
                _ => {}
            }
        }
        sort
    }
}

/// A window's place and size on the Canvas: its anchor (the window geometry's origin) and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

/// Reading order of `rects`: rows from the top, left to right in a row. The topmost rect not yet
/// ordered starts a row; every rect whose vertical centre lies within that rect's height joins it.
pub(crate) fn reading_order(rects: &[Cell]) -> Vec<usize> {
    let mut left: Vec<usize> = (0..rects.len()).collect();
    left.sort_by_key(|&i| (rects[i].y, rects[i].x));
    let mut order = Vec::with_capacity(rects.len());
    while let Some(&lead) = left.first() {
        let (top, bottom) = (rects[lead].y, rects[lead].y + rects[lead].h);
        let (mut row, rest): (Vec<usize>, Vec<usize>) = left.iter().partition(|&&i| {
            let c = rects[i].y + rects[i].h / 2;
            (top..=bottom).contains(&c)
        });
        row.sort_by_key(|&i| (rects[i].x, rects[i].y));
        order.extend(row);
        left = rest;
    }
    order
}

/// Tidy: `sizes` in order, at their own sizes, left to right from `origin`, a new row when the
/// next one would reach past `width`; rows `GAP` apart, as high as their tallest window.
pub(crate) fn tidy(sizes: &[(i64, i64)], origin: (i64, i64), width: i64) -> Vec<Cell> {
    let (mut x, mut y, mut row_h) = (origin.0, origin.1, 0);
    sizes
        .iter()
        .map(|&(w, h)| {
            if x > origin.0 && x + w > origin.0 + width {
                (x, y, row_h) = (origin.0, y + row_h + GAP, 0);
            }
            let cell = Cell { x, y, w, h };
            x += w + GAP;
            row_h = row_h.max(h);
            cell
        })
        .collect()
}

/// By app: `sizes` in order at their own sizes, left to right from `origin`, a new row where
/// `apps` changes; rows `GAP` apart, as high as their tallest window, as wide as they need.
pub(crate) fn by_app(sizes: &[(i64, i64)], apps: &[&str], origin: (i64, i64)) -> Vec<Cell> {
    let (mut x, mut y, mut row_h) = (origin.0, origin.1, 0);
    sizes
        .iter()
        .zip(apps)
        .enumerate()
        .map(|(i, (&(w, h), app))| {
            if i > 0 && apps[i - 1] != *app {
                (x, y, row_h) = (origin.0, y + row_h + GAP, 0);
            }
            let cell = Cell { x, y, w, h };
            x += w + GAP;
            row_h = row_h.max(h);
            cell
        })
        .collect()
}

/// Grid: `n` equal cells filling `room` from `origin`, row by row. The column count is the one
/// whose cell is closest to a square, each empty cell counted against it as `1/n` (so four
/// windows on a wide view get two by two, not three by two with two cells empty); the fewer
/// columns on a tie.
pub(crate) fn grid(n: usize, origin: (i64, i64), room: (i64, i64)) -> Vec<Cell> {
    if n == 0 {
        return Vec::new();
    }
    let side = |total: i64, k: i64| ((total - (k - 1) * GAP) / k).max(MIN_SIDE);
    let shape = |cols: usize| {
        let rows = n.div_ceil(cols);
        let (w, h) = (side(room.0, cols as i64), side(room.1, rows as i64));
        let empty = (rows * cols - n) as f64 / n as f64;
        (w as f64 / h as f64).ln().abs() + empty
    };
    let cols = (1..=n)
        .min_by(|&a, &b| shape(a).total_cmp(&shape(b)))
        .unwrap_or(1);
    let rows = n.div_ceil(cols);
    let (w, h) = (side(room.0, cols as i64), side(room.1, rows as i64));
    (0..n)
        .map(|i| Cell {
            x: origin.0 + (i % cols) as i64 * (w + GAP),
            y: origin.1 + (i / cols) as i64 * (h + GAP),
            w,
            h,
        })
        .collect()
}

/// Columns: `n` cells side by side from `origin`, sharing `room`'s width, each its full height.
pub(crate) fn columns(n: usize, origin: (i64, i64), room: (i64, i64)) -> Vec<Cell> {
    if n == 0 {
        return Vec::new();
    }
    let w = ((room.0 - (n as i64 - 1) * GAP) / n as i64).max(MIN_SIDE);
    let h = room.1.max(MIN_SIDE);
    (0..n)
        .map(|i| Cell {
            x: origin.0 + i as i64 * (w + GAP),
            y: origin.1,
            w,
            h,
        })
        .collect()
}

/// `cells` moved so that their bounding box's top-left corner is `origin`.
fn moved_to(mut cells: Vec<Cell>, origin: (i64, i64)) -> Vec<Cell> {
    let x0 = cells.iter().map(|c| c.x).min().unwrap_or(0);
    let y0 = cells.iter().map(|c| c.y).min().unwrap_or(0);
    for c in &mut cells {
        c.x += origin.0 - x0;
        c.y += origin.1 - y0;
    }
    cells
}

/// `sizes` in rows at most `width` wide (a wider window alone), each row centred on the widest
/// and its windows centred on the row's middle line; rows `GAP` apart. A window less than half
/// the area of its group's first starts a new group (big and small windows do not share a row),
/// and a group's rows hold the same number of windows give or take one (four in two by two, not
/// three and one). Cells in `sizes`' order, from (0, 0).
fn centred_rows(sizes: &[(i64, i64)], width: i64) -> Vec<Cell> {
    let area = |i: usize| sizes[i].0 * sizes[i].1;
    // greedy rows of `group` within `width`, at most `most` windows each
    let fill = |group: &[usize], most: usize| {
        let mut rows: Vec<Vec<usize>> = Vec::new();
        let mut used = 0;
        for &i in group {
            match rows.last_mut() {
                Some(row) if row.len() < most && used + GAP + sizes[i].0 <= width => {
                    row.push(i);
                    used += GAP + sizes[i].0;
                }
                _ => {
                    rows.push(vec![i]);
                    used = sizes[i].0;
                }
            }
        }
        rows
    };
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut start = 0;
    while start < sizes.len() {
        let end = (start..sizes.len())
            .find(|&i| 2 * area(i) < area(start))
            .unwrap_or(sizes.len());
        let group: Vec<usize> = (start..end).collect();
        let n = fill(&group, usize::MAX).len();
        rows.extend(fill(&group, group.len().div_ceil(n)));
        start = end;
    }
    let row_w =
        |row: &[usize]| row.iter().map(|&i| sizes[i].0).sum::<i64>() + (row.len() as i64 - 1) * GAP;
    let widest = rows.iter().map(|r| row_w(r)).max().unwrap_or(0);
    let mut cells = vec![
        Cell {
            x: 0,
            y: 0,
            w: 0,
            h: 0
        };
        sizes.len()
    ];
    let mut y = 0;
    for row in &rows {
        let h = row.iter().map(|&i| sizes[i].1).max().unwrap_or(0);
        let mut x = (widest - row_w(row)) / 2;
        for &i in row {
            let (w, wh) = sizes[i];
            cells[i] = Cell {
                x,
                y: y + (h - wh) / 2,
                w,
                h: wh,
            };
            x += w + GAP;
        }
        y += h + GAP;
    }
    cells
}

/// Box: `sizes` at their own sizes, the biggest (by area) first, in centred rows. The row width
/// is the one whose block is closest to `room`'s shape, each empty part of the block counted
/// against it; the narrower on a tie. Cells in `sizes`' order, the block's corner at `origin`.
pub(crate) fn boxed(sizes: &[(i64, i64)], origin: (i64, i64), room: (i64, i64)) -> Vec<Cell> {
    if sizes.is_empty() {
        return Vec::new();
    }
    let mut big_first: Vec<usize> = (0..sizes.len()).collect();
    big_first.sort_by_key(|&i| std::cmp::Reverse(sizes[i].0 * sizes[i].1));
    let sorted: Vec<(i64, i64)> = big_first.iter().map(|&i| sizes[i]).collect();
    let aspect = room.0.max(1) as f64 / room.1.max(1) as f64;
    let area: i64 = sizes.iter().map(|&(w, h)| w * h).sum();
    // every width at which a row can end: each run of windows from each start
    let mut widths: Vec<i64> = Vec::new();
    for start in 0..sorted.len() {
        let mut w = -GAP;
        for &(sw, _) in &sorted[start..] {
            w += sw + GAP;
            widths.push(w);
        }
    }
    widths.sort_unstable();
    widths.dedup();
    let score = |cells: &[Cell]| {
        let bw = cells.iter().map(|c| c.x + c.w).max().unwrap_or(1).max(1);
        let bh = cells.iter().map(|c| c.y + c.h).max().unwrap_or(1).max(1);
        let empty = 1.0 - area as f64 / (bw * bh) as f64;
        ((bw as f64 / bh as f64) / aspect).ln().abs() + empty
    };
    let best = widths
        .into_iter()
        .map(|w| centred_rows(&sorted, w))
        .min_by(|a, b| score(a).total_cmp(&score(b)))
        .unwrap_or_default();
    let mut cells = vec![
        Cell {
            x: 0,
            y: 0,
            w: 0,
            h: 0
        };
        sizes.len()
    ];
    for (k, &i) in big_first.iter().enumerate() {
        cells[i] = best[k];
    }
    moved_to(cells, origin)
}

/// Ring: the windows at least half the area of the biggest boxed in the middle, the others
/// centred on an ellipse around them in `sizes`' order, the first at the top, clockwise; the
/// ellipse grows until no two windows are within `GAP` of each other. With only one kind of
/// window it is a box. Cells in `sizes`' order, the corner of their bounding box at `origin`.
pub(crate) fn ring(sizes: &[(i64, i64)], origin: (i64, i64), room: (i64, i64)) -> Vec<Cell> {
    let biggest = sizes.iter().map(|&(w, h)| w * h).max().unwrap_or(0);
    let (inner, outer): (Vec<usize>, Vec<usize>) =
        (0..sizes.len()).partition(|&i| 2 * sizes[i].0 * sizes[i].1 >= biggest);
    if inner.is_empty() || outer.is_empty() {
        return boxed(sizes, origin, room);
    }
    let middle = boxed(
        &inner.iter().map(|&i| sizes[i]).collect::<Vec<_>>(),
        (0, 0),
        room,
    );
    let (mw, mh) = (
        middle.iter().map(|c| c.x + c.w).max().unwrap_or(0),
        middle.iter().map(|c| c.y + c.h).max().unwrap_or(0),
    );
    let (cx, cy) = (mw as f64 / 2.0, mh as f64 / 2.0);
    let max_w = outer.iter().map(|&i| sizes[i].0).max().unwrap_or(0) as f64;
    let max_h = outer.iter().map(|&i| sizes[i].1).max().unwrap_or(0) as f64;
    let n = outer.len() as f64;
    let around = |grow: f64| -> Vec<Cell> {
        let rx = (cx + max_w / 2.0 + GAP as f64) * grow;
        let ry = (cy + max_h / 2.0 + GAP as f64) * grow;
        outer
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * k as f64 / n;
                let (w, h) = sizes[i];
                Cell {
                    x: (cx + rx * a.cos()).round() as i64 - w / 2,
                    y: (cy + ry * a.sin()).round() as i64 - h / 2,
                    w,
                    h,
                }
            })
            .collect()
    };
    let clear = |a: &Cell, b: &Cell| {
        a.x + a.w + GAP <= b.x
            || b.x + b.w + GAP <= a.x
            || a.y + a.h + GAP <= b.y
            || b.y + b.h + GAP <= a.y
    };
    let mut grow = 1.0;
    let mut placed = around(grow);
    // a bound, not a guess: each step is 4% bigger; 200 steps reach over 2500 times the start
    for _ in 0..200 {
        let free = placed.iter().enumerate().all(|(k, a)| {
            middle.iter().all(|m| clear(a, m)) && placed[k + 1..].iter().all(|b| clear(a, b))
        });
        if free {
            break;
        }
        grow *= 1.04;
        placed = around(grow);
    }
    let mut cells = vec![
        Cell {
            x: 0,
            y: 0,
            w: 0,
            h: 0
        };
        sizes.len()
    ];
    for (k, &i) in inner.iter().enumerate() {
        cells[i] = middle[k];
    }
    for (k, &i) in outer.iter().enumerate() {
        cells[i] = placed[k];
    }
    moved_to(cells, origin)
}

/// The anchor of a window of `size` centred in `cell` (an app that keeps a size of its own).
pub(crate) fn centred_in(cell: Cell, size: (i64, i64)) -> crate::geom::CanvasPoint {
    crate::geom::CanvasPoint {
        x: cell.x + (cell.w - size.0) / 2,
        y: cell.y + (cell.h - size.1) / 2,
    }
}

impl Core {
    /// `CLUSTER_SORT`, read on save like the other placement keys.
    pub fn set_cluster_sort(&mut self, sort: ClusterSort) {
        self.cluster_sort = sort;
    }

    pub fn cluster_sort(&self) -> ClusterSort {
        self.cluster_sort
    }

    /// The windows of `members` a layout moves, with their places: mapped, shown, windowed, on
    /// the Canvas (not kept on top), not a notification and not a child of a mapped window.
    fn arranged(&self, members: &[ElementId]) -> Vec<(ElementId, Cell)> {
        members
            .iter()
            .filter_map(|&e| {
                let (a, t, _) = self.stage.mapped(e)?;
                let s = t.geometry.size;
                Some((
                    e,
                    Cell {
                        x: a.x,
                        y: a.y,
                        w: i64::from(s.w),
                        h: i64::from(s.h),
                    },
                ))
            })
            .filter(|&(e, _)| {
                !self.stage.is_minimized(e)
                    && self.fullscreen_of(e).is_none()
                    && !self.stage.is_above(e)
                    && !self.is_notification(e)
                    && self
                        .toplevel_parent(e)
                        .is_none_or(|p| self.stage.mapped(p).is_none())
            })
            .collect()
    }

    /// The room a layout fills: the usable area of the pointer's screen (else the first) at
    /// zoom 1, less the gap on each side.
    fn layout_room(&self) -> Option<(i64, i64)> {
        let output = self
            .pointer_output()
            .or_else(|| self.outputs().first().map(|o| o.id))?;
        let (_, _, w, h) = self.usable_area(output)?;
        let fit = |v: f64| ((v.floor() as i64) - 2 * GAP).max(MIN_SIDE);
        Some((fit(w), fit(h)))
    }

    /// Arranges `members` by `layout` from their top-left corner. A window whose cell has
    /// another size is asked for it and centred in the cell at the size it has now; its commits
    /// keep it centred until it answers (`laid_out_anchor`). Its children move with it. Returns
    /// the cells' bounding box `(x, y, w, h)`, or `None` when no window is arranged.
    pub(crate) fn arrange(
        &mut self,
        members: &[ElementId],
        layout: ClusterLayout,
        out: &mut Vec<Effect>,
    ) -> Option<(i64, i64, i64, i64)> {
        let (ids, rects): (Vec<ElementId>, Vec<Cell>) = self.arranged(members).into_iter().unzip();
        let mut order = reading_order(&rects);
        // by app: each app's windows together, the apps in the order their first window reads
        let app_of = |i: usize| -> String {
            self.stage
                .meta(ids[i])
                .and_then(|m| m.app_id.clone())
                .unwrap_or_default()
        };
        if layout == ClusterLayout::ByApp {
            let mut firsts: Vec<String> = Vec::new();
            for &i in &order {
                let a = app_of(i);
                if !firsts.contains(&a) {
                    firsts.push(a);
                }
            }
            order.sort_by_key(|&i| firsts.iter().position(|a| *a == app_of(i)));
        }
        let origin = (
            rects.iter().map(|r| r.x).min()?,
            rects.iter().map(|r| r.y).min()?,
        );
        let room = self.layout_room()?;
        let cells = match layout {
            ClusterLayout::Tidy => {
                let sizes: Vec<_> = order.iter().map(|&i| (rects[i].w, rects[i].h)).collect();
                tidy(&sizes, origin, room.0)
            }
            ClusterLayout::Grid => grid(ids.len(), origin, room),
            ClusterLayout::Columns => columns(ids.len(), origin, room),
            ClusterLayout::Box | ClusterLayout::Ring => {
                let sizes: Vec<_> = order.iter().map(|&i| (rects[i].w, rects[i].h)).collect();
                if layout == ClusterLayout::Box {
                    boxed(&sizes, origin, room)
                } else {
                    ring(&sizes, origin, room)
                }
            }
            ClusterLayout::ByApp => {
                let sizes: Vec<_> = order.iter().map(|&i| (rects[i].w, rects[i].h)).collect();
                let apps: Vec<String> = order.iter().map(|&i| app_of(i)).collect();
                let apps: Vec<&str> = apps.iter().map(String::as_str).collect();
                by_app(&sizes, &apps, origin)
            }
        };
        let children: Vec<(ElementId, ElementId)> = self
            .stage
            .mapped_elements()
            .into_iter()
            .filter_map(|c| Some((c, self.toplevel_parent(c)?)))
            .collect();
        for (&i, &cell) in order.iter().zip(&cells) {
            let (e, old) = (ids[i], rects[i]);
            self.layout_cells.retain(|(x, _)| *x != e);
            let at = if (cell.w, cell.h) == (old.w, old.h) {
                crate::geom::CanvasPoint {
                    x: cell.x,
                    y: cell.y,
                }
            } else {
                // the cell's size, asked as a resize asks (held, INV-PROTO-1): it ends a maximize
                self.resized_out_of_maximize(e);
                out.push(Effect::ToplevelConfigureSize {
                    element: e,
                    size: crate::geom::LocalSize {
                        w: cell.w as i32,
                        h: cell.h as i32,
                    },
                });
                self.layout_cells.push((e, cell));
                centred_in(cell, (old.w, old.h))
            };
            let (dx, dy) = (at.x - old.x, at.y - old.y);
            if self.place_by_shell(e, at, out).is_err() {
                continue;
            }
            for &(child, _) in children.iter().filter(|(_, p)| *p == e) {
                if let Some(a) = self.stage.anchor(child) {
                    let to = crate::geom::CanvasPoint {
                        x: a.x + dx,
                        y: a.y + dy,
                    };
                    let _ = self.place_by_shell(child, to, out);
                }
            }
        }
        let x0 = cells.iter().map(|c| c.x).min()?;
        let y0 = cells.iter().map(|c| c.y).min()?;
        let x1 = cells.iter().map(|c| c.x + c.w).max()?;
        let y1 = cells.iter().map(|c| c.y + c.h).max()?;
        Some((x0, y0, x1 - x0, y1 - y0))
    }

    /// The layout key (Super+G) and `infynitae msg arrange`: `around`'s cluster (its
    /// parent's, for a child) arranged by `layout`, or by the layout after the one this cluster
    /// got last (tidy first). With `proof`, the camera frames the result. False when nothing was
    /// arranged, and when `around` is a window a layout leaves out (a fullscreen game focused:
    /// the windows hidden behind it stay as they are).
    pub(crate) fn arrange_cluster(
        &mut self,
        around: ElementId,
        layout: Option<ClusterLayout>,
        proof: Option<crate::authority::Authorized>,
        out: &mut Vec<Effect>,
    ) -> bool {
        let around = self
            .toplevel_parent(around)
            .filter(|p| self.stage.mapped(*p).is_some())
            .unwrap_or(around);
        if self.arranged(&[around]).is_empty() {
            return false;
        }
        let layout = layout.unwrap_or(match self.last_layout {
            Some((e, last)) if e == around => last.next(),
            _ => ClusterLayout::Tidy,
        });
        let cluster = self.cluster_for_element(around);
        let Some(bounds) = self.arrange(&cluster, layout, out) else {
            return false;
        };
        self.last_layout = Some((around, layout));
        if let Some(p) = proof {
            self.frame_bounds(p, bounds, out);
        }
        true
    }

    /// `CLUSTER_SORT` "tidy", before `id` closes or is minimized: the rest of its cluster, to be
    /// tidied once it has left (`tidy_after_leave`). Empty when the setting is off, `id` is not a
    /// window a layout arranges (a dialog leaves no gap) or a drag is on.
    pub(crate) fn cluster_left_by(&self, id: ElementId) -> Vec<ElementId> {
        if !self.cluster_sort.tidy
            || self.input.interaction.active()
            || self.arranged(&[id]).is_empty()
        {
            return Vec::new();
        }
        let mut rest = self.cluster_for_element(id);
        rest.retain(|&e| e != id);
        rest
    }

    /// The windows `cluster_left_by` named, arranged again: the gap closes. A cluster last put
    /// in a box or a ring by hand keeps that layout; any other is tidied. The camera stays.
    pub(crate) fn tidy_after_leave(&mut self, rest: &[ElementId], out: &mut Vec<Effect>) {
        if rest.is_empty() {
            return;
        }
        let layout = match self.last_layout {
            Some((e, l)) if l.keeps_sizes() && rest.contains(&e) => l,
            _ => ClusterLayout::Tidy,
        };
        self.arrange(rest, layout, out);
    }

    /// A commit of a window a layout asked for a size: the anchor that centres the size it drew
    /// in its cell, until it answers that configure (`layout_answered`).
    pub(crate) fn laid_out_anchor(
        &self,
        id: ElementId,
        size: crate::geom::LocalSize,
    ) -> Option<crate::geom::CanvasPoint> {
        let (_, cell) = self.layout_cells.iter().find(|(e, _)| *e == id)?;
        Some(centred_in(*cell, (i64::from(size.w), i64::from(size.h))))
    }

    /// `id` drew a frame of `drawn` size under the configure that asked for `acked`: a layout's
    /// cell is done with once its size was acknowledged, whatever size the app drew. A window
    /// that kept a size bigger than its cell (an app's minimum, such as Discord's width) would
    /// cover its neighbours: once the last window of the layout has answered, its cluster is
    /// tidied at the sizes the windows took, so none covers another (owner, 2026-11-22).
    pub(crate) fn layout_answered(
        &mut self,
        id: ElementId,
        acked: Option<crate::geom::LocalSize>,
        drawn: crate::geom::LocalSize,
        out: &mut Vec<Effect>,
    ) {
        let Some(s) = acked else { return };
        let Some(k) = self
            .layout_cells
            .iter()
            .position(|(e, c)| *e == id && (i64::from(s.w), i64::from(s.h)) == (c.w, c.h))
        else {
            return;
        };
        let (_, cell) = self.layout_cells.remove(k);
        if i64::from(drawn.w) > cell.w || i64::from(drawn.h) > cell.h {
            self.layout_overflow = Some(id);
        }
        let Some(over) = self.layout_overflow else {
            return;
        };
        if self.stage.mapped(over).is_none() {
            self.layout_overflow = None;
            return;
        }
        let cluster = self.cluster_for_element(over);
        if self.layout_cells.iter().any(|(e, _)| cluster.contains(e)) {
            return;
        }
        self.layout_overflow = None;
        self.arrange(&cluster, ClusterLayout::Tidy, out);
    }

    /// `CLUSTER_SORT` "app": where a new window of an app goes when the app already has a window
    /// in the first screen's view: right of the newest such window, the gap kept (the free-space
    /// search moves it on from there if that spot is taken).
    pub(crate) fn beside_own_app(&self, id: ElementId) -> Option<(i64, i64)> {
        if !self.cluster_sort.app {
            return None;
        }
        let output = self.outputs().first()?.id;
        let app = self.stage.meta(id)?.app_id.as_deref()?.to_ascii_lowercase();
        let same = self
            .stage
            .mapped_in_creation_order()
            .into_iter()
            .rev()
            .find(|&e| {
                e != id
                    && !self.stage.is_minimized(e)
                    && !self.stage.is_above(e)
                    && self.fullscreen_of(e).is_none()
                    && self
                        .stage
                        .meta(e)
                        .and_then(|m| m.app_id.as_deref())
                        .is_some_and(|a| a.eq_ignore_ascii_case(&app))
                    && self.view_covers(output, e)
            })?;
        let (a, t) = (self.stage.anchor(same)?, self.stage.tree(same)?);
        Some((a.x + i64::from(t.geometry.size.w) + GAP, a.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{CanvasPoint, LocalPoint, LocalRect, LocalSize};
    use crate::navigation::PersistedCameras;
    use crate::output::{OutputDescriptor, OutputId, Transform};
    use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
    use crate::stage::ElementRef;
    use crate::{InputPort, PlacementPolicy, ShellCommand, ShellError, ShellPort};

    fn win(k: u64, w: i32, h: i32, mapped: bool) -> SurfaceTree {
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(std::num::NonZeroU64::new(k).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        }
    }

    /// One 1920 x 1080 screen at scale 1, camera at the origin: the room is 1896 x 1056.
    struct Desk {
        c: Core,
        i: InputPort,
        s: ShellPort,
        o: OutputId,
    }

    fn desk() -> Desk {
        let (mut c, i, s, mut t) = Core::new(PersistedCameras::default());
        c.set_placement_policy(PlacementPolicy::ClusterTiling);
        let o = c
            .output_connected(
                &mut t,
                OutputDescriptor {
                    key: crate::output::tests::key("A"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap()
            .0;
        Desk { c, i, s, o }
    }

    /// A window of app `app`, `w` x `h`, mapped and then put at `at`.
    fn open(d: &mut Desk, k: u64, app: &str, (w, h): (i32, i32), at: (i64, i64)) -> ElementId {
        let id = d.c.toplevel_created();
        d.c.toplevel_metadata(id, None, Some(app)).unwrap();
        d.c.toplevel_committed(id, win(k, w, h, false)).unwrap();
        d.c.toplevel_committed(id, win(k, w, h, true)).unwrap();
        let mut fx = Vec::new();
        d.c.place_by_shell(id, CanvasPoint { x: at.0, y: at.1 }, &mut fx)
            .unwrap();
        id
    }

    fn rect(c: &Core, id: ElementId) -> Cell {
        let (a, t, _) = c.stage().mapped(id).unwrap();
        Cell {
            x: a.x,
            y: a.y,
            w: i64::from(t.geometry.size.w),
            h: i64::from(t.geometry.size.h),
        }
    }

    fn anchor(c: &Core, id: ElementId) -> (i64, i64) {
        let a = c.stage().anchor(id).unwrap();
        (a.x, a.y)
    }

    fn asked(fx: &[Effect], id: ElementId) -> Option<(i32, i32)> {
        fx.iter().rev().find_map(|f| match f {
            Effect::ToplevelConfigureSize { element, size } if *element == id => {
                Some((size.w, size.h))
            }
            _ => None,
        })
    }

    fn cell(x: i64, y: i64, w: i64, h: i64) -> Cell {
        Cell { x, y, w, h }
    }

    fn apart(a: Cell, b: Cell) -> bool {
        a.x + a.w + GAP <= b.x
            || b.x + b.w + GAP <= a.x
            || a.y + a.h + GAP <= b.y
            || b.y + b.h + GAP <= a.y
    }

    fn all_apart(cells: &[Cell]) -> bool {
        cells
            .iter()
            .enumerate()
            .all(|(i, &a)| cells[i + 1..].iter().all(|&b| apart(a, b)))
    }

    /// Rows from the top, left to right in a row; a window a little lower is in the same row.
    #[test]
    fn reading_order_is_rows_then_left_to_right() {
        let rects = [
            cell(900, 20, 400, 300),  // top row, right
            cell(0, 0, 400, 300),     // top row, left
            cell(100, 500, 300, 200), // second row
            cell(450, 40, 300, 300),  // top row, middle (lower, centre within the first)
        ];
        assert_eq!(reading_order(&rects), vec![1, 3, 0, 2]);
    }

    /// Tidy keeps sizes and order, wraps at the width, keeps the gap and starts at the origin.
    #[test]
    fn tidy_packs_rows_with_the_gap() {
        let sizes = [(800, 600), (800, 400), (500, 300), (300, 200)];
        let cells = tidy(&sizes, (-50, 40), 1896);
        assert_eq!(cells[0], cell(-50, 40, 800, 600));
        assert_eq!(cells[1], cell(-50 + 812, 40, 800, 400));
        // 1624 + 500 > 1896: the third starts the second row, under the tallest of the first
        assert_eq!(cells[2], cell(-50, 40 + 612, 500, 300));
        assert_eq!(cells[3], cell(-50 + 512, 40 + 612, 300, 200));
        for (c, s) in cells.iter().zip(sizes) {
            assert_eq!((c.w, c.h), s, "sizes kept");
            assert!(c.x + c.w <= -50 + 1896, "within the width: {c:?}");
        }
        assert!(all_apart(&cells));
    }

    /// A window wider than the room has a row of its own.
    #[test]
    fn tidy_gives_a_wide_window_its_own_row() {
        let cells = tidy(&[(400, 300), (2500, 500), (400, 300)], (0, 0), 1896);
        assert_eq!(cells[1], cell(0, 312, 2500, 500));
        assert_eq!(cells[2], cell(0, 824, 400, 300));
        assert!(all_apart(&cells));
    }

    /// Grid: two windows side by side on a landscape view, stacked on a portrait one; four in
    /// two by two; cells equal, filling the room, the gap between.
    #[test]
    fn grid_picks_rows_and_columns_from_count_and_shape() {
        let room = (1896, 1032);
        let two = grid(2, (0, 0), room);
        assert_eq!(two, vec![cell(0, 0, 942, 1032), cell(954, 0, 942, 1032)]);
        let portrait = grid(2, (0, 0), (1032, 1896));
        assert_eq!(
            portrait,
            vec![cell(0, 0, 1032, 942), cell(0, 954, 1032, 942)]
        );
        let four = grid(4, (10, 20), room);
        assert_eq!(four[0], cell(10, 20, 942, 510));
        assert_eq!(four[3], cell(10 + 954, 20 + 522, 942, 510));
        assert!(all_apart(&four));
        let five = grid(5, (0, 0), room);
        assert!(all_apart(&five));
        assert!(five.iter().all(|c| (c.w, c.h) == (five[0].w, five[0].h)));
        let right = five.iter().map(|c| c.x + c.w).max().unwrap();
        let bottom = five.iter().map(|c| c.y + c.h).max().unwrap();
        assert!(right <= 1896 && bottom <= 1032, "inside the room");
    }

    /// Columns: side by side, the full height, the width shared with the gap.
    #[test]
    fn by_app_gives_each_app_a_row_at_own_sizes() {
        let cells = by_app(
            &[(900, 600), (900, 600), (1200, 800), (900, 600)],
            &["game", "game", "chat", "chat"],
            (0, 0),
        );
        assert_eq!(cells[0], cell(0, 0, 900, 600));
        assert_eq!(cells[1], cell(912, 0, 900, 600), "same app: same row");
        assert_eq!(
            cells[2],
            cell(0, 612, 1200, 800),
            "another app: the next row"
        );
        assert_eq!(cells[3], cell(1212, 612, 900, 600));
        // no width limit: ten windows stay in one row
        let ten = by_app(&[(900, 600); 10], &["game"; 10], (0, 0));
        assert!(ten.iter().all(|c| c.y == 0));
        assert_eq!(ten[9].x, 9 * 912);
        assert_eq!(ClusterLayout::parse("app"), Some(ClusterLayout::ByApp));
    }

    #[test]
    fn columns_share_the_width_at_full_height() {
        let cells = columns(3, (5, 6), (1896, 1032));
        assert_eq!(
            cells,
            vec![
                cell(5, 6, 624, 1032),
                cell(5 + 636, 6, 624, 1032),
                cell(5 + 1272, 6, 624, 1032)
            ]
        );
    }

    #[test]
    fn cluster_sort_reads_its_words() {
        assert_eq!(ClusterSort::parse("off"), ClusterSort::default());
        assert_eq!(ClusterSort::parse(""), ClusterSort::default());
        assert_eq!(
            ClusterSort::parse("tidy"),
            ClusterSort {
                tidy: true,
                app: false
            }
        );
        assert_eq!(
            ClusterSort::parse("Tidy app"),
            ClusterSort {
                tidy: true,
                app: true
            }
        );
        assert_eq!(
            ClusterSort::parse("app,tidy"),
            ClusterSort {
                tidy: true,
                app: true
            }
        );
    }

    /// Tidy on a real Core: three overlapping windows end up apart with the gap, at their own
    /// sizes, in reading order, from the cluster's top-left corner; no size is asked.
    #[test]
    fn tidy_separates_a_cluster_and_keeps_its_corner() {
        let mut d = desk();
        let a = open(&mut d, 1, "a", (400, 300), (-900, -500));
        let b = open(&mut d, 2, "b", (500, 400), (-700, -450));
        let c = open(&mut d, 3, "c", (300, 200), (-1000, -100));
        assert_eq!(d.c.cluster_for_element(a).len(), 3);
        let mut fx = Vec::new();
        assert!(d.c.arrange_cluster(a, Some(ClusterLayout::Tidy), None, &mut fx));
        assert_eq!(rect(&d.c, a), cell(-1000, -500, 400, 300));
        assert_eq!(rect(&d.c, b), cell(-1000 + 412, -500, 500, 400));
        assert_eq!(rect(&d.c, c), cell(-1000 + 412 + 512, -500, 300, 200));
        assert!(all_apart(&[rect(&d.c, a), rect(&d.c, b), rect(&d.c, c)]));
        assert!(
            fx.iter()
                .all(|f| !matches!(f, Effect::ToplevelConfigureSize { .. })),
            "tidy keeps sizes: {fx:?}"
        );
    }

    /// Grid: four windows get 2 x 2 cells of the room (942 x 522), asked as configures. Until a
    /// window answers it is centred in its cell; one that draws the cell size sits at the cell's
    /// corner; one that keeps its own size stays centred, and its cell is then done with.
    #[test]
    fn grid_sizes_windows_to_cells_and_centres_the_ones_that_refuse() {
        let mut d = desk();
        let a = open(&mut d, 1, "a", (400, 300), (0, 0));
        let b = open(&mut d, 2, "b", (400, 300), (420, 10));
        let c = open(&mut d, 3, "c", (400, 300), (0, 330));
        let e = open(&mut d, 4, "e", (300, 200), (430, 340));
        let mut fx = Vec::new();
        assert!(d.c.arrange_cluster(a, Some(ClusterLayout::Grid), None, &mut fx));
        for id in [a, b, c, e] {
            assert_eq!(asked(&fx, id), Some((942, 522)), "{id:?}");
        }
        assert_eq!(anchor(&d.c, a), (271, 111));
        assert_eq!(anchor(&d.c, b), (954 + 271, 111));
        assert_eq!(anchor(&d.c, c), (271, 534 + 111));
        assert_eq!(anchor(&d.c, e), (954 + 321, 534 + 161));
        // A draws the cell's size
        d.c.toplevel_committed(a, win(1, 942, 522, true)).unwrap();
        d.c.toplevel_size_drawn(
            a,
            Some(LocalSize { w: 942, h: 522 }),
            LocalSize { w: 942, h: 522 },
        );
        assert_eq!(rect(&d.c, a), cell(0, 0, 942, 522));
        // E acknowledges and keeps its size: centred, and no longer held to the cell
        d.c.toplevel_committed(e, win(4, 300, 200, true)).unwrap();
        d.c.toplevel_size_drawn(
            e,
            Some(LocalSize { w: 942, h: 522 }),
            LocalSize { w: 300, h: 200 },
        );
        assert_eq!(rect(&d.c, e), cell(954 + 321, 534 + 161, 300, 200));
        assert!(d.c.layout_cells.iter().all(|(x, _)| *x != a && *x != e));
        // B draws a frame of the old size before it acknowledged: still centred
        d.c.toplevel_committed(b, win(2, 400, 300, true)).unwrap();
        assert_eq!(anchor(&d.c, b), (954 + 271, 111));
    }

    /// Columns where one window keeps a bigger width than its cell (an app's minimum width):
    /// once both have answered, the cluster is tidied at the sizes they took and nothing covers
    /// anything (owner, 2026-11-22).
    #[test]
    fn a_window_bigger_than_its_cell_does_not_cover_its_neighbour() {
        let mut d = desk();
        let a = open(&mut d, 1, "a", (400, 300), (-500, -200));
        let b = open(&mut d, 2, "chat", (400, 300), (0, -200));
        let mut fx = Vec::new();
        assert!(d.c.arrange_cluster(b, Some(ClusterLayout::Columns), None, &mut fx));
        let (cw, ch) = asked(&fx, a).unwrap();
        let cell_size = Some(LocalSize { w: cw, h: ch });
        // A takes its cell; B acknowledges and stays wider than the cell
        d.c.toplevel_committed(a, win(1, cw, ch, true)).unwrap();
        d.c.toplevel_size_drawn(a, cell_size, LocalSize { w: cw, h: ch });
        d.c.toplevel_committed(b, win(2, cw + 300, ch, true))
            .unwrap();
        assert!(
            !all_apart(&[rect(&d.c, a), rect(&d.c, b)]),
            "centred over its neighbour"
        );
        d.c.toplevel_size_drawn(b, cell_size, LocalSize { w: cw + 300, h: ch });
        assert!(
            all_apart(&[rect(&d.c, a), rect(&d.c, b)]),
            "tidied: {:?} {:?}",
            rect(&d.c, a),
            rect(&d.c, b)
        );
        assert_eq!(
            rect(&d.c, b).w,
            i64::from(cw) + 300,
            "nothing asked to shrink again"
        );
    }

    /// Columns: two windows side by side, each half the room's width and its full height.
    #[test]
    fn columns_fill_the_usable_height() {
        let mut d = desk();
        let a = open(&mut d, 1, "a", (400, 300), (-500, -200));
        let b = open(&mut d, 2, "b", (400, 300), (0, -200));
        let mut fx = Vec::new();
        assert!(d.c.arrange_cluster(b, Some(ClusterLayout::Columns), None, &mut fx));
        assert_eq!(asked(&fx, a), Some((942, 1056)));
        assert_eq!(asked(&fx, b), Some((942, 1056)));
        d.c.toplevel_committed(a, win(1, 942, 1056, true)).unwrap();
        d.c.toplevel_committed(b, win(2, 942, 1056, true)).unwrap();
        assert_eq!(rect(&d.c, a), cell(-500, -200, 942, 1056));
        assert_eq!(rect(&d.c, b), cell(-500 + 954, -200, 942, 1056));
    }

    /// Kept-on-top, minimized and fullscreen windows are left where they are; a dialog moves with
    /// its parent.
    #[test]
    fn a_layout_leaves_out_what_is_not_on_the_canvas_and_dialogs_follow() {
        let mut d = desk();
        let q = open(&mut d, 1, "q", (400, 300), (-500, 0));
        let p = open(&mut d, 2, "p", (600, 400), (0, 0));
        let dialog = open(&mut d, 3, "p", (200, 100), (200, 150));
        d.c.set_toplevel_parent(dialog, Some(p));
        let top = open(&mut d, 4, "top", (300, 200), (100, 450));
        d.c.set_on_top(top, true);
        let mini = open(&mut d, 5, "mini", (300, 200), (-300, 350));
        let full = open(&mut d, 6, "full", (300, 200), (650, 100));
        let r = |c: &Core, id| ElementRef {
            instance: c.stage().instance(),
            element: id,
        };
        let element = r(&d.c, mini);
        d.c.shell_command(&mut d.s, ShellCommand::MinimizeElement { element })
            .unwrap();
        let element = r(&d.c, full);
        d.c.shell_command(
            &mut d.s,
            ShellCommand::FullscreenElement {
                element,
                output: Some(d.o),
                fullscreen: true,
            },
        )
        .unwrap();
        assert!(d.c.fullscreen_of(full).is_some());
        let before: Vec<_> = [top, mini, full].map(|e| anchor(&d.c, e)).to_vec();
        let mut fx = Vec::new();
        assert!(d.c.arrange_cluster(q, Some(ClusterLayout::Tidy), None, &mut fx));
        assert_eq!(anchor(&d.c, q), (-500, 0));
        assert_eq!(anchor(&d.c, p), (-500 + 412, 0));
        assert_eq!(
            anchor(&d.c, dialog),
            (200 - 88, 150),
            "the dialog moved with P"
        );
        let after: Vec<_> = [top, mini, full].map(|e| anchor(&d.c, e)).to_vec();
        assert_eq!(after, before);
        // pressed on the fullscreen window or the one kept on top: nothing is arranged
        let (q_at, p_at) = (anchor(&d.c, q), anchor(&d.c, p));
        for e in [full, top] {
            assert!(!d.c.arrange_cluster(e, Some(ClusterLayout::Grid), None, &mut fx));
        }
        assert_eq!((anchor(&d.c, q), anchor(&d.c, p)), (q_at, p_at));
        // pressed on the dialog, its parent's cluster is arranged
        assert!(d.c.arrange_cluster(dialog, Some(ClusterLayout::Columns), None, &mut fx));
        assert_eq!(asked(&fx, p), Some((942, 1056)));
        assert_eq!(asked(&fx, dialog), None);
    }

    /// Box: sizes kept, the biggest first, rows centred, a block near the room's shape.
    #[test]
    fn box_keeps_sizes_and_makes_a_block() {
        // four mains and twenty-one small windows, as the owner runs them
        let mut sizes = vec![(160, 90); 4];
        sizes.extend([(64, 48); 21]);
        let cells = boxed(&sizes, (-30, 70), (1896, 1056));
        assert!(all_apart(&cells));
        for (c, s) in cells.iter().zip(&sizes) {
            assert_eq!((c.w, c.h), *s, "sizes kept");
        }
        let x0 = cells.iter().map(|c| c.x).min().unwrap();
        let y0 = cells.iter().map(|c| c.y).min().unwrap();
        assert_eq!((x0, y0), (-30, 70), "the corner stays");
        let bw = cells.iter().map(|c| c.x + c.w).max().unwrap() - x0;
        let bh = cells.iter().map(|c| c.y + c.h).max().unwrap() - y0;
        let shape = bw as f64 / bh as f64;
        assert!(
            (1.0..3.0).contains(&shape),
            "a block, not a strip: {bw} x {bh}"
        );
        // the big ones are the top row
        let top = cells[..4].iter().map(|c| c.y).max().unwrap();
        assert!(cells[4..].iter().all(|c| c.y > top));
        assert!(boxed(&[], (0, 0), (1896, 1056)).is_empty());
    }

    /// Ring: the big windows in the middle, the small ones around them, nothing touching.
    #[test]
    fn ring_puts_small_windows_around_the_big_ones() {
        let mut sizes = vec![(160, 90); 4];
        sizes.extend([(64, 48); 21]);
        let cells = ring(&sizes, (100, -200), (1896, 1056));
        assert!(all_apart(&cells));
        for (c, s) in cells.iter().zip(&sizes) {
            assert_eq!((c.w, c.h), *s, "sizes kept");
        }
        let x0 = cells.iter().map(|c| c.x).min().unwrap();
        let y0 = cells.iter().map(|c| c.y).min().unwrap();
        assert_eq!((x0, y0), (100, -200));
        let mid = |c: &Cell| (c.x + c.w / 2, c.y + c.h / 2);
        let (l, t) = (
            cells[..4].iter().map(|c| c.x).min().unwrap(),
            cells[..4].iter().map(|c| c.y).min().unwrap(),
        );
        let (r, b) = (
            cells[..4].iter().map(|c| c.x + c.w).max().unwrap(),
            cells[..4].iter().map(|c| c.y + c.h).max().unwrap(),
        );
        for c in &cells[4..] {
            let (x, y) = mid(c);
            assert!(
                !(l..=r).contains(&x) || !(t..=b).contains(&y),
                "outside the middle"
            );
        }
        // the first small one is at the top, above the middle
        assert!(cells[4].y + cells[4].h <= t);
        // one kind of window: a box
        assert_eq!(
            ring(&[(64, 48); 5], (0, 0), (1896, 1056)),
            boxed(&[(64, 48); 5], (0, 0), (1896, 1056))
        );
        assert_eq!(ClusterLayout::parse("circle"), Some(ClusterLayout::Ring));
    }

    /// The key's cycle: tidy, grid, columns, box, ring, tidy again; another cluster starts at
    /// tidy.
    #[test]
    fn the_layout_key_cycles() {
        let mut d = desk();
        let a = open(&mut d, 1, "a", (400, 300), (0, 0));
        let b = open(&mut d, 2, "b", (400, 300), (5000, 0));
        let mut fx = Vec::new();
        let mut layouts = Vec::new();
        for _ in 0..6 {
            assert!(d.c.arrange_cluster(a, None, None, &mut fx));
            layouts.push(d.c.last_layout.unwrap().1);
        }
        assert_eq!(
            layouts,
            [
                ClusterLayout::Tidy,
                ClusterLayout::Grid,
                ClusterLayout::Columns,
                ClusterLayout::Box,
                ClusterLayout::Ring,
                ClusterLayout::Tidy
            ]
        );
        assert!(d.c.arrange_cluster(b, None, None, &mut fx));
        assert_eq!(d.c.last_layout, Some((b, ClusterLayout::Tidy)));
    }

    /// The shell's command: the focused window's cluster without an element; refused with no
    /// window focused and with nothing to arrange, and then nothing moved.
    #[test]
    fn the_shell_command_arranges_the_focused_cluster() {
        let mut d = desk();
        let a = open(&mut d, 1, "a", (400, 300), (-900, -500));
        let b = open(&mut d, 2, "b", (500, 400), (-700, -450));
        d.c.set_focus(&mut d.i, None).unwrap();
        let cmd = ShellCommand::ArrangeCluster {
            element: None,
            layout: ClusterLayout::Tidy,
        };
        assert_eq!(d.c.shell_command(&mut d.s, cmd), Err(ShellError::Refused));
        assert_eq!(anchor(&d.c, b), (-700, -450));
        d.c.set_focus(&mut d.i, Some(b)).unwrap();
        d.c.shell_command(&mut d.s, cmd).unwrap();
        assert_eq!(anchor(&d.c, a), (-900, -500));
        assert_eq!(anchor(&d.c, b), (-900 + 412, -500));
    }

    /// Super+Shift+G (owner, 2026-11-22): sizes kept, nothing asked to resize, one row per app.
    #[test]
    fn by_app_keeps_sizes_and_groups_apps() {
        let mut d = desk();
        let a = open(&mut d, 1, "game", (900, 600), (-900, -500));
        let b = open(&mut d, 2, "chat", (1200, 800), (-700, -450));
        let c = open(&mut d, 3, "game", (900, 600), (-500, -400));
        d.c.set_focus(&mut d.i, Some(b)).unwrap();
        let mut fx = Vec::new();
        assert!(d.c.arrange_cluster(b, Some(ClusterLayout::ByApp), None, &mut fx));
        for e in [a, b, c] {
            assert_eq!(asked(&fx, e), None, "no window is asked for another size");
        }
        assert_eq!(rect(&d.c, a), cell(-900, -500, 900, 600));
        assert_eq!(
            rect(&d.c, c),
            cell(-900 + 912, -500, 900, 600),
            "game beside game"
        );
        assert_eq!(
            rect(&d.c, b),
            cell(-900, -500 + 612, 1200, 800),
            "chat on the next row"
        );
    }

    /// Four windows in a row; B closes, then C is minimized: the others move only with
    /// CLUSTER_SORT "tidy", each time closing the gap the window left.
    #[test]
    fn cluster_sort_tidy_closes_the_gap_a_window_leaves() {
        for sort in ["off", "app", "tidy", "tidy app"] {
            let mut d = desk();
            d.c.set_cluster_sort(ClusterSort::parse(sort));
            let a = open(&mut d, 1, "a", (400, 300), (-900, 0));
            let b = open(&mut d, 2, "b", (400, 300), (-488, 0));
            let c = open(&mut d, 3, "c", (400, 300), (-76, 0));
            let e = open(&mut d, 4, "e", (400, 300), (336, 0));
            let tidy = sort.contains("tidy");
            // B closes: its client unmaps it, then destroys it
            d.c.toplevel_committed(b, win(2, 400, 300, false)).unwrap();
            d.c.toplevel_destroyed(b);
            assert_eq!(anchor(&d.c, a), (-900, 0), "{sort}");
            let want = if tidy {
                [(-488, 0), (-76, 0)]
            } else {
                [(-76, 0), (336, 0)]
            };
            assert_eq!(
                [anchor(&d.c, c), anchor(&d.c, e)],
                want,
                "{sort}: after B closed"
            );
            // C is minimized
            d.c.set_focus(&mut d.i, Some(c)).unwrap();
            d.c.client_minimize(c);
            assert!(d.c.stage().is_minimized(c));
            assert_eq!(anchor(&d.c, a), (-900, 0), "{sort}");
            let want = if tidy { (-488, 0) } else { (336, 0) };
            assert_eq!(anchor(&d.c, e), want, "{sort}: E after C was minimized");
        }
    }

    /// A window destroyed while still mapped tidies too, and one minimized by the shell; a
    /// dialog closing over its parent moves nothing.
    #[test]
    fn cluster_sort_tidy_on_destroy_and_not_for_a_dialog() {
        let mut d = desk();
        d.c.set_cluster_sort(ClusterSort::parse("tidy"));
        let a = open(&mut d, 1, "a", (400, 300), (-700, 0));
        let b = open(&mut d, 2, "b", (400, 300), (-200, 0));
        let dialog = open(&mut d, 3, "b", (200, 100), (-100, 100));
        d.c.set_toplevel_parent(dialog, Some(b));
        d.c.toplevel_destroyed(dialog);
        assert_eq!(anchor(&d.c, b), (-200, 0), "a dialog leaves no gap");
        d.c.toplevel_destroyed(a);
        assert_eq!(
            anchor(&d.c, b),
            (-200, 0),
            "B alone is its cluster's corner"
        );
        let c = open(&mut d, 4, "c", (400, 300), (300, 0));
        d.c.toplevel_destroyed(b);
        assert_eq!(anchor(&d.c, c), (300, 0));
        let e = open(&mut d, 5, "e", (400, 300), (-300, 0));
        d.c.toplevel_destroyed(e);
        assert_eq!(anchor(&d.c, c), (300, 0), "C keeps its corner");
        let f = open(&mut d, 6, "f", (400, 300), (712, 0));
        let g = open(&mut d, 7, "g", (400, 300), (1300, 0));
        d.c.toplevel_destroyed(f);
        assert_eq!(anchor(&d.c, g), (712, 0), "G closes the gap F left");
        // minimized from a taskbar or the edge (the shell's command)
        let h = open(&mut d, 8, "h", (400, 300), (1300, 0));
        let element = ElementRef {
            instance: d.c.stage().instance(),
            element: g,
        };
        d.c.shell_command(&mut d.s, ShellCommand::MinimizeElement { element })
            .unwrap();
        assert_eq!(anchor(&d.c, h), (712, 0), "H closes the gap G left");
    }

    /// CLUSTER_SORT "app": a new window of an app with a window in view opens right of it, the
    /// gap kept, instead of beside the cluster nearest the view's middle; not when that window is
    /// out of view, and not with the setting off.
    #[test]
    fn cluster_sort_app_opens_beside_the_apps_window() {
        let centre_spot = |sort: &str, far: bool| {
            let mut d = desk();
            d.c.set_cluster_sort(ClusterSort::parse(sort));
            let x = if far { 5000 } else { -900 };
            let _ = open(&mut d, 1, "foot", (400, 300), (x, -400));
            let _ = open(&mut d, 2, "other", (300, 200), (600, 200));
            let id = d.c.toplevel_created();
            d.c.toplevel_metadata(id, None, Some("foot")).unwrap();
            d.c.toplevel_committed(id, win(3, 400, 300, false)).unwrap();
            d.c.toplevel_committed(id, win(3, 400, 300, true)).unwrap();
            anchor(&d.c, id)
        };
        // without the setting, beside the cluster nearest the view's middle (the pointer is not
        // on the screen): foot's window, right of it at the middle's height
        assert_eq!(centre_spot("off", false), (-900 + 412, -150));
        assert_eq!(centre_spot("tidy", false), (-900 + 412, -150));
        assert_eq!(centre_spot("app", false), (-900 + 412, -400));
        assert_eq!(centre_spot("tidy app", false), (-900 + 412, -400));
        // foot's window out of view: beside the other one, left of it, bottom edges in line
        assert_eq!(centre_spot("app", true), (600 - 412, 400 - 300));
    }
}
