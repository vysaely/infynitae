//! Wayland `xdg_toplevel` client for `infynitae-files --portal` using `smithay-client-toolkit`
//! 0.20 and `calloop`: xdg-foreign parent attachment (`zxdg_importer_v2`), live `~/.cache/infynitae/look`
//! reload via `inotify`, background directory and preview workers, clipboard copy/paste, fractional
//! scale + viewporter, and cursor shapes.

use crate::draw;
use crate::fs_list::{DirLoaded, DirWorker};
use crate::look::{look_path, Look, LookWatcher};
use crate::portal::mode::{Focus, Key, KeyMods, OpenMenu, PickerEffect, PickerState};
use crate::portal::protocol::{PortalRequest, PortalResponse};
use crate::preview::{PreviewLoaded, PreviewWorker};
use crate::ui::{self, CursorKind, HitAction, LayoutHits, DEFAULT_H, DEFAULT_W, MIN_H, MIN_W};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::data_device_manager::data_device::{DataDevice, DataDeviceHandler};
use smithay_client_toolkit::data_device_manager::data_offer::DataOfferHandler;
use smithay_client_toolkit::data_device_manager::data_source::{
    CopyPasteSource, DataSourceHandler,
};
use smithay_client_toolkit::data_device_manager::{DataDeviceManagerState, WritePipe};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::generic::Generic;
use smithay_client_toolkit::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{
    PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::xdg::window::{
    Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_data_device, delegate_keyboard, delegate_output,
    delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_xdg_shell,
    delegate_xdg_window, registry_handlers,
};
use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{
    wl_data_device, wl_data_source, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols::xdg::foreign::zv2::client::{
    zxdg_imported_v2::{self, ZxdgImportedV2},
    zxdg_importer_v2::ZxdgImporterV2,
};

const DOUBLE_CLICK_MS: u64 = 400;
const DOUBLE_CLICK_PX: f64 = 6.0;
const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;

enum WorkerMsg {
    Dir(DirLoaded),
    Preview(PreviewLoaded),
    Paste(String),
}

struct LastClick {
    row: usize,
    at: Instant,
    pos: (f64, f64),
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    _compositor: CompositorState,
    shm: Shm,
    _xdg_shell: XdgShell,
    data_device_mgr: Option<DataDeviceManagerState>,
    cursor_shape_mgr: Option<WpCursorShapeManagerV1>,
    viewport: Option<WpViewport>,
    _fractional_scale: Option<WpFractionalScaleV1>,
    _importer: Option<ZxdgImporterV2>,
    imported_parent: Option<ZxdgImportedV2>,

    qh: QueueHandle<App>,
    window: Window,
    pool: SlotPool,
    width: u32,
    height: u32,
    scale_120: u32,
    configured: bool,
    dirty: bool,

    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_device: Option<WpCursorShapeDeviceV1>,
    data_device: Option<DataDevice>,
    copy_source: Option<CopyPasteSource>,
    clipboard_text: String,
    last_serial: u32,
    pointer_enter_serial: u32,
    last_cursor: Option<CursorKind>,

    modifiers: Modifiers,
    hover_pos: Option<(f64, f64)>,
    last_click: Option<LastClick>,
    hits: LayoutHits,

    look: Look,
    state: PickerState,
    dir_worker: DirWorker,
    dir_gen: u64,
    preview_worker: PreviewWorker,
    preview_gen: u64,
    worker_tx: Sender<WorkerMsg>,

    finished: Option<PortalResponse>,
}

pub fn run_portal_picker(req: PortalRequest) -> Result<PortalResponse, String> {
    let conn =
        Connection::connect_to_env().map_err(|e| format!("Wayland connection failed: {e}"))?;
    let (globals, event_queue) =
        registry_queue_init::<App>(&conn).map_err(|e| format!("Wayland registry failed: {e}"))?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor missing: {e}"))?;
    let xdg_shell =
        XdgShell::bind(&globals, &qh).map_err(|e| format!("xdg_wm_base missing: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm missing: {e}"))?;
    let data_device_mgr = DataDeviceManagerState::bind(&globals, &qh).ok();
    let viewporter: Option<WpViewporter> = globals.bind(&qh, 1..=1, ()).ok();
    let fractional_mgr: Option<WpFractionalScaleManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    let cursor_shape_mgr: Option<WpCursorShapeManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    let importer: Option<ZxdgImporterV2> = globals.bind(&qh, 1..=1, ()).ok();

    let (state, initial_effects) = PickerState::from_system(req);
    let look = Look::load();

    let surface = compositor.create_surface(&qh);
    let viewport = viewporter
        .as_ref()
        .map(|vp| vp.get_viewport(&surface, &qh, ()));
    let fractional_scale = fractional_mgr
        .as_ref()
        .map(|fm| fm.get_fractional_scale(&surface, &qh, ()));

    let window = xdg_shell.create_window(surface, WindowDecorations::None, &qh);
    window.set_title(&state.title);
    window.set_app_id("infynitae-files");
    window.set_min_size(Some((MIN_W, MIN_H)));

    let mut imported_parent = None;
    if let Some(handle) = state.parent_window.strip_prefix("wayland:") {
        if !handle.is_empty() {
            if let Some(ref imp) = importer {
                let imported = imp.import_toplevel(handle.to_string(), &qh, ());
                imported.set_parent_of(window.wl_surface());
                imported_parent = Some(imported);
            }
        }
    }
    window.commit();

    let pool = SlotPool::new((DEFAULT_W * DEFAULT_H * 4) as usize, &shm)
        .map_err(|e| format!("shm pool failed: {e}"))?;

    let mut event_loop: EventLoop<App> =
        EventLoop::try_new().map_err(|e| format!("event loop failed: {e}"))?;
    let loop_handle = event_loop.handle();

    let (worker_tx, worker_rx) = channel::channel::<WorkerMsg>();
    let dir_tx = worker_tx.clone();
    let dir_worker = DirWorker::spawn(Arc::clone(&state.mime_db), move |loaded| {
        let _ = dir_tx.send(WorkerMsg::Dir(loaded));
    });
    let prev_tx = worker_tx.clone();
    let preview_worker = PreviewWorker::spawn(Arc::clone(&state.mime_db), move |loaded| {
        let _ = prev_tx.send(WorkerMsg::Preview(loaded));
    });

    WaylandSource::new(conn.clone(), event_queue)
        .insert(loop_handle.clone())
        .map_err(|e| format!("WaylandSource failed: {e}"))?;

    loop_handle
        .insert_source(worker_rx, |ev, _, app| {
            if let channel::Event::Msg(msg) = ev {
                app.handle_worker_msg(msg);
            }
        })
        .map_err(|e| format!("worker channel source failed: {e}"))?;

    // Watch ~/.cache/infynitae/look so theme changes apply live without reopening the dialog.
    if let Some(watcher) = look_path().and_then(LookWatcher::watch) {
        let _ = loop_handle.insert_source(
            Generic::new(watcher, Interest::READ, Mode::Level),
            |_, watcher, app| {
                if let Some(updated) = watcher.poll_changed() {
                    if updated != app.look {
                        app.look = updated;
                        app.dirty = true;
                    }
                }
                Ok(PostAction::Continue)
            },
        );
    }

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        _compositor: compositor,
        shm,
        _xdg_shell: xdg_shell,
        data_device_mgr,
        cursor_shape_mgr,
        viewport,
        _fractional_scale: fractional_scale,
        _importer: importer,
        imported_parent,
        qh,
        window,
        pool,
        width: DEFAULT_W,
        height: DEFAULT_H,
        scale_120: 120,
        configured: false,
        dirty: true,
        seat: None,
        keyboard: None,
        pointer: None,
        cursor_device: None,
        data_device: None,
        copy_source: None,
        clipboard_text: String::new(),
        last_serial: 0,
        pointer_enter_serial: 0,
        last_cursor: None,
        modifiers: Modifiers::default(),
        hover_pos: None,
        last_click: None,
        hits: LayoutHits::default(),
        look,
        state,
        dir_worker,
        dir_gen: 0,
        preview_worker,
        preview_gen: 0,
        worker_tx,
        finished: None,
    };

    app.apply_effects(initial_effects);

    while app.finished.is_none() {
        if app.configured && app.dirty {
            app.draw();
        }
        event_loop
            .dispatch(None, &mut app)
            .map_err(|e| format!("event loop dispatch error: {e}"))?;
    }

    Ok(app.finished.unwrap_or_else(PortalResponse::cancelled))
}

impl App {
    fn handle_worker_msg(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::Dir(loaded) => {
                if loaded.generation == self.dir_gen {
                    let fx = self.state.apply_dir_loaded(loaded.dir, loaded.result);
                    self.apply_effects(fx);
                    self.dirty = true;
                }
            }
            WorkerMsg::Preview(loaded) => {
                if loaded.generation == self.preview_gen {
                    self.state.apply_preview_loaded(loaded.content);
                    self.dirty = true;
                }
            }
            WorkerMsg::Paste(text) => {
                let fx = self.state.handle_paste(&text);
                self.apply_effects(fx);
                self.dirty = true;
            }
        }
    }

    fn apply_effects(&mut self, effects: Vec<PickerEffect>) {
        let scale = (self.scale_120 as f64 / 120.0).max(1.0);
        let prev_w = (200.0 * scale).round() as u32;
        let prev_h = (132.0 * scale).round() as u32;

        for eff in effects {
            match eff {
                PickerEffect::LoadDir(dir) => {
                    self.dir_gen = self.dir_worker.request(dir);
                    self.dirty = true;
                }
                PickerEffect::LoadPreview(path) => {
                    self.preview_gen = self.preview_worker.request(path, prev_w, prev_h);
                    self.dirty = true;
                }
                PickerEffect::ClearPreview => {
                    self.preview_worker.cancel();
                    self.preview_gen = self.preview_gen.wrapping_add(1);
                    self.dirty = true;
                }
                PickerEffect::SetClipboard(text) => {
                    self.set_clipboard(text);
                }
                PickerEffect::Finish(resp) => {
                    self.dir_worker.cancel();
                    self.preview_worker.cancel();
                    self.finished = Some(resp);
                    return;
                }
            }
        }
        self.ensure_cursor_visible();
    }

    fn ensure_cursor_visible(&mut self) {
        let (Some(cur), Some(list_r)) = (self.state.cursor, self.hits.list_rect) else {
            return;
        };
        let row_h = if self.hits.row_height > 0.0 {
            self.hits.row_height
        } else {
            34.0
        };
        let top = 4.0 + cur as f64 * row_h;
        let bot = top + row_h;
        if top < self.state.list_scroll {
            self.state.list_scroll = top;
        } else if bot > self.state.list_scroll + list_r.h {
            self.state.list_scroll = (bot - list_r.h).max(0.0);
        }
        self.state.list_scroll = self.state.list_scroll.clamp(0.0, self.hits.list_max_scroll);
    }

    fn set_clipboard(&mut self, text: String) {
        self.clipboard_text = text;
        let (Some(mgr), Some(dd)) = (&self.data_device_mgr, &self.data_device) else {
            return;
        };
        let source =
            mgr.create_copy_paste_source(&self.qh, ["text/plain;charset=utf-8", "text/plain"]);
        source.set_selection(dd, self.last_serial);
        self.copy_source = Some(source);
    }

    fn request_paste(&self) {
        let Some(dd) = &self.data_device else {
            return;
        };
        let Some(offer) = dd.data().selection_offer() else {
            return;
        };
        let mime = offer.with_mime_types(|mimes| {
            if mimes.iter().any(|m| m == "text/plain;charset=utf-8") {
                Some("text/plain;charset=utf-8".to_string())
            } else if mimes.iter().any(|m| m == "text/plain") {
                Some("text/plain".to_string())
            } else {
                None
            }
        });
        let Some(mime) = mime else {
            return;
        };
        let Ok(mut pipe) = offer.receive(mime) else {
            return;
        };
        let tx = self.worker_tx.clone();
        thread::spawn(move || {
            let mut buf = Vec::new();
            let mut limited = (&mut pipe).take(MAX_CLIPBOARD_BYTES as u64);
            if limited.read_to_end(&mut buf).is_ok() {
                if let Ok(s) = String::from_utf8(buf) {
                    let _ = tx.send(WorkerMsg::Paste(s));
                }
            }
        });
    }

    fn draw(&mut self) {
        let lw = self.width.max(1);
        let lh = self.height.max(1);
        let phys_w = ((lw as u64 * self.scale_120 as u64 + 60) / 120).max(1) as u32;
        let phys_h = ((lh as u64 * self.scale_120 as u64 + 60) / 120).max(1) as u32;
        let stride = (phys_w * 4) as i32;

        let Ok((buffer, canvas)) = self.pool.create_buffer(
            phys_w as i32,
            phys_h as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) else {
            return;
        };

        let sx = phys_w as f64 / lw as f64;
        let sy = phys_h as f64 / lh as f64;

        let hits = {
            let tmp = vec![0u8; (stride as usize) * (phys_h as usize)];
            let Ok(mut surf) = cairo::ImageSurface::create_for_data(
                tmp,
                cairo::Format::ARgb32,
                phys_w as i32,
                phys_h as i32,
                stride,
            ) else {
                return;
            };
            let hits = {
                let Ok(cr) = cairo::Context::new(&surf) else {
                    return;
                };
                cr.scale(sx, sy);
                ui::render(
                    &cr,
                    &self.look,
                    &self.state,
                    lw as f64,
                    lh as f64,
                    self.hover_pos,
                )
            };
            surf.flush();
            if let Ok(data) = surf.data() {
                let n = canvas.len().min(data.len());
                canvas[..n].copy_from_slice(&data[..n]);
            }
            hits
        };
        self.hits = hits;

        let wl_surf = self.window.wl_surface();
        if let Some(ref vp) = self.viewport {
            vp.set_destination(lw as i32, lh as i32);
        } else {
            wl_surf.set_buffer_scale(((self.scale_120 + 60) / 120).max(1) as i32);
        }
        wl_surf.damage_buffer(0, 0, phys_w as i32, phys_h as i32);
        if buffer.attach_to(wl_surf).is_ok() {
            wl_surf.commit();
        }
        self.dirty = false;
    }

    fn update_cursor(&mut self) {
        let kind = self
            .hover_pos
            .and_then(|(x, y)| self.hits.hit_test(x, y))
            .map_or(CursorKind::Default, |t| t.cursor);
        if self.last_cursor == Some(kind) {
            return;
        }
        self.last_cursor = Some(kind);
        if let Some(ref dev) = self.cursor_device {
            let shape = match kind {
                CursorKind::Default => Shape::Default,
                CursorKind::Pointer => Shape::Pointer,
                CursorKind::Text => Shape::Text,
            };
            dev.set_shape(self.pointer_enter_serial, shape);
        }
    }

    fn handle_click(&mut self, pos: (f64, f64), serial: u32) {
        self.last_serial = serial;
        let hit = self.hits.hit_test(pos.0, pos.1).cloned();
        let Some(hit) = hit else {
            if self.state.open_menu.is_some() {
                self.state.open_menu = None;
                self.dirty = true;
            }
            return;
        };

        let mods = KeyMods {
            ctrl: self.modifiers.ctrl,
            alt: self.modifiers.alt,
            shift: self.modifiers.shift,
        };

        let effects = match hit.action {
            HitAction::WindowDrag => {
                if let Some(ref seat) = self.seat {
                    self.window.move_(seat, serial);
                }
                Vec::new()
            }
            HitAction::CloseBtn | HitAction::CancelBtn => self.state.cancel(),
            HitAction::AcceptBtn => self.state.try_accept(),
            HitAction::BackBtn => self.state.go_back(),
            HitAction::ForwardBtn => self.state.go_forward(),
            HitAction::UpBtn => self.state.go_up(),
            HitAction::Breadcrumb(path) => self.state.navigate_to(path),
            HitAction::PathBarEdit => {
                self.state.start_path_edit();
                Vec::new()
            }
            HitAction::PathInput => {
                self.state.focus = Focus::PathEdit;
                self.place_text_caret(Focus::PathEdit, pos.0);
                Vec::new()
            }
            HitAction::SearchInput => {
                self.state.open_menu = None;
                self.state.focus = Focus::Search;
                self.place_text_caret(Focus::Search, pos.0);
                Vec::new()
            }
            HitAction::SearchClear => {
                self.state.search_input.clear();
                self.state.rebuild_visible();
                self.state.focus = Focus::FileList;
                Vec::new()
            }
            HitAction::TogglePreview => self.state.toggle_preview(),
            HitAction::ToggleHidden => self.state.toggle_hidden(),
            HitAction::NewFolderBtn => {
                if self.state.new_folder.is_some() {
                    self.state.new_folder = None;
                    self.state.focus = Focus::FileList;
                } else {
                    self.state.start_new_folder();
                }
                Vec::new()
            }
            HitAction::NewFolderInput => {
                self.state.focus = Focus::NewFolder;
                self.place_text_caret(Focus::NewFolder, pos.0);
                Vec::new()
            }
            HitAction::NewFolderCreate => self.state.confirm_new_folder(),
            HitAction::NewFolderCancel => {
                self.state.new_folder = None;
                self.state.error_msg = None;
                self.state.focus = Focus::FileList;
                Vec::new()
            }
            HitAction::Place(target) => self.state.open_place(target),
            HitAction::SortCol(col) => self.state.set_sort(col),
            HitAction::FileRow(vi) => {
                let now = Instant::now();
                let is_double = self.last_click.as_ref().is_some_and(|lc| {
                    lc.row == vi
                        && now.duration_since(lc.at) <= Duration::from_millis(DOUBLE_CLICK_MS)
                        && (lc.pos.0 - pos.0).abs() <= DOUBLE_CLICK_PX
                        && (lc.pos.1 - pos.1).abs() <= DOUBLE_CLICK_PX
                });
                if is_double && !mods.ctrl && !mods.shift {
                    self.last_click = None;
                    self.state.activate_row(vi)
                } else {
                    self.last_click = Some(LastClick {
                        row: vi,
                        at: now,
                        pos,
                    });
                    self.state.click_row(vi, mods)
                }
            }
            HitAction::OverwriteCancel => {
                self.state.dismiss_overwrite();
                Vec::new()
            }
            HitAction::OverwriteReplace => self.state.confirm_overwrite_replace(),
            HitAction::OverwriteRename => self.state.confirm_overwrite_rename_colliding(),
            HitAction::SaveNameInput => {
                self.state.open_menu = None;
                self.state.focus = Focus::SaveName;
                self.place_text_caret(Focus::SaveName, pos.0);
                Vec::new()
            }
            HitAction::ChoiceBool(ci) => {
                self.state.toggle_bool_choice(ci);
                Vec::new()
            }
            HitAction::ChoiceMenuBtn(ci) => {
                self.state.open_menu = match self.state.open_menu {
                    Some(OpenMenu::Choice(cur)) if cur == ci => None,
                    _ => Some(OpenMenu::Choice(ci)),
                };
                Vec::new()
            }
            HitAction::ChoiceMenuItem(ci, opt_id) => {
                self.state.set_choice_option(ci, opt_id);
                Vec::new()
            }
            HitAction::FilterBtn => {
                self.state.open_menu = match self.state.open_menu {
                    Some(OpenMenu::Filter) => None,
                    _ => Some(OpenMenu::Filter),
                };
                Vec::new()
            }
            HitAction::FilterItem(idx) => self.state.select_filter(idx),
        };

        self.apply_effects(effects);
        self.dirty = true;
    }

    fn place_text_caret(&mut self, target_focus: Focus, click_x: f64) {
        let Some(&(_, rect, size, mono)) = self
            .hits
            .text_boxes
            .iter()
            .find(|(f, _, _, _)| *f == target_focus)
        else {
            return;
        };
        let Ok(surf) = cairo::ImageSurface::create(cairo::Format::ARgb32, 4, 4) else {
            return;
        };
        let Ok(cr) = cairo::Context::new(&surf) else {
            return;
        };
        let font = if mono {
            self.look.mono_font.clone()
        } else {
            self.look.font.clone()
        };
        let ti = match target_focus {
            Focus::PathEdit => Some(&mut self.state.path_input),
            Focus::Search => Some(&mut self.state.search_input),
            Focus::SaveName => Some(&mut self.state.save_input),
            Focus::NewFolder => self.state.new_folder.as_mut(),
            _ => None,
        };
        if let Some(ti) = ti {
            let local_x = (click_x - rect.x).max(0.0);
            let idx = draw::with_font(&font, || {
                let caret_x = draw::text_index_x(&cr, size, ti.as_str(), ti.cursor());
                let scroll = (caret_x - (rect.w - 4.0)).max(0.0);
                draw::text_x_to_index(&cr, size, ti.as_str(), local_x + scroll)
            });
            ti.set_cursor(idx, self.modifiers.shift);
        }
    }

    fn handle_key_event(&mut self, event: KeyEvent, serial: u32) {
        self.last_serial = serial;
        let mods = KeyMods {
            ctrl: self.modifiers.ctrl,
            alt: self.modifiers.alt,
            shift: self.modifiers.shift,
        };

        // Ctrl+V in text inputs triggers async Wayland clipboard read.
        if mods.ctrl
            && !mods.alt
            && matches!(event.keysym, Keysym::v | Keysym::V)
            && matches!(
                self.state.focus,
                Focus::PathEdit | Focus::Search | Focus::NewFolder | Focus::SaveName
            )
        {
            self.request_paste();
            return;
        }

        let Some(key) = map_key_event(&event, mods) else {
            return;
        };
        let fx = self.state.handle_key(key, mods);
        self.apply_effects(fx);
        self.dirty = true;
    }
}

