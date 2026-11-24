//! Where a window goes: at first map, beside a cluster, on top, after a close, moved by the shell or the user.

use super::*;

impl Core {
    /// C7.2 `MoveElement` after its validation (`Core::shell_command`): the anchor is written here,
    /// in the Stage's single writer (INV-STAGE-1), then the element's membership and the batch's
    /// one scale evaluation follow, as for any change of one element.
    pub(crate) fn place_by_shell(
        &mut self,
        id: ElementId,
        anchor: CanvasPoint,
        out: &mut Vec<Effect>,
    ) -> Result<(), ProtocolError> {
        self.stage
            .place(id, anchor)
            .map_err(|_| ProtocolError::StaleElement)?;
        self.invalidate_all_frames();
        // (spelled as a reborrow: the commit path's call is a seeded-control anchor)
        self.membership_pass_element(id, &mut *out);
        out.extend(self.evaluate_scale_policy(false));
        Ok(())
    }

    /// The usable view new windows open in, in Canvas px, `(x0, y0, x1, y1)`: what the camera of
    /// the pointer's screen (else the first) shows less the layers' exclusive zones (bars).
    fn usable_view(&self) -> Option<(i64, i64, i64, i64)> {
        self.placement_view().map(|(view, _)| view)
    }

    /// [`Core::usable_view`] and the pointer on the Canvas when it is on that screen, both where
    /// the camera comes to rest: during a glide (`ZOOM_FEEL` other than "instant") a window was
    /// kept inside where the camera was at that moment and ended off screen once it stopped.
    fn placement_view(&self) -> Option<(crate::stage::CanvasBox, Option<(i64, i64)>)> {
        let output = self
            .pointer_output()
            .and_then(|o| self.outputs.iter().find(|m| m.id == o))
            .or_else(|| self.outputs.first())?;
        let camera = self.cameras.camera_for_output(output.id)?;
        let rest = self.cameras.effective_value(camera)?;
        let zoom = rest.zoom;
        let extent = output.logical_extent();
        let (ux, uy, uw, uh) = self
            .usable_area(output.id)
            .unwrap_or((0.0, 0.0, extent.0, extent.1));
        let (cx, cy) = (
            rest.center.x as f64 / Q16_ONE as f64,
            rest.center.y as f64 / Q16_ONE as f64,
        );
        let x0 = cx + (ux - extent.0 / 2.0) / zoom;
        let y0 = cy + (uy - extent.1 / 2.0) / zoom;
        let view = (
            x0.ceil() as i64,
            y0.ceil() as i64,
            (x0 + uw / zoom).floor() as i64,
            (y0 + uh / zoom).floor() as i64,
        );
        let pointer = match self.pointer {
            pointer::PointerState::Located { output: o, p } if o == output.id => Some((
                (cx + (p.0 - extent.0 / 2.0) / zoom).round() as i64,
                (cy + (p.1 - extent.1 / 2.0) / zoom).round() as i64,
            )),
            _ => None,
        };
        Some((view, pointer))
    }

    /// A window that opened larger than its room and was offered the room (the adapter's fit,
    /// `fit_opening_window`) is placed again as if it had opened at that size (F-PLACE-1): its
    /// first place was chosen for a size it never keeps, and would leave the fitted window off
    /// screen.
    pub fn place_opening_at_size(
        &mut self,
        id: ElementId,
        size: (i32, i32),
        out: &mut EffectBuf,
    ) -> Result<(), ProtocolError> {
        let mut tree = self
            .stage
            .tree(id)
            .ok_or(ProtocolError::StaleElement)?
            .clone();
        tree.geometry.size = LocalSize {
            w: size.0,
            h: size.1,
        };
        let Some(anchor) = self.first_map_anchor(id, &tree) else {
            return Ok(());
        };
        // like any new window: in free space near the cursor, nothing moved (owner, 2026-10-31;
        // before, a window that filled the room took the middle of the view and pushed the
        // others aside)
        self.place_by_shell(id, anchor, &mut out.fx)
    }

    /// Super+left drag: the window's anchor follows the pointer, written here, in the Stage's single
    /// writer (INV-STAGE-1), with the same consequences as a shell `MoveElement` (membership, one
    /// scale evaluation).
    pub(crate) fn place_by_user(
        &mut self,
        id: ElementId,
        anchor: CanvasPoint,
        out: &mut Vec<Effect>,
    ) -> Result<(), ProtocolError> {
        // where the user puts it wins over a layout's cell it has not answered yet
        self.layout_cells.retain(|(e, _)| *e != id);
        self.place_by_shell(id, anchor, out)
    }

    /// The anchor written at the initial commit (INV-STAGE-1, C12.F1): the bound camera's centre
    /// of the first connected output (lowest `OutputId`), rounded R3; the Canvas origin without
    /// an output. Under the default policy it is provisional until the first map; under
    /// [`PlacementPolicy::OriginAtCameraCentre`] it is final.
    pub(super) fn initial_anchor(&self) -> CanvasPoint {
        self.outputs
            .first()
            .and_then(|o| self.cameras.camera_for_output(o.id))
            .and_then(|c| self.cameras.center(c))
            .and_then(|c| c.round_to_point())
            .unwrap_or(CanvasPoint { x: 0, y: 0 })
    }

    /// Default placement at first map, when the window-geometry size is known (F-PLACE-1).
    ///
    /// A child window opens centred over its mapped parent's geometry (owner, 2026-10-24).
    /// A window replacing a closed window of the same app reuses that spot if still in view
    /// (owner, 2026-10-25).
    ///
    /// Under [`PlacementPolicy::CentredInView`], the window geometry is centred in the usable
    /// region of the placement output (first connected output), in Canvas coordinates through its
    /// camera: per axis, with `c` the camera centre, `u` the usable logical extent and `s` the outer
    /// size, `anchor = c − u/2 + (u − s)/2 = c − s/2` when `s ≤ u`. An oversized window aligns to the
    /// region's origin (`anchor = c − u/2`) so the top-left corner stays reachable. Rounded by R3
    /// ([`CanvasPos::round_to_point`]). The usable region is the output view less layer surfaces'
    /// exclusive zones (L4).
    ///
    /// Under [`PlacementPolicy::ClusterTiling`], the window is placed at the nearest free slot
    /// in the cluster (or centred if empty).
    ///
    /// `None` keeps the anchor of the initial commit: under [`PlacementPolicy::OriginAtCameraCentre`],
    /// without an output, or when out of Canvas bounds.
    ///
    /// Whatever placed it, a new window never covers another window (owner, 2026-11-07: "this
    /// should cover any app at compositor level"; 2026-11-16: "in any scenario"): a spot that
    /// would goes to the nearest free one. Only a child may cover its own parent; a notification
    /// stacks in its corner; the `center` policy stacks every window in the middle by choice.
    pub(super) fn first_map_anchor(
        &self,
        id: ElementId,
        tree: &SurfaceTree,
    ) -> Option<CanvasPoint> {
        if self.stage.is_above(id) {
            return self
                .on_top_anchor(tree)
                .or_else(|| self.first_map_spot(id, tree));
        }
        let at = self.first_map_spot(id, tree)?;
        // `center` asks for every window in the middle of the view, stacked: the rule is off there
        if self.is_notification(id) || self.placement == PlacementPolicy::CentredInView {
            return Some(at);
        }
        let size = (
            i64::from(tree.geometry.size.w),
            i64::from(tree.geometry.size.h),
        );
        if size.0 <= 0 || size.1 <= 0 {
            return Some(at);
        }
        Some(self.free_place_for(id, size, at).unwrap_or(at))
    }

    /// [`Core::bring_into_view`]: a mapped window mostly outside the usable view (less than
    /// `SEEN` of it inside) moves into it; one larger than the view to its top left corner. A
    /// fullscreen window, a notification and a window kept above are left alone.
    pub(crate) fn bring_into_view_now(&mut self, id: ElementId) {
        const SEEN: f64 = 0.6;
        if self.fullscreen_of(id).is_some() || self.is_notification(id) || self.stage.is_above(id) {
            return;
        }
        let (Some(at), Some(tree), Some((x0, y0, x1, y1))) = (
            self.stage.anchor(id),
            self.stage.tree(id),
            self.usable_view(),
        ) else {
            return;
        };
        let (w, h) = (
            i64::from(tree.geometry.size.w),
            i64::from(tree.geometry.size.h),
        );
        if w <= 0 || h <= 0 {
            return;
        }
        let seen_w = (at.x + w).min(x1) - at.x.max(x0);
        let seen_h = (at.y + h).min(y1) - at.y.max(y0);
        if seen_w > 0 && seen_h > 0 && (seen_w as f64 * seen_h as f64) >= SEEN * (w * h) as f64 {
            return;
        }
        let fit = |p: i64, lo: i64, hi: i64, s: i64| {
            if hi - lo <= s {
                lo
            } else {
                p.clamp(lo, hi - s)
            }
        };
        // the nearest spot in the view, and the view's corners and edges: the one covering the
        // other windows least (the nearest of those that tie)
        let others = self.window_rects_except(id);
        let covered = |x: i64, y: i64| -> i64 {
            others
                .iter()
                .map(|&(ox0, oy0, ox1, oy1)| {
                    ((x + w).min(ox1) - x.max(ox0)).max(0) * ((y + h).min(oy1) - y.max(oy0)).max(0)
                })
                .sum()
        };
        let (cx, cy) = ((x0 + x1 - w) / 2, (y0 + y1 - h) / 2);
        let to = [
            (at.x, at.y),
            (x0, y0),
            (x1 - w, y0),
            (x0, y1 - h),
            (x1 - w, y1 - h),
            (cx, y0),
            (cx, y1 - h),
            (x0, cy),
            (x1 - w, cy),
        ]
        .into_iter()
        .map(|(x, y)| (fit(x, x0, x1, w), fit(y, y0, y1, h)))
        .min_by_key(|&(x, y)| {
            let (dx, dy) = ((x - at.x) as i128, (y - at.y) as i128);
            (covered(x, y), dx * dx + dy * dy)
        })
        .map(|(x, y)| CanvasPoint { x, y })
        .unwrap_or(at);
        if to != at && self.stage.place(id, to).is_ok() {
            self.invalidate_all_frames();
        }
    }

