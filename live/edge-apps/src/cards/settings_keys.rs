//! The Keys card (`docs/EDGE_SETTINGS.md`): lists compositor actions with plain doc-comment
//! names and custom `EXEC_*` commands, draws bound chords as key caps, captures the next chord via
//! `infynitae msg capture-chord`, and offers a swap on conflict.

use crate::card::{Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::cards::settings::sq;
use crate::cards::settings_rows::{change_command, After, ConfigData};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Size};
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub const KIND: CardKind = CardKind {
    id: "keys",
    title: "Keys",
    about: "Keyboard shortcuts",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "key_capture"],
};

const CONFLICT_H: f64 = 34.0;
const CHORD_PLACEHOLDER: &str = "__INFYNITAE_CAPTURE_CHORD__";

// Hit IDs MUST stay strictly below settings_all::CHILD_STRIDE (1000).
const HIT_SWAP: u32 = 1;
const HIT_CANCEL: u32 = 2;
const HIT_MORE: u32 = 3;
const HIT_ROW_BASE: u32 = 10;

const MOD_ORDER: [&str; 4] = ["SUPER", "CTRL", "ALT", "SHIFT"];

#[derive(Clone, Copy)]
struct BuiltinAction {
    action: &'static str,
    name: &'static str,
    /// Built-in default chords in config spelling (`keys.rs`); `[0]` is displayed when unset.
    defaults: &'static [&'static str],
    more: bool,
}

/// Built-in compositor actions (`crates/infynitae-core/src/input/keys.rs` `ACTION_NAMES` +
/// `resolve_key_binding`), in config order, with plain names derived from each doc comment.
const ACTIONS: &[BuiltinAction] = &[
    BuiltinAction {
        action: "TERMINAL",
        name: "Open a terminal",
        defaults: &["SUPER+Q"],
        more: false,
    },
    BuiltinAction {
        action: "FILE_MANAGER",
        name: "Open file manager",
        defaults: &["SUPER+E"],
        more: false,
    },
    BuiltinAction {
        action: "LAUNCHER",
        name: "Open app launcher",
        defaults: &["SUPER+W"],
        more: false,
    },
    BuiltinAction {
        action: "WINDOWS",
        name: "List open windows",
        defaults: &["SUPER+L"],
        more: false,
    },
    BuiltinAction {
        action: "OVERVIEW",
        name: "Toggle canvas overview",
        defaults: &["SUPER+SHIFT+TAB"],
        more: false,
    },
    BuiltinAction {
        action: "LOCK",
        name: "Lock the screen",
        defaults: &["SUPER+SHIFT+L"],
        more: false,
    },
    BuiltinAction {
        action: "SCREENSHOT",
        name: "Capture full screenshot",
        defaults: &["SUPER+SHIFT+S", "PRINT"],
        more: false,
    },
    BuiltinAction {
        action: "REGION_SCREENSHOT",
        name: "Capture region screenshot",
        defaults: &["SUPER+S", "SUPER+SHIFT+R"],
        more: false,
    },
    BuiltinAction {
        action: "CLIPBOARD",
        name: "Open clipboard history",
        defaults: &["SUPER+V"],
        more: false,
    },
    BuiltinAction {
        action: "CONTROL",
        name: "Open control menu",
        defaults: &["SUPER+A"],
        more: false,
    },
    BuiltinAction {
        action: "FIT",
        name: "Fit window to view",
        defaults: &["SUPER+RETURN"],
        more: false,
    },
    BuiltinAction {
        action: "FULLSCREEN",
        name: "Toggle window fullscreen",
        defaults: &["SUPER+SHIFT+RETURN"],
        more: false,
    },
    BuiltinAction {
        action: "CLOSE",
        name: "Close focused window",
        defaults: &["SUPER+SHIFT+C"],
        more: false,
    },
    BuiltinAction {
        action: "FORCE_CLOSE",
        name: "Force close window",
        defaults: &["SUPER+SHIFT+DELETE"],
        more: true,
    },
    BuiltinAction {
        action: "FOCUS_NEXT",
        name: "Focus next window",
        defaults: &["SUPER+TAB"],
        more: true,
    },
    BuiltinAction {
        action: "FOCUS_LEFT",
        name: "Focus window on the left",
        defaults: &["SUPER+LEFT"],
        more: true,
    },
    BuiltinAction {
        action: "FOCUS_RIGHT",
        name: "Focus window on the right",
        defaults: &["SUPER+RIGHT"],
        more: true,
    },
    BuiltinAction {
        action: "FOCUS_UP",
        name: "Focus window above",
        defaults: &["SUPER+UP"],
        more: true,
    },
    BuiltinAction {
        action: "FOCUS_DOWN",
        name: "Focus window below",
        defaults: &["SUPER+DOWN"],
        more: true,
    },
    BuiltinAction {
        action: "SYSTEM_MONITOR",
        name: "Open system monitor",
        defaults: &["SUPER+SHIFT+ESC"],
        more: true,
    },
    BuiltinAction {
        action: "RELOAD",
        name: "Reload shell UI",
        defaults: &["SUPER+SHIFT+BACKSPACE"],
        more: true,
    },
    BuiltinAction {
        action: "PRIVACY",
        name: "Toggle privacy mode",
        defaults: &["SUPER+SHIFT+P"],
        more: true,
    },
    BuiltinAction {
        action: "IDLE_INHIBIT",
        name: "Stay awake",
        defaults: &["SUPER+SHIFT+I"],
        more: true,
    },
    BuiltinAction {
        action: "TEARING",
        name: "Allow fullscreen tearing",
        defaults: &["SUPER+SHIFT+O", "SUPER+SHIFT+T"],
        more: true,
    },
    BuiltinAction {
        action: "SAVE_REPLAY",
        name: "Save instant replay",
        defaults: &["SUPER+ALT+C"],
        more: true,
    },
    BuiltinAction {
        action: "RECORD",
        name: "Toggle screen recording",
        defaults: &["SUPER+ALT+P", "SUPER+ALT+R"],
        more: true,
    },
    BuiltinAction {
        action: "KEEP_ON_TOP",
        name: "Keep window on top",
        defaults: &["SUPER+SHIFT+K"],
        more: true,
    },
    BuiltinAction {
        action: "RECENTER",
        name: "Recenter and reset zoom",
        defaults: &["SUPER+SPACE"],
        more: true,
    },
    BuiltinAction {
        action: "RECENTER_KEEP_ZOOM",
        name: "Recenter, keep zoom",
        defaults: &["SUPER+SHIFT+SPACE"],
        more: true,
    },
];

