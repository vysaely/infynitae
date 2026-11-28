//! Cluster layouts (owner, 2026-11-21): the windows of a cluster (`Core::clusters`, windows
//! within 300 px of each other) arranged once when the person asks (a key, `infynitae msg
//! arrange`). These are the only paths on which one window's change moves other windows
//! (docs/DECISIONS.md, 2026-11-06 and 2026-11-21). Nothing arranges by itself: a window that
//! closes or is minimized leaves its gap (owner, 2026-11-28).
//!
//! - grid (owner, 2026-11-28, in place of spiral, tiles and line): k windows in n x n slots,
//!   n = ceil(sqrt(k)) (1, 2x2, 3x3, ...), filled row by row, the focused window first and the
//!   others in reading order; the slots left over stay empty. A column is as wide as its widest
//!   window, a row as high as its tallest, the gap between; each window is centred in its slot.
//! - by app (Super+Shift+G): one row per app.
//!
//! Both keep every window's size (no configure is sent) and start at the cluster's top-left
//! corner. Fullscreen, minimized, kept-on-top and notification windows and children (dialogs)
//! are left out; a child moves with its parent.

use crate::Core;
use crate::protocol::Effect;
use crate::stage::ElementId;

/// The placement gap between windows (F-PLACE-1).
pub(crate) const GAP: i64 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClusterLayout {
    /// The layout key (`BIND_CLUSTER_LAYOUT`) and `infynitae msg arrange grid`.
    Grid,
    /// Super+Shift+G (owner, 2026-11-22): every window at its own size, one row per app, rows as
    /// wide as they need (not squeezed into one view: an app with a minimum size such as Discord
    /// keeps it).
    ByApp,
}

impl ClusterLayout {
    /// The layout's name as `infynitae msg arrange` takes it.
    pub fn parse(name: &str) -> Option<ClusterLayout> {
        match name.trim().to_ascii_lowercase().as_str() {
            "grid" => Some(ClusterLayout::Grid),
            "app" | "by-app" => Some(ClusterLayout::ByApp),
            _ => None,
        }
    }
}

/// `CLUSTER_SORT`: off by default, no window moves another unless the person asks (owner,
/// 2026-11-06).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClusterSort {
    /// "app": a new window opens beside its app's newest window in the first screen's view.
    pub app: bool,
}

