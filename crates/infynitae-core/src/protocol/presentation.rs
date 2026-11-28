//! Fullscreen presentations and maximized windows: grants, the presentation view, their end.

use super::*;

impl Core {
    /// F-PRESENT-1: the presentation view of the mapped fullscreen element of `model`'s output.
    /// Zoom 1; W's window-geometry origin at the output's origin, or W centred if it is smaller
    /// than the output: `v_P = anchor(W) − max(0, (view_O/scale_O − size(W)) / 2)`.
    pub(crate) fn presentation_view(
        &self,
        model: &crate::output::OutputModel,
    ) -> Option<crate::projection::View> {
        let element = self.presentation(model.id)?;
        let (anchor, tree, _) = self.stage.mapped(element)?;
        let extent = model.logical_extent();
        // the centre of a view whose origin is v_P
        let centre = |anchor: i64, extent: f64, size: i32| {
            let margin = ((extent - f64::from(size)) / 2.0).max(0.0);
            crate::geom::q16(anchor as f64 - margin + extent / 2.0)
        };
        let mut view = crate::projection::View::new(crate::projection::ViewParams {
            centre: crate::geom::CanvasPos {
                x: centre(anchor.x, extent.0, tree.geometry.size.w),
                y: centre(anchor.y, extent.1, tree.geometry.size.h),
            },
            zoom: 1.0,
            scale: model.scale,
            view_size: model.view_size(),
        })
        .ok()?;
        view.only = Some(element);
        Some(view)
    }

    /// C5.2 output choice: the output the client named; else W's frame-clock output if W is
    /// observed; else the output under the pointer; else the first output.
    pub(super) fn fullscreen_output(
        &self,
        id: ElementId,
        requested: Option<OutputId>,
    ) -> Option<OutputId> {
        requested
            .filter(|r| self.outputs.iter().any(|o| o.id == *r))
            .or_else(|| self.clock_output_of(id))
            .or_else(|| self.pointer_output())
            .or_else(|| self.outputs.first().map(|o| o.id))
    }

    /// C12.F13: the elements whose own presentation started or ended
    /// since `before` are the subjects of a client's fullscreen enter/exit; every other element
    /// the change touches may rise but not fall (`Sweep::presentation_change`).
    pub(super) fn presentation_stimulus(&mut self, before: &[(OutputId, Option<ElementId>)]) {
        for &(o, p) in before {
            let now = self.presentation(o);
            if now != p {
                for e in [p, now].into_iter().flatten() {
                    self.scale_sweep.stimulate(e);
                }
                self.scale_sweep.presentation_change = true;
            }
        }
    }

    /// The effects of an output's presentation having started, ended or changed shape, without
    /// the scale evaluation, which the caller runs once, after this (INV-SCALE-5): its
    /// ProjectionFrame is made stale, its membership recomputed (other elements leave or return)
    /// and every element it observed before or observes now marked for that evaluation (a
    /// scoped pass during a camera drag visits only marked elements), the constraints are
    /// reconciled (M13), and the pointer gets one true position if it is on that output, free and
    /// not held by a kept lock (M10; no absolute motion to a locked surface, INV-INPUT-5 as
    /// widened by C12.F17 Q2).
    pub(super) fn presentation_membership(&mut self, outputs: &[OutputId], out: &mut Vec<Effect>) {
        // the next table is built in its reused storage (as in `visibility_pass_camera`)
        let mut next = std::mem::take(&mut self.membership_next);
        let mut changed = false;
        for &o in outputs {
            self.invalidate_frame(o);
            if self.outputs.iter().any(|m| m.id == o) {
                let view = self.view(o);
                if changed {
                    next.recompute_output(o, view.as_ref(), &self.stage);
                } else {
                    next.assign_with_output(
                        &self.membership,
                        o,
                        view.as_ref(),
                        &self.stage,
                        &mut self.rows_buf,
                    );
                    changed = true;
                }
            }
        }
        if changed {
            // a scoped evaluation (during a camera drag the pass is not full) must still visit
            // every element these outputs observed before or observe now
            for (element, _, o) in self.membership.rows().iter().chain(next.rows()) {
                if outputs.contains(o) {
                    self.scale_sweep.mark(*element);
                }
            }
            self.membership.diff(&next, &self.stage, out);
            std::mem::swap(&mut self.membership, &mut next);
        }
        self.membership_next = next;
        for &o in outputs {
            if self.presentation(o).is_some()
                && !self.is_presentation_suppressed(o)
                && self.touches.on_output(o)
            {
                let frame_above = self.layer_frame(o).above;
                let kept_above = self.touches.all_points_on(|tp| {
                    tp.output == o
                        && matches!(tp.anchor, crate::touch::Anchor::Layer { .. })
                        && frame_above.iter().any(|e| e.surface == tp.target.surface)
                });
                if !kept_above && self.touches.clear() {
                    out.push(Effect::TouchCancel);
                }
                self.touch_resize_settle(out);
            }
        }
        let on_one = matches!(self.pointer, crate::pointer::PointerState::Located { output, .. } if outputs.contains(&output));
        out.extend(self.reconcile_constraints());
        if on_one && !self.input.interaction.active() && !self.locked() {
            self.true_position_into(out);
        }
    }

    /// What every output presents now, to tell afterwards which presentations a change touched.
    pub(super) fn presentations(&self) -> Vec<(OutputId, Option<ElementId>)> {
        let mut out = Vec::new();
        self.fill_presentations(&mut out);
        out
    }

    /// [`Core::presentations`] into reused storage (cleared, not freed).
    pub(super) fn fill_presentations(&self, out: &mut Vec<(OutputId, Option<ElementId>)>) {
        out.clear();
        out.extend(self.outputs.iter().map(|o| (o.id, self.presentation(o.id))));
    }

    /// Fullscreen entry and exit, and an output reconfiguration that resizes a presented element:
    /// every scale hint of the batch goes before its first configure,
    /// as the initial hint precedes the initial configure (M15), so the client draws the frame the
    /// configure asks for at the hint in force. Stable: each group keeps its order; in place.
    pub(crate) fn hints_before_configures(fx: &mut [Effect]) {
        let configure = |e: &Effect| {
            matches!(
                e,
                Effect::ToplevelConfigure(_) | Effect::ToplevelConfigureSize { .. }
            )
        };
        let Some(first) = fx.iter().position(configure) else {
            return;
        };
        let mut at = first;
        for k in first..fx.len() {
            if matches!(fx[k], Effect::PreferredScale { .. }) {
                fx[at..=k].rotate_right(1);
                at += 1;
            }
        }
    }

    /// The presentation change of the outputs whose presentation differs from `before`;
    /// when none does (a grant to an element that is not mapped yet, say), only the constraints
    /// are reconciled and the pointer is left alone.
    pub(super) fn presentation_changed_since(
        &mut self,
        before: &[(OutputId, Option<ElementId>)],
    ) -> Vec<Effect> {
        let changed: Vec<OutputId> = before
            .iter()
            .filter(|(o, p)| self.presentation(*o) != *p)
            .map(|(o, _)| *o)
            .collect();
        if changed.is_empty() {
            self.reconcile_constraints()
        } else {
            self.presentation_stimulus(before);
            let mut fx = Vec::new();
            self.presentation_membership(&changed, &mut fx);
            // C12.F13: subjects and touched elements are classified per element
            fx.extend(self.evaluate_scale_policy(true));
            fx
        }
    }

    /// The configure that tells `old`'s element it is no longer fullscreen, offering the size
    /// saved at its grant (M9: "configure with the saved size").
    fn restore_effect(old: FullscreenState) -> Effect {
        match old.saved {
            Some(size) => Effect::ToplevelConfigureSize {
                element: old.element,
                size,
            },
            None => Effect::ToplevelConfigure(old.element),
        }
    }

