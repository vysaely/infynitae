//! The switch modules (`wifi`, `bluetooth`, `dnd`, `nightlight`, `keyboard`, `osk`, `power`,
//! `replay`): data parsing, visibility, click actions, and drawing in `SWITCH_LOOK` (`rows`,
//! `icons`, `tiles`).

use crate::config::{Item, Module};
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{is_left_button, Rect, World};
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::f64::consts::PI;

pub const ROW_H: f64 = 42.0;
pub const TILE_ROW_H: f64 = 78.0;
pub const TILE_CELL_H: f64 = 68.0;

pub const CMDS: &[(&str, &str)] = &[
    (
        "wifi",
        "nmcli radio wifi 2>/dev/null; nmcli -t -f active,ssid dev wifi 2>/dev/null",
    ),
    (
        "bluetooth",
        "bluetoothctl show 2>/dev/null; bluetoothctl devices Connected 2>/dev/null",
    ),
    (
        "dnd",
        "[ -e \"${XDG_RUNTIME_DIR:-/tmp}/infynitae/do-not-disturb\" ] && echo on || makoctl mode 2>/dev/null",
    ),
    (
        "nightlight",
        "[ -e \"${XDG_RUNTIME_DIR:-/tmp}/infynitae/night-light\" ] && echo on || echo off",
    ),
    (
        "keyboard",
        "cat \"${XDG_RUNTIME_DIR:-/tmp}/infynitae/keyboard-layout\" 2>/dev/null",
    ),
    (
        "osk",
        "([ -x /usr/bin/squeekboard ] || [ -x /usr/local/bin/squeekboard ]) && { [ -e \"${XDG_RUNTIME_DIR:-/tmp}/infynitae/osk-visible\" ] && echo shown || echo hidden; } || echo none",
    ),
    ("power", "powerprofilesctl get 2>/dev/null || echo none"),
    (
        "replay",
        "for f in \"${XDG_RUNTIME_DIR:-/tmp}/infynitae-recorder-supervisor.pid\" \"${XDG_RUNTIME_DIR:-/tmp}/infynitae-replay.pid\"; do [ -f \"$f\" ] && kill -0 \"$(cat \"$f\" 2>/dev/null)\" 2>/dev/null && { echo on; exit 0; }; done; echo off",
    ),
];

#[derive(Clone, Debug)]
pub struct SwitchesData {
    pub ready: bool,
    pub wifi_on: bool,
    pub wifi_label: String,
    pub bt_on: bool,
    pub bt_label: String,
    pub dnd_on: bool,
    pub night_on: bool,
    pub kb_label: String,
    pub kb_count: u32,
    pub osk_have: bool,
    pub osk_on: bool,
    pub power_profile: String,
    pub replay_on: bool,
}

impl Default for SwitchesData {
    fn default() -> Self {
        Self {
            ready: false,
            wifi_on: false,
            wifi_label: "Off".into(),
            bt_on: false,
            bt_label: "Off".into(),
            dnd_on: false,
            night_on: false,
            kb_label: String::new(),
            kb_count: 0,
            osk_have: false,
            osk_on: false,
            power_profile: "none".into(),
            replay_on: false,
        }
    }
}

pub fn apply(d: &mut SwitchesData, key: &str, out: &str) -> bool {
    match key {
        "wifi" => {
            let (on, lbl) = parse_wifi(out);
            d.wifi_on = on;
            d.wifi_label = lbl;
            d.ready = true;
            true
        }
        "bluetooth" => {
            let (on, lbl) = parse_bluetooth(out);
            d.bt_on = on;
            d.bt_label = lbl;
            d.ready = true;
            true
        }
        "dnd" => {
            d.dnd_on = parse_dnd(out);
            d.ready = true;
            true
        }
        "nightlight" => {
            d.night_on = parse_bool(out);
            d.ready = true;
            true
        }
        "keyboard" => {
            let (lbl, count) = parse_keyboard(out);
            d.kb_label = lbl;
            d.kb_count = count;
            d.ready = true;
            true
        }
        "osk" => {
            let (have, on) = parse_osk(out);
            d.osk_have = have;
            d.osk_on = on;
            d.ready = true;
            true
        }
        "power" => {
            d.power_profile = parse_power(out);
            d.ready = true;
            true
        }
        "replay" => {
            d.replay_on = parse_bool(out);
            d.ready = true;
            true
        }
        _ => false,
    }
}

