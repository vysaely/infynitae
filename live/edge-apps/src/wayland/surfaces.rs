//! The layer surfaces: one per output for the panel and the widgets, and the notification strip's popup layer, with their shm buffers and scale.

use super::*;

pub(super) struct BufferSlot {
    pub(super) buffer: Buffer,
    pub(super) prev_box: Option<(i32, i32, i32, i32)>,
}

pub(super) fn scaled_dimension(logical: u32, scale_120: u32) -> u32 {
    ((u64::from(logical) * u64::from(scale_120.max(1)) + 60) / 120).max(1) as u32
}

pub(super) struct OutputSurface {
    pub(super) id: OutputId,
    pub(super) output: wl_output::WlOutput,
    pub(super) frac_scale: Option<WpFractionalScaleV1>,
    pub(super) viewport: Option<WpViewport>,
    pub(super) layer: LayerSurface,
    pub(super) pool: SlotPool,
    pub(super) slots: [Option<BufferSlot>; 2],
    pub(super) last_slot: usize,
    /// the box last committed (in either buffer, in buffer px): the next frame damages it too
    pub(super) committed_box: Option<(i32, i32, i32, i32)>,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) integer_scale: i32,
    pub(super) fractional_scale_120: Option<u32>,
    pub(super) last_buffer_scale: i32,
    pub(super) last_viewport_dest: Option<(i32, i32)>,
    pub(super) buf_size: (u32, u32),
    pub(super) configured: bool,
    pub(super) mapped_closed: bool,
    pub(super) clear_pending: bool,
    /// a fullscreen window is on this screen: closed, the surface goes below the windows
    /// (`below`), so the window can be shown directly (scanout, tearing)
    pub(super) fullscreen_under: bool,
    pub(super) below: bool,
    pub(super) dirty: bool,
    pub(super) frame_pending: bool,
}

impl Drop for OutputSurface {
    fn drop(&mut self) {
        if let Some(fs) = self.frac_scale.take() {
            fs.destroy();
        }
        if let Some(vp) = self.viewport.take() {
            vp.destroy();
        }
    }
}

impl OutputSurface {
    pub(super) fn effective_scale_120(&self) -> u32 {
        self.fractional_scale_120
            .unwrap_or_else(|| (self.integer_scale.max(1) as u32) * 120)
    }

    pub(super) fn effective_scale(&self) -> f64 {
        if self.viewport.is_some() {
            f64::from(self.effective_scale_120()) / 120.0
        } else {
            f64::from(self.integer_scale.max(1))
        }
    }

    pub(super) fn target_buf_size(&self) -> (u32, u32) {
        if self.width == 0 || self.height == 0 {
            return (0, 0);
        }
        if self.viewport.is_some() {
            let s120 = self.effective_scale_120();
            (
                scaled_dimension(self.width, s120),
                scaled_dimension(self.height, s120),
            )
        } else {
            let s = self.integer_scale.max(1) as u32;
            (
                self.width.saturating_mul(s).max(1),
                self.height.saturating_mul(s).max(1),
            )
        }
    }

    pub(super) fn apply_surface_scale(&mut self) {
        let wl = self.layer.wl_surface();
        if let Some(vp) = &self.viewport {
            if self.last_buffer_scale != 1 {
                wl.set_buffer_scale(1);
                self.last_buffer_scale = 1;
            }
            let dest = (self.width as i32, self.height as i32);
            if self.last_viewport_dest != Some(dest) {
                vp.set_destination(dest.0, dest.1);
                self.last_viewport_dest = Some(dest);
            }
        } else {
            let s = self.integer_scale.max(1);
            if self.last_buffer_scale != s {
                wl.set_buffer_scale(s);
                self.last_buffer_scale = s;
            }
        }
    }

