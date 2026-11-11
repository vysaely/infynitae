//! Sound (`volume` module): volume slider, optional brightness slider (when `brightnessctl`
//! reports a device), and the `"Output: <sink description>"` line.

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{is_left_button, Rect, World};
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::f64::consts::PI;

pub const SLIDER_ROW_H: f64 = 48.0;
pub const OUTPUT_LINE_H: f64 = 18.0;

pub const CMDS: &[(&str, &str)] = &[
    (
        "volume",
        "LC_ALL=C pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null; LC_ALL=C pactl get-sink-mute @DEFAULT_SINK@ 2>/dev/null; LC_ALL=C pactl get-default-sink 2>/dev/null; LC_ALL=C pactl list sinks 2>/dev/null | grep -E '^\\s*(Sink #|Name:|Description:)'",
    ),
    ("brightness", "brightnessctl -m 2>/dev/null"),
];

#[derive(Clone, Debug)]
pub struct VolumeData {
    pub ready: bool,
    pub pct: u8,
    pub muted: bool,
    pub sink: String,
    pub brightness: Option<u8>,
}

impl Default for VolumeData {
    fn default() -> Self {
        Self {
            ready: false,
            pct: 75,
            muted: false,
            sink: "Default".into(),
            brightness: None,
        }
    }
}

pub fn apply(d: &mut VolumeData, key: &str, out: &str) -> bool {
    match key {
        "volume" => {
            let (pct, muted, sink) = parse_pactl(out);
            d.pct = pct;
            d.muted = muted;
            d.sink = sink;
            d.ready = true;
            true
        }
        "brightness" => {
            d.brightness = parse_brightness(out);
            d.ready = true;
            true
        }
        _ => false,
    }
}

fn first_percent(s: &str) -> Option<u8> {
    let head = s.split_once('%')?.0;
    let digits: String = head
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let n = digits.parse::<u32>().ok()?;
    Some(n.min(100) as u8)
}

/// Parses `pactl get-sink-volume`, `get-sink-mute`, `get-default-sink` and `list sinks` output
/// into `(volume_pct, muted, sink_description)`.
pub fn parse_pactl(out: &str) -> (u8, bool, String) {
    let mut pct: Option<u8> = None;
    let mut muted = false;
    let mut default_sink = String::new();
    let mut cur_name = String::new();
    let mut matched_desc: Option<String> = None;
    let mut first_desc: Option<String> = None;

    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Volume:") {
            if pct.is_none() {
                pct = first_percent(line);
            }
        } else if let Some(rest) = line.strip_prefix("Mute:") {
            muted = rest.trim().eq_ignore_ascii_case("yes");
        } else if let Some(rest) = line.strip_prefix("Name:") {
            cur_name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Description:") {
            let desc = rest.trim().to_string();
            if first_desc.is_none() {
                first_desc = Some(desc.clone());
            }
            if !default_sink.is_empty() && cur_name == default_sink {
                matched_desc = Some(desc);
            }
        } else if let Some(rest) = line.strip_prefix("Output:") {
            matched_desc = Some(rest.trim().to_string());
        } else if !line.starts_with("Sink #")
            && !line.starts_with("balance")
            && !line.contains(':')
            && default_sink.is_empty()
        {
            if let Some(p) = first_percent(line) {
                if pct.is_none() {
                    pct = Some(p);
                }
            } else {
                default_sink = line.to_string();
            }
        }
    }
    let sink = matched_desc
        .or(first_desc)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if !default_sink.is_empty() {
                default_sink
            } else {
                "Default".to_string()
            }
        });
    (pct.unwrap_or(75), muted, sink)
}

/// Parses `brightnessctl -m`, `brightnessctl info`, or `brightnessctl get` + `max` output into
/// `Some(pct)` when a device is reported, or `None` when none exists.
pub fn parse_brightness(out: &str) -> Option<u8> {
    let trimmed = out.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some(p) = first_percent(trimmed) {
        return Some(p);
    }
    let nums: Vec<u64> = trimmed
        .lines()
        .filter_map(|l| l.trim().parse::<u64>().ok())
        .collect();
    match nums.as_slice() {
        [cur, max, ..] if *max > 0 => Some(((*cur * 100 + *max / 2) / *max).min(100) as u8),
        [pct] if *pct <= 100 => Some(*pct as u8),
        _ => None,
    }
}