/// Data emitted by the `key_capture` source (`capture_file_path()`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyCaptureData {
    pub token: String,
    pub outcome: CaptureOutcome,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CaptureOutcome {
    #[default]
    Idle,
    Done,
    Cancel,
    Applied(String),
    Conflict {
        chord: String,
        other_key: String,
    },
}

pub struct KeyCaptureSource {
    path: PathBuf,
}

impl KeyCaptureSource {
    pub fn new() -> Self {
        Self {
            path: capture_file_path(),
        }
    }

    pub fn parse(text: &str) -> KeyCaptureData {
        let mut parts = text.trim().split('\t');
        let kind = parts.next().unwrap_or("");
        let token = parts.next().unwrap_or("").to_string();
        if token.is_empty() {
            return KeyCaptureData::default();
        }
        let outcome = match kind {
            "done" => CaptureOutcome::Done,
            "cancel" => CaptureOutcome::Cancel,
            "applied" => match parts.next().map(normalize_chord) {
                Some(chord) if !chord.is_empty() => CaptureOutcome::Applied(chord),
                _ => CaptureOutcome::Cancel,
            },
            "conflict" => {
                let chord = parts.next().map(normalize_chord).unwrap_or_default();
                let other_key = parts.next().unwrap_or("").to_string();
                if chord.is_empty() || other_key.is_empty() {
                    CaptureOutcome::Cancel
                } else {
                    CaptureOutcome::Conflict { chord, other_key }
                }
            }
            _ => return KeyCaptureData::default(),
        };
        KeyCaptureData { token, outcome }
    }
}

impl Source for KeyCaptureSource {
    fn fetch(&mut self) -> Data {
        let data = std::fs::read_to_string(&self.path)
            .map(|t| Self::parse(&t))
            .unwrap_or_default();
        Box::new(data)
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

pub fn capture_file_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
        });
    base.join("infynitae").join("key-capture")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct KeyRow {
    key: String,
    name: String,
    chords: Vec<String>,
    raw_config: Option<String>,
    exec_command: Option<String>,
    more: bool,
}

impl KeyRow {
    fn primary_chord(&self) -> &str {
        self.chords.first().map(String::as_str).unwrap_or("")
    }

    fn is_exec(&self) -> bool {
        self.exec_command.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum UiState {
    Idle,
    Listening {
        key: String,
        token: String,
    },
    Conflict {
        key: String,
        captured: String,
        other_key: String,
        other_name: String,
    },
}

pub struct KeysCard {
    config: Option<ConfigData>,
    pending: HashMap<String, String>,
    state: UiState,
    handled_token: Option<String>,
    more: bool,
    query: String,
}

impl KeysCard {
    pub fn new() -> Self {
        Self {
            config: None,
            pending: HashMap::new(),
            state: UiState::Idle,
            handled_token: None,
            more: false,
            query: String::new(),
        }
    }

    /// True if `key` is a config binding key (`BIND_*` or `EXEC_*`) owned by this card.
    pub fn owns_config_key(key: &str) -> bool {
        key.starts_with("BIND_") || key.starts_with("EXEC_")
    }

    fn raw_value(&self, key: &str) -> Option<&str> {
        if let Some(v) = self.pending.get(key) {
            return Some(v.as_str());
        }
        self.config.as_ref()?.now.get(key).map(String::as_str)
    }

    fn persisted_raw<'a>(&'a self, key: &str) -> Option<&'a str> {
        self.config.as_ref()?.now.get(key).map(String::as_str)
    }

