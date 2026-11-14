//! Settings: Look (`docs/EDGE_SETTINGS.md` section 4): `settings/appearance`, `settings/wallpaper`,
//! and `settings/fonts`. Left to the file: `BORDER_ACTIVE`, `SURFACE_COLOR`, `TEXT_COLOR`,
//! `BORDER_INACTIVE`, `CANVAS_COLOR`, `THEME_SYNC`, `CORNERS`, `SQUARE_CORNERS`,
//! `BORDER_IGNORE_CSD`, `CLIP_TO_GEOMETRY`, `WINE_TRANSPARENCY`, `FORCE_TRANSPARENT_APPS`,
//! `SWITCH_LOOK`.

use super::settings::sq;
#[cfg(test)]
use super::settings_rows::Control;
use super::settings_rows::{change_command, After, ConfigData};
use crate::card::{
    render_value, run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status,
};
use crate::config::parse_hex;
use crate::draw::{self, Text};
use crate::icons::{self, DecodedImage};
use crate::provider::{CardKind, Size};
use crate::theme::{self, Rgba};
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

pub const KIND_APPEARANCE: CardKind = CardKind {
    id: "appearance",
    title: "Appearance",
    about: "Colours, corners, dark or light look",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "look", "face"],
};

pub const KIND_WALLPAPER: CardKind = CardKind {
    id: "wallpaper",
    title: "Wallpaper",
    about: "Picture, fit, depth",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "wallpapers"],
};

pub const KIND_FONTS: CardKind = CardKind {
    id: "fonts",
    title: "Fonts, icons, cursor",
    about: "Interface and terminal fonts, icons, cursor",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "fonts", "fonts-mono", "icons", "cursors"],
};

const ROW: u32 = 100;
const THUMB: u32 = 1000;
const MORE: u32 = 9000;
const LIST: u32 = 10_000;
const PICKER: u32 = 20_000;
const H_TAB_WP: u32 = PICKER + 1;
const H_TAB_ANY: u32 = PICKER + 2;
const H_SHUFFLE: u32 = PICKER + 3;
const H_WP_MODE_BOX: u32 = PICKER + 4;
const H_HUE: u32 = PICKER + 5;
const H_SV: u32 = PICKER + 6;
const H_CODE_ASK: u32 = PICKER + 7;
const H_MARK_BASE: u32 = PICKER + 20;
const H_CAND_SQ_BASE: u32 = PICKER + 30;
const H_WP_MODE_OPT_BASE: u32 = PICKER + 40;
const H_RECENT_BASE: u32 = PICKER + 50;
const H_SCHEME_BASE: u32 = PICKER + 70;
const FACE_ROW: u32 = 900;
const FACE_THUMB: u32 = 30_000;
const FACE_ROW_THUMB: i32 = 24;
const FACE_GRID_THUMB: i32 = 96;
const FACE_MAX_THUMBS: usize = MAX_WALLPAPERS;
const SLIDE_H: f64 = 22.0;
const SWATCH: f64 = 16.0;
const MAX_LIST_ROWS: usize = 8;
const MAX_WALLPAPERS: usize = 24;
const MAX_ACCENT_CANDIDATES: usize = 6;
const MAX_RECENT_LOG: usize = 8;
const PICKER_PREVIEW_SIZE: (i32, i32) = (384, 240);
const MARK_RADIUS: f64 = 8.0;
const ACTIVE_MARK_RADIUS: f64 = 11.0;
const MARK_MIN_DIST: f64 = 18.0;
const CAND_SQ_SIZE: f64 = 28.0;
const CAND_SQ_GAP: f64 = 8.0;
const RECENT_SQ_SIZE: f64 = 26.0;
const RECENT_SQ_GAP: f64 = 6.0;
const HUE_STRIP_H: f64 = 12.0;
const SV_SQUARE_H: f64 = 120.0;
const THUMB_PAD_X: f64 = 8.0;
const THUMB_GAP: f64 = 8.0;
const THUMB_TOP_PAD: f64 = 4.0;
const THUMB_RADIUS: f64 = 4.0;
const THUMB_NAME_H: f64 = 24.0;
const THUMB_S: (i32, i32) = (192, 108);
const THUMB_M: (i32, i32) = (129, 73);

/// A candidate accent extracted from the wallpaper (`#rrggbb x y`, `x`/`y` in `0..=1`).
#[derive(Clone, Debug, PartialEq)]
pub struct AccentCandidate {
    pub hex: String,
    pub rgb: [u8; 3],
    pub x: f64,
    pub y: f64,
}

/// Extra state for `settings/appearance`: wallpaper tool presence, invalid `#rrggbb` attempts,
/// extracted accent candidates with cached 16:10 wallpaper preview, and recent logged accents.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LookData {
    pub wallpaper_bin: bool,
    pub bad_colors: HashSet<String>,
    pub accent_candidates: Vec<AccentCandidate>,
    pub accent_thumb: Option<Arc<DecodedImage>>,
    pub recent_accents: Vec<String>,
}

/// Discovered wallpaper files, tool presence, and decoded thumbnails for `settings/wallpaper`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WallpaperFile {
    pub name: String,
    pub path: String,
}

pub type ThumbList = Vec<Option<Arc<DecodedImage>>>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WallpapersData {
    pub has_bin: bool,
    pub files: Vec<WallpaperFile>,
    pub thumbs_s: ThumbList,
    pub thumbs_m: ThumbList,
}

/// Names discovered for `fonts`, `fonts-mono`, `icons`, or `cursors`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FoundNames(pub Vec<String>);

/// Reads `~/.local/bin/infynitae-wallpaper` and any refused `#rrggbb` markers under the look runtime
/// directory.
pub struct LookSource {
    pub home: PathBuf,
    pub bad_dir: PathBuf,
}

impl LookSource {
    pub fn system() -> LookSource {
        let home = env_home();
        let bad_dir = system_bad_colors_dir(&home);
        LookSource { home, bad_dir }
    }
}

impl Source for LookSource {
    fn fetch(&mut self) -> Data {
        if let Some(raw) = render_value("look") {
            let mut out = LookData::default();
            let mut acc_lines = String::new();
            let mut thumb_path: Option<PathBuf> = None;
            for line in raw.lines() {
                let l = line.trim();
                if let Some(v) = l.strip_prefix("wallpaper_bin=") {
                    out.wallpaper_bin = matches!(v, "1" | "true" | "yes" | "on");
                } else if let Some(k) = l.strip_prefix("bad=") {
                    if !k.is_empty() {
                        out.bad_colors.insert(k.to_string());
                    }
                } else if let Some(cand) = l.strip_prefix("accent=") {
                    acc_lines.push_str(cand);
                    acc_lines.push('\n');
                } else if let Some(p) = l.strip_prefix("wp_thumb=") {
                    let expanded = expand_home(p.trim(), &self.home.to_string_lossy());
                    thumb_path = Some(PathBuf::from(expanded));
                } else if let Some(r) = l.strip_prefix("recent=") {
                    if let Some(hex) = normalize_hex(r.trim()) {
                        if !out.recent_accents.contains(&hex)
                            && out.recent_accents.len() < MAX_RECENT_LOG
                        {
                            out.recent_accents.push(hex);
                        }
                    }
                } else if matches!(l, "1" | "true") {
                    out.wallpaper_bin = true;
                }
            }
            if !acc_lines.is_empty() {
                out.accent_candidates = parse_accent_lines(&acc_lines);
            }
            if let Some(p) = thumb_path {
                out.accent_thumb =
                    icons::cached_wallpaper_thumb(&p, PICKER_PREVIEW_SIZE.0, PICKER_PREVIEW_SIZE.1);
            }
            return Box::new(out);
        }
        let mut bad_colors = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(&self.bad_dir) {
            for e in entries.flatten() {
                if e.path().is_file() {
                    if let Some(name) = e.file_name().to_str() {
                        bad_colors.insert(name.to_string());
                    }
                }
            }
        }
        let wp_bin = self.home.join(".local/bin/infynitae-wallpaper");
        let wallpaper_bin = wp_bin.is_file();
        let active_wp = resolve_active_wallpaper(&self.home);
        let mut accent_candidates = Vec::new();
        let mut accent_thumb = None;
        if let Some(ref wp) = active_wp {
            if wallpaper_bin {
                accent_thumb =
                    icons::cached_wallpaper_thumb(wp, PICKER_PREVIEW_SIZE.0, PICKER_PREVIEW_SIZE.1);
                if let Ok(out) = Command::new(&wp_bin).arg("--accents").arg(wp).output() {
                    if out.status.success() {
                        accent_candidates =
                            parse_accent_lines(&String::from_utf8_lossy(&out.stdout));
                    }
                }
            }
        }
        let recent_accents =
            std::fs::read_to_string(self.home.join(".local/state/infynitae/settings-changes"))
                .map(|raw| parse_recent_accents(&raw))
                .unwrap_or_default();
        Box::new(LookData {
            wallpaper_bin,
            bad_colors,
            accent_candidates,
            accent_thumb,
            recent_accents,
        })
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

fn normalize_hex(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() != 7 || !t.starts_with('#') {
        return None;
    }
    let rgb = parse_hex(t)?;
    Some(rgb_to_hex(rgb))
}

fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// Parses `#rrggbb x y` lines from `infynitae-wallpaper --accents` / `settings.sh wallpaper-colors accents`.
/// Malformed lines (missing coordinates, non-hex colours, out-of-range coordinates) are ignored.
pub fn parse_accent_lines(raw: &str) -> Vec<AccentCandidate> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let mut parts = line.split_whitespace();
        let Some(hex_raw) = parts.next() else {
            continue;
        };
        let (Some(xs), Some(ys), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Some(hex) = normalize_hex(hex_raw) else {
            continue;
        };
        let Some(rgb) = parse_hex(&hex) else {
            continue;
        };
        let (Ok(x), Ok(y)) = (xs.parse::<f64>(), ys.parse::<f64>()) else {
            continue;
        };
        if !x.is_finite()
            || !y.is_finite()
            || !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
        {
            continue;
        }
        out.push(AccentCandidate { hex, rgb, x, y });
        if out.len() >= MAX_ACCENT_CANDIDATES {
            break;
        }
    }
    out
}

/// Reads the last 8 distinct valid `#rrggbb` values logged for `ACCENT` (or `BORDER_ACTIVE` from
/// before `ACCENT` existed) in `settings-changes`, most recent first. Malformed lines and `-` markers
/// are skipped.
pub fn parse_recent_accents(log_text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in log_text.lines().rev() {
        let mut cols = line.split('\t');
        let (Some(_ts), Some(key), Some(old), None) =
            (cols.next(), cols.next(), cols.next(), cols.next())
        else {
            continue;
        };
        if !matches!(key, "ACCENT" | "BORDER_ACTIVE") {
            continue;
        }
        let Some(hex) = normalize_hex(old) else {
            continue;
        };
        if !out.contains(&hex) {
            out.push(hex);
            if out.len() >= MAX_RECENT_LOG {
                break;
            }
        }
    }
    out
}

fn resolve_active_wallpaper(home: &Path) -> Option<PathBuf> {
    let cfg_path = home.join(".config/infynitae/config");
    let raw = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|t| super::settings_rows::parse_config(&t).remove("WALLPAPER"))
        .unwrap_or_default();
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        let expanded = expand_home(trimmed, &home.to_string_lossy());
        let p = PathBuf::from(expanded);
        return p.is_file().then_some(p);
    }
    let dirs = [
        home.join("Pictures/Wallpapers"),
        home.join(".local/share/wallpapers"),
    ];
    scan_wallpapers(&dirs)
        .into_iter()
        .next()
        .map(|f| PathBuf::from(f.path))
        .filter(|p| p.is_file())
}

/// Scans `~/Pictures/Wallpapers` and `~/.local/share/wallpapers` for images and checks whether
/// `infynitae-wallpaper` is available on the system.
pub struct WallpapersSource {
    pub home: PathBuf,
    pub path: String,
}

impl WallpapersSource {
    pub fn system() -> WallpapersSource {
        WallpapersSource {
            home: env_home(),
            path: std::env::var("PATH").unwrap_or_default(),
        }
    }
}

impl Source for WallpapersSource {
    fn fetch(&mut self) -> Data {
        if let Some(raw) = render_value("wallpapers") {
            let mut has_bin = true;
            let mut files = Vec::new();
            for line in raw.lines() {
                let l = line.trim();
                if l.is_empty() {
                    continue;
                }
                if let Some(v) = l.strip_prefix("has_bin=") {
                    has_bin = matches!(v, "1" | "true" | "yes" | "on");
                    continue;
                }
                let name = Path::new(l)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(l)
                    .to_string();
                files.push(WallpaperFile {
                    name,
                    path: l.to_string(),
                });
            }
            let (thumbs_s, thumbs_m) = decode_wallpaper_thumbs(&files);
            return Box::new(WallpapersData {
                has_bin,
                files,
                thumbs_s,
                thumbs_m,
            });
        }
        let has_bin = has_wallpaper_bin(&self.home, &self.path);
        let dirs = [
            self.home.join("Pictures/Wallpapers"),
            self.home.join(".local/share/wallpapers"),
        ];
        let files = scan_wallpapers(&dirs);
        let (thumbs_s, thumbs_m) = decode_wallpaper_thumbs(&files);
        Box::new(WallpapersData {
            has_bin,
            files,
            thumbs_s,
            thumbs_m,
        })
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

fn decode_wallpaper_thumbs(files: &[WallpaperFile]) -> (ThumbList, ThumbList) {
    let shown = &files[..files.len().min(MAX_WALLPAPERS)];
    let thumbs_s = shown
        .iter()
        .map(|f| icons::cached_wallpaper_thumb(Path::new(&f.path), THUMB_S.0, THUMB_S.1))
        .collect();
    let thumbs_m = shown
        .iter()
        .map(|f| icons::cached_wallpaper_thumb(Path::new(&f.path), THUMB_M.0, THUMB_M.1))
        .collect();
    (thumbs_s, thumbs_m)
}

/// Current user picture (`~/.face`) and candidate pictures from `~/Pictures` and
/// `~/Pictures/Wallpapers`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceData {
    pub current: Option<Arc<DecodedImage>>,
    pub files: Vec<WallpaperFile>,
    pub thumbs: ThumbList,
}

/// Scans `~/.face` and `~/Pictures` / `~/Pictures/Wallpapers` off the UI thread, reusing
/// `icons::ThumbCache` via `icons::cached_wallpaper_thumb`.
pub struct FaceSource {
    pub home: PathBuf,
}

impl FaceSource {
    pub fn system() -> FaceSource {
        FaceSource { home: env_home() }
    }
}

impl Source for FaceSource {
    fn fetch(&mut self) -> Data {
        if let Some(raw) = render_value("face") {
            let mut current_path: Option<PathBuf> = None;
            let mut files = Vec::new();
            let home_s = self.home.to_string_lossy().into_owned();
            for line in raw.lines() {
                let l = line.trim();
                if l.is_empty() {
                    continue;
                }
                if let Some(p) = l.strip_prefix("current=") {
                    if !p.is_empty() {
                        current_path = Some(PathBuf::from(expand_home(p, &home_s)));
                    }
                    continue;
                }
                let expanded = expand_home(l, &home_s);
                let name = Path::new(&expanded)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(l)
                    .to_string();
                files.push(WallpaperFile {
                    name,
                    path: expanded,
                });
            }
            let current = current_path
                .filter(|p| p.is_file())
                .and_then(|p| icons::cached_wallpaper_thumb(&p, FACE_ROW_THUMB, FACE_ROW_THUMB));
            let thumbs = decode_face_thumbs(&files);
            return Box::new(FaceData {
                current,
                files,
                thumbs,
            });
        }
        let face_path = self.home.join(".face");
        let current = face_path
            .is_file()
            .then(|| icons::cached_wallpaper_thumb(&face_path, FACE_ROW_THUMB, FACE_ROW_THUMB))
            .flatten();
        let dirs = [
            self.home.join("Pictures"),
            self.home.join("Pictures/Wallpapers"),
        ];
        let files = scan_wallpapers(&dirs);
        let thumbs = decode_face_thumbs(&files);
        Box::new(FaceData {
            current,
            files,
            thumbs,
        })
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

fn decode_face_thumbs(files: &[WallpaperFile]) -> ThumbList {
    files[..files.len().min(FACE_MAX_THUMBS)]
        .iter()
        .map(|f| {
            icons::cached_wallpaper_thumb(Path::new(&f.path), FACE_GRID_THUMB, FACE_GRID_THUMB)
        })
        .collect()
}

/// Shell command to crop `src` into `~/.face` (256x256 PNG), keep `~/.face.bak`, and log `FACE`.
pub fn face_command(src: &str) -> String {
    let q = sq(src);
    format!(
        "E=\"${{INFYNITAE_EDGE_APPS:-$HOME/.local/bin/infynitae-edge-apps}}\"; \
         [ -x \"$E\" ] || E=infynitae-edge-apps; \
         f=\"$HOME/.face\"; b=\"$HOME/.face.bak\"; \
         if [ -e \"$f\" ]; then old=.face.bak; else old=-; fi; \
         if \"$E\" --crop-face {q} \"$f.new\"; then \
         if [ -e \"$f\" ]; then cp -f \"$f\" \"$b\"; fi; \
         mv -f \"$f.new\" \"$f\" && \
         sh \"$HOME/infynitae/live/settings.sh\" log FACE \"$old\" >/dev/null 2>&1; \
         else rm -f \"$f.new\"; false; fi"
    )
}

/// Lists installed font families from `fc-list`.
pub struct FontsSource {
    pub mono: bool,
}

impl FontsSource {
    pub fn all() -> FontsSource {
        FontsSource { mono: false }
    }

    pub fn mono() -> FontsSource {
        FontsSource { mono: true }
    }
}

impl Source for FontsSource {
    fn fetch(&mut self) -> Data {
        let (key, cmd, first): (&str, &str, &[&str]) = if self.mono {
            ("fonts-mono", "fc-list :spacing=100 family 2>/dev/null", &[])
        } else {
            (
                "fonts",
                "fc-list : family 2>/dev/null",
                &["sans-serif", "monospace"],
            )
        };
        let out = run_sh(key, cmd);
        Box::new(FoundNames(parse_fonts(&out, first)))
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

/// Lists installed icon themes, excluding `Infynitae-*` and cursor-only themes.
pub struct IconsSource {
    pub dirs: Vec<PathBuf>,
}

impl IconsSource {
    pub fn system() -> IconsSource {
        IconsSource {
            dirs: icon_roots(&env_home()),
        }
    }
}

impl Source for IconsSource {
    fn fetch(&mut self) -> Data {
        if let Some(raw) = render_value("icons") {
            return Box::new(FoundNames(parse_lines(&raw)));
        }
        Box::new(FoundNames(scan_icons(&self.dirs)))
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

/// Lists installed cursor themes (folders containing a `cursors/` subdirectory).
pub struct CursorsSource {
    pub dirs: Vec<PathBuf>,
}

impl CursorsSource {
    pub fn system() -> CursorsSource {
        CursorsSource {
            dirs: icon_roots(&env_home()),
        }
    }
}

impl Source for CursorsSource {
    fn fetch(&mut self) -> Data {
        if let Some(raw) = render_value("cursors") {
            return Box::new(FoundNames(parse_lines(&raw)));
        }
        Box::new(FoundNames(scan_cursors(&self.dirs)))
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

fn env_home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn system_bad_colors_dir(home: &Path) -> PathBuf {
    let base = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home.join(".cache"),
    };
    base.join("infynitae/look-bad")
}

fn icon_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/share/icons"),
        home.join(".local/share/icons"),
        home.join(".icons"),
    ]
}

fn has_wallpaper_bin(home: &Path, path: &str) -> bool {
    if home.join(".local/bin/infynitae-wallpaper").is_file()
        || home.join("infynitae/live/infynitae-wallpaper").is_file()
    {
        return true;
    }
    path.split(':')
        .filter(|d| !d.is_empty())
        .any(|d| Path::new(d).join("infynitae-wallpaper").is_file())
}

fn parse_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parses `fc-list` family output: takes the first comma-separated name per line, deduplicates,
/// sorts, and puts `prefer_first` entries at the front when non-empty.
pub fn parse_fonts(out: &str, prefer_first: &[&str]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for line in out.lines() {
        let first = line.split(',').next().unwrap_or("").trim();
        if !first.is_empty() {
            set.insert(first.to_string());
        }
    }
    if set.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<String> = set.into_iter().collect();
    sorted.sort_by(|a, b| {
        a.to_ascii_lowercase()
            .cmp(&b.to_ascii_lowercase())
            .then_with(|| a.cmp(b))
    });
    let mut result = Vec::with_capacity(sorted.len() + prefer_first.len());
    for &p in prefer_first {
        result.push(p.to_string());
    }
    for name in sorted {
        if !prefer_first.contains(&name.as_str()) {
            result.push(name);
        }
    }
    result
}

fn is_wallpaper_ext(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp"
    )
}

/// Scans wallpaper folders for `.png`, `.jpg`, `.jpeg`, `.webp` files, sorted by filename.
pub fn scan_wallpapers(dirs: &[PathBuf]) -> Vec<WallpaperFile> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for item in rd.flatten() {
            let path = item.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = item.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.starts_with('.') || !is_wallpaper_ext(&name) {
                continue;
            }
            let full = path.to_string_lossy().into_owned();
            if seen.insert(full.clone()) {
                entries.push(WallpaperFile { name, path: full });
            }
        }
    }
    entries.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    });
    entries
}

fn index_has_icon_directories(index_text: &str) -> bool {
    let mut in_icon_theme = false;
    for raw in index_text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_icon_theme = line == "[Icon Theme]";
            continue;
        }
        if !in_icon_theme {
            continue;
        }
        let Some(rest) = line.strip_prefix("Directories=") else {
            continue;
        };
        // Require at least one non-empty directory entry other than "cursors".
        return rest
            .split(',')
            .map(str::trim)
            .any(|d| !d.is_empty() && !d.eq_ignore_ascii_case("cursors"));
    }
    false
}