    pub(super) fn set_input_region(&self, compositor: &CompositorState, shell: &Shell) {
        let Ok(region) = Region::new(compositor) else {
            return;
        };
        let (w, h) = (self.width as f64, self.height as f64);
        let show_here = shell.visible_on(self.id);
        if w > 0.0 && h > 0.0 {
            if show_here && shell.ask.ask.on {
                // the question takes the whole screen: a click outside it cancels
                region.add(0, 0, w.ceil() as i32, h.ceil() as i32);
            } else if show_here && shell.open.is_some() {
                let r = open_input_rect(shell, shell.open.unwrap(), w, h);
                region.add(
                    r.0.floor() as i32,
                    r.1.floor() as i32,
                    r.2.ceil() as i32,
                    r.3.ceil() as i32,
                );
            } else {
                for i in 0..shell.panels.len() {
                    if shell.panels[i].panel.hover {
                        let s = shell.hover_strip(i, w, h);
                        region.add(
                            s.0.floor() as i32,
                            s.1.floor() as i32,
                            s.2.ceil() as i32,
                            s.3.ceil() as i32,
                        );
                    }
                }
            }
        }
        self.layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
    }

    pub(super) fn apply_mode(
        &mut self,
        qh: &QueueHandle<App>,
        compositor: &CompositorState,
        shell: &Shell,
    ) {
        let show_here = shell.visible_on(self.id);
        let below = !show_here && self.fullscreen_under;
        if below != self.below {
            // Below the windows the buffer stays attached, so the compositor keeps its copy on
            // the GPU: over a fullscreen window it draws no Bottom layer, and a window that fills
            // the screen is opaque and hides it. Until 2026-11-23 the surface was unmapped (null
            // buffer); each time it mapped again its whole buffer was uploaded again, 7-10 ms in
            // a slow "layers above" turn (session log, 2026-11-22).
            self.layer
                .set_layer(if below { Layer::Bottom } else { Layer::Top });
            self.below = below;
        }
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }
        let kb = if show_here {
            keyboard_interactivity(shell)
        } else {
            KeyboardInteractivity::None
        };
        self.layer.set_keyboard_interactivity(kb);
        self.set_input_region(compositor, shell);
        if show_here {
            self.dirty = true;
            self.mapped_closed = false;
            self.clear_pending = false;
            self.render_if_needed(qh, shell);
        } else {
            self.dirty = false;
            self.frame_pending = false;
            self.clear_and_commit(qh, shell.visible());
        }
    }

    pub(super) fn ensure_pool_size(&mut self) {
        let target = self.target_buf_size();
        if self.buf_size != target {
            self.slots = [None, None];
            self.buf_size = target;
            self.committed_box = None;
            self.mapped_closed = false;
            let need = (target.0 as usize) * (target.1 as usize) * 4 * 2;
            if self.pool.len() < need {
                let _ = self.pool.resize(need);
            }
        }
    }

    pub(super) fn acquire_slot(&mut self) -> Option<usize> {
        self.ensure_pool_size();
        let (w, h) = (self.buf_size.0 as i32, self.buf_size.1 as i32);
        if w <= 0 || h <= 0 {
            return None;
        }
        for i in 0..2 {
            if self.slots[i].is_none() {
                if let Ok((buffer, _)) =
                    self.pool
                        .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
                {
                    self.slots[i] = Some(BufferSlot {
                        buffer,
                        prev_box: None,
                    });
                }
            }
            if let Some(slot) = &self.slots[i] {
                if slot.buffer.canvas(&mut self.pool).is_some() {
                    return Some(i);
                }
            }
        }
        None
    }

    pub(super) fn clear_box(
        canvas: &mut [u8],
        stride: i32,
        w: i32,
        h: i32,
        rect: (i32, i32, i32, i32),
    ) {
        let x0 = rect.0.clamp(0, w);
        let y0 = rect.1.clamp(0, h);
        let x1 = (rect.0 + rect.2).clamp(0, w);
        let y1 = (rect.1 + rect.3).clamp(0, h);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let row_bytes = ((x1 - x0) * 4) as usize;
        for row in y0..y1 {
            let start = (row * stride + x0 * 4) as usize;
            if start + row_bytes <= canvas.len() {
                canvas[start..start + row_bytes].fill(0);
            }
        }
    }

    /// Closed: a clear frame, damaging what this buffer held and what is on screen (the other
    /// buffer's box); with both buffers still held, again on the next frame callback.
    pub(super) fn clear_and_commit(&mut self, qh: &QueueHandle<App>, any_visible: bool) {
        if self.mapped_closed && self.buf_size == self.target_buf_size() {
            self.apply_surface_scale();
            self.layer.commit();
            if !any_visible {
                crate::boxes::dump_closed();
            }
            return;
        }
        let Some(idx) = self.acquire_slot() else {
            self.clear_pending = true;
            let wl = self.layer.wl_surface();
            wl.frame(qh, wl.clone());
            self.layer.commit();
            return;
        };
        let (w, h) = (self.buf_size.0 as i32, self.buf_size.1 as i32);
        self.clear_pending = false;
        self.apply_surface_scale();
        let slot = self.slots[idx].as_mut().unwrap();
        let old = slot.prev_box.take();
        if let Some(canvas) = slot.buffer.canvas(&mut self.pool) {
            match old {
                Some(r) => Self::clear_box(canvas, w * 4, w, h, r),
                None => canvas.fill(0),
            }
        }
        let shown = self.committed_box.take();
        let wl = self.layer.wl_surface();
        if old.is_none() && shown.is_none() {
            wl.damage_buffer(0, 0, w, h);
        }
        for (x, y, bw, bh) in [old, shown].into_iter().flatten() {
            wl.damage_buffer(x, y, bw, bh);
        }
        if slot.buffer.attach_to(wl).is_ok() {
            self.mapped_closed = true;
            self.last_slot = idx;
            self.layer.commit();
        }
        if !any_visible {
            crate::boxes::dump_closed();
        }
    }

    pub(super) fn render_if_needed(&mut self, qh: &QueueHandle<App>, shell: &Shell) {
        if !self.configured || !self.dirty || self.frame_pending || !shell.visible_on(self.id) {
            return;
        }
        let (lw, lh) = (self.width as f64, self.height as f64);
        let Some((bx, by, bw, bh)) = shell.shown_box(lw, lh) else {
            return;
        };
        let Some(slot_idx) = self.acquire_slot() else {
            // both buffers still held: commit unchanged and draw on the callback
            let wl = self.layer.wl_surface();
            wl.frame(qh, wl.clone());
            self.layer.commit();
            self.frame_pending = true;
            return;
        };
        let (buf_w, buf_h) = (self.buf_size.0 as i32, self.buf_size.1 as i32);
        let stride = buf_w * 4;
        let (sx, sy) = (f64::from(buf_w) / lw, f64::from(buf_h) / lh);
        let pad = 4.0;
        let lift = if !shell.ask.ask.on && (shell.open_ms.is_finite() || shell.is_closing()) {
            ARRIVE_LIFT.max(crate::shell::LEAVE_DROP).ceil()
        } else {
            0.0
        };
        let x0 = ((bx - pad) * sx).floor().max(0.0) as i32;
        let y0 = ((by - pad) * sy).floor().max(0.0) as i32;
        let x1 = ((bx + bw + pad) * sx).ceil().min(f64::from(buf_w)) as i32;
        let y1 = ((by + bh + pad + lift) * sy).ceil().min(f64::from(buf_h)) as i32;
        let new_box = (x0, y0, (x1 - x0).max(0), (y1 - y0).max(0));
        self.apply_surface_scale();
        let slot = self.slots[slot_idx].as_mut().unwrap();
        let old_box = slot.prev_box.replace(new_box);
        if let Some(canvas) = slot.buffer.canvas(&mut self.pool) {
            match old_box {
                Some(r) => Self::clear_box(canvas, stride, buf_w, buf_h, r),
                None => canvas.fill(0),
            }
            Self::clear_box(canvas, stride, buf_w, buf_h, new_box);
            if let Ok(surface) = unsafe {
                cairo::ImageSurface::create_for_data_unsafe(
                    canvas.as_mut_ptr(),
                    cairo::Format::ARgb32,
                    buf_w,
                    buf_h,
                    stride,
                )
            } {
                if let Ok(cr) = cairo::Context::new(&surface) {
                    cr.scale(sx, sy);
                    shell.draw_on(self.id, &cr, lw, lh);
                }
                surface.flush();
            }
        }
        let wl = self.layer.wl_surface();
        for (dx, dy, dw, dh) in [old_box, self.committed_box.replace(new_box)]
            .into_iter()
            .flatten()
        {
            wl.damage_buffer(dx, dy, dw, dh);
        }
        wl.damage_buffer(new_box.0, new_box.1, new_box.2, new_box.3);
        wl.frame(qh, wl.clone());
        self.frame_pending = true;
        self.dirty = false;
        if slot.buffer.attach_to(wl).is_ok() {
            self.last_slot = slot_idx;
            self.layer.commit();
        }
        crate::boxes::dump_on(shell, self.id, lw, lh);
    }
}