    fn rows(&self) -> Vec<KeyRow> {
        let mut out = Vec::with_capacity(ACTIONS.len());
        for b in ACTIONS {
            let key = format!("BIND_{}", b.action);
            let raw = self.raw_value(&key).map(str::to_string);
            let chords = match &raw {
                Some(v) => parse_bind_chords(v),
                None => b.defaults.iter().map(|s| (*s).to_string()).collect(),
            };
            out.push(KeyRow {
                key,
                name: b.name.to_string(),
                chords,
                raw_config: self
                    .persisted_raw(&format!("BIND_{}", b.action))
                    .map(str::to_string),
                exec_command: None,
                more: b.more,
            });
        }

        // Include custom EXEC_* shortcuts sorted by config key name.
        let mut exec_keys: Vec<String> = self
            .config
            .as_ref()
            .map(|d| {
                d.now
                    .keys()
                    .filter(|k| is_safe_exec_key(k))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for k in self.pending.keys() {
            if is_safe_exec_key(k) && !exec_keys.contains(k) {
                exec_keys.push(k.clone());
            }
        }
        exec_keys.sort();

        for key in exec_keys {
            let Some(val) = self.raw_value(&key) else {
                continue;
            };
            let (chord, command) = match val.split_once(':') {
                Some((c, cmd)) => (normalize_chord(c), cmd.trim().to_string()),
                None => (String::new(), val.trim().to_string()),
            };
            let name = if command.is_empty() {
                fallback_exec_name(&key)
            } else {
                command.clone()
            };
            let chords = if chord.is_empty() {
                Vec::new()
            } else {
                vec![chord]
            };
            out.push(KeyRow {
                key: key.clone(),
                name,
                chords,
                raw_config: self.persisted_raw(&key).map(str::to_string),
                exec_command: Some(command),
                more: false,
            });
        }
        out
    }

    fn visible_rows(&self, cx: &Cx) -> (Vec<KeyRow>, bool) {
        let all = self.rows();
        let searching = !self.query.trim().is_empty();
        let show_all = cx.size != Size::S || self.more || searching;
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();

        let mut shown = Vec::new();
        let mut hidden_more = false;
        for row in all {
            if searching && !row_matches(&row, &words) {
                continue;
            }
            if row.more && !show_all {
                hidden_more = true;
                continue;
            }
            shown.push(row);
        }
        (
            shown,
            hidden_more || (self.more && cx.size == Size::S && !searching),
        )
    }

    fn row_height(&self, row: &KeyRow) -> f64 {
        if matches!(&self.state, UiState::Conflict { key, .. } if key == &row.key) {
            ui::ROW_H + CONFLICT_H
        } else {
            ui::ROW_H
        }
    }

    fn apply_config_change(&mut self, key: &str, new_val: String) {
        self.pending.insert(key.to_string(), new_val);
    }

    fn execute_swap(
        &mut self,
        row: &KeyRow,
        other: &KeyRow,
        captured: &str,
        out: &mut Vec<Action>,
    ) -> bool {
        let old_primary = row.primary_chord();
        if old_primary.is_empty() && other.is_exec() {
            return false;
        }
        let new_a = row_value_for_chord(row, captured);
        let new_b = swap_target_value(other, captured, old_primary);
        let cmd_a = change_command(&row.key, row.raw_config.as_deref(), &new_a, After::Save);
        let cmd_b = change_command(&other.key, other.raw_config.as_deref(), &new_b, After::Save);
        out.push(Action::Run(format!("{cmd_a} && {cmd_b}")));
        self.apply_config_change(&row.key, new_a);
        self.apply_config_change(&other.key, new_b);
        self.state = UiState::Idle;
        true
    }
}

impl Default for KeysCard {
    fn default() -> Self {
        Self::new()
    }
}

impl Card for KeysCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        if key == "config" {
            let Some(d) = data.downcast_ref::<ConfigData>() else {
                return false;
            };
            self.pending
                .retain(|k, v| d.now.get(k).map(String::as_str) != Some(v.as_str()));
            let changed = self.config.as_ref() != Some(d);
            self.config = Some(d.clone());
            return changed;
        }
        if key == "key_capture" {
            let Some(cap) = data.downcast_ref::<KeyCaptureData>() else {
                return false;
            };
            let UiState::Listening {
                key: row_key,
                token,
            } = &self.state
            else {
                return false;
            };
            if cap.token.is_empty()
                || &cap.token != token
                || self.handled_token.as_deref() == Some(token.as_str())
            {
                return false;
            }
            let row_key = row_key.clone();
            let token_done = token.clone();
            self.handled_token = Some(token_done);
            match &cap.outcome {
                CaptureOutcome::Idle => return false,
                CaptureOutcome::Done | CaptureOutcome::Cancel => {
                    self.state = UiState::Idle;
                }
                CaptureOutcome::Applied(chord) => {
                    if let Some(row) = self.rows().into_iter().find(|r| r.key == row_key) {
                        let val = row_value_for_chord(&row, chord);
                        self.apply_config_change(&row_key, val);
                    }
                    self.state = UiState::Idle;
                }
                CaptureOutcome::Conflict { chord, other_key } => {
                    let other_name = self
                        .rows()
                        .into_iter()
                        .find(|r| &r.key == other_key)
                        .map(|r| r.name)
                        .unwrap_or_else(|| "other shortcut".to_string());
                    self.state = UiState::Conflict {
                        key: row_key,
                        captured: chord.clone(),
                        other_key: other_key.clone(),
                        other_name,
                    };
                }
            }
            return true;
        }
        false
    }

    fn status(&self, cx: &Cx) -> Status {
        if self.config.is_none() {
            return Status::Loading;
        }
        let (rows, _) = self.visible_rows(cx);
        if rows.is_empty() {
            Status::Hidden
        } else {
            Status::Ready
        }
    }

    fn height(&self, cx: &Cx, _: f64) -> f64 {
        let (rows, more) = self.visible_rows(cx);
        rows.iter().map(|r| self.row_height(r)).sum::<f64>() + if more { ui::ROW_H } else { 0.0 }
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let (rows, more) = self.visible_rows(cx);
        let mut y = body.1;
        let right = body.0 + body.2 - 8.0;

        for (i, row) in rows.iter().enumerate() {
            let h = self.row_height(row);
            let r = (body.0, y, body.2, ui::ROW_H);
            let id = HIT_ROW_BASE + i as u32;
            let listening =
                matches!(&self.state, UiState::Listening { key, .. } if key == &row.key);
            let conflict = match &self.state {
                UiState::Conflict {
                    key,
                    captured,
                    other_name,
                    ..
                } if key == &row.key => Some((captured.as_str(), other_name.as_str())),
                _ => None,
            };

            if listening {
                let prompt = "Press the new keys";
                let pw = draw::measure(
                    cr,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: cx.theme.accent_text,
                        text: prompt,
                    },
                )
                .0;
                ui::light(cr, cx, r);
                ui::row(
                    cr,
                    cx,
                    hits,
                    id,
                    r,
                    &Row {
                        icon: None,
                        name: &row.name,
                        state: "",
                        on: false,
                        right: (pw + 12.0).min(body.2 / 2.0),
                    },
                );
                draw::text_right(
                    cr,
                    right,
                    r.1 + (r.3 - 15.0) / 2.0,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: cx.theme.accent_text,
                        text: prompt,
                    },
                );
            } else if let Some((captured, other_name)) = conflict {
                let caps: Vec<&str> = chord_cap_labels(captured);
                let caps_w = measure_caps_width(cr, cx, &caps);
                ui::light(cr, cx, (body.0, y, body.2, h));
                ui::row(
                    cr,
                    cx,
                    hits,
                    id,
                    r,
                    &Row {
                        icon: None,
                        name: &row.name,
                        state: "",
                        on: false,
                        right: (caps_w + 8.0).min(body.2 / 2.0),
                    },
                );
                draw_key_caps(cr, cx, r, right, &caps, true);

                let swap_label = format!("Swap with {other_name}");
                let btn_y = y + ui::ROW_H + 3.0;
                let cancel_w = measure_button_width(cr, cx, "Cancel");
                let max_swap_x = (right - cancel_w - 8.0).max(body.0 + 8.0);
                let swap_x = body.0 + 8.0;
                let sw = ui::button(cr, cx, hits, HIT_SWAP, swap_x, btn_y, &swap_label);
                let cancel_x = (swap_x + sw + 8.0).max(max_swap_x).min(right - cancel_w);
                ui::button(cr, cx, hits, HIT_CANCEL, cancel_x, btn_y, "Cancel");
            } else if row.primary_chord().is_empty() {
                let text = "Not set";
                let tw = draw::measure(
                    cr,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: cx.theme.muted,
                        text,
                    },
                )
                .0;
                ui::row(
                    cr,
                    cx,
                    hits,
                    id,
                    r,
                    &Row {
                        icon: None,
                        name: &row.name,
                        state: "",
                        on: false,
                        right: tw + 8.0,
                    },
                );
                draw::text_right(
                    cr,
                    right,
                    r.1 + (r.3 - 15.0) / 2.0,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: cx.theme.muted,
                        text,
                    },
                );
            } else {
                let caps = chord_cap_labels(row.primary_chord());
                let caps_w = measure_caps_width(cr, cx, &caps);
                ui::row(
                    cr,
                    cx,
                    hits,
                    id,
                    r,
                    &Row {
                        icon: None,
                        name: &row.name,
                        state: "",
                        on: false,
                        right: (caps_w + 8.0).min(body.2 / 2.0),
                    },
                );
                draw_key_caps(cr, cx, r, right, &caps, false);
            }
            y += h;
        }

        if more {
            ui::row(
                cr,
                cx,
                hits,
                HIT_MORE,
                (body.0, y, body.2, ui::ROW_H),
                &Row {
                    icon: None,
                    name: if self.more { "Less" } else { "More" },
                    state: "",
                    on: false,
                    right: 0.0,
                },
            );
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
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
                ..
            } => {
                let hit = *hit;
                if hit == HIT_CANCEL {
                    if matches!(self.state, UiState::Conflict { .. }) {
                        self.state = UiState::Idle;
                        return true;
                    }
                    return false;
                }
                if hit == HIT_SWAP {
                    let UiState::Conflict {
                        key,
                        captured,
                        other_key,
                        ..
                    } = self.state.clone()
                    else {
                        return false;
                    };
                    let all = self.rows();
                    let Some(row) = all.iter().find(|r| r.key == key) else {
                        self.state = UiState::Idle;
                        return true;
                    };
                    let Some(other) = all.iter().find(|r| r.key == other_key) else {
                        self.state = UiState::Idle;
                        return true;
                    };
                    return self.execute_swap(row, other, &captured, out);
                }
                if hit == HIT_MORE {
                    self.more = !self.more;
                    return true;
                }
                if hit < HIT_ROW_BASE {
                    return false;
                }
                // Compositor supports one capture at a time; ignore row clicks while waiting.
                if matches!(self.state, UiState::Listening { .. }) {
                    return false;
                }
                let idx = (hit - HIT_ROW_BASE) as usize;
                let (visible, _) = self.visible_rows(cx);
                let Some(row) = visible.get(idx) else {
                    return false;
                };
                let all = self.rows();
                let token = next_capture_token();
                let cmd = capture_command(row, &all, &token);
                self.state = UiState::Listening {
                    key: row.key.clone(),
                    token,
                };
                out.push(Action::Run(cmd));
                true
            }
            _ => false,
        }
    }
}

