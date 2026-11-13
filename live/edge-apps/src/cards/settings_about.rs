//! Settings card `settings/about` ("This PC"): hardware and OS facts, update and config
//! actions, the last undo entry, and a warning when a restart setting changed in this session
//! (docs/EDGE_SETTINGS.md section 4).

use crate::card::{self, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::cards::settings;
use crate::draw::{self, Text, FONT};
use crate::provider::{CardKind, Size};
use crate::theme::Theme;
use crate::ui;
use cairo::Context;
use std::any::Any;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

pub const KIND: CardKind = CardKind {
    id: "about",
    title: "This PC",
    about: "The PC's name and hardware, updates, and the config file",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["pc"],
};

const UPDATE_COMMAND: &str = r#"sh "$HOME/.local/bin/xdg-terminal-exec" infynitae-update"#;
const EDIT_COMMAND: &str =
    r#"sh "$HOME/.local/bin/xdg-terminal-exec" sh "$HOME/infynitae/live/settings.sh" edit"#;

const INFO_ROW_H: f64 = 26.0;
const SIDE_PAD: f64 = 8.0;
const BTN_H: f64 = 26.0;
const BTN_GAP_X: f64 = 8.0;
const BTN_GAP_Y: f64 = 6.0;
const ACTS_TOP_GAP: f64 = 8.0;
const RESTART_TOP_GAP: f64 = 10.0;
const RESTART_TITLE_H: f64 = 18.0;

const HIT_UPDATE: u32 = 1;
const HIT_EDIT: u32 = 2;
const HIT_UNDO: u32 = 3;

/// Keys tagged `[restart]` in `config/config`. All `OUTPUT_*` display keys also require restart
/// (and `BATTERY_LIMIT` runs `battery-limit`, `MAX_APP_SCALE`/`ZOOM_SHARPEN` are `[save]`).
const RESTART_KEYS: &[&str] = &[
    "XKB_DEFAULT_LAYOUT",
    "XKB_DEFAULT_OPTIONS",
    "OUTPUT_ORDER",
    "OUTPUT_LAYOUT",
    "OUTPUT_SCALE",
    "SCALING_FILTER",
    "VRR_ENABLED",
    "VIRTUAL_SCREEN",
    "OSK",
    "AUTO_ROTATE",
    "XWAYLAND",
    "START_TERMINAL",
    "START_LOCKED",
    "REMOTE_DESKTOP",
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct UndoEntry {
    key: String,
    old: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PcInfo {
    hostname: Option<String>,
    system: Option<String>,
    processor: Option<String>,
    memory: Option<String>,
    graphics: Option<String>,
    infynitae: Option<String>,
    timezone: Option<String>,
    undo: Option<UndoEntry>,
    restart_keys: Vec<String>,
}

#[derive(Clone, Debug)]
struct PcPaths {
    hostname: PathBuf,
    os_release: PathBuf,
    kernel_osrelease: PathBuf,
    cpuinfo: PathBuf,
    meminfo: PathBuf,
    settings_changes: PathBuf,
    wayland_socket: Option<PathBuf>,
    infynitae_repo: Option<PathBuf>,
    run_external_cmds: bool,
}

impl PcPaths {
    fn system() -> Self {
        let home = std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let settings_changes = home
            .as_ref()
            .map(|h| h.join(".local/state/infynitae/settings-changes"))
            .unwrap_or_else(|| PathBuf::from(".local/state/infynitae/settings-changes"));
        let infynitae_repo = home.map(|h| h.join("infynitae"));
        let wayland_socket = match (
            std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()),
            std::env::var_os("WAYLAND_DISPLAY").filter(|v| !v.is_empty()),
        ) {
            (Some(rt), Some(disp)) => Some(PathBuf::from(rt).join(disp)),
            _ => None,
        };
        Self {
            hostname: PathBuf::from("/etc/hostname"),
            os_release: PathBuf::from("/etc/os-release"),
            kernel_osrelease: PathBuf::from("/proc/sys/kernel/osrelease"),
            cpuinfo: PathBuf::from("/proc/cpuinfo"),
            meminfo: PathBuf::from("/proc/meminfo"),
            settings_changes,
            wayland_socket,
            infynitae_repo,
            run_external_cmds: true,
        }
    }
}

pub struct PcSource {
    paths: PcPaths,
}

impl PcSource {
    pub fn system() -> Self {
        Self {
            paths: PcPaths::system(),
        }
    }

    #[cfg(test)]
    fn with_paths(paths: PcPaths) -> Self {
        Self { paths }
    }
}

impl Source for PcSource {
    fn every(&self) -> Option<Duration> {
        None
    }

    fn fetch(&mut self) -> Data {
        let hostname_raw = read_or_render(&self.paths.hostname, "pc_hostname");
        let os_release_raw = read_or_render(&self.paths.os_release, "pc_os_release");
        let kernel_raw = read_or_render(&self.paths.kernel_osrelease, "pc_kernel");
        let cpuinfo_raw = read_or_render(&self.paths.cpuinfo, "pc_cpuinfo");
        let meminfo_raw = read_or_render(&self.paths.meminfo, "pc_meminfo");
        let changes_raw = read_or_render(&self.paths.settings_changes, "pc_changes");

        let lspci_raw = card::render_value("pc_lspci").or_else(|| {
            if self.paths.run_external_cmds {
                run_cmd("lspci", &["-mm"])
            } else {
                None
            }
        });
        let infynitae_raw = card::render_value("pc_infynitae").or_else(|| {
            if self.paths.run_external_cmds {
                self.paths.infynitae_repo.as_deref().and_then(run_git_log)
            } else {
                None
            }
        });
        let timezone_raw = card::render_value("pc_timezone").or_else(|| {
            if self.paths.run_external_cmds {
                run_cmd("timedatectl", &["show", "-p", "Timezone", "--value"])
            } else {
                None
            }
        });

        let compositor_start = card::render_value("pc_compositor_start")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .or_else(|| socket_mtime_secs(self.paths.wayland_socket.as_deref()));

        let info = PcInfo {
            hostname: hostname_raw.as_deref().and_then(parse_hostname),
            system: parse_system(
                os_release_raw.as_deref().unwrap_or(""),
                kernel_raw.as_deref().unwrap_or(""),
            ),
            processor: cpuinfo_raw.as_deref().and_then(parse_cpuinfo),
            memory: meminfo_raw.as_deref().and_then(parse_meminfo),
            graphics: lspci_raw.as_deref().and_then(parse_lspci),
            infynitae: infynitae_raw.as_deref().and_then(parse_trimmed),
            timezone: timezone_raw.as_deref().and_then(parse_trimmed),
            undo: changes_raw.as_deref().and_then(parse_undo),
            restart_keys: changes_raw
                .as_deref()
                .map(|s| parse_restart_keys(s, compositor_start))
                .unwrap_or_default(),
        };
        Box::new(info)
    }
}

fn read_or_render(path: &Path, key: &str) -> Option<String> {
    card::render_value(key).or_else(|| std::fs::read_to_string(path).ok())
}

fn run_cmd(prog: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(prog).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

fn run_git_log(repo: &Path) -> Option<String> {
    let repo_str = repo.to_str()?;
    run_cmd("git", &["-C", repo_str, "log", "-1", "--format=%h %cs"])
}

fn socket_mtime_secs(path: Option<&Path>) -> Option<u64> {
    let modified = std::fs::metadata(path?).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn parse_trimmed(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn parse_hostname(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
}

fn unquote_os_value(raw: &str) -> String {
    let s = raw.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else {
                out.push(c);
            }
        }
        out.trim().to_string()
    } else if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        s[1..s.len() - 1].trim().to_string()
    } else {
        s.to_string()
    }
}

fn parse_os_release(raw: &str) -> Option<String> {
    let mut pretty = None;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("PRETTY_NAME=") {
            let val = unquote_os_value(rest);
            if !val.is_empty() {
                pretty = Some(val);
            }
        }
    }
    pretty
}

fn parse_system(os_release_raw: &str, kernel_raw: &str) -> Option<String> {
    let os = parse_os_release(os_release_raw);
    let kernel = parse_trimmed(kernel_raw);
    match (os, kernel) {
        (Some(o), Some(k)) => Some(format!("{o}, kernel {k}")),
        (Some(o), None) => Some(o),
        (None, Some(k)) => Some(format!("kernel {k}")),
        (None, None) => None,
    }
}

fn parse_cpuinfo(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.trim() == "model name" {
            let model = v.trim();
            if !model.is_empty() {
                return Some(model.to_string());
            }
        }
    }
    None
}