pub fn height(world: &World) -> f64 {
    let rows = if world.volume.ready && world.volume.brightness.is_some() {
        2.0
    } else {
        1.0
    };
    2.0 * PAD_Y + rows * SLIDER_ROW_H + OUTPUT_LINE_H
}

pub fn set_volume_action(d: &mut VolumeData, pct: u8) -> String {
    let pct = pct.min(100);
    d.pct = pct;
    d.muted = false;
    format!("pactl set-sink-volume @DEFAULT_SINK@ {pct}% && pactl set-sink-mute @DEFAULT_SINK@ 0")
}

pub fn set_brightness_action(d: &mut VolumeData, pct: u8) -> String {
    let pct = pct.min(100);
    d.brightness = Some(pct);
    format!("brightnessctl set {pct}%")
}

/// Below this height the module is one line: the speaker, the slider, the value (the C edge's
/// 32 px volume); brightness and the output are left to the full size and the child card.
pub const COMPACT_H: f64 = 2.0 * PAD_Y + SLIDER_ROW_H;
const COMPACT_LEFT: f64 = PAD_X + 28.0;
const COMPACT_VALUE_W: f64 = 52.0;

fn compact(h: f64) -> bool {
    h > 0.0 && h < COMPACT_H
}

fn compact_pct_at(w: f64, x: f64) -> u8 {
    let iw = (w - COMPACT_LEFT - PAD_X - COMPACT_VALUE_W).max(1.0);
    (((x - COMPACT_LEFT) / iw).clamp(0.0, 1.0) * 100.0).round() as u8
}

fn pct_at(w: f64, x: f64) -> u8 {
    let iw = (w - 2.0 * PAD_X).max(1.0);
    (((x - PAD_X) / iw).clamp(0.0, 1.0) * 100.0).round() as u8
}

fn pointer_set(world: &mut World, size: (f64, f64), pos: (f64, f64)) -> bool {
    if !world.volume.ready {
        return false;
    }
    let (w, h) = size;
    let (x, y) = pos;
    // a drag sends a level only when it changed (one command per motion event, run in no set
    // order, could leave an older level last)
    if compact(h) {
        let pct = compact_pct_at(w, x);
        if pct == world.volume.pct && !world.volume.muted {
            return false;
        }
        let cmd = set_volume_action(&mut world.volume, pct);
        world.run_coalesced("volume", cmd);
        return true;
    }
    let rows = if world.volume.brightness.is_some() {
        2
    } else {
        1
    };
    let out_top = PAD_Y + (rows as f64) * SLIDER_ROW_H - 4.0;
    if y >= out_top {
        // the output line: click opens the Sound card; a drag does nothing there
        return false;
    }
    let pct = pct_at(w, x);
    let on_brightness = rows == 2 && y >= PAD_Y + SLIDER_ROW_H - 4.0;
    let same = if on_brightness {
        world.volume.brightness == Some(pct)
    } else {
        world.volume.pct == pct && !world.volume.muted
    };
    if same {
        return false;
    }
    let key = if on_brightness {
        "brightness"
    } else {
        "volume"
    };
    let cmd = if on_brightness {
        set_brightness_action(&mut world.volume, pct)
    } else {
        set_volume_action(&mut world.volume, pct)
    };
    world.run_coalesced(key, cmd);
    true
}

/// Whether `y` (in the card) is on the output line under the sliders.
fn on_output_line(world: &World, y: f64) -> bool {
    let rows = if world.volume.brightness.is_some() {
        2
    } else {
        1
    };
    y >= PAD_Y + (rows as f64) * SLIDER_ROW_H - 4.0
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if !is_left_button(button) {
        return false;
    }
    if !compact(size.1) && on_output_line(world, pos.1) {
        // "Output: <sink> ... More": the Sound card (the outputs, and each app's volume)
        return crate::modules::toggle_child(world, crate::modules::child::ChildKind::Audio);
    }
    pointer_set(world, size, pos)
}

