use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_buffer, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface,
};
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{
    PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT, BTN_RIGHT,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::raw::RawPool;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm, registry_handlers,
};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1, wp_cursor_shape_manager_v1,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

use crate::cli::Mode;
use crate::draw::{render_overlay, OverlayDraw};
use crate::edit::{Editor, Finish, Step};
use crate::geom::{
    crop_upright_surface, encode_png_bytes, join_selection_across_outputs, parse_snapshot_json,
    pick_window_at, upright_surface_from_panel, windows_on_output_with_size, CapturedOutputSlice,
    DragSelection, OutputTransform, PanelFrame, Rect, RectF, Snapshot,
};
use crate::look::Look;

#[derive(Debug)]
struct OfferedBuffer {
    format: wl_shm::Format,
    width: u32,
    height: u32,
    stride: u32,
}

enum CaptureStatus {
    Pending,
    Ready(cairo::ImageSurface),
    Failed(String),
}

struct CaptureJob {
    output_idx: usize,
    _frame: zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
    version: u32,
    offered: Option<OfferedBuffer>,
    copied: bool,
    y_invert: bool,
    pool: Option<RawPool>,
    wl_buffer: Option<wl_buffer::WlBuffer>,
    status: CaptureStatus,
}

struct OverlaySurface {
    layer: LayerSurface,
    viewport: Option<wp_viewport::WpViewport>,
    pool: SlotPool,
    buffers: Vec<Buffer>,
    buf_size: (u32, u32),
    configured: bool,
    logical_w: u32,
    logical_h: u32,
    dirty: bool,
}

struct PendingKey {
    keysym: Keysym,
    utf8: Option<String>,
    ctrl: bool,
}

struct OutputEntry {
    wl_output: wl_output::WlOutput,
    name: String,
    transform: OutputTransform,
    logical_x: i32,
    logical_y: i32,
    logical_w: u32,
    logical_h: u32,
    view_w: u32,
    view_h: u32,
    scale: f64,
    windows_px: Vec<Rect>,
    frozen: Option<cairo::ImageSurface>,
    overlay: Option<OverlaySurface>,
}

impl OutputEntry {
    fn logical_rect(&self) -> RectF {
        RectF::new(
            self.logical_x as f64,
            self.logical_y as f64,
            self.logical_w.max(1) as f64,
            self.logical_h.max(1) as f64,
        )
    }

    fn scale_xy(&self) -> (f64, f64) {
        let lw = self.logical_w.max(1) as f64;
        let lh = self.logical_h.max(1) as f64;
        (self.view_w as f64 / lw, self.view_h as f64 / lh)
    }
}

#[derive(Debug, Clone, Copy)]
enum FinalSelection {
    OutputWindow(usize, Rect),
    Logical(RectF),
    /// Control+A: the whole output
    Output(usize),
}

/// What the run produced.
pub enum Outcome {
    /// Escape or a right click while picking
    Cancelled,
    /// The capture without marks: to the file and the clipboard
    Plain(Vec<u8>),
    /// Marked up, to the clipboard only
    Copy(Vec<u8>),
    /// Marked up, to the file and the clipboard
    Save(Vec<u8>),
}

struct App {
    conn: Connection,
    registry_state: RegistryState,
    output_state: OutputState,
    compositor_state: CompositorState,
    shm: Shm,
    layer_shell: Option<LayerShell>,
    screencopy: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    viewporter: Option<wp_viewporter::WpViewporter>,
    cursor_shape_mgr: Option<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1>,
    seat_state: SeatState,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_device: Option<wp_cursor_shape_device_v1::WpCursorShapeDeviceV1>,

    mode: Mode,
    look: Look,
    snapshot: Snapshot,

    outputs: Vec<OutputEntry>,
    captures: Vec<CaptureJob>,

    pointer_output: Option<usize>,
    pointer_local_px: (i32, i32),
    hovered_window: Option<(usize, Rect)>,
    drag: Option<DragSelection>,
    space_held: bool,
    last_cursor: Option<wp_cursor_shape_device_v1::Shape>,

    accepted: Option<FinalSelection>,
    cancelled: bool,

    modifiers: Modifiers,
    keyboard_surface: Option<wl_surface::WlSurface>,
    pending_keys: Vec<PendingKey>,
    last_pointer_enter: Option<(usize, u32)>,
    /// Shift was held when the pick ended: no editor
    skip_editor: bool,
    picked_at: Option<Instant>,
    /// ms from `picked_at` to the editor's first frame
    ready_ms: Option<f64>,
    editor: Option<Editor>,
    editor_output: usize,
    finish: Option<Finish>,
    editor_failed: Option<String>,
    /// INFYNITAE_SHOT_BOXES: a file the VM tests read, rewritten after each editor frame
    boxes: Option<PathBuf>,
}

impl App {
    fn find_output_by_surface(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.outputs.iter().position(|o| {
            o.overlay
                .as_ref()
                .is_some_and(|ov| ov.layer.wl_surface() == surface)
        })
    }

    fn virtual_bounds(&self) -> RectF {
        let mut x0 = f64::INFINITY;
        let mut y0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for out in &self.outputs {
            let r = out.logical_rect();
            x0 = x0.min(r.x);
            y0 = y0.min(r.y);
            x1 = x1.max(r.right());
            y1 = y1.max(r.bottom());
        }
        if x0.is_finite() {
            RectF::new(x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0))
        } else {
            RectF::new(0.0, 0.0, 1.0, 1.0)
        }
    }

    fn mark_all_dirty(&mut self) {
        for out in &mut self.outputs {
            if let Some(ov) = &mut out.overlay {
                ov.dirty = true;
            }
        }
    }

    fn recompute_windows(&mut self) {
        for out in &mut self.outputs {
            out.windows_px = windows_on_output_with_size(
                &self.snapshot,
                &out.name,
                out.view_w,
                out.view_h,
                out.scale,
            );
        }
    }

    fn update_hover(&mut self) {
        let next = self.pointer_output.and_then(|idx| {
            let out = self.outputs.get(idx)?;
            pick_window_at(
                &out.windows_px,
                self.pointer_local_px.0,
                self.pointer_local_px.1,
            )
            .map(|r| (idx, r))
        });
        if next != self.hovered_window {
            self.hovered_window = next;
            self.mark_all_dirty();
        }
    }

    fn accept(&mut self, selection: FinalSelection) {
        self.accepted = Some(selection);
        self.skip_editor = self.modifiers.shift;
        self.picked_at = Some(Instant::now());
        self.drag = None;
        self.hovered_window = None;
        self.space_held = false;
    }

    fn has_keyboard_focus_on(&self, idx: usize) -> bool {
        let Some(focused) = self.keyboard_surface.as_ref() else {
            return false;
        };
        self.outputs
            .get(idx)
            .and_then(|o| o.overlay.as_ref())
            .is_some_and(|ov| ov.layer.wl_surface() == focused)
    }

    fn editor_output_for(&self, selection: FinalSelection) -> usize {
        match selection {
            FinalSelection::OutputWindow(idx, _) | FinalSelection::Output(idx) => idx,
            FinalSelection::Logical(sel) => {
                let (cx, cy) = (sel.x + sel.w / 2.0, sel.y + sel.h / 2.0);
                self.outputs
                    .iter()
                    .position(|o| {
                        let r = o.logical_rect();
                        cx >= r.x && cx < r.right() && cy >= r.y && cy < r.bottom()
                    })
                    .or(self.pointer_output)
                    .unwrap_or(0)
            }
        }
    }

    fn clear_overlay_transparent(&mut self, idx: usize) -> Result<(), String> {
        let use_viewporter = self.viewporter.is_some();
        let out = &mut self.outputs[idx];
        let Some(ov) = out.overlay.as_mut() else {
            return Ok(());
        };
        let (buf_w, buf_h) = overlay_buffer_size(use_viewporter, out.view_w, out.view_h, ov);
        if !ov.configured || buf_w == 0 || buf_h == 0 {
            return Ok(());
        }
        present(ov, buf_w, buf_h, |surface| {
            let cr =
                cairo::Context::new(surface).map_err(|e| format!("cairo context failed: {e}"))?;
            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            cr.paint().map_err(|e| format!("cairo clear failed: {e}"))?;
            drop(cr);
            surface.flush();
            Ok(())
        })
    }

    fn update_cursor_shape(&mut self, serial: u32) {
        let Some(dev) = &self.cursor_device else {
            return;
        };
        let shape = match self.mode {
            _ if self.editor.is_some() => wp_cursor_shape_device_v1::Shape::Crosshair,
            Mode::Region => wp_cursor_shape_device_v1::Shape::Crosshair,
            Mode::Window => {
                if self.hovered_window.is_some() {
                    wp_cursor_shape_device_v1::Shape::Pointer
                } else {
                    wp_cursor_shape_device_v1::Shape::Default
                }
            }
            Mode::Screen => return,
        };
        if self.last_cursor != Some(shape) {
            dev.set_shape(serial, shape);
            self.last_cursor = Some(shape);
        }
    }

    fn active_drag_size_px(&self, rect_logical: RectF) -> (i32, i32) {
        let mut max_scale = 1.0_f64;
        for out in &self.outputs {
            if rect_logical.intersect(out.logical_rect()).is_some() {
                max_scale = max_scale.max(out.scale);
            }
        }
        (
            (rect_logical.w * max_scale).round() as i32,
            (rect_logical.h * max_scale).round() as i32,
        )
    }

    fn redraw_dirty_overlays(&mut self) -> Result<(), String> {
        // Compute current drag rectangle & which output should carry the "W x H" badge.
        let drag_logical = self.drag.filter(|d| d.dragged).map(|d| d.rect());
        let label_info = drag_logical.map(|sel| {
            let (pw, ph) = self.active_drag_size_px(sel);
            format!("{pw} x {ph}")
        });
        let label_output_idx = drag_logical.and_then(|sel| {
            let br_x = sel.right();
            let br_y = sel.bottom();
            self.outputs
                .iter()
                .position(|o| {
                    let r = o.logical_rect();
                    br_x >= r.x && br_x <= r.right() && br_y >= r.y && br_y <= r.bottom()
                })
                .or(self.pointer_output)
                .or_else(|| {
                    self.outputs
                        .iter()
                        .position(|o| sel.intersect(o.logical_rect()).is_some())
                })
        });

        let use_viewporter = self.viewporter.is_some();

        for (idx, out) in self.outputs.iter_mut().enumerate() {
            let Some(ov) = out.overlay.as_mut() else {
                continue;
            };
            if !ov.configured || !ov.dirty {
                continue;
            }
            ov.dirty = false;

            let (buf_w, buf_h) = overlay_buffer_size(use_viewporter, out.view_w, out.view_h, ov);
            if buf_w == 0 || buf_h == 0 {
                continue;
            }

            let (sx, sy) = (
                buf_w as f64 / ov.logical_w.max(1) as f64,
                buf_h as f64 / ov.logical_h.max(1) as f64,
            );
            let sel_px = if let Some(sel) = drag_logical {
                let x0 = ((sel.x - out.logical_x as f64) * sx).round() as i32;
                let y0 = ((sel.y - out.logical_y as f64) * sy).round() as i32;
                let x1 = ((sel.right() - out.logical_x as f64) * sx).round() as i32;
                let y1 = ((sel.bottom() - out.logical_y as f64) * sy).round() as i32;
                Some(Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0)))
            } else if let Some((win_out, win_rect)) = self.hovered_window {
                if win_out == idx {
                    // Convert output-pixel window rect cleanly if viewporter isn't 1:1 with view_size.
                    if buf_w == out.view_w && buf_h == out.view_h {
                        Some(win_rect)
                    } else {
                        let rx = buf_w as f64 / out.view_w.max(1) as f64;
                        let ry = buf_h as f64 / out.view_h.max(1) as f64;
                        Some(Rect::new(
                            (win_rect.x as f64 * rx).round() as i32,
                            (win_rect.y as f64 * ry).round() as i32,
                            (win_rect.w as f64 * rx).round() as i32,
                            (win_rect.h as f64 * ry).round() as i32,
                        ))
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let frozen_scaled;
            let frozen_ref = match out.frozen.as_ref() {
                Some(f) if f.width() == buf_w as i32 && f.height() == buf_h as i32 => Some(f),
                Some(f) => {
                    let s = cairo::ImageSurface::create(
                        cairo::Format::ARgb32,
                        buf_w as i32,
                        buf_h as i32,
                    )
                    .map_err(|e| format!("cairo surface failed: {e}"))?;
                    let cr = cairo::Context::new(&s)
                        .map_err(|e| format!("cairo context failed: {e}"))?;
                    cr.set_operator(cairo::Operator::Source);
                    cr.scale(
                        buf_w as f64 / f.width().max(1) as f64,
                        buf_h as f64 / f.height().max(1) as f64,
                    );
                    cr.set_source_surface(f, 0.0, 0.0)
                        .map_err(|e| format!("cairo source failed: {e}"))?;
                    cr.paint().map_err(|e| format!("cairo paint failed: {e}"))?;
                    drop(cr);
                    s.flush();
                    frozen_scaled = s;
                    Some(&frozen_scaled)
                }
                None => None,
            };

            let size_label = if label_output_idx == Some(idx) {
                label_info.clone()
            } else {
                None
            };

            let scale = if use_viewporter { out.scale } else { 1.0 };
            let look = &self.look;
            present(ov, buf_w, buf_h, |surface| {
                render_overlay(
                    surface,
                    &OverlayDraw {
                        look,
                        frozen: frozen_ref,
                        selection_px: sel_px,
                        size_label,
                        scale,
                    },
                )
            })?;
        }
        Ok(())
    }

    /// Buffer pixels per logical pixel of the editor's surface.
    fn editor_ui(&self) -> f64 {
        let out = &self.outputs[self.editor_output];
        match (&self.viewporter, &out.overlay) {
            (Some(_), Some(ov)) => out.view_w as f64 / ov.logical_w.max(1) as f64,
            _ => 1.0,
        }
    }

    fn redraw_editor(&mut self) -> Result<(), String> {
        let ui = self.editor_ui();
        let use_viewporter = self.viewporter.is_some();
        let out = &mut self.outputs[self.editor_output];
        let (Some(ov), Some(editor)) = (out.overlay.as_mut(), self.editor.as_mut()) else {
            return Ok(());
        };
        if !ov.configured || !ov.dirty {
            return Ok(());
        }
        ov.dirty = false;
        let (buf_w, buf_h) = overlay_buffer_size(use_viewporter, out.view_w, out.view_h, ov);
        if buf_w == 0 || buf_h == 0 {
            return Ok(());
        }
        editor.set_view(buf_w, buf_h, ui);
        let look = &self.look;
        present(ov, buf_w, buf_h, |surface| editor.render(surface, look))?;

        if let Some(path) = &self.boxes {
            // Flush the commit before writing the dump so watchers see the frame already on the wire.
            let _ = self.conn.flush();
            // from the end of the pick to the editor's first frame
            let since_pick = *self.ready_ms.get_or_insert_with(|| {
                self.picked_at
                    .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0)
            });
            let (ox, oy) = (out.logical_x as f64, out.logical_y as f64);
            let mut dump = format!("editor {since_pick:.1}\n");
            let (img, _) = editor.frame();
            let _ = writeln!(
                dump,
                "image {:.0} {:.0} {:.0} {:.0}",
                ox + img.x / ui,
                oy + img.y / ui,
                img.w / ui,
                img.h / ui
            );
            for (r, hit) in editor.hits() {
                let _ = writeln!(
                    dump,
                    "hit {} {:.0} {:.0} {:.0} {:.0}",
                    hit.name(),
                    ox + r.x / ui,
                    oy + r.y / ui,
                    r.w / ui,
                    r.h / ui
                );
            }
            if let Err(e) = std::fs::write(path, dump) {
                eprintln!("infynitae-shot: cannot write {}: {e}", path.display());
            }
        }
        Ok(())
    }

    fn editor_step(&mut self, step: Result<Step, String>) {
        match step {
            Ok(Step::Nothing) => {}
            Ok(Step::Redraw) => {
                if let Some(ov) = &mut self.outputs[self.editor_output].overlay {
                    ov.dirty = true;
                }
            }
            Ok(Step::Done(finish)) => self.finish = Some(finish),
            Err(e) => self.editor_failed = Some(e),
        }
    }

    fn destroy_overlays_except(&mut self, keep: Option<usize>) {
        for (idx, out) in self.outputs.iter_mut().enumerate() {
            if keep == Some(idx) {
                continue;
            }
            if let Some(ov) = out.overlay.take() {
                if let Some(vp) = ov.viewport {
                    vp.destroy();
                }
                drop(ov.layer);
                drop(ov.buffers);
                drop(ov.pool);
            }
        }
    }

    fn destroy_overlays(&mut self) {
        self.destroy_overlays_except(None);
    }
}

