//! What every Settings card shares (docs/EDGE_SETTINGS.md section 3): the config as a source, a
//! setting's row as a table entry, and one card type that draws and changes any table. A Settings
//! card file is then its table and what hides its rows.
//!
//! A change shows at once, then one command: `settings.sh log KEY OLD && settings.sh set KEY NEW`,
//! plus the reload the key's tag asks for. The config source is fetched again after it, so the card
//! shows what the file now says.

use crate::card::{Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::cards::settings::sq;
use crate::draw::{self, Text};
use crate::provider::Size;
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// How a setting is changed.
#[derive(Clone, Copy, Debug)]
pub enum Control {
    /// a switch: the values written when on and off
    Switch { on: &'static str, off: &'static str },
    /// (value, word): the word at the row's right, the list opening in place
    Pick(&'static [(&'static str, &'static str)]),
    /// (value, word), at most 3: all shown under the name
    Words(&'static [(&'static str, &'static str)]),
    /// a number on a slider, written when let go
    Slide {
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
    },
    /// a whole number with minus and plus
    Step { min: i64, max: i64 },
    /// a line of text, asked with the edge's own question
    Text,
}

/// What a change needs after `settings.sh set` (the key's tag in config/config).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// [save]: the compositor reads the file by itself
    Save,
    /// [use]: read where it is used
    Use,
    /// [restart]: at the compositor's next start
    Restart,
    /// [reload] of a helper: `infynitae-reload PART`
    Reload(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct Setting {
    pub key: &'static str,
    pub name: &'static str,
    /// one plain line under the name (what it does); "" for none
    pub says: &'static str,
    pub control: Control,
    pub after: After,
    /// shown only under More (or at size M)
    pub more: bool,
    /// programs that must be on PATH for the row to show ("gamemoded")
    pub needs: &'static [&'static str],
}

/// The config as the cards read it: the owner's lines, and the shipped defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigData {
    pub now: HashMap<String, String>,
    pub default: HashMap<String, String>,
    /// every program name found on PATH (for `Setting::needs`)
    pub programs: HashSet<String>,
}

impl ConfigData {
    /// The value in effect: the owner's line, else the default, else "".
    pub fn value(&self, key: &str) -> &str {
        self.now
            .get(key)
            .or_else(|| self.default.get(key))
            .map(String::as_str)
            .unwrap_or("")
    }
    pub fn has(&self, program: &str) -> bool {
        self.programs.contains(program)
    }
}

/// `KEY=value` lines as a map (values unquoted as the edge reads them).
pub fn parse_config(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let l = line.trim_start();
        if l.starts_with('#') {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        out.insert(k.to_string(), crate::config::value_text(v));
    }
    out
}

/// The config files and PATH, read off the UI thread.
pub struct ConfigSource {
    pub config: PathBuf,
    pub defaults: PathBuf,
    pub path: String,
}

impl ConfigSource {
    pub fn system() -> ConfigSource {
        let home = std::env::var("HOME").unwrap_or_default();
        ConfigSource {
            config: crate::config::config_path(),
            defaults: PathBuf::from(&home).join("infynitae/config/config"),
            path: std::env::var("PATH").unwrap_or_default(),
        }
    }
}

impl Source for ConfigSource {
    fn fetch(&mut self) -> Data {
        let read = |p: &PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        let mut programs = HashSet::new();
        for dir in self.path.split(':').filter(|d| !d.is_empty()) {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    if let Some(n) = e.file_name().to_str() {
                        programs.insert(n.to_string());
                    }
                }
            }
        }
        Box::new(ConfigData {
            now: parse_config(&read(&self.config)),
            default: parse_config(&read(&self.defaults)),
            programs,
        })
    }
}

/// The command a change runs: the old value logged (Undo reads it), the new one set, then the
/// key's reload.
pub fn change_command(key: &str, old: Option<&str>, new: &str, after: After) -> String {
    let s = "sh \"$HOME/infynitae/live/settings.sh\"";
    let old = old.map_or("-".to_string(), sq);
    let mut cmd = format!(
        "{s} log {key} {old} && {s} set {key} {} >/dev/null 2>&1",
        sq(new)
    );
    if let After::Reload(part) = after {
        cmd += &format!(" && sh \"$HOME/infynitae/live/infynitae-reload\" {part} >/dev/null 2>&1");
    }
    cmd
}

