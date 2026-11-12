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
    Replay,
}

impl ChildKind {
    pub fn title(self) -> &'static str {
        match self {
            ChildKind::Wifi => "Network",
            ChildKind::Bluetooth => "Bluetooth devices",
            ChildKind::Audio => "Sound",
            ChildKind::Power => "Power profiles",
            ChildKind::Calendar => "Calendar",
            ChildKind::Replay => "Replay",
        }
    }

    pub fn parent_module(self) -> Module {
        match self {
            ChildKind::Wifi => Module::Wifi,
            ChildKind::Bluetooth => Module::Bluetooth,
            ChildKind::Audio => Module::Volume,
            ChildKind::Power => Module::Power,
            ChildKind::Calendar => Module::Clock,
            ChildKind::Replay => Module::Replay,
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
    /// the Network card: a search for networks was asked (the next list rescans)
    pub scan: bool,
    pub rect: Option<(f64, f64, f64, f64)>,
    /// the list drawn inside the Quick settings card (`opens_in_place`): kept while it folds away
    /// after closing, so the fold has something to show
    pub shown: Option<ChildKind>,
    /// how far that list is open, 0 to 1 (`Shell::tick` moves it; layout and drawing read it)
    pub reveal: f64,
}

/// Lists that open inside the Quick settings card, under their switch, instead of a card beside
/// the panel (owner 2026-11-13, the "More on hover" design): Network, Bluetooth, power, replay.
pub fn opens_in_place(kind: ChildKind) -> bool {
    matches!(
        kind,
        ChildKind::Wifi | ChildKind::Bluetooth | ChildKind::Power | ChildKind::Replay
    )
}

/// The list's height when fully open in place: its rows and a little room above and below.
pub fn list_height(item_count: usize) -> f64 {
    item_count.max(1) as f64 * ROW_HEIGHT + 2.0 * LIST_PAD
}

/// The room above and below the rows of a list opened in place.
pub const LIST_PAD: f64 = 6.0;

/// The eased height the in-place list takes now (0 when none is shown).
pub fn inplace_extra(state: &ChildState) -> f64 {
    if state.shown.is_none() || state.reveal <= 0.0 {
        return 0.0;
    }
    list_height(state.items.len()) * ease(state.reveal)
}

/// Ease out (cubic): fast at first, settling softly.
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Where the first row starts in a list's rectangle: under the title in a card beside the panel,
/// near the top when it opens in place.
pub fn list_top(rect: (f64, f64, f64, f64), inplace: bool) -> f64 {
    if inplace {
        rect.1 + LIST_PAD
    } else {
        rect.1 + PAD_Y + 22.0 + GAP
    }
}

/// The Network card's last row while Wi-Fi is on: search again for networks (the list is the
/// one the system last found; a search takes a few seconds).
pub const WIFI_SCAN: &str = "#wifi-scan";

/// The Replay card's rows: save a clip of what the buffer holds, open the clips folder, the
/// buffer on or off.
pub const REPLAY_SAVE: &str = "#replay-save";
pub const REPLAY_OPEN: &str = "#replay-open";
pub const REPLAY_TOGGLE: &str = "#replay-toggle";

/// The Replay card: with the buffer on, Save a clip first; the folder; the switch.
pub fn replay_rows(on: bool) -> Vec<ChildItem> {
    let row = |id: &str, label: &str, meta: &str| ChildItem {
        id: id.into(),
        label: label.into(),
        meta: meta.into(),
        active: false,
    };
    let mut rows = Vec::new();
    if on {
        rows.push(row(REPLAY_SAVE, "Save a clip", "Super+Alt+C"));
    }
    rows.push(row(REPLAY_OPEN, "Open the clips folder", "Videos/Clips"));
    rows.push(row(
        REPLAY_TOGGLE,
        "Keep the last minutes",
        if on { "On" } else { "Off" },
    ));
    rows
}

/// The Network card's switch row ids: clicking turns Wi-Fi on or off.
pub const WIFI_ON: &str = "#wifi-on";
pub const WIFI_OFF: &str = "#wifi-off";
/// A row that is only said (the cable's state, a heading): nothing happens on a click.
pub const SAID: &str = "#";

/// The Network card's first rows from `nmcli -t -f TYPE,STATE device` and `nmcli radio wifi`:
/// "Wired" with its state when the machine has a cable port, then the Wi-Fi switch (its id turns
/// it the other way: WIFI_OFF while on).
pub fn network_rows(out: &str) -> Vec<ChildItem> {
    let mut rows = Vec::new();
    let mut eth: Option<bool> = None;
    let mut radio: Option<bool> = None;
    let mut has_wifi = false;
    for l in out.lines().map(str::trim) {
        let low = l.to_ascii_lowercase();
        if let Some(state) = low.strip_prefix("ethernet:") {
            eth = Some(eth.unwrap_or(false) || state == "connected");
        } else if low.starts_with("wifi:") {
            has_wifi = true;
        } else if low == "enabled" {
            radio = Some(true);
        } else if low == "disabled" {
            radio = Some(false);
        }
    }
    if let Some(connected) = eth {
        rows.push(ChildItem {
            id: SAID.into(),
            label: "Wired".into(),
            meta: if connected { "Connected" } else { "Unplugged" }.into(),
            active: connected,
        });
    }
    if has_wifi || radio.is_some() {
        let on = radio.unwrap_or(false);
        rows.push(ChildItem {
            id: if on { WIFI_OFF } else { WIFI_ON }.into(),
            label: "Wi-Fi".into(),
            meta: if on { "On" } else { "Off" }.into(),
            active: false,
        });
    }
    rows
}

/// An app's row in the Sound card: id APP_PREFIX + its sink input's index, meta its level ("45%")
/// or "Muted"; drawn with a slider, a click on which sets the app's volume.
pub const APP_PREFIX: &str = "@app:";

/// The apps playing sound, from `pactl list sink-inputs`: a heading row ("Apps"), then one row
/// per app with its level. None playing: the heading and a row saying so.
pub fn parse_sink_inputs(out: &str) -> Vec<ChildItem> {
    let mut rows = vec![ChildItem {
        id: SAID.into(),
        label: "Apps".into(),
        meta: String::new(),
        active: false,
    }];
    let mut cur: Option<(String, String, String, bool)> = None; // index, name, level, muted
    let push = |c: Option<(String, String, String, bool)>, rows: &mut Vec<ChildItem>| {
        if let Some((idx, name, level, muted)) = c {
            if !idx.is_empty() && idx.bytes().all(|b| b.is_ascii_digit()) {
                rows.push(ChildItem {
                    id: format!("{APP_PREFIX}{idx}"),
                    label: if name.is_empty() { "App".into() } else { name },
                    meta: if muted { "Muted".into() } else { level },
                    active: false,
                });
            }
        }
    };
    for raw in out.lines() {
        let l = raw.trim();
        if let Some(idx) = l.strip_prefix("Sink Input #") {
            push(cur.take(), &mut rows);
            cur = Some((idx.trim().to_string(), String::new(), "100%".into(), false));
        } else if let Some(c) = cur.as_mut() {
            if let Some(m) = l.strip_prefix("Mute:") {
                c.3 = m.trim() == "yes";
            } else if let Some(v) = l.strip_prefix("Volume:") {
                if let Some(p) = v.split('/').map(str::trim).find(|t| t.ends_with('%')) {
                    c.2 = p.to_string();
                }
            } else if let Some(n) = l.strip_prefix("application.name = ") {
                c.1 = n.trim().trim_matches('"').to_string();
            } else if c.1.is_empty() {
                if let Some(n) = l.strip_prefix("application.process.binary = ") {
                    c.1 = n.trim().trim_matches('"').to_string();
                }
            }
        }
    }
    push(cur.take(), &mut rows);
    if rows.len() == 1 {
        rows.push(none_found("No app is playing sound"));
    }
    rows
}

/// The Sound card's microphone row id (the default input), and the line under it naming the apps
/// using it.
pub const MIC: &str = "@app:mic";
pub const MIC_USERS: &str = "#mic-users";

/// The microphone rows from `pactl get-default-source`, `get-source-volume @DEFAULT_SOURCE@`,
/// `get-source-mute @DEFAULT_SOURCE@` (one output, in that order) and `pactl list source-outputs`
/// (who records): a heading, the microphone with its level, and "Used by ..." when an app
/// listens. Nothing without a real input (no source, or only a monitor of an output).
pub fn mic_rows(source_out: &str, outputs_out: &str) -> Vec<ChildItem> {
    let mut lines = source_out.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(name) = lines.next() else {
        return Vec::new();
    };
    if name.ends_with(".monitor") || name.starts_with("Volume:") {
        return Vec::new();
    }
    let rest: Vec<&str> = lines.collect();
    let level = rest
        .iter()
        .find_map(|l| l.strip_prefix("Volume:"))
        .and_then(|v| v.split('/').map(str::trim).find(|t| t.ends_with('%')))
        .unwrap_or("100%")
        .to_string();
    let muted = rest.iter().any(|l| l.trim() == "Mute: yes");
    let mut users: Vec<String> = Vec::new();
    for l in outputs_out.lines().map(str::trim) {
        if let Some(n) = l.strip_prefix("application.name = ") {
            let n = n.trim_matches('"').to_string();
            // level meters (pavucontrol's peak detect) are not someone listening
            if !n.is_empty() && !users.contains(&n) && !n.eq_ignore_ascii_case("pavucontrol") {
                users.push(n);
            }
        }
    }
    let said = |id: &str, label: String| ChildItem {
        id: id.into(),
        label,
        meta: String::new(),
        active: false,
    };
    let mut rows = vec![
        said(SAID, "Input".into()),
        ChildItem {
            id: MIC.into(),
            label: "Microphone".into(),
            meta: if muted { "Muted".into() } else { level },
            active: false,
        },
    ];
    if !users.is_empty() {
        rows.push(said(MIC_USERS, format!("Used by {}", users.join(", "))));
    }
    rows
}

/// The level (0-150) an app row's meta says, None for other rows (and "Muted": 0).
pub fn app_level(item: &ChildItem) -> Option<u32> {
    item.id.strip_prefix(APP_PREFIX)?;
    if item.meta == "Muted" {
        return Some(0);
    }
    item.meta.trim_end_matches('%').parse().ok()
}

/// The slider's span `(x0, x1)` in an app row of a card at `cx` of width `cw`.
pub fn app_slider_span(cx: f64, cw: f64) -> (f64, f64) {
    (cx + PAD_X + 118.0, cx + cw - PAD_X - 44.0)
}

/// The Network card's "Hidden network..." row: asks the network's name, then its password.
pub const HIDDEN: &str = "#hidden";
/// A VPN's row in the Network card: id VPN_PREFIX + its NetworkManager connection name.
pub const VPN_PREFIX: &str = "@vpn:";

/// The VPNs set up in NetworkManager, from `nmcli -t -f NAME,TYPE,ACTIVE connection show`: one
/// row each (VPN and WireGuard connections), "Connected" or "Off".
pub fn vpn_rows(out: &str) -> Vec<ChildItem> {
    out.lines()
        .filter_map(|l| {
            // nmcli -t escapes a ':' in a name as "\:"; the last two fields never hold one
            let mut parts = l.rsplitn(3, ':');
            let active = parts.next()? == "yes";
            let kind = parts.next()?;
            let name = parts.next()?.replace("\\:", ":");
            (kind == "vpn" || kind == "wireguard").then(|| ChildItem {
                id: format!("{VPN_PREFIX}{name}"),
                label: name,
                meta: if active { "Connected" } else { "Off" }.into(),
                active,
            })
        })
        .collect()
}

/// The "Hidden network..." row.
pub fn hidden_row() -> ChildItem {
    ChildItem {
        id: HIDDEN.into(),
        label: "Hidden network...".into(),
        meta: String::new(),
        active: false,
    }
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
    if item.id.is_empty() || item.id == SAID {
        return None;
    }
    if item.id == WIFI_ON {
        return Some("nmcli radio wifi on >/dev/null 2>&1".into());
    }
    if item.id == REPLAY_SAVE {
        return Some("sh \"$HOME/infynitae/live/launch.sh\" save-replay >/dev/null 2>&1".into());
    }
    if item.id == REPLAY_OPEN {
        return Some(
            "d=\"${CLIPS_DIR:-$HOME/Videos/Clips}\"; mkdir -p \"$d\"; xdg-open \"$d\" >/dev/null 2>&1".into(),
        );
    }
    if item.id == WIFI_OFF {
        return Some("nmcli radio wifi off >/dev/null 2>&1".into());
    }
    if item.id == HIDDEN {
        return Some(
            "s=$(infynitae-edge ask --text \"Network name\") || exit 0; [ -n \"$s\" ] || exit 0; \
             pw=$(infynitae-edge ask --text --secret \"Password for $s (empty for an open network)\") || exit 0; \
             if [ -n \"$pw\" ]; then nmcli dev wifi connect \"$s\" password \"$pw\" hidden yes >/dev/null 2>&1; \
             else nmcli dev wifi connect \"$s\" hidden yes >/dev/null 2>&1; fi || \
             notify-send -a Infynitae Wi-Fi \"Could not join $s\""
                .into(),
        );
    }
    if let Some(name) = item.id.strip_prefix(VPN_PREFIX) {
        let name = sq(name);
        let verb = if item.active { "down" } else { "up" };
        return Some(format!(
            "nmcli connection {verb} id {name} >/dev/null 2>&1 || \
             notify-send -a Infynitae VPN \"Could not turn {} {verb}\"",
            item.label.replace('"', "")
        ));
    }
    if item.id.starts_with(APP_PREFIX) {
        // an app's volume: the level is set by where the slider was clicked (click_child)
        return None;
    }
    let id = sq(&item.id);
    match kind {
        // a known network, else an open one, else its password asked in the edge's own prompt
        // (a new secured network failed without a word before)
        ChildKind::Wifi => Some(format!(
            "s=$(printf '%s' {id} | tr '\\036' '\\t'); \
                 nmcli connection up id \"$s\" >/dev/null 2>&1 && exit 0; \
                 nmcli dev wifi connect \"$s\" >/dev/null 2>&1 && exit 0; \
                 pw=$(infynitae-edge ask --text --secret \"Password for $s\") || exit 0; \
                 [ -n \"$pw\" ] || exit 0; \
                 nmcli dev wifi connect \"$s\" password \"$pw\" >/dev/null 2>&1 || \
                 notify-send -a Infynitae Wi-Fi \"Could not join $s (wrong password?)\""
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
        // its rows are all named (REPLAY_*), handled above
        ChildKind::Calendar | ChildKind::Replay => None,
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

/// Whether `kind`'s list opens inside `panel` under its switch: a stacked panel with switches as
/// rows (the default look), and the switch in it.
pub fn in_place_here(
    panel: &Panel,
    rects: &[(f64, f64, f64, f64)],
    kind: ChildKind,
    switch_look: &str,
) -> bool {
    opens_in_place(kind)
        && !panel.placed
        && !matches!(switch_look, "icons" | "tiles")
        && find_parent_rect(panel, rects, kind).is_some()
}

/// The rectangle of the list open in place on screen (`origin`: the panel's top left with the
/// scroll applied): the panel's width (rows keep the card's padding), from under its switch, as
/// tall as it is open now.
pub fn inplace_rect(
    panel: &Panel,
    rects: &[(f64, f64, f64, f64)],
    state: &ChildState,
    switch_look: &str,
    origin: (f64, f64),
    pw: f64,
) -> Option<(f64, f64, f64, f64)> {
    let kind = state.shown?;
    if !in_place_here(panel, rects, kind, switch_look) {
        return None;
    }
    let h = inplace_extra(state);
    if h <= 0.0 {
        return None;
    }
    let r = find_parent_rect(panel, rects, kind)?;
    Some((origin.0, origin.1 + r.1 + 40.0, pw, h))
}

/// The card beside the panel for an open list that does not open in place (the sound outputs,
/// the calendar, or any list in a placed panel or with switches as icons or tiles).
#[allow(clippy::too_many_arguments)]
pub fn side_rect(
    panel: &Panel,
    rects: &[(f64, f64, f64, f64)],
    state: &ChildState,
    switch_look: &str,
    origin: (f64, f64),
    pw: f64,
    screen: (f64, f64),
) -> Option<(f64, f64, f64, f64)> {
    if !state.open {
        return None;
    }
    let kind = state.kind.unwrap_or(ChildKind::Wifi);
    if in_place_here(panel, rects, kind, switch_look) {
        return None;
    }
    let parent_y = find_parent_rect(panel, rects, kind)
        .map(|r| r.1)
        .unwrap_or(0.0);
    Some(child_card_geometry(
        kind,
        origin,
        pw,
        parent_y,
        state.items.len(),
        screen,
    ))
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
    inplace: bool,
) {
    let Some(kind) = (if inplace { state.shown } else { state.kind }) else {
        return;
    };
    let (cx, cy, cw, ch) = rect;

    if kind == ChildKind::Calendar {
        calendar::draw(cr, th, world, (cx, cy, cw, ch));
        return;
    }

    if inplace {
        // inside the Quick settings card: a soft shaded box, no card or title of its own (the
        // switch above says what it is)
        draw::round_rect(
            cr,
            cx + PAD_X - 8.0,
            cy,
            cw - 2.0 * PAD_X + 16.0,
            list_height(state.items.len()),
            th.small_radius(),
        );
        draw::set(cr, th.text.alpha(0.04));
        let _ = cr.fill();
    } else {
        draw::card(cr, th, cx, cy, cw, ch);
        draw::title(cr, th, cx + PAD_X, cy + PAD_Y, kind.title());
    }

    let start_y = list_top(rect, inplace);

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

        // a heading ("Apps"): small and muted, a line above it
        if item.id == SAID && item.meta.is_empty() {
            draw::round_rect(cr, cx + PAD_X, row_y + 4.0, cw - 2.0 * PAD_X, 1.0, 0.5);
            draw::set(cr, th.track);
            let _ = cr.fill();
            draw::text(
                cr,
                cx + PAD_X,
                row_y + ROW_HEIGHT - 17.0,
                &Text {
                    size: 11.5,
                    bold: false,
                    color: th.muted,
                    text: &item.label,
                },
            );
            continue;
        }

        // an app playing sound: its name, a slider, its level
        if let Some(level) = app_level(item) {
            let mid = row_y + ROW_HEIGHT / 2.0;
            super::hot(
                cr,
                th,
                world,
                cx + PAD_X - 4.0,
                row_y,
                cw - 2.0 * PAD_X + 8.0,
                ROW_HEIGHT - 2.0,
                th.small_radius(),
            );
            draw::text_max(
                cr,
                cx + PAD_X + 8.0,
                mid - 8.0,
                104.0,
                &Text {
                    size: 13.0,
                    bold: false,
                    color: th.text.alpha(0.9),
                    text: &item.label,
                },
            );
            let (x0, x1) = app_slider_span(cx, cw);
            let frac = (f64::from(level.min(100)) / 100.0).clamp(0.0, 1.0);
            draw::round_rect(cr, x0, mid - 2.0, x1 - x0, 4.0, 2.0);
            draw::set(cr, th.track);
            let _ = cr.fill();
            if frac > 0.0 {
                draw::round_rect(cr, x0, mid - 2.0, (x1 - x0) * frac, 4.0, 2.0);
                draw::set(cr, th.accent);
                let _ = cr.fill();
            }
            cr.new_sub_path();
            cr.arc(x0 + (x1 - x0) * frac, mid, 6.0, 0.0, std::f64::consts::TAU);
            draw::set(cr, th.text);
            let _ = cr.fill();
            draw::text_right(
                cr,
                cx + cw - PAD_X,
                mid - 8.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: &item.meta,
                },
            );
            continue;
        }

        // the hover light, and the active row's tint over it
        let is_hovered = super::hot(
            cr,
            th,
            world,
            cx + PAD_X - 4.0,
            row_y,
            cw - 2.0 * PAD_X + 8.0,
            ROW_HEIGHT - 2.0,
            th.small_radius(),
        );
        if item.active {
            draw::round_rect(
                cr,
                cx + PAD_X - 4.0,
                row_y,
                cw - 2.0 * PAD_X + 8.0,
                ROW_HEIGHT - 2.0,
                th.small_radius(),
            );
            draw::set(cr, th.accent.alpha(0.20));
            let _ = cr.fill();
        }

        // Active indicator dot/bar
        if item.active {
            draw::set(cr, th.accent);
            cr.rectangle(cx + PAD_X - 2.0, row_y + 8.0, 3.0, ROW_HEIGHT - 18.0);
            let _ = cr.fill();
        }

        // Label
        // a row that does something (search again, save a clip) reads as an action
        let action = item.id == WIFI_SCAN || item.id == REPLAY_SAVE;
        let text_color = if item.active || action {
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
    inplace: bool,
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
        let changed = calendar::click(world, (cw, rect.3), (px - cx, py - cy));
        return (changed, None);
    }

    let start_y = list_top(rect, inplace);
    if py < start_y {
        return (false, None);
    }

    let row = ((py - start_y) / ROW_HEIGHT).floor() as usize;
    if let Some(item) = world.child.items.get(row).cloned() {
        if item.id == SAID || item.id == MIC_USERS || item.id.is_empty() {
            return (false, None);
        }
        if let Some(idx) = item.id.strip_prefix(APP_PREFIX) {
            // the slider: the level where it was clicked (0-100 %); elsewhere on the row nothing
            let (x0, x1) = app_slider_span(cx, cw);
            let mic = idx == "mic";
            if !mic && !idx.bytes().all(|b| b.is_ascii_digit()) {
                return (false, None);
            }
            if px < x0 - 8.0 {
                // its name: mute or unmute it
                let cmd = if mic {
                    "pactl set-source-mute @DEFAULT_SOURCE@ toggle >/dev/null 2>&1".to_string()
                } else {
                    format!("pactl set-sink-input-mute {idx} toggle >/dev/null 2>&1")
                };
                world.run(cmd.clone());
                if let Some(it) = world.child.items.get_mut(row) {
                    // unmuted: a level until the card's reload says the real one
                    it.meta = if it.meta == "Muted" {
                        "100%".into()
                    } else {
                        "Muted".into()
                    };
                }
                world.child_pending = world.live.then_some(ChildKind::Audio);
                return (true, Some(cmd));
            }
            if px > x1 + 8.0 {
                return (false, None);
            }
            let pct = (((px - x0) / (x1 - x0)).clamp(0.0, 1.0) * 100.0).round() as u32;
            let cmd = if mic {
                format!(
                    "pactl set-source-mute @DEFAULT_SOURCE@ 0 >/dev/null 2>&1; pactl set-source-volume @DEFAULT_SOURCE@ {pct}% >/dev/null 2>&1"
                )
            } else {
                format!(
                    "pactl set-sink-input-mute {idx} 0 >/dev/null 2>&1; pactl set-sink-input-volume {idx} {pct}% >/dev/null 2>&1"
                )
            };
            world.run(cmd.clone());
            if let Some(it) = world.child.items.get_mut(row) {
                it.meta = format!("{pct}%");
            }
            return (true, Some(cmd));
        }
        if item.id == WIFI_SCAN {
            // search again: the list says so until the networks found come (a few seconds)
            world.child.scan = true;
            world
                .child
                .items
                .retain(|r| r.id.starts_with('#') && r.id != WIFI_SCAN);
            world.child.items.push(none_found("Searching for networks"));
            if world.live && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() {
                world.child_pending = Some(ChildKind::Wifi);
            } else {
                world.child.items = crate::modules::load_child_list(ChildKind::Wifi, true, false);
            }
            return (true, None);
        }
        if item.id == REPLAY_TOGGLE {
            let cmd = crate::modules::switches::toggle_action(Module::Replay, &mut world.switches);
            if let Some(ref c) = cmd {
                world.run(c.clone());
            }
            world.child.items = replay_rows(world.switches.replay_on);
            return (true, cmd);
        }
        if item.id == WIFI_ON || item.id == WIFI_OFF {
            let cmd = action_for_child_item(kind, &item);
            if let Some(ref c) = cmd {
                world.run(c.clone());
            }
            let on = item.id == WIFI_ON;
            world.switches.wifi_on = on;
            if let Some(it) = world.child.items.get_mut(row) {
                it.id = if on { WIFI_OFF } else { WIFI_ON }.into();
                it.meta = if on { "On" } else { "Off" }.into();
            }
            if !on {
                // Wi-Fi off: its networks go
                world.child.items.retain(|r| r.id.starts_with('#'));
            }
            return (true, cmd);
        }
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

        // Update active flag locally (the chosen network, output or mode; not the said rows,
        // the switch or the apps)
        for (i, it) in world.child.items.iter_mut().enumerate() {
            if !it.id.starts_with('#') && !it.id.starts_with(APP_PREFIX) {
                it.active = i == row;
            }
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
    fn vpns_come_from_nmcli_and_turn_on_or_off() {
        let rows = vpn_rows(
            "Home:802-11-wireless:yes\nWork VPN:vpn:no\nwg0:wireguard:yes\nOffice\\:Main:vpn:no\n",
        );
        let names: Vec<_> = rows
            .iter()
            .map(|r| (r.label.as_str(), r.meta.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("Work VPN", "Off"),
                ("wg0", "Connected"),
                ("Office:Main", "Off")
            ]
        );
        let up = action_for_child_item(ChildKind::Wifi, &rows[0]).unwrap();
        assert!(up.starts_with("nmcli connection up id 'Work VPN'"), "{up}");
        let down = action_for_child_item(ChildKind::Wifi, &rows[1]).unwrap();
        assert!(down.starts_with("nmcli connection down id 'wg0'"), "{down}");
    }

    #[test]
    fn the_hidden_network_row_asks_the_name_then_joins_hidden() {
        let row = hidden_row();
        assert!(
            row.id.starts_with('#'),
            "counts as a control row, not a network"
        );
        let cmd = action_for_child_item(ChildKind::Wifi, &row).unwrap();
        assert!(cmd.contains("ask --text \"Network name\""), "{cmd}");
        assert!(cmd.contains("hidden yes"), "{cmd}");
    }

    #[test]
    fn the_microphone_row_says_its_level_and_who_listens() {
        let src = "alsa_input.usb-mic\nVolume: front-left: 45000 /  69% / -9.7 dB,   front-right: 45000 /  69% / -9.7 dB\nMute: no\n";
        let rec = "Source Output #12\n\tProperties:\n\t\tapplication.name = \"Discord\"\nSource Output #13\n\tProperties:\n\t\tapplication.name = \"PulseAudio Volume Control\"\n";
        let rows = mic_rows(src, rec);
        assert_eq!(rows[1].id, MIC);
        assert_eq!(app_level(&rows[1]), Some(69));
        assert_eq!(rows[2].label, "Used by Discord, PulseAudio Volume Control");
        assert!(
            mic_rows("alsa_output.pci.monitor\nVolume: 1 / 100% / 0 dB\n", "").is_empty(),
            "no real input: no row"
        );
        assert!(mic_rows("", "").is_empty());
        assert_eq!(
            mic_rows(&src.replace("Mute: no", "Mute: yes"), "")[1].meta,
            "Muted"
        );
    }

    #[test]
    fn the_microphone_mutes_by_its_name_and_levels_by_its_slider() {
        let mut world = World::now();
        world.child.open = true;
        world.child.kind = Some(ChildKind::Audio);
        world.child.items = mic_rows(
            "alsa_input.mic\nVolume: mono: 1 / 50% / 0 dB\nMute: no\n",
            "Source Output #1\n\t\tapplication.name = \"OBS\"\n",
        );
        let rect = (0.0, 0.0, CHILD_WIDTH, 200.0);
        let row_y = PAD_Y + 22.0 + GAP + ROW_HEIGHT * 1.5;
        let (_, cmd) = click_child(&mut world, rect, (PAD_X + 20.0, row_y), 1, false);
        assert_eq!(
            cmd.as_deref(),
            Some("pactl set-source-mute @DEFAULT_SOURCE@ toggle >/dev/null 2>&1")
        );
        assert_eq!(world.child.items[1].meta, "Muted");
        let (x0, x1) = app_slider_span(0.0, CHILD_WIDTH);
        let (_, cmd) = click_child(&mut world, rect, ((x0 + x1) / 2.0, row_y), 1, false);
        assert!(cmd
            .unwrap()
            .ends_with("pactl set-source-volume @DEFAULT_SOURCE@ 50% >/dev/null 2>&1"));
        let users_y = PAD_Y + 22.0 + GAP + ROW_HEIGHT * 2.5;
        let (changed, cmd) = click_child(&mut world, rect, (60.0, users_y), 1, false);
        assert!(!changed && cmd.is_none(), "the Used by line does nothing");
    }

    #[test]
    fn joining_a_new_secured_network_asks_its_password() {
        let item = ChildItem {
            id: "Cafe".into(),
            label: "Cafe".into(),
            meta: "40%".into(),
            active: false,
        };
        let cmd = action_for_child_item(ChildKind::Wifi, &item).unwrap();
        assert!(cmd.contains("infynitae-edge ask --text --secret"));
        assert!(cmd.contains("nmcli dev wifi connect \"$s\" password \"$pw\""));
    }

    #[test]
    fn the_replay_card_saves_a_clip_opens_the_folder_and_switches() {
        let rows = replay_rows(true);
        assert_eq!(rows[0].id, REPLAY_SAVE, "on: Save a clip first");
        assert_eq!(
            action_for_child_item(ChildKind::Replay, &rows[0]).as_deref(),
            Some("sh \"$HOME/infynitae/live/launch.sh\" save-replay >/dev/null 2>&1")
        );
        assert!(action_for_child_item(ChildKind::Replay, &rows[1])
            .unwrap()
            .contains("xdg-open"));
        assert!(
            replay_rows(false).iter().all(|r| r.id != REPLAY_SAVE),
            "off: nothing to save"
        );
        let mut world = World::now();
        world.switches.replay_on = true;
        assert!(crate::modules::toggle_child(&mut world, ChildKind::Replay));
        let rect = (0.0, 0.0, CHILD_WIDTH, 200.0);
        let toggle_y = PAD_Y + 22.0 + GAP + ROW_HEIGHT * 2.5;
        let (changed, cmd) = click_child(&mut world, rect, (60.0, toggle_y), 1, false);
        assert!(changed && cmd.unwrap().contains("ENABLE_REPLAY_BUFFER false"));
        assert!(!world.switches.replay_on);
        assert!(world.child.items.iter().all(|r| r.id != REPLAY_SAVE));
    }

    #[test]
    fn the_network_card_can_search_again() {
        let mut world = World::now();
        world.child.open = true;
        world.child.kind = Some(ChildKind::Wifi);
        world.child.items = vec![
            ChildItem {
                id: WIFI_OFF.into(),
                label: "Wi-Fi".into(),
                meta: "On".into(),
                active: false,
            },
            ChildItem {
                id: "Home".into(),
                label: "Home".into(),
                meta: "80%".into(),
                active: true,
            },
            ChildItem {
                id: WIFI_SCAN.into(),
                label: "Search for networks".into(),
                meta: String::new(),
                active: false,
            },
        ];
        let rect = (0.0, 0.0, CHILD_WIDTH, 200.0);
        let y = PAD_Y + 22.0 + GAP + ROW_HEIGHT * 2.5;
        let (changed, _) = click_child(&mut world, rect, (60.0, y), 1, false);
        assert!(changed);
        assert!(world.child.scan, "the next list rescans");
    }

    #[test]
    fn network_rows_say_the_cable_and_switch_wifi() {
        let rows = network_rows(
            "ethernet:connected\nwifi:disconnected\nloopback:connected (externally)\nenabled\n",
        );
        assert_eq!(rows[0].label, "Wired");
        assert_eq!(rows[0].meta, "Connected");
        assert!(rows[0].active);
        assert_eq!(rows[1].id, WIFI_OFF, "on: the row turns it off");
        assert_eq!(action_for_child_item(ChildKind::Wifi, &rows[0]), None);
        assert_eq!(
            action_for_child_item(ChildKind::Wifi, &rows[1]).as_deref(),
            Some("nmcli radio wifi off >/dev/null 2>&1")
        );
        let rows = network_rows("wifi:unavailable\ndisabled\n");
        assert_eq!(rows.len(), 1, "no cable port: no Wired row");
        assert_eq!(rows[0].id, WIFI_ON);
    }

    #[test]
    fn apps_playing_sound_get_a_row_each_with_their_level() {
        let out = "Sink Input #45\n\tDriver: PipeWire\n\tMute: no\n\tVolume: front-left: 42598 /  65% / -11.23 dB,   front-right: 42598 /  65% / -11.23 dB\n\tProperties:\n\t\tapplication.name = \"Firefox\"\nSink Input #47\n\tMute: yes\n\tVolume: mono: 65536 / 100% / 0.00 dB\n\tProperties:\n\t\tapplication.process.binary = \"melodyae\"\nSink Input #x9\n";
        let rows = parse_sink_inputs(out);
        assert_eq!(rows[0].label, "Apps");
        assert_eq!(rows.len(), 3, "the malformed index is left out");
        assert_eq!(
            (rows[1].id.as_str(), rows[1].label.as_str()),
            ("@app:45", "Firefox")
        );
        assert_eq!(app_level(&rows[1]), Some(65));
        assert_eq!(
            (rows[2].label.as_str(), rows[2].meta.as_str()),
            ("melodyae", "Muted")
        );
        assert_eq!(app_level(&rows[2]), Some(0));
        assert_eq!(parse_sink_inputs("")[1].label, "No app is playing sound");
    }

    #[test]
    fn a_click_on_an_app_slider_sets_that_apps_volume() {
        let mut world = World::now();
        world.child.open = true;
        world.child.kind = Some(ChildKind::Audio);
        world.child.items = parse_sink_inputs("Sink Input #45\n\tVolume: mono: 1 / 65% / 0 dB\n");
        let rect = (0.0, 0.0, CHILD_WIDTH, 200.0);
        let row_y = PAD_Y + 22.0 + GAP + ROW_HEIGHT * 1.5;
        let (x0, x1) = app_slider_span(0.0, CHILD_WIDTH);
        let (changed, cmd) = click_child(&mut world, rect, ((x0 + x1) / 2.0, row_y), 1, false);
        assert!(changed);
        assert_eq!(
            cmd.as_deref(),
            Some("pactl set-sink-input-mute 45 0 >/dev/null 2>&1; pactl set-sink-input-volume 45 50% >/dev/null 2>&1")
        );
        assert_eq!(world.child.items[1].meta, "50%");
        let (changed, cmd) = click_child(&mut world, rect, (PAD_X + 20.0, row_y), 1, false);
        assert!(changed, "the name mutes the app");
        assert_eq!(
            cmd.as_deref(),
            Some("pactl set-sink-input-mute 45 toggle >/dev/null 2>&1")
        );
    }

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