fn scan_theme_dirs<F>(dirs: &[PathBuf], keep: F) -> Vec<String>
where
    F: Fn(&Path) -> bool,
{
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in dirs {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        for item in rd.flatten() {
            let path = item.path();
            if !path.is_dir() {
                continue;
            }
            let file_name = item.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if name.starts_with('.') || name.starts_with("Infynitae-") {
                continue;
            }
            if keep(&path) && seen.insert(name.to_string()) {
                out.push(name.to_string());
            }
        }
    }
    sort_names(&mut out);
    out
}

/// Scans icon directories for themes with an `index.theme` holding `Directories=`, excluding
/// `Infynitae-*` generated folder themes and cursor-only themes.
pub fn scan_icons(dirs: &[PathBuf]) -> Vec<String> {
    scan_theme_dirs(dirs, |path| {
        std::fs::read_to_string(path.join("index.theme"))
            .is_ok_and(|index| index_has_icon_directories(&index))
    })
}

/// Scans icon directories for cursor themes (folders holding a `cursors/` subdirectory).
pub fn scan_cursors(dirs: &[PathBuf]) -> Vec<String> {
    scan_theme_dirs(dirs, |path| path.join("cursors").is_dir())
}

fn sort_names(v: &mut [String]) {
    v.sort_by(|a, b| {
        a.to_ascii_lowercase()
            .cmp(&b.to_ascii_lowercase())
            .then_with(|| a.cmp(b))
    });
}

