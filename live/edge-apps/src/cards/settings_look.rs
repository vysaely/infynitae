//! Settings: Look (`docs/EDGE_SETTINGS.md` section 4): `settings/appearance`, `settings/wallpaper`,
//! and `settings/fonts`. Left to the file: `CORNERS`, `SQUARE_CORNERS`, `BORDER_IGNORE_CSD`,
//! `CLIP_TO_GEOMETRY`, `WINE_TRANSPARENCY`, `FORCE_TRANSPARENT_APPS`, `SWITCH_LOOK`.

use super::settings::sq;
use super::settings_rows::{change_command, After, ConfigData, Control};
use crate::card::{
    render_value, run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status,
};
use crate::config::parse_hex;
use crate::draw::{self, Text};
use crate::icons::{self, DecodedImage};
use crate::provider::{CardKind, Size};
use crate::theme::Rgba;
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const KIND_APPEARANCE: CardKind = CardKind {
    id: "appearance",
    title: "Appearance",
    about: "Colours, corners, dark or light look",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "look"],
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
const WORDS_H: f64 = 34.0;
const SLIDE_H: f64 = 22.0;
const SWATCH: f64 = 16.0;
const MAX_LIST_ROWS: usize = 8;
const MAX_WALLPAPERS: usize = 24;
const THUMB_PAD_X: f64 = 8.0;
const THUMB_GAP: f64 = 8.0;
const THUMB_TOP_PAD: f64 = 4.0;
const THUMB_RADIUS: f64 = 4.0;
const THUMB_NAME_H: f64 = 24.0;
const THUMB_S: (i32, i32) = (192, 108);
const THUMB_M: (i32, i32) = (129, 73);

/// Extra state for `settings/appearance`: wallpaper tool presence and invalid `#rrggbb` attempts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LookData {
    pub wallpaper_bin: bool,
    pub bad_colors: HashSet<String>,
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
            for line in raw.lines() {
                let l = line.trim();
                if let Some(v) = l.strip_prefix("wallpaper_bin=") {
                    out.wallpaper_bin = matches!(v, "1" | "true" | "yes" | "on");
                } else if let Some(k) = l.strip_prefix("bad=") {
                    if !k.is_empty() {
                        out.bad_colors.insert(k.to_string());
                    }
                } else if matches!(l, "1" | "true") {
                    out.wallpaper_bin = true;
                }
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
        let wallpaper_bin = self.home.join(".local/bin/infynitae-wallpaper").is_file();
        Box::new(LookData {
            wallpaper_bin,
            bad_colors,
        })
    }

    fn every(&self) -> Option<Duration> {
        None
    }
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
        // Require at least one non-empty directory entry that is not just "cursors".
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
    Words(&'static [(&'static str, &'static str)]),
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
    Color,
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
    pub fn as_control(&self) -> Option<Control> {
        match self.control {
            LookControl::Switch { on, off } => Some(Control::Switch { on, off }),
            LookControl::Pick(o) => Some(Control::Pick(o)),
            LookControl::Words(o) => Some(Control::Words(o)),
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
            LookControl::Color => Some(Control::Text),
            LookControl::Found { .. } | LookControl::Wallpapers => None,
        }
    }
}

