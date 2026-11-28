//! System health card `monitor/health` and embedded Health view for `settings/about`.
//!
//! Reads JSON from `infynitae-health now --json` and `infynitae-health log --since 24h --json`
//! (schema documented in `docs/HEALTH.md`) using a self-contained JSON parser so the edge crate
//! takes no new dependencies. Colours come exclusively from `cx.theme`.

use crate::card::{
    self, Action, Button, Card, CommandResult, Cx, Data, Event, Hits, Rect, Source, Status,
};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Size};
use crate::theme::Theme;
use crate::ui;
use cairo::Context;
use std::any::Any;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const KIND: CardKind = CardKind {
    id: "health",
    title: "Health",
    about: "Disk, memory, temperatures, services, compositor and recent log",
    sizes: &[Size::S, Size::M, Size::L],
    default: Size::M,
    half: false,
    sources: &["health"],
};

const SIDE_PAD: f64 = 8.0;
const OK_ROW_H: f64 = 24.0;
const LOG_ROW_H: f64 = 22.0;
const SECTION_H: f64 = 20.0;
const SECTION_GAP: f64 = 8.0;
const ALERT_PAD_Y: f64 = 6.0;
const ALERT_TITLE_H: f64 = 17.0;
const ALERT_ROW_GAP: f64 = 2.0;
const MAX_OK_M: usize = 6;
const MAX_LOG_S: usize = 3;
const MAX_LOG_M: usize = 5;
const MAX_LOG_L: usize = 10;

const HIT_TOGGLE_OK: u32 = 1;
const HIT_ITEM_BASE: u32 = 100;

const DISK_FIX_COMMAND: &str = "pkexec /usr/local/lib/infynitae-disk-fix";
const UPDATE_COMMAND: &str = r#"sh "$HOME/.local/bin/xdg-terminal-exec" infynitae-update"#;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HealthState {
    #[default]
    Ok,
    Watch,
    Problem,
}