/// The notification pop-ups' surface (in place of mako): a strip down the right of the first
/// screen on the Top layer, taking the pointer only on the cards, and unmapped (no buffer) while
/// there is nothing to show, so it costs nothing and is never one more surface over a full-screen
/// window. The desktop's widgets have one of these too (`widgets`): the whole first screen on the
/// Bottom layer, under the windows, taking the pointer only on the widgets.
pub(super) struct PopupLayer {
    pub(super) widgets: bool,
    pub(super) output: wl_output::WlOutput,
    pub(super) frac_scale: Option<WpFractionalScaleV1>,
    pub(super) viewport: Option<WpViewport>,
    pub(super) layer: LayerSurface,
    pub(super) pool: SlotPool,
    pub(super) buffer: Option<Buffer>,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) integer_scale: i32,
    pub(super) fractional_scale_120: Option<u32>,
    pub(super) last_buffer_scale: i32,
    pub(super) last_viewport_dest: Option<(i32, i32)>,
    pub(super) configured: bool,
    /// a buffer is attached (the surface shows)
    pub(super) mapped: bool,
    pub(super) frame_pending: bool,
    /// draw again on the next frame (a card still sliding in)
    pub(super) again: bool,
    /// where the pointer is on it
    pub(super) pointer: Option<(f64, f64)>,
}