fn next_capture_token() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    format!(
        "{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

fn is_safe_exec_key(key: &str) -> bool {
    key.strip_prefix("EXEC_")
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
}

fn fallback_exec_name(key: &str) -> String {
    let raw = key.strip_prefix("EXEC_").unwrap_or(key);
    let mut s = raw.replace('_', " ").to_lowercase();
    if let Some(first) = s.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    s
}

/// Normalizes a config key name into canonical `keys.rs` spelling.
fn canonical_key_token(token: &str) -> Option<&'static str> {
    let upper = token.trim().to_ascii_uppercase();
    Some(match upper.as_str() {
        "ESC" | "ESCAPE" => "ESC",
        "1" => "1",
        "2" => "2",
        "3" => "3",
        "4" => "4",
        "5" => "5",
        "6" => "6",
        "7" => "7",
        "8" => "8",
        "9" => "9",
        "0" => "0",
        "MINUS" => "MINUS",
        "EQUAL" => "EQUAL",
        "BACKSPACE" => "BACKSPACE",
        "TAB" => "TAB",
        "Q" => "Q",
        "W" => "W",
        "E" => "E",
        "R" => "R",
        "T" => "T",
        "Y" => "Y",
        "U" => "U",
        "I" => "I",
        "O" => "O",
        "P" => "P",
        "BRACKETLEFT" => "BRACKETLEFT",
        "BRACKETRIGHT" => "BRACKETRIGHT",
        "RETURN" | "ENTER" => "RETURN",
        "A" => "A",
        "S" => "S",
        "D" => "D",
        "F" => "F",
        "G" => "G",
        "H" => "H",
        "J" => "J",
        "K" => "K",
        "L" => "L",
        "SEMICOLON" => "SEMICOLON",
        "APOSTROPHE" => "APOSTROPHE",
        "GRAVE" => "GRAVE",
        "BACKSLASH" => "BACKSLASH",
        "Z" => "Z",
        "X" => "X",
        "C" => "C",
        "V" => "V",
        "B" => "B",
        "N" => "N",
        "M" => "M",
        "COMMA" => "COMMA",
        "PERIOD" | "DOT" => "PERIOD",
        "SLASH" => "SLASH",
        "SPACE" => "SPACE",
        "F1" => "F1",
        "F2" => "F2",
        "F3" => "F3",
        "F4" => "F4",
        "F5" => "F5",
        "F6" => "F6",
        "F7" => "F7",
        "F8" => "F8",
        "F9" => "F9",
        "F10" => "F10",
        "F11" => "F11",
        "F12" => "F12",
        "PRINT" => "PRINT",
        "HOME" => "HOME",
        "UP" => "UP",
        "PAGEUP" => "PAGEUP",
        "LEFT" => "LEFT",
        "RIGHT" => "RIGHT",
        "END" => "END",
        "DOWN" => "DOWN",
        "PAGEDOWN" => "PAGEDOWN",
        "INSERT" => "INSERT",
        "DELETE" => "DELETE",
        _ => return None,
    })
}

/// Normalizes a config chord (`logo + shift + enter` -> `SUPER+SHIFT+RETURN`).
pub fn normalize_chord(text: &str) -> String {
    let mut mods = [false; 4];
    let mut key: Option<&str> = None;
    for raw in text.split('+') {
        let up = raw.trim().to_ascii_uppercase();
        if up.is_empty() {
            continue;
        }
        match up.as_str() {
            "SUPER" | "LOGO" | "WIN" | "MOD4" => mods[0] = true,
            "CTRL" | "CONTROL" => mods[1] = true,
            "ALT" => mods[2] = true,
            "SHIFT" => mods[3] = true,
            _ => {
                if key.is_some() {
                    return String::new();
                }
                key = canonical_key_token(&up);
                if key.is_none() {
                    return String::new();
                }
            }
        }
    }
    let Some(k) = key else {
        return String::new();
    };
    let mut parts: Vec<&str> = MOD_ORDER
        .iter()
        .zip(mods)
        .filter_map(|(&name, on)| on.then_some(name))
        .collect();
    parts.push(k);
    parts.join("+")
}

