//! The `apps` module: loads `.desktop` entries, tracks recently launched apps, searches names,
//! generic names, keywords, commands and desktop IDs (porting `live/edge/app-search.h`), resolves
//! PNG/SVG icons from the icon theme with a first-letter accent tile fallback, and draws a
//! scrolling list of apps.

use super::{favorites, is_left_button, windows::WindowItem, Rect, World};
use crate::config::Item;
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_double, c_int, c_void, CString};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const TITLE_H: f64 = 28.0;
pub const ROW_H: f64 = 38.0;
pub const ICON_SZ: f64 = 28.0;
pub const MAX_VISIBLE_ROWS: usize = 8;
pub const MAX_RECENT_APPS: usize = 64;

const MATCH_NONE: i32 = 0;
const MATCH_FUZZY: i32 = 1;
const MATCH_OTHER: i32 = 2;
const MATCH_INSIDE: i32 = 3;
const MATCH_WORD_START: i32 = 4;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopApp {
    pub name: String,
    pub generic: String,
    pub keywords: String,
    pub command: String,
    pub icon: String,
    pub id: String,
    pub name_fold: String,
    pub other_fold: String,
    pub last_launched: i64,
}

impl DesktopApp {
    pub fn new(
        name: impl Into<String>,
        generic: impl Into<String>,
        keywords: impl Into<String>,
        command: impl Into<String>,
        icon: impl Into<String>,
        id: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let generic = generic.into();
        let keywords = keywords.into();
        let command = command.into();
        let icon = icon.into();
        let id = id.into();
        let name_fold = name.to_ascii_lowercase();
        let other_fold = format!("{generic};{keywords};{command};{id}").to_ascii_lowercase();
        Self {
            name,
            generic,
            keywords,
            command,
            icon,
            id,
            name_fold,
            other_fold,
            last_launched: 0,
        }
    }
}

