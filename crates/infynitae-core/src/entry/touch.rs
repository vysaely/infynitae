//! Touchscreen input: finger down and motion, canvas and window gestures, edge swipes.

use super::*;

impl Core {
    /// A finger's first contact (`touch`): hit-tested like a click (INV-INPUT-8: a tap on a
    /// window raises and focuses it, a tap on an on-demand layer surface gives it the keyboard),
    /// and the surface keeps the finger until it lifts. On empty canvas the finger joins the
    /// canvas gesture (`touch::CanvasGesture`). Nothing while a compositor interaction owns the
    /// input, or on an unknown output.
    pub(super) fn touch_down(
        &mut self,
        slot: u32,
        output: OutputId,
        p: (f64, f64),
        time_ms: u32,
        out: &mut Vec<Effect>,
    ) {
        // a slot still held (its lift was never seen: dropped while the session was locked) is
        // a new contact now
        self.touches.remove(slot);
        if self.output(output).is_none() || self.input.interaction.active() {
            return;
        }
        // a second finger moves the canvas, also on a window: it joins fingers on the canvas,
        // or takes the ones on windows from their app (cancelled there); not on a fullscreen
        // window (a game's own touch), never from a layer surface (the on-screen keyboard)
        let presenting =
            self.presentation(output).is_some() && !self.is_presentation_suppressed(output);
        if !presenting && let Some(camera) = self.cameras.camera_for_output(output) {
            if self.touches.window_down(slot, output, p) {
                return;
            }
            if self
                .touches
                .canvas
                .as_ref()
                .is_some_and(|g| g.output == output)
            {
                self.touches.canvas_down(slot, output, camera, p, time_ms);
                self.touch_window_take(output, out);
                return;
            }
            if let Some(fingers) = self.touches.take_window_fingers(output) {
                out.push(Effect::TouchCancel);
                for (s, at) in fingers {
                    self.touches.canvas_down(s, output, camera, at, time_ms);
                }
                self.touches.canvas_down(slot, output, camera, p, time_ms);
                return;
            }
        }
        let pt = crate::geom::OutputLogicalPoint { x: p.0, y: p.1 };
        let on_layer = self.layer_hit(output, p, true);
        let element = if on_layer.is_none() {
            self.hit_test(output, pt)
        } else {
            None
        };
        let on_layer = on_layer.or_else(|| {
            element
                .is_none()
                .then(|| self.layer_hit(output, p, false))
                .flatten()
        });
        let q = crate::geom::Q16_ONE;
        let (target, location, anchor) = if let Some(hit) = element {
            self.raise_by_user(hit.element);
            out.extend(
                self.focus
                    .set_focus_at(self.input.clock.now_us(), Some(hit.element)),
            );
            (
                crate::protocol::PointerTarget {
                    surface: hit.surface,
                    origin: hit.origin,
                },
                crate::geom::CanvasPos {
                    x: hit.origin.x * q + hit.local.x,
                    y: hit.origin.y * q + hit.local.y,
                },
                crate::touch::Anchor::Canvas,
            )
        } else if let Some(h) = on_layer {
            if self
                .layer(h.layer)
                .is_some_and(|r| r.state.keyboard == crate::layer::KeyboardInteractivity::OnDemand)
            {
                out.extend(self.focus.layer_take(h.layer, false));
            }
            let local = (h.local.x as f64 / q as f64, h.local.y as f64 / q as f64);
            (
                crate::protocol::PointerTarget {
                    surface: h.surface,
                    origin: crate::geom::CanvasPoint { x: 0, y: 0 },
                },
                crate::geom::CanvasPos {
                    x: h.local.x,
                    y: h.local.y,
                },
                crate::touch::Anchor::Layer {
                    offset: (p.0 - local.0, p.1 - local.1),
                },
            )
        } else {
            if let Some(camera) = self.cameras.camera_for_output(output) {
                self.touches.canvas_down(slot, output, camera, p, time_ms);
            }
            return;
        };
        self.touches.insert(
            slot,
            crate::touch::TouchPoint {
                output,
                target,
                anchor,
                at: p,
            },
        );
        out.push(Effect::TouchDown {
            slot,
            target,
            location,
            time_ms,
        });
    }

    /// A finger's motion, to the surface of its first contact (`touch`), or the canvas gesture's.
    pub(super) fn touch_motion(
        &mut self,
        proof: Option<authority::Authorized>,
        slot: u32,
        p: (f64, f64),
        time_ms: u32,
        out: &mut Vec<Effect>,
    ) {
        if let Some(proof) = proof {
            self.canvas_gesture_step(proof, slot, p, out);
            return;
        }
        self.touches.surface_finger_at(slot, p);
        let Some(tp) = self.touches.get(slot) else {
            return;
        };
        let location = match tp.anchor {
            crate::touch::Anchor::Canvas => {
                let Some(view) = self.current_frame(tp.output).map(|f| f.view) else {
                    return;
                };
                view.unproject(crate::geom::OutputLogicalPoint { x: p.0, y: p.1 })
            }
            crate::touch::Anchor::Layer { offset } => crate::geom::CanvasPos {
                x: crate::geom::q16(p.0 - offset.0),
                y: crate::geom::q16(p.1 - offset.1),
            },
        };
        out.push(Effect::TouchMotion {
            slot,
            location,
            time_ms,
        });
    }

