//! The modules. Each module lives in its own file under `src/modules/` and is dispatched here by
//! `Module` (one line per match arm).
//!
//! ## Fixed render data (`INFYNITAE_EDGE_RENDER_DATA`)
//!
//! When the environment variable `INFYNITAE_EDGE_RENDER_DATA` points to a file of `key=value`
//! lines (with `\n` and `\t` escape sequences decoded), [`run_sh`] returns the value for `key`
//! from that file instead of spawning `sh -c`. Supported keys:
//!
//! - `wifi`: output of `nmcli radio wifi` and `nmcli -t -f active,ssid dev wifi`
//!   (e.g. `enabled\nyes:HomeNet` or `disabled`).
//! - `bluetooth`: output of `bluetoothctl show` and `bluetoothctl devices Connected`
//!   (e.g. `Powered: yes\nDevice 4C:87:5D:11:22:33 Headphones` or `Powered: no`).
//! - `dnd`: `on` / `do-not-disturb` or `off` / `default`.
//! - `nightlight`: `on` or `off`.
//! - `keyboard`: `short\tfull\tcount` from `$XDG_RUNTIME_DIR/infynitae/keyboard-layout`
//!   (e.g. `US\tEnglish (US)\t2`).
//! - `osk`: `shown`, `hidden`, or `none` (when `squeekboard` is not installed).
//! - `power`: output of `powerprofilesctl get` (`balanced`, `performance`, `power-saver`, or `none`).
//! - `replay`: `on` or `off`.
//! - `volume`: output of `pactl get-sink-volume`, `get-sink-mute`, `get-default-sink` and
//!   `list sinks` (or e.g. `Volume: 60%\nMute: no\nDescription: Built-in Audio`).
//! - `brightness`: output of `brightnessctl -m` / `brightnessctl info`
//!   (e.g. `amdgpu_bl1,backlight,192,75%,255`, or `none` / empty when no device exists).
//! - `clipboard`: output of `cliphist list` (e.g. `108\tgit push origin main`).
//! - `notifications`: output of `makoctl list -j` / `makoctl history -j` (or `makoctl list` /
//!   `makoctl history` plain text, or `id\tapp\ttime\tsummary\tbody` lines).

pub mod apps;
pub mod ask;
pub mod calendar;
pub mod child;
pub mod clipboard;
pub mod clock;
pub mod edit;
pub mod favorites;
pub mod flow;
pub mod media;
pub mod memory;
pub mod notifications;
pub mod search;
pub mod session;
pub mod switches;
pub mod system;
pub mod tray;
pub mod volume;
pub mod windows;

#[cfg(test)]
pub use calendar::{arrows as calendar_arrows, shifted_month};

#[cfg(test)]
use crate::config::Config;
use crate::config::{Item, Module, Panel};
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::theme::Theme;
use cairo::Context;
use chrono::{Local, NaiveDateTime};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// A rectangle `(x, y, w, h)`.
pub type Rect = (f64, f64, f64, f64);

pub fn is_left_button(button: u32) -> bool {
    button == 1 || button == 0x110
}

pub fn is_right_button(button: u32) -> bool {
    button == 3 || button == 0x111
}

fn unescape_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses `key=value` lines (from `INFYNITAE_EDGE_RENDER_DATA`) into a map.
pub fn parse_render_data(text: &str) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim().to_string();
        let val = unescape_value(v.trim());
        map.entry(key)
            .and_modify(|existing| {
                existing.push('\n');
                existing.push_str(&val);
            })
            .or_insert(val);
    }
    map
}

/// Shared command helper: runs `sh -c cmd` and returns its stdout, or reads `key` from the file
/// named by `INFYNITAE_EDGE_RENDER_DATA` when set.
pub fn run_sh(key: &str, cmd: &str) -> String {
    if let Some(path) = std::env::var_os("INFYNITAE_EDGE_RENDER_DATA") {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let map = parse_render_data(&text);
        return map.get(key).cloned().unwrap_or_default();
    }
    if key == "tray" {
        return tray::fetch_dbus_string();
    }
    if key == "media" {
        return media::fetch_dbus_string();
    }
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Runs `cmds` in a background thread via [`run_sh`] and sends `(key, output)` pairs back to the
/// calloop event loop channel `tx`. At most one fetch runs at a time (`busy`).
pub fn spawn_fetch(
    tx: Sender<Vec<(String, String)>>,
    busy: Arc<AtomicBool>,
    cmds: Vec<(&'static str, &'static str)>,
) {
    if cmds.is_empty()
        || busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return;
    }
    std::thread::spawn(move || {
        let mut results = Vec::with_capacity(cmds.len());
        for (k, c) in cmds {
            results.push((k.to_string(), run_sh(k, c)));
        }
        busy.store(false, Ordering::SeqCst);
        let _ = tx.send(results);
    });
}

#[derive(Default)]
struct CoalesceSlot {
    running: bool,
    pending: Option<String>,
}

static COALESCE_MAP: std::sync::Mutex<
    Option<std::collections::HashMap<&'static str, CoalesceSlot>>,
> = std::sync::Mutex::new(None);

pub fn spawn_coalesced(key: &'static str, cmd: String) {
    spawn_coalesced_with(key, cmd, |c| {
        if let Ok(mut child) = Command::new("sh")
            .arg("-c")
            .arg(c)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            let _ = child.wait();
        }
    });
}