fn overlay_buffer_size(
    use_viewporter: bool,
    view_w: u32,
    view_h: u32,
    ov: &OverlaySurface,
) -> (u32, u32) {
    if use_viewporter {
        (view_w, view_h)
    } else {
        (ov.logical_w, ov.logical_h)
    }
}

/// Draws one frame into a free buffer of `ov` (`buf_w` x `buf_h`) and commits it.
fn present(
    ov: &mut OverlaySurface,
    buf_w: u32,
    buf_h: u32,
    draw: impl FnOnce(&cairo::ImageSurface) -> Result<(), String>,
) -> Result<(), String> {
    let stride = buf_w as i32 * 4;
    let len = buf_h as usize * stride as usize;
    if ov.pool.len() < len * 2 {
        let _ = ov.pool.resize(len * 2);
    }
    if ov.buf_size != (buf_w, buf_h) {
        ov.buffers.clear();
        ov.buf_size = (buf_w, buf_h);
    }

    let mut buffer_slot = None;
    for pos in 0..ov.buffers.len() {
        if ov.buffers[pos].canvas(&mut ov.pool).is_some() {
            buffer_slot = Some(ov.buffers.remove(pos));
            break;
        }
    }
    let buffer = match buffer_slot {
        Some(b) => b,
        None => {
            ov.pool
                .create_buffer(buf_w as i32, buf_h as i32, stride, wl_shm::Format::Argb8888)
                .map_err(|e| format!("shm buffer allocation failed: {e}"))?
                .0
        }
    };

    let slice = buffer
        .canvas(&mut ov.pool)
        .ok_or_else(|| "shm buffer still busy".to_string())?;
    unsafe {
        let surface = cairo::ImageSurface::create_for_data_unsafe(
            slice.as_mut_ptr(),
            cairo::Format::ARgb32,
            buf_w as i32,
            buf_h as i32,
            stride,
        )
        .map_err(|e| format!("cairo image surface failed: {e}"))?;
        draw(&surface)?;
    }

    if let Some(vp) = &ov.viewport {
        vp.set_destination(ov.logical_w as i32, ov.logical_h as i32);
    }
    let surf = ov.layer.wl_surface();
    surf.attach(Some(buffer.wl_buffer()), 0, 0);
    surf.damage_buffer(0, 0, buf_w as i32, buf_h as i32);
    surf.commit();
    ov.buffers.push(buffer);
    Ok(())
}

/// Puts a full-output layer surface that takes the keyboard on output `idx`.
fn create_overlay(
    app: &mut App,
    qh: &QueueHandle<App>,
    layer_shell: &LayerShell,
    idx: usize,
    namespace: &str,
) -> Result<(), String> {
    let wl_surface = app.compositor_state.create_surface(qh);
    let layer = layer_shell.create_layer_surface(
        qh,
        wl_surface,
        Layer::Overlay,
        Some(namespace),
        Some(&app.outputs[idx].wl_output),
    );
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.commit();

    let viewport = app
        .viewporter
        .as_ref()
        .map(|vp| vp.get_viewport(layer.wl_surface(), qh, ()));
    let init_bytes =
        (app.outputs[idx].view_w as usize * app.outputs[idx].view_h as usize * 4).max(4096) * 2;
    let pool = SlotPool::new(init_bytes, &app.shm).map_err(|e| format!("shm pool failed: {e}"))?;

    app.outputs[idx].overlay = Some(OverlaySurface {
        layer,
        viewport,
        pool,
        buffers: Vec::new(),
        buf_size: (0, 0),
        configured: false,
        logical_w: app.outputs[idx].logical_w,
        logical_h: app.outputs[idx].logical_h,
        dirty: true,
    });
    Ok(())
}

fn wait_configured(
    app: &mut App,
    queue: &mut wayland_client::EventQueue<App>,
) -> Result<(), String> {
    while app
        .outputs
        .iter()
        .any(|o| o.overlay.as_ref().is_some_and(|ov| !ov.configured))
    {
        queue
            .blocking_dispatch(app)
            .map_err(|e| format!("wayland dispatch failed waiting for configure: {e}"))?;
    }
    Ok(())
}

/// Shows `capture` in the editor on output `idx` (reusing the picker's layer surface so keyboard
/// focus stays intact from the moment the pick ends) until it is left; returns what to keep.
fn run_editor(
    app: &mut App,
    queue: &mut wayland_client::EventQueue<App>,
    idx: usize,
    capture: cairo::ImageSurface,
) -> Result<Outcome, String> {
    app.editor = Some(Editor::new(capture, &app.look)?);
    app.editor_output = idx;
    app.last_cursor = None;
    if let Some((enter_out, serial)) = app.last_pointer_enter {
        if enter_out == idx {
            app.update_cursor_shape(serial);
        }
    }
    if let Some(ov) = app.outputs[idx].overlay.as_mut() {
        ov.dirty = true;
    } else {
        app.finish = Some(Finish::Original);
    }
    for k in std::mem::take(&mut app.pending_keys) {
        if let Some(editor) = &mut app.editor {
            let step = editor.key(k.keysym, k.utf8.as_deref(), k.ctrl);
            app.editor_step(step);
        }
        if app.finish.is_some() || app.editor_failed.is_some() {
            break;
        }
    }

    if app.finish.is_none()
        && app.editor_failed.is_none()
        && app
            .outputs
            .iter()
            .enumerate()
            .any(|(i, o)| i != idx && o.overlay.is_some())
    {
        // Commit the editor surface first so it claims keyboard focus before other outputs drop.
        if let Err(e) = app.redraw_editor() {
            app.editor_failed = Some(e);
        } else {
            if !app.has_keyboard_focus_on(idx) {
                let _ = queue.roundtrip(app);
            }
            app.destroy_overlays_except(Some(idx));
        }
    }

    while app.finish.is_none() && app.editor_failed.is_none() {
        if let Err(e) = app.redraw_editor() {
            app.editor_failed = Some(e);
            break;
        }
        queue
            .blocking_dispatch(app)
            .map_err(|e| format!("wayland dispatch failed: {e}"))?;
    }
    app.destroy_overlays();
    let _ = queue.roundtrip(app);

    let editor = app
        .editor
        .take()
        .ok_or_else(|| "the editor is gone".to_string())?;
    let marked = match (app.editor_failed.take(), app.finish) {
        (None, Some(Finish::Copy | Finish::Save)) => editor.result(),
        (Some(e), _) => Err(e),
        _ => return Ok(Outcome::Plain(encode_png_bytes(&editor.into_base())?)),
    };
    match (marked, app.finish) {
        (Ok(img), Some(Finish::Save)) => Ok(Outcome::Save(encode_png_bytes(&img)?)),
        (Ok(img), _) => Ok(Outcome::Copy(encode_png_bytes(&img)?)),
        (Err(e), _) => {
            // what was picked is not lost to a drawing error
            eprintln!("infynitae-shot: markup failed ({e}); the capture is kept without marks");
            Ok(Outcome::Plain(encode_png_bytes(&editor.into_base())?))
        }
    }
}

/// Captures `indices` via `zwlr_screencopy_manager_v1` and waits until every job finishes.
fn capture_outputs(
    app: &mut App,
    queue: &mut wayland_client::EventQueue<App>,
    indices: &[usize],
) -> Result<(), String> {
    let mgr = app
        .screencopy
        .as_ref()
        .ok_or_else(|| "compositor does not expose zwlr_screencopy_manager_v1".to_string())?
        .clone();
    let qh = queue.handle();
    app.captures.clear();

    for (job_id, &out_idx) in indices.iter().enumerate() {
        let frame = mgr.capture_output(0, &app.outputs[out_idx].wl_output, &qh, job_id);
        let version = frame.version();
        app.captures.push(CaptureJob {
            output_idx: out_idx,
            _frame: frame,
            version,
            offered: None,
            copied: false,
            y_invert: false,
            pool: None,
            wl_buffer: None,
            status: CaptureStatus::Pending,
        });
    }

    while app
        .captures
        .iter()
        .any(|c| matches!(c.status, CaptureStatus::Pending))
    {
        queue
            .blocking_dispatch(app)
            .map_err(|e| format!("wayland dispatch failed during capture: {e}"))?;
    }

    for job in app.captures.drain(..) {
        match job.status {
            CaptureStatus::Ready(surf) => {
                let vw = surf.width().max(1) as u32;
                let vh = surf.height().max(1) as u32;
                let out = &mut app.outputs[job.output_idx];
                out.view_w = vw;
                out.view_h = vh;
                if out.logical_w > 0 {
                    out.scale = vw as f64 / out.logical_w as f64;
                }
                out.frozen = Some(surf);
            }
            CaptureStatus::Failed(err) => return Err(err),
            CaptureStatus::Pending => return Err("screencopy incomplete".to_string()),
        }
    }
    Ok(())
}

/// Runs `screen`, `region`, or `window` capture/selection against the compositor, then the
/// editor after a pick unless Shift was held.
pub fn run_wayland(mode: Mode, live: bool, look: Look) -> Result<Outcome, String> {
    let snapshot = match mode {
        Mode::Screen => Snapshot::default(),
        Mode::Region | Mode::Window => fetch_snapshot(),
    };

    let conn = Connection::connect_to_env()
        .map_err(|e| format!("failed to connect to Wayland display: {e}"))?;
    let boxes = env::var_os("INFYNITAE_SHOT_BOXES")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    run_wayland_with_conn(&conn, mode, live, look, snapshot, boxes)
}