fn map_key_event(event: &KeyEvent, mods: KeyMods) -> Option<Key> {
    let ks = event.keysym;
    match ks {
        Keysym::Up => return Some(Key::Up),
        Keysym::Down => return Some(Key::Down),
        Keysym::Left => return Some(Key::Left),
        Keysym::Right => return Some(Key::Right),
        Keysym::Page_Up => return Some(Key::PageUp),
        Keysym::Page_Down => return Some(Key::PageDown),
        Keysym::Home => return Some(Key::Home),
        Keysym::End => return Some(Key::End),
        Keysym::Return | Keysym::KP_Enter => return Some(Key::Enter),
        Keysym::Escape => return Some(Key::Escape),
        Keysym::Tab | Keysym::ISO_Left_Tab => return Some(Key::Tab),
        Keysym::BackSpace => return Some(Key::Backspace),
        Keysym::Delete => return Some(Key::Delete),
        Keysym::space => return Some(Key::Space),
        _ => {}
    }

    // With Ctrl or Alt held, map ASCII keysyms directly since utf8 is a control byte or None.
    if mods.ctrl || mods.alt {
        let raw = ks.raw();
        if (0x20..=0x7e).contains(&raw) {
            if let Some(ch) = char::from_u32(raw) {
                return Some(Key::Char(ch));
            }
        }
    }

    if let Some(ref s) = event.utf8 {
        if let Some(ch) = s.chars().next() {
            if !ch.is_control() {
                return Some(Key::Char(ch));
            }
        }
    }
    None
}