    /// M9 `Windowed → Fullscreen{O}`: stores the state (moving W's own if it was fullscreen
    /// elsewhere) and, if W is mapped, replaces every older grant on O, whose elements are told
    /// so. A grant to an element that is not mapped replaces nothing yet: it does so when the
    /// element maps ([`Core::take_output`]), so a client that never draws cannot end another
    /// element's presentation (audit finding A-3). Returns the replaced elements' configures. It
    /// sends no configure to W: the caller does, as the answer to the request.
    pub(super) fn store_fullscreen(
        &mut self,
        id: ElementId,
        output: OutputId,
        saved: Option<LocalSize>,
    ) -> Vec<Effect> {
        let mut fx = Vec::new();
        let mut saved = saved;
        let mut repeat = false;
        self.returning.retain(|e| *e != id);
        if let Some(i) = self.fullscreen.iter().position(|f| f.element == id) {
            // still fullscreen: the size to return to is the one saved at the first grant
            let old = self.fullscreen.remove(i);
            saved = old.saved;
            repeat = old.output == output;
        }
        self.fullscreen.push(FullscreenState {
            element: id,
            output,
            saved,
        });
        // an app asking again for the fullscreen it already has (games do on every focus) does
        // not take back a screen the user turned away from: that would raise it over the window
        // the user chose (owner, 2026-11-15: "fullscreen apps still appear above other windows")
        if repeat && self.is_presentation_suppressed(output) {
            return fx;
        }
        if self.stage.mapped(id).is_some() {
            fx.extend(self.take_output(id));
        }
        fx
    }

    /// `id`'s grant, now that its element is mapped, replaces the older grants on its output
    /// (INV-PRES-1 "a newer grant replaces an older one"): the presented element's, and any older
    /// grant still waiting for its element's first map. A newer grant still waiting is kept: it
    /// replaces this one when its element maps. `fullscreen` is kept in the order the grants
    /// were made. Returns the replaced elements' configures.
    pub(super) fn take_output(&mut self, id: ElementId) -> Vec<Effect> {
        let Some(mine) = self.fullscreen.iter().position(|f| f.element == id) else {
            return Vec::new();
        };
        let output = self.fullscreen[mine].output;
        // the grant's commit recomputes membership and scale for the output: reconciling here,
        // with the old grant still listed, would emit effects nobody delivers
        self.clear_presentation_suppression(output);
        let (ended, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.fullscreen)
            .into_iter()
            .enumerate()
            .partition(|(i, f)| {
                f.output == output
                    && f.element != id
                    && (*i < mine || self.stage.mapped(f.element).is_some())
            });
        self.fullscreen = kept.into_iter().map(|(_, f)| f).collect();
        ended
            .into_iter()
            .map(|(_, old)| self.fullscreen_ended(old))
            .collect()
    }

    /// Forgets `id`'s fullscreen state and every pending piece of it — a recorded pre-map
    /// request, a postponed windowed placement — without telling the client (unmap and
    /// destruction: the protocol state went with the surface's role state). The caller reports
    /// the presentation that ended.
    pub(super) fn drop_fullscreen(&mut self, id: ElementId) {
        // nor is it held to a size or maximized: its next map starts at its own size
        self.held_sizes.retain(|(e, _)| *e != id);
        self.layout_cells.retain(|(e, _)| *e != id);
        self.returning.retain(|e| *e != id);
        self.maximized.retain(|(e, _)| *e != id);
        self.maximized_at.retain(|(e, _)| *e != id);
        self.left_fullscreen.retain(|e| *e != id);
        self.stage.set_saved_floating(id, None);
        self.premap_fullscreen.retain(|(e, _)| *e != id);
        self.fullscreen.retain(|f| f.element != id);
        self.windowed_placement_pending.retain(|e| *e != id);
        self.restore_offers.retain(|(e, _)| *e != id);
    }

    /// Default windowed placement (F-PLACE-1), resolved once per element through `stage::place`.
    /// Does nothing for an element already placed.
    pub(super) fn place_by_default(
        &mut self,
        id: ElementId,
        tree: &SurfaceTree,
    ) -> Result<(), ProtocolError> {
        self.windowed_placement_pending.retain(|e| *e != id);
        if self.stage.default_placed(id) {
            return Ok(());
        }
        let stale = |_| ProtocolError::StaleElement;
        if let Some(anchor) = self.first_map_anchor(id, tree) {
            self.stage.place(id, anchor).map_err(stale)?;
        }
        self.stage.set_default_placed(id).map_err(stale)
    }

    /// An element's fullscreen ended while it stays mapped (unset, replacement, output removal):
    /// returns the configure that tells it so, and, if it has never been placed as a window,
    /// postpones its default placement to its first windowed presentation (P9b): the first
    /// commit after the client acknowledged a configure without the fullscreen state.
    pub(super) fn fullscreen_ended(&mut self, old: FullscreenState) -> Effect {
        if !self.stage.default_placed(old.element)
            && self.stage.mapped(old.element).is_some()
            && !self.windowed_placement_pending.contains(&old.element)
        {
            self.windowed_placement_pending.push(old.element);
        }
        // a placed window's first windowed frame decides whether it keeps its place
        if self.stage.default_placed(old.element) && !self.returning.contains(&old.element) {
            self.returning.push(old.element);
        }
        // The size offered is remembered: the element's geometry is still the fullscreen one
        // until it redraws, so a grant made before that must save the offered size again, not
        // the geometry (the size a window returns to is its last windowed size).
        self.restore_offers.retain(|(e, _)| *e != old.element);
        if let Some(size) = old.saved {
            self.restore_offers.push((old.element, size));
        }
        Self::restore_effect(old)
    }

    /// INV-OUTPUT-1 / M9 `Fullscreen{O} | O disconnected → Windowed`: the states on a removed
    /// output are dropped, each with its configure.
    /// The windows maximized on it fill the first remaining output's usable area instead (they
    /// stay maximized, at the size of the screen that is left), before a fullscreen one among
    /// them is told the size it returns to.
    pub(crate) fn fullscreen_output_removed(&mut self, output: OutputId) -> Vec<Effect> {
        let mut fx = Vec::new();
        match self.outputs.first().map(|o| o.id) {
            Some(first) => {
                self.maximized
                    .iter_mut()
                    .filter(|(_, o)| *o == output)
                    .for_each(|(_, o)| *o = first);
                self.refit_maximized(first, &mut fx);
            }
            None => self.maximized.retain(|(_, o)| *o != output),
        }
        self.premap_fullscreen
            .iter_mut()
            .filter(|(_, o)| *o == Some(output))
            .for_each(|(_, o)| *o = None);
        while let Some(i) = self.fullscreen.iter().position(|f| f.output == output) {
            let old = self.fullscreen.remove(i);
            fx.push(self.fullscreen_ended(old));
        }
        fx
    }

    /// M4 reconfiguration: "one configure only for an element in protocol fullscreen on this
    /// output" — its size follows the output's. (A window maximized on it fills the new usable
    /// area: the layers' arrangement before this refits it, [`Core::refit_maximized`].)
    pub(crate) fn fullscreen_output_reconfigured(&mut self, output: OutputId) -> Vec<Effect> {
        self.fullscreen
            .iter()
            .filter(|f| f.output == output)
            .map(|f| Effect::ToplevelConfigure(f.element))
            .collect()
    }

    /// After a change of the first output (a mode, scale or rotation): a window that is not
    /// maximized or fullscreen and is now larger than the room (the usable area less the 12 px
    /// gap on each side, INV-PROTO-1) is offered the room, as an oversized new window is (owner,
    /// 2026-10-31). Nothing moves; the side that fits keeps its size.
    pub(crate) fn offer_room_after_change(&mut self, output: OutputId, out: &mut Vec<Effect>) {
        if self.outputs.first().map(|o| o.id) != Some(output) {
            return;
        }
        let Some((_, _, uw, uh)) = self.usable_area(output) else {
            return;
        };
        let fit = |v: f64| ((v - 2.0 * 12.0).floor() as i32).max(1);
        let room = (fit(uw), fit(uh));
        for id in self.stage.mapped_elements() {
            if self.fullscreen_of(id).is_some() || self.maximized.iter().any(|(e, _)| *e == id) {
                continue;
            }
            let Some(tree) = self.stage.tree(id) else {
                continue;
            };
            let size = self.held_size(id).unwrap_or(tree.geometry.size);
            if size.w <= room.0 && size.h <= room.1 {
                continue;
            }
            let fitted = LocalSize {
                w: size.w.min(room.0),
                h: size.h.min(room.1),
            };
            self.hold_size(id, fitted);
            out.push(Effect::ToplevelConfigureSize {
                element: id,
                size: fitted,
            });
        }
    }

