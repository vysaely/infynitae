//! Settings, Infynitae's own (no desktop entry): the Network, Bluetooth and Power mode cards (stage 1 of
//! docs/EDGE_APPS.md), in version 1 of `docs/design/apps-edge/system-cards.html`. The data comes from
//! the same commands as today's edge (`nmcli`, `bluetoothctl`, `powerprofilesctl`).

use crate::card::{run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::ui::{self, Icon, List, Row};
use cairo::Context;
use std::any::Any;

pub struct Settings;

const KINDS: &[CardKind] = &[
    CardKind {
        id: "network",
        title: "Network",
        about: "Wired or Wi-Fi, the networks to join",
        sizes: &[Size::S, Size::M],
        default: Size::S,
        half: true,
        sources: &["network"],
    },
    CardKind {
        id: "bluetooth",
        title: "Bluetooth",
        about: "On or off, your devices, pairing new ones",
        sizes: &[Size::S, Size::M],
        default: Size::S,
        half: true,
        sources: &["bluetooth"],
    },
    CardKind {
        id: "power",
        title: "Power mode",
        about: "Power saver, balanced or performance",
        sizes: &[Size::S],
        default: Size::S,
        half: false,
        sources: &["power"],
    },
    crate::cards::quick::KIND,
    crate::cards::session::KIND,
    crate::cards::settings_display::KIND,
    crate::cards::settings_games::KIND,
];

impl Provider for Settings {
    fn id(&self) -> &'static str {
        "settings"
    }
    fn name(&self) -> &'static str {
        "Settings"
    }
    fn claims(&self) -> &'static [Claim] {
        &[]
    }
    fn listed(&self) -> Listed {
        Listed::Always
    }
    fn cards(&self) -> &'static [CardKind] {
        KINDS
    }
    fn card(&self, kind: &str, argument: Option<&str>) -> Option<Box<dyn Card>> {
        match kind {
            "quick" => Some(Box::new(crate::cards::quick::QuickCard::new(argument))),
            "session" if argument.is_none() => Some(Box::new(crate::cards::session::new())),
            "display" => Some(Box::new(crate::cards::settings_display::DisplayCard::new())),
            "games" => Some(Box::new(crate::cards::settings_rows::TableCard::new(
                crate::cards::settings_games::TABLE,
            ))),
            "network" => Some(Box::new(NetworkCard::default())),
            "bluetooth" => Some(Box::new(BluetoothCard::default())),
            "power" => Some(Box::new(PowerCard::default())),
            _ => None,
        }
    }
    fn source(&self, key: &str) -> Option<Box<dyn Source>> {
        match key {
            "network" => Some(Box::new(NetworkSource)),
            "bluetooth" => Some(Box::new(BluetoothSource)),
            "power" => Some(Box::new(PowerSource)),
            "quick" => Some(Box::new(crate::cards::quick::QuickSource)),
            "config" => Some(Box::new(crate::cards::settings_rows::ConfigSource::system())),
            "screens" => Some(Box::new(crate::cards::settings_display::ScreensSource)),
            _ => None,
        }
    }
}

/// A source's output cut at its "---" lines (each command's part), however empty a part is.
pub fn sections(out: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    for l in out.lines() {
        if l.trim() == "---" {
            parts.push(String::new());
        } else {
            let p = parts.last_mut().unwrap();
            p.push_str(l);
            p.push('\n');
        }
    }
    parts
}