pub fn spawn_coalesced_with<F>(key: &'static str, cmd: String, run_fn: F)
where
    F: Fn(&str) + Send + Sync + 'static,
{
    let run_fn = std::sync::Arc::new(run_fn);
    let mut guard = COALESCE_MAP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let slot = guard
        .get_or_insert_with(Default::default)
        .entry(key)
        .or_default();
    if slot.running {
        slot.pending = Some(cmd);
        return;
    }
    slot.running = true;
    let mut cur_cmd = cmd;
    let runner = run_fn.clone();
    std::thread::spawn(move || loop {
        runner(&cur_cmd);
        let mut guard = COALESCE_MAP
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(slot) = guard.as_mut().and_then(|m| m.get_mut(key)) else {
            break;
        };
        if let Some(next) = slot.pending.take() {
            cur_cmd = next;
        } else {
            slot.running = false;
            break;
        }
    });
}

fn current_time() -> NaiveDateTime {
    std::env::var("INFYNITAE_EDGE_RENDER_TIME")
        .ok()
        .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M").ok())
        .unwrap_or_else(|| Local::now().naive_local())
}

/// A keyboard-selectable target inside an open panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavTarget {
    Favorite(usize),
    App(usize),
    Window(usize),
    Module(Module),
}

/// What modules need from the world to draw (fixed in tests and `--render`).
pub struct World {
    pub now: NaiveDateTime,
    pub cal_month: i32,
    pub switches: switches::SwitchesData,
    pub volume: volume::VolumeData,
    pub system: system::SystemData,
    pub memory: memory::MemoryData,
    pub clipboard: clipboard::ClipboardData,
    pub notifications: notifications::NotificationsData,
    pub tray: tray::TrayData,
    pub media: media::MediaData,
    pub search: String,
    pub apps: apps::AppsData,
    pub windows: windows::WindowsData,
    pub ask: ask::AskState,
    pub calendar_events: Vec<calendar::CalendarEvent>,
    pub child: child::ChildState,
    pub selected: usize,
    pub active_target: Option<NavTarget>,
    pub panel_scroll: f64,
    pub close_requested: bool,
    /// a card's list to load on a helper thread (the live edge; taken by the event loop)
    pub child_pending: Option<child::ChildKind>,
    pub live: bool,
    pub panel_has_clipboard: bool,
    pub actions: Vec<String>,
    /// the panel is being edited: empty modules keep a placeholder card (height_in)
    pub edit_preview: bool,
}

impl Default for World {
    fn default() -> Self {
        Self::now()
    }
}

impl World {
    pub fn now() -> World {
        let mut w = World {
            now: current_time(),
            cal_month: 0,
            switches: switches::SwitchesData::default(),
            volume: volume::VolumeData::default(),
            system: system::SystemData::default(),
            memory: memory::MemoryData::default(),
            clipboard: clipboard::ClipboardData::default(),
            notifications: notifications::NotificationsData::default(),
            tray: tray::TrayData::default(),
            media: media::MediaData::default(),
            search: String::new(),
            apps: apps::AppsData::default(),
            windows: windows::WindowsData::default(),
            ask: ask::AskState::default(),
            calendar_events: Vec::new(),
            child: child::ChildState::default(),
            selected: 0,
            active_target: None,
            panel_scroll: 0.0,
            close_requested: false,
            child_pending: None,
            live: false,
            panel_has_clipboard: false,
            actions: Vec::new(),
            edit_preview: false,
        };
        if std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_some() {
            w.load_from_helper();
        }
        w
    }

    /// The clock to the current time; true when the shown minute changed (the widgets redraw on
    /// the minute, nothing more often).
    pub fn tick_clock(&mut self) -> bool {
        let n = current_time();
        let minute = |t: &NaiveDateTime| t.and_utc().timestamp().div_euclid(60);
        let changed = minute(&n) != minute(&self.now);
        self.now = n;
        changed
    }

    /// Updates the clock when a panel opens while keeping the last fetched module data.
    pub fn on_open(&mut self) {
        self.now = current_time();
        self.cal_month = 0;
        self.search.clear();
        self.selected = 0;
        self.active_target = None;
        self.panel_scroll = 0.0;
        self.close_requested = false;
        self.panel_has_clipboard = false;
        self.child = child::ChildState::default();
        self.apps.scroll = 0;
        self.windows.scroll = 0;
        self.apps.update_filter(&self.search);
        self.windows.update_filter(&self.search);
        if std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_some() {
            self.load_from_helper();
        }
    }

    /// Sets the search query, resets selection and list scroll positions, and re-filters `apps`
    /// and `windows`.
    pub fn set_search(&mut self, query: String) {
        self.search = query;
        self.selected = 0;
        self.active_target = None;
        self.panel_scroll = 0.0;
        self.apps.scroll = 0;
        self.windows.scroll = 0;
        self.apps.update_filter(&self.search);
        self.windows.update_filter(&self.search);
    }

    pub fn selected_favorite_slot(&self) -> Option<usize> {
        match self.active_target {
            Some(NavTarget::Favorite(s)) => Some(s),
            _ => None,
        }
    }

    pub fn selected_app_row(&self) -> Option<usize> {
        match self.active_target {
            Some(NavTarget::App(r)) => Some(r),
            Some(_) => None,
            None => (!self.apps.filtered.is_empty())
                .then(|| self.selected.min(self.apps.filtered.len() - 1)),
        }
    }

    pub fn selected_window_row(&self) -> Option<usize> {
        match self.active_target {
            Some(NavTarget::Window(r)) => Some(r),
            Some(_) => None,
            None => (!self.windows.filtered.is_empty())
                .then(|| self.selected.min(self.windows.filtered.len() - 1)),
        }
    }

