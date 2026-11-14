//! Settings: Keyboard and mouse (`docs/EDGE_SETTINGS.md`, `settings/input`): the "Keyboard &
//! mouse" keys of config/config (plus `XWAYLAND_KEYS`). Layouts uses the `ask` text prompt rather
//! than an `evdev.lst` picker.

use super::settings::sq;
use super::settings_rows::{change_command, After, ConfigData, Control, Setting};
use crate::card::{Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Size};
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const KIND: CardKind = CardKind {
    id: "input",
    title: "Keyboard and mouse",
    about: "Layouts, key repeat, pointer speed, touchpad",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "touchpad"],
};

const ON_OFF: Control = Control::Switch {
    on: "true",
    off: "false",
};

const XKB_OPTIONS: &[(&str, &str)] = &[
    ("", "None"),
    ("grp:alt_shift_toggle", "Alt+Shift"),
    ("grp:win_space_toggle", "Super+Space"),
    ("grp:caps_toggle", "Caps Lock"),
];

pub const TABLE: &[Setting] = &[
    Setting {
        key: "XKB_DEFAULT_LAYOUT",
        name: "Layouts",
        says: "",
        control: Control::Text,
        after: After::Restart,
        more: false,
        needs: &[],
    },
    Setting {
        key: "KEYBOARD_REPEAT_DELAY",
        name: "Repeat starts after",
        says: "",
        control: Control::Slide {
            min: 150.0,
            max: 600.0,
            step: 10.0,
            unit: " ms",
        },
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "KEYBOARD_REPEAT_RATE",
        name: "Repeat speed",
        says: "",
        control: Control::Slide {
            min: 10.0,
            max: 60.0,
            step: 1.0,
            unit: " a second",
        },
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "MOUSE_ACCEL_SPEED",
        name: "Pointer speed",
        says: "",
        control: Control::Slide {
            min: -1.0,
            max: 1.0,
            step: 0.05,
            unit: "",
        },
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "MOUSE_ACCEL_PROFILE",
        name: "Acceleration",
        says: "Off: the pointer moves 1:1",
        control: Control::Switch {
            on: "adaptive",
            off: "flat",
        },
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "TOUCHPAD_TAP",
        name: "Tap to click",
        says: "",
        control: ON_OFF,
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "TOUCHPAD_NATURAL_SCROLL",
        name: "Scroll follows the fingers",
        says: "",
        control: ON_OFF,
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "SUPER_FREES_CURSOR",
        name: "Super frees a captured pointer",
        says: "",
        control: ON_OFF,
        after: After::Save,
        more: true,
        needs: &[],
    },
    Setting {
        key: "XKB_DEFAULT_OPTIONS",
        name: "Switch layouts with",
        says: "",
        control: Control::Pick(XKB_OPTIONS),
        after: After::Restart,
        more: true,
        needs: &[],
    },
    Setting {
        key: "XWAYLAND_KEYS",
        name: "Keys heard by X11 apps",
        says: "",
        control: Control::Pick(&[
            ("off", "None"),
            ("modifiers", "Modifiers"),
            ("shortcuts", "Shortcuts"),
            ("all", "All"),
        ]),
        after: After::Save,
        more: true,
        needs: &[],
    },
];

/// Whether `/proc/bus/input/devices` lists a device with "Touchpad" in its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchpadData {
    pub present: bool,
}

/// Checks `N: Name=` lines in `/proc/bus/input/devices` for `"Touchpad"` (case-insensitive so
/// `TouchPad` on Synaptics devices matches too).
pub fn parse_touchpad(devices_text: &str) -> TouchpadData {
    let present = devices_text.lines().any(|line| {
        line.trim_start()
            .strip_prefix("N: Name=")
            .is_some_and(|name| name.to_ascii_lowercase().contains("touchpad"))
    });
    TouchpadData { present }
}

const PROC_INPUT_DEVICES: &str = "/proc/bus/input/devices";

/// Reads `/proc/bus/input/devices` off the UI thread (`path` is overridable in tests).
pub struct TouchpadSource {
    pub path: PathBuf,
}

impl TouchpadSource {
    pub fn system() -> TouchpadSource {
        TouchpadSource {
            path: PathBuf::from(PROC_INPUT_DEVICES),
        }
    }
}

impl Source for TouchpadSource {
    fn fetch(&mut self) -> Data {
        if self.path == Path::new(PROC_INPUT_DEVICES) {
            if let Some(fixture) = std::env::var_os("INFYNITAE_EDGE_RENDER_DATA")
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|t| crate::card::parse_render_data(&t).remove("touchpad"))
            {
                return Box::new(parse_touchpad(&fixture));
            }
        }
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        Box::new(parse_touchpad(&text))
    }

    fn every(&self) -> Option<std::time::Duration> {
        None
    }
}