    /// A canvas finger moved to `p`: the grab is taken from the camera and the fingers' places
    /// before it if they changed since, then the camera takes the zoom and centre that keep it
    /// under them. Nothing while a fullscreen window is presented there or a compositor
    /// interaction owns the input.
    fn canvas_gesture_step(
        &mut self,
        proof: authority::Authorized,
        slot: u32,
        p: (f64, f64),
        out: &mut Vec<Effect>,
    ) {
        let Some(g) = &self.touches.canvas else {
            return;
        };
        let (output, camera) = (g.output, g.camera);
        let blocked = self.input.interaction.active()
            || (self.presentation(output).is_some() && !self.is_presentation_suppressed(output));
        let camera_now = (
            self.cameras.zoom(camera),
            self.cameras.center(camera),
            self.output(output).map(|o| o.logical_extent()),
        );
        let Some(g) = &mut self.touches.canvas else {
            return;
        };
        let (false, (Some(zoom), Some(center), Some(extent))) = (blocked, camera_now) else {
            // the finger is followed still; the grab is taken again once the camera can move
            g.follow(slot, p);
            g.grab = None;
            return;
        };
        let q = crate::geom::Q16_ONE as f64;
        let grab = *g.grab.get_or_insert_with(|| {
            crate::touch::Grab::take(
                &g.fingers,
                (center.x as f64 / q, center.y as f64 / q),
                zoom,
                extent,
            )
        });
        g.follow(slot, p);
        let (zoom, (cx, cy)) = grab.camera(&g.fingers, extent);
        let limit = crate::geom::CANVAS_LIMIT_PX * crate::geom::Q16_ONE;
        let center = crate::geom::CanvasPos {
            x: crate::geom::q16(cx).clamp(-limit, limit),
            y: crate::geom::q16(cy).clamp(-limit, limit),
        };
        self.cameras
            .set_zoom_and_center(proof, camera, zoom, center);
        self.camera_written(output, camera, out);
    }

    /// Three or more canvas fingers whose centroid is on a window take it (raised and focused):
    /// from now on they move or resize it (`touch::WindowGesture`).
    fn touch_window_take(&mut self, output: OutputId, out: &mut Vec<Effect>) {
        let Some(g) = self
            .touches
            .canvas
            .as_ref()
            .filter(|g| g.fingers.len() >= 3)
        else {
            return;
        };
        let ((x, y), _) = crate::touch::WindowGesture::centroid_of(&g.fingers);
        let pt = crate::geom::OutputLogicalPoint { x, y };
        let Some(hit) = self.hit_test(output, pt) else {
            return;
        };
        self.touches.canvas_to_window(hit.element);
        self.raise_by_user(hit.element);
        out.extend(
            self.focus
                .set_focus_at(self.input.clock.now_us(), Some(hit.element)),
        );
    }