fn run_wayland_with_conn(
    conn: &Connection,
    mode: Mode,
    live: bool,
    look: Look,
    snapshot: Snapshot,
    boxes: Option<PathBuf>,
) -> Result<Outcome, String> {
    let (globals, mut queue) = registry_queue_init::<App>(conn)
        .map_err(|e| format!("failed to query Wayland globals: {e}"))?;
    let qh = queue.handle();

    let registry_state = RegistryState::new(&globals);
    let output_state = OutputState::new(&globals, &qh);
    let compositor_state =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor missing: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm missing: {e}"))?;
    let seat_state = SeatState::new(&globals, &qh);
    let layer_shell = LayerShell::bind(&globals, &qh).ok();
    let screencopy = globals
        .bind::<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, _, _>(&qh, 1..=3, ())
        .ok();
    let viewporter = globals
        .bind::<wp_viewporter::WpViewporter, _, _>(&qh, 1..=1, ())
        .ok();
    let cursor_shape_mgr = globals
        .bind::<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1, _, _>(&qh, 1..=1, ())
        .ok();

    let mut app = App {
        conn: conn.clone(),
        registry_state,
        output_state,
        compositor_state,
        shm,
        layer_shell,
        screencopy,
        viewporter,
        cursor_shape_mgr,
        seat_state,
        keyboard: None,
        pointer: None,
        cursor_device: None,
        mode,
        look,
        snapshot,
        outputs: Vec::new(),
        captures: Vec::new(),
        pointer_output: None,
        pointer_local_px: (0, 0),
        hovered_window: None,
        drag: None,
        space_held: false,
        last_cursor: None,
        accepted: None,
        cancelled: false,
        modifiers: Modifiers::default(),
        keyboard_surface: None,
        pending_keys: Vec::new(),
        last_pointer_enter: None,
        skip_editor: false,
        picked_at: None,
        ready_ms: None,
        editor: None,
        editor_output: 0,
        finish: None,
        editor_failed: None,
        boxes,
    };

    // Two roundtrips collect all wl_output and xdg_output info events.
    queue
        .roundtrip(&mut app)
        .map_err(|e| format!("wayland roundtrip failed: {e}"))?;
    queue
        .roundtrip(&mut app)
        .map_err(|e| format!("wayland roundtrip failed: {e}"))?;

    for wl_out in app.output_state.outputs() {
        let info = app.output_state.info(&wl_out);
        let name = info
            .as_ref()
            .and_then(|i| i.name.clone())
            .unwrap_or_else(|| format!("out-{}", app.outputs.len()));
        let snap_out = app.snapshot.outputs.iter().find(|o| o.name == name);

        let transform = info
            .as_ref()
            .map(|i| OutputTransform::from_wl(i.transform))
            .unwrap_or_else(|| {
                snap_out
                    .map(|s| OutputTransform::from_snapshot_str(&s.transform))
                    .unwrap_or_default()
            });

        let (phys_w, phys_h) = info
            .as_ref()
            .and_then(|i| {
                i.modes
                    .iter()
                    .find(|m| m.current)
                    .or_else(|| i.modes.first())
                    .map(|m| (m.dimensions.0.max(1) as u32, m.dimensions.1.max(1) as u32))
            })
            .or_else(|| snap_out.map(|s| (s.mode[0].max(1), s.mode[1].max(1))))
            .unwrap_or((1920, 1080));

        let (view_w, view_h) = transform.view_size(phys_w, phys_h);
        let scale = snap_out
            .map(|s| s.scale.max(0.1))
            .or_else(|| info.as_ref().map(|i| i.scale_factor.max(1) as f64))
            .unwrap_or(1.0);

        let (lx, ly) = info
            .as_ref()
            .map(|i| i.location)
            .or_else(|| snap_out.map(|s| (s.position[0], s.position[1])))
            .unwrap_or((0, 0));

        let (lw, lh) = info
            .as_ref()
            .and_then(|i| i.logical_size)
            .map(|(w, h)| (w.max(1) as u32, h.max(1) as u32))
            .unwrap_or_else(|| {
                (
                    (view_w as f64 / scale).round().max(1.0) as u32,
                    (view_h as f64 / scale).round().max(1.0) as u32,
                )
            });

        app.outputs.push(OutputEntry {
            wl_output: wl_out,
            name,
            transform,
            logical_x: lx,
            logical_y: ly,
            logical_w: lw,
            logical_h: lh,
            view_w,
            view_h,
            scale,
            windows_px: Vec::new(),
            frozen: None,
            overlay: None,
        });
    }

    if app.outputs.is_empty() {
        return Err("no Wayland outputs available".to_string());
    }

    let all_indices: Vec<usize> = (0..app.outputs.len()).collect();

    if mode == Mode::Screen {
        // Capture every output immediately at the moment of the call.
        capture_outputs(&mut app, &mut queue, &all_indices)?;

        // When multiple outputs exist, briefly probe with transparent overlays after capture to see
        // if the compositor tells us which output holds the pointer; otherwise use output 0.
        if app.outputs.len() > 1 && app.pointer.is_some() {
            if let Some(layer_shell) = app.layer_shell.take() {
                for idx in 0..app.outputs.len() {
                    let wl_surface = app.compositor_state.create_surface(&qh);
                    let layer = layer_shell.create_layer_surface(
                        &qh,
                        wl_surface,
                        Layer::Overlay,
                        Some("infynitae-shot-probe"),
                        Some(&app.outputs[idx].wl_output),
                    );
                    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
                    layer.set_exclusive_zone(-1);
                    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
                    layer.commit();
                    let pool = SlotPool::new(4096, &app.shm)
                        .map_err(|e| format!("shm pool failed: {e}"))?;
                    app.outputs[idx].overlay = Some(OverlaySurface {
                        layer,
                        viewport: None,
                        pool,
                        buffers: Vec::new(),
                        buf_size: (0, 0),
                        configured: false,
                        logical_w: app.outputs[idx].logical_w,
                        logical_h: app.outputs[idx].logical_h,
                        dirty: false,
                    });
                }
                let _ = queue.roundtrip(&mut app);
                for idx in 0..app.outputs.len() {
                    let (lw, lh) = (
                        app.outputs[idx].logical_w.max(1),
                        app.outputs[idx].logical_h.max(1),
                    );
                    if let Some(ov) = app.outputs[idx].overlay.as_mut() {
                        if let Ok((buf, slice)) =
                            ov.pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888)
                        {
                            slice[..4].fill(0);
                            if let Some(vp_mgr) = &app.viewporter {
                                let vp = vp_mgr.get_viewport(ov.layer.wl_surface(), &qh, ());
                                vp.set_destination(lw as i32, lh as i32);
                                ov.viewport = Some(vp);
                            }
                            let surf = ov.layer.wl_surface();
                            surf.attach(Some(buf.wl_buffer()), 0, 0);
                            surf.damage_buffer(0, 0, 1, 1);
                            surf.commit();
                            ov.buffers.push(buf);
                        }
                    }
                }
                let _ = queue.roundtrip(&mut app);
                app.destroy_overlays();
                let _ = queue.roundtrip(&mut app);
            }
        }

        let chosen = app.pointer_output.unwrap_or(0);
        let surface = app.outputs[chosen]
            .frozen
            .as_ref()
            .ok_or_else(|| "missing capture for chosen output".to_string())?;
        return Ok(Outcome::Plain(encode_png_bytes(surface)?));
    }

    // `region` or `window` mode:
    if !live {
        capture_outputs(&mut app, &mut queue, &all_indices)?;
    }

    let layer_shell = app
        .layer_shell
        .take()
        .ok_or_else(|| "compositor does not expose zwlr_layer_shell_v1".to_string())?;

    for idx in 0..app.outputs.len() {
        create_overlay(&mut app, &qh, &layer_shell, idx, "infynitae-shot")?;
    }
    wait_configured(&mut app, &mut queue)?;

    app.recompute_windows();
    app.mark_all_dirty();

    while app.accepted.is_none() && !app.cancelled {
        app.redraw_dirty_overlays()?;
        queue
            .blocking_dispatch(&mut app)
            .map_err(|e| format!("wayland dispatch failed: {e}"))?;
    }

    if app.cancelled && app.accepted.is_none() {
        app.destroy_overlays();
        let _ = queue.roundtrip(&mut app);
        return Ok(Outcome::Cancelled);
    }

    let selection = app
        .accepted
        .ok_or_else(|| "no selection chosen".to_string())?;
    let editor_output = app.editor_output_for(selection);

    if live {
        let capture_indices: Vec<usize> = match selection {
            FinalSelection::OutputWindow(idx, _) | FinalSelection::Output(idx) => vec![idx],
            FinalSelection::Logical(sel_logical) => (0..app.outputs.len())
                .filter(|&i| {
                    sel_logical
                        .intersect(app.outputs[i].logical_rect())
                        .is_some()
                })
                .collect(),
        };
        if capture_indices.is_empty() {
            app.destroy_overlays();
            let _ = queue.roundtrip(&mut app);
            return Err("selection outside every output".to_string());
        }
        if app.skip_editor {
            app.destroy_overlays();
            let _ = queue.roundtrip(&mut app);
        } else {
            // Clear the kept overlay to transparent so screencopy captures exposed windows while
            // keeping the surface mapped and holding keyboard focus.
            let mut cleared = false;
            if app.outputs.len() > 1 && !app.has_keyboard_focus_on(editor_output) {
                app.clear_overlay_transparent(editor_output)?;
                let _ = queue.roundtrip(&mut app);
                cleared = true;
            }
            app.destroy_overlays_except(Some(editor_output));
            if !cleared {
                app.clear_overlay_transparent(editor_output)?;
            }
            let _ = queue.roundtrip(&mut app);
        }
        capture_outputs(&mut app, &mut queue, &capture_indices)?;
    } else if app.skip_editor {
        app.destroy_overlays();
        let _ = queue.roundtrip(&mut app);
    }

    let result_surface = match selection {
        FinalSelection::Output(out_idx) => app.outputs[out_idx]
            .frozen
            .take()
            .ok_or_else(|| "missing capture for selected output".to_string())?,
        FinalSelection::OutputWindow(out_idx, rect_px) => {
            let src = app.outputs[out_idx]
                .frozen
                .as_ref()
                .ok_or_else(|| "missing capture for selected output".to_string())?;
            crop_upright_surface(src, rect_px)?
        }
        FinalSelection::Logical(sel_logical) => {
            let slices: Vec<CapturedOutputSlice<'_>> = app
                .outputs
                .iter()
                .filter_map(|o| {
                    o.frozen.as_ref().map(|surface| CapturedOutputSlice {
                        logical_rect: o.logical_rect(),
                        scale: o.scale,
                        surface,
                    })
                })
                .collect();
            join_selection_across_outputs(&slices, sel_logical)?
        }
    };

    if app.skip_editor {
        return Ok(Outcome::Plain(encode_png_bytes(&result_surface)?));
    }
    run_editor(&mut app, &mut queue, editor_output, result_surface)
}

fn fetch_snapshot() -> Snapshot {
    let bin = env::var_os("INFYNITAE_BIN")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|dir| dir.join("infynitae")))
                .filter(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("infynitae"));

    let Ok(out) = Command::new(bin).args(["msg", "snapshot"]).output() else {
        return Snapshot::default();
    };
    if !out.status.success() {
        return Snapshot::default();
    }
    let Ok(text) = String::from_utf8(out.stdout) else {
        return Snapshot::default();
    };
    parse_snapshot_json(&text).unwrap_or_default()
}

// --- Screencopy dispatch ---

