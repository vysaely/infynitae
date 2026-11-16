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
    about: "The volume, the screen's brightness, the output in use",
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeData {
    /// None: no sound server answered
    pub level: Option<u8>,
    pub muted: bool,
    /// the output's description
    pub output: String,
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

/// `pactl get-sink-volume`, `get-sink-mute`, `get-default-sink`, and the sinks' Name and Description
/// lines, as the source prints them.
pub fn parse_volume(out: &str) -> VolumeData {
    let mut d = VolumeData::default();
    let mut default_sink = String::new();
    let mut cur_name = String::new();
    let mut first_name = String::new();
    let mut first_desc = String::new();
    let mut matched_default = false;
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Volume:") {
            if d.level.is_none() {
                d.level = first_percent(line);
            }
        } else if let Some(rest) = line.strip_prefix("Mute:") {
            d.muted = rest.trim().eq_ignore_ascii_case("yes");
        } else if let Some(rest) = line.strip_prefix("Name:") {
            cur_name = rest.trim().to_string();
            if first_name.is_empty() {
                first_name = cur_name.clone();
            }
            if !default_sink.is_empty() && cur_name == default_sink {
                matched_default = true;
            }
        } else if let Some(rest) = line.strip_prefix("Description:") {
            let desc = rest.trim().to_string();
            if !desc.is_empty() {
                if first_desc.is_empty() {
                    first_desc = desc.clone();
                }
                if !default_sink.is_empty() && cur_name == default_sink {
                    d.output = desc;
                }
            }
        } else if !line.starts_with("Sink #")
            && !line.starts_with("balance")
            && !line.contains(':')
            && default_sink.is_empty()
        {
            default_sink = line.to_string();
        }
    }
    if d.output.is_empty() {
        d.output = if matched_default {
            default_sink
        } else if !first_desc.is_empty() {
            first_desc
        } else if !default_sink.is_empty() {
            default_sink
        } else {
            first_name
        };
    }
    d
}

/// `brightnessctl -m`: "device,class,current,NN%,max"; None without a device whose brightness can be set.
pub fn parse_brightness(out: &str) -> Option<u8> {
    out.lines().find_map(first_percent).map(|p| p.min(100))
}

struct SinkInfo {
    name: String,
    description: String,
    level: Option<u8>,
    muted: bool,
}

fn parse_pactl_list_sinks(out: &str) -> Vec<SinkInfo> {
    let mut sinks: Vec<SinkInfo> = Vec::new();
    for raw in out.lines() {
        let line = raw.trim();
        if line.starts_with("Sink #") {
            sinks.push(SinkInfo {
                name: String::new(),
                description: String::new(),
                level: None,
                muted: false,
            });
            continue;
        }
        let Some(cur) = sinks.last_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("Name:") {
            cur.name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Description:") {
            cur.description = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Mute:") {
            cur.muted = rest.trim().eq_ignore_ascii_case("yes");
        } else if line.starts_with("Volume:") && cur.level.is_none() {
            cur.level = first_percent(line);
        }
    }
    sinks
}

