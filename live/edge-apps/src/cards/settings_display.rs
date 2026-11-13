//! Settings: Display (docs/EDGE_SETTINGS.md, `settings/display`): one block per screen
//! (resolution, refresh rate, scale, rotation) and the global "All screens" settings (VRR,
//! night light warmth, monitor order, zoom sharpness and scaling filter).

use std::any::Any;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::time::Duration;

use cairo::Context;

use super::settings::sq;
use super::settings_rows::{change_command, After, ConfigData, Control, Setting};
use crate::card::{run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Size};
use crate::ui::{self, List, Row};

pub const KIND: CardKind = CardKind {
    id: "display",
    title: "Display",
    about: "Resolution, refresh rate, scale and night light",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "screens"],
};

/// Global ("All screens") settings shared with `ConfigData` and `config/config`.
pub const TABLE: &[Setting] = &[
    Setting {
        name: "Variable refresh rate",
        says: "",
        key: "VRR_ENABLED",
        control: Control::Pick(&[("In fullscreen", "auto"), ("On", "on"), ("Off", "off")]),
        after: After::Restart,
        more: false,
        needs: &[],
    },
    Setting {
        name: "Night light warmth",
        says: "",
        key: "NIGHT_LIGHT_TEMP",
        control: Control::Slide {
            min: 3000.0,
            max: 6500.0,
            step: 100.0,
            unit: " K",
        },
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        name: "Order, left to right",
        says: "names with commas",
        key: "OUTPUT_ORDER",
        control: Control::Text,
        after: After::Restart,
        more: true,
        needs: &[],
    },
    Setting {
        name: "Sharper when zoomed in",
        says: "",
        key: "MAX_APP_SCALE",
        control: Control::Pick(&[("Never", "1"), ("2x", "2"), ("3x", "3")]),
        after: After::Save,
        more: true,
        needs: &[],
    },
    Setting {
        name: "Zoomed windows",
        says: "",
        key: "SCALING_FILTER",
        control: Control::Pick(&[("Sharp", "nearest"), ("Smooth", "linear"), ("Fast", "fast")]),
        after: After::Restart,
        more: true,
        needs: &[],
    },
];

const SCALE_CHOICES: &[(&str, &str)] = &[("1", "1"), ("1.25", "1.25"), ("1.5", "1.5"), ("2", "2")];

const TRANSFORM_CHOICES: &[(&str, &str)] = &[
    ("Not turned", "normal"),
    ("90", "90"),
    ("180", "180"),
    ("270", "270"),
];

const SUB_ONE_H: f64 = 26.0;
const SUB_TWO_H: f64 = 42.0;
const SECTION_GAP: f64 = 6.0;
const SLIDER_ROW_H: f64 = 48.0;
pub const KEYS: &[&str] = &["OUTPUT_SCALE"];
const H_MORE: u32 = 1;
const H_ROW_BASE: u32 = 100;
const H_OPT_BASE: u32 = 10_000;