impl ClusterSort {
    /// The config's words separated by spaces or commas: "app" turns it on; "off", an empty value
    /// and other words (the removed "tidy", which live/config-check names) turn nothing on.
    pub fn parse(value: &str) -> ClusterSort {
        ClusterSort {
            app: value
                .split([' ', ',', '\t'])
                .any(|word| word.eq_ignore_ascii_case("app")),
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

/// Grid: the slots of `sizes` in order, n x n from `origin` with n = ceil(sqrt(len)), filled row
/// by row; each column as wide as its widest window, each row as high as its tallest, `GAP`
/// between. Slots in order (one per size; the empty ones are not returned).
pub(crate) fn grid(sizes: &[(i64, i64)], origin: (i64, i64)) -> Vec<Cell> {
    let mut n = 1;
    while n * n < sizes.len() {
        n += 1;
    }
    let (mut col_w, mut row_h) = (vec![0; n], vec![0; n]);
    for (i, &(w, h)) in sizes.iter().enumerate() {
        col_w[i % n] = col_w[i % n].max(w);
        row_h[i / n] = row_h[i / n].max(h);
    }
    let starts = |from: i64, sides: &[i64]| -> Vec<i64> {
        let mut at = from;
        sides
            .iter()
            .map(|&s| {
                let start = at;
                at += s + GAP;
                start
            })
            .collect()
    };
    let (xs, ys) = (starts(origin.0, &col_w), starts(origin.1, &row_h));
    (0..sizes.len())
        .map(|i| Cell {
            x: xs[i % n],
            y: ys[i / n],
            w: col_w[i % n],
            h: row_h[i / n],
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

/// The anchor of a window of `size` centred in `cell`.
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

    /// Arranges `members` by `layout`, `first` in the grid's first slot. Each window keeps its
    /// size and is centred in its slot; its children move with it. Returns the slots' bounding
    /// box `(x, y, w, h)`, or `None` when no window is arranged.
    pub(crate) fn arrange(
        &mut self,
        members: &[ElementId],
        layout: ClusterLayout,
        first: ElementId,
        out: &mut Vec<Effect>,
    ) -> Option<(i64, i64, i64, i64)> {
        let (ids, rects): (Vec<ElementId>, Vec<Cell>) = self.arranged(members).into_iter().unzip();
        let mut order = reading_order(&rects);
        let app_of = |i: usize| -> String {
            self.stage
                .meta(ids[i])
                .and_then(|m| m.app_id.clone())
                .unwrap_or_default()
        };
        match layout {
            ClusterLayout::Grid => {
                let lead = ids.iter().position(|&e| e == first)?;
                order.retain(|&i| i != lead);
                order.insert(0, lead);
            }
            // each app's windows together, the apps in the order their first window reads
            ClusterLayout::ByApp => {
                let mut firsts: Vec<String> = Vec::new();
                for &i in &order {
                    let a = app_of(i);
                    if !firsts.contains(&a) {
                        firsts.push(a);
                    }
                }
                order.sort_by_key(|&i| firsts.iter().position(|a| *a == app_of(i)));
            }
        }
        let origin = (
            rects.iter().map(|r| r.x).min()?,
            rects.iter().map(|r| r.y).min()?,
        );
        let sizes: Vec<_> = order.iter().map(|&i| (rects[i].w, rects[i].h)).collect();
        let cells = match layout {
            ClusterLayout::Grid => grid(&sizes, origin),
            ClusterLayout::ByApp => {
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
            let at = centred_in(cell, (old.w, old.h));
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

    /// The layout key (`BIND_CLUSTER_LAYOUT`, no default), Super+Shift+G and `infynitae msg
    /// arrange`: `around`'s cluster (its parent's, for a child) arranged by `layout`. With
    /// `proof`, the camera frames the result. False when nothing was arranged, and when `around`
    /// is a window a layout leaves out (a fullscreen game focused: the windows hidden behind it
    /// stay as they are).
    pub(crate) fn arrange_cluster(
        &mut self,
        around: ElementId,
        layout: ClusterLayout,
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
        let cluster = self.cluster_for_element(around);
        let Some(bounds) = self.arrange(&cluster, layout, around, out) else {
            return false;
        };
        if let Some(p) = proof {
            self.frame_bounds(p, bounds, out);
        }
        true
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

    /// One 1920 x 1080 screen at scale 1, camera at the origin.
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

    /// Five windows: 3 x 3 slots, two rows used and four slots left empty; a column as wide as
    /// its widest window, a row as high as its tallest, the gap between. n x n for n = 1..=4.
    #[test]
    fn grid_fills_square_slots_row_by_row() {
        let sizes = [(400, 300), (200, 100), (300, 500), (100, 100), (250, 200)];
        assert_eq!(
            grid(&sizes, (10, 20)),
            vec![
                cell(10, 20, 400, 500),
                cell(10 + 412, 20, 250, 500),
                cell(10 + 412 + 262, 20, 300, 500),
                cell(10, 20 + 512, 400, 200),
                cell(10 + 412, 20 + 512, 250, 200),
            ]
        );
        for (k, n) in [(1, 1), (2, 2), (4, 2), (5, 3), (9, 3), (10, 4), (16, 4)] {
            let cells = grid(&vec![(100, 100); k], (0, 0));
            let cols = cells.iter().filter(|c| c.y == 0).count();
            assert_eq!(cols, n, "{k} windows: {n} columns");
            assert!(cells.iter().all(|c| c.x <= (n as i64 - 1) * 112));
            assert!(all_apart(&cells));
        }
        assert!(grid(&[], (0, 0)).is_empty());
    }

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

    /// "tidy" was removed (owner, 2026-11-28): read as nothing, beside "app" too.
    #[test]
    fn cluster_sort_reads_its_words() {
        let app = ClusterSort { app: true };
        assert_eq!(ClusterSort::parse("off"), ClusterSort::default());
        assert_eq!(ClusterSort::parse(""), ClusterSort::default());
        assert_eq!(ClusterSort::parse("tidy"), ClusterSort::default());
        assert_eq!(ClusterSort::parse("Tidy App"), app);
        assert_eq!(ClusterSort::parse("app,tidy"), app);
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
        assert!(d.c.arrange_cluster(q, ClusterLayout::Grid, None, &mut fx));
        // Q and P in one row, the row as high as P: Q centred in its slot
        assert_eq!(anchor(&d.c, q), (-500, 50));
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
            assert!(!d.c.arrange_cluster(e, ClusterLayout::Grid, None, &mut fx));
        }
        assert_eq!((anchor(&d.c, q), anchor(&d.c, p)), (q_at, p_at));
        // pressed on the dialog, its parent's cluster is arranged, the parent first
        assert!(d.c.arrange_cluster(dialog, ClusterLayout::Grid, None, &mut fx));
        assert_eq!(anchor(&d.c, p), (-500, 0));
        assert_eq!(anchor(&d.c, q), (-500 + 612, 50));
        assert_eq!(anchor(&d.c, dialog), (200 - 500, 150));
        for e in [p, q, dialog] {
            assert_eq!(asked(&fx, e), None, "no window is asked for another size");
        }
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
            layout: ClusterLayout::Grid,
        };
        assert_eq!(d.c.shell_command(&mut d.s, cmd), Err(ShellError::Refused));
        assert_eq!(anchor(&d.c, b), (-700, -450));
        d.c.set_focus(&mut d.i, Some(b)).unwrap();
        d.c.shell_command(&mut d.s, cmd).unwrap();
        // B, focused, in the first slot; A beside it, centred in a slot as high as B
        assert_eq!(anchor(&d.c, b), (-900, -500));
        assert_eq!(anchor(&d.c, a), (-900 + 512, -450));
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
        assert!(d.c.arrange_cluster(b, ClusterLayout::ByApp, None, &mut fx));
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

    /// Nothing arranges by itself (owner, 2026-11-28): a window that closes, is destroyed while
    /// mapped or is minimized (by its app or the shell) leaves its gap, whatever `CLUSTER_SORT`
    /// says, "tidy" from an old config included.
    #[test]
    fn a_window_leaving_its_cluster_moves_nothing() {
        for sort in ["off", "tidy", "tidy app"] {
            let mut d = desk();
            d.c.set_cluster_sort(ClusterSort::parse(sort));
            let a = open(&mut d, 1, "a", (400, 300), (-900, 0));
            let b = open(&mut d, 2, "b", (400, 300), (-488, 0));
            let c = open(&mut d, 3, "c", (400, 300), (-76, 0));
            let e = open(&mut d, 4, "e", (400, 300), (336, 0));
            let f = open(&mut d, 5, "f", (400, 300), (748, 0));
            let g = open(&mut d, 6, "g", (400, 300), (1160, 0));
            // B closes: its client unmaps it, then destroys it
            d.c.toplevel_committed(b, win(2, 400, 300, false)).unwrap();
            d.c.toplevel_destroyed(b);
            // E is destroyed while mapped
            d.c.toplevel_destroyed(e);
            // C is minimized by its app, F by the shell
            d.c.set_focus(&mut d.i, Some(c)).unwrap();
            d.c.client_minimize(c);
            assert!(d.c.stage().is_minimized(c));
            let element = ElementRef {
                instance: d.c.stage().instance(),
                element: f,
            };
            d.c.shell_command(&mut d.s, ShellCommand::MinimizeElement { element })
                .unwrap();
            assert_eq!(anchor(&d.c, a), (-900, 0), "{sort}");
            assert_eq!(anchor(&d.c, g), (1160, 0), "{sort}");
        }
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