    /// Where a window opens before the rule that it covers no other app's window (`first_map_anchor`).
    fn first_map_spot(&self, id: ElementId, tree: &SurfaceTree) -> Option<CanvasPoint> {
        // an app's notification: the view's bottom right corner, above the ones already there
        if self.is_notification(id) {
            return self.notification_anchor(id, tree);
        }
        // a child window (xdg_toplevel.set_parent: dialogs, an app's second window) opens centred
        // over its mapped parent's window geometry, whatever the policy and whatever its size, kept
        // in the view. It may cover its parent and its app's other windows (owner, 2026-10-24 and
        // 2026-11-03), never another app's: one that would (Discord's main window, a child of its
        // small updater) goes to the nearest spot free of them (owner, 2026-11-07)
        if let Some(parent) = self.toplevel_parent(id)
            && self.stage.mapped(parent).is_some()
            && let (Some(a), Some(pt)) = (self.stage.anchor(parent), self.stage.tree(parent))
        {
            let (g, pg) = (tree.geometry, pt.geometry);
            let centre_x = a.x + i64::from(pg.loc.x) + i64::from(pg.size.w) / 2;
            let centre_y = a.y + i64::from(pg.loc.y) + i64::from(pg.size.h) / 2;
            let (mut x, mut y) = (
                centre_x - i64::from(g.size.w) / 2,
                centre_y - i64::from(g.size.h) / 2,
            );
            if let Some(view) = self.usable_view() {
                x = keep_in_range(x, i64::from(g.size.w), view.0, view.2);
                y = keep_in_range(y, i64::from(g.size.h), view.1, view.3);
            }
            return Some(CanvasPoint {
                x: x - i64::from(g.loc.x),
                y: y - i64::from(g.loc.y),
            });
        }
        if self.placement == PlacementPolicy::OriginAtCameraCentre {
            return None;
        }
        // an app swapping its own windows keeps the place (Steam: updater, then login, then the
        // main window; owner, 2026-10-25): no input since the close, no other window of the app
        // open, the place still in view and free. Anything else opens beside the windows in view.
        if let Some(at) = self.successor_anchor(id, tree) {
            return Some(at);
        }
        if self.placement == PlacementPolicy::CentredInView {
            let output = self.outputs.first()?;
            let centre = self
                .cameras
                .camera_for_output(output.id)
                .and_then(|c| self.cameras.center(c))?;
            // the usable region (L4): the output view less its layers' exclusive zones, centred at
            // its own centre, which lies `d` from the view's (Canvas px at the camera's zoom)
            let extent = output.logical_extent();
            let (ux, uy, uw, uh) = self
                .usable_area(output.id)
                .unwrap_or((0.0, 0.0, extent.0, extent.1));
            let zoom = self
                .cameras
                .camera_for_output(output.id)
                .and_then(|c| self.cameras.zoom(c))
                .unwrap_or(1.0);
            let d = (
                (ux + uw / 2.0 - extent.0 / 2.0) / zoom,
                (uy + uh / 2.0 - extent.1 / 2.0) / zoom,
            );
            let usable = (uw, uh);
            let axis = |c: i64, usable: f64, outer: i32| {
                let half_outer = i64::from(outer.max(0)) * Q16_ONE / 2;
                let half_usable = geom::q16(usable / 2.0);
                c - half_outer.min(half_usable)
            };
            let center_anchor = CanvasPos {
                x: axis(
                    centre.x.saturating_add(geom::q16(d.0)),
                    usable.0,
                    tree.geometry.size.w,
                ),
                y: axis(
                    centre.y.saturating_add(geom::q16(d.1)),
                    usable.1,
                    tree.geometry.size.h,
                ),
            }
            .round_to_point()?;
            return Some(center_anchor);
        }

        // PlacementPolicy::ClusterTiling (owner, 2026-11-22)
        let (view, pointer) = self.placement_view()?;
        let size = (
            i64::from(tree.geometry.size.w),
            i64::from(tree.geometry.size.h),
        );
        let middle = ((view.0 + view.2) / 2, (view.1 + view.3) / 2);
        // the window's top left corner for its centre at `c`, a window that fits kept inside the
        // usable view; one that does not keeps its top left there, so it stays reachable
        let centred_at = |c: (i64, i64)| {
            (
                keep_in_range(c.0 - size.0 / 2, size.0, view.0, view.2),
                keep_in_range(c.1 - size.1 / 2, size.1, view.1, view.3),
            )
        };
        if size.0 <= 0 || size.1 <= 0 {
            return Some(CanvasPoint {
                x: middle.0,
                y: middle.1,
            });
        }
        let existing = self.window_rects_except(id);
        // CLUSTER_SORT "app": right of its app's window in view, else the nearest free spot
        if let Some(beside) = self.beside_own_app(id) {
            let at = centred_at((beside.0 + size.0 / 2, beside.1 + size.1 / 2));
            let (x, y) = if overlaps_any(&existing, (at.0, at.1, size.0, size.1)) {
                self.free_spot_near(&existing, size, at)
                    .unwrap_or_else(|| beside_all(&existing, at))
            } else {
                at
            };
            return Some(CanvasPoint { x, y });
        }
        // no window in this view: the middle of the usable view, wherever the pointer rests
        // (owner, 2026-11-22: "in the middle of the viewport if it's the first in that viewport")
        let Some(cluster) = self.anchor_cluster(id, view, pointer.unwrap_or(middle)) else {
            let (x, y) = centred_at(middle);
            return Some(CanvasPoint { x, y });
        };
        // beside that cluster, on its side nearest the pointer: the nearest free spot touching
        // it, one wholly on screen first, else beyond the view, still touching it. Only free
        // Canvas is taken: no other window moves (owner, 2026-11-22)
        let target = centred_at(pointer.unwrap_or(middle));
        let (x, y) = self
            .free_spot_beside(&cluster, &existing, size, target)
            .or_else(|| self.free_spot_near(&existing, size, target))
            .unwrap_or_else(|| beside_all(&existing, target));
        Some(CanvasPoint { x, y })
    }

    /// A window that maps in fullscreen keeps its place on the Canvas off the other windows at
    /// its fullscreen size: it shows there when it steps aside, in the overview and on the other
    /// screens' cameras. Until 2026-11-19 its corner stayed at the view's centre, over whatever
    /// was there. Its windowed place is still chosen at its first windowed frame (P9b). `None`:
    /// the spot is free, or the policy stacks windows.
    pub(super) fn fullscreen_map_spot(
        &self,
        id: ElementId,
        tree: &SurfaceTree,
    ) -> Option<CanvasPoint> {
        if self.placement == PlacementPolicy::CentredInView
            || self.placement == PlacementPolicy::OriginAtCameraCentre
        {
            return None;
        }
        let size = (
            i64::from(tree.geometry.size.w),
            i64::from(tree.geometry.size.h),
        );
        if size.0 <= 0 || size.1 <= 0 {
            return None;
        }
        self.free_place_for(id, size, self.stage.anchor(id)?)
    }

    /// A window granted fullscreen as it opened takes the focus with the screen (2026-11-19):
    /// until then the keys went to the window it hid.
    pub(super) fn focus_opener(&mut self, id: ElementId) -> Vec<Effect> {
        let mut fx = self.focus.set_focus_at(self.input.clock.now_us(), Some(id));
        fx.extend(self.reconcile_constraints());
        fx
    }

    /// Where a remapped window goes when its kept anchor (INV-STAGE-5) now covers another window;
    /// `None` keeps it. Fullscreen, kept-on-top and notification windows and the `center` policy
    /// are left as they are, as at the first map.
    pub(super) fn free_place_on_remap(
        &self,
        id: ElementId,
        tree: &SurfaceTree,
    ) -> Option<CanvasPoint> {
        if self.fullscreen_of(id).is_some()
            || self.stage.is_above(id)
            || self.is_notification(id)
            || self.placement == PlacementPolicy::CentredInView
        {
            return None;
        }
        let at = self.stage.anchor(id)?;
        let size = (
            i64::from(tree.geometry.size.w),
            i64::from(tree.geometry.size.h),
        );
        if size.0 <= 0 || size.1 <= 0 {
            return None;
        }
        self.free_place_for(id, size, at)
    }