// ---------------------------------------------------------------------------------------------
// Source: `screens`

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeInfo {
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub preferred: bool,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenInfo {
    pub name: String,
    pub make: String,
    pub model: String,
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub scale: f64,
    pub transform: String,
    pub modes: Vec<ModeInfo>,
    pub adaptive_sync: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScreensData {
    pub screens: Vec<ScreenInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RateChoice {
    mhz: u32,
    value: String,
    label: String,
}

impl ScreenInfo {
    pub fn has_wlr_lists(&self) -> bool {
        !self.modes.is_empty()
    }

    pub fn title(&self) -> String {
        if self.width > 0 && self.height > 0 && self.refresh_mhz > 0 {
            format!(
                "{}  {}x{}  {}",
                self.name,
                self.width,
                self.height,
                fmt_hz(self.refresh_mhz)
            )
        } else if self.width > 0 && self.height > 0 {
            format!("{}  {}x{}", self.name, self.width, self.height)
        } else {
            self.name.clone()
        }
    }

    pub fn subtitle(&self) -> String {
        let make = clean_known(&self.make);
        let model = clean_known(&self.model);
        match (make, model) {
            (Some(mk), Some(md)) => {
                if md.to_lowercase().contains(&mk.to_lowercase()) {
                    md.to_string()
                } else {
                    format!("{mk} {md}")
                }
            }
            (None, Some(md)) => md.to_string(),
            (Some(mk), None) => mk.to_string(),
            (None, None) => String::new(),
        }
    }

    fn active_resolution(&self) -> (u32, u32) {
        if self.width > 0 && self.height > 0 {
            return (self.width, self.height);
        }
        self.modes
            .iter()
            .find(|m| m.current)
            .or_else(|| self.modes.iter().find(|m| m.preferred))
            .or_else(|| self.modes.first())
            .map(|m| (m.width, m.height))
            .unwrap_or((0, 0))
    }

    fn resolutions(&self) -> Vec<String> {
        let mut out = Vec::new();
        for m in &self.modes {
            if m.width == 0 || m.height == 0 {
                continue;
            }
            let s = format!("{}x{}", m.width, m.height);
            if !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    fn rates_for(&self, width: u32, height: u32) -> Vec<RateChoice> {
        let mut out: Vec<RateChoice> = Vec::new();
        for m in &self.modes {
            if m.width != width || m.height != height || m.refresh_mhz == 0 {
                continue;
            }
            let value = fmt_rate_num(m.refresh_mhz);
            if out.iter().any(|r| r.value == value) {
                continue;
            }
            out.push(RateChoice {
                mhz: m.refresh_mhz,
                label: format!("{value} Hz"),
                value,
            });
        }
        out
    }
}

impl ScreensData {
    pub fn has_wlr_lists(&self) -> bool {
        self.screens.iter().any(ScreenInfo::has_wlr_lists)
    }

    pub fn any_adaptive_sync(&self) -> bool {
        self.screens.iter().any(|s| s.adaptive_sync == Some(true))
    }
}

pub struct ScreensSource;

impl Source for ScreensSource {
    fn every(&self) -> Option<Duration> {
        None
    }

    fn fetch(&mut self) -> Data {
        let raw = run_sh(
            "screens",
            "infynitae msg snapshot 2>/dev/null; \
             printf '\\n===WLR_RANDR===\\n'; \
             command -v wlr-randr >/dev/null && wlr-randr --json 2>/dev/null || true",
        );
        Box::new(parse_screens(&raw))
    }
}

fn clean_known(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("unknown") {
        None
    } else {
        Some(t)
    }
}

pub fn screen_key_prefix(name: &str) -> String {
    format!("OUTPUT_{}", name.replace('-', "_"))
}

fn fmt_rate_num(mhz: u32) -> String {
    let centihz = (u64::from(mhz) + 5) / 10;
    if centihz % 100 == 0 {
        format!("{}", centihz / 100)
    } else if centihz % 10 == 0 {
        format!("{}.{}", centihz / 100, (centihz % 100) / 10)
    } else {
        format!("{}.{:02}", centihz / 100, centihz % 100)
    }
}

fn fmt_hz(mhz: u32) -> String {
    format!("{} Hz", fmt_rate_num(mhz))
}

fn fmt_scale(scale: f64) -> String {
    if !scale.is_finite() || scale <= 0.0 {
        return "1".to_string();
    }
    let rounded = (scale * 100.0).round() as i64;
    if rounded % 100 == 0 {
        format!("{}", rounded / 100)
    } else if rounded % 10 == 0 {
        format!("{}.{}", rounded / 100, (rounded % 100).abs() / 10)
    } else {
        format!("{:.2}", scale)
    }
}

fn parse_scale(s: &str) -> Option<f64> {
    let v = s.trim().parse::<f64>().ok()?;
    v.is_finite().then_some(v)
}

fn scale_matches(a: &str, b: &str) -> bool {
    match (parse_scale(a), parse_scale(b)) {
        (Some(x), Some(y)) => (x - y).abs() < 0.005,
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

fn rate_matches(cfg_rate: &str, mhz: u32) -> bool {
    let trimmed = cfg_rate.trim().trim_end_matches("Hz").trim();
    if let Ok(hz) = trimmed.parse::<f64>() {
        if hz.is_finite() && hz > 0.0 {
            let cfg_mhz = (hz * 1000.0).round() as i64;
            return (cfg_mhz - i64::from(mhz)).abs() < 500;
        }
    }
    false
}

fn normalize_transform(s: &str) -> &str {
    let t = s.trim();
    if t.is_empty() || t == "0" || t.eq_ignore_ascii_case("normal") {
        "normal"
    } else {
        t
    }
}

/// Parses combined `infynitae msg snapshot` + `wlr-randr --json` output into `ScreensData`.
pub fn parse_screens(out: &str) -> ScreensData {
    let vals = extract_json_values(out);
    let mut snapshot_outputs: Option<Vec<SnapOutput>> = None;
    let mut wlr_outputs: Option<Vec<WlrOutput>> = None;

    for v in &vals {
        if snapshot_outputs.is_none() {
            if let Some(outs) = extract_snapshot_outputs(v) {
                snapshot_outputs = Some(outs);
                continue;
            }
        }
        if wlr_outputs.is_none() {
            if let Some(outs) = extract_wlr_outputs(v) {
                wlr_outputs = Some(outs);
            }
        }
    }

    let mut wlr_by_name = wlr_outputs.unwrap_or_default();

    let screens = match snapshot_outputs {
        Some(snaps) => snaps
            .into_iter()
            .filter(|s| !s.name.is_empty())
            .map(|s| {
                let wlr = take_wlr(&mut wlr_by_name, &s.name);
                let (wlr_make, wlr_model, modes, adaptive_sync, wlr_scale, wlr_transform) = wlr
                    .map(|w| {
                        (
                            w.make,
                            w.model,
                            w.modes,
                            w.adaptive_sync,
                            w.scale,
                            w.transform,
                        )
                    })
                    .unwrap_or_default();
                ScreenInfo {
                    name: s.name,
                    make: if clean_known(&s.make).is_some() {
                        s.make
                    } else {
                        wlr_make
                    },
                    model: if clean_known(&s.model).is_some() {
                        s.model
                    } else {
                        wlr_model
                    },
                    width: s.width,
                    height: s.height,
                    refresh_mhz: s.refresh_mhz,
                    scale: if s.scale > 0.0 { s.scale } else { wlr_scale },
                    transform: if !s.transform.is_empty() {
                        s.transform
                    } else if !wlr_transform.is_empty() {
                        wlr_transform
                    } else {
                        "normal".to_string()
                    },
                    modes,
                    adaptive_sync,
                }
            })
            .collect(),
        None => wlr_by_name
            .into_iter()
            .filter(|w| !w.name.is_empty() && w.enabled != Some(false))
            .map(|w| {
                let cur = w
                    .modes
                    .iter()
                    .find(|m| m.current)
                    .or_else(|| w.modes.iter().find(|m| m.preferred))
                    .or_else(|| w.modes.first());
                ScreenInfo {
                    name: w.name,
                    make: w.make,
                    model: w.model,
                    width: cur.map_or(0, |m| m.width),
                    height: cur.map_or(0, |m| m.height),
                    refresh_mhz: cur.map_or(0, |m| m.refresh_mhz),
                    scale: if w.scale > 0.0 { w.scale } else { 1.0 },
                    transform: if w.transform.is_empty() {
                        "normal".to_string()
                    } else {
                        w.transform
                    },
                    modes: w.modes,
                    adaptive_sync: w.adaptive_sync,
                }
            })
            .collect(),
    };

    ScreensData { screens }
}

fn take_wlr(list: &mut Vec<WlrOutput>, name: &str) -> Option<WlrOutput> {
    let idx = list.iter().position(|w| w.name == name)?;
    Some(list.remove(idx))
}

#[derive(Default)]
struct SnapOutput {
    name: String,
    make: String,
    model: String,
    width: u32,
    height: u32,
    refresh_mhz: u32,
    scale: f64,
    transform: String,
}

#[derive(Default)]
struct WlrOutput {
    name: String,
    make: String,
    model: String,
    enabled: Option<bool>,
    scale: f64,
    transform: String,
    modes: Vec<ModeInfo>,
    adaptive_sync: Option<bool>,
}

fn extract_snapshot_outputs(v: &JsonVal) -> Option<Vec<SnapOutput>> {
    let root = v.get("snapshot").unwrap_or(v);
    let arr = root.get("outputs")?.as_arr()?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let name = item.get("name").and_then(JsonVal::as_str)?.to_string();
        let make = item
            .get("make")
            .and_then(JsonVal::as_str)
            .unwrap_or_default()
            .to_string();
        let model = item
            .get("model")
            .and_then(JsonVal::as_str)
            .unwrap_or_default()
            .to_string();
        let (width, height) = item
            .get("mode")
            .and_then(JsonVal::as_arr)
            .and_then(|m| Some((m.first()?.as_u32()?, m.get(1)?.as_u32()?)))
            .unwrap_or((0, 0));
        let refresh_mhz = item
            .get("refresh_mhz")
            .and_then(JsonVal::as_u32)
            .unwrap_or(0);
        let scale = item
            .get("scale")
            .and_then(JsonVal::as_f64)
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or(1.0);
        let transform = item
            .get("transform")
            .and_then(JsonVal::as_str)
            .unwrap_or("normal")
            .to_string();
        out.push(SnapOutput {
            name,
            make,
            model,
            width,
            height,
            refresh_mhz,
            scale,
            transform,
        });
    }
    Some(out)
}

fn extract_wlr_outputs(v: &JsonVal) -> Option<Vec<WlrOutput>> {
    let arr = v.as_arr()?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let name = item.get("name").and_then(JsonVal::as_str)?;
        let mut modes = Vec::new();
        if let Some(ms) = item.get("modes").and_then(JsonVal::as_arr) {
            for m in ms {
                let width = m.get("width").and_then(JsonVal::as_u32).unwrap_or(0);
                let height = m.get("height").and_then(JsonVal::as_u32).unwrap_or(0);
                let refresh = m.get("refresh").and_then(JsonVal::as_f64).unwrap_or(0.0);
                let refresh_mhz = if refresh.is_finite() && refresh > 0.0 {
                    (refresh * 1000.0).round() as u32
                } else {
                    0
                };
                if width > 0 && height > 0 {
                    modes.push(ModeInfo {
                        width,
                        height,
                        refresh_mhz,
                        preferred: m.get("preferred").and_then(JsonVal::as_bool) == Some(true),
                        current: m.get("current").and_then(JsonVal::as_bool) == Some(true),
                    });
                }
            }
        }
        let adaptive_sync = match item.get("adaptive_sync") {
            Some(JsonVal::Bool(b)) => Some(*b),
            Some(JsonVal::Str(s)) if s.eq_ignore_ascii_case("enabled") || s == "true" => Some(true),
            Some(JsonVal::Str(s)) if s.eq_ignore_ascii_case("disabled") || s == "false" => {
                Some(false)
            }
            _ => None,
        };
        out.push(WlrOutput {
            name: name.to_string(),
            make: item
                .get("make")
                .and_then(JsonVal::as_str)
                .unwrap_or_default()
                .to_string(),
            model: item
                .get("model")
                .and_then(JsonVal::as_str)
                .unwrap_or_default()
                .to_string(),
            enabled: item.get("enabled").and_then(JsonVal::as_bool),
            scale: item
                .get("scale")
                .and_then(JsonVal::as_f64)
                .filter(|s| s.is_finite() && *s > 0.0)
                .unwrap_or(1.0),
            transform: item
                .get("transform")
                .and_then(JsonVal::as_str)
                .unwrap_or("normal")
                .to_string(),
            modes,
            adaptive_sync,
        });
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// Minimal RFC 8259 JSON parser (self-contained, nesting limited, never panics)

const MAX_JSON_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq)]
enum JsonVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(Vec<(String, JsonVal)>),
}

impl JsonVal {
    fn get(&self, key: &str) -> Option<&JsonVal> {
        match self {
            JsonVal::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            JsonVal::Str(s) => Some(s),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            JsonVal::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            JsonVal::Num(n) => Some(*n),
            _ => None,
        }
    }

    fn as_u32(&self) -> Option<u32> {
        let n = self.as_f64()?;
        (n.is_finite() && n >= 0.0 && n <= f64::from(u32::MAX)).then_some(n.round() as u32)
    }

    fn as_arr(&self) -> Option<&[JsonVal]> {
        match self {
            JsonVal::Arr(a) => Some(a),
            _ => None,
        }
    }
}

fn extract_json_values(input: &str) -> Vec<JsonVal> {
    let mut vals = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && !matches!(bytes[i], b'{' | b'[') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let mut p = JsonParser { s: bytes, i };
        match p.value(0) {
            Ok(v) => {
                vals.push(v);
                i = p.i;
            }
            Err(()) => {
                // Skip past this delimiter or malformed line so a later JSON blob still parses.
                i += 1;
            }
        }
    }
    vals
}

struct JsonParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl JsonParser<'_> {
    fn ws(&mut self) {
        while self
            .s
            .get(self.i)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> Result<(), ()> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsonVal, ()> {
        if depth > MAX_JSON_DEPTH {
            return Err(());
        }
        self.ws();
        match self.s.get(self.i) {
            Some(b'n') => self.eat(b"null").map(|()| JsonVal::Null),
            Some(b't') => self.eat(b"true").map(|()| JsonVal::Bool(true)),
            Some(b'f') => self.eat(b"false").map(|()| JsonVal::Bool(false)),
            Some(b'"') => self.string().map(JsonVal::Str),
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(JsonVal::Arr(a));
                }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(JsonVal::Arr(a));
                        }
                        _ => return Err(()),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(JsonVal::Obj(m));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return Err(());
                    }
                    let k = self.string()?;
                    self.ws();
                    self.eat(b":")?;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    m.push((k, v));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(JsonVal::Obj(m));
                        }
                        _ => return Err(()),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(()),
        }
    }

    fn string(&mut self) -> Result<String, ()> {
        if self.s.get(self.i) != Some(&b'"') {
            return Err(());
        }
        self.i += 1;
        let mut out = String::new();
        while let Some(&b) = self.s.get(self.i) {
            if b == b'"' {
                self.i += 1;
                return Ok(out);
            }
            if b == b'\\' {
                self.i += 1;
                let esc = *self.s.get(self.i).ok_or(())?;
                self.i += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = std::str::from_utf8(self.s.get(self.i..self.i + 4).ok_or(())?)
                            .map_err(|_| ())?;
                        self.i += 4;
                        let code = u32::from_str_radix(hex, 16).map_err(|_| ())?;
                        let ch = char::from_u32(code).unwrap_or('\u{FFFD}');
                        out.push(ch);
                    }
                    _ => return Err(()),
                }
            } else if b < 0x20 {
                return Err(());
            } else {
                let s = std::str::from_utf8(&self.s[self.i..]).map_err(|_| ())?;
                let ch = s.chars().next().ok_or(())?;
                self.i += ch.len_utf8();
                out.push(ch);
            }
        }
        Err(())
    }

    fn number(&mut self) -> Result<JsonVal, ()> {
        let start = self.i;
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        let d0 = self.i;
        while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
            self.i += 1;
        }
        if self.i == d0 {
            return Err(());
        }
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            let f0 = self.i;
            while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
            }
            if self.i == f0 {
                return Err(());
            }
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            let e0 = self.i;
            while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
            }
            if self.i == e0 {
                return Err(());
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| ())?;
        let n = text.parse::<f64>().map_err(|_| ())?;
        n.is_finite().then_some(JsonVal::Num(n)).ok_or(())
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