fn parse_meminfo(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.trim() != "MemTotal" {
            continue;
        }
        let kb = v.split_whitespace().next()?.parse::<u64>().ok()?;
        if kb == 0 {
            return None;
        }
        return if kb >= 1_048_576 {
            let gb = (kb as f64 / 1_048_576.0).round() as u64;
            Some(format!("{gb} GB"))
        } else {
            let mb = ((kb as f64 / 1024.0).round() as u64).max(1);
            Some(format!("{mb} MB"))
        };
    }
    None
}

fn lspci_quoted_fields(line: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut field = String::new();
        let mut closed = false;
        while let Some(inner) = chars.next() {
            if inner == '\\' {
                field.push(chars.next()?);
            } else if inner == '"' {
                closed = true;
                break;
            } else {
                field.push(inner);
            }
        }
        if !closed {
            return None;
        }
        out.push(field);
    }
    Some(out)
}

fn parse_lspci(raw: &str) -> Option<String> {
    let mut devices: Vec<String> = Vec::new();
    for line in raw.lines() {
        let Some(fields) = lspci_quoted_fields(line) else {
            continue;
        };
        if fields.len() < 3 {
            continue;
        }
        let class = &fields[0];
        if !class.contains("VGA") && !class.contains("3D") {
            continue;
        }
        let dev = fields[2].trim();
        if !dev.is_empty() && !devices.iter().any(|existing| existing == dev) {
            devices.push(dev.to_string());
        }
    }
    if devices.is_empty() {
        None
    } else {
        Some(devices.join(", "))
    }
}