/// A string as one shell word.
pub fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ---------------------------------------------------------------------------------------------
// Network

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wlan {
    pub ssid: String,
    pub signal: u8,
    pub secure: bool,
    pub joined: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkData {
    /// NetworkManager answered
    pub service: bool,
    /// this PC has a Wi-Fi device
    pub wifi_device: bool,
    pub radio_on: bool,
    pub wired: bool,
    /// the networks in reach, the joined one first, then by signal
    pub networks: Vec<Wlan>,
    /// the saved connections' names (a saved network joins without a password)
    pub saved: Vec<String>,
}

impl NetworkData {
    pub fn joined(&self) -> Option<&Wlan> {
        self.networks.iter().find(|n| n.joined)
    }
    pub fn icon(&self) -> Icon {
        if self.wired {
            Icon::Wired
        } else if let Some(n) = self.joined() {
            Icon::Wifi(match n.signal {
                s if s < 34 => 1,
                s if s < 67 => 2,
                _ => 3,
            })
        } else if self.radio_on {
            Icon::WifiSearching
        } else {
            Icon::WifiOff
        }
    }
    /// What the row says (version 1 of the design): the state in words, a detail under it, and
    /// whether it is on. A cable first, then the joined network.
    pub fn state(&self) -> (String, String, bool) {
        if self.wired {
            ("Wired".into(), "Cable connected".into(), true)
        } else if let Some(n) = self.joined() {
            (n.ssid.clone(), format!("Wi-Fi, signal {}%", n.signal), true)
        } else if self.wifi_device && self.radio_on {
            ("Not connected".into(), "Wi-Fi is on".into(), false)
        } else if self.wifi_device {
            ("Off".into(), "Wi-Fi is off".into(), false)
        } else {
            ("Not connected".into(), "No cable, no Wi-Fi".into(), false)
        }
    }
}

/// Splits nmcli's terse output line on its unescaped colons ("\:" is a colon in a value).
pub fn nmcli_fields(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut it = line.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                if let Some(n) = it.next() {
                    out.last_mut().unwrap().push(n)
                }
            }
            ':' => out.push(String::new()),
            _ => out.last_mut().unwrap().push(c),
        }
    }
    out
}

/// Reads the network source's output: sections apart by "---": the radio, the devices
/// (TYPE:STATE), the networks (IN-USE:SIGNAL:SECURITY:SSID), the saved connections (NAME:TYPE).
pub fn parse_network(out: &str) -> NetworkData {
    let parts = sections(out);
    let sec = |i: usize| parts.get(i).map(String::as_str).unwrap_or("");
    let mut d = NetworkData {
        radio_on: sec(0).lines().any(|l| l.trim() == "enabled"),
        ..Default::default()
    };
    d.service = !sec(0).trim().is_empty() || !sec(1).trim().is_empty();
    for l in sec(1).lines() {
        let f = nmcli_fields(l.trim());
        if f.len() >= 2 {
            match f[0].as_str() {
                "wifi" => d.wifi_device = d.wifi_device || f[1] != "unavailable",
                "ethernet" if f[1] == "connected" => d.wired = true,
                _ => {}
            }
        }
    }
    for l in sec(2).lines() {
        let f = nmcli_fields(l.trim());
        if f.len() < 4 || f[3].trim().is_empty() {
            continue;
        }
        let ssid = f[3].trim().to_string();
        let joined = f[0].trim() == "*";
        let signal = f[1].trim().parse::<u32>().unwrap_or(0).min(100) as u8;
        let secure = !f[2].trim().is_empty() && f[2].trim() != "--";
        if let Some(n) = d.networks.iter_mut().find(|n| n.ssid == ssid) {
            // the same name on two radios: keep the stronger, and joined if either is
            n.joined |= joined;
            n.signal = n.signal.max(signal);
            continue;
        }
        d.networks.push(Wlan {
            ssid,
            signal,
            secure,
            joined,
        });
    }
    d.networks
        .sort_by(|a, b| b.joined.cmp(&a.joined).then(b.signal.cmp(&a.signal)));
    for l in sec(3).lines() {
        let f = nmcli_fields(l.trim());
        if f.len() >= 2 && f[1] == "802-11-wireless" {
            d.saved.push(f[0].clone());
        }
    }
    d
}

struct NetworkSource;
impl Source for NetworkSource {
    fn fetch(&mut self) -> Data {
        Box::new(parse_network(&run_sh(
            "network",
            "nmcli radio wifi 2>/dev/null; echo ---; nmcli -t -f TYPE,STATE device 2>/dev/null; echo ---; \
             nmcli -t -f IN-USE,SIGNAL,SECURITY,SSID dev wifi list --rescan no 2>/dev/null; echo ---; \
             nmcli -t -f NAME,TYPE connection show 2>/dev/null",
        )))
    }
}

/// The networks shown in the list at most (then Hidden network and Search again).
const MAX_NETS: usize = 6;
const H_ROW: u32 = 0;
const H_SWITCH: u32 = 1;
const H_MORE: u32 = 2;
const H_HIDDEN: u32 = 3;
const H_SEARCH: u32 = 4;
const H_NET: u32 = 10;