impl Drop for PopupLayer {
    fn drop(&mut self) {
        if let Some(fs) = self.frac_scale.take() {
            fs.destroy();
        }
        if let Some(vp) = self.viewport.take() {
            vp.destroy();
        }
    }
}

/// The strip's width: the cards, their margin from the screen's right, a little room.
pub(super) const POPUP_STRIP_W: u32 =
    (crate::notify::POPUP_W + crate::notify::MARGIN + 16.0) as u32;

impl PopupLayer {
    /// The pop-ups' strip (`widgets` false) or the widgets' surface (true) on `output`.
    pub(super) fn new(app: &App, output: wl_output::WlOutput, widgets: bool) -> Option<PopupLayer> {
        let pool = SlotPool::new(4096, &app.shm).ok()?;
        let integer_scale = app
            .outputs
            .iter()
            .find(|o| o.output == output)
            .map(|o| o.integer_scale)
            .or_else(|| {
                app.output_state
                    .info(&output)
                    .map(|i| i.scale_factor.max(1))
            })
            .unwrap_or(1);
        let fractional_scale_120 = app
            .outputs
            .iter()
            .find(|o| o.output == output)
            .and_then(|o| o.fractional_scale_120);
        let surface = app.compositor.create_surface(&app.qh);
        let (viewport, frac_scale) = match (&app.viewporter, &app.fractional_scale_mgr) {
            (Some(vp), Some(fs)) => (
                Some(vp.get_viewport(&surface, &app.qh, ())),
                Some(fs.get_fractional_scale(&surface, &app.qh, surface.clone())),
            ),
            _ => (None, None),
        };
        // the compositor's layer rules know both names
        let layer = if widgets {
            let l = app.layer_shell.create_layer_surface(
                &app.qh,
                surface,
                Layer::Bottom,
                Some("infynitae-widgets"),
                Some(&output),
            );
            l.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            l
        } else {
            let l = app.layer_shell.create_layer_surface(
                &app.qh,
                surface,
                Layer::Top,
                Some("infynitae-notifications"),
                Some(&output),
            );
            l.set_anchor(Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM);
            l.set_size(POPUP_STRIP_W, 0);
            l
        };
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(region) = Region::new(&app.compositor) {
            layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        layer.commit();
        Some(PopupLayer {
            widgets,
            output,
            frac_scale,
            viewport,
            layer,
            pool,
            buffer: None,
            width: 0,
            height: 0,
            integer_scale,
            fractional_scale_120,
            last_buffer_scale: 1,
            last_viewport_dest: None,
            configured: false,
            mapped: false,
            frame_pending: false,
            again: false,
            pointer: None,
        })
    }

    pub(super) fn target_buf_size(&self) -> (u32, u32) {
        if self.width == 0 || self.height == 0 {
            return (0, 0);
        }
        if self.viewport.is_some() {
            let s120 = self
                .fractional_scale_120
                .unwrap_or_else(|| (self.integer_scale.max(1) as u32) * 120);
            (
                scaled_dimension(self.width, s120),
                scaled_dimension(self.height, s120),
            )
        } else {
            let s = self.integer_scale.max(1) as u32;
            (
                self.width.saturating_mul(s).max(1),
                self.height.saturating_mul(s).max(1),
            )
        }
    }

    pub(super) fn apply_surface_scale(&mut self) {
        let wl = self.layer.wl_surface();
        if let Some(vp) = &self.viewport {
            if self.last_buffer_scale != 1 {
                wl.set_buffer_scale(1);
                self.last_buffer_scale = 1;
            }
            let dest = (self.width as i32, self.height as i32);
            if self.last_viewport_dest != Some(dest) {
                vp.set_destination(dest.0, dest.1);
                self.last_viewport_dest = Some(dest);
            }
        } else {
            let s = self.integer_scale.max(1);
            if self.last_buffer_scale != s {
                wl.set_buffer_scale(s);
                self.last_buffer_scale = s;
            }
        }
    }

    /// Shows the pop-ups as they are now (or unmaps the strip when there are none), or the
    /// widgets.
    pub(super) fn render(
        &mut self,
        qh: &QueueHandle<App>,
        compositor: &CompositorState,
        shell: &Shell,
    ) {
        if self.widgets {
            // a frame after each: the cards' animations (a list opening) go on from there
            let region = shell.widget_region();
            self.present(qh, compositor, &region, true, |cr, _, _| {
                shell.draw_widgets(cr)
            });
            return;
        }
        let n = &shell.notify;
        if n.popups.is_empty() {
            if self.mapped {
                // no buffer: unmapped; it maps again after a commit and the configure answering it
                self.layer.wl_surface().attach(None, 0, 0);
                self.layer.commit();
                self.mapped = false;
                self.configured = false;
                self.buffer = None;
                self.pointer = None;
                self.last_viewport_dest = None;
            }
            return;
        }
        let now = Instant::now();
        // only the cards take the pointer; the rest of the strip lets clicks through
        let region: Vec<(i32, i32, i32, i32)> = crate::notify::layout(n, self.width as f64)
            .into_iter()
            .map(|(_, r)| (r.0 as i32, r.1 as i32, r.2.ceil() as i32, r.3.ceil() as i32))
            .collect();
        // a card still sliding in: the next frame too
        let arriving = n.popups.iter().any(|p| {
            now.saturating_duration_since(p.born).as_secs_f64() * 1000.0 < crate::notify::ARRIVE_MS
        });
        let pointer = self.pointer;
        self.present(qh, compositor, &region, arriving, |cr, lw, _| {
            crate::notify::draw(cr, &shell.theme, n, lw, pointer, now)
        });
    }

    /// One frame drawn by `draw` (given the surface's logical size), the pointer taken only on
    /// `region`; `more` asks for a frame callback after it.
    pub(super) fn present(
        &mut self,
        qh: &QueueHandle<App>,
        compositor: &CompositorState,
        region: &[(i32, i32, i32, i32)],
        more: bool,
        draw: impl FnOnce(&cairo::Context, f64, f64),
    ) {
        if !self.configured {
            if !self.mapped {
                self.layer.commit();
            }
            return;
        }
        if self.frame_pending {
            self.again = true;
            return;
        }
        let (lw, lh) = (self.width as f64, self.height as f64);
        let (bw_u, bh_u) = self.target_buf_size();
        let (bw, bh) = (bw_u as i32, bh_u as i32);
        if bw <= 0 || bh <= 0 {
            return;
        }
        let stride = bw * 4;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(bw, bh, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };
        canvas.fill(0);
        let (sx, sy) = (f64::from(bw) / lw, f64::from(bh) / lh);
        if let Ok(surface) = unsafe {
            cairo::ImageSurface::create_for_data_unsafe(
                canvas.as_mut_ptr(),
                cairo::Format::ARgb32,
                bw,
                bh,
                stride,
            )
        } {
            if let Ok(cr) = cairo::Context::new(&surface) {
                cr.scale(sx, sy);
                draw(&cr, lw, lh);
            }
            surface.flush();
        }
        if let Ok(r) = Region::new(compositor) {
            for &(x, y, w, h) in region {
                r.add(x, y, w, h);
            }
            self.layer
                .wl_surface()
                .set_input_region(Some(r.wl_region()));
        }
        self.apply_surface_scale();
        let wl = self.layer.wl_surface();
        wl.damage_buffer(0, 0, bw, bh);
        if more {
            wl.frame(qh, wl.clone());
            self.frame_pending = true;
        }
        if buffer.attach_to(wl).is_ok() {
            self.layer.commit();
            self.mapped = true;
            self.buffer = Some(buffer);
        }
        self.again = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wayland::tests::*;

    #[test]
    fn headless_integer_scale_creates_scaled_buffers_and_updates_on_scale_change() {
        let (mock, conn) = MockWayland::start(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 2,
                fractional_120: 240,
            }],
            false,
        );
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/power\"\n",
            ),
            Vec::new(),
        );
        sh.apply(
            "settings",
            "power",
            &crate::cards::settings::parse_power("* balanced:\n  performance:\n")
                as &dyn std::any::Any,
        );
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(app.outputs.len(), 1);
        assert!(app.outputs[0].configured);
        assert!(app.outputs[0].viewport.is_none());

        let sid = surface_wl_id(&app.outputs[0]);
        let target = app.key_target_output();
        assert!(app.shell.toggle_named_on(target, "control"));
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        let c1 = mock.last_commit(sid);
        assert_eq!(c1.buffer_size, Some((2560, 1600)));
        assert_eq!(c1.buffer_scale, 2);
        assert_eq!(c1.viewport_dest, None);
        // opened by its key: the pointer is taken over the panel only, the desktop keeps the rest
        let idx = app.shell.open.unwrap();
        let (pb, _) = app.shell.layout(idx, 1280.0, 800.0);
        assert_eq!(
            c1.input_rects,
            vec![(
                pb.0.floor() as i32,
                pb.1.floor() as i32,
                pb.2.ceil() as i32,
                pb.3.ceil() as i32
            )]
        );
        assert!(surface_has_nonzero_pixels(&mut app.outputs[0]));

        // Compositor updates wl_output scale to 3; buffers and set_buffer_scale follow.
        mock.send_output_scale(0, 3);
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();

        let c2 = mock.last_commit(sid);
        assert_eq!(c2.buffer_size, Some((3840, 2400)));
        assert_eq!(c2.buffer_scale, 3);

        // wl_surface::preferred_buffer_scale (CompositorHandler::scale_factor_changed) updates too.
        mock.send_surface_preferred_buffer_scale(sid, 2);
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();

        let c3 = mock.last_commit(sid);
        assert_eq!(c3.buffer_size, Some((2560, 1600)));
        assert_eq!(c3.buffer_scale, 2);
    }

    #[test]
    fn headless_fractional_scale_uses_viewport_and_scaled_buffer() {
        let (mock, conn) = MockWayland::start(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 2,
                fractional_120: 180, // 1.5x
            }],
            true,
        );
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/power\"\n",
            ),
            Vec::new(),
        );
        sh.apply(
            "settings",
            "power",
            &crate::cards::settings::parse_power("* balanced:\n  performance:\n")
                as &dyn std::any::Any,
        );
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert!(app.outputs[0].viewport.is_some());
        assert_eq!(app.outputs[0].fractional_scale_120, Some(180));

        let sid = surface_wl_id(&app.outputs[0]);
        let target = app.key_target_output();
        assert!(app.shell.toggle_named_on(target, "control"));
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        let c1 = mock.last_commit(sid);
        assert_eq!(c1.buffer_size, Some((1920, 1200)));
        assert_eq!(c1.viewport_dest, Some((1280, 800)));
        assert_eq!(c1.buffer_scale, 1);
        assert!((app.last_display_scale - 1.5).abs() < 1e-4);

        // Preferred scale changes to 2.0x (240/120).
        mock.send_preferred_scale(sid, 240);
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();

        let c2 = mock.last_commit(sid);
        assert_eq!(c2.buffer_size, Some((2560, 1600)));
        assert_eq!(c2.viewport_dest, Some((1280, 800)));
        assert_eq!(c2.buffer_scale, 1);
        assert!((app.last_display_scale - 2.0).abs() < 1e-4);
    }
}