fn is_valid_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_change_line(line: &str) -> Option<(u64, String, String)> {
    let line = line.trim_end_matches('\r');
    let mut parts = line.splitn(3, '\t');
    let ts_raw = parts.next()?.trim();
    let key = parts.next()?.trim();
    let old = parts.next()?;
    let ts = ts_raw.parse::<u64>().ok()?;
    if !is_valid_key(key) {
        return None;
    }
    Some((ts, key.to_string(), old.to_string()))
}

fn parse_undo(raw: &str) -> Option<UndoEntry> {
    let last = raw
        .lines()
        .rev()
        .map(|l| l.trim_end_matches('\r'))
        .find(|l| !l.trim().is_empty())?;
    let (_, key, old) = parse_change_line(last)?;
    Some(UndoEntry { key, old })
}

fn is_restart_key(key: &str) -> bool {
    key.starts_with("OUTPUT_") || RESTART_KEYS.contains(&key)
}

fn parse_restart_keys(raw: &str, compositor_start: Option<u64>) -> Vec<String> {
    let Some(start) = compositor_start else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    for line in raw.lines() {
        let Some((ts, key, _)) = parse_change_line(line) else {
            continue;
        };
        if ts >= start && is_restart_key(&key) && !keys.iter().any(|k| k == &key) {
            keys.push(key);
        }
    }
    keys
}

fn undo_old_word(old: &str) -> &str {
    match old {
        "-" => "unset",
        "" => "empty",
        s => s,
    }
}

fn undo_button_label(cr: &Context, th: &Theme, max_w: f64, entry: &UndoEntry) -> String {
    let full = format!("Undo: {} was {}", entry.key, undo_old_word(&entry.old));
    let fits = |text: &str| -> bool {
        draw::measure(
            cr,
            &Text {
                size: 12.0,
                bold: false,
                color: th.text,
                text,
            },
        )
        .0 + 18.0
            <= max_w
    };
    if fits(&full) {
        return full;
    }
    let old_word = undo_old_word(&entry.old);
    let prefix = format!("Undo: {} was ", entry.key);
    let mut len = old_word.len();
    while len > 0 {
        while len > 0 && !old_word.is_char_boundary(len) {
            len -= 1;
        }
        if len == 0 {
            break;
        }
        // Step back one UTF-8 scalar boundary at a time so ellipsis stays inside `max_w`.
        let prev = old_word[..len]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        len = prev;
        if len > 0 {
            let candidate = format!("{prefix}{}…", &old_word[..len]);
            if fits(&candidate) {
                return candidate;
            }
        }
    }
    let key_only = format!("Undo: {}", entry.key);
    if fits(&key_only) {
        key_only
    } else {
        "Undo".to_string()
    }
}