#[derive(Clone, Copy, Debug)]
pub enum LookControl {
    Switch {
        on: &'static str,
        off: &'static str,
    },
    Pick(&'static [(&'static str, &'static str)]),
    Slide {
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
    },
    Step {
        min: i64,
        max: i64,
    },
    Accent,
    Found {
        source: &'static str,
        first: Option<(&'static str, &'static str)>,
        other_file: bool,
        file_name: bool,
    },
    Wallpapers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HideRule {
    None,
    WallpaperBin,
    PapirusIcons,
    FcList,
}

#[derive(Clone, Copy, Debug)]
pub struct LookRow {
    pub key: &'static str,
    pub name: &'static str,
    pub says: &'static str,
    pub control: LookControl,
    pub after: After,
    pub more: bool,
    pub hide: HideRule,
}

impl LookRow {
    /// Converts standard controls into `settings_rows::Control` for config verification tests.
    #[cfg(test)]
    pub fn as_control(&self) -> Option<Control> {
        match self.control {
            LookControl::Switch { on, off } => Some(Control::Switch { on, off }),
            LookControl::Pick(o) => Some(Control::Pick(o)),
            LookControl::Slide {
                min,
                max,
                step,
                unit,
            } => Some(Control::Slide {
                min,
                max,
                step,
                unit,
            }),
            LookControl::Step { min, max } => Some(Control::Step { min, max }),
            LookControl::Accent => Some(Control::Text),
            LookControl::Found { .. } | LookControl::Wallpapers => None,
        }
    }
}

const WP_MODE_OPTS: &[(&str, &str)] = &[
    ("false", "Off"),
    ("accent", "The accent"),
    ("all", "Everything"),
    ("shuffle", "A new accent each time"),
];

const SCHEME_OPTS: &[(&str, &str)] = &[
    ("yours", "Yours"),
    ("monochrome", "Monochrome"),
    ("neutral", "Neutral"),
    ("tonal", "Tonal"),
    ("vibrant", "Vibrant"),
    ("expressive", "Expressive"),
];

const ACCENT_ROW: usize = 1;
const WP_MODE_ROW: usize = 2;
const SCHEME_ROW: usize = 4;

pub const TABLE_APPEARANCE: &[LookRow] = &[
    LookRow {
        key: "COLOR_SCHEME",
        name: "Dark or light",
        says: "",
        control: LookControl::Pick(&[("prefer-dark", "Dark"), ("prefer-light", "Light")]),
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "ACCENT",
        name: "Accent",
        says: "",
        control: LookControl::Accent,
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "THEME_FROM_WALLPAPER",
        name: "Colours from the wallpaper",
        says: "",
        control: LookControl::Pick(WP_MODE_OPTS),
        after: After::Save,
        more: false,
        hide: HideRule::WallpaperBin,
    },
    LookRow {
        key: "WINDOW_CORNERS",
        name: "Corners",
        says: "",
        control: LookControl::Slide {
            min: 0.0,
            max: 24.0,
            step: 1.0,
            unit: " px",
        },
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "THEME_SCHEME",
        name: "Colours",
        says: "",
        control: LookControl::Pick(SCHEME_OPTS),
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "CARD_OPACITY",
        name: "Card opacity",
        says: "",
        control: LookControl::Slide {
            min: 20.0,
            max: 100.0,
            step: 1.0,
            unit: " %",
        },
        after: After::Save,
        more: true,
        hide: HideRule::None,
    },
    LookRow {
        key: "BORDER_WIDTH",
        name: "Window border",
        says: "",
        control: LookControl::Step { min: 0, max: 8 },
        after: After::Save,
        more: true,
        hide: HideRule::None,
    },
    LookRow {
        key: "FOLDER_COLOR",
        name: "Folders in the accent",
        says: "",
        control: LookControl::Switch {
            on: "accent",
            off: "theme",
        },
        after: After::Save,
        more: true,
        hide: HideRule::PapirusIcons,
    },
];

pub const TABLE_WALLPAPER: &[LookRow] = &[
    LookRow {
        key: "WALLPAPER",
        name: "Picture",
        says: "",
        control: LookControl::Wallpapers,
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "WALLPAPER_MODE",
        name: "Fit",
        says: "",
        control: LookControl::Pick(&[
            ("fill", "Fill"),
            ("fit", "Fit"),
            ("stretch", "Stretch"),
            ("center", "Center"),
            ("tile", "Tile"),
        ]),
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "WALLPAPER_PARALLAX",
        name: "Depth",
        says: "",
        control: LookControl::Pick(&[("off", "Off"), ("subtle", "Subtle"), ("strong", "Strong")]),
        after: After::Save,
        more: true,
        hide: HideRule::None,
    },
];

pub const TABLE_FONTS: &[LookRow] = &[
    LookRow {
        key: "FONT_NAME",
        name: "Font",
        says: "Apps take a new font at their next start",
        control: LookControl::Found {
            source: "fonts",
            first: None,
            other_file: false,
            file_name: false,
        },
        after: After::Save,
        more: false,
        hide: HideRule::FcList,
    },
    LookRow {
        key: "FONT_SIZE",
        name: "Size",
        says: "",
        control: LookControl::Step { min: 6, max: 24 },
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "ICON_THEME",
        name: "Icons",
        says: "",
        control: LookControl::Found {
            source: "icons",
            first: None,
            other_file: false,
            file_name: false,
        },
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "CURSOR_THEME",
        name: "Cursor",
        says: "Apps take a new cursor at their next start",
        control: LookControl::Found {
            source: "cursors",
            first: None,
            other_file: false,
            file_name: false,
        },
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "CURSOR_SIZE",
        name: "Cursor size",
        says: "Apps take a new cursor at their next start",
        control: LookControl::Pick(&[("16", "16"), ("24", "24"), ("32", "32"), ("48", "48")]),
        after: After::Save,
        more: true,
        hide: HideRule::None,
    },
    LookRow {
        key: "TERMINAL_FONT",
        name: "Terminal font",
        says: "",
        control: LookControl::Found {
            source: "fonts-mono",
            first: Some(("", "Same as Font")),
            other_file: false,
            file_name: false,
        },
        after: After::Save,
        more: true,
        hide: HideRule::FcList,
    },
];

pub const KEYS: &[&str] = &[
    TABLE_APPEARANCE[0].key,
    TABLE_APPEARANCE[1].key,
    TABLE_APPEARANCE[2].key,
    TABLE_APPEARANCE[3].key,
    TABLE_APPEARANCE[4].key,
    TABLE_APPEARANCE[5].key,
    TABLE_APPEARANCE[6].key,
    TABLE_APPEARANCE[7].key,
    TABLE_WALLPAPER[0].key,
    TABLE_WALLPAPER[1].key,
    TABLE_WALLPAPER[2].key,
    TABLE_FONTS[0].key,
    TABLE_FONTS[1].key,
    TABLE_FONTS[2].key,
    TABLE_FONTS[3].key,
    TABLE_FONTS[4].key,
    TABLE_FONTS[5].key,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LookKind {
    Appearance,
    Wallpaper,
    Fonts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ListChoiceAction {
    Set(String),
    AskOther,
}

#[derive(Clone, Debug)]
struct ListEntry {
    label: String,
    action: ListChoiceAction,
    active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccentTab {
    Wallpaper,
    AnyColor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccentDrag {
    Hue,
    Sv,
}

/// Shared card implementation for `settings/appearance`, `settings/wallpaper`, and `settings/fonts`.
pub struct LookCard {
    kind: LookKind,
    table: &'static [LookRow],
    config: Option<ConfigData>,
    look: Option<LookData>,
    wallpapers: Option<WallpapersData>,
    face: Option<FaceData>,
    face_open: bool,
    face_preview: Option<Arc<DecodedImage>>,
    found: HashMap<&'static str, Vec<String>>,
    pending: HashMap<&'static str, String>,
    pick: Option<usize>,
    list: ui::List,
    list_scroll: usize,
    accent_tab: AccentTab,
    accent_hsv: (f64, f64, f64),
    accent_drag: Option<AccentDrag>,
    accent_preview: Option<String>,
    wp_mode_open: bool,
    hue_rect: RefCell<Option<Rect>>,
    sv_rect: RefCell<Option<Rect>>,
    more: bool,
    query: String,
    sliding: Option<(usize, f64)>,
    slide_rects: std::cell::RefCell<HashMap<usize, Rect>>,
    home: String,
}

impl LookCard {
    pub fn appearance() -> LookCard {
        Self::with_table(LookKind::Appearance, TABLE_APPEARANCE)
    }

    pub fn wallpaper() -> LookCard {
        Self::with_table(LookKind::Wallpaper, TABLE_WALLPAPER)
    }

    pub fn fonts() -> LookCard {
        Self::with_table(LookKind::Fonts, TABLE_FONTS)
    }

    fn with_table(kind: LookKind, table: &'static [LookRow]) -> LookCard {
        LookCard {
            kind,
            table,
            config: None,
            look: None,
            wallpapers: None,
            face: None,
            face_open: false,
            face_preview: None,
            found: HashMap::new(),
            pending: HashMap::new(),
            pick: None,
            list: ui::List::default(),
            list_scroll: 0,
            accent_tab: AccentTab::Wallpaper,
            accent_hsv: (220.0, 0.50, 0.97),
            accent_drag: None,
            accent_preview: None,
            wp_mode_open: false,
            hue_rect: RefCell::new(None),
            sv_rect: RefCell::new(None),
            more: false,
            query: String::new(),
            sliding: None,
            slide_rects: Default::default(),
            home: std::env::var("HOME").unwrap_or_default(),
        }
    }

    fn ready_for_status(&self) -> bool {
        let Some(_) = self.config else {
            return false;
        };
        match self.kind {
            LookKind::Appearance => self.look.is_some(),
            LookKind::Wallpaper => self.wallpapers.is_some(),
            LookKind::Fonts => ["fonts", "fonts-mono", "icons", "cursors"]
                .iter()
                .all(|k| self.found.contains_key(k)),
        }
    }

    fn key_value(&self, key: &str) -> String {
        if let Some(v) = self.pending.get(key) {
            return v.clone();
        }
        let Some(cfg) = &self.config else {
            return String::new();
        };
        if key == "ACCENT" {
            if let Some(v) = cfg.now.get("ACCENT").filter(|v| !v.is_empty()) {
                return v.clone();
            }
            // Configs saved before ACCENT existed only have BORDER_ACTIVE.
            if let Some(v) = cfg.now.get("BORDER_ACTIVE").filter(|v| !v.is_empty()) {
                return v.clone();
            }
        }
        cfg.value(key).to_string()
    }

    fn value(&self, i: usize) -> String {
        let r = &self.table[i];
        if r.key == "ACCENT" {
            if let Some(ref preview) = self.accent_preview {
                return preview.clone();
            }
        }
        if let Some((_, v)) = self.sliding.filter(|(idx, _)| *idx == i) {
            return fmt_number(v);
        }
        self.key_value(r.key)
    }

    fn accent_picker_open(&self) -> bool {
        self.kind == LookKind::Appearance && self.pick == Some(ACCENT_ROW) && self.list.open
    }

    fn row_visible(&self, r: &LookRow) -> bool {
        let Some(cfg) = &self.config else {
            return false;
        };
        if self.kind == LookKind::Wallpaper && !self.wallpapers.as_ref().is_some_and(|w| w.has_bin)
        {
            return false;
        }
        match r.hide {
            HideRule::None => true,
            HideRule::WallpaperBin => self.look.as_ref().is_some_and(|l| l.wallpaper_bin),
            HideRule::PapirusIcons => self.key_value("ICON_THEME").starts_with("Papirus"),
            HideRule::FcList => cfg.has("fc-list"),
        }
    }

    fn matches(&self, r: &LookRow) -> bool {
        let hay = format!("{} {} {}", r.name, r.says, r.key).to_lowercase();
        self.query
            .split_whitespace()
            .all(|w| hay.contains(&w.to_lowercase()))
    }

    fn shown(&self, cx: &Cx) -> (Vec<usize>, bool) {
        if !self.ready_for_status() {
            return (Vec::new(), false);
        }
        let searching = !self.query.trim().is_empty();
        let all = cx.size != Size::S || self.more || searching;
        let picker_open = self.kind == LookKind::Appearance
            && self.pick == Some(ACCENT_ROW)
            && (self.list.open || self.list.shown())
            && !searching;
        let mut rows = Vec::new();
        let mut hidden_more = false;
        for (i, r) in self.table.iter().enumerate() {
            if !self.row_visible(r) || (searching && !self.matches(r)) {
                continue;
            }
            if picker_open && matches!(r.key, "THEME_FROM_WALLPAPER" | "THEME_SCHEME") {
                continue;
            }
            if r.more && !all {
                hidden_more = true;
                continue;
            }
            rows.push(i);
        }
        let more_row = !searching && cx.size == Size::S && (hidden_more || self.more);
        (rows, more_row)
    }

    fn has_more_wallpapers(&self) -> bool {
        self.wallpapers
            .as_ref()
            .is_some_and(|w| w.files.len() > MAX_WALLPAPERS)
    }

    fn list_entries(&self, i: usize) -> Vec<ListEntry> {
        let r = &self.table[i];
        let cur = self.value(i);
        match r.control {
            LookControl::Pick(opts) => opts
                .iter()
                .map(|(val, word)| ListEntry {
                    label: (*word).to_string(),
                    action: ListChoiceAction::Set((*val).to_string()),
                    active: val.eq_ignore_ascii_case(&cur),
                })
                .collect(),
            LookControl::Wallpapers => {
                let mut out = Vec::new();
                let cur_expanded = expand_home(&cur, &self.home);
                let mut any_file_matched = false;
                if let Some(wp) = &self.wallpapers {
                    for (idx, f) in wp.files.iter().enumerate() {
                        let active = !cur.is_empty() && (cur == f.path || cur_expanded == f.path);
                        if active {
                            any_file_matched = true;
                        }
                        if idx >= MAX_WALLPAPERS {
                            out.push(ListEntry {
                                label: f.name.clone(),
                                action: ListChoiceAction::Set(f.path.clone()),
                                active,
                            });
                        }
                    }
                }
                out.push(ListEntry {
                    label: "The first in Pictures/Wallpapers".to_string(),
                    action: ListChoiceAction::Set(String::new()),
                    active: cur.is_empty(),
                });
                out.push(ListEntry {
                    label: "Other file".to_string(),
                    action: ListChoiceAction::AskOther,
                    active: !cur.is_empty() && !any_file_matched,
                });
                out
            }
            LookControl::Found {
                source,
                first,
                other_file,
                file_name,
            } => {
                let mut out = Vec::new();
                if let Some((val, word)) = first {
                    out.push(ListEntry {
                        label: (*word).to_string(),
                        action: ListChoiceAction::Set((*val).to_string()),
                        active: cur == *val,
                    });
                }
                let mut any_file_matched = false;
                if source == "wallpapers" {
                    let cur_expanded = expand_home(&cur, &self.home);
                    if let Some(wp) = &self.wallpapers {
                        for f in &wp.files {
                            let active =
                                !cur.is_empty() && (cur == f.path || cur_expanded == f.path);
                            if active {
                                any_file_matched = true;
                            }
                            out.push(ListEntry {
                                label: f.name.clone(),
                                action: ListChoiceAction::Set(f.path.clone()),
                                active,
                            });
                        }
                    }
                } else if let Some(names) = self.found.get(source) {
                    let eq = |a: &str, b: &str| {
                        if file_name {
                            a == b
                        } else {
                            a.eq_ignore_ascii_case(b)
                        }
                    };
                    for name in names {
                        out.push(ListEntry {
                            label: name.clone(),
                            action: ListChoiceAction::Set(name.clone()),
                            active: eq(name, &cur),
                        });
                    }
                }
                if other_file {
                    out.push(ListEntry {
                        label: "Other file".to_string(),
                        action: ListChoiceAction::AskOther,
                        active: !cur.is_empty() && !any_file_matched,
                    });
                }
                out
            }
            _ => Vec::new(),
        }
    }

    fn visible_list_rows(&self, i: usize) -> usize {
        self.list_entries(i).len().clamp(1, MAX_LIST_ROWS)
    }

    fn row_h(&self, cx: &Cx, i: usize, width: f64) -> f64 {
        match self.table[i].control {
            LookControl::Wallpapers => {
                let files_len = self.wallpapers.as_ref().map_or(0, |w| w.files.len());
                let shown_len = files_len.min(MAX_WALLPAPERS);
                if shown_len == 0 {
                    return ui::ROW_H;
                }
                let (cols, _, ch) = thumb_cell_size(width, cx.size);
                let grid_rows = shown_len.div_ceil(cols);
                let grid_h = THUMB_TOP_PAD
                    + grid_rows as f64 * ch
                    + (grid_rows.saturating_sub(1)) as f64 * THUMB_GAP;
                let more_h = if files_len > MAX_WALLPAPERS {
                    ui::ROW_H
                        + if self.pick == Some(i) {
                            self.list.height(self.visible_list_rows(i))
                        } else {
                            0.0
                        }
                } else {
                    0.0
                };
                grid_h + THUMB_NAME_H + more_h
            }
            _ => {
                ui::ROW_H
                    + match self.table[i].control {
                        LookControl::Slide { .. } => SLIDE_H,
                        _ => 0.0,
                    }
                    + if self.pick == Some(i) {
                        match self.table[i].control {
                            LookControl::Pick(_) | LookControl::Found { .. } => {
                                self.list.height(self.visible_list_rows(i))
                            }
                            LookControl::Accent => {
                                let full_h = self.accent_picker_full_h(cx, width);
                                self.list.height_for(full_h)
                            }
                            _ => 0.0,
                        }
                    } else {
                        0.0
                    }
            }
        }
    }

    fn display_word(&self, i: usize) -> String {
        let v = self.value(i);
        match self.table[i].control {
            LookControl::Pick(opts) => opts
                .iter()
                .find(|(val, _)| val.eq_ignore_ascii_case(&v))
                .map_or(v, |(_, w)| (*w).to_string()),
            LookControl::Wallpapers => {
                if v.is_empty() {
                    "The first in Pictures/Wallpapers".to_string()
                } else {
                    Path::new(&v)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&v)
                        .to_string()
                }
            }
            LookControl::Found {
                first, file_name, ..
            } => {
                if let Some((fval, fword)) = first {
                    if v == *fval {
                        return (*fword).to_string();
                    }
                }
                if file_name {
                    Path::new(&v)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&v)
                        .to_string()
                } else if v.is_empty() {
                    "Not set".to_string()
                } else {
                    v
                }
            }
            _ => v,
        }
    }

    fn is_default(&self, i: usize) -> bool {
        let key = self.table[i].key;
        let Some(cfg) = &self.config else {
            return true;
        };
        let def = cfg.default.get(key).map(String::as_str).unwrap_or("");
        self.value(i) == def
    }

    fn is_bad_color(&self, r: &LookRow) -> bool {
        matches!(r.control, LookControl::Accent)
            && self.look.as_ref().is_some_and(|l| {
                l.bad_colors.contains(r.key)
                    || (r.key == "ACCENT"
                        && !self
                            .config
                            .as_ref()
                            .is_some_and(|d| d.now.get("ACCENT").is_some_and(|v| !v.is_empty()))
                        && l.bad_colors.contains("BORDER_ACTIVE"))
            })
    }

    fn state(&self, r: &LookRow) -> (&'static str, bool) {
        if self.is_bad_color(r) {
            ("Not a colour", true)
        } else {
            (r.says, false)
        }
    }

    fn set(&mut self, i: usize, new: String, out: &mut Vec<Action>) -> bool {
        let r = self.table[i];
        let old_line = self.config.as_ref().and_then(|d| d.now.get(r.key).cloned());
        if self.value(i) == new {
            return false;
        }
        out.push(Action::Run(change_command(
            r.key,
            old_line.as_deref(),
            &new,
            r.after,
        )));
        self.pending.insert(r.key, new);
        true
    }

    fn ask_text_command(&self, r: &LookRow, cur: &str) -> String {
        let old_line = self.config.as_ref().and_then(|d| d.now.get(r.key).cloned());
        // The answer comes back through shell variable `$v` so it never becomes shell code.
        let set = change_command(r.key, old_line.as_deref(), "@@V@@", r.after)
            .replace("'@@V@@'", "\"$v\"");
        format!(
            "v=$(\"$HOME/.local/bin/infynitae-edge\" ask --text --default {} {}) || exit 0; {set}",
            sq(cur),
            sq(r.name),
        )
    }

    fn color_ask_command(&self, r: &LookRow, cur: &str) -> String {
        let old_line = self.config.as_ref().and_then(|d| d.now.get(r.key).cloned());
        let set = change_command(r.key, old_line.as_deref(), "@@V@@", r.after)
            .replace("'@@V@@'", "\"$v\"");
        format!(
            "v=$(\"$HOME/.local/bin/infynitae-edge\" ask --text --default {} {}) || exit 0; \
             bd=\"${{XDG_RUNTIME_DIR:-$HOME/.cache}}/infynitae/look-bad\"; \
             case \"$v\" in \
             \"#\"[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) \
             rm -f \"$bd\"/{k}; {set} ;; \
             *) mkdir -p \"$bd\" && : > \"$bd\"/{k} ;; \
             esac",
            sq(cur),
            sq(r.name),
            k = sq(r.key),
        )
    }

    fn open_pick(&mut self, i: usize) {
        self.wp_mode_open = false;
        self.accent_drag = None;
        self.accent_preview = None;
        if self.pick == Some(i) {
            self.list.toggle();
            return;
        }
        let entries = self.list_entries(i);
        let max_scroll = entries.len().saturating_sub(MAX_LIST_ROWS);
        let initial = entries
            .iter()
            .position(|e| e.active)
            .map_or(0, |pos| pos.saturating_sub(MAX_LIST_ROWS / 2))
            .min(max_scroll);
        self.pick = Some(i);
        self.list = ui::List::default();
        self.list.toggle();
        self.list_scroll = initial;
    }

    fn sync_accent_hsv(&mut self) {
        let cur = self.key_value("ACCENT");
        if let Some(rgb) = parse_hex(&cur) {
            self.accent_hsv = rgb_to_hsv(rgb);
        }
    }

    fn open_accent_pick(&mut self, i: usize) {
        self.wp_mode_open = false;
        self.accent_drag = None;
        self.accent_preview = None;
        if self.pick == Some(i) {
            self.list.toggle();
            return;
        }
        self.pick = Some(i);
        self.list = ui::List::default();
        self.list.toggle();
        self.list_scroll = 0;
        self.sync_accent_hsv();
        let has_cand = self
            .look
            .as_ref()
            .is_some_and(|l| !l.accent_candidates.is_empty());
        self.accent_tab = if has_cand {
            AccentTab::Wallpaper
        } else {
            AccentTab::AnyColor
        };
    }

    fn set_accent(&mut self, new_hex: String, out: &mut Vec<Action>) -> bool {
        if let Some(rgb) = parse_hex(&new_hex) {
            self.accent_hsv = rgb_to_hsv(rgb);
        }
        let cur = self.value(ACCENT_ROW);
        if cur.eq_ignore_ascii_case(&new_hex) {
            return false;
        }
        self.set(ACCENT_ROW, new_hex, out)
    }

    fn recent_colors(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(cur) = normalize_hex(&self.value(ACCENT_ROW)) {
            out.push(cur);
        }
        if let Some(look) = &self.look {
            for hex in look.recent_accents.iter().take(MAX_RECENT_LOG) {
                if !out.contains(hex) {
                    out.push(hex.clone());
                }
            }
        }
        out
    }

    fn accent_adjusted_warning(&self, cx: &Cx) -> bool {
        let cur = self.value(ACCENT_ROW);
        let Some(rgb) = normalize_hex(&cur).and_then(|h| parse_hex(&h)) else {
            return false;
        };
        let scheme = self.key_value("THEME_SCHEME");
        let bg = if scheme.is_empty() || scheme.eq_ignore_ascii_case("yours") {
            normalize_hex(&self.key_value("SURFACE_COLOR"))
                .and_then(|h| parse_hex(&h))
                .map(Rgba::rgb)
                .unwrap_or(cx.theme.card)
        } else if scheme.eq_ignore_ascii_case("monochrome") {
            // Monochrome uses a fixed accent independent of ACCENT.
            return false;
        } else {
            cx.theme.card
        };
        !theme::is_readable(Rgba::rgb(rgb), bg)
    }

    fn accent_picker_full_h(&self, cx: &Cx, width: f64) -> f64 {
        let iw = (width - 16.0).max(40.0);
        self.accent_picker_layout(cx, 0.0, 0.0, iw).total_h
    }

    fn accent_picker_layout(&self, cx: &Cx, ix: f64, top_y: f64, iw: f64) -> AccentPickerLayout {
        let mut y = top_y + 8.0;
        let tabs_r = (ix, y, iw, 30.0);
        y += 30.0 + 10.0;

        let cand_len = self.look.as_ref().map_or(0, |l| l.accent_candidates.len());
        let has_wp_bin = self.row_visible(&self.table[WP_MODE_ROW]);

        let mut no_cand_y = None;
        let mut pic_r = None;
        let mut cand_sq_y = None;
        let mut shuffle_line_y = None;
        let mut wp_mode_list_r = None;
        let mut hue_r = None;
        let mut sv_r = None;
        let mut code_line_y = None;
        let mut recent_label_y = None;
        let mut recent_sq_y = None;

        match self.accent_tab {
            AccentTab::Wallpaper => {
                if cand_len == 0 {
                    no_cand_y = Some(y);
                    y += 20.0 + 10.0;
                } else {
                    let pic_h = (iw * 10.0 / 16.0).round().max(40.0);
                    pic_r = Some((ix, y, iw, pic_h));
                    y += pic_h + 10.0;

                    cand_sq_y = Some(y);
                    let cols = ((iw + CAND_SQ_GAP) / (CAND_SQ_SIZE + CAND_SQ_GAP))
                        .floor()
                        .max(1.0) as usize;
                    let sq_rows = cand_len.div_ceil(cols);
                    let sq_h = sq_rows as f64 * CAND_SQ_SIZE
                        + sq_rows.saturating_sub(1) as f64 * CAND_SQ_GAP;
                    y += sq_h + 10.0;

                    if has_wp_bin {
                        shuffle_line_y = Some(y);
                        y += 32.0 + 6.0;
                        if self.wp_mode_open {
                            let lh = WP_MODE_OPTS.len() as f64 * ui::LIST_ROW_H + 8.0;
                            wp_mode_list_r = Some((ix, y, iw, lh));
                            y += lh + 6.0;
                        }
                    }
                }
            }
            AccentTab::AnyColor => {
                let hr = (ix, y, iw, HUE_STRIP_H);
                hue_r = Some(hr);
                y += HUE_STRIP_H + 10.0;

                let sr = (ix, y, iw, SV_SQUARE_H);
                sv_r = Some(sr);
                y += SV_SQUARE_H + 10.0;

                code_line_y = Some(y);
                y += 30.0 + 10.0;

                let recent_len = self.recent_colors().len();
                if recent_len > 0 {
                    recent_label_y = Some(y);
                    y += 20.0;
                    recent_sq_y = Some(y);
                    let cols = ((iw + RECENT_SQ_GAP) / (RECENT_SQ_SIZE + RECENT_SQ_GAP))
                        .floor()
                        .max(1.0) as usize;
                    let r_rows = recent_len.div_ceil(cols);
                    let rh = r_rows as f64 * RECENT_SQ_SIZE
                        + r_rows.saturating_sub(1) as f64 * RECENT_SQ_GAP;
                    y += rh + 10.0;
                }
            }
        }

        let style_label_y = y;
        y += 20.0;
        let style_y = y;
        let style_h = if cx.size == Size::S {
            28.0 * 2.0 + 6.0
        } else {
            28.0
        };
        y += style_h + 8.0;

        let warn_y = if self.accent_adjusted_warning(cx) {
            let wy = y;
            y += 20.0 + 4.0;
            Some(wy)
        } else {
            None
        };

        y += 8.0;
        AccentPickerLayout {
            tabs_r,
            no_cand_y,
            pic_r,
            cand_sq_y,
            shuffle_line_y,
            wp_mode_list_r,
            hue_r,
            sv_r,
            code_line_y,
            recent_label_y,
            recent_sq_y,
            style_label_y,
            style_y,
            warn_y,
            total_h: y - top_y,
        }
    }

    fn draw_list(&self, cr: &Context, cx: &Cx, hits: &mut Hits, lr: Rect, entries: &[ListEntry]) {
        ui::list_box(cr, cx, lr);
        if entries.is_empty() {
            if lr.3 >= ui::LIST_ROW_H + 4.0 {
                draw::text_max(
                    cr,
                    lr.0 + 10.0,
                    lr.1 + 4.0 + (ui::LIST_ROW_H - 16.0) / 2.0,
                    (lr.2 - 20.0).max(20.0),
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: cx.theme.muted,
                        text: "None found",
                    },
                );
            }
            return;
        }
        let vis = entries.len().min(MAX_LIST_ROWS);
        let max_scroll = entries.len().saturating_sub(vis);
        let start = self.list_scroll.min(max_scroll);
        let end = (start + vis).min(entries.len());
        for (offset, j) in (start..end).enumerate() {
            let rr = (
                lr.0 + 4.0,
                lr.1 + 4.0 + offset as f64 * ui::LIST_ROW_H,
                lr.2 - 8.0,
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
                &entries[j].label,
                "",
                entries[j].active,
                None,
            );
        }
    }
    fn draw_accent_picker(&self, cr: &Context, cx: &Cx, hits: &mut Hits, lr: Rect) {
        ui::list_box(cr, cx, lr);
        let _ = cr.save();
        cr.rectangle(lr.0, lr.1, lr.2, lr.3);
        cr.clip();
        let iw = (lr.2 - 16.0).max(40.0);
        let ix = lr.0 + 8.0;
        let lay = self.accent_picker_layout(cx, ix, lr.1, iw);
        *self.hue_rect.borrow_mut() = lay.hue_r;
        *self.sv_rect.borrow_mut() = lay.sv_r;

        ui::choice(
            cr,
            cx,
            hits,
            H_TAB_WP,
            lay.tabs_r,
            &["From your wallpaper", "Any colour"],
            usize::from(self.accent_tab == AccentTab::AnyColor),
        );

        let cur_hex = normalize_hex(&self.value(ACCENT_ROW));

        if let Some(ny) = lay.no_cand_y {
            draw::text_max(
                cr,
                ix,
                ny + 2.0,
                iw,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "No colours found in the wallpaper",
                },
            );
        }

        if let Some(pic_r) = lay.pic_r {
            let _ = cr.save();
            draw::round_rect(cr, pic_r.0, pic_r.1, pic_r.2, pic_r.3, THUMB_RADIUS);
            cr.clip();
            if let Some(surf) = self
                .look
                .as_ref()
                .and_then(|l| l.accent_thumb.as_ref())
                .and_then(thumb_surface)
            {
                let sw = surf.width() as f64;
                let sh = surf.height() as f64;
                cr.translate(pic_r.0, pic_r.1);
                cr.scale(pic_r.2 / sw, pic_r.3 / sh);
                let _ = cr.set_source_surface(&surf, 0.0, 0.0);
                let _ = cr.paint();
            } else {
                draw::set(cr, cx.theme.track);
                let _ = cr.paint();
            }
            let _ = cr.restore();

            draw::round_rect(
                cr,
                pic_r.0 + 0.5,
                pic_r.1 + 0.5,
                (pic_r.2 - 1.0).max(1.0),
                (pic_r.3 - 1.0).max(1.0),
                THUMB_RADIUS,
            );
            draw::set(cr, cx.theme.edge);
            cr.set_line_width(1.0);
            let _ = cr.stroke();

            if let Some(cands) = self.look.as_ref().map(|l| &l.accent_candidates) {
                let margin = (ACTIVE_MARK_RADIUS + 1.0)
                    .min(pic_r.2 * 0.25)
                    .min(pic_r.3 * 0.25);
                let bounds = (
                    pic_r.0 + margin,
                    pic_r.1 + margin,
                    (pic_r.2 - 2.0 * margin).max(0.0),
                    (pic_r.3 - 2.0 * margin).max(0.0),
                );
                let mut pts: Vec<(f64, f64)> = cands
                    .iter()
                    .map(|c| {
                        (
                            (pic_r.0 + c.x * pic_r.2).clamp(bounds.0, bounds.0 + bounds.2),
                            (pic_r.1 + c.y * pic_r.3).clamp(bounds.1, bounds.1 + bounds.3),
                        )
                    })
                    .collect();
                nudge_marks(&mut pts, bounds, MARK_MIN_DIST);

                for active_pass in [false, true] {
                    for (idx, c) in cands.iter().enumerate() {
                        let active = cur_hex.as_deref() == Some(c.hex.as_str());
                        if active != active_pass {
                            continue;
                        }
                        let (mx, my) = pts[idx];
                        let rad = if active {
                            ACTIVE_MARK_RADIUS
                        } else {
                            MARK_RADIUS
                        };
                        let id = H_MARK_BASE + idx as u32;
                        let hit_r = (
                            mx - rad - 1.0,
                            my - rad - 1.0,
                            rad * 2.0 + 2.0,
                            rad * 2.0 + 2.0,
                        );
                        let _ = hits.add(id, hit_r) || hits.selected(id);
                        cr.new_sub_path();
                        cr.arc(mx, my, rad, 0.0, 2.0 * std::f64::consts::PI);
                        draw::set(cr, Rgba::rgb(c.rgb));
                        let _ = cr.fill_preserve();
                        // Thin card backing keeps the active accent ring visible on matching patches.
                        draw::set(cr, cx.theme.card.alpha(0.95));
                        cr.set_line_width(3.5);
                        let _ = cr.stroke_preserve();
                        draw::set(
                            cr,
                            if active {
                                cx.theme.accent
                            } else {
                                cx.theme.text
                            },
                        );
                        cr.set_line_width(2.0);
                        let _ = cr.stroke();
                    }
                }
            }
        }

        if let (Some(sq_y), Some(cands)) = (
            lay.cand_sq_y,
            self.look.as_ref().map(|l| &l.accent_candidates),
        ) {
            let cols = ((iw + CAND_SQ_GAP) / (CAND_SQ_SIZE + CAND_SQ_GAP))
                .floor()
                .max(1.0) as usize;
            for (idx, c) in cands.iter().enumerate() {
                let col = idx % cols;
                let row = idx / cols;
                let r = (
                    ix + col as f64 * (CAND_SQ_SIZE + CAND_SQ_GAP),
                    sq_y + row as f64 * (CAND_SQ_SIZE + CAND_SQ_GAP),
                    CAND_SQ_SIZE,
                    CAND_SQ_SIZE,
                );
                let active = cur_hex.as_deref() == Some(c.hex.as_str());
                draw_color_square(cr, cx, hits, H_CAND_SQ_BASE + idx as u32, r, c.rgb, active);
            }
        }

        if let Some(sy) = lay.shuffle_line_y {
            let btn_w = ui::button(cr, cx, hits, H_SHUFFLE, ix, sy + 3.0, "Shuffle");
            let mx = ix + btn_w + 10.0;
            let mw = (iw - btn_w - 10.0).max(40.0);
            let mode_r = (mx, sy, mw, 32.0);
            let lit = hits.add(H_WP_MODE_BOX, mode_r) || hits.selected(H_WP_MODE_BOX);
            let word = self.display_word(WP_MODE_ROW);
            let vbox_w = ui::value_box(cr, cx, mode_r, ix + iw, &word, lit, self.wp_mode_open);
            draw::text_max(
                cr,
                mx,
                sy + (32.0 - 16.0) / 2.0,
                (mw - vbox_w - 8.0).max(20.0),
                &Text {
                    size: 13.0,
                    bold: false,
                    color: cx.theme.text,
                    text: self.table[WP_MODE_ROW].name,
                },
            );
        }

        if let Some(mlr) = lay.wp_mode_list_r {
            ui::list_box(cr, cx, mlr);
            let entries = self.list_entries(WP_MODE_ROW);
            for (idx, e) in entries.iter().enumerate() {
                let rr = (
                    mlr.0 + 4.0,
                    mlr.1 + 4.0 + idx as f64 * ui::LIST_ROW_H,
                    mlr.2 - 8.0,
                    ui::LIST_ROW_H,
                );
                ui::list_row(
                    cr,
                    cx,
                    hits,
                    H_WP_MODE_OPT_BASE + idx as u32,
                    rr,
                    &e.label,
                    "",
                    e.active,
                    None,
                );
            }
        }

        if let Some(hr) = lay.hue_r {
            draw_hue_strip(cr, cx, hits, H_HUE, hr, self.accent_hsv.0);
        }

        if let Some(sr) = lay.sv_r {
            draw_sv_square(cr, cx, hits, H_SV, sr, self.accent_hsv);
        }

        if let Some(cy) = lay.code_line_y {
            let box_w = 104.0_f64.min(iw * 0.55);
            let box_r = (ix + iw - box_w, cy + 2.0, box_w, 26.0);
            let lit = hits.add(H_CODE_ASK, box_r) || hits.selected(H_CODE_ASK);
            draw::round_rect(
                cr,
                box_r.0 + 0.5,
                box_r.1 + 0.5,
                box_r.2 - 1.0,
                box_r.3 - 1.0,
                cx.theme.small_radius(),
            );
            draw::set(
                cr,
                if lit {
                    cx.theme.text.alpha(ui::LIGHT_ALPHA)
                } else {
                    cx.theme.text.alpha(ui::BOX_ALPHA)
                },
            );
            let _ = cr.fill_preserve();
            draw::set(
                cr,
                if lit {
                    cx.theme.text.alpha(0.28)
                } else {
                    cx.theme.edge
                },
            );
            cr.set_line_width(1.0);
            let _ = cr.stroke();

            let code_label = self.value(ACCENT_ROW);
            let code_text = if code_label.is_empty() {
                "Not set"
            } else {
                &code_label
            };
            draw::text_center(
                cr,
                box_r.0 + box_r.2 / 2.0,
                box_r.1 + 5.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: if code_label.is_empty() {
                        cx.theme.muted
                    } else {
                        cx.theme.text
                    },
                    text: code_text,
                },
            );

            let bad = self.is_bad_color(&self.table[ACCENT_ROW]);
            let left_w = (iw - box_w - 8.0).max(20.0);
            if bad {
                let bw = draw::text_right(
                    cr,
                    box_r.0 - 8.0,
                    cy + 7.0,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: cx.theme.alert,
                        text: "Not a colour",
                    },
                );
                draw::text_max(
                    cr,
                    ix,
                    cy + 7.0,
                    (left_w - bw - 8.0).max(20.0),
                    &Text {
                        size: 13.0,
                        bold: false,
                        color: cx.theme.text,
                        text: "Code",
                    },
                );
            } else {
                draw::text_max(
                    cr,
                    ix,
                    cy + 7.0,
                    left_w,
                    &Text {
                        size: 13.0,
                        bold: false,
                        color: cx.theme.text,
                        text: "Code",
                    },
                );
            }
        }

        if let (Some(ry), Some(sq_y)) = (lay.recent_label_y, lay.recent_sq_y) {
            draw::text(
                cr,
                ix,
                ry,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "Recent",
                },
            );
            let recent = self.recent_colors();
            let cols = ((iw + RECENT_SQ_GAP) / (RECENT_SQ_SIZE + RECENT_SQ_GAP))
                .floor()
                .max(1.0) as usize;
            for (idx, hex) in recent.iter().enumerate() {
                if let Some(rgb) = parse_hex(hex) {
                    let col = idx % cols;
                    let row = idx / cols;
                    let r = (
                        ix + col as f64 * (RECENT_SQ_SIZE + RECENT_SQ_GAP),
                        sq_y + row as f64 * (RECENT_SQ_SIZE + RECENT_SQ_GAP),
                        RECENT_SQ_SIZE,
                        RECENT_SQ_SIZE,
                    );
                    let active = cur_hex.as_deref() == Some(hex.as_str());
                    draw_color_square(cr, cx, hits, H_RECENT_BASE + idx as u32, r, rgb, active);
                }
            }
        }