    /// A spot for `id` at `size` near `at` that covers no other window, when `at` would:
    /// the nearest free one, else beside them all; `None` when `at` covers none.
    pub(crate) fn free_place_for(
        &self,
        id: ElementId,
        size: (i64, i64),
        at: CanvasPoint,
    ) -> Option<CanvasPoint> {
        let parent = self
            .toplevel_parent(id)
            .filter(|p| self.stage.mapped(*p).is_some())
            .unwrap_or(id);
        let strangers = self.other_apps_rects(id, parent);
        if !overlaps_any(&strangers, (at.x, at.y, size.0, size.1)) {
            return None;
        }
        let (x, y) = self
            .free_spot_near(&strangers, size, (at.x, at.y))
            .unwrap_or_else(|| beside_all(&strangers, (at.x, at.y)));
        Some(CanvasPoint { x, y })
    }

    /// The window-geometry rects a new window must keep off: every mapped window but `id`, its
    /// `parent` (a dialog opens over its own parent) and windows kept on top (they float over the
    /// view, not on the Canvas). Windows of the same app count like any other (owner, 2026-11-16:
    /// "make all windows appear not on top of each other in any scenario"): a second Firefox
    /// window no longer opens over the first.
    fn other_apps_rects(&self, id: ElementId, parent: ElementId) -> Vec<(i64, i64, i64, i64)> {
        let mut out = Vec::new();
        for elem in self.stage.mapped_elements() {
            if elem == id || elem == parent || self.stage.is_above(elem) {
                continue;
            }
            if let (Some(a), Some(t)) = (self.stage.anchor(elem), self.stage.tree(elem)) {
                let (w, h) = (i64::from(t.geometry.size.w), i64::from(t.geometry.size.h));
                if w > 0 && h > 0 {
                    out.push((a.x, a.y, a.x.saturating_add(w), a.y.saturating_add(h)));
                }
            }
        }
        out
    }

    /// The window-geometry rects `(x0, y0, x1, y1)` of every mapped window but `id`.
    fn window_rects_except(&self, id: ElementId) -> Vec<(i64, i64, i64, i64)> {
        let mut existing = Vec::new();
        for elem in self.stage.mapped_elements() {
            if elem == id {
                continue;
            }
            if let Some(a) = self.stage.anchor(elem)
                && let Some(t) = self.stage.tree(elem)
            {
                let ew = t.geometry.size.w as i64;
                let eh = t.geometry.size.h as i64;
                if ew > 0 && eh > 0 {
                    let gx0 = a.x;
                    let gy0 = a.y;
                    existing.push((gx0, gy0, gx0.saturating_add(ew), gy0.saturating_add(eh)));
                }
            }
        }
        existing
    }

    /// The cluster a new window opens beside (owner, 2026-11-22), the window-geometry rects of
    /// its windows: the focused window's when it shows in `view`; else, of the clusters with a
    /// window in `view`, the one nearest `near` (the pointer, or the view's middle). `None`: no
    /// window shows in `view`. Windows kept on top, minimized or fullscreen are in no cluster.
    fn anchor_cluster(
        &self,
        id: ElementId,
        view: (i64, i64, i64, i64),
        near: (i64, i64),
    ) -> Option<Vec<(i64, i64, i64, i64)>> {
        let mut elems = Vec::new();
        let mut rects = Vec::new();
        for e in self.stage.mapped_elements() {
            if e == id
                || self.stage.is_above(e)
                || self.stage.is_minimized(e)
                || self.fullscreen_of(e).is_some()
            {
                continue;
            }
            if let (Some(a), Some(t)) = (self.stage.anchor(e), self.stage.tree(e)) {
                let (w, h) = (i64::from(t.geometry.size.w), i64::from(t.geometry.size.h));
                if w > 0 && h > 0 {
                    elems.push(e);
                    rects.push((a.x, a.y, a.x.saturating_add(w), a.y.saturating_add(h)));
                }
            }
        }
        let shows =
            |r: &(i64, i64, i64, i64)| r.0 < view.2 && r.2 > view.0 && r.1 < view.3 && r.3 > view.1;
        let groups: Vec<Vec<usize>> = crate::groups_near(&rects)
            .into_iter()
            .filter(|g| g.iter().any(|&i| shows(&rects[i])))
            .collect();
        let focused = self.focus.target().and_then(|f| {
            let i = elems.iter().position(|&e| e == f)?;
            groups.iter().find(|g| g.contains(&i) && shows(&rects[i]))
        });
        let distance = |g: &&Vec<usize>| {
            g.iter()
                .map(|&i| {
                    let (x0, y0, x1, y1) = rects[i];
                    let dx = i128::from((x0 - near.0).max(near.0 - x1).max(0));
                    let dy = i128::from((y0 - near.1).max(near.1 - y1).max(0));
                    dx * dx + dy * dy
                })
                .min()
        };
        let group = focused.or_else(|| groups.iter().min_by_key(distance))?;
        Some(group.iter().map(|&i| rects[i]).collect())
    }

    /// The free spot nearest `target` for a window of `size` among `existing` (the cluster
    /// placement's search, F-PLACE-1): grid-aligned candidates flush with the windows' edges and
    /// the target's lines, the 12 px gap kept; a spot wholly in the usable view wins over a
    /// nearer one that is not. `None` when no candidate is free.
    pub(crate) fn free_spot_near(
        &self,
        existing: &[(i64, i64, i64, i64)],
        size: (i64, i64),
        target: (i64, i64),
    ) -> Option<(i64, i64)> {
        let in_view = self.wholly_in_view(size);
        spot_candidates(existing, existing, size, target)
            .into_iter()
            .filter(|&(gx, gy)| !overlaps_any(existing, (gx, gy, size.0, size.1)))
            .min_by_key(|&(gx, gy)| {
                let dx = (gx - target.0) as i128;
                let dy = (gy - target.1) as i128;
                (!in_view(gx, gy), dx * dx + dy * dy)
            })
    }

    /// [`Core::free_spot_near`] among the spots touching one of the rects `around` (the gap
    /// kept), free of all of `existing`. Among spots about as near, one whose edges line up with
    /// a neighbour's is preferred (owner, 2026-11-22): the same top or bottom edge beside it,
    /// the same left or right edge above or below it, each worth `ALIGN` px of distance.
    fn free_spot_beside(
        &self,
        around: &[(i64, i64, i64, i64)],
        existing: &[(i64, i64, i64, i64)],
        size: (i64, i64),
        target: (i64, i64),
    ) -> Option<(i64, i64)> {
        const ALIGN: i64 = 32;
        let (w, h) = size;
        // `r` is within the gap of the spot at `(x, y)` on one axis and overlaps it on the other:
        // (beside it in a row, above or below it in a column)
        let beside = |r: &(i64, i64, i64, i64), x: i64, y: i64| {
            let dx = (r.0 - (x + w)).max(x - r.2);
            let dy = (r.1 - (y + h)).max(y - r.3);
            (dx <= GAP && dy < 0, dy <= GAP && dx < 0)
        };
        let touches = |x: i64, y: i64| {
            around.iter().any(|r| {
                let (row, column) = beside(r, x, y);
                row || column
            })
        };
        let aligned = |x: i64, y: i64| {
            existing
                .iter()
                .map(|r| match beside(r, x, y) {
                    (true, _) => i64::from(r.1 == y) + i64::from(r.3 == y + h),
                    (_, true) => i64::from(r.0 == x) + i64::from(r.2 == x + w),
                    _ => 0,
                })
                .max()
                .unwrap_or(0)
        };
        let in_view = self.wholly_in_view(size);
        spot_candidates(around, existing, size, target)
            .into_iter()
            .filter(|&(x, y)| !overlaps_any(existing, (x, y, w, h)) && touches(x, y))
            .min_by_key(|&(x, y)| {
                let near = ((x - target.0) as f64).hypot((y - target.1) as f64);
                (!in_view(x, y), near.round() as i64 - ALIGN * aligned(x, y))
            })
    }

    /// Whether a window of `size` at a spot lies wholly in the usable view (a window an app
    /// opens by itself does not bring the camera to it).
    fn wholly_in_view(&self, size: (i64, i64)) -> impl Fn(i64, i64) -> bool {
        let view = self.usable_view();
        move |x, y| {
            view.is_some_and(|(x0, y0, x1, y1)| {
                x >= x0 && y >= y0 && x + size.0 <= x1 && y + size.1 <= y1
            })
        }
    }

    /// Owner, 2026-10-31: a window back from fullscreen never pushes its neighbours and never
    /// overlaps them. At the size it now draws: if its place is free (the 12 px gap kept) it
    /// stays; else (it came back larger, or a window moved or opened into its place meanwhile) it
    /// moves to the nearest free spot by the cluster placement's search, one wholly in view
    /// preferred. `None`: it stays. Cluster tiling only; the other policies let windows overlap.
    pub(super) fn returned_from_fullscreen(&self, id: ElementId) -> Option<CanvasPoint> {
        if self.placement != PlacementPolicy::ClusterTiling {
            return None;
        }
        let (anchor, tree) = (self.stage.anchor(id)?, self.stage.tree(id)?);
        let size = (
            i64::from(tree.geometry.size.w),
            i64::from(tree.geometry.size.h),
        );
        if size.0 <= 0 || size.1 <= 0 {
            return None;
        }
        let existing = self.window_rects_except(id);
        if !overlaps_any(&existing, (anchor.x, anchor.y, size.0, size.1)) {
            return None;
        }
        let (x, y) = self
            .free_spot_near(&existing, size, (anchor.x, anchor.y))
            .unwrap_or_else(|| beside_all(&existing, (anchor.x, anchor.y)));
        Some(CanvasPoint { x, y })
    }

