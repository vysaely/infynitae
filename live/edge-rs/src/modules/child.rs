//! Child lists: cards drawn beside an open panel for switch and module submenus.
//!
//! Right click (or Shift+Enter) on a switch / module opens its list as a card beside the panel
//! on the side with room:
//! - Wi-Fi networks (connect)
//! - Bluetooth devices (connect / disconnect, "Find new devices")
//! - Sound outputs (set default)
//! - Power profiles (set active profile)
//! - Calendar (month view beside clock)
//!
//! Escape or clicking outside closes the child list first, leaving the panel open.

use crate::config::{Module, Panel};
use crate::draw::{self, Text, GAP, PAD_X, PAD_Y};
use crate::modules::{calendar, is_left_button, World};
use crate::theme::Theme;
use cairo::Context;

pub const CHILD_WIDTH: f64 = 300.0;
pub const ROW_HEIGHT: f64 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildKind {
    Wifi,
    Bluetooth,
    Audio,
    Power,
    Calendar,
}

impl ChildKind {
    pub fn title(self) -> &'static str {
        match self {
            ChildKind::Wifi => "Wi-Fi networks",
            ChildKind::Bluetooth => "Bluetooth devices",
            ChildKind::Audio => "Sound output",
            ChildKind::Power => "Power profiles",
            ChildKind::Calendar => "Calendar",
        }
    }

    pub fn parent_module(self) -> Module {
        match self {
            ChildKind::Wifi => Module::Wifi,
            ChildKind::Bluetooth => Module::Bluetooth,
            ChildKind::Audio => Module::Volume,
            ChildKind::Power => Module::Power,
            ChildKind::Calendar => Module::Clock,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildItem {
    pub id: String,
    pub label: String,
    pub meta: String,
    pub active: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChildState {
    pub open: bool,
    pub kind: Option<ChildKind>,
    pub items: Vec<ChildItem>,
    pub bt_scan: bool,
    pub rect: Option<(f64, f64, f64, f64)>,
    pub hover_row: Option<usize>,
}

/// Parses raw nmcli wifi list output into `ChildItem`s.
pub fn parse_wifi_list(output: &str) -> Vec<ChildItem> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // TSV format: active \t id \t label \t meta
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let active = parts[0] == "1" || parts[0] == "*";
            let id = parts[1].to_string();
            let label = parts[2].to_string();
            let meta = if parts.len() > 3 {
                parts[3].to_string()
            } else {
                String::new()
            };
            if !seen.contains(&label) {
                seen.insert(label.clone());
                items.push(ChildItem {
                    id,
                    label,
                    meta,
                    active,
                });
            }
            continue;
        }

        // Colon-separated nmcli output: IN-USE:SIGNAL:SECURITY:SSID
        let cols: Vec<&str> = line.split(':').collect();
        if cols.len() >= 4 {
            let active = cols[0].trim() == "*";
            let signal = cols[1].trim();
            let ssid = cols[3..].join(":");
            let ssid = ssid.trim();
            if !ssid.is_empty() && !seen.contains(ssid) {
                seen.insert(ssid.to_string());
                items.push(ChildItem {
                    id: ssid.to_string(),
                    label: ssid.to_string(),
                    meta: format!("{signal}%"),
                    active,
                });
            }
        }
    }
    items.truncate(6);
    items
}

/// Parses raw bluetoothctl output into `ChildItem`s.
pub fn parse_bluetooth_list(
    paired_out: &str,
    scan_devices_out: &str,
    scanning: bool,
) -> Vec<ChildItem> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Paired devices
    for line in paired_out.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Device ") {
            if let Some((mac, name)) = rest.split_once(' ') {
                let mac = mac.trim();
                let name = name.trim();
                if !mac.is_empty() && !name.is_empty() {
                    seen.insert(mac.to_string());
                    items.push(ChildItem {
                        id: mac.to_string(),
                        label: name.to_string(),
                        meta: "Paired".to_string(),
                        active: false,
                    });
                }
            }
        } else {
            // TSV format: active \t mac \t name \t status
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let active = parts[0] == "1";
                let mac = parts[1].to_string();
                let name = parts[2].to_string();
                let meta = if parts.len() > 3 {
                    parts[3].to_string()
                } else {
                    if active { "Connected" } else { "Paired" }.to_string()
                };
                if !seen.contains(&mac) {
                    seen.insert(mac.clone());
                    items.push(ChildItem {
                        id: mac,
                        label: name,
                        meta,
                        active,
                    });
                }
            }
        }
    }
    items.truncate(5);

    // 2. Scanned / new devices
    if scanning {
        for line in scan_devices_out.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("Device ") {
                if let Some((mac, name)) = rest.split_once(' ') {
                    let mac = mac.trim();
                    let name = name.trim();
                    if !mac.is_empty()
                        && !name.is_empty()
                        && !seen.contains(mac)
                        && name != mac.replace(':', "-")
                    {
                        seen.insert(mac.to_string());
                        items.push(ChildItem {
                            id: mac.to_string(),
                            label: name.to_string(),
                            meta: "Pair".to_string(),
                            active: false,
                        });
                        if items.len() >= 8 {
                            break;
                        }
                    }
                }
            }
        }
    } else {
        // "Find new devices" entry
        items.push(ChildItem {
            id: "-".to_string(),
            label: "Find new devices".to_string(),
            meta: String::new(),
            active: false,
        });
    }

    items
}