        draw::text(
            cr,
            ix,
            lay.style_label_y,
            &Text {
                size: 12.0,
                bold: false,
                color: cx.theme.muted,
                text: "Style",
            },
        );
        let scheme = self.value(SCHEME_ROW);
        let scheme_idx = SCHEME_OPTS
            .iter()
            .position(|(v, _)| v.eq_ignore_ascii_case(&scheme));
        if cx.size == Size::S {
            draw_style_row(
                cr,
                cx,
                hits,
                H_SCHEME_BASE,
                (ix, lay.style_y, iw, 28.0),
                &SCHEME_OPTS[..3],
                scheme_idx,
            );
            draw_style_row(
                cr,
                cx,
                hits,
                H_SCHEME_BASE + 3,
                (ix, lay.style_y + 34.0, iw, 28.0),
                &SCHEME_OPTS[3..],
                scheme_idx.and_then(|s| s.checked_sub(3)),
            );
        } else {
            draw_style_row(
                cr,
                cx,
                hits,
                H_SCHEME_BASE,
                (ix, lay.style_y, iw, 28.0),
                SCHEME_OPTS,
                scheme_idx,
            );
        }

        if let Some(wy) = lay.warn_y {
            draw::text_max(
                cr,
                ix,
                wy + 2.0,
                iw,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "Text on the accent is adjusted to stay readable",
                },
            );
        }

        let _ = cr.restore();
    }
}

struct AccentPickerLayout {
    tabs_r: Rect,
    no_cand_y: Option<f64>,
    pic_r: Option<Rect>,
    cand_sq_y: Option<f64>,
    shuffle_line_y: Option<f64>,
    wp_mode_list_r: Option<Rect>,
    hue_r: Option<Rect>,
    sv_r: Option<Rect>,
    code_line_y: Option<f64>,
    recent_label_y: Option<f64>,
    recent_sq_y: Option<f64>,
    style_label_y: f64,
    style_y: f64,
    warn_y: Option<f64>,
    total_h: f64,
}

fn shuffle_command() -> &'static str {
    "sh \"$HOME/infynitae/live/settings.sh\" wallpaper-colors shuffle >/dev/null 2>&1"
}

