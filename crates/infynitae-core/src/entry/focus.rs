//! Moving between windows: recenter, focus slots and directions, jumps, clusters, the overview.

use super::*;

impl Core {
    /// Super+Space: the pointer's output's camera is centred on the focused window — the centre of
    /// its window geometry at the centre of the usable area (L4) — with its zoom unchanged and no
    /// history entry (as a shell `Navigate`). Refused, nothing changed, when no output holds the
    /// pointer, that output presents a protocol-fullscreen element (C12.F17 Q1 B), nothing is
    /// focused, or the focused window is not mapped or has a degenerate geometry. Then the
    /// camera-write consequences, as after a fit.
    pub(crate) fn recenter_on_element(
        &mut self,
        proof: authority::Authorized,
        element: crate::stage::ElementId,
        keep_zoom: bool,
        record_history: bool,
        out: &mut Vec<Effect>,
    ) {
        let output = match self.pointer {
            PointerState::Located { output, .. } => Some(output),
            PointerState::NoOutput => self.outputs.first().map(|o| o.id),
        };
        if let Some(output) = output {
            self.recenter_on_element_at(proof, output, element, keep_zoom, record_history, out);
        }
    }

    /// [`Core::recenter_on_element`] on `output`'s camera.
    fn recenter_on_element_at(
        &mut self,
        proof: authority::Authorized,
        output: OutputId,
        element: crate::stage::ElementId,
        keep_zoom: bool,
        record_history: bool,
        out: &mut Vec<Effect>,
    ) {
        if self.presentation(output).is_some() && !self.is_presentation_suppressed(output) {
            return;
        }
        if self.presentation(output) == Some(element) && self.is_presentation_suppressed(output) {
            self.unsuppress_presentation(output, out);
            return;
        }
        if self.stage.mapped(element).is_none() {
            return;
        }
        let (Some(anchor), Some(tree)) = (self.stage.anchor(element), self.stage.tree(element))
        else {
            return;
        };
        let g = (
            anchor.x,
            anchor.y,
            tree.geometry.size.w as i64,
            tree.geometry.size.h as i64,
        );
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return;
        };
        let (Some(extent), Some(usable)) = (
            self.output(output).map(|m| m.logical_extent()),
            self.usable_area(output),
        ) else {
            return;
        };
        // Super+Space resets the zoom to 1.0 (a window filling the screen is centred on all of
        // it), or less for a window bigger than the screen (all of it shown); Super+Shift+Space
        // keeps it
        let (target_zoom, area) = if keep_zoom {
            (
                self.cameras.effective_value(camera).map_or(1.0, |v| v.zoom),
                usable,
            )
        } else {
            crate::navigation::recenter_zoom(extent, usable, g.2, g.3)
        };
        let Some(center) = crate::navigation::recenter_target(extent, area, target_zoom, g) else {
            return;
        };
        let target = crate::navigation::CameraValue {
            center,
            zoom: target_zoom,
        };
        self.cameras
            .glide_to(proof, camera, target, record_history, self.zoom_feel);
        self.camera_written(output, camera, out);
    }

    /// A two-finger double tap at `at` (output logical px of `output`): Super+Space for the
    /// window under the fingers (raised, focused, centred at zoom 1.0); on empty canvas, zoom 1.0.
    pub(super) fn touch_recenter(
        &mut self,
        proof: authority::Authorized,
        output: OutputId,
        at: (f64, f64),
        out: &mut Vec<Effect>,
    ) {
        if self.input.interaction.active() {
            return;
        }
        let pt = crate::geom::OutputLogicalPoint { x: at.0, y: at.1 };
        if let Some(hit) = self.hit_test(output, pt) {
            self.raise_by_user(hit.element);
            out.extend(
                self.focus
                    .set_focus_at(self.input.clock.now_us(), Some(hit.element)),
            );
            self.recenter_on_element_at(proof, output, hit.element, false, false, out);
        } else if let Some(camera) = self.cameras.camera_for_output(output) {
            self.zoom_reset(proof, output, camera, out);
        }
    }

    pub(super) fn recenter(
        &mut self,
        proof: authority::Authorized,
        keep_zoom: bool,
        record_history: bool,
        out: &mut Vec<Effect>,
    ) {
        if let Some(element) = self.focus.target() {
            self.recenter_on_element(proof, element, keep_zoom, record_history, out);
        } else if !keep_zoom {
            let output = match self.pointer {
                PointerState::Located { output, .. } => Some(output),
                PointerState::NoOutput => self.outputs.first().map(|o| o.id),
            };
            if let Some(output) = output
                && let Some(camera) = self.cameras.camera_for_output(output)
            {
                self.zoom_reset(proof, output, camera, out);
            }
        }
    }

    /// Super+1..9: focus, raise, and recenter camera on mapped window at slot `slot` (0-indexed creation order).
    pub(super) fn focus_slot(
        &mut self,
        slot: usize,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        let live = self.focusable_elements_in_creation_order();
        let Some(&element) = live.get(slot) else {
            return;
        };
        self.jump_to_window(element, proof, out);
    }

    /// Super+arrow: focus, raise, and recenter camera on the nearest window that way, from the
    /// focused window, or from the middle of the view with nothing focused.
    pub(super) fn focus_direction(
        &mut self,
        direction: crate::input::keys::Direction,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        let focused = self.focus.target();
        let centre = |c: &Self, e: crate::stage::ElementId| {
            let anchor = c.stage.anchor(e)?;
            let size = c.stage.tree(e)?.geometry.size;
            Some((
                anchor.x + i64::from(size.w) / 2,
                anchor.y + i64::from(size.h) / 2,
            ))
        };
        let from = focused.and_then(|e| centre(self, e)).or_else(|| {
            let output = match self.pointer {
                PointerState::Located { output, .. } => output,
                PointerState::NoOutput => self.outputs.first()?.id,
            };
            let at = self
                .cameras
                .center(self.cameras.camera_for_output(output)?)?;
            Some((at.x >> 16, at.y >> 16))
        });
        let Some(from) = from else {
            return;
        };
        let windows: Vec<_> = self
            .focusable_elements_in_creation_order()
            .into_iter()
            .filter(|&e| Some(e) != focused)
            .filter_map(|e| Some((e, centre(self, e)?)))
            .collect();
        if let Some(element) = crate::input::focus::nearest_in_direction(from, direction, &windows)
        {
            self.jump_to_window(element, proof, out);
        }
    }

    /// Focus, raise, and recenter the camera on `element`, out of a fullscreen window if need be.
    pub(super) fn jump_to_window(
        &mut self,
        element: crate::stage::ElementId,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        self.show_chosen(element, out);
        self.raise_and_focus(element, out);
        if let Some(p) = proof {
            self.recenter(p, false, true, out);
        }
        // what is under the pointer now is not a crossing: the camera moved, not the hand
        self.hover_seen = self.hovered_window();
        self.keyed_focus = Some(element);
    }

    /// Raises `element` and gives it keyboard focus, as a user's pick of that window.
    pub(super) fn raise_and_focus(
        &mut self,
        element: crate::stage::ElementId,
        out: &mut Vec<Effect>,
    ) {
        let raised = self.raise_by_user(element);
        out.extend(
            self.focus
                .set_focus_at(self.input.clock.now_us(), Some(element)),
        );
        out.extend(self.reconcile_constraints());
        if raised
            && matches!(self.pointer, pointer::PointerState::Located { .. })
            && !self.input.interaction.active()
            && !self.locked()
        {
            self.true_position_into(out);
        }
    }

    /// Super+Shift+1..9: focus, raise primary window, and frame/recenter camera on cluster at slot `slot` (0-indexed creation order).
    pub(super) fn focus_cluster_slot(
        &mut self,
        slot: usize,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        let clusters = self.clusters();
        let Some(cluster) = clusters.get(slot) else {
            return;
        };
        let Some(&primary) = cluster.first() else {
            return;
        };
        self.show_chosen(primary, out);
        self.keyed_focus = Some(primary);
        let raised = self.raise_by_user(primary);
        out.extend(
            self.focus
                .set_focus_at(self.input.clock.now_us(), Some(primary)),
        );
        out.extend(self.reconcile_constraints());
        if raised
            && matches!(self.pointer, pointer::PointerState::Located { .. })
            && !self.input.interaction.active()
            && !self.locked()
        {
            self.true_position_into(out);
        }
        if let Some(p) = proof {
            self.frame_cluster(p, cluster, out);
        }
    }

    /// Fits or centers the camera on the cluster's bounding box.
    fn frame_cluster(
        &mut self,
        proof: authority::Authorized,
        cluster: &[crate::stage::ElementId],
        out: &mut Vec<Effect>,
    ) {
        if cluster.is_empty() {
            return;
        }
        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        for &e in cluster {
            let a = self
                .stage
                .anchor(e)
                .unwrap_or(crate::geom::CanvasPoint { x: 0, y: 0 });
            let (w, h) = self
                .stage
                .tree(e)
                .map(|t| (t.geometry.size.w as i64, t.geometry.size.h as i64))
                .unwrap_or((0, 0));
            min_x = min_x.min(a.x);
            min_y = min_y.min(a.y);
            max_x = max_x.max(a.x.saturating_add(w));
            max_y = max_y.max(a.y.saturating_add(h));
        }
        if min_x > max_x || min_y > max_y {
            return;
        }
        let g = (min_x, min_y, (max_x - min_x).max(1), (max_y - min_y).max(1));
        self.frame_bounds(proof, g, out);
    }

    /// Fits or centres the pointer's screen's camera on the Canvas rect `g` `(x, y, w, h)`, at
    /// zoom 1 at most (a cluster, or the cells a layout gave it).
    pub(crate) fn frame_bounds(
        &mut self,
        proof: authority::Authorized,
        g: (i64, i64, i64, i64),
        out: &mut Vec<Effect>,
    ) {
        let Some(output) = self
            .pointer_output()
            .or_else(|| self.outputs().first().map(|o| o.id))
        else {
            return;
        };
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return;
        };
        let (Some((view_size, scale)), Some(extent), Some(usable)) = (
            self.output(output).map(|m| (m.view_size(), m.scale)),
            self.output(output).map(|m| m.logical_extent()),
            self.usable_area(output),
        ) else {
            return;
        };
        if let Some(mut target) =
            crate::navigation::fit_target_in(view_size, extent, usable, scale, g)
        {
            // Cap zoom at 1.0 (clusters are not magnified beyond scale 1.0), centred in the
            // usable area at the capped zoom (the fit's centre is for its own zoom)
            target.zoom = target.zoom.min(1.0);
            if let Some(c) = crate::navigation::recenter_target(extent, usable, target.zoom, g) {
                target.center = c;
            }
            self.cameras
                .glide_fit(proof, camera, target, self.zoom_feel);
            self.camera_written(output, camera, out);
        }
    }

    /// Super+Shift+Tab: scales the camera out to capture all mapped windows on the canvas.
    /// If the camera is already framing the overview, restores the previous camera zoom and position.
    pub(crate) fn overview_toggle(&mut self, proof: authority::Authorized, out: &mut Vec<Effect>) {
        let Some(output) = self
            .pointer_output()
            .or_else(|| self.outputs.first().map(|o| o.id))
        else {
            return;
        };
        let Some(camera) = self.cameras.camera_for_output(output) else {
            return;
        };
        let (Some((view_size, scale)), Some(extent), Some(usable)) = (
            self.output(output).map(|m| (m.view_size(), m.scale)),
            self.output(output).map(|m| m.logical_extent()),
            self.usable_area(output),
        ) else {
            return;
        };

        // a minimized window is not shown: the overview does not frame where it was (B13)
        let mapped: Vec<_> = self
            .stage
            .mapped_elements()
            .into_iter()
            .filter(|&e| !self.stage.is_minimized(e))
            .collect();
        if mapped.is_empty() {
            out.push(Effect::ToggleOverview);
            return;
        }

        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        for &e in &mapped {
            let a = self
                .stage
                .anchor(e)
                .unwrap_or(crate::geom::CanvasPoint { x: 0, y: 0 });
            let (w, h) = self
                .stage
                .tree(e)
                .map(|t| (t.geometry.size.w as i64, t.geometry.size.h as i64))
                .unwrap_or((0, 0));
            min_x = min_x.min(a.x);
            min_y = min_y.min(a.y);
            max_x = max_x.max(a.x.saturating_add(w));
            max_y = max_y.max(a.y.saturating_add(h));
        }

        if min_x > max_x || min_y > max_y {
            return;
        }

        // Add canvas margin (at least 60px)
        let span_w = (max_x - min_x).max(1);
        let span_h = (max_y - min_y).max(1);
        let pad_x = (span_w / 12).max(60);
        let pad_y = (span_h / 12).max(60);

        let g = (
            min_x - pad_x,
            min_y - pad_y,
            span_w + 2 * pad_x,
            span_h + 2 * pad_y,
        );

        let Some(mut target) =
            crate::navigation::fit_target_in(view_size, extent, usable, scale, g)
        else {
            return;
        };
        target.zoom = target.zoom.min(1.0);
        // centred in the usable area at the capped zoom (the fit's centre is for its own zoom)
        if let Some(c) = crate::navigation::recenter_target(extent, usable, target.zoom, g) {
            target.center = c;
        }

        // where the overview left the camera, not where it would go now: windows move under it.
        // A fullscreen window on top means the overview is not showing, wherever the camera is.
        let view = self
            .cameras
            .effective_value(camera)
            .map_or((None, None), |v| (Some(v.center), Some(v.zoom.to_bits())));
        let on_top =
            self.presentation(output).is_some() && !self.is_presentation_suppressed(output);
        let at_overview = !on_top
            && self.overview_at.contains(&(output, view))
            && self.cameras.history(camera).is_some_and(|h| !h.is_empty());
        self.overview_at.retain(|o| o.0 != output);

        if at_overview {
            // back from the overview: the camera first (a presenting output refuses a restore,
            // B5), then into the fullscreen window if it still has the focus
            self.viewport_restore(proof, out);
            if self.presentation(output).is_some()
                && self.presentation(output) == self.focus.target()
            {
                self.unsuppress_presentation(output, out);
            }
        } else {
            // the overview shows the canvas: a fullscreen window steps aside for it
            if self.presentation(output).is_some() {
                self.suppress_presentation(output, out);
            }
            self.cameras
                .glide_fit(proof, camera, target, self.zoom_feel);
            let stored = self
                .cameras
                .effective_value(camera)
                .map_or((None, None), |v| (Some(v.center), Some(v.zoom.to_bits())));
            self.overview_at.push((output, stored));
            self.camera_written(output, camera, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Core;
    use crate::entry::tests::*;
    use crate::geom::CanvasPos;
    use crate::input::bindings::Modifiers;
    use crate::navigation::PersistedCameras;
    use crate::output::tests::key;
    use crate::pointer::PointerState;
    use crate::protocol::Effect;

    #[test]
    fn focus_cycle_navigate_recenters_camera() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_cycle_navigate(true);

        let w1 = c.toplevel_created();
        let w2 = c.toplevel_created();
        let win = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(k).unwrap()),
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
        c.toplevel_committed(w1, win(1, 400, 300, false)).unwrap();
        c.toplevel_committed(w1, win(1, 400, 300, true)).unwrap();
        c.toplevel_committed(w2, win(2, 400, 300, false)).unwrap();
        c.toplevel_committed(w2, win(2, 400, 300, true)).unwrap();
        c.render_list(a).unwrap();

        c.set_focus(&mut i, Some(w1)).unwrap();
        assert_eq!(c.focus().target(), Some(w1));

        let logo = Modifiers {
            logo: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let _ = c
            .physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key: crate::input::keys::KEY_TAB,
                    pressed: true,
                    modifiers: logo,
                    mods_changed: false,
                    time_us: 1000,
                },
            )
            .unwrap();

        assert_eq!(c.focus().target(), Some(w2), "focus should cycle to w2");
    }

    #[test]
    fn recenter_element_centers_camera_on_target_window() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        use std::num::NonZeroU64;

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();

        let w1 = c.toplevel_created();
        let win = |mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w: 800, h: 600 },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(NonZeroU64::new(1).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w: 800, h: 600 }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        c.toplevel_committed(w1, win(false)).unwrap();
        c.toplevel_committed(w1, win(true)).unwrap();

        // Place w1 at an offset anchor
        let anchor = crate::geom::CanvasPoint { x: 5000, y: 3000 };
        let mut fx = Vec::new();
        c.place_by_shell(w1, anchor, &mut fx).unwrap();

        // Recenter element w1
        let mut buf = crate::protocol::EffectBuf::new();
        c.recenter_element(&mut i, w1, &mut buf).unwrap();

        // Camera must now be centered on w1's window center
        let camera = c.cameras.camera_for_output(a).unwrap();
        let center = c.cameras.center(camera).unwrap();
        let cx = center.x / crate::geom::Q16_ONE;
        let cy = center.y / crate::geom::Q16_ONE;
        assert_eq!(cx, 5400);
        assert_eq!(cy, 3300);
    }

    /// With focus following the mouse, Super+Tab's window keeps the focus when the mouse is nudged;
    /// only going to another window moves it (owner, 2026-11-01).
    #[test]
    fn super_tab_keeps_its_window_when_the_mouse_is_nudged() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        c.set_focus_follows_mouse_strict(true);
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        // ws[1] far off, ws[0] under the pointer at the middle of the screen
        c.place_by_user(
            ws[1],
            crate::geom::CanvasPoint { x: 5000, y: 5000 },
            &mut fx,
        )
        .unwrap();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: -100, y: -100 },
            &mut fx,
        )
        .unwrap();
        c.render_list(a).unwrap();
        let motion = |c: &mut Core, i: &mut crate::InputPort, delta: (f64, f64), time_us: u64| {
            c.physical_input_vec(
                i,
                crate::PhysicalEvent::PointerMotion {
                    delta,
                    delta_unaccel: delta,
                    time_us,
                },
            )
            .unwrap();
        };
        c.set_focus(&mut i, Some(ws[1])).unwrap();
        motion(&mut c, &mut i, (1.0, 1.0), 1000);
        assert_eq!(c.focus().target(), Some(ws[0]), "the pointer went onto it");
        c.focus.settle_hover(1_000_000);

        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(ws[1]));
        motion(&mut c, &mut i, (1.0, 1.0), 2_000_000);
        assert_eq!(c.focus().target(), Some(ws[1]), "a nudge does not take it");
        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(ws[0]), "Tab, Tab returns");
    }

    /// Super+Tab (owner, 2026-11-01) through the key path: every press flips between the last two
    /// windows used, whether Super is let go in between or held.
    #[test]
    fn super_tab_flips_the_last_two_and_never_goes_further_back() {
        use crate::input::keys::{KEY_LEFTMETA, KEY_TAB};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 3);
        let [w1, w2, w3] = [ws[0], ws[1], ws[2]];
        for w in [w1, w2, w3] {
            c.set_focus(&mut i, Some(w)).unwrap();
        }
        // used: w3, w2, w1
        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(w2));
        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(w3), "Tab, Tab returns");

        // Super held: still only those two
        press(&mut c, &mut i, KEY_LEFTMETA, true, true);
        for expected in [w2, w3, w2] {
            press(&mut c, &mut i, KEY_TAB, true, true);
            press(&mut c, &mut i, KEY_TAB, false, true);
            assert_eq!(c.focus().target(), Some(expected));
        }
        press(&mut c, &mut i, KEY_LEFTMETA, false, false);
        assert_eq!(c.focus().history(), &[w2, w3, w1]);

        // a window closed is dropped from the history and skipped
        let _ = c.toplevel_destroyed(w3);
        assert!(!c.focus().history().contains(&w3));
        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(w1));
        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(w2));
    }

    /// Super+Shift+Tab over a fullscreen window (owner, 2026-11-01): the overview shows the canvas;
    /// again, and the window is back.
    #[test]
    fn the_overview_opens_over_a_fullscreen_window_and_returns_to_it() {
        use crate::input::keys::{KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_TAB};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        c.toplevel_request(
            ws[0],
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
        )
        .unwrap();
        assert_eq!(c.presentation(a), Some(ws[0]));
        let overview = |c: &mut Core, i: &mut crate::InputPort| {
            let mods = |logo, shift| crate::input::bindings::Modifiers {
                logo,
                shift,
                ..Default::default()
            };
            let mut key = |c: &mut Core, key, pressed, logo, shift, changed| {
                c.physical_input_vec(
                    i,
                    crate::PhysicalEvent::KeyboardKey {
                        key,
                        pressed,
                        modifiers: mods(logo, shift),
                        mods_changed: changed,
                        time_us: 1000,
                    },
                )
                .unwrap();
            };
            key(c, KEY_LEFTMETA, true, true, false, true);
            key(c, KEY_LEFTSHIFT, true, true, true, true);
            key(c, KEY_TAB, true, true, true, false);
            key(c, KEY_TAB, false, true, true, false);
            key(c, KEY_LEFTSHIFT, false, true, false, true);
            key(c, KEY_LEFTMETA, false, false, false, true);
        };
        overview(&mut c, &mut i);
        assert!(c.is_presentation_suppressed(a), "the canvas is shown");
        overview(&mut c, &mut i);
        assert!(!c.is_presentation_suppressed(a), "the window is back");
    }

    /// Back in the game with the camera still framing the canvas: the next press opens the
    /// overview. It used to count as closing it, so the game stayed and nothing seemed to happen.
    #[test]
    fn the_overview_opens_after_returning_to_the_fullscreen_window_another_way() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = fullscreen_rig(&mut c, &mut i, &mut t);
        press_overview(&mut c, &mut i);
        assert!(c.is_presentation_suppressed(a));
        let mut fx = Vec::new();
        c.unsuppress_presentation(a, &mut fx);
        assert_eq!(c.focus().target(), Some(ws[0]));
        press_overview(&mut c, &mut i);
        assert!(c.is_presentation_suppressed(a), "the overview opened");
    }

    /// A window moving while the overview is open must not stop the next press closing it.
    #[test]
    fn the_overview_closes_after_a_window_moved_under_it() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, ws) = fullscreen_rig(&mut c, &mut i, &mut t);
        press_overview(&mut c, &mut i);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[1],
            crate::geom::CanvasPoint { x: 9000, y: 9000 },
            &mut fx,
        )
        .unwrap();
        press_overview(&mut c, &mut i);
        assert!(!c.is_presentation_suppressed(a), "the window is back");
    }

    /// A fullscreen window the user left holds on to nothing: with it stepped aside, hovering
    /// moves the focus as anywhere else, or the mouse seems dead over the other windows.
    #[test]
    fn hovering_moves_the_focus_away_from_a_fullscreen_window_stepped_aside() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (_, ws) = fullscreen_rig(&mut c, &mut i, &mut t);
        c.set_focus_follows_mouse(true);
        c.set_focus_follows_mouse_strict(true);
        press_overview(&mut c, &mut i);
        c.hover_seen = Some(ws[1]); // the next motion is a crossing, onto empty canvas
        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerMotion {
                delta: (0.5, 0.5),
                delta_unaccel: (0.5, 0.5),
                time_us: 2000,
            },
        )
        .unwrap();
        assert_ne!(c.focus().target(), Some(ws[0]));
    }

    /// Super+arrow (owner, 2026-11-01): to the nearest window that way, the camera with it.
    #[test]
    fn super_arrow_goes_to_the_nearest_window_that_way() {
        use crate::input::keys::{KEY_LEFT, KEY_LEFTMETA, KEY_RIGHT, KEY_UP};
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 6000, y: 0 }, &mut fx)
            .unwrap();
        c.place_by_user(ws[0], crate::geom::CanvasPoint { x: 0, y: 0 }, &mut fx)
            .unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        let cam = camera_of(&c, a);
        let arrow = |c: &mut Core, i: &mut crate::InputPort, key: u32| {
            press(c, i, KEY_LEFTMETA, true, true);
            press(c, i, key, true, true);
            press(c, i, key, false, true);
            press(c, i, KEY_LEFTMETA, false, false);
        };

        arrow(&mut c, &mut i, KEY_RIGHT);
        assert_eq!(c.focus().target(), Some(ws[1]));
        let at = c.cameras().center(cam).unwrap();
        assert!(at.x > 5000 << 16, "the camera came along: {at:?}");
        arrow(&mut c, &mut i, KEY_UP);
        assert_eq!(c.focus().target(), Some(ws[1]), "nothing that way: no move");
        arrow(&mut c, &mut i, KEY_LEFT);
        assert_eq!(c.focus().target(), Some(ws[0]));
    }

    #[test]
    fn jumps_record_history_and_bind_back_restores_while_recenter_does_not() {
        use crate::input::keys::{
            ACTION_NAMES, KEY_3, KEY_BACKSPACE, KEY_LEFT, KEY_LEFTMETA, KEY_SPACE, KEY_TAB,
            UserBindings, parse_chord,
        };
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 3);
        let mut fx = Vec::new();
        for (idx, x) in [0, 3000, 6000].into_iter().enumerate() {
            c.place_by_user(ws[idx], crate::geom::CanvasPoint { x, y: 0 }, &mut fx)
                .unwrap();
        }
        c.render_list(a).unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (100.0, 100.0),
        };
        let back = ACTION_NAMES
            .iter()
            .find(|(name, _)| *name == "BACK")
            .map(|(_, action)| *action);
        c.set_key_bindings(UserBindings {
            chords: vec![(parse_chord("SUPER+BACKSPACE").unwrap(), back)],
            moved: Vec::new(),
        });
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        let cam = camera_of(&c, a);
        let super_key = |c: &mut Core, i: &mut crate::InputPort, key: u32| {
            press(c, i, KEY_LEFTMETA, true, true);
            press(c, i, key, true, true);
            press(c, i, key, false, true);
            press(c, i, KEY_LEFTMETA, false, false);
        };

        // Super+Space recenters without pushing to navigation history.
        super_key(&mut c, &mut i, KEY_SPACE);
        assert_eq!(c.cameras().history(cam).unwrap().len(), 0);
        let pos0 = c.cameras().center(cam).unwrap();

        // Super+3 (FocusSlot) jumps to ws[2] and records pos0.
        super_key(&mut c, &mut i, KEY_3);
        assert_eq!(c.focus().target(), Some(ws[2]));
        assert_eq!(c.cameras().history(cam).unwrap().len(), 1);
        let pos_w2 = c.cameras().center(cam).unwrap();
        assert_ne!(pos_w2, pos0);

        // Super+Left (FocusDirection) jumps to ws[1] and records pos_w2.
        super_key(&mut c, &mut i, KEY_LEFT);
        assert_eq!(c.focus().target(), Some(ws[1]));
        assert_eq!(c.cameras().history(cam).unwrap().len(), 2);
        let pos_w1 = c.cameras().center(cam).unwrap();

        // Super+Tab with FOCUS_CYCLE_NAVIGATE moves back to ws[2] and records pos_w1.
        c.set_focus_cycle_navigate(true);
        super_key(&mut c, &mut i, KEY_TAB);
        assert_eq!(c.focus().target(), Some(ws[2]));
        assert_eq!(c.cameras().history(cam).unwrap().len(), 3);

        // BIND_BACK walks backwards through recorded jump positions.
        super_key(&mut c, &mut i, KEY_BACKSPACE);
        assert_eq!(c.cameras().center(cam), Some(pos_w1));
        super_key(&mut c, &mut i, KEY_BACKSPACE);
        assert_eq!(c.cameras().center(cam), Some(pos_w2));
        super_key(&mut c, &mut i, KEY_BACKSPACE);
        assert_eq!(c.cameras().center(cam), Some(pos0));
        assert_eq!(c.cameras().history(cam).unwrap().len(), 0);
    }

    #[test]
    fn strict_successor_moves_focus_once_and_keeps_attention() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        c.set_focus_follows_mouse_strict(true);
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(ws[0], crate::geom::CanvasPoint { x: 2000, y: 0 }, &mut fx)
            .unwrap();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 3000, y: 0 }, &mut fx)
            .unwrap();
        c.render_list(a).unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        c.set_focus(&mut i, Some(ws[1])).unwrap();
        c.focus.mark_attention(ws[0]);
        c.pointer = PointerState::Located {
            output: a,
            p: (100.0, 100.0),
        };
        assert_eq!(c.hovered_window(), None);

        let destroy_fx = c.toplevel_destroyed(ws[1]);
        assert!(
            !destroy_fx.contains(&Effect::KeyboardFocus(Some(ws[0]))),
            "strict_successor should not emit transient KeyboardFocus(Some(ws[0])) before KeyboardFocus(None): {destroy_fx:?}"
        );
        assert!(
            c.focus().attention().contains(&ws[0]),
            "ws[0]'s attention mark should survive when strict mode focuses None instead"
        );
    }

    #[test]
    fn audit_strict_successor_steals_on_demand_layer_focus() {
        use crate::geom::{LocalPoint, LocalRect, LocalSize};
        use crate::layer::{Anchors, KeyboardInteractivity, Layer, LayerState};
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree};
        let layer_tree = |k: u64, w: i32, h: i32, mapped: bool| SurfaceTree {
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
        };

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        c.set_focus_follows_mouse_strict(true);

        let panel = c.layer_created(Some(a)).unwrap();
        let lstate = LayerState {
            layer: Some(Layer::Top),
            anchors: Anchors {
                top: true,
                bottom: false,
                left: true,
                right: false,
            },
            size: (200, 80),
            margins: (0, 0, 0, 0),
            exclusive_zone: 0,
            keyboard: KeyboardInteractivity::OnDemand,
        };
        let lfx = c
            .layer_committed(panel, lstate, layer_tree(99, 0, 0, false))
            .unwrap();
        let lsize = lfx
            .iter()
            .find_map(|e| match e {
                Effect::LayerConfigure { layer, size } if *layer == panel => Some(*size),
                _ => None,
            })
            .expect("layer configure");
        c.layer_committed(panel, lstate, layer_tree(99, lsize.w, lsize.h, true))
            .unwrap();
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(ws[0], crate::geom::CanvasPoint { x: 3000, y: 0 }, &mut fx)
            .unwrap();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 4000, y: 0 }, &mut fx)
            .unwrap();
        c.render_list(a).unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        c.set_focus(&mut i, Some(ws[1])).unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (50.0, 40.0),
        };
        mouse_button(&mut c, &mut i, true);
        mouse_button(&mut c, &mut i, false);
        assert_eq!(c.focus().layer(), Some(panel));

        let _ = c.toplevel_destroyed(ws[1]);
        assert_eq!(
            c.focus().layer(),
            Some(panel),
            "closing underlying target window should not strip OnDemand layer keyboard focus"
        );
    }

    #[test]
    fn hover_over_300ms_before_a_non_keyboard_focus_switch_stays_in_history() {
        use crate::ShellCommand;
        use crate::stage::ElementRef;
        let (mut c, mut i, mut s, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.set_focus_follows_mouse(true);
        let ws = map_windows(&mut c, a, 3);
        let mut fx = Vec::new();
        c.place_by_user(
            ws[0],
            crate::geom::CanvasPoint { x: -450, y: -150 },
            &mut fx,
        )
        .unwrap();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 50, y: -150 }, &mut fx)
            .unwrap();
        c.place_by_user(ws[2], crate::geom::CanvasPoint { x: 500, y: -150 }, &mut fx)
            .unwrap();
        c.render_list(a).unwrap();

        c.set_focus(&mut i, Some(ws[0])).unwrap();
        c.set_focus(&mut i, Some(ws[1])).unwrap();
        c.pointer = PointerState::Located {
            output: a,
            p: (1455.0, 540.0),
        };
        c.hover_seen = None;
        // Hover enters ws[2] at t = 1.0s
        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerMotion {
                delta: (10.0, 0.0),
                delta_unaccel: (10.0, 0.0),
                time_us: 1_000_000,
            },
        )
        .unwrap();
        assert_eq!(c.focus().target(), Some(ws[2]));
        // 500ms later (>= 300ms HOVER_SETTLES_US) without keyboard input
        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerMotion {
                delta: (1.0, 0.0),
                delta_unaccel: (1.0, 0.0),
                time_us: 1_500_000,
            },
        )
        .unwrap();

        let cref = ElementRef {
            instance: c.stage().instance(),
            element: ws[0],
        };
        c.shell_command(&mut s, ShellCommand::Focus { element: cref })
            .unwrap();
        assert_eq!(c.focus().target(), Some(ws[0]));
        assert_eq!(
            c.focus().history().get(1).copied(),
            Some(ws[2]),
            "ws[2] stayed hover-focused for 500ms before ShellCommand::Focus and should be in Super+Tab history"
        );
    }

    #[test]
    fn audit_presentation_overview_toggle_closing_on_fullscreen_leaves_camera_zoomed_out() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        let mut fx = Vec::new();
        c.place_by_user(ws[1], crate::geom::CanvasPoint { x: 4000, y: 0 }, &mut fx)
            .unwrap();
        c.set_focus(&mut i, Some(ws[0])).unwrap();
        c.toplevel_request(
            ws[0],
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
        )
        .unwrap();
        let cam = c.cameras().camera_for_output(a).unwrap();
        let z0 = c.cameras().zoom(cam).unwrap();

        press_overview(&mut c, &mut i);
        assert!(c.is_presentation_suppressed(a));
        assert_eq!(c.cameras().history(cam).map(|h| h.len()), Some(1));
        assert!(c.cameras().zoom(cam).unwrap() < z0);

        press_overview(&mut c, &mut i);
        assert!(!c.is_presentation_suppressed(a));
        assert_eq!(c.presentation(a), Some(ws[0]));
        assert_eq!(
            c.cameras().history(cam).map(|h| h.len()),
            Some(0),
            "closing overview back to fullscreen must pop the overview camera history"
        );
        assert!(
            (c.cameras().zoom(cam).unwrap() - z0).abs() < 1e-6,
            "closing overview back to fullscreen must restore camera zoom (not stay zoomed out behind presentation)"
        );
    }

    #[test]
    fn audit_presentation_focus_next_to_other_output_window_wrongly_suppresses_fullscreen() {
        let far = PersistedCameras::from_entries(vec![(
            key("B"),
            CanvasPos {
                x: 50_000 * (1 << 16),
                y: 0,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, mut i, _, mut t) = Core::new(far);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        let (w1, w2) = (ws[0], ws[1]);
        let mut fx = Vec::new();
        c.place_by_user(w2, crate::geom::CanvasPoint { x: 49_800, y: -150 }, &mut fx)
            .unwrap();
        c.render_list(b).unwrap();
        assert_eq!(c.membership().observers(w2), vec![b]);
        assert_eq!(c.pointer_output(), Some(a));

        c.set_focus(&mut i, Some(w2)).unwrap();
        c.set_focus(&mut i, Some(w1)).unwrap();
        c.toplevel_request(
            w1,
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
        )
        .unwrap();

        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(w2));
        assert!(
            !c.is_presentation_suppressed(a),
            "cycling focus to w2 (already visible on B) must not suppress w1's presentation on A"
        );
    }

    #[test]
    fn audit_presentation_focus_next_with_pointer_on_other_output_fails_to_unsuppress() {
        let far = PersistedCameras::from_entries(vec![(
            key("B"),
            CanvasPos {
                x: 50_000 * (1 << 16),
                y: 0,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, mut i, _, mut t) = Core::new(far);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        let (w1, w2) = (ws[0], ws[1]);
        let mut fx = Vec::new();
        c.place_by_user(w2, crate::geom::CanvasPoint { x: 49_800, y: -150 }, &mut fx)
            .unwrap();
        c.render_list(b).unwrap();

        c.set_focus(&mut i, Some(w2)).unwrap();
        c.set_focus(&mut i, Some(w1)).unwrap();
        c.toplevel_request(
            w1,
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
        )
        .unwrap();
        c.suppress_presentation(a, &mut fx);
        c.set_focus(&mut i, Some(w2)).unwrap();
        assert!(c.is_presentation_suppressed(a));

        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerMotion {
                delta: (1000.0, 0.0),
                delta_unaccel: (1000.0, 0.0),
                time_us: 1000,
            },
        )
        .unwrap();
        assert_eq!(c.pointer_output(), Some(b));

        super_tab(&mut c, &mut i);
        assert_eq!(c.focus().target(), Some(w1));
        assert!(
            !c.is_presentation_suppressed(a),
            "returning focus to w1 (presenting on A) must unsuppress A even when the pointer is on B"
        );
    }

    #[test]
    fn audit_presentation_jump_to_window_with_pointer_on_other_output_leaves_presentation_covering_target()
     {
        let far = PersistedCameras::from_entries(vec![(
            key("B"),
            CanvasPos {
                x: 50_000 * (1 << 16),
                y: 0,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, mut i, _, mut t) = Core::new(far);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        let (w1, w3) = (ws[0], ws[1]);
        c.set_focus(&mut i, Some(w1)).unwrap();
        c.toplevel_request(
            w1,
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
        )
        .unwrap();
        assert!(c.hidden_by_presentation(w3));

        c.physical_input_vec(
            &mut i,
            crate::PhysicalEvent::PointerMotion {
                delta: (1000.0, 0.0),
                delta_unaccel: (1000.0, 0.0),
                time_us: 1000,
            },
        )
        .unwrap();
        assert_eq!(c.pointer_output(), Some(b));

        let mut fx = Vec::new();
        c.jump_to_window(w3, None, &mut fx);
        assert_eq!(c.focus().target(), Some(w3));
        assert!(
            c.is_presentation_suppressed(a),
            "jumping to w3 (hidden by A's presentation) must suppress A even when the pointer is on B"
        );
    }

    #[test]
    fn audit_presentation_show_chosen_over_suppresses_unrelated_second_output() {
        let far = PersistedCameras::from_entries(vec![(
            key("B"),
            CanvasPos {
                x: 50_000 * (1 << 16),
                y: 0,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, mut i, mut s, mut t) = Core::new(far);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        let ws = map_windows(&mut c, a, 3);
        let (w1, w2, w3) = (ws[0], ws[1], ws[2]);
        let mut fx = Vec::new();
        c.place_by_user(w2, crate::geom::CanvasPoint { x: 49_800, y: -150 }, &mut fx)
            .unwrap();
        c.render_list(b).unwrap();
        assert_eq!(c.membership().observers(w2), vec![b]);

        c.set_focus(&mut i, Some(w1)).unwrap();
        c.toplevel_request(
            w1,
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(a) },
        )
        .unwrap();
        c.set_focus(&mut i, Some(w2)).unwrap();
        c.toplevel_request(
            w2,
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(b) },
        )
        .unwrap();
        assert_eq!(c.presentation(a), Some(w1));
        assert_eq!(c.presentation(b), Some(w2));

        let r3 = crate::stage::ElementRef {
            instance: c.stage().instance(),
            element: w3,
        };
        c.shell_command(&mut s, crate::ShellCommand::Focus { element: r3 })
            .unwrap();
        assert!(c.is_presentation_suppressed(a));
        assert!(
            !c.is_presentation_suppressed(b),
            "focusing w3 (on A) must not suppress unrelated presentation on B"
        );
    }

    #[test]
    fn audit_presentation_shell_focus_does_not_raise_buried_window() {
        let (mut c, _, mut s, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        let (w1, w2) = (ws[0], ws[1]);
        assert_eq!(c.stage().z_order().last().copied(), Some(w2));

        let r1 = crate::stage::ElementRef {
            instance: c.stage().instance(),
            element: w1,
        };
        c.shell_command(&mut s, crate::ShellCommand::Focus { element: r1 })
            .unwrap();
        assert_eq!(c.focus().target(), Some(w1));
        assert_eq!(
            c.stage().z_order().last().copied(),
            Some(w1),
            "user-driven ShellCommand::Focus must raise the focused window above overlapping windows"
        );
    }

    #[test]
    fn audit_presentation_overview_toggle_includes_minimized_window() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let ws = map_windows(&mut c, a, 2);
        let (_w1, w2) = (ws[0], ws[1]);
        let mut fx = Vec::new();
        c.place_by_user(w2, crate::geom::CanvasPoint { x: 20_000, y: 0 }, &mut fx)
            .unwrap();
        // an app minimizes only itself while focused (client_minimize)
        c.set_focus(&mut i, Some(w2)).unwrap();
        c.client_minimize(w2);
        assert!(c.stage().is_minimized(w2), "the test needs w2 minimized");

        press_overview(&mut c, &mut i);
        let cam = c.cameras().camera_for_output(a).unwrap();
        let center_x = c.cameras().center(cam).unwrap().x >> 16;
        assert!(
            center_x.abs() < 1000,
            "overview center x={center_x} must ignore minimized w2 at x=20000"
        );
    }
}