// hits: a row is ROW + 10 * i, its control ROW + 10 * i + 1.. (words up to +4, minus +1 and
// plus +2 for a step); the open list's items LIST + j; More MORE.
const ROW: u32 = 100;
const LIST: u32 = 5000;
const MORE: u32 = 9000;
const WORDS_H: f64 = 34.0;
const SLIDE_H: f64 = 22.0;

/// One card of Settings drawn from its table.
pub struct TableCard {
    pub table: &'static [Setting],
    data: Option<ConfigData>,
    /// a value changed here, shown before the file says it
    pending: HashMap<&'static str, String>,
    /// keys of [restart] settings changed since the edge started
    restart: HashSet<&'static str>,
    /// the row whose list is open, and the list
    pick: Option<usize>,
    list: ui::List,
    /// More opened (size S)
    more: bool,
    query: String,
    /// a slider held: its row and the value under the pointer
    sliding: Option<(usize, f64)>,
    /// each shown row's rectangle in the last drawing (sliders read the pointer against it)
    slide_rects: std::cell::RefCell<HashMap<usize, Rect>>,
}

impl TableCard {
    pub fn new(table: &'static [Setting]) -> TableCard {
        TableCard {
            table,
            data: None,
            pending: HashMap::new(),
            restart: HashSet::new(),
            pick: None,
            list: ui::List::default(),
            more: false,
            query: String::new(),
            sliding: None,
            slide_rects: Default::default(),
        }
    }

    fn value(&self, i: usize) -> String {
        let s = &self.table[i];
        if let Some((row, v)) = self.sliding.filter(|(r, _)| *r == i) {
            let _ = row;
            return fmt_number(v);
        }
        if let Some(v) = self.pending.get(s.key) {
            return v.clone();
        }
        self.data
            .as_ref()
            .map_or(String::new(), |d| d.value(s.key).to_string())
    }

    fn matches(&self, s: &Setting) -> bool {
        let hay = format!("{} {} {}", s.name, s.says, s.key).to_lowercase();
        self.query
            .split_whitespace()
            .all(|w| hay.contains(&w.to_lowercase()))
    }