/// Converts sRGB bytes `[r, g, b]` to HSV `(h in 0..360, s in 0..=1, v in 0..=1)`.
pub fn rgb_to_hsv(rgb: [u8; 3]) -> (f64, f64, f64) {
    let r = rgb[0] as f64 / 255.0;
    let g = rgb[1] as f64 / 255.0;
    let b = rgb[2] as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    let s = if max == 0.0 { 0.0 } else { d / max };
    let h = if d == 0.0 {
        0.0
    } else if (max - r).abs() < 1e-12 {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if (max - g).abs() < 1e-12 {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, v)
}

/// Converts HSV `(h in degrees, s in 0..=1, v in 0..=1)` to sRGB bytes `[r, g, b]`.
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> [u8; 3] {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0).floor() as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((g + m).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((b + m).clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn hsv_to_hex(h: f64, s: f64, v: f64) -> String {
    rgb_to_hex(hsv_to_rgb(h, s, v))
}

/// Nudges any pair of candidate marks closer than `min_dist` pixels apart within `bounds`.
pub fn nudge_marks(pts: &mut [(f64, f64)], bounds: Rect, min_dist: f64) {
    if pts.len() < 2 || min_dist <= 0.0 {
        return;
    }
    let min_x = bounds.0;
    let min_y = bounds.1;
    let max_x = bounds.0 + bounds.2.max(0.0);
    let max_y = bounds.1 + bounds.3.max(0.0);
    for _pass in 0..48 {
        let mut moved = false;
        for i in 0..pts.len() {
            for j in (i + 1)..pts.len() {
                let dx = pts[j].0 - pts[i].0;
                let dy = pts[j].1 - pts[i].1;
                let d = dx.hypot(dy);
                if d + 1e-6 >= min_dist {
                    continue;
                }
                let (ux, uy) = if d > 1e-6 {
                    (dx / d, dy / d)
                } else {
                    let angle = (i + j) as f64 * 2.399_963;
                    (angle.cos(), angle.sin())
                };
                let push = 0.5 * (min_dist - d) + 0.05;
                pts[i].0 -= ux * push;
                pts[i].1 -= uy * push;
                pts[j].0 += ux * push;
                pts[j].1 += uy * push;
                moved = true;
            }
        }
        for p in pts.iter_mut() {
            p.0 = p.0.clamp(min_x, max_x);
            p.1 = p.1.clamp(min_y, max_y);
        }
        if !moved {
            break;
        }
    }
}

fn draw_color_square(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    id: u32,
    r: Rect,
    rgb: [u8; 3],
    active: bool,
) {
    let lit = hits.add(id, r) || hits.selected(id);
    draw::round_rect(cr, r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, THUMB_RADIUS);
    draw::set(cr, Rgba::rgb(rgb));
    let _ = cr.fill_preserve();
    if active {
        draw::set(cr, cx.theme.text);
        cr.set_line_width(2.0);
    } else if lit {
        draw::set(cr, cx.theme.text.alpha(0.55));
        cr.set_line_width(1.5);
    } else {
        draw::set(cr, cx.theme.edge);
        cr.set_line_width(1.0);
    }
    let _ = cr.stroke();
}

fn draw_hue_strip(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, r: Rect, hue: f64) {
    let _ = hits.add(id, r) || hits.selected(id);
    let pat = cairo::LinearGradient::new(r.0, r.1, r.0 + r.2, r.1);
    let stops = [
        (0.0 / 6.0, (1.0, 0.0, 0.0)),
        (1.0 / 6.0, (1.0, 1.0, 0.0)),
        (2.0 / 6.0, (0.0, 1.0, 0.0)),
        (3.0 / 6.0, (0.0, 1.0, 1.0)),
        (4.0 / 6.0, (0.0, 0.0, 1.0)),
        (5.0 / 6.0, (1.0, 0.0, 1.0)),
        (1.0, (1.0, 0.0, 0.0)),
    ];
    for &(off, (rc, gc, bc)) in &stops {
        pat.add_color_stop_rgb(off, rc, gc, bc);
    }
    let _ = cr.save();
    draw::round_rect(cr, r.0, r.1, r.2, r.3, r.3 / 2.0);
    cr.clip();
    let _ = cr.set_source(&pat);
    let _ = cr.paint();
    let _ = cr.restore();

    draw::round_rect(cr, r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, r.3 / 2.0);
    draw::set(cr, cx.theme.edge);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    let rx = r.0 + (hue.clamp(0.0, 360.0) / 360.0) * r.2;
    let ry = r.1 + r.3 / 2.0;
    let pure = hsv_to_rgb(hue, 1.0, 1.0);
    draw_picker_ring(cr, cx, rx, ry, 5.5, Rgba::rgb(pure));
}

fn draw_sv_square(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, r: Rect, hsv: (f64, f64, f64)) {
    let _ = hits.add(id, r) || hits.selected(id);
    let pure = Rgba::rgb(hsv_to_rgb(hsv.0, 1.0, 1.0));
    let _ = cr.save();
    draw::round_rect(cr, r.0, r.1, r.2, r.3, THUMB_RADIUS);
    cr.clip();
    let horiz = cairo::LinearGradient::new(r.0, r.1, r.0 + r.2, r.1);
    horiz.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0);
    horiz.add_color_stop_rgb(1.0, pure.0, pure.1, pure.2);
    let _ = cr.set_source(&horiz);
    let _ = cr.paint();

    let vert = cairo::LinearGradient::new(r.0, r.1, r.0, r.1 + r.3);
    vert.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
    vert.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 1.0);
    let _ = cr.set_source(&vert);
    let _ = cr.paint();
    let _ = cr.restore();

    draw::round_rect(cr, r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, THUMB_RADIUS);
    draw::set(cr, cx.theme.edge);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    let rx = r.0 + hsv.1.clamp(0.0, 1.0) * r.2;
    let ry = r.1 + (1.0 - hsv.2.clamp(0.0, 1.0)) * r.3;
    let cur_rgb = hsv_to_rgb(hsv.0, hsv.1, hsv.2);
    draw_picker_ring(cr, cx, rx, ry, 6.0, Rgba::rgb(cur_rgb));
}

fn draw_picker_ring(cr: &Context, cx: &Cx, x: f64, y: f64, radius: f64, fill: Rgba) {
    cr.new_sub_path();
    cr.arc(x, y, radius, 0.0, 2.0 * std::f64::consts::PI);
    draw::set(cr, fill);
    let _ = cr.fill_preserve();
    draw::set(cr, cx.theme.card.alpha(1.0));
    cr.set_line_width(3.0);
    let _ = cr.stroke_preserve();
    draw::set(cr, cx.theme.text);
    cr.set_line_width(1.5);
    let _ = cr.stroke();
}

fn draw_style_row(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    first_id: u32,
    r: Rect,
    opts: &[(&str, &str)],
    chosen: Option<usize>,
) {
    if opts.is_empty() {
        return;
    }
    let gap = 5.0;
    let raw_w: Vec<f64> = opts
        .iter()
        .map(|(_, word)| {
            draw::measure(
                cr,
                &Text {
                    size: draw::SIZE_SMALL,
                    bold: true,
                    color: cx.theme.text,
                    text: word,
                },
            )
            .0 + 12.0
        })
        .collect();
    let total_raw: f64 = raw_w.iter().sum();
    let avail = (r.2 - gap * (opts.len() as f64 - 1.0)).max(20.0);
    let mut x = r.0;
    for (idx, (_, word)) in opts.iter().enumerate() {
        let w = if total_raw <= avail {
            raw_w[idx] + (avail - total_raw) / opts.len() as f64
        } else {
            raw_w[idx] * (avail / total_raw)
        };
        let b = (x, r.1, w, r.3);
        let id = first_id + idx as u32;
        ui::choice_button(cr, cx, hits, id, b, word, chosen == Some(idx));
        x += w + gap;
    }
}

fn expand_home(raw: &str, home: &str) -> String {
    if home.is_empty() {
        return raw.to_string();
    }
    let h = home.trim_end_matches('/');
    if raw == "~" {
        h.to_string()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        format!("{h}/{rest}")
    } else if let Some(rest) = raw.strip_prefix("$HOME/") {
        format!("{h}/{rest}")
    } else {
        raw.to_string()
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

fn value_at_right_capped(
    cr: &Context,
    cx: &Cx,
    r: Rect,
    x_right: f64,
    max_w: f64,
    text: &str,
    changed: bool,
) -> f64 {
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
    let w = draw::measure(cr, &t).0.min(max_w);
    draw::text_max(cr, x_right - w, r.1 + (r.3 - 15.0) / 2.0, max_w, &t)
}

fn thumb_cell_size(width: f64, size: Size) -> (usize, f64, f64) {
    let cols = if size == Size::M { 3 } else { 2 };
    let avail = (width - 2.0 * THUMB_PAD_X - (cols - 1) as f64 * THUMB_GAP).max(cols as f64 * 16.0);
    let cw = (avail / cols as f64).floor();
    let ch = (cw * 9.0 / 16.0).round().max(9.0);
    (cols, cw, ch)
}

thread_local! {
    static THUMB_SURFACES: RefCell<HashMap<usize, (Arc<DecodedImage>, cairo::ImageSurface)>> =
        RefCell::new(HashMap::new());
}

fn thumb_surface(img: &Arc<DecodedImage>) -> Option<cairo::ImageSurface> {
    let key = Arc::as_ptr(img) as usize;
    THUMB_SURFACES.with(|cache| {
        let mut map = cache.borrow_mut();
        if let Some((held, surf)) = map.get(&key) {
            if Arc::ptr_eq(held, img) {
                return Some(surf.clone());
            }
        }
        let surf = icons::image_surface_from_decoded(img)?;
        if map.len() >= 64 {
            map.clear();
        }
        map.insert(key, (Arc::clone(img), surf.clone()));
        Some(surf)
    })
}

impl LookCard {
    #[allow(clippy::too_many_arguments)]
    fn draw_wallpaper_grid(
        &self,
        cr: &Context,
        cx: &Cx,
        hits: &mut Hits,
        body: Rect,
        y: f64,
        i: usize,
        id: u32,
        changed: bool,
    ) {
        let files = self
            .wallpapers
            .as_ref()
            .map(|w| w.files.as_slice())
            .unwrap_or(&[]);
        let shown_len = files.len().min(MAX_WALLPAPERS);
        let right = body.0 + body.2 - 8.0;
        let word = self.display_word(i);

        if shown_len == 0 {
            draw::text_max(
                cr,
                body.0 + 8.0,
                y + (ui::ROW_H - 15.0) / 2.0,
                (body.2 - 16.0).max(20.0),
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "No pictures in Pictures/Wallpapers",
                },
            );
            if !self.value(i).is_empty() {
                value_at_right_capped(
                    cr,
                    cx,
                    (body.0, y, body.2, ui::ROW_H),
                    right,
                    (body.2 * 0.45).max(40.0),
                    &word,
                    changed,
                );
            }
            return;
        }

        let (cols, cw, ch) = thumb_cell_size(body.2, cx.size);
        let grid_rows = shown_len.div_ceil(cols);
        let cur = self.value(i);
        let cur_expanded = expand_home(&cur, &self.home);
        let thumbs = self.wallpapers.as_ref().map(|w| {
            if cx.size == Size::M {
                &w.thumbs_m
            } else {
                &w.thumbs_s
            }
        });

        for (idx, file) in files.iter().take(shown_len).enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            let tx = body.0 + THUMB_PAD_X + col as f64 * (cw + THUMB_GAP);
            let ty = y + THUMB_TOP_PAD + row as f64 * (ch + THUMB_GAP);
            let tid = THUMB + idx as u32;
            let lit = hits.add(tid, (tx, ty, cw, ch)) || hits.selected(tid);
            let active = !cur.is_empty() && (cur == file.path || cur_expanded == file.path);

            let _ = cr.save();
            draw::round_rect(cr, tx, ty, cw, ch, THUMB_RADIUS);
            cr.clip();
            if let Some(surf) = thumbs
                .and_then(|t| t.get(idx))
                .and_then(|o| o.as_ref())
                .and_then(thumb_surface)
            {
                let sw = surf.width() as f64;
                let sh = surf.height() as f64;
                if (sw - cw).abs() > 1.5 || (sh - ch).abs() > 1.5 {
                    cr.translate(tx, ty);
                    cr.scale(cw / sw, ch / sh);
                    let _ = cr.set_source_surface(&surf, 0.0, 0.0);
                } else {
                    let dx = ((cw - sw) / 2.0).round();
                    let dy = ((ch - sh) / 2.0).round();
                    let _ = cr.set_source_surface(&surf, tx + dx, ty + dy);
                }
                let _ = cr.paint();
            } else {
                draw::set(cr, cx.theme.track);
                let _ = cr.paint();
            }
            if lit {
                draw::set(cr, cx.theme.text.alpha(ui::LIGHT_ALPHA));
                let _ = cr.paint();
            }
            let _ = cr.restore();

            if active {
                draw::round_rect(
                    cr,
                    tx + 1.0,
                    ty + 1.0,
                    (cw - 2.0).max(1.0),
                    (ch - 2.0).max(1.0),
                    (THUMB_RADIUS - 0.5).max(0.0),
                );
                draw::set(cr, cx.theme.accent);
                cr.set_line_width(2.0);
                let _ = cr.stroke();
            } else if lit {
                draw::round_rect(
                    cr,
                    tx + 0.5,
                    ty + 0.5,
                    (cw - 1.0).max(1.0),
                    (ch - 1.0).max(1.0),
                    THUMB_RADIUS,
                );
                draw::set(cr, cx.theme.text.alpha(0.35));
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
        }

        let name_y = y
            + THUMB_TOP_PAD
            + grid_rows as f64 * ch
            + (grid_rows.saturating_sub(1)) as f64 * THUMB_GAP;
        draw::text_max(
            cr,
            body.0 + THUMB_PAD_X,
            name_y + (THUMB_NAME_H - 15.0) / 2.0,
            (body.2 - 2.0 * THUMB_PAD_X).max(20.0),
            &Text {
                size: 12.0,
                bold: false,
                color: if changed {
                    cx.theme.accent_text
                } else {
                    cx.theme.muted
                },
                text: &word,
            },
        );

        if files.len() > MAX_WALLPAPERS {
            let more_y = name_y + THUMB_NAME_H;
            let extra = format!("{} more", files.len() - MAX_WALLPAPERS);
            let entries = if self.pick == Some(i) && self.list.shown() {
                self.list_entries(i)
            } else {
                Vec::new()
            };
            let vis = entries.len().clamp(1, MAX_LIST_ROWS);
            let (row_r, list_r) = if self.pick == Some(i) && self.list.shown() {
                let lh = self.list.height(vis);
                self.list
                    .split((body.0, more_y, body.2, ui::ROW_H + lh), vis, cx.list_above)
            } else {
                ((body.0, more_y, body.2, ui::ROW_H), (0.0, 0.0, 0.0, 0.0))
            };
            let ew = draw::measure(
                cr,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: &extra,
                },
            )
            .0 + 14.0;
            ui::row(
                cr,
                cx,
                hits,
                id,
                row_r,
                &Row {
                    icon: None,
                    name: "More",
                    state: "",
                    on: false,
                    right: ew,
                },
            );
            value_at_right_capped(cr, cx, row_r, right, 96.0, &extra, false);
            if self.pick == Some(i) && self.list.shown() {
                self.draw_list(cr, cx, hits, list_r, &entries);
            }
        }
    }
}

fn draw_swatch(cr: &Context, cx: &Cx, x: f64, r: Rect, rgb: [u8; 3]) {
    let sy = r.1 + (r.3 - SWATCH) / 2.0;
    draw::round_rect(
        cr,
        x + 0.5,
        sy + 0.5,
        SWATCH - 1.0,
        SWATCH - 1.0,
        cx.theme.small_radius(),
    );
    draw::set(cr, Rgba::rgb(rgb));
    let _ = cr.fill_preserve();
    draw::set(cr, cx.theme.edge);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

impl LookCard {
    fn show_face_row(&self, cx: &Cx) -> bool {
        let _ = cx;
        if self.kind != LookKind::Appearance || self.config.is_none() {
            return false;
        }
        if self.query.trim().is_empty() {
            return true;
        }
        let hay = "your picture face avatar photo";
        self.query
            .split_whitespace()
            .all(|w| hay.contains(&w.to_lowercase()))
    }

    fn face_grid_geom(cx: &Cx, width: f64) -> (usize, f64) {
        let cols = if cx.size == Size::S { 3 } else { 4 };
        let avail =
            (width - 2.0 * THUMB_PAD_X - (cols - 1) as f64 * THUMB_GAP).max(cols as f64 * 32.0);
        let cell = (avail / cols as f64).floor();
        (cols, cell)
    }

    fn face_h(&self, cx: &Cx, width: f64) -> f64 {
        if !self.show_face_row(cx) {
            return 0.0;
        }
        if !self.face_open {
            return ui::ROW_H;
        }
        let n = self
            .face
            .as_ref()
            .map_or(0, |f| f.files.len().min(FACE_MAX_THUMBS));
        if n == 0 {
            return ui::ROW_H * 2.0;
        }
        let (cols, cell) = Self::face_grid_geom(cx, width);
        let rows = n.div_ceil(cols);
        ui::ROW_H + THUMB_TOP_PAD + rows as f64 * cell + rows.saturating_sub(1) as f64 * THUMB_GAP
    }

    fn draw_face_section(&self, cr: &Context, cx: &Cx, hits: &mut Hits, body: Rect, y: f64) -> f64 {
        let r = (body.0, y, body.2, ui::ROW_H);
        let right = body.0 + body.2 - 8.0;
        let current = self
            .face_preview
            .as_ref()
            .or_else(|| self.face.as_ref().and_then(|f| f.current.as_ref()));
        let right_pad = if current.is_some() { 32.0 } else { 44.0 };
        ui::row(
            cr,
            cx,
            hits,
            FACE_ROW,
            r,
            &Row {
                icon: None,
                name: "Your picture",
                state: "",
                on: self.face_open,
                right: right_pad,
            },
        );
        if let Some(img) = current {
            let rad = 12.0;
            let cx_c = right - rad;
            let cy_c = y + ui::ROW_H / 2.0;
            let _ = cr.save();
            cr.arc(cx_c, cy_c, rad, 0.0, std::f64::consts::TAU);
            cr.clip();
            if let Some(surf) = thumb_surface(img) {
                let sw = surf.width() as f64;
                let sh = surf.height() as f64;
                let d = 2.0 * rad;
                cr.translate(cx_c - rad, cy_c - rad);
                cr.scale(d / sw, d / sh);
                let _ = cr.set_source_surface(&surf, 0.0, 0.0);
                let _ = cr.paint();
            } else {
                draw::set(cr, cx.theme.track.alpha(0.35));
                let _ = cr.paint();
            }
            let _ = cr.restore();
        } else {
            value_at_right_capped(cr, cx, r, right, body.2 * 0.4, "None", false);
        }

        if !self.face_open {
            return ui::ROW_H;
        }
        let grid_y = y + ui::ROW_H;
        let files = self
            .face
            .as_ref()
            .map(|f| &f.files[..f.files.len().min(FACE_MAX_THUMBS)])
            .unwrap_or(&[]);
        if files.is_empty() {
            draw::text_max(
                cr,
                body.0 + 8.0,
                grid_y + (ui::ROW_H - 15.0) / 2.0,
                (body.2 - 16.0).max(20.0),
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "No pictures in ~/Pictures",
                },
            );
            return ui::ROW_H * 2.0;
        }
        let (cols, cell) = Self::face_grid_geom(cx, body.2);
        let thumbs = self.face.as_ref().map(|f| f.thumbs.as_slice());
        for idx in 0..files.len() {
            let row = idx / cols;
            let col = idx % cols;
            let tx = body.0 + THUMB_PAD_X + col as f64 * (cell + THUMB_GAP);
            let ty = grid_y + THUMB_TOP_PAD + row as f64 * (cell + THUMB_GAP);
            let cell_r = (tx, ty, cell, cell);
            let id = FACE_THUMB + idx as u32;
            let hover = hits.add(id, cell_r);
            let sel = hits.selected(id);

            let _ = cr.save();
            draw::round_rect(cr, tx, ty, cell, cell, THUMB_RADIUS);
            cr.clip();
            if let Some(surf) = thumbs
                .and_then(|t| t.get(idx))
                .and_then(|o| o.as_ref())
                .and_then(thumb_surface)
            {
                let sw = surf.width() as f64;
                let sh = surf.height() as f64;
                if (sw - cell).abs() > 1.0 || (sh - cell).abs() > 1.0 {
                    cr.translate(tx, ty);
                    cr.scale(cell / sw, cell / sh);
                    let _ = cr.set_source_surface(&surf, 0.0, 0.0);
                } else {
                    let _ = cr.set_source_surface(&surf, tx, ty);
                }
                let _ = cr.paint();
            } else {
                draw::set(cr, cx.theme.track.alpha(0.28));
                let _ = cr.paint();
            }
            if hover {
                draw::set(cr, cx.theme.text.alpha(ui::LIGHT_ALPHA));
                let _ = cr.paint();
            }
            let _ = cr.restore();

            if sel {
                cr.set_line_width(2.0);
                draw::set(cr, cx.theme.accent);
                draw::round_rect(cr, tx, ty, cell, cell, THUMB_RADIUS);
                let _ = cr.stroke();
            }
        }
        self.face_h(cx, body.2)
    }
}

impl Card for LookCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        match key {
            "config" => {
                let Some(d) = data.downcast_ref::<ConfigData>() else {
                    return false;
                };
                self.pending.retain(|k, v| d.value(k) != v.as_str());
                let changed = self.config.as_ref() != Some(d);
                self.config = Some(d.clone());
                if self.accent_drag.is_none() && self.accent_preview.is_none() {
                    self.sync_accent_hsv();
                }
                changed
            }
            "look" => {
                let Some(d) = data.downcast_ref::<LookData>() else {
                    return false;
                };
                let changed = self.look.as_ref() != Some(d);
                self.look = Some(d.clone());
                changed
            }
            "wallpapers" => {
                let Some(d) = data.downcast_ref::<WallpapersData>() else {
                    return false;
                };
                let changed = self.wallpapers.as_ref() != Some(d);
                self.wallpapers = Some(d.clone());
                changed
            }
            "face" => {
                let Some(d) = data.downcast_ref::<FaceData>() else {
                    return false;
                };
                let changed = self.face.as_ref() != Some(d) || self.face_preview.is_some();
                self.face = Some(d.clone());
                self.face_preview = None;
                changed
            }
            "fonts" | "fonts-mono" | "icons" | "cursors" => {
                let Some(d) = data.downcast_ref::<FoundNames>() else {
                    return false;
                };
                let slot = match key {
                    "fonts" => "fonts",
                    "fonts-mono" => "fonts-mono",
                    "icons" => "icons",
                    "cursors" => "cursors",
                    _ => unreachable!(),
                };
                let changed = self.found.get(slot) != Some(&d.0);
                self.found.insert(slot, d.0.clone());
                changed
            }
            _ => false,
        }
    }

    fn status(&self, cx: &Cx) -> Status {
        if !self.ready_for_status() {
            Status::Loading
        } else if self.shown(cx).0.is_empty() && !self.show_face_row(cx) {
            Status::Hidden
        } else {
            Status::Ready
        }
    }

    fn height(&self, cx: &Cx, width: f64) -> f64 {
        let (rows, more) = self.shown(cx);
        rows.iter().map(|&i| self.row_h(cx, i, width)).sum::<f64>()
            + self.face_h(cx, width)
            + if more { ui::ROW_H } else { 0.0 }
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let (rows, more) = self.shown(cx);
        let mut y = body.1;
        let right = body.0 + body.2 - 8.0;
        let mut slides = HashMap::new();
        for i in rows {
            let s = &self.table[i];
            let id = ROW + 10 * i as u32;
            let h = self.row_h(cx, i, body.2);
            let r = (body.0, y, body.2, ui::ROW_H);
            let (state, state_alert) = self.state(s);
            let changed = !self.is_default(i);
            let name_w = draw::measure(
                cr,
                &Text {
                    size: 13.0,
                    bold: false,
                    color: cx.theme.text,
                    text: s.name,
                },
            )
            .0;
            let max_val_w = (body.2 - name_w - 32.0).max(body.2 * 0.4);
            match s.control {
                LookControl::Wallpapers => {
                    self.draw_wallpaper_grid(cr, cx, hits, body, y, i, id, changed);
                }
                LookControl::Switch { on, .. } => {
                    let is_on = self.value(i).eq_ignore_ascii_case(on);
                    ui::row_alert(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: false,
                            right: 46.0,
                        },
                        state_alert,
                    );
                    ui::switch(cr, cx, hits, id + 1, r, right, is_on);
                }
                LookControl::Pick(_) | LookControl::Found { .. } => {
                    let entries = if self.pick == Some(i) && self.list.shown() {
                        self.list_entries(i)
                    } else {
                        Vec::new()
                    };
                    let vis = entries.len().clamp(1, MAX_LIST_ROWS);
                    let (row_r, list_r) = if self.pick == Some(i) && self.list.shown() {
                        self.list.split((body.0, y, body.2, h), vis, cx.list_above)
                    } else {
                        (r, (0.0, 0.0, 0.0, 0.0))
                    };
                    let word = self.display_word(i);
                    let vw = draw::measure(
                        cr,
                        &Text {
                            size: 12.0,
                            bold: false,
                            color: cx.theme.muted,
                            text: &word,
                        },
                    )
                    .0
                    .min(max_val_w)
                        + 40.0;
                    let lit = ui::row_alert(
                        cr,
                        cx,
                        hits,
                        id,
                        row_r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: false,
                            right: vw,
                        },
                        state_alert,
                    );
                    let open = self.pick == Some(i) && self.list.shown();
                    ui::value_box(cr, cx, row_r, right, &word, lit, open);
                    if self.pick == Some(i) && self.list.shown() {
                        self.draw_list(cr, cx, hits, list_r, &entries);
                    }
                }
                LookControl::Slide { min, max, unit, .. } => {
                    let v: f64 = self.value(i).parse().unwrap_or(min);
                    let label = format!("{}{unit}", fmt_number(v));
                    ui::row_alert(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: false,
                            right: 60.0,
                        },
                        state_alert,
                    );
                    value_at_right_capped(cr, cx, r, right, 80.0, &label, changed);
                    let sr = (body.0 + 8.0, y + ui::ROW_H, body.2 - 16.0, SLIDE_H - 4.0);
                    let part = if max > min {
                        (v - min) / (max - min)
                    } else {
                        0.0
                    };
                    ui::slider(cr, cx, hits, id + 1, sr, part);
                    slides.insert(i, sr);
                }
                LookControl::Step { .. } => {
                    ui::row_alert(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: false,
                            right: 96.0,
                        },
                        state_alert,
                    );
                    let v = self.value(i);
                    ui::button(cr, cx, hits, id + 2, right - 26.0, y + 7.0, "+");
                    value_at_right_capped(cr, cx, r, right - 32.0, 34.0, &v, changed);
                    ui::button(cr, cx, hits, id + 1, right - 90.0, y + 7.0, "−");
                }
                LookControl::Accent => {
                    let full_h = self.accent_picker_full_h(cx, body.2);
                    let open = self.pick == Some(i) && self.list.shown();
                    let (row_r, list_r) = if open {
                        self.list
                            .split_for((body.0, y, body.2, h), full_h, cx.list_above)
                    } else {
                        (r, (0.0, 0.0, 0.0, 0.0))
                    };
                    let raw_val = self.value(i);
                    let rgb = parse_hex(&raw_val);
                    let shown = if raw_val.is_empty() {
                        "Not set".to_string()
                    } else {
                        raw_val
                    };
                    let swatch_space = if rgb.is_some() { SWATCH + 8.0 } else { 0.0 };
                    let text_w = draw::measure(
                        cr,
                        &Text {
                            size: 12.0,
                            bold: false,
                            color: cx.theme.muted,
                            text: &shown,
                        },
                    )
                    .0
                    .min(max_val_w);
                    let reserve = (text_w + swatch_space + 44.0).min(body.2 * 0.65);
                    let lit = ui::row_alert(
                        cr,
                        cx,
                        hits,
                        id,
                        row_r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: false,
                            right: reserve,
                        },
                        state_alert,
                    );
                    let box_w = ui::value_box(cr, cx, row_r, right, &shown, lit, open);
                    if let Some(rgb) = rgb {
                        draw_swatch(cr, cx, right - box_w - 8.0 - SWATCH, row_r, rgb);
                    }
                    if open {
                        self.draw_accent_picker(cr, cx, hits, list_r);
                    }
                }
            }
            y += h;
        }
        if self.show_face_row(cx) {
            y += self.draw_face_section(cr, cx, hits, body, y);
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

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Text(q) => {
                if self.query == *q {
                    return false;
                }
                self.query = (*q).to_string();
                if let Some(p) = self.pick {
                    let (rows, _) = self.shown(cx);
                    if !rows.contains(&p) {
                        self.pick = None;
                        self.list.open = false;
                        self.list.settle();
                    }
                }
                true
            }
            Event::Scroll { dy, .. } => {
                let Some(i) = self.pick else {
                    return false;
                };
                if !self.list.open {
                    return false;
                }
                let total = self.list_entries(i).len();
                let max_scroll = total.saturating_sub(MAX_LIST_ROWS);
                if max_scroll == 0 {
                    return false;
                }
                let cur = self.list_scroll.min(max_scroll);
                let next = if *dy > 0.0 {
                    (cur + 1).min(max_scroll)
                } else if *dy < 0.0 {
                    cur.saturating_sub(1)
                } else {
                    return false;
                };
                if next == self.list_scroll {
                    return false;
                }
                self.list_scroll = next;
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
                if hit == FACE_ROW {
                    if !self.show_face_row(cx) {
                        return false;
                    }
                    self.face_open = !self.face_open;
                    if self.face_open {
                        self.pick = None;
                        self.list.open = false;
                        self.wp_mode_open = false;
                    }
                    return true;
                }
                if (FACE_THUMB..FACE_THUMB + FACE_MAX_THUMBS as u32).contains(&hit) {
                    if !self.face_open || !self.show_face_row(cx) {
                        return false;
                    }
                    let idx = (hit - FACE_THUMB) as usize;
                    let Some(face) = &self.face else {
                        return false;
                    };
                    let Some(file) = face.files.get(idx) else {
                        return false;
                    };
                    let path = file.path.clone();
                    self.face_preview = face.thumbs.get(idx).and_then(|o| o.clone());
                    self.face_open = false;
                    out.push(Action::Run(face_command(&path)));
                    return true;
                }
                if hit >= PICKER {
                    if !self.accent_picker_open() {
                        return false;
                    }
                    match hit {
                        H_TAB_WP => {
                            self.accent_tab = AccentTab::Wallpaper;
                            self.wp_mode_open = false;
                            return true;
                        }
                        H_TAB_ANY => {
                            self.accent_tab = AccentTab::AnyColor;
                            self.wp_mode_open = false;
                            if self.accent_preview.is_none() {
                                self.sync_accent_hsv();
                            }
                            return true;
                        }
                        H_SHUFFLE => {
                            out.push(Action::Run(shuffle_command().to_string()));
                            return true;
                        }
                        H_WP_MODE_BOX => {
                            self.wp_mode_open = !self.wp_mode_open;
                            return true;
                        }
                        H_HUE => {
                            let rect = *self.hue_rect.borrow();
                            let Some(r) = rect else { return false };
                            let h = ui::slider_part(r, *at) * 360.0;
                            self.accent_hsv.0 = h;
                            self.accent_drag = Some(AccentDrag::Hue);
                            self.accent_preview = Some(hsv_to_hex(
                                self.accent_hsv.0,
                                self.accent_hsv.1,
                                self.accent_hsv.2,
                            ));
                            return true;
                        }
                        H_SV => {
                            let rect = *self.sv_rect.borrow();
                            let Some(r) = rect else { return false };
                            let s = ui::slider_part(r, *at);
                            let v = if r.3 > 0.0 {
                                1.0 - ((at.1 - r.1) / r.3).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            self.accent_hsv.1 = s;
                            self.accent_hsv.2 = v;
                            self.accent_drag = Some(AccentDrag::Sv);
                            self.accent_preview = Some(hsv_to_hex(
                                self.accent_hsv.0,
                                self.accent_hsv.1,
                                self.accent_hsv.2,
                            ));
                            return true;
                        }
                        H_CODE_ASK => {
                            let r = self.table[ACCENT_ROW];
                            let now = self.value(ACCENT_ROW);
                            out.push(Action::Run(self.color_ask_command(&r, &now)));
                            return true;
                        }
                        h if (H_MARK_BASE..H_MARK_BASE + MAX_ACCENT_CANDIDATES as u32)
                            .contains(&h) =>
                        {
                            let idx = (h - H_MARK_BASE) as usize;
                            let hex = self
                                .look
                                .as_ref()
                                .and_then(|l| l.accent_candidates.get(idx))
                                .map(|c| c.hex.clone());
                            let Some(hex) = hex else { return false };
                            self.set_accent(hex, out);
                            return true;
                        }
                        h if (H_CAND_SQ_BASE..H_CAND_SQ_BASE + MAX_ACCENT_CANDIDATES as u32)
                            .contains(&h) =>
                        {
                            let idx = (h - H_CAND_SQ_BASE) as usize;
                            let hex = self
                                .look
                                .as_ref()
                                .and_then(|l| l.accent_candidates.get(idx))
                                .map(|c| c.hex.clone());
                            let Some(hex) = hex else { return false };
                            self.set_accent(hex, out);
                            return true;
                        }
                        h if (H_WP_MODE_OPT_BASE
                            ..H_WP_MODE_OPT_BASE + WP_MODE_OPTS.len() as u32)
                            .contains(&h) =>
                        {
                            let idx = (h - H_WP_MODE_OPT_BASE) as usize;
                            let (val, _) = WP_MODE_OPTS[idx];
                            self.wp_mode_open = false;
                            self.set(WP_MODE_ROW, val.to_string(), out);
                            return true;
                        }
                        h if (H_RECENT_BASE..H_RECENT_BASE + 16).contains(&h) => {
                            let idx = (h - H_RECENT_BASE) as usize;
                            let Some(hex) = self.recent_colors().get(idx).cloned() else {
                                return false;
                            };
                            self.set_accent(hex, out);
                            return true;
                        }
                        h if (H_SCHEME_BASE..H_SCHEME_BASE + SCHEME_OPTS.len() as u32)
                            .contains(&h) =>
                        {
                            let idx = (h - H_SCHEME_BASE) as usize;
                            let (val, _) = SCHEME_OPTS[idx];
                            self.set(SCHEME_ROW, val.to_string(), out);
                            return true;
                        }
                        _ => return false,
                    }
                }
                if hit >= LIST {
                    let Some(i) = self.pick else {
                        return false;
                    };
                    let entries = self.list_entries(i);
                    let Some(entry) = entries.get((hit - LIST) as usize) else {
                        return false;
                    };
                    self.list.open = false;
                    match &entry.action {
                        ListChoiceAction::Set(val) => {
                            self.set(i, val.clone(), out);
                        }
                        ListChoiceAction::AskOther => {
                            let r = self.table[i];
                            let now = self.value(i);
                            out.push(Action::Run(self.ask_text_command(&r, &now)));
                        }
                    }
                    return true;
                }
                if !(ROW..MORE).contains(&hit) {
                    return false;
                }
                if (THUMB..THUMB + MAX_WALLPAPERS as u32).contains(&hit) {
                    let idx = (hit - THUMB) as usize;
                    let Some(path) = self
                        .wallpapers
                        .as_ref()
                        .and_then(|w| w.files.get(idx))
                        .map(|f| f.path.clone())
                    else {
                        return false;
                    };
                    let Some(i) = self
                        .table
                        .iter()
                        .position(|r| matches!(r.control, LookControl::Wallpapers))
                    else {
                        return false;
                    };
                    if !self.row_visible(&self.table[i]) {
                        return false;
                    }
                    self.list.open = false;
                    self.list.settle();
                    return self.set(i, path, out);
                }
                let i = ((hit - ROW) / 10) as usize;
                let part = (hit - ROW) % 10;
                let Some(r) = self.table.get(i).copied() else {
                    return false;
                };
                if !self.row_visible(&r) {
                    return false;
                }
                match r.control {
                    LookControl::Switch { on, off } => {
                        let is_on = self.value(i).eq_ignore_ascii_case(on);
                        self.set(i, if is_on { off } else { on }.to_string(), out);
                        true
                    }
                    LookControl::Wallpapers => {
                        if part == 0 && self.has_more_wallpapers() {
                            self.open_pick(i);
                            true
                        } else {
                            false
                        }
                    }
                    LookControl::Pick(_) | LookControl::Found { .. } => {
                        self.open_pick(i);
                        true
                    }
                    LookControl::Slide { min, max, step, .. } if part == 1 => {
                        let rect = self.slide_rects.borrow().get(&i).copied();
                        let Some(rect) = rect else { return false };
                        let v = snap(
                            min + ui::slider_part(rect, *at) * (max - min),
                            min,
                            max,
                            step,
                        );
                        self.sliding = Some((i, v));
                        true
                    }
                    LookControl::Step { min, max } if part == 1 || part == 2 => {
                        let v: i64 = self.value(i).parse().unwrap_or(min);
                        let n = if part == 1 { v - 1 } else { v + 1 }.clamp(min, max);
                        self.set(i, n.to_string(), out)
                    }
                    LookControl::Accent => {
                        self.open_accent_pick(i);
                        true
                    }
                    _ => false,
                }
            }
            Event::Drag { hit, at } => {
                if let Some(drag) = self.accent_drag {
                    let rect = match drag {
                        AccentDrag::Hue if *hit == H_HUE => *self.hue_rect.borrow(),
                        AccentDrag::Sv if *hit == H_SV => *self.sv_rect.borrow(),
                        _ => return false,
                    };
                    let Some(rect) = rect else { return false };
                    match drag {
                        AccentDrag::Hue => {
                            self.accent_hsv.0 = ui::slider_part(rect, *at) * 360.0;
                        }
                        AccentDrag::Sv => {
                            self.accent_hsv.1 = ui::slider_part(rect, *at);
                            self.accent_hsv.2 = if rect.3 > 0.0 {
                                1.0 - ((at.1 - rect.1) / rect.3).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                        }
                    }
                    self.accent_preview = Some(hsv_to_hex(
                        self.accent_hsv.0,
                        self.accent_hsv.1,
                        self.accent_hsv.2,
                    ));
                    return true;
                }
                let Some((i, _)) = self.sliding else {
                    return false;
                };
                if *hit != ROW + 10 * i as u32 + 1 {
                    return false;
                }
                let LookControl::Slide { min, max, step, .. } = self.table[i].control else {
                    return false;
                };
                let rect = self.slide_rects.borrow().get(&i).copied();
                let Some(rect) = rect else { return false };
                let v = snap(
                    min + ui::slider_part(rect, *at) * (max - min),
                    min,
                    max,
                    step,
                );
                self.sliding = Some((i, v));
                true
            }
            // Sliders and the Any-colour picker write once on release: one log entry, one apply.
            Event::Release { .. } => {
                if self.accent_drag.take().is_some() {
                    if let Some(hex) = self.accent_preview.take() {
                        self.set_accent(hex, out);
                    }
                    return true;
                }
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
        if self.kind == LookKind::Appearance {
            if std::env::var_os("INFYNITAE_EDGE_RENDER_FACE")
                .is_some_and(|v| v == "1" || v == "true")
            {
                self.face_open = true;
                return;
            }
            if let Ok(tab) = std::env::var("INFYNITAE_EDGE_RENDER_ACCENT_TAB") {
                if tab == "wallpaper" || tab == "any" {
                    self.open_accent_pick(ACCENT_ROW);
                    self.accent_tab = if tab == "any" {
                        AccentTab::AnyColor
                    } else {
                        AccentTab::Wallpaper
                    };
                    self.list.open = true;
                    self.list.settle();
                    return;
                }
            }
        }
        if let Some(i) = self.table.iter().position(|r| {
            self.row_visible(r)
                && match r.control {
                    LookControl::Pick(_) | LookControl::Found { .. } => true,
                    LookControl::Wallpapers => self.has_more_wallpapers(),
                    _ => false,
                }
        }) {
            self.pick = Some(i);
            self.list.open = true;
            self.list.settle();
            self.list_scroll = 0;
        }
    }

    fn panel_closed(&mut self) {
        self.face_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_rows::{parse_config, tests as row_tests};
    use crate::theme::Theme;
    use cairo::{Format, ImageSurface};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TMP_SEQ: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let n = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!(
                "infynitae-look-test-{}-{tag}-{n}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn shipped_defaults() -> HashMap<String, String> {
        let text = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
            .unwrap();
        parse_config(&text)
    }

    fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    fn test_theme() -> Theme {
        Theme::from_config(&crate::config::Config::from_text(""))
    }

    fn draw_card(c: &dyn Card, cx: &Cx, width: f64) -> (Hits, f64) {
        let h = c.height(cx, width);
        let surf = ImageSurface::create(
            Format::ARgb32,
            width.ceil() as i32,
            h.ceil().max(1.0) as i32,
        )
        .unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, cx, (0.0, 0.0, width, h), &mut hits);
        (hits, h)
    }

    fn cfg(now: &[(&str, &str)], progs: &[&str]) -> ConfigData {
        row_tests::data(
            now,
            &[
                ("COLOR_SCHEME", "prefer-dark"),
                ("ACCENT", "#7aa2f7"),
                ("THEME_FROM_WALLPAPER", "false"),
                ("WINDOW_CORNERS", "8"),
                ("THEME_SCHEME", "yours"),
                ("CARD_OPACITY", "90"),
                ("SURFACE_COLOR", "#111317"),
                ("TEXT_COLOR", "#eef2f8"),
                ("BORDER_WIDTH", "1"),
                ("BORDER_INACTIVE", "#333b54"),
                ("CANVAS_COLOR", "#0b0f14"),
                ("FOLDER_COLOR", "accent"),
                ("THEME_SYNC", "true"),
                ("WALLPAPER", ""),
                ("WALLPAPER_MODE", "fill"),
                ("WALLPAPER_PARALLAX", "off"),
                ("FONT_NAME", "sans-serif"),
                ("FONT_SIZE", "10"),
                ("ICON_THEME", "Adwaita"),
                ("CURSOR_THEME", "Adwaita"),
                ("CURSOR_SIZE", "24"),
                ("TERMINAL_FONT", ""),
            ],
            progs,
        )
    }

    fn init_appearance(now: &[(&str, &str)], wallpaper_bin: bool, bad: &[&str]) -> LookCard {
        let mut c = LookCard::appearance();
        c.apply("config", &cfg(now, &[]));
        c.apply(
            "look",
            &LookData {
                wallpaper_bin,
                bad_colors: bad.iter().map(|s| (*s).to_string()).collect(),
                ..LookData::default()
            },
        );
        c
    }

    fn init_wallpaper(now: &[(&str, &str)], has_bin: bool, files: &[(&str, &str)]) -> LookCard {
        let mut c = LookCard::wallpaper();
        c.apply("config", &cfg(now, &[]));
        c.apply(
            "wallpapers",
            &WallpapersData {
                has_bin,
                files: files
                    .iter()
                    .map(|(name, path)| WallpaperFile {
                        name: (*name).to_string(),
                        path: (*path).to_string(),
                    })
                    .collect(),
                ..WallpapersData::default()
            },
        );
        c
    }

    fn init_fonts(now: &[(&str, &str)], progs: &[&str]) -> LookCard {
        let mut c = LookCard::fonts();
        c.apply("config", &cfg(now, progs));
        c.apply(
            "fonts",
            &FoundNames(vec![
                "sans-serif".into(),
                "monospace".into(),
                "DejaVu Sans".into(),
                "Noto Sans".into(),
            ]),
        );
        c.apply(
            "fonts-mono",
            &FoundNames(vec!["DejaVu Sans Mono".into(), "Liberation Mono".into()]),
        );
        c.apply(
            "icons",
            &FoundNames(vec!["Adwaita".into(), "Papirus-Dark".into()]),
        );
        c.apply("cursors", &FoundNames(vec!["Adwaita".into()]));
        c
    }

    #[test]
    fn every_key_is_in_config_config_with_a_value_its_control_has() {
        let def = shipped_defaults();
        assert_eq!(TABLE_APPEARANCE.len(), 8);
        for table in [TABLE_APPEARANCE, TABLE_WALLPAPER, TABLE_FONTS] {
            for r in table {
                let v = def
                    .get(r.key)
                    .unwrap_or_else(|| panic!("{} not in config/config", r.key));
                match r.control {
                    LookControl::Accent => {
                        assert!(parse_hex(v).is_some(), "{} = {v} is not #rrggbb", r.key);
                    }
                    LookControl::Found { .. } | LookControl::Wallpapers => {
                        if r.key == "WALLPAPER" || r.key == "TERMINAL_FONT" {
                            assert!(v.is_empty(), "{} = {v}", r.key);
                        } else {
                            assert!(!v.is_empty(), "{} default should not be empty", r.key);
                        }
                    }
                    _ => match r.as_control().unwrap() {
                        Control::Switch { on, off } => {
                            assert!(v == on || v == off, "{} = {v}", r.key)
                        }
                        Control::Pick(o) => {
                            assert!(o.iter().any(|(x, _)| x == v), "{} = {v}", r.key)
                        }
                        Control::Slide { min, max, .. } => {
                            let n: f64 = v.parse().unwrap_or_else(|_| panic!("{} = {v}", r.key));
                            assert!((min..=max).contains(&n), "{} = {n}", r.key);
                        }
                        Control::Step { min, max } => {
                            let n: i64 = v.parse().unwrap_or_else(|_| panic!("{} = {v}", r.key));
                            assert!((min..=max).contains(&n), "{} = {n}", r.key);
                        }
                        Control::Text => unreachable!(),
                    },
                }
            }
        }
    }

    #[test]
    fn parses_fonts_from_usual_empty_and_broken_fc_list_output() {
        let usual = "Noto Sans Malayalam,Noto Sans Malayalam Light\n\
                     DejaVu Sans\n\
                     Noto Sans Malayalam\n\
                     monospace\n\
                     Adwaita Sans,Adwaita Sans Bold\n";
        assert_eq!(
            parse_fonts(usual, &["sans-serif", "monospace"]),
            vec![
                "sans-serif",
                "monospace",
                "Adwaita Sans",
                "DejaVu Sans",
                "Noto Sans Malayalam",
            ]
        );
        assert_eq!(
            parse_fonts("Liberation Mono,Bold\nDejaVu Sans Mono\n", &[]),
            vec!["DejaVu Sans Mono", "Liberation Mono"]
        );
        assert!(parse_fonts("", &["sans-serif", "monospace"]).is_empty());

        let broken = "\n   \n,AltOnly\n  ,  \nInter , Inter Bold\r\n";
        assert_eq!(parse_fonts(broken, &[]), vec!["Inter"]);
    }

    #[test]
    fn scans_icons_and_cursors_from_usual_empty_and_broken_folders() {
        let root = TempDir::new("icons");
        let sys = root.0.join("sys");
        let user = root.0.join("user");
        let empty = root.0.join("empty");
        fs::create_dir_all(&empty).unwrap();

        // Usual icon+cursor theme, Papirus-Dark, and user override duplicate
        fs::create_dir_all(sys.join("Adwaita/cursors")).unwrap();
        fs::write(
            sys.join("Adwaita/index.theme"),
            "[Icon Theme]\nName=Adwaita\nHidden=true\nDirectories=16x16/apps,scalable/places\n",
        )
        .unwrap();
        fs::create_dir_all(sys.join("Papirus-Dark")).unwrap();
        fs::write(
            sys.join("Papirus-Dark/index.theme"),
            "[Icon Theme]\r\nDirectories=16x16/places,24x24/apps\r\n",
        )
        .unwrap();
        fs::create_dir_all(user.join("Adwaita/cursors")).unwrap();
        fs::write(
            user.join("Adwaita/index.theme"),
            "[Icon Theme]\nDirectories=16x16/apps\n",
        )
        .unwrap();

        // Infynitae-* generated theme must not appear in Icons
        fs::create_dir_all(user.join("Infynitae-Papirus-Dark")).unwrap();
        fs::write(
            user.join("Infynitae-Papirus-Dark/index.theme"),
            "[Icon Theme]\nDirectories=16x16/places\n",
        )
        .unwrap();

        // Cursor-only theme: no icon Directories= (or only Directories=cursors)
        fs::create_dir_all(sys.join("Bibata-Cursor/cursors")).unwrap();
        fs::write(
            sys.join("Bibata-Cursor/index.theme"),
            "[Icon Theme]\nName=Bibata\nInherits=hicolor\n",
        )
        .unwrap();
        fs::create_dir_all(sys.join("CursorsOnlyDir/cursors")).unwrap();
        fs::write(
            sys.join("CursorsOnlyDir/index.theme"),
            "[Icon Theme]\nDirectories=cursors\n",
        )
        .unwrap();

        // Broken entries: invalid UTF-8 index.theme, missing Directories, "cursors" as a regular file
        fs::create_dir_all(sys.join("BrokenTheme")).unwrap();
        fs::write(sys.join("BrokenTheme/index.theme"), [0xff, 0xfe, 0x00]).unwrap();
        fs::create_dir_all(sys.join("EmptyDirs")).unwrap();
        fs::write(
            sys.join("EmptyDirs/index.theme"),
            "[Icon Theme]\nDirectories= , \n",
        )
        .unwrap();
        fs::create_dir_all(sys.join("NotACursorDir")).unwrap();
        fs::write(sys.join("NotACursorDir/cursors"), b"file").unwrap();
        fs::create_dir_all(sys.join(".hidden/cursors")).unwrap();

        let roots = vec![
            sys.clone(),
            user.clone(),
            empty.clone(),
            root.0.join("missing"),
        ];
        assert_eq!(scan_icons(&roots), vec!["Adwaita", "Papirus-Dark"]);
        assert_eq!(
            scan_cursors(&roots),
            vec!["Adwaita", "Bibata-Cursor", "CursorsOnlyDir"]
        );
        assert!(scan_icons(std::slice::from_ref(&empty)).is_empty());
        assert!(scan_cursors(&[empty]).is_empty());
    }

    #[test]
    fn scans_wallpapers_sorted_decodes_up_to_24_and_handles_empty_and_broken() {
        let root = TempDir::new("wallpapers");
        let w1 = root.0.join("Pictures/Wallpapers");
        let w2 = root.0.join(".local/share/wallpapers");
        fs::create_dir_all(&w1).unwrap();
        fs::create_dir_all(&w2).unwrap();

        // Empty folder
        let mut empty_src = WallpapersSource {
            home: root.0.clone(),
            path: String::new(),
        };
        let empty_data = empty_src.fetch().downcast::<WallpapersData>().unwrap();
        assert!(empty_data.files.is_empty());
        assert!(empty_data.thumbs_s.is_empty());
        assert!(scan_wallpapers(&[w1.clone(), root.0.join("missing")]).is_empty());

        // Write 26 valid PNGs + 1 broken image + non-image files
        let make_png = |path: &Path| {
            let s = ImageSurface::create(Format::ARgb32, 32, 18).unwrap();
            s.write_to_png(&mut fs::File::create(path).unwrap())
                .unwrap();
        };
        for n in 1..=25 {
            make_png(&w1.join(format!("img_{n:02}.png")));
        }
        make_png(&w2.join("a_top.png"));
        fs::write(w2.join("broken.webp"), b"not an image").unwrap();
        fs::write(w2.join("notes.txt"), b"").unwrap();
        fs::write(w2.join(".hidden.png"), b"").unwrap();
        fs::create_dir_all(w2.join("dir.png")).unwrap();

        let found = scan_wallpapers(&[w1, w2]);
        assert_eq!(found.len(), 27);
        assert_eq!(found[0].name, "a_top.png");
        assert_eq!(found[1].name, "broken.webp");
        assert_eq!(found[26].name, "img_25.png");

        let data = empty_src.fetch().downcast::<WallpapersData>().unwrap();
        assert_eq!(data.files.len(), 27);
        // Only the first 24 shown items are decoded into S/M thumbnails.
        assert_eq!(data.thumbs_s.len(), 24);
        assert_eq!(data.thumbs_m.len(), 24);
        assert!(data.thumbs_s[0].is_some());
        // Broken file stays None without failing valid neighbors.
        assert!(data.thumbs_s[1].is_none());
        assert!(data.thumbs_m[1].is_none());
        assert!(data.thumbs_s[2].is_some());
    }

    #[test]
    fn look_source_reads_wallpaper_bin_and_bad_color_markers() {
        let root = TempDir::new("look-src");
        let home = root.0.join("home");
        let bad = root.0.join("bad");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&bad).unwrap();

        let mut src = LookSource {
            home: home.clone(),
            bad_dir: bad.clone(),
        };
        let empty = src.fetch().downcast::<LookData>().unwrap();
        assert!(!empty.wallpaper_bin);
        assert!(empty.bad_colors.is_empty());

        fs::create_dir_all(home.join(".local/bin")).unwrap();
        fs::write(home.join(".local/bin/infynitae-wallpaper"), b"").unwrap();
        fs::write(bad.join("ACCENT"), b"").unwrap();
        // Subdirectories inside bad_dir must be ignored
        fs::create_dir_all(bad.join("dir_ignored")).unwrap();

        let full = src.fetch().downcast::<LookData>().unwrap();
        assert!(full.wallpaper_bin);
        assert_eq!(full.bad_colors, HashSet::from(["ACCENT".to_string()]));
    }

    #[test]
    fn hiding_rules_and_wallpaper_color_locks_work() {
        let th = test_theme();
        let cx_m = Cx {
            size: Size::M,
            ..test_cx(&th)
        };

        // Appearance: without wallpaper bin, THEME_FROM_WALLPAPER is hidden; without Papirus, FOLDER_COLOR is hidden
        let c1 = init_appearance(&[("ICON_THEME", "Adwaita")], false, &[]);
        let (rows1, _) = c1.shown(&cx_m);
        let keys1: Vec<&str> = rows1.iter().map(|&i| c1.table[i].key).collect();
        assert!(!keys1.contains(&"THEME_FROM_WALLPAPER"));
        assert!(!keys1.contains(&"FOLDER_COLOR"));

        // With wallpaper bin + Papirus-Dark, all 8 Appearance rows appear at size M
        let c2 = init_appearance(&[("ICON_THEME", "Papirus-Dark")], true, &[]);
        let (rows2, _) = c2.shown(&cx_m);
        let keys2: Vec<&str> = rows2.iter().map(|&i| c2.table[i].key).collect();
        assert!(keys2.contains(&"ACCENT"));
        assert!(keys2.contains(&"THEME_FROM_WALLPAPER"));
        assert!(keys2.contains(&"FOLDER_COLOR"));
        assert!(!keys2.contains(&"MOTION"));
        for file_only_key in [
            "BORDER_ACTIVE",
            "SURFACE_COLOR",
            "TEXT_COLOR",
            "BORDER_INACTIVE",
            "CANVAS_COLOR",
            "THEME_SYNC",
        ] {
            assert!(!keys2.contains(&file_only_key), "{file_only_key}");
        }

        // THEME_FROM_WALLPAPER="accent" / "all" / "shuffle" leave ACCENT and THEME_SCHEME clickable
        for mode in ["accent", "all", "shuffle"] {
            let mut c_wp = init_appearance(&[("THEME_FROM_WALLPAPER", mode)], true, &[]);
            let (hits_wp, _) = draw_card(&c_wp, &cx_m, 404.0);
            let accent_hit = ROW + 10; // ACCENT is row 1
            assert!(hits_wp.list.iter().any(|(id, _)| *id == accent_hit));
            assert!(hits_wp.list.iter().any(|(id, _)| *id == ROW + 40)); // THEME_SCHEME
            let mut out = Vec::new();
            assert!(c_wp.event(&press(accent_hit), &cx_m, &mut out));
            assert!(c_wp.accent_picker_open());
            assert!(out.is_empty());
        }

        // Wallpaper card hidden without infynitae-wallpaper, ready with it
        let wp_hidden = init_wallpaper(&[], false, &[]);
        assert_eq!(wp_hidden.status(&cx_m), Status::Hidden);
        let wp_ready = init_wallpaper(&[], true, &[]);
        assert_eq!(wp_ready.status(&cx_m), Status::Ready);

        // Fonts card hides Font and Terminal font without fc-list
        let fonts_no_fc = init_fonts(&[], &[]);
        let (frows, _) = fonts_no_fc.shown(&cx_m);
        let fkeys: Vec<&str> = frows.iter().map(|&i| fonts_no_fc.table[i].key).collect();
        assert!(!fkeys.contains(&"FONT_NAME"));
        assert!(!fkeys.contains(&"TERMINAL_FONT"));
        assert!(fkeys.contains(&"ICON_THEME"));
        assert!(fkeys.contains(&"CURSOR_THEME"));
    }

    #[test]
    fn exact_commands_for_every_control_kind() {
        let th = test_theme();
        let cx_m = Cx {
            size: Size::M,
            ..test_cx(&th)
        };
        let cx_s = test_cx(&th);

        // 1. A short list: COLOR_SCHEME Dark -> Light (open the row, choose the second)
        let mut c = init_appearance(&[("ICON_THEME", "Papirus-Dark")], true, &[]);
        let mut out = Vec::new();
        assert!(c.event(&press(ROW), &cx_m, &mut out));
        assert!(out.is_empty());
        assert!(c.event(&press(LIST + 1), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log COLOR_SCHEME - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set COLOR_SCHEME 'prefer-light' >/dev/null 2>&1"
                    .to_string()
            )]
        );
        assert_eq!(c.value(0), "prefer-light");

        // 2. Switch: FOLDER_COLOR (row 7) accent -> theme
        out.clear();
        assert!(c.event(&press(ROW + 7 * 10 + 1), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log FOLDER_COLOR - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set FOLDER_COLOR 'theme' >/dev/null 2>&1"
                    .to_string()
            )]
        );
        assert_eq!(c.value(7), "theme");

        // 3. Pick: THEME_SCHEME (row 4) -> Tonal (index 3)
        out.clear();
        assert!(c.event(&press(ROW + 4 * 10), &cx_m, &mut out));
        assert!(out.is_empty());
        assert!(c.event(&press(LIST + 3), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log THEME_SCHEME - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set THEME_SCHEME 'tonal' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // 4. Slide: WINDOW_CORNERS (row 3) emits nothing on drag, once on release
        draw_card(&c, &cx_m, 404.0);
        out.clear();
        let rect = *c.slide_rects.borrow().get(&3).unwrap();
        let mid_x = rect.0 + rect.2 / 2.0;
        let mid_y = rect.1 + rect.3 / 2.0;
        assert!(c.event(
            &Event::Press {
                hit: ROW + 31,
                button: Button::Left,
                at: (mid_x, mid_y),
            },
            &cx_m,
            &mut out,
        ));
        assert!(out.is_empty());
        assert!(c.event(
            &Event::Drag {
                hit: ROW + 31,
                at: (rect.0 + rect.2 * 0.75, mid_y),
            },
            &cx_m,
            &mut out,
        ));
        assert!(out.is_empty());
        assert_eq!(c.value(3), "18");
        assert!(c.event(&Event::Release { hit: ROW + 31 }, &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log WINDOW_CORNERS - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set WINDOW_CORNERS '18' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // 5. Step: BORDER_WIDTH (row 6) + -> 2
        out.clear();
        assert!(c.event(&press(ROW + 62), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log BORDER_WIDTH - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set BORDER_WIDTH '2' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // 6. Accent picker opens in place; Any-colour "Code" button (H_CODE_ASK) uses ask --text and checks #rrggbb in shell; bad flag shows state
        out.clear();
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        assert!(c.accent_picker_open());
        assert!(out.is_empty());
        assert!(c.event(&press(H_TAB_ANY), &cx_m, &mut out));
        assert!(c.event(&press(H_CODE_ASK), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "v=$(\"$HOME/.local/bin/infynitae-edge\" ask --text --default '#7aa2f7' 'Accent') || exit 0; \
                 bd=\"${XDG_RUNTIME_DIR:-$HOME/.cache}/infynitae/look-bad\"; \
                 case \"$v\" in \"#\"[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) \
                 rm -f \"$bd\"/'ACCENT'; \
                 sh \"$HOME/infynitae/live/settings.sh\" log ACCENT - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT \"$v\" >/dev/null 2>&1 ;; \
                 *) mkdir -p \"$bd\" && : > \"$bd\"/'ACCENT' ;; esac"
                    .to_string()
            )]
        );
        // Close accent picker so subsequent checks don't have it open, and check bad accent marker (new ACCENT or legacy BORDER_ACTIVE)
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        let c_bad = init_appearance(&[], true, &["ACCENT"]);
        assert_eq!(c_bad.state(&c_bad.table[1]), ("Not a colour", true));
        let c_bad_legacy =
            init_appearance(&[("BORDER_ACTIVE", "#112233")], true, &["BORDER_ACTIVE"]);
        assert_eq!(c_bad_legacy.value(ACCENT_ROW), "#112233");
        assert_eq!(
            c_bad_legacy.state(&c_bad_legacy.table[1]),
            ("Not a colour", true)
        );

        // 7. Wallpapers thumbnail grid: click thumbnail sets WALLPAPER; >24 shows More row opening remaining list + "The first..." + "Other file"
        let mut wp = init_wallpaper(
            &[("WALLPAPER", "/home/u/Pictures/Wallpapers/sky.png")],
            true,
            &[
                ("sky.png", "/home/u/Pictures/Wallpapers/sky.png"),
                (
                    "dawn's light.jpg",
                    "/home/u/Pictures/Wallpapers/dawn's light.jpg",
                ),
            ],
        );
        assert_eq!(wp.display_word(0), "sky.png");
        // No More row under grid when <= 24 wallpapers (pressing ROW on Picture does nothing).
        out.clear();
        assert!(!wp.event(&press(ROW), &cx_s, &mut out));
        let (hits_wp_s, _) = draw_card(&wp, &cx_s, 408.0);
        let (hits_wp_m, _) = draw_card(&wp, &cx_m, 420.0);
        let thumbs_s: Vec<(u32, Rect)> = hits_wp_s
            .list
            .iter()
            .filter(|(id, _)| (THUMB..THUMB + 24).contains(id))
            .copied()
            .collect();
        let thumbs_m: Vec<(u32, Rect)> = hits_wp_m
            .list
            .iter()
            .filter(|(id, _)| (THUMB..THUMB + 24).contains(id))
            .copied()
            .collect();
        assert_eq!(thumbs_s.len(), 2);
        assert_eq!(thumbs_m.len(), 2);
        // S is 2 across (y equal), M is 3 across; 16:9 ratio.
        assert_eq!(thumbs_s[0].1 .1, thumbs_s[1].1 .1);
        assert!((thumbs_s[0].1 .2 / thumbs_s[0].1 .3 - 16.0 / 9.0).abs() < 0.05);
        assert!((thumbs_m[0].1 .2 / thumbs_m[0].1 .3 - 16.0 / 9.0).abs() < 0.05);
        // Click second thumbnail sets WALLPAPER with sq quoting.
        assert!(wp.event(&press(THUMB + 1), &cx_s, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(format!(
                "sh \"$HOME/infynitae/live/settings.sh\" log WALLPAPER '/home/u/Pictures/Wallpapers/sky.png' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set WALLPAPER {} >/dev/null 2>&1",
                sq("/home/u/Pictures/Wallpapers/dawn's light.jpg")
            ))]
        );
        assert_eq!(wp.display_word(0), "dawn's light.jpg");

        // Empty folder draws without panicking and matches card height.
        let wp_empty = init_wallpaper(&[], true, &[]);
        let (hits_empty, h_empty) = draw_card(&wp_empty, &cx_s, 408.0);
        assert!(h_empty >= 2.0 * ui::ROW_H);
        assert!(hits_empty
            .list
            .iter()
            .all(|(id, _)| !(THUMB..THUMB + MAX_WALLPAPERS as u32).contains(id)));

        // Grid with 26 wallpapers shows at most 24 thumbnails, then a "More" row (2 across at S -> 12 rows, 3 at M -> 8 rows).
        let many_files: Vec<(String, String)> = (1..=26)
            .map(|n| {
                (
                    format!("w{n:02}.png"),
                    format!("/home/u/Pictures/Wallpapers/w{n:02}.png"),
                )
            })
            .collect();
        let many_refs: Vec<(&str, &str)> = many_files
            .iter()
            .map(|(n, p)| (n.as_str(), p.as_str()))
            .collect();
        let mut wp26 = init_wallpaper(
            &[("WALLPAPER", "/home/u/Pictures/Wallpapers/w01.png")],
            true,
            &many_refs,
        );
        let (hits26_s, h26_s) = draw_card(&wp26, &cx_s, 408.0);
        let (hits26_m, h26_m) = draw_card(&wp26, &cx_m, 420.0);
        assert_eq!(
            hits26_s
                .list
                .iter()
                .filter(|(id, _)| (THUMB..THUMB + MAX_WALLPAPERS as u32).contains(id))
                .count(),
            24
        );
        // More row hit at ROW when > 24 exists at both S and M.
        assert!(hits26_s.list.iter().any(|(id, _)| *id == ROW));
        assert!(hits26_m.list.iter().any(|(id, _)| *id == ROW));
        // Row 0 thumbnail 0 and row 1 thumbnail 2 differ in y at S (2 across) vs thumbnail 2 on same row at M (3 across).
        let r0s = hits26_s.list.iter().find(|(id, _)| *id == THUMB).unwrap().1;
        let r2s = hits26_s
            .list
            .iter()
            .find(|(id, _)| *id == THUMB + 2)
            .unwrap()
            .1;
        let r0m = hits26_m.list.iter().find(|(id, _)| *id == THUMB).unwrap().1;
        let r2m = hits26_m
            .list
            .iter()
            .find(|(id, _)| *id == THUMB + 2)
            .unwrap()
            .1;
        assert!(r2s.1 > r0s.1);
        assert_eq!(r2m.1, r0m.1);
        assert!(h26_s > h26_m);

        // Open More row -> list has w25.png, w26.png, "The first...", "Other file".
        out.clear();
        assert!(wp26.event(&press(ROW), &cx_m, &mut out));
        let entries = wp26.list_entries(0);
        assert_eq!(entries[0].label, "w25.png");
        assert_eq!(entries[1].label, "w26.png");
        assert_eq!(entries[2].label, "The first in Pictures/Wallpapers");
        assert_eq!(entries[3].label, "Other file");
        // Choose w25.png -> writes w25 path.
        assert!(wp26.event(&press(LIST), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log WALLPAPER '/home/u/Pictures/Wallpapers/w01.png' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set WALLPAPER '/home/u/Pictures/Wallpapers/w25.png' >/dev/null 2>&1"
                    .to_string()
            )]
        );
        // Re-open More and pick "Other file" -> ask --text with current value prefilled
        out.clear();
        assert!(wp26.event(&press(ROW), &cx_m, &mut out));
        assert!(wp26.event(&press(LIST + 3), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "v=$(\"$HOME/.local/bin/infynitae-edge\" ask --text --default '/home/u/Pictures/Wallpapers/w25.png' 'Picture') || exit 0; \
                 sh \"$HOME/infynitae/live/settings.sh\" log WALLPAPER '/home/u/Pictures/Wallpapers/w01.png' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set WALLPAPER \"$v\" >/dev/null 2>&1"
                    .to_string()
            )]
        );
    }

    #[test]
    fn height_matches_drawn_bounds_more_scroll_and_text_search() {
        let th = test_theme();
        let cx_s = test_cx(&th);
        let cx_m = Cx {
            size: Size::M,
            ..test_cx(&th)
        };

        let mut f = init_fonts(&[], &["fc-list"]);
        let (hits_s, h_s) = draw_card(&f, &cx_s, 404.0);
        let max_bottom_s = hits_s
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0_f64, f64::max);
        assert!((h_s - max_bottom_s).abs() < 1.0);

        let mut out = Vec::new();
        assert!(f.event(&press(MORE), &cx_s, &mut out));
        let h_more = f.height(&cx_s, 404.0);
        assert!(h_more > h_s);
        assert_eq!(f.display_word(5), "Same as Font");

        // Text search filters rows across More and hides card on no match
        assert!(f.event(&Event::Text("cursor"), &cx_s, &mut out));
        assert_eq!(f.shown(&cx_s).0.len(), 2);
        assert!(f.event(&Event::Text("zzzznotfound"), &cx_s, &mut out));
        assert_eq!(f.status(&cx_s), Status::Hidden);
        assert!(f.event(&Event::Text(""), &cx_s, &mut out));
        assert_eq!(f.status(&cx_s), Status::Ready);

        // Long list caps visible viewport at MAX_LIST_ROWS and scrolls inside the list
        let many: Vec<String> = (0..15).map(|i| format!("Font {i:02}")).collect();
        f.apply("fonts", &FoundNames(many));
        f.show_list();
        let (hits_list, h_list) = draw_card(&f, &cx_m, 404.0);
        let list_hits: Vec<u32> = hits_list
            .list
            .iter()
            .filter_map(|(id, _)| (*id >= LIST).then_some(*id))
            .collect();
        assert_eq!(list_hits.len(), MAX_LIST_ROWS);
        let max_bottom_l = hits_list
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0_f64, f64::max);
        assert!((h_list - max_bottom_l).abs() < 1.0);
        assert!(f.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_m, &mut out));
        assert_eq!(f.list_scroll, 1);

        // When the bottom-most row's list opens and finishes animating, list box bottom sits 4 px inside height
        assert!(f.event(&press(ROW + 50), &cx_m, &mut out));
        assert!(f.tick(500.0));
        assert!(!f.tick(10.0));
        let (hits_last, h_last) = draw_card(&f, &cx_m, 404.0);
        let max_bottom_last = hits_last
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0_f64, f64::max);
        assert!((h_last - (max_bottom_last + 4.0)).abs() < 1.0);
    }

    #[test]
    fn color_shell_command_validates_hex_and_refuses_injection() {
        let root = TempDir::new("color-sh");
        let home = root.0.join("home");
        let xdg = root.0.join("xdg");
        let bin = home.join(".local/bin");
        let live = home.join("infynitae/live");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&xdg).unwrap();

        let calls = root.0.join("calls");
        let answer = root.0.join("answer");
        let pwn = root.0.join("pwn");
        fs::write(
            bin.join("infynitae-edge"),
            format!("#!/bin/sh\ncat {}\n", sq(&answer.to_string_lossy())),
        )
        .unwrap();
        fs::write(
            live.join("settings.sh"),
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n",
                sq(&calls.to_string_lossy())
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            bin.join("infynitae-edge"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fs::set_permissions(live.join("settings.sh"), fs::Permissions::from_mode(0o755)).unwrap();

        let mut c = init_appearance(&[], true, &[]);
        let th = test_theme();
        let cx_m = test_cx(&th);
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        assert!(c.event(&press(H_TAB_ANY), &cx_m, &mut out));
        assert!(c.event(&press(H_CODE_ASK), &cx_m, &mut out));
        let Action::Run(cmd) = out.pop().unwrap() else {
            panic!("expected Run action");
        };

        let run_with_answer = |ans: &str| {
            fs::write(&answer, ans).unwrap();
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .env("HOME", &home)
                .env("XDG_RUNTIME_DIR", &xdg)
                .status()
                .unwrap()
        };

        // Invalid input containing shell metacharacters is refused without setting or executing
        let evil = format!("$(touch {}); notacolour", pwn.display());
        assert!(run_with_answer(&evil).success());
        assert!(!pwn.exists());
        assert!(!calls.exists());
        let bad_dir = xdg.join("infynitae/look-bad");
        assert!(bad_dir.join("ACCENT").is_file());

        let mut src = LookSource {
            home: home.clone(),
            bad_dir: bad_dir.clone(),
        };
        let d1 = src.fetch().downcast::<LookData>().unwrap();
        assert!(d1.bad_colors.contains("ACCENT"));

        // Valid #rrggbb clears the marker and runs log + set
        assert!(run_with_answer("#22aa44").success());
        assert!(!bad_dir.join("ACCENT").exists());
        let d2 = src.fetch().downcast::<LookData>().unwrap();
        assert!(!d2.bad_colors.contains("ACCENT"));
        let logged = fs::read_to_string(&calls).unwrap();
        assert_eq!(logged, "log ACCENT -\nset ACCENT #22aa44\n");
    }

    #[test]
    fn provider_registers_look_cards_and_sources() {
        let p = crate::provider::by_id("settings").unwrap();
        for kind in ["appearance", "wallpaper", "fonts"] {
            assert!(p.kind(kind).is_some(), "missing {kind}");
            assert!(p.card(kind, None).is_some(), "missing card {kind}");
        }
        for key in [
            "look",
            "face",
            "wallpapers",
            "fonts",
            "fonts-mono",
            "icons",
            "cursors",
        ] {
            let src = p
                .source(key)
                .unwrap_or_else(|| panic!("missing source {key}"));
            assert_eq!(src.every(), None, "{key} every() should be None");
        }
    }

    #[test]
    fn parses_accent_candidates_and_ignores_malformed() {
        let raw = "\n\
                   #7AA2F7 0.125 0.875\n\
                   malformed\n\
                   #ff00aa 0 1\n\
                   #gggggg 0.5 0.5\n\
                   #fff 0.5 0.5\n\
                   #112233 -0.01 0.5\n\
                   #112233 0.5 1.01\n\
                   #112233 NaN 0.5\n\
                   #112233 0.5 0.5 extra\n\
                   #223344 0.25\n\
                   #00aabb 0.500 0.250\n\
                   #334455 0.750 0.750\n\
                   #445566 0.100 0.200\n\
                   #556677 0.900 0.900\n\
                   #667788 0.400 0.400\n";
        let cands = parse_accent_lines(raw);
        assert_eq!(cands.len(), MAX_ACCENT_CANDIDATES);
        assert_eq!(cands[0].hex, "#7aa2f7");
        assert_eq!(cands[0].rgb, [0x7a, 0xa2, 0xf7]);
        assert!((cands[0].x - 0.125).abs() < 1e-6);
        assert!((cands[0].y - 0.875).abs() < 1e-6);
        assert_eq!(cands[1].hex, "#ff00aa");
        assert_eq!(cands[2].hex, "#00aabb");
        assert_eq!(cands[5].hex, "#556677");

        assert!(parse_accent_lines("").is_empty());
        assert!(parse_accent_lines("  \n#nothex 0.5 0.5\n").is_empty());
    }

    #[test]
    fn nudges_close_candidate_marks_without_leaving_bounds() {
        let bounds = (10.0, 20.0, 300.0, 180.0);
        let mut pts = vec![(50.0, 50.0), (50.0, 50.0), (54.0, 52.0), (250.0, 140.0)];
        nudge_marks(&mut pts, bounds, MARK_MIN_DIST);
        for i in 0..pts.len() {
            let (px, py) = pts[i];
            assert!(px >= bounds.0 && px <= bounds.0 + bounds.2);
            assert!(py >= bounds.1 && py <= bounds.1 + bounds.3);
            for (j, &(qx, qy)) in pts.iter().enumerate().skip(i + 1) {
                let d = (qx - px).hypot(qy - py);
                assert!(d + 1e-3 >= MARK_MIN_DIST, "pair ({i},{j}) d={d}");
            }
        }
        // Far-apart marks stay where they were.
        let mut far = vec![(40.0, 40.0), (200.0, 150.0)];
        nudge_marks(&mut far, bounds, MARK_MIN_DIST);
        assert_eq!(far, vec![(40.0, 40.0), (200.0, 150.0)]);
    }

    #[test]
    fn hsv_rgb_hex_round_trip_and_drag_preview_writes_once_on_release() {
        for sample in [
            "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#00ffff", "#ff00ff",
            "#7aa2f7", "#3b82f6", "#d97706", "#a855f7", "#1e293b", "#808080",
        ] {
            let rgb = parse_hex(sample).unwrap();
            let (h, s, v) = rgb_to_hsv(rgb);
            let back = hsv_to_rgb(h, s, v);
            assert_eq!(back, rgb, "round trip failed for {sample} -> {h},{s},{v}");
            assert_eq!(hsv_to_hex(h, s, v), sample);
        }

        let th = test_theme();
        let cx_m = Cx {
            size: Size::M,
            ..test_cx(&th)
        };
        let mut c = init_appearance(&[("ACCENT", "#7aa2f7")], true, &[]);
        c.look.as_mut().unwrap().accent_candidates =
            parse_accent_lines("#e05d44 0.2 0.2\n#44cc88 0.8 0.8\n");

        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        assert!(c.event(&press(H_TAB_ANY), &cx_m, &mut out));
        assert!(c.tick(500.0));
        draw_card(&c, &cx_m, 404.0);
        let hr = c.hue_rect.borrow().unwrap();
        let sr = c.sv_rect.borrow().unwrap();

        // Press and Drag on Hue strip and SV square change live value without emitting any command.
        out.clear();
        assert!(c.event(
            &Event::Press {
                hit: H_HUE,
                button: Button::Left,
                at: (hr.0 + hr.2 * 0.25, hr.1 + hr.3 / 2.0),
            },
            &cx_m,
            &mut out,
        ));
        assert!(out.is_empty());
        assert!(c.event(
            &Event::Drag {
                hit: H_HUE,
                at: (hr.0 + hr.2 / 3.0, hr.1 + hr.3 / 2.0),
            },
            &cx_m,
            &mut out,
        ));
        assert!(out.is_empty());
        assert!(c.event(&Event::Release { hit: H_HUE }, &cx_m, &mut out,));
        assert_eq!(out.len(), 1);
        let (_, orig_s, orig_v) = rgb_to_hsv(parse_hex("#7aa2f7").unwrap());
        assert_eq!(c.value(ACCENT_ROW), hsv_to_hex(120.0, orig_s, orig_v));
        assert!(c.accent_preview.is_none());

        out.clear();
        assert!(c.event(
            &Event::Press {
                hit: H_SV,
                button: Button::Left,
                at: (sr.0 + sr.2 * 0.5, sr.1 + sr.3 * 0.5),
            },
            &cx_m,
            &mut out,
        ));
        assert!(c.event(
            &Event::Drag {
                hit: H_SV,
                at: (sr.0 + sr.2, sr.1 + sr.3), // v = 0 -> black
            },
            &cx_m,
            &mut out,
        ));
        assert_eq!(c.value(ACCENT_ROW), "#000000");
        assert!(out.is_empty());
        assert!(c.event(&Event::Release { hit: H_SV }, &cx_m, &mut out));
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0],
            Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log ACCENT '#7aa2f7' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT '#000000' >/dev/null 2>&1"
                    .to_string()
            )
        );
        // Pressing + dragging and releasing on the current value emits no command.
        out.clear();
        assert!(c.event(
            &Event::Press {
                hit: H_SV,
                button: Button::Left,
                at: (sr.0 + sr.2 * 0.5, sr.1 + sr.3),
            },
            &cx_m,
            &mut out,
        ));
        assert!(c.event(&Event::Release { hit: H_SV }, &cx_m, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn reads_recent_accents_from_temporary_log() {
        let log = "\
100\tFONT_NAME\tSans\n\
101\tACCENT\t-\n\
102\tBORDER_ACTIVE\t#111111\n\
103\tACCENT\tnotacolor\n\
104\tBORDER_ACTIVE\t#222222\n\
105\tBORDER_ACTIVE\t#333333\n\
106\tACCENT\t#444444\n\
107\tACCENT\t#555555\n\
108\tBORDER_ACTIVE\t#666666\n\
109\tACCENT\t#777777\n\
110\tACCENT\t#222222\tEXTRA\n\
111\tBORDER_ACTIVE\t#888888\n\
112\tACCENT\t#999999\n\
113\tACCENT\t#888888\n";
        let parsed = parse_recent_accents(log);
        assert_eq!(
            parsed,
            vec![
                "#888888", "#999999", "#777777", "#666666", "#555555", "#444444", "#333333",
                "#222222"
            ]
        );

        let root = TempDir::new("recent-log");
        let state = root.0.join(".local/state/infynitae");
        fs::create_dir_all(&state).unwrap();
        fs::write(state.join("settings-changes"), log).unwrap();

        let mut src = LookSource {
            home: root.0.clone(),
            bad_dir: root.0.join("bad"),
        };
        let look = src.fetch().downcast::<LookData>().unwrap();
        assert_eq!(look.recent_accents, parsed);

        let mut c = init_appearance(&[("ACCENT", "#ABCDEF")], true, &[]);
        c.look = Some(*look);
        let colors = c.recent_colors();
        assert_eq!(colors.len(), 9);
        assert_eq!(colors[0], "#abcdef");
        assert_eq!(colors[1], "#888888");
        assert_eq!(colors[8], "#222222");
    }

    #[test]
    fn accent_picker_commands_quote_values_and_height_matches_drawn_bounds() {
        let th = test_theme();
        let cx_s = test_cx(&th);
        let cx_m = Cx {
            size: Size::M,
            ..test_cx(&th)
        };

        let mut c = init_appearance(
            &[("ACCENT", "#11111b"), ("SURFACE_COLOR", "#11111b")],
            true,
            &[],
        );
        {
            let look = c.look.as_mut().unwrap();
            look.accent_candidates = parse_accent_lines(
                "#e05d44 0.150 0.250\n#44cc88 0.850 0.750\n#3b82f6 0.500 0.500\n",
            );
            look.recent_accents = vec!["#e05d44".to_string(), "#44cc88".to_string()];
        }

        // Open accent picker (defaults to "From your wallpaper" when candidates exist).
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        assert!(c.accent_picker_open());
        assert_eq!(c.accent_tab, AccentTab::Wallpaper);
        assert!(c.tick(500.0));

        // While Accent picker is open without search, outer THEME_FROM_WALLPAPER and THEME_SCHEME are hidden.
        let shown_keys: Vec<&str> = c.shown(&cx_m).0.iter().map(|&i| c.table[i].key).collect();
        assert!(!shown_keys.contains(&"THEME_FROM_WALLPAPER"));
        assert!(!shown_keys.contains(&"THEME_SCHEME"));

        // Verify height == drawn bounds on Wallpaper tab (S and M, with and without WP dropdown open).
        for (label, cx_cur) in [("wp-s", &cx_s), ("wp-m", &cx_m)] {
            let (hits, h) = draw_card(&c, cx_cur, 404.0);
            let max_bottom = hits
                .list
                .iter()
                .map(|(_, r)| r.1 + r.3)
                .fold(0.0_f64, f64::max);
            assert!(
                (h - max_bottom).abs() < 1.0,
                "{label}: h={h} max_bottom={max_bottom}"
            );
            assert!(hits.list.iter().any(|(id, _)| *id == H_MARK_BASE));
            assert!(hits.list.iter().any(|(id, _)| *id == H_CAND_SQ_BASE));
            assert!(hits.list.iter().any(|(id, _)| *id == H_SHUFFLE));
            assert!(hits.list.iter().any(|(id, _)| *id == H_WP_MODE_BOX));
        }

        // Open Colours-from-wallpaper inline dropdown and choose "shuffle" ("A new accent each time").
        out.clear();
        assert!(c.event(&press(H_WP_MODE_BOX), &cx_m, &mut out));
        let (hits_dd, h_dd) = draw_card(&c, &cx_m, 404.0);
        let max_bottom_dd = hits_dd
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0_f64, f64::max);
        assert!((h_dd - max_bottom_dd).abs() < 1.0);
        assert!(hits_dd
            .list
            .iter()
            .any(|(id, _)| *id == H_WP_MODE_OPT_BASE + 3));
        assert!(c.event(&press(H_WP_MODE_OPT_BASE + 3), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log THEME_FROM_WALLPAPER - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set THEME_FROM_WALLPAPER 'shuffle' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Shuffle button runs wallpaper-colors shuffle.
        out.clear();
        assert!(c.event(&press(H_SHUFFLE), &cx_m, &mut out));
        assert_eq!(out, vec![Action::Run(shuffle_command().to_string())]);

        // Click candidate mark and candidate square -> change_command with sq quoting.
        out.clear();
        assert!(c.event(&press(H_MARK_BASE + 1), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log ACCENT '#11111b' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT '#44cc88' >/dev/null 2>&1"
                    .to_string()
            )]
        );
        out.clear();
        assert!(c.event(&press(H_CAND_SQ_BASE + 2), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log ACCENT '#11111b' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT '#3b82f6' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Style scheme words set THEME_SCHEME.
        out.clear();
        assert!(c.event(&press(H_SCHEME_BASE + 5), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log THEME_SCHEME - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set THEME_SCHEME 'expressive' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Switch to Any colour tab with Recent + unreadably dark accent (#11111b on #11111b).
        out.clear();
        assert!(c.event(&press(H_TAB_ANY), &cx_m, &mut out));
        assert!(c.event(&press(H_RECENT_BASE + 1), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log ACCENT '#11111b' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT '#e05d44' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Force unreadable current accent to verify warning line height at S and M.
        c.pending.insert("ACCENT", "#11111b".to_string());
        assert!(c.accent_adjusted_warning(&cx_m));
        for (label, cx_cur) in [("any-s", &cx_s), ("any-m", &cx_m)] {
            let (hits_any, h_any) = draw_card(&c, cx_cur, 404.0);
            let max_bottom_any = hits_any
                .list
                .iter()
                .map(|(_, r)| r.1 + r.3)
                .fold(0.0_f64, f64::max);
            assert!(
                (h_any - max_bottom_any).abs() < 1.0,
                "{label}: h={h_any} max_bottom={max_bottom_any}"
            );
            assert!(hits_any.list.iter().any(|(id, _)| *id == H_SCHEME_BASE + 5));
        }

        // With 0 candidates, Accent opens directly on "Any colour", and Wallpaper tab shows exact bounds.
        let mut c_empty = init_appearance(&[("ACCENT", "#7aa2f7")], true, &[]);
        assert!(c_empty.event(&press(ROW + 10), &cx_m, &mut out));
        assert_eq!(c_empty.accent_tab, AccentTab::AnyColor);
        assert!(c_empty.event(&press(H_TAB_WP), &cx_m, &mut out));
        c_empty.tick(500.0);
        let (hits_e, h_e) = draw_card(&c_empty, &cx_m, 404.0);
        let max_bottom_e = hits_e
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0_f64, f64::max);
        assert!((h_e - max_bottom_e).abs() < 1.0);
    }

    #[test]
    fn style_row_uses_borderless_soft_fills_via_choice_button() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let opts = [("balanced", "Balanced"), ("dark", "Dark")];
        let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 320, 60).unwrap();
        let mut hits = Hits::new(None, None);
        {
            let cr = Context::new(&surf).unwrap();
            draw_style_row(
                &cr,
                &cx,
                &mut hits,
                H_SCHEME_BASE,
                (10.0, 10.0, 280.0, 28.0),
                &opts,
                Some(0),
            );
        }
        surf.flush();
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        let px = |x: usize, y: usize| -> [u8; 4] {
            let off = y * stride + x * 4;
            [data[off], data[off + 1], data[off + 2], data[off + 3]]
        };
        let b0 = hits.list[0].1;
        let b1 = hits.list[1].1;
        // Chosen pill top edge matches interior (no outline stroke) and differs from unchosen pill.
        let chosen_edge = px((b0.0 + 10.0) as usize, b0.1 as usize);
        let chosen_in = px((b0.0 + 10.0) as usize, (b0.1 + 2.0) as usize);
        let other_in = px((b1.0 + 10.0) as usize, (b1.1 + 2.0) as usize);
        assert!(chosen_in[3] > 0);
        assert!(other_in[3] > 0);
        assert_eq!(chosen_edge, chosen_in);
        assert_ne!(chosen_in, other_in);
    }

    #[test]
    fn legacy_border_active_feeds_accent_and_saves_accent_key() {
        let th = test_theme();
        let cx_m = Cx {
            size: Size::M,
            ..test_cx(&th)
        };
        let mut c = init_appearance(&[("BORDER_ACTIVE", "#cc70a9")], true, &[]);
        c.look.as_mut().unwrap().accent_candidates = parse_accent_lines("#44cc88 0.5 0.5\n");
        assert_eq!(c.value(ACCENT_ROW), "#cc70a9");

        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        assert!(c.event(&press(H_CAND_SQ_BASE), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log ACCENT - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT '#44cc88' >/dev/null 2>&1"
                    .to_string()
            )]
        );
    }

    #[test]
    fn picker_error_uses_theme_alert_not_accent() {
        let mut th1 = test_theme();
        th1.alert = Rgba::rgb([0xff, 0x22, 0x22]);
        let mut th2 = th1.clone();
        th2.alert = Rgba::rgb([0x22, 0xff, 0x22]);

        let render = |th: &Theme| -> Vec<u8> {
            let cx = Cx {
                size: Size::M,
                ..test_cx(th)
            };
            let mut c = init_appearance(&[], true, &["ACCENT"]);
            let mut out = Vec::new();
            assert!(c.event(&press(ROW + 10), &cx, &mut out));
            c.tick(500.0);
            let h = c.height(&cx, 404.0).ceil() as i32 + 10;
            let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 420, h).unwrap();
            let mut hits = Hits::new(None, None);
            {
                let cr = Context::new(&surf).unwrap();
                c.draw(&cr, &cx, (0.0, 0.0, 404.0, h as f64), &mut hits);
            }
            surf.flush();
            let bytes = surf.data().unwrap().to_vec();
            bytes
        };

        assert_ne!(render(&th1), render(&th2));
    }

    #[test]
    fn closed_bad_accent_row_renders_state_in_theme_alert_not_accent() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        assert!(th.alert.0 > th.alert.2 && th.accent_text.2 > th.accent_text.0);
        let cx = Cx {
            size: Size::M,
            ..test_cx(&th)
        };
        let c = init_appearance(&[], true, &["ACCENT"]);
        assert!(!c.accent_picker_open());

        let h = c.height(&cx, 404.0).ceil() as i32 + 10;
        let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 420, h).unwrap();
        let mut hits = Hits::new(None, None);
        {
            let cr = Context::new(&surf).unwrap();
            c.draw(&cr, &cx, (0.0, 0.0, 404.0, h as f64), &mut hits);
        }
        surf.flush();
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        // ACCENT is row 1: scan the lower half of row 1 on the left where "Not a colour" sits.
        let y0 = ui::ROW_H as usize + 22;
        let y1 = (2.0 * ui::ROW_H) as usize;
        let (mut red_dom, mut blue_dom) = (0usize, 0usize);
        for y in y0..y1 {
            for x in 0..200 {
                let off = y * stride + x * 4;
                let (b, _, r, a) = (data[off], data[off + 1], data[off + 2], data[off + 3]);
                if a > 40 {
                    if r > b + 20 {
                        red_dom += 1;
                    } else if b > r + 20 {
                        blue_dom += 1;
                    }
                }
            }
        }
        assert!(
            red_dom > 10,
            "expected alert pixels on Not a colour row, got {red_dom}"
        );
        assert_eq!(
            blue_dom, 0,
            "expected no accent blue pixels on Not a colour row"
        );
    }

    fn write_png_rgba(path: &Path, w: i32, h: i32, rgba: [u8; 4]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let surf = ImageSurface::create(Format::ARgb32, w, h).unwrap();
        {
            let cr = Context::new(&surf).unwrap();
            cr.set_source_rgba(
                rgba[0] as f64 / 255.0,
                rgba[1] as f64 / 255.0,
                rgba[2] as f64 / 255.0,
                rgba[3] as f64 / 255.0,
            );
            let _ = cr.paint();
        }
        let mut f = fs::File::create(path).unwrap();
        surf.write_to_png(&mut f).unwrap();
    }

    fn has_hit(hits: &Hits, id: u32) -> bool {
        hits.list.iter().any(|(h, _)| *h == id)
    }

    #[test]
    fn face_row_visibility_query_filter_and_panel_close() {
        let th = test_theme();
        let cx = test_cx(&th);

        // Wallpaper card never draws Your picture.
        let wall = init_wallpaper(&[], true, &[]);
        let (wall_hits, _) = draw_card(&wall, &cx, 404.0);
        assert!(!has_hit(&wall_hits, FACE_ROW));

        // Appearance shows Your picture at bottom; None when empty.
        let mut card = init_appearance(&[], true, &[]);
        assert!(!card.face_open);
        let (hits, h_closed) = draw_card(&card, &cx, 404.0);
        assert!(has_hit(&hits, FACE_ROW));

        // Clicking opens the empty grid with an honest empty state, and panel_closed closes it.
        let mut out = Vec::new();
        assert!(card.event(&press(FACE_ROW), &cx, &mut out));
        assert!(card.face_open);
        assert!(out.is_empty());
        let h_empty_open = card.height(&cx, 404.0);
        assert!((h_empty_open - (h_closed + ui::ROW_H)).abs() < 0.01);

        card.panel_closed();
        assert!(!card.face_open);

        // Query matching "picture" keeps the card visible with Your picture shown.
        assert!(card.event(&Event::Text("picture"), &cx, &mut out));
        assert_eq!(card.status(&cx), Status::Ready);
        let (q_hits, _) = draw_card(&card, &cx, 404.0);
        assert!(has_hit(&q_hits, FACE_ROW));

        // Non-matching query hides Appearance completely.
        assert!(card.event(&Event::Text("zzz-missing-zzz"), &cx, &mut out));
        assert_eq!(card.status(&cx), Status::Hidden);
    }

    #[test]
    fn face_source_scans_and_grid_picks_image_with_quoted_command() {
        let dir = TempDir::new("face-source");
        let home = &dir.0;
        write_png_rgba(&home.join(".face"), 32, 32, [30, 80, 200, 255]);
        let pic1 = home.join("Pictures/avatar 'one'.png");
        let pic2 = home.join("Pictures/Wallpapers/wide.png");
        write_png_rgba(&pic1, 64, 32, [220, 40, 40, 255]);
        write_png_rgba(&pic2, 32, 80, [40, 200, 60, 255]);

        let mut src = FaceSource { home: home.clone() };
        let data = src.fetch();
        let face = data.downcast_ref::<FaceData>().unwrap();
        assert!(face.current.is_some());
        assert_eq!(face.files.len(), 2);
        assert_eq!(face.thumbs.len(), 2);
        assert!(face.thumbs.iter().all(|t| t.is_some()));

        let th = test_theme();
        let cx = test_cx(&th);
        let mut card = init_appearance(&[], true, &[]);
        let h_no_face = card.height(&cx, 404.0);
        card.apply("face", &*data);
        let h_with_face = card.height(&cx, 404.0);
        assert!((h_no_face - h_with_face).abs() < 0.01);

        let mut out = Vec::new();
        assert!(card.event(&press(FACE_ROW), &cx, &mut out));
        assert!(card.face_open);

        let (hits, h_open) = draw_card(&card, &cx, 404.0);
        assert!(h_open > h_with_face + ui::ROW_H);
        assert!(has_hit(&hits, FACE_THUMB));
        assert!(has_hit(&hits, FACE_THUMB + 1));

        let pic1_idx = face
            .files
            .iter()
            .position(|f| f.path.contains("avatar 'one'.png"))
            .unwrap() as u32;
        assert!(card.event(&press(FACE_THUMB + pic1_idx), &cx, &mut out));
        assert!(!card.face_open);
        assert!(card.face_preview.is_some());
        assert_eq!(out.len(), 1);
        match &out[0] {
            Action::Run(cmd) => {
                assert!(cmd.contains("--crop-face"));
                assert!(cmd.contains(&sq(&pic1.to_string_lossy())));
                assert!(cmd.contains("log FACE"));
            }
            other => panic!("expected Action::Run, got {other:?}"),
        }
    }

    #[test]
    fn face_command_backs_up_existing_face_and_logs_undo_entry() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new("face-cmd");
        let home = &dir.0;
        let tricky = home.join("Pictures/it's \"me\" (1).png");
        write_png_rgba(&tricky, 100, 60, [200, 30, 50, 255]);

        let bin = dir.0.join("fake-edge");
        let log_file = dir.0.join("log.txt");
        fs::write(
            &bin,
            format!(
                "#!/bin/sh\nif [ \"$1\" != \"--crop-face\" ]; then exit 2; fi\nprintf 'crop:%s\\n' \"$2\" >> '{}'\ncp \"$2\" \"$3\"\n",
                log_file.display()
            ),
        )
        .unwrap();
        let mut perm = fs::metadata(&bin).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&bin, perm).unwrap();

        let sh_path = home.join("infynitae/live/settings.sh");
        fs::create_dir_all(sh_path.parent().unwrap()).unwrap();
        fs::write(
            &sh_path,
            format!(
                "#!/bin/sh\nprintf 'sh:%s %s %s\\n' \"$1\" \"$2\" \"$3\" >> '{}'\n",
                log_file.display()
            ),
        )
        .unwrap();

        let run_face = |src: &Path| {
            let cmd = face_command(&src.to_string_lossy());
            std::process::Command::new("sh")
                .args(["-c", &cmd])
                .env("HOME", home)
                .env("INFYNITAE_EDGE_APPS", &bin)
                .status()
                .unwrap()
        };

        // First set when ~/.face does not exist -> old is "-".
        assert!(run_face(&tricky).success());
        assert!(home.join(".face").is_file());
        assert!(!home.join(".face.bak").exists());
        let first_bytes = fs::read(home.join(".face")).unwrap();
        assert_eq!(first_bytes, fs::read(&tricky).unwrap());

        // Second set when ~/.face exists -> old backed up to ~/.face.bak and old is ".face.bak".
        let second = home.join("Pictures/second.png");
        write_png_rgba(&second, 40, 40, [20, 200, 40, 255]);
        assert!(run_face(&second).success());
        assert_eq!(fs::read(home.join(".face.bak")).unwrap(), first_bytes);
        assert_eq!(
            fs::read(home.join(".face")).unwrap(),
            fs::read(&second).unwrap()
        );

        let logged = fs::read_to_string(&log_file).unwrap();
        assert_eq!(
            logged,
            format!(
                "crop:{}\nsh:log FACE -\ncrop:{}\nsh:log FACE .face.bak\n",
                tricky.display(),
                second.display()
            )
        );
    }
}
