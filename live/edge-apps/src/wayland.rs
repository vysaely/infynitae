//! The Wayland client of `infynitae-edge-apps` (`docs/EDGE_APPS.md` section 5): one layer
//! surface per output on the Top layer, two shm buffers each, drawn on frame callbacks, nothing
//! drawn while closed, toplevel list from the compositor, signals and request files, worker
//! threads for sources, detached actions, and the `org.freedesktop.Notifications` strip.

use crate::card::{Action, Button, CommandResult, Data, Source, Window};
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
use std::collections::{HashMap, VecDeque};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod handlers;
mod surfaces;
mod workers;

use surfaces::{OutputSurface, PopupLayer, SurfaceKind};
use workers::{
    refresh_sources, run_queued_with, run_sh_result, spawn_apps_worker, spawn_worker, AppRefresh,
    CommandQueue, FetchSchedule, Fetched, SourceId, SourceWorker,
};

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

struct Toplevel {
    id: u64,
    handle: ZwlrForeignToplevelHandleV1,
    title: String,
    app_id: String,
    focused: bool,
    fullscreen: bool,
    outputs: Vec<wl_output::WlOutput>,
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
    /// cards with a command running, and distinct commands waiting in order
    queued: std::sync::Arc<std::sync::Mutex<CommandQueue>>,
    /// the launcher's file search (`Action::Locate`)
    locator: crate::locate::Locator,
    /// tests: the commands the actions asked for, in order, recorded when asked (the worker does
    /// not run them in tests: they would change the machine's settings; and its exit, which
    /// empties `queued`, does not race a test's check)
    #[cfg(test)]
    ran: Vec<String>,
    was_animating: bool,
    /// the notification pop-ups' strip, made with the first pop-up
    popups: Option<PopupLayer>,
    /// the desktop widgets' surface on the first screen, there only while the WIDGETS line has one
    widget_layer: Option<PopupLayer>,
    /// the volume/brightness OSD surface, made lazily with the first level notification
    osd_layer: Option<PopupLayer>,
    /// the touch point that went down on a widget
    widget_touch: Option<i32>,
    /// active touch point on the notification pop-ups strip (id, start position)
    popup_touch: Option<(i32, (f64, f64))>,
    /// the session bus connection serving org.freedesktop.Notifications
    notify_conn: Option<zbus::blocking::Connection>,
    /// serves the tray watcher while the edge runs (`tray_watcher.rs`)
    tray_conn: Option<zbus::blocking::Connection>,
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
        if !self.shell.notify.popups.is_empty() || self.shell.notify.osd.is_some() {
            self.reload_config();
        }
        if self.popups.is_none() && !self.shell.notify.popups.is_empty() {
            if let Some(output) = self.outputs.first().map(|o| o.output.clone()) {
                self.popups = PopupLayer::new(self, output, SurfaceKind::Popups);
            }
        }
        if let Some(pl) = self.popups.as_mut() {
            pl.render(&self.qh, &self.compositor, &self.shell);
        }
        if self.shell.notify.osd.is_some() {
            if let Some(output) = self.target_osd_output() {
                if self
                    .osd_layer
                    .as_ref()
                    .is_some_and(|ol| ol.output != output)
                {
                    self.osd_layer = None;
                }
                if self.osd_layer.is_none() {
                    self.osd_layer = PopupLayer::new(self, output, SurfaceKind::Osd);
                }
            }
        }
        if let Some(ol) = self.osd_layer.as_mut() {
            if ol.render(&self.qh, &self.compositor, &self.shell) {
                self.dump_osd_boxes();
            }
        }
        if self.shell.open.is_some() {
            // the open panel's cards list them too
            self.sync_all();
        }
    }

    fn target_osd_output(&self) -> Option<wl_output::WlOutput> {
        self.key_target_output()
            .and_then(|id| self.outputs.iter().find(|o| o.id == id))
            .or_else(|| self.outputs.first())
            .map(|o| o.output.clone())
    }

    pub(super) fn dump_osd_boxes(&self) {
        let os = self
            .shell
            .visible_output()
            .and_then(|id| self.outputs.iter().find(|o| o.id == id))
            .or_else(|| {
                self.osd_layer
                    .as_ref()
                    .and_then(|ol| self.outputs.iter().find(|o| o.output == ol.output))
            })
            .or_else(|| self.outputs.first());
        if let Some(os) = os.filter(|o| o.width > 0 && o.height > 0) {
            crate::boxes::dump(&self.shell, f64::from(os.width), f64::from(os.height));
        } else if self.shell.notify.osd.is_none() && !self.shell.visible() {
            crate::boxes::dump_closed();
        }
    }

    /// The widgets' surface made on the first screen when the WIDGETS line has a widget and
    /// removed when it has none (no surface at all without the line).
    fn sync_widgets(&mut self) {
        let want = !self.shell.widgets.list.is_empty();
        if !want {
            self.widget_layer = None;
            self.widget_touch = None;
        } else if self.widget_layer.is_none() {
            if let Some(output) = self.outputs.first().map(|o| o.output.clone()) {
                self.widget_layer = PopupLayer::new(self, output, SurfaceKind::Widgets);
            }
        }
    }

    fn redraw_widgets(&mut self) {
        if let Some(wl) = self.widget_layer.as_mut() {
            wl.render(&self.qh, &self.compositor, &self.shell);
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
            below: false,
            dirty: false,
            frame_pending: false,
            release_pending: false,
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
        let mut osd_changed = false;
        for pl in self
            .popups
            .iter_mut()
            .chain(self.widget_layer.iter_mut())
            .chain(self.osd_layer.iter_mut())
            .filter(|p| &p.output == output)
        {
            if pl.integer_scale != scale {
                pl.integer_scale = scale;
                if pl.viewport.is_none()
                    && pl.render(qh, &self.compositor, &self.shell)
                    && pl.kind == SurfaceKind::Osd
                {
                    osd_changed = true;
                }
            }
        }
        if osd_changed {
            self.dump_osd_boxes();
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
        // the WIDGETS line or the look may have changed
        self.sync_widgets();
        self.redraw_widgets();
        true
    }

    /// After anything changed: the actions run, the sources fetched as needed, every surface drawn.
    fn sync_all(&mut self) {
        self.reload_config();
        self.sync_widgets();
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

    /// What the sources are fetched for: an open panel (its index + 1), else the desktop's widgets
    /// alone (0); None: nothing shows, nothing is fetched. When it changes, the sources fetched
    /// once per showing are fetched again.
    fn shown_key(&self) -> Option<usize> {
        self.shell
            .open
            .map(|i| i + 1)
            .or((!self.shell.widgets.list.is_empty()).then_some(0))
    }

    /// The open panel's sources and the widgets'.
    fn shown_sources(&self) -> Vec<SourceId> {
        let mut sources = self.shell.sources();
        for id in self.shell.widget_sources() {
            if !sources.contains(&id) {
                sources.push(id);
            }
        }
        sources
    }

    /// The open panel's and the widgets' sources, fetched on one long-lived worker thread per
    /// source while shown; `force` fetches again before its time.
    fn fetch_due(&mut self, force: bool) {
        let reloaded = if self.shell.open.is_some() {
            self.reload_config()
        } else {
            false
        };
        match self.shell.open {
            Some(open) => {
                if let Some((dirs, mtimes)) = self.app_refresh.check_open(Some(open)) {
                    if !spawn_apps_worker(dirs, mtimes, self.fetch_tx.clone()) {
                        self.app_refresh.in_flight = false;
                    }
                }
            }
            None => {
                self.app_refresh.sync_closed();
                // a file search still running for a panel no longer shown
                self.locator.cancel();
            }
        }
        let Some(shown) = self.shown_key() else {
            self.fetch.sync_panel(None, &[]);
            self.workers.clear();
            return;
        };
        let force = force || self.shell.take_fetch_needed();
        let sources = self.shown_sources();
        self.workers.retain(|id, _| sources.contains(id));
        let now = Instant::now();
        for (id, src) in self
            .fetch
            .take_due(Some(shown), &sources, force, now, |p, k| {
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

    fn redraw_open_outputs(&mut self) {
        if self.shell.open.is_none() {
            return;
        }
        for os in &mut self.outputs {
            if self.shell.visible_on(os.id) {
                os.dirty = true;
                os.render_if_needed(&self.qh, &self.shell);
                // a card that grew or shrank: the input region follows
                os.set_input_region(&self.compositor, &self.shell);
            }
        }
    }

    fn fetch_timeout(&self, now: Instant) -> Option<Duration> {
        self.shown_key()?;
        let sources = self.shown_sources();
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
                    #[cfg(test)]
                    self.ran.push(cmd.clone());
                    let start = self
                        .queued
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .enqueue(&card, cmd.clone());
                    if !start {
                        continue;
                    }
                    let (tx, queued) = (self.fetch_tx.clone(), self.queued.clone());
                    let keys: Vec<&'static str> = keys.to_vec();
                    std::thread::spawn(move || {
                        run_queued_with(&queued, &card, cmd, |c| {
                            let res = run_sh_result(c);
                            let _ = tx.send(Fetched::CommandDone(card.clone(), res));
                        });
                        // what the commands changed, shown at once
                        refresh_sources(&tx, prov, &keys);
                    });
                }
                Action::RunBackground(cmd) => {
                    #[cfg(test)]
                    self.ran.push(cmd.clone());
                    let tx = self.fetch_tx.clone();
                    let keys: Vec<&'static str> = keys.to_vec();
                    std::thread::spawn(move || {
                        let res = run_sh_result(&cmd);
                        let _ = tx.send(Fetched::CommandDone(card, res));
                        refresh_sources(&tx, prov, &keys);
                    });
                }
                Action::Launch(id) => {
                    if let Some(app) = self.shell.apps.iter().find(|x| x.id == id) {
                        let cmd = if app.terminal {
                            format!(
                                "\"${{INFYNITAE_DIR:-$HOME/infynitae}}/live/launch.sh\" terminal sh -c {}",
                                crate::cards::settings::sq(&app.exec)
                            )
                        } else {
                            app.exec.clone()
                        };
                        spawn_detached(&cmd);
                        self.shell.note_launch(&id);
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
                Action::Locate(q) => {
                    let tx = self.fetch_tx.clone();
                    self.locator.start(&q, move |found| {
                        let _ = tx.send(Fetched::Data("apps", "located", Box::new(found)));
                    });
                }
                // the shell takes these before they get here
                Action::SetQuery(_)
                | Action::ShowCards(_)
                | Action::RemoveCard
                | Action::OpenPanel(..) => {}
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
        locator: crate::locate::Locator::new("plocate"),
        #[cfg(test)]
        ran: Vec::new(),
        was_animating: false,
        popups: None,
        widget_layer: None,
        osd_layer: None,
        widget_touch: None,
        popup_touch: None,
        notify_conn: None,
        tray_conn: None,
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
    if let Some(p) = crate::recent::path() {
        shell.load_recent(p);
    }
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
    // the tray's watcher, which the Background card reads (or another one's, see the module)
    match crate::tray_watcher::start() {
        Ok(conn) => app.tray_conn = Some(conn),
        Err(e) => eprintln!("infynitae-edge-apps: tray: no watcher: {e}"),
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
                channel::Event::Msg(Fetched::CommandDone(card, res)) => {
                    if app.shell.command_done(&card, &res, Instant::now()) {
                        app.redraw_open_outputs();
                        app.redraw_widgets();
                    }
                    return;
                }
                _ => return,
            };
            let changed = app.shell.apply(prov, key, data.as_ref());
            let reloaded = app.reload_config();
            if changed || reloaded {
                app.redraw_open_outputs();
            }
            if changed && app.shell.widget_sources().contains(&(prov, key)) {
                app.redraw_widgets();
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
                app.redraw_widgets();
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
        if app.shell.open.is_some() || !app.shell.widgets.list.is_empty() {
            let prev = app.shell.now;
            let now = chrono::Local::now().naive_local();
            app.shell.now = now;
            let new_minute = minute_key(prev) != minute_key(now);
            if app.shell.open.is_some() && app.shell.wants_clock() && new_minute {
                for os in &mut app.outputs {
                    if app.shell.visible_on(os.id) {
                        os.dirty = true;
                        os.render_if_needed(&app.qh, &app.shell);
                    }
                }
            }
            if new_minute && app.shell.widgets_want_clock() {
                app.redraw_widgets();
            }
            app.fetch_due(false);
        }
        // while a panel is open or widgets show, wake when the shortest periodic source interval
        // is next due; closed without widgets (or only once-per-open sources), nothing wakes the
        // edge for data fetches
        let mut timeout = app.fetch_timeout(Instant::now());
        if (app.shell.open.is_some() && app.shell.wants_clock()) || app.shell.widgets_want_clock() {
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
        // transient card error messages clear cleanly without a 165 Hz tick loop
        if app.shell.card_deadline().is_some() {
            let now = Instant::now();
            if app.shell.expire_cards(now) {
                app.redraw_open_outputs();
                app.redraw_widgets();
            }
            if let Some(d) = app.shell.card_deadline() {
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

/// What of the screen takes the pointer while panel `idx` is open. Opened by hover: the panel
/// and the way to it, so leaving closes it. Opened by its key: the panel alone, so the desktop
/// around it keeps working (owner, 2026-11-22). Editing or a menu of the edge open: the whole
/// screen, so a click outside reaches the edge.
fn open_input_rect(shell: &Shell, idx: usize, w: f64, h: f64) -> (f64, f64, f64, f64) {
    if shell.opened_by_hover {
        shell.hover_box(idx, w, h)
    } else if shell.editing() || shell.edit_menu.is_some() || shell.window_menu.is_some() {
        (0.0, 0.0, w, h)
    } else {
        shell.layout(idx, w, h).0
    }
}

fn keyboard_interactivity(shell: &Shell) -> KeyboardInteractivity {
    if !shell.active() {
        KeyboardInteractivity::None
    } else if shell.ask.ask.on || shell.shortcuts.open || shell.editing() {
        KeyboardInteractivity::Exclusive
    } else if shell.opened_by_hover || shell.keyboard_held {
        // on demand: a click on the panel gives it the keyboard, a click on a window gives it
        // back (owner, 2026-11-22). The compositor focuses only an exclusive layer when it opens,
        // so a panel opened by its key asks for it exclusively until the keyboard has come.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    pub(super) struct CountingSource {
        pub(super) calls: Arc<AtomicUsize>,
        pub(super) every: Option<Duration>,
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

    pub(super) fn run_due(
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
    fn an_open_panel_holds_the_keyboard_on_demand() {
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
        sh.click(1280.0, 800.0, ox + 10.0, oy + 10.0, Button::Left);
        assert_eq!(sh.open, Some(idx), "a click inside keeps the panel open");
        assert_eq!(
            keyboard_interactivity(&sh),
            KeyboardInteractivity::OnDemand,
            "a click inside a hover panel keeps it on demand, so a click on a window takes the keyboard"
        );

        assert!(sh.close_at(t0));
        assert_eq!(keyboard_interactivity(&sh), KeyboardInteractivity::None);

        assert!(sh.open_panel(idx, false, t0));
        assert_eq!(
            keyboard_interactivity(&sh),
            KeyboardInteractivity::Exclusive,
            "key-opened panel takes Exclusive keyboard interactivity at once"
        );
        sh.keyboard_held = true;
        assert_eq!(
            keyboard_interactivity(&sh),
            KeyboardInteractivity::OnDemand,
            "once the keyboard came, a key-opened panel holds it on demand so windows can take it"
        );
        assert!(sh.close_at(t0));
        assert!(sh.open_panel(idx, false, t0));
        assert_eq!(
            keyboard_interactivity(&sh),
            KeyboardInteractivity::Exclusive,
            "opening again takes the keyboard again"
        );
        let (pbox, _) = sh.layout(idx, 1280.0, 800.0);
        assert_eq!(
            open_input_rect(&sh, idx, 1280.0, 800.0),
            pbox,
            "a key-opened panel takes the pointer only over itself"
        );
        assert!(sh.close_at(t0));

        assert!(sh.toggle_named_on(None, "shortcuts"));
        assert_eq!(
            keyboard_interactivity(&sh),
            KeyboardInteractivity::Exclusive,
            "shortcuts overlay takes Exclusive keyboard interactivity"
        );
        assert!(sh.close_shortcuts());
        assert_eq!(keyboard_interactivity(&sh), KeyboardInteractivity::None);
    }

    #[derive(Clone, Debug)]
    pub(super) struct MockOutputSpec {
        pub(super) width: i32,
        pub(super) height: i32,
        pub(super) scale: i32,
        pub(super) fractional_120: u32,
    }

    #[derive(Clone, Debug)]
    pub(super) struct CommittedSurfaceState {
        pub(super) buffer_size: Option<(i32, i32)>,
        pub(super) buffer_scale: i32,
        pub(super) viewport_dest: Option<(i32, i32)>,
        pub(super) input_rects: Vec<(i32, i32, i32, i32)>,
        pub(super) keyboard_interactivity: u32,
    }

    #[derive(Default)]
    pub(super) struct MockServerState {
        pub(super) surfaces: HashMap<u32, usize>,
        surface_commits: HashMap<u32, Vec<CommittedSurfaceState>>,
        surface_scale: HashMap<u32, i32>,
        surface_buf: HashMap<u32, Option<u32>>,
        surface_input: HashMap<u32, Vec<(i32, i32, i32, i32)>>,
        surface_viewport_dest: HashMap<u32, Option<(i32, i32)>>,
        surface_kb: HashMap<u32, u32>,
        layers_by_surface: HashMap<u32, u32>,
        /// each layer surface's layer (0 background, 1 bottom, 2 top, 3 overlay) and name
        pub(super) layer_kind: HashMap<u32, (u32, String)>,
        layer_to_surface: HashMap<u32, u32>,
        layer_to_output_global: HashMap<u32, u32>,
        layer_requested_size: HashMap<u32, (u32, u32)>,
        layer_configured: std::collections::HashSet<u32>,
        layer_was_mapped: std::collections::HashSet<u32>,
        layer_unmapped: std::collections::HashSet<u32>,
        buffers: HashMap<u32, (i32, i32)>,
        regions: HashMap<u32, Vec<(i32, i32, i32, i32)>>,
        viewports: HashMap<u32, u32>,
        frac_scales: HashMap<u32, u32>,
        output_globals: HashMap<u32, u32>,
        output_obj_to_global: HashMap<u32, u32>,
        pointer_id: Option<u32>,
        pub(super) cursor_shapes: Vec<(u32, u32)>,
        touch_id: Option<u32>,
        toplevel_mgr_id: Option<u32>,
        next_serial: u32,
        next_server_id: u32,
    }

    pub(super) struct MockWayland {
        server_sock: std::os::unix::net::UnixStream,
        pub(super) state: Arc<std::sync::Mutex<MockServerState>>,
        pub(super) join: Option<std::thread::JoinHandle<()>>,
    }

    impl MockWayland {
        pub(super) fn start(
            specs: Vec<MockOutputSpec>,
            offer_fractional: bool,
        ) -> (Self, Connection) {
            Self::start_with(specs, offer_fractional, true)
        }

        pub(super) fn start_with(
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
                            let layer = Self::read_u32(body, &mut off);
                            let namespace = Self::read_str(body, &mut off);
                            objs.insert(lid, Obj::LayerSurface);
                            let mut st = state.lock().unwrap();
                            st.layer_kind.insert(sid, (layer, namespace));
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
                        Some(Obj::LayerSurface) => match opcode {
                            0 => {
                                let w = Self::read_u32(body, &mut off);
                                let h = Self::read_u32(body, &mut off);
                                state
                                    .lock()
                                    .unwrap()
                                    .layer_requested_size
                                    .insert(obj_id, (w, h));
                            }
                            4 => {
                                let kb = Self::read_u32(body, &mut off);
                                let mut st = state.lock().unwrap();
                                if let Some(&sid) = st.layer_to_surface.get(&obj_id) {
                                    st.surface_kb.insert(sid, kb);
                                }
                            }
                            _ => {}
                        },
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
                                let buf_id = st.surface_buf.get(&obj_id).copied().flatten();
                                let buf_sz = buf_id.and_then(|b| st.buffers.get(&b).copied());
                                if let Some(lid) = lid {
                                    let first = st.layer_configured.insert(lid);
                                    let mapped = buf_sz.is_some();
                                    let was_mapped = st.layer_was_mapped.contains(&lid);
                                    let unmapped = st.layer_unmapped.contains(&lid);
                                    let remap = !mapped && !was_mapped && unmapped;
                                    if mapped {
                                        st.layer_was_mapped.insert(lid);
                                    } else {
                                        st.layer_was_mapped.remove(&lid);
                                    }
                                    if !mapped && (was_mapped || remap) {
                                        st.layer_unmapped.insert(lid);
                                    } else {
                                        st.layer_unmapped.remove(&lid);
                                    }
                                    if first || remap {
                                        if let Some(&gidx) = st.layer_to_output_global.get(&lid) {
                                            st.next_serial += 1;
                                            let serial = st.next_serial;
                                            let spec = &specs[gidx as usize];
                                            let (rw, rh) = st
                                                .layer_requested_size
                                                .get(&lid)
                                                .copied()
                                                .unwrap_or((0, 0));
                                            let cw = if rw == 0 { spec.width as u32 } else { rw };
                                            let ch = if rh == 0 { spec.height as u32 } else { rh };
                                            let mut cb = Vec::new();
                                            Self::push_u32(&mut cb, serial);
                                            Self::push_u32(&mut cb, cw);
                                            Self::push_u32(&mut cb, ch);
                                            out_bytes.extend(Self::msg(lid, 0, &cb));
                                        }
                                    }
                                }
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

        pub(super) fn send_output_scale(&self, output_idx: u32, scale: i32) {
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

        pub(super) fn send_surface_preferred_buffer_scale(&self, surface_id: u32, scale: i32) {
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

        pub(super) fn send_preferred_scale(&self, surface_id: u32, scale_120: u32) {
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

        pub(super) fn send_pointer_move(&self, surface_id: u32, x: f64, y: f64, enter: bool) {
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

        pub(super) fn send_pointer_leave(&self, surface_id: u32) {
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

        pub(super) fn send_pointer_click(&self, button: u32) {
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

        pub(super) fn send_pointer_axis_value120(&self, value120: i32, absolute: f64) {
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

        pub(super) fn send_pointer_axis_continuous(&self, absolute: f64) {
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

        pub(super) fn send_touch_down(&self, surface_id: u32, id: i32, x: f64, y: f64) {
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

        pub(super) fn send_touch_motion(&self, id: i32, x: f64, y: f64) {
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

        pub(super) fn send_touch_up(&self, id: i32) {
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

        pub(super) fn send_touch_cancel(&self) {
            let tid = self.state.lock().unwrap().touch_id.unwrap();
            self.send_raw(Self::msg(tid, 4, &[]));
        }

        pub(super) fn send_focused_toplevel(&self, output_idx: u32) {
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

        pub(super) fn last_commit(&self, surface_id: u32) -> CommittedSurfaceState {
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

    pub(super) fn surface_wl_id(os: &OutputSurface) -> u32 {
        use smithay_client_toolkit::reexports::client::Proxy;
        os.layer.wl_surface().id().protocol_id()
    }

    pub(super) fn surface_has_nonzero_pixels(os: &mut OutputSurface) -> bool {
        let idx = os.last_slot;
        let Some(slot) = &os.slots[idx] else {
            return false;
        };
        slot.buffer
            .canvas(&mut os.pool)
            .is_some_and(|bytes| bytes.iter().any(|&b| b != 0))
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
}