    /// Returns the ordered list of keyboard-selectable targets in `panel` along with each item's
    /// vertical `(y_top, y_bottom)` span in unclipped panel coordinates.
    pub fn nav_items(&self, panel: &Panel, rects: &[Rect]) -> Vec<(NavTarget, f64, f64)> {
        let mut out = Vec::new();
        for (it, &(_, ry, _, rh)) in panel.items.iter().zip(rects) {
            if rh <= 0.0 {
                continue;
            }
            match it.module {
                Module::Favorites if self.search.is_empty() => {
                    for slot in 0..self.apps.fav_indices.len() {
                        let row = slot / favorites::COLS;
                        let top = ry
                            + PAD_Y
                            + favorites::TITLE_H
                            + row as f64 * (favorites::CELL_H + favorites::ROW_GAP);
                        out.push((
                            NavTarget::Favorite(slot),
                            (top - PAD_Y).max(ry),
                            (top + favorites::CELL_H + PAD_Y).min(ry + rh),
                        ));
                    }
                }
                Module::Apps => {
                    for row in 0..self.apps.filtered.len() {
                        out.push((NavTarget::App(row), ry, ry + rh));
                    }
                }
                Module::Windows => {
                    for row in 0..self.windows.filtered.len() {
                        out.push((NavTarget::Window(row), ry, ry + rh));
                    }
                }
                m if m.is_switch() || m == Module::Volume || m == Module::Clock => {
                    out.push((NavTarget::Module(m), ry, ry + rh));
                }
                _ => {}
            }
        }
        out
    }

    /// Keeps `selected`, module list scrolls (`apps.scroll`, `windows.scroll`), and `panel_scroll`
    /// synchronized so the selected item is always in view inside its card and inside the panel.
    pub fn ensure_selection_visible(
        &mut self,
        panel: &Panel,
        rects: &[Rect],
        content_h: f64,
        max_ph: f64,
    ) {
        let nav = self.nav_items(panel, rects);
        if nav.is_empty() {
            self.selected = 0;
            self.active_target = None;
            return;
        }
        if self.selected >= nav.len() {
            self.selected = nav.len() - 1;
        }
        let (target, y_top, y_bot) = nav[self.selected];
        self.active_target = Some(target);

        match target {
            NavTarget::App(row) => {
                let card_h = panel
                    .items
                    .iter()
                    .zip(rects)
                    .find(|(it, _)| it.module == Module::Apps)
                    .map(|(_, r)| r.3)
                    .unwrap_or(0.0);
                let vis = apps::visible_rows_for_height(card_h).max(1);
                if row < self.apps.scroll {
                    self.apps.scroll = row;
                } else if row >= self.apps.scroll + vis {
                    self.apps.scroll = row + 1 - vis;
                }
            }
            NavTarget::Window(row) => {
                let card_h = panel
                    .items
                    .iter()
                    .zip(rects)
                    .find(|(it, _)| it.module == Module::Windows)
                    .map(|(_, r)| r.3)
                    .unwrap_or(0.0);
                let vis = windows::visible_rows_for_height(card_h).max(1);
                if row < self.windows.scroll {
                    self.windows.scroll = row;
                } else if row >= self.windows.scroll + vis {
                    self.windows.scroll = row + 1 - vis;
                }
            }
            NavTarget::Favorite(_) => {}
            NavTarget::Module(_) => {}
        }

        let ph = content_h.min(max_ph);
        let max_scroll = (content_h - ph).max(0.0);
        if y_top < self.panel_scroll {
            self.panel_scroll = y_top;
        } else if y_bot > self.panel_scroll + ph {
            self.panel_scroll = (y_bot - ph).max(0.0);
        }
        self.panel_scroll = self.panel_scroll.clamp(0.0, max_scroll);
    }

    /// Loads all module keys synchronously through [`run_sh`] (used when
    /// `INFYNITAE_EDGE_RENDER_DATA` is set).
    pub fn load_from_helper(&mut self) {
        for &(k, c) in switches::CMDS
            .iter()
            .chain(volume::CMDS.iter())
            .chain(system::CMDS.iter())
            .chain(memory::CMDS.iter())
            .chain(clipboard::CMDS.iter())
            .chain(notifications::CMDS.iter())
            .chain(tray::CMDS.iter())
            .chain(media::CMDS.iter())
        {
            let out = run_sh(k, c);
            self.apply(k, &out);
        }
        if let Some(path) = std::env::var_os("INFYNITAE_EDGE_RENDER_DATA") {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            for (k, v) in parse_render_data(&text) {
                self.apply(&k, &v);
            }
        }
        if let Ok(sub) = std::env::var("INFYNITAE_EDGE_RENDER_SUB") {
            let kind = match sub.as_str() {
                "wifi" => child::ChildKind::Wifi,
                "bt" => child::ChildKind::Bluetooth,
                "audio" | "sound" => child::ChildKind::Audio,
                "power" => child::ChildKind::Power,
                "replay" => child::ChildKind::Replay,
                "cal" => child::ChildKind::Calendar,
                _ => child::ChildKind::Wifi,
            };
            self.child.kind = Some(kind);
            match kind {
                child::ChildKind::Wifi => {
                    self.child.items = vec![
                        child::ChildItem {
                            id: "HomeNet".into(),
                            label: "HomeNet".into(),
                            meta: "90%".into(),
                            active: true,
                        },
                        child::ChildItem {
                            id: "Neighbours 5G".into(),
                            label: "Neighbours 5G".into(),
                            meta: "62%".into(),
                            active: false,
                        },
                        child::ChildItem {
                            id: "Cafe guest".into(),
                            label: "Cafe guest".into(),
                            meta: "40%".into(),
                            active: false,
                        },
                    ];
                }
                child::ChildKind::Bluetooth => {
                    self.child.items = vec![
                        child::ChildItem {
                            id: "4C:87:5D:11:22:33".into(),
                            label: "Headphones".into(),
                            meta: "Connected".into(),
                            active: true,
                        },
                        child::ChildItem {
                            id: "-".into(),
                            label: "Find new devices".into(),
                            meta: "".into(),
                            active: false,
                        },
                    ];
                }
                child::ChildKind::Audio => {
                    self.child.items = vec![child::ChildItem {
                        id: "default_sink".into(),
                        label: "Built-in Audio".into(),
                        meta: "Output".into(),
                        active: true,
                    }];
                }
                child::ChildKind::Power => {
                    self.child.items = child::power_profiles_list(&self.switches.power_profile);
                }
                child::ChildKind::Calendar => {}
                child::ChildKind::Replay => {
                    self.child.items = child::replay_rows(self.switches.replay_on);
                }
            }
        }
    }