    /// A finger of the window gesture moved to `p`: once the fingers moved or spread
    /// `WINDOW_SLOP`, the window follows their centroid (a move) or their extent (a resize, its
    /// top-left corner staying). Nothing while a compositor interaction owns the input; the
    /// gesture ends if its window went.
    pub(super) fn window_gesture_step(&mut self, slot: u32, p: (f64, f64), out: &mut Vec<Effect>) {
        use crate::touch::WindowMode;
        let Some(g) = self.touches.window.as_mut() else {
            return;
        };
        let (output, element, mode) = (g.output, g.element, g.mode);
        if self.input.interaction.active() {
            // the finger is followed still; the grab is taken again once the window can move
            g.follow(slot, p);
            g.grab = None;
            return;
        }
        let (Some(anchor), Some(tree), Some(view)) = (
            self.stage.anchor(element),
            self.stage.tree(element),
            self.view(output),
        ) else {
            self.touches.end_window();
            return;
        };
        let size = self
            .resize
            .filter(|r| r.element == element && !r.released)
            .map_or((tree.geometry.size.w, tree.geometry.size.h), |r| r.target);
        let Some(g) = self.touches.window.as_mut() else {
            return;
        };
        // the grab is of the fingers before this motion, so none of it is lost
        if g.grab.is_none() {
            let (centroid, extent) = g.shape();
            g.grab = Some(crate::touch::WindowGrab {
                centroid,
                extent,
                anchor,
                size,
            });
        }
        g.follow(slot, p);
        let grab = g.grab.expect("just taken");
        let (centroid, extent) = g.shape();
        let moved = (centroid.0 - grab.centroid.0).hypot(centroid.1 - grab.centroid.1);
        let spread = (extent.0 - grab.extent.0)
            .abs()
            .max((extent.1 - grab.extent.1).abs());
        let mode = match mode {
            WindowMode::Undecided if moved > crate::touch::WINDOW_SLOP && moved >= spread => {
                WindowMode::Move
            }
            WindowMode::Undecided if spread > crate::touch::WINDOW_SLOP => {
                if self.begin_resize(element, false, false).is_none() {
                    return;
                }
                WindowMode::Resize
            }
            m => m,
        };
        if let Some(g) = self.touches.window.as_mut() {
            g.mode = mode;
        }
        match mode {
            WindowMode::Undecided => {}
            WindowMode::Move => {
                let at =
                    |(x, y): (f64, f64)| view.unproject(crate::geom::OutputLogicalPoint { x, y });
                let (now, then) = (at(centroid), at(grab.centroid));
                let q = crate::geom::Q16_ONE;
                let target = crate::geom::CanvasPos {
                    x: grab.anchor.x * q + (now.x - then.x),
                    y: grab.anchor.y * q + (now.y - then.y),
                };
                if let Some(to) = target.round_to_point()
                    && to != anchor
                {
                    let _ = self.place_by_user(element, to, out);
                }
            }
            WindowMode::Resize => {
                let zoom = view.zoom.max(0.001);
                let grow = |start: i32, d: f64| {
                    (f64::from(start) + d / zoom)
                        .round()
                        .clamp(32.0, f64::from(i32::MAX)) as i32
                };
                self.resize_to(
                    (
                        grow(grab.size.0, extent.0 - grab.extent.0),
                        grow(grab.size.1, extent.1 - grab.extent.1),
                    ),
                    out,
                );
            }
        }
        self.invalidate_frame(output);
    }

    /// A window gesture that was resizing ended: the last configure, without `resizing`.
    pub(crate) fn touch_resize_settle(&mut self, out: &mut Vec<Effect>) {
        if self.touches.take_resize_done() && self.resize.is_some_and(|r| !r.released) {
            self.end_resize(out);
        }
    }

    /// Cancels any active touches in Core (when the touchscreen disconnects or the session locks).
    pub fn clear_touches(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        if self.touches.clear() {
            out.push(Effect::TouchCancel);
        }
        self.touch_resize_settle(&mut out);
        out
    }

    /// Arms a screen-edge swipe candidate when the first finger lands near an output edge
    /// (unless it landed inside an open non-strip overlay or panel).
    pub(super) fn arm_edge_swipe(&mut self, slot: u32, output: output::OutputId, p: (f64, f64)) {
        if !self.touch_edge_swipe || self.locked() || self.input.interaction.active() {
            self.touches.edge_swipe = None;
            return;
        }
        let no_other_fingers = !self.touches.any_point(|_| true)
            && self.touches.canvas.is_none()
            && self.touches.window.is_none();
        if !no_other_fingers {
            self.touches.edge_swipe = None;
            return;
        }
        let Some(o) = self.output(output) else {
            return;
        };
        let extent = o.logical_extent();
        let Some(candidate) = crate::touch::EdgeSwipeCandidate::from_edge(slot, output, p, extent)
        else {
            return;
        };
        // An above-canvas layer thicker than a 2px edge hover strip is an open panel or OSK:
        // leave its touches alone even when the finger lands within 2px of the screen edge.
        let on_open_overlay = self
            .layer_hit(output, p, true)
            .and_then(|h| self.layer(h.layer))
            .is_some_and(|r| r.geometry.size.w > 8 && r.geometry.size.h > 8);
        if !on_open_overlay {
            self.touches.edge_swipe = Some(candidate);
        }
    }

    /// Advances an armed edge-swipe candidate; returns `true` when it opens an edge panel.
    pub(super) fn step_edge_swipe(
        &mut self,
        slot: u32,
        p: (f64, f64),
        out: &mut Vec<Effect>,
    ) -> bool {
        let Some(cand) = self.touches.edge_swipe else {
            return false;
        };
        if cand.slot != slot {
            return false;
        }
        match cand.step(p) {
            crate::touch::EdgeSwipeStep::Pending => false,
            crate::touch::EdgeSwipeStep::Cancel => {
                self.touches.edge_swipe = None;
                false
            }
            crate::touch::EdgeSwipeStep::Trigger(edge_pt) => {
                self.touches.edge_swipe = None;
                // Only hijack into pointer-at-edge if an above-canvas strip exists on that edge.
                if self.layer_hit(cand.output, edge_pt, true).is_none() {
                    return false;
                }
                if self.touches.clear() {
                    out.push(Effect::TouchCancel);
                }
                self.touch_resize_settle(out);
                self.pointer = pointer::PointerState::Located {
                    output: cand.output,
                    p: edge_pt,
                };
                self.true_position_into(out);
                true
            }
        }
    }
}
