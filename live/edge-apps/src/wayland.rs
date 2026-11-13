//! The Wayland client of `infynitae-edge-apps` (`docs/EDGE_APPS.md` section 5): one layer
//! surface per output on the Top layer, two shm buffers each, drawn on frame callbacks, nothing
//! drawn while closed, toplevel list from the compositor, signals and request files, worker
//! threads for sources, detached actions, and the `org.freedesktop.Notifications` strip.

use crate::card::{Action, Button, Data, Source, Window};
use crate::shell::{Shell, ARRIVE_LIFT};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
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
            protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
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
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT,
        },
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
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// the same files as today's edge: only one edge runs at a time, and every caller finds it so
const PID_FILE: &str = "infynitae-edge-rs.pid";
const PANEL_FILE: &str = "infynitae-edge-rs.panel";

pub fn runtime_path(name: &str) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/tmp".to_string());
    PathBuf::from(dir).join(name)
}

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

struct OutputSurface {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    pool: SlotPool,
    slots: [Option<BufferSlot>; 2],
    /// the box last committed (in either buffer): the next frame damages it too
    committed_box: Option<(i32, i32, i32, i32)>,
    width: u32,
    height: u32,
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

impl OutputSurface {
    fn set_input_region(&self, compositor: &CompositorState, shell: &Shell) {
        let Ok(region) = Region::new(compositor) else {
            return;
        };
        let (w, h) = (self.width as f64, self.height as f64);
        if w > 0.0 && h > 0.0 {
            if shell.ask.ask.on {
                // the question takes the whole screen: a click outside it cancels
                region.add(0, 0, w.ceil() as i32, h.ceil() as i32);
            } else if let Some(idx) = shell.open {
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
        let active = shell.active();
        if !active && self.fullscreen_under {
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
        self.layer.set_keyboard_interactivity(if active {
            KeyboardInteractivity::Exclusive
        } else {
            KeyboardInteractivity::None
        });
        self.set_input_region(compositor, shell);
        if active {
            self.dirty = true;
            self.mapped_closed = false;
            self.clear_pending = false;
            self.render_if_needed(qh, shell);
        } else {
            self.dirty = false;
            self.frame_pending = false;
            self.clear_and_commit(qh);
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
        let (w, h) = (self.width as i32, self.height as i32);
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
    fn clear_and_commit(&mut self, qh: &QueueHandle<App>) {
        if self.mapped_closed {
            self.layer.commit();
            return;
        }
        let (w, h) = (self.width as i32, self.height as i32);
        let Some(idx) = self.acquire_slot() else {
            self.clear_pending = true;
            let wl = self.layer.wl_surface();
            wl.frame(qh, wl.clone());
            self.layer.commit();
            return;
        };
        self.clear_pending = false;
        let slot = self.slots[idx].as_mut().unwrap();
        let old = slot.prev_box.take();
        if let Some(canvas) = slot.buffer.canvas(&mut self.pool) {
            if let Some(r) = old {
                Self::clear_box(canvas, w * 4, w, h, r);
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
            self.layer.commit();
        }
        crate::boxes::dump_closed();
    }

    fn render_if_needed(&mut self, qh: &QueueHandle<App>, shell: &Shell) {
        if !self.configured || !self.dirty || self.frame_pending || !shell.active() {
            return;
        }
        let (w, h) = (self.width as i32, self.height as i32);
        let stride = w * 4;
        let Some(slot_idx) = self.acquire_slot() else {
            // both buffers still held: commit unchanged and draw on the callback
            let wl = self.layer.wl_surface();
            wl.frame(qh, wl.clone());
            self.layer.commit();
            self.frame_pending = true;
            return;
        };
        let Some((bx, by, bw, bh)) = shell.shown_box(w as f64, h as f64) else {
            return;
        };
        let pad = 4;
        let lift = if shell.open_ms.is_finite() && !shell.ask.ask.on {
            ARRIVE_LIFT.ceil() as i32
        } else {
            0
        };
        let new_box = (
            (bx.floor() as i32 - pad).max(0),
            (by.floor() as i32 - pad).max(0),
            (bw.ceil() as i32 + 2 * pad).min(w),
            (bh.ceil() as i32 + 2 * pad + lift).min(h),
        );
        let slot = self.slots[slot_idx].as_mut().unwrap();
        let old_box = slot.prev_box.replace(new_box);
        if let Some(canvas) = slot.buffer.canvas(&mut self.pool) {
            match old_box {
                Some(r) => Self::clear_box(canvas, stride, w, h, r),
                None => canvas.fill(0),
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
                    shell.draw(&cr, w as f64, h as f64);
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
            self.layer.commit();
        }
        crate::boxes::dump(shell, w as f64, h as f64);
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
    Data(&'static str, &'static str, Data),
}

type SourceId = (&'static str, &'static str);

struct SourceEntry {
    last: Instant,
    every: Option<Duration>,
}

/// When each source on the open panel last ran and its interval (`None` = once per panel opening).
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
                self.records
                    .entry(id)
                    .or_insert(SourceEntry { last: now, every });
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
            let Some(src) = make(id.0, id.1) else {
                continue;
            };
            let every = src.every();
            self.records.insert(id, SourceEntry { last: now, every });
            self.in_flight.insert(id, every);
            due_list.push((id, src));
        }
        due_list
    }

    fn mark_done(&mut self, id: SourceId, now: Instant) {
        self.in_flight.remove(&id);
        if self.open_panel.is_some() {
            if let Some(entry) = self.records.get_mut(&id) {
                entry.last = now;
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

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    qh: QueueHandle<App>,
    outputs: Vec<OutputSurface>,
    toplevels: Vec<Toplevel>,
    next_toplevel_id: u64,
    fullscreen_changed: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    modifiers: Modifiers,
    shell: Shell,
    fetch_tx: Sender<Fetched>,
    fetch: FetchSchedule,
    /// cards with a command running, and the latest one waiting after it
    queued: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Option<String>>>>,
    was_animating: bool,
    /// the notification pop-ups' strip, made with the first pop-up
    popups: Option<PopupLayer>,
    /// the session bus connection serving org.freedesktop.Notifications
    notify_conn: Option<zbus::blocking::Connection>,
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
            committed_box: None,
            width: 0,
            height: 0,
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

    fn surface_dims(&self, surface: &wl_surface::WlSurface) -> Option<(f64, f64)> {
        self.outputs
            .iter()
            .find(|o| o.layer.wl_surface() == surface && o.configured)
            .map(|o| (o.width as f64, o.height as f64))
    }

    /// Reloads config, theme and boards when changed on disk, marking active surfaces dirty.
    fn reload_config(&mut self) -> bool {
        if !self.shell.reload_if_changed() {
            return false;
        }
        if self.shell.active() {
            for os in &mut self.outputs {
                os.dirty = true;
            }
        }
        true
    }

    /// After anything changed: the actions run, the sources fetched as needed, every surface drawn.
    fn sync_all(&mut self) {
        self.reload_config();
        self.run_actions();
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

    /// The open panel's sources, fetched on worker threads (each at most once at a time); `force`
    /// fetches again before its time.
    fn fetch_due(&mut self, force: bool) {
        let reloaded = if self.shell.open.is_some() {
            self.reload_config()
        } else {
            false
        };
        let Some(open) = self.shell.open else {
            self.fetch.sync_panel(None, &[]);
            return;
        };
        let force = force || self.shell.take_fetch_needed();
        let sources = self.shell.sources();
        let now = Instant::now();
        for ((prov, key), mut src) in
            self.fetch
                .take_due(Some(open), &sources, force, now, |p, k| {
                    crate::provider::by_id(p).and_then(|prov| prov.source(k))
                })
        {
            let tx = self.fetch_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Fetched::Data(prov, key, src.fetch()));
            });
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
                // the shell takes these before they get here
                Action::SetQuery(_) | Action::ShowCards(_) => {}
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
                let name = std::fs::read_to_string(runtime_path(PANEL_FILE)).unwrap_or_default();
                let name = name.trim();
                if name == "stop" {
                    self.exit = true;
                    false
                } else {
                    self.shell.toggle_named(name)
                }
            }
            b'K' => {
                // a question in the ask file, or a cancel; else: fetch the open panel's data again
                // (a script changed something)
                if self.shell.ask_load() {
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
    fn frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.shell.active()
            || self.popups.as_ref().is_some_and(|p| {
                p.layer.wl_surface() == surface && !self.shell.notify.popups.is_empty()
            })
        {
            let _ = self.reload_config();
        }
        // an animation running (the cards arriving, a list opening, drag autoscroll): draw again
        let animating = self.shell.open.is_some()
            && (self.shell.tick(Instant::now()) || self.shell.wants_frame());
        let ended = self.was_animating && !animating;
        self.was_animating = animating;
        if let Some(os) = self
            .outputs
            .iter_mut()
            .find(|o| o.layer.wl_surface() == surface)
        {
            os.frame_pending = false;
            if animating {
                os.dirty = true;
            }
            if os.dirty {
                os.render_if_needed(qh, &self.shell);
            } else if os.clear_pending {
                os.clear_and_commit(qh);
            }
        }
        if ended {
            // the panel's size settled: its input region follows
            self.sync_all();
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
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.create_output_surface(output);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.outputs.retain(|o| o.output != output);
        if self.popups.as_ref().is_some_and(|p| p.output == output) {
            // the next pop-up makes the strip again, on the first screen left
            self.popups = None;
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.outputs.retain(|o| &o.layer != layer);
        if self.popups.as_ref().is_some_and(|p| &p.layer == layer) {
            self.popups = None;
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
        if self.shell.active()
            || self
                .popups
                .as_ref()
                .is_some_and(|p| &p.layer == layer && !self.shell.notify.popups.is_empty())
        {
            let _ = self.reload_config();
        }
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
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
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
            if let Some(p) = self.pointer.take() {
                p.release();
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
        Keysym::Up => Key::Up,
        Keysym::Down => Key::Down,
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
        // held: arrows, Backspace and characters repeat; Escape and Enter do not
        if let Some(k) = key_of(&event, &self.modifiers).filter(|k| {
            !matches!(
                k,
                crate::shell::Key::Escape | crate::shell::Key::Enter | crate::shell::Key::Super(_)
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
            let Some((w, h)) = self.surface_dims(&event.surface) else {
                continue;
            };
            let (x, y) = event.position;
            changed |= match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.shell.pointer_motion(w, h, x, y)
                }
                PointerEventKind::Leave { .. } => self.shell.pointer_leave(),
                PointerEventKind::Press { button, .. } => {
                    let b = match button {
                        BTN_LEFT => Button::Left,
                        BTN_RIGHT => Button::Right,
                        BTN_MIDDLE => Button::Middle,
                        _ => continue,
                    };
                    self.shell.click(w, h, x, y, b)
                }
                PointerEventKind::Release { .. } => self.shell.release(),
                PointerEventKind::Axis { vertical, .. } => {
                    let dy = if vertical.discrete != 0 {
                        vertical.discrete as f64
                    } else {
                        vertical.absolute
                    };
                    dy.abs() > 0.0 && self.shell.scroll(x, y, dy)
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

    let mut shell = Shell::new(
        crate::config::Config::load(),
        crate::app::load(&crate::app::dirs()),
    );
    shell.live = true;
    let (fetch_tx, fetch_rx) = channel::channel::<Fetched>();
    let (notify_tx, notify_rx) = channel::channel::<crate::notify::Incoming>();
    let output_state = OutputState::new(&globals, &qh);
    let existing: Vec<_> = output_state.outputs().collect();
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
        fullscreen_changed: false,
        keyboard: None,
        pointer: None,
        modifiers: Modifiers::default(),
        shell,
        fetch_tx,
        fetch: FetchSchedule::default(),
        queued: Default::default(),
        was_animating: false,
        popups: None,
        notify_conn: None,
        exit: false,
    };
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
    for output in existing {
        app.create_output_surface(output);
    }
    // notes the config file's time: a later change is read again
    app.shell.reload_if_changed();
    if let Some(name) = initial_panel {
        app.shell.toggle_named(name);
    }
    app.sync_all();

    let mut event_loop: EventLoop<App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let handle = event_loop.handle();
    handle
        .insert_source(fetch_rx, |evt, _, app| {
            if let channel::Event::Msg(Fetched::Data(prov, key, data)) = evt {
                app.fetch.mark_done((prov, key), Instant::now());
                let changed = app.shell.apply(prov, key, data.as_ref());
                let reloaded = app.reload_config();
                if (changed || reloaded) && app.shell.open.is_some() {
                    for os in &mut app.outputs {
                        os.dirty = true;
                        os.render_if_needed(&app.qh, &app.shell);
                    }
                    // a card that grew or shrank: the input region follows
                    for os in &mut app.outputs {
                        os.set_input_region(&app.compositor, &app.shell);
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
        .insert(handle)
        .map_err(|e| e.to_string())?;

    // callers read this to know the running edge can ask (live/infynitae-format-drive)
    let features_file = runtime_path(crate::ask::FEATURES_FILE);
    write_runtime_file(&features_file, "ask\n");
    // a question sent before the edge started
    if app.shell.ask_load() {
        app.sync_all();
    }
    while !app.exit {
        if app.shell.open.is_some() {
            app.shell.now = chrono::Local::now().naive_local();
            app.fetch_due(false);
        }
        // while a panel is open, wake when its shortest periodic source interval is next due;
        // closed (or only once-per-open sources), nothing wakes the edge for data fetches
        let mut timeout = app.fetch_timeout(Instant::now());
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

/// The notification pop-ups' surface (in place of mako): a strip down the right of the first
/// screen on the Top layer, taking the pointer only on the cards, and unmapped (no buffer) while
/// there is nothing to show, so it costs nothing and is never one more surface over a full-screen
/// window.
struct PopupLayer {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    pool: SlotPool,
    buffer: Option<Buffer>,
    width: u32,
    height: u32,
    configured: bool,
    /// a buffer is attached (the surface shows)
    mapped: bool,
    frame_pending: bool,
    /// draw again on the next frame (a card still sliding in)
    again: bool,
    /// where the pointer is on it
    pointer: Option<(f64, f64)>,
}

/// The strip's width: the cards, their margin from the screen's right, a little room.
const POPUP_STRIP_W: u32 = (crate::notify::POPUP_W + crate::notify::MARGIN + 16.0) as u32;

impl PopupLayer {
    fn new(app: &App, output: wl_output::WlOutput) -> Option<PopupLayer> {
        let pool = SlotPool::new(4096, &app.shm).ok()?;
        let surface = app.compositor.create_surface(&app.qh);
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
            layer,
            pool,
            buffer: None,
            width: 0,
            height: 0,
            configured: false,
            mapped: false,
            frame_pending: false,
            again: false,
            pointer: None,
        })
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
        let (w, h) = (self.width as i32, self.height as i32);
        let stride = w * 4;
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };
        canvas.fill(0);
        let now = Instant::now();
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
                crate::notify::draw(&cr, &shell.theme, n, w as f64, self.pointer, now);
            }
            surface.flush();
        }
        // only the cards take the pointer; the rest of the strip lets clicks through
        if let Ok(region) = Region::new(compositor) {
            for (_, r) in crate::notify::layout(n, w as f64) {
                region.add(r.0 as i32, r.1 as i32, r.2.ceil() as i32, r.3.ceil() as i32);
            }
            self.layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        let wl = self.layer.wl_surface();
        wl.damage_buffer(0, 0, w, h);
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
        sched.mark_done(("about", "info"), t0 + Duration::from_millis(10));

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
        sched.mark_done(("about", "info"), t0 + Duration::from_secs(62));

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
        sched.mark_done(("about", "info"), t0 + Duration::from_millis(20));
        sched.mark_done(("audio", "sink"), t_done_fast);
        assert_eq!(
            sched.next_timeout(&ids, t_done_fast),
            Some(Duration::from_secs(2))
        );

        let t_done_slow = t0 + Duration::from_millis(300);
        sched.mark_done(("weather", "now"), t_done_slow);
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
        sched.mark_done(("audio", "sink"), t_tick1 + Duration::from_millis(20));

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
        sched.mark_done(("about", "info"), t0 + Duration::from_millis(40));
        sched.mark_done(("audio", "sink"), t0 + Duration::from_millis(40));

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
        sched.mark_done(("about", "info"), t0 + Duration::from_millis(10));

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
}