    /// Owner, 2026-10-31: a window that changed its size by itself (the app, not the user)
    /// never pushes its neighbours and never overlaps them. `old` and `new` are its rects before
    /// and after (same anchor). If the new size reaches no window the old rect did not already
    /// reach, it stays; else it keeps the edge away from them (its right, bottom or both edges,
    /// or its centre) if that fits; else it moves to the nearest free spot. Windows the user
    /// stacked over it stay stacked. `None`: it stays.
    pub(super) fn self_resized(
        &self,
        id: ElementId,
        old: (i64, i64, i64, i64),
        new: (i64, i64, i64, i64),
    ) -> Option<CanvasPoint> {
        let (w, h) = (new.2 - new.0, new.3 - new.1);
        if w <= 0 || h <= 0 {
            return None;
        }
        let all = self.window_rects_except(id);
        let ow = (old.2 - old.0, old.3 - old.1);
        let others: Vec<_> = all
            .iter()
            .copied()
            .filter(|r| !overlaps_any(&[*r], (old.0, old.1, ow.0, ow.1)))
            .collect();
        let free = |x: i64, y: i64| !overlaps_any(&others, (x, y, w, h));
        if free(new.0, new.1) {
            return None;
        }
        let tries = [
            (old.2 - w, old.1),
            (old.0, old.3 - h),
            (old.2 - w, old.3 - h),
            ((old.0 + old.2) / 2 - w / 2, (old.1 + old.3) / 2 - h / 2),
        ];
        if let Some(&(x, y)) = tries.iter().find(|&&(x, y)| free(x, y)) {
            return Some(CanvasPoint { x, y });
        }
        let (x, y) = self
            .free_spot_near(&all, (w, h), (new.0, new.1))
            .unwrap_or_else(|| beside_all(&all, (new.0, new.1)));
        Some(CanvasPoint { x, y })
    }

    /// The placement policy for elements mapped from now on. It never moves an element that is
    /// already placed.
    pub fn set_placement_policy(&mut self, policy: PlacementPolicy) {
        self.placement = policy;
    }

    /// The titles or app_ids of windows kept on top from the moment they say so
    /// (`KEEP_ON_TOP`, compared ignoring case); a window already kept on top stays so.
    pub fn set_keep_on_top(&mut self, names: &[&str]) {
        self.keep_on_top = names
            .iter()
            .map(|n| n.trim().to_lowercase())
            .filter(|n| !n.is_empty())
            .collect();
    }

    /// Whether the window's title or app_id is one of [`Core::set_keep_on_top`]'s.
    pub(super) fn named_on_top(&self, id: ElementId) -> bool {
        let Some(meta) = self.stage.meta(id) else {
            return false;
        };
        [meta.title.as_deref(), meta.app_id.as_deref()]
            .into_iter()
            .flatten()
            .any(|n| self.keep_on_top.iter().any(|k| n.eq_ignore_ascii_case(k)))
    }

    /// A window kept on top opens in the usable view's bottom right corner, over whatever is
    /// there (a picture-in-picture's place).
    pub(super) fn on_top_anchor(&self, tree: &SurfaceTree) -> Option<CanvasPoint> {
        const MARGIN: f64 = 24.0;
        let output = self.outputs.first()?;
        let camera = self.cameras.camera_for_output(output.id)?;
        let (centre, zoom) = (self.cameras.center(camera)?, self.cameras.zoom(camera)?);
        let extent = output.logical_extent();
        let (ux, uy, uw, uh) = self
            .usable_area(output.id)
            .unwrap_or((0.0, 0.0, extent.0, extent.1));
        let q = Q16_ONE as f64;
        let g = tree.geometry;
        let x = centre.x as f64 / q + (ux + uw - MARGIN - extent.0 / 2.0) / zoom;
        let y = centre.y as f64 / q + (uy + uh - MARGIN - extent.1 / 2.0) / zoom;
        CanvasPoint::new(
            x.round() as i64 - i64::from(g.size.w),
            y.round() as i64 - i64::from(g.size.h),
        )
    }

    /// Keeps `id` on top, or no longer: it goes above every other window at once and stays
    /// there, keeping its place on the screen when the view moves.
    pub(crate) fn set_on_top(&mut self, id: ElementId, on: bool) {
        if self.stage.is_above(id) == on || self.stage.set_above(id, on).is_err() {
            return;
        }
        if on {
            self.note_on_top_views();
        }
        self.invalidate_all_frames();
    }

    /// Notes every camera's centre and zoom now: what the windows kept on top follow from.
    fn note_on_top_views(&mut self) {
        self.on_top_view.clear();
        for camera in 0..self.cameras.len() {
            if let (Some(c), Some(z)) = (self.cameras.center(camera), self.cameras.zoom(camera)) {
                self.on_top_view.push((camera, c, z));
            }
        }
    }

    /// After `camera` moved: each window kept on top that was in its view keeps its place on
    /// the screen (picture-in-picture stays where it was put while the canvas pans under it).
    pub(crate) fn on_top_follow(&mut self, camera: usize, out: &mut Vec<Effect>) {
        let (Some(now), Some(zoom)) = (self.cameras.center(camera), self.cameras.zoom(camera))
        else {
            return;
        };
        let Some(i) = self.on_top_view.iter().position(|(c, _, _)| *c == camera) else {
            self.note_on_top_views();
            return;
        };
        let (_, was, was_zoom) = self.on_top_view[i];
        self.on_top_view[i] = (camera, now, zoom);
        if (was.x, was.y, was_zoom.to_bits()) == (now.x, now.y, zoom.to_bits()) {
            return;
        }
        let Some(extent) = self
            .cameras
            .bound_output(camera)
            .and_then(|o| self.output(o))
            .map(|o| o.logical_extent())
        else {
            return;
        };
        let q = Q16_ONE as f64;
        let (wx, wy) = (was.x as f64 / q, was.y as f64 / q);
        let half = (extent.0 / 2.0 / was_zoom, extent.1 / 2.0 / was_zoom);
        for id in self.stage.kept_on_top() {
            if self.stage.mapped(id).is_none()
                || self.fullscreen_of(id).is_some()
                || self.input.interaction.element() == Some(id)
            {
                continue;
            }
            let (Some(a), Some(t)) = (self.stage.anchor(id), self.stage.tree(id)) else {
                continue;
            };
            let s = t.geometry.size;
            let mid = (
                a.x as f64 + f64::from(s.w) / 2.0,
                a.y as f64 + f64::from(s.h) / 2.0,
            );
            // only one that was on this screen
            if (mid.0 - wx).abs() > half.0 || (mid.1 - wy).abs() > half.1 {
                continue;
            }
            let k = was_zoom / zoom;
            let to = CanvasPoint::new(
                (now.x as f64 / q + (a.x as f64 - wx) * k).round() as i64,
                (now.y as f64 / q + (a.y as f64 - wy) * k).round() as i64,
            );
            if let Some(to) = to {
                let _ = self.place_by_shell(id, to, out);
            }
        }
    }

    /// A mapped window being destroyed: its app's last window centre, for its successor.
    pub(super) fn remember_closed(&mut self, id: ElementId) {
        let (Some(app), Some((anchor, tree, _))) = (
            self.stage
                .meta(id)
                .and_then(|m| m.app_id.as_deref())
                .map(str::to_ascii_lowercase),
            self.stage.mapped(id),
        ) else {
            return;
        };
        let g = tree.geometry;
        let centre = CanvasPoint {
            x: anchor.x + i64::from(g.loc.x) + i64::from(g.size.w) / 2,
            y: anchor.y + i64::from(g.loc.y) + i64::from(g.size.h) / 2,
        };
        self.closed_windows.retain(|(a, _)| *a != app);
        if self.closed_windows.len() >= 32 {
            self.closed_windows.remove(0);
        }
        self.closed_windows.push((app, centre));
    }

