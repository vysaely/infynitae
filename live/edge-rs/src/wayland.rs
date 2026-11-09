//! The Wayland client: one wlr-layer-shell surface per output, two shm buffers per surface,
//! paced by frame callbacks, zero work while closed, and signals / request files in calloop.

use crate::{
    hover_strip, layout_in, max_panel_height, modules, panel_hover_box_in, panel_origin,
    panel_width, Key, Shell,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm, delegate_touch,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            channel::{self, Sender},
            generic::Generic,
            EventLoop, Interest, Mode, PostAction,
        },
        calloop_wayland_source::WaylandSource,
        client::{
            globals::registry_queue_init,
            protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface, wl_touch},
            Connection, Dispatch, QueueHandle,
        },
        protocols_wlr::foreign_toplevel::v1::client::{
            zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
            zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT},
        touch::TouchHandler,
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

const PID_FILE: &str = "infynitae-edge-rs.pid";
const PANEL_FILE: &str = "infynitae-edge-rs.panel";

pub fn runtime_path(name: &str) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/tmp".to_string());
    PathBuf::from(dir).join(name)
}

/// Returns the PID of a running `infynitae-edge-rs`, if one exists.
pub fn running_pid() -> Option<i32> {
    let text = std::fs::read_to_string(runtime_path(PID_FILE)).ok()?;
    let pid: i32 = text.trim().parse().ok()?;
    if pid <= 1 || pid == std::process::id() as i32 {
        return None;
    }
    if unsafe { libc::kill(pid, 0) } != 0 {
        return None;
    }
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    // the pid file is this edge's own (-rs); its process is named infynitae-edge-rs (cut to
    // "infynitae-edge-" by the kernel), or infynitae-edge once it replaces the C one (stage 6)
    let comm = comm.trim();
    (comm.starts_with("infynitae-edge-") || comm == "infynitae-edge").then_some(pid)
}

/// Sends a panel name to a running `infynitae-edge-rs` via `.panel` + `SIGRTMIN+2`.
pub fn send_panel(name: &str) -> bool {
    let Some(pid) = running_pid() else {
        return false;
    };
    let path = runtime_path(PANEL_FILE);
    if std::fs::write(&path, format!("{name}\n")).is_err() {
        return false;
    }
    unsafe { libc::kill(pid, libc::SIGRTMIN() + 2) == 0 }
}

/// Stops a running `infynitae-edge-rs` with `SIGTERM`.
pub fn stop_running() -> bool {
    let Some(pid) = running_pid() else {
        return false;
    };
    unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
}

struct BufferSlot {
    buffer: Buffer,
    prev_box: Option<(i32, i32, i32, i32)>,
}

struct OutputSurface {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    pool: SlotPool,
    slots: [Option<BufferSlot>; 2],
    width: u32,
    height: u32,
    buf_size: (u32, u32),
    configured: bool,
    mapped_closed: bool,
    dirty: bool,
    frame_pending: bool,
}

impl OutputSurface {
    fn set_input_region(&self, compositor: &CompositorState, shell: &Shell) {
        let Ok(region) = Region::new(compositor) else {
            return;
        };
        let w = self.width as f64;
        let h = self.height as f64;
        if w > 0.0 && h > 0.0 {
            if let Some(idx) = shell.open {
                if let Some(p) = shell.cfg.panels.get(idx) {
                    // opened by its key: the whole screen takes the pointer, so a click
                    // outside the panel reaches the edge and closes it (as the C edge while it
                    // holds the keyboard); opened by hover: the panel and the way to it, so
                    // leaving it closes it
                    let (rx, ry, rw, rh) = if shell.opened_by_hover {
                        panel_hover_box_in(p, w, h, &shell.world, &shell.theme)
                    } else {
                        (0.0, 0.0, w, h)
                    };
                    region.add(
                        rx.floor() as i32,
                        ry.floor() as i32,
                        rw.ceil() as i32,
                        rh.ceil() as i32,
                    );
                }
            } else {
                for p in &shell.cfg.panels {
                    if p.hover {
                        let (sx, sy, sw, sh) = hover_strip(p, w, h);
                        region.add(
                            sx.floor() as i32,
                            sy.floor() as i32,
                            sw.ceil() as i32,
                            sh.ceil() as i32,
                        );
                    }
                }
            }
        }
        self.layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
    }