fn undo_command(entry: &UndoEntry) -> String {
    let k = settings::sq(&entry.key);
    let restore = if entry.old == "-" {
        format!(r#"sh "$HOME/infynitae/live/settings.sh" unset {k} >/dev/null 2>&1"#)
    } else {
        let v = settings::sq(&entry.old);
        format!(r#"sh "$HOME/infynitae/live/settings.sh" set {k} {v} >/dev/null 2>&1"#)
    };
    // only while the log's last line is still the change the button shows: data read when the panel
    // opened can be a change behind, and undoing it would drop the record of a newer one
    let (sk, so) = (settings::sq(&entry.key), settings::sq(&entry.old));
    format!(
        r#"p="$HOME/.local/state/infynitae/settings-changes"; if [ "$(tail -n 1 "$p" | cut -f 2-)" = "$(printf '%s\t%s' {sk} {so})" ] && {restore}; then t="$p.tmp.$$"; sed '$d' "$p" > "$t" && mv "$t" "$p" || rm -f "$t"; fi"#
    )
}

#[derive(Default)]
pub struct AboutCard {
    loaded: bool,
    info: PcInfo,
    query: String,
}

struct InfoRow {
    left: String,
    right: String,
    hay: String,
}

struct ActButton {
    id: u32,
    label: String,
    x: f64,
    y: f64,
}

impl AboutCard {
    fn matches(&self, hay: &str) -> bool {
        if self.query.trim().is_empty() {
            return true;
        }
        let lower = hay.to_lowercase();
        self.query
            .split_whitespace()
            .all(|w| lower.contains(&w.to_lowercase()))
    }

    fn info_rows(&self, size: Size) -> Vec<InfoRow> {
        let searching = !self.query.trim().is_empty();
        let mut rows = Vec::new();
        if size == Size::S && !searching {
            match (&self.info.hostname, &self.info.system) {
                (Some(h), Some(s)) => rows.push(InfoRow {
                    left: h.clone(),
                    right: s.clone(),
                    hay: format!("name pc hostname system os kernel {h} {s}"),
                }),
                (Some(h), None) => rows.push(InfoRow {
                    left: h.clone(),
                    right: String::new(),
                    hay: format!("name pc hostname {h}"),
                }),
                (None, Some(s)) => rows.push(InfoRow {
                    left: "System".to_string(),
                    right: s.clone(),
                    hay: format!("system os kernel {s}"),
                }),
                (None, None) => {}
            }
            return rows;
        }

        if let Some(h) = &self.info.hostname {
            rows.push(InfoRow {
                left: h.clone(),
                right: String::new(),
                hay: format!("name pc hostname {h}"),
            });
        }
        let pairs: [(&str, &Option<String>, &str); 6] = [
            ("System", &self.info.system, "system os linux kernel"),
            ("Processor", &self.info.processor, "processor cpu"),
            ("Memory", &self.info.memory, "memory ram"),
            ("Graphics", &self.info.graphics, "graphics gpu video"),
            ("Infynitae", &self.info.infynitae, "infynitae version git"),
            ("Time zone", &self.info.timezone, "time zone timezone"),
        ];
        for (label, val, extra) in pairs {
            if let Some(v) = val {
                rows.push(InfoRow {
                    left: label.to_string(),
                    right: v.clone(),
                    hay: format!("{label} {extra} {v}"),
                });
            }
        }
        rows.into_iter().filter(|r| self.matches(&r.hay)).collect()
    }

    fn restart_subtext(&self) -> Option<String> {
        if self.info.restart_keys.is_empty() {
            return None;
        }
        let keys = self.info.restart_keys.join(", ");
        let sub = format!("{keys}. Ctrl+Alt+Backspace restarts; it closes your windows");
        let hay = format!("restart needed {sub}");
        if self.matches(&hay) {
            Some(sub)
        } else {
            None
        }
    }

    fn visible_buttons(&self, cr: &Context, th: &Theme, max_w: f64) -> Vec<(u32, String)> {
        let mut out = Vec::new();
        if self.matches("check for updates update") {
            out.push((HIT_UPDATE, "Check for updates".to_string()));
        }
        if self.matches("open the config file edit settings") {
            out.push((HIT_EDIT, "Open the config file".to_string()));
        }
        if let Some(entry) = &self.info.undo {
            let old_word = undo_old_word(&entry.old);
            let hay = format!("undo {} {} {old_word}", entry.key, entry.old);
            if self.matches(&hay) {
                let label = undo_button_label(cr, th, max_w, entry);
                out.push((HIT_UNDO, label));
            }
        }
        out
    }

    fn layout_buttons(
        &self,
        cr: &Context,
        th: &Theme,
        left: f64,
        right: f64,
        start_y: f64,
    ) -> (Vec<ActButton>, f64) {
        let max_w = (right - left).max(60.0);
        let items = self.visible_buttons(cr, th, max_w);
        if items.is_empty() {
            return (Vec::new(), start_y);
        }
        let mut placed = Vec::with_capacity(items.len());
        let mut x = left;
        let mut y = start_y;
        for (id, label) in items {
            let bw = draw::measure(
                cr,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.text,
                    text: &label,
                },
            )
            .0 + 18.0;
            if x > left && x + bw > right + 0.5 {
                x = left;
                y += BTN_H + BTN_GAP_Y;
            }
            placed.push(ActButton { id, label, x, y });
            x += bw + BTN_GAP_X;
        }
        (placed, y + BTN_H)
    }

    fn wrap_subtext_layout(cr: &Context, max_w: f64, text: &str) -> pango::Layout {
        let l = pangocairo::functions::create_layout(cr);
        let mut fd = pango::FontDescription::from_string(FONT);
        fd.set_absolute_size(11.0 * pango::SCALE as f64);
        l.set_font_description(Some(&fd));
        l.set_width((max_w.max(40.0) * pango::SCALE as f64) as i32);
        l.set_wrap(pango::WrapMode::WordChar);
        l.set_text(text);
        l
    }

    fn measure_and_draw(
        &self,
        cr: &Context,
        cx: &Cx,
        body: Rect,
        mut hits: Option<&mut Hits>,
    ) -> f64 {
        let th = cx.theme;
        let left = body.0 + SIDE_PAD;
        let right = body.0 + body.2 - SIDE_PAD;
        let inner_w = (right - left).max(40.0);
        let mut y = body.1;

        let rows = self.info_rows(cx.size);
        for row in &rows {
            if hits.is_some() {
                draw_info_row(cr, th, left, right, y, inner_w, row);
            }
            y += INFO_ROW_H;
        }

        let btn_start_y = if !rows.is_empty() && !self.visible_buttons(cr, th, inner_w).is_empty() {
            y + ACTS_TOP_GAP
        } else {
            y
        };
        let (buttons, after_buttons_y) = self.layout_buttons(cr, th, left, right, btn_start_y);
        if let Some(ref mut h) = hits {
            for b in &buttons {
                ui::button(cr, cx, h, b.id, b.x, b.y, &b.label);
            }
        }
        y = after_buttons_y;

        if let Some(sub) = self.restart_subtext() {
            if y > body.1 {
                y += RESTART_TOP_GAP;
            }
            let sub_layout = Self::wrap_subtext_layout(cr, inner_w, &sub);
            let sub_h = sub_layout.pixel_size().1 as f64;
            if hits.is_some() {
                draw::text(
                    cr,
                    left,
                    y,
                    &Text {
                        size: 13.0,
                        bold: false,
                        color: th.accent_text,
                        text: "Restart needed",
                    },
                );
                draw::set(cr, th.muted);
                cr.move_to(left.round(), (y + RESTART_TITLE_H).round());
                pangocairo::functions::show_layout(cr, &sub_layout);
            }
            y += RESTART_TITLE_H + sub_h + 2.0;
        }

        y - body.1
    }

    fn has_visible_content(&self, cx: &Cx) -> bool {
        if !self.info_rows(cx.size).is_empty() || self.restart_subtext().is_some() {
            return true;
        }
        let undo_match = self.info.undo.as_ref().is_some_and(|e| {
            let old_word = undo_old_word(&e.old);
            self.matches(&format!("undo {} {} {old_word}", e.key, e.old))
        });
        self.matches("check for updates update")
            || self.matches("open the config file edit settings")
            || undo_match
    }
}

fn draw_info_row(
    cr: &Context,
    th: &Theme,
    left: f64,
    right: f64,
    y: f64,
    inner_w: f64,
    row: &InfoRow,
) {
    let left_t = Text {
        size: 13.0,
        bold: false,
        color: th.text,
        text: &row.left,
    };
    if row.right.is_empty() {
        draw::text_max(cr, left, y + 5.0, inner_w, &left_t);
        return;
    }
    let lw = draw::measure(cr, &left_t).0;
    let right_t = Text {
        size: 12.0,
        bold: false,
        color: th.muted,
        text: &row.right,
    };
    let vw = draw::measure(cr, &right_t).0;
    let max_left = if lw + 12.0 + vw <= inner_w {
        lw
    } else {
        lw.min(((inner_w - 12.0) * 0.45).max(60.0))
    };
    draw::text_max(cr, left, y + 5.0, max_left, &left_t);
    let avail_right = (inner_w - max_left - 12.0).max(40.0);
    if vw <= avail_right {
        draw::text(cr, right - vw, y + 5.5, &right_t);
    } else {
        draw::text_max(cr, right - avail_right, y + 5.5, avail_right, &right_t);
    }
}

impl Card for AboutCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        if key != "pc" {
            return false;
        }
        let Some(info) = data.downcast_ref::<PcInfo>() else {
            return false;
        };
        let changed = !self.loaded || self.info != *info;
        self.info = info.clone();
        self.loaded = true;
        changed
    }

    fn status(&self, cx: &Cx) -> Status {
        if !self.loaded {
            return Status::Loading;
        }
        if !self.has_visible_content(cx) {
            return Status::Hidden;
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
        self.measure_and_draw(&cr, cx, (0.0, 0.0, width, 0.0), None)
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let _ = self.measure_and_draw(cr, cx, body, Some(hits));
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
                ..
            } => match *hit {
                HIT_UPDATE => {
                    out.push(Action::Run(UPDATE_COMMAND.to_string()));
                    out.push(Action::ClosePanel);
                    false
                }
                HIT_EDIT => {
                    out.push(Action::Run(EDIT_COMMAND.to_string()));
                    out.push(Action::ClosePanel);
                    false
                }
                HIT_UNDO => {
                    if let Some(entry) = &self.info.undo {
                        out.push(Action::Run(undo_command(entry)));
                    }
                    false
                }
                _ => false,
            },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::config::Config;
    use crate::provider::Provider;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::SystemTime;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("edge-apps-about-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sample_info() -> PcInfo {
        PcInfo {
            hostname: Some("Aevyternae".into()),
            system: Some("Arch Linux, kernel 7.2.15-arch1-1".into()),
            processor: Some("AMD Ryzen 5 7600X3D 6-Core Processor".into()),
            memory: Some("31 GB".into()),
            graphics: Some("Navi 48 [Radeon RX 9070/9070 XT/9070 GRE]".into()),
            infynitae: Some("0872790 2026-11-13".into()),
            timezone: Some("Europe/Chisinau".into()),
            undo: Some(UndoEntry {
                key: "ROUNDING".into(),
                old: "8".into(),
            }),
            restart_keys: vec!["OUTPUT_SCALE".into(), "XKB_DEFAULT_LAYOUT".into()],
        }
    }

    #[test]
    fn provider_wires_about_card_and_pc_source() {
        let p = crate::cards::settings::Settings;
        let k = p.kind("about").expect("about kind");
        assert_eq!(k.sources, ["pc"]);
        assert!(p.card("about", None).is_some());
        assert!(p.card("about", Some("extra")).is_none());
        let src = p.source("pc").expect("pc source registered");
        assert_eq!(src.every(), None);

        let cfg = Config::from_text("PANEL_SETTINGS=\"SUPER+I top center 440 settings/about\"\n");
        let panel = cfg.panels.iter().find(|p| p.name == "settings").unwrap();
        let placed = crate::shell::place(panel, &[], "");
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].kind.id, "about");
        assert_eq!(placed[0].size, Size::S);
    }

    #[test]
    fn parses_real_looking_inputs_and_temp_folder() {
        assert_eq!(
            parse_hostname("# comment\nAevyternae\n"),
            Some("Aevyternae".into())
        );
        assert_eq!(parse_hostname("\n   \n"), None);

        let os = "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\n";
        assert_eq!(
            parse_system(os, "7.2.15-arch1-1\n"),
            Some("Arch Linux, kernel 7.2.15-arch1-1".into())
        );
        assert_eq!(
            parse_system("PRETTY_NAME=\"Custom \\\"OS\\\"\"\n", ""),
            Some("Custom \"OS\"".into())
        );
        assert_eq!(
            parse_system("", "7.2.15-arch1-1"),
            Some("kernel 7.2.15-arch1-1".into())
        );
        assert_eq!(parse_system("ID=broken\n", "  "), None);

        let cpu = "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD Ryzen 5 7600X3D 6-Core Processor\nprocessor\t: 1\nmodel name\t: Other\n";
        assert_eq!(
            parse_cpuinfo(cpu),
            Some("AMD Ryzen 5 7600X3D 6-Core Processor".into())
        );
        assert_eq!(parse_cpuinfo(""), None);
        assert_eq!(parse_cpuinfo("model name\t:   \n"), None);

        let mem = "MemTotal:       32453280 kB\nMemFree:        1000 kB\n";
        assert_eq!(parse_meminfo(mem), Some("31 GB".into()));
        assert_eq!(
            parse_meminfo("MemTotal: 524288 kB\n"),
            Some("512 MB".into())
        );
        assert_eq!(parse_meminfo("MemTotal: 0 kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal: notanumber\n"), None);
        assert_eq!(parse_meminfo(""), None);

        let lspci = "\
00:02.0 \"Host bridge\" \"AMD\" \"Root Complex\"\n\
03:00.0 \"VGA compatible controller\" \"Advanced Micro Devices, Inc. [AMD/ATI]\" \"Navi 48 [Radeon RX 9070/9070 XT/9070 GRE]\" -rc0 -p00 \"Tul Corporation / PowerColor\" \"Radeon RX 9070 XT 16GB\"\n\
04:00.0 \"3D controller\" \"NVIDIA\" \"AD107M\"\n";
        assert_eq!(
            parse_lspci(lspci),
            Some("Navi 48 [Radeon RX 9070/9070 XT/9070 GRE], AD107M".into())
        );
        assert_eq!(parse_lspci(""), None);
        assert_eq!(parse_lspci("00:00.0 \"VGA\" \"Vendor\" \"unclosed"), None);
        assert_eq!(
            parse_lspci("00:00.0 \"Network controller\" \"Intel\" \"AX210\""),
            None
        );

        let tmp = TestDir::new("usual");
        let dir = tmp.path();
        std::fs::write(dir.join("hostname"), "studio\n").unwrap();
        std::fs::write(dir.join("os-release"), "PRETTY_NAME=\"Arch Linux\"\n").unwrap();
        std::fs::write(dir.join("osrelease"), "7.2.15\n").unwrap();
        std::fs::write(dir.join("cpuinfo"), "model name : Ryzen\n").unwrap();
        std::fs::write(dir.join("meminfo"), "MemTotal: 16777216 kB\n").unwrap();
        std::fs::write(
            dir.join("settings-changes"),
            "900\tOUTPUT_SCALE\t1\n1000\tXKB_DEFAULT_LAYOUT\tus\n1050\tROUNDING\t8\n",
        )
        .unwrap();
        let sock = dir.join("wayland-0");
        let f = std::fs::File::create(&sock).unwrap();
        f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1000))
            .unwrap();

        let mut src = PcSource::with_paths(PcPaths {
            hostname: dir.join("hostname"),
            os_release: dir.join("os-release"),
            kernel_osrelease: dir.join("osrelease"),
            cpuinfo: dir.join("cpuinfo"),
            meminfo: dir.join("meminfo"),
            settings_changes: dir.join("settings-changes"),
            wayland_socket: Some(sock),
            infynitae_repo: None,
            run_external_cmds: false,
        });
        let data = src.fetch();
        let info = data.downcast_ref::<PcInfo>().unwrap();
        assert_eq!(info.hostname.as_deref(), Some("studio"));
        assert_eq!(info.system.as_deref(), Some("Arch Linux, kernel 7.2.15"));
        assert_eq!(info.processor.as_deref(), Some("Ryzen"));
        assert_eq!(info.memory.as_deref(), Some("16 GB"));
        assert_eq!(info.graphics, None);
        assert_eq!(info.infynitae, None);
        assert_eq!(info.timezone, None);
        assert_eq!(
            info.undo,
            Some(UndoEntry {
                key: "ROUNDING".into(),
                old: "8".into(),
            })
        );
        // 900 < compositor start (1000), so only XKB_DEFAULT_LAYOUT is flagged.
        assert_eq!(info.restart_keys, vec!["XKB_DEFAULT_LAYOUT"]);
    }

    #[test]
    fn temp_folder_handles_missing_and_broken_inputs() {
        let tmp = TestDir::new("usual");
        let dir = tmp.path();
        std::fs::write(
            dir.join("settings-changes"),
            "1000\tGOOD\t1\nbadline\nnot_ts\tKEY\t2\n1001\t;bad_key\t3\n",
        )
        .unwrap();
        let mut src = PcSource::with_paths(PcPaths {
            hostname: dir.join("missing-hostname"),
            os_release: dir.join("missing-os"),
            kernel_osrelease: dir.join("missing-kernel"),
            cpuinfo: dir.join("missing-cpu"),
            meminfo: dir.join("missing-mem"),
            settings_changes: dir.join("settings-changes"),
            wayland_socket: None,
            infynitae_repo: None,
            run_external_cmds: false,
        });
        let data = src.fetch();
        let info = data.downcast_ref::<PcInfo>().unwrap();
        assert_eq!(*info, PcInfo::default());
    }

    #[test]
    fn undo_and_restart_parsing_cover_empty_broken_and_injection() {
        assert_eq!(parse_undo(""), None);
        assert_eq!(parse_undo("   \n\n"), None);
        assert_eq!(
            parse_undo("100\tVALID_KEY\t-\n   \n"),
            Some(UndoEntry {
                key: "VALID_KEY".into(),
                old: "-".into(),
            })
        );
        assert_eq!(parse_undo("100\tBAD;KEY\tval\n"), None);
        assert_eq!(parse_undo("100\tROUNDING\t8\nbroken_tail\n"), None);

        let changes = "500\tOUTPUT_SCALE\t1\n1000\tOUTPUT_SCALE\t2\n1010\tROUNDING\t8\n1020\tOUTPUT_DP_1\t1920x1080\n1030\tOUTPUT_SCALE\t1.5\n1040\tBATTERY_LIMIT\t80\n";
        assert_eq!(parse_restart_keys(changes, None), Vec::<String>::new());
        assert_eq!(
            parse_restart_keys(changes, Some(1000)),
            vec!["OUTPUT_SCALE", "OUTPUT_DP_1"]
        );

        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap();
        let def = super::super::settings_rows::parse_config(&text);
        for k in RESTART_KEYS {
            assert!(def.contains_key(*k), "{k} missing from config/config");
        }
        assert!(!is_restart_key("BATTERY_LIMIT"));
        assert!(!is_restart_key("MAX_APP_SCALE"));
        assert!(!is_restart_key("ZOOM_SHARPEN"));
    }

    #[test]
    fn hiding_rules_and_search_filter() {
        let th = Theme::from_config(&Config::from_text(""));
        let mut cx = test_cx(&th);
        let mut card = AboutCard::default();
        assert_eq!(card.status(&cx), Status::Loading);

        let mut info = sample_info();
        card.apply("other", &info);
        assert_eq!(card.status(&cx), Status::Loading);

        assert!(card.apply("pc", &info));
        assert!(!card.apply("pc", &info));
        assert_eq!(card.status(&cx), Status::Ready);

        // Size S hides hardware, Infynitae and Time zone, keeping name, System, Restart and actions.
        cx.size = Size::S;
        let s_rows = card.info_rows(Size::S);
        assert_eq!(s_rows.len(), 1);
        assert_eq!(s_rows[0].left, "Aevyternae");
        assert_eq!(s_rows[0].right, "Arch Linux, kernel 7.2.15-arch1-1");

        // Size M shows all populated rows; empty ones are hidden.
        cx.size = Size::M;
        assert_eq!(card.info_rows(Size::M).len(), 7);

        info.processor = None;
        info.graphics = None;
        info.undo = None;
        info.restart_keys.clear();
        card.apply("pc", &info);
        assert_eq!(card.info_rows(Size::M).len(), 5);
        assert_eq!(card.restart_subtext(), None);

        // Search filters rows (even when Size::S) and hides card when nothing matches.
        let mut out = Vec::new();
        assert!(card.event(&Event::Text("memory"), &cx, &mut out));
        cx.size = Size::S;
        assert_eq!(card.status(&cx), Status::Ready);
        let filtered = card.info_rows(Size::S);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].left, "Memory");
        assert_eq!(filtered[0].right, "31 GB");

        assert!(card.event(&Event::Text("zzz-missing-zzz"), &cx, &mut out));
        assert_eq!(card.status(&cx), Status::Hidden);
    }

    #[test]
    fn button_and_undo_commands_quote_values() {
        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th);
        let mut card = AboutCard::default();
        let mut info = sample_info();
        info.undo = Some(UndoEntry {
            key: "CURSOR_THEME".into(),
            old: "a'b $(touch /tmp/pwn)".into(),
        });
        card.apply("pc", &info);

        let mut out = Vec::new();
        card.event(
            &Event::Press {
                hit: HIT_UPDATE,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        );
        assert_eq!(
            out,
            vec![Action::Run(UPDATE_COMMAND.into()), Action::ClosePanel]
        );

        out.clear();
        card.event(
            &Event::Press {
                hit: HIT_EDIT,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        );
        assert_eq!(
            out,
            vec![Action::Run(EDIT_COMMAND.into()), Action::ClosePanel]
        );

        out.clear();
        card.event(
            &Event::Press {
                hit: HIT_UNDO,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        );
        assert_eq!(
            out,
            vec![Action::Run(
                r#"p="$HOME/.local/state/infynitae/settings-changes"; if [ "$(tail -n 1 "$p" | cut -f 2-)" = "$(printf '%s\t%s' 'CURSOR_THEME' 'a'\''b $(touch /tmp/pwn)')" ] && sh "$HOME/infynitae/live/settings.sh" set 'CURSOR_THEME' 'a'\''b $(touch /tmp/pwn)' >/dev/null 2>&1; then t="$p.tmp.$$"; sed '$d' "$p" > "$t" && mv "$t" "$p" || rm -f "$t"; fi"#.into()
            )]
        );

        let unset_cmd = undo_command(&UndoEntry {
            key: "GAPS_IN".into(),
            old: "-".into(),
        });
        assert_eq!(
            unset_cmd,
            r#"p="$HOME/.local/state/infynitae/settings-changes"; if [ "$(tail -n 1 "$p" | cut -f 2-)" = "$(printf '%s\t%s' 'GAPS_IN' '-')" ] && sh "$HOME/infynitae/live/settings.sh" unset 'GAPS_IN' >/dev/null 2>&1; then t="$p.tmp.$$"; sed '$d' "$p" > "$t" && mv "$t" "$p" || rm -f "$t"; fi"#
        );

        // Execute undo command against an isolated stub HOME to verify quoting and tail removal.
        let tmp = TestDir::new("undo-exec");
        let h = tmp.path();
        let pwn = h.join("injected");
        let inj_old = format!("a'b $(touch {})", pwn.display());
        info.undo = Some(UndoEntry {
            key: "CURSOR_THEME".into(),
            old: inj_old.clone(),
        });
        card.apply("pc", &info);
        out.clear();
        card.event(
            &Event::Press {
                hit: HIT_UNDO,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        );
        std::fs::create_dir_all(h.join("infynitae/live")).unwrap();
        std::fs::create_dir_all(h.join(".local/state/infynitae")).unwrap();
        std::fs::write(
            h.join("infynitae/live/settings.sh"),
            "#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" \"$3\" > \"$HOME/args.log\"\n",
        )
        .unwrap();
        std::fs::write(
            h.join(".local/state/infynitae/settings-changes"),
            format!("100\tROUNDING\t-\n200\tCURSOR_THEME\t{inj_old}\n"),
        )
        .unwrap();
        let cmd = match &out[0] {
            Action::Run(s) => s,
            _ => panic!("expected Run action"),
        };
        let status = Command::new("sh")
            .args(["-c", cmd])
            .env("HOME", h)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!pwn.exists());
        assert_eq!(
            std::fs::read_to_string(h.join("args.log")).unwrap(),
            format!("set\nCURSOR_THEME\n{inj_old}\n")
        );
        assert_eq!(
            std::fs::read_to_string(h.join(".local/state/infynitae/settings-changes")).unwrap(),
            "100\tROUNDING\t-\n"
        );

        let status = Command::new("sh")
            .args([
                "-c",
                &undo_command(&UndoEntry {
                    key: "ROUNDING".into(),
                    old: "-".into(),
                }),
            ])
            .env("HOME", h)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(h.join("args.log")).unwrap(),
            "unset\nROUNDING\n\n"
        );
        assert_eq!(
            std::fs::read_to_string(h.join(".local/state/infynitae/settings-changes")).unwrap(),
            ""
        );

        // a stale button (read before a newer change) does nothing: no set, the newer line kept
        let log = "300\tGAME_MODE\ttrue\n400\tSUPER_ENTER_ACTION\tmaximize\n";
        std::fs::write(h.join(".local/state/infynitae/settings-changes"), log).unwrap();
        std::fs::remove_file(h.join("args.log")).unwrap();
        let stale = undo_command(&UndoEntry {
            key: "GAME_MODE".into(),
            old: "true".into(),
        });
        let status = Command::new("sh")
            .args(["-c", &stale])
            .env("HOME", h)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!h.join("args.log").exists());
        assert_eq!(
            std::fs::read_to_string(h.join(".local/state/infynitae/settings-changes")).unwrap(),
            log
        );
    }

    #[test]
    fn height_matches_what_is_drawn() {
        let th = Theme::from_config(&Config::from_text(""));
        let mut cx = test_cx(&th);
        let mut card = AboutCard::default();
        let mut info = sample_info();
        info.undo = Some(UndoEntry {
            key: "CURSOR_THEME".into(),
            old: "very-long-cursor-theme-name-that-would-overflow-narrow-card".into(),
        });
        card.apply("pc", &info);

        for (size, width) in [(Size::M, 420.0), (Size::M, 260.0), (Size::S, 420.0)] {
            cx.size = size;
            let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 500, 600).unwrap();
            let cr = Context::new(&surf).unwrap();
            let mut hits = Hits::default();
            let h = card.height(&cx, width);
            let drawn = card.measure_and_draw(&cr, &cx, (10.0, 20.0, width, h), Some(&mut hits));
            assert!((h - drawn).abs() < 0.01, "size={size:?} width={width}");
            assert!(hits
                .list
                .iter()
                .all(|(_, r)| r.0 >= 10.0 && r.0 + r.2 <= 10.0 + width + 0.5));
        }
    }
}