    /// A maximized window the user resizes (a drag of any kind, a scaled cluster) is maximized no
    /// longer (owner, 2026-11-06): its size is the one the user gives it, so a bar or a screen
    /// change does not snap it back, and the next Super+Enter maximizes it again instead of
    /// restoring the size before.
    pub(crate) fn resized_out_of_maximize(&mut self, id: ElementId) {
        if !self.maximized.iter().any(|(e, _)| *e == id) {
            return;
        }
        self.set_maximized(id, None);
        self.stage.set_saved_floating(id, None);
    }

    /// Whether `id` is maximized on an output.
    pub fn is_maximized(&self, id: ElementId) -> bool {
        self.maximized.iter().any(|(e, _)| *e == id)
    }

    /// Super+Enter maximized `id` to fill `output`'s usable area (`Some`), or restored it.
    pub(crate) fn set_maximized(&mut self, id: ElementId, output: Option<OutputId>) {
        self.maximized.retain(|(e, _)| *e != id);
        self.maximized_at.retain(|(e, _)| *e != id);
        if let Some(o) = output {
            self.maximized.push((id, o));
        }
    }

    /// The windows maximized on `output` fill its usable area again after it changed (a mode,
    /// scale or rotation, or the output they were on removed): each keeps its centre on the Canvas
    /// and is asked for the new size, rounded as Super+Enter rounds it. One that is fullscreen too
    /// is not told now: the size its fullscreen returns it to becomes the new one. Returns the
    /// windows it moved; the caller runs their membership (or the visibility pass).
    pub(crate) fn refit_maximized(
        &mut self,
        output: OutputId,
        out: &mut Vec<Effect>,
    ) -> Vec<ElementId> {
        let Some((_, _, uw, uh)) = self.usable_area(output) else {
            return Vec::new();
        };
        let size = LocalSize {
            w: uw.round().max(1.0) as i32,
            h: uh.round().max(1.0) as i32,
        };
        let mut moved = Vec::new();
        for i in 0..self.maximized.len() {
            let (id, o) = self.maximized[i];
            if o != output {
                continue;
            }
            let (Some(anchor), Some(tree)) = (self.stage.anchor(id), self.stage.tree(id)) else {
                continue;
            };
            let was = self.held_size(id).unwrap_or(tree.geometry.size);
            if was == size {
                continue;
            }
            let at = CanvasPoint {
                x: anchor.x + i64::from(was.w) / 2 - i64::from(size.w) / 2,
                y: anchor.y + i64::from(was.h) / 2 - i64::from(size.h) / 2,
            };
            if self.stage.place(id, at).is_err() {
                continue;
            }
            for m in self.maximized_at.iter_mut().filter(|(e, _)| *e == id) {
                m.1 = at;
            }
            moved.push(id);
            self.hold_size(id, size);
            match self.fullscreen.iter_mut().find(|f| f.element == id) {
                Some(f) => f.saved = Some(size),
                None => out.push(Effect::ToplevelConfigureSize { element: id, size }),
            }
        }
        // only a window that moved makes the frames stale (the output's own frame is the
        // caller's: INC2-F07)
        if !moved.is_empty() {
            self.invalidate_all_frames();
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::PersistedCameras;
    use crate::output::{OutputDescriptor, Transform};
    use crate::protocol::tests::*;
    use crate::snapshot::tests::key;

    /// M9 rows 1, 2, 3 and C5.2 Case B, with values derived by hand. Output A 1920x1080 at scale
    /// 1, camera (0,0). W1 and W2 are 400x300, centred: anchor (−200,−150).
    ///
    /// - W2 (not focused) asks: declined, one configure, nothing stored.
    /// - W1 (focused) asks: granted on A. Saved size 400x300. Its configure carries `fullscreen`
    ///   and 1920x1080. W2 leaves A. While W1 is still 400x300 the presentation view centres it:
    ///   v_P = (−200,−150) − ((1920−400)/2, (1080−300)/2) = (−960,−540), so it is drawn at
    ///   [760,1160)x[390,690), where the camera showed it.
    /// - W1 commits 1920x1080: v_P = anchor, so it covers [0,1920)x[0,1080); the anchor is still
    ///   (−200,−150).
    /// - W1 asks to leave: one configure offering 400x300; W2 returns; the view is the camera's
    ///   again, exactly as before.
    #[test]
    fn fullscreen_is_granted_to_the_focused_element_and_presented_on_its_output() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w1 = map_win(&mut c, 1, 400, 300);
        let w2 = map_win(&mut c, 2, 400, 300);
        focus(&mut c, &mut i, w1);
        let camera_view = c.view(a).unwrap();
        let untouched = authority_state(&c);
        assert_eq!(
            rects(&mut c, a),
            vec![(w2, (760, 390, 1160, 690)), (w1, (760, 390, 1160, 690))]
        );

        // not focused: declined
        let (d, fx) = c.toplevel_request(w2, FS).unwrap();
        assert_eq!(
            (d, fx),
            (
                ToplevelDecision::Declined,
                vec![Effect::ToplevelConfigure(w2)]
            )
        );
        assert!(
            c.fullscreen().is_empty(),
            "a declined request stored something"
        );
        assert_eq!(
            c.toplevel_configure_state(w2),
            ToplevelConfigureState::default()
        );

        // focused: granted
        let (d, fx) = c.toplevel_request(w1, FS).unwrap();
        assert_eq!(d, ToplevelDecision::Granted);
        assert_eq!(
            fx[0],
            Effect::ToplevelConfigure(w1),
            "the answer comes first"
        );
        assert_eq!(
            fx.iter()
                .filter(|e| matches!(
                    e,
                    Effect::ToplevelConfigure(_) | Effect::ToplevelConfigureSize { .. }
                ))
                .count(),
            1,
            "exactly one configure: {fx:?}"
        );
        assert!(
            fx.contains(&Effect::Leave {
                output: a,
                surface: key(2)
            }),
            "the other element leaves the output: {fx:?}"
        );
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Leave { surface, .. } if *surface == key(1)))
        );
        assert_eq!(
            c.fullscreen(),
            &[FullscreenState {
                element: w1,
                output: a,
                saved: Some(LocalSize { w: 400, h: 300 })
            }]
        );
        assert_eq!(
            c.toplevel_configure_state(w1),
            ToplevelConfigureState {
                fullscreen: Some(a),
                resizing: false,
                maximized: false,
                size: Some(LocalSize { w: 1920, h: 1080 }),
                held: None,
            }
        );
        assert_eq!(c.presentation(a), Some(w1));
        assert_eq!(c.membership().observers(w2), vec![], "W2 is not shown on A");
        let v = c.view(a).unwrap();
        assert_eq!(v.only, Some(w1));
        assert_eq!(
            (v.v.x, v.v.y, v.zoom),
            (-960 * Q16_ONE, -540 * Q16_ONE, 1.0)
        );
        assert_eq!(rects(&mut c, a), vec![(w1, (760, 390, 1160, 690))]);
        assert_eq!(
            authority_state(&c),
            untouched,
            "a grant moved an anchor, a camera or the focus"
        );

        // the client resizes to the configured size
        let fx = c.toplevel_committed(w1, win(1, 1920, 1080, true)).unwrap();
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::ToplevelConfigure(_))),
            "{fx:?}"
        );
        assert_eq!(c.stage().anchor(w1), Some(CanvasPoint { x: -200, y: -150 }));
        let v = c.view(a).unwrap();
        assert_eq!(
            (v.v.x, v.v.y),
            (-200 * Q16_ONE, -150 * Q16_ONE),
            "v_P = anchor"
        );
        assert_eq!(rects(&mut c, a), vec![(w1, (0, 0, 1920, 1080))]);
        // every pixel of the output is W1's: the corners hit it at its own local coordinates
        for (p, local) in [((0.5, 0.5), (0, 0)), ((1919.5, 1079.5), (1919, 1079))] {
            let hit = c
                .hit_test(a, crate::geom::OutputLogicalPoint { x: p.0, y: p.1 })
                .unwrap();
            assert_eq!(hit.element, w1);
            assert_eq!((hit.local.x / Q16_ONE, hit.local.y / Q16_ONE), local);
        }
        // popups of the fullscreen element are constrained to the output: its whole surface
        assert_eq!(
            c.popup_constraint_target(w1, None),
            Some(LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size: LocalSize { w: 1920, h: 1080 }
            })
        );

        // a second request while fullscreen is answered, and the saved size is not overwritten
        let (d, fx) = c.toplevel_request(w1, FS).unwrap();
        assert_eq!(
            (d, fx),
            (
                ToplevelDecision::Granted,
                vec![Effect::ToplevelConfigure(w1)]
            )
        );
        assert_eq!(c.fullscreen()[0].saved, Some(LocalSize { w: 400, h: 300 }));

        // leave
        let (d, fx) = c
            .toplevel_request(w1, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(d, ToplevelDecision::Released);
        assert_eq!(
            fx[0],
            Effect::ToplevelConfigureSize {
                element: w1,
                size: LocalSize { w: 400, h: 300 }
            }
        );
        assert!(
            fx.contains(&Effect::Enter {
                output: a,
                surface: key(2)
            }),
            "W2 returns: {fx:?}"
        );
        assert!(c.fullscreen().is_empty());
        assert_eq!(
            c.toplevel_configure_state(w1),
            ToplevelConfigureState::default()
        );
        assert_eq!(
            c.view(a),
            Some(camera_view),
            "the camera's view, exactly as it was"
        );
        assert_eq!(authority_state(&c), untouched);
        // not fullscreen any more: a further unset is answered once and changes nothing
        let (d, fx) = c
            .toplevel_request(w1, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(
            (d, fx),
            (
                ToplevelDecision::Declined,
                vec![Effect::ToplevelConfigure(w1)]
            )
        );
    }

    /// M2 pre-configure and C5.2 "request before first map": the request is recorded, the
    /// initial configure carries `fullscreen` and the output's size, nothing is presented until
    /// the element is mapped, and placement runs regardless. An unset before the initial commit
    /// withdraws it. After a remap the client must ask again, and then needs the focus.
    #[test]
    fn a_fullscreen_request_before_the_first_map_is_granted_in_the_initial_configure() {
        let (mut c, _i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let other = map_win(&mut c, 9, 300, 200);
        let w = c.toplevel_created();
        assert_eq!(
            c.toplevel_request(w, FS).unwrap(),
            (ToplevelDecision::Recorded, vec![])
        );
        assert!(
            c.fullscreen().is_empty(),
            "decided at the initial commit, not before"
        );
        let fx = c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        // with the scale policy present the initial hint precedes the configure (M15)
        assert!(
            matches!(fx.first(), Some(Effect::PreferredScale { element, .. }) if *element == w),
            "the initial scale hint comes first: {fx:?}"
        );
        assert_eq!(
            fx[1..],
            [Effect::InitialConfigure(w)],
            "one configure, the initial one"
        );
        assert_eq!(
            c.toplevel_configure_state(w),
            ToplevelConfigureState {
                fullscreen: Some(a),
                resizing: false,
                maximized: false,
                size: Some(LocalSize { w: 1920, h: 1080 }),
                held: None,
            }
        );
        assert_eq!(
            c.fullscreen(),
            &[FullscreenState {
                element: w,
                output: a,
                saved: None
            }]
        );
        assert_eq!(
            c.presentation(a),
            None,
            "nothing of W exists to present yet"
        );
        assert_eq!(
            c.membership().observers(other),
            vec![a],
            "the output still shows the Stage"
        );
        // first map at the configured size: presented, and the other element leaves. It is not
        // placed as a window yet: the anchor is still the provisional one, the camera centre
        let fx = c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: 0, y: 0 }));
        assert!(
            !c.stage().default_placed(w),
            "a fullscreen map used up the placement"
        );
        assert!(
            fx.contains(&Effect::Leave {
                output: a,
                surface: key(9)
            }),
            "{fx:?}"
        );
        assert!(
            fx.contains(&Effect::Enter {
                output: a,
                surface: key(1)
            }),
            "{fx:?}"
        );
        assert_eq!(c.presentation(a), Some(w));
        assert_eq!(rects(&mut c, a), vec![(w, (0, 0, 1920, 1080))]);
        // leaving: no size was saved, so none is offered
        let (d, fx) = c
            .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(d, ToplevelDecision::Released);
        assert_eq!(fx[0], Effect::ToplevelConfigure(w));
        // unmap, re-show: before the buffer, without the focus, the request is declined
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        assert_eq!(
            c.toplevel_request(w, FS).unwrap(),
            (ToplevelDecision::Ignored, vec![])
        );
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        assert_eq!(c.stage().state(w), Some(Lifecycle::AwaitingFirstBuffer));
        assert_eq!(
            c.toplevel_request(w, FS).unwrap(),
            (
                ToplevelDecision::Declined,
                vec![Effect::ToplevelConfigure(w)]
            ),
            "a remap is not a first map"
        );

        // withdrawn before the initial commit
        let x = c.toplevel_created();
        c.toplevel_request(x, FS).unwrap();
        assert_eq!(
            c.toplevel_request(x, ToplevelRequest::UnsetFullscreen)
                .unwrap(),
            (ToplevelDecision::Ignored, vec![])
        );
        c.toplevel_committed(x, win(3, 0, 0, false)).unwrap();
        assert_eq!(
            c.toplevel_configure_state(x),
            ToplevelConfigureState::default()
        );
        // between the initial configure and the first buffer: granted at once, one configure
        let (d, fx) = c.toplevel_request(x, FS).unwrap();
        assert_eq!(
            (d, fx),
            (
                ToplevelDecision::Granted,
                vec![Effect::ToplevelConfigure(x)]
            )
        );
    }

    /// A window whose first map is in protocol fullscreen keeps its default-placement
    /// opportunity. It is placed by F-PLACE-1 at its first windowed presentation: the first
    /// commit, after fullscreen ended, whose geometry differs from the one it had then. Commits
    /// still in flight at the fullscreen size are not that. After it, the anchor is final.
    /// Camera at (700, −300), so every value differs from the default-camera case:
    /// provisional anchor (700, −300); 400x300 centred there is (500, −450).
    #[test]
    fn a_window_that_starts_fullscreen_is_placed_at_its_first_windowed_size() {
        let cam = PersistedCameras::from_entries(vec![(
            crate::output::tests::key("A"),
            CanvasPos {
                x: 700 * Q16_ONE,
                y: -300 * Q16_ONE,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, _i, _, mut t) = Core::new(cam);
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = c.toplevel_created();
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        let provisional = CanvasPoint { x: 700, y: -300 };
        assert_eq!(c.stage().anchor(w), Some(provisional));
        assert_eq!(
            rects(&mut c, a),
            vec![(w, (0, 0, 1920, 1080))],
            "presented at the origin"
        );
        // a second commit while fullscreen changes nothing about placement
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert!(!c.stage().default_placed(w));

        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(
            c.stage().anchor(w),
            Some(provisional),
            "the exit itself moves nothing"
        );
        // frames still in flight: committed before the client acknowledged the exit, whatever
        // their size (P9b: the acknowledgement, not the size, marks the first windowed frame)
        c.toplevel_committed_acked(w, win(1, 1920, 1080, true), true)
            .unwrap();
        c.toplevel_committed_acked(w, win(1, 400, 300, true), true)
            .unwrap();
        assert_eq!(
            c.stage().anchor(w),
            Some(provisional),
            "placed at a frame committed under the fullscreen state"
        );
        // the first commit after the acknowledgement: placed now, centred on the camera
        c.toplevel_committed_acked(w, win(1, 400, 300, true), false)
            .unwrap();
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: 500, y: -450 }));
        assert!(c.stage().default_placed(w));
        assert_eq!(
            rects(&mut c, a),
            vec![(w, (760, 390, 1160, 690))],
            "centred in the view"
        );
        // final: a later size, another fullscreen round trip and a remap keep it
        let placed = CanvasPoint { x: 500, y: -450 };
        c.toplevel_committed(w, win(1, 640, 480, true)).unwrap();
        assert_eq!(c.stage().anchor(w), Some(placed));
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 300, 200, true)).unwrap();
        assert_eq!(c.stage().anchor(w), Some(placed), "a remap placed it again");
    }

    /// P9b: a client that keeps the fullscreen size as its windowed size is placed at its first
    /// windowed commit (oversized: F-PLACE-1's fallback puts its origin at the view's corner);
    /// a second fullscreen round trip before it acknowledges keeps the placement waiting; Core
    /// never resizes it.
    #[test]
    fn a_window_keeping_the_fullscreen_size_is_placed_at_its_first_windowed_commit() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = c.toplevel_created();
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        focus(&mut c, &mut i, w);
        let provisional = c.stage().anchor(w);
        // out of fullscreen and back in before the client acknowledged the exit
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        let fx = c.toplevel_request(w, FS).unwrap().1;
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::ToplevelConfigureSize { .. }))
        );
        c.toplevel_committed_acked(w, win(1, 1920, 1080, true), true)
            .unwrap();
        assert_eq!(c.stage().anchor(w), provisional, "placed while fullscreen");
        // the exit, acknowledged; the client keeps 1920x1080. Placement sends nothing: no
        // configure, no size (Core never resizes a client to place it)
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        let fx = c
            .toplevel_committed_acked(w, win(1, 1920, 1080, true), false)
            .unwrap();
        assert!(
            !fx.iter().any(|e| matches!(
                e,
                Effect::ToplevelConfigure(_) | Effect::ToplevelConfigureSize { .. }
            )),
            "placement configured the client: {fx:?}"
        );
        assert!(
            c.stage().default_placed(w),
            "a window keeping its size is never placed"
        );
        // 1920x1080 fills the 1920x1080 view exactly: its origin at the view's corner
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: -960, y: -540 }));
        assert!(c.placements_pending().is_empty());
    }

    /// The other ways such a window first becomes a window: fullscreen ends by replacement or by
    /// the removal of its output (placed at its next size, like after an unset); it is unmapped
    /// while fullscreen and re-shown as a window (placed at that map). And a window that was
    /// placed before going fullscreen returns to exactly its anchor: nothing is re-resolved.
    #[test]
    fn placement_after_fullscreen_covers_every_exit_and_restores_existing_anchors() {
        // unmapped while fullscreen-first, re-shown windowed
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = c.toplevel_created();
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap(); // unmap
        assert!(c.fullscreen().is_empty());
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        assert_eq!(
            c.stage().anchor(w),
            Some(CanvasPoint { x: -200, y: -150 }),
            "placed at the re-show"
        );

        // replaced by a newer grant, then resized
        let x = c.toplevel_created();
        c.toplevel_request(x, FS).unwrap();
        c.toplevel_committed(x, win(2, 0, 0, false)).unwrap();
        c.toplevel_committed(x, win(2, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(a), Some(x));
        // W is hidden on A; a second output shows it, so it may take the focus (A-8)
        let b = out(&mut c, &mut t, "B", (1920, 1080), 120);
        focus(&mut c, &mut i, w);
        let on_a = ToplevelRequest::SetFullscreen { output: Some(a) };
        c.toplevel_request(w, on_a).unwrap(); // replaces x on A
        c.output_disconnected(&mut t, b).unwrap();
        assert_eq!(c.presentation(a), Some(w));
        assert_eq!(
            c.stage().anchor(x),
            Some(CanvasPoint { x: 0, y: 0 }),
            "still provisional"
        );
        c.toplevel_committed(x, win(2, 600, 400, true)).unwrap();
        assert_eq!(c.stage().anchor(x), Some(CanvasPoint { x: -300, y: -200 }));

        // w was a placed window before it went fullscreen: its anchor comes back untouched
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: -200, y: -150 }));
        c.toplevel_committed(w, win(1, 500, 300, true)).unwrap();
        assert_eq!(
            c.stage().anchor(w),
            Some(CanvasPoint { x: -200, y: -150 }),
            "re-centred"
        );

        // its output is removed while it is fullscreen-first; destroyed before its first
        // windowed size, it leaves nothing pending for a later element
        let (mut c, _i, _, mut t) = Core::new(PersistedCameras::default());
        let _a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1280, 720), 120);
        let y = c.toplevel_created();
        c.toplevel_request(y, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        c.toplevel_committed(y, win(3, 0, 0, false)).unwrap();
        c.toplevel_committed(y, win(3, 1280, 720, true)).unwrap();
        c.output_disconnected(&mut t, b).unwrap();
        assert!(c.fullscreen().is_empty());
        assert!(!c.stage().default_placed(y));
        c.toplevel_committed(y, win(3, 500, 300, true)).unwrap();
        assert_eq!(
            c.stage().anchor(y),
            Some(CanvasPoint { x: -250, y: -150 }),
            "placed on the surviving output"
        );
        let z = c.toplevel_created();
        c.toplevel_request(z, FS).unwrap();
        c.toplevel_committed(z, win(4, 0, 0, false)).unwrap();
        c.toplevel_committed(z, win(4, 1920, 1080, true)).unwrap();
        c.toplevel_request(z, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(c.placements_pending(), vec![z], "waiting for its size");
        c.toplevel_destroyed(z);
        assert!(c.placements_pending().is_empty(), "gone with the element");
        // the same when it is unmapped instead
        let u = c.toplevel_created();
        c.toplevel_request(u, FS).unwrap();
        c.toplevel_committed(u, win(6, 0, 0, false)).unwrap();
        c.toplevel_committed(u, win(6, 1920, 1080, true)).unwrap();
        c.toplevel_request(u, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(c.placements_pending(), vec![u]);
        c.toplevel_committed(u, win(6, 0, 0, false)).unwrap(); // unmap
        assert!(c.placements_pending().is_empty(), "gone with the unmap");
        c.toplevel_destroyed(u);
        let n = c.toplevel_created();
        c.toplevel_committed(n, win(5, 0, 0, false)).unwrap();
        c.toplevel_committed(n, win(5, 200, 100, true)).unwrap();
        c.toplevel_committed(n, win(5, 300, 100, true)).unwrap();
        assert_eq!(
            c.stage().anchor(n),
            Some(CanvasPoint { x: -100, y: -50 }),
            "an ordinary window, placed once"
        );
    }

    /// M9 rows 4 and 5: losing the focus does not end fullscreen; a newer grant on the same
    /// output replaces the older one, whose element gets one configure with its saved size.
    #[test]
    fn a_newer_grant_replaces_the_older_one_and_focus_loss_does_not() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w1 = map_win(&mut c, 1, 400, 300);
        let w2 = map_win(&mut c, 2, 500, 350);
        focus(&mut c, &mut i, w1);
        c.toplevel_request(w1, FS).unwrap();
        // W2 is hidden on A by the presentation; a second output shows it, so it may take the
        // focus (A-8), and it names A in its request
        out(&mut c, &mut t, "B", (1920, 1080), 120);
        focus(&mut c, &mut i, w2);
        assert_eq!(c.focus().target(), Some(w2));
        assert_eq!(c.presentation(a), Some(w1), "focus loss ended fullscreen");
        assert_eq!(c.fullscreen().len(), 1);
        let on_a = ToplevelRequest::SetFullscreen { output: Some(a) };
        let (d, fx) = c.toplevel_request(w2, on_a).unwrap();
        assert_eq!(d, ToplevelDecision::Granted);
        assert_eq!(fx[0], Effect::ToplevelConfigure(w2));
        assert_eq!(
            fx[1],
            Effect::ToplevelConfigureSize {
                element: w1,
                size: LocalSize { w: 400, h: 300 }
            },
            "the replaced element is told, with its saved size"
        );
        assert!(fx.contains(&Effect::Leave {
            output: a,
            surface: key(1)
        }));
        assert!(fx.contains(&Effect::Enter {
            output: a,
            surface: key(2)
        }));
        assert_eq!(
            c.fullscreen(),
            &[FullscreenState {
                element: w2,
                output: a,
                saved: Some(LocalSize { w: 500, h: 350 })
            }]
        );
        assert_eq!(
            c.toplevel_configure_state(w1),
            ToplevelConfigureState::default()
        );
    }

    /// Every way a presentation ends leaves no state behind: unmap, destruction (mapped, and
    /// between grant and first map), and removal of the output, which sends the configure with
    /// the saved size before the output goes. A reconfigured output re-sends one configure with
    /// its new size.
    #[test]
    fn fullscreen_ends_cleanly_on_unmap_destroy_and_output_changes() {
        // unmap
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let other = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(c.membership().observers(other), vec![]);
        let fx = c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        assert!(c.fullscreen().is_empty(), "state survived the unmap");
        assert!(
            fx.contains(&Effect::Enter {
                output: a,
                surface: key(9)
            }),
            "{fx:?}"
        );
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::ToplevelConfigureSize { .. })),
            "an unmapped surface was sent a size: {fx:?}"
        );
        assert_eq!(c.presentation(a), None);
        // remapped: not fullscreen, and placed where it was
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        assert_eq!(
            c.toplevel_configure_state(w),
            ToplevelConfigureState::default()
        );
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: -200, y: -150 }));

        // destroyed while presented
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(c.presentation(a), Some(w));
        let fx = c.toplevel_destroyed(w);
        assert!(c.fullscreen().is_empty());
        assert!(
            fx.contains(&Effect::Enter {
                output: a,
                surface: key(9)
            }),
            "{fx:?}"
        );
        assert_eq!(c.membership().observers(other), vec![a]);
        // destroyed between the grant and the first map, and with a request only recorded
        let x = c.toplevel_created();
        c.toplevel_request(x, FS).unwrap();
        c.toplevel_committed(x, win(3, 0, 0, false)).unwrap();
        assert_eq!(c.fullscreen().len(), 1);
        c.toplevel_destroyed(x);
        let y = c.toplevel_created();
        c.toplevel_request(y, FS).unwrap();
        c.toplevel_destroyed(y);
        assert!(c.fullscreen().is_empty());
        let z = c.toplevel_created();
        c.toplevel_committed(z, win(4, 0, 0, false)).unwrap();
        assert_eq!(
            c.toplevel_configure_state(z),
            ToplevelConfigureState::default(),
            "a dead element's recorded request reached another"
        );

        // output reconfigured, then removed
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1280, 720), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        let (d, _) = c
            .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(d, ToplevelDecision::Granted);
        assert_eq!(c.fullscreen()[0].output, b, "the output the client named");
        assert_eq!(
            c.toplevel_configure_state(w).size,
            Some(LocalSize { w: 1280, h: 720 })
        );
        assert_eq!(
            c.membership().observers(w),
            vec![a, b],
            "A's camera still shows it"
        );
        let fx = c
            .output_reconfigured(
                &mut t,
                b,
                OutputDescriptor {
                    key: crate::output::tests::key("B"),
                    mode: (2560, 1440),
                    transform: Transform::Normal,
                    scale_120: 240,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        assert_eq!(
            fx.iter()
                .filter(|e| **e == Effect::ToplevelConfigure(w))
                .count(),
            1,
            "one configure for the element in fullscreen on it: {fx:?}"
        );
        assert_eq!(
            c.toplevel_configure_state(w).size,
            Some(LocalSize { w: 1280, h: 720 })
        );
        let fx = c.output_disconnected(&mut t, b).unwrap();
        let restore = fx
            .iter()
            .position(|e| {
                *e == Effect::ToplevelConfigureSize {
                    element: w,
                    size: LocalSize { w: 400, h: 300 },
                }
            })
            .expect("the configure with the saved size");
        let removed = fx
            .iter()
            .position(|e| *e == Effect::OutputRemoved(b))
            .unwrap();
        assert!(restore < removed);
        assert!(c.fullscreen().is_empty());
        assert_eq!(
            c.toplevel_configure_state(w),
            ToplevelConfigureState::default()
        );
    }

    /// C5.2 output choice: the named output if it exists; else the frame-clock output of an
    /// observed element; else the output under the pointer.
    #[test]
    fn the_fullscreen_output_is_chosen_by_the_sealed_rule() {
        let far = PersistedCameras::from_entries(vec![(
            crate::output::tests::key("B"),
            CanvasPos {
                x: 50_000 * Q16_ONE,
                y: 0,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, mut i, _, mut t) = Core::new(far);
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1280, 720), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        assert_eq!(c.membership().observers(w), vec![a], "shown on A only");
        let on = |c: &mut Core, output| {
            c.toplevel_request(w, ToplevelRequest::SetFullscreen { output })
                .unwrap();
            let o = c.fullscreen()[0].output;
            c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                .unwrap();
            o
        };
        assert_eq!(on(&mut c, Some(b)), b, "named");
        assert_eq!(on(&mut c, None), a, "its clock output");
        let gone = {
            let x = out(&mut c, &mut t, "X", (800, 600), 120);
            c.output_disconnected(&mut t, x).unwrap();
            x
        };
        assert_eq!(
            on(&mut c, Some(gone)),
            a,
            "a named output that no longer exists"
        );
        // presented on B while A's camera still shows it at its real size and place
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.presentation(b), Some(w));
        assert_eq!(c.presentation(a), None);
        assert_eq!(rects(&mut c, a), vec![(w, (760, 390, 1160, 690))]);
        // centred on B: (1280−400)/2 = 440, (720−300)/2 = 210
        assert_eq!(rects(&mut c, b), vec![(w, (440, 210, 840, 510))]);
        // its popups are constrained to the fullscreen output B, whose origin is (440, 210) up
        // and left of the window — not to A's camera view, which would be (−760, −390) 1920x1080
        assert_eq!(
            c.popup_constraint_target(w, Some(a)),
            Some(LocalRect {
                loc: LocalPoint { x: -440, y: -210 },
                size: LocalSize { w: 1280, h: 720 },
            })
        );
        assert_eq!(
            c.cameras().center(1),
            Some(CanvasPos {
                x: 50_000 * Q16_ONE,
                y: 0
            })
        );
    }

    /// F-PRESENT-1 on fractional outputs, by hand. The anchor of a 400x300 window centred on a
    /// camera at (0,0) is (−200,−150).
    ///
    /// - 1920x1080 at scale 1.5: logical 1280x720, the configure size. Smaller window: margins
    ///   (440, 210), v_P = (−640,−360), drawn at physical [660,1260)x[315,765). A 2000x1000
    ///   window: wider than the output, so x is origin-aligned and cropped (v_P.x = anchor.x);
    ///   taller too (1000 > 720): v_P.y = anchor.y.
    /// - 1366x768 at scale 1.25: logical 1092.8x614.4; the configure size rounds down to
    ///   1092x614, and a window of that size has margins (0.4, 0.2).
    #[test]
    fn the_presentation_view_is_f_present_1() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 180);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(
            c.toplevel_configure_state(w).size,
            Some(LocalSize { w: 1280, h: 720 })
        );
        let v = c.view(a).unwrap();
        assert_eq!((v.v.x, v.v.y, v.k), (-640 * Q16_ONE, -360 * Q16_ONE, 1.5));
        assert_eq!(rects(&mut c, a), vec![(w, (660, 315, 1260, 765))]);
        c.toplevel_committed(w, win(1, 2000, 1000, true)).unwrap();
        let v = c.view(a).unwrap();
        assert_eq!(
            (v.v.x, v.v.y),
            (-200 * Q16_ONE, -150 * Q16_ONE),
            "origin-aligned"
        );
        assert_eq!(
            rects(&mut c, a),
            vec![(w, (0, 0, 3000, 1500))],
            "cropped by the output"
        );

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1366, 768), 150);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(
            c.toplevel_configure_state(w).size,
            Some(LocalSize { w: 1092, h: 614 })
        );
        c.toplevel_committed(w, win(1, 1092, 614, true)).unwrap();
        let v = c.view(a).unwrap();
        // v_P = (−200 − 0.4, −150 − 0.2), Q16 by R7: 0.4·65536 = 26214.4 → 26214, 0.2·65536 = 13107.2 → 13107
        assert_eq!(
            (v.v.x, v.v.y),
            (-200 * Q16_ONE - 26214, -150 * Q16_ONE - 13107)
        );
    }

    /// A-8: keyboard focus does not move to an element a presentation hides (an output presents
    /// another element and its camera would show this one, and no output observes it): neither
    /// by `set_focus`, nor FocusNext, nor the initial grant, nor as the successor of a lost
    /// target. The same element is focusable while another output shows it.
    #[test]
    fn focus_does_not_move_to_an_element_a_presentation_hides() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        let o = map_win(&mut c, 2, 200, 100);
        let v = map_win(&mut c, 3, 100, 100);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(a), Some(w));
        assert!(c.hidden_by_presentation(o) && c.hidden_by_presentation(v));
        assert!(
            !c.hidden_by_presentation(w),
            "the presenter is never hidden"
        );

        // set_focus is declined; Super+Tab cycles among what may be focused (W alone)
        assert_eq!(c.set_focus(&mut i, Some(o)).unwrap(), vec![]);
        assert_eq!(c.focus().target(), Some(w));
        use crate::input::keys::{KEY_LEFTMETA, KEY_TAB};
        for (key, pressed, logo) in [
            (KEY_LEFTMETA, true, true),
            (KEY_TAB, true, true),
            (KEY_TAB, false, true),
            (KEY_LEFTMETA, false, false),
        ] {
            let modifiers = crate::input::bindings::Modifiers {
                logo,
                ..Default::default()
            };
            c.physical_input_vec(
                &mut i,
                crate::PhysicalEvent::KeyboardKey {
                    key,
                    pressed,
                    modifiers,
                    mods_changed: key == KEY_LEFTMETA,
                    time_us: 1000,
                },
            )
            .unwrap();
        }
        assert_eq!(
            c.focus().target(),
            Some(w),
            "FocusNext moved to a hidden element"
        );

        // a second output whose camera shows O and V: they are not hidden there
        let b = out(&mut c, &mut t, "B", (1920, 1080), 120);
        assert!(!c.membership().observers(o).is_empty());
        assert!(!c.hidden_by_presentation(o));
        focus(&mut c, &mut i, o);
        focus(&mut c, &mut i, v);
        assert_eq!(c.focus().target(), Some(v));
        // B goes: O and V are hidden again; V keeps the focus it had (nothing moved it)
        c.output_disconnected(&mut t, b).unwrap();
        assert!(c.hidden_by_presentation(o) && c.hidden_by_presentation(v));
        // V goes: the successor skips O (more recent, hidden) for W
        c.toplevel_destroyed(v);
        assert_eq!(
            c.focus().target(),
            Some(w),
            "the successor was a hidden element"
        );

        // the presentation ends: O is focusable again
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert!(!c.hidden_by_presentation(o));
        focus(&mut c, &mut i, o);
        assert_eq!(c.focus().target(), Some(o));

        // the initial grant (nothing focused yet) skips a hidden element
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let o = map_win(&mut c, 2, 200, 100);
        let x = c.toplevel_created();
        c.toplevel_request(x, FS).unwrap();
        c.toplevel_committed(x, win(4, 0, 0, false)).unwrap();
        c.toplevel_committed(x, win(4, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(a), Some(x));
        assert_eq!(c.grant_initial_focus(&mut i, o).unwrap(), vec![]);
        assert_eq!(c.focus().target(), None);
        assert!(c.focus().is_initial_eligible());
        c.grant_initial_focus(&mut i, x).unwrap();
        assert_eq!(c.focus().target(), Some(x));
    }

    /// A-3: a grant made before an element's first map takes the output only when the element
    /// maps. Until then the presented element keeps it and is told nothing; a newcomer that is
    /// destroyed or unmapped first, or never draws, ends nothing. Two waiting grants take the
    /// output in the order their elements map. A grant to a mapped, focused element still
    /// replaces every older grant on the output, a waiting one included. Output removal ends
    /// every grant on the output.
    #[test]
    fn a_pre_map_grant_takes_the_output_when_its_element_maps() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let o = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(o), Some(w));
        let told = |fx: &[Effect], e: ElementId| {
            fx.iter().any(|f| {
                matches!(f, Effect::ToplevelConfigure(x) | Effect::ToplevelConfigureSize { element: x, .. } if *x == e)
            })
        };
        // a newcomer asks before its initial commit: granted, waiting, W untouched
        let premap = |c: &mut Core, k: u64| {
            let x = c.toplevel_created();
            assert_eq!(
                c.toplevel_request(x, FS).unwrap().0,
                ToplevelDecision::Recorded
            );
            let fx = c.toplevel_committed(x, win(k, 0, 0, false)).unwrap();
            (x, fx)
        };
        let (x, fx) = premap(&mut c, 2);
        assert!(
            !told(&fx, w),
            "W was told to leave at another element's initial commit"
        );
        assert_eq!(c.presentation(o), Some(w));
        assert_eq!(c.fullscreen_of(x).map(|f| f.output), Some(o));
        // destroyed before drawing: nothing ends
        let fx = c.toplevel_destroyed(x);
        assert!(!told(&fx, w));
        assert_eq!(c.presentation(o), Some(w));
        assert_eq!(c.fullscreen_of(w).map(|f| f.output), Some(o));

        // two waiting newcomers, the older maps first: it takes the output and the newer keeps
        // waiting; when the newer maps, it replaces the older
        let (x, _) = premap(&mut c, 3);
        let (y, _) = premap(&mut c, 4);
        assert_eq!(c.presentation(o), Some(w));
        let fx = c.toplevel_committed(x, win(3, 1920, 1080, true)).unwrap();
        assert!(
            told(&fx, w),
            "W keeps the output after a newer grant's element mapped"
        );
        assert!(
            !told(&fx, y),
            "a newer waiting grant was ended by an older one's map"
        );
        assert_eq!(c.presentation(o), Some(x));
        assert_eq!(c.fullscreen_of(w), None);
        let fx = c.toplevel_committed(y, win(4, 1920, 1080, true)).unwrap();
        assert!(told(&fx, x));
        assert_eq!(c.presentation(o), Some(y));
        // the newer maps first: it replaces the presenter and the older waiting grant
        let (p, _) = premap(&mut c, 8);
        let (q, _) = premap(&mut c, 9);
        let fx = c.toplevel_committed(q, win(9, 1920, 1080, true)).unwrap();
        assert!(
            told(&fx, y) && told(&fx, p),
            "older grants survived a newer one's map"
        );
        assert_eq!(c.fullscreen_of(p), None);
        c.toplevel_committed(p, win(8, 640, 480, true)).unwrap();
        assert_eq!(
            c.presentation(o),
            Some(q),
            "an ended waiting grant took the output"
        );
        let x = q;

        // the presenter unmaps while a grant waits: no presentation until the waiting one maps
        let (z, _) = premap(&mut c, 5);
        c.toplevel_committed(x, win(3, 1920, 1080, false)).unwrap();
        assert_eq!(c.presentation(o), None);
        c.toplevel_committed(z, win(5, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(o), Some(z));

        // a focused, mapped element's grant replaces the presenter and a waiting grant
        let (v, _) = premap(&mut c, 6);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        let fx = c.toplevel_request(w, FS).unwrap().1;
        assert!(
            told(&fx, z) && told(&fx, v),
            "older grants on the output survived"
        );
        assert_eq!(c.fullscreen_of(v), None);
        c.toplevel_committed(v, win(6, 640, 480, true)).unwrap();
        assert_eq!(
            c.presentation(o),
            Some(w),
            "a replaced waiting grant took the output"
        );

        // output removal ends a waiting grant too
        let (u, _) = premap(&mut c, 7);
        assert!(c.fullscreen_of(u).is_some());
        c.output_disconnected(&mut t, o).unwrap();
        assert_eq!(c.fullscreen_of(u), None);
        assert_eq!(c.fullscreen_of(w), None);
    }

    /// M9 "configure with the saved size": the size offered is the last *windowed* size. A grant
    /// made while the element still has its fullscreen geometry (it was told to leave and has not
    /// redrawn) saves the size on offer again; once the element redraws at another size the offer
    /// is spent and the geometry is what is saved. Unmap and destruction forget the offer.
    #[test]
    fn the_saved_size_is_the_last_windowed_size() {
        const UNFS: ToplevelRequest = ToplevelRequest::UnsetFullscreen;
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        let offer = |size: (i32, i32)| Effect::ToplevelConfigureSize {
            element: w,
            size: LocalSize {
                w: size.0,
                h: size.1,
            },
        };
        let saved = |c: &Core| c.fullscreen_of(w).and_then(|f| f.saved).map(|s| (s.w, s.h));

        c.toplevel_request(w, FS).unwrap();
        assert_eq!(saved(&c), Some((400, 300)));
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.toplevel_request(w, UNFS).unwrap().1[0], offer((400, 300)));
        // again before the element redrew: its geometry is still 1920x1080
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(
            saved(&c),
            Some((400, 300)),
            "the fullscreen geometry was saved"
        );
        assert_eq!(c.toplevel_request(w, UNFS).unwrap().1[0], offer((400, 300)));

        // the element redraws at a size of its own choosing: the offer is spent
        c.toplevel_committed(w, win(1, 500, 350, true)).unwrap();
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(saved(&c), Some((500, 350)));
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.toplevel_request(w, UNFS).unwrap().1[0], offer((500, 350)));

        // an unmap forgets the offer: after the re-show the geometry is the only size known
        c.toplevel_committed(w, win(1, 1920, 1080, false)).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, false)).unwrap();
        c.toplevel_committed(w, win(1, 640, 480, true)).unwrap();
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(saved(&c), Some((640, 480)));
    }

    /// "Fullscreen never touches cameras": a camera drag on a fullscreen output moves that
    /// output's camera and not what the output shows; when fullscreen ends, the output shows the
    /// camera where the drag left it.
    #[test]
    fn a_presentation_is_independent_of_the_camera() {
        use crate::PhysicalEvent;
        use crate::input::bindings::Modifiers;
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        let presented = c.view(a).unwrap();
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: logo,
            time_us: 0,
        };
        c.physical_input_vec(&mut i, button(true)).unwrap();
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (-300.0, 0.0),
                delta_unaccel: (-300.0, 0.0),
                time_us: 0,
            },
        )
        .unwrap();
        c.physical_input_vec(&mut i, button(false)).unwrap();
        assert_eq!(
            c.cameras().center(0),
            Some(CanvasPos {
                x: 300 * Q16_ONE,
                y: 0
            }),
            "the camera moved"
        );
        assert_eq!(c.view(a), Some(presented), "what the output shows did not");
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: -200, y: -150 }));
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        let v = c.view(a).unwrap();
        assert_eq!((v.only, v.v.x), (None, (300 - 960) * Q16_ONE));
    }

    #[test]
    #[ignore = "AUDIT_PRESENTATION: unfocused premap fullscreen client steals output, ends active fullscreen, and clears user suppression"]
    fn audit_presentation_premap_fullscreen_steals_output_and_clears_suppression() {
        let (mut c, mut i, mut s, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w1 = map_win(&mut c, 1, 400, 300);
        let w2 = map_win(&mut c, 2, 400, 300);
        focus(&mut c, &mut i, w1);
        c.toplevel_request(w1, FS).unwrap();
        assert_eq!(c.presentation(a), Some(w1));

        // User chooses w2 over w1's fullscreen (suppressing A).
        c.shell_command(
            &mut s,
            crate::ShellCommand::Focus {
                element: crate::stage::ElementRef {
                    instance: c.stage().instance(),
                    element: w2,
                },
            },
        )
        .unwrap();
        assert!(c.is_presentation_suppressed(a));
        assert_eq!(c.focus().target(), Some(w2));

        // Background client opens w3 with premap SetFullscreen (AwaitingInitialCommit) without focus.
        let w3 = c.toplevel_created();
        let _ = c.toplevel_request(w3, FS);
        c.toplevel_committed(w3, win(3, 0, 0, false)).unwrap();
        c.toplevel_committed(w3, win(3, 1920, 1080, true)).unwrap();

        assert_eq!(c.focus().target(), Some(w2), "user focus stayed on w2");
        assert!(
            c.presentation(a) != Some(w3),
            "unfocused premap client must not steal fullscreen output over the user's chosen w2"
        );
        assert!(
            c.is_presentation_suppressed(a),
            "unfocused client commit must not clear the user's presentation suppression"
        );
    }

    #[test]
    fn audit_presentation_suppression_leaks_when_fullscreen_ends_without_take_output() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 120);
        let w1 = map_win(&mut c, 1, 400, 300);
        let w2 = map_win(&mut c, 2, 400, 300);

        // 1. UnsetFullscreen leaves presentation_suppressed set on A
        focus(&mut c, &mut i, w1);
        c.toplevel_request(w1, ToplevelRequest::SetFullscreen { output: Some(a) })
            .unwrap();
        let mut fx = Vec::new();
        c.suppress_presentation(a, &mut fx);
        assert!(c.is_presentation_suppressed(a));
        c.toplevel_request(w1, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(c.presentation(a), None);
        let mut leaked = Vec::new();
        if c.is_presentation_suppressed(a) {
            leaked.push("UnsetFullscreen");
        }

        // 2. Unmap leaves presentation_suppressed set on A
        c.clear_presentation_suppression(a);
        focus(&mut c, &mut i, w1);
        c.toplevel_request(w1, ToplevelRequest::SetFullscreen { output: Some(a) })
            .unwrap();
        c.suppress_presentation(a, &mut fx);
        c.toplevel_committed(w1, win(1, 0, 0, false)).unwrap();
        assert_eq!(c.presentation(a), None);
        if c.is_presentation_suppressed(a) {
            leaked.push("unmap");
        }

        // 3. Move fullscreen A -> B leaves presentation_suppressed set on A
        c.clear_presentation_suppression(a);
        focus(&mut c, &mut i, w2);
        c.toplevel_request(w2, ToplevelRequest::SetFullscreen { output: Some(a) })
            .unwrap();
        c.suppress_presentation(a, &mut fx);
        c.toplevel_request(w2, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.presentation(a), None);
        if c.is_presentation_suppressed(a) {
            leaked.push("move A->B");
        }

        // 4. Destroy leaves presentation_suppressed set on B
        c.suppress_presentation(b, &mut fx);
        c.toplevel_destroyed(w2);
        assert_eq!(c.presentation(b), None);
        if c.is_presentation_suppressed(b) {
            leaked.push("toplevel_destroyed");
        }

        assert!(
            leaked.is_empty(),
            "presentation_suppressed leaked after fullscreen ended via: {leaked:?}"
        );
    }

    #[test]
    #[ignore = "AUDIT_PRESENTATION: client map/remap raises buried/unfocused window over focused window without user action"]
    fn audit_presentation_remap_raises_unfocused_window_over_focused_window() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w1 = map_win(&mut c, 1, 400, 300);
        let w2 = map_win(&mut c, 2, 400, 300);
        // Focus stays on w2 while w1 unmaps and remaps behind it without user action.
        focus(&mut c, &mut i, w2);
        assert_eq!(c.stage().z_order(), vec![w1, w2]);

        c.toplevel_committed(w1, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w1, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w1, win(1, 400, 300, true)).unwrap();

        assert_eq!(c.focus().target(), Some(w2));
        assert_eq!(
            c.stage().z_order().last().copied(),
            Some(w2),
            "unfocused w1 remapping without user action raised itself over focused w2"
        );
    }
}