fn settings_sh_cmd(sub: &str, args: &str) -> String {
    format!(
        "{{ [ -f \"$HOME/infynitae/live/settings.sh\" ] && sh \"$HOME/infynitae/live/settings.sh\" {sub} {args} || infynitae-settings {sub} {args}; }}"
    )
}

fn ask_cmd() -> &'static str {
    "if [ -x \"$HOME/.local/bin/infynitae-edge\" ]; then EDGE=\"$HOME/.local/bin/infynitae-edge\"; else EDGE=infynitae-edge; fi"
}

fn mode_change_command(
    output: &str,
    new_mode: &str,
    old_mode: &str,
    key: &str,
    old_cfg: Option<&str>,
    new_cfg: &str,
) -> String {
    let qo = sq(output);
    let qnew = sq(new_mode);
    let qold = sq(old_mode);
    let write = change_command(key, old_cfg, new_cfg, After::Restart);
    format!(
        "wlr-randr --output {qo} --mode {qnew} >/dev/null 2>&1 || exit 1; \
         {}; \
         ans=$(printf 'Keep\\nBack\\n' | timeout 10 \"$EDGE\" ask 'Keep this? 10' 2>/dev/null) || ans=''; \
         if [ \"$ans\" = 'Keep' ]; then {{ {write}; }}; \
         else wlr-randr --output {qo} --mode {qold} >/dev/null 2>&1 || true; fi",
        ask_cmd(),
    )
}

fn wlr_and_write_command(
    output: &str,
    flag: &str,
    arg: &str,
    key: &str,
    old_cfg: Option<&str>,
    new_cfg: &str,
) -> String {
    let write = change_command(key, old_cfg, new_cfg, After::Restart);
    format!(
        "wlr-randr --output {} {flag} {} >/dev/null 2>&1 && {{ {write}; }}",
        sq(output),
        sq(arg),
    )
}

fn text_change_command(key: &str, name: &str, old: &str) -> String {
    let qk = sq(key);
    let qo = sq(old);
    format!(
        "{}; v=$(\"$EDGE\" ask --text --default {qo} {} 2>/dev/null) || exit 0; \
         [ \"$v\" = {qo} ] && exit 0; \
         {} >/dev/null 2>&1; {} >/dev/null 2>&1",
        ask_cmd(),
        sq(name),
        settings_sh_cmd("log", &format!("{qk} {qo}")),
        settings_sh_cmd("set", &format!("{qk} \"$v\"")),
    )
}

// ---------------------------------------------------------------------------------------------
// DisplayCard row model & Card implementation

