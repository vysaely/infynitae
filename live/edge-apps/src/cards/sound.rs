//! Sound, Infynitae's own: the Volume card (the volume, the screen's brightness where it can be set,
//! the output in use), from the same commands as today's edge (`pactl`, `brightnessctl`;
//! live/edge-rs/src/modules/volume.rs). A level that cannot be read is not shown as a number.

use crate::card::{
    has_render_data, render_value, run_cmd, run_sh, Action, Button, Card, Cx, Data, Event, Hits,
    Rect, Source, Status,
};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::ui;
use cairo::Context;
use std::any::Any;
use std::f64::consts::PI;

pub struct Sound;

const KINDS: &[CardKind] = &[CardKind {
    id: "volume",
    title: "Volume",
    about: "Volume, output device, microphone, per-app levels and brightness",
    sizes: &[Size::M],
    default: Size::M,
    half: false,
    sources: &["volume", "brightness"],
}];

impl Provider for Sound {
    fn id(&self) -> &'static str {
        "sound"
    }
    fn name(&self) -> &'static str {
        "Sound"
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
    fn card(&self, kind: &str, _: Option<&str>) -> Option<Box<dyn Card>> {
        (kind == "volume").then(|| Box::new(VolumeCard::default()) as Box<dyn Card>)
    }
    fn source(&self, key: &str) -> Option<Box<dyn Source>> {
        match key {
            "volume" => Some(Box::new(VolumeSource)),
            "brightness" => Some(Box::new(BrightnessSource)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub description: String,
    pub level: Option<u8>,
    pub muted: bool,
}

impl DeviceInfo {
    pub fn display_name(&self) -> &str {
        if self.description.is_empty() || self.description == "(null)" {
            &self.name
        } else {
            &self.description
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SinkInput {
    pub id: u32,
    pub name: String,
    pub level: Option<u8>,
    pub muted: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeData {
    /// None: no sound server answered
    pub level: Option<u8>,
    pub muted: bool,
    /// the output's description
    pub output: String,
    /// the default sink's pactl Name
    pub default_sink: String,
    pub sinks: Vec<DeviceInfo>,
    /// default non-monitor input source, None when none exists
    pub mic: Option<DeviceInfo>,
    /// every input that is not a monitor of an output, for choosing the microphone
    pub sources: Vec<DeviceInfo>,
    /// playing streams (pactl list sink-inputs)
    pub apps: Vec<SinkInput>,
}

/// The first "N%" in a line.
fn first_percent(line: &str) -> Option<u8> {
    let b = line.as_bytes();
    let i = line.find('%')?;
    let mut j = i;
    while j > 0 && b[j - 1].is_ascii_digit() {
        j -= 1;
    }
    line[j..i].parse::<u32>().ok().map(|v| v.min(150) as u8)
}

/// `pactl get-sink-volume`, `get-sink-mute`, `get-default-sink`, and `pactl list` blocks
/// (`Sink #`, `Source #`, `Sink Input #`), as the source or render fixture prints them.
pub fn parse_volume(out: &str) -> VolumeData {
    let mut d = VolumeData::default();
    let mut default_sink = String::new();
    let mut default_source = String::new();
    let mut top_desc = String::new();
    let mut in_block = false;
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() || line == "---" {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Default Sink:") {
            default_sink = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("Default Source:") {
            default_source = rest.trim().to_string();
            continue;
        }
        if line.starts_with("Sink #")
            || line.starts_with("Source #")
            || line.starts_with("Sink Input #")
            || line.starts_with("Source Output #")
        {
            in_block = true;
            continue;
        }
        if in_block {
            continue;
        }
        if line.starts_with("Volume:") {
            if d.level.is_none() {
                d.level = first_percent(line);
            }
        } else if let Some(rest) = line.strip_prefix("Mute:") {
            d.muted = rest.trim().eq_ignore_ascii_case("yes");
        } else if let Some(rest) = line.strip_prefix("Description:") {
            let desc = rest.trim();
            if top_desc.is_empty() && !desc.is_empty() {
                top_desc = desc.to_string();
            }
        } else if !line.starts_with("balance") && !line.contains(':') && default_sink.is_empty() {
            default_sink = line.to_string();
        }
    }

    d.sinks = parse_pactl_devices(out, "Sink #", false);
    if let Some(chosen) = if !default_sink.is_empty() {
        d.sinks
            .iter()
            .find(|s| s.name == default_sink)
            .or_else(|| d.sinks.first())
    } else {
        d.sinks.first()
    } {
        d.default_sink = chosen.name.clone();
        d.output = chosen.display_name().to_string();
        if d.level.is_none() {
            d.level = chosen.level;
            d.muted = chosen.muted;
        }
    } else if !top_desc.is_empty() {
        d.default_sink = default_sink;
        d.output = top_desc;
    } else if !default_sink.is_empty() {
        d.default_sink = default_sink.clone();
        d.output = default_sink;
    }

    let sources = parse_pactl_devices(out, "Source #", true);
    d.sources = sources.clone();
    d.mic = if !default_source.is_empty() {
        if default_source.ends_with(".monitor") {
            None
        } else {
            sources.into_iter().find(|s| s.name == default_source)
        }
    } else if sources.len() == 1 {
        sources.into_iter().next()
    } else {
        None
    };

    d.apps = parse_pactl_sink_inputs(out);
    d
}

/// `brightnessctl -m`: "device,class,current,NN%,max"; None without a device whose brightness can be set.
pub fn parse_brightness(out: &str) -> Option<u8> {
    out.lines().find_map(first_percent).map(|p| p.min(100))
}

/// Parses `pactl list sinks` (`prefix = "Sink #"`) or `pactl list sources` (`prefix = "Source #"`).
pub fn parse_pactl_devices(out: &str, prefix: &str, skip_monitors: bool) -> Vec<DeviceInfo> {
    let mut out_list: Vec<DeviceInfo> = Vec::new();
    let mut cur: Option<(DeviceInfo, bool)> = None;
    let mut in_block = false;
    let mut in_sub = false;
    let push = |item: Option<(DeviceInfo, bool)>, list: &mut Vec<DeviceInfo>| {
        if let Some((d, is_mon)) = item {
            if !d.name.is_empty() && !(skip_monitors && (is_mon || d.name.ends_with(".monitor"))) {
                list.push(d);
            }
        }
    };
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Sink #")
            || line.starts_with("Source #")
            || line.starts_with("Sink Input #")
            || line.starts_with("Source Output #")
        {
            push(cur.take(), &mut out_list);
            in_block = line.starts_with(prefix);
            in_sub = false;
            if in_block {
                cur = Some((DeviceInfo::default(), false));
            }
            continue;
        }
        if !in_block {
            continue;
        }
        if matches!(line, "Properties:" | "Ports:" | "Formats:") {
            in_sub = true;
            continue;
        }
        if line.starts_with("Active Port:") {
            in_sub = false;
            continue;
        }
        if in_sub {
            continue;
        }
        let Some((d, is_mon)) = cur.as_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("Name:") {
            d.name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Description:") {
            d.description = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Mute:") {
            d.muted = rest.trim().eq_ignore_ascii_case("yes");
        } else if line.starts_with("Volume:") && d.level.is_none() {
            d.level = first_percent(line);
        } else if let Some(rest) = line.strip_prefix("Monitor of Sink:") {
            let m = rest.trim();
            if !m.is_empty() && !m.eq_ignore_ascii_case("n/a") {
                *is_mon = true;
            }
        }
    }
    push(cur.take(), &mut out_list);
    out_list
}

/// Parses `pactl list sink-inputs` into one entry per playing stream.
pub fn parse_pactl_sink_inputs(out: &str) -> Vec<SinkInput> {
    let mut streams: Vec<SinkInput> = Vec::new();
    let mut cur: Option<(u32, String, String, Option<u8>, bool, bool)> = None;
    let mut in_block = false;
    let push = |item: Option<(u32, String, String, Option<u8>, bool, bool)>,
                list: &mut Vec<SinkInput>| {
        let Some((id, app, bin, level, muted, has_fields)) = item else {
            return;
        };
        if !has_fields {
            return;
        }
        let name = if !app.is_empty() {
            app
        } else if !bin.is_empty() {
            bin
        } else {
            "App".to_string()
        };
        list.push(SinkInput {
            id,
            name,
            level,
            muted,
        });
    };
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Sink #")
            || line.starts_with("Source #")
            || line.starts_with("Source Output #")
        {
            push(cur.take(), &mut streams);
            in_block = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix("Sink Input #") {
            push(cur.take(), &mut streams);
            if let Ok(id) = rest.trim().parse::<u32>() {
                in_block = true;
                cur = Some((id, String::new(), String::new(), None, false, false));
            } else {
                in_block = false;
            }
            continue;
        }
        if !in_block {
            continue;
        }
        let Some(c) = cur.as_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("Mute:") {
            c.4 = rest.trim().eq_ignore_ascii_case("yes");
            c.5 = true;
        } else if line.starts_with("Volume:") && c.3.is_none() {
            c.3 = first_percent(line);
            if c.3.is_some() {
                c.5 = true;
            }
        } else if let Some(rest) = line.strip_prefix("application.name = ") {
            let val = rest.trim().trim_matches('"').to_string();
            if !val.is_empty() {
                c.1 = val;
                c.5 = true;
            }
        } else if let Some(rest) = line.strip_prefix("application.process.binary = ") {
            let val = rest.trim().trim_matches('"').to_string();
            if !val.is_empty() {
                c.2 = val;
                c.5 = true;
            }
        }
    }
    push(cur.take(), &mut streams);
    streams
}

fn pactl_mute_cmd(kind: &str, target: &str, muted: bool) -> String {
    let val = if muted { 1 } else { 0 };
    format!("pactl set-{kind}-mute {target} {val} >/dev/null 2>&1")
}

fn pactl_vol_cmd(kind: &str, target: &str, pct: u8) -> String {
    format!("pactl set-{kind}-volume {target} {pct}% && pactl set-{kind}-mute {target} 0")
}

/// Sets `sink_name` as the default sink and moves every playing stream to it.
pub fn switch_sink_command(sink_name: &str) -> String {
    let s = crate::cards::settings::sq(sink_name);
    format!(
        "pactl set-default-sink {s} >/dev/null 2>&1 && \
         for id in $(pactl list short sink-inputs 2>/dev/null | cut -f1); do \
         pactl move-sink-input \"$id\" {s} >/dev/null 2>&1; done"
    )
}

/// Sets `source_name` as the default input and moves the streams that recorded from the old
/// default to it. Streams on other inputs stay: the replay recorder also records an output's
/// monitor, which must not end up on a microphone.
pub fn switch_source_command(source_name: &str) -> String {
    let s = crate::cards::settings::sq(source_name);
    format!(
        "old=$(pactl get-default-source 2>/dev/null); \
         pactl set-default-source {s} >/dev/null 2>&1 || exit 1; \
         oldid=$(pactl list short sources 2>/dev/null | awk -v n=\"$old\" '$2==n{{print $1}}'); \
         pactl list short source-outputs 2>/dev/null | while read -r id src _; do \
         [ \"$src\" = \"$oldid\" ] && pactl move-source-output \"$id\" {s} >/dev/null 2>&1; done; true"
    )
}

pub fn mic_mute_command(muted: bool) -> String {
    pactl_mute_cmd("source", "@DEFAULT_SOURCE@", muted)
}

pub fn mic_volume_command(pct: u8) -> String {
    pactl_vol_cmd("source", "@DEFAULT_SOURCE@", pct)
}

pub fn app_mute_command(id: u32, muted: bool) -> String {
    pactl_mute_cmd("sink-input", &id.to_string(), muted)
}

pub fn app_volume_command(id: u32, pct: u8) -> String {
    pactl_vol_cmd("sink-input", &id.to_string(), pct)
}

pub fn fetch_volume_with(mut run: impl FnMut(&[&str]) -> String) -> VolumeData {
    let sinks_out = run(&["pactl", "list", "sinks"]);
    let sinks = parse_pactl_devices(&sinks_out, "Sink #", false);
    if sinks.is_empty() {
        return VolumeData::default();
    }
    let chosen = if sinks.len() == 1 {
        &sinks[0]
    } else {
        let def = run(&["pactl", "get-default-sink"]);
        let def = def.trim();
        sinks
            .iter()
            .find(|s| !def.is_empty() && s.name == def)
            .unwrap_or(&sinks[0])
    };
    let default_sink = chosen.name.clone();
    let level = chosen.level;
    let muted = chosen.muted;
    let output = chosen.display_name().to_string();

    let sources_out = run(&["pactl", "list", "sources"]);
    let sources = parse_pactl_devices(&sources_out, "Source #", true);
    let all_sources = sources.clone();
    let mic = if sources.is_empty() {
        None
    } else {
        let def_src = run(&["pactl", "get-default-source"]);
        let def_src = def_src.trim();
        if def_src.is_empty() || def_src.ends_with(".monitor") {
            None
        } else {
            sources.into_iter().find(|s| s.name == def_src)
        }
    };

    let inputs_out = run(&["pactl", "list", "sink-inputs"]);
    let apps = parse_pactl_sink_inputs(&inputs_out);

    VolumeData {
        level,
        muted,
        output,
        default_sink,
        sinks,
        mic,
        sources: all_sources,
        apps,
    }
}

struct VolumeSource;
impl Source for VolumeSource {
    fn fetch(&mut self) -> Data {
        if has_render_data() {
            return Box::new(parse_volume(&render_value("volume").unwrap_or_default()));
        }
        Box::new(fetch_volume_with(|args| {
            run_cmd("volume", args[0], &args[1..])
        }))
    }
}

/// The brightness, read as its own source (a PC without a backlight has none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Brightness(pub Option<u8>);

struct BrightnessSource;
impl Source for BrightnessSource {
    fn fetch(&mut self) -> Data {
        Box::new(Brightness(parse_brightness(&run_sh(
            "brightness",
            "brightnessctl -m 2>/dev/null",
        ))))
    }
}

const H_MUTE: u32 = 0;
const H_VOLUME: u32 = 1;
const H_BRIGHT: u32 = 2;
const H_OUTPUT: u32 = 3;
const H_MIC_MUTE: u32 = 4;
const H_MIC_VOL: u32 = 5;
const H_INPUT: u32 = 6;
const H_SINK_BASE: u32 = 100;
const H_SOURCE_BASE: u32 = 500;
const H_APP_BASE: u32 = 1000;
const SLIDER_ROW: f64 = 48.0;
const OUTPUT_ROW: f64 = 26.0;
const DIVIDER_H: f64 = 8.0;
const MIC_SLIDE_H: f64 = 22.0;
const APPS_HEAD_H: f64 = 24.0;
const APP_ROW_H: f64 = 50.0;

#[derive(Clone, Copy)]
enum SliderTarget {
    Master,
    Bright,
    Mic,
    App(u32),
}

#[derive(Default)]
pub struct VolumeCard {
    pub volume: Option<VolumeData>,
    pub brightness: Option<Brightness>,
    output_list: ui::List,
    input_list: ui::List,
    /// where each slider hit was drawn, for a press or drag that sets a level
    sliders: std::cell::RefCell<Vec<(u32, Rect)>>,
    /// slot index -> sink-input id from the last draw
    app_slots: std::cell::RefCell<Vec<(usize, u32)>>,
    /// active app slider drag: (hit_id, stream_id)
    drag_stream: Option<(u32, u32)>,
    scroll_accum: f64,
}

impl VolumeCard {
    /// The "Input" row with the microphone in use, and the list to choose another, from `y`;
    /// returns the `y` under them.
    #[allow(clippy::too_many_arguments)]
    fn draw_input_choice(
        &self,
        cr: &Context,
        cx: &Cx,
        hits: &mut Hits,
        body: Rect,
        mut y: f64,
        v: &VolumeData,
        mic: &DeviceInfo,
    ) -> f64 {
        let row_r: Rect = (body.0, y, body.2, ui::ROW_H);
        let right_x = body.0 + body.2 - 8.0;
        let name = mic.display_name();
        let tw = draw::measure(
            cr,
            &Text {
                size: draw::SIZE_SMALL,
                bold: false,
                color: cx.theme.accent_text,
                text: name,
            },
        )
        .0;
        let box_w = (tw + 33.0).min(row_r.2 * 0.6);
        let lit = ui::row(
            cr,
            cx,
            hits,
            H_INPUT,
            row_r,
            &ui::Row {
                icon: None,
                name: "Input",
                state: "",
                on: false,
                right: box_w + 8.0,
            },
        );
        ui::value_box(cr, cx, row_r, right_x, name, lit, self.input_list.shown());
        y += ui::ROW_H;
        if self.input_list.shown() {
            let lh = self.input_list.height(v.sources.len());
            if lh > 0.0 {
                let lr: Rect = (body.0, y, body.2, lh);
                cr.save().ok();
                cr.rectangle(lr.0, lr.1, lr.2, lr.3);
                cr.clip();
                ui::list_box(cr, cx, lr);
                for (i, src) in v.sources.iter().enumerate() {
                    let rr: Rect = (
                        body.0 + 4.0,
                        lr.1 + 4.0 + i as f64 * ui::LIST_ROW_H,
                        (body.2 - 8.0).max(0.0),
                        ui::LIST_ROW_H,
                    );
                    if rr.1 + rr.3 > lr.1 + lr.3 + 0.5 {
                        break;
                    }
                    ui::list_row(
                        cr,
                        cx,
                        hits,
                        H_SOURCE_BASE + i as u32,
                        rr,
                        src.display_name(),
                        "",
                        src.name == mic.name,
                        None,
                    );
                }
                cr.restore().ok();
                y += lh;
            }
        }
        y
    }

    fn has_brightness(&self) -> bool {
        self.brightness.as_ref().is_some_and(|b| b.0.is_some())
    }

    fn slider_rect(&self, hit: u32) -> Option<Rect> {
        self.sliders
            .borrow()
            .iter()
            .find(|(id, _)| *id == hit)
            .map(|(_, r)| *r)
    }

    fn slot_stream_id(&self, slot: usize) -> Option<u32> {
        let slots = self.app_slots.borrow();
        if let Some((_, sid)) = slots.iter().find(|(s, _)| *s == slot) {
            return Some(*sid);
        }
        if slots.is_empty() {
            return self
                .volume
                .as_ref()
                .and_then(|v| v.apps.get(slot))
                .map(|a| a.id);
        }
        None
    }

    fn target_for_hit(
        &self,
        hit: u32,
        for_drag: bool,
        allow_mute_hit: bool,
    ) -> Option<SliderTarget> {
        match hit {
            H_VOLUME => Some(SliderTarget::Master),
            H_MUTE if allow_mute_hit => Some(SliderTarget::Master),
            H_BRIGHT => Some(SliderTarget::Bright),
            H_MIC_VOL => Some(SliderTarget::Mic),
            H_MIC_MUTE if allow_mute_hit => Some(SliderTarget::Mic),
            h if h >= H_APP_BASE => {
                let off = h - H_APP_BASE;
                if !allow_mute_hit && off.is_multiple_of(2) {
                    return None;
                }
                let slot = (off / 2) as usize;
                let sid = if for_drag {
                    match self.drag_stream {
                        Some((dhit, sid)) if dhit == h => self
                            .volume
                            .as_ref()?
                            .apps
                            .iter()
                            .any(|a| a.id == sid)
                            .then_some(sid)?,
                        Some(_) => return None,
                        None => self.slot_stream_id(slot)?,
                    }
                } else {
                    self.slot_stream_id(slot)?
                };
                Some(SliderTarget::App(sid))
            }
            _ => None,
        }
    }

    fn current_level(&self, target: SliderTarget) -> Option<u8> {
        match target {
            SliderTarget::Master => self.volume.as_ref().and_then(|v| v.level),
            SliderTarget::Bright => self.brightness.as_ref().and_then(|b| b.0),
            SliderTarget::Mic => self.volume.as_ref().and_then(|v| v.mic.as_ref())?.level,
            SliderTarget::App(id) => {
                self.volume
                    .as_ref()?
                    .apps
                    .iter()
                    .find(|a| a.id == id)?
                    .level
            }
        }
    }

    fn set_target_level(&mut self, target: SliderTarget, pct: u8, out: &mut Vec<Action>) -> bool {
        match target {
            SliderTarget::Master => {
                let Some(v) = self.volume.as_mut() else {
                    return false;
                };
                if v.level == Some(pct) && !v.muted {
                    return false;
                }
                v.level = Some(pct);
                v.muted = false;
                if let Some(s) = v.sinks.iter_mut().find(|s| s.name == v.default_sink) {
                    s.level = Some(pct);
                    s.muted = false;
                }
                out.push(Action::Run(pactl_vol_cmd("sink", "@DEFAULT_SINK@", pct)));
                true
            }
            SliderTarget::Bright => {
                let Some(b) = self.brightness.as_mut() else {
                    return false;
                };
                if b.0 == Some(pct) {
                    return false;
                }
                b.0 = Some(pct);
                out.push(Action::Run(format!(
                    "brightnessctl set {pct}% >/dev/null 2>&1"
                )));
                true
            }
            SliderTarget::Mic => {
                let Some(mic) = self.volume.as_mut().and_then(|v| v.mic.as_mut()) else {
                    return false;
                };
                if mic.level == Some(pct) && !mic.muted {
                    return false;
                }
                mic.level = Some(pct);
                mic.muted = false;
                out.push(Action::Run(mic_volume_command(pct)));
                true
            }
            SliderTarget::App(id) => {
                let Some(app) = self
                    .volume
                    .as_mut()
                    .and_then(|v| v.apps.iter_mut().find(|a| a.id == id))
                else {
                    return false;
                };
                if app.level == Some(pct) && !app.muted {
                    return false;
                }
                app.level = Some(pct);
                app.muted = false;
                out.push(Action::Run(app_volume_command(id, pct)));
                true
            }
        }
    }
}

fn format_level(level: Option<u8>, muted: bool) -> String {
    if muted {
        "Muted".to_string()
    } else if let Some(v) = level {
        format!("{v}%")
    } else {
        String::new()
    }
}

fn slider_part_for(level: Option<u8>, muted: bool) -> f64 {
    if muted {
        0.0
    } else {
        level.unwrap_or(0) as f64 / 100.0
    }
}

fn draw_divider(cr: &Context, th: &crate::theme::Theme, body: Rect, y: f64) {
    draw::round_rect(
        cr,
        body.0 + 8.0,
        y + 3.0,
        (body.2 - 16.0).max(0.0),
        1.0,
        0.5,
    );
    draw::set(cr, th.track);
    let _ = cr.fill();
}

/// The speaker: a cone, and a slash while muted.
fn speaker(cr: &Context, x: f64, y: f64, color: crate::theme::Rgba, muted: bool) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(x + 1.0, y + 6.0);
    cr.line_to(x + 4.5, y + 6.0);
    cr.line_to(x + 8.5, y + 2.5);
    cr.line_to(x + 8.5, y + 13.5);
    cr.line_to(x + 4.5, y + 10.0);
    cr.line_to(x + 1.0, y + 10.0);
    cr.close_path();
    let _ = cr.stroke();
    if muted {
        cr.move_to(x + 11.0, y + 5.0);
        cr.line_to(x + 15.5, y + 11.0);
        cr.move_to(x + 15.5, y + 5.0);
        cr.line_to(x + 11.0, y + 11.0);
    } else {
        cr.new_sub_path();
        cr.arc(x + 9.0, y + 8.0, 4.0, -0.3 * PI, 0.3 * PI);
    }
    let _ = cr.stroke();
}

impl Card for VolumeCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        match key {
            "volume" => {
                let Some(d) = data.downcast_ref::<VolumeData>() else {
                    return false;
                };
                let changed = self.volume.as_ref() != Some(d);
                if d.sinks.len() <= 1 {
                    self.output_list = ui::List::default();
                }
                if d.sources.len() <= 1 {
                    self.input_list = ui::List::default();
                }
                self.volume = Some(d.clone());
                changed
            }
            "brightness" => {
                let Some(d) = data.downcast_ref::<Brightness>() else {
                    return false;
                };
                let changed = self.brightness.as_ref() != Some(d);
                self.brightness = Some(d.clone());
                changed
            }
            _ => false,
        }
    }
    fn status(&self, _: &Cx) -> Status {
        match &self.volume {
            None => Status::Loading,
            Some(v)
                if v.level.is_none()
                    && v.sinks.is_empty()
                    && v.mic.is_none()
                    && v.apps.is_empty() =>
            {
                Status::Missing("No sound output".into())
            }
            Some(_) => Status::Ready,
        }
    }
    fn height(&self, _: &Cx, _: f64) -> f64 {
        let Some(v) = &self.volume else { return 0.0 };
        let mut h = 0.0;
        if v.level.is_some() {
            h += SLIDER_ROW;
        }
        if self.has_brightness() {
            h += SLIDER_ROW;
        }
        if v.sinks.len() > 1 {
            h += ui::ROW_H + self.output_list.height(v.sinks.len());
        } else if !v.output.is_empty() {
            h += OUTPUT_ROW;
        }
        if v.mic.is_some() {
            h += DIVIDER_H + ui::ROW_H + MIC_SLIDE_H;
            if v.sources.len() > 1 {
                h += ui::ROW_H + self.input_list.height(v.sources.len());
            }
        }
        if !v.apps.is_empty() {
            h += APPS_HEAD_H + v.apps.len() as f64 * APP_ROW_H;
        }
        h
    }
    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(v) = &self.volume else { return };
        let th = cx.theme;
        let mut sliders = Vec::new();
        let mut app_slots = Vec::new();
        let mut y = body.1;
        let x = body.0 + 8.0;
        let w = (body.2 - 16.0).max(0.0);

        if let Some(level) = v.level {
            let ir = (x - 4.0, y, 24.0, 24.0);
            if hits.add(H_MUTE, ir) {
                ui::light(cr, cx, ir);
            }
            speaker(
                cr,
                x,
                y + 4.0,
                if v.muted { th.muted } else { th.text },
                v.muted,
            );
            draw::text(
                cr,
                x + 26.0,
                y + 2.0,
                &Text {
                    size: 13.0,
                    bold: false,
                    color: th.text,
                    text: "Volume",
                },
            );
            let value = format_level(Some(level), v.muted);
            draw::text_right(
                cr,
                x + w,
                y + 2.0,
                &Text {
                    size: 13.0,
                    bold: true,
                    color: th.text,
                    text: &value,
                },
            );
            let sr = (x, y + 26.0, w, 16.0);
            ui::slider(
                cr,
                cx,
                hits,
                H_VOLUME,
                sr,
                slider_part_for(Some(level), v.muted),
            );
            sliders.push((H_VOLUME, sr));
            y += SLIDER_ROW;
        }

        if let Some(Brightness(Some(b))) = &self.brightness {
            ui::icon(cr, ui::Icon::Power, x, y + 4.0, th.text);
            draw::text(
                cr,
                x + 26.0,
                y + 2.0,
                &Text {
                    size: 13.0,
                    bold: false,
                    color: th.text,
                    text: "Brightness",
                },
            );
            let value = format!("{b}%");
            draw::text_right(
                cr,
                x + w,
                y + 2.0,
                &Text {
                    size: 13.0,
                    bold: true,
                    color: th.text,
                    text: &value,
                },
            );
            let sr = (x, y + 26.0, w, 16.0);
            ui::slider(cr, cx, hits, H_BRIGHT, sr, *b as f64 / 100.0);
            sliders.push((H_BRIGHT, sr));
            y += SLIDER_ROW;
        }

        if v.sinks.len() > 1 {
            let row_r: Rect = (body.0, y, body.2, ui::ROW_H);
            let right_x = body.0 + body.2 - 8.0;
            let tw = draw::measure(
                cr,
                &Text {
                    size: draw::SIZE_SMALL,
                    bold: false,
                    color: th.accent_text,
                    text: &v.output,
                },
            )
            .0;
            let box_w = (tw + 33.0).min(row_r.2 * 0.6);
            let lit = ui::row(
                cr,
                cx,
                hits,
                H_OUTPUT,
                row_r,
                &ui::Row {
                    icon: None,
                    name: "Output",
                    state: "",
                    on: false,
                    right: box_w + 8.0,
                },
            );
            ui::value_box(
                cr,
                cx,
                row_r,
                right_x,
                &v.output,
                lit,
                self.output_list.shown(),
            );
            y += ui::ROW_H;
            if self.output_list.shown() {
                let lh = self.output_list.height(v.sinks.len());
                if lh > 0.0 {
                    let lr: Rect = (body.0, y, body.2, lh);
                    cr.save().ok();
                    cr.rectangle(lr.0, lr.1, lr.2, lr.3);
                    cr.clip();
                    ui::list_box(cr, cx, lr);
                    for (i, sink) in v.sinks.iter().enumerate() {
                        let rr: Rect = (
                            body.0 + 4.0,
                            lr.1 + 4.0 + i as f64 * ui::LIST_ROW_H,
                            (body.2 - 8.0).max(0.0),
                            ui::LIST_ROW_H,
                        );
                        if rr.1 + rr.3 > lr.1 + lr.3 + 0.5 {
                            break;
                        }
                        let active = sink.name == v.default_sink;
                        ui::list_row(
                            cr,
                            cx,
                            hits,
                            H_SINK_BASE + i as u32,
                            rr,
                            sink.display_name(),
                            "",
                            active,
                            None,
                        );
                    }
                    cr.restore().ok();
                    y += lh;
                }
            }
        } else if !v.output.is_empty() {
            draw::text_max(
                cr,
                x,
                y + 3.0,
                w,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: &format!("Output: {}", v.output),
                },
            );
            y += OUTPUT_ROW;
        }

        if let Some(mic) = &v.mic {
            draw_divider(cr, th, body, y);
            y += DIVIDER_H;
            let right_x = body.0 + body.2 - 8.0;
            let choose = v.sources.len() > 1;
            if choose {
                y = self.draw_input_choice(cr, cx, hits, body, y, v, mic);
            }
            let val_str = format_level(mic.level, mic.muted);
            let val_t = Text {
                size: 13.0,
                bold: true,
                color: if mic.muted { th.muted } else { th.text },
                text: &val_str,
            };
            let val_w = if val_str.is_empty() {
                0.0
            } else {
                draw::measure(cr, &val_t).0 + 8.0
            };
            let r: Rect = (body.0, y, body.2, ui::ROW_H);
            ui::row(
                cr,
                cx,
                hits,
                H_MIC_MUTE,
                r,
                &ui::Row {
                    icon: None,
                    name: "Microphone",
                    state: if choose { "" } else { &mic.description },
                    on: false,
                    right: 30.0 + 8.0 + val_w + 8.0,
                },
            );
            if !val_str.is_empty() {
                draw::text_right(cr, right_x - 38.0, y + (ui::ROW_H - 16.0) / 2.0, &val_t);
            }
            ui::switch(cr, cx, hits, H_MIC_MUTE, r, right_x, !mic.muted);
            y += ui::ROW_H;
            let sr = (x, y, w, MIC_SLIDE_H - 4.0);
            ui::slider(
                cr,
                cx,
                hits,
                H_MIC_VOL,
                sr,
                slider_part_for(mic.level, mic.muted),
            );
            sliders.push((H_MIC_VOL, sr));
            y += MIC_SLIDE_H;
        }

        if !v.apps.is_empty() {
            draw_divider(cr, th, body, y);
            draw::text_max(
                cr,
                x,
                y + 8.0,
                w,
                &Text {
                    size: 11.0,
                    bold: true,
                    color: th.muted,
                    text: "Apps",
                },
            );
            y += APPS_HEAD_H;
            for (i, app) in v.apps.iter().enumerate() {
                let hit_mute = H_APP_BASE + i as u32 * 2;
                let hit_vol = hit_mute + 1;
                app_slots.push((i, app.id));
                let btn_word = if app.muted { "Unmute" } else { "Mute" };
                let btn_tw = draw::measure(
                    cr,
                    &Text {
                        size: draw::SIZE_SMALL,
                        bold: false,
                        color: th.text,
                        text: btn_word,
                    },
                )
                .0;
                let btn_w = (btn_tw + 18.0).min(ui::BUTTON_MAX_W);
                let btn_x = x + w - btn_w;
                ui::button(cr, cx, hits, hit_mute, btn_x, y + 1.0, btn_word);

                let val_str = format_level(app.level, app.muted);
                let val_w = if val_str.is_empty() {
                    0.0
                } else {
                    draw::text_right(
                        cr,
                        btn_x - 8.0,
                        y + 5.0,
                        &Text {
                            size: 13.0,
                            bold: true,
                            color: if app.muted { th.muted } else { th.text },
                            text: &val_str,
                        },
                    ) + 8.0
                };
                let name_w = (btn_x - 8.0 - val_w - 8.0 - x).max(20.0);
                draw::text_max(
                    cr,
                    x,
                    y + 5.0,
                    name_w,
                    &Text {
                        size: 13.0,
                        bold: false,
                        color: th.text,
                        text: &app.name,
                    },
                );
                let sr = (x, y + 28.0, w, 18.0);
                ui::slider(
                    cr,
                    cx,
                    hits,
                    hit_vol,
                    sr,
                    slider_part_for(app.level, app.muted),
                );
                sliders.push((hit_vol, sr));
                y += APP_ROW_H;
            }
        }

        *self.sliders.borrow_mut() = sliders;
        *self.app_slots.borrow_mut() = app_slots;
    }
    fn event(&mut self, ev: &Event, _: &Cx, out: &mut Vec<Action>) -> bool {
        let pick = match ev {
            Event::Press {
                hit,
                button: Button::Left,
                at,
            } => {
                self.drag_stream = None;
                match *hit {
                    H_MUTE => {
                        let Some(v) = self.volume.as_mut() else {
                            return false;
                        };
                        v.muted = !v.muted;
                        if let Some(s) = v.sinks.iter_mut().find(|s| s.name == v.default_sink) {
                            s.muted = v.muted;
                        }
                        out.push(Action::Run(
                            "pactl set-sink-mute @DEFAULT_SINK@ toggle >/dev/null 2>&1".into(),
                        ));
                        return true;
                    }
                    H_OUTPUT => {
                        let has_multi = self.volume.as_ref().is_some_and(|v| v.sinks.len() > 1);
                        if !has_multi {
                            return false;
                        }
                        self.output_list.toggle();
                        return true;
                    }
                    H_INPUT => {
                        if !self.volume.as_ref().is_some_and(|v| v.sources.len() > 1) {
                            return false;
                        }
                        self.input_list.toggle();
                        return true;
                    }
                    h if (H_SOURCE_BASE..H_APP_BASE).contains(&h) && self.input_list.open => {
                        let idx = (h - H_SOURCE_BASE) as usize;
                        let Some(v) = self.volume.as_mut() else {
                            return false;
                        };
                        let Some(src) = v.sources.get(idx).cloned() else {
                            return false;
                        };
                        self.input_list.open = false;
                        if v.mic.as_ref().is_some_and(|m| m.name == src.name) {
                            return true;
                        }
                        out.push(Action::Run(switch_source_command(&src.name)));
                        v.mic = Some(src);
                        return true;
                    }
                    h if (H_SINK_BASE..H_SOURCE_BASE).contains(&h) && self.output_list.open => {
                        let idx = (h - H_SINK_BASE) as usize;
                        let Some(v) = self.volume.as_mut() else {
                            return false;
                        };
                        let Some(sink) = v.sinks.get(idx).cloned() else {
                            return false;
                        };
                        self.output_list.open = false;
                        if sink.name != v.default_sink {
                            v.default_sink = sink.name.clone();
                            v.output = sink.display_name().to_string();
                            if sink.level.is_some() {
                                v.level = sink.level;
                            }
                            v.muted = sink.muted;
                            out.push(Action::Run(switch_sink_command(&sink.name)));
                        }
                        return true;
                    }
                    H_MIC_MUTE => {
                        let Some(mic) = self.volume.as_mut().and_then(|v| v.mic.as_mut()) else {
                            return false;
                        };
                        mic.muted = !mic.muted;
                        out.push(Action::Run(mic_mute_command(mic.muted)));
                        return true;
                    }
                    h if h >= H_APP_BASE && (h - H_APP_BASE).is_multiple_of(2) => {
                        let slot = ((h - H_APP_BASE) / 2) as usize;
                        let Some(sid) = self.slot_stream_id(slot) else {
                            return false;
                        };
                        let Some(app) = self
                            .volume
                            .as_mut()
                            .and_then(|v| v.apps.iter_mut().find(|a| a.id == sid))
                        else {
                            return false;
                        };
                        app.muted = !app.muted;
                        out.push(Action::Run(app_mute_command(app.id, app.muted)));
                        return true;
                    }
                    _ => {
                        let Some(target) = self.target_for_hit(*hit, false, false) else {
                            return false;
                        };
                        let Some(r) = self.slider_rect(*hit) else {
                            return false;
                        };
                        if let SliderTarget::App(sid) = target {
                            self.drag_stream = Some((*hit, sid));
                        }
                        Some((target, (ui::slider_part(r, *at) * 100.0).round() as u8))
                    }
                }
            }
            Event::Drag { hit, at } => {
                let Some(target) = self.target_for_hit(*hit, true, false) else {
                    return false;
                };
                let Some(r) = self.slider_rect(*hit) else {
                    return false;
                };
                Some((target, (ui::slider_part(r, *at) * 100.0).round() as u8))
            }
            Event::Release { .. } | Event::Cancel { .. } => {
                self.drag_stream = None;
                return false;
            }
            Event::Scroll { hit, dy, notches } => {
                let id = hit.unwrap_or(H_VOLUME);
                let Some(target) = self.target_for_hit(id, false, true) else {
                    return false;
                };
                let Some(c) = self.current_level(target) else {
                    return false;
                };
                let dir = if *notches != 0.0 { *notches } else { *dy };
                let can_move = (dir < 0.0 && c < 100) || (dir > 0.0 && c > 0);
                if !can_move {
                    self.scroll_accum = 0.0;
                    return false;
                }
                let steps = crate::card::scroll_steps(
                    &mut self.scroll_accum,
                    *dy,
                    *notches,
                    crate::card::SCROLL_NOTCH_PX,
                );
                if steps == 0 {
                    return true;
                }
                Some((target, (c as i32 - steps as i32 * 5).clamp(0, 100) as u8))
            }
            Event::Step { hit, dir } => {
                if *dir == 0 {
                    return false;
                }
                let Some(target) = self.target_for_hit(*hit, false, false) else {
                    return false;
                };
                let Some(c) = self.current_level(target) else {
                    return false;
                };
                if (*dir > 0 && c >= 100) || (*dir < 0 && c == 0) {
                    return false;
                }
                Some((target, (c as i32 + i32::from(*dir) * 5).clamp(0, 100) as u8))
            }
            _ => None,
        };
        let Some((target, pct)) = pick else {
            return false;
        };
        self.set_target_level(target, pct, out)
    }
    fn tick(&mut self, dt_ms: f64) -> bool {
        let a = self.output_list.tick(dt_ms);
        self.input_list.tick(dt_ms) || a
    }
    fn show_list(&mut self) {
        if self.volume.as_ref().is_some_and(|v| v.sinks.len() > 1) {
            self.output_list.open = true;
            self.output_list.settle();
        }
    }
    fn panel_closed(&mut self) {
        self.output_list = ui::List::default();
        self.input_list = ui::List::default();
        self.drag_stream = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pactl_output_from_a_real_machine() {
        let d = parse_volume(
            "Volume: front-left: 39321 /  60% / -13.30 dB,   front-right: 39321 /  60% / -13.30 dB\n        balance 0.00\nMute: no\n\
             alsa_output.pci-0000_0c_00.4.analog-stereo\nSink #57\n\tName: alsa_output.usb-Headset.analog-stereo\n\tDescription: USB Headset\n\
             Sink #58\n\tName: alsa_output.pci-0000_0c_00.4.analog-stereo\n\tDescription: Built-in Audio Analog Stereo\n",
        );
        assert_eq!(d.level, Some(60));
        assert!(!d.muted);
        assert_eq!(
            d.output, "Built-in Audio Analog Stereo",
            "the default sink's description"
        );
        assert_eq!(
            parse_volume("").level,
            None,
            "no sound server: no level, not an invented one"
        );
        assert_eq!(
            parse_brightness("amdgpu_bl1,backlight,192,75%,255"),
            Some(75)
        );
        assert_eq!(parse_brightness(""), None);
    }

    #[test]
    fn a_drag_sets_the_level_and_the_same_level_sends_nothing() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        c.apply(
            "volume",
            &parse_volume("Volume: front-left: 1 / 60% /\nMute: no\n") as &dyn Any,
        );
        c.apply("brightness", &Brightness(None) as &dyn Any);
        assert_eq!(c.status(&cx), Status::Ready);
        assert_eq!(
            c.height(&cx, 300.0),
            SLIDER_ROW,
            "no brightness row, and no output line without the output's name"
        );
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 200).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 316.0, c.height(&cx, 316.0)), &mut hits);
        let r = hits.list.iter().find(|(id, _)| *id == H_VOLUME).unwrap().1;
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Drag {
                hit: H_VOLUME,
                at: (r.0 + r.2 * 0.25, r.1)
            },
            &cx,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Run(
                "pactl set-sink-volume @DEFAULT_SINK@ 25% && pactl set-sink-mute @DEFAULT_SINK@ 0"
                    .into()
            )]
        );
        let mut out = Vec::new();
        assert!(!c.event(
            &Event::Drag {
                hit: H_VOLUME,
                at: (r.0 + r.2 * 0.25, r.1)
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
        let mut out = Vec::new();
        assert!(
            c.event(
                &Event::Scroll {
                    hit: None,
                    dy: -36.0,
                    notches: -1.0,
                },
                &cx,
                &mut out
            ),
            "scroll up: +5"
        );
        assert_eq!(c.volume.as_ref().unwrap().level, Some(30));
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: H_MUTE,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx,
            &mut out
        ));
        assert!(c.volume.as_ref().unwrap().muted);
    }

    #[test]
    fn volume_fetch_minimizes_spawned_commands() {
        let mut calls: Vec<Vec<String>> = Vec::new();
        let down = fetch_volume_with(|args| {
            calls.push(args.iter().map(|s| s.to_string()).collect());
            String::new()
        });
        assert_eq!(
            calls,
            vec![vec!["pactl", "list", "sinks"]],
            "no sinks stops after one pactl call"
        );
        assert_eq!(down, VolumeData::default());

        calls.clear();
        let one = fetch_volume_with(|args| {
            calls.push(args.iter().map(|s| s.to_string()).collect());
            match args {
                ["pactl", "list", "sinks"] => "Sink #1\n\tName: alsa.1\n\tDescription: Speakers\n\tMute: no\n\
                     \tVolume: front-left: 32768 /  50% / -18.0 dB\n\tBase Volume: 65536 / 100% / 0.0 dB\n"
                    .into(),
                _ => String::new(),
            }
        });
        assert_eq!(
            calls,
            vec![
                vec!["pactl", "list", "sinks"],
                vec!["pactl", "list", "sources"],
                vec!["pactl", "list", "sink-inputs"],
            ],
            "one sink and no mic skips both get-default-sink and get-default-source"
        );
        assert_eq!(one.level, Some(50));
        assert!(!one.muted);
        assert_eq!(one.output, "Speakers");

        calls.clear();
        let multi = fetch_volume_with(|args| {
            calls.push(args.iter().map(|s| s.to_string()).collect());
            match args {
                ["pactl", "list", "sinks"] => {
                    "Sink #1\n\tName: s1\n\tDescription: First\n\tMute: no\n\
                     \tVolume: front-left: 1 / 20% /\n\
                     Sink #2\n\tName: s2\n\tDescription: Second\n\tMute: yes\n\
                     \tVolume: front-left: 1 / 75% /\n"
                        .into()
                }
                ["pactl", "get-default-sink"] => "s2\n".into(),
                ["pactl", "list", "sources"] => SAMPLE_SOURCES.into(),
                ["pactl", "get-default-source"] => {
                    "alsa_input.pci-0000_00_1b.0.analog-stereo\n".into()
                }
                ["pactl", "list", "sink-inputs"] => SAMPLE_SINK_INPUTS.into(),
                _ => String::new(),
            }
        });
        assert_eq!(
            calls,
            vec![
                vec!["pactl", "list", "sinks"],
                vec!["pactl", "get-default-sink"],
                vec!["pactl", "list", "sources"],
                vec!["pactl", "get-default-source"],
                vec!["pactl", "list", "sink-inputs"],
            ],
            "no extra process per sink, source, or playing stream"
        );
        assert_eq!(multi.level, Some(75));
        assert!(multi.muted);
        assert_eq!(multi.output, "Second");
        assert_eq!(multi.apps.len(), 2);
    }

    #[test]
    fn parse_volume_and_brightness_table_covers_fixtures_no_description_and_150_percent() {
        let cases: &[(&str, Option<u8>, bool, &str)] = &[
            (
                "Volume: front-left: 39321 /  60% / -13.30 dB,   front-right: 39321 /  60% / -13.30 dB\n\
                 \tbalance 0.00\nMute: no\nalsa_output.pci-0000_0c_00.4.analog-stereo\n\
                 Sink #57\n\tName: alsa_output.usb-Headset.analog-stereo\n\tDescription: USB Headset\n\
                 Sink #58\n\tName: alsa_output.pci-0000_0c_00.4.analog-stereo\n\tDescription: Built-in Audio Analog Stereo\n",
                Some(60),
                false,
                "Built-in Audio Analog Stereo",
            ),
            (
                "Volume: front-left: 98304 / 150% / 10.58 dB,   front-right: 98304 / 150% / 10.58 dB\n\
                 Mute: no\nalsa_output.usb-DAC.analog-stereo\n\
                 Sink #1\n\tName: alsa_output.usb-DAC.analog-stereo\n\tDescription: External USB DAC\n",
                Some(150),
                false,
                "External USB DAC",
            ),
            (
                "Volume: front-left: 131072 / 200% / 18.06 dB\nMute: yes\nSink #1\n\tName: alsa.raw\n",
                Some(150),
                true,
                "alsa.raw",
            ),
            (
                "Volume: front-left: 26214 /  40% / -23.88 dB\nMute: yes\n\
                 alsa_output.pci-0000_03_00.1.hdmi-stereo\n\
                 Sink #62\n\tName: alsa_output.usb-Headset.analog-stereo\n\tDescription: USB Headset\n\
                 Sink #6698\n\tName: alsa_output.pci-0000_03_00.1.hdmi-stereo\n",
                Some(40),
                true,
                "alsa_output.pci-0000_03_00.1.hdmi-stereo",
            ),
            (
                "Volume: mono: 0 /   0% / -inf dB\nMute: no\n\
                 Sink #9\n\tName: bluez_output.AA_BB_CC.1\n\tDescription:\n",
                Some(0),
                false,
                "bluez_output.AA_BB_CC.1",
            ),
            ("", None, false, ""),
        ];
        for (i, (raw, level, muted, output)) in cases.iter().enumerate() {
            let got = parse_volume(raw);
            assert_eq!(
                (got.level, got.muted, got.output.as_str()),
                (*level, *muted, *output),
                "case {i}"
            );
        }

        // fetch_volume_with: sink with no Description falls back to Name, and 150% volume is preserved.
        let no_desc = fetch_volume_with(|args| {
            match args {
            ["pactl", "list", "sinks"] => {
                "Sink #1\n\tName: s1\n\tDescription: First\n\tMute: no\n\tVolume: front-left: 1 / 50% /\n\
                 Sink #2\n\tName: alsa_output.pci-0000_00_1f.3\n\tMute: no\n\
                 \tVolume: front-left: 98304 / 150% / 10.58 dB\n"
                    .into()
            }
            ["pactl", "get-default-sink"] => "alsa_output.pci-0000_00_1f.3\n".into(),
            _ => String::new(),
        }
        });
        assert_eq!(
            (no_desc.level, no_desc.muted, no_desc.output.as_str()),
            (Some(150), false, "alsa_output.pci-0000_00_1f.3")
        );

        let brightness_cases: &[(&str, Option<u8>)] = &[
            ("amdgpu_bl1,backlight,192,75%,255\n", Some(75)),
            ("intel_backlight,backlight,0,0%,1200\n", Some(0)),
            ("intel_backlight,backlight,1500,125%,1200\n", Some(100)),
            ("", None),
            ("no_percent_here\n", None),
        ];
        for (raw, expected) in brightness_cases {
            assert_eq!(parse_brightness(raw), *expected, "input={raw:?}");
        }
    }

    #[test]
    fn slider_drag_past_both_ends_and_step_at_ends() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        c.apply(
            "volume",
            &VolumeData {
                level: Some(50),
                muted: false,
                output: "Speakers".into(),
                ..Default::default()
            } as &dyn Any,
        );
        c.apply("brightness", &Brightness(Some(50)) as &dyn Any);

        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 200).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 316.0, c.height(&cx, 316.0)), &mut hits);
        let vr = hits.list.iter().find(|(id, _)| *id == H_VOLUME).unwrap().1;
        let br = hits.list.iter().find(|(id, _)| *id == H_BRIGHT).unwrap().1;

        // Drag past left end of volume slider -> clamps to 0%.
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Drag {
                hit: H_VOLUME,
                at: (vr.0 - 80.0, vr.1 + 4.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(c.volume.as_ref().unwrap().level, Some(0));
        assert_eq!(
            out,
            vec![Action::Run(
                "pactl set-sink-volume @DEFAULT_SINK@ 0% && pactl set-sink-mute @DEFAULT_SINK@ 0"
                    .into()
            )]
        );

        // Step left at 0% -> false and no action (even if muted).
        c.volume.as_mut().unwrap().muted = true;
        out.clear();
        assert!(!c.event(
            &Event::Step {
                hit: H_VOLUME,
                dir: -1
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());

        // Step right from 0% -> 5% and unmutes.
        assert!(c.event(
            &Event::Step {
                hit: H_VOLUME,
                dir: 1
            },
            &cx,
            &mut out
        ));
        assert_eq!(c.volume.as_ref().unwrap().level, Some(5));
        assert!(!c.volume.as_ref().unwrap().muted);

        // Drag past right end of volume slider -> clamps to 100%.
        out.clear();
        assert!(c.event(
            &Event::Drag {
                hit: H_VOLUME,
                at: (vr.0 + vr.2 + 120.0, vr.1 + 4.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(c.volume.as_ref().unwrap().level, Some(100));
        assert_eq!(
            out,
            vec![Action::Run(
                "pactl set-sink-volume @DEFAULT_SINK@ 100% && pactl set-sink-mute @DEFAULT_SINK@ 0"
                    .into()
            )]
        );

        // Step right at 100% -> false and no action; step left -> 95%.
        out.clear();
        assert!(!c.event(
            &Event::Step {
                hit: H_VOLUME,
                dir: 1
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
        assert!(c.event(
            &Event::Step {
                hit: H_VOLUME,
                dir: -1
            },
            &cx,
            &mut out
        ));
        assert_eq!(c.volume.as_ref().unwrap().level, Some(95));

        // At 150% volume, stepping right does not drop volume to 100%.
        c.volume.as_mut().unwrap().level = Some(150);
        out.clear();
        assert!(!c.event(
            &Event::Step {
                hit: H_VOLUME,
                dir: 1
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
        assert_eq!(c.volume.as_ref().unwrap().level, Some(150));

        // Brightness slider: drag past both ends and Step at both ends.
        out.clear();
        assert!(c.event(
            &Event::Drag {
                hit: H_BRIGHT,
                at: (br.0 - 50.0, br.1),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(c.brightness, Some(Brightness(Some(0))));
        out.clear();
        assert!(!c.event(
            &Event::Step {
                hit: H_BRIGHT,
                dir: -1
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
        assert!(c.event(
            &Event::Drag {
                hit: H_BRIGHT,
                at: (br.0 + br.2 + 50.0, br.1),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(c.brightness, Some(Brightness(Some(100))));
        out.clear();
        assert!(!c.event(
            &Event::Step {
                hit: H_BRIGHT,
                dir: 1
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
    }

    const SAMPLE_SINKS: &str = "\
Sink #0
\tState: RUNNING
\tName: alsa_output.pci-0000_00_1f.3.analog-stereo
\tDescription: Built-in Audio Analog Stereo
\tDriver: PipeWire
\tMute: no
\tVolume: front-left: 39321 /  60% / -13.30 dB,   front-right: 39321 /  60% / -13.30 dB
\t        balance 0.00
\tBase Volume: 65536 / 100% / 0.00 dB
\tProperties:
\t\tdevice.description = \"Built-in Audio Analog Stereo\"
\tPorts:
\t\tanalog-output-lineout: Line Out (type: Line, priority: 9000, available)
\tActive Port: analog-output-lineout
Sink #1
\tState: SUSPENDED
\tName: alsa_output.pci-0000_01_00.1.hdmi-stereo
\tDescription: HDA NVidia Digital Stereo (HDMI)
\tMute: yes
\tVolume: front-left: 26214 /  40% / -23.88 dB,   front-right: 26214 /  40% / -23.88 dB
\tBase Volume: 65536 / 100% / 0.00 dB
";

    const SAMPLE_SOURCES: &str = "\
Source #0
\tState: SUSPENDED
\tName: alsa_output.pci-0000_00_1f.3.analog-stereo.monitor
\tDescription: Monitor of Built-in Audio Analog Stereo
\tDriver: module-alsa-card.c
\tMute: no
\tVolume: front-left: 65536 / 100% / 0.00 dB,   front-right: 65536 / 100% / 0.00 dB
\tBase Volume: 100% / 0.00 dB
\tMonitor of Sink: alsa_output.pci-0000_00_1f.3.analog-stereo
\tProperties:
\t\tdevice.class = \"monitor\"
Source #1
\tState: RUNNING
\tName: alsa_input.pci-0000_00_1b.0.analog-stereo
\tDescription: Built-in Audio Analog Stereo Internal Microphone
\tDriver: module-alsa-card.c
\tMute: no
\tVolume: front-left: 45875 /  70% / -9.31 dB,   front-right: 45875 /  70% / -9.31 dB
\tBase Volume: 32% / -30.00 dB
\tMonitor of Sink: n/a
\tProperties:
\t\tdevice.description = \"Internal Microphone\"
\t\tdevice.class = \"sound\"
";

    const SAMPLE_SINK_INPUTS: &str = "\
Sink Input #14
\tDriver: protocol-native.c
\tSink: 0
\tMute: no
\tVolume: front-left: 42598 /  65% / -11.23 dB,   front-right: 42598 /  65% / -11.23 dB
\tProperties:
\t\tmedia.name = \"Playback Stream\"
\t\tapplication.name = \"Firefox\"
\t\tapplication.process.binary = \"firefox\"
Sink Input #19
\tDriver: protocol-native.c
\tSink: 0
\tMute: yes
\tVolume: front-left: 65536 / 100% / 0.00 dB,   front-right: 65536 / 100% / 0.00 dB
\tProperties:
\t\tmedia.name = \"AudioStream\"
\t\tapplication.process.binary = \"vlc\"
Sink Input #bad
\tMute: no
\tVolume: mono: 32768 /  50% / -18.0 dB
Sink Input #99
\tUnknown line
";

    #[test]
    fn parses_real_pactl_sinks_sources_and_sink_inputs() {
        let sinks = parse_pactl_devices(SAMPLE_SINKS, "Sink #", false);
        assert_eq!(sinks.len(), 2);
        assert_eq!(sinks[0].name, "alsa_output.pci-0000_00_1f.3.analog-stereo");
        assert_eq!(sinks[0].description, "Built-in Audio Analog Stereo");
        assert_eq!(sinks[0].level, Some(60));
        assert!(!sinks[0].muted);
        assert_eq!(sinks[1].name, "alsa_output.pci-0000_01_00.1.hdmi-stereo");
        assert!(sinks[1].muted);
        assert_eq!(sinks[1].level, Some(40));

        let sources = parse_pactl_devices(SAMPLE_SOURCES, "Source #", true);
        assert_eq!(sources.len(), 1, "monitor source must be excluded");
        assert_eq!(sources[0].name, "alsa_input.pci-0000_00_1b.0.analog-stereo");
        assert_eq!(
            sources[0].description,
            "Built-in Audio Analog Stereo Internal Microphone"
        );
        assert_eq!(sources[0].level, Some(70));
        assert!(!sources[0].muted);

        let apps = parse_pactl_sink_inputs(SAMPLE_SINK_INPUTS);
        assert_eq!(
            apps,
            vec![
                SinkInput {
                    id: 14,
                    name: "Firefox".into(),
                    level: Some(65),
                    muted: false,
                },
                SinkInput {
                    id: 19,
                    name: "vlc".into(),
                    level: Some(100),
                    muted: true,
                },
            ]
        );

        assert!(parse_pactl_devices("", "Sink #", false).is_empty());
        assert!(parse_pactl_devices("garbled output", "Source #", true).is_empty());
        assert!(parse_pactl_sink_inputs("").is_empty());
        assert!(parse_pactl_sink_inputs("Sink Input #nope\n").is_empty());

        let full = fetch_volume_with(|args| match args {
            ["pactl", "list", "sinks"] => SAMPLE_SINKS.into(),
            ["pactl", "get-default-sink"] => "alsa_output.pci-0000_01_00.1.hdmi-stereo\n".into(),
            ["pactl", "list", "sources"] => SAMPLE_SOURCES.into(),
            ["pactl", "get-default-source"] => "alsa_input.pci-0000_00_1b.0.analog-stereo\n".into(),
            ["pactl", "list", "sink-inputs"] => SAMPLE_SINK_INPUTS.into(),
            _ => String::new(),
        });
        assert_eq!(full.sinks.len(), 2);
        assert_eq!(
            full.default_sink,
            "alsa_output.pci-0000_01_00.1.hdmi-stereo"
        );
        assert_eq!(full.output, "HDA NVidia Digital Stereo (HDMI)");
        assert_eq!(full.level, Some(40));
        assert!(full.muted);
        assert_eq!(full.mic.as_ref().unwrap().level, Some(70));
        assert_eq!(full.apps.len(), 2);

        // When default source is a monitor, no microphone is reported.
        let mon_only = fetch_volume_with(|args| match args {
            ["pactl", "list", "sinks"] => SAMPLE_SINKS.into(),
            ["pactl", "get-default-sink"] => "alsa_output.pci-0000_00_1f.3.analog-stereo\n".into(),
            ["pactl", "list", "sources"] => SAMPLE_SOURCES.into(),
            ["pactl", "get-default-source"] => {
                "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor\n".into()
            }
            _ => String::new(),
        });
        assert!(mon_only.mic.is_none());

        // Combined render fixture format parses sinks, default source, and sink-inputs.
        let combined = format!(
            "Default Sink: alsa_output.pci-0000_01_00.1.hdmi-stereo\n\
             Default Source: alsa_input.pci-0000_00_1b.0.analog-stereo\n\
             {SAMPLE_SINKS}{SAMPLE_SOURCES}{SAMPLE_SINK_INPUTS}"
        );
        let parsed = parse_volume(&combined);
        assert_eq!(parsed, full);
    }

    #[test]
    fn input_choice_switches_the_microphone_and_moves_only_its_streams() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let built_in = DeviceInfo {
            name: "alsa_input.pci-0000_00_1b.0.analog-stereo".into(),
            description: "Built-in Audio Analog Stereo".into(),
            level: Some(80),
            muted: false,
        };
        let usb = DeviceInfo {
            name: "alsa_input.usb-Example_Mic-00.mono-fallback".into(),
            description: "USB Microphone Mono".into(),
            level: Some(100),
            muted: false,
        };
        let mut c = VolumeCard::default();
        let data = VolumeData {
            level: Some(60),
            output: "Built-in Audio Analog Stereo".into(),
            mic: Some(built_in.clone()),
            sources: vec![built_in.clone(), usb.clone()],
            ..Default::default()
        };
        c.apply("volume", &data as &dyn Any);
        c.apply("brightness", &Brightness(None) as &dyn Any);

        let closed_h = c.height(&cx, 316.0);
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: H_INPUT,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        while c.tick(16.0) {}
        let open_h = c.height(&cx, 316.0);
        assert!(open_h > closed_h, "the list of inputs opens");
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 600).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 316.0, open_h), &mut hits);
        assert!(hits.list.iter().any(|(id, _)| *id == H_SOURCE_BASE + 1));

        out.clear();
        assert!(c.event(
            &Event::Press {
                hit: H_SOURCE_BASE + 1,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Run(switch_source_command(&usb.name))]);
        let Action::Run(cmd) = &out[0] else {
            unreachable!()
        };
        assert!(
            cmd.contains("pactl set-default-source 'alsa_input.usb-Example_Mic-00.mono-fallback'")
        );
        assert!(
            cmd.contains("[ \"$src\" = \"$oldid\" ]"),
            "only streams on the old input move: {cmd}"
        );
        assert_eq!(c.volume.as_ref().unwrap().mic.as_ref(), Some(&usb));

        // one input: no choice to make, the row is not drawn
        let mut one = VolumeCard::default();
        let single = VolumeData {
            mic: Some(built_in.clone()),
            sources: vec![built_in],
            ..data
        };
        one.apply("volume", &single as &dyn Any);
        let mut hits = Hits::new(None, None);
        let h = one.height(&cx, 316.0);
        one.draw(&cr, &cx, (0.0, 0.0, 316.0, h), &mut hits);
        assert!(!hits.list.iter().any(|(id, _)| *id == H_INPUT));
    }

    #[test]
    fn output_switch_click_sets_default_sink_and_moves_streams() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        let data = VolumeData {
            level: Some(60),
            muted: false,
            output: "Built-in Audio Analog Stereo".into(),
            default_sink: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
            sinks: parse_pactl_devices(SAMPLE_SINKS, "Sink #", false),
            mic: None,
            sources: Vec::new(),
            apps: parse_pactl_sink_inputs(SAMPLE_SINK_INPUTS),
        };
        c.apply("volume", &data as &dyn Any);
        c.apply("brightness", &Brightness(None) as &dyn Any);

        let closed_h = c.height(&cx, 316.0);
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: H_OUTPUT,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
        while c.tick(16.0) {}
        let open_h = c.height(&cx, 316.0);
        assert!(open_h > closed_h);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 400).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 316.0, open_h), &mut hits);
        assert!(hits.list.iter().any(|(id, _)| *id == H_SINK_BASE));
        assert!(hits.list.iter().any(|(id, _)| *id == H_SINK_BASE + 1));

        // Click second sink -> sets default sink and moves playing streams.
        out.clear();
        assert!(c.event(
            &Event::Press {
                hit: H_SINK_BASE + 1,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Run(switch_sink_command(
                "alsa_output.pci-0000_01_00.1.hdmi-stereo"
            ))]
        );
        assert!(out[0] == Action::Run(
            "pactl set-default-sink 'alsa_output.pci-0000_01_00.1.hdmi-stereo' >/dev/null 2>&1 && \
             for id in $(pactl list short sink-inputs 2>/dev/null | cut -f1); do \
             pactl move-sink-input \"$id\" 'alsa_output.pci-0000_01_00.1.hdmi-stereo' >/dev/null 2>&1; done"
                .into()
        ));
        let v = c.volume.as_ref().unwrap();
        assert_eq!(v.default_sink, "alsa_output.pci-0000_01_00.1.hdmi-stereo");
        assert_eq!(v.output, "HDA NVidia Digital Stereo (HDMI)");
        assert_eq!(v.level, Some(40));
        assert!(v.muted);
    }

    #[test]
    fn mic_switch_mutes_and_slider_sets_source_volume() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        let base = VolumeData {
            level: Some(60),
            muted: false,
            output: "Built-in Audio".into(),
            default_sink: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
            sinks: vec![],
            mic: None,
            sources: Vec::new(),
            apps: vec![],
        };
        c.apply("volume", &base as &dyn Any);
        c.apply("brightness", &Brightness(None) as &dyn Any);
        let no_mic_h = c.height(&cx, 316.0);

        let with_mic = VolumeData {
            mic: Some(DeviceInfo {
                name: "alsa_input.pci-0000_00_1b.0.analog-stereo".into(),
                description: "Internal Microphone".into(),
                level: Some(70),
                muted: false,
            }),
            ..base
        };
        assert!(c.apply("volume", &with_mic as &dyn Any));
        let mic_h = c.height(&cx, 316.0);
        assert!(mic_h > no_mic_h);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 400).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 316.0, mic_h), &mut hits);
        let mr = hits.list.iter().find(|(id, _)| *id == H_MIC_VOL).unwrap().1;

        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: H_MIC_MUTE,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(c.volume.as_ref().unwrap().mic.as_ref().unwrap().muted);
        assert_eq!(out, vec![Action::Run(mic_mute_command(true))]);
        assert_eq!(
            out[0],
            Action::Run("pactl set-source-mute @DEFAULT_SOURCE@ 1 >/dev/null 2>&1".into())
        );

        out.clear();
        assert!(c.event(
            &Event::Drag {
                hit: H_MIC_VOL,
                at: (mr.0 + mr.2 * 0.4, mr.1 + mr.3 / 2.0),
            },
            &cx,
            &mut out
        ));
        let mic = c.volume.as_ref().unwrap().mic.as_ref().unwrap();
        assert_eq!(mic.level, Some(40));
        assert!(!mic.muted);
        assert_eq!(out, vec![Action::Run(mic_volume_command(40))]);
        assert_eq!(
            out[0],
            Action::Run(
                "pactl set-source-volume @DEFAULT_SOURCE@ 40% && pactl set-source-mute @DEFAULT_SOURCE@ 0"
                    .into()
            )
        );

        // Keyboard Step on mic slider changes by 5%.
        out.clear();
        assert!(c.event(
            &Event::Step {
                hit: H_MIC_VOL,
                dir: 1,
            },
            &cx,
            &mut out
        ));
        assert_eq!(
            c.volume.as_ref().unwrap().mic.as_ref().unwrap().level,
            Some(45)
        );
        assert_eq!(out, vec![Action::Run(mic_volume_command(45))]);
    }

    #[test]
    fn per_app_rows_appear_and_disappear_and_control_streams() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        let empty = VolumeData {
            level: Some(60),
            muted: false,
            output: "Built-in Audio".into(),
            ..Default::default()
        };
        c.apply("volume", &empty as &dyn Any);
        c.apply("brightness", &Brightness(None) as &dyn Any);
        let base_h = c.height(&cx, 316.0);

        // 2 streams appear.
        let two = VolumeData {
            sources: Vec::new(),
            apps: parse_pactl_sink_inputs(SAMPLE_SINK_INPUTS),
            ..empty.clone()
        };
        assert!(c.apply("volume", &two as &dyn Any));
        let two_h = c.height(&cx, 316.0);
        assert!(two_h > base_h);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 500).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 316.0, two_h), &mut hits);
        let sr1 = hits
            .list
            .iter()
            .find(|(id, _)| *id == H_APP_BASE + 3)
            .unwrap()
            .1;

        // Mute stream #14 (Firefox).
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: H_APP_BASE,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(c.volume.as_ref().unwrap().apps[0].muted);
        assert_eq!(out, vec![Action::Run(app_mute_command(14, true))]);
        assert_eq!(
            out[0],
            Action::Run("pactl set-sink-input-mute 14 1 >/dev/null 2>&1".into())
        );

        // Drag stream #19 (vlc) slider -> 50% and unmutes.
        out.clear();
        assert!(c.event(
            &Event::Drag {
                hit: H_APP_BASE + 3,
                at: (sr1.0 + sr1.2 * 0.5, sr1.1 + sr1.3 / 2.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(c.volume.as_ref().unwrap().apps[1].level, Some(50));
        assert!(!c.volume.as_ref().unwrap().apps[1].muted);
        assert_eq!(out, vec![Action::Run(app_volume_command(19, 50))]);
        assert_eq!(
            out[0],
            Action::Run(
                "pactl set-sink-input-volume 19 50% && pactl set-sink-input-mute 19 0".into()
            )
        );

        // Stream #14 ends -> only #19 remains, height shrinks.
        let one = VolumeData {
            sources: Vec::new(),
            apps: vec![two.apps[1].clone()],
            ..empty.clone()
        };
        assert!(c.apply("volume", &one as &dyn Any));
        let one_h = c.height(&cx, 316.0);
        assert!(one_h > base_h && one_h < two_h);

        // All streams end -> nothing shown when none plays, height returns to base.
        assert!(c.apply("volume", &empty as &dyn Any));
        assert_eq!(c.height(&cx, 316.0), base_h);
    }

    #[test]
    fn height_matches_drawn_bounds_across_states() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        let data = VolumeData {
            level: Some(60),
            muted: false,
            output: "Built-in Audio Analog Stereo".into(),
            default_sink: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
            sinks: parse_pactl_devices(SAMPLE_SINKS, "Sink #", false),
            mic: Some(DeviceInfo {
                name: "alsa_input.pci-0000_00_1b.0.analog-stereo".into(),
                description: "Internal Microphone".into(),
                level: Some(70),
                muted: false,
            }),
            sources: Vec::new(),
            apps: parse_pactl_sink_inputs(SAMPLE_SINK_INPUTS),
        };
        c.apply("volume", &data as &dyn Any);
        c.apply("brightness", &Brightness(Some(50)) as &dyn Any);

        let check_state = |card: &VolumeCard, label: &str| {
            let h = card.height(&cx, 316.0);
            let mut surf =
                cairo::ImageSurface::create(cairo::Format::ARgb32, 360, h.ceil() as i32 + 24)
                    .unwrap();
            let mut hits = Hits::new(None, None);
            {
                let cr = cairo::Context::new(&surf).unwrap();
                card.draw(&cr, &cx, (0.0, 0.0, 316.0, h), &mut hits);
            }
            for (id, r) in &hits.list {
                assert!(
                    r.1 >= -0.1 && r.1 + r.3 <= h + 0.5,
                    "{label}: hit {id} rect {r:?} exceeded height {h}"
                );
            }
            surf.flush();
            let stride = surf.stride() as usize;
            let sh = surf.height() as usize;
            let bytes = surf.data().unwrap();
            let first_below = h.ceil() as usize + 1;
            for row in first_below..sh {
                let row_bytes = &bytes[row * stride..(row + 1) * stride];
                assert!(
                    row_bytes.iter().all(|&b| b == 0),
                    "{label}: drew pixels at row {row} below height {h}"
                );
            }
            (h, hits)
        };

        let (closed_h, closed_hits) = check_state(&c, "closed");
        assert!(closed_hits.list.iter().any(|(id, _)| *id == H_OUTPUT));
        assert!(closed_hits.list.iter().any(|(id, _)| *id == H_MIC_VOL));
        assert!(closed_hits.list.iter().any(|(id, _)| *id == H_APP_BASE));
        assert!(!closed_hits.list.iter().any(|(id, _)| *id == H_SINK_BASE));

        c.event(
            &Event::Press {
                hit: H_OUTPUT,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut Vec::new(),
        );
        c.tick(100.0);
        let (half_h, _) = check_state(&c, "half-open");
        c.show_list();
        let (open_h, open_hits) = check_state(&c, "open");
        assert!(
            open_h > half_h && half_h > closed_h,
            "heights should grow as list opens: closed={closed_h}, half={half_h}, open={open_h}"
        );
        assert!(open_hits.list.iter().any(|(id, _)| *id == H_SINK_BASE));
        assert!(open_hits.list.iter().any(|(id, _)| *id == H_SINK_BASE + 1));
        assert!(open_hits.list.iter().any(|(id, _)| *id == H_MIC_VOL));
        assert!(open_hits.list.iter().any(|(id, _)| *id == H_APP_BASE));
        assert!(open_hits.list.iter().any(|(id, _)| *id == H_APP_BASE + 3));
    }

    #[test]
    fn mid_drag_stream_removal_does_not_hit_neighbor_stream() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        let two = VolumeData {
            level: Some(60),
            muted: false,
            output: "Built-in Audio".into(),
            default_sink: "sink0".into(),
            sinks: vec![],
            mic: None,
            sources: Vec::new(),
            apps: parse_pactl_sink_inputs(SAMPLE_SINK_INPUTS),
        };
        c.apply("volume", &two as &dyn Any);

        let draw_once = |card: &VolumeCard| {
            let h = card.height(&cx, 316.0);
            let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 400).unwrap();
            let cr = cairo::Context::new(&surf).unwrap();
            let mut hits = Hits::new(None, None);
            card.draw(&cr, &cx, (0.0, 0.0, 316.0, h), &mut hits);
            hits
        };

        let hits = draw_once(&c);
        let sr0 = hits
            .list
            .iter()
            .find(|(id, _)| *id == H_APP_BASE + 1)
            .unwrap()
            .1;

        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: H_APP_BASE + 1,
                button: Button::Left,
                at: (sr0.0 + sr0.2 * 0.4, sr0.1 + sr0.3 / 2.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Run(app_volume_command(14, 40))]);

        // Stream #14 stops mid-drag; stream #19 shifts into slot 0 (still hit id H_APP_BASE + 1).
        let shifted = VolumeData {
            sources: Vec::new(),
            apps: vec![two.apps[1].clone()],
            ..two
        };
        assert!(c.apply("volume", &shifted as &dyn Any));
        let _ = draw_once(&c);

        out.clear();
        assert!(!c.event(
            &Event::Drag {
                hit: H_APP_BASE + 1,
                at: (sr0.0 + sr0.2 * 0.2, sr0.1 + sr0.3 / 2.0),
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
        assert_eq!(c.volume.as_ref().unwrap().apps[0].id, 19);
        assert_eq!(c.volume.as_ref().unwrap().apps[0].level, Some(100));
        assert!(c.volume.as_ref().unwrap().apps[0].muted);

        // Release clears drag state; a fresh press targets #19 normally.
        c.event(
            &Event::Release {
                hit: H_APP_BASE + 1,
            },
            &cx,
            &mut out,
        );
        assert!(c.event(
            &Event::Press {
                hit: H_APP_BASE + 1,
                button: Button::Left,
                at: (sr0.0 + sr0.2 * 0.2, sr0.1 + sr0.3 / 2.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Run(app_volume_command(19, 20))]);
    }

    #[test]
    fn panel_closed_folds_sink_list_and_clears_drag_stream() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        let data = VolumeData {
            level: Some(60),
            muted: false,
            output: "Built-in Audio Analog Stereo".into(),
            default_sink: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
            sinks: parse_pactl_devices(SAMPLE_SINKS, "Sink #", false),
            mic: None,
            sources: Vec::new(),
            apps: parse_pactl_sink_inputs(SAMPLE_SINK_INPUTS),
        };
        c.apply("volume", &data as &dyn Any);
        let closed_h = c.height(&cx, 316.0);
        c.show_list();
        assert!(c.output_list.open && c.output_list.shown());
        assert!(c.height(&cx, 316.0) > closed_h);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 500).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 316.0, c.height(&cx, 316.0)), &mut hits);
        let sr0 = hits
            .list
            .iter()
            .find(|(id, _)| *id == H_APP_BASE + 1)
            .unwrap()
            .1;
        c.event(
            &Event::Press {
                hit: H_APP_BASE + 1,
                button: Button::Left,
                at: (sr0.0 + sr0.2 * 0.5, sr0.1 + sr0.3 / 2.0),
            },
            &cx,
            &mut Vec::new(),
        );
        assert!(c.drag_stream.is_some());

        c.panel_closed();
        assert!(!c.output_list.open && !c.output_list.shown());
        assert_eq!(c.height(&cx, 316.0), closed_h);
        assert!(c.drag_stream.is_none());
    }

    #[test]
    fn scroll_and_step_control_mic_and_app_sliders_and_ignore_list_hits() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        let data = VolumeData {
            level: Some(60),
            muted: false,
            output: "Built-in Audio Analog Stereo".into(),
            default_sink: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
            sinks: parse_pactl_devices(SAMPLE_SINKS, "Sink #", false),
            mic: Some(DeviceInfo {
                name: "mic0".into(),
                description: "Internal Microphone".into(),
                level: Some(50),
                muted: false,
            }),
            sources: Vec::new(),
            apps: parse_pactl_sink_inputs(SAMPLE_SINK_INPUTS),
        };
        c.apply("volume", &data as &dyn Any);
        c.show_list();

        let h = c.height(&cx, 316.0);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 600).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 316.0, h), &mut hits);

        let mut out = Vec::new();
        assert!(c.event(
            &Event::Scroll {
                hit: Some(H_MIC_VOL),
                dy: 0.0,
                notches: -1.0,
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Run(mic_volume_command(55))]);

        out.clear();
        assert!(c.event(
            &Event::Step {
                hit: H_MIC_VOL,
                dir: -1,
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Run(mic_volume_command(50))]);

        out.clear();
        assert!(c.event(
            &Event::Scroll {
                hit: Some(H_MIC_MUTE),
                dy: 0.0,
                notches: -1.0,
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Run(mic_volume_command(55))]);
        assert!(!c.event(
            &Event::Step {
                hit: H_MIC_MUTE,
                dir: 1,
            },
            &cx,
            &mut out
        ));

        out.clear();
        assert!(c.event(
            &Event::Scroll {
                hit: Some(H_APP_BASE + 1),
                dy: 0.0,
                notches: 1.0,
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Run(app_volume_command(14, 60))]);

        out.clear();
        assert!(c.event(
            &Event::Step {
                hit: H_APP_BASE + 1,
                dir: 1,
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Run(app_volume_command(14, 65))]);

        out.clear();
        assert!(!c.event(
            &Event::Step {
                hit: H_APP_BASE,
                dir: 1,
            },
            &cx,
            &mut out
        ));
        assert!(!c.event(
            &Event::Scroll {
                hit: Some(H_OUTPUT),
                dy: 0.0,
                notches: -1.0,
            },
            &cx,
            &mut out
        ));
        assert!(!c.event(
            &Event::Scroll {
                hit: Some(H_SINK_BASE),
                dy: 0.0,
                notches: -1.0,
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
    }

    #[test]
    fn single_sink_and_empty_status_behave_honestly() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = crate::cards::settings::tests::test_cx(&th);
        let mut c = VolumeCard::default();
        assert!(matches!(c.status(&cx), Status::Loading));

        let empty = VolumeData {
            level: None,
            muted: false,
            output: String::new(),
            default_sink: String::new(),
            sinks: vec![],
            mic: None,
            sources: Vec::new(),
            apps: vec![],
        };
        c.apply("volume", &empty as &dyn Any);
        assert!(matches!(c.status(&cx), Status::Missing(m) if m == "No sound output"));

        let single = VolumeData {
            level: Some(50),
            muted: false,
            output: "Built-in Audio".into(),
            default_sink: "sink0".into(),
            sinks: vec![DeviceInfo {
                name: "sink0".into(),
                description: "Built-in Audio".into(),
                level: Some(50),
                muted: false,
            }],
            mic: None,
            sources: Vec::new(),
            apps: vec![],
        };
        c.apply("volume", &single as &dyn Any);
        assert!(matches!(c.status(&cx), Status::Ready));
        c.show_list();
        assert!(!c.output_list.open && !c.output_list.shown());

        let h = c.height(&cx, 316.0);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 200).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 316.0, h), &mut hits);
        assert!(!hits.list.iter().any(|(id, _)| *id == H_OUTPUT));
        assert!(!hits.list.iter().any(|(id, _)| *id == H_SINK_BASE));
    }
}
