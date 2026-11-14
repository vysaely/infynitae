//! Sound, Infynitae's own: the Volume card (the volume, the screen's brightness where it can be set,
//! the output in use), from the same commands as today's edge (`pactl`, `brightnessctl`;
//! live/edge-rs/src/modules/volume.rs). A level that cannot be read is not shown as a number.

use crate::card::{run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
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
    let mut first_desc = String::new();
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
        } else if let Some(rest) = line.strip_prefix("Description:") {
            let desc = rest.trim().to_string();
            if first_desc.is_empty() {
                first_desc = desc.clone();
            }
            if !default_sink.is_empty() && cur_name == default_sink {
                d.output = desc;
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
        d.output = if first_desc.is_empty() {
            default_sink
        } else {
            first_desc
        };
    }
    d
}

/// `brightnessctl -m`: "device,class,current,NN%,max"; None without a device whose brightness can be set.
pub fn parse_brightness(out: &str) -> Option<u8> {
    out.lines().find_map(first_percent).map(|p| p.min(100))
}

struct VolumeSource;
impl Source for VolumeSource {
    fn fetch(&mut self) -> Data {
        Box::new(parse_volume(&run_sh(
            "volume",
            "LC_ALL=C pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null; LC_ALL=C pactl get-sink-mute @DEFAULT_SINK@ 2>/dev/null; \
             LC_ALL=C pactl get-default-sink 2>/dev/null; LC_ALL=C pactl list sinks 2>/dev/null | grep -E '^\\s*(Sink #|Name:|Description:)'",
        )))
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
            Event::Scroll { hit, dy } => {
                let id = hit.unwrap_or(H_VOLUME);
                let cur = if id == H_BRIGHT {
                    self.brightness.as_ref().and_then(|b| b.0)
                } else {
                    self.volume.as_ref().and_then(|v| v.level)
                };
                cur.map(|c| {
                    (
                        id,
                        (c as i32 + if *dy < 0.0 { 5 } else { -5 }).clamp(0, 100) as u8,
                    )
                })
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
                    dy: -1.0
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
}