    /// Where a new window takes its app's closed window's place: centred there, when no other
    /// window of the app is mapped and that centre lies in the first output's current view.
    /// A notification's place: the first output's usable area, bottom right, 12 px in from its
    /// edges, at the camera's zoom; one already there pushes it up, as notifications stack.
    pub(super) fn notification_anchor(
        &self,
        id: ElementId,
        tree: &SurfaceTree,
    ) -> Option<CanvasPoint> {
        const MARGIN: f64 = 12.0;
        let output = self.outputs.first()?;
        let camera = self.cameras.camera_for_output(output.id)?;
        let (centre, zoom) = (self.cameras.center(camera)?, self.cameras.zoom(camera)?);
        let extent = output.logical_extent();
        let (ux, uy, uw, uh) = self
            .usable_area(output.id)
            .unwrap_or((0.0, 0.0, extent.0, extent.1));
        let q = Q16_ONE as f64;
        // the usable area's bottom right corner in Canvas px
        let corner = (
            centre.x as f64 / q + (ux + uw - MARGIN - extent.0 / 2.0) / zoom,
            centre.y as f64 / q + (uy + uh - MARGIN - extent.1 / 2.0) / zoom,
        );
        let g = tree.geometry;
        let (w, h) = (i64::from(g.size.w), i64::from(g.size.h));
        let gap = (MARGIN / zoom).round() as i64;
        let x = corner.0.round() as i64 - w;
        let mut y = corner.1.round() as i64 - h;
        let others: Vec<(i64, i64, i64, i64)> = self
            .stage
            .mapped_elements()
            .into_iter()
            .filter(|&e| e != id && self.is_notification(e))
            .filter_map(|e| {
                let (a, t) = (self.stage.anchor(e)?, self.stage.tree(e)?);
                let s = t.geometry.size;
                Some((a.x, a.y, a.x + i64::from(s.w), a.y + i64::from(s.h)))
            })
            .collect();
        // up past each one in the way, at most once per notification open
        for _ in 0..=others.len() {
            let Some(top) = others
                .iter()
                .filter(|o| x < o.2 && o.0 < x + w && y < o.3 + gap && o.1 < y + h + gap)
                .map(|o| o.1)
                .min()
            else {
                break;
            };
            y = top - gap - h;
        }
        // the anchor is the window geometry's origin (INV-STAGE-2)
        Some(CanvasPoint { x, y })
    }

    fn successor_anchor(&self, id: ElementId, tree: &SurfaceTree) -> Option<CanvasPoint> {
        let app = self.stage.meta(id)?.app_id.as_deref()?.to_ascii_lowercase();
        let same_app = |e: ElementId| {
            self.stage
                .meta(e)
                .and_then(|m| m.app_id.as_deref())
                .is_some_and(|a| a.eq_ignore_ascii_case(&app))
        };
        if self
            .stage
            .mapped_elements()
            .into_iter()
            .any(|e| e != id && same_app(e))
        {
            return None;
        }
        let centre = self
            .closed_windows
            .iter()
            .rev()
            .find(|(a, _)| *a == app)
            .map(|(_, c)| *c)?;
        let output = self.outputs.first()?;
        let camera = self.cameras.camera_for_output(output.id)?;
        let (view_c, zoom) = (self.cameras.center(camera)?, self.cameras.zoom(camera)?);
        let extent = output.logical_extent();
        let (cx, cy) = (
            view_c.x as f64 / Q16_ONE as f64,
            view_c.y as f64 / Q16_ONE as f64,
        );
        let in_view = (centre.x as f64 - cx).abs() <= extent.0 / 2.0 / zoom
            && (centre.y as f64 - cy).abs() <= extent.1 / 2.0 / zoom;
        if !in_view {
            return None;
        }
        let g = tree.geometry;
        // the old place, moved as little as it takes for the whole window to show: it may have been
        // near the edge, or the camera has moved since
        let (mut x, mut y) = (
            centre.x - i64::from(g.size.w) / 2,
            centre.y - i64::from(g.size.h) / 2,
        );
        if let Some(view) = self.usable_view() {
            x = keep_in_range(x, i64::from(g.size.w), view.0, view.2);
            y = keep_in_range(y, i64::from(g.size.h), view.1, view.3);
        }
        let at = CanvasPoint {
            x: x - i64::from(g.loc.x),
            y: y - i64::from(g.loc.y),
        };
        // a window opened on the old place since (a browser centred in the view) is not covered:
        // the policy places this one instead
        let taken = self.stage.mapped_elements().into_iter().any(|e| {
            let (Some(a), Some(t)) = (self.stage.anchor(e), self.stage.tree(e)) else {
                return false;
            };
            let (x0, y0) = (
                a.x + i64::from(t.geometry.loc.x),
                a.y + i64::from(t.geometry.loc.y),
            );
            e != id
                && (x0..x0 + i64::from(t.geometry.size.w)).contains(&centre.x)
                && (y0..y0 + i64::from(t.geometry.size.h)).contains(&centre.y)
        });
        if taken {
            return None;
        }
        // a bigger successor that would reach over a neighbour (Discord's window after its small
        // updater) goes to the nearest free spot from the old place instead: it never covers
        // another window (owner, 2026-11-07; until then only the old centre had to be free)
        let size = (i64::from(g.size.w), i64::from(g.size.h));
        let existing = self.window_rects_except(id);
        if !overlaps_any(&existing, (x, y, size.0, size.1)) {
            return Some(at);
        }
        let (fx, fy) = self
            .free_spot_near(&existing, size, (x, y))
            .unwrap_or_else(|| beside_all(&existing, (x, y)));
        Some(CanvasPoint {
            x: fx - i64::from(g.loc.x),
            y: fy - i64::from(g.loc.y),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PhysicalEvent;
    use crate::input::bindings::Modifiers;
    use crate::navigation::PersistedCameras;
    use crate::output::tests::key as okey;
    use crate::output::{OutputDescriptor, Transform};
    use crate::protocol::tests::*;
    use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, tests::key};

    /// Core's default is the centred policy (C12.F1). The legacy rule stays available as an
    /// explicit policy: the geometry origin at the camera centre, decided at the initial commit
    /// and not revised at the first map, for any size.
    #[test]
    fn the_legacy_policy_places_the_origin_at_the_camera_centre() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        assert_eq!(c.placement_policy(), PlacementPolicy::CentredInView);
        c.set_placement_policy(PlacementPolicy::OriginAtCameraCentre);
        c.output_connected(&mut t, plain("A")).unwrap();
        assert_eq!(placed(&mut c, 646, 513).1, CanvasPoint { x: 0, y: 0 });
        assert_eq!(placed(&mut c, 5000, 4000).1, CanvasPoint { x: 0, y: 0 });
        // a window placed under the legacy rule keeps its anchor when the policy changes
        let (id, a) = placed(&mut c, 400, 300);
        c.set_placement_policy(PlacementPolicy::CentredInView);
        c.toplevel_committed(id, sized(400, 300, true)).unwrap();
        assert_eq!(c.stage().anchor(id), Some(a));
        assert_eq!(placed(&mut c, 400, 300).1, CanvasPoint { x: -200, y: -150 });
    }

    /// The legacy rule through a persisted camera away from the origin, negative coordinates
    /// included: the anchor is the camera centre, rounded by R3.
    #[test]
    fn the_legacy_policy_follows_the_camera() {
        let entries = vec![(
            okey("A"),
            CanvasPos {
                x: -5000 * Q16_ONE + Q16_ONE / 2,
                y: 3001 * Q16_ONE + Q16_ONE / 2,
            },
            1.0,
        )];
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::from_entries(entries).unwrap());
        c.set_placement_policy(PlacementPolicy::OriginAtCameraCentre);
        c.output_connected(&mut t, plain("A")).unwrap();
        // −4999.5 → −5000 and 3001.5 → 3002: ties to even
        assert_eq!(
            placed(&mut c, 646, 513).1,
            CanvasPoint { x: -5000, y: 3002 }
        );
    }

    /// A window that fits is centred: x = usable_x + (usable_w − w)/2, y likewise. KCalc's
    /// 646x513 on a 1280x800 output with the camera at the Canvas origin: the view is
    /// [−640, 640) × [−400, 400), so the anchor is (−640 + 317, −400 + 143.5) = (−323, −256.5),
    /// and R3 (ties to even) gives −256. The whole window is inside the view.
    #[test]
    fn a_window_that_fits_is_centred_in_the_view() {
        let mut c = centred((1280, 800), 120, None);
        let (_, a) = placed(&mut c, 646, 513);
        assert_eq!(a, CanvasPoint { x: -323, y: -256 });
        assert!(a.x >= -640 && a.x + 646 <= 640 && a.y >= -400 && a.y + 513 <= 400);
        // even sizes centre exactly
        let (_, a) = placed(&mut c, 400, 300);
        assert_eq!(a, CanvasPoint { x: -200, y: -150 });
    }

    /// An oversized window is aligned to the region's origin on the axis it does not fit, and
    /// centred on the other: its top-left corner is what stays reachable.
    #[test]
    fn an_oversized_window_is_aligned_to_the_region_origin() {
        let mut c = centred((1280, 800), 120, None);
        assert_eq!(
            placed(&mut c, 2000, 300).1,
            CanvasPoint { x: -640, y: -150 }
        );
        assert_eq!(
            placed(&mut c, 300, 2000).1,
            CanvasPoint { x: -150, y: -400 }
        );
        assert_eq!(
            placed(&mut c, 5000, 4000).1,
            CanvasPoint { x: -640, y: -400 }
        );
        // exactly the size of the region: both rules agree
        assert_eq!(
            placed(&mut c, 1280, 800).1,
            CanvasPoint { x: -640, y: -400 }
        );
    }

    /// The usable region is the output's logical extent, so the output scale matters and the
    /// physical mode does not: 1920x1080 at 1.5 is 1280x720 logical; 3840x2160 at 2.0 is
    /// 1920x1080 logical.
    #[test]
    fn placement_uses_the_logical_extent_of_a_scaled_output() {
        let mut c = centred((1920, 1080), 180, None);
        assert_eq!(placed(&mut c, 646, 513).1, CanvasPoint { x: -323, y: -256 });
        // 800 logical px fits in 1280 but 750 does not fit in 720: aligned to the top
        assert_eq!(placed(&mut c, 800, 750).1, CanvasPoint { x: -400, y: -360 });
        let mut c = centred((3840, 2160), 240, None);
        assert_eq!(
            placed(&mut c, 1900, 1000).1,
            CanvasPoint { x: -950, y: -500 }
        );
        assert_eq!(
            placed(&mut c, 2000, 1000).1,
            CanvasPoint { x: -960, y: -500 }
        );
    }