/// How many non-empty comma-separated layouts are configured.
pub fn layout_count(value: &str) -> usize {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .count()
}

// Same hit numbering and geometry as `TableCard` in `settings_rows.rs`.
const ROW: u32 = 100;
const LIST: u32 = 5000;
const MORE: u32 = 9000;
const SLIDE_H: f64 = 22.0;

/// Keyboard and mouse settings card (`settings/input`).
#[derive(Default)]
pub struct InputCard {
    data: Option<ConfigData>,
    touchpad: Option<TouchpadData>,
    pending: HashMap<&'static str, String>,
    restart: HashSet<&'static str>,
    pick: Option<usize>,
    list: ui::List,
    more: bool,
    query: String,
    sliding: Option<(usize, f64)>,
    slide_rects: RefCell<HashMap<usize, Rect>>,
}

impl InputCard {
    fn value(&self, i: usize) -> String {
        let s = &TABLE[i];
        if let Some((_, v)) = self.sliding.filter(|(r, _)| *r == i) {
            if let Control::Slide { step, .. } = s.control {
                return fmt_slide(v, step);
            }
        }
        if let Some(v) = self.pending.get(s.key) {
            return v.clone();
        }
        self.data
            .as_ref()
            .map_or(String::new(), |d| d.value(s.key).to_string())
    }

    fn has_touchpad(&self) -> bool {
        self.touchpad.is_some_and(|t| t.present)
    }

    fn multi_layout(&self) -> bool {
        let idx = TABLE
            .iter()
            .position(|s| s.key == "XKB_DEFAULT_LAYOUT")
            .unwrap_or(0);
        layout_count(&self.value(idx)) > 1
    }

    fn row_available(&self, i: usize) -> bool {
        let Some(d) = &self.data else {
            return false;
        };
        if !TABLE[i].needs.iter().all(|p| d.has(p)) {
            return false;
        }
        match TABLE[i].key {
            "TOUCHPAD_TAP" | "TOUCHPAD_NATURAL_SCROLL" => self.has_touchpad(),
            "XKB_DEFAULT_OPTIONS" => self.multi_layout(),
            _ => true,
        }
    }

    fn is_custom_xkb_options(&self, i: usize) -> bool {
        if TABLE[i].key != "XKB_DEFAULT_OPTIONS" {
            return false;
        }
        let v = self.value(i);
        let t = v.trim();
        !t.is_empty()
            && !XKB_OPTIONS
                .iter()
                .any(|(opt, _)| opt.eq_ignore_ascii_case(t))
    }

    fn matches(&self, s: &Setting) -> bool {
        let hay = format!("{} {} {}", s.name, s.says, s.key).to_lowercase();
        self.query
            .split_whitespace()
            .all(|w| hay.contains(&w.to_lowercase()))
    }

    fn shown(&self, cx: &Cx) -> (Vec<usize>, bool) {
        if self.data.is_none() {
            return (Vec::new(), false);
        }
        let searching = !self.query.trim().is_empty();
        let all = cx.size != Size::S || self.more || searching;
        let mut rows = Vec::new();
        let mut hidden_more = false;
        for (i, s) in TABLE.iter().enumerate() {
            if !self.row_available(i) || (searching && !self.matches(s)) {
                continue;
            }
            if s.more && !all {
                hidden_more = true;
                continue;
            }
            rows.push(i);
        }
        let more_row = !searching && cx.size == Size::S && (hidden_more || self.more);
        (rows, more_row)
    }

    fn row_h(&self, i: usize) -> f64 {
        ui::ROW_H
            + match TABLE[i].control {
                Control::Slide { .. } => SLIDE_H,
                _ => 0.0,
            }
            + if self.pick == Some(i) {
                match TABLE[i].control {
                    Control::Pick(opts) => self.list.height(opts.len()),
                    _ => 0.0,
                }
            } else {
                0.0
            }
    }

    fn word(&self, i: usize) -> String {
        let s = &TABLE[i];
        let v = self.value(i);
        match s.control {
            Control::Pick(opts) => {
                if self.is_custom_xkb_options(i) {
                    return "Set in the file".to_string();
                }
                let t = v.trim();
                opts.iter()
                    .find(|(val, _)| val.eq_ignore_ascii_case(t))
                    .map_or(v, |(_, w)| (*w).to_string())
            }
            Control::Text => {
                if v.is_empty() {
                    if s.key == "XKB_DEFAULT_LAYOUT" {
                        "The system's".to_string()
                    } else {
                        "Not set".to_string()
                    }
                } else {
                    v
                }
            }
            _ => v,
        }
    }