    /// Applies one `(key, output)` result from [`run_sh`] to the matching module state.
    pub fn apply(&mut self, key: &str, out: &str) -> bool {
        if key == "search" {
            self.set_search(out.trim().to_string());
            return true;
        }
        if key == "calendar" || key == "events" {
            self.calendar_events = calendar::parse_calendar_events(out);
            return true;
        }
        switches::apply(&mut self.switches, key, out)
            || volume::apply(&mut self.volume, key, out)
            || system::apply(&mut self.system, key, out)
            || memory::apply(&mut self.memory, key, out)
            || clipboard::apply(&mut self.clipboard, key, out)
            || notifications::apply(&mut self.notifications, key, out)
            || tray::apply(&mut self.tray, key, out)
            || media::apply(&mut self.media, key, out)
            || apps::apply(&mut self.apps, key, out, &self.search)
            || windows::apply(&mut self.windows, key, out, &self.search)
            || ask::apply(&mut self.ask, key, out)
    }

    /// Records `cmd` in `actions` and, when running live in the Wayland daemon, spawns `sh -c cmd`
    /// detached in the background.
    pub fn run(&mut self, cmd: String) {
        if self.live && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() {
            if let Ok(mut child) = Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                // reaped when it ends (each one was left a zombie until the edge exited)
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            return;
        }
        // the tests read what would have run (live, the list would only grow)
        self.actions.push(cmd);
    }

    /// Runs `cmd` coalesced by `key`: at most one command in flight at a time, the latest value
    /// winning when commands arrive while one is running (e.g. slider dragging).
    pub fn run_coalesced(&mut self, key: &'static str, cmd: String) {
        if self.live && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() {
            spawn_coalesced(key, cmd);
            return;
        }
        self.actions.push(cmd);
    }

    /// Sets a config line (`config::write_value`) in the live edge; the tests see `set KEY="v"`.
    pub fn set_config(&mut self, key: &str, value: &str) {
        if self.live && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() {
            let _ = crate::config::write_value(key, value);
        } else {
            self.actions.push(format!("set {key}=\"{value}\""));
        }
    }

    /// A panel's or the widgets' line before it was changed, added to panels.backup (live).
    pub fn backup_line(&mut self, key: &str, value: &str) {
        if self.live && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() {
            use std::io::Write;
            let path = crate::config::config_path().with_file_name("panels.backup");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let now = chrono::Local::now().format("%F %T");
                let _ = writeln!(f, "{now} {key}=\"{value}\"");
            }
        } else {
            self.actions.push(format!("backup {key}=\"{value}\""));
        }
    }

    /// Records what a click did that is not a command (a D-Bus call made directly: `media:next`,
    /// `tray:activate:ID`), for the tests; never given to a shell (a song title or a tray id in
    /// it could run as a command).
    pub fn note(&mut self, what: String) {
        if !self.live {
            self.actions.push(what);
        }
    }

    /// Collects the deduplicated `(key, cmd)` pairs needed to refresh `panel`'s modules.
    pub fn cmds_for_panel(panel: &Panel) -> Vec<(&'static str, &'static str)> {
        let mut out: Vec<(&'static str, &'static str)> = Vec::new();
        for it in &panel.items {
            for &(k, c) in refresh_cmds(it.module) {
                if !out.iter().any(|&(ek, _)| ek == k) {
                    out.push((k, c));
                }
            }
        }
        out
    }
}

/// The `(key, cmd)` commands a module needs fetched when its panel is open.
pub fn refresh_cmds(m: Module) -> &'static [(&'static str, &'static str)] {
    match m {
        m if m.is_switch() => switches::CMDS,
        Module::Volume => volume::CMDS,
        Module::System => system::CMDS,
        Module::Memory => memory::CMDS,
        Module::Clipboard => clipboard::CMDS,
        Module::Notifications => notifications::CMDS,
        Module::Tray => tray::CMDS,
        Module::Media => media::CMDS,
        _ => &[],
    }
}

/// The module's height at width `w` with default world/theme.
#[cfg(test)]
pub fn height(it: &Item, w: f64) -> f64 {
    let world = World::now();
    let th = Theme::from_config(&Config::from_text(""));
    height_in(it, &world, &th, w)
}

/// The module's height at width `w` given `world` and `th` (its card included).
pub fn height_in(it: &Item, world: &World, th: &Theme, w: f64) -> f64 {
    let h = shown_height(it, world, th, w);
    // while the panel is edited, a module with nothing to show (no tray apps, no
    // notifications, nothing playing) keeps a small card, to be moved or taken out
    if h <= 0.0 && world.edit_preview && !it.module.is_switch() {
        return PLACEHOLDER_H;
    }
    h
}

/// The height of an empty module's card while the panel is edited.
pub const PLACEHOLDER_H: f64 = 52.0;

/// Whether `it` shows only its placeholder (edited, nothing to show).
pub fn is_placeholder(it: &Item, world: &World, th: &Theme, w: f64) -> bool {
    world.edit_preview && !it.module.is_switch() && shown_height(it, world, th, w) <= 0.0
}

/// The module's own height: 0 when it has nothing to show.
fn shown_height(it: &Item, world: &World, th: &Theme, w: f64) -> f64 {
    let _ = w;
    match it.module {
        Module::Search => search::height(it, world),
        Module::Favorites => favorites::height(it, world),
        Module::Apps => apps::height(it, world),
        Module::Windows => windows::height(it, world),
        Module::Clock => clock::height(it),
        Module::Calendar => calendar::height(),
        Module::Volume => volume::height(world),
        Module::System => system::height(world),
        Module::Memory => memory::height(world),
        Module::Session => session::height(),
        Module::Clipboard => clipboard::height(world),
        Module::Notifications => notifications::height(world),
        Module::Tray => tray::height(world),
        Module::Media => media::height(it, world),
        m if m.is_switch() => switches::height(it, world, th),
        _ => 2.0 * PAD_Y + 40.0,
    }
}