fn parse_bind_chords(value: &str) -> Vec<String> {
    let t = value.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") {
        return Vec::new();
    }
    t.split(',')
        .map(normalize_chord)
        .filter(|s| !s.is_empty())
        .collect()
}

fn cap_label(token: &str) -> &str {
    match token {
        "SUPER" => "Super",
        "CTRL" => "Ctrl",
        "ALT" => "Alt",
        "SHIFT" => "Shift",
        "RETURN" | "ENTER" => "Enter",
        "BACKSPACE" => "Backspace",
        "ESC" | "ESCAPE" => "Esc",
        "DELETE" => "Del",
        "INSERT" => "Ins",
        "PAGEUP" => "PgUp",
        "PAGEDOWN" => "PgDn",
        "HOME" => "Home",
        "END" => "End",
        "TAB" => "Tab",
        "SPACE" => "Space",
        "PRINT" => "Print",
        "LEFT" => "Left",
        "RIGHT" => "Right",
        "UP" => "Up",
        "DOWN" => "Down",
        "MINUS" => "-",
        "EQUAL" => "=",
        "BRACKETLEFT" => "[",
        "BRACKETRIGHT" => "]",
        "SEMICOLON" => ";",
        "APOSTROPHE" => "'",
        "GRAVE" => "`",
        "BACKSLASH" => "\\",
        "COMMA" => ",",
        "PERIOD" => ".",
        "SLASH" => "/",
        other => other,
    }
}

fn chord_cap_labels(chord: &str) -> Vec<&str> {
    chord
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(cap_label)
        .collect()
}

fn row_matches(row: &KeyRow, words: &[String]) -> bool {
    if words.len() >= 2 {
        let joined_chord = normalize_chord(&words.join("+"));
        if joined_chord.contains('+') {
            return row.chords.iter().any(|c| c == &joined_chord);
        }
    }
    let mut hay = row.name.to_lowercase();
    for chord in &row.chords {
        hay.push(' ');
        hay.push_str(&chord.to_ascii_lowercase());
        hay.push(' ');
        hay.push_str(&chord.replace('+', " ").to_ascii_lowercase());
        for cap in chord_cap_labels(chord) {
            hay.push(' ');
            hay.push_str(&cap.to_lowercase());
        }
        // Keep RETURN/ENTER and ESC/ESCAPE both searchable.
        if chord.contains("RETURN") {
            hay.push_str(" return");
        }
        if chord.contains("ESC") {
            hay.push_str(" escape");
        }
        if chord.contains("DELETE") {
            hay.push_str(" delete");
        }
    }
    words.iter().all(|w| hay.contains(w))
}

fn row_value_for_chord(row: &KeyRow, chord: &str) -> String {
    match &row.exec_command {
        Some(cmd) => format!("{chord}:{cmd}"),
        None => {
            if chord.is_empty() {
                "none".to_string()
            } else {
                chord.to_string()
            }
        }
    }
}

fn swap_target_value(other: &KeyRow, taken_chord: &str, replacement: &str) -> String {
    if let Some(cmd) = &other.exec_command {
        return format!("{replacement}:{cmd}");
    }
    let mut updated = Vec::new();
    for c in &other.chords {
        if c == taken_chord {
            if !replacement.is_empty() && !updated.iter().any(|u| u == replacement) {
                updated.push(replacement.to_string());
            }
        } else if !updated.iter().any(|u| u == c) {
            updated.push(c.clone());
        }
    }
    if updated.is_empty() {
        "none".to_string()
    } else {
        updated.join(", ")
    }
}

fn capture_set_command(row: &KeyRow) -> String {
    let base = change_command(
        &row.key,
        row.raw_config.as_deref(),
        CHORD_PLACEHOLDER,
        After::Save,
    );
    let placeholder_q = sq(CHORD_PLACEHOLDER);
    let replacement = match &row.exec_command {
        Some(cmd) => format!("\"$v\"{}", sq(&format!(":{cmd}"))),
        None => "\"$v\"".to_string(),
    };
    if let Some((before, after)) = base.rsplit_once(&placeholder_q) {
        format!("{before}{replacement}{after}")
    } else {
        base
    }
}

/// Builds the single `sh -c` command that captures a chord, checks all other bindings for
/// conflict, writes `config` via `change_command` if free, and updates `key-capture`.
fn capture_command(row: &KeyRow, all_rows: &[KeyRow], token: &str) -> String {
    let token_q = sq(token);
    let same_check = if !row.primary_chord().is_empty() && row.chords.len() == 1 {
        format!(
            "if [ \"$v\" = {} ]; then printf 'done\\t%s\\n' {token_q} > \"$tmp\" && mv \"$tmp\" \"$cap\"; exit 0; fi; ",
            sq(row.primary_chord())
        )
    } else {
        String::new()
    };

    let mut cases = String::new();
    for other in all_rows {
        if other.key == row.key {
            continue;
        }
        for c in &other.chords {
            cases.push_str(&format!("  {}) conflict={} ;; ", sq(c), sq(&other.key)));
        }
    }
    let conflict_check = if cases.is_empty() {
        "conflict=\"\"".to_string()
    } else {
        format!("conflict=\"\"; case \"$v\" in {cases}esac")
    };

    let set_cmd = capture_set_command(row);

    format!(
        "dir=\"${{XDG_RUNTIME_DIR:-$HOME/.cache}}/infynitae\"; mkdir -p \"$dir\" || exit 0; \
         cap=\"$dir/key-capture\"; tmp=\"$cap.$$\"; \
         bin=$(command -v infynitae 2>/dev/null || printf '%s/.local/bin/infynitae' \"$HOME\"); \
         v=$(\"$bin\" msg capture-chord 2>/dev/null) || {{ printf 'cancel\\t%s\\n' {token_q} > \"$tmp\" && mv \"$tmp\" \"$cap\"; exit 0; }}; \
         case \"$v\" in ''|*[!A-Za-z0-9_+]*) printf 'cancel\\t%s\\n' {token_q} > \"$tmp\" && mv \"$tmp\" \"$cap\"; exit 0 ;; esac; \
         {same_check}{conflict_check}; \
         if [ -n \"$conflict\" ]; then printf 'conflict\\t%s\\t%s\\t%s\\n' {token_q} \"$v\" \"$conflict\" > \"$tmp\" && mv \"$tmp\" \"$cap\"; \
         elif {set_cmd}; then printf 'applied\\t%s\\t%s\\n' {token_q} \"$v\" > \"$tmp\" && mv \"$tmp\" \"$cap\"; \
         else printf 'cancel\\t%s\\n' {token_q} > \"$tmp\" && mv \"$tmp\" \"$cap\"; fi"
    )
}