    fn is_default(&self, i: usize) -> bool {
        let s = &TABLE[i];
        let Some(d) = &self.data else { return true };
        let def = d.default.get(s.key).map(String::as_str).unwrap_or("");
        values_equal(s.control, &self.value(i), def)
    }

    fn set(&mut self, i: usize, new: String, out: &mut Vec<Action>) -> bool {
        let s = TABLE[i];
        let old_line = self.data.as_ref().and_then(|d| d.now.get(s.key).cloned());
        if values_equal(s.control, &self.value(i), &new) {
            return false;
        }
        out.push(Action::Run(change_command(
            s.key,
            old_line.as_deref(),
            &new,
            s.after,
        )));
        if s.after == After::Restart {
            self.restart.insert(s.key);
        }
        self.pending.insert(s.key, new);
        true
    }

    fn state(&self, i: usize) -> String {
        let s = &TABLE[i];
        if self.restart.contains(s.key) {
            return "At the next start".into();
        }
        s.says.to_string()
    }
}

fn values_equal(control: Control, a: &str, b: &str) -> bool {
    match control {
        Control::Slide { .. } => match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
            (Ok(x), Ok(y)) => (x - y).abs() < 1e-9,
            _ => a == b,
        },
        Control::Switch { .. } | Control::Pick(_) => a.trim().eq_ignore_ascii_case(b.trim()),
        _ => a == b,
    }
}

