//! The layer surfaces: one per output for the panel and the widgets, and the notification strip's popup layer, with their shm buffers and scale.

use super::*;

pub(super) struct BufferSlot {
    pub(super) buffer: Buffer,
    /// what was drawn in this buffer: it is zero outside this box, all zero when None
    pub(super) prev_box: Option<(i32, i32, i32, i32)>,
}

/// Makes `bytes` zero, giving their whole pages back to the kernel (in the compositor's mapping of
/// the shared memory too) instead of writing them: a page reads as zero until it is drawn on, so
/// a buffer holds memory only where something was drawn since.
pub(super) fn zero_by_release(bytes: &mut [u8]) {
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as usize;
    let start = bytes.as_ptr() as usize;
    let first = start.next_multiple_of(page) - start;
    let end = ((start + bytes.len()) / page * page).saturating_sub(start);
    if end <= first {
        bytes.fill(0);
        return;
    }
    let mid = &mut bytes[first..end];
    if unsafe { libc::madvise(mid.as_mut_ptr().cast(), mid.len(), libc::MADV_REMOVE) } != 0 {
        eprintln!(
            "infynitae-edge-apps: madvise(MADV_REMOVE): {}",
            std::io::Error::last_os_error()
        );
        mid.fill(0);
    }
    bytes[..first].fill(0);
    bytes[end..].fill(0);
}