#[derive(Default)]
pub struct NetworkCard {
    pub data: Option<NetworkData>,
    pub list: List,
}

impl NetworkCard {
    fn rows(&self) -> usize {
        self.data
            .as_ref()
            .map_or(0, |d| d.networks.len().min(MAX_NETS) + 2)
    }
    fn has_list(&self) -> bool {
        self.data.as_ref().is_some_and(|d| d.wifi_device)
    }
    fn list_open(&self, cx: &Cx) -> bool {
        cx.size == Size::M || self.list.open
    }
}

/// The command that joins `n`: a saved network by its connection; an open one at once; a new
/// secured one after the password is asked in the edge's own card, handed to nmcli on its standard
/// input (never on a command line, where other programs could read it).
pub fn join_command(n: &Wlan, saved: &[String]) -> String {
    let ssid = sq(&n.ssid);
    if saved.contains(&n.ssid) {
        format!("nmcli connection up id {ssid} >/dev/null 2>&1")
    } else if !n.secure {
        format!("nmcli dev wifi connect {ssid} >/dev/null 2>&1")
    } else {
        let title = sq(&format!("Password for {}", n.ssid));
        format!(
            "pw=$(infynitae-edge ask --text --secret {title}) || exit 0; \
             nmcli connection add type wifi con-name {ssid} ssid {ssid} wifi-sec.key-mgmt wpa-psk >/dev/null 2>&1 \
             && printf '802-11-wireless-security.psk:%s\\n' \"$pw\" | nmcli connection up id {ssid} passwd-file /dev/stdin >/dev/null 2>&1; \
             unset pw"
        )
    }
}