#[derive(Clone, Debug, PartialEq, Eq)]
enum RowId {
    ScreenRes(String),
    ScreenRate(String),
    ScreenScale(String),
    ScreenTransform(String),
    Global(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OptionItem {
    label: String,
    value: String,
    active: bool,
}

#[derive(Clone, Debug)]
enum RowAction {
    List(Vec<OptionItem>),
    Slide {
        min: i32,
        max: i32,
        step: i32,
        value: i32,
        unit: &'static str,
    },
    Text {
        setting_idx: usize,
    },
}

#[derive(Clone, Debug)]
struct DisplayRow {
    id: RowId,
    name: &'static str,
    key: String,
    search_extra: String,
    value_text: String,
    old_cfg: String,
    changed: bool,
    more: bool,
    current_mode_for_revert: String,
    active_res_prefix: String,
    wlr_live: bool,
    action: RowAction,
}

pub struct DisplayCard {
    cfg: Option<ConfigData>,
    screens: Option<ScreensData>,
    query: String,
    more: bool,
    open: Option<RowId>,
    list: List,
    restarted: BTreeSet<String>,
    pending_text_restart: Option<(String, String)>,
    sliding: Option<i32>,
    pending_night_light: Option<i32>,
    sliders: RefCell<Vec<(u32, Rect)>>,
}

impl Default for DisplayCard {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayCard {
    pub fn new() -> Self {
        Self {
            cfg: None,
            screens: None,
            query: String::new(),
            more: false,
            open: None,
            list: List::default(),
            restarted: BTreeSet::new(),
            pending_text_restart: None,
            sliding: None,
            pending_night_light: None,
            sliders: RefCell::new(Vec::new()),
        }
    }

    fn build_layout(&self, cx: &Cx) -> (Vec<DisplayRow>, Vec<(usize, String, String)>, bool) {
        let Some(cfg) = &self.cfg else {
            return (Vec::new(), Vec::new(), false);
        };
        let Some(screens) = &self.screens else {
            return (Vec::new(), Vec::new(), false);
        };
        let num_screens = screens.screens.len();
        let searching = !self.query.is_empty();
        let show_more = searching || self.more || cx.size != Size::S;
        let mut all_rows: Vec<DisplayRow> = Vec::new();
        let mut headers: Vec<(usize, String, String)> = Vec::new();
        let mut has_more_hidden = false;

        for s in &screens.screens {
            let prefix = screen_key_prefix(&s.name);
            let wlr = s.has_wlr_lists();
            let old_mode = if s.width > 0 && s.height > 0 && s.refresh_mhz > 0 {
                format!("{}x{}@{}Hz", s.width, s.height, fmt_rate_num(s.refresh_mhz))
            } else if s.width > 0 && s.height > 0 {
                format!("{}x{}", s.width, s.height)
            } else {
                String::new()
            };
            let (cur_w, cur_h) = s.active_resolution();
            let cur_res = if cur_w > 0 && cur_h > 0 {
                format!("{cur_w}x{cur_h}")
            } else {
                String::new()
            };

            let mut screen_rows = Vec::new();

            if wlr {
                // Resolution
                let res_key = format!("{prefix}_MODE");
                let cfg_res = cfg.value(&res_key).trim().to_string();
                let shown_res = if !cfg_res.is_empty() {
                    cfg_res.clone()
                } else {
                    cur_res.clone()
                };
                let opts = s
                    .resolutions()
                    .into_iter()
                    .map(|r| OptionItem {
                        active: r == shown_res,
                        label: r.clone(),
                        value: r,
                    })
                    .collect::<Vec<_>>();
                screen_rows.push(DisplayRow {
                    id: RowId::ScreenRes(s.name.clone()),
                    name: "Resolution",
                    key: res_key,
                    search_extra: format!("{} {}", s.name, s.subtitle()),
                    value_text: shown_res,
                    old_cfg: cfg_res.clone(),
                    changed: !cfg_res.is_empty(),
                    more: false,
                    current_mode_for_revert: old_mode.clone(),
                    active_res_prefix: cur_res.clone(),
                    wlr_live: true,
                    action: RowAction::List(opts),
                });

                // Refresh rate (hidden with 1 rate)
                let rates = s.rates_for(cur_w, cur_h);
                if rates.len() > 1 {
                    let rate_key = format!("{prefix}_RATE");
                    let cfg_rate = cfg.value(&rate_key).trim().to_string();
                    let matched_cfg = rates.iter().find(|r| rate_matches(&cfg_rate, r.mhz));
                    let matched_live = rates.iter().find(|r| r.mhz == s.refresh_mhz);
                    let shown_rate = if !cfg_rate.is_empty() {
                        matched_cfg
                            .map(|r| r.label.clone())
                            .unwrap_or_else(|| format!("{cfg_rate} Hz"))
                    } else if let Some(m) = matched_live {
                        m.label.clone()
                    } else if s.refresh_mhz > 0 {
                        fmt_hz(s.refresh_mhz)
                    } else {
                        String::new()
                    };
                    let opts = rates
                        .iter()
                        .map(|r| OptionItem {
                            active: if !cfg_rate.is_empty() {
                                rate_matches(&cfg_rate, r.mhz)
                            } else {
                                r.mhz == s.refresh_mhz
                            },
                            label: r.label.clone(),
                            value: r.value.clone(),
                        })
                        .collect::<Vec<_>>();
                    screen_rows.push(DisplayRow {
                        id: RowId::ScreenRate(s.name.clone()),
                        name: "Refresh rate",
                        key: rate_key,
                        search_extra: format!("{} {}", s.name, s.subtitle()),
                        value_text: shown_rate,
                        old_cfg: cfg_rate.clone(),
                        changed: !cfg_rate.is_empty(),
                        more: false,
                        current_mode_for_revert: old_mode.clone(),
                        active_res_prefix: cur_res.clone(),
                        wlr_live: true,
                        action: RowAction::List(opts),
                    });
                }
            }

            // Scale (always shown; with 1 screen writes OUTPUT_SCALE)
            let scale_key = if num_screens == 1 {
                "OUTPUT_SCALE".to_string()
            } else {
                format!("{prefix}_SCALE")
            };
            let cfg_scale = cfg.value(&scale_key).trim().to_string();
            let raw_scale = if !cfg_scale.is_empty() {
                cfg_scale.clone()
            } else if s.scale > 0.0 {
                fmt_scale(s.scale)
            } else {
                let global_scale = cfg.value("OUTPUT_SCALE").trim();
                if global_scale.is_empty() {
                    "1".to_string()
                } else {
                    global_scale.to_string()
                }
            };
            let matched_scale = SCALE_CHOICES
                .iter()
                .find(|(_, v)| scale_matches(&raw_scale, v));
            let shown_scale = matched_scale
                .map(|(l, _)| (*l).to_string())
                .unwrap_or_else(|| raw_scale.clone());
            let is_scale_changed = !scale_matches(&raw_scale, "1");
            let scale_opts = SCALE_CHOICES
                .iter()
                .map(|(l, v)| OptionItem {
                    active: scale_matches(&raw_scale, v),
                    label: (*l).to_string(),
                    value: (*v).to_string(),
                })
                .collect::<Vec<_>>();
            screen_rows.push(DisplayRow {
                id: RowId::ScreenScale(s.name.clone()),
                name: "Scale",
                key: scale_key,
                search_extra: format!("{} {}", s.name, s.subtitle()),
                value_text: shown_scale,
                old_cfg: cfg_scale,
                changed: is_scale_changed,
                more: false,
                current_mode_for_revert: String::new(),
                active_res_prefix: String::new(),
                wlr_live: wlr,
                action: RowAction::List(scale_opts),
            });

            // M Turned (hidden without wlr-randr lists)
            if wlr {
                let tr_key = format!("{prefix}_TRANSFORM");
                let cfg_tr = cfg.value(&tr_key).trim().to_string();
                let eff_tr = if !cfg_tr.is_empty() {
                    normalize_transform(&cfg_tr).to_string()
                } else {
                    normalize_transform(&s.transform).to_string()
                };
                let shown_tr = TRANSFORM_CHOICES
                    .iter()
                    .find(|(_, v)| v.eq_ignore_ascii_case(&eff_tr))
                    .map(|(l, _)| (*l).to_string())
                    .unwrap_or_else(|| eff_tr.clone());
                let tr_opts = TRANSFORM_CHOICES
                    .iter()
                    .map(|(l, v)| OptionItem {
                        active: v.eq_ignore_ascii_case(&eff_tr),
                        label: (*l).to_string(),
                        value: (*v).to_string(),
                    })
                    .collect::<Vec<_>>();
                has_more_hidden = true;
                screen_rows.push(DisplayRow {
                    id: RowId::ScreenTransform(s.name.clone()),
                    name: "Turned",
                    key: tr_key,
                    search_extra: format!("{} {}", s.name, s.subtitle()),
                    value_text: shown_tr,
                    old_cfg: cfg_tr,
                    changed: !eff_tr.eq_ignore_ascii_case("normal"),
                    more: true,
                    current_mode_for_revert: String::new(),
                    active_res_prefix: String::new(),
                    wlr_live: true,
                    action: RowAction::List(tr_opts),
                });
            }

            let matched_rows: Vec<DisplayRow> = screen_rows
                .into_iter()
                .filter(|r| (show_more || !r.more) && row_matches_query(r, &self.query))
                .collect();
            if !matched_rows.is_empty() {
                headers.push((all_rows.len(), s.title(), s.subtitle()));
                all_rows.extend(matched_rows);
            }
        }

        // Global ("All screens") rows from TABLE
        let any_screen_shown = !headers.is_empty();
        let mut global_rows: Vec<DisplayRow> = Vec::new();
        for (idx, st) in TABLE.iter().enumerate() {
            let cur = cfg.value(st.key).trim();
            if st.key == "VRR_ENABLED" {
                let can_vrr = screens.has_wlr_lists()
                    && (screens.any_adaptive_sync() || cur.eq_ignore_ascii_case("off"));
                if !can_vrr {
                    continue;
                }
            }
            if st.key == "OUTPUT_ORDER" && num_screens <= 1 {
                continue;
            }
            if st.more {
                has_more_hidden = true;
            }
            if !show_more && st.more {
                continue;
            }
            let def = cfg
                .default
                .get(st.key)
                .map(String::as_str)
                .unwrap_or("")
                .trim();
            let row = match st.control {
                Control::Pick(choices) => {
                    let opts = choices
                        .iter()
                        .map(|(l, v)| OptionItem {
                            active: v.eq_ignore_ascii_case(cur),
                            label: (*l).to_string(),
                            value: (*v).to_string(),
                        })
                        .collect::<Vec<_>>();
                    let shown = choices
                        .iter()
                        .find(|(_, v)| v.eq_ignore_ascii_case(cur))
                        .map(|(l, _)| (*l).to_string())
                        .unwrap_or_else(|| cur.to_string());
                    DisplayRow {
                        id: RowId::Global(st.key),
                        name: st.name,
                        key: st.key.to_string(),
                        search_extra: String::new(),
                        value_text: shown,
                        old_cfg: cur.to_string(),
                        changed: !cur.eq_ignore_ascii_case(def),
                        more: st.more,
                        current_mode_for_revert: String::new(),
                        active_res_prefix: String::new(),
                        wlr_live: false,
                        action: RowAction::List(opts),
                    }
                }
                Control::Slide {
                    min,
                    max,
                    step,
                    unit,
                } => {
                    let min_i = min.round() as i32;
                    let max_i = max.round() as i32;
                    let step_i = step.round().max(1.0) as i32;
                    let base_val = cur
                        .parse::<i32>()
                        .ok()
                        .or_else(|| def.parse::<i32>().ok())
                        .unwrap_or(min_i)
                        .clamp(min_i, max_i);
                    let eff_val = self
                        .sliding
                        .or(self.pending_night_light)
                        .unwrap_or(base_val)
                        .clamp(min_i, max_i);
                    let def_num = def.parse::<i32>().unwrap_or(min_i);
                    DisplayRow {
                        id: RowId::Global(st.key),
                        name: st.name,
                        key: st.key.to_string(),
                        search_extra: String::new(),
                        value_text: format!("{eff_val}{unit}"),
                        old_cfg: cur.to_string(),
                        changed: eff_val != def_num,
                        more: st.more,
                        current_mode_for_revert: String::new(),
                        active_res_prefix: String::new(),
                        wlr_live: false,
                        action: RowAction::Slide {
                            min: min_i,
                            max: max_i,
                            step: step_i,
                            value: eff_val,
                            unit,
                        },
                    }
                }
                Control::Text => {
                    let shown = if cur.is_empty() {
                        st.says.to_string()
                    } else {
                        cur.to_string()
                    };
                    DisplayRow {
                        id: RowId::Global(st.key),
                        name: st.name,
                        key: st.key.to_string(),
                        search_extra: st.says.to_string(),
                        value_text: shown,
                        old_cfg: cur.to_string(),
                        changed: cur != def,
                        more: st.more,
                        current_mode_for_revert: String::new(),
                        active_res_prefix: String::new(),
                        wlr_live: false,
                        action: RowAction::Text { setting_idx: idx },
                    }
                }
                _ => continue,
            };
            if row_matches_query(&row, &self.query) {
                global_rows.push(row);
            }
        }

        if any_screen_shown && !global_rows.is_empty() {
            headers.push((all_rows.len(), "All screens".to_string(), String::new()));
        }
        all_rows.extend(global_rows);

        (
            all_rows,
            headers,
            has_more_hidden && !searching && cx.size == Size::S,
        )
    }

    fn header_height(title: &str, subtitle: &str, first: bool) -> f64 {
        let gap = if first { 0.0 } else { SECTION_GAP };
        let h = if !subtitle.is_empty() {
            SUB_TWO_H
        } else if !title.is_empty() {
            SUB_ONE_H
        } else {
            0.0
        };
        gap + h
    }

    fn row_height(row: &DisplayRow) -> f64 {
        match row.action {
            RowAction::Slide { .. } => SLIDER_ROW_H,
            _ => ui::ROW_H,
        }
    }

    fn commit_option(&mut self, row: &DisplayRow, opt: &OptionItem, out: &mut Vec<Action>) -> bool {
        self.open = None;
        if self.list.open {
            self.list.toggle();
            self.list.settle();
        }
        if opt.active {
            return true;
        }
        let old_opt = (!row.old_cfg.is_empty()).then_some(row.old_cfg.as_str());
        let cmd = match &row.id {
            RowId::ScreenRes(name) => mode_change_command(
                name,
                &opt.value,
                &row.current_mode_for_revert,
                &row.key,
                old_opt,
                &opt.value,
            ),
            RowId::ScreenRate(name) => {
                let new_mode = if row.active_res_prefix.is_empty() {
                    opt.value.clone()
                } else {
                    format!("{}@{}Hz", row.active_res_prefix, opt.value)
                };
                mode_change_command(
                    name,
                    &new_mode,
                    &row.current_mode_for_revert,
                    &row.key,
                    old_opt,
                    &opt.value,
                )
            }
            RowId::ScreenScale(name) => {
                if row.wlr_live {
                    wlr_and_write_command(
                        name, "--scale", &opt.value, &row.key, old_opt, &opt.value,
                    )
                } else {
                    self.restarted.insert(row.key.clone());
                    change_command(&row.key, old_opt, &opt.value, After::Restart)
                }
            }
            RowId::ScreenTransform(name) => wlr_and_write_command(
                name,
                "--transform",
                &opt.value,
                &row.key,
                old_opt,
                &opt.value,
            ),
            RowId::Global(key) => {
                let Some(setting) = TABLE.iter().find(|s| s.key == *key) else {
                    return true;
                };
                if matches!(setting.after, After::Restart) {
                    self.restarted.insert(row.key.clone());
                }
                change_command(setting.key, old_opt, &opt.value, setting.after)
            }
        };
        out.push(Action::Run(cmd));
        true
    }
}

fn row_matches_query(row: &DisplayRow, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    row.name.to_lowercase().contains(&q)
        || row.key.to_lowercase().contains(&q)
        || row.value_text.to_lowercase().contains(&q)
        || row.search_extra.to_lowercase().contains(&q)
}

impl Card for DisplayCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        match key {
            "config" => {
                let Some(d) = data.downcast_ref::<ConfigData>() else {
                    return false;
                };
                let changed = self.cfg.as_ref() != Some(d);
                if changed {
                    self.pending_night_light = None;
                    if let Some((k, prev)) = self.pending_text_restart.take() {
                        if d.value(&k).trim() != prev {
                            self.restarted.insert(k);
                        }
                    }
                }
                self.cfg = Some(d.clone());
                changed
            }
            "screens" => {
                let Some(d) = data.downcast_ref::<ScreensData>() else {
                    return false;
                };
                let changed = self.screens.as_ref() != Some(d);
                self.screens = Some(d.clone());
                changed
            }
            _ => false,
        }
    }

    fn status(&self, cx: &Cx) -> Status {
        if self.cfg.is_none() || self.screens.is_none() {
            return Status::Loading;
        }
        let (rows, _, _) = self.build_layout(cx);
        if rows.is_empty() && !self.query.is_empty() {
            return Status::Hidden;
        }
        Status::Ready
    }

    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        let (rows, headers, has_more) = self.build_layout(cx);
        let mut h = 0.0;
        let mut header_idx = 0;
        let mut first = true;
        for (i, row) in rows.iter().enumerate() {
            while let Some((at, title, sub)) = headers.get(header_idx) {
                if *at != i {
                    break;
                }
                h += Self::header_height(title, sub, first);
                first = false;
                header_idx += 1;
            }
            first = false;
            h += Self::row_height(row);
            if self.open.as_ref() == Some(&row.id) {
                if let RowAction::List(opts) = &row.action {
                    h += self.list.height(opts.len());
                }
            }
        }
        if has_more {
            h += ui::ROW_H;
        }
        h
    }

