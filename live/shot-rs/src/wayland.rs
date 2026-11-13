use std::env;
use std::path::PathBuf;
use std::process::Command;

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
    configured: bool,
    logical_w: u32,
    logical_h: u32,
    dirty: bool,
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
}

struct App {
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

    fn update_cursor_shape(&mut self, serial: u32) {
        let Some(dev) = &self.cursor_device else {
            return;
        };
        let shape = match self.mode {
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

            let (buf_w, buf_h) = if use_viewporter {
                (out.view_w, out.view_h)
            } else {
                (ov.logical_w, ov.logical_h)
            };
            if buf_w == 0 || buf_h == 0 {
                continue;
            }
            let stride = buf_w as i32 * 4;
            let len = buf_h as usize * stride as usize;
            if ov.pool.len() < len * 2 {
                let _ = ov.pool.resize(len * 2);
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
                render_overlay(
                    &surface,
                    &OverlayDraw {
                        look: self.look,
                        frozen: frozen_ref,
                        selection_px: sel_px,
                        size_label,
                        scale: if use_viewporter { out.scale } else { 1.0 },
                    },
                )?;
            }

            if let Some(vp) = &ov.viewport {
                vp.set_destination(ov.logical_w as i32, ov.logical_h as i32);
            }
            let surf = ov.layer.wl_surface();
            surf.attach(Some(buffer.wl_buffer()), 0, 0);
            surf.damage_buffer(0, 0, buf_w as i32, buf_h as i32);
            surf.commit();
            ov.buffers.push(buffer);
        }
        Ok(())
    }

    fn destroy_overlays(&mut self) {
        for out in &mut self.outputs {
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

/// Runs `screen`, `region`, or `window` capture/selection against the compositor.
/// Returns `Ok(None)` when the user cancels with Escape or right click (exit code 1).
pub fn run_wayland(mode: Mode, live: bool, look: Look) -> Result<Option<Vec<u8>>, String> {
    let snapshot = match mode {
        Mode::Screen => Snapshot::default(),
        Mode::Region | Mode::Window => fetch_snapshot(),
    };

    let conn = Connection::connect_to_env()
        .map_err(|e| format!("failed to connect to Wayland display: {e}"))?;
    let (globals, mut queue) = registry_queue_init::<App>(&conn)
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
        return Ok(Some(encode_png_bytes(surface)?));
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
        let wl_surface = app.compositor_state.create_surface(&qh);
        let layer = layer_shell.create_layer_surface(
            &qh,
            wl_surface,
            Layer::Overlay,
            Some("infynitae-shot"),
            Some(&app.outputs[idx].wl_output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.commit();

        let viewport = app
            .viewporter
            .as_ref()
            .map(|vp| vp.get_viewport(layer.wl_surface(), &qh, ()));
        let init_bytes =
            (app.outputs[idx].view_w as usize * app.outputs[idx].view_h as usize * 4).max(4096) * 2;
        let pool =
            SlotPool::new(init_bytes, &app.shm).map_err(|e| format!("shm pool failed: {e}"))?;

        app.outputs[idx].overlay = Some(OverlaySurface {
            layer,
            viewport,
            pool,
            buffers: Vec::new(),
            configured: false,
            logical_w: app.outputs[idx].logical_w,
            logical_h: app.outputs[idx].logical_h,
            dirty: true,
        });
    }

    while app
        .outputs
        .iter()
        .any(|o| o.overlay.as_ref().is_some_and(|ov| !ov.configured))
    {
        queue
            .blocking_dispatch(&mut app)
            .map_err(|e| format!("wayland dispatch failed waiting for configure: {e}"))?;
    }

    app.recompute_windows();
    app.mark_all_dirty();

    while app.accepted.is_none() && !app.cancelled {
        app.redraw_dirty_overlays()?;
        queue
            .blocking_dispatch(&mut app)
            .map_err(|e| format!("wayland dispatch failed: {e}"))?;
    }

    app.destroy_overlays();
    let _ = queue.roundtrip(&mut app);

    if app.cancelled {
        return Ok(None);
    }

    let selection = app
        .accepted
        .ok_or_else(|| "no selection chosen".to_string())?;

    if live {
        let capture_indices: Vec<usize> = match selection {
            FinalSelection::OutputWindow(idx, _) => vec![idx],
            FinalSelection::Logical(sel_logical) => (0..app.outputs.len())
                .filter(|&i| {
                    sel_logical
                        .intersect(app.outputs[i].logical_rect())
                        .is_some()
                })
                .collect(),
        };
        if capture_indices.is_empty() {
            return Err("selection outside every output".to_string());
        }
        // Let the compositor finish unmapping the overlay and repainting exposed content.
        let _ = queue.roundtrip(&mut app);
        capture_outputs(&mut app, &mut queue, &capture_indices)?;
    }

    let result_surface = match selection {
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

    Ok(Some(encode_png_bytes(&result_surface)?))
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
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.cancelled = true;
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
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        match event.keysym {
            Keysym::Escape => {
                self.cancelled = true;
            }
            Keysym::Return | Keysym::KP_Enter => {
                if self.mode == Mode::Region {
                    if let Some(drag) = self.drag.filter(|d| d.dragged) {
                        let rect = drag.rect();
                        let (pw, ph) = self.active_drag_size_px(rect);
                        if pw > 0 && ph > 0 {
                            self.accepted = Some(FinalSelection::Logical(rect));
                            return;
                        }
                    }
                }
                if let Some((out_idx, win_rect)) = self.hovered_window {
                    self.accepted = Some(FinalSelection::OutputWindow(out_idx, win_rect));
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
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
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
                                    self.accepted = Some(FinalSelection::Logical(rect));
                                    return;
                                }
                            } else {
                                self.update_hover();
                                if let Some((win_out, win_rect)) = self.hovered_window {
                                    self.accepted =
                                        Some(FinalSelection::OutputWindow(win_out, win_rect));
                                    return;
                                }
                            }
                            self.mark_all_dirty();
                        }
                        Mode::Window => {
                            self.update_hover();
                            if let Some((win_out, win_rect)) = self.hovered_window {
                                self.accepted =
                                    Some(FinalSelection::OutputWindow(win_out, win_rect));
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