/// Parses raw pactl output into sound output sinks.
pub fn parse_audio_list(sinks_out: &str, default_sink: &str) -> Vec<ChildItem> {
    let mut items = Vec::new();
    let mut cur_name = String::new();
    let mut cur_desc = String::new();

    let def = default_sink.trim();

    for line in sinks_out.lines() {
        let trimmed = line.trim();

        // TSV format: active \t id \t label \t Output (a 0 or 1 at the line's start; pactl's own
        // property lines are indented with tabs too, and were read as outputs)
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 && matches!(parts[0], "0" | "1") {
            let active = parts[0] == "1";
            let id = parts[1].to_string();
            let label = parts[2].to_string();
            items.push(ChildItem {
                id,
                label,
                meta: "Output".to_string(),
                active,
            });
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Name: ") {
            if !cur_name.is_empty() && !cur_desc.is_empty() {
                let active = cur_name == def;
                items.push(ChildItem {
                    id: cur_name,
                    label: cur_desc.clone(),
                    meta: "Output".to_string(),
                    active,
                });
            }
            cur_name = rest.trim().to_string();
            cur_desc.clear();
        } else if let Some(rest) = trimmed.strip_prefix("Description: ") {
            cur_desc = rest.trim().to_string();
        }
    }

    if !cur_name.is_empty() && !cur_desc.is_empty() {
        let active = cur_name == def;
        items.push(ChildItem {
            id: cur_name,
            label: cur_desc,
            meta: "Output".to_string(),
            active,
        });
    }

    items.truncate(6);
    items
}

/// Returns the list of standard power profiles with the active one marked.
pub fn power_profiles_list(current_profile: &str) -> Vec<ChildItem> {
    let cur = current_profile.trim().to_ascii_lowercase();
    vec![
        ChildItem {
            id: "performance".to_string(),
            label: "Performance".to_string(),
            meta: String::new(),
            active: cur == "performance",
        },
        ChildItem {
            id: "balanced".to_string(),
            label: "Balanced".to_string(),
            meta: String::new(),
            active: cur == "balanced" || cur.is_empty() || cur == "none",
        },
        ChildItem {
            id: "power-saver".to_string(),
            label: "Power saver".to_string(),
            meta: String::new(),
            active: cur == "power-saver",
        },
    ]
}

/// Computes the command to execute for selecting `item` in `kind`.
/// A row that only says nothing was found (it does nothing when clicked). The sample rows
/// (HomeNet, Headphones) are for `--render` and the tests only, never the live edge.
pub fn none_found(label: &str) -> ChildItem {
    ChildItem {
        id: String::new(),
        label: label.into(),
        meta: String::new(),
        active: false,
    }
}

pub fn action_for_child_item(kind: ChildKind, item: &ChildItem) -> Option<String> {
    use crate::modules::sq;
    if item.id.is_empty() {
        return None;
    }
    let id = sq(&item.id);
    match kind {
        ChildKind::Wifi => Some(format!(
            "s=$(printf '%s' {id} | tr '\\036' '\\t'); \
                 nmcli connection up id \"$s\" >/dev/null 2>&1 || \
                 nmcli dev wifi connect \"$s\" >/dev/null 2>&1"
        )),
        ChildKind::Bluetooth => {
            if item.id == "-" {
                None // signals entering scan mode
            } else if item.meta == "Pair" {
                let msg = sq(&format!("Could not pair with {}", item.label));
                Some(format!(
                    "bluetoothctl pair {id} >/dev/null 2>&1 && \
                     bluetoothctl trust {id} >/dev/null 2>&1 && \
                     bluetoothctl connect {id} >/dev/null 2>&1 || \
                     notify-send -a Infynitae Bluetooth {msg}"
                ))
            } else if item.active {
                Some(format!("bluetoothctl disconnect {id} >/dev/null 2>&1"))
            } else {
                Some(format!("bluetoothctl connect {id} >/dev/null 2>&1"))
            }
        }
        ChildKind::Audio => Some(format!("pactl set-default-sink {id} >/dev/null 2>&1")),
        ChildKind::Power => {
            let msg = sq(&format!("This machine has no {} mode", item.id));
            Some(format!(
                "powerprofilesctl set {id} 2>/dev/null || \
                 notify-send -a Infynitae 'Power modes' {msg}"
            ))
        }
        ChildKind::Calendar => None,
    }
}

