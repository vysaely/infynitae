//! Settings card `settings/about` ("This PC"): hardware and OS facts, update and config
//! actions, the last undo entry, and a warning when a restart setting changed in this session
//! (docs/EDGE_SETTINGS.md section 4).

use crate::card::{self, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::cards::settings;
use crate::draw::{self, Text, FONT};
use crate::provider::{CardKind, Size};
use crate::theme::Theme;
use crate::ui::{self, Row};
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
const SAVE_COMMAND: &str = r#"sh "$HOME/infynitae/live/settings.sh" export >/dev/null 2>&1"#;

const INFO_ROW_H: f64 = 26.0;
const SIDE_PAD: f64 = 8.0;
const BTN_H: f64 = 26.0;
const BTN_GAP_X: f64 = 8.0;
const BTN_GAP_Y: f64 = 6.0;
const ACTS_TOP_GAP: f64 = 8.0;
const RESTART_TOP_GAP: f64 = 10.0;
const RESTART_TITLE_H: f64 = 18.0;
const FILE_SUB_H: f64 = 16.0;
const MAX_LOAD_ROWS: usize = 6;

const HIT_UPDATE: u32 = 1;
const HIT_EDIT: u32 = 2;
const HIT_UNDO: u32 = 3;
const HIT_SAVE: u32 = 4;
const HIT_LOAD: u32 = 5;
const HIT_UNDO_LOAD: u32 = 6;
const HIT_LOAD_APPLY: u32 = 7;
const HIT_LOAD_CANCEL: u32 = 8;
const HIT_LOAD_ITEM_BASE: u32 = 100;

/// Keys tagged `[restart]` in `config/config`. All `OUTPUT_*` display keys also require restart
/// (and `BATTERY_LIMIT` runs `battery-limit`; `SCALING_FILTER`, `MAX_APP_SCALE`, `ZOOM_SHARPEN`
/// reload live on `[save]`).
const RESTART_KEYS: &[&str] = &[
    "XKB_DEFAULT_LAYOUT",
    "XKB_DEFAULT_OPTIONS",
    "OUTPUT_ORDER",
    "OUTPUT_LAYOUT",
    "OUTPUT_SCALE",
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct SavedFile {
    name: String,
    path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImportPreview {
    id: String,
    path: String,
    result: Result<Vec<String>, String>,
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
    saved_files: Vec<SavedFile>,
    preview: Option<ImportPreview>,
    load_active: bool,
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
    export_dir: PathBuf,
    preview_file: PathBuf,
    load_active_file: PathBuf,
    run_external_cmds: bool,
}

fn runtime_infynitae_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".cache"))
        })
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("infynitae")
}

/// Clears transient "Load settings" preview and "Undo load" markers when the panel closes.
pub(crate) fn clear_load_state() {
    let dir = runtime_infynitae_dir();
    let _ = std::fs::remove_file(dir.join("settings-load-active"));
    let _ = std::fs::remove_file(dir.join("settings-load-preview"));
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
        let export_dir = home
            .as_ref()
            .map(|h| h.join("Documents/Infynitae"))
            .unwrap_or_else(|| PathBuf::from("Documents/Infynitae"));
        let rt_dir = runtime_infynitae_dir();
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
            export_dir,
            preview_file: rt_dir.join("settings-load-preview"),
            load_active_file: rt_dir.join("settings-load-active"),
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

        let saved_files = card::render_value("pc_saved")
            .map(|s| parse_saved_fixture(&s))
            .unwrap_or_else(|| scan_saved_files(&self.paths.export_dir));
        let preview = read_or_render(&self.paths.preview_file, "pc_preview")
            .as_deref()
            .and_then(parse_import_preview);
        let load_active = card::render_value("pc_load_active")
            .map(|v| v == "1" || v == "true")
            .unwrap_or_else(|| self.paths.load_active_file.is_file());

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
            saved_files,
            preview,
            load_active,
        };
        Box::new(info)
    }
}

fn parse_saved_fixture(raw: &str) -> Vec<SavedFile> {
    let home = std::env::var("HOME").unwrap_or_default();
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let expanded = if !home.is_empty() && l.starts_with("~/") {
                format!("{home}{}", &l[1..])
            } else {
                l.to_string()
            };
            let name = Path::new(&expanded)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(l)
                .to_string();
            SavedFile {
                name,
                path: expanded,
            }
        })
        .collect()
}