impl Card for NetworkCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "network", data.downcast_ref::<NetworkData>()) else {
            return false;
        };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
    }
    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if !d.service => Status::Missing("NetworkManager is not running".into()),
            Some(_) => Status::Ready,
        }
    }
    fn height(&self, cx: &Cx, _: f64) -> f64 {
        let mut h = ui::ROW_H;
        if self.has_list() {
            h += if cx.size == Size::M {
                self.rows() as f64 * ui::LIST_ROW_H + 8.0
            } else {
                self.list.height(self.rows())
            };
        }
        h
    }
    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else { return };
        let (word, detail, on) = d.state();
        let open = self.list_open(cx);
        let (row_r, list_r) = if !self.has_list() {
            (body, (0.0, 0.0, 0.0, 0.0))
        } else if cx.size == Size::M {
            let lh = body.3 - ui::ROW_H;
            (
                (body.0, body.1, body.2, ui::ROW_H),
                (body.0, body.1 + ui::ROW_H, body.2, lh),
            )
        } else {
            self.list.split(body, self.rows(), cx.list_above)
        };
        let right = if self.has_list() {
            30.0 + 8.0 + ui::MORE_W + 4.0
        } else {
            0.0
        };
        let lit = ui::row(
            cr,
            cx,
            hits,
            H_ROW,
            row_r,
            &Row {
                icon: Some(d.icon()),
                name: &word,
                state: &detail,
                on,
                right,
            },
        );
        if self.has_list() {
            let x_right = row_r.0 + row_r.2 - 8.0;
            ui::switch(cr, cx, hits, H_SWITCH, row_r, x_right, d.radio_on);
            if cx.size != Size::M {
                ui::more(cr, cx, hits, H_MORE, row_r, x_right - 30.0 - 8.0, lit, open);
            }
        }
        if list_r.3 > 1.0 {
            cr.save().ok();
            cr.rectangle(list_r.0, list_r.1, list_r.2, list_r.3);
            cr.clip();
            ui::list_box(cr, cx, list_r);
            let mut y = list_r.1 + 4.0;
            let w = list_r.2;
            for (i, n) in d.networks.iter().take(MAX_NETS).enumerate() {
                let note = if n.joined {
                    "Connected".to_string()
                } else {
                    format!("{}%", n.signal)
                };
                ui::list_row(
                    cr,
                    cx,
                    hits,
                    H_NET + i as u32,
                    (list_r.0, y, w, ui::LIST_ROW_H),
                    &n.ssid,
                    &note,
                    n.joined,
                    n.secure.then_some(Icon::Lock),
                );
                y += ui::LIST_ROW_H;
            }
            ui::list_row(
                cr,
                cx,
                hits,
                H_HIDDEN,
                (list_r.0, y, w, ui::LIST_ROW_H),
                "Hidden network",
                "",
                false,
                None,
            );
            y += ui::LIST_ROW_H;
            ui::list_row(
                cr,
                cx,
                hits,
                H_SEARCH,
                (list_r.0, y, w, ui::LIST_ROW_H),
                "Search again",
                "",
                false,
                None,
            );
            cr.restore().ok();
        }
    }
    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Some(d) = self.data.as_mut() else {
            return false;
        };
        let Event::Press { hit, button, .. } = ev else {
            return false;
        };
        match (*hit, *button) {
            (H_SWITCH, Button::Left) if d.wifi_device => {
                d.radio_on = !d.radio_on;
                out.push(Action::Run(format!(
                    "nmcli radio wifi {} >/dev/null 2>&1",
                    if d.radio_on { "on" } else { "off" }
                )));
                true
            }
            (H_MORE, Button::Left) | (H_ROW, Button::Right)
                if d.wifi_device && cx.size != Size::M =>
            {
                self.list.toggle();
                true
            }
            (H_ROW, Button::Left) if d.wifi_device && cx.size != Size::M => {
                self.list.toggle();
                true
            }
            (H_HIDDEN, Button::Left) => {
                out.push(Action::Run(
                    "ssid=$(infynitae-edge ask --text 'Name of the hidden network') || exit 0; \
                     [ -n \"$ssid\" ] || exit 0; \
                     pw=$(infynitae-edge ask --text --secret \"Password for $ssid\") || exit 0; \
                     nmcli connection add type wifi con-name \"$ssid\" ssid \"$ssid\" wifi.hidden yes wifi-sec.key-mgmt wpa-psk >/dev/null 2>&1 \
                     && printf '802-11-wireless-security.psk:%s\\n' \"$pw\" | nmcli connection up id \"$ssid\" passwd-file /dev/stdin >/dev/null 2>&1; \
                     unset pw"
                        .into(),
                ));
                true
            }
            (H_SEARCH, Button::Left) => {
                out.push(Action::Run(
                    "nmcli dev wifi list --rescan yes >/dev/null 2>&1".into(),
                ));
                true
            }
            (h, Button::Left) if h >= H_NET => {
                let i = (h - H_NET) as usize;
                let Some(n) = d.networks.get(i).cloned() else {
                    return false;
                };
                if n.joined {
                    out.push(Action::Run(format!(
                        "nmcli connection down id {} >/dev/null 2>&1",
                        sq(&n.ssid)
                    )));
                } else {
                    out.push(Action::Run(join_command(&n, &d.saved)));
                }
                true
            }
            _ => false,
        }
    }
    fn tick(&mut self, dt_ms: f64) -> bool {
        self.list.tick(dt_ms)
    }
    fn show_list(&mut self) {
        self.list.open = true;
        self.list.settle();
    }
}

// ---------------------------------------------------------------------------------------------
// Bluetooth

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub mac: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BluetoothData {
    /// a Bluetooth adapter answered
    pub adapter: bool,
    pub powered: bool,
    /// paired ones (connected first), then ones found by a search
    pub devices: Vec<Device>,
}

fn device_lines(s: &str) -> Vec<(String, String)> {
    s.lines()
        .filter_map(|l| l.trim().strip_prefix("Device "))
        .filter_map(|r| r.split_once(' '))
        .map(|(m, n)| (m.trim().to_string(), n.trim().to_string()))
        .filter(|(m, n)| !m.is_empty() && !n.is_empty())
        .collect()
}