/* ---- SCTK and Wayland protocol trait implementations ---- */

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if self._fractional_scale.is_none() {
            self.scale_120 = (new_factor.max(1) as u32) * 120;
            self.dirty = true;
        }
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
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
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
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for App {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        let fx = self.state.cancel();
        self.apply_effects(fx);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let new_w = configure
            .new_size
            .0
            .map(|n| n.get())
            .unwrap_or(self.width)
            .max(MIN_W);
        let new_h = configure
            .new_size
            .1
            .map(|n| n.get())
            .unwrap_or(self.height)
            .max(MIN_H);
        self.width = new_w;
        self.height = new_h;
        self.configured = true;
        self.dirty = true;
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if self.seat.is_none() {
            if let Some(ref ddm) = self.data_device_mgr {
                self.data_device = Some(ddm.get_data_device(qh, &seat));
            }
            self.seat = Some(seat);
        }
    }

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            if let Ok(kbd) = self.seat_state.get_keyboard(qh, &seat, None) {
                self.keyboard = Some(kbd);
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Ok(ptr) = self.seat_state.get_pointer(qh, &seat) {
                if let Some(ref mgr) = self.cursor_shape_mgr {
                    self.cursor_device = Some(mgr.get_pointer(&ptr, qh, ()));
                }
                self.pointer = Some(ptr);
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(kbd) = self.keyboard.take() {
                kbd.release();
            }
        }
        if capability == Capability::Pointer {
            self.cursor_device = None;
            if let Some(ptr) = self.pointer.take() {
                ptr.release();
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
        serial: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.last_serial = serial;
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
        serial: u32,
        event: KeyEvent,
    ) {
        self.handle_key_event(event, serial);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.handle_key_event(event, serial);
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
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            match ev.kind {
                PointerEventKind::Enter { serial } => {
                    self.pointer_enter_serial = serial;
                    self.hover_pos = Some(ev.position);
                    self.update_cursor();
                    self.dirty = true;
                }
                PointerEventKind::Leave { .. } => {
                    self.hover_pos = None;
                    self.last_cursor = None;
                    self.dirty = true;
                }
                PointerEventKind::Motion { .. } => {
                    self.hover_pos = Some(ev.position);
                    self.update_cursor();
                    self.dirty = true;
                }
                PointerEventKind::Press { button, serial, .. } => {
                    self.hover_pos = Some(ev.position);
                    if button == BTN_LEFT {
                        self.handle_click(ev.position, serial);
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let delta = if vertical.discrete != 0 {
                        vertical.discrete as f64 * 36.0
                    } else {
                        vertical.absolute * 2.0
                    };
                    if let Some(pos) = self.hover_pos {
                        if self
                            .hits
                            .places_rect
                            .is_some_and(|r| r.contains(pos.0, pos.1))
                        {
                            self.state.places_scroll = (self.state.places_scroll + delta)
                                .clamp(0.0, self.hits.places_max_scroll);
                            self.dirty = true;
                        } else if self
                            .hits
                            .list_rect
                            .is_some_and(|r| r.contains(pos.0, pos.1))
                        {
                            self.state.list_scroll = (self.state.list_scroll + delta)
                                .clamp(0.0, self.hits.list_max_scroll);
                            self.dirty = true;
                        }
                    }
                }
                PointerEventKind::Release { .. } => {}
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl DataDeviceHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
    ) {
    }
    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
    fn drop_performed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
}

impl DataOfferHandler for App {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        _: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
    }
    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        _: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
    }
}

impl DataSourceHandler for App {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _mime: String,
        mut fd: WritePipe,
    ) {
        let bytes = self.clipboard_text.as_bytes().to_vec();
        thread::spawn(move || {
            let _ = fd.write_all(&bytes);
        });
    }

    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
        self.copy_source = None;
    }

    fn dnd_dropped(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }

    fn dnd_finished(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }

    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for App {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if scale >= 120 && scale != state.scale_120 {
                state.scale_120 = scale;
                state.dirty = true;
            }
        }
    }
}

impl Dispatch<ZxdgImportedV2, ()> for App {
    fn event(
        state: &mut Self,
        _: &ZxdgImportedV2,
        event: zxdg_imported_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_imported_v2::Event::Destroyed = event {
            state.imported_parent = None;
        }
    }
}

delegate_noop!(App: ignore WpViewporter);
delegate_noop!(App: ignore WpViewport);
delegate_noop!(App: ignore WpFractionalScaleManagerV1);
delegate_noop!(App: ignore WpCursorShapeManagerV1);
delegate_noop!(App: ignore WpCursorShapeDeviceV1);
delegate_noop!(App: ignore ZxdgImporterV2);

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_xdg_shell!(App);
delegate_xdg_window!(App);
delegate_data_device!(App);
delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