impl HealthState {
    fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "problem" | "error" | "critical" => Self::Problem,
            "watch" | "warn" | "warning" => Self::Watch,
            _ => Self::Ok,
        }
    }

    fn severity_rank(self) -> u8 {
        match self {
            Self::Problem => 2,
            Self::Watch => 1,
            Self::Ok => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthCheckItem {
    pub id: String,
    pub label: String,
    pub state: HealthState,
    pub summary: String,
    pub reason: String,
    pub action: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthLogEntry {
    pub local_time: String,
    pub kind: String,
    pub severity: HealthState,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HealthData {
    pub available: bool,
    pub error: Option<String>,
    pub overall: HealthState,
    pub items: Vec<HealthCheckItem>,
    pub events: Vec<HealthLogEntry>,
}

impl HealthData {
    pub(crate) fn missing(msg: impl Into<String>) -> Self {
        Self {
            available: false,
            error: Some(msg.into()),
            ..Self::default()
        }
    }

    pub(crate) fn summary_text(&self) -> (String, HealthState) {
        if !self.available {
            return (
                self.error
                    .clone()
                    .unwrap_or_else(|| "Unavailable".to_string()),
                HealthState::Ok,
            );
        }
        let mut problems = 0usize;
        let mut watches = 0usize;
        let mut first_problem: Option<&str> = None;
        for item in &self.items {
            match item.state {
                HealthState::Problem => {
                    problems += 1;
                    if first_problem.is_none() {
                        first_problem = Some(&item.label);
                    }
                }
                HealthState::Watch => watches += 1,
                HealthState::Ok => {}
            }
        }
        if problems > 0 {
            let text = if problems == 1 && watches == 0 {
                if let Some(lbl) = first_problem {
                    format!("1 problem: {lbl}")
                } else {
                    "1 problem".to_string()
                }
            } else if watches > 0 {
                let p_word = if problems == 1 { "problem" } else { "problems" };
                format!("{problems} {p_word}, {watches} to watch")
            } else {
                format!("{problems} problems")
            };
            (text, HealthState::Problem)
        } else if watches > 0 {
            (format!("{watches} to watch"), HealthState::Watch)
        } else {
            ("All ok".to_string(), HealthState::Ok)
        }
    }

    pub(crate) fn search_haystack(&self) -> String {
        let mut out = String::from("health disk memory cpu gpu btrfs services compositor log ");
        let (sum, _) = self.summary_text();
        out.push_str(&sum);
        for item in &self.items {
            out.push(' ');
            out.push_str(&item.label);
            out.push(' ');
            out.push_str(&item.summary);
            if item.state != HealthState::Ok {
                out.push(' ');
                out.push_str(&item.reason);
            }
        }
        out
    }
}

fn action_cue(action: Option<&str>) -> Option<&'static str> {
    match action? {
        "disk-fix" => Some("Fix disk"),
        "updates" => Some("Updates"),
        "settings" | "settings:advanced" | "settings:about" => Some("Settings"),
        _ => None,
    }
}

fn action_to_edge_actions(action: &str, out: &mut Vec<Action>) -> bool {
    match action {
        "disk-fix" => {
            // Keep the panel open so command_done shows any pkexec/disk-fix failure in place.
            out.push(Action::Run(DISK_FIX_COMMAND.to_string()));
            true
        }
        "updates" => {
            out.push(Action::Run(UPDATE_COMMAND.to_string()));
            out.push(Action::ClosePanel);
            false
        }
        "settings:advanced" => {
            out.push(Action::OpenPanel("settings".into(), "FRAME_GUARD".into()));
            false
        }
        "settings:about" => {
            out.push(Action::OpenPanel("settings".into(), "This PC".into()));
            false
        }
        "settings" => {
            out.push(Action::OpenPanel("settings".into(), String::new()));
            false
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Source

fn read_render_fixture(key: &str) -> Option<String> {
    let path = std::env::var_os("INFYNITAE_EDGE_RENDER_DATA")?;
    let text = std::fs::read_to_string(path).ok()?;
    card::parse_render_data(&text).remove(key)
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
}

fn find_health_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("INFYNITAE_HEALTH_BIN").filter(|v| !v.is_empty()) {
        let pb = PathBuf::from(p);
        if is_executable(&pb) {
            return Some(pb);
        }
    }
    if let Some(home) = crate::paths::home() {
        let local = home.join(".local/bin/infynitae-health");
        if is_executable(&local) {
            return Some(local);
        }
    }
    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let cand = dir.join("infynitae-health");
            if is_executable(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

fn run_health_cmd(prog: &OsStr, args: &[&str]) -> Option<String> {
    let out = Command::new(prog).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

pub(crate) fn fetch_health_data(
    now_fixture_key: &str,
    log_fixture_key: &str,
    run_external_cmds: bool,
) -> Option<HealthData> {
    if let Some(now_json) = read_render_fixture(now_fixture_key) {
        let mut data = parse_health_now_json(&now_json)
            .unwrap_or_else(|| HealthData::missing("Could not parse health fixture"));
        if let Some(log_json) = read_render_fixture(log_fixture_key) {
            if let Some(events) = parse_health_log_json(&log_json) {
                data.events = events;
            }
        }
        return Some(data);
    }
    if !run_external_cmds {
        return None;
    }
    let Some(bin) = find_health_binary() else {
        return Some(HealthData::missing(
            "Install infynitae-health to see PC health",
        ));
    };
    let Some(now_raw) = run_health_cmd(bin.as_os_str(), &["now", "--json"]) else {
        return Some(HealthData::missing("Could not read PC health"));
    };
    let Some(mut data) = parse_health_now_json(&now_raw) else {
        return Some(HealthData::missing("Could not parse PC health data"));
    };
    if let Some(log_raw) = run_health_cmd(bin.as_os_str(), &["log", "--since", "24h", "--json"]) {
        if let Some(events) = parse_health_log_json(&log_raw) {
            data.events = events;
        }
    }
    Some(data)
}

pub struct HealthSource;

impl HealthSource {
    pub fn real() -> Self {
        Self
    }
}

impl Source for HealthSource {
    fn every(&self) -> Option<Duration> {
        Some(Duration::from_secs(15))
    }

    fn fetch(&mut self) -> Data {
        let data = fetch_health_data("health", "health_log", true)
            .unwrap_or_else(|| HealthData::missing("Could not read PC health"));
        Box::new(data)
    }
}

// ---------------------------------------------------------------------------------------------
// JSON parsers (RFC 8259 subset, self-contained, zero new crate dependencies)

pub fn parse_health_now_json(text: &str) -> Option<HealthData> {
    let root = parse_json(text)?;
    let arr = root.get("items")?.as_arr()?;
    let mut items = Vec::with_capacity(arr.len());
    for entry in arr {
        let id = entry.get("id").and_then(JsonVal::as_str)?.to_string();
        let label = entry.get("label").and_then(JsonVal::as_str)?.to_string();
        let state = entry
            .get("state")
            .and_then(JsonVal::as_str)
            .map(HealthState::from_str)
            .unwrap_or(HealthState::Ok);
        let summary = entry
            .get("summary")
            .and_then(JsonVal::as_str)
            .unwrap_or("")
            .to_string();
        let reason = entry
            .get("reason")
            .and_then(JsonVal::as_str)
            .unwrap_or(&summary)
            .to_string();
        let action = entry
            .get("action")
            .and_then(JsonVal::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        items.push(HealthCheckItem {
            id,
            label,
            state,
            summary,
            reason,
            action,
        });
    }
    // Always order problem first, then watch, then ok.
    items.sort_by_key(|i| std::cmp::Reverse(i.state.severity_rank()));

    let mut overall = root
        .get("state")
        .and_then(JsonVal::as_str)
        .map(HealthState::from_str)
        .unwrap_or(HealthState::Ok);
    for item in &items {
        if item.state.severity_rank() > overall.severity_rank() {
            overall = item.state;
        }
    }

    Some(HealthData {
        available: true,
        error: None,
        overall,
        items,
        events: Vec::new(),
    })
}

pub fn parse_health_log_json(text: &str) -> Option<Vec<HealthLogEntry>> {
    let root = parse_json(text)?;
    let arr = root.get("events")?.as_arr()?;
    let mut events = Vec::with_capacity(arr.len());
    for entry in arr {
        let local_time = entry
            .get("local_time")
            .or_else(|| entry.get("time"))
            .and_then(JsonVal::as_str)
            .unwrap_or("")
            .to_string();
        let kind = entry
            .get("kind")
            .and_then(JsonVal::as_str)
            .unwrap_or("")
            .to_string();
        let severity = entry
            .get("severity")
            .and_then(JsonVal::as_str)
            .map(HealthState::from_str)
            .unwrap_or(HealthState::Ok);
        let summary = entry
            .get("summary")
            .and_then(JsonVal::as_str)
            .unwrap_or("")
            .to_string();
        if !summary.is_empty() {
            events.push(HealthLogEntry {
                local_time,
                kind,
                severity,
                summary,
            });
        }
    }
    Some(events)
}

// ---------------------------------------------------------------------------------------------
// HealthCard

#[derive(Default)]
pub struct HealthCard {
    loaded: bool,
    data: HealthData,
    show_all_ok: bool,
    log_scroll: usize,
    log_scroll_accum: f64,
    error: Option<(String, Instant)>,
}

impl HealthCard {
    pub(crate) fn set_data(&mut self, data: HealthData) -> bool {
        let changed = !self.loaded || self.data != data;
        self.data = data;
        self.loaded = true;
        changed
    }

    fn alert_items(&self) -> impl Iterator<Item = (usize, &HealthCheckItem)> {
        self.data
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.state != HealthState::Ok)
    }

    fn ok_items(&self) -> impl Iterator<Item = &HealthCheckItem> {
        self.data
            .items
            .iter()
            .filter(|i| i.state == HealthState::Ok)
    }

    fn visible_log_cap(&self, size: Size) -> usize {
        match size {
            Size::S => MAX_LOG_S,
            Size::M => MAX_LOG_M,
            Size::L => MAX_LOG_L,
        }
    }

    fn show_log_section(&self, size: Size) -> bool {
        size != Size::S || self.show_all_ok || !self.data.events.is_empty()
    }

    fn visible_ok_count(&self, size: Size, total_ok: usize) -> usize {
        if total_ok == 0 {
            return 0;
        }
        if self.show_all_ok || size == Size::L {
            total_ok
        } else if size == Size::M {
            total_ok.min(MAX_OK_M)
        } else {
            0
        }
    }

    fn measure_and_draw(
        &self,
        cr: &Context,
        cx: &Cx,
        body: Rect,
        hits: Option<&mut Hits>,
        hit_offset: u32,
    ) -> f64 {
        draw::with_font(&cx.theme.font, || {
            self.measure_and_draw_inner(cr, cx, body, hits, hit_offset)
        })
    }

    pub(crate) fn measure_and_draw_embedded(
        &self,
        cr: &Context,
        cx: &Cx,
        body: Rect,
        hits: Option<&mut Hits>,
        hit_offset: u32,
    ) -> f64 {
        if !self.data.available {
            let msg = self
                .data
                .error
                .as_deref()
                .unwrap_or("Install infynitae-health to see PC health");
            let left = body.0 + SIDE_PAD;
            let inner_w = (body.2 - 2.0 * SIDE_PAD).max(40.0);
            if let Some(h) = hits {
                draw::with_font(&cx.theme.font, || {
                    draw::text_max(
                        cr,
                        left,
                        body.1 + 4.0,
                        inner_w,
                        &Text {
                            size: draw::SIZE_SMALL,
                            bold: false,
                            color: cx.theme.muted,
                            text: msg,
                        },
                    );
                });
                h.note(msg);
            }
            return OK_ROW_H;
        }
        self.measure_and_draw(cr, cx, body, hits, hit_offset)
    }

    fn measure_and_draw_inner(
        &self,
        cr: &Context,
        cx: &Cx,
        body: Rect,
        mut hits: Option<&mut Hits>,
        hit_offset: u32,
    ) -> f64 {
        let th = cx.theme;
        let left = body.0 + SIDE_PAD;
        let right = body.0 + body.2 - SIDE_PAD;
        let inner_w = (right - left).max(40.0);
        let mut y = body.1;

        // 1. Problem and watch items first.
        let mut alert_count = 0usize;
        for (idx, item) in self.alert_items() {
            alert_count += 1;
            y = draw_alert_item(
                cr,
                cx,
                body,
                left,
                right,
                inner_w,
                y,
                item,
                hit_offset + HIT_ITEM_BASE + idx as u32,
                &mut hits,
            );
            y += ALERT_ROW_GAP;
        }

        // 2. Transient command error line (e.g. pkexec cancelled or disk-fix error).
        if let Some((err, _)) = &self.error {
            if let Some(h) = hits.as_deref_mut() {
                draw::text_max(
                    cr,
                    left,
                    y + 3.0,
                    inner_w,
                    &Text {
                        size: draw::SIZE_NOTE,
                        bold: false,
                        color: th.alert,
                        text: err,
                    },
                );
                h.note(err.as_str());
            }
            y += ui::ERROR_H;
        }

        // 3. Ok checks below problem/watch items.
        let ok_list: Vec<&HealthCheckItem> = self.ok_items().collect();
        let total_ok = ok_list.len();
        if total_ok > 0 {
            if alert_count > 0 {
                y += 4.0;
            }
            let vis_ok = self.visible_ok_count(cx.size, total_ok);
            for item in &ok_list[..vis_ok] {
                if let Some(h) = hits.as_deref_mut() {
                    draw_ok_row(cr, th, left, right, inner_w, y, item);
                    h.note(format!("{}: {}", item.label, item.summary));
                }
                y += OK_ROW_H;
            }
            if vis_ok < total_ok
                || (self.show_all_ok && cx.size != Size::L && total_ok > MAX_OK_M)
                || (self.show_all_ok && cx.size == Size::S)
            {
                let toggle_label = if vis_ok < total_ok {
                    if alert_count == 0 && vis_ok == 0 {
                        format!("All {total_ok} checks ok")
                    } else {
                        let rest = total_ok - vis_ok;
                        let word = if rest == 1 { "check" } else { "checks" };
                        format!("Show {rest} more ok {word}")
                    }
                } else {
                    "Hide ok checks".to_string()
                };
                let toggle_rect = (body.0, y, body.2, OK_ROW_H);
                let hit_id = hit_offset + HIT_TOGGLE_OK;
                if let Some(h) = hits.as_deref_mut() {
                    let lit = h.add(hit_id, toggle_rect) || h.selected(hit_id);
                    if lit {
                        ui::light(cr, cx, toggle_rect);
                    }
                    let cue = if vis_ok < total_ok { "Show" } else { "Hide" };
                    let cue_t = Text {
                        size: draw::SIZE_NOTE,
                        bold: false,
                        color: th.accent_text,
                        text: cue,
                    };
                    let cw = draw::measure(cr, &cue_t).0;
                    draw::text_max(
                        cr,
                        left,
                        y + 5.0,
                        (inner_w - cw - 8.0).max(40.0),
                        &Text {
                            size: draw::SIZE_SMALL,
                            bold: false,
                            color: th.muted,
                            text: &toggle_label,
                        },
                    );
                    draw::text(cr, right - cw, y + 5.5, &cue_t);
                    h.label(hit_id, toggle_label);
                }
                y += OK_ROW_H;
            }
        } else if alert_count == 0 {
            if let Some(h) = hits.as_deref_mut() {
                draw::text_max(
                    cr,
                    left,
                    y + 4.0,
                    inner_w,
                    &Text {
                        size: draw::SIZE_SMALL,
                        bold: false,
                        color: th.muted,
                        text: "No health checks reported",
                    },
                );
                h.note("No health checks reported");
            }
            y += OK_ROW_H;
        }

        // 4. Recent log below checks.
        if self.show_log_section(cx.size) {
            if y > body.1 {
                y += SECTION_GAP;
            }
            if let Some(h) = hits.as_deref_mut() {
                draw::text(
                    cr,
                    left,
                    y + 2.0,
                    &Text {
                        size: draw::SIZE_SMALL,
                        bold: true,
                        color: th.muted,
                        text: "Recent log",
                    },
                );
                h.note("Recent log");
            }
            y += SECTION_H;

            if self.data.events.is_empty() {
                if let Some(h) = hits.as_deref_mut() {
                    draw::text_max(
                        cr,
                        left,
                        y + 3.0,
                        inner_w,
                        &Text {
                            size: draw::SIZE_SMALL,
                            bold: false,
                            color: th.muted,
                            text: "No events in the last 24 hours",
                        },
                    );
                    h.note("No events in the last 24 hours");
                }
                y += LOG_ROW_H;
            } else {
                let cap = self.visible_log_cap(cx.size);
                let total = self.data.events.len();
                let max_scroll = total.saturating_sub(cap);
                let start = self.log_scroll.min(max_scroll);
                let end = (start + cap).min(total);
                let list_top = y;
                for ev in &self.data.events[start..end] {
                    if let Some(h) = hits.as_deref_mut() {
                        draw_log_row(cr, th, left, inner_w, y, ev);
                        h.note(format!("{} {}", ev.local_time, ev.summary));
                    }
                    y += LOG_ROW_H;
                }
                if hits.is_some() && total > cap {
                    ui::scrollbar(
                        cr,
                        cx,
                        (body.0, list_top, body.2, (end - start) as f64 * LOG_ROW_H),
                        total as f64 * LOG_ROW_H,
                        start as f64 * LOG_ROW_H,
                    );
                }
            }
        }

        y - body.1
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_alert_item(
    cr: &Context,
    cx: &Cx,
    body: Rect,
    left: f64,
    right: f64,
    inner_w: f64,
    y: f64,
    item: &HealthCheckItem,
    hit_id: u32,
    hits: &mut Option<&mut Hits>,
) -> f64 {
    let th = cx.theme;
    let cue = action_cue(item.action.as_deref());
    let state_color = match item.state {
        HealthState::Problem => th.alert,
        HealthState::Watch => th.accent_text,
        HealthState::Ok => th.muted,
    };
    let cue_w = cue
        .map(|label| {
            draw::measure(
                cr,
                &Text {
                    size: draw::SIZE_SMALL,
                    bold: false,
                    color: state_color,
                    text: label,
                },
            )
            .0 + 10.0
        })
        .unwrap_or(0.0);

    let title_w = (inner_w - cue_w).max(60.0);
    let reason_layout =
        draw::wrapped_layout(cr, draw::SIZE_NOTE, false, inner_w, Some(2), &item.reason);
    let reason_h = reason_layout.pixel_size().1 as f64;
    let row_h = (ALERT_PAD_Y + ALERT_TITLE_H + reason_h + ALERT_PAD_Y).max(ui::ROW_H);
    let rect = (body.0, y, body.2, row_h);

    if let Some(h) = hits.as_deref_mut() {
        if let Some(cue_label) = cue {
            let lit = h.add(hit_id, rect) || h.selected(hit_id);
            if lit {
                ui::light(cr, cx, rect);
            }
            let cw = draw::measure(
                cr,
                &Text {
                    size: draw::SIZE_SMALL,
                    bold: false,
                    color: state_color,
                    text: cue_label,
                },
            )
            .0;
            draw::text(
                cr,
                right - cw,
                y + ALERT_PAD_Y + 0.5,
                &Text {
                    size: draw::SIZE_SMALL,
                    bold: false,
                    color: state_color,
                    text: cue_label,
                },
            );
            h.label(hit_id, format!("{} ({cue_label})", item.label));
        } else {
            h.note(format!("{}: {}", item.label, item.reason));
        }

        draw::text_max(
            cr,
            left,
            y + ALERT_PAD_Y,
            title_w,
            &Text {
                size: draw::SIZE_BODY,
                bold: item.state == HealthState::Problem,
                color: th.text,
                text: &item.label,
            },
        );
        draw::set(cr, state_color);
        cr.move_to(left.round(), (y + ALERT_PAD_Y + ALERT_TITLE_H).round());
        pangocairo::functions::show_layout(cr, &reason_layout);
    }

    y + row_h
}

fn draw_ok_row(
    cr: &Context,
    th: &Theme,
    left: f64,
    right: f64,
    inner_w: f64,
    y: f64,
    item: &HealthCheckItem,
) {
    let left_t = Text {
        size: draw::SIZE_SMALL,
        bold: false,
        color: th.text,
        text: &item.label,
    };
    let right_t = Text {
        size: draw::SIZE_NOTE,
        bold: false,
        color: th.muted,
        text: &item.summary,
    };
    let lw = draw::measure(cr, &left_t).0;
    let rw = draw::measure(cr, &right_t).0;
    let max_left = if lw + 10.0 + rw <= inner_w {
        lw
    } else {
        lw.min(((inner_w - 10.0) * 0.48).max(60.0))
    };
    draw::text_max(cr, left, y + 4.5, max_left, &left_t);
    let avail_right = (inner_w - max_left - 10.0).max(40.0);
    if rw <= avail_right {
        draw::text(cr, right - rw, y + 5.0, &right_t);
    } else {
        draw::text_max(cr, right - avail_right, y + 5.0, avail_right, &right_t);
    }
}

fn draw_log_row(cr: &Context, th: &Theme, left: f64, inner_w: f64, y: f64, ev: &HealthLogEntry) {
    let time_t = Text {
        size: draw::SIZE_NOTE,
        bold: false,
        color: th.muted,
        text: &ev.local_time,
    };
    let tw = draw::measure(cr, &time_t).0;
    let time_col = tw.min((inner_w * 0.36).max(38.0));
    draw::text_max(cr, left, y + 4.0, time_col, &time_t);

    let msg_x = left + time_col + 8.0;
    let msg_w = (inner_w - time_col - 8.0).max(40.0);
    let msg_color = match ev.severity {
        HealthState::Problem => th.alert,
        HealthState::Watch => th.accent_text,
        HealthState::Ok => th.text,
    };
    draw::text_max(
        cr,
        msg_x,
        y + 3.5,
        msg_w,
        &Text {
            size: draw::SIZE_NOTE,
            bold: false,
            color: msg_color,
            text: &ev.summary,
        },
    );
}

impl Card for HealthCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        if key != "health" {
            return false;
        }
        let Some(d) = data.downcast_ref::<HealthData>() else {
            return false;
        };
        self.set_data(d.clone())
    }

    fn status(&self, _cx: &Cx) -> Status {
        if !self.loaded {
            return Status::Loading;
        }
        if !self.data.available {
            return Status::Missing(
                self.data
                    .error
                    .clone()
                    .unwrap_or_else(|| "Install infynitae-health to see PC health".into()),
            );
        }
        Status::Ready
    }

    fn height(&self, cx: &Cx, width: f64) -> f64 {
        let Ok(surf) = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1) else {
            return 0.0;
        };
        let Ok(cr) = Context::new(&surf) else {
            return 0.0;
        };
        self.measure_and_draw(&cr, cx, (0.0, 0.0, width, 0.0), None, 0)
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let _ = self.measure_and_draw(cr, cx, body, Some(hits), 0);
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Scroll { dy, notches, .. } => {
                if !self.show_log_section(cx.size) {
                    return false;
                }
                let cap = self.visible_log_cap(cx.size);
                let max_scroll = self.data.events.len().saturating_sub(cap);
                card::scroll_rows(
                    &mut self.log_scroll,
                    &mut self.log_scroll_accum,
                    max_scroll,
                    *dy,
                    *notches,
                )
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } => {
                if *hit == HIT_TOGGLE_OK {
                    self.show_all_ok = !self.show_all_ok;
                    return true;
                }
                if *hit >= HIT_ITEM_BASE {
                    let idx = (*hit - HIT_ITEM_BASE) as usize;
                    if let Some(item) = self.data.items.get(idx) {
                        if let Some(act) = item.action.as_deref() {
                            return action_to_edge_actions(act, out);
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn show_list(&mut self) {
        self.show_all_ok = true;
    }

    fn panel_closed(&mut self) {
        self.log_scroll = 0;
        self.log_scroll_accum = 0.0;
        self.error = None;
    }

    fn command_done(&mut self, res: &CommandResult, now: Instant) -> bool {
        if res.status == Some(0) {
            return self.error.take().is_some();
        }
        let msg = if !res.stderr_line.is_empty() {
            res.stderr_line.clone()
        } else {
            "Action failed".to_string()
        };
        self.error = Some((msg, now + ui::ERROR_TTL));
        true
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.error.as_ref().map(|(_, at)| *at)
    }

    fn expire(&mut self, now: Instant) -> bool {
        if self.error.as_ref().is_some_and(|(_, at)| now >= *at) {
            self.error = None;
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------------------------
// Minimal RFC 8259 JSON parser (nesting-limited, never panics)

const MAX_JSON_DEPTH: usize = 32;

#[derive(Clone, Debug)]
enum JsonVal {
    Null,
    Bool,
    Num,
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

    fn as_arr(&self) -> Option<&[JsonVal]> {
        match self {
            JsonVal::Arr(a) => Some(a),
            _ => None,
        }
    }
}

fn parse_json(input: &str) -> Option<JsonVal> {
    let mut p = JsonParser {
        s: input.as_bytes(),
        i: 0,
    };
    let val = p.value(0).ok()?;
    p.ws();
    (p.i == p.s.len()).then_some(val)
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
            Some(b't') => self.eat(b"true").map(|()| JsonVal::Bool),
            Some(b'f') => self.eat(b"false").map(|()| JsonVal::Bool),
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

    fn hex4(&mut self) -> Result<u16, ()> {
        let slice = self.s.get(self.i..self.i + 4).ok_or(())?;
        let text = std::str::from_utf8(slice).map_err(|_| ())?;
        self.i += 4;
        u16::from_str_radix(text, 16).map_err(|_| ())
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
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let code = self.hex4()?;
                        if (0xD800..=0xDBFF).contains(&code) {
                            self.eat(b"\\u")?;
                            let low = self.hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err(());
                            }
                            let cp = 0x10000
                                + (((u32::from(code) - 0xD800) << 10) | (u32::from(low) - 0xDC00));
                            out.push(char::from_u32(cp).ok_or(())?);
                        } else {
                            out.push(char::from_u32(u32::from(code)).ok_or(())?);
                        }
                    }
                    _ => return Err(()),
                }
            } else if b < 0x20 {
                return Err(());
            } else {
                let start = self.i;
                while self
                    .s
                    .get(self.i)
                    .is_some_and(|&c| c != b'"' && c != b'\\' && c >= 0x20)
                {
                    self.i += 1;
                }
                let chunk = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| ())?;
                out.push_str(chunk);
            }
        }
        Err(())
    }

    fn number(&mut self) -> Result<JsonVal, ()> {
        let start = self.i;
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        let mut has_digit = false;
        while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
            has_digit = true;
            self.i += 1;
        }
        if !has_digit {
            return Err(());
        }
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            let mut frac = false;
            while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                frac = true;
                self.i += 1;
            }
            if !frac {
                return Err(());
            }
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            let mut exp = false;
            while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                exp = true;
                self.i += 1;
            }
            if !exp {
                return Err(());
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| ())?;
        text.parse::<f64>().map_err(|_| ())?;
        Ok(JsonVal::Num)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx as base_cx;
    use crate::config::Config;
    use crate::provider;

    fn cx_for<'a>(th: &'a Theme, size: Size) -> Cx<'a> {
        let mut cx = base_cx(th);
        cx.size = size;
        cx
    }

    fn sample_now_json() -> &'static str {
        r#"{
          "schema_version": 1,
          "timestamp_unix": 1795856400,
          "time": "2026-11-28T07:00:00Z",
          "state": "problem",
          "items": [
            {
              "id": "cpu",
              "category": "cpu",
              "label": "CPU",
              "state": "ok",
              "summary": "18%, 52°C",
              "reason": "CPU 18%, 52°C"
            },
            {
              "id": "updates",
              "category": "updates",
              "label": "Updates",
              "state": "watch",
              "summary": "2 Infynitae behind",
              "reason": "Infynitae is 2 commits behind origin/main",
              "action": "updates"
            },
            {
              "id": "btrfs:/",
              "category": "disk",
              "label": "Btrfs metadata /",
              "state": "problem",
              "summary": "92% used (180 MB free)",
              "reason": "btrfs metadata on / is 92% full (180 MB free, 512 MB unallocated): a full one froze the PC on Nov 21",
              "action": "disk-fix"
            },
            {
              "id": "compositor",
              "category": "compositor",
              "label": "Compositor",
              "state": "watch",
              "summary": "7 stalls",
              "reason": "Compositor had 7 stalls in the last hour",
              "action": "settings:advanced"
            },
            {
              "id": "memory:ram",
              "category": "memory",
              "label": "Memory",
              "state": "ok",
              "summary": "9.1 GB of 31 GB (29%)",
              "reason": "RAM 9.1 GB of 31 GB"
            }
          ]
        }"#
    }

    fn sample_log_json() -> &'static str {
        r#"{
          "schema_version": 1,
          "events": [
            {"local_time": "08:55", "kind": "oom", "severity": "problem", "summary": "OOM killed python3"},
            {"local_time": "08:40", "kind": "disk_guard", "severity": "watch", "summary": "btrfs metadata on / is 82% full"},
            {"local_time": "08:10", "kind": "update", "severity": "info", "summary": "Upgraded 3 packages (mesa, linux, systemd)"}
          ]
        }"#
    }

    #[test]
    fn parses_json_orders_problems_first_and_rejects_broken_json() {
        let mut data = parse_health_now_json(sample_now_json()).unwrap();
        assert!(data.available);
        assert_eq!(data.overall, HealthState::Problem);
        assert_eq!(data.items.len(), 5);
        assert_eq!(data.items[0].id, "btrfs:/");
        assert_eq!(data.items[0].state, HealthState::Problem);
        assert_eq!(data.items[1].state, HealthState::Watch);
        assert_eq!(data.items[2].state, HealthState::Watch);
        assert_eq!(data.items[3].state, HealthState::Ok);
        assert_eq!(data.items[4].state, HealthState::Ok);

        data.events = parse_health_log_json(sample_log_json()).unwrap();
        assert_eq!(data.events.len(), 3);
        assert_eq!(data.events[0].severity, HealthState::Problem);
        assert_eq!(data.events[0].local_time, "08:55");

        let (sum, st) = data.summary_text();
        assert_eq!(st, HealthState::Problem);
        assert_eq!(sum, "1 problem, 2 to watch");

        assert!(parse_health_now_json("not json").is_none());
        assert!(parse_health_now_json("{}").is_none());
        assert!(parse_health_log_json("{}").is_none());
    }

    #[test]
    fn provider_registers_monitor_health_card_and_source() {
        let p = provider::by_id("monitor").unwrap();
        let kind = p.kind("health").expect("monitor/health kind");
        assert_eq!(kind.sources, &["health"]);
        assert_eq!(kind.default, Size::M);
        assert!(p.card("health", None).is_some());
        assert!(p.card("health", Some("arg")).is_none());
        let src = p.source("health").expect("health source");
        assert_eq!(src.every(), Some(Duration::from_secs(15)));
    }

    #[test]
    fn health_card_status_actions_errors_and_height_match_draw() {
        let th = Theme::from_config(&Config::from_text(""));
        let mut card = HealthCard::default();
        let cx_m = cx_for(&th, Size::M);
        assert_eq!(card.status(&cx_m), Status::Loading);

        card.apply("health", &HealthData::missing("not installed"));
        assert!(matches!(card.status(&cx_m), Status::Missing(_)));

        let mut data = parse_health_now_json(sample_now_json()).unwrap();
        data.events = parse_health_log_json(sample_log_json()).unwrap();
        assert!(card.apply("health", &data));
        assert_eq!(card.status(&cx_m), Status::Ready);

        // Check height == drawn extent across sizes S, M, L.
        for size in [Size::S, Size::M, Size::L] {
            let cx = cx_for(&th, size);
            for width in [300.0, 404.0] {
                let h = card.height(&cx, width);
                assert!(h > 60.0, "size={size:?} width={width} h={h}");
                let surf =
                    cairo::ImageSurface::create(cairo::Format::ARgb32, 460, (h + 40.0) as i32)
                        .unwrap();
                let cr = Context::new(&surf).unwrap();
                let mut hits = Hits::new(None, None);
                let drawn =
                    card.measure_and_draw(&cr, &cx, (12.0, 8.0, width, h), Some(&mut hits), 0);
                assert!(
                    (drawn - h).abs() < 0.01,
                    "size={size:?} width={width}: height={h} drawn={drawn}"
                );
                // Problem item with action=disk-fix must be clickable.
                assert!(
                    hits.list.iter().any(|(id, _)| *id == HIT_ITEM_BASE),
                    "disk-fix problem item must register a hit"
                );
                // Recent log note must appear.
                assert!(hits
                    .texts
                    .iter()
                    .any(|(_, t)| t.contains("OOM killed python3")));
            }
        }

        // Click problem item -> runs disk-fix without closing panel.
        let mut out = Vec::new();
        card.event(
            &Event::Press {
                hit: HIT_ITEM_BASE,
                button: Button::Left,
                at: (20.0, 20.0),
            },
            &cx_m,
            &mut out,
        );
        assert_eq!(out, vec![Action::Run(DISK_FIX_COMMAND.to_string())]);

        // Click Updates item (index 1) -> runs infynitae-update + closes panel.
        out.clear();
        card.event(
            &Event::Press {
                hit: HIT_ITEM_BASE + 1,
                button: Button::Left,
                at: (20.0, 60.0),
            },
            &cx_m,
            &mut out,
        );
        assert_eq!(
            out,
            vec![Action::Run(UPDATE_COMMAND.to_string()), Action::ClosePanel]
        );

        // Click Compositor item (index 2) -> opens Settings filtered to FRAME_GUARD.
        out.clear();
        card.event(
            &Event::Press {
                hit: HIT_ITEM_BASE + 2,
                button: Button::Left,
                at: (20.0, 100.0),
            },
            &cx_m,
            &mut out,
        );
        assert_eq!(
            out,
            vec![Action::OpenPanel("settings".into(), "FRAME_GUARD".into())]
        );

        // Failed fix command shows error until deadline expires.
        let now = Instant::now();
        assert!(card.command_done(
            &CommandResult {
                command: DISK_FIX_COMMAND.into(),
                status: Some(126),
                stderr_line: "Authentication cancelled".into(),
            },
            now
        ));
        assert!(card.next_deadline().is_some());
        let err_h = card.height(&cx_m, 404.0);
        assert!(card.expire(now + ui::ERROR_TTL));
        assert!(card.next_deadline().is_none());
        assert!(card.height(&cx_m, 404.0) < err_h);
    }

    #[test]
    fn health_card_empty_log_renders_honest_empty_state_and_scrolls_long_log() {
        let th = Theme::from_config(&Config::from_text(""));
        let mut data = parse_health_now_json(sample_now_json()).unwrap();
        let mut card = HealthCard::default();
        card.apply("health", &data);
        let cx = cx_for(&th, Size::M);
        let h = card.height(&cx, 380.0);
        let surf =
            cairo::ImageSurface::create(cairo::Format::ARgb32, 420, (h + 20.0) as i32).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        card.draw(&cr, &cx, (0.0, 0.0, 380.0, h), &mut hits);
        assert!(hits
            .texts
            .iter()
            .any(|(_, t)| t == "No events in the last 24 hours"));

        // Populate 12 events and verify scrolling advances log_scroll.
        data.events = (0..12)
            .map(|i| HealthLogEntry {
                local_time: format!("08:{i:02}"),
                kind: "update".into(),
                severity: HealthState::Ok,
                summary: format!("Event {i}"),
            })
            .collect();
        card.apply("health", &data);
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Scroll {
                hit: None,
                dy: 0.0,
                notches: 2.0,
            },
            &cx,
            &mut out
        ));
        assert_eq!(card.log_scroll, 2);
    }
}