/// Reads the bluetooth source's output: `bluetoothctl show`, `devices Paired`, `devices Connected`,
/// `devices` (all known, found ones too), apart by "---".
pub fn parse_bluetooth(out: &str) -> BluetoothData {
    let parts = sections(out);
    let sec = |i: usize| parts.get(i).map(String::as_str).unwrap_or("");
    let show = sec(0);
    let mut d = BluetoothData {
        adapter: show.contains("Controller "),
        powered: show.lines().any(|l| l.trim() == "Powered: yes"),
        devices: Vec::new(),
    };
    let connected: Vec<String> = device_lines(sec(2)).into_iter().map(|(m, _)| m).collect();
    for (mac, name) in device_lines(sec(1)) {
        let c = connected.contains(&mac);
        d.devices.push(Device {
            mac,
            name,
            paired: true,
            connected: c,
        });
    }
    for (mac, name) in device_lines(sec(3)) {
        // a device with no name of its own shows its address with dashes: not listed
        if d.devices.iter().any(|x| x.mac == mac) || name == mac.replace(':', "-") {
            continue;
        }
        d.devices.push(Device {
            mac,
            name,
            paired: false,
            connected: false,
        });
    }
    d.devices
        .sort_by(|a, b| b.connected.cmp(&a.connected).then(b.paired.cmp(&a.paired)));
    d
}

struct BluetoothSource;
impl Source for BluetoothSource {
    fn fetch(&mut self) -> Data {
        Box::new(parse_bluetooth(&run_sh(
            "bluetooth",
            "bluetoothctl show 2>/dev/null; echo ---; bluetoothctl devices Paired 2>/dev/null; echo ---; \
             bluetoothctl devices Connected 2>/dev/null; echo ---; bluetoothctl devices 2>/dev/null",
        )))
    }
}

const MAX_DEVICES: usize = 6;
const H_FIND: u32 = 5;
const H_DEV: u32 = 10;

#[derive(Default)]
pub struct BluetoothCard {
    pub data: Option<BluetoothData>,
    pub list: List,
    /// a search was asked: found devices are listed with Pair
    pub searching: bool,
}

impl BluetoothCard {
    fn shown(&self) -> Vec<&Device> {
        self.data.as_ref().map_or(Vec::new(), |d| {
            d.devices
                .iter()
                .filter(|x| x.paired || self.searching)
                .take(MAX_DEVICES)
                .collect()
        })
    }
    fn rows(&self) -> usize {
        self.shown().len() + 1
    }
}

