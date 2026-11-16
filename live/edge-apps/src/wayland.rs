//! The Wayland client of `infynitae-edge-apps` (`docs/EDGE_APPS.md` section 5): one layer
//! surface per output on the Top layer, two shm buffers each, drawn on frame callbacks, nothing
//! drawn while closed, toplevel list from the compositor, signals and request files, worker
//! threads for sources, detached actions, and the `org.freedesktop.Notifications` strip.

use crate::card::{Action, Button, Data, Source, Window};
use crate::paths::runtime_path;
use crate::shell::{OutputId, Shell, ARRIVE_LIFT};
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
            delegate_noop,
            globals::registry_queue_init,
            protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface, wl_touch},
            Connection, Dispatch, QueueHandle,
        },
        protocols::wp::{
            cursor_shape::v1::client::{
                wp_cursor_shape_device_v1::{self, WpCursorShapeDeviceV1},
                wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
            },
            fractional_scale::v1::client::{
                wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
                wp_fractional_scale_v1::{self, WpFractionalScaleV1},
            },
            viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
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
        pointer::{
            AxisScroll, PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT, BTN_MIDDLE,
            BTN_RIGHT,
        },
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
use std::collections::HashMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// the same files as today's edge: only one edge runs at a time, and every caller finds it so
const PID_FILE: &str = "infynitae-edge-rs.pid";
const PANEL_FILE: &str = "infynitae-edge-rs.panel";
const CONFIG_STAT_INTERVAL: Duration = Duration::from_millis(250);

/// The running edge's pid (this one's or today's: any `infynitae-edge-*`).
pub fn running_pid() -> Option<i32> {
    let text = std::fs::read_to_string(runtime_path(PID_FILE)).ok()?;
    let pid: i32 = text.trim().parse().ok()?;
    if pid <= 1 || pid == std::process::id() as i32 || unsafe { libc::kill(pid, 0) } != 0 {
        return None;
    }
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let comm = comm.trim();
    (comm.starts_with("infynitae-edge-") || comm == "infynitae-edge").then_some(pid)
}

/// Asks the running edge to open or close panel `name`.
pub fn send_panel(name: &str) -> bool {
    let Some(pid) = running_pid() else {
        return false;
    };
    if std::fs::write(runtime_path(PANEL_FILE), format!("{name}\n")).is_err() {
        return false;
    }
    unsafe { libc::kill(pid, libc::SIGRTMIN() + 2) == 0 }
}

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

fn scaled_dimension(logical: u32, scale_120: u32) -> u32 {
    ((u64::from(logical) * u64::from(scale_120.max(1)) + 60) / 120).max(1) as u32
}

struct OutputSurface {
    id: OutputId,
    output: wl_output::WlOutput,
    frac_scale: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    layer: LayerSurface,
    pool: SlotPool,
    slots: [Option<BufferSlot>; 2],
    last_slot: usize,
    /// the box last committed (in either buffer, in buffer px): the next frame damages it too
    committed_box: Option<(i32, i32, i32, i32)>,
    width: u32,
    height: u32,
    integer_scale: i32,
    fractional_scale_120: Option<u32>,
    last_buffer_scale: i32,
    last_viewport_dest: Option<(i32, i32)>,
    buf_size: (u32, u32),
    configured: bool,
    mapped_closed: bool,
    clear_pending: bool,
    /// a fullscreen window is on this screen: closed, the surface is unmapped, so the window can
    /// be shown directly (scanout, tearing)
    fullscreen_under: bool,
    unmapped: bool,
    dirty: bool,
    frame_pending: bool,
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
    fn effective_scale_120(&self) -> u32 {
        self.fractional_scale_120
            .unwrap_or_else(|| (self.integer_scale.max(1) as u32) * 120)
    }

    fn effective_scale(&self) -> f64 {
        if self.viewport.is_some() {
            f64::from(self.effective_scale_120()) / 120.0
        } else {
            f64::from(self.integer_scale.max(1))
        }
    }

    fn target_buf_size(&self) -> (u32, u32) {
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

    fn apply_surface_scale(&mut self) {
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

    fn set_input_region(&self, compositor: &CompositorState, shell: &Shell) {
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
                let idx = shell.open.unwrap();
                // opened by its key: the whole screen, so a click outside reaches the edge and
                // closes it; opened by hover: the panel and the way to it, so leaving closes it
                let r = if shell.opened_by_hover {
                    shell.hover_box(idx, w, h)
                } else {
                    (0.0, 0.0, w, h)
                };
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

    fn apply_mode(&mut self, qh: &QueueHandle<App>, compositor: &CompositorState, shell: &Shell) {
        let show_here = shell.visible_on(self.id);
        if !show_here && self.fullscreen_under {
            if !self.unmapped {
                // a null buffer unmaps a layer surface; it maps again after a commit and its configure
                self.layer.wl_surface().attach(None, 0, 0);
                self.layer.commit();
                self.unmapped = true;
                self.configured = false;
                self.mapped_closed = false;
                self.clear_pending = false;
                self.dirty = false;
                self.frame_pending = false;
                self.committed_box = None;
                self.last_viewport_dest = None;
                for slot in self.slots.iter_mut().flatten() {
                    slot.prev_box = None;
                }
            }
            return;
        }
        if self.unmapped {
            self.unmapped = false;
            self.layer.commit();
            return;
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

    fn ensure_pool_size(&mut self) {
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

    fn acquire_slot(&mut self) -> Option<usize> {
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

    /// Closed: a clear frame, damaging what this buffer held and what is on screen (the other
    /// buffer's box); with both buffers still held, again on the next frame callback.
    fn clear_and_commit(&mut self, qh: &QueueHandle<App>, any_visible: bool) {
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

    fn render_if_needed(&mut self, qh: &QueueHandle<App>, shell: &Shell) {
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

struct Toplevel {
    id: u64,
    handle: ZwlrForeignToplevelHandleV1,
    title: String,
    app_id: String,
    focused: bool,
    fullscreen: bool,
    outputs: Vec<wl_output::WlOutput>,
}

/// What comes back to the loop from a worker thread.
enum Fetched {
    /// Result from a source's worker along with the `Source` object so its state (such as CPU
    /// `/proc/stat` deltas) persists across fetches while the panel stays open.
    Source(&'static str, &'static str, Data, Box<dyn Source>),
    /// One-off result from `Action::Run` after a card command finishes.
    Data(&'static str, &'static str, Data),
    /// Installed apps re-read on a worker thread along with the folder mtimes statted before the
    /// read started.
    Apps(Vec<crate::app::App>, Vec<Option<std::time::SystemTime>>),
}

type SourceId = (&'static str, &'static str);

struct SourceEntry {
    last: Instant,
    every: Option<Duration>,
    source: Option<Box<dyn Source>>,
}

/// When each source on the open panel last ran, its interval (`None` = once per panel opening),
/// and its live `Source` object kept between fetches while the panel remains open.
#[derive(Default)]
struct FetchSchedule {
    open_panel: Option<usize>,
    records: HashMap<SourceId, SourceEntry>,
    in_flight: HashMap<SourceId, Option<Duration>>,
}

impl FetchSchedule {
    fn sync_panel(&mut self, open: Option<usize>, sources: &[SourceId]) {
        if self.open_panel != open {
            self.open_panel = open;
            self.records.clear();
        }
        if open.is_none() {
            return;
        }
        self.records.retain(|id, _| sources.contains(id));
    }

    fn take_due(
        &mut self,
        open: Option<usize>,
        sources: &[SourceId],
        force: bool,
        now: Instant,
        mut make: impl FnMut(&'static str, &'static str) -> Option<Box<dyn Source>>,
    ) -> Vec<(SourceId, Box<dyn Source>)> {
        self.sync_panel(open, sources);
        if open.is_none() {
            return Vec::new();
        }
        let mut due_list = Vec::new();
        for &id in sources {
            if let Some(&every) = self.in_flight.get(&id) {
                // Remember in-flight interval across reopen so finishing before close-reopen settle
                // does not schedule a duplicate fetch for the same opening.
                self.records.entry(id).or_insert(SourceEntry {
                    last: now,
                    every,
                    source: None,
                });
                continue;
            }
            let due = match self.records.get(&id) {
                None => true,
                Some(_) if force => true,
                Some(entry) => entry
                    .every
                    .is_some_and(|every| now.saturating_duration_since(entry.last) >= every),
            };
            if !due {
                continue;
            }
            let (src, every) = match self.records.get_mut(&id) {
                Some(entry) => {
                    let src = match entry.source.take() {
                        Some(s) => s,
                        None => match make(id.0, id.1) {
                            Some(s) => s,
                            None => continue,
                        },
                    };
                    let every = src.every();
                    entry.last = now;
                    entry.every = every;
                    (src, every)
                }
                None => {
                    let Some(src) = make(id.0, id.1) else {
                        continue;
                    };
                    let every = src.every();
                    self.records.insert(
                        id,
                        SourceEntry {
                            last: now,
                            every,
                            source: None,
                        },
                    );
                    (src, every)
                }
            };
            self.in_flight.insert(id, every);
            due_list.push((id, src));
        }
        due_list
    }

    fn mark_done(&mut self, id: SourceId, src: Option<Box<dyn Source>>, now: Instant) {
        self.in_flight.remove(&id);
        if self.open_panel.is_some() {
            if let Some(entry) = self.records.get_mut(&id) {
                entry.last = now;
                if let Some(s) = src {
                    entry.source = Some(s);
                }
            }
        }
    }

    fn next_timeout(&self, sources: &[SourceId], now: Instant) -> Option<Duration> {
        self.open_panel?;
        sources
            .iter()
            .filter(|id| !self.in_flight.contains_key(*id))
            .filter_map(|id| {
                let entry = self.records.get(id)?;
                let every = entry.every?;
                let due_at = entry.last.checked_add(every)?;
                Some(due_at.saturating_duration_since(now))
            })
            .min()
    }
}

struct SourceWorker {
    tx: std::sync::mpsc::Sender<Box<dyn Source>>,
    #[cfg(test)]
    handle: Option<std::thread::JoinHandle<()>>,
}

fn spawn_worker(id: SourceId, fetch_tx: Sender<Fetched>) -> SourceWorker {
    let (tx, rx) = std::sync::mpsc::channel::<Box<dyn Source>>();
    let (prov, key) = id;
    let run = move || {
        while let Ok(mut src) = rx.recv() {
            let data = src.fetch();
            if fetch_tx
                .send(Fetched::Source(prov, key, data, src))
                .is_err()
            {
                break;
            }
        }
    };
    let _handle = match std::thread::Builder::new()
        .name(format!("edge-{prov}-{key}"))
        .spawn(run)
    {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("infynitae-edge-apps: spawn worker {prov}/{key}: {e}");
            None
        }
    };
    SourceWorker {
        tx,
        #[cfg(test)]
        handle: _handle,
    }
}

/// Tracks desktop-entry folder modification times so `app::load` runs on a worker thread only
/// when a panel opens and at least one folder's modification time changed.
#[derive(Default)]
struct AppRefresh {
    dirs: Vec<std::path::PathBuf>,
    last_mtimes: Vec<Option<std::time::SystemTime>>,
    last_open: Option<usize>,
    in_flight: bool,
}

impl AppRefresh {
    fn new(dirs: Vec<std::path::PathBuf>, last_mtimes: Vec<Option<std::time::SystemTime>>) -> Self {
        Self {
            dirs,
            last_mtimes,
            last_open: None,
            in_flight: false,
        }
    }

    fn sync_closed(&mut self) {
        self.last_open = None;
    }

    fn check_open(
        &mut self,
        open: Option<usize>,
    ) -> Option<(Vec<std::path::PathBuf>, Vec<Option<std::time::SystemTime>>)> {
        let Some(idx) = open else {
            self.last_open = None;
            return None;
        };
        if self.last_open == Some(idx) {
            return None;
        }
        self.last_open = Some(idx);
        if self.in_flight || self.dirs.is_empty() {
            return None;
        }
        let mtimes = crate::app::dir_mtimes(&self.dirs);
        if mtimes == self.last_mtimes {
            return None;
        }
        self.in_flight = true;
        Some((self.dirs.clone(), mtimes))
    }

    fn mark_done(&mut self, mtimes: Vec<Option<std::time::SystemTime>>, applied: bool) {
        self.in_flight = false;
        if applied {
            self.last_mtimes = mtimes;
        }
    }
}

fn spawn_apps_worker(
    dirs: Vec<std::path::PathBuf>,
    mtimes: Vec<Option<std::time::SystemTime>>,
    fetch_tx: Sender<Fetched>,
) -> bool {
    match std::thread::Builder::new()
        .name("edge-apps-scan".into())
        .spawn(move || {
            let apps = crate::app::load(&dirs);
            let _ = fetch_tx.send(Fetched::Apps(apps, mtimes));
        }) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("infynitae-edge-apps: spawn apps worker: {e}");
            false
        }
    }
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    viewporter: Option<WpViewporter>,
    fractional_scale_mgr: Option<WpFractionalScaleManagerV1>,
    cursor_shape_mgr: Option<WpCursorShapeManagerV1>,
    qh: QueueHandle<App>,
    outputs: Vec<OutputSurface>,
    next_output_id: OutputId,
    toplevels: Vec<Toplevel>,
    next_toplevel_id: u64,
    fullscreen_changed: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_device: Option<WpCursorShapeDeviceV1>,
    touch: Option<wl_touch::WlTouch>,
    keyboard_output: Option<OutputId>,
    current_pointer_output: Option<OutputId>,
    last_pointer_output: Option<OutputId>,
    last_display_scale: f64,
    last_config_check: Option<Instant>,
    #[cfg(test)]
    clock: Option<std::rc::Rc<std::cell::Cell<Instant>>>,
    #[cfg(test)]
    config_path: Option<std::path::PathBuf>,
    #[cfg(test)]
    config_stat_calls: std::rc::Rc<std::cell::Cell<usize>>,
    modifiers: Modifiers,
    shell: Shell,
    fetch_tx: Sender<Fetched>,
    fetch: FetchSchedule,
    app_refresh: AppRefresh,
    /// one long-lived worker thread per active source while its panel is open
    workers: HashMap<SourceId, SourceWorker>,
    /// cards with a command running, and the latest one waiting after it
    queued: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Option<String>>>>,
    was_animating: bool,
    /// the notification pop-ups' strip, made with the first pop-up
    popups: Option<PopupLayer>,
    /// active touch point on the notification pop-ups strip (id, start position)
    popup_touch: Option<(i32, (f64, f64))>,
    /// the session bus connection serving org.freedesktop.Notifications
    notify_conn: Option<zbus::blocking::Connection>,
    /// the compositor's shell IPC socket (`$XDG_RUNTIME_DIR/infynitae/shell.sock`)
    shell_sock: Option<std::os::unix::net::UnixStream>,
    shell_buf: Vec<u8>,
    exit: bool,
}

impl App {
    /// After the notifications changed: the senders told (signals), the cards' history from it,
    /// the pop-ups drawn (their strip made on the first screen with the first pop-up).
    fn sync_popups(&mut self) {
        let out = std::mem::take(&mut self.shell.notify.out);
        if let Some(conn) = &self.notify_conn {
            crate::notify_server::send(conn, out);
        }
        self.shell.notes = self
            .shell
            .notify
            .history
            .iter()
            .map(|n| crate::card::Note {
                id: n.id,
                app: n.app.clone(),
                summary: n.summary.clone(),
                body: n.body.clone(),
                actions: n.actions.clone(),
                urgency: n.urgency,
                time: n.time.clone(),
            })
            .collect();
        if !self.shell.notify.popups.is_empty() {
            self.reload_config();
        }
        if self.popups.is_none() && !self.shell.notify.popups.is_empty() {
            if let Some(output) = self.outputs.first().map(|o| o.output.clone()) {
                self.popups = PopupLayer::new(self, output);
            }
        }
        if let Some(pl) = self.popups.as_mut() {
            pl.render(&self.qh, &self.compositor, &self.shell);
        }
        if self.shell.open.is_some() {
            // the open panel's cards list them too
            self.sync_all();
        }
    }

    fn create_output_surface(&mut self, output: wl_output::WlOutput) {
        if self.outputs.iter().any(|o| o.output == output) {
            return;
        }
        let Ok(pool) = SlotPool::new(4096, &self.shm) else {
            return;
        };
        let integer_scale = self
            .output_state
            .info(&output)
            .map_or(1, |i| i.scale_factor.max(1));
        let surface = self.compositor.create_surface(&self.qh);
        let (viewport, frac_scale) = match (&self.viewporter, &self.fractional_scale_mgr) {
            (Some(vp), Some(fs)) => (
                Some(vp.get_viewport(&surface, &self.qh, ())),
                Some(fs.get_fractional_scale(&surface, &self.qh, surface.clone())),
            ),
            _ => (None, None),
        };
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
        let id = self.next_output_id;
        self.next_output_id += 1;
        self.outputs.push(OutputSurface {
            id,
            output,
            frac_scale,
            viewport,
            layer,
            pool,
            slots: [None, None],
            last_slot: 0,
            committed_box: None,
            width: 0,
            height: 0,
            integer_scale,
            fractional_scale_120: None,
            last_buffer_scale: 1,
            last_viewport_dest: None,
            buf_size: (0, 0),
            configured: false,
            mapped_closed: false,
            clear_pending: false,
            fullscreen_under: false,
            unmapped: false,
            dirty: false,
            frame_pending: false,
        });
    }

    fn key_target_output(&self) -> Option<OutputId> {
        let valid = |id: OutputId| self.outputs.iter().any(|o| o.id == id);
        if let Some(id) = self.keyboard_output.filter(|&id| valid(id)) {
            return Some(id);
        }
        if let Some(id) = self.toplevels.iter().find(|t| t.focused).and_then(|t| {
            self.outputs
                .iter()
                .find(|o| t.outputs.contains(&o.output))
                .map(|o| o.id)
        }) {
            return Some(id);
        }
        if let Some(id) = self.current_pointer_output.filter(|&id| valid(id)) {
            return Some(id);
        }
        if let Some(id) = self.last_pointer_output.filter(|&id| valid(id)) {
            return Some(id);
        }
        self.outputs.first().map(|o| o.id)
    }

    fn sync_output_scale(&mut self, output: &wl_output::WlOutput, qh: &QueueHandle<App>) {
        let Some(info) = self.output_state.info(output) else {
            return;
        };
        let scale = info.scale_factor.max(1);
        let mut need_sync = false;
        if let Some(os) = self.outputs.iter_mut().find(|o| &o.output == output) {
            if os.integer_scale != scale {
                os.integer_scale = scale;
                if os.viewport.is_none() {
                    os.mapped_closed = false;
                    if self.shell.visible_on(os.id) {
                        os.dirty = true;
                        need_sync = true;
                    } else {
                        os.apply_mode(qh, &self.compositor, &self.shell);
                    }
                }
            }
        }
        if let Some(pl) = self.popups.as_mut().filter(|p| &p.output == output) {
            if pl.integer_scale != scale {
                pl.integer_scale = scale;
                if pl.viewport.is_none() {
                    pl.render(qh, &self.compositor, &self.shell);
                }
            }
        }
        if need_sync {
            self.sync_all();
        }
    }

    fn update_display_scale(&mut self) {
        let target = self
            .shell
            .visible_output()
            .or_else(|| self.key_target_output());
        let scale = target
            .and_then(|id| self.outputs.iter().find(|o| o.id == id))
            .map_or(1.0, OutputSurface::effective_scale);
        if (scale - self.last_display_scale).abs() > 1e-4 {
            self.last_display_scale = scale;
            crate::icons::set_display_scale(scale);
            if self.shell.open.is_some() {
                self.shell.fetch_needed = true;
            }
        }
    }

    fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(c) = &self.clock {
            return c.get();
        }
        Instant::now()
    }

    /// Reloads config, theme and boards when changed on disk (statted at most every 250 ms),
    /// marking active surfaces dirty.
    fn reload_config(&mut self) -> bool {
        let now = self.now();
        if self
            .last_config_check
            .is_some_and(|prev| now.saturating_duration_since(prev) < CONFIG_STAT_INTERVAL)
        {
            return false;
        }
        self.last_config_check = Some(now);
        #[cfg(test)]
        self.config_stat_calls
            .set(self.config_stat_calls.get().saturating_add(1));
        #[cfg(test)]
        let changed = match self.config_path.clone() {
            Some(p) => self.shell.reload_if_changed_at(&p),
            None => self.shell.reload_if_changed(),
        };
        #[cfg(not(test))]
        let changed = self.shell.reload_if_changed();
        if !changed {
            return false;
        }
        if self.shell.visible() {
            for os in &mut self.outputs {
                if self.shell.visible_on(os.id) {
                    os.dirty = true;
                }
            }
        }
        true
    }

    /// After anything changed: the actions run, the sources fetched as needed, every surface drawn.
    fn sync_all(&mut self) {
        self.reload_config();
        self.run_actions();
        if self.shell.open.is_some() {
            self.shell.now = chrono::Local::now().naive_local();
        }
        self.update_display_scale();
        let force = self.shell.take_fetch_needed();
        self.fetch_due(force);
        for os in &mut self.outputs {
            os.apply_mode(&self.qh, &self.compositor, &self.shell);
        }
    }

    fn apply_modes(&mut self) {
        for os in &mut self.outputs {
            os.fullscreen_under = self
                .toplevels
                .iter()
                .any(|t| t.fullscreen && t.outputs.contains(&os.output));
        }
        self.sync_all();
    }

    /// The open panel's sources, fetched on one long-lived worker thread per source while open;
    /// `force` fetches again before its time.
    fn fetch_due(&mut self, force: bool) {
        let reloaded = if self.shell.open.is_some() {
            self.reload_config()
        } else {
            false
        };
        let Some(open) = self.shell.open else {
            self.fetch.sync_panel(None, &[]);
            self.workers.clear();
            self.app_refresh.sync_closed();
            return;
        };
        if let Some((dirs, mtimes)) = self.app_refresh.check_open(Some(open)) {
            if !spawn_apps_worker(dirs, mtimes, self.fetch_tx.clone()) {
                self.app_refresh.in_flight = false;
            }
        }
        let force = force || self.shell.take_fetch_needed();
        let sources = self.shell.sources();
        self.workers.retain(|id, _| sources.contains(id));
        let now = Instant::now();
        for (id, src) in self
            .fetch
            .take_due(Some(open), &sources, force, now, |p, k| {
                crate::provider::by_id(p).and_then(|prov| prov.source(k))
            })
        {
            let worker = self
                .workers
                .entry(id)
                .or_insert_with(|| spawn_worker(id, self.fetch_tx.clone()));
            if let Err(std::sync::mpsc::SendError(src)) = worker.tx.send(src) {
                *worker = spawn_worker(id, self.fetch_tx.clone());
                let _ = worker.tx.send(src);
            }
        }
        if reloaded {
            for os in &mut self.outputs {
                os.apply_mode(&self.qh, &self.compositor, &self.shell);
            }
        }
    }

    fn fetch_timeout(&self, now: Instant) -> Option<Duration> {
        self.shell.open?;
        let sources = self.shell.sources();
        self.fetch
            .next_timeout(&sources, now)
            .map(|d| d.max(Duration::from_millis(5)))
    }

    /// The cards' actions: commands on a thread each (then their sources fetched again), apps and
    /// links started detached, windows focused or closed through the compositor's list.
    fn run_actions(&mut self) {
        let actions = std::mem::take(&mut self.shell.actions);
        let mut notes_changed = false;
        for (a, card, prov, keys) in actions {
            match a {
                Action::Run(cmd) => {
                    // one command at a time per card, the latest waiting one next: a slider dragged
                    // sends many levels, and run in no order an older one could land last
                    let mut q = self.queued.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(waiting) = q.get_mut(&card) {
                        *waiting = Some(cmd);
                        continue;
                    }
                    q.insert(card.clone(), None);
                    drop(q);
                    let (tx, queued) = (self.fetch_tx.clone(), self.queued.clone());
                    let keys: Vec<&'static str> = keys.to_vec();
                    std::thread::spawn(move || {
                        let mut cmd = cmd;
                        loop {
                            if let Err(e) = Command::new("sh")
                                .arg("-c")
                                .arg(&cmd)
                                .stdin(Stdio::null())
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .status()
                            {
                                eprintln!("infynitae-edge-apps: run {cmd}: {e}");
                            }
                            let mut q = queued.lock().unwrap_or_else(|p| p.into_inner());
                            match q.get_mut(&card).and_then(Option::take) {
                                Some(next) => cmd = next,
                                None => {
                                    q.remove(&card);
                                    break;
                                }
                            }
                        }
                        // what the commands changed, shown at once
                        for key in keys {
                            if let Some(mut src) =
                                crate::provider::by_id(prov).and_then(|p| p.source(key))
                            {
                                let _ = tx.send(Fetched::Data(prov, key, src.fetch()));
                            }
                        }
                    });
                }
                Action::Launch(id) => {
                    if let Some(app) = self.shell.apps.iter().find(|x| x.id == id) {
                        let cmd = if app.terminal {
                            format!(
                                "\"$HOME/infynitae/live/launch.sh\" terminal sh -c {}",
                                crate::cards::settings::sq(&app.exec)
                            )
                        } else {
                            app.exec.clone()
                        };
                        spawn_detached(&cmd);
                    }
                    self.shell.close();
                }
                Action::OpenUri(uri) => {
                    spawn_detached(&format!("xdg-open {}", crate::cards::settings::sq(&uri)));
                    self.shell.close();
                }
                Action::Focus(id) => {
                    if let (Some(t), Some(seat)) = (
                        self.toplevels.iter().find(|t| t.id == id),
                        self.seat_state.seats().next(),
                    ) {
                        t.handle.activate(&seat);
                    }
                    self.shell.close();
                }
                Action::CloseWindow(id) => {
                    if let Some(t) = self.toplevels.iter().find(|t| t.id == id) {
                        t.handle.close();
                    }
                }
                Action::Dismiss(id) => notes_changed |= self.shell.notify.dismiss(id),
                Action::Invoke(id, key) => notes_changed |= self.shell.notify.invoke(id, &key),
                Action::ClosePanel => {
                    self.shell.close();
                }
                Action::ShellCommand(line) => {
                    use std::io::Write;
                    let sent = self
                        .shell_sock
                        .as_mut()
                        .is_some_and(|s| s.write_all(line.as_bytes()).is_ok());
                    if !sent {
                        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(runtime_path(
                            "infynitae/shell.sock",
                        )) {
                            let _ = s.write_all(line.as_bytes());
                        }
                    }
                }
                // the shell takes these before they get here
                Action::SetQuery(_) | Action::ShowCards(_) | Action::RemoveCard => {}
            }
        }
        if notes_changed {
            self.sync_popups();
        }
    }

    fn sync_windows(&mut self) {
        self.shell.windows = self
            .toplevels
            .iter()
            .map(|t| Window {
                id: t.id,
                title: t.title.clone(),
                app_id: t.app_id.clone(),
                focused: t.focused,
            })
            .collect();
    }

    fn handle_signal_byte(&mut self, b: u8) {
        let target = self.key_target_output();
        let changed = match b {
            b'Q' => {
                self.exit = true;
                false
            }
            b'W' => self.shell.toggle_named_on(target, "windows"),
            b'C' => self.shell.toggle_named_on(target, "control"),
            b'V' => self.shell.toggle_named_on(target, "clipboard"),
            b'A' => self.shell.toggle_named_on(target, "apps"),
            b'P' => {
                let name = std::fs::read_to_string(runtime_path(PANEL_FILE)).unwrap_or_default();
                let name = name.trim();
                if name == "stop" {
                    self.exit = true;
                    false
                } else {
                    self.shell.toggle_named_on(target, name)
                }
            }
            b'K' => {
                // a question in the ask file, or a cancel; else: fetch the open panel's data again
                // (a script changed something)
                if self.shell.ask_load() {
                    if self.shell.ask.ask.on && self.shell.visible_output().is_none() {
                        self.shell.set_ask_output(target);
                    }
                    true
                } else {
                    self.fetch_due(true);
                    false
                }
            }
            _ => false,
        };
        if changed {
            self.sync_all();
        }
    }

    fn remove_output_matching(&mut self, pred: impl Fn(&OutputSurface) -> bool) {
        let removed: Vec<OutputId> = self
            .outputs
            .iter()
            .filter(|o| pred(o))
            .map(|o| o.id)
            .collect();
        self.outputs.retain(|o| !pred(o));
        for id in removed {
            if self.current_pointer_output == Some(id) {
                self.current_pointer_output = None;
            }
            if self.last_pointer_output == Some(id) {
                self.last_pointer_output = None;
            }
            if self.keyboard_output == Some(id) {
                self.keyboard_output = None;
            }
            let _ = self.shell.touch_cancel_on(id);
            if self.shell.visible_output() == Some(id) {
                self.shell.close();
            }
        }
    }
}

fn spawn_detached(cmd: &str) {
    let label = cmd.to_string();
    match Command::new("setsid")
        .arg("-f")
        .arg("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut c) => {
            std::thread::spawn(move || {
                if let Err(e) = c.wait() {
                    eprintln!("infynitae-edge-apps: wait detached {label}: {e}");
                }
            });
        }
        Err(e) => eprintln!("infynitae-edge-apps: spawn {cmd}: {e}"),
    }
}

fn write_runtime_file(path: &std::path::Path, text: &str) -> bool {
    match std::fs::write(path, text) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("infynitae-edge-apps: write {}: {e}", path.display());
            false
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let factor = new_factor.max(1);
        let mut need_sync = false;
        if let Some(os) = self
            .outputs
            .iter_mut()
            .find(|o| o.layer.wl_surface() == surface)
        {
            if os.integer_scale != factor {
                os.integer_scale = factor;
                if os.viewport.is_none() {
                    os.mapped_closed = false;
                    if self.shell.visible_on(os.id) {
                        os.dirty = true;
                        need_sync = true;
                    } else {
                        os.apply_mode(qh, &self.compositor, &self.shell);
                    }
                }
            }
        }
        if let Some(pl) = self
            .popups
            .as_mut()
            .filter(|p| p.layer.wl_surface() == surface)
        {
            if pl.integer_scale != factor {
                pl.integer_scale = factor;
                if pl.viewport.is_none() {
                    pl.render(qh, &self.compositor, &self.shell);
                }
            }
        }
        if need_sync {
            self.sync_all();
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        let is_output_surface = self.outputs.iter().any(|o| o.layer.wl_surface() == surface);
        if is_output_surface {
            // an animation running (arrival, leaving, a list opening, drag autoscroll): draw again
            let was_visible = self.shell.visible();
            let animating =
                was_visible && (self.shell.tick(Instant::now()) || self.shell.wants_frame());
            let ended = self.was_animating && !animating;
            let closed_finished = was_visible && !self.shell.visible();
            self.was_animating = animating;
            if let Some(os) = self
                .outputs
                .iter_mut()
                .find(|o| o.layer.wl_surface() == surface)
            {
                os.frame_pending = false;
                if animating && self.shell.visible_on(os.id) {
                    os.dirty = true;
                }
                if os.dirty && self.shell.visible_on(os.id) {
                    os.render_if_needed(qh, &self.shell);
                } else if os.clear_pending && !self.shell.visible_on(os.id) {
                    os.clear_and_commit(qh, self.shell.visible());
                }
            }
            if ended || closed_finished {
                // the panel's size settled or its leaving fade ended: input region / clear follows
                self.sync_all();
            }
        }
        if let Some(pl) = self
            .popups
            .as_mut()
            .filter(|p| p.layer.wl_surface() == surface)
        {
            pl.frame_pending = false;
            pl.render(qh, &self.compositor, &self.shell);
        }
    }
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
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.create_output_surface(output.clone());
        self.sync_output_scale(&output, qh);
    }
    fn update_output(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.sync_output_scale(&output, qh);
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.remove_output_matching(|o| o.output == output);
        if self.popups.as_ref().is_some_and(|p| p.output == output) {
            // the next pop-up makes the strip again, on the first screen left
            self.popups = None;
            self.popup_touch = None;
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.remove_output_matching(|o| &o.layer == layer);
        if self.popups.as_ref().is_some_and(|p| &p.layer == layer) {
            self.popups = None;
            self.popup_touch = None;
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(os) = self.outputs.iter_mut().find(|o| &o.layer == layer) {
            os.width = configure.new_size.0.max(1);
            os.height = configure.new_size.1.max(1);
            os.configured = true;
            os.apply_mode(qh, &self.compositor, &self.shell);
        }
        if let Some(pl) = self.popups.as_mut().filter(|p| &p.layer == layer) {
            pl.width = configure.new_size.0.max(1);
            pl.height = configure.new_size.1.max(1);
            pl.configured = true;
            pl.render(qh, &self.compositor, &self.shell);
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
        if capability == Capability::Touch && self.touch.is_none() {
            self.touch = self.seat_state.get_touch(qh, &seat).ok();
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
            if let Some(kb) = self.keyboard.take() {
                kb.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(dev) = self.cursor_device.take() {
                dev.destroy();
            }
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
        if capability == Capability::Touch {
            if let Some(t) = self.touch.take() {
                t.release();
            }
            self.popup_touch = None;
            if self.shell.touch_cancel() {
                self.sync_all();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

/// What a key press means to the open panel.
fn key_of(event: &KeyEvent, m: &Modifiers) -> Option<crate::shell::Key> {
    use crate::shell::Key;
    Some(match event.keysym {
        Keysym::Escape => Key::Escape,
        Keysym::Up | Keysym::KP_Up => Key::Up,
        Keysym::Down | Keysym::KP_Down => Key::Down,
        Keysym::Left | Keysym::KP_Left => Key::Left,
        Keysym::Right | Keysym::KP_Right => Key::Right,
        Keysym::Home | Keysym::KP_Home => Key::Home,
        Keysym::End | Keysym::KP_End => Key::End,
        Keysym::Page_Up | Keysym::KP_Page_Up => Key::PageUp,
        Keysym::Page_Down | Keysym::KP_Page_Down => Key::PageDown,
        Keysym::Delete | Keysym::KP_Delete => Key::Delete,
        Keysym::Tab if m.shift => Key::BackTab,
        Keysym::ISO_Left_Tab => Key::BackTab,
        Keysym::Tab => Key::Tab,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::BackSpace => Key::Backspace,
        k if m.logo => {
            let c = k.key_char()?;
            if !c.is_ascii_alphabetic() {
                return None;
            }
            Key::Super(c)
        }
        _ if m.ctrl || m.alt => return None,
        k => {
            let c = event
                .utf8
                .as_deref()
                .and_then(|s| s.chars().next())
                .or_else(|| k.key_char())?;
            Key::Char(c)
        }
    })
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
        self.keyboard_output = self
            .outputs
            .iter()
            .find(|o| o.layer.wl_surface() == surface)
            .map(|o| o.id);
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(id) = self
            .outputs
            .iter()
            .find(|o| o.layer.wl_surface() == surface)
            .map(|o| o.id)
        {
            if self.keyboard_output == Some(id) {
                self.keyboard_output = None;
            }
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
        if let Some(k) = key_of(&event, &self.modifiers) {
            if self.shell.key(k) {
                self.sync_all();
            }
        }
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        // held: arrows, paging, Backspace and characters repeat; Escape, Enter, Delete and Super do not
        if let Some(k) = key_of(&event, &self.modifiers).filter(|k| {
            !matches!(
                k,
                crate::shell::Key::Escape
                    | crate::shell::Key::Enter
                    | crate::shell::Key::Delete
                    | crate::shell::Key::Super(_)
            )
        }) {
            if self.shell.key(k) {
                self.sync_all();
            }
        }
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
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

/// Converts a Wayland vertical axis event into `(pixels, notches)`.
/// Discrete wheel events (`value120` or `discrete`) scale by 36 px per notch; continuous touchpad
/// events pass `absolute` pixels 1:1 with `0.0` notches.
pub(crate) fn scroll_from_axis(vertical: &AxisScroll) -> (f64, f64) {
    if vertical.value120 != 0 {
        let notches = f64::from(vertical.value120) / 120.0;
        (notches * crate::card::SCROLL_NOTCH_PX, notches)
    } else if vertical.discrete != 0 {
        let notches = f64::from(vertical.discrete);
        (notches * crate::card::SCROLL_NOTCH_PX, notches)
    } else {
        (vertical.absolute, 0.0)
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
        let mut changed = false;
        let mut pop_changed = false;
        for event in events {
            if let PointerEventKind::Enter { serial } = event.kind {
                if let Some(dev) = &self.cursor_device {
                    dev.set_shape(serial, wp_cursor_shape_device_v1::Shape::Default);
                }
            }
            if let Some(pl) = self
                .popups
                .as_mut()
                .filter(|p| p.layer.wl_surface() == &event.surface)
            {
                // the pop-ups: pointing holds one's time; a click on its x, a button, or the card
                use crate::notify::{hit, popup_at, Hit};
                let w = pl.width as f64;
                let now = Instant::now();
                let n = &mut self.shell.notify;
                let before = pl.pointer.and_then(|p| hit(n, w, p));
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        pl.pointer = Some(event.position);
                        let over = popup_at(n, w, event.position);
                        pop_changed |= n.hover(over, now) || hit(n, w, event.position) != before;
                    }
                    PointerEventKind::Leave { .. } => {
                        pl.pointer = None;
                        pop_changed |= n.hover(None, now) || before.is_some();
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        pop_changed |= match hit(n, w, event.position) {
                            Some(Hit::Dismiss(id)) => n.dismiss(id),
                            Some(Hit::Action(id, key)) => n.invoke(id, &key),
                            Some(Hit::Open(id)) => n.invoke(id, "default"),
                            None => false,
                        };
                    }
                    _ => {}
                }
                continue;
            }
            let Some((os_id, w, h)) = self
                .outputs
                .iter()
                .find(|o| o.layer.wl_surface() == &event.surface && o.configured)
                .map(|o| (o.id, o.width as f64, o.height as f64))
            else {
                continue;
            };
            let (x, y) = event.position;
            let now = Instant::now();
            changed |= match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.current_pointer_output = Some(os_id);
                    self.last_pointer_output = Some(os_id);
                    self.shell.pointer_motion_on(os_id, w, h, x, y, now)
                }
                PointerEventKind::Leave { .. } => {
                    if self.current_pointer_output == Some(os_id) {
                        self.current_pointer_output = None;
                    }
                    self.shell.pointer_leave_on(os_id, now)
                }
                PointerEventKind::Press { button, .. } => {
                    let b = match button {
                        BTN_LEFT => Button::Left,
                        BTN_RIGHT => Button::Right,
                        BTN_MIDDLE => Button::Middle,
                        _ => continue,
                    };
                    self.shell.click_on(os_id, w, h, x, y, b)
                }
                PointerEventKind::Release { .. } => self.shell.release_on(os_id),
                PointerEventKind::Axis { vertical, .. } => {
                    let (dy, notches) = scroll_from_axis(&vertical);
                    if notches != 0.0 {
                        self.shell.scroll_notches_on(os_id, w, h, x, y, notches)
                    } else {
                        dy.abs() > 0.0 && self.shell.scroll_on(os_id, w, h, x, y, dy)
                    }
                }
            };
        }
        if pop_changed {
            self.sync_popups();
        }
        if changed || !self.shell.actions.is_empty() {
            self.sync_all();
        }
    }
}

impl TouchHandler for App {
    fn down(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        _: u32,
        surface: wl_surface::WlSurface,
        id: i32,
        position: (f64, f64),
    ) {
        if self
            .popups
            .as_ref()
            .is_some_and(|p| p.layer.wl_surface() == &surface)
        {
            if self.popup_touch.is_none() {
                self.popup_touch = Some((id, position));
            }
            return;
        }
        let Some((os_id, w, h)) = self
            .outputs
            .iter()
            .find(|o| o.layer.wl_surface() == &surface && o.configured)
            .map(|o| (o.id, o.width as f64, o.height as f64))
        else {
            return;
        };
        if self
            .shell
            .touch_down_on(os_id, w, h, id, position.0, position.1)
            || !self.shell.actions.is_empty()
        {
            self.sync_all();
        }
    }

    fn up(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        _: u32,
        id: i32,
    ) {
        if let Some((_, start)) = self.popup_touch.take().filter(|(pid, _)| *pid == id) {
            let pop_changed = if let Some(pl) = self.popups.as_mut() {
                use crate::notify::{hit, Hit};
                let w = pl.width as f64;
                let n = &mut self.shell.notify;
                match hit(n, w, start) {
                    Some(Hit::Dismiss(nid)) => n.dismiss(nid),
                    Some(Hit::Action(nid, key)) => n.invoke(nid, &key),
                    Some(Hit::Open(nid)) => n.invoke(nid, "default"),
                    None => false,
                }
            } else {
                false
            };
            if pop_changed {
                self.sync_popups();
            }
            return;
        }
        if self.shell.touch_up(id) || !self.shell.actions.is_empty() {
            self.sync_all();
        }
    }

    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        id: i32,
        position: (f64, f64),
    ) {
        if let Some((pid, start)) = self.popup_touch {
            if pid == id {
                let dx = position.0 - start.0;
                let dy = position.1 - start.1;
                if dx * dx + dy * dy >= 64.0 {
                    self.popup_touch = None;
                }
                return;
            }
        }
        if self.shell.touch_motion(id, position.0, position.1) {
            self.sync_all();
        }
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

    fn cancel(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch) {
        self.popup_touch = None;
        if self.shell.touch_cancel() {
            self.sync_all();
        }
    }
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
delegate_noop!(App: ignore WpViewporter);
delegate_noop!(App: ignore WpViewport);
delegate_noop!(App: ignore WpFractionalScaleManagerV1);
delegate_noop!(App: ignore WpCursorShapeManagerV1);
delegate_noop!(App: ignore WpCursorShapeDeviceV1);

impl Dispatch<WpFractionalScaleV1, wl_surface::WlSurface> for App {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        surface: &wl_surface::WlSurface,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = event else {
            return;
        };
        if scale == 0 {
            return;
        }
        let mut need_sync = false;
        if let Some(os) = state
            .outputs
            .iter_mut()
            .find(|o| o.layer.wl_surface() == surface)
        {
            if os.fractional_scale_120 != Some(scale) {
                os.fractional_scale_120 = Some(scale);
                os.mapped_closed = false;
                if state.shell.visible_on(os.id) {
                    os.dirty = true;
                    need_sync = true;
                } else {
                    os.apply_mode(qh, &state.compositor, &state.shell);
                }
            }
        }
        if let Some(pl) = state
            .popups
            .as_mut()
            .filter(|p| p.layer.wl_surface() == surface)
        {
            if pl.fractional_scale_120 != Some(scale) {
                pl.fractional_scale_120 = Some(scale);
                pl.render(qh, &state.compositor, &state.shell);
            }
        }
        if need_sync {
            state.sync_all();
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for App {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            let id = state.next_toplevel_id;
            state.next_toplevel_id += 1;
            state.toplevels.push(Toplevel {
                id,
                handle: toplevel,
                title: String::new(),
                app_id: String::new(),
                focused: false,
                fullscreen: false,
                outputs: Vec::new(),
            });
        }
    }

    smithay_client_toolkit::reexports::client::event_created_child!(
        App,
        ZwlrForeignToplevelManagerV1,
        [zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ())]
    );
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for App {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::{Event as E, State as S};
        let t = state.toplevels.iter_mut().find(|t| &t.handle == handle);
        match event {
            E::Title { title } => {
                if let Some(t) = t {
                    t.title = title
                }
            }
            E::AppId { app_id } => {
                if let Some(t) = t {
                    t.app_id = app_id
                }
            }
            E::State { state: raw } => {
                let has = |s: S| {
                    raw.as_chunks::<4>()
                        .0
                        .iter()
                        .any(|&c| u32::from_ne_bytes(c) == s as u32)
                };
                let (active, fullscreen) = (has(S::Activated), has(S::Fullscreen));
                if let Some(t) = t {
                    t.focused = active;
                    if t.fullscreen != fullscreen {
                        t.fullscreen = fullscreen;
                        state.fullscreen_changed = true;
                    }
                }
            }
            E::OutputEnter { output } => {
                if let Some(t) = t {
                    if !t.outputs.contains(&output) {
                        t.outputs.push(output);
                        state.fullscreen_changed |= t.fullscreen;
                    }
                }
            }
            E::OutputLeave { output } => {
                if let Some(t) = t {
                    let before = t.outputs.len();
                    t.outputs.retain(|o| o != &output);
                    state.fullscreen_changed |= t.fullscreen && t.outputs.len() != before;
                }
            }
            E::Done => {
                if std::mem::take(&mut state.fullscreen_changed) {
                    state.apply_modes();
                }
                state.sync_windows();
            }
            E::Closed => {
                handle.destroy();
                let was_fullscreen = state
                    .toplevels
                    .iter()
                    .any(|t| &t.handle == handle && t.fullscreen);
                state.toplevels.retain(|t| &t.handle != handle);
                if was_fullscreen {
                    state.apply_modes();
                }
                state.sync_windows();
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
        unsafe {
            signal_hook_registry::register(sig, move || {
                let buf = [byte];
                // Ignore self-pipe write errors here: signal handlers cannot allocate or lock stderr.
                let _ = libc::write(wr, buf.as_ptr().cast(), 1);
            })
        }
        .map_err(|e| format!("signal {sig}: {e}"))?;
    }
    Ok(rd)
}

pub fn run(initial_panel: Option<&str>) -> Result<(), String> {
    if stop_running() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let pid_file = runtime_path(PID_FILE);
    write_runtime_file(&pid_file, &format!("{}\n", std::process::id()));
    let res = run_loop(initial_panel);
    if std::fs::read_to_string(&pid_file).is_ok_and(|t| t.trim() == std::process::id().to_string())
    {
        let _ = std::fs::remove_file(&pid_file);
    }
    res
}

fn init_app(
    conn: &Connection,
    shell: Shell,
    fetch_tx: Sender<Fetched>,
) -> Result<
    (
        App,
        smithay_client_toolkit::reexports::client::EventQueue<App>,
    ),
    String,
> {
    let (globals, event_queue) = registry_queue_init(conn).map_err(|e| e.to_string())?;
    let qh = event_queue.handle();
    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|e| format!("zwlr_layer_shell_v1: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
    let _toplevel_mgr: Option<ZwlrForeignToplevelManagerV1> = globals.bind(&qh, 1..=3, ()).ok();
    let viewporter: Option<WpViewporter> = globals.bind(&qh, 1..=1, ()).ok();
    let fractional_scale_mgr: Option<WpFractionalScaleManagerV1> = if viewporter.is_some() {
        globals.bind(&qh, 1..=1, ()).ok()
    } else {
        None
    };
    let cursor_shape_mgr: Option<WpCursorShapeManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    let app_dirs = shell.app_dirs.clone();
    let apps_mtime = crate::app::dir_mtimes(&app_dirs);
    let output_state = OutputState::new(&globals, &qh);
    let existing: Vec<_> = output_state.outputs().collect();
    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state,
        compositor,
        layer_shell,
        shm,
        viewporter,
        fractional_scale_mgr,
        cursor_shape_mgr,
        qh: qh.clone(),
        outputs: Vec::new(),
        next_output_id: 1,
        toplevels: Vec::new(),
        next_toplevel_id: 1,
        fullscreen_changed: false,
        keyboard: None,
        pointer: None,
        cursor_device: None,
        touch: None,
        keyboard_output: None,
        current_pointer_output: None,
        last_pointer_output: None,
        last_display_scale: 1.0,
        last_config_check: None,
        #[cfg(test)]
        clock: None,
        #[cfg(test)]
        config_path: None,
        #[cfg(test)]
        config_stat_calls: std::rc::Rc::new(std::cell::Cell::new(0)),
        modifiers: Modifiers::default(),
        shell,
        fetch_tx,
        fetch: FetchSchedule::default(),
        app_refresh: AppRefresh::new(app_dirs, apps_mtime),
        workers: HashMap::new(),
        queued: Default::default(),
        was_animating: false,
        popups: None,
        popup_touch: None,
        notify_conn: None,
        shell_sock: None,
        shell_buf: Vec::new(),
        exit: false,
    };
    for output in existing {
        app.create_output_surface(output);
    }
    Ok((app, event_queue))
}

fn run_loop(initial_panel: Option<&str>) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let app_dirs = crate::app::dirs();
    let apps_mtime = crate::app::dir_mtimes(&app_dirs);
    let mut shell = Shell::new(crate::config::Config::load(), crate::app::load(&app_dirs));
    shell.set_app_dirs_with_mtimes(app_dirs, apps_mtime);
    shell.live = true;
    let (fetch_tx, fetch_rx) = channel::channel::<Fetched>();
    let (icon_tx, icon_rx) = channel::channel::<()>();
    crate::icons::set_icon_wakeup(move || {
        let _ = icon_tx.send(());
    });
    let (notify_tx, notify_rx) = channel::channel::<crate::notify::Incoming>();
    let (mut app, event_queue) = init_app(&conn, shell, fetch_tx)?;
    // the session's notifications (NOTIFICATIONS "auto" or "edge"), unless another daemon has them
    if app.shell.cfg.serves_notifications() {
        match crate::notify_server::start(notify_tx) {
            Ok(conn) => {
                app.notify_conn = Some(conn);
                app.shell.notify.serving = true;
            }
            Err(e) => eprintln!("infynitae-edge-apps: notifications: not serving: {e}"),
        }
    }
    // catch any config change or creation between startup and the first event-loop wait
    app.shell.reload_if_changed();
    if let Some(name) = initial_panel {
        let target = app.key_target_output();
        app.shell.toggle_named_on(target, name);
    }
    app.sync_all();

    let mut event_loop: EventLoop<App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let handle = event_loop.handle();
    handle
        .insert_source(fetch_rx, |evt, _, app| {
            let (prov, key, data) = match evt {
                channel::Event::Msg(Fetched::Source(prov, key, data, src)) => {
                    app.fetch.mark_done((prov, key), Some(src), Instant::now());
                    (prov, key, data)
                }
                channel::Event::Msg(Fetched::Data(prov, key, data)) => (prov, key, data),
                channel::Event::Msg(Fetched::Apps(apps, mtimes)) => {
                    let deferred = app.shell.edit.active || app.shell.transient.is_some();
                    app.app_refresh.mark_done(mtimes, !deferred);
                    if !deferred && app.shell.apply_apps(apps) {
                        app.sync_all();
                    }
                    return;
                }
                _ => return,
            };
            let changed = app.shell.apply(prov, key, data.as_ref());
            let reloaded = app.reload_config();
            if (changed || reloaded) && app.shell.open.is_some() {
                for os in &mut app.outputs {
                    if app.shell.visible_on(os.id) {
                        os.dirty = true;
                        os.render_if_needed(&app.qh, &app.shell);
                        // a card that grew or shrank: the input region follows
                        os.set_input_region(&app.compositor, &app.shell);
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    handle
        .insert_source(icon_rx, |evt, _, app| {
            if let channel::Event::Msg(()) = evt {
                if app.shell.open.is_some() {
                    for os in &mut app.outputs {
                        if app.shell.visible_on(os.id) {
                            os.dirty = true;
                            os.render_if_needed(&app.qh, &app.shell);
                        }
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    handle
        .insert_source(notify_rx, |evt, _, app| {
            if let channel::Event::Msg(m) = evt {
                let n = &mut app.shell.notify;
                match m {
                    crate::notify::Incoming::Notify(note) => {
                        n.receive(note, Instant::now(), crate::notify::quiet_now());
                    }
                    crate::notify::Incoming::Close(id) => {
                        n.close(id);
                    }
                }
                app.sync_popups();
            }
        })
        .map_err(|e| e.to_string())?;
    let sig_rd = setup_signal_pipe()?;
    handle
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
        .insert(handle.clone())
        .map_err(|e| e.to_string())?;
    if let Ok(mut sock) =
        std::os::unix::net::UnixStream::connect(runtime_path("infynitae/shell.sock"))
    {
        use std::io::Write;
        if sock
            .write_all(b"{\"id\":1,\"cmd\":\"subscribe\"}\n")
            .is_ok()
            && sock.set_nonblocking(true).is_ok()
        {
            app.shell_sock = sock.try_clone().ok();
            let _ = handle.insert_source(
                Generic::new(sock, Interest::READ, Mode::Level),
                |_, fd, app| {
                    let mut buf = [0u8; 4096];
                    let gone = loop {
                        // SAFETY: reads into a live buffer of its own length from our socket
                        let n = unsafe {
                            libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len())
                        };
                        if n > 0 {
                            app.shell_buf.extend_from_slice(&buf[..n as usize]);
                            continue;
                        }
                        // 0 is the compositor gone (a level source would fire on it forever);
                        // an error other than "nothing more now" too
                        break n == 0
                            || std::io::Error::last_os_error().kind()
                                != std::io::ErrorKind::WouldBlock;
                    };
                    let mut changed = false;
                    // whole lines only: a read can end inside a line, or inside a character
                    while let Some(nl) = app.shell_buf.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = app.shell_buf.drain(..=nl).collect();
                        if let Some(wm) = std::str::from_utf8(&line)
                            .ok()
                            .and_then(crate::shell::parse_window_menu_event)
                        {
                            changed |= app.shell.open_window_menu(wm);
                        }
                    }
                    if changed {
                        app.sync_all();
                    }
                    if gone {
                        app.shell_sock = None;
                        app.shell_buf.clear();
                        return Ok(PostAction::Remove);
                    }
                    Ok(PostAction::Continue)
                },
            );
        }
    }

    // callers read this to know the running edge can ask (live/infynitae-format-drive)
    let features_file = runtime_path(crate::ask::FEATURES_FILE);
    write_runtime_file(&features_file, "ask\n");
    // a question sent before the edge started
    if app.shell.ask_load() {
        if app.shell.ask.ask.on && app.shell.visible_output().is_none() {
            app.shell.set_ask_output(app.key_target_output());
        }
        app.sync_all();
    }
    while !app.exit {
        if app.shell.hover_tick(Instant::now()) {
            app.sync_all();
        }
        if app.shell.open.is_some() {
            let prev = app.shell.now;
            let now = chrono::Local::now().naive_local();
            app.shell.now = now;
            if app.shell.wants_clock() && minute_key(prev) != minute_key(now) {
                for os in &mut app.outputs {
                    if app.shell.visible_on(os.id) {
                        os.dirty = true;
                        os.render_if_needed(&app.qh, &app.shell);
                    }
                }
            }
            app.fetch_due(false);
        }
        // while a panel is open, wake when its shortest periodic source interval is next due;
        // closed (or only once-per-open sources), nothing wakes the edge for data fetches
        let mut timeout = app.fetch_timeout(Instant::now());
        if app.shell.open.is_some() && app.shell.wants_clock() {
            let rem = next_minute_timeout(app.shell.now);
            timeout = Some(timeout.map_or(rem, |t| t.min(rem)));
        }
        if let Some(d) = app.shell.hover_timeout(Instant::now()) {
            let rem = d.max(Duration::from_millis(1));
            timeout = Some(timeout.map_or(rem, |t| t.min(rem)));
        }
        // an asker that went away (its pipe closed, or it exited): checked every 100 ms while it
        // asks. No watcher thread: a thread holding a copy of the pipe kept the asker from
        // seeing the answer's end (it reads to the end of the pipe).
        if app.shell.ask.ask.on {
            if crate::ask::check_asker_gone(&mut app.shell.ask) {
                app.sync_all();
            }
            timeout = Some(timeout.map_or(Duration::from_millis(100), |t| {
                t.min(Duration::from_millis(100))
            }));
        }
        // the pop-ups' time: the next one up wakes the loop
        if app.shell.notify.next_deadline().is_some() {
            let now = Instant::now();
            if app.shell.notify.expire(now) {
                app.sync_popups();
            }
            if let Some(d) = app.shell.notify.next_deadline() {
                let rem = d
                    .saturating_duration_since(now)
                    .max(Duration::from_millis(5));
                timeout = Some(timeout.map_or(rem, |t| t.min(rem)));
            }
        }
        event_loop
            .dispatch(timeout, &mut app)
            .map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&features_file);
    // whoever still waits hears "cancelled", not silence
    while app.shell.ask.ask.on {
        crate::ask::answer(&mut app.shell.ask, None);
    }
    Ok(())
}

fn keyboard_interactivity(shell: &Shell) -> KeyboardInteractivity {
    if !shell.active() {
        KeyboardInteractivity::None
    } else if shell.opened_by_hover
        && !shell.clicked_inside
        && !shell.ask.ask.on
        && !shell.editing()
    {
        KeyboardInteractivity::OnDemand
    } else {
        KeyboardInteractivity::Exclusive
    }
}

fn minute_key(now: chrono::NaiveDateTime) -> (chrono::NaiveDate, u32, u32) {
    use chrono::Timelike;
    (now.date(), now.hour(), now.minute())
}

/// Duration until just past the next minute boundary so `Local::now()` has ticked over.
fn next_minute_timeout(now: chrono::NaiveDateTime) -> Duration {
    use chrono::Timelike;
    let secs_left = 59u64.saturating_sub(u64::from(now.second()));
    let nanos_used = u64::from(now.nanosecond() % 1_000_000_000);
    let nanos_left = 1_000_000_000u64.saturating_sub(nanos_used);
    Duration::from_secs(secs_left) + Duration::from_nanos(nanos_left) + Duration::from_millis(5)
}

/// The notification pop-ups' surface (in place of mako): a strip down the right of the first
/// screen on the Top layer, taking the pointer only on the cards, and unmapped (no buffer) while
/// there is nothing to show, so it costs nothing and is never one more surface over a full-screen
/// window.
struct PopupLayer {
    output: wl_output::WlOutput,
    frac_scale: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    layer: LayerSurface,
    pool: SlotPool,
    buffer: Option<Buffer>,
    width: u32,
    height: u32,
    integer_scale: i32,
    fractional_scale_120: Option<u32>,
    last_buffer_scale: i32,
    last_viewport_dest: Option<(i32, i32)>,
    configured: bool,
    /// a buffer is attached (the surface shows)
    mapped: bool,
    frame_pending: bool,
    /// draw again on the next frame (a card still sliding in)
    again: bool,
    /// where the pointer is on it
    pointer: Option<(f64, f64)>,
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
const POPUP_STRIP_W: u32 = (crate::notify::POPUP_W + crate::notify::MARGIN + 16.0) as u32;

impl PopupLayer {
    fn new(app: &App, output: wl_output::WlOutput) -> Option<PopupLayer> {
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
        let layer = app.layer_shell.create_layer_surface(
            &app.qh,
            surface,
            Layer::Top,
            Some("infynitae-notifications"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM);
        layer.set_size(POPUP_STRIP_W, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(region) = Region::new(&app.compositor) {
            layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        layer.commit();
        Some(PopupLayer {
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

    fn target_buf_size(&self) -> (u32, u32) {
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

    fn apply_surface_scale(&mut self) {
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

    /// Shows the pop-ups as they are now (or unmaps the strip when there are none).
    fn render(&mut self, qh: &QueueHandle<App>, compositor: &CompositorState, shell: &Shell) {
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
        let now = Instant::now();
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
                crate::notify::draw(&cr, &shell.theme, n, lw, self.pointer, now);
            }
            surface.flush();
        }
        // only the cards take the pointer; the rest of the strip lets clicks through
        if let Ok(region) = Region::new(compositor) {
            for (_, r) in crate::notify::layout(n, lw) {
                region.add(r.0 as i32, r.1 as i32, r.2.ceil() as i32, r.3.ceil() as i32);
            }
            self.layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        self.apply_surface_scale();
        let wl = self.layer.wl_surface();
        wl.damage_buffer(0, 0, bw, bh);
        // a card still sliding in: the next frame too
        let arriving = n.popups.iter().any(|p| {
            now.saturating_duration_since(p.born).as_secs_f64() * 1000.0 < crate::notify::ARRIVE_MS
        });
        if arriving {
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
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct CountingSource {
        calls: Arc<AtomicUsize>,
        every: Option<Duration>,
    }

    impl Source for CountingSource {
        fn fetch(&mut self) -> Data {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Box::new(())
        }

        fn every(&self) -> Option<Duration> {
            self.every
        }
    }

    fn run_due(
        sched: &mut FetchSchedule,
        open: Option<usize>,
        specs: &[(SourceId, &Arc<AtomicUsize>, Option<Duration>)],
        force: bool,
        now: Instant,
    ) -> Vec<SourceId> {
        let ids: Vec<SourceId> = specs.iter().map(|(id, _, _)| *id).collect();
        let due = sched.take_due(open, &ids, force, now, |p, k| {
            let (_, calls, every) = specs.iter().find(|(id, _, _)| *id == (p, k))?;
            Some(Box::new(CountingSource {
                calls: Arc::clone(calls),
                every: *every,
            }))
        });
        let mut fetched = Vec::with_capacity(due.len());
        for (id, mut src) in due {
            let _ = src.fetch();
            fetched.push(id);
        }
        fetched
    }

    #[test]
    fn write_runtime_file_writes_and_reports_missing_parent() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-edge-apps-runtime-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let ok_path = dir.join("pid");
        assert!(write_runtime_file(&ok_path, "42\n"));
        assert_eq!(std::fs::read_to_string(&ok_path).unwrap(), "42\n");

        let bad_path = dir.join("missing").join("pid");
        assert!(!write_runtime_file(&bad_path, "42\n"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn none_interval_fetches_once_until_reopen_or_switch() {
        let mut sched = FetchSchedule::default();
        let once_calls = Arc::new(AtomicUsize::new(0));
        let specs = [(("about", "info"), &once_calls, None)];
        let ids = [("about", "info")];
        let t0 = Instant::now();

        assert_eq!(
            run_due(&mut sched, Some(0), &specs, false, t0),
            vec![("about", "info")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);
        sched.mark_done(("about", "info"), None, t0 + Duration::from_millis(10));

        // While the same panel stays open, it never fetches again and needs no wakeup timeout.
        assert_eq!(sched.next_timeout(&ids, t0 + Duration::from_secs(1)), None);
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            false,
            t0 + Duration::from_secs(60)
        )
        .is_empty());
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);

        // Closing and reopening the same panel fetches once again.
        assert!(run_due(
            &mut sched,
            None,
            &specs,
            false,
            t0 + Duration::from_secs(61)
        )
        .is_empty());
        assert_eq!(sched.next_timeout(&ids, t0 + Duration::from_secs(61)), None);
        assert_eq!(
            run_due(
                &mut sched,
                Some(0),
                &specs,
                false,
                t0 + Duration::from_secs(62)
            ),
            vec![("about", "info")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 2);
        sched.mark_done(("about", "info"), None, t0 + Duration::from_secs(62));

        // Switching directly to another panel also treats the new opening as fresh.
        assert_eq!(
            run_due(
                &mut sched,
                Some(1),
                &specs,
                false,
                t0 + Duration::from_secs(63)
            ),
            vec![("about", "info")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn honours_each_periodic_source_interval_and_next_timeout() {
        let mut sched = FetchSchedule::default();
        let once_calls = Arc::new(AtomicUsize::new(0));
        let fast_calls = Arc::new(AtomicUsize::new(0));
        let slow_calls = Arc::new(AtomicUsize::new(0));
        let specs = [
            (("about", "info"), &once_calls, None),
            (("audio", "sink"), &fast_calls, Some(Duration::from_secs(2))),
            (
                ("weather", "now"),
                &slow_calls,
                Some(Duration::from_secs(10)),
            ),
        ];
        let ids: Vec<SourceId> = specs.iter().map(|(id, _, _)| *id).collect();
        let t0 = Instant::now();

        // Opening fetches all three once; while all three are in flight no timer wakeup is needed.
        let initial = run_due(&mut sched, Some(0), &specs, false, t0);
        assert_eq!(initial, ids);
        assert_eq!(sched.next_timeout(&ids, t0), None);

        // Complete at distinct instants; next_timeout tracks the earliest remaining deadline.
        let t_done_fast = t0 + Duration::from_millis(50);
        sched.mark_done(("about", "info"), None, t0 + Duration::from_millis(20));
        sched.mark_done(("audio", "sink"), None, t_done_fast);
        assert_eq!(
            sched.next_timeout(&ids, t_done_fast),
            Some(Duration::from_secs(2))
        );

        let t_done_slow = t0 + Duration::from_millis(300);
        sched.mark_done(("weather", "now"), None, t_done_slow);
        assert_eq!(
            sched.next_timeout(&ids, t_done_slow),
            Some(Duration::from_millis(1750))
        );

        // Before 2 s elapses from fast's completion, nothing is due.
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            false,
            t_done_fast + Duration::from_millis(1999),
        )
        .is_empty());

        // At 2 s from fast's completion, only fast runs; slow and None stay idle.
        let t_tick1 = t_done_fast + Duration::from_secs(2);
        assert_eq!(
            run_due(&mut sched, Some(0), &specs, false, t_tick1),
            vec![("audio", "sink")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);
        assert_eq!(fast_calls.load(Ordering::Relaxed), 2);
        assert_eq!(slow_calls.load(Ordering::Relaxed), 1);

        // While fast is in flight again, next_timeout looks at slow's deadline.
        assert_eq!(
            sched.next_timeout(&ids, t_tick1),
            Some(Duration::from_secs(10) - (t_tick1 - t_done_slow))
        );
        sched.mark_done(("audio", "sink"), None, t_tick1 + Duration::from_millis(20));

        // After 10 s from slow's completion, slow becomes due alongside any due fast interval.
        let t_tick_slow = t_done_slow + Duration::from_secs(10);
        assert_eq!(
            run_due(&mut sched, Some(0), &specs, false, t_tick_slow),
            vec![("audio", "sink"), ("weather", "now")]
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);
        assert_eq!(fast_calls.load(Ordering::Relaxed), 3);
        assert_eq!(slow_calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn skips_in_flight_reopen_duplicates_and_honours_force() {
        let mut sched = FetchSchedule::default();
        let once_calls = Arc::new(AtomicUsize::new(0));
        let fast_calls = Arc::new(AtomicUsize::new(0));
        let specs = [
            (("about", "info"), &once_calls, None),
            (("audio", "sink"), &fast_calls, Some(Duration::from_secs(2))),
        ];
        let ids: Vec<SourceId> = specs.iter().map(|(id, _, _)| *id).collect();
        let t0 = Instant::now();

        assert_eq!(run_due(&mut sched, Some(0), &specs, false, t0), ids);

        // In-flight sources are never overlapped, even on force.
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            true,
            t0 + Duration::from_millis(10)
        )
        .is_empty());

        // Close and reopen while `about/info` (None) is still in flight, then complete it.
        assert!(run_due(
            &mut sched,
            None,
            &specs,
            false,
            t0 + Duration::from_millis(20)
        )
        .is_empty());
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            false,
            t0 + Duration::from_millis(30)
        )
        .is_empty());
        sched.mark_done(("about", "info"), None, t0 + Duration::from_millis(40));
        sched.mark_done(("audio", "sink"), None, t0 + Duration::from_millis(40));

        // On later ticks in this reopened panel, `about/info` stays single-shot.
        assert!(run_due(
            &mut sched,
            Some(0),
            &specs,
            false,
            t0 + Duration::from_millis(500)
        )
        .is_empty());
        assert_eq!(once_calls.load(Ordering::Relaxed), 1);

        // Forced fetch runs both `None` and not-yet-due periodic sources immediately.
        assert_eq!(
            run_due(
                &mut sched,
                Some(0),
                &specs,
                true,
                t0 + Duration::from_millis(600)
            ),
            ids
        );
        assert_eq!(once_calls.load(Ordering::Relaxed), 2);
        assert_eq!(fast_calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn removed_source_fetches_again_if_re_added() {
        let mut sched = FetchSchedule::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let full = [(("about", "info"), &calls, None)];
        let empty: [(SourceId, &Arc<AtomicUsize>, Option<Duration>); 0] = [];
        let t0 = Instant::now();

        assert_eq!(
            run_due(&mut sched, Some(0), &full, false, t0),
            vec![("about", "info")]
        );
        sched.mark_done(("about", "info"), None, t0 + Duration::from_millis(10));

        // Card removed from the open panel drops its record; re-adding fetches once again.
        assert!(run_due(
            &mut sched,
            Some(0),
            &empty,
            false,
            t0 + Duration::from_secs(1)
        )
        .is_empty());
        assert_eq!(
            run_due(
                &mut sched,
                Some(0),
                &full,
                false,
                t0 + Duration::from_secs(2)
            ),
            vec![("about", "info")]
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    struct InstanceSource {
        calls: usize,
    }

    impl Source for InstanceSource {
        fn fetch(&mut self) -> Data {
            self.calls += 1;
            Box::new((std::thread::current().id(), self.calls))
        }
    }

    #[test]
    fn keeps_source_instance_across_fetches_in_one_opening() {
        let mut sched = FetchSchedule::default();
        let ids = [("monitor", "cpu")];
        let mut make_calls = 0usize;
        let t0 = Instant::now();

        let mut due1 = sched.take_due(Some(0), &ids, false, t0, |_, _| {
            make_calls += 1;
            Some(Box::new(InstanceSource { calls: 0 }))
        });
        assert_eq!(due1.len(), 1);
        let (id1, mut src1) = due1.pop().unwrap();
        let (_, n1) = *src1
            .fetch()
            .downcast::<(std::thread::ThreadId, usize)>()
            .unwrap();
        assert_eq!(n1, 1);
        assert_eq!(make_calls, 1);
        let t_done1 = t0 + Duration::from_millis(10);
        sched.mark_done(id1, Some(src1), t_done1);

        let t1 = t_done1 + Duration::from_secs(2);
        let mut due2 = sched.take_due(Some(0), &ids, false, t1, |_, _| {
            make_calls += 1;
            Some(Box::new(InstanceSource { calls: 0 }))
        });
        assert_eq!(due2.len(), 1);
        let (id2, mut src2) = due2.pop().unwrap();
        let (_, n2) = *src2
            .fetch()
            .downcast::<(std::thread::ThreadId, usize)>()
            .unwrap();
        assert_eq!(n2, 2, "second fetch must reuse the same Source instance");
        assert_eq!(
            make_calls, 1,
            "source must not be rebuilt while panel stays open"
        );
        sched.mark_done(id2, Some(src2), t1 + Duration::from_millis(10));

        // Closing and reopening builds a fresh Source.
        assert!(sched
            .take_due(None, &[], false, t1 + Duration::from_secs(1), |_, _| None)
            .is_empty());
        let mut due3 = sched.take_due(Some(0), &ids, false, t1 + Duration::from_secs(2), |_, _| {
            make_calls += 1;
            Some(Box::new(InstanceSource { calls: 0 }))
        });
        let (_, mut src3) = due3.pop().unwrap();
        let (_, n3) = *src3
            .fetch()
            .downcast::<(std::thread::ThreadId, usize)>()
            .unwrap();
        assert_eq!(n3, 1);
        assert_eq!(make_calls, 2);
    }

    #[test]
    fn worker_reuses_one_thread_while_open_and_stops_on_close() {
        type WorkerMsg = (std::thread::ThreadId, usize, Box<dyn Source>);
        let (fetch_tx, fetch_rx) = channel::channel::<Fetched>();
        let mut event_loop: EventLoop<Option<WorkerMsg>> = EventLoop::try_new().unwrap();
        event_loop
            .handle()
            .insert_source(fetch_rx, |evt, _, out| {
                if let channel::Event::Msg(Fetched::Source(_, _, data, src)) = evt {
                    let (tid, n) = *data.downcast::<(std::thread::ThreadId, usize)>().unwrap();
                    *out = Some((tid, n, src));
                }
            })
            .unwrap();

        let mut worker = spawn_worker(("monitor", "cpu"), fetch_tx);
        let src: Box<dyn Source> = Box::new(InstanceSource { calls: 0 });
        worker.tx.send(src).unwrap();
        let mut res = None;
        while res.is_none() {
            event_loop
                .dispatch(Some(Duration::from_millis(100)), &mut res)
                .unwrap();
        }
        let (tid1, n1, src) = res.take().unwrap();
        assert_eq!(n1, 1);

        worker.tx.send(src).unwrap();
        while res.is_none() {
            event_loop
                .dispatch(Some(Duration::from_millis(100)), &mut res)
                .unwrap();
        }
        let (tid2, n2, _src) = res.take().unwrap();
        assert_eq!(n2, 2);
        assert_eq!(tid1, tid2, "same worker thread serves both fetches");

        let handle = worker.handle.take().unwrap();
        drop(worker);
        handle.join().unwrap();
    }

    #[test]
    fn cpu_source_reports_percent_on_second_fetch_of_one_opening() {
        let dir =
            std::env::temp_dir().join(format!("infynitae-edge-apps-f1-cpu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stat"), "cpu  100 0 50 850 0 0 0 0\n").unwrap();
        std::env::set_var("INFYNITAE_PROC_DIR", &dir);

        let mut sched = FetchSchedule::default();
        let ids = [("monitor", "cpu")];
        let t0 = Instant::now();
        let make = |p, k| crate::provider::by_id(p).and_then(|prov| prov.source(k));

        let mut due1 = sched.take_due(Some(0), &ids, false, t0, make);
        assert_eq!(due1.len(), 1);
        let (id1, mut src1) = due1.pop().unwrap();
        let d1 = src1
            .fetch()
            .downcast::<crate::cards::monitor::CpuData>()
            .unwrap();
        assert_eq!(d1.busy_pct, None);
        let t_done1 = t0 + Duration::from_millis(10);
        sched.mark_done(id1, Some(src1), t_done1);

        std::fs::write(dir.join("stat"), "cpu  180 0 70 1150 0 0 0 0\n").unwrap();
        let t1 = t_done1 + Duration::from_secs(2);
        let mut due2 = sched.take_due(Some(0), &ids, false, t1, make);
        assert_eq!(due2.len(), 1);
        let (id2, mut src2) = due2.pop().unwrap();
        let d2 = src2
            .fetch()
            .downcast::<crate::cards::monitor::CpuData>()
            .unwrap();
        assert_eq!(d2.busy_pct, Some(25));
        sched.mark_done(id2, Some(src2), t1 + Duration::from_millis(10));

        std::env::remove_var("INFYNITAE_PROC_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_config_source_fetches_once_per_open_with_no_timer() {
        let mut sched = FetchSchedule::default();
        let ids = [("settings", "config")];
        let t0 = Instant::now();
        let make = |p, k| crate::provider::by_id(p).and_then(|prov| prov.source(k));

        let mut due1 = sched.take_due(Some(0), &ids, false, t0, make);
        assert_eq!(due1.len(), 1);
        let (id, mut src) = due1.pop().unwrap();
        assert_eq!(src.every(), None);
        let _ = src.fetch();
        sched.mark_done(id, Some(src), t0 + Duration::from_millis(10));

        assert_eq!(sched.next_timeout(&ids, t0 + Duration::from_secs(1)), None);
        assert!(sched
            .take_due(Some(0), &ids, false, t0 + Duration::from_secs(60), make)
            .is_empty());
    }

    #[test]
    fn clock_timeout_hits_next_full_minute_and_shell_reports_wants_clock() {
        use chrono::{NaiveDate, Timelike};
        let d = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();

        let t0 = d.and_hms_milli_opt(12, 34, 0, 0).unwrap();
        let d0 = next_minute_timeout(t0);
        assert_eq!(d0, Duration::from_millis(60_005));
        assert_eq!((t0 + chrono::Duration::from_std(d0).unwrap()).minute(), 35);

        let t1 = d.and_hms_milli_opt(12, 34, 30, 500).unwrap();
        let d1 = next_minute_timeout(t1);
        assert_eq!(d1, Duration::from_millis(29_505));
        assert_eq!((t1 + chrono::Duration::from_std(d1).unwrap()).minute(), 35);

        let t2 = d.and_hms_milli_opt(12, 34, 59, 900).unwrap();
        let d2 = next_minute_timeout(t2);
        assert_eq!(d2, Duration::from_millis(105));
        assert_eq!((t2 + chrono::Duration::from_std(d2).unwrap()).minute(), 35);

        // Shell reports wants_clock only when an open panel holds a clock or month card.
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 calendar/clock\"\nPANEL_OTHER=\"SUPER+K top right 380 settings/power\"\n",
            ),
            Vec::new(),
        );
        assert!(!sh.wants_clock(), "closed panel needs no clock wake");
        let ctrl = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let other = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "other")
            .unwrap();
        assert!(sh.open_panel(ctrl, false, Instant::now()));
        assert!(sh.wants_clock());
        assert!(sh.open_panel(other, false, Instant::now()));
        assert!(!sh.wants_clock());
    }

    #[test]
    fn hover_panel_uses_ondemand_keyboard_until_clicked_inside() {
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 hover settings/session\"\n",
            ),
            Vec::new(),
        );
        assert_eq!(keyboard_interactivity(&sh), KeyboardInteractivity::None);

        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let t0 = Instant::now();
        assert!(sh.open_panel(idx, true, t0));
        sh.open_ms = f64::INFINITY;
        assert_eq!(
            keyboard_interactivity(&sh),
            KeyboardInteractivity::OnDemand,
            "hover-opened panel must not steal exclusive keyboard focus"
        );

        let ((ox, oy, _, _), _) = sh.layout(idx, 1280.0, 800.0);
        assert!(sh.click(1280.0, 800.0, ox + 10.0, oy + 10.0, Button::Left));
        assert!(sh.clicked_inside);
        assert_eq!(
            keyboard_interactivity(&sh),
            KeyboardInteractivity::Exclusive,
            "clicking inside a hover panel promotes keyboard interactivity to Exclusive"
        );

        assert!(sh.close_at(t0));
        assert_eq!(keyboard_interactivity(&sh), KeyboardInteractivity::None);

        assert!(sh.open_panel(idx, false, t0));
        assert_eq!(
            keyboard_interactivity(&sh),
            KeyboardInteractivity::Exclusive,
            "key-opened panel takes Exclusive keyboard interactivity at once"
        );
    }

    #[derive(Clone, Debug)]
    struct MockOutputSpec {
        width: i32,
        height: i32,
        scale: i32,
        fractional_120: u32,
    }

    #[derive(Clone, Debug)]
    struct CommittedSurfaceState {
        buffer_size: Option<(i32, i32)>,
        buffer_scale: i32,
        viewport_dest: Option<(i32, i32)>,
        input_rects: Vec<(i32, i32, i32, i32)>,
        keyboard_interactivity: u32,
    }

    #[derive(Default)]
    struct MockServerState {
        surfaces: HashMap<u32, usize>,
        surface_commits: HashMap<u32, Vec<CommittedSurfaceState>>,
        surface_scale: HashMap<u32, i32>,
        surface_buf: HashMap<u32, Option<u32>>,
        surface_input: HashMap<u32, Vec<(i32, i32, i32, i32)>>,
        surface_viewport_dest: HashMap<u32, Option<(i32, i32)>>,
        surface_kb: HashMap<u32, u32>,
        layers_by_surface: HashMap<u32, u32>,
        layer_to_surface: HashMap<u32, u32>,
        layer_to_output_global: HashMap<u32, u32>,
        buffers: HashMap<u32, (i32, i32)>,
        regions: HashMap<u32, Vec<(i32, i32, i32, i32)>>,
        viewports: HashMap<u32, u32>,
        frac_scales: HashMap<u32, u32>,
        output_globals: HashMap<u32, u32>,
        output_obj_to_global: HashMap<u32, u32>,
        pointer_id: Option<u32>,
        cursor_shapes: Vec<(u32, u32)>,
        touch_id: Option<u32>,
        toplevel_mgr_id: Option<u32>,
        next_serial: u32,
        next_server_id: u32,
    }

    struct MockWayland {
        server_sock: std::os::unix::net::UnixStream,
        state: Arc<std::sync::Mutex<MockServerState>>,
        join: Option<std::thread::JoinHandle<()>>,
    }

    impl MockWayland {
        fn start(specs: Vec<MockOutputSpec>, offer_fractional: bool) -> (Self, Connection) {
            Self::start_with(specs, offer_fractional, true)
        }

        fn start_with(
            specs: Vec<MockOutputSpec>,
            offer_fractional: bool,
            offer_cursor_shape: bool,
        ) -> (Self, Connection) {
            use std::os::unix::net::UnixStream;
            let (client_sock, server_sock) = UnixStream::pair().unwrap();
            let write_sock = server_sock.try_clone().unwrap();
            let conn = Connection::from_socket(client_sock).unwrap();
            let state = Arc::new(std::sync::Mutex::new(MockServerState {
                next_serial: 10,
                next_server_id: 0xff00_0000,
                ..Default::default()
            }));
            let st_thread = Arc::clone(&state);

            let join = std::thread::spawn(move || {
                Self::serve(
                    server_sock,
                    specs,
                    offer_fractional,
                    offer_cursor_shape,
                    st_thread,
                );
            });

            (
                Self {
                    server_sock: write_sock,
                    state,
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

        fn write_all(sock: &std::os::unix::net::UnixStream, data: &[u8]) {
            use std::io::Write;
            let mut s = sock;
            let _ = s.write_all(data);
        }

        fn recv_chunk(sock: &std::os::unix::net::UnixStream, out: &mut Vec<u8>) -> bool {
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
                            libc::close(fd);
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

        fn serve(
            sock: std::os::unix::net::UnixStream,
            specs: Vec<MockOutputSpec>,
            offer_fractional: bool,
            offer_cursor_shape: bool,
            state: Arc<std::sync::Mutex<MockServerState>>,
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
                Region,
                Seat,
                Pointer,
                Keyboard,
                Touch,
                Output,
                Viewporter,
                Viewport,
                FracMgr,
                FracScale,
                CursorShapeMgr,
                CursorShapeDevice,
                ToplevelMgr,
            }
            let mut objs: HashMap<u32, Obj> = HashMap::new();
            objs.insert(1, Obj::Display);

            let mut globals: Vec<(u32, &'static str, u32)> = vec![
                (1, "wl_compositor", 6),
                (2, "wl_shm", 1),
                (3, "wl_seat", 8),
                (4, "zwlr_layer_shell_v1", 4),
                (5, "zwlr_foreign_toplevel_manager_v1", 3),
            ];
            if offer_fractional {
                globals.push((6, "wp_viewporter", 1));
                globals.push((7, "wp_fractional_scale_manager_v1", 1));
            }
            if offer_cursor_shape {
                globals.push((8, "wp_cursor_shape_manager_v1", 1));
            }
            let out_base = 10u32;
            for idx in 0..specs.len() {
                globals.push((out_base + idx as u32, "wl_output", 4));
            }

            let mut pending = Vec::<u8>::new();
            loop {
                if !Self::recv_chunk(&sock, &mut pending) {
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
                                    Self::push_u32(&mut b, 7); // pointer | keyboard | touch
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
                                "wp_fractional_scale_manager_v1" => {
                                    objs.insert(new_id, Obj::FracMgr);
                                }
                                "wp_cursor_shape_manager_v1" => {
                                    objs.insert(new_id, Obj::CursorShapeMgr);
                                }
                                "zwlr_foreign_toplevel_manager_v1" => {
                                    objs.insert(new_id, Obj::ToplevelMgr);
                                    state.lock().unwrap().toplevel_mgr_id = Some(new_id);
                                }
                                "wl_output" => {
                                    let out_idx = (name - out_base) as usize;
                                    objs.insert(new_id, Obj::Output);
                                    let mut st = state.lock().unwrap();
                                    st.output_globals.insert(out_idx as u32, new_id);
                                    st.output_obj_to_global.insert(new_id, out_idx as u32);
                                    let spec = &specs[out_idx];
                                    let mut gb = Vec::new();
                                    Self::push_i32(&mut gb, 0);
                                    Self::push_i32(&mut gb, 0);
                                    Self::push_i32(&mut gb, spec.width);
                                    Self::push_i32(&mut gb, spec.height);
                                    Self::push_i32(&mut gb, 0);
                                    Self::push_str(&mut gb, "Mock");
                                    Self::push_str(&mut gb, "Out");
                                    Self::push_i32(&mut gb, 0);
                                    out_bytes.extend(Self::msg(new_id, 0, &gb));
                                    let mut mb = Vec::new();
                                    Self::push_u32(&mut mb, 3); // current | preferred
                                    Self::push_i32(&mut mb, spec.width * spec.scale);
                                    Self::push_i32(&mut mb, spec.height * spec.scale);
                                    Self::push_i32(&mut mb, 60_000);
                                    out_bytes.extend(Self::msg(new_id, 1, &mb));
                                    let mut sb = Vec::new();
                                    Self::push_i32(&mut sb, spec.scale);
                                    out_bytes.extend(Self::msg(new_id, 3, &sb));
                                    out_bytes.extend(Self::msg(new_id, 2, &[]));
                                }
                                _ => {}
                            }
                        }
                        Some(Obj::Compositor) => match opcode {
                            0 => {
                                let sid = Self::read_u32(body, &mut off);
                                objs.insert(sid, Obj::Surface);
                                state.lock().unwrap().surface_scale.insert(sid, 1);
                            }
                            1 => {
                                let rid = Self::read_u32(body, &mut off);
                                objs.insert(rid, Obj::Region);
                                state.lock().unwrap().regions.insert(rid, Vec::new());
                            }
                            _ => {}
                        },
                        Some(Obj::Region) => match opcode {
                            0 => {
                                state.lock().unwrap().regions.remove(&obj_id);
                            }
                            1 => {
                                let x = Self::read_i32(body, &mut off);
                                let y = Self::read_i32(body, &mut off);
                                let w = Self::read_i32(body, &mut off);
                                let h = Self::read_i32(body, &mut off);
                                if let Some(list) = state.lock().unwrap().regions.get_mut(&obj_id) {
                                    list.push((x, y, w, h));
                                }
                            }
                            _ => {}
                        },
                        Some(Obj::Shm) if opcode == 0 => {
                            let pid = Self::read_u32(body, &mut off);
                            objs.insert(pid, Obj::ShmPool);
                        }
                        Some(Obj::ShmPool) if opcode == 0 => {
                            let bid = Self::read_u32(body, &mut off);
                            let _offset = Self::read_i32(body, &mut off);
                            let w = Self::read_i32(body, &mut off);
                            let h = Self::read_i32(body, &mut off);
                            objs.insert(bid, Obj::Buffer);
                            state.lock().unwrap().buffers.insert(bid, (w, h));
                        }
                        Some(Obj::LayerShell) if opcode == 0 => {
                            let lid = Self::read_u32(body, &mut off);
                            let sid = Self::read_u32(body, &mut off);
                            let out_obj = Self::read_u32(body, &mut off);
                            objs.insert(lid, Obj::LayerSurface);
                            let mut st = state.lock().unwrap();
                            st.layers_by_surface.insert(sid, lid);
                            st.layer_to_surface.insert(lid, sid);
                            if let Some(&gidx) = st.output_obj_to_global.get(&out_obj) {
                                st.layer_to_output_global.insert(lid, gidx);
                                st.surfaces.insert(sid, gidx as usize);
                                if let Some(fsid) = st
                                    .frac_scales
                                    .iter()
                                    .find(|(_, &s)| s == sid)
                                    .map(|(&f, _)| f)
                                {
                                    let s120 = specs[gidx as usize].fractional_120;
                                    let mut b = Vec::new();
                                    Self::push_u32(&mut b, s120);
                                    out_bytes.extend(Self::msg(fsid, 0, &b));
                                }
                            }
                        }
                        Some(Obj::LayerSurface) if opcode == 4 => {
                            let kb = Self::read_u32(body, &mut off);
                            let mut st = state.lock().unwrap();
                            if let Some(&sid) = st.layer_to_surface.get(&obj_id) {
                                st.surface_kb.insert(sid, kb);
                            }
                        }
                        Some(Obj::Viewporter) if opcode == 1 => {
                            let vid = Self::read_u32(body, &mut off);
                            let sid = Self::read_u32(body, &mut off);
                            objs.insert(vid, Obj::Viewport);
                            state.lock().unwrap().viewports.insert(vid, sid);
                        }
                        Some(Obj::Viewport) if opcode == 2 => {
                            let w = Self::read_i32(body, &mut off);
                            let h = Self::read_i32(body, &mut off);
                            let mut st = state.lock().unwrap();
                            if let Some(&sid) = st.viewports.get(&obj_id) {
                                st.surface_viewport_dest.insert(sid, Some((w, h)));
                            }
                        }
                        Some(Obj::FracMgr) if opcode == 1 => {
                            let fsid = Self::read_u32(body, &mut off);
                            let sid = Self::read_u32(body, &mut off);
                            objs.insert(fsid, Obj::FracScale);
                            let mut st = state.lock().unwrap();
                            st.frac_scales.insert(fsid, sid);
                            if let Some(&out_idx) = st.surfaces.get(&sid) {
                                let s120 = specs[out_idx].fractional_120;
                                let mut b = Vec::new();
                                Self::push_u32(&mut b, s120);
                                out_bytes.extend(Self::msg(fsid, 0, &b));
                            }
                        }
                        Some(Obj::CursorShapeMgr) if opcode == 1 => {
                            let dev_id = Self::read_u32(body, &mut off);
                            let _ptr_id = Self::read_u32(body, &mut off);
                            objs.insert(dev_id, Obj::CursorShapeDevice);
                        }
                        Some(Obj::CursorShapeDevice) if opcode == 1 => {
                            let serial = Self::read_u32(body, &mut off);
                            let shape = Self::read_u32(body, &mut off);
                            state.lock().unwrap().cursor_shapes.push((serial, shape));
                        }
                        Some(Obj::Seat) => match opcode {
                            0 => {
                                let pid = Self::read_u32(body, &mut off);
                                objs.insert(pid, Obj::Pointer);
                                state.lock().unwrap().pointer_id = Some(pid);
                            }
                            1 => {
                                let kid = Self::read_u32(body, &mut off);
                                objs.insert(kid, Obj::Keyboard);
                            }
                            2 => {
                                let tid = Self::read_u32(body, &mut off);
                                objs.insert(tid, Obj::Touch);
                                state.lock().unwrap().touch_id = Some(tid);
                            }
                            _ => {}
                        },
                        Some(Obj::Surface) => match opcode {
                            1 => {
                                let bid = Self::read_u32(body, &mut off);
                                state
                                    .lock()
                                    .unwrap()
                                    .surface_buf
                                    .insert(obj_id, (bid != 0).then_some(bid));
                            }
                            3 => {
                                let cb = Self::read_u32(body, &mut off);
                                let mut b = Vec::new();
                                Self::push_u32(&mut b, 0);
                                out_bytes.extend(Self::msg(cb, 0, &b));
                                let mut d = Vec::new();
                                Self::push_u32(&mut d, cb);
                                out_bytes.extend(Self::msg(1, 1, &d));
                            }
                            5 => {
                                let rid = Self::read_u32(body, &mut off);
                                let mut st = state.lock().unwrap();
                                let rects = st.regions.get(&rid).cloned().unwrap_or_default();
                                st.surface_input.insert(obj_id, rects);
                            }
                            6 => {
                                let mut st = state.lock().unwrap();
                                let lid = st.layers_by_surface.get(&obj_id).copied();
                                let prev_had_commit = st
                                    .surface_commits
                                    .get(&obj_id)
                                    .is_some_and(|v| !v.is_empty());
                                if let (Some(lid), false) = (lid, prev_had_commit) {
                                    if let Some(&gidx) = st.layer_to_output_global.get(&lid) {
                                        st.next_serial += 1;
                                        let serial = st.next_serial;
                                        let spec = &specs[gidx as usize];
                                        let mut cb = Vec::new();
                                        Self::push_u32(&mut cb, serial);
                                        Self::push_u32(&mut cb, spec.width as u32);
                                        Self::push_u32(&mut cb, spec.height as u32);
                                        out_bytes.extend(Self::msg(lid, 0, &cb));
                                    }
                                }
                                let buf_id = st.surface_buf.get(&obj_id).copied().flatten();
                                let buf_sz = buf_id.and_then(|b| st.buffers.get(&b).copied());
                                if let Some(bid) = buf_id {
                                    out_bytes.extend(Self::msg(bid, 0, &[]));
                                }
                                let snap = CommittedSurfaceState {
                                    buffer_size: buf_sz,
                                    buffer_scale: *st.surface_scale.get(&obj_id).unwrap_or(&1),
                                    viewport_dest: st
                                        .surface_viewport_dest
                                        .get(&obj_id)
                                        .copied()
                                        .flatten(),
                                    input_rects: st
                                        .surface_input
                                        .get(&obj_id)
                                        .cloned()
                                        .unwrap_or_default(),
                                    keyboard_interactivity: *st
                                        .surface_kb
                                        .get(&obj_id)
                                        .unwrap_or(&0),
                                };
                                st.surface_commits.entry(obj_id).or_default().push(snap);
                            }
                            8 => {
                                let scale = Self::read_i32(body, &mut off);
                                state.lock().unwrap().surface_scale.insert(obj_id, scale);
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

        fn send_raw(&self, bytes: Vec<u8>) {
            Self::write_all(&self.server_sock, &bytes);
        }

        fn send_output_scale(&self, output_idx: u32, scale: i32) {
            let out_id = *self
                .state
                .lock()
                .unwrap()
                .output_globals
                .get(&output_idx)
                .unwrap();
            let mut sb = Vec::new();
            Self::push_i32(&mut sb, scale);
            let mut raw = Self::msg(out_id, 3, &sb);
            raw.extend(Self::msg(out_id, 2, &[]));
            self.send_raw(raw);
        }

        fn send_surface_preferred_buffer_scale(&self, surface_id: u32, scale: i32) {
            let mut sb = Vec::new();
            Self::push_i32(&mut sb, scale);
            self.send_raw(Self::msg(surface_id, 2, &sb));
        }

        fn send_layer_configure(&self, surface_id: u32, width: u32, height: u32) {
            let mut st = self.state.lock().unwrap();
            let lid = *st.layers_by_surface.get(&surface_id).unwrap();
            st.next_serial += 1;
            let serial = st.next_serial;
            drop(st);
            let mut cb = Vec::new();
            Self::push_u32(&mut cb, serial);
            Self::push_u32(&mut cb, width);
            Self::push_u32(&mut cb, height);
            self.send_raw(Self::msg(lid, 0, &cb));
        }

        fn send_preferred_scale(&self, surface_id: u32, scale_120: u32) {
            let fs_id = self
                .state
                .lock()
                .unwrap()
                .frac_scales
                .iter()
                .find(|(_, &sid)| sid == surface_id)
                .map(|(&fid, _)| fid)
                .unwrap();
            let mut b = Vec::new();
            Self::push_u32(&mut b, scale_120);
            self.send_raw(Self::msg(fs_id, 0, &b));
        }

        fn send_pointer_move(&self, surface_id: u32, x: f64, y: f64, enter: bool) {
            let mut st = self.state.lock().unwrap();
            let pid = st.pointer_id.unwrap();
            st.next_serial += 1;
            let serial = st.next_serial;
            drop(st);
            let mut raw = Vec::new();
            let fx = (x * 256.0).round() as i32;
            let fy = (y * 256.0).round() as i32;
            if enter {
                let mut eb = Vec::new();
                Self::push_u32(&mut eb, serial);
                Self::push_u32(&mut eb, surface_id);
                Self::push_i32(&mut eb, fx);
                Self::push_i32(&mut eb, fy);
                raw.extend(Self::msg(pid, 0, &eb));
            } else {
                let mut mb = Vec::new();
                Self::push_u32(&mut mb, 100);
                Self::push_i32(&mut mb, fx);
                Self::push_i32(&mut mb, fy);
                raw.extend(Self::msg(pid, 2, &mb));
            }
            raw.extend(Self::msg(pid, 5, &[]));
            self.send_raw(raw);
        }

        fn send_pointer_leave(&self, surface_id: u32) {
            let mut st = self.state.lock().unwrap();
            let pid = st.pointer_id.unwrap();
            st.next_serial += 1;
            let serial = st.next_serial;
            drop(st);
            let mut raw = Vec::new();
            let mut lb = Vec::new();
            Self::push_u32(&mut lb, serial);
            Self::push_u32(&mut lb, surface_id);
            raw.extend(Self::msg(pid, 1, &lb));
            raw.extend(Self::msg(pid, 5, &[]));
            self.send_raw(raw);
        }

        fn send_pointer_click(&self, button: u32) {
            let mut st = self.state.lock().unwrap();
            let pid = st.pointer_id.unwrap();
            st.next_serial += 1;
            let s1 = st.next_serial;
            st.next_serial += 1;
            let s2 = st.next_serial;
            drop(st);
            let mut raw = Vec::new();
            let mut pb = Vec::new();
            Self::push_u32(&mut pb, s1);
            Self::push_u32(&mut pb, 110);
            Self::push_u32(&mut pb, button);
            Self::push_u32(&mut pb, 1); // pressed
            raw.extend(Self::msg(pid, 3, &pb));
            raw.extend(Self::msg(pid, 5, &[]));
            let mut rb = Vec::new();
            Self::push_u32(&mut rb, s2);
            Self::push_u32(&mut rb, 111);
            Self::push_u32(&mut rb, button);
            Self::push_u32(&mut rb, 0); // released
            raw.extend(Self::msg(pid, 3, &rb));
            raw.extend(Self::msg(pid, 5, &[]));
            self.send_raw(raw);
        }

        fn send_pointer_axis_value120(&self, value120: i32, absolute: f64) {
            let pid = self.state.lock().unwrap().pointer_id.unwrap();
            let mut raw = Vec::new();
            let mut sb = Vec::new();
            Self::push_u32(&mut sb, 0); // wheel
            raw.extend(Self::msg(pid, 6, &sb));
            let mut vb = Vec::new();
            Self::push_u32(&mut vb, 0); // vertical
            Self::push_i32(&mut vb, value120);
            raw.extend(Self::msg(pid, 9, &vb));
            let mut ab = Vec::new();
            Self::push_u32(&mut ab, 120);
            Self::push_u32(&mut ab, 0); // vertical
            Self::push_i32(&mut ab, (absolute * 256.0).round() as i32);
            raw.extend(Self::msg(pid, 4, &ab));
            raw.extend(Self::msg(pid, 5, &[]));
            self.send_raw(raw);
        }

        fn send_pointer_axis_continuous(&self, absolute: f64) {
            let pid = self.state.lock().unwrap().pointer_id.unwrap();
            let mut raw = Vec::new();
            let mut sb = Vec::new();
            Self::push_u32(&mut sb, 1); // finger
            raw.extend(Self::msg(pid, 6, &sb));
            let mut ab = Vec::new();
            Self::push_u32(&mut ab, 121);
            Self::push_u32(&mut ab, 0); // vertical
            Self::push_i32(&mut ab, (absolute * 256.0).round() as i32);
            raw.extend(Self::msg(pid, 4, &ab));
            raw.extend(Self::msg(pid, 5, &[]));
            self.send_raw(raw);
        }

        fn send_touch_down(&self, surface_id: u32, id: i32, x: f64, y: f64) {
            let mut st = self.state.lock().unwrap();
            let tid = st.touch_id.unwrap();
            st.next_serial += 1;
            let serial = st.next_serial;
            drop(st);
            let mut raw = Vec::new();
            let mut db = Vec::new();
            Self::push_u32(&mut db, serial);
            Self::push_u32(&mut db, 130);
            Self::push_u32(&mut db, surface_id);
            Self::push_i32(&mut db, id);
            Self::push_i32(&mut db, (x * 256.0).round() as i32);
            Self::push_i32(&mut db, (y * 256.0).round() as i32);
            raw.extend(Self::msg(tid, 0, &db));
            raw.extend(Self::msg(tid, 3, &[]));
            self.send_raw(raw);
        }

        fn send_touch_motion(&self, id: i32, x: f64, y: f64) {
            let tid = self.state.lock().unwrap().touch_id.unwrap();
            let mut raw = Vec::new();
            let mut mb = Vec::new();
            Self::push_u32(&mut mb, 131);
            Self::push_i32(&mut mb, id);
            Self::push_i32(&mut mb, (x * 256.0).round() as i32);
            Self::push_i32(&mut mb, (y * 256.0).round() as i32);
            raw.extend(Self::msg(tid, 2, &mb));
            raw.extend(Self::msg(tid, 3, &[]));
            self.send_raw(raw);
        }

        fn send_touch_up(&self, id: i32) {
            let mut st = self.state.lock().unwrap();
            let tid = st.touch_id.unwrap();
            st.next_serial += 1;
            let serial = st.next_serial;
            drop(st);
            let mut raw = Vec::new();
            let mut ub = Vec::new();
            Self::push_u32(&mut ub, serial);
            Self::push_u32(&mut ub, 132);
            Self::push_i32(&mut ub, id);
            raw.extend(Self::msg(tid, 1, &ub));
            raw.extend(Self::msg(tid, 3, &[]));
            self.send_raw(raw);
        }

        fn send_touch_cancel(&self) {
            let tid = self.state.lock().unwrap().touch_id.unwrap();
            self.send_raw(Self::msg(tid, 4, &[]));
        }

        fn send_focused_toplevel(&self, output_idx: u32) {
            let mut st = self.state.lock().unwrap();
            let mgr = st.toplevel_mgr_id.unwrap();
            let out_id = *st.output_globals.get(&output_idx).unwrap();
            let handle = st.next_server_id;
            st.next_server_id += 1;
            drop(st);
            let mut raw = Vec::new();
            let mut tb = Vec::new();
            Self::push_u32(&mut tb, handle);
            raw.extend(Self::msg(mgr, 0, &tb));
            let mut ob = Vec::new();
            Self::push_u32(&mut ob, out_id);
            raw.extend(Self::msg(handle, 2, &ob));
            // zwlr_foreign_toplevel_handle_v1::State::Activated = 2
            let mut sb = Vec::new();
            Self::push_array(&mut sb, &2u32.to_ne_bytes());
            raw.extend(Self::msg(handle, 4, &sb));
            raw.extend(Self::msg(handle, 5, &[]));
            self.send_raw(raw);
        }

        fn last_commit(&self, surface_id: u32) -> CommittedSurfaceState {
            self.state
                .lock()
                .unwrap()
                .surface_commits
                .get(&surface_id)
                .and_then(|v| v.last())
                .cloned()
                .expect("surface must have at least one commit")
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

    fn surface_wl_id(os: &OutputSurface) -> u32 {
        use smithay_client_toolkit::reexports::client::Proxy;
        os.layer.wl_surface().id().protocol_id()
    }

    fn surface_has_nonzero_pixels(os: &mut OutputSurface) -> bool {
        let idx = os.last_slot;
        let Some(slot) = &os.slots[idx] else {
            return false;
        };
        slot.buffer
            .canvas(&mut os.pool)
            .is_some_and(|bytes| bytes.iter().any(|&b| b != 0))
    }

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
        assert_eq!(c1.input_rects, vec![(0, 0, 1280, 800)]);
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

    #[test]
    fn headless_two_outputs_shows_panel_on_one_output_and_routes_click() {
        let (mock, conn) = MockWayland::start(
            vec![
                MockOutputSpec {
                    width: 1920,
                    height: 1080,
                    scale: 1,
                    fractional_120: 120,
                },
                MockOutputSpec {
                    width: 1280,
                    height: 800,
                    scale: 2,
                    fractional_120: 240,
                },
            ],
            false,
        );
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 hover settings/power\"\n",
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
        assert_eq!(app.outputs.len(), 2);

        let sid1 = surface_wl_id(&app.outputs[0]);
        let sid2 = surface_wl_id(&app.outputs[1]);
        let out2_id = app.outputs[1].id;
        let ctrl_idx = app
            .shell
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();

        // Open by hover on Output 2's 2 px strip with injected Instants (no sleep).
        let strip2 = app.shell.hover_strip(ctrl_idx, 1280.0, 800.0);
        let t0 = Instant::now();
        assert!(!app.shell.pointer_motion_on(
            out2_id,
            1280.0,
            800.0,
            strip2.0 + 10.0,
            strip2.1 + 1.0,
            t0
        ));
        assert!(app
            .shell
            .hover_tick(t0 + Duration::from_millis(crate::shell::HOVER_OPEN_MS as u64)));
        assert_eq!(app.shell.open, Some(ctrl_idx));
        assert_eq!(app.shell.open_output, Some(out2_id));
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        // Output 2 draws at 2560x1600; Output 1 stays clear with only its 2 px hover strips.
        let c1 = mock.last_commit(sid1);
        let c2 = mock.last_commit(sid2);
        assert_eq!(c2.buffer_size, Some((2560, 1600)));
        assert!(surface_has_nonzero_pixels(&mut app.outputs[1]));
        assert!(!surface_has_nonzero_pixels(&mut app.outputs[0]));
        assert_eq!(c1.keyboard_interactivity, 0); // KeyboardInteractivity::None
        let expected_strips1: Vec<(i32, i32, i32, i32)> = (0..app.shell.panels.len())
            .filter(|&i| app.shell.panels[i].panel.hover)
            .map(|i| {
                let s = app.shell.hover_strip(i, 1920.0, 1080.0);
                (
                    s.0.floor() as i32,
                    s.1.floor() as i32,
                    s.2.ceil() as i32,
                    s.3.ceil() as i32,
                )
            })
            .collect();
        assert_eq!(c1.input_rects, expected_strips1);

        // Pointer event on Output 1 (while Output 2 shows the panel) does not close or click Output 2.
        let s1 = expected_strips1[0];
        mock.send_pointer_move(sid1, f64::from(s1.0) + 10.0, f64::from(s1.1) + 1.0, true);
        mock.send_pointer_click(BTN_LEFT);
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(app.shell.open, Some(ctrl_idx));
        assert_eq!(app.shell.open_output, Some(out2_id));
        assert!(app.shell.actions.is_empty());

        // Click the second power choice ("performance") on Output 2 in logical coordinates.
        let perf_rect = app.shell.frame_hits(Some(out2_id))[0].2.list[1].1;
        mock.send_pointer_move(sid2, perf_rect.0 + 5.0, perf_rect.1 + 5.0, true);
        mock.send_pointer_click(BTN_LEFT);
        eq.roundtrip(&mut app).unwrap();
        assert!(
            app.shell.actions.iter().any(|(a, _, _, _)| matches!(
                a,
                Action::Run(cmd) if cmd.contains("performance")
            )) || app.queued.lock().unwrap().contains_key("settings/power"),
            "click on Output 2 must trigger performance action"
        );
        app.shell.actions.clear();

        // Close, focus a toplevel on Output 2, and open by key/signal -> opens on Output 2 only.
        assert!(app.shell.close_at(t0));
        app.shell
            .tick(t0 + Duration::from_millis(crate::shell::LEAVE_MS as u64 + 10));
        app.shell
            .tick(t0 + Duration::from_millis(crate::shell::LEAVE_MS as u64 + 60));
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        mock.send_focused_toplevel(1);
        eq.roundtrip(&mut app).unwrap();
        app.handle_signal_byte(b'C');
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        eq.roundtrip(&mut app).unwrap();
        app.outputs[1].dirty = true;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        assert_eq!(app.shell.open_output, Some(out2_id));
        assert!(surface_has_nonzero_pixels(&mut app.outputs[1]));
        assert!(!surface_has_nonzero_pixels(&mut app.outputs[0]));
    }

    #[test]
    fn apps_refresh_skips_when_closed_and_reloads_on_worker_when_opened() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-wayland-apps-refresh-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let apps_dir = dir.join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();

        let dirs = vec![apps_dir.clone()];
        let initial_mtimes = crate::app::dir_mtimes(&dirs);
        let mut sh = Shell::new(
            crate::config::Config::from_text("PANEL_APPS=\"SUPER+A top right 380 apps/apps\"\n"),
            crate::app::load(&dirs),
        );
        sh.live = true;
        sh.set_app_dirs_with_mtimes(dirs.clone(), initial_mtimes.clone());
        let mut refresh = AppRefresh::new(dirs, initial_mtimes);

        // Add a .desktop file while all panels are closed.
        std::fs::write(
            apps_dir.join("hello.desktop"),
            "[Desktop Entry]\nName=Hello\nExec=hello\n",
        )
        .unwrap();
        let t1 = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::open(&apps_dir)
            .unwrap()
            .set_modified(t1)
            .unwrap();

        assert!(
            refresh.check_open(None).is_none(),
            "nothing runs while all panels are closed"
        );
        assert!(sh.apps.is_empty());

        // Panel opens: detects changed folder mtime and schedules one worker load.
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "apps")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        let (scan_dirs, scan_mtimes) = refresh.check_open(Some(idx)).expect("should trigger load");
        assert!(
            refresh.check_open(Some(idx)).is_none(),
            "same opening does not stat or reload again"
        );

        type AppsMsg = (Vec<crate::app::App>, Vec<Option<std::time::SystemTime>>);
        let (fetch_tx, fetch_rx) = channel::channel::<Fetched>();
        let mut event_loop: EventLoop<Option<AppsMsg>> = EventLoop::try_new().unwrap();
        event_loop
            .handle()
            .insert_source(fetch_rx, |evt, _, out| {
                if let channel::Event::Msg(Fetched::Apps(apps, mtimes)) = evt {
                    *out = Some((apps, mtimes));
                }
            })
            .unwrap();

        assert!(spawn_apps_worker(scan_dirs, scan_mtimes, fetch_tx));
        let mut received = None;
        while received.is_none() {
            event_loop
                .dispatch(Some(Duration::from_millis(100)), &mut received)
                .unwrap();
        }
        let (apps, done_mtimes) = received.unwrap();
        refresh.mark_done(done_mtimes, true);
        assert!(sh.apply_apps(apps));
        assert_eq!(sh.open, Some(idx));
        assert_eq!(
            sh.apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            vec!["hello"]
        );

        // Closing and reopening with no folder change does not trigger another load.
        refresh.sync_closed();
        assert!(refresh.check_open(Some(idx)).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scroll_from_axis_distinguishes_discrete_value120_and_touchpad_pixels() {
        let disc = AxisScroll {
            discrete: 1,
            absolute: 10.0,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&disc), (36.0, 1.0));

        let v120 = AxisScroll {
            value120: 120,
            absolute: 15.0,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&v120), (36.0, 1.0));

        let half_v120 = AxisScroll {
            value120: 60,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&half_v120), (18.0, 0.5));

        let pad_small = AxisScroll {
            absolute: 2.5,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&pad_small), (2.5, 0.0));

        let pad_tiny = AxisScroll {
            absolute: -0.5,
            ..Default::default()
        };
        assert_eq!(scroll_from_axis(&pad_tiny), (-0.5, 0.0));
    }

    #[test]
    fn key_of_maps_navigation_editing_and_delete_keys() {
        use crate::shell::Key;
        let mods = Modifiers::default();
        let mk = |keysym| KeyEvent {
            raw_code: 0,
            keysym,
            utf8: None,
            time: 0,
        };
        assert_eq!(key_of(&mk(Keysym::Left), &mods), Some(Key::Left));
        assert_eq!(key_of(&mk(Keysym::Right), &mods), Some(Key::Right));
        assert_eq!(key_of(&mk(Keysym::Home), &mods), Some(Key::Home));
        assert_eq!(key_of(&mk(Keysym::End), &mods), Some(Key::End));
        assert_eq!(key_of(&mk(Keysym::Page_Up), &mods), Some(Key::PageUp));
        assert_eq!(key_of(&mk(Keysym::Page_Down), &mods), Some(Key::PageDown));
        assert_eq!(key_of(&mk(Keysym::Delete), &mods), Some(Key::Delete));
    }

    #[test]
    fn headless_two_outputs_touch_and_axis_route_to_open_output() {
        use crate::cards::settings_rows::ConfigData;
        let (mock, conn) = MockWayland::start(
            vec![
                MockOutputSpec {
                    width: 1920,
                    height: 1080,
                    scale: 1,
                    fractional_120: 120,
                },
                MockOutputSpec {
                    width: 1280,
                    height: 320,
                    scale: 1,
                    fractional_120: 120,
                },
            ],
            false,
        );
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/power settings/input settings/session\"\n",
            ),
            Vec::new(),
        );
        sh.apply(
            "settings",
            "power",
            &crate::cards::settings::parse_power("* balanced:\n  power-saver:\n  performance:\n")
                as &dyn std::any::Any,
        );
        let mut input_cfg = ConfigData::default();
        input_cfg
            .default
            .insert("KEYBOARD_REPEAT_DELAY".into(), "400".into());
        sh.apply("settings", "config", &input_cfg as &dyn std::any::Any);

        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert!(app.touch.is_some());

        let sid1 = surface_wl_id(&app.outputs[0]);
        let sid2 = surface_wl_id(&app.outputs[1]);
        let out2_id = app.outputs[1].id;
        assert!(app.shell.toggle_named_on(Some(out2_id), "control"));
        app.shell.open_ms = f64::INFINITY;
        app.shell.opened_at = None;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();

        let pbox2 = app.shell.shown_box(1280.0, 320.0).unwrap();

        // Pointer axis on Output 1 (panel is on Output 2) is ignored.
        mock.send_pointer_move(sid1, 100.0, 100.0, true);
        mock.send_pointer_axis_value120(120, 10.0);
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(app.shell.panel_scroll, 0.0);

        // Pointer axis on Output 2: continuous pixels 1:1, then 1 value120 notch = 36 px.
        mock.send_pointer_move(sid2, pbox2.0 + 10.0, pbox2.1 + 10.0, true);
        mock.send_pointer_axis_continuous(2.5);
        eq.roundtrip(&mut app).unwrap();
        assert!((app.shell.panel_scroll - 2.5).abs() < 1e-3);
        mock.send_pointer_axis_value120(120, 15.0);
        eq.roundtrip(&mut app).unwrap();
        assert!((app.shell.panel_scroll - 38.5).abs() < 1e-3);

        // Touch on Output 1 does nothing.
        mock.send_touch_down(sid1, 0, 100.0, 100.0);
        mock.send_touch_up(0);
        eq.roundtrip(&mut app).unwrap();
        assert!(app.shell.open.is_some());
        assert!(app.shell.actions.is_empty());

        // Touch tap on Output 2 ("performance" choice) triggers its command.
        app.shell.panel_scroll = 0.0;
        app.outputs[1].dirty = true;
        app.sync_all();
        eq.roundtrip(&mut app).unwrap();
        let perf_r = app.shell.frame_hits(Some(out2_id))[0].2.list[2].1;
        mock.send_touch_down(sid2, 1, perf_r.0 + 4.0, perf_r.1 + 4.0);
        mock.send_touch_up(1);
        eq.roundtrip(&mut app).unwrap();
        assert!(
            app.shell
                .actions
                .iter()
                .any(|(a, ..)| matches!(a, Action::Run(cmd) if cmd.contains("performance")))
                || app.queued.lock().unwrap().contains_key("settings/power")
        );
        app.shell.actions.clear();

        // Touch drag on slider followed by cancel clears state without committing.
        let slider_r = {
            let hits = app.shell.frame_hits(Some(out2_id));
            hits[1]
                .2
                .list
                .iter()
                .find(|(id, _)| hits[1].2.is_draggable(*id))
                .expect("input slider draggable")
                .1
        };
        let iy = slider_r.1 + slider_r.3 / 2.0;
        mock.send_touch_down(sid2, 2, slider_r.0 + slider_r.2 * 0.2, iy);
        mock.send_touch_motion(2, slider_r.0 + slider_r.2 * 0.8, iy);
        mock.send_touch_cancel();
        eq.roundtrip(&mut app).unwrap();
        assert!(app.shell.actions.is_empty());
        assert!(!app.queued.lock().unwrap().contains_key("settings/input"));
    }

    #[test]
    fn config_stat_throttled_to_250ms_and_never_on_frame_or_configure() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-wayland-config-stat-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config");
        std::fs::write(
            &cfg_path,
            "ACCENT=\"#112233\"\nPANEL_CONTROL=\"SUPER+A top right 380 settings/session\"\n",
        )
        .unwrap();

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
        eq.roundtrip(&mut app).unwrap();

        let t0 = Instant::now();
        let clock = std::rc::Rc::new(std::cell::Cell::new(t0));
        app.clock = Some(std::rc::Rc::clone(&clock));
        app.config_path = Some(cfg_path.clone());
        assert_eq!(app.config_stat_calls.get(), 0);

        let target = app.key_target_output();
        assert!(app.shell.toggle_named_on(target, "control"));
        app.sync_all();
        assert_eq!(app.config_stat_calls.get(), 1);

        // Within 250 ms, repeated sync_all, sync_popups, frame and configure calls never stat.
        let wl_surf = app.outputs[0].layer.wl_surface().clone();
        let sid = surface_wl_id(&app.outputs[0]);
        let qh = app.qh.clone();
        for ms in [0u64, 16, 32, 64, 128, 249] {
            clock.set(t0 + Duration::from_millis(ms));
            app.frame(&conn, &qh, &wl_surf, 0);
            mock.send_layer_configure(sid, 1280, 800);
            eq.roundtrip(&mut app).unwrap();
            app.sync_popups();
            app.sync_all();
            assert_eq!(
                app.config_stat_calls.get(),
                1,
                "config must not be statted again at +{ms} ms"
            );
        }

        // Even after 250 ms, frame and configure callbacks themselves do not stat the config.
        clock.set(t0 + Duration::from_millis(250));
        app.frame(&conn, &qh, &wl_surf, 0);
        mock.send_layer_configure(sid, 1280, 800);
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(app.config_stat_calls.get(), 1);

        // At >= 250 ms, sync_all stats once and picks up a disk change.
        std::fs::write(
            &cfg_path,
            "ACCENT=\"#445566\"\nPANEL_CONTROL=\"SUPER+A top right 380 settings/session\"\n",
        )
        .unwrap();
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&cfg_path)
            .unwrap()
            .set_modified(future)
            .unwrap();
        app.sync_all();
        assert_eq!(app.config_stat_calls.get(), 2);
        assert_eq!(app.shell.cfg.accent, [0x44, 0x55, 0x66]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn headless_pointer_enter_sets_default_cursor_shape_when_offered() {
        // 1. Compositor offers wp_cursor_shape_manager_v1: pointer enter sets Shape::Default.
        let (mock, conn) = MockWayland::start_with(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
            true,
        );
        let sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/session\"\n",
            ),
            Vec::new(),
        );
        let (fetch_tx, _fetch_rx) = channel::channel::<Fetched>();
        let (mut app, mut eq) = init_app(&conn, sh, fetch_tx).unwrap();
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert!(app.cursor_device.is_some());

        let sid = surface_wl_id(&app.outputs[0]);
        mock.send_pointer_move(sid, 100.0, 50.0, true);
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        let shapes = mock.state.lock().unwrap().cursor_shapes.clone();
        assert_eq!(shapes.len(), 1);
        assert_eq!(
            shapes[0].1,
            wp_cursor_shape_device_v1::Shape::Default as u32
        );

        // Subsequent motion does not re-send set_shape; leaving and entering again sends it with the new serial.
        mock.send_pointer_move(sid, 120.0, 60.0, false);
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        assert_eq!(mock.state.lock().unwrap().cursor_shapes.len(), 1);

        mock.send_pointer_leave(sid);
        mock.send_pointer_move(sid, 140.0, 70.0, true);
        eq.roundtrip(&mut app).unwrap();
        eq.roundtrip(&mut app).unwrap();
        let shapes = mock.state.lock().unwrap().cursor_shapes.clone();
        assert_eq!(shapes.len(), 2);
        assert!(shapes[1].0 > shapes[0].0);
        assert_eq!(
            shapes[1].1,
            wp_cursor_shape_device_v1::Shape::Default as u32
        );

        // 2. Compositor does not offer wp_cursor_shape_manager_v1: leaves cursor as is without error.
        let (mock_no, conn_no) = MockWayland::start_with(
            vec![MockOutputSpec {
                width: 1280,
                height: 800,
                scale: 1,
                fractional_120: 120,
            }],
            false,
            false,
        );
        let sh_no = Shell::new(crate::config::Config::from_text(""), Vec::new());
        let (fetch_tx_no, _fetch_rx_no) = channel::channel::<Fetched>();
        let (mut app_no, mut eq_no) = init_app(&conn_no, sh_no, fetch_tx_no).unwrap();
        eq_no.roundtrip(&mut app_no).unwrap();
        eq_no.roundtrip(&mut app_no).unwrap();
        assert!(app_no.cursor_device.is_none());
        let sid_no = surface_wl_id(&app_no.outputs[0]);
        mock_no.send_pointer_move(sid_no, 50.0, 20.0, true);
        eq_no.roundtrip(&mut app_no).unwrap();
        eq_no.roundtrip(&mut app_no).unwrap();
        assert!(mock_no.state.lock().unwrap().cursor_shapes.is_empty());
    }
}