const CAP_H: f64 = 22.0;
const CAP_PAD_X: f64 = 6.0;
const CAP_GAP: f64 = 4.0;
const CAP_MIN_W: f64 = 22.0;

fn cap_widths(cr: &Context, cx: &Cx, caps: &[&str]) -> Vec<f64> {
    caps.iter()
        .map(|label| {
            let tw = draw::measure(
                cr,
                &Text {
                    size: draw::SIZE_NOTE,
                    bold: false,
                    color: cx.theme.text,
                    text: label,
                },
            )
            .0;
            (tw + 2.0 * CAP_PAD_X).max(CAP_MIN_W)
        })
        .collect()
}

fn measure_caps_width(cr: &Context, cx: &Cx, caps: &[&str]) -> f64 {
    if caps.is_empty() {
        return 0.0;
    }
    let ws = cap_widths(cr, cx, caps);
    ws.iter().sum::<f64>() + CAP_GAP * (ws.len().saturating_sub(1) as f64)
}

fn measure_button_width(cr: &Context, cx: &Cx, word: &str) -> f64 {
    draw::measure(
        cr,
        &Text {
            size: 12.0,
            bold: false,
            color: cx.theme.text,
            text: word,
        },
    )
    .0 + 20.0
}

fn draw_key_caps(
    cr: &Context,
    cx: &Cx,
    row_rect: Rect,
    x_right: f64,
    caps: &[&str],
    accented: bool,
) {
    if caps.is_empty() {
        return;
    }
    let th = cx.theme;
    let ws = cap_widths(cr, cx, caps);
    let cy = row_rect.1 + (row_rect.3 - CAP_H) / 2.0;
    let r = th.small_radius().min(5.0);
    let fill_color = if accented {
        th.accent.alpha(ui::CHOSEN_ALPHA)
    } else {
        th.track
    };
    let border_color = if accented { th.accent } else { th.edge };

    let mut x = x_right;
    for (&label, &cw) in caps.iter().rev().zip(ws.iter().rev()) {
        x -= cw;
        draw::round_rect(cr, x.round(), cy.round(), cw.round(), CAP_H, r);
        draw::set(cr, fill_color);
        let _ = cr.fill_preserve();
        cr.new_path();
        draw::round_rect(
            cr,
            x.round() + 0.5,
            cy.round() + 0.5,
            cw.round() - 1.0,
            CAP_H - 1.0,
            (r - 0.5).max(0.0),
        );
        draw::set(cr, border_color);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
        draw::text_center(
            cr,
            x + cw / 2.0,
            cy + (CAP_H - 14.0) / 2.0,
            &Text {
                size: draw::SIZE_NOTE,
                bold: false,
                color: th.text,
                text: label,
            },
        );
        x -= CAP_GAP;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_rows::parse_config;
    use crate::theme::Theme;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn cfg(text: &str) -> ConfigData {
        ConfigData {
            now: parse_config(text),
            default: HashMap::new(),
            programs: Default::default(),
        }
    }

    fn default_theme() -> Theme {
        Theme::from_config(&crate::config::Config::from_text(""))
    }

    fn key_card(text: &str) -> KeysCard {
        let mut card = KeysCard::new();
        let data = cfg(text);
        card.apply("config", &data);
        card
    }

    fn cx_size<'a>(th: &'a Theme, size: Size) -> Cx<'a> {
        Cx {
            size,
            ..test_cx(th)
        }
    }

    #[test]
    fn defaults_come_from_keys_rs_and_titles_contain_no_key_names() {
        let card = key_card("");
        let rows = card.rows();
        assert_eq!(
            rows.len(),
            29,
            "every built-in ACTION_NAMES entry has a row"
        );

        let by_key: HashMap<_, _> = rows.iter().map(|r| (r.key.as_str(), r)).collect();
        assert_eq!(by_key["BIND_TERMINAL"].name, "Open a terminal");
        assert_eq!(by_key["BIND_TERMINAL"].primary_chord(), "SUPER+Q");
        assert_eq!(by_key["BIND_LOCK"].name, "Lock the screen");
        assert_eq!(by_key["BIND_LOCK"].primary_chord(), "SUPER+SHIFT+L");
        assert_eq!(by_key["BIND_FIT"].primary_chord(), "SUPER+RETURN");
        assert_eq!(
            by_key["BIND_FULLSCREEN"].primary_chord(),
            "SUPER+SHIFT+RETURN"
        );
        assert_eq!(
            by_key["BIND_SYSTEM_MONITOR"].primary_chord(),
            "SUPER+SHIFT+ESC"
        );
        assert!(rows.iter().all(|r| r.raw_config.is_none()));

        let forbidden_words = [
            "super",
            "ctrl",
            "alt",
            "shift",
            "tab",
            "return",
            "enter",
            "escape",
            "esc",
            "backspace",
            "delete",
            "print",
            "space",
            "bind_",
        ];
        for row in &rows {
            let lower = row.name.to_ascii_lowercase();
            let title_words: Vec<_> = lower
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .filter(|w| !w.is_empty())
                .collect();
            for bad in forbidden_words {
                assert!(
                    !title_words.contains(&bad),
                    "title {:?} for {} must not mention key name {:?}",
                    row.name,
                    row.key,
                    bad
                );
            }
        }
    }

    #[test]
    fn config_overrides_unbound_and_exec_rows_parse_cleanly() {
        let card = key_card(
            "BIND_TERMINAL=\"logo+return\"\n\
             BIND_LOCK=\"none\"\n\
             EXEC_WEB=\"SUPER+B:firefox https://example.com\"\n\
             EXEC_Z=\"ALT+M:mpc 'next'\"\n",
        );
        let rows = card.rows();
        let by_key: HashMap<_, _> = rows.iter().map(|r| (r.key.as_str(), r)).collect();

        assert_eq!(by_key["BIND_TERMINAL"].primary_chord(), "SUPER+RETURN");
        assert!(by_key["BIND_TERMINAL"].raw_config.is_some());
        assert!(by_key["BIND_LOCK"].chords.is_empty());
        assert_eq!(by_key["EXEC_WEB"].name, "firefox https://example.com");
        assert_eq!(by_key["EXEC_WEB"].primary_chord(), "SUPER+B");
        assert_eq!(by_key["EXEC_Z"].name, "mpc 'next'");
        assert_eq!(by_key["EXEC_Z"].primary_chord(), "ALT+M");
    }

    #[test]
    fn search_matches_plain_name_and_chord_even_when_hidden_behind_more() {
        let mut card = key_card("BIND_TERMINAL=\"SUPER+T\"\nEXEC_WEB=\"SUPER+B:firefox\"\n");
        let th = default_theme();
        let cx = cx_size(&th, Size::S);

        // Plain name match (including More rows).
        assert!(card.event(&Event::Text("awake"), &cx, &mut Vec::new()));
        let shown: Vec<_> = card
            .visible_rows(&cx)
            .0
            .into_iter()
            .map(|r| r.key)
            .collect();
        assert_eq!(shown, ["BIND_IDLE_INHIBIT"]);

        // Search by chord in config spelling or plain space spelling.
        card.event(&Event::Text("super+shift+l"), &cx, &mut Vec::new());
        let shown: Vec<_> = card
            .visible_rows(&cx)
            .0
            .into_iter()
            .map(|r| r.key)
            .collect();
        assert_eq!(shown, ["BIND_LOCK"]);

        // Old default SUPER+Q no longer matches Open a terminal once rebound.
        card.event(&Event::Text("super+q"), &cx, &mut Vec::new());
        assert!(card.visible_rows(&cx).0.is_empty());

        card.event(&Event::Text("super t"), &cx, &mut Vec::new());
        let shown: Vec<_> = card
            .visible_rows(&cx)
            .0
            .into_iter()
            .map(|r| r.key)
            .collect();
        assert_eq!(shown, ["BIND_TERMINAL"]);

        // Enter searches canonical RETURN as well.
        card.event(&Event::Text("enter"), &cx, &mut Vec::new());
        let shown: Vec<_> = card
            .visible_rows(&cx)
            .0
            .into_iter()
            .map(|r| r.key)
            .collect();
        assert!(shown.contains(&"BIND_FIT".to_string()));
        assert!(shown.contains(&"BIND_FULLSCREEN".to_string()));
    }

    #[test]
    fn clicking_row_enters_listening_ignores_second_click_and_handles_capture_outcomes() {
        let mut card = key_card("");
        let th = default_theme();
        let cx = cx_size(&th, Size::S);
        assert_eq!(card.status(&cx), Status::Ready);

        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: HIT_ROW_BASE,
                button: Button::Left,
                at: (20.0, 20.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(out.len(), 1);
        let UiState::Listening { key, token } = card.state.clone() else {
            panic!("expected Listening state");
        };
        assert_eq!(key, "BIND_TERMINAL");

        // Second click while already listening must not launch another capture.
        out.clear();
        assert!(!card.event(
            &Event::Press {
                hit: HIT_ROW_BASE + 1,
                button: Button::Left,
                at: (20.0, 60.0),
            },
            &cx,
            &mut out,
        ));
        assert!(out.is_empty());

        // Mismatched/stale token is ignored.
        assert!(!card.apply(
            "key_capture",
            &KeyCaptureData {
                token: "stale-0".into(),
                outcome: CaptureOutcome::Cancel,
            },
        ));
        assert!(matches!(card.state, UiState::Listening { .. }));

        // Matching Applied updates pending value and exits Listening.
        assert!(card.apply(
            "key_capture",
            &KeyCaptureData {
                token,
                outcome: CaptureOutcome::Applied("SUPER+T".into()),
            },
        ));
        assert_eq!(card.state, UiState::Idle);
        let term = card
            .rows()
            .into_iter()
            .find(|r| r.key == "BIND_TERMINAL")
            .unwrap();
        assert_eq!(term.primary_chord(), "SUPER+T");
    }

    #[test]
    fn conflict_offers_swap_or_cancel_and_writes_both_lines_via_change_command() {
        let mut card = key_card("");
        let th = default_theme();
        let cx = cx_size(&th, Size::S);

        // Click TERMINAL (index 0).
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: HIT_ROW_BASE,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out,
        ));
        let token = match &card.state {
            UiState::Listening { token, .. } => token.clone(),
            _ => panic!("expected listening"),
        };

        // Press SUPER+L (owned by BIND_WINDOWS -> "List open windows").
        assert!(card.apply(
            "key_capture",
            &KeyCaptureData {
                token: token.clone(),
                outcome: CaptureOutcome::Conflict {
                    chord: "SUPER+L".into(),
                    other_key: "BIND_WINDOWS".into(),
                },
            },
        ));
        assert!(matches!(
            &card.state,
            UiState::Conflict {
                key,
                captured,
                other_key,
                other_name,
            } if key == "BIND_TERMINAL"
                && captured == "SUPER+L"
                && other_key == "BIND_WINDOWS"
                && other_name == "List open windows"
        ));

        // Conflict expands card height for Swap/Cancel buttons, and hits stay < CHILD_STRIDE.
        let (rows_count, _) = card.visible_rows(&cx);
        assert!(card.height(&cx, 360.0) > rows_count.len() as f64 * ui::ROW_H);

        // Cancel returns to Idle without emitting commands.
        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: HIT_CANCEL,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(card.state, UiState::Idle);
        assert!(out.is_empty());

        // Re-enter conflict and confirm Swap.
        card.state = UiState::Conflict {
            key: "BIND_TERMINAL".into(),
            captured: "SUPER+L".into(),
            other_key: "BIND_WINDOWS".into(),
            other_name: "List open windows".into(),
        };
        assert!(card.event(
            &Event::Press {
                hit: HIT_SWAP,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(card.state, UiState::Idle);
        assert_eq!(out.len(), 1);
        let Action::Run(cmd) = &out[0] else {
            panic!("expected Action::Run");
        };
        let want_a = change_command("BIND_TERMINAL", None, "SUPER+L", After::Save);
        let want_b = change_command("BIND_WINDOWS", None, "SUPER+Q", After::Save);
        assert_eq!(cmd, &format!("{want_a} && {want_b}"));

        // Pending updated both rows immediately.
        let rows: HashMap<_, _> = card
            .rows()
            .into_iter()
            .map(|r| (r.key.clone(), r))
            .collect();
        assert_eq!(rows["BIND_TERMINAL"].primary_chord(), "SUPER+L");
        assert_eq!(rows["BIND_WINDOWS"].primary_chord(), "SUPER+Q");

        // Swap involving quoted EXEC_* preserves both single and double quotes via sq().
        let mut quoted_cfg = ConfigData::default();
        quoted_cfg
            .now
            .insert("EXEC_QUOTED".into(), "SUPER+B:mpc 'hi' \"there\"".into());
        card.apply("config", &quoted_cfg);
        card.state = UiState::Conflict {
            key: "BIND_TERMINAL".into(),
            captured: "SUPER+B".into(),
            other_key: "EXEC_QUOTED".into(),
            other_name: "mpc 'hi' \"there\"".into(),
        };
        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: HIT_SWAP,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        ));
        let Action::Run(exec_swap_cmd) = &out[0] else {
            panic!("expected Action::Run");
        };
        assert!(exec_swap_cmd.contains(&sq("SUPER+L:mpc 'hi' \"there\"")));
        assert!(exec_swap_cmd.contains("settings.sh\" log EXEC_QUOTED"));
    }

    #[test]
    fn capture_shell_command_executes_safely_in_temp_home_for_change_and_conflict() {
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "infynitae-keys-cmd-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let live_dir = root.join("infynitae/live");
        let bin_dir = root.join("bin");
        let rt_dir = root.join("rt");
        fs::create_dir_all(&live_dir).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&rt_dir).unwrap();

        let log_file = root.join("settings.calls");
        let settings_sh = live_dir.join("settings.sh");
        fs::write(
            &settings_sh,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n",
                sq(&log_file.to_string_lossy())
            ),
        )
        .unwrap();

        let chord_out = root.join("next_chord");
        let fake_infynitae = bin_dir.join("infynitae");
        fs::write(
            &fake_infynitae,
            format!(
                "#!/bin/sh\n[ \"$1\" = msg ] && [ \"$2\" = capture-chord ] || exit 2\ncat {}\n",
                sq(&chord_out.to_string_lossy())
            ),
        )
        .unwrap();
        fs::set_permissions(&fake_infynitae, fs::Permissions::from_mode(0o755)).unwrap();

        let card = key_card("EXEC_QUOTED=\"SUPER+B:mpc 'next'; echo $HOME\"\n");
        let rows = card.rows();
        let exec_row = rows.iter().find(|r| r.key == "EXEC_QUOTED").unwrap();

        // 1) Non-conflicting chord updates settings via log+set and writes `applied`.
        fs::write(&chord_out, "SUPER+Y\n").unwrap();
        let cmd = capture_command(exec_row, &rows, "tok-1");
        let path_env = format!("{}:/usr/bin:/bin", bin_dir.display());
        let status = std::process::Command::new("sh")
            .args(["-c", &cmd])
            .env("HOME", &root)
            .env("XDG_RUNTIME_DIR", &rt_dir)
            .env("PATH", &path_env)
            .status()
            .unwrap();
        assert!(status.success());

        let cap_text = fs::read_to_string(rt_dir.join("infynitae/key-capture")).unwrap();
        assert_eq!(
            KeyCaptureSource::parse(&cap_text),
            KeyCaptureData {
                token: "tok-1".into(),
                outcome: CaptureOutcome::Applied("SUPER+Y".into()),
            }
        );
        let logged = fs::read_to_string(&log_file).unwrap();
        assert!(logged.contains("log EXEC_QUOTED SUPER+B:mpc 'next'; echo $HOME"));
        assert!(logged.contains("set EXEC_QUOTED SUPER+Y:mpc 'next'; echo $HOME"));
        // Verify neither $HOME nor command punctuation ran as shell code inside settings.sh.
        assert!(!logged.contains(root.to_string_lossy().as_ref()));

        // 2) Conflicting chord does NOT touch settings.sh and writes `conflict`.
        let before_len = logged.len();
        fs::write(&chord_out, "SUPER+SHIFT+L\n").unwrap();
        let cmd2 = capture_command(exec_row, &rows, "tok-2");
        let status2 = std::process::Command::new("sh")
            .args(["-c", &cmd2])
            .env("HOME", &root)
            .env("XDG_RUNTIME_DIR", &rt_dir)
            .env("PATH", &path_env)
            .status()
            .unwrap();
        assert!(status2.success());
        assert_eq!(fs::read_to_string(&log_file).unwrap().len(), before_len);
        let cap_text2 = fs::read_to_string(rt_dir.join("infynitae/key-capture")).unwrap();
        assert_eq!(
            KeyCaptureSource::parse(&cap_text2),
            KeyCaptureData {
                token: "tok-2".into(),
                outcome: CaptureOutcome::Conflict {
                    chord: "SUPER+SHIFT+L".into(),
                    other_key: "BIND_LOCK".into(),
                },
            }
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draws_idle_listening_and_conflict_within_child_stride_hits() {
        let mut card = key_card("");
        let th = default_theme();
        let cx = cx_size(&th, Size::M);

        let draw_once = |c: &KeysCard| {
            let h = c.height(&cx, 380.0).ceil() as i32;
            let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 380, h.max(40)).unwrap();
            let cr = cairo::Context::new(&surf).unwrap();
            let mut hits = Hits::new(Some((40.0, 20.0)), None);
            c.draw(&cr, &cx, (0.0, 0.0, 380.0, h as f64), &mut hits);
            assert!(
                hits.list.iter().all(|(id, _)| *id > 0 && *id < 1000),
                "all hit IDs must stay within < 1000 (CHILD_STRIDE)"
            );
            hits
        };

        let idle_hits = draw_once(&card);
        assert_eq!(idle_hits.list.len(), 29);

        card.state = UiState::Listening {
            key: "BIND_TERMINAL".into(),
            token: "tok".into(),
        };
        draw_once(&card);

        card.state = UiState::Conflict {
            key: "BIND_TERMINAL".into(),
            captured: "SUPER+L".into(),
            other_key: "BIND_WINDOWS".into(),
            other_name: "List open windows".into(),
        };
        let conflict_hits = draw_once(&card);
        assert!(conflict_hits.list.iter().any(|(id, _)| *id == HIT_SWAP));
        assert!(conflict_hits.list.iter().any(|(id, _)| *id == HIT_CANCEL));
    }
}