/// Parses `nmcli radio wifi` and `nmcli -t -f active,ssid dev wifi` output into `(on, state_label)`.
pub fn parse_wifi(out: &str) -> (bool, String) {
    let mut radio_on: Option<bool> = None;
    let mut ssid: Option<String> = None;
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("enabled") || line.eq_ignore_ascii_case("on") {
            radio_on = Some(true);
        } else if line.eq_ignore_ascii_case("disabled") || line.eq_ignore_ascii_case("off") {
            radio_on = Some(false);
        } else if let Some(rest) = line.strip_prefix("yes:") {
            let cleaned: String = rest
                .replace("\\:", ":")
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            let trimmed = cleaned.trim();
            if !trimmed.is_empty() && ssid.is_none() {
                ssid = Some(trimmed.to_string());
            }
        }
    }
    let on = radio_on.unwrap_or_else(|| ssid.is_some());
    if on {
        (true, ssid.unwrap_or_else(|| "On".to_string()))
    } else {
        (false, "Off".to_string())
    }
}

/// Parses `bluetoothctl show` and `bluetoothctl devices Connected` output into `(on, state_label)`.
pub fn parse_bluetooth(out: &str) -> (bool, String) {
    let mut powered = false;
    let mut dev: Option<String> = None;
    for raw in out.lines() {
        let line = raw.trim();
        if line.eq_ignore_ascii_case("Powered: yes") || line.eq_ignore_ascii_case("on") {
            powered = true;
        } else if line.eq_ignore_ascii_case("Powered: no") || line.eq_ignore_ascii_case("off") {
            powered = false;
        } else if let Some(rest) = line.strip_prefix("Device ") {
            if let Some((_, name)) = rest.trim_start().split_once(' ') {
                let name = name.trim();
                if !name.is_empty() && dev.is_none() {
                    dev = Some(name.to_string());
                }
            }
        }
    }
    if powered {
        (true, dev.unwrap_or_else(|| "On".to_string()))
    } else {
        (false, "Off".to_string())
    }
}

/// Parses `powerprofilesctl get` output into a profile identifier (`"balanced"`, `"performance"`,
/// `"power-saver"`, or `"none"`).
pub fn parse_power(out: &str) -> String {
    let s = out.lines().next().unwrap_or("").trim().to_ascii_lowercase();
    if s.is_empty() {
        "none".to_string()
    } else {
        s
    }
}

/// Parses `$XDG_RUNTIME_DIR/infynitae/keyboard-layout` (`short\tfull\tcount`) into `(full, count)`.
pub fn parse_keyboard(out: &str) -> (String, u32) {
    let Some(line) = out.lines().next() else {
        return (String::new(), 0);
    };
    let mut parts = line.split('\t');
    let _short = parts.next().unwrap_or("");
    let full = parts.next().unwrap_or("").trim();
    let count = parts
        .next()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);
    (full.to_string(), count)
}

/// Parses OSK state into `(have_squeekboard, is_shown)`.
pub fn parse_osk(out: &str) -> (bool, bool) {
    match out
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "shown" | "on" | "true" | "1" => (true, true),
        "hidden" | "off" | "false" | "0" => (true, false),
        _ => (false, false),
    }
}

/// Parses DND state from `do-not-disturb` file check or `makoctl mode`.
pub fn parse_dnd(out: &str) -> bool {
    out.lines().any(|l| {
        matches!(
            l.trim().to_ascii_lowercase().as_str(),
            "on" | "true" | "1" | "do-not-disturb"
        )
    })
}