impl Card for BluetoothCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "bluetooth", data.downcast_ref::<BluetoothData>()) else {
            return false;
        };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
    }
    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if !d.adapter => Status::Missing("This PC has no Bluetooth adapter".into()),
            Some(_) => Status::Ready,
        }
    }
    fn height(&self, cx: &Cx, _: f64) -> f64 {
        ui::ROW_H
            + if cx.size == Size::M {
                self.rows() as f64 * ui::LIST_ROW_H + 8.0
            } else {
                self.list.height(self.rows())
            }
    }
    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else { return };
        let connected: Vec<&str> = d
            .devices
            .iter()
            .filter(|x| x.connected)
            .map(|x| x.name.as_str())
            .collect();
        let saved = d
            .devices
            .iter()
            .filter(|x| x.paired && !x.connected)
            .count();
        let (word, detail) = if !d.powered {
            ("Off".to_string(), "Bluetooth is off".to_string())
        } else if !connected.is_empty() {
            (
                connected.join(", "),
                format!("{} connected, {saved} saved", connected.len()),
            )
        } else {
            ("No device connected".to_string(), format!("{saved} saved"))
        };
        let (row_r, list_r) = if cx.size == Size::M {
            (
                (body.0, body.1, body.2, ui::ROW_H),
                (body.0, body.1 + ui::ROW_H, body.2, body.3 - ui::ROW_H),
            )
        } else {
            self.list.split(body, self.rows(), cx.list_above)
        };
        let lit = ui::row(
            cr,
            cx,
            hits,
            H_ROW,
            row_r,
            &Row {
                icon: Some(if d.powered {
                    Icon::Bluetooth
                } else {
                    Icon::BluetoothOff
                }),
                name: &word,
                state: &detail,
                on: d.powered && !connected.is_empty(),
                right: 30.0 + 8.0 + ui::MORE_W + 4.0,
            },
        );
        let x_right = row_r.0 + row_r.2 - 8.0;
        ui::switch(cr, cx, hits, H_SWITCH, row_r, x_right, d.powered);
        if cx.size != Size::M {
            ui::more(
                cr,
                cx,
                hits,
                H_MORE,
                row_r,
                x_right - 38.0,
                lit,
                self.list.open,
            );
        }
        if list_r.3 > 1.0 {
            cr.save().ok();
            cr.rectangle(list_r.0, list_r.1, list_r.2, list_r.3);
            cr.clip();
            ui::list_box(cr, cx, list_r);
            let mut y = list_r.1 + 4.0;
            for (i, dev) in self.shown().iter().enumerate() {
                let note = if dev.connected {
                    "Connected"
                } else if dev.paired {
                    "Paired"
                } else {
                    "Pair"
                };
                ui::list_row(
                    cr,
                    cx,
                    hits,
                    H_DEV + i as u32,
                    (list_r.0, y, list_r.2, ui::LIST_ROW_H),
                    &dev.name,
                    note,
                    dev.connected,
                    None,
                );
                y += ui::LIST_ROW_H;
            }
            let find = if self.searching {
                "Looking for devices nearby"
            } else {
                "Find new devices"
            };
            ui::list_row(
                cr,
                cx,
                hits,
                H_FIND,
                (list_r.0, y, list_r.2, ui::LIST_ROW_H),
                find,
                "",
                false,
                None,
            );
            cr.restore().ok();
        }
    }
    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Event::Press { hit, button, .. } = ev else {
            return false;
        };
        let shown: Vec<Device> = self.shown().into_iter().cloned().collect();
        let Some(d) = self.data.as_mut() else {
            return false;
        };
        match (*hit, *button) {
            (H_SWITCH, Button::Left) => {
                d.powered = !d.powered;
                out.push(Action::Run(format!(
                    "bluetoothctl power {} >/dev/null 2>&1",
                    if d.powered { "on" } else { "off" }
                )));
                true
            }
            (H_MORE, Button::Left) | (H_ROW, _) if cx.size != Size::M => {
                self.list.toggle();
                true
            }
            (H_FIND, Button::Left) if d.powered => {
                self.searching = true;
                out.push(Action::Run(
                    "bluetoothctl --timeout 15 scan on >/dev/null 2>&1".into(),
                ));
                true
            }
            (h, Button::Left) if h >= H_DEV => {
                let Some(dev) = shown.get((h - H_DEV) as usize) else {
                    return false;
                };
                let mac = sq(&dev.mac);
                out.push(Action::Run(if dev.connected {
                    format!("bluetoothctl disconnect {mac} >/dev/null 2>&1")
                } else if dev.paired {
                    format!("bluetoothctl connect {mac} >/dev/null 2>&1")
                } else {
                    format!(
                        "bluetoothctl pair {mac} >/dev/null 2>&1 && bluetoothctl trust {mac} >/dev/null 2>&1 \
                         && bluetoothctl connect {mac} >/dev/null 2>&1"
                    )
                }));
                true
            }
            _ => false,
        }
    }
    fn tick(&mut self, dt_ms: f64) -> bool {
        self.list.tick(dt_ms)
    }
    fn show_list(&mut self) {
        self.list.open = true;
        self.list.settle();
    }
}

// ---------------------------------------------------------------------------------------------
// Power mode

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerData {
    /// empty: no power-profiles-daemon
    pub modes: Vec<String>,
    pub active: String,
}

/// `powerprofilesctl list`: a mode per "name:" line, the active one starred.
pub fn parse_power(out: &str) -> PowerData {
    let mut d = PowerData::default();
    for l in out.lines() {
        let t = l.trim();
        let (star, rest) = match t.strip_prefix('*') {
            Some(r) => (true, r.trim()),
            None => (false, t),
        };
        if let Some(name) = rest.strip_suffix(':') {
            if !name.is_empty() && !name.contains(' ') {
                d.modes.push(name.to_string());
                if star {
                    d.active = name.to_string();
                }
            }
        }
    }
    // saver first, performance last
    let order = |m: &str| match m {
        "power-saver" => 0,
        "balanced" => 1,
        "performance" => 2,
        _ => 3,
    };
    d.modes.sort_by_key(|m| order(m));
    d
}

/// What a mode does, in one line under the choice.
pub fn mode_says(m: &str) -> &'static str {
    match m {
        "power-saver" => "Quieter and cooler, slower",
        "balanced" => "Speed when it is needed",
        "performance" => "Full speed, more heat and noise",
        _ => "",
    }
}

pub fn mode_word(m: &str) -> &str {
    match m {
        "power-saver" => "Saver",
        "balanced" => "Balanced",
        "performance" => "Performance",
        other => other,
    }
}