    fn tick(&mut self, dt_ms: f64) -> bool {
        self.list.tick(dt_ms)
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let th = cx.theme;
        let (rows, headers, has_more) = self.build_layout(cx);
        let mut sliders = Vec::new();
        let mut y = body.1;
        let mut header_idx = 0;
        let mut first = true;

        for (i, row) in rows.iter().enumerate() {
            while let Some((at, title, sub)) = headers.get(header_idx) {
                if *at != i {
                    break;
                }
                if !first {
                    y += SECTION_GAP;
                }
                first = false;
                let max_w = (body.2 - 16.0).max(0.0);
                if !title.is_empty() {
                    draw::text_max(
                        cr,
                        body.0 + 8.0,
                        y + 6.0,
                        max_w,
                        &Text {
                            size: 12.5,
                            bold: true,
                            color: th.text,
                            text: title,
                        },
                    );
                }
                if !sub.is_empty() {
                    draw::text_max(
                        cr,
                        body.0 + 8.0,
                        y + 22.0,
                        max_w,
                        &Text {
                            size: 11.5,
                            bold: false,
                            color: th.muted,
                            text: sub,
                        },
                    );
                    y += SUB_TWO_H;
                } else if !title.is_empty() {
                    y += SUB_ONE_H;
                }
                header_idx += 1;
            }
            first = false;

            let id = H_ROW_BASE + i as u32;
            let rh = Self::row_height(row);
            let row_r: Rect = (body.0, y, body.2, rh);
            let restarted = self.restarted.contains(&row.key);
            let state_text = if restarted { "At the next start" } else { "" };

            match &row.action {
                RowAction::List(opts) => {
                    let value_color = if row.changed {
                        th.accent_text
                    } else {
                        th.muted
                    };
                    let val_t = Text {
                        size: 12.5,
                        bold: row.changed,
                        color: value_color,
                        text: &row.value_text,
                    };
                    let vw = if row.value_text.is_empty() {
                        0.0
                    } else {
                        draw::measure(cr, &val_t).0
                    };
                    let right_pad = (vw + 16.0).max(24.0);
                    let _lit = ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        row_r,
                        &Row {
                            icon: None,
                            name: row.name,
                            state: state_text,
                            on: row.changed,
                            right: right_pad,
                        },
                    );
                    if !row.value_text.is_empty() {
                        draw::text_right(
                            cr,
                            row_r.0 + row_r.2 - 8.0,
                            row_r.1 + (row_r.3 - 15.0) / 2.0,
                            &val_t,
                        );
                    }
                    y += rh;

                    if self.open.as_ref() == Some(&row.id) {
                        let lh = self.list.height(opts.len());
                        if lh > 0.0 {
                            let lr: Rect = (
                                body.0 + 8.0,
                                y + 2.0,
                                (body.2 - 16.0).max(0.0),
                                (lh - 4.0).max(0.0),
                            );
                            ui::list_box(cr, cx, lr);
                            for (oi, opt) in opts.iter().enumerate() {
                                let item_r: Rect = (
                                    lr.0,
                                    lr.1 + oi as f64 * ui::LIST_ROW_H,
                                    lr.2,
                                    ui::LIST_ROW_H,
                                );
                                ui::list_row(
                                    cr,
                                    cx,
                                    hits,
                                    H_OPT_BASE + oi as u32,
                                    item_r,
                                    &opt.label,
                                    "",
                                    opt.active,
                                    None,
                                );
                            }
                            y += lh;
                        }
                    }
                }
                RowAction::Slide {
                    min,
                    max,
                    value,
                    unit,
                    ..
                } => {
                    let x = body.0 + 8.0;
                    let w = (body.2 - 16.0).max(0.0);
                    draw::text_max(
                        cr,
                        x,
                        y + 4.0,
                        (w - 64.0).max(0.0),
                        &Text {
                            size: 13.0,
                            bold: false,
                            color: th.text,
                            text: row.name,
                        },
                    );
                    let val_str = format!("{value}{unit}");
                    draw::text_right(
                        cr,
                        x + w,
                        y + 4.0,
                        &Text {
                            size: 13.0,
                            bold: row.changed,
                            color: if row.changed {
                                th.accent_text
                            } else {
                                th.muted
                            },
                            text: &val_str,
                        },
                    );
                    let sr: Rect = (x, y + 26.0, w, 16.0);
                    let part = f64::from(*value - *min) / f64::from((*max - *min).max(1));
                    ui::slider(cr, cx, hits, id, sr, part.clamp(0.0, 1.0));
                    sliders.push((id, sr));
                    y += rh;
                }
                RowAction::Text { .. } => {
                    let empty_val = row.old_cfg.is_empty();
                    let color = if row.changed && !empty_val {
                        th.accent_text
                    } else {
                        th.muted
                    };
                    let val_t = Text {
                        size: 12.0,
                        bold: row.changed && !empty_val,
                        color,
                        text: &row.value_text,
                    };
                    let vw = draw::measure(cr, &val_t).0;
                    let right_pad = (vw + 16.0).min(body.2 * 0.5).max(40.0);
                    let _lit = ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        row_r,
                        &Row {
                            icon: None,
                            name: row.name,
                            state: state_text,
                            on: row.changed && !empty_val,
                            right: right_pad,
                        },
                    );
                    draw::text_right(
                        cr,
                        row_r.0 + row_r.2 - 8.0,
                        row_r.1 + (row_r.3 - 15.0) / 2.0,
                        &val_t,
                    );
                    y += rh;
                }
            }
        }

        if has_more {
            let mr: Rect = (body.0, y, body.2, ui::ROW_H);
            ui::row(
                cr,
                cx,
                hits,
                H_MORE,
                mr,
                &Row {
                    icon: None,
                    name: if self.more { "Less" } else { "More" },
                    state: "",
                    on: false,
                    right: 0.0,
                },
            );
        }

        *self.sliders.borrow_mut() = sliders;
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        if let Event::Text(q) = ev {
            if self.query == *q {
                return false;
            }
            self.query = q.to_string();
            return true;
        }

        let (rows, _, _) = self.build_layout(cx);

        match ev {
            Event::Press {
                hit: H_MORE,
                button: Button::Left,
                ..
            } => {
                self.more = !self.more;
                true
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } if *hit >= H_OPT_BASE => {
                let oi = (*hit - H_OPT_BASE) as usize;
                let Some(open_id) = self.open.clone() else {
                    return false;
                };
                let Some(row) = rows.iter().find(|r| r.id == open_id) else {
                    return false;
                };
                let RowAction::List(opts) = &row.action else {
                    return false;
                };
                let Some(opt) = opts.get(oi) else {
                    return false;
                };
                self.commit_option(row, opt, out)
            }
            Event::Press {
                hit,
                button: Button::Left,
                at,
            } if *hit >= H_ROW_BASE => {
                let idx = (*hit - H_ROW_BASE) as usize;
                let Some(row) = rows.get(idx) else {
                    return false;
                };
                match &row.action {
                    RowAction::List(_) => {
                        if self.open.as_ref() == Some(&row.id) {
                            self.open = None;
                            if self.list.open {
                                self.list.toggle();
                                self.list.settle();
                            }
                        } else {
                            self.open = Some(row.id.clone());
                            self.list = List::default();
                            self.list.toggle();
                        }
                        true
                    }
                    RowAction::Slide { min, max, step, .. } => {
                        let sr = self
                            .sliders
                            .borrow()
                            .iter()
                            .find(|(sid, _)| sid == hit)
                            .map(|(_, r)| *r);
                        let Some(r) = sr else { return false };
                        let part = ui::slider_part(r, *at);
                        let v = snap_slide(*min, *max, *step, part);
                        self.sliding = Some(v);
                        true
                    }
                    RowAction::Text { setting_idx } => {
                        let setting = &TABLE[*setting_idx];
                        if matches!(setting.after, After::Restart) {
                            self.pending_text_restart =
                                Some((row.key.clone(), row.old_cfg.clone()));
                        }
                        out.push(Action::Run(text_change_command(
                            setting.key,
                            setting.name,
                            &row.old_cfg,
                        )));
                        true
                    }
                }
            }
            Event::Drag { hit, at } if *hit >= H_ROW_BASE => {
                let idx = (*hit - H_ROW_BASE) as usize;
                let Some(row) = rows.get(idx) else {
                    return false;
                };
                if let RowAction::Slide { min, max, step, .. } = row.action {
                    let sr = self
                        .sliders
                        .borrow()
                        .iter()
                        .find(|(sid, _)| sid == hit)
                        .map(|(_, r)| *r);
                    let Some(r) = sr else { return false };
                    let v = snap_slide(min, max, step, ui::slider_part(r, *at));
                    if self.sliding != Some(v) {
                        self.sliding = Some(v);
                        return true;
                    }
                }
                false
            }
            Event::Release { .. } => {
                let Some(v) = self.sliding.take() else {
                    return false;
                };
                let Some(row) = rows
                    .iter()
                    .find(|r| matches!(r.action, RowAction::Slide { .. }))
                else {
                    return false;
                };
                let setting = &TABLE[1];
                let prev = row.old_cfg.parse::<i32>().ok();
                if prev != Some(v) {
                    let old_opt = (!row.old_cfg.is_empty()).then_some(row.old_cfg.as_str());
                    self.pending_night_light = Some(v);
                    out.push(Action::Run(change_command(
                        setting.key,
                        old_opt,
                        &v.to_string(),
                        setting.after,
                    )));
                }
                true
            }
            Event::Scroll { hit, dy } => {
                let Some(hid) = hit else { return false };
                if *hid < H_ROW_BASE {
                    return false;
                }
                let idx = (*hid - H_ROW_BASE) as usize;
                let Some(row) = rows.get(idx) else {
                    return false;
                };
                let RowAction::Slide {
                    min,
                    max,
                    step,
                    value,
                    ..
                } = row.action
                else {
                    return false;
                };
                let delta = if *dy < 0.0 { step } else { -step };
                let next = (value + delta).clamp(min, max);
                if next == value {
                    return false;
                }
                let old_opt = (!row.old_cfg.is_empty()).then_some(row.old_cfg.as_str());
                self.pending_night_light = Some(next);
                out.push(Action::Run(change_command(
                    TABLE[1].key,
                    old_opt,
                    &next.to_string(),
                    TABLE[1].after,
                )));
                true
            }
            _ => false,
        }
    }
}