/// Parses a boolean switch state (`nightlight`, `replay`).
pub fn parse_bool(out: &str) -> bool {
    let s = out.lines().next().unwrap_or("").trim().to_ascii_lowercase();
    matches!(s.as_str(), "on" | "true" | "1" | "yes" | "recording")
        || s.parse::<u32>().is_ok_and(|n| n > 0)
}

/// Whether switch `m` should be shown given the current `SwitchesData`.
/// Hides `osk` without squeekboard, `keyboard` with at most one layout, and `power` without
/// `powerprofilesctl`.
pub fn visible(m: Module, d: &SwitchesData) -> bool {
    match m {
        Module::Wifi | Module::Bluetooth | Module::Dnd | Module::Night | Module::Replay => true,
        Module::Keyboard => d.kb_count > 1 && !d.kb_label.is_empty(),
        Module::Osk => d.osk_have,
        Module::Power => {
            !d.power_profile.is_empty() && !d.power_profile.eq_ignore_ascii_case("none")
        }
        _ => false,
    }
}

pub fn power_label(profile: &str) -> &'static str {
    if profile.eq_ignore_ascii_case("performance") {
        "Performance"
    } else if profile.eq_ignore_ascii_case("power-saver") {
        "Power saver"
    } else {
        "Balanced"
    }
}

pub struct SwitchInfo {
    pub name: &'static str,
    pub short_name: &'static str,
    pub state: String,
    pub on: bool,
}

pub fn info(m: Module, d: &SwitchesData) -> SwitchInfo {
    match m {
        Module::Wifi => SwitchInfo {
            name: "Wi-Fi",
            short_name: "Wi-Fi",
            state: if d.wifi_on {
                if d.wifi_label.is_empty() {
                    "On".into()
                } else {
                    d.wifi_label.clone()
                }
            } else {
                "Off".into()
            },
            on: d.wifi_on,
        },
        Module::Bluetooth => SwitchInfo {
            name: "Bluetooth",
            short_name: "Bluetooth",
            state: if d.bt_on {
                if d.bt_label.is_empty() {
                    "On".into()
                } else {
                    d.bt_label.clone()
                }
            } else {
                "Off".into()
            },
            on: d.bt_on,
        },
        Module::Dnd => SwitchInfo {
            name: "Do not disturb",
            short_name: "Quiet",
            state: if d.dnd_on { "On".into() } else { "Off".into() },
            on: d.dnd_on,
        },
        Module::Night => SwitchInfo {
            name: "Night light",
            short_name: "Night",
            state: if d.night_on {
                "On".into()
            } else {
                "Off".into()
            },
            on: d.night_on,
        },
        Module::Keyboard => SwitchInfo {
            name: "Keyboard",
            short_name: "Keyboard",
            state: if d.kb_label.is_empty() {
                "Default".into()
            } else {
                d.kb_label.clone()
            },
            on: false,
        },
        Module::Osk => SwitchInfo {
            name: "Screen keyboard",
            short_name: "Screen keys",
            state: if d.osk_on {
                "Shown".into()
            } else {
                "Hidden".into()
            },
            on: d.osk_on,
        },
        Module::Power => SwitchInfo {
            name: "Power mode",
            short_name: "Power",
            state: power_label(&d.power_profile).into(),
            on: false,
        },
        Module::Replay => SwitchInfo {
            name: "Replay",
            short_name: "Replay",
            state: if d.replay_on {
                "Recording".into()
            } else {
                "Off".into()
            },
            on: d.replay_on,
        },
        _ => SwitchInfo {
            name: m.word(),
            short_name: m.word(),
            state: "Off".into(),
            on: false,
        },
    }
}