    /// F-PLACE-1 in exact integers, independent of Core's Q16 and f64 path: per axis
    /// `c − min(s, u)/2` with `u = 120·view/scale_120`, rounded half to even.
    fn exact_axis(c: i64, view: u32, scale_120: u32, s: i32) -> i64 {
        let d = 2 * i128::from(scale_120);
        let half = (i128::from(s) * i128::from(scale_120)).min(120 * i128::from(view));
        let n = 2 * i128::from(c) * i128::from(scale_120) - half;
        let (q, r) = (n.div_euclid(d), n.rem_euclid(d));
        let up = 2 * r > d || (2 * r == d && q % 2 != 0);
        (q + i128::from(up)) as i64
    }

    /// The five examples of the amendment draft that no other test carries (fractional usable
    /// extent, half-pixel ties of both parities), then a sweep against the exact model:
    /// 6 scales × 3 cameras × 4 modes × 40 sizes.
    #[test]
    fn placement_agrees_with_the_exact_model_on_fractional_extents_and_ties() {
        let mut c = centred((1366, 768), 150, None); // 1092.8 × 614.4 logical
        assert_eq!(placed(&mut c, 801, 601).1, CanvasPoint { x: -400, y: -300 });
        assert_eq!(
            placed(&mut c, 2000, 601).1,
            CanvasPoint { x: -546, y: -300 }
        );
        let mut c = centred((1280, 800), 120, None);
        // −322.5 → −322 and −255.5 → −256: ties go to the even pixel
        assert_eq!(placed(&mut c, 645, 511).1, CanvasPoint { x: -322, y: -256 });
        assert_eq!(placed(&mut c, 1, 1).1, CanvasPoint { x: 0, y: 0 });
        let mut c = centred((1280, 800), 120, Some((1, 1)));
        // −321.5 → −322 and −254.5 → −254
        assert_eq!(placed(&mut c, 645, 511).1, CanvasPoint { x: -322, y: -254 });

        let mut checked = 0;
        for scale_120 in [90u32, 120, 150, 180, 210, 240] {
            for camera in [(0i64, 0i64), (7001, 333), (-123_457, -9)] {
                for mode in [(1280u32, 800u32), (1366, 768), (1365, 767), (3840, 2160)] {
                    let mut c = centred(mode, scale_120, Some(camera));
                    for k in 0..40 {
                        let (w, h) = (1 + 97 * k, 3 + 61 * k);
                        let want = CanvasPoint {
                            x: exact_axis(camera.0, mode.0, scale_120, w),
                            y: exact_axis(camera.1, mode.1, scale_120, h),
                        };
                        assert_eq!(
                            placed(&mut c, w, h).1,
                            want,
                            "{w}×{h} on {mode:?} at {scale_120}/120, camera {camera:?}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 2880);
    }

    /// Placement goes through the camera: with the camera elsewhere on the Canvas, positive or
    /// negative, the window is centred in what that camera shows.
    #[test]
    fn placement_follows_the_camera_including_negative_canvas_coordinates() {
        let mut c = centred((1280, 800), 120, Some((9000, 700)));
        assert_eq!(
            placed(&mut c, 646, 513).1,
            CanvasPoint {
                x: 9000 - 323,
                y: 700 - 256
            }
        );
        let mut c = centred((1280, 800), 120, Some((-5000, -3000)));
        // −3000 − 256.5 = −3256.5, ties to even: −3256
        assert_eq!(
            placed(&mut c, 646, 513).1,
            CanvasPoint { x: -5323, y: -3256 }
        );
        assert_eq!(
            placed(&mut c, 2000, 300).1,
            CanvasPoint { x: -5640, y: -3150 }
        );
    }

    /// Placement happens once. A later commit with another size, a tree change, a camera pan,
    /// an output reconfiguration, a second output, and an unmap followed by a remap all leave
    /// the anchor where it is (INV-STAGE-5; placement is not camera movement).
    #[test]
    fn a_placed_window_is_not_moved_again() {
        let entries = PersistedCameras::default();
        let (mut c, mut port, _, mut t) = Core::new(entries);
        c.set_placement_policy(PlacementPolicy::CentredInView);
        let d = |mode, scale_120| OutputDescriptor {
            key: okey("A"),
            mode,
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: 60_000,
        };
        let (o, _) = c.output_connected(&mut t, d((1280, 800), 120)).unwrap();
        let (id, a) = placed(&mut c, 646, 513);
        assert_eq!(a, CanvasPoint { x: -323, y: -256 });
        let anchor = |c: &Core| c.stage().anchor(id).unwrap();

        c.toplevel_committed(id, sized(900, 700, true)).unwrap();
        assert_eq!(anchor(&c), a, "a resize commit");
        c.toplevel_tree_changed(id, sized(300, 200, true)).unwrap();
        assert_eq!(anchor(&c), a, "a tree change");

        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: Modifiers {
                logo: true,
                ..Modifiers::default()
            },
            time_us: 0,
        };
        c.physical_input_vec(&mut port, button(true)).unwrap();
        c.physical_input_vec(
            &mut port,
            PhysicalEvent::PointerMotion {
                delta: (-400.0, 250.0),
                delta_unaccel: (-400.0, 250.0),
                time_us: 0,
            },
        )
        .unwrap();
        c.physical_input_vec(&mut port, button(false)).unwrap();
        assert_eq!(anchor(&c), a, "a camera pan");

        c.output_reconfigured(&mut t, o, d((2560, 1440), 180))
            .unwrap();
        assert_eq!(anchor(&c), a, "an output reconfiguration");
        let second = OutputDescriptor {
            key: okey("B"),
            ..d((1920, 1080), 120)
        };
        c.output_connected(&mut t, second).unwrap();
        assert_eq!(anchor(&c), a, "another output");

        c.toplevel_committed(id, sized(300, 200, false)).unwrap(); // unmap
        c.toplevel_committed(id, sized(300, 200, false)).unwrap(); // initial commit of the remap
        c.toplevel_committed(id, sized(300, 200, true)).unwrap(); // remap
        assert_eq!(anchor(&c), a, "a remap keeps its anchor");

        // a new window is placed by the camera as it is now, not as it was
        let (_, b) = placed(&mut c, 400, 300);
        assert_ne!(
            b,
            CanvasPoint { x: -200, y: -150 },
            "the pan moved the view"
        );
    }

    /// Popups and subsurfaces belong to their parent's tree: they do not change where the
    /// window is placed (only the window geometry counts) and are never placed by themselves.
    #[test]
    fn popups_and_subsurfaces_do_not_take_part_in_placement() {
        let mut c = centred((1280, 800), 120, None);
        let id = c.toplevel_created();
        c.toplevel_committed(id, sized(646, 513, false)).unwrap();
        let mut tree = sized(646, 513, true);
        tree.surfaces.push(SnapshotSurface {
            key: key(2),
            role: SurfaceRole::Subsurface {
                parent: 0,
                offset: LocalPoint { x: -200, y: 900 },
            },
            size: LocalSize { w: 3000, h: 50 },
            mapped: true,
            input: InputRegion::Infinite,
            frame_requested: false,
        });
        tree.surfaces.push(SnapshotSurface {
            key: key(3),
            role: SurfaceRole::Popup {
                position: LocalPoint { x: 10, y: 30 },
                geometry_loc: LocalPoint::default(),
            },
            size: LocalSize { w: 180, h: 240 },
            mapped: true,
            input: InputRegion::Infinite,
            frame_requested: false,
        });
        c.toplevel_committed(id, tree).unwrap();
        assert_eq!(c.stage().anchor(id), Some(CanvasPoint { x: -323, y: -256 }));
    }

    /// Without an output there is no view to centre in: the anchor of the initial commit is
    /// kept, and an output that connects later does not move the window.
    #[test]
    fn without_an_output_the_initial_anchor_is_kept() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        c.set_placement_policy(PlacementPolicy::CentredInView);
        let (id, a) = placed(&mut c, 646, 513);
        assert_eq!(a, CanvasPoint { x: 0, y: 0 });
        c.output_connected(&mut t, plain("A")).unwrap();
        assert_eq!(c.stage().anchor(id), Some(a));
    }

    /// INV-STAGE-2: an element's anchor is the Canvas position of its window-geometry origin.
    /// Cluster tiling uses the same convention as CentredInView
    /// and stage.rs even when geometry.loc != 0 (CSD window with client margins/shadows).
    /// A window that grows by itself pushes nothing (owner, 2026-10-31): it keeps the 12 px gap
    /// from the neighbour it would reach by moving itself; windows the user stacked stay stacked
    /// when one of them grows.
    #[test]
    fn a_window_that_grows_by_itself_moves_off_its_neighbour() {
        let win = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: key(k),
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
        };
        let mut c = centred((1280, 800), 120, None);
        c.set_placement_policy(PlacementPolicy::ClusterTiling);
        let open = |c: &mut Core, k: u64, w: i32, h: i32| {
            let id = c.toplevel_created();
            c.toplevel_committed(id, win(k, w, h, false)).unwrap();
            c.toplevel_committed(id, win(k, w, h, true)).unwrap();
            id
        };
        let a = open(&mut c, 1, 400, 300);
        let b = open(&mut c, 2, 400, 300);
        let (aa, ab) = (c.stage().anchor(a).unwrap(), c.stage().anchor(b).unwrap());
        // B is beside or below A; A grows by 200 px toward it, on both axes
        c.toplevel_committed(a, win(1, 600, 500, true)).unwrap();
        assert_eq!(c.stage().anchor(b).unwrap(), ab, "B is not pushed");
        let aa2 = c.stage().anchor(a).unwrap();
        let (bx0, by0) = (ab.x, ab.y);
        let apart = bx0 >= aa2.x + 600 + 12
            || bx0 + 400 + 12 <= aa2.x
            || by0 >= aa2.y + 500 + 12
            || by0 + 300 + 12 <= aa2.y;
        assert!(
            apart,
            "A {aa2:?} grown to 600x500 keeps the gap from B {ab:?} (was {aa:?})"
        );
        let aa = aa2;

        // the user drags B onto A: when A grows again, B is not pushed off it
        let mut fx = Vec::new();
        c.place_by_user(
            b,
            CanvasPoint {
                x: aa.x + 100,
                y: aa.y + 100,
            },
            &mut fx,
        )
        .unwrap();
        c.toplevel_committed(a, win(1, 700, 550, true)).unwrap();
        assert_eq!(
            c.stage().anchor(b).unwrap(),
            CanvasPoint {
                x: aa.x + 100,
                y: aa.y + 100
            }
        );
    }

    /// A free spot wholly in view wins over a nearer one that is not: beside the first window,
    /// not below it past the bottom of a 1280 x 800 screen (the VM's second terminal, 2026-10-31).
    #[test]
    fn cluster_placement_prefers_a_spot_in_view() {
        let win = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: key(k),
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
        };
        let mut c = centred((1280, 800), 120, None);
        c.set_placement_policy(PlacementPolicy::ClusterTiling);
        let open = |c: &mut Core, k: u64, w: i32, h: i32| {
            let id = c.toplevel_created();
            c.toplevel_committed(id, win(k, w, h, false)).unwrap();
            c.toplevel_committed(id, win(k, w, h, true)).unwrap();
            id
        };
        let a = open(&mut c, 1, 400, 300);
        let b = open(&mut c, 2, 400, 300);
        let (aa, ab) = (c.stage().anchor(a).unwrap(), c.stage().anchor(b).unwrap());
        // A is centred: the view is A's centre +- 640 x 400
        let (cx, cy) = (aa.x + 200, aa.y + 150);
        assert!(
            ab.x >= cx - 640
                && ab.x + 400 <= cx + 640
                && ab.y >= cy - 400
                && ab.y + 300 <= cy + 400,
            "B {ab:?} wholly in the view around A {aa:?}"
        );
    }

    #[test]
    fn cluster_placement_and_push_with_csd_window_respects_stage_anchor_convention() {
        let csd = |k: u64, w: i32, h: i32, lx: i32, ly: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint { x: lx, y: ly },
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: key(k),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize {
                        w: w + lx,
                        h: h + ly,
                    }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };

        // 1. CentredInView reference
        let mut ref_c = centred((1280, 800), 120, None);
        let ref_id = ref_c.toplevel_created();
        ref_c
            .toplevel_committed(ref_id, csd(1, 400, 300, 20, 30, false))
            .unwrap();
        ref_c
            .toplevel_committed(ref_id, csd(1, 400, 300, 20, 30, true))
            .unwrap();
        let expected_center_anchor = ref_c.stage().anchor(ref_id).unwrap();

        // 2. ClusterTiling initial window placement
        let mut c = centred((1280, 800), 120, None);
        c.set_placement_policy(PlacementPolicy::ClusterTiling);

        let w1 = c.toplevel_created();
        c.toplevel_committed(w1, csd(1, 400, 300, 20, 30, false))
            .unwrap();
        c.toplevel_committed(w1, csd(1, 400, 300, 20, 30, true))
            .unwrap();
        let a1 = c.stage().anchor(w1).unwrap();
        assert_eq!(
            a1, expected_center_anchor,
            "first cluster window anchor must match CentredInView anchor"
        );

        // 3. Second CSD window placed adjacent: beside it, flush with a 12 px gap (below would be
        // nearer the centre, since h < w, but runs past the bottom of the 800 px view)
        let w2 = c.toplevel_created();
        c.toplevel_committed(w2, csd(2, 400, 300, 20, 30, false))
            .unwrap();
        c.toplevel_committed(w2, csd(2, 400, 300, 20, 30, true))
            .unwrap();
        let a2 = c.stage().anchor(w2).unwrap();
        assert_eq!(
            a2,
            CanvasPoint {
                x: a1.x + 412,
                y: a1.y,
            },
            "second window placed flush beside with 12px gap based on window geometry"
        );
    }

