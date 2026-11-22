//! Cluster layouts (owner, 2026-11-21): the windows of a cluster (`Core::clusters`, windows
//! within 300 px of each other) arranged once when the person asks (a key, `infynitae msg
//! arrange`).
//!
//! - tidy: the windows at their own sizes, in reading order, packed in rows as wide as the usable
//!   view at zoom 1, 12 px apart;
//! - grid: equal cells, the columns chosen so that a cell is as square as the view allows;
//! - columns: side by side, each the usable height.
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
}

impl ClusterLayout {
    /// The layout's name as `infynitae msg arrange` takes it.
    pub fn parse(name: &str) -> Option<ClusterLayout> {
        match name.trim().to_ascii_lowercase().as_str() {
            "tidy" => Some(ClusterLayout::Tidy),
            "grid" => Some(ClusterLayout::Grid),
            "columns" => Some(ClusterLayout::Columns),
            _ => None,
        }
    }

    fn next(self) -> ClusterLayout {
        match self {
            ClusterLayout::Tidy => ClusterLayout::Grid,
            ClusterLayout::Grid => ClusterLayout::Columns,
            ClusterLayout::Columns => ClusterLayout::Tidy,
        }
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

/// The anchor of a window of `size` centred in `cell` (an app that keeps a size of its own).
pub(crate) fn centred_in(cell: Cell, size: (i64, i64)) -> crate::geom::CanvasPoint {
    crate::geom::CanvasPoint {
        x: cell.x + (cell.w - size.0) / 2,
        y: cell.y + (cell.h - size.1) / 2,
    }
}

impl Core {
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
        let order = reading_order(&rects);
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
    /// arranged.
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

    /// `id` drew a frame under the configure that asked for `acked`: a layout's cell is done
    /// with once its size was acknowledged, whatever size the app drew.
    pub(crate) fn layout_answered(&mut self, id: ElementId, acked: Option<crate::geom::LocalSize>) {
        self.layout_cells.retain(|(e, cell)| {
            *e != id || acked.is_none_or(|s| (i64::from(s.w), i64::from(s.h)) != (cell.w, cell.h))
        });
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
        // pressed on the dialog, its parent's cluster is arranged
        assert!(d.c.arrange_cluster(dialog, Some(ClusterLayout::Columns), None, &mut fx));
        assert_eq!(asked(&fx, p), Some((942, 1056)));
        assert_eq!(asked(&fx, dialog), None);
    }

    /// The key's cycle: tidy, grid, columns, tidy again; another cluster starts at tidy.
    #[test]
    fn the_layout_key_cycles() {
        let mut d = desk();
        let a = open(&mut d, 1, "a", (400, 300), (0, 0));
        let b = open(&mut d, 2, "b", (400, 300), (5000, 0));
        let mut fx = Vec::new();
        let mut layouts = Vec::new();
        for _ in 0..4 {
            assert!(d.c.arrange_cluster(a, None, None, &mut fx));
            layouts.push(d.c.last_layout.unwrap().1);
        }
        assert_eq!(
            layouts,
            [
                ClusterLayout::Tidy,
                ClusterLayout::Grid,
                ClusterLayout::Columns,
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
}