pub fn drag(world: &mut World, size: (f64, f64), pos: (f64, f64)) -> bool {
    pointer_set(world, size, pos)
}

/// Scroll changes volume (or brightness when over the brightness row) by 5.
/// Negative `dy` scrolls up (+5), positive `dy` scrolls down (-5).
pub fn scroll(world: &mut World, size: (f64, f64), pos: (f64, f64), dy: f64) -> bool {
    if !world.volume.ready || dy.abs() < 0.01 {
        return false;
    }
    let step: i32 = if dy < 0.0 { 5 } else { -5 };
    let on_brightness = !compact(size.1)
        && world.volume.brightness.is_some()
        && pos.1 >= PAD_Y + SLIDER_ROW_H - 4.0;
    let cmd = if on_brightness {
        let cur = world.volume.brightness.unwrap_or(50) as i32;
        let next = (cur + step).clamp(0, 100) as u8;
        set_brightness_action(&mut world.volume, next)
    } else {
        let cur = world.volume.pct as i32;
        let next = (cur + step).clamp(0, 100) as u8;
        set_volume_action(&mut world.volume, next)
    };
    world.run_coalesced(
        if on_brightness {
            "brightness"
        } else {
            "volume"
        },
        cmd,
    );
    true
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    draw::card(cr, th, x, y, w, h);
    if !world.volume.ready {
        draw::title(cr, th, x + PAD_X, y + PAD_Y, "Volume");
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

    let d = &world.volume;
    if compact(h) {
        draw_compact(cr, th, d, rect);
        return;
    }
    let vol_val = if d.muted {
        "Muted".to_string()
    } else {
        format!("{}%", d.pct)
    };
    let vol_part = if d.muted {
        0.0
    } else {
        (d.pct.min(100) as f64) / 100.0
    };
    draw_slider_row(
        cr,
        th,
        x + PAD_X,
        y + PAD_Y,
        w - 2.0 * PAD_X,
        false,
        d.muted,
        "Volume",
        &vol_val,
        vol_part,
    );

    let mut oy = y + PAD_Y + SLIDER_ROW_H;
    if let Some(br) = d.brightness {
        let br_val = format!("{}%", br.min(100));
        let br_part = (br.min(100) as f64) / 100.0;
        draw_slider_row(
            cr,
            th,
            x + PAD_X,
            oy,
            w - 2.0 * PAD_X,
            true,
            false,
            "Brightness",
            &br_val,
            br_part,
        );
        oy += SLIDER_ROW_H;
    }

    // "Output: <sink>" left and "Change" right, as in the design: the whole line opens the list
    let change = Text {
        size: 12.0,
        bold: false,
        color: th.muted,
        text: "Change",
    };
    let (cw, _) = draw::measure(cr, &change);
    draw::text_right(cr, x + w - PAD_X, oy + 2.0, &change);
    let out_text = format!("Output: {}", d.sink);
    draw::text_max(
        cr,
        x + PAD_X,
        oy + 2.0,
        (w - 2.0 * PAD_X - cw - 12.0).max(20.0),
        &Text {
            size: 12.0,
            bold: false,
            color: th.muted,
            text: &out_text,
        },
    );
}

fn draw_compact(cr: &Context, th: &Theme, d: &VolumeData, rect: Rect) {
    let (x, y, w, h) = rect;
    let cy = (y + h / 2.0).round();
    draw_speaker_icon(
        cr,
        x + PAD_X,
        cy - 9.0,
        if d.muted { th.muted } else { th.text },
        d.muted,
    );
    let value = if d.muted {
        "Muted".to_string()
    } else {
        format!("{}%", d.pct)
    };
    draw::text_right(
        cr,
        x + w - PAD_X,
        cy - 9.0,
        &Text {
            size: 13.0,
            bold: true,
            color: th.text,
            text: &value,
        },
    );
    let sx = x + COMPACT_LEFT;
    let sw = (w - COMPACT_LEFT - PAD_X - COMPACT_VALUE_W).max(10.0);
    let part = if d.muted {
        0.0
    } else {
        d.pct.min(100) as f64 / 100.0
    };
    draw::round_rect(cr, sx, cy - 2.0, sw, 4.0, 2.0);
    draw::set(cr, th.track);
    let _ = cr.fill();
    if part > 0.0 {
        draw::round_rect(cr, sx, cy - 2.0, (sw * part).max(4.0), 4.0, 2.0);
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }
    cr.new_sub_path();
    cr.arc(sx + sw * part, cy, 7.0, 0.0, 2.0 * PI);
    draw::set(cr, th.text);
    let _ = cr.fill();
}

#[allow(clippy::too_many_arguments)]
fn draw_slider_row(
    cr: &Context,
    th: &Theme,
    x: f64,
    y: f64,
    w: f64,
    is_brightness: bool,
    muted: bool,
    label: &str,
    value: &str,
    part: f64,
) {
    let icon_col = if muted { th.muted } else { th.text };
    if is_brightness {
        draw_sun_icon(cr, x, y, icon_col);
    } else {
        draw_speaker_icon(cr, x, y, icon_col, muted);
    }
    draw::text(
        cr,
        x + 26.0,
        y,
        &Text {
            size: 13.0,
            bold: false,
            color: th.text,
            text: label,
        },
    );
    draw::text_right(
        cr,
        x + w,
        y,
        &Text {
            size: 13.0,
            bold: true,
            color: th.text,
            text: value,
        },
    );

    let ty = (y + 26.0).round();
    draw::round_rect(cr, x, ty, w, 4.0, 2.0);
    draw::set(cr, th.track);
    let _ = cr.fill();

    let clamped = part.clamp(0.0, 1.0);
    let kx = x + w * clamped;
    if clamped > 0.0 {
        draw::round_rect(cr, x, ty, (w * clamped).max(4.0), 4.0, 2.0);
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }
    cr.new_sub_path();
    cr.arc(kx, ty + 2.0, 7.0, 0.0, 2.0 * PI);
    draw::set(cr, th.text);
    let _ = cr.fill();
}

fn draw_speaker_icon(cr: &Context, x: f64, y: f64, color: Rgba, muted: bool) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(x + 2.0, y + 6.5);
    cr.line_to(x + 5.5, y + 6.5);
    cr.line_to(x + 9.5, y + 3.0);
    cr.line_to(x + 9.5, y + 15.0);
    cr.line_to(x + 5.5, y + 11.5);
    cr.line_to(x + 2.0, y + 11.5);
    cr.close_path();
    let _ = cr.stroke();
    if muted {
        cr.move_to(x + 12.5, y + 6.5);
        cr.line_to(x + 16.5, y + 11.5);
        cr.move_to(x + 16.5, y + 6.5);
        cr.line_to(x + 12.5, y + 11.5);
        let _ = cr.stroke();
    } else {
        cr.new_sub_path();
        cr.arc(x + 10.5, y + 9.0, 3.0, -0.35 * PI, 0.35 * PI);
        let _ = cr.stroke();
        cr.new_sub_path();
        cr.arc(x + 10.5, y + 9.0, 5.5, -0.38 * PI, 0.38 * PI);
        let _ = cr.stroke();
    }
}