impl Dispatch<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
        _: zwlr_screencopy_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl App {
    fn start_screencopy_transfer(
        &mut self,
        job_id: usize,
        frame: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        qh: &QueueHandle<Self>,
    ) {
        let Some(job) = self.captures.get_mut(job_id) else {
            return;
        };
        if job.copied {
            return;
        }
        let Some(offered) = &job.offered else {
            job.status = CaptureStatus::Failed("screencopy offered no usable shm format".into());
            return;
        };
        let total = offered.height as usize * offered.stride as usize;
        let Ok(mut pool) = RawPool::new(total, &self.shm) else {
            job.status = CaptureStatus::Failed("screencopy shm pool allocation failed".into());
            return;
        };
        let wl_buffer = pool.create_buffer(
            0,
            offered.width as i32,
            offered.height as i32,
            offered.stride as i32,
            offered.format,
            (),
            qh,
        );
        frame.copy(&wl_buffer);
        job.copied = true;
        job.pool = Some(pool);
        job.wl_buffer = Some(wl_buffer);
    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, usize> for App {
    fn event(
        state: &mut Self,
        frame: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        &job_id: &usize,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let WEnum::Value(fmt) = format else {
                    return;
                };
                if !matches!(fmt, wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888) {
                    return;
                }
                let Some(job) = state.captures.get_mut(job_id) else {
                    return;
                };
                let prefer_new = job.offered.is_none() || fmt == wl_shm::Format::Argb8888;
                if prefer_new {
                    job.offered = Some(OfferedBuffer {
                        format: fmt,
                        width,
                        height,
                        stride,
                    });
                }
                if job.version < 3 {
                    state.start_screencopy_transfer(job_id, frame, qh);
                }
            }
            zwlr_screencopy_frame_v1::Event::BufferDone => {
                state.start_screencopy_transfer(job_id, frame, qh);
            }
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                if let Some(job) = state.captures.get_mut(job_id) {
                    job.y_invert = flags
                        .into_result()
                        .is_ok_and(|f| f.contains(zwlr_screencopy_frame_v1::Flags::YInvert));
                }
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                let Some(job) = state.captures.get_mut(job_id) else {
                    frame.destroy();
                    return;
                };
                let status = (|| -> Result<cairo::ImageSurface, String> {
                    let offered = job
                        .offered
                        .as_ref()
                        .ok_or_else(|| "missing offered screencopy format".to_string())?;
                    let pool = job
                        .pool
                        .as_mut()
                        .ok_or_else(|| "missing screencopy shm pool".to_string())?;
                    let transform = state.outputs[job.output_idx].transform;
                    let force_opaque = offered.format == wl_shm::Format::Xrgb8888;
                    upright_surface_from_panel(&PanelFrame {
                        data: pool.mmap(),
                        width: offered.width,
                        height: offered.height,
                        stride: offered.stride as usize,
                        force_opaque,
                        y_invert: job.y_invert,
                        transform,
                    })
                })();
                if let Some(buf) = job.wl_buffer.take() {
                    buf.destroy();
                }
                job.pool = None;
                job.status = match status {
                    Ok(surf) => CaptureStatus::Ready(surf),
                    Err(e) => CaptureStatus::Failed(e),
                };
                frame.destroy();
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                if let Some(job) = state.captures.get_mut(job_id) {
                    if let Some(buf) = job.wl_buffer.take() {
                        buf.destroy();
                    }
                    job.pool = None;
                    job.status = CaptureStatus::Failed("screencopy failed".to_string());
                }
                frame.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_viewporter::WpViewporter, ()> for App {
    fn event(
        _: &mut Self,
        _: &wp_viewporter::WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_viewport::WpViewport, ()> for App {
    fn event(
        _: &mut Self,
        _: &wp_viewport::WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
        _: wp_cursor_shape_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_cursor_shape_device_v1::WpCursorShapeDeviceV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &wp_cursor_shape_device_v1::WpCursorShapeDeviceV1,
        _: wp_cursor_shape_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// --- SCTK handlers ---

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if self.editor.is_some() {
            if self.outputs[self.editor_output]
                .overlay
                .as_ref()
                .is_some_and(|ov| ov.layer.wl_surface() == layer.wl_surface())
            {
                self.finish = Some(Finish::Original);
            }
        } else if self.accepted.is_none() {
            self.cancelled = true;
        } else if let Some(idx) = self.find_output_by_surface(layer.wl_surface()) {
            if let Some(ov) = self.outputs[idx].overlay.take() {
                if let Some(vp) = ov.viewport {
                    vp.destroy();
                }
                drop(ov.layer);
            }
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(idx) = self.find_output_by_surface(layer.wl_surface()) else {
            return;
        };
        let out = &mut self.outputs[idx];
        if configure.new_size.0 > 0 {
            out.logical_w = configure.new_size.0;
        }
        if configure.new_size.1 > 0 {
            out.logical_h = configure.new_size.1;
        }
        if out.logical_w > 0 {
            out.scale = out.view_w as f64 / out.logical_w as f64;
        }
        if let Some(ov) = &mut out.overlay {
            if ov.logical_w != out.logical_w || ov.logical_h != out.logical_h {
                ov.buffers.clear();
            }
            ov.logical_w = out.logical_w;
            ov.logical_h = out.logical_h;
            ov.configured = true;
            ov.dirty = true;
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Ok(ptr) = self.seat_state.get_pointer(qh, &seat) {
                if let Some(mgr) = &self.cursor_shape_mgr {
                    self.cursor_device = Some(mgr.get_pointer(&ptr, qh, ()));
                }
                self.pointer = Some(ptr);
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(kbd) = self.keyboard.take() {
                kbd.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(dev) = self.cursor_device.take() {
                dev.destroy();
            }
            if let Some(ptr) = self.pointer.take() {
                ptr.release();
            }
            self.last_cursor = None;
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.keyboard_surface = Some(surface.clone());
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.keyboard_surface.as_ref() == Some(surface) {
            self.keyboard_surface = None;
        }
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if let Some(editor) = &mut self.editor {
            let step = editor.key(event.keysym, event.utf8.as_deref(), self.modifiers.ctrl);
            self.editor_step(step);
            return;
        }
        if self.accepted.is_some() {
            if !self.skip_editor {
                self.pending_keys.push(PendingKey {
                    keysym: event.keysym,
                    utf8: event.utf8,
                    ctrl: self.modifiers.ctrl,
                });
            }
            return;
        }
        if self.cancelled {
            return;
        }
        match event.keysym {
            Keysym::a | Keysym::A if self.modifiers.ctrl => {
                let idx = self.pointer_output.unwrap_or(0);
                self.accept(FinalSelection::Output(idx));
            }
            Keysym::Escape => {
                self.cancelled = true;
            }
            Keysym::Return | Keysym::KP_Enter => {
                if self.mode == Mode::Region {
                    if let Some(drag) = self.drag.filter(|d| d.dragged) {
                        let rect = drag.rect();
                        let (pw, ph) = self.active_drag_size_px(rect);
                        if pw > 0 && ph > 0 {
                            self.accept(FinalSelection::Logical(rect));
                            return;
                        }
                    }
                }
                if let Some((out_idx, win_rect)) = self.hovered_window {
                    self.accept(FinalSelection::OutputWindow(out_idx, win_rect));
                }
            }
            Keysym::space => {
                self.space_held = true;
                if let Some(drag) = &mut self.drag {
                    drag.space_held = true;
                }
            }
            _ => {}
        }
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if event.keysym == Keysym::space {
            self.space_held = false;
            if let Some(drag) = &mut self.drag {
                drag.space_held = false;
            }
        }
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            let Some(out_idx) = self.find_output_by_surface(&ev.surface) else {
                continue;
            };
            match ev.kind {
                PointerEventKind::Enter { serial } => {
                    self.last_pointer_enter = Some((out_idx, serial));
                }
                PointerEventKind::Leave { .. }
                    if self
                        .last_pointer_enter
                        .is_some_and(|(idx, _)| idx == out_idx) =>
                {
                    self.last_pointer_enter = None;
                }
                _ => {}
            }
            if self.editor.is_some() {
                self.editor_pointer(ev);
                continue;
            }
            if self.accepted.is_some() || self.cancelled {
                continue;
            }
            let (gx, gy, ox, oy) = {
                let out = &self.outputs[out_idx];
                let (sx, sy) = out.scale_xy();
                (
                    out.logical_x as f64 + ev.position.0,
                    out.logical_y as f64 + ev.position.1,
                    (ev.position.0 * sx).round() as i32,
                    (ev.position.1 * sy).round() as i32,
                )
            };

            match ev.kind {
                PointerEventKind::Enter { serial } => {
                    self.pointer_output = Some(out_idx);
                    self.pointer_local_px = (ox, oy);
                    self.update_hover();
                    self.update_cursor_shape(serial);
                }
                PointerEventKind::Leave { .. } => {
                    if self.pointer_output == Some(out_idx) && self.drag.is_none() {
                        self.pointer_output = None;
                        self.hovered_window = None;
                        self.mark_all_dirty();
                    }
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_output = Some(out_idx);
                    self.pointer_local_px = (ox, oy);
                    let bounds = self.virtual_bounds();
                    if let Some(drag) = &mut self.drag {
                        drag.update(gx, gy, bounds);
                        self.mark_all_dirty();
                    } else {
                        self.update_hover();
                    }
                }
                PointerEventKind::Press { button, .. } => {
                    self.pointer_output = Some(out_idx);
                    self.pointer_local_px = (ox, oy);
                    if button == BTN_RIGHT {
                        self.cancelled = true;
                        return;
                    }
                    if button == BTN_LEFT && self.mode == Mode::Region {
                        let mut drag = DragSelection::begin(gx, gy);
                        drag.space_held = self.space_held;
                        self.drag = Some(drag);
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    self.pointer_output = Some(out_idx);
                    self.pointer_local_px = (ox, oy);
                    if button != BTN_LEFT {
                        continue;
                    }
                    match self.mode {
                        Mode::Region => {
                            let Some(drag) = self.drag.take() else {
                                continue;
                            };
                            if drag.dragged {
                                let rect = drag.rect();
                                let (pw, ph) = self.active_drag_size_px(rect);
                                if pw > 0 && ph > 0 {
                                    self.accept(FinalSelection::Logical(rect));
                                    return;
                                }
                            } else {
                                self.update_hover();
                                if let Some((win_out, win_rect)) = self.hovered_window {
                                    self.accept(FinalSelection::OutputWindow(win_out, win_rect));
                                    return;
                                }
                            }
                            self.mark_all_dirty();
                        }
                        Mode::Window => {
                            self.update_hover();
                            if let Some((win_out, win_rect)) = self.hovered_window {
                                self.accept(FinalSelection::OutputWindow(win_out, win_rect));
                                return;
                            }
                        }
                        Mode::Screen => {}
                    }
                }
                PointerEventKind::Axis { .. } => {}
            }
        }
    }
}

impl App {
    fn editor_pointer(&mut self, ev: &PointerEvent) {
        let ui = self.editor_ui();
        let (x, y) = (ev.position.0 * ui, ev.position.1 * ui);
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let step = match ev.kind {
            PointerEventKind::Enter { serial } => {
                self.update_cursor_shape(serial);
                return;
            }
            PointerEventKind::Motion { .. } => Ok(editor.motion(x, y)),
            PointerEventKind::Press { button, .. } if button == BTN_LEFT => editor.press(x, y),
            PointerEventKind::Release { button, .. } if button == BTN_LEFT => editor.release(x, y),
            _ => return,
        };
        self.editor_step(step);
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_layer!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_registry!(App);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use crate::look::Color;

    // Minimal US XKB keymap covering the keys used by the picker and editor tests.
    // Evdev keycodes are XKB keycodes minus 8.
    const TEST_KEYMAP: &[u8] = b"xkb_keymap {
\txkb_keycodes \"(unnamed)\" {
\t\tminimum = 8;
\t\tmaximum = 255;
\t\t<ESC> = 9;
\t\t<RTRN> = 36;
\t\t<LCTL> = 37;
\t\t<LFSH> = 50;
\t\t<AE01> = 10;
\t\t<AE02> = 11;
\t\t<AE03> = 12;
\t\t<AC01> = 38;
\t\t<AC02> = 39;
\t\t<AB01> = 52;
\t\t<AB03> = 54;
\t\t<AB05> = 56;
\t};
\txkb_types \"(unnamed)\" {
\t\tvirtual_modifiers NumLock;
\t\ttype \"ONE_LEVEL\" {
\t\t\tmodifiers = none;
\t\t};
\t\ttype \"ALPHABETIC\" {
\t\t\tmodifiers = Shift;
\t\t\tmap[Shift] = Level2;
\t\t\tlevel_name[Level1] = \"Base\";
\t\t\tlevel_name[Level2] = \"Caps\";
\t\t};
\t\ttype \"TWO_LEVEL\" {
\t\t\tmodifiers = Shift;
\t\t\tmap[Shift] = Level2;
\t\t\tlevel_name[Level1] = \"Base\";
\t\t\tlevel_name[Level2] = \"Shift\";
\t\t};
\t};
\txkb_compatibility \"(unnamed)\" {
\t\tinterpret VoidSymbol { action = NoAction(); };
\t};
\txkb_symbols \"(unnamed)\" {
\t\tkey <ESC>  { [ Escape ] };
\t\tkey <RTRN> { [ Return ] };
\t\tkey <LCTL> { [ Control_L ] };
\t\tkey <LFSH> { [ Shift_L ] };
\t\tkey <AE01> { [ 1, exclam ] };
\t\tkey <AE02> { [ 2, at ] };
\t\tkey <AE03> { [ 3, numbersign ] };
\t\tkey <AC01> { [ a, A ] };
\t\tkey <AC02> { [ s, S ] };
\t\tkey <AB01> { [ z, Z ] };
\t\tkey <AB03> { [ c, C ] };
\t\tkey <AB05> { [ b, B ] };
\t\tmodifier_map Control { <LCTL> };
\t\tmodifier_map Shift { <LFSH> };
\t};
};
\0";

    const KEY_ESC: u32 = 1;
    const KEY_S: u32 = 31;
    const KEY_B: u32 = 48;
    const KEY_LCTL: u32 = 29;
    const KEY_BTN_LEFT: u32 = 0x110;

    #[derive(Clone)]
    struct MockOutputSpec {
        width: i32,
        height: i32,
        x: i32,
        y: i32,
        name: &'static str,
    }

    #[derive(Clone)]
    struct ShmPoolInfo {
        fd: Arc<std::fs::File>,
        size: usize,
    }

    #[derive(Clone, Copy)]
    struct ShmBufInfo {
        pool_id: u32,
        offset: usize,
        width: i32,
        height: i32,
        stride: usize,
    }

    #[derive(Default)]
    struct LayerInfo {
        surface_id: u32,
        output_idx: usize,
        kb_interactivity: u32,
        configured: bool,
        mapped: bool,
        destroyed: bool,
    }

    #[derive(Default)]
    struct MockServerState {
        next_serial: u32,
        output_globals: HashMap<usize, u32>,
        output_obj_to_idx: HashMap<u32, usize>,
        pointer_id: Option<u32>,
        keyboard_id: Option<u32>,
        layers: HashMap<u32, LayerInfo>,
        surface_to_layer: HashMap<u32, u32>,
        pending_buf: HashMap<u32, Option<u32>>,
        shm_pools: HashMap<u32, ShmPoolInfo>,
        shm_bufs: HashMap<u32, ShmBufInfo>,
        focused_surface: Option<u32>,
        pointer_surface: Option<u32>,
        layer_creates: usize,
        layer_destroys: usize,
        layer_configures_sent: usize,
        focus_losses: usize,
        dropped_keys: Vec<u32>,
        cursor_shapes: Vec<u32>,
        screen_bg: [u8; 4],
        last_overlay_center_alpha: Option<u8>,
        /// Models compositor scheduling latency before a newly created 2nd layer surface gets
        /// `wl_keyboard.enter` after mapping.
        defer_second_surface_focus: bool,
        deferred_focus_surface: Option<u32>,
    }

    struct MockWayland {
        server_sock: UnixStream,
        shared: Arc<(Mutex<MockServerState>, Condvar)>,
        join: Option<std::thread::JoinHandle<()>>,
    }

    impl MockWayland {
        fn start(specs: Vec<MockOutputSpec>) -> (Self, Connection) {
            let (client_sock, server_sock) = UnixStream::pair().unwrap();
            let write_sock = server_sock.try_clone().unwrap();
            let conn = Connection::from_socket(client_sock).unwrap();
            let state = MockServerState {
                next_serial: 10,
                // Solid dark-blue background in ARGB8888 (BGRA bytes: B=200, G=100, R=50, A=255)
                screen_bg: [200, 100, 50, 255],
                ..Default::default()
            };
            let shared = Arc::new((Mutex::new(state), Condvar::new()));
            let st_thread = Arc::clone(&shared);
            let join = std::thread::spawn(move || {
                Self::serve(server_sock, specs, st_thread);
            });
            (
                Self {
                    server_sock: write_sock,
                    shared,
                    join: Some(join),
                },
                conn,
            )
        }

        fn push_u32(buf: &mut Vec<u8>, v: u32) {
            buf.extend_from_slice(&v.to_ne_bytes());
        }

        fn push_i32(buf: &mut Vec<u8>, v: i32) {
            buf.extend_from_slice(&v.to_ne_bytes());
        }

        fn push_str(buf: &mut Vec<u8>, s: &str) {
            let bytes = s.as_bytes();
            let len = bytes.len() + 1;
            Self::push_u32(buf, len as u32);
            buf.extend_from_slice(bytes);
            buf.push(0);
            while !buf.len().is_multiple_of(4) {
                buf.push(0);
            }
        }

        fn push_array(buf: &mut Vec<u8>, data: &[u8]) {
            Self::push_u32(buf, data.len() as u32);
            buf.extend_from_slice(data);
            while !buf.len().is_multiple_of(4) {
                buf.push(0);
            }
        }

        fn msg(obj: u32, opcode: u16, body: &[u8]) -> Vec<u8> {
            let size = (8 + body.len()) as u32;
            let mut out = Vec::with_capacity(size as usize);
            out.extend_from_slice(&obj.to_ne_bytes());
            out.extend_from_slice(&((size << 16) | u32::from(opcode)).to_ne_bytes());
            out.extend_from_slice(body);
            out
        }

        fn write_all(sock: &UnixStream, data: &[u8]) {
            let mut s = sock;
            let _ = s.write_all(data);
        }

        fn send_with_fd(sock: &UnixStream, data: &[u8], fd: i32) {
            let mut iov = libc::iovec {
                iov_base: data.as_ptr() as *mut libc::c_void,
                iov_len: data.len(),
            };
            let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32) } as usize;
            let mut cmsg_buf = vec![0u8; space];
            let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = cmsg_buf.as_mut_ptr().cast();
            msg.msg_controllen = space;
            unsafe {
                let cmsg = libc::CMSG_FIRSTHDR(&msg);
                (*cmsg).cmsg_level = libc::SOL_SOCKET;
                (*cmsg).cmsg_type = libc::SCM_RIGHTS;
                (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as u32) as usize;
                std::ptr::copy_nonoverlapping(
                    &fd as *const i32 as *const u8,
                    libc::CMSG_DATA(cmsg),
                    std::mem::size_of::<i32>(),
                );
                libc::sendmsg(sock.as_raw_fd(), &msg, 0);
            }
        }

        fn recv_chunk(sock: &UnixStream, out: &mut Vec<u8>, fds: &mut Vec<std::fs::File>) -> bool {
            use std::os::fd::FromRawFd;
            let mut buf = [0u8; 4096];
            let mut cmsg_buf = [0u8; 256];
            let mut iov = libc::iovec {
                iov_base: buf.as_mut_ptr().cast(),
                iov_len: buf.len(),
            };
            let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = cmsg_buf.as_mut_ptr().cast();
            msg.msg_controllen = cmsg_buf.len();
            let n = unsafe { libc::recvmsg(sock.as_raw_fd(), &mut msg, 0) };
            if n <= 0 {
                return false;
            }
            unsafe {
                let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
                while !cmsg.is_null() {
                    if (*cmsg).cmsg_level == libc::SOL_SOCKET
                        && (*cmsg).cmsg_type == libc::SCM_RIGHTS
                    {
                        let data_ptr = libc::CMSG_DATA(cmsg);
                        let data_len = (*cmsg).cmsg_len - libc::CMSG_LEN(0) as usize;
                        for off in (0..data_len).step_by(4) {
                            let mut fd_bytes = [0u8; 4];
                            std::ptr::copy_nonoverlapping(
                                data_ptr.add(off),
                                fd_bytes.as_mut_ptr(),
                                4,
                            );
                            let fd = i32::from_ne_bytes(fd_bytes);
                            fds.push(std::fs::File::from_raw_fd(fd));
                        }
                    }
                    cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
                }
            }
            out.extend_from_slice(&buf[..n as usize]);
            true
        }

        fn read_u32(b: &[u8], off: &mut usize) -> u32 {
            let v = u32::from_ne_bytes(b[*off..*off + 4].try_into().unwrap());
            *off += 4;
            v
        }

        fn read_i32(b: &[u8], off: &mut usize) -> i32 {
            let v = i32::from_ne_bytes(b[*off..*off + 4].try_into().unwrap());
            *off += 4;
            v
        }

        fn read_str(b: &[u8], off: &mut usize) -> String {
            let len = Self::read_u32(b, off) as usize;
            let s = String::from_utf8_lossy(&b[*off..*off + len.saturating_sub(1)]).into_owned();
            let padded = (len + 3) & !3;
            *off += padded;
            s
        }

        fn make_keymap_file() -> std::fs::File {
            let path = std::env::temp_dir().join(format!(
                "shot-rs-test-keymap-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_file(&path);
            let mut f = std::fs::File::options()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            let _ = std::fs::remove_file(&path);
            f.write_all(TEST_KEYMAP).unwrap();
            f.flush().unwrap();
            f.seek(SeekFrom::Start(0)).unwrap();
            f
        }

        fn push_keyboard_leave(st: &mut MockServerState, sid: u32, out_bytes: &mut Vec<u8>) {
            if let Some(kid) = st.keyboard_id {
                st.next_serial += 1;
                let mut lb = Vec::new();
                Self::push_u32(&mut lb, st.next_serial);
                Self::push_u32(&mut lb, sid);
                out_bytes.extend(Self::msg(kid, 2, &lb));
            }
        }

        fn push_keyboard_enter(st: &mut MockServerState, sid: u32, out_bytes: &mut Vec<u8>) {
            if let Some(kid) = st.keyboard_id {
                st.next_serial += 1;
                let s1 = st.next_serial;
                let mut eb = Vec::new();
                Self::push_u32(&mut eb, s1);
                Self::push_u32(&mut eb, sid);
                Self::push_array(&mut eb, &[]);
                out_bytes.extend(Self::msg(kid, 1, &eb));
                st.next_serial += 1;
                let mut mb = Vec::new();
                Self::push_u32(&mut mb, st.next_serial);
                Self::push_u32(&mut mb, 0);
                Self::push_u32(&mut mb, 0);
                Self::push_u32(&mut mb, 0);
                Self::push_u32(&mut mb, 0);
                out_bytes.extend(Self::msg(kid, 4, &mb));
            }
        }

        fn release_focus_for(st: &mut MockServerState, sid: u32, out_bytes: &mut Vec<u8>) {
            if st.focused_surface == Some(sid) {
                st.focused_surface = None;
                st.focus_losses += 1;
                Self::push_keyboard_leave(st, sid, out_bytes);
            }
        }

        fn serve(
            sock: UnixStream,
            specs: Vec<MockOutputSpec>,
            shared: Arc<(Mutex<MockServerState>, Condvar)>,
        ) {
            #[derive(Clone, Copy)]
            enum Obj {
                Display,
                Registry,
                Compositor,
                Shm,
                ShmPool,
                Buffer,
                LayerShell,
                LayerSurface,
                Surface,
                Seat,
                Pointer,
                Keyboard,
                Output,
                Viewporter,
                Viewport,
                CursorShapeMgr,
                CursorShapeDevice,
                ScreencopyMgr,
                ScreencopyFrame,
            }
            let mut objs: HashMap<u32, Obj> = HashMap::new();
            objs.insert(1, Obj::Display);

            let mut globals: Vec<(u32, &'static str, u32)> = vec![
                (1, "wl_compositor", 6),
                (2, "wl_shm", 1),
                (3, "wl_seat", 8),
                (4, "zwlr_layer_shell_v1", 4),
                (5, "wp_viewporter", 1),
                (6, "wp_cursor_shape_manager_v1", 1),
                (7, "zwlr_screencopy_manager_v1", 3),
            ];
            let out_base = 10u32;
            for idx in 0..specs.len() {
                globals.push((out_base + idx as u32, "wl_output", 4));
            }

            let mut pending = Vec::<u8>::new();
            let mut fds = Vec::<std::fs::File>::new();
            loop {
                if !Self::recv_chunk(&sock, &mut pending, &mut fds) {
                    return;
                }
                while pending.len() >= 8 {
                    let obj_id = u32::from_ne_bytes(pending[0..4].try_into().unwrap());
                    let word = u32::from_ne_bytes(pending[4..8].try_into().unwrap());
                    let size = (word >> 16) as usize;
                    let opcode = (word & 0xffff) as u16;
                    if size < 8 || pending.len() < size {
                        break;
                    }
                    let frame = pending.drain(..size).collect::<Vec<u8>>();
                    let body = &frame[8..];
                    let mut off = 0usize;
                    let mut out_bytes = Vec::new();
                    match objs.get(&obj_id).copied() {
                        Some(Obj::Display) => match opcode {
                            0 => {
                                let cb = Self::read_u32(body, &mut off);
                                let mut b = Vec::new();
                                Self::push_u32(&mut b, 0);
                                out_bytes.extend(Self::msg(cb, 0, &b));
                                let mut d = Vec::new();
                                Self::push_u32(&mut d, cb);
                                out_bytes.extend(Self::msg(1, 1, &d));
                            }
                            1 => {
                                let reg = Self::read_u32(body, &mut off);
                                objs.insert(reg, Obj::Registry);
                                for &(name, iface, ver) in &globals {
                                    let mut b = Vec::new();
                                    Self::push_u32(&mut b, name);
                                    Self::push_str(&mut b, iface);
                                    Self::push_u32(&mut b, ver);
                                    out_bytes.extend(Self::msg(reg, 0, &b));
                                }
                            }
                            _ => {}
                        },
                        Some(Obj::Registry) if opcode == 0 => {
                            let name = Self::read_u32(body, &mut off);
                            let iface = Self::read_str(body, &mut off);
                            let _ver = Self::read_u32(body, &mut off);
                            let new_id = Self::read_u32(body, &mut off);
                            match iface.as_str() {
                                "wl_compositor" => {
                                    objs.insert(new_id, Obj::Compositor);
                                }
                                "wl_shm" => {
                                    objs.insert(new_id, Obj::Shm);
                                    for fmt in [0u32, 1u32] {
                                        let mut b = Vec::new();
                                        Self::push_u32(&mut b, fmt);
                                        out_bytes.extend(Self::msg(new_id, 0, &b));
                                    }
                                }
                                "wl_seat" => {
                                    objs.insert(new_id, Obj::Seat);
                                    let mut b = Vec::new();
                                    Self::push_u32(&mut b, 3); // pointer | keyboard
                                    out_bytes.extend(Self::msg(new_id, 0, &b));
                                    let mut nb = Vec::new();
                                    Self::push_str(&mut nb, "seat0");
                                    out_bytes.extend(Self::msg(new_id, 1, &nb));
                                }
                                "zwlr_layer_shell_v1" => {
                                    objs.insert(new_id, Obj::LayerShell);
                                }
                                "wp_viewporter" => {
                                    objs.insert(new_id, Obj::Viewporter);
                                }
                                "wp_cursor_shape_manager_v1" => {
                                    objs.insert(new_id, Obj::CursorShapeMgr);
                                }
                                "zwlr_screencopy_manager_v1" => {
                                    objs.insert(new_id, Obj::ScreencopyMgr);
                                }
                                "wl_output" => {
                                    let out_idx = (name - out_base) as usize;
                                    objs.insert(new_id, Obj::Output);
                                    let mut st = shared.0.lock().unwrap();
                                    st.output_globals.insert(out_idx, new_id);
                                    st.output_obj_to_idx.insert(new_id, out_idx);
                                    let spec = &specs[out_idx];
                                    let mut gb = Vec::new();
                                    Self::push_i32(&mut gb, spec.x);
                                    Self::push_i32(&mut gb, spec.y);
                                    Self::push_i32(&mut gb, spec.width);
                                    Self::push_i32(&mut gb, spec.height);
                                    Self::push_i32(&mut gb, 0);
                                    Self::push_str(&mut gb, "Mock");
                                    Self::push_str(&mut gb, spec.name);
                                    Self::push_i32(&mut gb, 0);
                                    out_bytes.extend(Self::msg(new_id, 0, &gb));
                                    let mut mb = Vec::new();
                                    Self::push_u32(&mut mb, 3);
                                    Self::push_i32(&mut mb, spec.width);
                                    Self::push_i32(&mut mb, spec.height);
                                    Self::push_i32(&mut mb, 60_000);
                                    out_bytes.extend(Self::msg(new_id, 1, &mb));
                                    let mut sb = Vec::new();
                                    Self::push_i32(&mut sb, 1);
                                    out_bytes.extend(Self::msg(new_id, 3, &sb));
                                    let mut nb = Vec::new();
                                    Self::push_str(&mut nb, spec.name);
                                    out_bytes.extend(Self::msg(new_id, 4, &nb));
                                    out_bytes.extend(Self::msg(new_id, 2, &[]));
                                    shared.1.notify_all();
                                }
                                _ => {}
                            }
                        }
                        Some(Obj::Compositor) if opcode == 0 => {
                            let sid = Self::read_u32(body, &mut off);
                            objs.insert(sid, Obj::Surface);
                        }
                        Some(Obj::Shm) if opcode == 0 => {
                            let pid = Self::read_u32(body, &mut off);
                            let size = Self::read_i32(body, &mut off) as usize;
                            objs.insert(pid, Obj::ShmPool);
                            let fd = Arc::new(fds.remove(0));
                            shared
                                .0
                                .lock()
                                .unwrap()
                                .shm_pools
                                .insert(pid, ShmPoolInfo { fd, size });
                        }
                        Some(Obj::ShmPool) => match opcode {
                            0 => {
                                let bid = Self::read_u32(body, &mut off);
                                let offset = Self::read_i32(body, &mut off) as usize;
                                let width = Self::read_i32(body, &mut off);
                                let height = Self::read_i32(body, &mut off);
                                let stride = Self::read_i32(body, &mut off) as usize;
                                objs.insert(bid, Obj::Buffer);
                                shared.0.lock().unwrap().shm_bufs.insert(
                                    bid,
                                    ShmBufInfo {
                                        pool_id: obj_id,
                                        offset,
                                        width,
                                        height,
                                        stride,
                                    },
                                );
                            }
                            2 => {
                                let size = Self::read_i32(body, &mut off) as usize;
                                if let Some(p) = shared.0.lock().unwrap().shm_pools.get_mut(&obj_id)
                                {
                                    p.size = size;
                                }
                            }
                            _ => {}
                        },
                        Some(Obj::LayerShell) if opcode == 0 => {
                            let lid = Self::read_u32(body, &mut off);
                            let sid = Self::read_u32(body, &mut off);
                            let out_obj = Self::read_u32(body, &mut off);
                            let _layer = Self::read_u32(body, &mut off);
                            let _namespace = Self::read_str(body, &mut off);
                            objs.insert(lid, Obj::LayerSurface);
                            let mut st = shared.0.lock().unwrap();
                            let out_idx = *st.output_obj_to_idx.get(&out_obj).unwrap_or(&0);
                            st.layer_creates += 1;
                            st.layers.insert(
                                lid,
                                LayerInfo {
                                    surface_id: sid,
                                    output_idx: out_idx,
                                    ..Default::default()
                                },
                            );
                            st.surface_to_layer.insert(sid, lid);
                            shared.1.notify_all();
                        }
                        Some(Obj::LayerSurface) => match opcode {
                            4 => {
                                let kb = Self::read_u32(body, &mut off);
                                let mut st = shared.0.lock().unwrap();
                                if let Some(info) = st.layers.get_mut(&obj_id) {
                                    info.kb_interactivity = kb;
                                }
                            }
                            7 => {
                                let mut st = shared.0.lock().unwrap();
                                if let Some(info) = st.layers.get_mut(&obj_id) {
                                    info.destroyed = true;
                                    info.mapped = false;
                                    let sid = info.surface_id;
                                    st.layer_destroys += 1;
                                    Self::release_focus_for(&mut st, sid, &mut out_bytes);
                                    shared.1.notify_all();
                                }
                            }
                            _ => {}
                        },
                        Some(Obj::Viewporter) if opcode == 1 => {
                            let vid = Self::read_u32(body, &mut off);
                            objs.insert(vid, Obj::Viewport);
                        }
                        Some(Obj::CursorShapeMgr) if opcode == 1 => {
                            let dev_id = Self::read_u32(body, &mut off);
                            objs.insert(dev_id, Obj::CursorShapeDevice);
                        }
                        Some(Obj::CursorShapeDevice) if opcode == 1 => {
                            let _serial = Self::read_u32(body, &mut off);
                            let shape = Self::read_u32(body, &mut off);
                            let mut st = shared.0.lock().unwrap();
                            st.cursor_shapes.push(shape);
                            shared.1.notify_all();
                        }
                        Some(Obj::Seat) => match opcode {
                            0 => {
                                let pid = Self::read_u32(body, &mut off);
                                objs.insert(pid, Obj::Pointer);
                                let mut st = shared.0.lock().unwrap();
                                st.pointer_id = Some(pid);
                                shared.1.notify_all();
                            }
                            1 => {
                                let kid = Self::read_u32(body, &mut off);
                                objs.insert(kid, Obj::Keyboard);
                                {
                                    let mut st = shared.0.lock().unwrap();
                                    st.keyboard_id = Some(kid);
                                    shared.1.notify_all();
                                }
                                let keymap_file = Self::make_keymap_file();
                                let mut kb = Vec::new();
                                Self::push_u32(&mut kb, 1); // XkbV1
                                Self::push_u32(&mut kb, TEST_KEYMAP.len() as u32);
                                Self::send_with_fd(
                                    &sock,
                                    &Self::msg(kid, 0, &kb),
                                    keymap_file.as_raw_fd(),
                                );
                            }
                            _ => {}
                        },
                        Some(Obj::ScreencopyMgr) if opcode == 0 => {
                            let fid = Self::read_u32(body, &mut off);
                            let _overlay_cursor = Self::read_i32(body, &mut off);
                            let out_obj = Self::read_u32(body, &mut off);
                            objs.insert(fid, Obj::ScreencopyFrame);
                            let out_idx = *shared
                                .0
                                .lock()
                                .unwrap()
                                .output_obj_to_idx
                                .get(&out_obj)
                                .unwrap_or(&0);
                            let spec = &specs[out_idx];
                            // wl_shm::Format::Argb8888 = 0
                            let mut b = Vec::new();
                            Self::push_u32(&mut b, 0);
                            Self::push_u32(&mut b, spec.width as u32);
                            Self::push_u32(&mut b, spec.height as u32);
                            Self::push_u32(&mut b, spec.width as u32 * 4);
                            out_bytes.extend(Self::msg(fid, 0, &b));
                            out_bytes.extend(Self::msg(fid, 6, &[])); // buffer_done
                        }
                        Some(Obj::ScreencopyFrame) if opcode == 0 => {
                            let bid = Self::read_u32(body, &mut off);
                            let st = shared.0.lock().unwrap();
                            let buf = st.shm_bufs.get(&bid).copied().unwrap();
                            let pool = st.shm_pools.get(&buf.pool_id).unwrap().clone();
                            let bg = st.screen_bg;
                            let overlay_alpha = st.last_overlay_center_alpha;
                            drop(st);
                            let mut f = pool.fd.try_clone().unwrap();
                            let total = buf.height as usize * buf.stride;
                            let mut bytes = vec![0u8; total];
                            // Simulate compositing: background + any mapped overlay pixel alpha.
                            let pixel = if let Some(a) = overlay_alpha.filter(|&a| a > 0) {
                                // Overlay wasn't transparent: blend a 40% tint marker so tests catch it.
                                let k = 255 - a as u32;
                                [
                                    ((bg[0] as u32 * k) / 255) as u8,
                                    ((bg[1] as u32 * k) / 255) as u8,
                                    ((bg[2] as u32 * k) / 255) as u8,
                                    255,
                                ]
                            } else {
                                bg
                            };
                            for px in bytes.as_chunks_mut::<4>().0 {
                                px.copy_from_slice(&pixel);
                            }
                            f.seek(SeekFrom::Start(buf.offset as u64)).unwrap();
                            f.write_all(&bytes).unwrap();
                            f.flush().unwrap();
                            let mut fb = Vec::new();
                            Self::push_u32(&mut fb, 0);
                            out_bytes.extend(Self::msg(obj_id, 1, &fb)); // flags
                            let mut rb = Vec::new();
                            Self::push_u32(&mut rb, 0);
                            Self::push_u32(&mut rb, 1);
                            Self::push_u32(&mut rb, 0);
                            out_bytes.extend(Self::msg(obj_id, 2, &rb)); // ready
                        }
                        Some(Obj::Surface) => match opcode {
                            0 => {
                                let mut st = shared.0.lock().unwrap();
                                Self::release_focus_for(&mut st, obj_id, &mut out_bytes);
                                st.pending_buf.remove(&obj_id);
                                if let Some(&lid) = st.surface_to_layer.get(&obj_id) {
                                    if let Some(info) = st.layers.get_mut(&lid) {
                                        if !info.destroyed {
                                            info.destroyed = true;
                                            info.mapped = false;
                                            st.layer_destroys += 1;
                                        }
                                    }
                                }
                                shared.1.notify_all();
                            }
                            1 => {
                                let bid = Self::read_u32(body, &mut off);
                                shared
                                    .0
                                    .lock()
                                    .unwrap()
                                    .pending_buf
                                    .insert(obj_id, (bid != 0).then_some(bid));
                            }
                            6 => {
                                let mut st = shared.0.lock().unwrap();
                                let buf_id = st.pending_buf.get(&obj_id).copied().flatten();
                                if let Some(bid) = buf_id {
                                    if let Some(buf) = st.shm_bufs.get(&bid).copied() {
                                        if let Some(pool) = st.shm_pools.get(&buf.pool_id) {
                                            let mut f = pool.fd.try_clone().unwrap();
                                            let cx = (buf.width / 2).max(0) as usize;
                                            let cy = (buf.height / 2).max(0) as usize;
                                            let pos = buf.offset + cy * buf.stride + cx * 4 + 3;
                                            let mut a = [0u8; 1];
                                            if f.seek(SeekFrom::Start(pos as u64)).is_ok()
                                                && f.read_exact(&mut a).is_ok()
                                            {
                                                st.last_overlay_center_alpha = Some(a[0]);
                                            }
                                        }
                                    }
                                    out_bytes.extend(Self::msg(bid, 0, &[])); // wl_buffer.release
                                } else {
                                    st.last_overlay_center_alpha = None;
                                }
                                if let Some(&lid) = st.surface_to_layer.get(&obj_id) {
                                    let (out_idx, configured, kb, destroyed) = {
                                        let info = &st.layers[&lid];
                                        (
                                            info.output_idx,
                                            info.configured,
                                            info.kb_interactivity,
                                            info.destroyed,
                                        )
                                    };
                                    if !destroyed {
                                        if !configured {
                                            st.next_serial += 1;
                                            let serial = st.next_serial;
                                            let spec = &specs[out_idx];
                                            let mut cb = Vec::new();
                                            Self::push_u32(&mut cb, serial);
                                            Self::push_u32(&mut cb, spec.width as u32);
                                            Self::push_u32(&mut cb, spec.height as u32);
                                            out_bytes.extend(Self::msg(lid, 0, &cb));
                                            if let Some(info) = st.layers.get_mut(&lid) {
                                                info.configured = true;
                                            }
                                            st.layer_configures_sent += 1;
                                        }
                                        let mapped = buf_id.is_some();
                                        if let Some(info) = st.layers.get_mut(&lid) {
                                            info.mapped = mapped;
                                        }
                                        if mapped && kb == 1 {
                                            if st.defer_second_surface_focus
                                                && st.layer_creates > 1
                                                && st.focused_surface.is_none()
                                            {
                                                st.deferred_focus_surface = Some(obj_id);
                                            } else if st.focused_surface != Some(obj_id) {
                                                if let Some(old) = st.focused_surface {
                                                    Self::push_keyboard_leave(
                                                        &mut st,
                                                        old,
                                                        &mut out_bytes,
                                                    );
                                                }
                                                st.focused_surface = Some(obj_id);
                                                Self::push_keyboard_enter(
                                                    &mut st,
                                                    obj_id,
                                                    &mut out_bytes,
                                                );
                                            }
                                        } else if !mapped {
                                            Self::release_focus_for(
                                                &mut st,
                                                obj_id,
                                                &mut out_bytes,
                                            );
                                        }
                                    }
                                }
                                shared.1.notify_all();
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                    if !out_bytes.is_empty() {
                        Self::write_all(&sock, &out_bytes);
                    }
                }
            }
        }

        fn wait_until(&self, mut pred: impl FnMut(&MockServerState) -> bool) {
            let (lock, cvar) = &*self.shared;
            let mut st = lock.lock().unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !pred(&st) {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                assert!(!left.is_zero(), "timed out waiting on mock Wayland state");
                let (next, _) = cvar.wait_timeout(st, left).unwrap();
                st = next;
            }
        }

        fn focused_surface(&self) -> Option<u32> {
            self.shared.0.lock().unwrap().focused_surface
        }

        fn push_pointer_enter_bytes(
            st: &mut MockServerState,
            pid: u32,
            sid: u32,
            x: f64,
            y: f64,
            raw: &mut Vec<u8>,
        ) {
            st.next_serial += 1;
            let mut eb = Vec::new();
            Self::push_u32(&mut eb, st.next_serial);
            Self::push_u32(&mut eb, sid);
            Self::push_i32(&mut eb, (x * 256.0).round() as i32);
            Self::push_i32(&mut eb, (y * 256.0).round() as i32);
            raw.extend(Self::msg(pid, 0, &eb));
            raw.extend(Self::msg(pid, 5, &[]));
            st.pointer_surface = Some(sid);
        }

        fn push_pointer_motion_bytes(pid: u32, x: f64, y: f64, raw: &mut Vec<u8>) {
            let mut mb = Vec::new();
            Self::push_u32(&mut mb, 100);
            Self::push_i32(&mut mb, (x * 256.0).round() as i32);
            Self::push_i32(&mut mb, (y * 256.0).round() as i32);
            raw.extend(Self::msg(pid, 2, &mb));
            raw.extend(Self::msg(pid, 5, &[]));
        }

        fn push_pointer_button_bytes(
            st: &mut MockServerState,
            pid: u32,
            button: u32,
            pressed: bool,
            raw: &mut Vec<u8>,
        ) {
            st.next_serial += 1;
            let mut bb = Vec::new();
            Self::push_u32(&mut bb, st.next_serial);
            Self::push_u32(&mut bb, 110);
            Self::push_u32(&mut bb, button);
            Self::push_u32(&mut bb, u32::from(pressed));
            raw.extend(Self::msg(pid, 3, &bb));
            raw.extend(Self::msg(pid, 5, &[]));
        }

        fn send_pointer_click(&self, sid: u32, pos: (f64, f64), shift: bool) {
            let mut st = self.shared.0.lock().unwrap();
            let pid = st.pointer_id.unwrap();
            let kid = st.keyboard_id;
            let mut raw = Vec::new();
            if shift {
                if let Some(kid) = kid {
                    Self::push_modifiers(&mut st, kid, 1, &mut raw);
                }
            }
            if st.pointer_surface != Some(sid) {
                Self::push_pointer_enter_bytes(&mut st, pid, sid, pos.0, pos.1, &mut raw);
            } else {
                Self::push_pointer_motion_bytes(pid, pos.0, pos.1, &mut raw);
            }
            Self::push_pointer_button_bytes(&mut st, pid, KEY_BTN_LEFT, true, &mut raw);
            Self::push_pointer_button_bytes(&mut st, pid, KEY_BTN_LEFT, false, &mut raw);
            if shift {
                if let Some(kid) = kid {
                    Self::push_modifiers(&mut st, kid, 0, &mut raw);
                }
            }
            drop(st);
            Self::write_all(&self.server_sock, &raw);
        }

        fn send_pointer_drag(&self, sid: u32, from: (f64, f64), to: (f64, f64)) {
            self.send_pointer_drag_and_optional_key(sid, from, to, None);
        }

        fn send_pointer_drag_and_optional_key(
            &self,
            sid: u32,
            from: (f64, f64),
            to: (f64, f64),
            key_after: Option<(u32, bool)>,
        ) {
            let mut st = self.shared.0.lock().unwrap();
            let pid = st.pointer_id.unwrap();
            let mut raw = Vec::new();
            if st.pointer_surface != Some(sid) {
                Self::push_pointer_enter_bytes(&mut st, pid, sid, from.0, from.1, &mut raw);
            } else {
                Self::push_pointer_motion_bytes(pid, from.0, from.1, &mut raw);
            }
            Self::push_pointer_button_bytes(&mut st, pid, KEY_BTN_LEFT, true, &mut raw);
            Self::push_pointer_motion_bytes(pid, to.0, to.1, &mut raw);
            Self::push_pointer_button_bytes(&mut st, pid, KEY_BTN_LEFT, false, &mut raw);
            if let Some((key, ctrl)) = key_after {
                if let (Some(kid), Some(_)) = (st.keyboard_id, st.focused_surface) {
                    if ctrl {
                        Self::push_modifiers(&mut st, kid, 4, &mut raw);
                    }
                    Self::push_key(&mut st, kid, key, true, &mut raw);
                    Self::push_key(&mut st, kid, key, false, &mut raw);
                    if ctrl {
                        Self::push_modifiers(&mut st, kid, 0, &mut raw);
                    }
                }
            }
            drop(st);
            Self::write_all(&self.server_sock, &raw);
        }

        fn grant_deferred_focus(&self) {
            let mut st = self.shared.0.lock().unwrap();
            if let Some(sid) = st.deferred_focus_surface.take() {
                let mut out_bytes = Vec::new();
                st.focused_surface = Some(sid);
                Self::push_keyboard_enter(&mut st, sid, &mut out_bytes);
                drop(st);
                Self::write_all(&self.server_sock, &out_bytes);
                self.shared.1.notify_all();
            }
        }

        /// Delivers a key press + release ONLY if the client currently holds compositor focus;
        /// otherwise records it in `dropped_keys` (what a real compositor does when focus is
        /// elsewhere or unset).
        fn send_key_if_focused(&self, key: u32, ctrl: bool) -> bool {
            let mut st = self.shared.0.lock().unwrap();
            let Some(kid) = st.keyboard_id else {
                st.dropped_keys.push(key);
                return false;
            };
            if st.focused_surface.is_none() {
                st.dropped_keys.push(key);
                return false;
            }
            let mut raw = Vec::new();
            if ctrl {
                Self::push_modifiers(&mut st, kid, 4, &mut raw);
            }
            Self::push_key(&mut st, kid, key, true, &mut raw);
            Self::push_key(&mut st, kid, key, false, &mut raw);
            if ctrl {
                Self::push_modifiers(&mut st, kid, 0, &mut raw);
            }
            drop(st);
            Self::write_all(&self.server_sock, &raw);
            true
        }

        fn push_key(
            st: &mut MockServerState,
            kid: u32,
            key: u32,
            pressed: bool,
            raw: &mut Vec<u8>,
        ) {
            st.next_serial += 1;
            let mut kb = Vec::new();
            Self::push_u32(&mut kb, st.next_serial);
            Self::push_u32(&mut kb, 120);
            Self::push_u32(&mut kb, key);
            Self::push_u32(&mut kb, u32::from(pressed));
            raw.extend(Self::msg(kid, 3, &kb));
        }

        fn push_modifiers(st: &mut MockServerState, kid: u32, depressed: u32, raw: &mut Vec<u8>) {
            st.next_serial += 1;
            let mut mb = Vec::new();
            Self::push_u32(&mut mb, st.next_serial);
            Self::push_u32(&mut mb, depressed);
            Self::push_u32(&mut mb, 0);
            Self::push_u32(&mut mb, 0);
            Self::push_u32(&mut mb, 0);
            raw.extend(Self::msg(kid, 4, &mb));
        }
    }

    impl Drop for MockWayland {
        fn drop(&mut self) {
            let _ = self.server_sock.shutdown(std::net::Shutdown::Both);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    fn decode_png_pixel(png_bytes: &[u8], x: i32, y: i32) -> (u8, u8, u8) {
        let mut cursor = std::io::Cursor::new(png_bytes);
        let mut surf = cairo::ImageSurface::create_from_png(&mut cursor).unwrap();
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        let off = y as usize * stride + x as usize * 4;
        // ARgb32 in little-endian memory is [B, G, R, A].
        (data[off + 2], data[off + 1], data[off])
    }

    fn test_look() -> Look {
        Look {
            accent: Color::rgb8(0, 255, 0),
            text: Color::rgb8(255, 255, 255),
            card: Color::rgb8(17, 19, 23),
            ..Default::default()
        }
    }

    fn wait_boxes_file(path: &Path) -> Option<(f64, f64, f64)> {
        for _ in 0..1000 {
            if let Ok(text) = std::fs::read_to_string(path) {
                let mut editor_ms = None;
                let mut image = None;
                for line in text.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.first() == Some(&"editor") && parts.len() >= 2 {
                        editor_ms = parts[1].parse::<f64>().ok();
                    }
                    if parts.first() == Some(&"image") && parts.len() >= 3 {
                        if let (Ok(x), Ok(y)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>()) {
                            image = Some((x, y));
                        }
                    }
                }
                if let (Some(ms), Some((ix, iy))) = (editor_ms, image) {
                    return Some((ms, ix, iy));
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    }

    /// Reproduces the VM bug from tools/vm-edge/pshot.py:
    /// Pick a 200x200 region, press B right as the pick ends (as the editor opens, before any
    /// separate editor surface could gain focus), drag a box inside the image, and Ctrl+S.
    /// Today `destroy_overlays()` drops focus between surfaces so B never reaches the editor
    /// and the drag draws an arrow instead of a box.
    #[test]
    fn key_b_right_after_pick_selects_box_tool_in_editor() {
        let box_path = std::env::temp_dir().join(format!("shot-boxes-b-{}", std::process::id()));
        let _ = std::fs::remove_file(&box_path);
        let (mock, conn) = MockWayland::start(vec![MockOutputSpec {
            width: 800,
            height: 600,
            x: 0,
            y: 0,
            name: "DP-1",
        }]);
        mock.shared.0.lock().unwrap().defer_second_surface_focus = true;
        let box_for_client = box_path.clone();
        let client = std::thread::spawn(move || {
            run_wayland_with_conn(
                &conn,
                Mode::Region,
                false,
                test_look(),
                Snapshot::default(),
                Some(box_for_client),
            )
        });

        mock.wait_until(|st| st.focused_surface.is_some());
        let picker_sid = mock.focused_surface().unwrap();

        mock.send_pointer_drag(picker_sid, (100.0, 100.0), (300.0, 300.0));

        let (ready_ms, ix, iy) = wait_boxes_file(&box_path).expect("editor boxes dump");
        let _ = std::fs::remove_file(&box_path);

        // Deliver B immediately on the first editor frame, before any new surface gains focus.
        let b_delivered = mock.send_key_if_focused(KEY_B, false);
        mock.grant_deferred_focus();
        let editor_sid = mock.focused_surface().unwrap_or(picker_sid);
        assert_eq!(mock.shared.0.lock().unwrap().focus_losses, 0);

        mock.send_pointer_drag(editor_sid, (ix + 20.0, iy + 20.0), (ix + 120.0, iy + 120.0));
        assert!(
            mock.send_key_if_focused(KEY_S, true),
            "Ctrl+S must hit focused editor"
        );

        let outcome = client.join().unwrap().unwrap();
        let st = mock.shared.0.lock().unwrap();
        let summary = format!(
            "ready_ms={ready_ms:.2} creates={} destroys={} configures={} focus_losses={} dropped={:?}",
            st.layer_creates,
            st.layer_destroys,
            st.layer_configures_sent,
            st.focus_losses,
            st.dropped_keys
        );
        assert_eq!(st.layer_creates, 1, "{summary}");
        assert_eq!(st.layer_configures_sent, 1, "{summary}");
        assert_eq!(st.layer_destroys, 1, "{summary}");
        assert!(st.dropped_keys.is_empty(), "{summary}");
        drop(st);
        let png = match outcome {
            Outcome::Save(bytes) => bytes,
            _ => panic!("expected Save outcome ({summary})"),
        };
        let px = decode_png_pixel(&png, 20, 70);
        assert!(
            b_delivered && px == (0, 255, 0),
            "B missed editor (pixel at (20,70)={px:?}, want (0, 255, 0)): {summary}"
        );
    }

    #[test]
    fn key_esc_right_after_pick_closes_editor_cleanly() {
        let box_path = std::env::temp_dir().join(format!("shot-boxes-esc-{}", std::process::id()));
        let _ = std::fs::remove_file(&box_path);
        let (mock, conn) = MockWayland::start(vec![MockOutputSpec {
            width: 800,
            height: 600,
            x: 0,
            y: 0,
            name: "DP-1",
        }]);
        mock.shared.0.lock().unwrap().defer_second_surface_focus = true;
        let box_for_client = box_path.clone();
        let client = std::thread::spawn(move || {
            run_wayland_with_conn(
                &conn,
                Mode::Region,
                false,
                test_look(),
                Snapshot::default(),
                Some(box_for_client),
            )
        });

        mock.wait_until(|st| st.focused_surface.is_some());
        let picker_sid = mock.focused_surface().unwrap();
        mock.send_pointer_drag(picker_sid, (100.0, 100.0), (300.0, 300.0));

        let (ready_ms, _, _) = wait_boxes_file(&box_path).expect("editor boxes dump");
        let _ = std::fs::remove_file(&box_path);

        let esc_delivered = mock.send_key_if_focused(KEY_ESC, false);
        assert!(
            esc_delivered,
            "Esc dropped right after pick (ready_ms={ready_ms:.2}, focus_losses={})",
            mock.shared.0.lock().unwrap().focus_losses
        );
        let outcome = client.join().unwrap().unwrap();
        assert!(matches!(outcome, Outcome::Plain(_)));
    }

    #[test]
    fn same_batch_b_and_esc_after_release_reach_editor() {
        let _ = (KEY_ESC, KEY_LCTL);
        // 1. Release + B in the same wire batch -> Box drawn
        let (mock, conn) = MockWayland::start(vec![MockOutputSpec {
            width: 800,
            height: 600,
            x: 0,
            y: 0,
            name: "DP-1",
        }]);
        let box_path = std::env::temp_dir().join(format!("shot-boxes-sb-{}", std::process::id()));
        let _ = std::fs::remove_file(&box_path);
        let box_for_client = box_path.clone();
        let client = std::thread::spawn(move || {
            run_wayland_with_conn(
                &conn,
                Mode::Region,
                false,
                test_look(),
                Snapshot::default(),
                Some(box_for_client),
            )
        });
        mock.wait_until(|st| st.focused_surface.is_some());
        let picker_sid = mock.focused_surface().unwrap();
        mock.send_pointer_drag_and_optional_key(
            picker_sid,
            (100.0, 100.0),
            (300.0, 300.0),
            Some((KEY_B, false)),
        );
        let (_, ix, iy) = wait_boxes_file(&box_path).expect("editor boxes dump");
        let _ = std::fs::remove_file(&box_path);
        let editor_sid = mock.focused_surface().unwrap_or(picker_sid);
        mock.send_pointer_drag(editor_sid, (ix + 20.0, iy + 20.0), (ix + 120.0, iy + 120.0));
        assert!(mock.send_key_if_focused(KEY_S, true));
        let Outcome::Save(png) = client.join().unwrap().unwrap() else {
            panic!("expected Save outcome");
        };
        assert_eq!(
            decode_png_pixel(&png, 20, 70),
            (0, 255, 0),
            "same-batch B after release must select Box tool"
        );

        // 2. Release + Esc in the same wire batch -> Plain (saved original), not Cancelled
        let (mock2, conn2) = MockWayland::start(vec![MockOutputSpec {
            width: 800,
            height: 600,
            x: 0,
            y: 0,
            name: "DP-1",
        }]);
        let client2 = std::thread::spawn(move || {
            run_wayland_with_conn(
                &conn2,
                Mode::Region,
                false,
                test_look(),
                Snapshot::default(),
                None,
            )
        });
        mock2.wait_until(|st| st.focused_surface.is_some());
        let picker_sid2 = mock2.focused_surface().unwrap();
        mock2.send_pointer_drag_and_optional_key(
            picker_sid2,
            (100.0, 100.0),
            (300.0, 300.0),
            Some((KEY_ESC, false)),
        );
        let out2 = client2.join().unwrap().unwrap();
        assert!(
            matches!(out2, Outcome::Plain(_)),
            "same-batch Esc after release must finish editor with Plain, not Cancel"
        );
    }

    #[test]
    fn live_mode_keeps_focus_and_captures_clean_background() {
        let box_path = std::env::temp_dir().join(format!("shot-boxes-live-{}", std::process::id()));
        let _ = std::fs::remove_file(&box_path);
        let (mock, conn) = MockWayland::start(vec![MockOutputSpec {
            width: 800,
            height: 600,
            x: 0,
            y: 0,
            name: "DP-1",
        }]);
        let box_for_client = box_path.clone();
        let client = std::thread::spawn(move || {
            run_wayland_with_conn(
                &conn,
                Mode::Region,
                true,
                test_look(),
                Snapshot::default(),
                Some(box_for_client),
            )
        });

        mock.wait_until(|st| st.focused_surface.is_some());
        let picker_sid = mock.focused_surface().unwrap();
        mock.send_pointer_drag_and_optional_key(
            picker_sid,
            (100.0, 100.0),
            (300.0, 300.0),
            Some((KEY_B, false)),
        );

        let (_, ix, iy) = wait_boxes_file(&box_path).expect("editor boxes dump");
        let _ = std::fs::remove_file(&box_path);
        assert_eq!(mock.shared.0.lock().unwrap().focus_losses, 0);

        let editor_sid = mock.focused_surface().unwrap();
        mock.send_pointer_drag(editor_sid, (ix + 20.0, iy + 20.0), (ix + 120.0, iy + 120.0));
        assert!(mock.send_key_if_focused(KEY_S, true));

        let Outcome::Save(png) = client.join().unwrap().unwrap() else {
            panic!("expected Save outcome");
        };
        assert_eq!(decode_png_pixel(&png, 20, 70), (0, 255, 0));
        // Untouched pixel must match the un-dimmed simulated desktop background (50, 100, 200).
        assert_eq!(decode_png_pixel(&png, 5, 5), (50, 100, 200));
    }

    #[test]
    fn multi_output_transfers_focus_to_editor_output_without_dropping() {
        let box_path =
            std::env::temp_dir().join(format!("shot-boxes-multi-{}", std::process::id()));
        let _ = std::fs::remove_file(&box_path);
        let (mock, conn) = MockWayland::start(vec![
            MockOutputSpec {
                width: 800,
                height: 600,
                x: 0,
                y: 0,
                name: "DP-1",
            },
            MockOutputSpec {
                width: 800,
                height: 600,
                x: 800,
                y: 0,
                name: "DP-2",
            },
        ]);
        let box_for_client = box_path.clone();
        let client = std::thread::spawn(move || {
            run_wayland_with_conn(
                &conn,
                Mode::Region,
                false,
                test_look(),
                Snapshot::default(),
                Some(box_for_client),
            )
        });

        // Both outputs map; DP-2 (index 1) commits second and holds initial focus.
        mock.wait_until(|st| st.layers.len() == 2 && st.focused_surface.is_some());
        let (dp1_sid, dp2_sid) = {
            let st = mock.shared.0.lock().unwrap();
            let dp1 = st
                .layers
                .values()
                .find(|l| l.output_idx == 0)
                .unwrap()
                .surface_id;
            let dp2 = st
                .layers
                .values()
                .find(|l| l.output_idx == 1)
                .unwrap()
                .surface_id;
            (dp1, dp2)
        };
        mock.wait_until(|st| st.focused_surface == Some(dp2_sid));

        // Drag a region on DP-1 (output 0) while focus was on DP-2, and press B immediately.
        mock.send_pointer_drag_and_optional_key(
            dp1_sid,
            (100.0, 100.0),
            (300.0, 300.0),
            Some((KEY_B, false)),
        );

        let (_, ix, iy) = wait_boxes_file(&box_path).expect("editor boxes dump");
        let _ = std::fs::remove_file(&box_path);

        mock.wait_until(|st| st.layer_destroys == 1);
        assert_eq!(mock.focused_surface(), Some(dp1_sid));
        assert_eq!(mock.shared.0.lock().unwrap().focus_losses, 0);

        mock.send_pointer_drag(dp1_sid, (ix + 20.0, iy + 20.0), (ix + 120.0, iy + 120.0));
        assert!(mock.send_key_if_focused(KEY_S, true));

        let Outcome::Save(png) = client.join().unwrap().unwrap() else {
            panic!("expected Save outcome");
        };
        assert_eq!(decode_png_pixel(&png, 20, 70), (0, 255, 0));
    }

    #[test]
    fn shift_pick_skips_editor_and_destroys_overlay() {
        let box_path =
            std::env::temp_dir().join(format!("shot-boxes-shift-{}", std::process::id()));
        let _ = std::fs::remove_file(&box_path);
        let (mock, conn) = MockWayland::start(vec![MockOutputSpec {
            width: 800,
            height: 600,
            x: 0,
            y: 0,
            name: "DP-1",
        }]);
        let snap = parse_snapshot_json(
            r#"{
                "outputs": [{
                    "name": "DP-1",
                    "mode": [800, 600],
                    "scale": 1.0,
                    "transform": "normal",
                    "position": [0, 0],
                    "camera": { "center": [0.0, 0.0], "zoom": 1.0 }
                }],
                "elements": [
                    { "state": "mapped", "anchor": [-200.0, -150.0], "size": [400.0, 300.0], "minimized": false }
                ]
            }"#,
        )
        .unwrap();
        let box_for_client = box_path.clone();
        let client = std::thread::spawn(move || {
            run_wayland_with_conn(
                &conn,
                Mode::Window,
                false,
                test_look(),
                snap,
                Some(box_for_client),
            )
        });

        mock.wait_until(|st| st.focused_surface.is_some());
        let picker_sid = mock.focused_surface().unwrap();
        mock.send_pointer_click(picker_sid, (400.0, 300.0), true);

        let outcome = client.join().unwrap().unwrap();
        assert!(matches!(outcome, Outcome::Plain(_)));
        assert!(!box_path.exists());
        let st = mock.shared.0.lock().unwrap();
        assert_eq!(st.layer_creates, 1);
        assert_eq!(st.layer_destroys, 1);
    }

    #[test]
    fn window_mode_updates_cursor_to_crosshair_when_reused_for_editor() {
        let box_path = std::env::temp_dir().join(format!("shot-boxes-win-{}", std::process::id()));
        let _ = std::fs::remove_file(&box_path);
        let (mock, conn) = MockWayland::start(vec![MockOutputSpec {
            width: 800,
            height: 600,
            x: 0,
            y: 0,
            name: "DP-1",
        }]);
        let snap = parse_snapshot_json(
            r#"{
                "outputs": [{
                    "name": "DP-1",
                    "mode": [800, 600],
                    "scale": 1.0,
                    "transform": "normal",
                    "position": [0, 0],
                    "camera": { "center": [0.0, 0.0], "zoom": 1.0 }
                }],
                "elements": [
                    { "state": "mapped", "anchor": [-200.0, -150.0], "size": [400.0, 300.0], "minimized": false }
                ]
            }"#,
        )
        .unwrap();
        let box_for_client = box_path.clone();
        let client = std::thread::spawn(move || {
            run_wayland_with_conn(
                &conn,
                Mode::Window,
                false,
                test_look(),
                snap,
                Some(box_for_client),
            )
        });

        mock.wait_until(|st| st.focused_surface.is_some());
        let picker_sid = mock.focused_surface().unwrap();
        mock.send_pointer_click(picker_sid, (400.0, 300.0), false);

        wait_boxes_file(&box_path).expect("editor boxes dump");
        let _ = std::fs::remove_file(&box_path);
        // Window hover sets Pointer (4), then reusing surface for editor updates to Crosshair (8).
        mock.wait_until(|st| st.cursor_shapes.ends_with(&[4, 8]));
        assert!(mock.send_key_if_focused(KEY_ESC, false));
        let outcome = client.join().unwrap().unwrap();
        assert!(matches!(outcome, Outcome::Plain(_)));
    }
}