/// Applies a left-click toggle to `d` and returns the shell command string to run ( matching the
/// C edge's `execute_hitbox`).
pub fn toggle_action(m: Module, d: &mut SwitchesData) -> Option<String> {
    match m {
        Module::Wifi => {
            d.wifi_on = !d.wifi_on;
            d.wifi_label = if d.wifi_on { "On".into() } else { "Off".into() };
            Some(if d.wifi_on {
                "nmcli radio wifi on".into()
            } else {
                "nmcli radio wifi off".into()
            })
        }
        Module::Bluetooth => {
            d.bt_on = !d.bt_on;
            d.bt_label = if d.bt_on { "On".into() } else { "Off".into() };
            Some(if d.bt_on {
                "bluetoothctl power on".into()
            } else {
                "bluetoothctl power off".into()
            })
        }
        Module::Dnd => {
            let rt = std::env::var("XDG_RUNTIME_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/tmp".to_string());
            let dnd_path = format!("{rt}/infynitae/do-not-disturb");
            d.dnd_on = !d.dnd_on;
            if d.dnd_on {
                Some(format!(
                    "mkdir -p '{rt}/infynitae' && : > '{dnd_path}' && (makoctl mode -a do-not-disturb 2>/dev/null || swaync-client --dnd-on 2>/dev/null)"
                ))
            } else {
                Some(format!(
                    "rm -f '{dnd_path}' && (makoctl mode -r do-not-disturb 2>/dev/null || swaync-client --dnd-off 2>/dev/null)"
                ))
            }
        }
        Module::Night => {
            d.night_on = !d.night_on;
            Some(if d.night_on {
                "infynitae msg night-light on".into()
            } else {
                "infynitae msg night-light off".into()
            })
        }
        Module::Keyboard => Some("infynitae msg keyboard-layout".into()),
        Module::Osk => {
            d.osk_on = !d.osk_on;
            Some(if d.osk_on {
                "infynitae msg osk on".into()
            } else {
                "infynitae msg osk off".into()
            })
        }
        Module::Power => {
            if d.power_profile.is_empty() || d.power_profile.eq_ignore_ascii_case("none") {
                return Some(
                    "notify-send -a Infynitae 'Power modes' 'They need power-profiles-daemon, which is not installed'".into(),
                );
            }
            let next = if d.power_profile.eq_ignore_ascii_case("balanced") {
                "performance"
            } else if d.power_profile.eq_ignore_ascii_case("performance") {
                "power-saver"
            } else {
                "balanced"
            };
            d.power_profile = next.into();
            Some(format!(
                "powerprofilesctl set {next} 2>/dev/null || notify-send -a Infynitae 'Power modes' 'This machine has no {next} mode'"
            ))
        }
        Module::Replay => {
            d.replay_on = !d.replay_on;
            let val = if d.replay_on { "true" } else { "false" };
            Some(format!(
                "sh \"$HOME/infynitae/live/settings.sh\" set ENABLE_REPLAY_BUFFER {val} >/dev/null 2>&1; sh \"$HOME/infynitae/live/infynitae-reload\" recorder >/dev/null 2>&1"
            ))
        }
        _ => None,
    }
}

pub fn click(m: Module, world: &mut World, button: u32) -> bool {
    if !is_left_button(button) {
        // Right click or Shift+Enter opens its list (stage 4).
        return false;
    }
    if let Some(cmd) = toggle_action(m, &mut world.switches) {
        world.run(cmd);
        true
    } else {
        false
    }
}

pub fn card_height_for_count(n: usize, switch_look: &str) -> f64 {
    if n == 0 {
        return 0.0;
    }
    match switch_look {
        "icons" => 2.0 * PAD_Y + 28.0,
        "tiles" => {
            let rows = n.div_ceil(4);
            2.0 * PAD_Y + (rows as f64) * TILE_ROW_H - 10.0
        }
        _ => {
            let rows = n.div_ceil(2);
            2.0 * PAD_Y + 28.0 + (rows as f64) * ROW_H - 6.0
        }
    }
}

pub fn height(it: &Item, world: &World, th: &Theme) -> f64 {
    if world.switches.ready && !visible(it.module, &world.switches) {
        return 0.0;
    }
    card_height_for_count(1, &th.switch_look)
}