    fn apply_mode(&mut self, qh: &QueueHandle<App>, compositor: &CompositorState, shell: &Shell) {
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }
        let kb = if shell.open.is_some() {
            KeyboardInteractivity::Exclusive
        } else {
            KeyboardInteractivity::None
        };
        self.layer.set_keyboard_interactivity(kb);
        self.set_input_region(compositor, shell);

        if shell.open.is_some() {
            self.dirty = true;
            self.mapped_closed = false;
            self.render_if_needed(qh, shell);
        } else {
            self.dirty = false;
            self.frame_pending = false;
            self.clear_and_commit();
        }
    }

    fn ensure_pool_size(&mut self) {
        if self.buf_size != (self.width, self.height) {
            self.slots = [None, None];
            self.buf_size = (self.width, self.height);
            let need = (self.width as usize) * (self.height as usize) * 4 * 2;
            if self.pool.len() < need {
                let _ = self.pool.resize(need);
            }
        }
    }

    fn acquire_slot(&mut self) -> Option<usize> {
        self.ensure_pool_size();
        let w = self.width as i32;
        let h = self.height as i32;
        let stride = w * 4;
        for i in 0..2 {
            if self.slots[i].is_none() {
                if let Ok((buffer, _)) =
                    self.pool
                        .create_buffer(w, h, stride, wl_shm::Format::Argb8888)
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

    fn clear_box(canvas: &mut [u8], stride: i32, w: i32, h: i32, rect: (i32, i32, i32, i32)) {
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

    fn clear_and_commit(&mut self) {
        if self.mapped_closed {
            self.layer.commit();
            return;
        }
        let w = self.width as i32;
        let h = self.height as i32;
        let stride = w * 4;
        let Some(idx) = self.acquire_slot() else {
            return;
        };
        let slot = self.slots[idx].as_mut().unwrap();
        let old = slot.prev_box.take();
        if let Some(canvas) = slot.buffer.canvas(&mut self.pool) {
            if let Some(r) = old {
                Self::clear_box(canvas, stride, w, h, r);
            }
        }
        let wl = self.layer.wl_surface();
        if let Some((x, y, bw, bh)) = old {
            wl.damage_buffer(x, y, bw, bh);
        } else {
            wl.damage_buffer(0, 0, w, h);
        }
        if slot.buffer.attach_to(wl).is_ok() {
            self.mapped_closed = true;
            self.layer.commit();
        }
    }

    fn render_if_needed(&mut self, qh: &QueueHandle<App>, shell: &Shell) {
        if !self.configured || !self.dirty || self.frame_pending {
            return;
        }
        let Some(open_idx) = shell.open else {
            return;
        };
        let Some(panel) = shell.cfg.panels.get(open_idx) else {
            return;
        };
        let w = self.width as i32;
        let h = self.height as i32;
        let stride = w * 4;
        let Some(slot_idx) = self.acquire_slot() else {
            let wl = self.layer.wl_surface();
            wl.frame(qh, wl.clone());
            self.frame_pending = true;
            return;
        };

        let pw = panel_width(panel, w as f64);
        let (rects, content_h) = layout_in(panel, pw, &shell.world, &shell.theme);
        let ph = content_h.min(max_panel_height(h as f64));
        let (ox, oy) = panel_origin(panel, w as f64, h as f64, pw, ph);
        let pad = 4;
        let mut tray_m_rect = None;
        if let Some(open_idx) = shell.world.tray.open_menu {
            if let Some((_, r)) = panel
                .items
                .iter()
                .zip(&rects)
                .find(|(it, _)| it.module == crate::config::Module::Tray)
            {
                let item_count = shell
                    .world
                    .tray
                    .items
                    .get(open_idx)
                    .map(|it| it.effective_menu_items().len())
                    .unwrap_or(3);
                tray_m_rect = Some(modules::tray::menu_rect(
                    (ox, oy),
                    (pw, ph),
                    *r,
                    item_count,
                    (w as f64, h as f64),
                ));
            }
        }

        let new_box = if let Some(mr) = tray_m_rect {
            let min_x = (ox.min(mr.0).floor() as i32 - pad).max(0);
            let min_y = (oy.min(mr.1).floor() as i32 - pad).max(0);
            let max_x = ((ox + pw).max(mr.0 + mr.2).ceil() as i32 + pad).min(w);
            let max_y = ((oy + ph).max(mr.1 + mr.3).ceil() as i32 + pad).min(h);
            (min_x, min_y, (max_x - min_x).max(1), (max_y - min_y).max(1))
        } else {
            (
                (ox.floor() as i32 - pad).max(0),
                (oy.floor() as i32 - pad).max(0),
                (pw.ceil() as i32 + 2 * pad).min(w),
                (ph.ceil() as i32 + 2 * pad).min(h),
            )
        };

        let slot = self.slots[slot_idx].as_mut().unwrap();
        let old_box = slot.prev_box.replace(new_box);
        if let Some(canvas) = slot.buffer.canvas(&mut self.pool) {
            if let Some(r) = old_box {
                Self::clear_box(canvas, stride, w, h, r);
            }
            Self::clear_box(canvas, stride, w, h, new_box);
            if let Ok(surface) = unsafe {
                cairo::ImageSurface::create_for_data_unsafe(
                    canvas.as_mut_ptr(),
                    cairo::Format::ARgb32,
                    w,
                    h,
                    stride,
                )
            } {
                if let Ok(cr) = cairo::Context::new(&surface) {
                    if content_h > ph {
                        let _ = cr.save();
                        cr.rectangle(ox, oy, pw, ph);
                        cr.clip();
                        modules::draw_panel(
                            &cr,
                            &shell.theme,
                            panel,
                            &rects,
                            &shell.world,
                            (ox, oy),
                            pw,
                        );
                        let _ = cr.restore();
                    } else {
                        modules::draw_panel(
                            &cr,
                            &shell.theme,
                            panel,
                            &rects,
                            &shell.world,
                            (ox, oy),
                            pw,
                        );
                    }
                    if let Some(mr) = tray_m_rect {
                        modules::tray::draw_menu(&cr, &shell.theme, &shell.world, mr);
                    }
                }
                surface.flush();
            }
        }

        let wl = self.layer.wl_surface();
        if let Some((ox, oy, ow, oh)) = old_box {
            wl.damage_buffer(ox, oy, ow, oh);
        }
        wl.damage_buffer(new_box.0, new_box.1, new_box.2, new_box.3);
        wl.frame(qh, wl.clone());
        self.frame_pending = true;
        self.dirty = false;
        if slot.buffer.attach_to(wl).is_ok() {
            self.layer.commit();
        }
    }
}

struct ToplevelEntry {
    id: u64,
    handle: ZwlrForeignToplevelHandleV1,
    title: String,
    app_id: String,
    focused: bool,
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    qh: QueueHandle<App>,

    outputs: Vec<OutputSurface>,
    toplevels: Vec<ToplevelEntry>,
    next_toplevel_id: u64,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    touch: Option<wl_touch::WlTouch>,
    modifiers: Modifiers,

    shell: Shell,
    fetch_tx: Sender<Vec<(String, String)>>,
    fetch_busy: Arc<AtomicBool>,
    last_fetch: Option<Instant>,
    prev_open: Option<usize>,
    exit: bool,
}

impl App {
    fn sync_toplevels_to_world(&mut self) {
        let items: Vec<modules::windows::WindowItem> = self
            .toplevels
            .iter()
            .map(|t| {
                let app_id = if t.app_id.is_empty() {
                    "Window".to_string()
                } else {
                    t.app_id.clone()
                };
                let title = if t.title.is_empty() {
                    app_id.clone()
                } else {
                    t.title.clone()
                };
                modules::windows::WindowItem {
                    id: t.id,
                    inst: None,
                    app_id,
                    title,
                    focused: t.focused,
                    attention: false,
                }
            })
            .collect();
        let query = self.shell.world.search.clone();
        self.shell.world.windows.set_windows(items, &query);
    }

    fn flush_window_ops(&mut self) {
        let ops = std::mem::take(&mut self.shell.world.windows.pending_ops);
        if ops.is_empty() {
            return;
        }
        let seat = self.seat_state.seats().next();
        for op in ops {
            match op {
                modules::windows::WindowOp::Activate(id) => {
                    if let (Some(ref seat), Some(entry)) =
                        (&seat, self.toplevels.iter().find(|t| t.id == id))
                    {
                        entry.handle.activate(seat);
                    }
                }
                modules::windows::WindowOp::Close(id) => {
                    if let Some(entry) = self.toplevels.iter().find(|t| t.id == id) {
                        entry.handle.close();
                    }
                }
            }
        }
    }

    fn request_fetch(&mut self, idx: usize) {
        if let Some(panel) = self.shell.cfg.panels.get(idx) {
            let cmds = modules::World::cmds_for_panel(panel);
            self.last_fetch = Some(Instant::now());
            modules::spawn_fetch(self.fetch_tx.clone(), self.fetch_busy.clone(), cmds);
        }
    }

    fn sync_all(&mut self) {
        self.flush_window_ops();
        if self.shell.open != self.prev_open {
            self.prev_open = self.shell.open;
            if let Some(idx) = self.shell.open {
                self.request_fetch(idx);
            } else {
                self.last_fetch = None;
            }
        }
        let qh = self.qh.clone();
        for os in &mut self.outputs {
            os.apply_mode(&qh, &self.compositor, &self.shell);
        }
    }

    fn create_output_surface(&mut self, output: wl_output::WlOutput) {
        if self.outputs.iter().any(|o| o.output == output) {
            return;
        }
        let Ok(pool) = SlotPool::new(4096, &self.shm) else {
            return;
        };
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Top,
            Some("infynitae-edge"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();

        self.outputs.push(OutputSurface {
            output,
            layer,
            pool,
            slots: [None, None],
            width: 0,
            height: 0,
            buf_size: (0, 0),
            configured: false,
            mapped_closed: false,
            dirty: false,
            frame_pending: false,
        });
    }

    fn surface_dims(&self, surface: &wl_surface::WlSurface) -> Option<(f64, f64)> {
        self.outputs
            .iter()
            .find(|o| o.layer.wl_surface() == surface && o.configured)
            .map(|o| (o.width as f64, o.height as f64))
    }

    fn handle_signal_byte(&mut self, b: u8) {
        let changed = match b {
            b'Q' => {
                self.exit = true;
                false
            }
            b'W' => self.shell.toggle_named("windows"),
            b'C' => self.shell.toggle_named("control"),
            b'V' => self.shell.toggle_named("clipboard"),
            b'A' => self.shell.toggle_named("apps"),
            b'P' => {
                let path = runtime_path(PANEL_FILE);
                let name = std::fs::read_to_string(&path).unwrap_or_default();
                let _ = std::fs::remove_file(&path);
                let trimmed = name.trim();
                if !trimmed.is_empty() {
                    self.shell.toggle_named(trimmed)
                } else {
                    false
                }
            }
            b'K' => {
                if let Some(idx) = self.shell.open {
                    self.request_fetch(idx);
                }
                false
            }
            _ => false,
        };
        if changed {
            self.sync_all();
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if let Some(os) = self
            .outputs
            .iter_mut()
            .find(|o| o.layer.wl_surface() == surface)
        {
            os.frame_pending = false;
            if os.dirty {
                os.render_if_needed(qh, &self.shell);
            }
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.create_output_surface(output);
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.outputs.retain(|o| o.output != output);
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        self.outputs.retain(|o| &o.layer != layer);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if let Some(os) = self.outputs.iter_mut().find(|o| &o.layer == layer) {
            os.width = configure.new_size.0.max(1);
            os.height = configure.new_size.1.max(1);
            os.configured = true;
            os.apply_mode(qh, &self.compositor, &self.shell);
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
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
        if capability == Capability::Touch && self.touch.is_none() {
            self.touch = self.seat_state.get_touch(qh, &seat).ok();
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(kb) = self.keyboard.take() {
                kb.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(ptr) = self.pointer.take() {
                ptr.release();
            }
        }
        if capability == Capability::Touch {
            if let Some(t) = self.touch.take() {
                t.release();
            }
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
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        let key = if event.keysym == Keysym::Escape {
            Key::Escape
        } else if self.modifiers.shift && matches!(event.keysym, Keysym::Return | Keysym::KP_Enter)
        {
            Key::ShiftReturn
        } else if self.modifiers.logo {
            let ch = event
                .keysym
                .key_char()
                .or_else(|| char::from_u32(event.keysym.raw()));
            match ch {
                Some(c) if c.is_ascii_alphabetic() => Key::Super(c.to_ascii_lowercase()),
                _ => Key::Sym {
                    keysym: event.keysym.raw(),
                    ch,
                },
            }
        } else {
            let ch = event
                .utf8
                .as_deref()
                .and_then(|s| s.chars().next())
                .or_else(|| event.keysym.key_char());
            Key::Sym {
                keysym: event.keysym.raw(),
                ch,
            }
        };
        if self.shell.key(key) {
            self.sync_all();
        }
    }

    fn repeat_key(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.press_key(conn, qh, keyboard, serial, event);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let mut changed = false;
        for event in events {
            let Some((w, h)) = self.surface_dims(&event.surface) else {
                continue;
            };
            let (x, y) = event.position;
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    changed |= self.shell.pointer_motion(w, h, x, y);
                }
                PointerEventKind::Leave { .. } => {
                    changed |= self.shell.pointer_leave();
                }
                PointerEventKind::Press { button, .. } => {
                    changed |= self.shell.click(w, h, x, y, button);
                }
                PointerEventKind::Release { .. } => {
                    self.shell.pointer_release();
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let dy = if vertical.discrete != 0 {
                        vertical.discrete as f64
                    } else {
                        vertical.absolute
                    };
                    if dy.abs() > 0.0 {
                        changed |= self.shell.scroll(w, h, x, y, dy);
                    }
                }
            }
        }
        if changed {
            self.sync_all();
        }
    }
}

impl TouchHandler for App {
    fn down(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _touch: &wl_touch::WlTouch,
        _serial: u32,
        _time: u32,
        surface: wl_surface::WlSurface,
        _id: i32,
        position: (f64, f64),
    ) {
        if let Some((w, h)) = self.surface_dims(&surface) {
            if self.shell.click(w, h, position.0, position.1, BTN_LEFT) {
                self.sync_all();
            }
        }
    }

    fn up(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        _: u32,
        _: i32,
    ) {
    }

    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        _: i32,
        _: (f64, f64),
    ) {
    }

    fn shape(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: i32,
        _: f64,
        _: f64,
    ) {
    }

    fn orientation(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: i32,
        _: f64,
    ) {
    }

    fn cancel(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_touch!(App);
delegate_layer!(App);
delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for App {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            let id = state.next_toplevel_id;
            state.next_toplevel_id += 1;
            state.toplevels.push(ToplevelEntry {
                id,
                handle: toplevel,
                title: String::new(),
                app_id: String::new(),
                focused: false,
            });
        }
    }

    smithay_client_toolkit::reexports::client::event_created_child!(
        App,
        ZwlrForeignToplevelManagerV1,
        [
            zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
        ]
    );
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for App {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(t) = state.toplevels.iter_mut().find(|t| &t.handle == handle) {
                    t.title = title;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(t) = state.toplevels.iter_mut().find(|t| &t.handle == handle) {
                    t.app_id = app_id;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: raw } => {
                let activated = zwlr_foreign_toplevel_handle_v1::State::Activated as u32;
                let is_active = raw
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|&c| u32::from_ne_bytes(c) == activated);
                if let Some(t) = state.toplevels.iter_mut().find(|t| &t.handle == handle) {
                    t.focused = is_active;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                state.sync_toplevels_to_world();
                if state.shell.open.is_some() {
                    state.sync_all();
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                handle.destroy();
                state.toplevels.retain(|t| &t.handle != handle);
                state.sync_toplevels_to_world();
                if state.shell.open.is_some() {
                    state.sync_all();
                }
            }
            _ => {}
        }
    }
}

fn setup_signal_pipe() -> Result<OwnedFd, String> {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
        return Err("failed to create signal pipe".to_string());
    }
    let rd = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let wr = fds[1];
    let rt = libc::SIGRTMIN();
    let pairs: &[(i32, u8)] = &[
        (libc::SIGTERM, b'Q'),
        (libc::SIGINT, b'Q'),
        (libc::SIGUSR1, b'W'),
        (libc::SIGUSR2, b'C'),
        (rt, b'V'),
        (rt + 1, b'A'),
        (rt + 2, b'P'),
        (rt + 3, b'K'),
    ];
    for &(sig, byte) in pairs {
        let _ = unsafe {
            signal_hook_registry::register(sig, move || {
                let buf = [byte];
                let _ = libc::write(wr, buf.as_ptr().cast(), 1);
            })
        };
    }
    Ok(rd)
}

pub fn run(initial_panel: Option<&str>) -> Result<(), String> {
    if stop_running() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let pid_file = runtime_path(PID_FILE);
    let _ = std::fs::write(&pid_file, format!("{}\n", std::process::id()));

    let res = run_loop(initial_panel);
    let _ = std::fs::remove_file(&pid_file);
    res
}

fn run_loop(initial_panel: Option<&str>) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let (globals, event_queue) = registry_queue_init(&conn).map_err(|e| e.to_string())?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|e| format!("zwlr_layer_shell_v1: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
    let _toplevel_mgr: Option<ZwlrForeignToplevelManagerV1> = globals.bind(&qh, 1..=3, ()).ok();

    let mut shell = Shell::new();
    shell.world.live = true;
    if let Some(name) = initial_panel {
        shell.toggle_named(name);
    }

    let (fetch_tx, fetch_rx) = channel::channel::<Vec<(String, String)>>();
    let fetch_busy = Arc::new(AtomicBool::new(false));

    let output_state = OutputState::new(&globals, &qh);
    let existing_outputs: Vec<_> = output_state.outputs().collect();

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state,
        compositor,
        layer_shell,
        shm,
        qh: qh.clone(),
        outputs: Vec::new(),
        toplevels: Vec::new(),
        next_toplevel_id: 1,
        keyboard: None,
        pointer: None,
        touch: None,
        modifiers: Modifiers::default(),
        shell,
        fetch_tx,
        fetch_busy,
        last_fetch: None,
        prev_open: None,
        exit: false,
    };

    if let Some(idx) = app.shell.open {
        app.prev_open = Some(idx);
        app.request_fetch(idx);
    }

    for output in existing_outputs {
        app.create_output_surface(output);
    }

    let mut event_loop: EventLoop<App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let loop_handle = event_loop.handle();

    loop_handle
        .insert_source(fetch_rx, |evt, _, app| {
            if let channel::Event::Msg(pairs) = evt {
                let mut any = false;
                for (k, v) in pairs {
                    any |= app.shell.world.apply(&k, &v);
                }
                if any && app.shell.open.is_some() {
                    app.sync_all();
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let sig_rd = setup_signal_pipe()?;
    loop_handle
        .insert_source(
            Generic::new(sig_rd, Interest::READ, Mode::Level),
            |_, fd, app| {
                let mut buf = [0u8; 32];
                loop {
                    let n =
                        unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                    if n <= 0 {
                        break;
                    }
                    for &b in &buf[..n as usize] {
                        app.handle_signal_byte(b);
                    }
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| e.to_string())?;

    WaylandSource::new(conn, event_queue)
        .insert(loop_handle)
        .map_err(|e| e.to_string())?;

    const REFRESH: Duration = Duration::from_secs(2);
    while !app.exit {
        let timeout = if let Some(idx) = app.shell.open {
            let elapsed = app.last_fetch.map(|t| t.elapsed()).unwrap_or(REFRESH);
            if elapsed >= REFRESH {
                app.request_fetch(idx);
                Some(REFRESH)
            } else {
                Some(REFRESH - elapsed)
            }
        } else {
            None
        };
        event_loop
            .dispatch(timeout, &mut app)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