/// Strips `%f`, `%u`, `%F`, `%U`, etc. field codes from a desktop file's `Exec=` line, turning
/// `%%` into `%` (matching `app_clean_exec` in `live/edge/app-search.h`).
pub fn clean_exec(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => out.push('%'),
                Some(_) => {
                    while out.ends_with(' ') {
                        out.pop();
                    }
                }
                None => break,
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// Parses a `.desktop` file's contents into a [`DesktopApp`], returning `(seen_id, Option<DesktopApp>)`.
/// Returns `None` when `NoDisplay=true`, `Hidden=true`, or `Name`/`Exec` is missing.
pub fn parse_desktop_entry(id: &str, text: &str) -> Option<DesktopApp> {
    let mut in_entry = false;
    let mut no_display = false;
    let mut hidden = false;
    let mut name = String::new();
    let mut generic = String::new();
    let mut keywords = String::new();
    let mut exec = String::new();
    let mut icon = String::new();

    for raw in text.lines() {
        let line = raw.trim_end_matches(['\r', '\n']);
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(rest) = line.strip_prefix("NoDisplay=") {
            if rest.trim() == "true" {
                no_display = true;
            }
        } else if let Some(rest) = line.strip_prefix("Hidden=") {
            if rest.trim() == "true" {
                hidden = true;
            }
        } else if let Some(rest) = line.strip_prefix("Name=") {
            if name.is_empty() {
                name = rest.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("GenericName=") {
            if generic.is_empty() {
                generic = rest.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("Keywords=") {
            if keywords.is_empty() {
                keywords = rest.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("Exec=") {
            if exec.is_empty() {
                exec = rest.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("Icon=") {
            if icon.is_empty() {
                icon = rest.trim().to_string();
            }
        }
    }

    if no_display || hidden || name.is_empty() || exec.is_empty() {
        return None;
    }
    let command = clean_exec(&exec);
    if command.is_empty() {
        return None;
    }
    Some(DesktopApp::new(name, generic, keywords, command, icon, id))
}

/// Loads `.desktop` files from `dirs` in order. A `.desktop` filename seen in an earlier directory
/// (even if `NoDisplay=true` or `Hidden=true`) hides any file with the same name in later
/// directories, and duplicate app names (case-insensitive) are skipped.
pub fn load_apps_from_dirs(dirs: &[PathBuf]) -> Vec<DesktopApp> {
    let mut apps: Vec<DesktopApp> = Vec::new();
    let mut seen_ids: Vec<String> = Vec::new();

    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let fname = entry.file_name();
            let Some(id) = fname.to_str() else {
                continue;
            };
            if id.len() <= 8 || !id.ends_with(".desktop") {
                continue;
            }
            if seen_ids.iter().any(|s| s == id) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            seen_ids.push(id.to_string());
            let Some(app) = parse_desktop_entry(id, &text) else {
                continue;
            };
            if apps.iter().any(|a| a.name.eq_ignore_ascii_case(&app.name)) {
                continue;
            }
            apps.push(app);
        }
    }
    apps
}

/// Standard XDG application directories (matching `app_load_all` in `live/edge/app-search.h`).
pub fn default_app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var("HOME").ok().filter(|s| !s.is_empty());
    let data_home = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty());

    if let Some(dh) = data_home {
        dirs.push(PathBuf::from(dh).join("applications"));
    } else if let Some(ref h) = home {
        dirs.push(PathBuf::from(h).join(".local/share/applications"));
    }
    if let Some(ref h) = home {
        dirs.push(PathBuf::from(h).join(".local/share/flatpak/exports/share/applications"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    for part in data_dirs.split(':').filter(|p| !p.is_empty()) {
        dirs.push(PathBuf::from(part).join("applications"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentEntry {
    pub id: String,
    pub when: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentApps {
    pub entries: Vec<RecentEntry>,
}

pub fn recent_apps_path() -> PathBuf {
    if let Some(sh) = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(sh).join("infynitae/recent-apps");
    }
    if let Some(h) = std::env::var("HOME").ok().filter(|s| !s.is_empty()) {
        return PathBuf::from(h).join(".local/state/infynitae/recent-apps");
    }
    PathBuf::new()
}

impl RecentApps {
    pub fn parse(text: &str) -> Self {
        let mut entries = Vec::new();
        for raw in text.lines() {
            if entries.len() >= MAX_RECENT_APPS {
                break;
            }
            let line = raw.trim_end_matches(['\r', '\n']);
            let Some((when_s, id)) = line.split_once('\t') else {
                continue;
            };
            let Ok(when) = when_s.trim().parse::<i64>() else {
                continue;
            };
            if when <= 0 || id.is_empty() {
                continue;
            }
            entries.push(RecentEntry {
                id: id.to_string(),
                when,
            });
        }
        Self { entries }
    }

    pub fn format(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&format!("{}\t{}\n", e.when, e.id));
        }
        out
    }

    pub fn load_from(path: &Path) -> Self {
        if path.as_os_str().is_empty() {
            return Self::default();
        }
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Self::parse(&text)
    }

    pub fn save_to(&self, path: &Path) {
        if path.as_os_str().is_empty() {
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension(format!("recent-apps.{}", std::process::id()));
        if std::fs::write(&tmp, self.format()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    pub fn note(&mut self, id: &str, now: i64) {
        if id.is_empty() || now <= 0 {
            return;
        }
        self.entries.retain(|e| e.id != id);
        self.entries.insert(
            0,
            RecentEntry {
                id: id.to_string(),
                when: now,
            },
        );
        if self.entries.len() > MAX_RECENT_APPS {
            self.entries.truncate(MAX_RECENT_APPS);
        }
    }

    pub fn apply(&self, apps: &mut [DesktopApp]) {
        for app in apps.iter_mut() {
            app.last_launched = 0;
            for e in &self.entries {
                if e.id == app.id {
                    app.last_launched = e.when;
                    break;
                }
            }
        }
    }
}

fn is_word_boundary(b: u8) -> bool {
    matches!(b, b' ' | b'-' | b'_' | b'.' | b':' | b'/' | b'(' | b';')
}

fn name_match(hay: &str, needle: &str) -> i32 {
    if needle.is_empty() {
        return MATCH_WORD_START;
    }
    let hay_bytes = hay.as_bytes();
    let mut start = 0;
    let mut best = MATCH_NONE;
    while start <= hay.len() {
        let Some(rel) = hay[start..].find(needle) else {
            break;
        };
        let pos = start + rel;
        if pos == 0 || is_word_boundary(hay_bytes[pos - 1]) {
            return MATCH_WORD_START;
        }
        best = MATCH_INSIDE;
        start = pos + 1;
    }
    best
}

fn subseq_match(hay: &str, needle: &str) -> bool {
    if needle.len() < 2 {
        return false;
    }
    let mut n_iter = needle.bytes().peekable();
    for b in hay.bytes() {
        if n_iter.peek() == Some(&b) {
            n_iter.next();
            if n_iter.peek().is_none() {
                return true;
            }
        }
    }
    n_iter.peek().is_none()
}

fn word_match(app: &DesktopApp, word: &str) -> i32 {
    let m = name_match(&app.name_fold, word);
    if m > MATCH_NONE {
        return m;
    }
    if app.other_fold.contains(word) {
        return MATCH_OTHER;
    }
    if subseq_match(&app.name_fold, word) {
        return MATCH_FUZZY;
    }
    MATCH_NONE
}

/// Searches and ranks `apps` against `query`, returning indices into `apps` in ranked order
/// (port of `app_search` in `live/edge/app-search.h`).
pub fn search_apps(apps: &[DesktopApp], query: &str) -> Vec<usize> {
    let q_fold = query.to_ascii_lowercase();
    let words: Vec<&str> = q_fold.split_ascii_whitespace().take(8).collect();

    let mut ranked: Vec<(i32, usize)> = Vec::with_capacity(apps.len());
    for (i, app) in apps.iter().enumerate() {
        let worst = if words.is_empty() {
            MATCH_WORD_START
        } else {
            let mut w_min = MATCH_WORD_START;
            for &w in &words {
                let m = word_match(app, w);
                if m < w_min {
                    w_min = m;
                }
                if w_min == MATCH_NONE {
                    break;
                }
            }
            if w_min == MATCH_NONE {
                continue;
            }
            w_min
        };

        // how good a match: the exact name, the name starting with the query, every word at a word
        // start, inside the name, in keywords only, letters in order. Recent use orders apps within
        // one of these, never above a better match (a recent app merely containing "a" was listed
        // before every app whose name starts with "a").
        let typed = q_fold.trim();
        let group = if words.is_empty() {
            if app.last_launched > 0 {
                0
            } else {
                1
            }
        } else if app.name_fold == typed {
            0
        } else if app.name_fold.starts_with(typed) {
            1
        } else if worst == MATCH_WORD_START {
            2
        } else if worst == MATCH_INSIDE {
            3
        } else if worst == MATCH_OTHER {
            4
        } else {
            5
        };

        ranked.push((group, i));
    }

    ranked.sort_by(|&(ga, ia), &(gb, ib)| {
        ga.cmp(&gb)
            .then_with(|| apps[ib].last_launched.cmp(&apps[ia].last_launched))
            .then_with(|| apps[ia].name_fold.cmp(&apps[ib].name_fold))
            .then_with(|| ia.cmp(&ib))
    });

    ranked.into_iter().map(|(_, idx)| idx).collect()
}

/// Parses fixed `apps` data (from `INFYNITAE_EDGE_RENDER_DATA` or tests).
/// Accepts either tab-separated lines (`name\tcommand\ticon\tid\tgeneric\tkeywords\tlast_launched`)
/// or `[Desktop Entry]` blocks.
pub fn parse_apps(out: &str) -> Vec<DesktopApp> {
    if out.contains("[Desktop Entry]") {
        let mut apps = Vec::new();
        for (idx, chunk) in out.split("[Desktop Entry]").skip(1).enumerate() {
            let block = format!("[Desktop Entry]\n{chunk}");
            let id = block
                .lines()
                .find_map(|l| {
                    l.strip_prefix("X-Desktop-Id=")
                        .or_else(|| l.strip_prefix("Id="))
                })
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| format!("app-{idx}.desktop"));
            if let Some(app) = parse_desktop_entry(&id, &block) {
                if !apps
                    .iter()
                    .any(|a: &DesktopApp| a.name.eq_ignore_ascii_case(&app.name))
                {
                    apps.push(app);
                }
            }
        }
        return apps;
    }

    let mut apps = Vec::new();
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        let name = parts.first().copied().unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        let cmd_raw = parts
            .get(1)
            .copied()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(name);
        let command = clean_exec(cmd_raw);
        let icon = parts.get(2).copied().unwrap_or("").trim();
        let default_id = format!(
            "{}.desktop",
            command
                .split_whitespace()
                .next()
                .unwrap_or(name)
                .to_ascii_lowercase()
        );
        let id = parts
            .get(3)
            .copied()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&default_id);
        let generic = parts.get(4).copied().unwrap_or("").trim();
        let keywords = parts.get(5).copied().unwrap_or("").trim();
        let last_launched = parts
            .get(6)
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(0);
        let mut app = DesktopApp::new(name, generic, keywords, command, icon, id);
        app.last_launched = last_launched;
        apps.push(app);
    }
    apps
}

/// Checks whether `app` matches any open window in `windows`.
pub fn is_app_running(app: &DesktopApp, windows: &[WindowItem]) -> bool {
    let app_stem = app.id.strip_suffix(".desktop").unwrap_or(&app.id);
    let app_last = app_stem.rsplit('.').next().unwrap_or(app_stem);
    let cmd_bin = app
        .command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");

    for w in windows {
        let w_stem = w.app_id.strip_suffix(".desktop").unwrap_or(&w.app_id);
        if w_stem.is_empty() {
            continue;
        }
        let w_last = w_stem.rsplit('.').next().unwrap_or(w_stem);
        if w_stem.eq_ignore_ascii_case(app_stem)
            || (!app_last.is_empty() && w_last.eq_ignore_ascii_case(app_last))
            || w_stem.eq_ignore_ascii_case(&app.name)
            || w_last.eq_ignore_ascii_case(&app.name)
            || (!cmd_bin.is_empty()
                && (w_stem.eq_ignore_ascii_case(cmd_bin) || w_last.eq_ignore_ascii_case(cmd_bin)))
        {
            return true;
        }
    }
    false
}

/// Resolves an icon name or path from a `.desktop` file to a PNG or SVG file on disk.
pub fn resolve_icon_path(icon: &str) -> Option<PathBuf> {
    let icon = icon.trim();
    if icon.is_empty() {
        return None;
    }
    let direct = Path::new(icon);
    if direct.is_absolute() {
        return direct.is_file().then(|| direct.to_path_buf());
    }

    let mut base_dirs: Vec<PathBuf> = Vec::new();
    if let Some(dh) = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
    {
        base_dirs.push(PathBuf::from(dh).join("icons"));
    } else if let Some(h) = std::env::var("HOME").ok().filter(|s| !s.is_empty()) {
        let hp = PathBuf::from(h);
        base_dirs.push(hp.join(".local/share/icons"));
        base_dirs.push(hp.join(".icons"));
        base_dirs.push(hp.join(".local/share/flatpak/exports/share/icons"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    for part in data_dirs.split(':').filter(|p| !p.is_empty()) {
        base_dirs.push(PathBuf::from(part).join("icons"));
    }
    base_dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));

    let themes = ["hicolor", "Adwaita", "Papirus", "breeze", "gnome"];
    let subdirs = [
        "scalable/apps",
        "48x48/apps",
        "64x64/apps",
        "32x32/apps",
        "128x128/apps",
        "256x256/apps",
        "24x24/apps",
        "apps/48",
        "apps/64",
        "apps/scalable",
        "symbolic/apps",
        "scalable/devices",
        "scalable/status",
        "scalable/places",
        "scalable/categories",
    ];
    let exts = ["svg", "png"];

    let stem = icon
        .strip_suffix(".png")
        .or_else(|| icon.strip_suffix(".svg"))
        .unwrap_or(icon);

    for base in &base_dirs {
        for theme in &themes {
            let theme_dir = base.join(theme);
            if !theme_dir.is_dir() {
                continue;
            }
            for sub in &subdirs {
                for ext in &exts {
                    let candidate = theme_dir.join(sub).join(format!("{stem}.{ext}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    let pixmaps = Path::new("/usr/share/pixmaps");
    for ext in &exts {
        let candidate = pixmaps.join(format!("{stem}.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[repr(C)]
struct RsvgRectangle {
    x: c_double,
    y: c_double,
    width: c_double,
    height: c_double,
}

struct RsvgFns {
    new_from_file: unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> *mut c_void,
    render_document: unsafe extern "C" fn(
        *mut c_void,
        *mut cairo::ffi::cairo_t,
        *const RsvgRectangle,
        *mut *mut c_void,
    ) -> c_int,
    g_object_unref: unsafe extern "C" fn(*mut c_void),
    g_error_free: Option<unsafe extern "C" fn(*mut c_void)>,
}

fn rsvg_fns() -> Option<&'static RsvgFns> {
    static RSVG: OnceLock<Option<RsvgFns>> = OnceLock::new();
    RSVG.get_or_init(|| unsafe {
        let rsvg_name = CString::new("librsvg-2.so.2").ok()?;
        let gobj_name = CString::new("libgobject-2.0.so.0").ok()?;
        let glib_name = CString::new("libglib-2.0.so.0").ok()?;
        let rsvg = libc::dlopen(rsvg_name.as_ptr(), libc::RTLD_LAZY);
        if rsvg.is_null() {
            return None;
        }
        let gobj = libc::dlopen(gobj_name.as_ptr(), libc::RTLD_LAZY);
        if gobj.is_null() {
            return None;
        }
        let glib = libc::dlopen(glib_name.as_ptr(), libc::RTLD_LAZY);

        let s_new = CString::new("rsvg_handle_new_from_file").ok()?;
        let s_render = CString::new("rsvg_handle_render_document").ok()?;
        let s_unref = CString::new("g_object_unref").ok()?;
        let s_err = CString::new("g_error_free").ok()?;

        let p_new = libc::dlsym(rsvg, s_new.as_ptr());
        let p_render = libc::dlsym(rsvg, s_render.as_ptr());
        let p_unref = libc::dlsym(gobj, s_unref.as_ptr());
        if p_new.is_null() || p_render.is_null() || p_unref.is_null() {
            return None;
        }
        let p_err = if !glib.is_null() {
            let p = libc::dlsym(glib, s_err.as_ptr());
            if p.is_null() {
                None
            } else {
                Some(std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(*mut c_void),
                >(p))
            }
        } else {
            None
        };

        Some(RsvgFns {
            new_from_file: std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> *mut c_void,
            >(p_new),
            render_document: std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(
                    *mut c_void,
                    *mut cairo::ffi::cairo_t,
                    *const RsvgRectangle,
                    *mut *mut c_void,
                ) -> c_int,
            >(p_render),
            g_object_unref: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(
                p_unref,
            ),
            g_error_free: p_err,
        })
    })
    .as_ref()
}

/// Loads a PNG or SVG icon file into a `size x size` `cairo::ImageSurface`.
/// gdk-pixbuf, loaded when first needed like librsvg (it is what librsvg reads images with, so a
/// system that shows SVG icons has it): JPEG covers and any other image it has a loader for.
struct PixbufFns {
    new_at_scale:
        unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, *mut *mut c_void) -> *mut c_void,
    pixels: unsafe extern "C" fn(*mut c_void) -> *mut u8,
    width: unsafe extern "C" fn(*mut c_void) -> c_int,
    height: unsafe extern "C" fn(*mut c_void) -> c_int,
    rowstride: unsafe extern "C" fn(*mut c_void) -> c_int,
    channels: unsafe extern "C" fn(*mut c_void) -> c_int,
    unref: unsafe extern "C" fn(*mut c_void),
    g_error_free: Option<unsafe extern "C" fn(*mut c_void)>,
}

fn pixbuf_fns() -> Option<&'static PixbufFns> {
    static PB: OnceLock<Option<PixbufFns>> = OnceLock::new();
    PB.get_or_init(|| unsafe {
        let lib = libc::dlopen(
            CString::new("libgdk_pixbuf-2.0.so.0").ok()?.as_ptr(),
            libc::RTLD_LAZY,
        );
        let gobj = libc::dlopen(
            CString::new("libgobject-2.0.so.0").ok()?.as_ptr(),
            libc::RTLD_LAZY,
        );
        if lib.is_null() || gobj.is_null() {
            return None;
        }
        let glib = libc::dlopen(
            CString::new("libglib-2.0.so.0").ok()?.as_ptr(),
            libc::RTLD_LAZY,
        );
        let sym = |h: *mut c_void, n: &str| -> Option<*mut c_void> {
            let c = CString::new(n).ok()?;
            let p = libc::dlsym(h, c.as_ptr());
            (!p.is_null()).then_some(p)
        };
        let err = if glib.is_null() {
            None
        } else {
            sym(glib, "g_error_free")
                .map(|p| std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(p))
        };
        Some(PixbufFns {
            new_at_scale: std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, *mut *mut c_void) -> *mut c_void,
            >(sym(lib, "gdk_pixbuf_new_from_file_at_scale")?),
            pixels: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> *mut u8>(
                sym(lib, "gdk_pixbuf_get_pixels")?,
            ),
            width: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_width")?,
            ),
            height: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_height")?,
            ),
            rowstride: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_rowstride")?,
            ),
            channels: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_n_channels")?,
            ),
            unref: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(
                sym(gobj, "g_object_unref")?,
            ),
            g_error_free: err,
        })
    })
    .as_ref()
}

/// An image file at `size` x `size` through gdk-pixbuf (a cover is filled, not letterboxed),
/// as a cairo surface (premultiplied ARGB).
fn load_pixbuf_surface(path: &Path, size: i32) -> Option<cairo::ImageSurface> {
    let fns = pixbuf_fns()?;
    let c_path = CString::new(path.to_str()?).ok()?;
    unsafe {
        let mut err: *mut c_void = std::ptr::null_mut();
        let pb = (fns.new_at_scale)(c_path.as_ptr(), size, size, 0, &mut err);
        if !err.is_null() {
            if let Some(free_fn) = fns.g_error_free {
                free_fn(err);
            }
        }
        if pb.is_null() {
            return None;
        }
        let (w, h) = ((fns.width)(pb), (fns.height)(pb));
        let (stride, ch) = ((fns.rowstride)(pb) as usize, (fns.channels)(pb) as usize);
        let px = (fns.pixels)(pb);
        let out = (|| {
            if w <= 0 || h <= 0 || px.is_null() || !(ch == 3 || ch == 4) {
                return None;
            }
            let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).ok()?;
            let dst_stride = surf.stride() as usize;
            {
                let mut data = surf.data().ok()?;
                for y in 0..h as usize {
                    let row = std::slice::from_raw_parts(px.add(y * stride), w as usize * ch);
                    for x in 0..w as usize {
                        let s = &row[x * ch..x * ch + ch];
                        let a = if ch == 4 { s[3] as u32 } else { 255 };
                        let pm = |c: u8| (c as u32 * a + 127) / 255;
                        let v = (a << 24) | (pm(s[0]) << 16) | (pm(s[1]) << 8) | pm(s[2]);
                        data[y * dst_stride + x * 4..y * dst_stride + x * 4 + 4]
                            .copy_from_slice(&v.to_ne_bytes());
                    }
                }
            }
            surf.mark_dirty();
            Some(surf)
        })();
        (fns.unref)(pb);
        out
    }
}

pub fn load_icon_surface(path: &Path, size: i32) -> Option<cairo::ImageSurface> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "png" {
        let mut f = File::open(path).ok()?;
        let src = cairo::ImageSurface::create_from_png(&mut f).ok()?;
        let sw = src.width();
        let sh = src.height();
        if sw <= 0 || sh <= 0 {
            return None;
        }
        if sw == size && sh == size {
            return Some(src);
        }
        let dst = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size).ok()?;
        let cr = cairo::Context::new(&dst).ok()?;
        cr.scale(size as f64 / sw as f64, size as f64 / sh as f64);
        let _ = cr.set_source_surface(&src, 0.0, 0.0);
        let _ = cr.paint();
        drop(cr);
        dst.flush();
        return Some(dst);
    }
    if ext == "svg" {
        let fns = rsvg_fns()?;
        let c_path = CString::new(path.to_str()?).ok()?;
        unsafe {
            let mut err: *mut c_void = std::ptr::null_mut();
            let handle = (fns.new_from_file)(c_path.as_ptr(), &mut err);
            if !err.is_null() {
                if let Some(free_fn) = fns.g_error_free {
                    free_fn(err);
                }
            }
            if handle.is_null() {
                return None;
            }
            let dst = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size).ok()?;
            let cr = cairo::Context::new(&dst).ok()?;
            let viewport = RsvgRectangle {
                x: 0.0,
                y: 0.0,
                width: size as f64,
                height: size as f64,
            };
            let mut render_err: *mut c_void = std::ptr::null_mut();
            let ok = (fns.render_document)(handle, cr.to_raw_none(), &viewport, &mut render_err);
            if !render_err.is_null() {
                if let Some(free_fn) = fns.g_error_free {
                    free_fn(render_err);
                }
            }
            (fns.g_object_unref)(handle);
            drop(cr);
            if ok == 0 {
                return None;
            }
            dst.flush();
            return Some(dst);
        }
    }
    // anything else (JPEG covers, an embedded cover written without an extension): gdk-pixbuf
    load_pixbuf_surface(path, size)
}

thread_local! {
    static ICON_CACHE: RefCell<HashMap<(String, i32), Option<cairo::ImageSurface>>> =
        RefCell::new(HashMap::new());
}

fn cached_icon_surface(icon: &str, size: i32) -> Option<cairo::ImageSurface> {
    if icon.is_empty() || size <= 0 {
        return None;
    }
    let key = (icon.to_string(), size);
    ICON_CACHE.with(|cache| {
        if let Some(found) = cache.borrow().get(&key) {
            return found.clone();
        }
        let loaded = resolve_icon_path(icon).and_then(|p| load_icon_surface(&p, size));
        cache.borrow_mut().insert(key, loaded.clone());
        loaded
    })
}

/// Draws an app's icon at `(x, y)` of `size x size` px, falling back to a first-letter tile in the
/// accent when no icon is found.
pub fn draw_app_icon(
    cr: &Context,
    th: &Theme,
    icon_name: &str,
    app_name: &str,
    x: f64,
    y: f64,
    size: f64,
) {
    if let Some(surface) = cached_icon_surface(icon_name, size.round() as i32) {
        let _ = cr.save();
        let _ = cr.set_source_surface(&surface, x.round(), y.round());
        let _ = cr.paint();
        let _ = cr.restore();
        return;
    }

    let r = (size * 0.24).round().max(5.0);
    draw::round_rect(cr, x.round(), y.round(), size.round(), size.round(), r);
    draw::set(cr, Rgba(th.accent.0, th.accent.1, th.accent.2, 0.20));
    let _ = cr.fill_preserve();
    draw::set(cr, Rgba(th.accent.0, th.accent.1, th.accent.2, 0.55));
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    let letter: String = app_name
        .chars()
        .find(|c| c.is_alphanumeric())
        .or_else(|| app_name.chars().next())
        .unwrap_or('?')
        .to_uppercase()
        .collect();
    let font_sz = (size * 0.45).round().max(11.0);
    let t = Text {
        size: font_sz,
        bold: true,
        color: th.accent,
        text: &letter,
    };
    let (_, th_px) = draw::measure(cr, &t);
    draw::text_center(cr, x + size / 2.0, y + (size - th_px) / 2.0, &t);
}

#[derive(Clone, Debug, Default)]
pub struct AppsData {
    pub apps: Vec<DesktopApp>,
    pub filtered: Vec<usize>,
    pub favorites: String,
    pub fav_indices: Vec<usize>,
    pub recent: RecentApps,
    pub recent_path: PathBuf,
    pub scroll: usize,
    pub loaded: bool,
}

impl AppsData {
    pub fn ensure_loaded(&mut self, query: &str) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() {
            self.apps = load_apps_from_dirs(&default_app_dirs());
            self.recent_path = recent_apps_path();
            self.recent = RecentApps::load_from(&self.recent_path);
            self.recent.apply(&mut self.apps);
        }
        self.update_filter(query);
        self.resolve_favorites();
    }

    pub fn update_filter(&mut self, query: &str) {
        self.filtered = search_apps(&self.apps, query);
        if self.scroll >= self.filtered.len() {
            self.scroll = 0;
        }
    }

    pub fn resolve_favorites(&mut self) {
        self.fav_indices = favorites::resolve_favorites(&self.apps, &self.favorites);
    }

    pub fn set_favorites(&mut self, favs: &str) {
        self.favorites = favs.to_string();
        self.resolve_favorites();
    }
}

pub fn apply(data: &mut AppsData, key: &str, out: &str, query: &str) -> bool {
    match key {
        "apps" => {
            data.apps = parse_apps(out);
            data.recent.apply(&mut data.apps);
            data.loaded = true;
            data.update_filter(query);
            data.resolve_favorites();
            true
        }
        "recent_apps" | "recent-apps" => {
            data.recent = RecentApps::parse(out);
            data.recent.apply(&mut data.apps);
            data.update_filter(query);
            true
        }
        "favorites" => {
            data.set_favorites(out.trim());
            true
        }
        _ => false,
    }
}

pub fn visible_rows_for_height(h: f64) -> usize {
    let inner = (h - 2.0 * PAD_Y - TITLE_H).max(ROW_H);
    (inner / ROW_H).floor().max(1.0) as usize
}

pub fn height(it: &Item, world: &World) -> f64 {
    if let Some((_, _, _, rh)) = it.rect {
        return rh as f64;
    }
    let count = world.apps.filtered.len();
    let rows = count.clamp(1, MAX_VISIBLE_ROWS);
    2.0 * PAD_Y + TITLE_H + rows as f64 * ROW_H
}

/// Launches the app at `app_idx` in `world.apps.apps`, records its launch timestamp in
/// `world.apps.recent`, re-sorts `world.apps.filtered`, runs its command, and requests closing the
/// panel.
pub fn launch_app(world: &mut World, app_idx: usize) -> bool {
    let Some(app) = world.apps.apps.get(app_idx) else {
        return false;
    };
    let id = app.id.clone();
    let cmd = app.command.clone();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(1);
    world.apps.recent.note(&id, now);
    if world.live && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() {
        world.apps.recent.save_to(&world.apps.recent_path);
    }
    world.apps.recent.apply(&mut world.apps.apps);
    world.apps.update_filter(&world.search);
    world.close_requested = true;
    world.run(cmd);
    true
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    let (_, h) = size;
    let (_, py) = pos;
    if py < PAD_Y + TITLE_H || world.apps.filtered.is_empty() {
        return false;
    }
    let vis = visible_rows_for_height(h).min(world.apps.filtered.len());
    let max_scroll = world.apps.filtered.len().saturating_sub(vis);
    let scroll = world.apps.scroll.min(max_scroll);
    let row = ((py - (PAD_Y + TITLE_H)) / ROW_H).floor() as isize;
    if row < 0 || (row as usize) >= vis {
        return false;
    }
    let idx = scroll + row as usize;
    let Some(&app_idx) = world.apps.filtered.get(idx) else {
        return false;
    };
    if button == 3 || button == 0x111 {
        return favorites::toggle_favorite(world, app_idx);
    }
    if is_left_button(button) {
        return launch_app(world, app_idx);
    }
    false
}

pub fn scroll(world: &mut World, size: (f64, f64), _pos: (f64, f64), dy: f64) -> bool {
    let count = world.apps.filtered.len();
    let vis = visible_rows_for_height(size.1);
    if count <= vis {
        return false;
    }
    let max_scroll = count - vis;
    let cur = world.apps.scroll.min(max_scroll);
    let next = if dy > 0.0 {
        (cur + 1).min(max_scroll)
    } else if dy < 0.0 {
        cur.saturating_sub(1)
    } else {
        cur
    };
    if next != world.apps.scroll {
        world.apps.scroll = next;
        true
    } else {
        false
    }
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    draw::card(cr, th, x, y, w, h);
    draw::title(cr, th, x + PAD_X, y + PAD_Y, "Apps");

    let count = world.apps.filtered.len();
    if count == 0 {
        draw::text_max(
            cr,
            x + PAD_X,
            y + PAD_Y + TITLE_H + 10.0,
            (w - 2.0 * PAD_X).max(0.0),
            &Text {
                size: 13.0,
                bold: false,
                color: th.muted,
                text: "No matching apps",
            },
        );
        return;
    }

    let vis = visible_rows_for_height(h).min(count);
    let max_scroll = count.saturating_sub(vis);
    let scroll = world.apps.scroll.min(max_scroll);
    let sel_app_row = world.selected_app_row();

    for k in 0..vis {
        let i = scroll + k;
        let app_idx = world.apps.filtered[i];
        let Some(app) = world.apps.apps.get(app_idx) else {
            continue;
        };
        let ry = y + PAD_Y + TITLE_H + k as f64 * ROW_H;
        let is_sel = sel_app_row == Some(i);

        if is_sel {
            draw::round_rect(
                cr,
                x + PAD_X - 8.0,
                ry + 1.0,
                (w - 2.0 * (PAD_X - 8.0)).max(0.0),
                ROW_H - 2.0,
                6.0,
            );
            draw::set(cr, Rgba(th.accent.0, th.accent.1, th.accent.2, 0.18));
            let _ = cr.fill();
        }

        let icon_y = ry + (ROW_H - ICON_SZ) / 2.0;
        draw_app_icon(cr, th, &app.icon, &app.name, x + PAD_X, icon_y, ICON_SZ);

        let running = is_app_running(app, &world.windows.windows);
        let right_w = if running {
            draw::text_right(
                cr,
                x + w - PAD_X,
                ry + (ROW_H - 14.0) / 2.0,
                &Text {
                    size: 11.5,
                    bold: false,
                    color: th.muted,
                    text: "running",
                },
            ) + 10.0
        } else {
            0.0
        };

        let tx = x + PAD_X + ICON_SZ + 10.0;
        let max_w = (w - 2.0 * PAD_X - ICON_SZ - 10.0 - right_w).max(20.0);
        draw::text_max(
            cr,
            tx,
            ry + (ROW_H - 16.0) / 2.0,
            max_w,
            &Text {
                size: 13.0,
                bold: is_sel,
                color: th.text,
                text: &app.name,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JPEG cover (8x8, one colour) loads through gdk-pixbuf at the size asked for; it drew the
    /// placeholder note before (PNG and SVG only). Skipped where gdk-pixbuf is not installed.
    #[test]
    fn a_jpeg_cover_loads_at_its_size() {
        if pixbuf_fns().is_none() {
            return;
        }
        const JPEG: &[u8] = &[
            255, 216, 255, 224, 0, 16, 74, 70, 73, 70, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 255, 219, 0,
            67, 0, 2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 2, 2, 2, 2, 2, 4, 3, 2, 2, 2, 2, 5, 4, 4, 3, 4, 6,
            5, 6, 6, 6, 5, 6, 6, 6, 7, 9, 8, 6, 7, 9, 7, 6, 6, 8, 11, 8, 9, 10, 10, 10, 10, 10, 6,
            8, 11, 12, 11, 10, 12, 9, 10, 10, 10, 255, 219, 0, 67, 1, 2, 2, 2, 2, 2, 2, 5, 3, 3, 5,
            10, 7, 6, 7, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
            10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
            10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 255, 192, 0, 17, 8, 0, 8, 0, 8, 3, 1, 34, 0, 2,
            17, 1, 3, 17, 1, 255, 196, 0, 31, 0, 0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 255, 196, 0, 181, 16, 0, 2, 1, 3, 3, 2, 4, 3, 5, 5,
            4, 4, 0, 0, 1, 125, 1, 2, 3, 0, 4, 17, 5, 18, 33, 49, 65, 6, 19, 81, 97, 7, 34, 113,
            20, 50, 129, 145, 161, 8, 35, 66, 177, 193, 21, 82, 209, 240, 36, 51, 98, 114, 130, 9,
            10, 22, 23, 24, 25, 26, 37, 38, 39, 40, 41, 42, 52, 53, 54, 55, 56, 57, 58, 67, 68, 69,
            70, 71, 72, 73, 74, 83, 84, 85, 86, 87, 88, 89, 90, 99, 100, 101, 102, 103, 104, 105,
            106, 115, 116, 117, 118, 119, 120, 121, 122, 131, 132, 133, 134, 135, 136, 137, 138,
            146, 147, 148, 149, 150, 151, 152, 153, 154, 162, 163, 164, 165, 166, 167, 168, 169,
            170, 178, 179, 180, 181, 182, 183, 184, 185, 186, 194, 195, 196, 197, 198, 199, 200,
            201, 202, 210, 211, 212, 213, 214, 215, 216, 217, 218, 225, 226, 227, 228, 229, 230,
            231, 232, 233, 234, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 255, 196, 0, 31,
            1, 0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
            11, 255, 196, 0, 181, 17, 0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 119, 0, 1, 2, 3,
            17, 4, 5, 33, 49, 6, 18, 65, 81, 7, 97, 113, 19, 34, 50, 129, 8, 20, 66, 145, 161, 177,
            193, 9, 35, 51, 82, 240, 21, 98, 114, 209, 10, 22, 36, 52, 225, 37, 241, 23, 24, 25,
            26, 38, 39, 40, 41, 42, 53, 54, 55, 56, 57, 58, 67, 68, 69, 70, 71, 72, 73, 74, 83, 84,
            85, 86, 87, 88, 89, 90, 99, 100, 101, 102, 103, 104, 105, 106, 115, 116, 117, 118, 119,
            120, 121, 122, 130, 131, 132, 133, 134, 135, 136, 137, 138, 146, 147, 148, 149, 150,
            151, 152, 153, 154, 162, 163, 164, 165, 166, 167, 168, 169, 170, 178, 179, 180, 181,
            182, 183, 184, 185, 186, 194, 195, 196, 197, 198, 199, 200, 201, 202, 210, 211, 212,
            213, 214, 215, 216, 217, 218, 226, 227, 228, 229, 230, 231, 232, 233, 234, 242, 243,
            244, 245, 246, 247, 248, 249, 250, 255, 218, 0, 12, 3, 1, 0, 2, 17, 3, 17, 0, 63, 0,
            243, 58, 40, 162, 189, 131, 253, 4, 63, 255, 217,
        ];
        let path = std::env::temp_dir().join(format!("edge-cover-{}.jpg", std::process::id()));
        std::fs::write(&path, JPEG).unwrap();
        let mut surf = load_icon_surface(&path, 64).expect("a JPEG loads");
        let _ = std::fs::remove_file(&path);
        assert_eq!((surf.width(), surf.height()), (64, 64));
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        let px = u32::from_ne_bytes(
            data[32 * stride + 32 * 4..32 * stride + 32 * 4 + 4]
                .try_into()
                .unwrap(),
        );
        let (r, g, b) = ((px >> 16) & 255, (px >> 8) & 255, px & 255);
        assert!(
            r.abs_diff(200) < 12 && g.abs_diff(60) < 12 && b.abs_diff(120) < 12,
            "{r} {g} {b}"
        );
    }

    fn sample_apps() -> Vec<DesktopApp> {
        vec![
            DesktopApp::new(
                "Firefox",
                "Web Browser",
                "internet;www;browser;",
                "firefox",
                "firefox",
                "org.mozilla.firefox.desktop",
            ),
            DesktopApp::new(
                "Files",
                "File Manager",
                "folder;manager;explore;disk;",
                "nautilus --new-window",
                "org.gnome.Nautilus",
                "org.gnome.Nautilus.desktop",
            ),
            DesktopApp::new(
                "Foot",
                "Terminal Emulator",
                "shell;prompt;command;",
                "foot",
                "foot",
                "foot.desktop",
            ),
            DesktopApp::new(
                "GNU Image Manipulation Program",
                "Image Editor",
                "gimp;photo;",
                "gimp-2.10",
                "gimp",
                "gimp.desktop",
            ),
            DesktopApp::new(
                "System Monitor",
                "Process Viewer",
                "top;cpu;memory;",
                "gnome-system-monitor",
                "org.gnome.SystemMonitor",
                "org.gnome.SystemMonitor.desktop",
            ),
            DesktopApp::new(
                "Settings",
                "Settings",
                "preferences;configure;",
                "gnome-control-center",
                "org.gnome.Settings",
                "org.gnome.Settings.desktop",
            ),
            DesktopApp::new(
                "LibreOffice Writer",
                "Word Processor",
                "doc;text;",
                "libreoffice --writer",
                "libreoffice-writer",
                "libreoffice-writer.desktop",
            ),
            DesktopApp::new(
                "Firetools",
                "Sandbox Launcher",
                "firejail;security;",
                "firetools",
                "firetools",
                "firetools.desktop",
            ),
        ]
    }

    #[test]
    fn clean_exec_strips_field_codes_and_unescapes_percent() {
        assert_eq!(clean_exec("firefox %u"), "firefox");
        assert_eq!(
            clean_exec("  nautilus --new-window %U  "),
            "nautilus --new-window"
        );
        assert_eq!(clean_exec("cmd --literal %% %f %F"), "cmd --literal %");
    }

    #[test]
    fn desktop_entry_parses_and_user_nodisplay_overrides_system_entry() {
        let base = std::env::temp_dir().join(format!("edge-rs-apps-test-{}", std::process::id()));
        let user_dir = base.join("user");
        let sys_dir = base.join("sys");
        let _ = std::fs::create_dir_all(&user_dir);
        let _ = std::fs::create_dir_all(&sys_dir);

        std::fs::write(
            user_dir.join("hidden-app.desktop"),
            "[Desktop Entry]\nName=Hidden App\nExec=hidden-app\nNoDisplay=true\n",
        )
        .unwrap();
        std::fs::write(
            sys_dir.join("hidden-app.desktop"),
            "[Desktop Entry]\nName=Hidden App\nExec=hidden-app\n",
        )
        .unwrap();
        std::fs::write(
            sys_dir.join("foot.desktop"),
            "[Desktop Entry]\nName=Foot\nGenericName=Terminal\nKeywords=shell;prompt;\nExec=foot %F\nIcon=foot\n\n[Desktop Action new]\nName=New Window\nExec=foot\n",
        )
        .unwrap();

        let loaded = load_apps_from_dirs(&[user_dir, sys_dir]);
        let _ = std::fs::remove_dir_all(&base);

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Foot");
        assert_eq!(loaded[0].generic, "Terminal");
        assert_eq!(loaded[0].keywords, "shell;prompt;");
        assert_eq!(loaded[0].command, "foot");
        assert_eq!(loaded[0].icon, "foot");
        assert_eq!(loaded[0].id, "foot.desktop");
    }

    #[test]
    fn app_search_matches_name_generic_keywords_command_and_recency() {
        let mut apps = sample_apps();

        // Word-start beats substring; alphabetical tie-break
        let out = search_apps(&apps, "fire");
        assert_eq!(apps[out[0]].name, "Firefox");
        assert_eq!(apps[out[1]].name, "Firetools");

        // Exact name match beats another prefix match
        apps.push(DesktopApp::new(
            "Settings Daemon",
            "",
            "",
            "settingsd",
            "",
            "settingsd.desktop",
        ));
        let out = search_apps(&apps, "settings");
        assert_eq!(apps[out[0]].name, "Settings");
        assert_eq!(apps[out[1]].name, "Settings Daemon");
        apps.pop();

        // Word boundary inside multi-word name beats mid-word match
        let out = search_apps(&apps, "writer");
        assert_eq!(apps[out[0]].name, "LibreOffice Writer");

        // GenericName, Keywords, command, and desktop id matches
        let out = search_apps(&apps, "terminal");
        assert_eq!(apps[out[0]].name, "Foot");
        let out = search_apps(&apps, "gimp");
        assert_eq!(apps[out[0]].name, "GNU Image Manipulation Program");
        let out = search_apps(&apps, "nautilus");
        assert_eq!(apps[out[0]].name, "Files");

        // Multi-word query across fields
        let out = search_apps(&apps, "web moz");
        assert_eq!(out.len(), 1);
        assert_eq!(apps[out[0]].name, "Firefox");

        // Subsequence fuzzy match
        let out = search_apps(&apps, "ffx");
        assert_eq!(apps[out[0]].name, "Firefox");

        // Recency ordering on empty query and prefix query
        let mut recent = RecentApps::default();
        recent.note("foot.desktop", 100);
        recent.note("org.mozilla.firefox.desktop", 200);
        recent.apply(&mut apps);
        let out = search_apps(&apps, "");
        assert_eq!(apps[out[0]].name, "Firefox");
        assert_eq!(apps[out[1]].name, "Foot");

        recent.note("firetools.desktop", 300);
        recent.apply(&mut apps);
        let out = search_apps(&apps, "fire");
        assert_eq!(apps[out[0]].name, "Firetools");
        assert_eq!(apps[out[1]].name, "Firefox");
    }

    /// Owner 2026-11-11 ("search works kinda weird"): typing "a" listed recently used apps that only
    /// contain an "a" (Melodyae, Steam, Telegram) before every app whose name starts with "a".
    #[test]
    fn a_name_starting_with_the_query_beats_a_recent_app_that_contains_it() {
        let mk = |name: &str, id: &str| DesktopApp::new(name, "", "", id, "", id);
        let mut apps = vec![
            mk("Melodyae", "melodyae.desktop"),
            mk("Steam", "steam.desktop"),
            mk("AEAM", "aeam.desktop"),
            mk("About Xfce", "about.desktop"),
            mk("Mana Overlay", "mana.desktop"),
        ];
        let mut recent = RecentApps::default();
        recent.note("melodyae.desktop", 300);
        recent.note("steam.desktop", 200);
        recent.note("aeam.desktop", 100);
        recent.apply(&mut apps);
        let names: Vec<&str> = search_apps(&apps, "a")
            .iter()
            .map(|&i| apps[i].name.as_str())
            .collect();
        // starting with "a" first (the recently used one of them first), then the rest by recent use
        assert_eq!(
            names,
            ["AEAM", "About Xfce", "Melodyae", "Steam", "Mana Overlay"]
        );
        // a word starting with the query, inside a longer name, still beats a mere "contains"
        let names: Vec<&str> = search_apps(&apps, "o")
            .iter()
            .map(|&i| apps[i].name.as_str())
            .collect();
        assert_eq!(names[0], "Mana Overlay");
    }

    #[test]
    fn clicking_app_row_launches_command_and_right_click_toggles_favorite() {
        let mut world = World::now();
        world.apply(
            "apps",
            "Firefox\tfirefox %u\tfirefox\torg.mozilla.firefox.desktop\tWeb Browser\tweb;\n\
             Foot\tfoot\tfoot\tfoot.desktop\tTerminal\tshell;\n",
        );
        world.apps.set_favorites("foot");
        assert_eq!(world.apps.fav_indices, vec![1]);

        let size = (560.0, 200.0);
        // Left-click first row (Firefox)
        assert!(click(&mut world, size, (60.0, PAD_Y + TITLE_H + 10.0), 1));
        assert_eq!(world.actions.last().map(String::as_str), Some("firefox"));
        assert!(world.close_requested);
        assert!(world.apps.apps[0].last_launched > 0);

        // Right-click first row (now Firefox) toggles favorite
        world.close_requested = false;
        assert!(click(&mut world, size, (60.0, PAD_Y + TITLE_H + 10.0), 3));
        assert_eq!(world.apps.favorites, "foot org.mozilla.firefox");
        assert_eq!(world.apps.fav_indices, vec![1, 0]);
    }

    #[test]
    fn loads_png_and_svg_icons_into_surfaces() {
        let dir = std::env::temp_dir().join(format!("edge-rs-icons-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let png_path = dir.join("sample.png");
        let svg_path = dir.join("sample.svg");

        let src = cairo::ImageSurface::create(cairo::Format::ARgb32, 16, 16).unwrap();
        let cr = cairo::Context::new(&src).unwrap();
        cr.set_source_rgb(0.2, 0.6, 0.9);
        let _ = cr.paint();
        drop(cr);
        {
            let mut f = File::create(&png_path).unwrap();
            src.write_to_png(&mut f).unwrap();
        }
        std::fs::write(
            &svg_path,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\">\
             <circle cx=\"12\" cy=\"12\" r=\"10\" fill=\"#5ea1ff\"/></svg>",
        )
        .unwrap();

        let png_loaded = load_icon_surface(&png_path, 44).expect("PNG icon loads");
        assert_eq!(png_loaded.width(), 44);
        assert_eq!(png_loaded.height(), 44);

        if rsvg_fns().is_some() {
            let svg_loaded = load_icon_surface(&svg_path, 44).expect("SVG icon loads");
            assert_eq!(svg_loaded.width(), 44);
            assert_eq!(svg_loaded.height(), 44);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