struct PowerSource;
impl Source for PowerSource {
    fn fetch(&mut self) -> Data {
        Box::new(parse_power(&run_sh(
            "power",
            "powerprofilesctl list 2>/dev/null",
        )))
    }
}

#[derive(Default)]
pub struct PowerCard {
    pub data: Option<PowerData>,
}

impl Card for PowerCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "power", data.downcast_ref::<PowerData>()) else {
            return false;
        };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
    }
    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if d.modes.is_empty() => Status::Missing("This PC has no power modes".into()),
            Some(_) => Status::Ready,
        }
    }
    fn height(&self, _: &Cx, _: f64) -> f64 {
        34.0 + 8.0 + 16.0
    }
    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else { return };
        let words: Vec<&str> = d.modes.iter().map(|m| mode_word(m)).collect();
        let chosen = d
            .modes
            .iter()
            .position(|m| *m == d.active)
            .unwrap_or(usize::MAX);
        ui::choice(
            cr,
            cx,
            hits,
            0,
            (body.0 + 8.0, body.1, body.2 - 16.0, 34.0),
            &words,
            chosen,
        );
        crate::draw::text_max(
            cr,
            body.0 + 8.0,
            body.1 + 34.0 + 8.0,
            body.2 - 16.0,
            &crate::draw::Text {
                size: 11.0,
                bold: false,
                color: cx.theme.muted,
                text: mode_says(&d.active),
            },
        );
    }
    fn event(&mut self, ev: &Event, _: &Cx, out: &mut Vec<Action>) -> bool {
        let (
            Event::Press {
                hit,
                button: Button::Left,
                ..
            },
            Some(d),
        ) = (ev, &mut self.data)
        else {
            return false;
        };
        let Some(mode) = d.modes.get(*hit as usize).cloned() else {
            return false;
        };
        if mode == d.active {
            return false;
        }
        out.push(Action::Run(format!(
            "powerprofilesctl set {} >/dev/null 2>&1",
            sq(&mode)
        )));
        // shown at once; the fetch after the command confirms or corrects it
        d.active = mode;
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn network_output_from_a_real_machine() {
        let d = parse_network(
            "enabled\n---\nethernet:connected\nwifi:disconnected\nloopback:connected (externally)\n---\n\
             *:72:WPA2:Home\\:Net\n :40::Cafe Free\n :81:WPA1 WPA2:Neighbour 5G\n :20:WPA2:Home\\:Net\n---\n\
             Home\\:Net:802-11-wireless\nWired connection 1:802-3-ethernet\n",
        );
        assert!(d.service && d.wifi_device && d.radio_on && d.wired);
        assert_eq!(
            d.networks
                .iter()
                .map(|n| n.ssid.as_str())
                .collect::<Vec<_>>(),
            vec!["Home:Net", "Neighbour 5G", "Cafe Free"],
            "joined first, then by signal; a name seen twice once"
        );
        assert!(d.networks[0].secure && !d.networks[2].secure);
        assert_eq!(d.saved, vec!["Home:Net"]);
        assert_eq!(d.state(), ("Wired".into(), "Cable connected".into(), true));
        assert_eq!(d.icon(), Icon::Wired);
    }

    #[test]
    fn network_states_and_icons() {
        let mut d = parse_network("enabled\n---\nwifi:connected\n---\n*:50:WPA2:Home\n");
        assert_eq!(
            (d.state().0.as_str(), d.state().1.as_str(), d.icon()),
            ("Home", "Wi-Fi, signal 50%", Icon::Wifi(2))
        );
        d.networks.clear();
        assert_eq!(
            (d.state().0.as_str(), d.icon()),
            ("Not connected", Icon::WifiSearching)
        );
        d.radio_on = false;
        assert_eq!((d.state().0.as_str(), d.icon()), ("Off", Icon::WifiOff));
        let none = parse_network("");
        assert!(
            !none.service,
            "nothing answered: NetworkManager is not running"
        );
        let wired_only = parse_network("enabled\n---\nethernet:connected\nwifi:unavailable\n");
        assert!(
            !wired_only.wifi_device,
            "the owner's PC: no Wi-Fi device, no switch, no list"
        );
    }

    #[test]
    fn joining_never_puts_a_password_on_a_command_line() {
        let open = Wlan {
            ssid: "Cafe".into(),
            signal: 40,
            secure: false,
            joined: false,
        };
        assert_eq!(
            join_command(&open, &[]),
            "nmcli dev wifi connect 'Cafe' >/dev/null 2>&1"
        );
        let saved = Wlan {
            ssid: "Home's".into(),
            signal: 70,
            secure: true,
            joined: false,
        };
        assert_eq!(
            join_command(&saved, &["Home's".into()]),
            "nmcli connection up id 'Home'\\''s' >/dev/null 2>&1"
        );
        let new = join_command(
            &Wlan {
                ssid: "New".into(),
                signal: 60,
                secure: true,
                joined: false,
            },
            &[],
        );
        assert!(new.contains("ask --text --secret") && new.contains("passwd-file /dev/stdin"));
        assert!(
            !new.contains("password "),
            "no `nmcli ... password X`: {new}"
        );
    }

    #[test]
    fn bluetooth_output_from_a_real_machine() {
        let d = parse_bluetooth(
            "Controller 00:1A:7D:DA:71:13 (public)\n\tPowered: yes\n---\nDevice 4C:87:5D:11:22:33 Headphones\nDevice AA:BB:CC:DD:EE:FF Controller\n---\n\
             Device 4C:87:5D:11:22:33 Headphones\n---\nDevice 4C:87:5D:11:22:33 Headphones\nDevice AA:BB:CC:DD:EE:FF Controller\n\
             Device 11:22:33:44:55:66 Phone\nDevice 77:88:99:AA:BB:CC 77-88-99-AA-BB-CC\n",
        );
        assert!(d.adapter && d.powered);
        assert_eq!(
            d.devices
                .iter()
                .map(|x| (x.name.as_str(), x.paired, x.connected))
                .collect::<Vec<_>>(),
            vec![
                ("Headphones", true, true),
                ("Controller", true, false),
                ("Phone", false, false)
            ],
            "connected first; one without a name left out"
        );
        assert!(!parse_bluetooth("").adapter);
    }

    #[test]
    fn power_modes_in_order_and_a_click_sets_one() {
        let d = parse_power("  performance:\n    CpuDriver:\tamd_pstate\n\n* balanced:\n    CpuDriver:\tamd_pstate\n\n  power-saver:\n    Degraded:  no\n");
        assert_eq!(d.modes, vec!["power-saver", "balanced", "performance"]);
        assert_eq!(d.active, "balanced");
        let mut c = PowerCard { data: Some(d) };
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: 2,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Run(
                "powerprofilesctl set 'performance' >/dev/null 2>&1".into()
            )]
        );
        assert_eq!(
            c.data.as_ref().unwrap().active,
            "performance",
            "shown chosen at once"
        );
        let mut out = Vec::new();
        assert!(
            !c.event(
                &Event::Press {
                    hit: 2,
                    button: Button::Left,
                    at: (0.0, 0.0)
                },
                &cx,
                &mut out
            ),
            "the chosen one again: nothing"
        );
        assert_eq!(
            PowerCard {
                data: Some(PowerData::default())
            }
            .status(&cx),
            Status::Missing("This PC has no power modes".into())
        );
    }

    #[test]
    fn the_network_list_opens_on_more_and_the_card_grows() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = NetworkCard::default();
        c.apply(
            "network",
            &parse_network("enabled\n---\nwifi:connected\n---\n*:50:WPA2:Home\n :30::Cafe\n")
                as &dyn Any,
        );
        let h0 = c.height(&cx, 300.0);
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: H_MORE,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx,
            &mut out
        ));
        while c.tick(16.0) {}
        assert_eq!(
            c.height(&cx, 300.0),
            h0 + 4.0 * ui::LIST_ROW_H + 8.0,
            "two networks, Hidden network, Search again"
        );
        let mut out = Vec::new();
        c.event(
            &Event::Press {
                hit: H_NET + 1,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        );
        assert_eq!(
            out,
            vec![Action::Run(
                "nmcli dev wifi connect 'Cafe' >/dev/null 2>&1".into()
            )]
        );
    }

    pub fn test_cx(th: &crate::theme::Theme) -> Cx<'_> {
        Cx {
            theme: th,
            now: chrono::NaiveDate::from_ymd_opt(2026, 11, 13)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            size: Size::S,
            app: None,
            apps: &[],
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
        }
    }
}