/// Measures the height needed for the child card.
pub fn child_height(kind: ChildKind, item_count: usize) -> f64 {
    if kind == ChildKind::Calendar {
        calendar::height()
    } else {
        let rows = if item_count == 0 { 1 } else { item_count };
        PAD_Y + 22.0 + GAP + rows as f64 * ROW_HEIGHT + PAD_Y
    }
}

/// Calculates the position `(cx, cy, cw, ch)` for the child card beside `panel`.
pub fn child_card_geometry(
    kind: ChildKind,
    panel_origin: (f64, f64),
    pw: f64,
    parent_y: f64,
    item_count: usize,
    screen: (f64, f64),
) -> (f64, f64, f64, f64) {
    let (ox, oy) = panel_origin;
    let (sw, sh) = screen;
    let cw = CHILD_WIDTH;
    let ch = child_height(kind, item_count);

    // Beside the panel on the side with room
    let left = ox + pw / 2.0 > sw / 2.0;
    let mut cx = if left { ox - cw - 8.0 } else { ox + pw + 8.0 };
    if cx < 8.0 {
        cx = 8.0;
    }
    if cx + cw > sw - 8.0 {
        cx = sw - 8.0 - cw;
    }

    let mut cy = oy + parent_y;
    if cy + ch > sh - 8.0 {
        cy = sh - 8.0 - ch;
    }
    if cy < 8.0 {
        cy = 8.0;
    }

    (cx, cy, cw, ch)
}

/// Finds the parent module rectangle in `panel` for child kind `kind`.
pub fn find_parent_rect(
    panel: &Panel,
    rects: &[(f64, f64, f64, f64)],
    kind: ChildKind,
) -> Option<(f64, f64, f64, f64)> {
    // where the module is laid out, placed by hand or stacked (a stacked panel has no rect of
    // its own: the card was then never drawn, while clicks went to its invisible rows)
    let m = kind.parent_module();
    panel
        .items
        .iter()
        .zip(rects)
        .find(|(it, r)| it.module == m && r.2 > 0.0 && r.3 > 0.0)
        .map(|(_, r)| *r)
}

/// Draws the child card (Wi-Fi, Bluetooth, Audio, Power, or Calendar).
pub fn draw_child_card(
    cr: &Context,
    th: &Theme,
    world: &World,
    state: &ChildState,
    rect: (f64, f64, f64, f64),
) {
    let Some(kind) = state.kind else {
        return;
    };
    let (cx, cy, cw, ch) = rect;

    if kind == ChildKind::Calendar {
        calendar::draw(cr, th, world, (cx, cy, cw, ch));
        return;
    }

    // Card background
    draw::card(cr, th, cx, cy, cw, ch);

    // Title
    draw::title(cr, th, cx + PAD_X, cy + PAD_Y, kind.title());

    let start_y = cy + PAD_Y + 22.0 + GAP;

    if state.items.is_empty() {
        let msg = if kind == ChildKind::Bluetooth && state.bt_scan {
            "Looking for devices nearby"
        } else {
            "None found"
        };
        draw::text(
            cr,
            cx + PAD_X,
            start_y + (ROW_HEIGHT - 13.0) / 2.0,
            &Text {
                size: 13.0,
                bold: false,
                color: th.muted,
                text: msg,
            },
        );
        return;
    }

    for (i, item) in state.items.iter().enumerate() {
        let row_y = start_y + i as f64 * ROW_HEIGHT;
        let is_hovered = state.hover_row == Some(i);

        // Row background on active / hover
        if item.active || is_hovered {
            draw::round_rect(
                cr,
                cx + PAD_X - 4.0,
                row_y,
                cw - 2.0 * PAD_X + 8.0,
                ROW_HEIGHT - 2.0,
                th.small_radius(),
            );
            if item.active {
                draw::set(cr, th.accent.alpha(0.20));
                let _ = cr.fill();
            } else {
                draw::set(cr, th.track.alpha(0.35));
                let _ = cr.fill();
            }
        }

        // Active indicator dot/bar
        if item.active {
            draw::set(cr, th.accent);
            cr.rectangle(cx + PAD_X - 2.0, row_y + 8.0, 3.0, ROW_HEIGHT - 18.0);
            let _ = cr.fill();
        }

        // Label
        let text_color = if item.active {
            th.accent
        } else if is_hovered {
            th.text
        } else {
            th.text.alpha(0.85)
        };

        draw::text_max(
            cr,
            cx + PAD_X + 8.0,
            row_y + (ROW_HEIGHT - 13.0) / 2.0 - 1.0,
            cw - 2.0 * PAD_X - 80.0,
            &Text {
                size: 13.0,
                bold: item.active,
                color: text_color,
                text: &item.label,
            },
        );

        // Meta (right-aligned)
        if !item.meta.is_empty() {
            let meta_color = if item.active {
                th.accent.alpha(0.90)
            } else {
                th.muted
            };
            draw::text_right(
                cr,
                cx + cw - PAD_X,
                row_y + (ROW_HEIGHT - 11.5) / 2.0,
                &Text {
                    size: 11.5,
                    bold: false,
                    color: meta_color,
                    text: &item.meta,
                },
            );
        }
    }
}