fn draw_sun_icon(cr: &Context, x: f64, y: f64, color: Rgba) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    let cx = x + 9.0;
    let cy = y + 9.0;
    cr.new_sub_path();
    cr.arc(cx, cy, 3.5, 0.0, 2.0 * PI);
    let _ = cr.stroke();
    for i in 0..8 {
        let a = (i as f64) * PI / 4.0;
        cr.move_to(cx + 5.5 * a.cos(), cy + 5.5 * a.sin());
        cr.line_to(cx + 7.5 * a.cos(), cy + 7.5 * a.sin());
    }
    let _ = cr.stroke();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_pactl_and_brightnessctl_outputs() {
        let pactl_out = "\
Volume: front-left: 52428 /  80% / -5.81 dB,   front-right: 52428 /  80% / -5.81 dB
        balance 0.00
Mute: no
alsa_output.usb-Logitech_PRO_X_000000000000-00.analog-stereo
Sink #62
\tName: alsa_output.usb-Logitech_PRO_X_000000000000-00.analog-stereo
\tDescription: Logitech G PRO X Gaming Headset Analog Stereo
Sink #6698
\tName: alsa_output.pci-0000_03_00.1.hdmi-stereo
\tDescription: Navi 48 HDMI/DP Audio Controller Digital Stereo (HDMI) [VX2718 Series]
";
        let (pct, muted, sink) = parse_pactl(pactl_out);
        assert_eq!(pct, 80);
        assert!(!muted);
        assert_eq!(sink, "Logitech G PRO X Gaming Headset Analog Stereo");

        let muted_hdmi = "\
Volume: front-left: 26214 /  40% / -23.88 dB
Mute: yes
alsa_output.pci-0000_03_00.1.hdmi-stereo
Sink #62
\tName: alsa_output.usb-Logitech_PRO_X_000000000000-00.analog-stereo
\tDescription: Logitech G PRO X Gaming Headset Analog Stereo
Sink #6698
\tName: alsa_output.pci-0000_03_00.1.hdmi-stereo
\tDescription: Navi 48 HDMI/DP Audio Controller Digital Stereo (HDMI) [VX2718 Series]
";
        let (pct2, muted2, sink2) = parse_pactl(muted_hdmi);
        assert_eq!(pct2, 40);
        assert!(muted2);
        assert_eq!(
            sink2,
            "Navi 48 HDMI/DP Audio Controller Digital Stereo (HDMI) [VX2718 Series]"
        );

        assert_eq!(
            parse_brightness("amdgpu_bl1,backlight,192,75%,255\n"),
            Some(75)
        );
        assert_eq!(
            parse_brightness(
                "Device 'intel_backlight' of class 'backlight':\n\tCurrent brightness: 600 (50%)\n\tMax brightness: 1200\n"
            ),
            Some(50)
        );
        assert_eq!(parse_brightness("192\n255\n"), Some(75));
        assert_eq!(parse_brightness(""), None);
        assert_eq!(parse_brightness("none\n"), None);
    }

    #[test]
    fn volume_and_brightness_click_drag_scroll_choose_commands() {
        let mut world = World::now();
        world.volume = VolumeData {
            ready: true,
            pct: 60,
            muted: false,
            sink: "Built-in Audio".into(),
            brightness: Some(50),
        };
        let w = 380.0;
        let h = height(&world);
        let mid_x = w / 2.0;

        // Click on volume row sets volume to 50%
        assert!(click(&mut world, (w, h), (mid_x, PAD_Y + 20.0), 1));
        assert_eq!(world.volume.pct, 50);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some(
                "pactl set-sink-volume @DEFAULT_SINK@ 50% && pactl set-sink-mute @DEFAULT_SINK@ 0"
            )
        );

        // Drag on brightness row sets brightness to 100%
        assert!(drag(
            &mut world,
            (w, h),
            (w - PAD_X, PAD_Y + SLIDER_ROW_H + 20.0)
        ));
        assert_eq!(world.volume.brightness, Some(100));
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("brightnessctl set 100%")
        );

        // Scroll up on volume row increases by 5%
        assert!(scroll(&mut world, (w, h), (mid_x, PAD_Y + 10.0), -1.0));
        assert_eq!(world.volume.pct, 55);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some(
                "pactl set-sink-volume @DEFAULT_SINK@ 55% && pactl set-sink-mute @DEFAULT_SINK@ 0"
            )
        );

        // Scroll down on brightness row decreases by 5%
        assert!(scroll(
            &mut world,
            (w, h),
            (mid_x, PAD_Y + SLIDER_ROW_H + 10.0),
            1.0
        ));
        assert_eq!(world.volume.brightness, Some(95));
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("brightnessctl set 95%")
        );
    }

    #[test]
    fn a_short_volume_is_one_slider() {
        assert!(compact(32.0) && !compact(COMPACT_H) && !compact(0.0));
        assert_eq!(compact_pct_at(344.0, COMPACT_LEFT), 0);
        assert_eq!(compact_pct_at(344.0, 344.0 - PAD_X - COMPACT_VALUE_W), 100);
    }
}