fn scan_saved_files(dir: &Path) -> Vec<SavedFile> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
        found.push((
            mtime,
            SavedFile {
                name: name.to_string(),
                path: path.to_string_lossy().into_owned(),
            },
        ));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.name.cmp(&a.1.name)));
    found.into_iter().map(|(_, f)| f).collect()
}

fn parse_import_preview(raw: &str) -> Option<ImportPreview> {
    let mut lines = raw.lines();
    let id = lines.next()?.strip_prefix("id=")?.trim().to_string();
    let path = lines.next()?.strip_prefix("path=")?.trim().to_string();
    let ok = lines.next()?.strip_prefix("ok=")?.trim();
    if id.is_empty() || path.is_empty() {
        return None;
    }
    let result = match ok {
        "1" => Ok(lines
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()),
        "0" => {
            let msg = lines
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("Could not read archive.")
                .to_string();
            Err(msg)
        }
        _ => return None,
    };
    Some(ImportPreview { id, path, result })
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

fn undo_old_word_for<'a>(key: &str, old: &'a str) -> &'a str {
    if key == "FACE" && old != "-" && !old.is_empty() {
        return "previous";
    }
    undo_old_word(old)
}

fn undo_button_label(cr: &Context, th: &Theme, max_w: f64, entry: &UndoEntry) -> String {
    let old_word = undo_old_word_for(&entry.key, &entry.old);
    let full = format!("Undo: {} was {old_word}", entry.key);
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
    let restore = if entry.key == "FACE" {
        if entry.old == "-" {
            r#"rm -f "$HOME/.face""#.to_string()
        } else {
            r#"[ -e "$HOME/.face.bak" ] && mv -f "$HOME/.face.bak" "$HOME/.face""#.to_string()
        }
    } else {
        let k = settings::sq(&entry.key);
        if entry.old == "-" {
            format!(r#"sh "$HOME/infynitae/live/settings.sh" unset {k} >/dev/null 2>&1"#)
        } else {
            let v = settings::sq(&entry.old);
            format!(r#"sh "$HOME/infynitae/live/settings.sh" set {k} {v} >/dev/null 2>&1"#)
        }
    };
    // only while the log's last line is still the change the button shows: data read when the panel
    // opened can be a change behind, and undoing it would drop the record of a newer one
    let (sk, so) = (settings::sq(&entry.key), settings::sq(&entry.old));
    format!(
        r#"p="$HOME/.local/state/infynitae/settings-changes"; if [ "$(tail -n 1 "$p" | cut -f 2-)" = "$(printf '%s\t%s' {sk} {so})" ] && {restore}; then t="$p.tmp.$$"; sed '$d' "$p" > "$t" && mv "$t" "$p" || rm -f "$t"; fi"#
    )
}

fn preview_command(path: &str) -> String {
    let q = settings::sq(path);
    format!(
        "d=\"${{XDG_RUNTIME_DIR:-$HOME/.cache}}/infynitae\"; mkdir -p \"$d\"; \
         t=$(mktemp \"$d/.load-preview.XXXXXX\") || exit 1; \
         id=$(date +%s%N 2>/dev/null || date +%s)-$$; \
         if k=$(sh \"$HOME/infynitae/live/settings.sh\" import-preview {q} 2>\"$t.err\"); then \
         printf 'id=%s\\npath=%s\\nok=1\\n%s\\n' \"$id\" {q} \"$k\" > \"$t\"; \
         else \
         err=$(head -n 1 \"$t.err\"); \
         printf 'id=%s\\npath=%s\\nok=0\\n%s\\n' \"$id\" {q} \"${{err:-Could not read archive.}}\" > \"$t\"; \
         fi; \
         rm -f \"$t.err\"; mv -f \"$t\" \"$d/settings-load-preview\""
    )
}

fn clear_preview_command() -> String {
    r#"rm -f "${XDG_RUNTIME_DIR:-$HOME/.cache}/infynitae/settings-load-preview""#.to_string()
}

fn apply_load_command(path: &str) -> String {
    let q = settings::sq(path);
    format!(
        "d=\"${{XDG_RUNTIME_DIR:-$HOME/.cache}}/infynitae\"; mkdir -p \"$d\"; \
         if sh \"$HOME/infynitae/live/settings.sh\" import {q} >/dev/null 2>&1; then \
         rm -f \"$d/settings-load-preview\"; : > \"$d/settings-load-active\"; \
         else false; fi"
    )
}

fn undo_load_command() -> String {
    r#"d="${XDG_RUNTIME_DIR:-$HOME/.cache}/infynitae"; rm -f "$d/settings-load-active"; c="${CONFIG_FILE:-$HOME/.config/infynitae/config}"; b="${c}.before-load"; if [ -f "$b" ]; then mv -f "$b" "$c" && sh "$HOME/infynitae/live/settings.sh" apply >/dev/null 2>&1; fi"#.to_string()
}

fn key_count_word(n: usize) -> String {
    if n == 1 {
        "1 key".to_string()
    } else {
        format!("{n} keys")
    }
}

fn preview_summary(keys: &[String]) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    if keys.len() <= 8 {
        Some(keys.join(", "))
    } else {
        Some(format!(
            "{}, and {} more",
            keys[..8].join(", "),
            keys.len() - 8
        ))
    }
}

#[derive(Default)]
pub struct AboutCard {
    loaded: bool,
    info: PcInfo,
    query: String,
    load_open: bool,
    load_scroll: usize,
    dismissed_preview: Option<String>,
    undo_load: bool,
    loading_name: Option<String>,
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

    fn active_preview(&self) -> Option<&ImportPreview> {
        let prev = self.info.preview.as_ref()?;
        if self.dismissed_preview.as_deref() == Some(prev.id.as_str()) {
            return None;
        }
        Some(prev)
    }

    fn showing_load_list(&self) -> bool {
        self.load_open
            && (self.matches("load settings import restore archive")
                || self.info.saved_files.iter().any(|f| self.matches(&f.name)))
    }

    fn showing_preview(&self) -> bool {
        self.active_preview().is_some_and(|p| {
            self.matches(&format!("load settings preview apply cancel {}", p.path))
        })
    }

    fn showing_loading(&self) -> bool {
        self.loading_name.is_some()
            && self.active_preview().is_none()
            && self.matches("load settings checking")
    }

    fn visible_buttons(&self, cr: &Context, th: &Theme, max_w: f64) -> Vec<(u32, String)> {
        let mut out = Vec::new();
        if self.matches("check for updates update") {
            out.push((HIT_UPDATE, "Check for updates".to_string()));
        }
        if self.matches("open the config file edit settings") {
            out.push((HIT_EDIT, "Open the config file".to_string()));
        }
        if self.matches("save my settings export backup archive") {
            out.push((HIT_SAVE, "Save my settings".to_string()));
        }
        if self.matches("load settings import restore archive") {
            out.push((HIT_LOAD, "Load settings".to_string()));
        }
        if self.undo_load && self.matches("undo load restore settings") {
            out.push((HIT_UNDO_LOAD, "Undo load".to_string()));
        }
        if let Some(entry) = &self.info.undo {
            let old_word = undo_old_word_for(&entry.key, &entry.old);
            let hay = format!("undo {} {} {old_word}", entry.key, entry.old);
            if self.matches(&hay) {
                let label = undo_button_label(cr, th, max_w, entry);
                out.push((HIT_UNDO, label));
            }
        }
        out
    }

    fn place_buttons(
        cr: &Context,
        th: &Theme,
        left: f64,
        right: f64,
        start_y: f64,
        items: Vec<(u32, String)>,
    ) -> (Vec<ActButton>, f64) {
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
        Self::place_buttons(cr, th, left, right, start_y, items)
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

        y = self.measure_and_draw_load(cr, cx, body, left, right, inner_w, y, &mut hits);

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

    #[allow(clippy::too_many_arguments)]
    fn measure_and_draw_load(
        &self,
        cr: &Context,
        cx: &Cx,
        body: Rect,
        left: f64,
        right: f64,
        inner_w: f64,
        mut y: f64,
        hits: &mut Option<&mut Hits>,
    ) -> f64 {
        let th = cx.theme;
        if self.showing_loading() {
            if let Some(name) = &self.loading_name {
                if y > body.1 {
                    y += ACTS_TOP_GAP;
                }
                if hits.is_some() {
                    let msg = format!("Checking {name}…");
                    draw::text_max(
                        cr,
                        left,
                        y + 5.0,
                        inner_w,
                        &Text {
                            size: 12.0,
                            bold: false,
                            color: th.muted,
                            text: &msg,
                        },
                    );
                }
                y += INFO_ROW_H;
            }
        } else if self.showing_preview() {
            if let Some(prev) = self.active_preview() {
                if y > body.1 {
                    y += RESTART_TOP_GAP;
                }
                let fname = Path::new(&prev.path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&prev.path);
                if hits.is_some() {
                    draw::text_max(
                        cr,
                        left,
                        y,
                        inner_w,
                        &Text {
                            size: 11.0,
                            bold: false,
                            color: th.muted,
                            text: fname,
                        },
                    );
                }
                y += FILE_SUB_H;
                match &prev.result {
                    Ok(keys) => {
                        let title = format!("{} would change", key_count_word(keys.len()));
                        if hits.is_some() {
                            draw::text_max(
                                cr,
                                left,
                                y,
                                inner_w,
                                &Text {
                                    size: 13.0,
                                    bold: false,
                                    color: th.text,
                                    text: &title,
                                },
                            );
                        }
                        y += RESTART_TITLE_H;
                        if let Some(sum) = preview_summary(keys) {
                            let layout = Self::wrap_subtext_layout(cr, inner_w, &sum);
                            let sub_h = layout.pixel_size().1 as f64;
                            if hits.is_some() {
                                draw::set(cr, th.muted);
                                cr.move_to(left.round(), y.round());
                                pangocairo::functions::show_layout(cr, &layout);
                            }
                            y += sub_h + 2.0;
                        }
                        y += BTN_GAP_Y;
                        let (btns, next_y) = Self::place_buttons(
                            cr,
                            th,
                            left,
                            right,
                            y,
                            vec![
                                (HIT_LOAD_APPLY, "Apply".to_string()),
                                (HIT_LOAD_CANCEL, "Cancel".to_string()),
                            ],
                        );
                        if let Some(h) = hits.as_deref_mut() {
                            for b in &btns {
                                ui::button(cr, cx, h, b.id, b.x, b.y, &b.label);
                            }
                        }
                        y = next_y;
                    }
                    Err(err) => {
                        if hits.is_some() {
                            draw::text(
                                cr,
                                left,
                                y,
                                &Text {
                                    size: 13.0,
                                    bold: false,
                                    color: th.alert,
                                    text: "Cannot load settings",
                                },
                            );
                        }
                        y += RESTART_TITLE_H;
                        let layout = Self::wrap_subtext_layout(cr, inner_w, err);
                        let sub_h = layout.pixel_size().1 as f64;
                        if hits.is_some() {
                            draw::set(cr, th.muted);
                            cr.move_to(left.round(), y.round());
                            pangocairo::functions::show_layout(cr, &layout);
                        }
                        y += sub_h + 2.0;
                        y += BTN_GAP_Y;
                        let (btns, next_y) = Self::place_buttons(
                            cr,
                            th,
                            left,
                            right,
                            y,
                            vec![(HIT_LOAD_CANCEL, "Close".to_string())],
                        );
                        if let Some(h) = hits.as_deref_mut() {
                            for b in &btns {
                                ui::button(cr, cx, h, b.id, b.x, b.y, &b.label);
                            }
                        }
                        y = next_y;
                    }
                }
            }
        } else if self.showing_load_list() {
            if y > body.1 {
                y += ACTS_TOP_GAP;
            }
            if self.info.saved_files.is_empty() {
                if hits.is_some() {
                    draw::text_max(
                        cr,
                        left,
                        y + 5.0,
                        inner_w,
                        &Text {
                            size: 12.0,
                            bold: false,
                            color: th.muted,
                            text: "No saved settings",
                        },
                    );
                }
                y += INFO_ROW_H;
            } else {
                let total = self.info.saved_files.len();
                let max_scroll = total.saturating_sub(MAX_LOAD_ROWS);
                let start = self.load_scroll.min(max_scroll);
                let end = (start + MAX_LOAD_ROWS).min(total);
                for i in start..end {
                    let f = &self.info.saved_files[i];
                    if let Some(h) = hits.as_deref_mut() {
                        ui::row(
                            cr,
                            cx,
                            h,
                            HIT_LOAD_ITEM_BASE + i as u32,
                            (body.0, y, body.2, ui::ROW_H),
                            &Row {
                                icon: None,
                                name: &f.name,
                                state: "",
                                on: false,
                                right: 0.0,
                            },
                        );
                    }
                    y += ui::ROW_H;
                }
            }
        }
        y
    }

    fn has_visible_content(&self, cx: &Cx) -> bool {
        if !self.info_rows(cx.size).is_empty()
            || self.restart_subtext().is_some()
            || self.showing_load_list()
            || self.showing_preview()
            || self.showing_loading()
        {
            return true;
        }
        let undo_match = self.info.undo.as_ref().is_some_and(|e| {
            let old_word = undo_old_word_for(&e.key, &e.old);
            self.matches(&format!("undo {} {} {old_word}", e.key, e.old))
        });
        let undo_load_match = self.undo_load && self.matches("undo load restore settings");
        self.matches("check for updates update")
            || self.matches("open the config file edit settings")
            || self.matches("save my settings export backup archive")
            || self.matches("load settings import restore archive")
            || undo_load_match
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
        let preview_changed = self.info.preview != info.preview;
        if preview_changed
            && info.preview.as_ref().map(|p| &p.id) != self.dismissed_preview.as_ref()
        {
            self.loading_name = None;
        }
        let load_active_changed = !self.loaded || self.info.load_active != info.load_active;
        if load_active_changed {
            self.undo_load = info.load_active;
        }
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
            Event::Scroll { dy, .. } => {
                if !self.showing_load_list() {
                    return false;
                }
                let max_scroll = self.info.saved_files.len().saturating_sub(MAX_LOAD_ROWS);
                if max_scroll == 0 {
                    return false;
                }
                let cur = self.load_scroll.min(max_scroll);
                let next = if *dy > 0.0 {
                    (cur + 1).min(max_scroll)
                } else if *dy < 0.0 {
                    cur.saturating_sub(1)
                } else {
                    return false;
                };
                if next == self.load_scroll {
                    return false;
                }
                self.load_scroll = next;
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
                HIT_SAVE => {
                    out.push(Action::Run(SAVE_COMMAND.to_string()));
                    false
                }
                HIT_LOAD => {
                    self.load_open = !self.load_open;
                    self.load_scroll = 0;
                    true
                }
                HIT_UNDO_LOAD => {
                    self.undo_load = false;
                    out.push(Action::Run(undo_load_command()));
                    true
                }
                HIT_LOAD_APPLY => {
                    let Some(prev) = self.active_preview() else {
                        return false;
                    };
                    if prev.result.is_err() {
                        return false;
                    }
                    let path = prev.path.clone();
                    self.dismissed_preview = Some(prev.id.clone());
                    self.load_open = false;
                    self.loading_name = None;
                    self.undo_load = true;
                    out.push(Action::Run(apply_load_command(&path)));
                    true
                }
                HIT_LOAD_CANCEL => {
                    self.dismissed_preview = self.info.preview.as_ref().map(|p| p.id.clone());
                    self.loading_name = None;
                    out.push(Action::Run(clear_preview_command()));
                    true
                }
                h if h >= HIT_LOAD_ITEM_BASE => {
                    let idx = (h - HIT_LOAD_ITEM_BASE) as usize;
                    let Some(item) = self.info.saved_files.get(idx) else {
                        return false;
                    };
                    let path = item.path.clone();
                    let name = item.name.clone();
                    self.load_open = false;
                    self.dismissed_preview = self.info.preview.as_ref().map(|p| p.id.clone());
                    self.loading_name = Some(name);
                    out.push(Action::Run(preview_command(&path)));
                    true
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

    fn show_list(&mut self) {
        if std::env::var_os("INFYNITAE_EDGE_RENDER_LOAD").is_some_and(|v| v == "1" || v == "true") {
            self.load_open = true;
        }
    }

    fn panel_closed(&mut self) {
        self.load_open = false;
        self.load_scroll = 0;
        self.loading_name = None;
        self.undo_load = false;
        self.dismissed_preview = self.info.preview.as_ref().map(|p| p.id.clone());
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
            ..PcInfo::default()
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
            export_dir: dir.join("missing-exports"),
            preview_file: dir.join("missing-preview"),
            load_active_file: dir.join("missing-active"),
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
            export_dir: dir.join("missing-exports"),
            preview_file: dir.join("missing-preview"),
            load_active_file: dir.join("missing-active"),
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
        assert!(!is_restart_key("SCALING_FILTER"));
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

    fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    #[test]
    fn save_and_load_buttons_list_newest_first_and_open_preview() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TestDir::new("save-load-list");
        let dir = tmp.path();
        let exports = dir.join("Documents/Infynitae");
        std::fs::create_dir_all(&exports).unwrap();
        let old = exports.join("settings-2026-11-10-0800.tar.gz");
        let mid = exports.join("settings-2026-11-12-0900.tar.gz");
        let newest = exports.join("settings 'newest'.tar.gz");
        let hidden = exports.join(".tmp-export");
        for (p, sec) in [(&old, 100), (&mid, 200), (&newest, 300), (&hidden, 400)] {
            let f = std::fs::File::create(p).unwrap();
            f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(sec))
                .unwrap();
        }
        std::fs::create_dir_all(exports.join("subdir")).unwrap();

        let mut src = PcSource::with_paths(PcPaths {
            hostname: dir.join("h"),
            os_release: dir.join("o"),
            kernel_osrelease: dir.join("k"),
            cpuinfo: dir.join("c"),
            meminfo: dir.join("m"),
            settings_changes: dir.join("sc"),
            wayland_socket: None,
            infynitae_repo: None,
            export_dir: exports,
            preview_file: dir.join("rt/infynitae/settings-load-preview"),
            load_active_file: dir.join("rt/infynitae/settings-load-active"),
            run_external_cmds: false,
        });
        let data = src.fetch();
        let info = data.downcast_ref::<PcInfo>().unwrap();
        assert_eq!(
            info.saved_files
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "settings 'newest'.tar.gz",
                "settings-2026-11-12-0900.tar.gz",
                "settings-2026-11-10-0800.tar.gz",
            ]
        );

        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th);
        let mut card = AboutCard::default();
        card.apply("pc", &*data);

        // Save my settings runs settings.sh export.
        let mut out = Vec::new();
        card.event(&press(HIT_SAVE), &cx, &mut out);
        assert_eq!(out, vec![Action::Run(SAVE_COMMAND.into())]);

        // Load settings opens list; picking item 0 runs quoted import-preview command.
        out.clear();
        assert!(card.event(&press(HIT_LOAD), &cx, &mut out));
        assert!(card.load_open);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 500, 600).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::default();
        let h_open = card.height(&cx, 404.0);
        let drawn = card.measure_and_draw(&cr, &cx, (0.0, 0.0, 404.0, h_open), Some(&mut hits));
        assert!((h_open - drawn).abs() < 0.01);
        assert!(hits.list.iter().any(|(id, _)| *id == HIT_SAVE));
        assert!(hits.list.iter().any(|(id, _)| *id == HIT_LOAD));
        assert!(hits.list.iter().any(|(id, _)| *id == HIT_LOAD_ITEM_BASE));
        assert!(hits
            .list
            .iter()
            .any(|(id, _)| *id == HIT_LOAD_ITEM_BASE + 2));

        assert!(card.event(&press(HIT_LOAD_ITEM_BASE), &cx, &mut out));
        assert!(!card.load_open);
        assert_eq!(out.len(), 1);
        let cmd = match &out[0] {
            Action::Run(s) => s,
            other => panic!("expected Run action, got {other:?}"),
        };
        assert!(cmd.contains("import-preview"));
        assert!(cmd.contains(&settings::sq(&newest.to_string_lossy())));

        // Run generated preview command against a stub settings.sh and verify quoting + file output.
        let sh_dir = dir.join("infynitae/live");
        std::fs::create_dir_all(&sh_dir).unwrap();
        let stub = sh_dir.join("settings.sh");
        std::fs::write(
            &stub,
            "#!/bin/sh\nif [ \"$1\" = \"import-preview\" ]; then\nprintf 'ACCENT\\nBORDER_WIDTH\\n'\nelse exit 1; fi\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub, perms).unwrap();

        let status = Command::new("sh")
            .args(["-c", cmd])
            .env("HOME", dir)
            .env("XDG_RUNTIME_DIR", dir.join("rt"))
            .status()
            .unwrap();
        assert!(status.success());
        let data2 = src.fetch();
        let info2 = data2.downcast_ref::<PcInfo>().unwrap();
        let prev = info2.preview.as_ref().unwrap();
        assert_eq!(prev.path, newest.to_string_lossy());
        assert_eq!(
            prev.result,
            Ok(vec!["ACCENT".to_string(), "BORDER_WIDTH".to_string()])
        );
    }

    #[test]
    fn preview_formats_counts_error_cancel_apply_and_panel_closed_undo_load() {
        let many: Vec<String> = (1..=11).map(|i| format!("KEY_{i}")).collect();
        assert_eq!(preview_summary(&[]), None);
        assert_eq!(
            preview_summary(&many[..8]),
            Some("KEY_1, KEY_2, KEY_3, KEY_4, KEY_5, KEY_6, KEY_7, KEY_8".into())
        );
        assert_eq!(
            preview_summary(&many),
            Some("KEY_1, KEY_2, KEY_3, KEY_4, KEY_5, KEY_6, KEY_7, KEY_8, and 3 more".into())
        );

        let parsed_err = parse_import_preview(
            "id=p1\npath=/tmp/bad.tar.gz\nok=0\nNot a saved settings archive.\n",
        )
        .unwrap();
        assert_eq!(
            parsed_err.result,
            Err("Not a saved settings archive.".to_string())
        );

        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th);
        let mut card = AboutCard::default();
        let mut info = sample_info();
        info.preview = Some(ImportPreview {
            id: "prev-1".into(),
            path: "/tmp/settings-a.tar.gz".into(),
            result: Ok(many),
        });
        card.apply("pc", &info);

        // Cancel dismisses preview without resurrecting on identical info.
        let mut out = Vec::new();
        assert!(card.event(&press(HIT_LOAD_CANCEL), &cx, &mut out));
        assert!(card.active_preview().is_none());
        card.apply("pc", &info);
        assert!(card.active_preview().is_none());

        // New preview id opens preview; Apply starts import and shows Undo load.
        info.preview.as_mut().unwrap().id = "prev-2".into();
        card.apply("pc", &info);
        assert!(card.active_preview().is_some());
        out.clear();
        assert!(card.event(&press(HIT_LOAD_APPLY), &cx, &mut out));
        assert!(card.active_preview().is_none());
        assert!(card.undo_load);
        assert!(matches!(&out[0], Action::Run(s) if s.contains("import '/tmp/settings-a.tar.gz'")));

        // Undo load stays until Undo load or panel_closed.
        let buttons = {
            let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 500, 100).unwrap();
            let cr = Context::new(&surf).unwrap();
            card.visible_buttons(&cr, &th, 400.0)
        };
        assert!(buttons
            .iter()
            .any(|(id, l)| *id == HIT_UNDO_LOAD && l == "Undo load"));

        card.panel_closed();
        assert!(!card.undo_load);

        // Error preview never runs Apply.
        info.preview = Some(parsed_err);
        card.apply("pc", &info);
        out.clear();
        assert!(!card.event(&press(HIT_LOAD_APPLY), &cx, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn load_and_face_undo_commands_execute_in_temp_home() {
        let tmp = TestDir::new("exec-cmds");
        let h = tmp.path();
        let sh_dir = h.join("infynitae/live");
        let cfg_dir = h.join(".config/infynitae");
        let rt_dir = h.join("rt");
        let state_dir = h.join(".local/state/infynitae");
        std::fs::create_dir_all(&sh_dir).unwrap();
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::create_dir_all(&rt_dir).unwrap();
        std::fs::create_dir_all(&state_dir).unwrap();

        std::fs::write(cfg_dir.join("config"), "ROUNDING=8\n").unwrap();
        let calls_log = h.join("calls.log");
        let arch = h.join("Documents/settings 'saved'.tar.gz");
        std::fs::create_dir_all(arch.parent().unwrap()).unwrap();
        std::fs::write(&arch, b"stub").unwrap();
        std::fs::write(
            sh_dir.join("settings.sh"),
            format!(
                "#!/bin/sh\nprintf '%s|%s\\n' \"$1\" \"$2\" >> '{}'\nif [ \"$1\" = \"import\" ]; then\ncp \"$HOME/.config/infynitae/config\" \"$HOME/.config/infynitae/config.before-load\"\nprintf 'ROUNDING=0\\n' > \"$HOME/.config/infynitae/config\"\nfi\n",
                calls_log.display()
            ),
        )
        .unwrap();

        let run_sh_cmd = |cmd: &str| {
            Command::new("sh")
                .args(["-c", cmd])
                .env("HOME", h)
                .env("XDG_RUNTIME_DIR", &rt_dir)
                .env_remove("CONFIG_FILE")
                .status()
                .unwrap()
        };

        // Apply sets settings-load-active, Undo load restores config.before-load and calls apply.
        assert!(run_sh_cmd(&apply_load_command(&arch.to_string_lossy())).success());
        assert!(rt_dir.join("infynitae/settings-load-active").is_file());
        assert_eq!(
            std::fs::read_to_string(cfg_dir.join("config")).unwrap(),
            "ROUNDING=0\n"
        );
        assert!(run_sh_cmd(&undo_load_command()).success());
        assert!(!rt_dir.join("infynitae/settings-load-active").exists());
        assert_eq!(
            std::fs::read_to_string(cfg_dir.join("config")).unwrap(),
            "ROUNDING=8\n"
        );

        // Undo FACE restores .face.bak and unsets when old was "-".
        std::fs::write(h.join(".face"), b"new-face").unwrap();
        std::fs::write(h.join(".face.bak"), b"old-face").unwrap();
        std::fs::write(
            state_dir.join("settings-changes"),
            "100\tFACE\t-\n200\tFACE\t.face.bak\n",
        )
        .unwrap();
        assert!(run_sh_cmd(&undo_command(&UndoEntry {
            key: "FACE".into(),
            old: ".face.bak".into(),
        }))
        .success());
        assert_eq!(std::fs::read(h.join(".face")).unwrap(), b"old-face");
        assert!(!h.join(".face.bak").exists());

        assert!(run_sh_cmd(&undo_command(&UndoEntry {
            key: "FACE".into(),
            old: "-".into(),
        }))
        .success());
        assert!(!h.join(".face").exists());
        assert_eq!(
            std::fs::read_to_string(state_dir.join("settings-changes")).unwrap(),
            ""
        );

        let log = std::fs::read_to_string(&calls_log).unwrap();
        assert_eq!(log, format!("import|{}\napply|\n", arch.display()));
    }

    #[test]
    fn height_matches_what_is_drawn_for_load_and_preview_states() {
        let th = Theme::from_config(&Config::from_text(""));
        let mut cx = test_cx(&th);
        let many: Vec<String> = (1..=10).map(|i| format!("KEY_{i}")).collect();

        for size in [Size::M, Size::S] {
            cx.size = size;
            for width in [420.0, 260.0] {
                // 1) Open empty list
                let mut card = AboutCard::default();
                let mut info = sample_info();
                card.apply("pc", &info);
                card.load_open = true;

                let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 500, 900).unwrap();
                let cr = Context::new(&surf).unwrap();
                for stage in 0..5 {
                    match stage {
                        0 => {} // open empty list
                        1 => {
                            info.saved_files = (1..=8)
                                .map(|i| SavedFile {
                                    name: format!("settings-{i}.tar.gz"),
                                    path: format!("/tmp/settings-{i}.tar.gz"),
                                })
                                .collect();
                            card.apply("pc", &info);
                        }
                        2 => {
                            card.load_open = false;
                            card.loading_name = Some("settings-1.tar.gz".into());
                        }
                        3 => {
                            card.loading_name = None;
                            info.preview = Some(ImportPreview {
                                id: "ok10".into(),
                                path: "/tmp/settings-1.tar.gz".into(),
                                result: Ok(many.clone()),
                            });
                            card.apply("pc", &info);
                        }
                        _ => {
                            card.undo_load = true;
                            info.preview = Some(ImportPreview {
                                id: "err1".into(),
                                path: "/tmp/bad.tar.gz".into(),
                                result: Err("That archive holds an unexpected file.".into()),
                            });
                            card.apply("pc", &info);
                        }
                    }
                    let mut hits = Hits::default();
                    let h = card.height(&cx, width);
                    let drawn =
                        card.measure_and_draw(&cr, &cx, (10.0, 20.0, width, h), Some(&mut hits));
                    assert!(
                        (h - drawn).abs() < 0.01,
                        "stage={stage} size={size:?} width={width}: h={h} drawn={drawn}"
                    );
                    assert!(hits
                        .list
                        .iter()
                        .all(|(_, r)| r.0 >= 10.0 && r.0 + r.2 <= 10.0 + width + 0.5));
                }
            }
        }
    }
}