pub fn toggle_child(world: &mut World, kind: child::ChildKind) -> bool {
    if world.child.open && world.child.kind == Some(kind) {
        world.child.open = false;
        world.child.kind = None;
        return true;
    }
    world.child.open = true;
    world.child.kind = Some(kind);
    // a card opens on the list as it is; a search is asked from inside it
    world.child.scan = false;
    match kind {
        child::ChildKind::Power => {
            world.child.items = child::power_profiles_list(&world.switches.power_profile);
        }
        child::ChildKind::Replay => {
            world.child.items = child::replay_rows(world.switches.replay_on);
        }
        child::ChildKind::Calendar => world.child.items.clear(),
        // the lists from nmcli, bluetoothctl and pactl: in the live edge on a helper thread
        // (a Wi-Fi list can take seconds: the panel froze meanwhile); the card says Loading
        // until they come (World::child_pending, the event loop's child channel)
        _ if world.live && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() => {
            world.child.items = vec![child::none_found("Loading")];
            world.child_pending = Some(kind);
        }
        _ => world.child.items = load_child_list(kind, world.child.bt_scan, world.live),
    }
    true
}

/// The rows of a Wi-Fi, Bluetooth or outputs card (blocking: nmcli, bluetoothctl, pactl).
/// Nothing found: live, a row saying so (Bluetooth: Find new devices); offline (--render, the
/// tests), sample rows.
pub fn load_child_list(kind: child::ChildKind, bt_scan: bool, live: bool) -> Vec<child::ChildItem> {
    let row = |id: &str, label: &str, meta: &str, active: bool| child::ChildItem {
        id: id.into(),
        label: label.into(),
        meta: meta.into(),
        active,
    };
    match kind {
        child::ChildKind::Wifi => {
            let mut out = run_sh(
                "wifi_list",
                if bt_scan {
                    // a search asked from the card: nmcli scans and waits for what it finds
                    "nmcli -t -f IN-USE,SIGNAL,SECURITY,SSID dev wifi list --rescan yes 2>/dev/null"
                } else {
                    "nmcli -t -f IN-USE,SIGNAL,SECURITY,SSID dev wifi list --rescan no 2>/dev/null"
                },
            );
            if out.is_empty() && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_some() {
                out = run_sh("wifi", "");
            }
            // the cable and the Wi-Fi switch first, then the networks (Wi-Fi on only)
            let dev = run_sh(
                "net_dev",
                "nmcli -t -f TYPE,STATE device 2>/dev/null; nmcli radio wifi 2>/dev/null",
            );
            let mut head = child::network_rows(&dev);
            let wifi_on = head.iter().any(|r| r.id == child::WIFI_OFF);
            let items = if wifi_on {
                child::parse_wifi_list(&out)
            } else {
                Vec::new()
            };
            if !items.is_empty() || (live && !head.is_empty()) {
                head.extend(items);
                if wifi_on {
                    head.push(child::ChildItem {
                        id: child::WIFI_SCAN.into(),
                        label: "Search for networks".into(),
                        meta: String::new(),
                        active: false,
                    });
                }
                if wifi_on && head.iter().all(|r| r.id.starts_with('#')) {
                    head.push(child::none_found("No networks found"));
                }
                head
            } else if live {
                vec![child::none_found("No networks found")]
            } else {
                vec![
                    row("HomeNet", "HomeNet", "90%", true),
                    row("Neighbours 5G", "Neighbours 5G", "62%", false),
                    row("Cafe guest", "Cafe guest", "40%", false),
                ]
            }
        }
        child::ChildKind::Bluetooth => {
            let paired = run_sh("bt_paired", "bluetoothctl devices Paired 2>/dev/null");
            let scanned = run_sh("bt_scanned", "bluetoothctl devices 2>/dev/null");
            let items = child::parse_bluetooth_list(&paired, &scanned, bt_scan);
            if !items.is_empty() {
                items
            } else if live {
                vec![row("-", "Find new devices", "", false)]
            } else {
                vec![
                    row("4C:87:5D:11:22:33", "Headphones", "Connected", true),
                    row("-", "Find new devices", "", false),
                ]
            }
        }
        child::ChildKind::Audio => {
            let def = run_sh("audio_def", "pactl get-default-sink 2>/dev/null");
            let sinks = run_sh("audio_sinks", "pactl list sinks 2>/dev/null");
            let mut items = child::parse_audio_list(&sinks, &def);
            if !items.is_empty() {
                // then each app playing sound, with its own volume
                // the microphone (the default input) and who uses it, then each app playing
                let src = run_sh(
                    "audio_mic",
                    "LC_ALL=C pactl get-default-source 2>/dev/null; LC_ALL=C pactl get-source-volume @DEFAULT_SOURCE@ 2>/dev/null; LC_ALL=C pactl get-source-mute @DEFAULT_SOURCE@ 2>/dev/null",
                );
                let recording = run_sh(
                    "audio_mic_users",
                    "LC_ALL=C pactl list source-outputs 2>/dev/null",
                );
                items.extend(child::mic_rows(&src, &recording));
                let inputs = run_sh("audio_apps", "LC_ALL=C pactl list sink-inputs 2>/dev/null");
                items.extend(child::parse_sink_inputs(&inputs));
                items
            } else if live {
                vec![child::none_found("No outputs found")]
            } else {
                vec![row("default_sink", "Built-in Audio", "Output", true)]
            }
        }
        child::ChildKind::Power | child::ChildKind::Calendar | child::ChildKind::Replay => {
            Vec::new()
        }
    }
}