    /// The rows shown, in order (hidden ones left out), and whether a More row follows.
    fn shown(&self, cx: &Cx) -> (Vec<usize>, bool) {
        let Some(d) = &self.data else {
            return (Vec::new(), false);
        };
        let searching = !self.query.trim().is_empty();
        let all = cx.size != Size::S || self.more || searching;
        let mut rows = Vec::new();
        let mut hidden_more = false;
        for (i, s) in self.table.iter().enumerate() {
            if !s.needs.iter().all(|p| d.has(p)) || (searching && !self.matches(s)) {
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
            + match self.table[i].control {
                Control::Words(_) => WORDS_H,
                Control::Slide { .. } => SLIDE_H,
                _ => 0.0,
            }
            + if self.pick == Some(i) {
                match self.table[i].control {
                    Control::Pick(opts) => self.list.height(opts.len()),
                    _ => 0.0,
                }
            } else {
                0.0
            }
    }

    /// The word shown for the value in effect (the value itself when no word names it).
    fn word(&self, i: usize) -> String {
        let v = self.value(i);
        let opts: &[(&str, &str)] = match self.table[i].control {
            Control::Pick(o) | Control::Words(o) => o,
            _ => &[],
        };
        opts.iter()
            .find(|(val, _)| val.eq_ignore_ascii_case(&v))
            .map_or(v.clone(), |(_, w)| w.to_string())
    }

    fn is_default(&self, i: usize) -> bool {
        let key = self.table[i].key;
        let Some(d) = &self.data else { return true };
        self.value(i) == d.default.get(key).map(String::as_str).unwrap_or("")
    }

    /// Changes row `i` to `new`: shown at once, then the command.
    fn set(&mut self, i: usize, new: String, out: &mut Vec<Action>) -> bool {
        let s = self.table[i];
        let old_line = self.data.as_ref().and_then(|d| d.now.get(s.key).cloned());
        if self.value(i) == new {
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
        let s = &self.table[i];
        if self.restart.contains(s.key) {
            return "At the next start".into();
        }
        s.says.to_string()
    }
}

fn fmt_number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
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

/// A value at a row's right: muted, in the accent when it is not the default.
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

impl Card for TableCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "config", data.downcast_ref::<ConfigData>()) else {
            return false;
        };
        // what the file says now replaces what was shown before it
        self.pending.retain(|k, v| d.value(k) != v.as_str());
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
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
            let s = &self.table[i];
            let id = ROW + 10 * i as u32;
            let h = self.row_h(i);
            let r = (body.0, y, body.2, ui::ROW_H);
            let state = self.state(i);
            let changed = !self.is_default(i) || self.restart.contains(s.key);
            match s.control {
                Control::Switch { on, .. } => {
                    let is_on = self.value(i).eq_ignore_ascii_case(on);
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
                    let vw = 12.0 + word.chars().count() as f64 * 7.0;
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
                    value_at_right(cr, cx, r, right, &word, changed);
                    if self.pick == Some(i) && self.list.shown() {
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
                                val.eq_ignore_ascii_case(&cur),
                                None,
                            );
                        }
                    }
                }
                Control::Words(opts) => {
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
                            right: 0.0,
                        },
                    );
                    let cur = self.value(i);
                    let words: Vec<&str> = opts.iter().map(|(_, w)| *w).collect();
                    let chosen = opts
                        .iter()
                        .position(|(v, _)| v.eq_ignore_ascii_case(&cur))
                        .unwrap_or(usize::MAX);
                    ui::choice(
                        cr,
                        cx,
                        hits,
                        id + 1,
                        (body.0 + 8.0, y + ui::ROW_H, body.2 - 16.0, WORDS_H - 4.0),
                        &words,
                        chosen,
                    );
                }
                Control::Slide { min, max, unit, .. } => {
                    let v: f64 = self.value(i).parse().unwrap_or(min);
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
                            right: 60.0,
                        },
                    );
                    value_at_right(
                        cr,
                        cx,
                        r,
                        right,
                        &format!("{}{unit}", fmt_number(v)),
                        changed,
                    );
                    let sr = (body.0 + 8.0, y + ui::ROW_H, body.2 - 16.0, SLIDE_H - 4.0);
                    let part = if max > min {
                        (v - min) / (max - min)
                    } else {
                        0.0
                    };
                    ui::slider(cr, cx, hits, id + 1, sr, part);
                    slides.insert(i, sr);
                }
                Control::Step { .. } => {
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
                            right: 96.0,
                        },
                    );
                    let v = self.value(i);
                    let bw = ui::button(cr, cx, hits, id + 2, right - 26.0, y + 7.0, "+");
                    let _ = bw;
                    value_at_right(cr, cx, r, right - 32.0, &v, changed);
                    ui::button(cr, cx, hits, id + 1, right - 90.0, y + 7.0, "−");
                }
                Control::Text => {
                    let v = self.value(i);
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
                    let shown = if v.is_empty() {
                        "Not set".to_string()
                    } else {
                        v
                    };
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
                    let Control::Pick(opts) = self.table[i].control else {
                        return false;
                    };
                    let Some((val, _)) = opts.get((hit - LIST) as usize) else {
                        return false;
                    };
                    self.list.open = false;
                    self.set(i, val.to_string(), out);
                    return true;
                }
                if hit < ROW {
                    return false;
                }
                let i = ((hit - ROW) / 10) as usize;
                let part = (hit - ROW) % 10;
                let Some(s) = self.table.get(i).copied() else {
                    return false;
                };
                match s.control {
                    Control::Switch { on, off } => {
                        let is_on = self.value(i).eq_ignore_ascii_case(on);
                        self.set(i, if is_on { off } else { on }.to_string(), out);
                        true
                    }
                    Control::Pick(_) => {
                        if self.pick == Some(i) {
                            self.list.toggle();
                        } else {
                            self.pick = Some(i);
                            self.list = ui::List::default();
                            self.list.toggle();
                        }
                        true
                    }
                    Control::Words(opts) => {
                        let Some(j) = part.checked_sub(1) else {
                            return false;
                        };
                        let Some((val, _)) = opts.get(j as usize) else {
                            return false;
                        };
                        self.set(i, val.to_string(), out)
                    }
                    Control::Slide { min, max, step, .. } if part == 1 => {
                        let r = self.slide_rects.borrow().get(&i).copied();
                        let Some(r) = r else { return false };
                        let v = snap(min + ui::slider_part(r, *at) * (max - min), min, max, step);
                        self.sliding = Some((i, v));
                        true
                    }
                    Control::Step { min, max } if part == 1 || part == 2 => {
                        let v: i64 = self.value(i).parse().unwrap_or(min);
                        let n = if part == 1 { v - 1 } else { v + 1 }.clamp(min, max);
                        self.set(i, n.to_string(), out)
                    }
                    Control::Text => {
                        let s = self.table[i];
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
                let Control::Slide { min, max, step, .. } = self.table[i].control else {
                    return false;
                };
                let r = self.slide_rects.borrow().get(&i).copied();
                let Some(r) = r else { return false };
                let v = snap(min + ui::slider_part(r, *at) * (max - min), min, max, step);
                self.sliding = Some((i, v));
                true
            }
            // a slider writes when let go: one line in the log, one apply
            Event::Release { .. } => {
                let Some((i, v)) = self.sliding.take() else {
                    return false;
                };
                self.set(i, fmt_number(v), out);
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, dt_ms: f64) -> bool {
        self.list.tick(dt_ms)
    }

    fn show_list(&mut self) {
        if let Some(i) = self
            .table
            .iter()
            .position(|s| matches!(s.control, Control::Pick(_)))
        {
            self.pick = Some(i);
            self.list.open = true;
            self.list.settle();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::theme::Theme;

    const T: &[Setting] = &[
        Setting {
            key: "GAME_MODE",
            name: "Game mode",
            says: "",
            control: Control::Switch {
                on: "true",
                off: "false",
            },
            after: After::Save,
            more: false,
            needs: &["gamemoded"],
        },
        Setting {
            key: "TEARING",
            name: "Tearing in fullscreen",
            says: "",
            control: Control::Pick(&[("always", "Always"), ("true", "When the game asks")]),
            after: After::Save,
            more: false,
            needs: &[],
        },
        Setting {
            key: "FONT_SIZE",
            name: "Font size",
            says: "",
            control: Control::Step { min: 6, max: 30 },
            after: After::Reload("fonts"),
            more: true,
            needs: &[],
        },
    ];

    pub fn data(now: &[(&str, &str)], def: &[(&str, &str)], progs: &[&str]) -> ConfigData {
        let m = |p: &[(&str, &str)]| {
            p.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        ConfigData {
            now: m(now),
            default: m(def),
            programs: progs.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    #[test]
    fn reads_both_quotings_and_skips_comments() {
        let m = parse_config("# x\nA=\"one\"\nB='two'\nC=three # note\n  # D=4\nbad key=1\n");
        assert_eq!(m["A"], "one");
        assert_eq!(m["B"], "two");
        assert_eq!(m["C"], "three");
        assert!(!m.contains_key("D"));
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn the_command_logs_sets_and_reloads_with_every_value_quoted() {
        assert_eq!(
            change_command("TEARING", Some("true"), "always", After::Save),
            "sh \"$HOME/infynitae/live/settings.sh\" log TEARING 'true' && sh \"$HOME/infynitae/live/settings.sh\" set TEARING 'always' >/dev/null 2>&1"
        );
        let c = change_command("FONT", None, "a'b; rm -rf /", After::Reload("fonts"));
        assert!(c.contains("log FONT - &&"));
        assert!(c.contains("set FONT 'a'\\''b; rm -rf /'"));
        assert!(
            c.ends_with("&& sh \"$HOME/infynitae/live/infynitae-reload\" fonts >/dev/null 2>&1")
        );
    }

    #[test]
    fn rows_hide_without_their_program_and_more_folds_at_s() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = TableCard::new(T);
        assert_eq!(c.status(&cx), Status::Loading);
        c.apply(
            "config",
            &data(&[], &[("TEARING", "true")], &[]) as &dyn Any,
        );
        // no gamemoded: Game mode hidden; FONT_SIZE under More
        assert_eq!(c.shown(&cx), (vec![1], true));
        assert!(c.event(&press(MORE), &cx, &mut Vec::new()));
        assert_eq!(c.shown(&cx), (vec![1, 2], true));
        c.apply("config", &data(&[], &[], &["gamemoded"]) as &dyn Any);
        assert_eq!(c.shown(&cx).0, vec![0, 1, 2]);
    }

    #[test]
    fn a_change_shows_at_once_and_runs_once() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = TableCard::new(T);
        c.apply(
            "config",
            &data(&[("GAME_MODE", "false")], &[], &["gamemoded"]) as &dyn Any,
        );
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 1), &cx, &mut out));
        assert_eq!(c.value(0), "true");
        assert_eq!(
            out,
            vec![Action::Run(change_command(
                "GAME_MODE",
                Some("false"),
                "true",
                After::Save
            ))]
        );
        // the file catching up drops what was pending
        c.apply(
            "config",
            &data(&[("GAME_MODE", "true")], &[], &["gamemoded"]) as &dyn Any,
        );
        assert!(c.pending.is_empty());
    }

    #[test]
    fn a_pick_opens_its_list_and_a_choice_sets_it() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = TableCard::new(T);
        c.apply(
            "config",
            &data(&[], &[("TEARING", "true")], &[]) as &dyn Any,
        );
        assert_eq!(c.word(1), "When the game asks");
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10), &cx, &mut out));
        assert!(c.list.open && c.pick == Some(1));
        assert!(c.event(&press(LIST), &cx, &mut out));
        assert_eq!(c.value(1), "always");
        assert!(!c.list.open);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_step_stays_in_its_range_and_typing_filters_rows() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = TableCard::new(T);
        c.apply(
            "config",
            &data(&[("FONT_SIZE", "30")], &[], &[]) as &dyn Any,
        );
        let mut out = Vec::new();
        assert!(
            !c.event(&press(ROW + 22), &cx, &mut out),
            "already at the top"
        );
        assert!(c.event(&press(ROW + 21), &cx, &mut out));
        assert_eq!(c.value(2), "29");
        c.event(&Event::Text("tearing"), &cx, &mut out);
        assert_eq!(c.shown(&cx), (vec![1], false));
        c.event(&Event::Text("nothing like it"), &cx, &mut out);
        assert_eq!(c.status(&cx), Status::Hidden);
    }

    #[test]
    fn a_text_row_asks_and_never_puts_the_answer_in_the_command() {
        const TX: &[Setting] = &[Setting {
            key: "BROWSER",
            name: "Browser",
            says: "",
            control: Control::Text,
            after: After::Use,
            more: false,
            needs: &[],
        }];
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = TableCard::new(TX);
        c.apply(
            "config",
            &data(&[("BROWSER", "fire'fox")], &[], &[]) as &dyn Any,
        );
        let mut out = Vec::new();
        assert!(c.event(&press(ROW), &cx, &mut out));
        let Action::Run(cmd) = &out[0] else { panic!() };
        assert!(cmd.contains("ask --text --default 'fire'\\''fox' 'Browser'"));
        assert!(cmd.contains("set BROWSER \"$v\""));
    }

    #[test]
    fn height_matches_what_is_drawn() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = TableCard::new(T);
        c.apply("config", &data(&[], &[], &["gamemoded"]) as &dyn Any);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 400).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 360.0, 400.0), &mut hits);
        let bottom = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        assert!((bottom - c.height(&cx, 360.0)).abs() < 0.5, "{bottom}");
    }
}