fn snap_slide(min: i32, max: i32, step: i32, part: f64) -> i32 {
    let span = f64::from((max - min).max(0));
    let raw = f64::from(min) + part.clamp(0.0, 1.0) * span;
    let stepped = ((raw - f64::from(min)) / f64::from(step.max(1))).round() as i32 * step.max(1);
    (min + stepped).clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings_rows::parse_config;
    use crate::config::Config;
    use crate::provider::Provider;
    use crate::theme::Theme;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const REPO_CONFIG: &str = include_str!("../../../../config/config");

    fn test_theme() -> Theme {
        Theme::from_config(&Config::from_text(""))
    }

    fn make_cx<'a>(size: Size, theme: &'a Theme) -> Cx<'a> {
        let mut cx = crate::cards::settings::tests::test_cx(theme);
        cx.size = size;
        cx
    }

    fn sample_cfg(user_overrides: &str) -> ConfigData {
        ConfigData {
            default: parse_config(REPO_CONFIG),
            now: parse_config(user_overrides),
            ..ConfigData::default()
        }
    }

    fn snap_and_wlr_two_screens(vrr_first: bool) -> String {
        format!(
            r#"{{
  "id": 1,
  "ok": true,
  "snapshot": {{
    "outputs": [
      {{
        "id": 1,
        "name": "DP-1",
        "make": "LG",
        "model": "27GP850",
        "mode": [2560, 1440],
        "scale": 1.0,
        "transform": "normal",
        "refresh_mhz": 165000
      }},
      {{
        "id": 2,
        "name": "HDMI-A-1",
        "make": "Unknown",
        "model": "Unknown",
        "mode": [1920, 1080],
        "scale": 1.25,
        "transform": "90",
        "refresh_mhz": 60000
      }}
    ]
  }}
}}
===WLR_RANDR===
[
  {{
    "name": "DP-1",
    "make": "LG",
    "model": "27GP850",
    "enabled": true,
    "modes": [
      {{"width": 2560, "height": 1440, "refresh": 165.000, "preferred": true, "current": true}},
      {{"width": 2560, "height": 1440, "refresh": 120.000, "preferred": false, "current": false}},
      {{"width": 2560, "height": 1440, "refresh": 59.940, "preferred": false, "current": false}},
      {{"width": 1920, "height": 1080, "refresh": 60.000, "preferred": false, "current": false}}
    ],
    "scale": 1.0,
    "transform": "normal",
    "adaptive_sync": {vrr_first}
  }},
  {{
    "name": "HDMI-A-1",
    "enabled": true,
    "modes": [
      {{"width": 1920, "height": 1080, "refresh": 60.000, "preferred": true, "current": true}}
    ],
    "scale": 1.25,
    "transform": "90",
    "adaptive_sync": false
  }}
]
"#
        )
    }

    fn snap_one_screen_no_wlr() -> String {
        r#"{
  "ok": true,
  "snapshot": {
    "outputs": [
      {
        "id": 1,
        "name": "eDP-1",
        "make": "BOE",
        "model": "NV140FHM",
        "mode": [1920, 1080],
        "scale": 1.0,
        "transform": "normal",
        "refresh_mhz": 60000
      }
    ]
  }
}
===WLR_RANDR===
"#
        .to_string()
    }

    fn draw_card(card: &DisplayCard, cx: &Cx) -> (f64, Hits) {
        let h = card.height(cx, 440.0);
        let surf =
            cairo::ImageSurface::create(cairo::Format::ARgb32, 440, (h as i32).max(1)).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        card.draw(&cr, cx, (0.0, 0.0, 440.0, h), &mut hits);
        (h, hits)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-display-test-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_exec(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn every_key_is_in_config_config_with_a_value_its_control_knows() {
        let cfg = sample_cfg("");
        for st in TABLE {
            assert!(
                cfg.default.contains_key(st.key),
                "missing default key {} in config/config",
                st.key
            );
            let val = cfg.value(st.key).trim();
            match st.control {
                Control::Pick(choices) => {
                    assert!(
                        choices.iter().any(|(_, v)| v.eq_ignore_ascii_case(val)),
                        "key {} default {:?} is not in Pick choices {:?}",
                        st.key,
                        val,
                        choices
                    );
                }
                Control::Slide { min, max, step, .. } => {
                    let n: f64 = val
                        .parse()
                        .unwrap_or_else(|e| panic!("key {} value {:?}: {e}", st.key, val));
                    assert!(
                        n >= min && n <= max,
                        "key {} value {n} out of [{min}, {max}]",
                        st.key
                    );
                    let steps = ((n - min) / step).round();
                    assert!(
                        ((min + steps * step) - n).abs() < 1e-6,
                        "key {} value {n} not on step {step}",
                        st.key
                    );
                }
                Control::Text => {
                    // Text control accepts any string including empty "".
                    let _ = val;
                }
                _ => panic!("unexpected control in settings_display::TABLE"),
            }
        }
        assert!(
            cfg.default.contains_key("OUTPUT_SCALE"),
            "missing OUTPUT_SCALE in config/config"
        );
        let scale_val = cfg.value("OUTPUT_SCALE");
        assert!(
            SCALE_CHOICES
                .iter()
                .any(|(_, v)| scale_matches(scale_val, v)),
            "OUTPUT_SCALE default {:?} does not match {:?}",
            scale_val,
            SCALE_CHOICES
        );
    }

    #[test]
    fn registration_metadata_and_source_every_none() {
        let prov = crate::cards::settings::Settings;
        let kind = prov.cards().iter().find(|k| k.id == "display").unwrap();
        assert_eq!(kind.title, "Display");
        assert_eq!(kind.sizes, &[Size::S, Size::M]);
        assert_eq!(kind.default, Size::S);
        assert!(!kind.half);
        assert_eq!(kind.sources, &["config", "screens"]);
        assert!(prov.card("display", None).is_some());
        let src = prov.source("screens").expect("screens source registered");
        assert_eq!(src.every(), None);
    }

    #[test]
    fn parse_screens_usual_missing_wlr_empty_and_broken() {
        let two = parse_screens(&snap_and_wlr_two_screens(true));
        assert_eq!(two.screens.len(), 2);
        assert!(two.has_wlr_lists());
        assert!(two.any_adaptive_sync());

        let dp = &two.screens[0];
        assert_eq!(dp.title(), "DP-1  2560x1440  165 Hz");
        assert_eq!(dp.subtitle(), "LG 27GP850");
        assert_eq!(dp.resolutions(), vec!["2560x1440", "1920x1080"]);
        let rates: Vec<_> = dp
            .rates_for(2560, 1440)
            .into_iter()
            .map(|r| r.label)
            .collect();
        assert_eq!(rates, vec!["165 Hz", "120 Hz", "59.94 Hz"]);

        let hdmi = &two.screens[1];
        assert_eq!(hdmi.title(), "HDMI-A-1  1920x1080  60 Hz");
        assert_eq!(hdmi.subtitle(), "");
        assert_eq!(hdmi.rates_for(1920, 1080).len(), 1);
        assert_eq!(hdmi.adaptive_sync, Some(false));

        // Without wlr-randr lists
        let no_wlr = parse_screens(&snap_one_screen_no_wlr());
        assert_eq!(no_wlr.screens.len(), 1);
        assert!(!no_wlr.has_wlr_lists());
        assert!(!no_wlr.any_adaptive_sync());
        assert_eq!(no_wlr.screens[0].title(), "eDP-1  1920x1080  60 Hz");
        assert_eq!(no_wlr.screens[0].subtitle(), "BOE NV140FHM");

        // Failed snapshot falls back cleanly to wlr-randr enabled outputs only
        let fallback = parse_screens(
            r#"{"ok":false,"error":"socket_not_found"}
===WLR_RANDR===
[
  {"name":"DP-1","enabled":true,"modes":[{"width":1920,"height":1080,"refresh":60.0,"current":true}],"scale":1.5,"transform":"normal"},
  {"name":"DP-2","enabled":false,"modes":[{"width":800,"height":600,"refresh":60.0}]}
]"#,
        );
        assert_eq!(fallback.screens.len(), 1);
        assert_eq!(fallback.screens[0].name, "DP-1");
        assert_eq!(fallback.screens[0].scale, 1.5);

        // Empty and broken inputs never panic
        assert!(parse_screens("").screens.is_empty());
        assert!(parse_screens("garbage\n===WLR_RANDR===\n{broken")
            .screens
            .is_empty());
        assert!(parse_screens("[1, 2, \"not_an_output\"]")
            .screens
            .is_empty());

        // Broken snapshot followed by valid wlr-randr array recovers the array
        let recovered = parse_screens(
            "{\"ok\":true,\"snapshot\":{\"outputs\":[broken\n[{\"name\":\"DP-3\",\"enabled\":true,\"modes\":[{\"width\":1280,\"height\":720,\"refresh\":60.0,\"current\":true}]}]",
        );
        assert_eq!(recovered.screens.len(), 1);
        assert_eq!(recovered.screens[0].name, "DP-3");
    }

    #[test]
    fn parse_screens_from_temp_folder_usual_empty_and_broken() {
        let dir = temp_dir("sources");
        let usual = dir.join("usual");
        let empty = dir.join("empty");
        let broken = dir.join("broken");
        fs::write(&usual, snap_and_wlr_two_screens(true)).unwrap();
        fs::write(&empty, "").unwrap();
        fs::write(&broken, "{not: valid json\n===WLR_RANDR===\n[{{{").unwrap();

        let parsed_usual = parse_screens(&fs::read_to_string(&usual).unwrap());
        let parsed_empty = parse_screens(&fs::read_to_string(&empty).unwrap());
        let parsed_broken = parse_screens(&fs::read_to_string(&broken).unwrap());
        assert_eq!(parsed_usual.screens.len(), 2);
        assert!(parsed_empty.screens.is_empty());
        assert!(parsed_broken.screens.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hiding_rules_wlr_rate_order_vrr_more_and_search() {
        let th = test_theme();
        let cx_s = make_cx(Size::S, &th);
        let cx_m = make_cx(Size::M, &th);

        // 1) No wlr-randr lists + 1 screen: hides Resolution, Refresh rate, Turned, VRR, Order.
        let mut card = DisplayCard::new();
        card.apply("config", &sample_cfg(""));
        card.apply("screens", &parse_screens(&snap_one_screen_no_wlr()));
        let (rows, headers, has_more) = card.build_layout(&cx_s);
        let names: Vec<_> = rows.iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["Scale", "Night light warmth"]);
        assert_eq!(rows[0].key, "OUTPUT_SCALE");
        assert!(!rows[0].wlr_live);
        assert!(has_more);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[1].1, "All screens");

        // At Size::M without wlr-randr, Order & VRR remain hidden while Sharper & Zoomed show.
        let (rows_m, _, has_more_m) = card.build_layout(&cx_m);
        let names_m: Vec<_> = rows_m.iter().map(|r| r.name).collect();
        assert_eq!(
            names_m,
            vec![
                "Scale",
                "Night light warmth",
                "Sharper when zoomed in",
                "Zoomed windows"
            ]
        );
        assert!(!has_more_m);

        // 2) 2 screens with wlr-randr lists, VRR = true on screen 0, HDMI has 1 rate.
        let mut card2 = DisplayCard::new();
        card2.apply("config", &sample_cfg(""));
        card2.apply("screens", &parse_screens(&snap_and_wlr_two_screens(true)));
        let (rows2_s, _, has_more2_s) = card2.build_layout(&cx_s);
        assert!(has_more2_s);
        let s_keys: Vec<_> = rows2_s.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            s_keys,
            vec![
                "OUTPUT_DP_1_MODE",
                "OUTPUT_DP_1_RATE",
                "OUTPUT_DP_1_SCALE",
                "OUTPUT_HDMI_A_1_MODE",
                // OUTPUT_HDMI_A_1_RATE hidden because 1 rate!
                "OUTPUT_HDMI_A_1_SCALE",
                "VRR_ENABLED",
                "NIGHT_LIGHT_TEMP",
            ]
        );

        let (rows2_m, _, has_more2_m) = card2.build_layout(&cx_m);
        assert!(!has_more2_m);
        let m_keys: Vec<_> = rows2_m.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            m_keys,
            vec![
                "OUTPUT_DP_1_MODE",
                "OUTPUT_DP_1_RATE",
                "OUTPUT_DP_1_SCALE",
                "OUTPUT_DP_1_TRANSFORM",
                "OUTPUT_HDMI_A_1_MODE",
                "OUTPUT_HDMI_A_1_SCALE",
                "OUTPUT_HDMI_A_1_TRANSFORM",
                "VRR_ENABLED",
                "NIGHT_LIGHT_TEMP",
                "OUTPUT_ORDER",
                "MAX_APP_SCALE",
                "SCALING_FILTER",
            ]
        );

        // 3) adaptive_sync = false hides VRR_ENABLED when config is auto, keeps it if off.
        card2.apply("screens", &parse_screens(&snap_and_wlr_two_screens(false)));
        let (no_vrr_rows, _, _) = card2.build_layout(&cx_m);
        assert!(!no_vrr_rows.iter().any(|r| r.key == "VRR_ENABLED"));

        card2.apply("config", &sample_cfg("VRR_ENABLED='off'\n"));
        let (off_vrr_rows, _, _) = card2.build_layout(&cx_m);
        assert!(off_vrr_rows.iter().any(|r| r.key == "VRR_ENABLED"));

        // 4) Search reveals More rows and Status::Hidden when nothing matches.
        let mut out = Vec::new();
        assert!(card2.event(&Event::Text("zoomed"), &cx_s, &mut out));
        assert_eq!(card2.status(&cx_s), Status::Ready);
        let (q_rows, _, q_has_more) = card2.build_layout(&cx_s);
        assert!(!q_has_more);
        let q_keys: Vec<_> = q_rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(q_keys, vec!["MAX_APP_SCALE", "SCALING_FILTER"]);

        assert!(card2.event(&Event::Text("no_such_setting"), &cx_s, &mut out));
        assert_eq!(card2.status(&cx_s), Status::Hidden);
    }

    #[test]
    fn exact_commands_per_control_and_safe_shell_execution() {
        let th = test_theme();
        let cx_m = make_cx(Size::M, &th);
        let dir = temp_dir("cmds");
        let log_file = dir.join("calls.log");
        let ans_file = dir.join("ask_answer");
        let bin_dir = dir.join("bin");
        let live_dir = dir.join("infynitae/live");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&live_dir).unwrap();

        let pwned = dir.join("pwned");
        write_exec(
            &bin_dir.join("wlr-randr"),
            &format!(
                "#!/bin/sh\nprintf 'wlr:%s\\n' \"$*\" >> {}\n",
                sq(log_file.to_str().unwrap())
            ),
        );
        write_exec(
            &dir.join(".local/bin/infynitae-edge"),
            &format!(
                "#!/bin/sh\nprintf 'ask:%s\\n' \"$*\" >> {}\ncat {}\n",
                sq(log_file.to_str().unwrap()),
                sq(ans_file.to_str().unwrap())
            ),
        );
        write_exec(
            &live_dir.join("settings.sh"),
            &format!(
                "#!/bin/sh\nprintf 'set:%s' \"$1\" >> {log}\nshift\nfor a in \"$@\"; do printf '|%s' \"$a\" >> {log}; done\nprintf '\\n' >> {log}\n",
                log = sq(log_file.to_str().unwrap())
            ),
        );

        let path_env = format!(
            "{}:{}",
            bin_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );

        let mut card = DisplayCard::new();
        card.apply("config", &sample_cfg(""));
        card.apply("screens", &parse_screens(&snap_and_wlr_two_screens(true)));
        let _ = draw_card(&card, &cx_m);

        // 1) Control Found: Resolution -> opens list, pick 1920x1080 -> mode command + Keep/Back
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_ROW_BASE, // OUTPUT_DP_1_MODE
                button: Button::Left,
                at: (20.0, 50.0)
            },
            &cx_m,
            &mut out
        ));
        assert!(out.is_empty());
        assert!(card.event(
            &Event::Press {
                hit: H_OPT_BASE + 1, // 1920x1080
                button: Button::Left,
                at: (20.0, 100.0)
            },
            &cx_m,
            &mut out
        ));
        let Action::Run(res_cmd) = out.remove(0) else {
            panic!("expected Action::Run for Resolution");
        };
        assert!(res_cmd.contains("wlr-randr --output 'DP-1' --mode '1920x1080'"));
        assert!(res_cmd.contains("timeout 10 \"$EDGE\" ask 'Keep this? 10'"));
        assert!(res_cmd.contains("'2560x1440@165Hz'"));

        // Execute with Back -> reverts to old mode and writes NOTHING
        fs::write(&ans_file, "Back").unwrap();
        let st = Command::new("sh")
            .arg("-c")
            .arg(&res_cmd)
            .env("HOME", &dir)
            .env("PATH", &path_env)
            .status()
            .unwrap();
        assert!(st.success());
        let calls_back = fs::read_to_string(&log_file).unwrap();
        assert!(calls_back.contains("wlr:--output DP-1 --mode 1920x1080\n"));
        assert!(calls_back.contains("wlr:--output DP-1 --mode 2560x1440@165Hz\n"));
        assert!(!calls_back.contains("set:"));

        // Execute with Keep -> switches mode AND logs + sets key
        fs::write(&log_file, "").unwrap();
        fs::write(&ans_file, "Keep").unwrap();
        let st = Command::new("sh")
            .arg("-c")
            .arg(&res_cmd)
            .env("HOME", &dir)
            .env("PATH", &path_env)
            .status()
            .unwrap();
        assert!(st.success());
        let calls_keep = fs::read_to_string(&log_file).unwrap();
        assert!(calls_keep.contains("wlr:--output DP-1 --mode 1920x1080\n"));
        assert!(!calls_keep.contains("wlr:--output DP-1 --mode 2560x1440@165Hz"));
        assert!(calls_keep.contains("set:set|OUTPUT_DP_1_MODE|1920x1080\n"));

        // Refresh rate command includes WxH@R
        let rate_cmd = mode_change_command(
            "DP-1",
            "2560x1440@120Hz",
            "2560x1440@165Hz",
            "OUTPUT_DP_1_RATE",
            None,
            "120",
        );
        assert!(rate_cmd.contains("--mode '2560x1440@120Hz'"));
        assert!(rate_cmd.contains("--mode '2560x1440@165Hz'"));

        // 2) Control Pick: Scale with wlr-randr + Scale without wlr-randr ("At the next start")
        assert!(card.event(
            &Event::Press {
                hit: H_ROW_BASE + 2, // OUTPUT_DP_1_SCALE
                button: Button::Left,
                at: (20.0, 130.0)
            },
            &cx_m,
            &mut out
        ));
        assert!(card.event(
            &Event::Press {
                hit: H_OPT_BASE + 1, // 1.25
                button: Button::Left,
                at: (20.0, 170.0)
            },
            &cx_m,
            &mut out
        ));
        let Action::Run(scale_live_cmd) = out.remove(0) else {
            panic!("expected Action::Run for Scale");
        };
        assert!(scale_live_cmd.contains("wlr-randr --output 'DP-1' --scale '1.25'"));
        assert!(scale_live_cmd.contains(&change_command(
            "OUTPUT_DP_1_SCALE",
            None,
            "1.25",
            After::Restart
        )));
        assert!(!card.restarted.contains("OUTPUT_DP_1_SCALE"));

        // Scale without wlr-randr writes key with change_command and shows "At the next start"
        let mut no_wlr_card = DisplayCard::new();
        no_wlr_card.apply("config", &sample_cfg(""));
        no_wlr_card.apply("screens", &parse_screens(&snap_one_screen_no_wlr()));
        let _ = draw_card(&no_wlr_card, &cx_m);
        assert!(no_wlr_card.event(
            &Event::Press {
                hit: H_ROW_BASE, // OUTPUT_SCALE
                button: Button::Left,
                at: (20.0, 50.0)
            },
            &cx_m,
            &mut out
        ));
        assert!(no_wlr_card.event(
            &Event::Press {
                hit: H_OPT_BASE + 2, // 1.5
                button: Button::Left,
                at: (20.0, 120.0)
            },
            &cx_m,
            &mut out
        ));
        let Action::Run(scale_offline_cmd) = out.remove(0) else {
            panic!("expected Action::Run for offline Scale");
        };
        assert!(!scale_offline_cmd.contains("wlr-randr"));
        assert_eq!(
            scale_offline_cmd,
            change_command("OUTPUT_SCALE", Some("1.0"), "1.5", After::Restart)
        );
        assert!(no_wlr_card.restarted.contains("OUTPUT_SCALE"));

        // 3) Control Slide: Press/Drag preview without Run, Release + Scroll commit After::Save
        let (rows, _, _) = card.build_layout(&cx_m);
        let nl_idx = rows
            .iter()
            .position(|r| r.key == "NIGHT_LIGHT_TEMP")
            .unwrap();
        let nl_hit = H_ROW_BASE + nl_idx as u32;
        let nl_rect = card
            .sliders
            .borrow()
            .iter()
            .find(|(id, _)| *id == nl_hit)
            .unwrap()
            .1;
        assert!(card.event(
            &Event::Press {
                hit: nl_hit,
                button: Button::Left,
                at: (nl_rect.0 + nl_rect.2, nl_rect.1 + 8.0) // max => 6500 K
            },
            &cx_m,
            &mut out
        ));
        assert!(out.is_empty());
        assert!(card.event(&Event::Release { hit: nl_hit }, &cx_m, &mut out));
        let Action::Run(slide_cmd) = out.remove(0) else {
            panic!("expected Action::Run on slider release");
        };
        assert_eq!(
            slide_cmd,
            change_command("NIGHT_LIGHT_TEMP", Some("4000"), "6500", After::Save)
        );

        assert!(card.event(
            &Event::Scroll {
                hit: Some(nl_hit),
                dy: 1.0
            },
            &cx_m,
            &mut out
        ));
        let Action::Run(scroll_cmd) = out.remove(0) else {
            panic!("expected Action::Run on slider scroll");
        };
        assert_eq!(
            scroll_cmd,
            change_command("NIGHT_LIGHT_TEMP", Some("4000"), "6400", After::Save)
        );

        // 4) Control Text: OUTPUT_ORDER quotes value safely and never executes metacharacters
        let ord_idx = rows.iter().position(|r| r.key == "OUTPUT_ORDER").unwrap();
        assert!(card.event(
            &Event::Press {
                hit: H_ROW_BASE + ord_idx as u32,
                button: Button::Left,
                at: (20.0, 200.0)
            },
            &cx_m,
            &mut out
        ));
        let Action::Run(text_cmd) = out.remove(0) else {
            panic!("expected Action::Run for Text row");
        };
        assert!(text_cmd.contains("ask --text --default '' 'Order, left to right'"));
        assert!(text_cmd.contains("\"$v\""));
        let evil = format!("DP-1;$(touch {})", pwned.display());
        fs::write(&log_file, "").unwrap();
        fs::write(&ans_file, &evil).unwrap();
        let st = Command::new("sh")
            .arg("-c")
            .arg(&text_cmd)
            .env("HOME", &dir)
            .env("PATH", &path_env)
            .status()
            .unwrap();
        assert!(st.success());
        assert!(
            !pwned.exists(),
            "user input must never execute as shell code"
        );
        let ord_log = fs::read_to_string(&log_file).unwrap();
        assert!(ord_log.contains(&format!("set:set|OUTPUT_ORDER|{evil}\n")));

        // Weird connector names are single-quoted cleanly
        let quoted_cmd = wlr_and_write_command(
            "DP-1'$(evil)",
            "--scale",
            "1.5",
            "OUTPUT_DP_1_SCALE",
            None,
            "1.5",
        );
        assert!(quoted_cmd.contains("'DP-1'\\''$(evil)'"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn height_matches_drawn_hits_and_tick_animates_open_list() {
        let th = test_theme();
        let cx_s = make_cx(Size::S, &th);
        let cx_m = make_cx(Size::M, &th);

        let mut card = DisplayCard::new();
        assert_eq!(card.status(&cx_s), Status::Loading);
        card.apply(
            "config",
            &sample_cfg("NIGHT_LIGHT_TEMP='4500'\nSCALING_FILTER='linear'\n"),
        );
        card.apply("screens", &parse_screens(&snap_and_wlr_two_screens(true)));
        assert_eq!(card.status(&cx_s), Status::Ready);

        // Size S closed
        let (h_s, hits_s) = draw_card(&card, &cx_s);
        assert!(h_s > 0.0);
        for (_, r) in &hits_s.list {
            assert!(r.1 >= 0.0 && r.1 + r.3 <= h_s + 0.5);
        }
        let more_hit = hits_s.list.iter().find(|(id, _)| *id == H_MORE).unwrap().1;
        assert!((more_hit.1 + more_hit.3 - h_s).abs() < 0.5);

        // Expand More at Size S
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_MORE,
                button: Button::Left,
                at: (more_hit.0 + 10.0, more_hit.1 + 10.0)
            },
            &cx_s,
            &mut out
        ));
        let (h_more, hits_more) = draw_card(&card, &cx_s);
        assert!(h_more > h_s);
        for (_, r) in &hits_more.list {
            assert!(r.1 >= 0.0 && r.1 + r.3 <= h_more + 0.5);
        }

        // Open Resolution Found list and advance animation with tick()
        assert!(card.event(
            &Event::Press {
                hit: H_ROW_BASE,
                button: Button::Left,
                at: (20.0, 50.0)
            },
            &cx_m,
            &mut out
        ));
        let h_m_before = card.height(&cx_m, 440.0);
        let ticked = card.tick(1000.0);
        assert!(ticked);
        let (h_m_open, hits_open) = draw_card(&card, &cx_m);
        assert!(h_m_open > h_m_before);
        assert!(hits_open.list.iter().any(|(id, _)| *id == H_OPT_BASE));
        assert!(hits_open.list.iter().any(|(id, _)| *id == H_OPT_BASE + 1));
        for (_, r) in &hits_open.list {
            assert!(r.1 >= 0.0 && r.1 + r.3 <= h_m_open + 0.5);
        }
    }
}