/// Dispatches a pointer click at `pos` inside `it`'s `size` card.
pub fn click(it: &Item, world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if is_right_button(button) {
        match it.module {
            Module::Wifi => return toggle_child(world, child::ChildKind::Wifi),
            Module::Bluetooth => return toggle_child(world, child::ChildKind::Bluetooth),
            Module::Volume => return toggle_child(world, child::ChildKind::Audio),
            Module::Power => return toggle_child(world, child::ChildKind::Power),
            Module::Replay => return toggle_child(world, child::ChildKind::Replay),
            Module::Clock if clock::size_look(size.0, size.1) != "calendar" => {
                return toggle_child(world, child::ChildKind::Calendar);
            }
            _ => {}
        }
    }
    match it.module {
        Module::Search => search::click(world, size, pos, button),
        Module::Favorites => favorites::click(world, size, pos, button),
        Module::Apps => apps::click(world, size, pos, button),
        Module::Windows => windows::click(world, size, pos, button),
        Module::Calendar => calendar::click(world, size, pos),
        Module::Clock => {
            if clock::size_look(size.0, size.1) != "calendar" && is_left_button(button) {
                toggle_child(world, child::ChildKind::Calendar)
            } else {
                clock::click(it, world, size, pos)
            }
        }
        Module::Volume => volume::click(world, size, pos, button),
        Module::System => system::click(world, size, pos, button),
        Module::Memory => memory::click(world, size, pos, button),
        Module::Session => session::click(world, size, pos, button),
        Module::Clipboard => clipboard::click(world, size, pos, button),
        Module::Notifications => notifications::click(world, size, pos, button),
        Module::Tray => tray::click(world, size, pos, button),
        Module::Media => media::click(world, size, pos, button),
        m if m.is_switch() => switches::click(m, world, button),
        _ => false,
    }
}

/// Dispatches a pointer drag at `pos` inside `it`'s `size` card.
pub fn drag(it: &Item, world: &mut World, size: (f64, f64), pos: (f64, f64)) -> bool {
    match it.module {
        Module::Volume => volume::drag(world, size, pos),
        Module::Media => media::drag(world, size, pos),
        _ => false,
    }
}

/// Dispatches a pointer scroll (`dy`) at `pos` inside `it`'s `size` card.
pub fn scroll(it: &Item, world: &mut World, size: (f64, f64), pos: (f64, f64), dy: f64) -> bool {
    match it.module {
        Module::Apps => apps::scroll(world, size, pos, dy),
        Module::Windows => windows::scroll(world, size, pos, dy),
        Module::Volume => volume::scroll(world, size, pos, dy),
        Module::Clipboard => clipboard::scroll(world, size, pos, dy),
        Module::Media => media::scroll(world, size, pos, dy),
        _ => false,
    }
}

/// Dispatches a key press to `it`.
pub fn key(it: &Item, world: &mut World, keysym: u32, ch: Option<char>) -> bool {
    let _ = (it, world, keysym, ch);
    false
}

/// INFYNITAE_EDGE_BOXES=FILE: the panel's, each module's, the edit bar's and the child card's boxes
/// in screen coordinates after every drawing, for the VM's real-input tests (vmt/pcanvas.py reads
/// the same lines from the C edge).
/// INFYNITAE_EDGE_BOXES=FILE: "closed" when no panel is open (the boxes were left as they were
/// last drawn, so a closed panel read as open).
pub fn dump_closed() {
    if let Some(path) = std::env::var_os("INFYNITAE_EDGE_BOXES").filter(|p| !p.is_empty()) {
        let _ = std::fs::write(path, "closed\n");
    }
}

pub fn dump_boxes(
    panel: &crate::config::Panel,
    rects: &[Rect],
    (ox, oy, pw, ph): Rect,
    bar: Option<Rect>,
    child: Option<Rect>,
) {
    let Some(path) = std::env::var_os("INFYNITAE_EDGE_BOXES").filter(|p| !p.is_empty()) else {
        return;
    };
    let mut out = format!(
        "panel {} {ox:.0} {oy:.0} {pw:.0} {ph:.0} {}\n",
        panel.name,
        if bar.is_some() { "editing" } else { "locked" }
    );
    for (it, r) in panel.items.iter().zip(rects) {
        if r.3 > 0.0 {
            out += &format!(
                "module {} {:.0} {:.0} {:.0} {:.0}\n",
                it.module.word(),
                ox + r.0,
                oy + r.1,
                r.2,
                r.3
            );
        }
    }
    if let Some((x, y, w, h)) = bar {
        out += &format!("bar {x:.0} {y:.0} {w:.0} {h:.0}\n");
    }
    if let Some((x, y, w, h)) = child {
        out += &format!("child {x:.0} {y:.0} {w:.0} {h:.0}\n");
    }
    let _ = std::fs::write(path, out);
}

/// INFYNITAE_EDGE_BOXES=FILE: the desktop's widgets in FILE.widgets: `widgets W H
/// editing|locked`, each widget's `module NAME x y w h`, the edit bar's `bar x y w h`.
pub fn dump_widget_boxes(
    panel: &crate::config::Panel,
    rects: &[Rect],
    screen: (f64, f64),
    bar: Option<Rect>,
) {
    let Some(mut path) = std::env::var_os("INFYNITAE_EDGE_BOXES").filter(|p| !p.is_empty()) else {
        return;
    };
    path.push(".widgets");
    let mut out = format!(
        "widgets {:.0} {:.0} {}\n",
        screen.0,
        screen.1,
        if bar.is_some() { "editing" } else { "locked" }
    );
    for (it, r) in panel.items.iter().zip(rects) {
        if r.2 > 0.0 && r.3 > 0.0 {
            out += &format!(
                "module {} {:.0} {:.0} {:.0} {:.0}\n",
                it.module.word(),
                r.0,
                r.1,
                r.2,
                r.3
            );
        }
    }
    if let Some((x, y, w, h)) = bar {
        out += &format!("bar {x:.0} {y:.0} {w:.0} {h:.0}\n");
    }
    let _ = std::fs::write(path, out);
}