/// Handles click event inside child card, returning whether state changed and any action to run.
pub fn click_child(
    world: &mut World,
    rect: (f64, f64, f64, f64),
    pos: (f64, f64),
    button: u32,
) -> (bool, Option<String>) {
    let Some(kind) = world.child.kind else {
        return (false, None);
    };

    if !is_left_button(button) {
        return (false, None);
    }

    let (cx, cy, cw, _) = rect;
    let (px, py) = pos;

    if kind == ChildKind::Calendar {
        let changed = calendar::click(world, cw, (px - cx, py - cy));
        return (changed, None);
    }

    let start_y = cy + PAD_Y + 22.0 + GAP;
    if py < start_y {
        return (false, None);
    }

    let row = ((py - start_y) / ROW_HEIGHT).floor() as usize;
    if let Some(item) = world.child.items.get(row).cloned() {
        if kind == ChildKind::Bluetooth && item.id == "-" {
            // look for devices for 15 s (it only set a flag: nothing searched); the devices
            // found are listed the next time the list opens
            world.child.bt_scan = true;
            world.run("bluetoothctl --timeout 15 scan on >/dev/null 2>&1".to_string());
            return (true, None);
        }

        let cmd = action_for_child_item(kind, &item);
        if let Some(ref c) = cmd {
            world.run(c.clone());
        }

        // Update active flag locally
        for (i, it) in world.child.items.iter_mut().enumerate() {
            it.active = i == row;
        }

        if kind == ChildKind::Power {
            world.switches.power_profile = item.id.clone();
        }

        (true, cmd)
    } else {
        (false, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_wifi_nmcli_output() {
        let sample = "*:90:WPA2:HomeNet\n :62:WPA2:Neighbours 5G\n :40:WPA2:Cafe guest\n";
        let items = parse_wifi_list(sample);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "HomeNet");
        assert_eq!(items[0].meta, "90%");
        assert!(items[0].active);
        assert_eq!(items[1].label, "Neighbours 5G");
        assert_eq!(items[1].meta, "62%");
        assert!(!items[1].active);
    }

    #[test]
    fn parses_real_bluetooth_output() {
        let paired = "Device 4C:87:5D:11:22:33 Headphones\nDevice AA:BB:CC:DD:EE:FF Keyboard\n";
        let scanned = "Device 11:22:33:44:55:66 Speaker\n";

        // Without scanning: paired devices + Find new devices
        let items = parse_bluetooth_list(paired, scanned, false);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "Headphones");
        assert_eq!(items[0].id, "4C:87:5D:11:22:33");
        assert_eq!(items[2].label, "Find new devices");
        assert_eq!(items[2].id, "-");

        // With scanning: paired devices + Pair entry
        let items_scan = parse_bluetooth_list(paired, scanned, true);
        assert_eq!(items_scan.len(), 3);
        assert_eq!(items_scan[2].label, "Speaker");
        assert_eq!(items_scan[2].meta, "Pair");
    }

    #[test]
    fn parses_real_pactl_audio_sinks() {
        let sample = "Sink #1\n\tState: RUNNING\n\tName: alsa_output.pci-0000_00_1f.3.analog-stereo\n\tDescription: Built-in Audio\nSink #2\n\tState: IDLE\n\tName: alsa_output.pci-0000_01_00.1.hdmi-stereo\n\tDescription: HDMI Audio\n";
        let items = parse_audio_list(sample, "alsa_output.pci-0000_00_1f.3.analog-stereo");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "Built-in Audio");
        assert_eq!(items[0].id, "alsa_output.pci-0000_00_1f.3.analog-stereo");
        assert!(items[0].active);
        assert_eq!(items[1].label, "HDMI Audio");
        assert!(!items[1].active);
    }

    #[test]
    fn power_profiles_list_marks_current() {
        let list = power_profiles_list("performance");
        assert_eq!(list.len(), 3);
        assert!(list[0].active);
        assert_eq!(list[0].id, "performance");
        assert!(!list[1].active);

        let list_bal = power_profiles_list("balanced");
        assert!(list_bal[1].active);
    }

    #[test]
    fn real_pactl_output_lists_each_output_once() {
        let out = "Sink #57\n\tState: SUSPENDED\n\tName: auto_null\n\tDescription: Dummy Output\n\tProperties:\n\t\tnode.name = \"auto_null\"\n\t\taudio.rate = \"48000\"\nSink #60\n\tName: alsa_output.usb\n\tDescription: USB Headphones\n\tProperties:\n\t\tmedia.class = \"Audio/Sink\"\n";
        let items = parse_audio_list(out, "alsa_output.usb");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["Dummy Output", "USB Headphones"]);
        assert!(items[1].active && !items[0].active);
    }

    #[test]
    fn outside_names_never_run_as_commands() {
        let evil = ChildItem {
            id: "$(touch /tmp/pwned)'; rm -rf x '".into(),
            label: "\"; reboot; \"".into(),
            meta: "Pair".into(),
            active: false,
        };
        let cmd = action_for_child_item(ChildKind::Bluetooth, &evil).unwrap();
        assert!(
            cmd.contains("bluetoothctl pair '$(touch /tmp/pwned)'\\''; rm -rf x '\\'''"),
            "{cmd}"
        );
        let wifi = action_for_child_item(ChildKind::Wifi, &evil).unwrap();
        assert!(wifi.starts_with("s=$(printf '%s' '$(touch"), "{wifi}");
    }

    #[test]
    fn action_for_child_item_generates_expected_commands() {
        let wifi_item = ChildItem {
            id: "HomeNet".into(),
            label: "HomeNet".into(),
            meta: "90%".into(),
            active: false,
        };
        let wifi_cmd = action_for_child_item(ChildKind::Wifi, &wifi_item).unwrap();
        assert!(wifi_cmd.contains("nmcli connection up id \"$s\""));
        assert!(wifi_cmd.contains("nmcli dev wifi connect \"$s\""));

        let bt_conn = ChildItem {
            id: "AA:BB:CC:11:22:33".into(),
            label: "Headphones".into(),
            meta: "Paired".into(),
            active: false,
        };
        let bt_cmd = action_for_child_item(ChildKind::Bluetooth, &bt_conn).unwrap();
        assert_eq!(
            bt_cmd,
            "bluetoothctl connect 'AA:BB:CC:11:22:33' >/dev/null 2>&1"
        );

        let bt_disconn = ChildItem {
            id: "AA:BB:CC:11:22:33".into(),
            label: "Headphones".into(),
            meta: "Connected".into(),
            active: true,
        };
        let bt_dis_cmd = action_for_child_item(ChildKind::Bluetooth, &bt_disconn).unwrap();
        assert_eq!(
            bt_dis_cmd,
            "bluetoothctl disconnect 'AA:BB:CC:11:22:33' >/dev/null 2>&1"
        );

        let bt_pair = ChildItem {
            id: "11:22:33:44:55:66".into(),
            label: "Mouse".into(),
            meta: "Pair".into(),
            active: false,
        };
        let pair_cmd = action_for_child_item(ChildKind::Bluetooth, &bt_pair).unwrap();
        assert!(pair_cmd.contains("bluetoothctl pair"));
        assert!(pair_cmd.contains("bluetoothctl trust"));

        let audio_item = ChildItem {
            id: "sink.analog".into(),
            label: "Speakers".into(),
            meta: "Output".into(),
            active: false,
        };
        let audio_cmd = action_for_child_item(ChildKind::Audio, &audio_item).unwrap();
        assert_eq!(
            audio_cmd,
            "pactl set-default-sink 'sink.analog' >/dev/null 2>&1"
        );

        let power_item = ChildItem {
            id: "performance".into(),
            label: "Performance".into(),
            meta: "".into(),
            active: false,
        };
        let power_cmd = action_for_child_item(ChildKind::Power, &power_item).unwrap();
        assert!(power_cmd.contains("powerprofilesctl set 'performance'"));
    }
}