pub const TABLE_APPEARANCE: &[LookRow] = &[
    LookRow {
        key: "COLOR_SCHEME",
        name: "Apps look",
        says: "",
        control: LookControl::Words(&[("prefer-dark", "Dark"), ("prefer-light", "Light")]),
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "BORDER_ACTIVE",
        name: "Accent",
        says: "",
        control: LookControl::Color,
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "THEME_FROM_WALLPAPER",
        name: "Colours from the wallpaper",
        says: "",
        control: LookControl::Pick(&[
            ("false", "Off"),
            ("accent", "The accent"),
            ("all", "Everything"),
        ]),
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
        name: "Panels' colours",
        says: "",
        control: LookControl::Pick(&[
            ("yours", "Yours"),
            ("monochrome", "Monochrome"),
            ("neutral", "Neutral"),
            ("tonal", "Tonal"),
            ("vibrant", "Vibrant"),
            ("expressive", "Expressive"),
        ]),
        after: After::Save,
        more: false,
        hide: HideRule::None,
    },
    LookRow {
        key: "CARD_OPACITY",
        name: "Cards",
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
        key: "SURFACE_COLOR",
        name: "Background",
        says: "",
        control: LookControl::Color,
        after: After::Save,
        more: true,
        hide: HideRule::None,
    },
    LookRow {
        key: "TEXT_COLOR",
        name: "Text",
        says: "",
        control: LookControl::Color,
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
        key: "BORDER_INACTIVE",
        name: "Other windows' border",
        says: "",
        control: LookControl::Color,
        after: After::Save,
        more: true,
        hide: HideRule::None,
    },
    LookRow {
        key: "CANVAS_COLOR",
        name: "Behind the windows",
        says: "",
        control: LookControl::Color,
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
    LookRow {
        key: "THEME_SYNC",
        name: "Apps follow these colours",
        says: "",
        control: LookControl::Switch {
            on: "true",
            off: "false",
        },
        after: After::Save,
        more: true,
        hide: HideRule::None,
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
    TABLE_APPEARANCE[8].key,
    TABLE_APPEARANCE[9].key,
    TABLE_APPEARANCE[10].key,
    TABLE_APPEARANCE[11].key,
    TABLE_APPEARANCE[12].key,
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
enum WallpaperColorLock {
    Off,
    Accent,
    All,
}

/// Shared card implementation for `settings/appearance`, `settings/wallpaper`, and `settings/fonts`.
pub struct LookCard {
    kind: LookKind,
    table: &'static [LookRow],
    config: Option<ConfigData>,
    look: Option<LookData>,
    wallpapers: Option<WallpapersData>,
    found: HashMap<&'static str, Vec<String>>,
    pending: HashMap<&'static str, String>,
    pick: Option<usize>,
    list: ui::List,
    list_scroll: usize,
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
            found: HashMap::new(),
            pending: HashMap::new(),
            pick: None,
            list: ui::List::default(),
            list_scroll: 0,
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
        self.config
            .as_ref()
            .map_or(String::new(), |d| d.value(key).to_string())
    }

    fn value(&self, i: usize) -> String {
        let r = &self.table[i];
        if let Some((_, v)) = self.sliding.filter(|(idx, _)| *idx == i) {
            return fmt_number(v);
        }
        self.key_value(r.key)
    }

    fn wallpaper_lock(&self) -> WallpaperColorLock {
        let v = self.key_value("THEME_FROM_WALLPAPER");
        if v.is_empty() || v.eq_ignore_ascii_case("false") {
            WallpaperColorLock::Off
        } else if v.eq_ignore_ascii_case("accent") {
            WallpaperColorLock::Accent
        } else {
            WallpaperColorLock::All
        }
    }

    fn is_locked(&self, key: &str) -> bool {
        if self.kind != LookKind::Appearance {
            return false;
        }
        match self.wallpaper_lock() {
            WallpaperColorLock::Off => false,
            WallpaperColorLock::Accent => key == "BORDER_ACTIVE",
            WallpaperColorLock::All => matches!(
                key,
                "BORDER_ACTIVE" | "SURFACE_COLOR" | "BORDER_INACTIVE" | "CANVAS_COLOR"
            ),
        }
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
        let mut rows = Vec::new();
        let mut hidden_more = false;
        for (i, r) in self.table.iter().enumerate() {
            if !self.row_visible(r) || (searching && !self.matches(r)) {
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
                        LookControl::Words(_) => WORDS_H,
                        LookControl::Slide { .. } => SLIDE_H,
                        _ => 0.0,
                    }
                    + if self.pick == Some(i) {
                        match self.table[i].control {
                            LookControl::Pick(_) | LookControl::Found { .. } => {
                                self.list.height(self.visible_list_rows(i))
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
            LookControl::Pick(opts) | LookControl::Words(opts) => opts
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
        r.control_is_color()
            && !self.is_locked(r.key)
            && self
                .look
                .as_ref()
                .is_some_and(|l| l.bad_colors.contains(r.key))
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
}

impl LookRow {
    fn control_is_color(&self) -> bool {
        matches!(self.control, LookControl::Color)
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
                draw::set(cr, cx.theme.text.alpha(0.08));
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
        } else if self.shown(cx).0.is_empty() {
            Status::Hidden
        } else {
            Status::Ready
        }
    }

    fn height(&self, cx: &Cx, width: f64) -> f64 {
        let (rows, more) = self.shown(cx);
        rows.iter().map(|&i| self.row_h(cx, i, width)).sum::<f64>()
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
            let (state, state_accent) = self.state(s);
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
                    ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: state_accent,
                            right: 46.0,
                        },
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
                    let lit = ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        row_r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: state_accent,
                            right: vw,
                        },
                    );
                    let open = self.pick == Some(i) && self.list.shown();
                    ui::value_box(cr, cx, row_r, right, &word, changed, lit, open);
                    if self.pick == Some(i) && self.list.shown() {
                        self.draw_list(cr, cx, hits, list_r, &entries);
                    }
                }
                LookControl::Words(opts) => {
                    ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: state_accent,
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
                LookControl::Slide { min, max, unit, .. } => {
                    let v: f64 = self.value(i).parse().unwrap_or(min);
                    let label = format!("{}{unit}", fmt_number(v));
                    ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: state_accent,
                            right: 60.0,
                        },
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
                    ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: state_accent,
                            right: 96.0,
                        },
                    );
                    let v = self.value(i);
                    ui::button(cr, cx, hits, id + 2, right - 26.0, y + 7.0, "+");
                    value_at_right_capped(cr, cx, r, right - 32.0, 34.0, &v, changed);
                    ui::button(cr, cx, hits, id + 1, right - 90.0, y + 7.0, "−");
                }
                LookControl::Color => {
                    let locked = self.is_locked(s.key);
                    let raw_val = self.value(i);
                    let rgb = parse_hex(&raw_val);
                    let shown = if locked {
                        "From the wallpaper".to_string()
                    } else if raw_val.is_empty() {
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
                    let reserve = (text_w + swatch_space + 14.0).min(body.2 * 0.65);
                    let mut no_hits = Hits::new(None, None);
                    let target_hits = if locked { &mut no_hits } else { &mut *hits };
                    ui::row(
                        cr,
                        cx,
                        target_hits,
                        id,
                        r,
                        &Row {
                            icon: None,
                            name: s.name,
                            state,
                            on: state_accent,
                            right: reserve,
                        },
                    );
                    let tw = value_at_right_capped(
                        cr,
                        cx,
                        r,
                        right,
                        (max_val_w - swatch_space).max(40.0),
                        &shown,
                        !locked && changed,
                    );
                    if let Some(rgb) = rgb {
                        draw_swatch(cr, cx, right - tw - 8.0 - SWATCH, r, rgb);
                    }
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
                if !self.row_visible(&r) || self.is_locked(r.key) {
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
                    LookControl::Words(opts) => {
                        let Some(j) = part.checked_sub(1) else {
                            return false;
                        };
                        let Some((val, _)) = opts.get(j as usize) else {
                            return false;
                        };
                        self.set(i, (*val).to_string(), out)
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
                    LookControl::Color => {
                        let now = self.value(i);
                        out.push(Action::Run(self.color_ask_command(&r, &now)));
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
            // Sliders write once on release: one log entry, one settings apply.
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
        if let Some(i) = self.table.iter().position(|r| {
            self.row_visible(r)
                && !self.is_locked(r.key)
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
                ("BORDER_ACTIVE", "#7aa2f7"),
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
        for table in [TABLE_APPEARANCE, TABLE_WALLPAPER, TABLE_FONTS] {
            for r in table {
                let v = def
                    .get(r.key)
                    .unwrap_or_else(|| panic!("{} not in config/config", r.key));
                match r.control {
                    LookControl::Color => {
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
                        Control::Pick(o) | Control::Words(o) => {
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
        // Broken file stays None cleanly without failing valid neighbors.
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
        fs::write(bad.join("BORDER_ACTIVE"), b"").unwrap();
        // Subdirectories inside bad_dir must be ignored
        fs::create_dir_all(bad.join("dir_ignored")).unwrap();

        let full = src.fetch().downcast::<LookData>().unwrap();
        assert!(full.wallpaper_bin);
        assert_eq!(
            full.bad_colors,
            HashSet::from(["BORDER_ACTIVE".to_string()])
        );
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

        // With wallpaper bin + Papirus-Dark, both appear
        let c2 = init_appearance(&[("ICON_THEME", "Papirus-Dark")], true, &[]);
        let (rows2, _) = c2.shown(&cx_m);
        let keys2: Vec<&str> = rows2.iter().map(|&i| c2.table[i].key).collect();
        assert!(keys2.contains(&"THEME_FROM_WALLPAPER"));
        assert!(keys2.contains(&"FOLDER_COLOR"));

        // THEME_FROM_WALLPAPER="accent" locks only BORDER_ACTIVE
        let mut c_accent = init_appearance(&[("THEME_FROM_WALLPAPER", "accent")], true, &[]);
        let (hits_acc, _) = draw_card(&c_accent, &cx_m, 404.0);
        let accent_hit = ROW + 10; // BORDER_ACTIVE is row 1
        assert!(hits_acc.list.iter().all(|(id, _)| *id != accent_hit));
        let mut out = Vec::new();
        assert!(!c_accent.event(&press(accent_hit), &cx_m, &mut out));
        assert!(out.is_empty());
        // SURFACE_COLOR (row 6) stays clickable
        assert!(hits_acc.list.iter().any(|(id, _)| *id == ROW + 60));

        // THEME_FROM_WALLPAPER="all" locks BORDER_ACTIVE, SURFACE_COLOR, BORDER_INACTIVE, CANVAS_COLOR; TEXT_COLOR stays unlocked
        let mut c_all = init_appearance(&[("THEME_FROM_WALLPAPER", "all")], true, &[]);
        let (hits_all, _) = draw_card(&c_all, &cx_m, 404.0);
        for locked_row in [1u32, 6, 9, 10] {
            let hid = ROW + 10 * locked_row;
            assert!(hits_all.list.iter().all(|(id, _)| *id != hid));
            assert!(!c_all.event(&press(hid), &cx_m, &mut out));
        }
        assert!(c_all.event(&press(ROW + 70), &cx_m, &mut out)); // TEXT_COLOR
        assert_eq!(out.len(), 1);

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

        // 1. Words: COLOR_SCHEME Dark -> Light
        let mut c = init_appearance(&[("ICON_THEME", "Papirus-Dark")], true, &[]);
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 2), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log COLOR_SCHEME - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set COLOR_SCHEME 'prefer-light' >/dev/null 2>&1"
                    .to_string()
            )]
        );
        assert_eq!(c.value(0), "prefer-light");

        // 2. Switch: THEME_SYNC (row 12) true -> false
        out.clear();
        assert!(c.event(&press(ROW + 12 * 10), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log THEME_SYNC - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set THEME_SYNC 'false' >/dev/null 2>&1"
                    .to_string()
            )]
        );

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

        // 5. Step: BORDER_WIDTH (row 8) + -> 2
        out.clear();
        assert!(c.event(&press(ROW + 82), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log BORDER_WIDTH - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set BORDER_WIDTH '2' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // 6. Color: BORDER_ACTIVE (row 1) uses ask --text and checks #rrggbb in shell; bad flag shows state
        out.clear();
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "v=$(\"$HOME/.local/bin/infynitae-edge\" ask --text --default '#7aa2f7' 'Accent') || exit 0; \
                 bd=\"${XDG_RUNTIME_DIR:-$HOME/.cache}/infynitae/look-bad\"; \
                 case \"$v\" in \"#\"[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) \
                 rm -f \"$bd\"/'BORDER_ACTIVE'; \
                 sh \"$HOME/infynitae/live/settings.sh\" log BORDER_ACTIVE - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set BORDER_ACTIVE \"$v\" >/dev/null 2>&1 ;; \
                 *) mkdir -p \"$bd\" && : > \"$bd\"/'BORDER_ACTIVE' ;; esac"
                    .to_string()
            )]
        );
        let c_bad = init_appearance(&[], true, &["BORDER_ACTIVE"]);
        assert_eq!(c_bad.state(&c_bad.table[1]), ("Not a colour", true));

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

        // Empty folder still draws cleanly without panicking and matches card height.
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
        assert!(bad_dir.join("BORDER_ACTIVE").is_file());

        let mut src = LookSource {
            home: home.clone(),
            bad_dir: bad_dir.clone(),
        };
        let d1 = src.fetch().downcast::<LookData>().unwrap();
        assert!(d1.bad_colors.contains("BORDER_ACTIVE"));

        // Valid #rrggbb clears the marker and runs log + set
        assert!(run_with_answer("#22aa44").success());
        assert!(!bad_dir.join("BORDER_ACTIVE").exists());
        let d2 = src.fetch().downcast::<LookData>().unwrap();
        assert!(!d2.bad_colors.contains("BORDER_ACTIVE"));
        let logged = fs::read_to_string(&calls).unwrap();
        assert_eq!(logged, "log BORDER_ACTIVE -\nset BORDER_ACTIVE #22aa44\n");
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
}