/// Clears where private notifications were drawn (`Shell::drawn_private`, device pixels), so the
/// surface, and every capture of it, has nothing there. Returns those places in `cr`'s user space
/// (the surface's logical pixels): a private overlay (`SurfaceKind::PrivatePanel`,
/// `PrivateWidgets`) draws the same frame there, kept out of captures by the compositor.
pub(super) fn cut_private(cr: &cairo::Context, rects: &[(i32, i32, i32, i32)]) -> Vec<Rect> {
    if rects.is_empty() {
        return Vec::new();
    }
    // the surfaces draw at a scale (and nothing else) from logical to buffer pixels
    let m = cr.matrix();
    let logical = rects
        .iter()
        .map(|&(x, y, w, h)| {
            (
                (f64::from(x) - m.x0()) / m.xx(),
                (f64::from(y) - m.y0()) / m.yy(),
                f64::from(w) / m.xx(),
                f64::from(h) / m.yy(),
            )
        })
        .collect();
    cr.save().ok();
    cr.identity_matrix();
    cr.reset_clip();
    cr.set_operator(cairo::Operator::Clear);
    for &(x, y, w, h) in rects {
        cr.rectangle(f64::from(x), f64::from(y), f64::from(w), f64::from(h));
    }
    let _ = cr.fill();
    cr.restore().ok();
    logical
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
    /// a clear frame is committed: on its frame callback, when the compositor has read it, its
    /// buffer's pages are given back (`release_shown_buffer`)
    pub(super) release_pending: bool,
    /// where the last frame left private notifications out (`cut_private`), logical pixels
    pub(super) private: Vec<Rect>,
    /// `private` is new: the private overlay draws again (`App::sync_private`)
    pub(super) private_changed: bool,
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
            if show_here && (shell.ask.ask.on || shell.shortcuts.open) {
                // questions and overlays take the whole screen: a click outside closes
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
            self.release_pending = false;
            self.render_if_needed(qh, shell);
        } else {
            self.dirty = false;
            self.frame_pending = false;
            self.clear_and_commit(qh, shell);
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
        // the buffer not shown last first, when there is one: after a close the shown one's pages
        // were given back (`release_shown_buffer`), and a compositor may release it at once
        let other = 1 - self.last_slot;
        let order = if self.slots[other].is_some() {
            [other, self.last_slot]
        } else {
            [self.last_slot, other]
        };
        for i in order {
            if self.slots[i].is_none() {
                if let Ok((buffer, canvas)) =
                    self.pool
                        .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
                {
                    zero_by_release(canvas);
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
    pub(super) fn clear_and_commit(&mut self, qh: &QueueHandle<App>, shell: &Shell) {
        if !self.private.is_empty() {
            self.private.clear();
            self.private_changed = true;
        }
        let any_visible = shell.visible();
        let dump = |this: &Self| {
            if !any_visible {
                if this.width > 0 && this.height > 0 {
                    crate::boxes::dump(shell, f64::from(this.width), f64::from(this.height));
                } else if shell.notify.osd.is_none() {
                    crate::boxes::dump_closed();
                }
            }
        };
        if self.mapped_closed && self.buf_size == self.target_buf_size() {
            self.apply_surface_scale();
            self.layer.commit();
            dump(self);
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
        if let (Some(canvas), Some(r)) = (slot.buffer.canvas(&mut self.pool), old) {
            Self::clear_box(canvas, w * 4, w, h, r);
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
            wl.frame(qh, wl.clone());
            self.release_pending = true;
            self.mapped_closed = true;
            self.last_slot = idx;
            self.layer.commit();
        }
        dump(self);
    }

    /// Closed and its clear frame shown: that buffer's pages (all zero) given back, 8 MB on a
    /// 1920×1080 screen. The other buffer stays in memory: the compositor holds the shown one,
    /// so the next opening draws its first frame in the other, and there freed pages would come
    /// back at a cost (2.3 ms for the rows of a 440 by 1080 panel, 0.02 ms when held).
    pub(super) fn release_shown_buffer(&mut self) {
        self.release_pending = false;
        if let Some(slot) = &mut self.slots[self.last_slot] {
            zero_by_release(self.pool.raw_data_mut(&slot.buffer.slot()));
            slot.prev_box = None;
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
        let x0 = ((bx - pad - lift) * sx).floor().max(0.0) as i32;
        let y0 = ((by - pad - lift) * sy).floor().max(0.0) as i32;
        let x1 = ((bx + bw + pad + lift) * sx).ceil().min(f64::from(buf_w)) as i32;
        let y1 = ((by + bh + pad + lift) * sy).ceil().min(f64::from(buf_h)) as i32;
        let new_box = (x0, y0, (x1 - x0).max(0), (y1 - y0).max(0));
        self.apply_surface_scale();
        let slot = self.slots[slot_idx].as_mut().unwrap();
        let old_box = slot.prev_box.replace(new_box);
        if let Some(canvas) = slot.buffer.canvas(&mut self.pool) {
            if let Some(r) = old_box {
                Self::clear_box(canvas, stride, buf_w, buf_h, r);
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
                    let cut = cut_private(&cr, &shell.drawn_private.borrow());
                    // drawn again with each frame while one shows (a light, a scroll may move it)
                    self.private_changed |= !cut.is_empty() || !self.private.is_empty();
                    self.private = cut;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SurfaceKind {
    Popups,
    /// the pop-ups of private apps (PRIVACY_APPS), on a strip of their own kept out of captures
    PrivatePopups,
    Widgets,
    Osd,
    /// over the panel on this output, kept out of captures: the panel's frame where it left
    /// private notifications out (`cut_private`), nothing elsewhere
    PrivatePanel(OutputId),
    /// the same over the desktop's widgets, on the Bottom layer with them
    PrivateWidgets,
}

/// The namespace the compositor keeps out of every capture, as it does private windows
/// (`PRIVATE_NAMESPACE` in crates/infynitae-wayland/src/layers.rs).
pub(super) const PRIVATE_NAMESPACE: &str = "infynitae-private";

/// The notification pop-ups' surface (in place of mako): a strip down the right of the first
/// screen on the Top layer, taking the pointer only on the cards, and unmapped (no buffer) while
/// there is nothing to show (private apps' pop-ups on a second strip over it, `PrivatePopups`), so it costs nothing and is never one more surface over a full-screen
/// window. The desktop's widgets (`Widgets`) sit on the Bottom layer on the first screen, and the
/// volume/brightness OSD (`Osd`) sits at the bottom centre on the Top layer with no input region
/// and unmapped when hidden.
pub(super) struct PopupLayer {
    pub(super) kind: SurfaceKind,
    pub(super) output: wl_output::WlOutput,
    pub(super) frac_scale: Option<WpFractionalScaleV1>,
    pub(super) viewport: Option<WpViewport>,
    pub(super) layer: LayerSurface,
    pub(super) pool: SlotPool,
    /// for a new pool when the surface unmaps
    pub(super) shm: Shm,
    pub(super) buffer: Option<Buffer>,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) integer_scale: i32,
    pub(super) fractional_scale_120: Option<u32>,
    pub(super) last_buffer_scale: i32,
    pub(super) last_viewport_dest: Option<(i32, i32)>,
    pub(super) configured: bool,
    pub(super) awaiting_configure: bool,
    /// a buffer is attached (the surface shows)
    pub(super) mapped: bool,
    pub(super) frame_pending: bool,
    /// draw again on the next frame (a card still sliding in)
    pub(super) again: bool,
    /// where the pointer is on it
    pub(super) pointer: Option<(f64, f64)>,
    /// `Widgets`: where the last frame left private notifications out (`cut_private`); a private
    /// overlay: where it draws them. Logical pixels.
    pub(super) private: Vec<Rect>,
    /// `Widgets`: `private` is new, its overlay draws again (`App::sync_private`)
    pub(super) private_changed: bool,
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
    /// Creates a `Popups`, `Widgets`, or `Osd` layer surface on `output`.
    pub(super) fn new(
        app: &App,
        output: wl_output::WlOutput,
        kind: SurfaceKind,
    ) -> Option<PopupLayer> {
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
        let layer = match kind {
            SurfaceKind::Widgets => {
                let l = app.layer_shell.create_layer_surface(
                    &app.qh,
                    surface,
                    Layer::Bottom,
                    Some("infynitae-widgets"),
                    Some(&output),
                );
                l.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
                l
            }
            SurfaceKind::Popups | SurfaceKind::PrivatePopups => {
                let namespace = if kind == SurfaceKind::PrivatePopups {
                    PRIVATE_NAMESPACE
                } else {
                    "infynitae-notifications"
                };
                let l = app.layer_shell.create_layer_surface(
                    &app.qh,
                    surface,
                    Layer::Top,
                    Some(namespace),
                    Some(&output),
                );
                l.set_anchor(Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM);
                l.set_size(POPUP_STRIP_W, 0);
                l
            }
            SurfaceKind::PrivatePanel(_) | SurfaceKind::PrivateWidgets => {
                let layer = if kind == SurfaceKind::PrivateWidgets {
                    Layer::Bottom
                } else {
                    Layer::Top
                };
                let l = app.layer_shell.create_layer_surface(
                    &app.qh,
                    surface,
                    layer,
                    Some(PRIVATE_NAMESPACE),
                    Some(&output),
                );
                l.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
                l
            }
            SurfaceKind::Osd => {
                let l = app.layer_shell.create_layer_surface(
                    &app.qh,
                    surface,
                    Layer::Top,
                    Some("infynitae-osd"),
                    Some(&output),
                );
                l.set_anchor(Anchor::BOTTOM);
                l.set_size(crate::notify::OSD_W as u32, crate::notify::OSD_H as u32);
                l.set_margin(0, 0, crate::notify::MARGIN as i32, 0);
                l
            }
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
            kind,
            output,
            frac_scale,
            viewport,
            layer,
            pool,
            shm: Shm::from(app.shm.wl_shm().clone()),
            buffer: None,
            width: 0,
            height: 0,
            integer_scale,
            fractional_scale_120,
            last_buffer_scale: 1,
            last_viewport_dest: None,
            configured: false,
            awaiting_configure: true,
            mapped: false,
            frame_pending: false,
            again: false,
            pointer: None,
            private: Vec::new(),
            private_changed: false,
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

    fn unmap(&mut self) -> bool {
        if !self.mapped {
            return false;
        }
        // no buffer: unmapped; it maps again after a commit and the configure answering it
        self.layer.wl_surface().attach(None, 0, 0);
        self.layer.commit();
        self.mapped = false;
        self.configured = false;
        self.awaiting_configure = false;
        self.buffer = None;
        self.pointer = None;
        self.last_viewport_dest = None;
        // the pool is not kept while nothing shows; each frame is drawn whole in a new buffer
        match SlotPool::new(4096, &self.shm) {
            Ok(pool) => self.pool = pool,
            Err(e) => eprintln!("infynitae-edge-apps: notification pool: {e}"),
        }
        true
    }

    /// Shows the pop-ups, widgets, or OSD as they are now (unmapping when empty). Returns `true`
    /// when a buffer was committed or the surface unmapped.
    pub(super) fn render(
        &mut self,
        qh: &QueueHandle<App>,
        compositor: &CompositorState,
        shell: &Shell,
    ) -> bool {
        match self.kind {
            SurfaceKind::Widgets => {
                // a frame after each: the cards' animations (a list opening) go on from there
                let region = shell.widget_region();
                let mut cut = None;
                let committed = self.present(qh, compositor, &region, true, |cr, _, _| {
                    shell.draw_widgets(cr);
                    cut = Some(cut_private(cr, &shell.drawn_private.borrow()));
                });
                if let Some(cut) = cut {
                    self.private_changed |= !cut.is_empty() || !self.private.is_empty();
                    self.private = cut;
                }
                committed
            }
            SurfaceKind::PrivatePanel(_) | SurfaceKind::PrivateWidgets => {
                if self.private.is_empty() {
                    return self.unmap();
                }
                let clip = self.private.clone();
                let kind = self.kind;
                // no input: clicks go to the panel or widgets under it
                self.present(qh, compositor, &[], false, |cr, lw, lh| {
                    for &(x, y, w, h) in &clip {
                        cr.rectangle(x, y, w, h);
                    }
                    cr.clip();
                    match kind {
                        SurfaceKind::PrivatePanel(id) => {
                            shell.draw_on(id, cr, lw, lh);
                        }
                        _ => shell.draw_widgets(cr),
                    }
                })
            }
            SurfaceKind::Popups | SurfaceKind::PrivatePopups => {
                let n = &shell.notify;
                let private = self.kind == SurfaceKind::PrivatePopups;
                if !n.popups.iter().any(|p| p.note.private == private) {
                    return self.unmap();
                }
                let now = Instant::now();
                // only its own cards take the pointer; the rest of the strip lets clicks through
                // (to the other strip's cards, or what is under both)
                let region: Vec<(i32, i32, i32, i32)> = crate::notify::layout(n, self.width as f64)
                    .into_iter()
                    .zip(&n.popups)
                    .filter(|(_, p)| p.note.private == private)
                    .map(|((_, r), _)| {
                        (r.0 as i32, r.1 as i32, r.2.ceil() as i32, r.3.ceil() as i32)
                    })
                    .collect();
                // a card still sliding in: the next frame too
                let arriving = n.popups.iter().any(|p| {
                    p.note.private == private
                        && now.saturating_duration_since(p.born).as_secs_f64() * 1000.0
                            < crate::notify::ARRIVE_MS
                });
                let pointer = self.pointer;
                self.present(qh, compositor, &region, arriving, |cr, lw, _| {
                    crate::notify::draw(cr, &shell.theme, n, lw, private, pointer, now)
                })
            }
            SurfaceKind::Osd => match &shell.notify.osd {
                Some(osd) => self.present(qh, compositor, &[], false, |cr, lw, lh| {
                    crate::notify::draw_osd(cr, &shell.theme, osd, lw, lh)
                }),
                None => self.unmap(),
            },
        }
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
    ) -> bool {
        if !self.configured {
            if !self.mapped && !self.awaiting_configure {
                self.layer.commit();
                self.awaiting_configure = true;
            }
            return false;
        }
        if self.frame_pending {
            self.again = true;
            return false;
        }
        let (lw, lh) = (self.width as f64, self.height as f64);
        let (bw_u, bh_u) = self.target_buf_size();
        let (bw, bh) = (bw_u as i32, bh_u as i32);
        if bw <= 0 || bh <= 0 {
            return false;
        }
        let stride = bw * 4;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(bw, bh, stride, wl_shm::Format::Argb8888)
        else {
            return false;
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
        let committed = if buffer.attach_to(wl).is_ok() {
            self.layer.commit();
            self.mapped = true;
            self.buffer = Some(buffer);
            true
        } else {
            false
        };
        self.again = false;
        committed
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

    /// Bytes of `bytes` held in memory (its pages that are resident).
    fn resident_bytes(bytes: &[u8]) -> usize {
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let start = bytes.as_ptr() as usize / page * page;
        let len = bytes.as_ptr() as usize + bytes.len() - start;
        let mut pages = vec![0u8; len.div_ceil(page)];
        let r = unsafe { libc::mincore(start as *mut libc::c_void, len, pages.as_mut_ptr()) };
        assert_eq!(r, 0, "mincore: {}", std::io::Error::last_os_error());
        pages.iter().filter(|p| *p & 1 != 0).count() * page
    }

    fn slots_resident(os: &mut OutputSurface) -> Vec<usize> {
        let slots: Vec<_> = os.slots.iter().flatten().map(|s| s.buffer.slot()).collect();
        slots
            .iter()
            .map(|s| resident_bytes(os.pool.raw_data_mut(s)))
            .collect()
    }

    #[test]
    fn headless_closed_panel_gives_its_shown_buffers_pages_back_and_draws_again() {
        let (_mock, conn) = MockWayland::start(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
        );
        let sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/session\"\n",
            ),
            Vec::new(),
        );
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        app.config_path = Some(std::env::temp_dir().join("infynitae-edge-test-no-config"));
        eq.roundtrip(&mut app).unwrap();
        let full = 1280 * 800 * 4;

        let target = app.key_target_output();
        assert!(app.shell.toggle_named_on(target, "control"));
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        app.sync_all();
        for _ in 0..3 {
            eq.roundtrip(&mut app).unwrap();
        }
        assert!(surface_has_nonzero_pixels(&mut app.outputs[0]));
        let open = slots_resident(&mut app.outputs[0]);
        assert!(open.iter().any(|&b| b > 0), "{open:?}");

        // closed (its leaving fade already over): the clear frame, then on its callback the pages go
        let faded = Instant::now() - Duration::from_millis(500);
        assert!(app.shell.close_at(faded));
        app.sync_all();
        for _ in 0..4 {
            eq.roundtrip(&mut app).unwrap();
        }
        assert!(!app.shell.visible());
        assert!(!app.outputs[0].release_pending);
        let shown = app.outputs[0].last_slot;
        let closed = slots_resident(&mut app.outputs[0]);
        assert_eq!(closed.len(), 2);
        assert_eq!(
            closed[shown], 0,
            "the shown clear buffer is held: {closed:?}"
        );
        // kept, holding only the pages drawn on: a new buffer is zeroed without touching them
        let kept = closed[1 - shown];
        assert!(kept > 0 && kept < full, "the other is kept: {closed:?}");

        // open again: the first frame is drawn in the kept buffer, as before
        assert!(app.shell.toggle_named_on(target, "control"));
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        app.sync_all();
        assert_eq!(app.outputs[0].last_slot, 1 - shown);
        eq.roundtrip(&mut app).unwrap();
        assert!(surface_has_nonzero_pixels(&mut app.outputs[0]));
    }

    #[test]
    fn headless_notification_strip_drops_its_pool_when_the_last_popup_goes() {
        let (_mock, conn) = MockWayland::start(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
        );
        let sh = Shell::new(crate::config::Config::from_text(""), Vec::new());
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        app.config_path = Some(std::env::temp_dir().join("infynitae-edge-test-no-config"));
        eq.roundtrip(&mut app).unwrap();

        let note = crate::notify::Note {
            id: 1,
            app: "Test".into(),
            summary: "Copied".into(),
            body: String::new(),
            actions: vec![],
            urgency: 1,
            value: None,
            sync: None,
            expire: Some(None),
            transient: false,
            resident: false,
            time: "12:00".into(),
            desktop_entry: String::new(),
            program: String::new(),
            private: false,
        };
        let quiet = crate::notify::Quiet::No;
        assert!(app.shell.notify.receive(note, Instant::now(), quiet));
        app.sync_popups();
        for _ in 0..3 {
            eq.roundtrip(&mut app).unwrap();
        }
        let pl = app
            .popups
            .first()
            .expect("the strip is made with the first pop-up");
        assert!(pl.mapped);
        let drawn = pl.pool.len();
        assert!(drawn >= (POPUP_STRIP_W * 800 * 4) as usize, "{drawn}");

        assert!(app.shell.notify.close(1));
        app.sync_popups();
        let pl = app.popups.first().unwrap();
        assert!(!pl.mapped);
        assert_eq!(
            pl.pool.len(),
            4096,
            "the drawn pool ({drawn} bytes) is let go"
        );
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

    #[test]
    fn headless_osd_surface_shows_at_bottom_centre_replaces_unmaps_and_remaps() {
        use smithay_client_toolkit::reexports::client::Proxy;

        let (mock, conn) = MockWayland::start(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
        );
        let sh = Shell::new(crate::config::Config::from_text(""), Vec::new());
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        app.config_path = Some(std::env::temp_dir().join("infynitae-edge-test-no-config"));
        eq.roundtrip(&mut app).unwrap();
        assert!(app.osd_layer.is_none());

        let osd_layers = || {
            mock.state
                .lock()
                .unwrap()
                .layer_kind
                .values()
                .filter(|(_, name)| name == "infynitae-osd")
                .cloned()
                .collect::<Vec<_>>()
        };
        assert!(osd_layers().is_empty());

        let osd_note = |id, sync: &str, value: Option<i32>, summary: &str| crate::notify::Note {
            id,
            app: "Audio".into(),
            summary: summary.into(),
            body: String::new(),
            actions: vec![],
            urgency: 1,
            value,
            sync: Some(sync.into()),
            expire: Some(Some(Duration::from_millis(1200))),
            transient: true,
            resident: false,
            time: "12:00".into(),
            desktop_entry: String::new(),
            program: String::new(),
            private: false,
        };

        let t0 = Instant::now();
        assert!(app.shell.notify.receive(
            osd_note(1, "volume", Some(60), "Volume: 60%"),
            t0,
            crate::notify::Quiet::No,
        ));
        assert!(app.shell.notify.history.is_empty());
        assert!(app.shell.notify.popups.is_empty());
        app.sync_popups();
        for _ in 0..3 {
            eq.roundtrip(&mut app).unwrap();
        }

        assert_eq!(osd_layers(), vec![(2, "infynitae-osd".to_string())]);
        let sid = app
            .osd_layer
            .as_ref()
            .expect("osd surface")
            .layer
            .wl_surface()
            .id()
            .protocol_id();
        let c1 = mock.last_commit(sid);
        assert_eq!(c1.buffer_size, Some((240, 56)));
        assert_eq!(c1.keyboard_interactivity, 0);
        assert!(c1.input_rects.is_empty());
        assert_eq!(
            crate::boxes::text(&app.shell, 1280.0, 800.0),
            "osd 520 720 240 56 60 Volume\n"
        );
        let px1 = {
            let ol = app.osd_layer.as_mut().unwrap();
            assert!(ol.mapped);
            assert_eq!(ol.kind, SurfaceKind::Osd);
            ol.buffer
                .as_ref()
                .unwrap()
                .canvas(&mut ol.pool)
                .unwrap()
                .to_vec()
        };
        assert!(px1.iter().any(|&b| b != 0));

        // Second press (launch.sh mute format without int:value) replaces in place on the same surface.
        assert!(app.shell.notify.receive(
            osd_note(2, "volume", None, "Muted"),
            t0 + Duration::from_millis(100),
            crate::notify::Quiet::No,
        ));
        assert!(app.shell.notify.history.is_empty());
        app.sync_popups();
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(osd_layers().len(), 1);
        assert_eq!(
            crate::boxes::text(&app.shell, 1280.0, 800.0),
            "osd 520 720 240 56 0 Muted\n"
        );
        let px2 = {
            let ol = app.osd_layer.as_mut().unwrap();
            ol.buffer
                .as_ref()
                .unwrap()
                .canvas(&mut ol.pool)
                .unwrap()
                .to_vec()
        };
        assert_ne!(px1, px2);

        // Expiry after 1.2 s unmaps the layer surface and releases the buffer pool.
        assert!(app
            .shell
            .notify
            .expire(t0 + Duration::from_millis(100) + crate::notify::OSD_TIMEOUT));
        app.sync_popups();
        eq.roundtrip(&mut app).unwrap();
        let ol = app.osd_layer.as_ref().unwrap();
        assert!(!ol.mapped);
        assert_eq!(ol.pool.len(), 4096);
        assert_eq!(mock.last_commit(sid).buffer_size, None);
        assert_eq!(crate::boxes::text(&app.shell, 1280.0, 800.0), "closed\n");

        // Pressing brightness afterward remaps the existing OSD surface cleanly.
        assert!(app.shell.notify.receive(
            osd_note(3, "brightness", Some(80), "Brightness: 80%"),
            t0 + Duration::from_millis(2000),
            crate::notify::Quiet::No,
        ));
        app.sync_popups();
        for _ in 0..3 {
            eq.roundtrip(&mut app).unwrap();
        }
        assert!(app.osd_layer.as_ref().unwrap().mapped);
        assert_eq!(mock.last_commit(sid).buffer_size, Some((240, 56)));
        assert_eq!(
            crate::boxes::text(&app.shell, 1280.0, 800.0),
            "osd 520 720 240 56 80 Brightness\n"
        );
    }

    fn private_test_note(id: u32, app: &str, desktop_entry: &str) -> crate::notify::Note {
        crate::notify::Note {
            id,
            app: app.into(),
            summary: format!("Secret {id}"),
            body: "body".into(),
            actions: vec![],
            urgency: 2,
            value: None,
            sync: None,
            expire: None,
            transient: false,
            resident: false,
            time: "12:00".into(),
            desktop_entry: desktop_entry.into(),
            program: String::new(),
            private: false,
        }
    }

    /// Whether any pixel of `pl`'s last buffer inside `r` (logical = buffer pixels, scale 1) is
    /// drawn.
    fn drawn_in(pl: &mut PopupLayer, r: (i32, i32, i32, i32)) -> bool {
        let w = pl.width as i32;
        let Some(buffer) = pl.buffer.as_ref() else {
            return false;
        };
        let Some(bytes) = buffer.canvas(&mut pl.pool) else {
            return false;
        };
        (r.1..r.1 + r.3).any(|y| {
            (r.0..r.0 + r.2).any(|x| bytes[((y * w + x) * 4) as usize..][..4] != [0, 0, 0, 0])
        })
    }

    fn strip(app: &mut App, kind: SurfaceKind) -> &mut PopupLayer {
        app.popups
            .iter_mut()
            .find(|p| p.kind == kind)
            .expect("the strip is made")
    }

    fn write_config(path: &std::path::Path, text: &str, ahead_s: u64) {
        std::fs::write(path, text).unwrap();
        let t = std::time::SystemTime::now() + Duration::from_secs(ahead_s);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    /// PRIVACY_APPS="signal": Signal's pop-up is drawn on the strip named "infynitae-private"
    /// (which the compositor keeps out of captures), Mail's on the usual strip, each only where
    /// its card is. Mail added to the config moves its pop-up over while it shows; an app found by
    /// its desktop entry alone goes there too.
    #[test]
    fn headless_a_private_apps_pop_up_is_on_the_private_strip_and_follows_the_config() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-edge-private-popups-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config");
        write_config(&cfg_path, "PRIVACY_APPS=\"signal\"\n", 0);
        let (mock, conn) = MockWayland::start(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
        );
        let sh = Shell::new_at(
            crate::config::Config::from_text(&std::fs::read_to_string(&cfg_path).unwrap()),
            Vec::new(),
            &cfg_path,
        );
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        let t0 = Instant::now();
        let clock = std::rc::Rc::new(std::cell::Cell::new(t0));
        app.clock = Some(std::rc::Rc::clone(&clock));
        app.config_path = Some(cfg_path.clone());
        eq.roundtrip(&mut app).unwrap();

        let quiet = crate::notify::Quiet::No;
        // born long ago: drawn whole, not sliding in
        let born = Instant::now() - Duration::from_secs(5);
        app.shell
            .notify
            .receive(private_test_note(1, "Signal", ""), born, quiet);
        app.shell
            .notify
            .receive(private_test_note(2, "Mail", ""), born, quiet);
        app.sync_popups();
        for _ in 0..3 {
            eq.roundtrip(&mut app).unwrap();
        }
        let names: Vec<String> = mock
            .state
            .lock()
            .unwrap()
            .layer_kind
            .values()
            .map(|(_, n)| n.clone())
            .collect();
        assert!(names.contains(&PRIVATE_NAMESPACE.to_string()), "{names:?}");
        assert!(names.contains(&"infynitae-notifications".to_string()));
        // newest at the top: Mail, then Signal
        let w = strip(&mut app, SurfaceKind::Popups).width as f64;
        let cards: Vec<(i32, i32, i32, i32)> = crate::notify::layout(&app.shell.notify, w)
            .into_iter()
            .map(|(_, r)| (r.0 as i32, r.1 as i32, r.2 as i32, r.3 as i32))
            .collect();
        let (mail, signal) = (cards[0], cards[1]);
        let public = strip(&mut app, SurfaceKind::Popups);
        assert!(public.mapped);
        assert!(drawn_in(public, mail), "Mail's card on the usual strip");
        assert!(!drawn_in(public, signal), "nothing of Signal's there");
        let private = strip(&mut app, SurfaceKind::PrivatePopups);
        assert!(private.mapped);
        assert!(
            drawn_in(private, signal),
            "Signal's card on the private strip"
        );
        assert!(!drawn_in(private, mail));
        let sid = |pl: &PopupLayer| {
            use smithay_client_toolkit::reexports::client::Proxy;
            pl.layer.wl_surface().id().protocol_id()
        };
        let private_sid = sid(strip(&mut app, SurfaceKind::PrivatePopups));
        assert_eq!(
            mock.last_commit(private_sid).input_rects,
            vec![signal],
            "the private strip takes the pointer on its own card only"
        );

        // Mail added (as Super+Shift+P writes it): read at the next turn after 250 ms
        write_config(&cfg_path, "PRIVACY_APPS=\"signal,mail\"\n", 5);
        clock.set(t0 + Duration::from_millis(300));
        app.sync_popups();
        for _ in 0..3 {
            eq.roundtrip(&mut app).unwrap();
        }
        assert!(
            !strip(&mut app, SurfaceKind::Popups).mapped,
            "nothing left on the usual strip"
        );
        let private = strip(&mut app, SurfaceKind::PrivatePopups);
        assert!(drawn_in(private, mail) && drawn_in(private, signal));
        assert!(app.shell.notes.iter().all(|n| n.private), "the history too");

        // found by its desktop entry, its name says nothing
        write_config(&cfg_path, "PRIVACY_APPS=\"keepassxc\"\n", 10);
        clock.set(t0 + Duration::from_millis(600));
        app.shell.notify.receive(
            private_test_note(3, "Passwords", "org.keepassxc.KeePassXC"),
            born,
            quiet,
        );
        app.sync_popups();
        let shown: Vec<(String, bool)> = app
            .shell
            .notify
            .popups
            .iter()
            .map(|p| (p.note.app.clone(), p.note.private))
            .collect();
        assert_eq!(
            shown,
            vec![
                ("Passwords".to_string(), true),
                ("Mail".to_string(), false),
                ("Signal".to_string(), false),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Notifications card lists a private app's entry: the panel's surface has nothing where
    /// it is (a capture of it shows nothing there), the private overlay draws it there, and the
    /// other entry stays on the panel. Taken out of the history, the overlay goes.
    #[test]
    fn headless_a_private_entry_in_the_history_is_drawn_only_on_the_private_overlay() {
        let (mock, conn) = MockWayland::start(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
        );
        let cfg = crate::config::Config::from_text(
            "PANEL_CONTROL=\"SUPER+A top right 380 notifications/list\"\nPRIVACY_APPS=\"signal\"\n",
        );
        let sh = Shell::new(cfg, Vec::new());
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        app.config_path = Some(std::env::temp_dir().join("infynitae-edge-test-no-config"));
        eq.roundtrip(&mut app).unwrap();
        // in the history only: a game kept the pop-ups back
        let quiet = crate::notify::Quiet::Game;
        let now = Instant::now();
        app.shell
            .notify
            .receive(private_test_note(1, "Signal", ""), now, quiet);
        app.shell
            .notify
            .receive(private_test_note(2, "Mail", ""), now, quiet);
        app.sync_popups();
        assert_eq!(app.shell.notes.len(), 2);

        let target = app.key_target_output();
        assert!(app.shell.toggle_named_on(target, "control"));
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();
        app.sync_private();
        for _ in 0..3 {
            eq.roundtrip(&mut app).unwrap();
        }

        let rects = app.outputs[0].private.clone();
        assert_eq!(rects.len(), 1, "one private entry: {rects:?}");
        let r = rects[0];
        let inner = (
            r.0.ceil() as i32 + 2,
            r.1.ceil() as i32 + 2,
            r.2 as i32 - 4,
            r.3 as i32 - 4,
        );
        let os = &mut app.outputs[0];
        let w = os.buf_size.0 as i32;
        let slot = os.slots[os.last_slot].as_ref().unwrap();
        let bytes = slot.buffer.canvas(&mut os.pool).unwrap();
        let px = |x: i32, y: i32| bytes[((y * w + x) * 4) as usize..][..4].to_vec();
        let clear = (inner.1..inner.1 + inner.3)
            .all(|y| (inner.0..inner.0 + inner.2).all(|x| px(x, y) == [0, 0, 0, 0]));
        assert!(clear, "nothing of the private entry on the panel's surface");
        // the card (and Mail's entry) right under it is drawn
        let below = (inner.1 + inner.3 + 6..inner.1 + inner.3 + 40)
            .any(|y| (inner.0..inner.0 + inner.2).any(|x| px(x, y) != [0, 0, 0, 0]));
        assert!(below, "the rest of the card is on the panel's surface");

        let overlay = app
            .private_layers
            .iter_mut()
            .find(|p| matches!(p.kind, SurfaceKind::PrivatePanel(_)))
            .expect("the overlay is made");
        assert!(overlay.mapped);
        assert!(
            drawn_in(overlay, inner),
            "the entry is drawn on the overlay"
        );
        let outside = (0, 0, 1280, inner.1 - 4);
        assert!(!drawn_in(overlay, outside), "and nothing else");
        let names: Vec<String> = mock
            .state
            .lock()
            .unwrap()
            .layer_kind
            .values()
            .map(|(_, n)| n.clone())
            .collect();
        assert_eq!(
            names.iter().filter(|n| *n == PRIVATE_NAMESPACE).count(),
            1,
            "{names:?}"
        );

        assert!(app.shell.notify.dismiss(1));
        app.sync_popups();
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();
        app.sync_private();
        eq.roundtrip(&mut app).unwrap();
        assert!(app.outputs[0].private.is_empty());
        let overlay = app
            .private_layers
            .iter()
            .find(|p| matches!(p.kind, SurfaceKind::PrivatePanel(_)))
            .unwrap();
        assert!(!overlay.mapped, "no private entry: the overlay unmaps");
    }
}