    /// New windows under PLACEMENT_POLICY="cluster" (owner, 2026-11-22): on screen, beside the
    /// cluster nearest the pointer; the first in a view in its middle; never moving another.
    mod opening_beside_a_cluster {
        use super::*;
        use crate::PhysicalEvent;
        use crate::layer::{Anchors, KeyboardInteractivity, Layer, LayerState};
        use crate::navigation::{PersistedCameras, ZoomFeel};
        use crate::output::tests::key as okey;
        use crate::pointer::PointerState;

        /// A Core with one 1920 x 1080 screen at scale 1 whose camera is at `at` and `zoom`, and
        /// its input port (in a struct: no function returns an authority value).
        struct Desk {
            c: Core,
            i: crate::InputPort,
        }

        fn desk(zoom: f64, at: (i64, i64)) -> Desk {
            let centre = CanvasPos {
                x: at.0 * Q16_ONE,
                y: at.1 * Q16_ONE,
            };
            let cameras = PersistedCameras::from_entries(vec![(okey("A"), centre, zoom)]).unwrap();
            let (mut c, i, _, mut t) = Core::new(cameras);
            c.set_placement_policy(PlacementPolicy::ClusterTiling);
            c.output_connected(
                &mut t,
                OutputDescriptor {
                    key: okey("A"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
            Desk { c, i }
        }

        fn win(k: u64, w: i32, h: i32, mapped: bool) -> SurfaceTree {
            SurfaceTree {
                geometry: LocalRect {
                    loc: LocalPoint::default(),
                    size: LocalSize { w, h },
                },
                surfaces: vec![SnapshotSurface {
                    key: key(k),
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

        /// Opens a `w` x `h` window by itself (nothing focused or moved for it) and returns it
        /// with its window-geometry rect `(x0, y0, x1, y1)`.
        fn open(c: &mut Core, k: u64, w: i32, h: i32) -> (ElementId, (i64, i64, i64, i64)) {
            let id = c.toplevel_created();
            c.toplevel_committed(id, win(k, w, h, false)).unwrap();
            c.toplevel_committed(id, win(k, w, h, true)).unwrap();
            let a = c.stage().anchor(id).unwrap();
            (id, (a.x, a.y, a.x + i64::from(w), a.y + i64::from(h)))
        }

        /// A window opened and then moved to `at`, as the user leaves windows on the Canvas.
        fn put(c: &mut Core, k: u64, w: i32, h: i32, at: (i64, i64)) -> ElementId {
            let (id, _) = open(c, k, w, h);
            let mut fx = Vec::new();
            c.place_by_user(id, CanvasPoint { x: at.0, y: at.1 }, &mut fx)
                .unwrap();
            id
        }

        fn pointer_at(c: &mut Core, p: (f64, f64)) {
            let output = c.outputs.first().unwrap().id;
            c.pointer = PointerState::Located { output, p };
        }

        /// `r` on the screen in screen px, as the VM's everyday test reads it: the camera's
        /// centre and zoom, the screen's middle at (960, 540).
        fn on_screen(c: &Core, r: (i64, i64, i64, i64)) -> (f64, f64, f64, f64) {
            let centre = c.cameras().center(0).unwrap();
            let zoom = c.cameras().zoom(0).unwrap();
            let s = |v: i64, mid: f64, q: i64| (v as f64 - q as f64 / Q16_ONE as f64) * zoom + mid;
            (
                s(r.0, 960.0, centre.x),
                s(r.1, 540.0, centre.y),
                s(r.2, 960.0, centre.x),
                s(r.3, 540.0, centre.y),
            )
        }

        fn wholly_on(s: (f64, f64, f64, f64), x1: f64) -> bool {
            s.0 >= 0.0 && s.1 >= 0.0 && s.2 <= x1 && s.3 <= 1080.0
        }

        /// `a` lies within the 12 px gap of `b` on one axis and overlaps it on the other.
        fn touching(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
            let dx = (b.0 - a.2).max(a.0 - b.2);
            let dy = (b.1 - a.3).max(a.1 - b.3);
            (dx == GAP && dy < 0) || (dy == GAP && dx < 0)
        }

        fn apart(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
            !overlaps_any(&[b], (a.0, a.1, a.2 - a.0, a.3 - a.1))
        }

        /// A window that opens while the camera glides (`ZOOM_FEEL` "smooth", the shipped
        /// default) is kept in the view the camera stops at. It was kept in the view of that
        /// moment and ended at screen (-3291, -2094), wholly off it.
        #[test]
        fn a_window_opening_during_a_glide_is_on_screen_when_the_camera_stops() {
            let Desk { mut c, mut i } = desk(1.0, (0, 0));
            c.set_zoom_feel(ZoomFeel::Smooth);
            let a = put(&mut c, 1, 640, 480, (5000, 3000));
            c.physical_input_vec(&mut i, PhysicalEvent::RecenterElement { element: a })
                .unwrap();
            let output = c.outputs.first().unwrap().id;
            c.zoom_tick(output, std::time::Duration::from_millis(16));
            assert_ne!(
                c.cameras().center(0),
                c.cameras().effective_value(0).map(|v| v.center),
                "the camera is still on its way"
            );
            let (_, b) = open(&mut c, 2, 400, 300);
            for k in 2..200 {
                c.zoom_tick(output, std::time::Duration::from_millis(16 * k));
            }
            let s = on_screen(&c, b);
            assert!(
                wholly_on(s, 1920.0),
                "B at screen {s:?} once the camera stopped"
            );
        }

        /// The first window in a view opens in the middle of the usable view, wherever the
        /// pointer rests (an edge, a corner) and wherever the camera is (zoomed out to 0.85 as
        /// the overview leaves it, in to 1.5, far from the origin). The edge's 300 px strip on
        /// the right is not usable. It used to open centred on the pointer, kept in the view.
        #[test]
        fn the_first_window_in_a_view_opens_in_its_middle() {
            for zoom in [0.85, 1.0, 1.5] {
                for at in [(0, 0), (23456, -17890), (-40000, 9000)] {
                    for p in [(1919.0, 540.0), (0.0, 0.0), (1919.0, 1079.0), (960.0, 0.0)] {
                        let Desk { mut c, .. } = desk(zoom, at);
                        let output = c.outputs.first().unwrap().id;
                        let edge = c.layer_created(Some(output)).unwrap();
                        let state = LayerState {
                            layer: Some(Layer::Top),
                            anchors: Anchors {
                                top: true,
                                bottom: true,
                                left: false,
                                right: true,
                            },
                            size: (300, 0),
                            margins: (0, 0, 0, 0),
                            exclusive_zone: 300,
                            keyboard: KeyboardInteractivity::None,
                        };
                        c.layer_committed(edge, state, win(90, 0, 0, false))
                            .unwrap();
                        c.layer_committed(edge, state, win(90, 300, 1080, true))
                            .unwrap();
                        assert_eq!(c.usable_area(output), Some((0.0, 0.0, 1620.0, 1080.0)));
                        // a window far away on the Canvas is in no view of this screen
                        put(&mut c, 1, 400, 300, (at.0 + 90_000, at.1));
                        pointer_at(&mut c, p);
                        let (_, r) = open(&mut c, 2, 640, 480);
                        let s = on_screen(&c, r);
                        let mid = ((s.0 + s.2) / 2.0, (s.1 + s.3) / 2.0);
                        assert!(
                            (mid.0 - 810.0).abs() <= 1.0 && (mid.1 - 540.0).abs() <= 1.0,
                            "zoom {zoom} camera {at:?} pointer {p:?}: middle at {mid:?}"
                        );
                    }
                }
            }
        }

        /// The cluster nearest the pointer gets the window, on its side nearest the pointer.
        /// Before, a window opened centred on the pointer when that spot was free, touching no
        /// window (between the two clusters here).
        #[test]
        fn a_new_window_opens_beside_the_cluster_nearest_the_pointer() {
            let Desk { mut c, .. } = desk(1.0, (0, 0));
            put(&mut c, 1, 400, 300, (-900, -400));
            put(&mut c, 2, 400, 300, (-900, -88));
            let right = put(&mut c, 3, 400, 300, (500, -150));
            let r = (500, -150, 900, 150);
            // right of the middle, nearer the right window, left of it
            pointer_at(&mut c, (1100.0, 560.0));
            let (_, b) = open(&mut c, 4, 300, 200);
            assert!(touching(b, r), "B {b:?} touches the right window {r:?}");
            assert!(
                b.2 <= r.0,
                "B {b:?} on the right window's side facing the pointer"
            );
            assert!(wholly_on(on_screen(&c, b), 1920.0));
            assert_eq!(
                c.stage().anchor(right),
                Some(CanvasPoint { x: 500, y: -150 })
            );
        }

        /// The focused window's cluster gets the window when it shows on the screen, wherever
        /// the pointer rests (on a panel, by a game).
        #[test]
        fn the_focused_windows_cluster_comes_first() {
            let Desk { mut c, mut i } = desk(1.0, (0, 0));
            let left = put(&mut c, 1, 400, 300, (-900, -150));
            put(&mut c, 2, 400, 300, (500, -150));
            c.set_focus(&mut i, Some(left)).unwrap();
            pointer_at(&mut c, (1900.0, 540.0));
            let (_, b) = open(&mut c, 3, 300, 200);
            assert!(
                touching(b, (-900, -150, -500, 150)),
                "B {b:?} beside the focused window"
            );
        }

        /// Among spots about as near, one in line with its neighbour wins: the pointer 20 px
        /// below the window's middle row puts the new window level with its top edge.
        #[test]
        fn a_spot_in_line_with_a_neighbour_is_preferred() {
            let Desk { mut c, .. } = desk(1.0, (0, 0));
            put(&mut c, 1, 400, 300, (-600, -150));
            // the new window centred on the pointer would start 20 px below A's top
            pointer_at(&mut c, (960.0 + 300.0, 540.0 + 20.0));
            let (_, b) = open(&mut c, 2, 400, 300);
            assert_eq!(
                (b.0, b.1),
                (-600 + 412, -150),
                "beside A, top edges in line"
            );
        }

        /// With no free spot wholly on screen the window goes beyond the view, still touching
        /// the cluster, on its side nearest the pointer; it covers nothing. The VM's five
        /// 640 x 480 windows on a 1920 x 1080 screen: from the second on none fits beside the
        /// first in the middle (640 + 12 + 640 px > 960 px to an edge; at most four such windows
        /// fit on that screen whatever the order).
        #[test]
        fn with_no_room_on_screen_a_window_goes_beyond_the_view_beside_the_cluster() {
            let Desk { mut c, .. } = desk(1.0, (0, 0));
            let mut rects: Vec<(i64, i64, i64, i64)> = Vec::new();
            for k in 0..5 {
                pointer_at(&mut c, (1500.0, 540.0));
                let (_, r) = open(&mut c, k + 1, 640, 480);
                if k == 0 {
                    assert!(
                        wholly_on(on_screen(&c, r), 1920.0),
                        "the first {r:?} on screen"
                    );
                    assert_eq!(r, (-320, -240, 320, 240), "in the middle");
                } else {
                    assert!(
                        rects.iter().any(|&o| touching(r, o)),
                        "window {k} {r:?} touches the cluster {rects:?}"
                    );
                }
                assert!(
                    rects.iter().all(|&o| apart(r, o)),
                    "window {k} {r:?} covers none"
                );
                rects.push(r);
            }
            assert!(
                rects[1].0 > rects[0].2,
                "the second on the side nearest the pointer"
            );
        }

        /// A new window takes only free Canvas: no window moves, whatever the windows around
        /// it, the pointer or the view (owner, 2026-11-22: "I was already avoiding moving
        /// existing windows").
        #[test]
        fn opening_a_window_moves_no_other_window() {
            let Desk { mut c, mut i } = desk(0.85, (300, -200));
            let spots = [
                (-900, -400),
                (-470, -400),
                (-900, -60),
                (600, 300),
                (40_000, 0),
                (-1200, 500),
            ];
            let mut ids = Vec::new();
            for (k, &at) in spots.iter().enumerate() {
                ids.push(put(&mut c, k as u64 + 1, 420, 330, at));
            }
            c.set_focus(&mut i, Some(ids[3])).unwrap();
            let pointers = [(0.0, 0.0), (1919.0, 1079.0), (960.0, 540.0), (100.0, 900.0)];
            for (n, size) in [
                (500, 400),
                (1200, 900),
                (2400, 1400),
                (300, 200),
                (800, 700),
            ]
            .into_iter()
            .enumerate()
            {
                pointer_at(&mut c, pointers[n % pointers.len()]);
                let before: Vec<_> = ids.iter().map(|&e| c.stage().anchor(e)).collect();
                let (id, r) = open(&mut c, 100 + n as u64, size.0, size.1);
                let after: Vec<_> = ids.iter().map(|&e| c.stage().anchor(e)).collect();
                assert_eq!(before, after, "opening a {size:?} window moved none");
                for &e in &ids {
                    let (a, t) = (c.stage().anchor(e).unwrap(), c.stage().tree(e).unwrap());
                    let o = (
                        a.x,
                        a.y,
                        a.x + i64::from(t.geometry.size.w),
                        a.y + i64::from(t.geometry.size.h),
                    );
                    assert!(apart(r, o), "the {size:?} window {r:?} covers {o:?}");
                }
                ids.push(id);
            }
        }
    }
}
