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

pub mod calendar;
pub mod clock;
pub mod memory;
pub mod switches;
pub mod system;
pub mod volume;

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

fn current_time() -> NaiveDateTime {
    std::env::var("INFYNITAE_EDGE_RENDER_TIME")
        .ok()
        .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M").ok())
        .unwrap_or_else(|| Local::now().naive_local())
}

/// What modules need from the world to draw (fixed in tests and `--render`).
pub struct World {
    pub now: NaiveDateTime,
    pub cal_month: i32,
    pub switches: switches::SwitchesData,
    pub volume: volume::VolumeData,
    pub system: system::SystemData,
    pub memory: memory::MemoryData,
    pub live: bool,
    pub actions: Vec<String>,
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
            live: false,
            actions: Vec::new(),
        };
        if std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_some() {
            w.load_from_helper();
        }
        w
    }

    /// Updates the clock when a panel opens while keeping the last fetched module data.
    pub fn on_open(&mut self) {
        self.now = current_time();
        self.cal_month = 0;
        if std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_some() {
            self.load_from_helper();
        }
    }

    /// Loads all module keys synchronously through [`run_sh`] (used when
    /// `INFYNITAE_EDGE_RENDER_DATA` is set).
    pub fn load_from_helper(&mut self) {
        for &(k, c) in switches::CMDS
            .iter()
            .chain(volume::CMDS.iter())
            .chain(system::CMDS.iter())
            .chain(memory::CMDS.iter())
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
    }

    /// Applies one `(key, output)` result from [`run_sh`] to the matching module state.
    pub fn apply(&mut self, key: &str, out: &str) -> bool {
        switches::apply(&mut self.switches, key, out)
            || volume::apply(&mut self.volume, key, out)
            || system::apply(&mut self.system, key, out)
            || memory::apply(&mut self.memory, key, out)
    }

    /// Records `cmd` in `actions` and, when running live in the Wayland daemon, spawns `sh -c cmd`
    /// detached in the background.
    pub fn run(&mut self, cmd: String) {
        if self.live && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() {
            let _ = Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        self.actions.push(cmd);
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
    let _ = w;
    match it.module {
        Module::Clock => clock::height(it),
        Module::Calendar => calendar::height(),
        Module::Volume => volume::height(world),
        Module::System => system::height(world),
        Module::Memory => memory::height(world),
        m if m.is_switch() => switches::height(it, world, th),
        _ => 2.0 * PAD_Y + 40.0,
    }
}

/// Dispatches a pointer click at `pos` inside `it`'s `size` card.
pub fn click(it: &Item, world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    match it.module {
        Module::Calendar => calendar::click(world, size.0, pos),
        Module::Clock => clock::click(it, world, size, pos),
        Module::Volume => volume::click(world, size, pos, button),
        Module::System => system::click(world, size, pos, button),
        Module::Memory => memory::click(world, size, pos, button),
        m if m.is_switch() => switches::click(m, world, button),
        _ => false,
    }
}

/// Dispatches a pointer drag at `pos` inside `it`'s `size` card.
pub fn drag(it: &Item, world: &mut World, size: (f64, f64), pos: (f64, f64)) -> bool {
    match it.module {
        Module::Volume => volume::drag(world, size, pos),
        _ => false,
    }
}

/// Dispatches a pointer scroll (`dy`) at `pos` inside `it`'s `size` card.
pub fn scroll(it: &Item, world: &mut World, size: (f64, f64), pos: (f64, f64), dy: f64) -> bool {
    match it.module {
        Module::Volume => volume::scroll(world, size, pos, dy),
        _ => false,
    }
}

/// Dispatches a key press to `it`.
pub fn key(it: &Item, world: &mut World, keysym: u32, ch: Option<char>) -> bool {
    let _ = (it, world, keysym, ch);
    false
}

/// A module drawn into its rectangle `(x, y, w, h)`.
pub fn draw(cr: &Context, th: &Theme, it: &Item, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    match it.module {
        Module::Clock => clock::draw(cr, th, it, world, rect),
        Module::Calendar => calendar::draw(cr, th, world, rect),
        Module::Volume => volume::draw(cr, th, world, rect),
        Module::System => system::draw(cr, th, world, rect),
        Module::Memory => memory::draw(cr, th, world, rect),
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
    let (ox, oy) = origin;
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
                let h = rects.get(i).map(|r| r.3).unwrap_or(0.0);
                if h > 0.0 {
                    y += h + draw::GAP;
                }
                i += 1;
            }
        }
    }
    for (it, r) in panel.items.iter().zip(rects) {
        if r.2 > 0.0 && r.3 > 0.0 {
            if panel.placed && it.module.is_switch() {
                draw::card(cr, th, ox + r.0, oy + r.1, r.2, r.3);
                let cell = if th.switch_look == "rows" || th.switch_look.is_empty() {
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
                draw(cr, th, it, world, (ox + r.0, oy + r.1, r.2, r.3));
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
    }
}