fn fmt_slide(v: f64, step: f64) -> String {
    let v = if v.abs() < 1e-9 { 0.0 } else { v };
    if step < 1.0 && (v - v.round()).abs() < 1e-9 {
        format!("{v:.1}")
    } else if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn snap(v: f64, min: f64, max: f64, step: f64) -> f64 {
    let v = v.clamp(min, max);
    if step > 0.0 {
        (min + ((v - min) / step).round() * step).clamp(min, max)
    } else {
        v
    }
}

fn value_at_right(cr: &Context, cx: &Cx, r: Rect, x_right: f64, text: &str, changed: bool) -> f64 {
    let t = Text {
        size: 12.0,
        bold: false,
        color: if changed {
            cx.theme.accent_text
        } else {
            cx.theme.muted
        },
        text,
    };
    draw::text_right(cr, x_right, r.1 + (r.3 - 15.0) / 2.0, &t)
}

impl Card for InputCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        match key {
            "config" => {
                let Some(d) = data.downcast_ref::<ConfigData>() else {
                    return false;
                };
                self.pending.retain(|k, v| {
                    let file_v = d.value(k);
                    TABLE
                        .iter()
                        .find(|s| s.key == *k)
                        .is_none_or(|s| !values_equal(s.control, file_v, v))
                });
                let changed = self.data.as_ref() != Some(d);
                self.data = Some(d.clone());
                if let Some(p) = self.pick {
                    if !self.row_available(p) || self.is_custom_xkb_options(p) {
                        self.pick = None;
                        self.list.open = false;
                    }
                }
                changed
            }
            "touchpad" => {
                let Some(t) = data.downcast_ref::<TouchpadData>() else {
                    return false;
                };
                let changed = self.touchpad.as_ref() != Some(t);
                self.touchpad = Some(*t);
                changed
            }
            _ => false,
        }
    }

    fn status(&self, cx: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(_) if self.shown(cx).0.is_empty() => Status::Hidden,
            Some(_) => Status::Ready,
        }
    }

    fn height(&self, cx: &Cx, _: f64) -> f64 {
        let (rows, more) = self.shown(cx);
        rows.iter().map(|&i| self.row_h(i)).sum::<f64>() + if more { ui::ROW_H } else { 0.0 }
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let (rows, more) = self.shown(cx);
        let mut y = body.1;
        let right = body.0 + body.2 - 8.0;
        let mut slides = HashMap::new();
        for i in rows {
            let s = &TABLE[i];
            let id = ROW + 10 * i as u32;
            let h = self.row_h(i);
            let r = (body.0, y, body.2, ui::ROW_H);
            let state = self.state(i);
            let changed = !self.is_default(i) || self.restart.contains(s.key);
            match s.control {
                Control::Switch { on, .. } => {
                    let is_on = self.value(i).trim().eq_ignore_ascii_case(on);
                    ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state: &state,
                            on: self.restart.contains(s.key),
                            right: 46.0,
                        },
                    );
                    ui::switch(cr, cx, hits, id + 1, r, right, is_on);
                }
                Control::Pick(opts) => {
                    let word = self.word(i);
                    let vw = 40.0 + word.chars().count() as f64 * 7.0;
                    let locked = self.is_custom_xkb_options(i);
                    let row_spec = Row {
                        icon: None,
                        name: s.name,
                        state: &state,
                        on: self.restart.contains(s.key),
                        right: vw.min(body.2 * 0.6),
                    };
                    let lit = if locked {
                        // Other XKB options set in the file must not be overwritten from here.
                        let mut dummy = Hits::default();
                        ui::row(cr, cx, &mut dummy, id, r, &row_spec);
                        false
                    } else {
                        ui::row(cr, cx, hits, id, r, &row_spec)
                    };
                    if locked {
                        value_at_right(cr, cx, r, right, &word, changed);
                    } else {
                        let open = self.pick == Some(i) && self.list.shown();
                        ui::value_box(cr, cx, r, right, &word, lit, open);
                    }
                    if !locked && self.pick == Some(i) && self.list.shown() {
                        let lh = self.list.height(opts.len());
                        let lr = (body.0, y + ui::ROW_H, body.2, lh);
                        ui::list_box(cr, cx, lr);
                        let cur = self.value(i);
                        for (j, (val, w)) in opts.iter().enumerate() {
                            let rr = (
                                body.0 + 4.0,
                                lr.1 + 4.0 + j as f64 * ui::LIST_ROW_H,
                                body.2 - 8.0,
                                ui::LIST_ROW_H,
                            );
                            if rr.1 + rr.3 > lr.1 + lr.3 + 0.5 {
                                break;
                            }
                            ui::list_row(
                                cr,
                                cx,
                                hits,
                                LIST + j as u32,
                                rr,
                                w,
                                "",
                                val.eq_ignore_ascii_case(cur.trim()),
                                None,
                            );
                        }
                    }
                }
                Control::Slide {
                    min,
                    max,
                    step,
                    unit,
                } => {
                    let v: f64 = self.value(i).trim().parse().unwrap_or(min);
                    let label = format!("{}{unit}", fmt_slide(v, step));
                    let vw = (12.0 + label.chars().count() as f64 * 7.0).max(60.0);
                    ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state: &state,
                            on: self.restart.contains(s.key),
                            right: vw.min(body.2 / 2.0),
                        },
                    );
                    value_at_right(cr, cx, r, right, &label, changed);
                    let sr = (body.0 + 8.0, y + ui::ROW_H, body.2 - 16.0, SLIDE_H - 4.0);
                    let part = if max > min {
                        (v - min) / (max - min)
                    } else {
                        0.0
                    };
                    ui::slider(cr, cx, hits, id + 1, sr, part);
                    slides.insert(i, sr);
                }
                Control::Text => {
                    let shown = self.word(i);
                    ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state: &state,
                            on: self.restart.contains(s.key),
                            right: (body.2 / 2.0).min(160.0),
                        },
                    );
                    let t = Text {
                        size: 12.0,
                        bold: false,
                        color: if changed {
                            cx.theme.accent_text
                        } else {
                            cx.theme.muted
                        },
                        text: &shown,
                    };
                    let w = (body.2 / 2.0).min(150.0);
                    let tw = draw::measure(cr, &t).0.min(w);
                    draw::text_max(cr, right - tw, r.1 + (r.3 - 15.0) / 2.0, w, &t);
                }
                Control::Step { .. } => {}
            }
            y += h;
        }
        if more {
            let r = (body.0, y, body.2, ui::ROW_H);
            ui::row(
                cr,
                cx,
                hits,
                MORE,
                r,
                &Row {
                    icon: None,
                    name: if self.more { "Less" } else { "More" },
                    state: "",
                    on: false,
                    right: 0.0,
                },
            );
        }
        *self.slide_rects.borrow_mut() = slides;
    }

    fn event(&mut self, ev: &Event, _cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Text(q) => {
                if self.query == *q {
                    return false;
                }
                self.query = (*q).to_string();
                true
            }
            Event::Press {
                hit,
                button: Button::Left,
                at,
            } => {
                let hit = *hit;
                if hit == MORE {
                    self.more = !self.more;
                    return true;
                }
                if (LIST..MORE).contains(&hit) {
                    let Some(i) = self.pick else { return false };
                    if !self.row_available(i) || self.is_custom_xkb_options(i) {
                        return false;
                    }
                    let Control::Pick(opts) = TABLE[i].control else {
                        return false;
                    };
                    let Some((val, _)) = opts.get((hit - LIST) as usize) else {
                        return false;
                    };
                    self.list.open = false;
                    self.set(i, (*val).to_string(), out);
                    return true;
                }
                if hit < ROW {
                    return false;
                }
                let i = ((hit - ROW) / 10) as usize;
                let part = (hit - ROW) % 10;
                let Some(s) = TABLE.get(i).copied() else {
                    return false;
                };
                if !self.row_available(i) {
                    return false;
                }
                match s.control {
                    Control::Switch { on, off } => {
                        let is_on = self.value(i).trim().eq_ignore_ascii_case(on);
                        self.set(i, if is_on { off } else { on }.to_string(), out);
                        true
                    }
                    Control::Pick(_) => {
                        if self.is_custom_xkb_options(i) {
                            return false;
                        }
                        if self.pick == Some(i) {
                            self.list.toggle();
                        } else {
                            self.pick = Some(i);
                            self.list = ui::List::default();
                            self.list.toggle();
                        }
                        true
                    }
                    Control::Slide { min, max, step, .. } if part == 1 => {
                        let r = self.slide_rects.borrow().get(&i).copied();
                        let Some(r) = r else { return false };
                        let v = snap(min + ui::slider_part(r, *at) * (max - min), min, max, step);
                        self.sliding = Some((i, v));
                        true
                    }
                    Control::Text => {
                        let now = self.value(i);
                        let old_line = self.data.as_ref().and_then(|d| d.now.get(s.key).cloned());
                        // the answer is set by the shell, never put into this command's text
                        let set = change_command(s.key, old_line.as_deref(), "@@V@@", s.after)
                            .replace("'@@V@@'", "\"$v\"");
                        out.push(Action::Run(format!(
                            "v=$(\"$HOME/.local/bin/infynitae-edge\" ask --text --default {} {}) || exit 0; {set}",
                            sq(&now),
                            sq(s.name)
                        )));
                        if s.after == After::Restart {
                            self.restart.insert(s.key);
                        }
                        true
                    }
                    _ => false,
                }
            }
            Event::Drag { hit, at } => {
                let Some((i, _)) = self.sliding else {
                    return false;
                };
                if *hit != ROW + 10 * i as u32 + 1 {
                    return false;
                }
                let Control::Slide { min, max, step, .. } = TABLE[i].control else {
                    return false;
                };
                let r = self.slide_rects.borrow().get(&i).copied();
                let Some(r) = r else { return false };
                let v = snap(min + ui::slider_part(r, *at) * (max - min), min, max, step);
                self.sliding = Some((i, v));
                true
            }
            Event::Release { .. } => {
                let Some((i, v)) = self.sliding.take() else {
                    return false;
                };
                let Control::Slide { step, .. } = TABLE[i].control else {
                    return false;
                };
                self.set(i, fmt_slide(v, step), out);
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, dt_ms: f64) -> bool {
        self.list.tick(dt_ms)
    }

    fn show_list(&mut self) {
        self.more = true;
        if let Some(i) = (0..TABLE.len()).find(|&i| {
            matches!(TABLE[i].control, Control::Pick(_))
                && self.row_available(i)
                && !self.is_custom_xkb_options(i)
        }) {
            self.pick = Some(i);
            self.list.open = true;
            self.list.settle();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_rows::tests::data;
    use crate::provider::Provider;
    use crate::theme::Theme;

    fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    fn row_idx(key: &str) -> usize {
        TABLE.iter().position(|s| s.key == key).unwrap()
    }

    /// Parses the `[tag]` in force for each key in `config/config` (either on the key's comment
    /// block or the section header above it).
    fn config_tags(text: &str) -> HashMap<String, String> {
        let mut section_tag = "save".to_string();
        let mut current_key: Option<String> = None;
        let mut out = HashMap::new();
        for raw in text.lines() {
            let trimmed = raw.trim();
            if trimmed.starts_with("# ═══") {
                if let Some(tag) = last_bracket_tag(trimmed) {
                    section_tag = tag;
                }
                current_key = None;
                continue;
            }
            if let Some(k) = raw.split('=').next().filter(|k| {
                !raw.trim_start().starts_with('#')
                    && !k.is_empty()
                    && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            }) {
                current_key = Some(k.to_string());
                out.insert(k.to_string(), section_tag.clone());
                continue;
            }
            if raw.starts_with("    #") {
                if let (Some(k), Some(tag)) = (&current_key, last_bracket_tag(trimmed)) {
                    if matches!(tag.as_str(), "save" | "reload" | "use" | "restart") {
                        out.insert(k.clone(), tag);
                    }
                }
            } else if trimmed.is_empty() {
                current_key = None;
            }
        }
        out
    }

    fn last_bracket_tag(line: &str) -> Option<String> {
        let open = line.rfind('[')?;
        let close = line[open..].find(']')?;
        Some(line[open + 1..open + close].to_string())
    }

    /// Every key here exists in the shipped config, with a default the controls know, and its
    /// `After` matches the tag in `config/config`.
    #[test]
    fn every_key_is_in_config_config_with_a_value_its_control_has() {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap();
        let def = super::super::settings_rows::parse_config(&text);
        let tags = config_tags(&text);
        for s in TABLE {
            let v = def
                .get(s.key)
                .unwrap_or_else(|| panic!("{} not in config/config", s.key));
            match s.control {
                Control::Switch { on, off } => assert!(v == on || v == off, "{} = {v}", s.key),
                Control::Pick(o) => {
                    assert!(o.iter().any(|(x, _)| x == v), "{} = {v}", s.key)
                }
                Control::Slide { min, max, step, .. } => {
                    let n: f64 = v
                        .parse()
                        .unwrap_or_else(|_| panic!("{} = {v} not float", s.key));
                    assert!(n >= min && n <= max, "{} = {v} out of {min}..{max}", s.key);
                    let steps = (n - min) / step;
                    assert!(
                        (steps - steps.round()).abs() < 1e-9,
                        "{} = {v} off step {step}",
                        s.key
                    );
                }
                _ => {}
            }
            let expected_tag = match s.after {
                After::Save => "save",
                After::Use => "use",
                After::Restart => "restart",
                After::Reload(_) => "reload",
            };
            assert_eq!(
                tags.get(s.key).map(String::as_str),
                Some(expected_tag),
                "tag mismatch for {}",
                s.key
            );
        }
    }

    #[test]
    fn touchpad_source_parses_usual_empty_and_broken_devices_files() {
        // Real-looking desktop output without a touchpad
        let desktop = "\
I: Bus=0019 Vendor=0000 Product=0001 Version=0000
N: Name=\"Power Button\"
P: Phys=PNP0C0C/button/input0

I: Bus=0003 Vendor=3367 Product=1970 Version=0110
N: Name=\"Endgame Gear Endgame Gear HS Dongle\"
P: Phys=usb-0000:75:00.4-2.4/input0
";
        assert!(!parse_touchpad(desktop).present);

        // Laptop output with Synaptics TouchPad and ELAN Touchpad
        let laptop_syn = "I: Bus=0011 Vendor=0002 Product=0007 Version=01b1\nN: Name=\"SynPS/2 Synaptics TouchPad\"\nH: Handlers=mouse1 event8\n";
        let laptop_elan = "I: Bus=0018 Vendor=04f3 Product=309f Version=0100\nN: Name=\"ELAN1200:00 04F3:309F Touchpad\"\nH: Handlers=mouse0 event4\n";
        assert!(parse_touchpad(laptop_syn).present);
        assert!(parse_touchpad(laptop_elan).present);

        // Empty and broken inputs: "Touchpad" outside `N: Name=` does not count
        assert!(!parse_touchpad("").present);
        assert!(!parse_touchpad("garbage\nP: Phys=usb/Touchpad/input0\nN: missing_equals").present);

        // Temporary folder tests for TouchpadSource
        let tmp = std::env::temp_dir().join(format!("infynitae_test_tp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let dev_path = tmp.join("devices");
        std::fs::write(&dev_path, laptop_elan).unwrap();
        let d = TouchpadSource {
            path: dev_path.clone(),
        }
        .fetch();
        assert_eq!(
            d.downcast_ref::<TouchpadData>(),
            Some(&TouchpadData { present: true })
        );

        std::fs::write(&dev_path, "").unwrap();
        let d = TouchpadSource {
            path: dev_path.clone(),
        }
        .fetch();
        assert_eq!(
            d.downcast_ref::<TouchpadData>(),
            Some(&TouchpadData { present: false })
        );

        std::fs::write(&dev_path, "broken\n\x00\nS: Sysfs=/Touchpad\n").unwrap();
        let d = TouchpadSource {
            path: dev_path.clone(),
        }
        .fetch();
        assert_eq!(
            d.downcast_ref::<TouchpadData>(),
            Some(&TouchpadData { present: false })
        );

        let missing = TouchpadSource {
            path: tmp.join("does_not_exist"),
        }
        .fetch();
        assert_eq!(
            missing.downcast_ref::<TouchpadData>(),
            Some(&TouchpadData { present: false })
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn hiding_rules_for_touchpad_rows_and_switch_layouts_with() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let mut cx = test_cx(&th);
        cx.size = Size::M;
        let mut c = InputCard::default();
        assert_eq!(c.status(&cx), Status::Loading);

        // No touchpad, single/system layout (""): hides touchpad rows and XKB_DEFAULT_OPTIONS
        c.apply("config", &data(&[], &[("XKB_DEFAULT_LAYOUT", "")], &[]));
        c.apply("touchpad", &TouchpadData { present: false });
        assert_eq!(c.status(&cx), Status::Ready);
        let tap = row_idx("TOUCHPAD_TAP");
        let nat = row_idx("TOUCHPAD_NATURAL_SCROLL");
        let opts = row_idx("XKB_DEFAULT_OPTIONS");
        let (rows, more) = c.shown(&cx);
        assert!(!more);
        assert!(!rows.contains(&tap) && !rows.contains(&nat) && !rows.contains(&opts));
        assert_eq!(rows.len(), 7);

        // Searching "tap" or "switch" does not reveal hidden rows
        c.event(&Event::Text("tap"), &cx, &mut Vec::new());
        assert_eq!(c.status(&cx), Status::Hidden);
        c.event(&Event::Text("switch"), &cx, &mut Vec::new());
        assert_eq!(c.status(&cx), Status::Hidden);
        c.event(&Event::Text(""), &cx, &mut Vec::new());

        // One explicit layout ("us") or trailing comma ("us,") still hides Switch layouts with
        c.apply("config", &data(&[("XKB_DEFAULT_LAYOUT", "us")], &[], &[]));
        assert!(!c.shown(&cx).0.contains(&opts));
        c.apply("config", &data(&[("XKB_DEFAULT_LAYOUT", "us,")], &[], &[]));
        assert!(!c.shown(&cx).0.contains(&opts));

        // Multiple layouts ("us,de") + touchpad present reveals all 10 rows
        c.apply(
            "config",
            &data(&[("XKB_DEFAULT_LAYOUT", "us,de")], &[], &[]),
        );
        c.apply("touchpad", &TouchpadData { present: true });
        let (rows, _) = c.shown(&cx);
        assert_eq!(rows.len(), 10);
        assert!(rows.contains(&tap) && rows.contains(&nat) && rows.contains(&opts));

        // At S: More hides the 3 M rows and shows a More row
        cx.size = Size::S;
        let (s_rows, more) = c.shown(&cx);
        assert!(more);
        assert_eq!(s_rows.len(), 7);
        assert!(c.event(&press(MORE), &cx, &mut Vec::new()));
        assert_eq!(c.shown(&cx).0.len(), 10);
    }

    #[test]
    fn empty_layout_shows_the_systems_and_custom_xkb_options_stay_unchanged() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let mut cx = test_cx(&th);
        cx.size = Size::M;
        let mut c = InputCard::default();
        c.apply(
            "config",
            &data(
                &[
                    ("XKB_DEFAULT_LAYOUT", "us,de"),
                    ("XKB_DEFAULT_OPTIONS", "caps:swapescape"),
                ],
                &[("XKB_DEFAULT_LAYOUT", ""), ("XKB_DEFAULT_OPTIONS", "")],
                &[],
            ),
        );
        let opts = row_idx("XKB_DEFAULT_OPTIONS");
        assert_eq!(c.word(opts), "Set in the file");

        // Clicking Switch layouts with while custom options are set does not open or change it
        let mut out = Vec::new();
        assert!(!c.event(&press(ROW + 10 * opts as u32), &cx, &mut out));
        assert!(out.is_empty());
        assert!(c.pick.is_none());

        // Drawn hits omit the locked XKB_DEFAULT_OPTIONS row so hover/click stays inactive
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 440, 600).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 400.0, 600.0), &mut hits);
        assert!(!hits
            .list
            .iter()
            .any(|(id, _)| *id == ROW + 10 * opts as u32));

        // Empty layout displays "The system's", non-empty shows the value
        let layout_idx = row_idx("XKB_DEFAULT_LAYOUT");
        assert_eq!(c.word(layout_idx), "us,de");
        c.apply(
            "config",
            &data(
                &[],
                &[("XKB_DEFAULT_LAYOUT", ""), ("XKB_DEFAULT_OPTIONS", "")],
                &[],
            ),
        );
        assert_eq!(c.word(layout_idx), "The system's");
    }

    #[test]
    fn exact_commands_for_every_control_kind() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let mut cx = test_cx(&th);
        cx.size = Size::M;
        let mut c = InputCard::default();
        c.apply(
            "config",
            &data(
                &[
                    ("XKB_DEFAULT_LAYOUT", "us,de"),
                    ("KEYBOARD_REPEAT_DELAY", "200"),
                    ("MOUSE_ACCEL_SPEED", "0.0"),
                    ("MOUSE_ACCEL_PROFILE", "flat"),
                    ("XKB_DEFAULT_OPTIONS", ""),
                    ("XWAYLAND_KEYS", "modifiers"),
                ],
                &[
                    ("XKB_DEFAULT_LAYOUT", ""),
                    ("KEYBOARD_REPEAT_DELAY", "200"),
                    ("MOUSE_ACCEL_SPEED", "0.0"),
                    ("MOUSE_ACCEL_PROFILE", "flat"),
                    ("XKB_DEFAULT_OPTIONS", ""),
                    ("XWAYLAND_KEYS", "modifiers"),
                ],
                &[],
            ),
        );
        c.apply("touchpad", &TouchpadData { present: false });

        // Draw once so slide_rects are populated
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 440, 700).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 400.0, 700.0), &mut hits);

        // 1. Control::Text (Layouts)
        let mut out = Vec::new();
        assert!(c.event(&press(ROW), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "v=$(\"$HOME/.local/bin/infynitae-edge\" ask --text --default 'us,de' 'Layouts') || exit 0; \
                 sh \"$HOME/infynitae/live/settings.sh\" log XKB_DEFAULT_LAYOUT 'us,de' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set XKB_DEFAULT_LAYOUT \"$v\" >/dev/null 2>&1"
                    .to_string()
            )]
        );
        assert_eq!(c.state(0), "At the next start");

        // 2. Control::Slide (Pointer speed): Press + Drag emits nothing; Release emits once
        let speed_idx = row_idx("MOUSE_ACCEL_SPEED");
        let sr = c.slide_rects.borrow()[&speed_idx];
        let mut out = Vec::new();
        let slide_hit = ROW + 10 * speed_idx as u32 + 1;
        assert!(c.event(
            &Event::Press {
                hit: slide_hit,
                button: Button::Left,
                at: (sr.0 + sr.2 * 0.75, sr.1 + sr.3 / 2.0),
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
        assert!(c.event(
            &Event::Drag {
                hit: slide_hit,
                at: (sr.0 + sr.2 * 0.75, sr.1 + sr.3 / 2.0),
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
        assert_eq!(c.value(speed_idx), "0.5");
        assert!(c.event(&Event::Release { hit: slide_hit }, &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log MOUSE_ACCEL_SPEED '0.0' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set MOUSE_ACCEL_SPEED '0.5' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // 3. Control::Switch (Acceleration)
        let accel_idx = row_idx("MOUSE_ACCEL_PROFILE");
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10 * accel_idx as u32 + 1), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log MOUSE_ACCEL_PROFILE 'flat' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set MOUSE_ACCEL_PROFILE 'adaptive' >/dev/null 2>&1"
                    .to_string()
            )]
        );
        assert_eq!(c.value(accel_idx), "adaptive");

        // 4. Control::Pick (Switch layouts with, After::Restart)
        let opts_idx = row_idx("XKB_DEFAULT_OPTIONS");
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10 * opts_idx as u32), &cx, &mut out));
        assert!(out.is_empty() && c.list.open && c.pick == Some(opts_idx));
        assert!(c.event(&press(LIST + 2), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log XKB_DEFAULT_OPTIONS '' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set XKB_DEFAULT_OPTIONS 'grp:win_space_toggle' >/dev/null 2>&1"
                    .to_string()
            )]
        );
        assert_eq!(c.word(opts_idx), "Super+Space");
        assert_eq!(c.state(opts_idx), "At the next start");
    }

    #[test]
    fn height_matches_what_is_drawn_and_provider_registers_input() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let mut cx = test_cx(&th);
        cx.size = Size::M;
        let mut c = InputCard::default();
        c.apply(
            "config",
            &data(&[("XKB_DEFAULT_LAYOUT", "us,de")], &[], &[]),
        );
        c.apply("touchpad", &TouchpadData { present: true });
        c.show_list();

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 440, 800).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 400.0, 800.0), &mut hits);
        let bottom = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        assert!((bottom - c.height(&cx, 400.0)).abs() < 0.5, "{bottom}");

        let p = super::super::settings::Settings;
        assert!(p.kind("input").is_some());
        assert!(p.card("input", None).is_some());
        assert!(p.card("input", Some("x")).is_none());
        let src = p.source("touchpad").unwrap();
        assert_eq!(src.every(), None);
    }
}