/// `s` as one shell word in single quotes: text from outside (a Wi-Fi name, a device's name, a
/// sink) put in a command is never read by the shell as code.
pub fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A module drawn into its rectangle `(x, y, w, h)`, clipped to it: a module never draws over its
/// neighbours, whatever size it was given.
pub fn draw(cr: &Context, th: &Theme, it: &Item, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    cr.save().ok();
    cr.rectangle(x - 1.0, y - 1.0, w + 2.0, h + 2.0);
    cr.clip();
    draw_unclipped(cr, th, it, world, rect);
    cr.restore().ok();
}

fn draw_unclipped(cr: &Context, th: &Theme, it: &Item, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    match it.module {
        Module::Search => search::draw(cr, th, it, world, rect),
        Module::Favorites => favorites::draw(cr, th, world, rect),
        Module::Apps => apps::draw(cr, th, world, rect),
        Module::Windows => windows::draw(cr, th, world, rect),
        Module::Clock => clock::draw(cr, th, it, world, rect),
        Module::Calendar => calendar::draw(cr, th, world, rect),
        Module::Volume => volume::draw(cr, th, world, rect),
        Module::System => system::draw(cr, th, world, rect),
        Module::Memory => memory::draw(cr, th, world, rect),
        Module::Session => session::draw(cr, th, rect),
        Module::Clipboard => clipboard::draw(cr, th, world, rect),
        Module::Notifications => notifications::draw(cr, th, world, rect),
        Module::Tray => tray::draw(cr, th, world, rect),
        Module::Media => media::draw(cr, th, world, rect),
        m if m.is_switch() => switches::draw(cr, th, it, world, rect),
        m => {
            draw::card(cr, th, x, y, w, h);
            draw::title(cr, th, x + PAD_X, y + PAD_Y, m.word());
            draw::text_right(
                cr,
                x + w - PAD_X,
                y + PAD_Y + 2.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: "not in Rust yet",
                },
            );
        }
    }
}

pub fn draw_placeholder(cr: &Context, th: &Theme, it: &Item, rect: Rect) {
    let (x, y, w, h) = rect;
    let dash = [4.0, 3.0];
    draw::round_rect(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, th.corners.min(7.0));
    draw::set(cr, th.muted.alpha(0.35));
    cr.set_line_width(1.0);
    cr.set_dash(&dash, 0.0);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
    let name = Text {
        size: 13.0,
        bold: true,
        color: th.text.alpha(0.75),
        text: edit::module_display_name(it.module),
    };
    let (nw, _) = draw::measure(cr, &name);
    draw::text(cr, x + PAD_X, y + (h.min(52.0) - 15.0) / 2.0, &name);
    draw::text_max(
        cr,
        x + PAD_X + nw + 10.0,
        y + (h.min(52.0) - 13.0) / 2.0 + 1.0,
        (w - 2.0 * PAD_X - nw - 10.0).max(10.0),
        &Text {
            size: 12.0,
            bold: false,
            color: th.muted,
            text: "nothing to show now",
        },
    );
}