pub fn fetch_volume_with(mut run: impl FnMut(&[&str]) -> String) -> VolumeData {
    let sinks_out = run(&["pactl", "list", "sinks"]);
    let sinks = parse_pactl_list_sinks(&sinks_out);
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
    VolumeData {
        level: chosen.level,
        muted: chosen.muted,
        output: if chosen.description.is_empty() {
            chosen.name.clone()
        } else {
            chosen.description.clone()
        },
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
const SLIDER_ROW: f64 = 48.0;
const OUTPUT_ROW: f64 = 26.0;

#[derive(Default)]
pub struct VolumeCard {
    pub volume: Option<VolumeData>,
    pub brightness: Option<Brightness>,
    /// where the sliders were drawn, for a press or drag that sets a level
    sliders: std::cell::Cell<[Rect; 2]>,
    scroll_accum: f64,
}

impl VolumeCard {
    fn has_brightness(&self) -> bool {
        self.brightness.as_ref().is_some_and(|b| b.0.is_some())
    }
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
            Some(v) if v.level.is_none() => Status::Missing("No sound output".into()),
            Some(_) => Status::Ready,
        }
    }
    fn height(&self, _: &Cx, _: f64) -> f64 {
        // the output's line only when its name is known
        let output = self.volume.as_ref().is_some_and(|v| !v.output.is_empty());
        SLIDER_ROW * if self.has_brightness() { 2.0 } else { 1.0 }
            + if output { OUTPUT_ROW } else { 0.0 }
    }
    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(v) = &self.volume else { return };
        let th = cx.theme;
        let level = v.level.unwrap_or(0);
        let mut rows: Vec<(u32, &str, u8, bool)> = vec![(H_VOLUME, "Volume", level, v.muted)];
        if let Some(Brightness(Some(b))) = &self.brightness {
            rows.push((H_BRIGHT, "Brightness", *b, false));
        }
        let mut sliders = [(0.0, 0.0, 0.0, 0.0); 2];
        let mut y = body.1;
        for (i, (id, name, val, muted)) in rows.iter().enumerate() {
            let x = body.0 + 8.0;
            let w = body.2 - 16.0;
            if *id == H_VOLUME {
                let ir = (x - 4.0, y, 24.0, 24.0);
                if hits.add(H_MUTE, ir) {
                    ui::light(cr, cx, ir);
                }
                speaker(
                    cr,
                    x,
                    y + 4.0,
                    if *muted { th.muted } else { th.text },
                    *muted,
                );
            } else {
                ui::icon(cr, ui::Icon::Power, x, y + 4.0, th.text);
            }
            draw::text(
                cr,
                x + 26.0,
                y + 2.0,
                &Text {
                    size: 13.0,
                    bold: false,
                    color: th.text,
                    text: name,
                },
            );
            let value = if *muted {
                "Muted".to_string()
            } else {
                format!("{val}%")
            };
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
                *id,
                sr,
                if *muted { 0.0 } else { *val as f64 / 100.0 },
            );
            sliders[i] = sr;
            y += SLIDER_ROW;
        }
        self.sliders.set(sliders);
        if v.output.is_empty() {
            return;
        }
        draw::text_max(
            cr,
            body.0 + 8.0,
            y + 3.0,
            body.2 - 16.0,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: &format!("Output: {}", v.output),
            },
        );
    }
    fn event(&mut self, ev: &Event, _: &Cx, out: &mut Vec<Action>) -> bool {
        let sliders = self.sliders.get();
        let set = |hit: u32, at: (f64, f64)| -> Option<(u32, u8)> {
            let r = match hit {
                H_VOLUME => sliders[0],
                H_BRIGHT => sliders[1],
                _ => return None,
            };
            Some((hit, (ui::slider_part(r, at) * 100.0).round() as u8))
        };
        let pick = match ev {
            Event::Press {
                hit: H_MUTE,
                button: Button::Left,
                ..
            } => {
                let Some(v) = self.volume.as_mut() else {
                    return false;
                };
                v.muted = !v.muted;
                out.push(Action::Run(
                    "pactl set-sink-mute @DEFAULT_SINK@ toggle >/dev/null 2>&1".into(),
                ));
                return true;
            }
            Event::Press {
                hit,
                button: Button::Left,
                at,
            }
            | Event::Drag { hit, at } => set(*hit, *at),
            Event::Scroll { hit, dy, notches } => {
                let id = hit.unwrap_or(H_VOLUME);
                let cur = if id == H_BRIGHT {
                    self.brightness.as_ref().and_then(|b| b.0)
                } else {
                    self.volume.as_ref().and_then(|v| v.level)
                };
                let Some(c) = cur else {
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
                Some((id, (c as i32 - steps as i32 * 5).clamp(0, 100) as u8))
            }
            Event::Step { hit, dir } => {
                if (*hit != H_VOLUME && *hit != H_BRIGHT) || *dir == 0 {
                    return false;
                }
                let cur = if *hit == H_BRIGHT {
                    self.brightness.as_ref().and_then(|b| b.0)
                } else {
                    self.volume.as_ref().and_then(|v| v.level)
                };
                let Some(c) = cur else {
                    return false;
                };
                if (*dir > 0 && c >= 100) || (*dir < 0 && c == 0) {
                    return false;
                }
                Some((*hit, (c as i32 + i32::from(*dir) * 5).clamp(0, 100) as u8))
            }
            _ => None,
        };
        let Some((id, pct)) = pick else { return false };
        if id == H_BRIGHT {
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
        } else {
            let Some(v) = self.volume.as_mut() else {
                return false;
            };
            if v.level == Some(pct) && !v.muted {
                return false;
            }
            v.level = Some(pct);
            v.muted = false;
            out.push(Action::Run(format!(
                "pactl set-sink-volume @DEFAULT_SINK@ {pct}% && pactl set-sink-mute @DEFAULT_SINK@ 0"
            )));
        }
        true
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
        let one = fetch_volume_with(|args| {
            calls.push(args.iter().map(|s| s.to_string()).collect());
            "Sink #1\n\tName: alsa.1\n\tDescription: Speakers\n\tMute: no\n\
             \tVolume: front-left: 32768 /  50% / -18.0 dB\n\tBase Volume: 65536 / 100% / 0.0 dB\n"
                .into()
        });
        assert_eq!(calls, vec![vec!["pactl", "list", "sinks"]]);
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
                _ => String::new(),
            }
        });
        assert_eq!(
            calls,
            vec![
                vec!["pactl", "list", "sinks"],
                vec!["pactl", "get-default-sink"]
            ]
        );
        assert_eq!(multi.level, Some(75));
        assert!(multi.muted);
        assert_eq!(multi.output, "Second");
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
                got,
                VolumeData {
                    level: *level,
                    muted: *muted,
                    output: (*output).to_string(),
                },
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
            no_desc,
            VolumeData {
                level: Some(150),
                muted: false,
                output: "alsa_output.pci-0000_00_1f.3".into(),
            }
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
}