/// Lays out a consecutive run of switch items `items` starting at `y0` in a panel of width `pw`.
/// Returns `(per_item_rects, shared_card_height)`.
pub fn layout_run(
    items: &[Item],
    world: &World,
    switch_look: &str,
    pw: f64,
    y0: f64,
) -> (Vec<Rect>, f64) {
    let mut rects = vec![(0.0, y0, 0.0, 0.0); items.len()];
    if items.is_empty() {
        return (rects, 0.0);
    }
    if !world.switches.ready {
        let est = items.len().clamp(1, 6);
        return (rects, card_height_for_count(est, switch_look));
    }
    let vis: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| visible(it.module, &world.switches))
        .map(|(i, _)| i)
        .collect();
    let n = vis.len();
    if n == 0 {
        return (rects, 0.0);
    }
    let card_h = card_height_for_count(n, switch_look);
    match switch_look {
        "icons" => {
            let slot_w = (pw - 2.0 * PAD_X) / (n as f64);
            for (idx, &k) in vis.iter().enumerate() {
                rects[k] = (PAD_X + (idx as f64) * slot_w, y0 + PAD_Y, slot_w, 28.0);
            }
        }
        "tiles" => {
            let col_w = (pw - 2.0 * PAD_X) / 4.0;
            for (idx, &k) in vis.iter().enumerate() {
                let col = idx % 4;
                let row = idx / 4;
                rects[k] = (
                    PAD_X + (col as f64) * col_w,
                    y0 + PAD_Y + (row as f64) * TILE_ROW_H,
                    col_w,
                    TILE_CELL_H,
                );
            }
        }
        _ => {
            let col_w = (pw - 2.0 * PAD_X - draw::GAP) / 2.0;
            for (idx, &k) in vis.iter().enumerate() {
                let col = idx % 2;
                let row = idx / 2;
                rects[k] = (
                    PAD_X + (col as f64) * (col_w + draw::GAP),
                    y0 + PAD_Y + 28.0 + (row as f64) * ROW_H,
                    col_w,
                    36.0,
                );
            }
        }
    }
    (rects, card_h)
}

/// Draws the shared card background (and `"Quick settings"` title or `"..."` placeholder) for a
/// consecutive run of switches.
pub fn draw_card(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    draw::card(cr, th, x, y, w, h);
    if !world.switches.ready {
        draw::title(cr, th, x + PAD_X, y + PAD_Y, "Quick settings");
        draw::text_right(
            cr,
            x + w - PAD_X,
            y + PAD_Y + 2.0,
            &Text {
                size: 13.0,
                bold: false,
                color: th.muted,
                text: "...",
            },
        );
    } else if th.switch_look != "icons" && th.switch_look != "tiles" {
        draw::title(cr, th, x + PAD_X, y + PAD_Y, "Quick settings");
    }
}

/// Draws a single switch item inside its cell `rect`.
pub fn draw(cr: &Context, th: &Theme, it: &Item, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    if !world.switches.ready {
        draw::title(cr, th, x + PAD_X, y + PAD_Y, it.module.word());
        draw::text_right(
            cr,
            x + w - PAD_X,
            y + PAD_Y + 2.0,
            &Text {
                size: 13.0,
                bold: false,
                color: th.muted,
                text: "...",
            },
        );
        return;
    }
    let sw = info(it.module, &world.switches);
    // a name and its state need 32 px; below that a placed switch is its icon
    let look = if (it.rect.is_some() && h < 32.0) || th.switch_look == "icons" {
        "icons"
    } else if th.switch_look == "tiles" {
        "tiles"
    } else {
        "rows"
    };
    match look {
        "icons" => {
            let cx = (x + w / 2.0).round();
            let iy = (y + (h - 22.0) / 2.0).max(y).round();
            let col = if sw.on { th.accent } else { th.text };
            draw_icon(cr, it.module, cx - 9.0, iy, col);
            if sw.on {
                cr.new_sub_path();
                cr.arc(cx, (iy + 21.0).min(y + h - 2.0), 2.0, 0.0, 2.0 * PI);
                draw::set(cr, th.accent);
                let _ = cr.fill();
            }
        }
        "tiles" => {
            let cx = (x + w / 2.0).round();
            let ty = y.round();
            let r = th.corners.min(25.0);
            draw::round_rect(cr, cx - 25.0, ty, 50.0, 50.0, r);
            if sw.on {
                draw::set(cr, th.accent);
                let _ = cr.fill();
                draw_icon(cr, it.module, cx - 9.0, ty + 16.0, th.on_accent);
            } else {
                draw::set(cr, th.track);
                let _ = cr.fill();
                draw_icon(cr, it.module, cx - 9.0, ty + 16.0, th.text);
            }
            draw::text_center(
                cr,
                cx,
                ty + 55.0,
                &Text {
                    size: 11.0,
                    bold: false,
                    color: if sw.on { th.text } else { th.muted },
                    text: sw.short_name,
                },
            );
        }
        _ => {
            let icon_col = if sw.on { th.accent } else { th.text };
            let state_col = if sw.on { th.accent } else { th.muted };
            draw_icon(cr, it.module, x.round(), (y + 7.0).round(), icon_col);
            let tx = x + 26.0;
            let max_w = (w - 26.0).max(16.0);
            draw::text_max(
                cr,
                tx,
                y,
                max_w,
                &Text {
                    size: 13.0,
                    bold: false,
                    color: th.text,
                    text: sw.name,
                },
            );
            draw::text_max(
                cr,
                tx,
                y + 18.0,
                max_w,
                &Text {
                    size: 11.0,
                    bold: false,
                    color: state_col,
                    text: &sw.state,
                },
            );
        }
    }
}