/// Draws all items of `panel` at `origin = (ox, oy)` using their laid-out `rects`, including the
/// shared card for consecutive switch runs.
pub fn draw_panel(
    cr: &Context,
    th: &Theme,
    panel: &Panel,
    rects: &[Rect],
    world: &World,
    origin: (f64, f64),
    pw: f64,
) {
    let (ox, oy_raw) = origin;
    let oy = oy_raw - world.panel_scroll;
    if !panel.placed {
        let mut y = 0.0;
        let mut i = 0;
        while i < panel.items.len() {
            if panel.items[i].module.is_switch() {
                let mut j = i + 1;
                while j < panel.items.len() && panel.items[j].module.is_switch() {
                    j += 1;
                }
                let (_, card_h) =
                    switches::layout_run(&panel.items[i..j], world, &th.switch_look, pw, y);
                if card_h > 0.0 {
                    switches::draw_card(cr, th, world, (ox, oy + y, pw, card_h));
                    y += card_h + draw::GAP;
                }
                i = j;
            } else {
                // the next card starts under this one; two halves share a row (their own rects
                // say where: adding both heights put the Quick settings card too low)
                if let Some(r) = rects.get(i).filter(|r| r.3 > 0.0) {
                    y = y.max(r.1 + r.3 + draw::GAP);
                }
                i += 1;
            }
        }
    }
    for (it, r) in panel.items.iter().zip(rects) {
        if r.2 > 0.0 && r.3 > 0.0 {
            if panel.placed && it.module.is_switch() {
                draw::card(cr, th, ox + r.0, oy + r.1, r.2, r.3);
                let cell = if r.3 < 56.0 {
                    (ox + r.0, oy + r.1, r.2, r.3)
                } else if th.switch_look == "rows" || th.switch_look.is_empty() {
                    (
                        ox + r.0 + 12.0,
                        oy + r.1 + ((r.3 - 34.0) / 2.0).max(4.0),
                        (r.2 - 24.0).max(20.0),
                        34.0,
                    )
                } else {
                    (ox + r.0, oy + r.1, r.2, r.3)
                };
                draw(cr, th, it, world, cell);
            } else {
                let is_editing = std::env::var_os("INFYNITAE_EDGE_RENDER_EDIT").is_some();
                if is_placeholder(it, world, th, r.2)
                    || (panel.placed
                        && is_editing
                        && (it.module == Module::Media && world.media.is_empty()))
                {
                    draw_placeholder(cr, th, it, (ox + r.0, oy + r.1, r.2, r.3));
                } else {
                    draw(cr, th, it, world, (ox + r.0, oy + r.1, r.2, r.3));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn calendar_month_arrows_change_shown_month() {
        let now = NaiveDate::from_ymd_opt(2026, 11, 9)
            .unwrap()
            .and_hms_opt(17, 23, 0)
            .unwrap();
        let mut world = World {
            now,
            cal_month: 0,
            ..World::now()
        };
        let cal = Item {
            module: Module::Calendar,
            look: None,
            rect: None,
            half: false,
        };
        let w = 380.0;
        let h = height(&cal, w);
        let (prev, next) = calendar_arrows(w);

        assert!(!click(&cal, &mut world, (w, h), (40.0, 100.0), 1));
        assert_eq!(world.cal_month, 0);

        assert!(click(
            &cal,
            &mut world,
            (w, h),
            (prev.0 + 4.0, prev.1 + 10.0),
            1
        ));
        assert_eq!(world.cal_month, -1);
        assert_eq!(
            shifted_month(now.date(), world.cal_month),
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
        );

        assert!(click(
            &cal,
            &mut world,
            (w, h),
            (next.0 + 4.0, next.1 + 10.0),
            1
        ));
        assert!(click(
            &cal,
            &mut world,
            (w, h),
            (next.0 + 4.0, next.1 + 10.0),
            1
        ));
        assert!(click(
            &cal,
            &mut world,
            (w, h),
            (next.0 + 4.0, next.1 + 10.0),
            1
        ));
        assert_eq!(world.cal_month, 2);
        assert_eq!(
            shifted_month(now.date(), world.cal_month),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }

    #[test]
    fn clock_with_calendar_look_forwards_arrow_clicks() {
        let now = NaiveDate::from_ymd_opt(2026, 1, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let mut world = World {
            now,
            cal_month: 0,
            ..World::now()
        };
        let clock_cal = Item {
            module: Module::Clock,
            look: Some(2),
            rect: None,
            half: false,
        };
        let w = 380.0;
        let h = height(&clock_cal, w);
        let (prev, _) = calendar_arrows(w);
        let cal_top = 2.0 * PAD_Y + 86.0 + draw::GAP;

        assert!(click(
            &clock_cal,
            &mut world,
            (w, h),
            (prev.0 + 5.0, cal_top + 12.0),
            1
        ));
        assert_eq!(world.cal_month, -1);
        assert_eq!(
            shifted_month(now.date(), world.cal_month),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
    }

    #[test]
    fn render_data_parses_escaped_lines_and_populates_world() {
        let data = "\
# sample render data
wifi=enabled\\nyes:HomeNet
bluetooth=Powered: yes\\nDevice 4C:87:5D:11:22:33 Headphones
dnd=on
nightlight=off
keyboard=US\\tEnglish (US)\\t2
osk=shown
power=performance
replay=on
volume=Volume: front-left: 39321 / 60% / -13.30 dB\\nMute: no\\nDescription: Studio Speakers
brightness=amdgpu_bl1,backlight,192,75%,255
clipboard=108\\tgit push origin main\\n107\\t[[ binary data 48 KiB png 1920x1080 ]]
notifications=42\\tFirefox\\t2m\\tBuild finished\\tAll 20 checks passed on main
";
        let map = parse_render_data(data);
        let mut world = World::now();
        for (k, v) in &map {
            world.apply(k, v);
        }
        assert!(world.switches.ready);
        assert!(world.switches.wifi_on);
        assert_eq!(world.switches.wifi_label, "HomeNet");
        assert!(world.switches.bt_on);
        assert_eq!(world.switches.bt_label, "Headphones");
        assert!(world.switches.dnd_on);
        assert!(!world.switches.night_on);
        assert_eq!(world.switches.kb_label, "English (US)");
        assert_eq!(world.switches.kb_count, 2);
        assert!(world.switches.osk_have);
        assert!(world.switches.osk_on);
        assert_eq!(world.switches.power_profile, "performance");
        assert!(world.switches.replay_on);
        assert!(world.volume.ready);
        assert_eq!(world.volume.pct, 60);
        assert!(!world.volume.muted);
        assert_eq!(world.volume.sink, "Studio Speakers");
        assert_eq!(world.volume.brightness, Some(75));
        assert!(world.clipboard.ready);
        assert_eq!(world.clipboard.items.len(), 2);
        assert_eq!(world.clipboard.items[0].preview, "git push origin main");
        assert_eq!(world.clipboard.items[1].preview, "Image 1920x1080");
        assert!(world.notifications.ready);
        assert_eq!(world.notifications.items.len(), 1);
        assert_eq!(world.notifications.items[0].id, 42);
        assert_eq!(world.notifications.items[0].app, "Firefox");
        assert_eq!(world.notifications.items[0].title, "Build finished");
    }

    #[test]
    fn spawn_coalesced_runs_at_most_one_in_flight_and_latest_wins() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(std::sync::Mutex::new(Vec::new()));

        let in_flight_clone = in_flight.clone();
        let max_in_flight_clone = max_in_flight.clone();
        let completed_clone = completed.clone();

        let runner = move |cmd: &str| {
            let cur = in_flight_clone.fetch_add(1, Ordering::SeqCst) + 1;
            let mut m = max_in_flight_clone.load(Ordering::SeqCst);
            while cur > m {
                match max_in_flight_clone.compare_exchange_weak(
                    m,
                    cur,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break,
                    Err(actual) => m = actual,
                }
            }
            std::thread::sleep(Duration::from_millis(30));
            completed_clone.lock().unwrap().push(cmd.to_string());
            in_flight_clone.fetch_sub(1, Ordering::SeqCst);
        };

        // Fire initial command 1
        spawn_coalesced_with("test_volume_coalesce", "val_1".into(), runner.clone());

        // Immediately fire values 2..=10 while 1 is in flight
        for i in 2..=10 {
            spawn_coalesced_with("test_volume_coalesce", format!("val_{i}"), runner.clone());
        }

        // Wait for all in-flight work to settle
        std::thread::sleep(Duration::from_millis(150));

        assert_eq!(max_in_flight.load(Ordering::SeqCst), 1, "at most 1 in flight");
        let done = completed.lock().unwrap().clone();
        assert_eq!(
            done,
            vec!["val_1".to_string(), "val_10".to_string()],
            "first value runs, values 2-9 are coalesced, latest value (10) wins"
        );
    }
}