/// Draws an 18x18 stroked line icon at `(x, y)` for switch module `m`.
pub fn draw_icon(cr: &Context, m: Module, x: f64, y: f64, color: Rgba) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    let cx = x + 9.0;
    let cy = y + 9.0;
    match m {
        Module::Wifi => {
            for &(r, dy) in &[(8.0, 5.0), (5.2, 5.0), (2.5, 5.0)] {
                cr.new_sub_path();
                cr.arc(cx, cy + dy, r, -0.78 * PI, -0.22 * PI);
                let _ = cr.stroke();
            }
            cr.new_sub_path();
            cr.arc(cx, cy + 5.5, 1.2, 0.0, 2.0 * PI);
            let _ = cr.fill();
        }
        Module::Bluetooth => {
            cr.move_to(cx - 3.5, cy - 3.5);
            cr.line_to(cx + 3.5, cy + 3.5);
            cr.line_to(cx, cy + 7.0);
            cr.line_to(cx, cy - 7.0);
            cr.line_to(cx + 3.5, cy - 3.5);
            cr.line_to(cx - 3.5, cy + 3.5);
            let _ = cr.stroke();
        }
        Module::Dnd => {
            cr.new_sub_path();
            cr.arc(cx, cy, 7.0, 0.0, 2.0 * PI);
            let _ = cr.stroke();
            cr.move_to(cx - 3.5, cy);
            cr.line_to(cx + 3.5, cy);
            let _ = cr.stroke();
        }
        Module::Night => {
            cr.new_sub_path();
            cr.arc(cx, cy, 6.5, 0.25 * PI, 1.45 * PI);
            cr.arc_negative(cx + 3.0, cy - 2.0, 5.5, 1.35 * PI, 0.35 * PI);
            cr.close_path();
            let _ = cr.stroke();
        }
        Module::Keyboard | Module::Osk => {
            draw::round_rect(cr, x + 1.5, y + 3.5, 15.0, 11.0, 2.0);
            let _ = cr.stroke();
            for kx in [x + 4.5, x + 7.5, x + 10.5, x + 13.5] {
                cr.move_to(kx, y + 6.5);
                cr.line_to(kx + 0.5, y + 6.5);
                cr.move_to(kx, y + 9.0);
                cr.line_to(kx + 0.5, y + 9.0);
            }
            cr.move_to(x + 5.5, y + 12.0);
            cr.line_to(x + 12.5, y + 12.0);
            let _ = cr.stroke();
        }
        Module::Power => {
            cr.new_sub_path();
            cr.arc(cx, cy + 2.0, 6.8, -1.1 * PI, 0.1 * PI);
            let _ = cr.stroke();
            cr.move_to(cx, cy + 2.0);
            cr.line_to(cx + 3.8, cy - 2.2);
            let _ = cr.stroke();
        }
        Module::Replay => {
            cr.new_sub_path();
            cr.arc(cx, cy, 6.5, -0.35 * PI, 1.3 * PI);
            let _ = cr.stroke();
            cr.move_to(cx - 2.5, cy - 6.8);
            cr.line_to(cx - 5.5, cy - 4.2);
            cr.line_to(cx - 1.8, cy - 3.2);
            let _ = cr.stroke();
            cr.new_sub_path();
            cr.arc(cx, cy, 2.0, 0.0, 2.0 * PI);
            let _ = cr.fill();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_nmcli_outputs() {
        let connected = "enabled\nno:Neighbours 5G\nyes:HomeNet\nno:Cafe guest\n";
        assert_eq!(parse_wifi(connected), (true, "HomeNet".to_string()));

        let escaped = "enabled\nyes:Studio\\:5GHz\n";
        assert_eq!(parse_wifi(escaped), (true, "Studio:5GHz".to_string()));

        let disconnected = "enabled\nno:Neighbours 5G\n";
        assert_eq!(parse_wifi(disconnected), (true, "On".to_string()));

        let radio_off = "disabled\n";
        assert_eq!(parse_wifi(radio_off), (false, "Off".to_string()));
    }

    #[test]
    fn parses_real_bluetoothctl_outputs() {
        let show_and_dev = "\
Controller AC:36:1B:F9:5D:AF (public)
\tManufacturer: 0x005d (93)
\tVersion: 0x0c (12)
\tName: Aevyternae
\tAlias: Aevyternae
\tClass: 0x00000000 (0)
\tPowered: yes
\tPowerState: on
\tDiscoverable: no
Device 4C:87:5D:11:22:33 Sony WH-1000XM5
";
        assert_eq!(
            parse_bluetooth(show_and_dev),
            (true, "Sony WH-1000XM5".to_string())
        );

        let show_on_no_dev = "\
Controller AC:36:1B:F9:5D:AF (public)
\tName: Aevyternae
\tPowered: yes
";
        assert_eq!(parse_bluetooth(show_on_no_dev), (true, "On".to_string()));

        let show_off = "\
Controller AC:36:1B:F9:5D:AF (public)
\tName: Aevyternae
\tPowered: no
\tPowerState: off
";
        assert_eq!(parse_bluetooth(show_off), (false, "Off".to_string()));
    }

    #[test]
    fn parses_powerprofilesctl_and_desk_files() {
        assert_eq!(parse_power("balanced\n"), "balanced");
        assert_eq!(parse_power("performance\n"), "performance");
        assert_eq!(parse_power("power-saver\n"), "power-saver");
        assert_eq!(parse_power("none\n"), "none");
        assert_eq!(parse_power(""), "none");
        assert_eq!(power_label("balanced"), "Balanced");
        assert_eq!(power_label("performance"), "Performance");
        assert_eq!(power_label("power-saver"), "Power saver");

        assert_eq!(
            parse_keyboard("US\tEnglish (US)\t2\n"),
            ("English (US)".to_string(), 2)
        );
        assert_eq!(
            parse_keyboard("US\tEnglish (US)\t1\n"),
            ("English (US)".to_string(), 1)
        );
        assert_eq!(parse_osk("shown\n"), (true, true));
        assert_eq!(parse_osk("hidden\n"), (true, false));
        assert_eq!(parse_osk("none\n"), (false, false));
        assert!(parse_dnd("default\ndo-not-disturb\n"));
        assert!(!parse_dnd("default\n"));
    }

    #[test]
    fn switches_hide_when_unavailable() {
        let mut d = SwitchesData {
            ready: true,
            ..Default::default()
        };
        // Default: kb_count=0, osk_have=false, power_profile="none" -> all three hide
        assert!(visible(Module::Wifi, &d));
        assert!(visible(Module::Bluetooth, &d));
        assert!(visible(Module::Dnd, &d));
        assert!(visible(Module::Night, &d));
        assert!(visible(Module::Replay, &d));
        assert!(!visible(Module::Keyboard, &d));
        assert!(!visible(Module::Osk, &d));
        assert!(!visible(Module::Power, &d));

        // 1 keyboard layout still hides; 2 layouts shows
        d.kb_label = "English (US)".into();
        d.kb_count = 1;
        assert!(!visible(Module::Keyboard, &d));
        d.kb_count = 2;
        assert!(visible(Module::Keyboard, &d));

        // squeekboard present shows osk
        d.osk_have = true;
        assert!(visible(Module::Osk, &d));

        // powerprofilesctl reporting a profile shows power
        d.power_profile = "balanced".into();
        assert!(visible(Module::Power, &d));
    }

    #[test]
    fn switch_clicks_choose_expected_commands_without_running_them() {
        let mut d = SwitchesData {
            ready: true,
            wifi_on: true,
            wifi_label: "HomeNet".into(),
            bt_on: false,
            bt_label: "Off".into(),
            dnd_on: false,
            night_on: false,
            kb_label: "English (US)".into(),
            kb_count: 2,
            osk_have: true,
            osk_on: false,
            power_profile: "balanced".into(),
            replay_on: false,
        };

        assert_eq!(
            toggle_action(Module::Wifi, &mut d).as_deref(),
            Some("nmcli radio wifi off")
        );
        assert!(!d.wifi_on);
        assert_eq!(
            toggle_action(Module::Wifi, &mut d).as_deref(),
            Some("nmcli radio wifi on")
        );
        assert!(d.wifi_on);

        assert_eq!(
            toggle_action(Module::Bluetooth, &mut d).as_deref(),
            Some("bluetoothctl power on")
        );
        assert!(d.bt_on);
        assert_eq!(
            toggle_action(Module::Bluetooth, &mut d).as_deref(),
            Some("bluetoothctl power off")
        );
        assert!(!d.bt_on);

        let dnd_on_cmd = toggle_action(Module::Dnd, &mut d).unwrap();
        assert!(dnd_on_cmd.contains("makoctl mode -a do-not-disturb"));
        assert!(d.dnd_on);
        let dnd_off_cmd = toggle_action(Module::Dnd, &mut d).unwrap();
        assert!(dnd_off_cmd.contains("makoctl mode -r do-not-disturb"));
        assert!(!d.dnd_on);

        assert_eq!(
            toggle_action(Module::Night, &mut d).as_deref(),
            Some("infynitae msg night-light on")
        );
        assert_eq!(
            toggle_action(Module::Night, &mut d).as_deref(),
            Some("infynitae msg night-light off")
        );

        assert_eq!(
            toggle_action(Module::Keyboard, &mut d).as_deref(),
            Some("infynitae msg keyboard-layout")
        );

        assert_eq!(
            toggle_action(Module::Osk, &mut d).as_deref(),
            Some("infynitae msg osk on")
        );
        assert_eq!(
            toggle_action(Module::Osk, &mut d).as_deref(),
            Some("infynitae msg osk off")
        );

        let p1 = toggle_action(Module::Power, &mut d).unwrap();
        assert!(p1.starts_with("powerprofilesctl set performance"));
        assert_eq!(d.power_profile, "performance");
        let p2 = toggle_action(Module::Power, &mut d).unwrap();
        assert!(p2.starts_with("powerprofilesctl set power-saver"));
        assert_eq!(d.power_profile, "power-saver");
        let p3 = toggle_action(Module::Power, &mut d).unwrap();
        assert!(p3.starts_with("powerprofilesctl set balanced"));
        assert_eq!(d.power_profile, "balanced");

        let r_on = toggle_action(Module::Replay, &mut d).unwrap();
        assert!(r_on.contains("ENABLE_REPLAY_BUFFER true"));
        let r_off = toggle_action(Module::Replay, &mut d).unwrap();
        assert!(r_off.contains("ENABLE_REPLAY_BUFFER false"));
    }
}
