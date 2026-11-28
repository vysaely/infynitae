//! Settings: Look (`docs/EDGE_SETTINGS.md` section 4): `settings/appearance`, `settings/wallpaper`,
//! and `settings/fonts`. Left to the file: `BORDER_ACTIVE`, `SURFACE_COLOR`, `TEXT_COLOR`,
//! `BORDER_INACTIVE`, `CANVAS_COLOR`, `THEME_SYNC`, `CORNERS`, `SQUARE_CORNERS`,
//! `BORDER_IGNORE_CSD`, `SWITCH_LOOK`.

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

mod accent;
mod drawing;
mod sources;

use accent::hsv_to_hex;
use drawing::{draw_swatch, fmt_number, snap, thumb_cell_size, value_at_right_capped};
#[cfg(test)]
use sources::parse_accent_lines;
#[cfg(test)]
pub use sources::WallpaperFile;
use sources::{face_command, FaceData};
pub use sources::{
    CursorsSource, FaceSource, FontsSource, FoundNames, IconsSource, LookData, LookSource,
    WallpapersData, WallpapersSource,
};

pub const KIND_APPEARANCE: CardKind = CardKind {
    id: "appearance",
    title: "Appearance",
    about: "Colours, corners, dark or light look",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "look", "face", "icons"],
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
            LookControl::Accent => Some(Control::Text),
            LookControl::Step { .. } | LookControl::Found { .. } | LookControl::Wallpapers => None,
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
        name: "Placement",
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
            first: Some(("auto", "Automatic")),
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
    list_scroll_accum: f64,
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
    home: Option<PathBuf>,
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
            list_scroll_accum: 0.0,
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
            home: crate::paths::home(),
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
            HideRule::PapirusIcons => match self.key_value("ICON_THEME").as_str() {
                // as settings.sh resolves auto: Papirus-Dark (Papirus with prefer-light) when
                // installed
                "auto" | "" => {
                    let papirus = if self.key_value("COLOR_SCHEME") == "prefer-light" {
                        "Papirus"
                    } else {
                        "Papirus-Dark"
                    };
                    self.found
                        .get("icons")
                        .is_some_and(|names| names.iter().any(|n| n == papirus))
                }
                theme => theme.starts_with("Papirus"),
            },
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
                let cur_expanded = crate::paths::expand_home_with(&cur, self.home.as_deref())
                    .to_string_lossy()
                    .into_owned();
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
                    let cur_expanded = crate::paths::expand_home_with(&cur, self.home.as_deref())
                        .to_string_lossy()
                        .into_owned();
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
}

fn shuffle_command() -> &'static str {
    "sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" wallpaper-colors shuffle >/dev/null 2>&1"
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
            Event::Scroll { dy, notches, .. } => {
                let Some(i) = self.pick else {
                    return false;
                };
                if !self.list.open {
                    return false;
                }
                let total = self.list_entries(i).len();
                let max_scroll = total.saturating_sub(MAX_LIST_ROWS);
                crate::card::scroll_rows(
                    &mut self.list_scroll,
                    &mut self.list_scroll_accum,
                    max_scroll,
                    *dy,
                    *notches,
                )
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
            Event::Cancel { .. } => {
                let had_drag = self.accent_drag.take().is_some();
                self.accent_preview = None;
                self.sliding.take().is_some() || had_drag
            }
            Event::Step { hit, dir } if *dir != 0 => {
                let hit = *hit;
                if self.accent_picker_open() {
                    if hit == H_TAB_WP || hit == H_TAB_ANY {
                        let next = if *dir > 0 {
                            AccentTab::AnyColor
                        } else {
                            AccentTab::Wallpaper
                        };
                        if next == self.accent_tab {
                            return false;
                        }
                        self.accent_tab = next;
                        self.wp_mode_open = false;
                        if next == AccentTab::AnyColor && self.accent_preview.is_none() {
                            self.sync_accent_hsv();
                        }
                        return true;
                    }
                    if hit == H_HUE {
                        self.accent_hsv.0 =
                            (self.accent_hsv.0 + f64::from(*dir) * 10.0).rem_euclid(360.0);
                        let hex =
                            hsv_to_hex(self.accent_hsv.0, self.accent_hsv.1, self.accent_hsv.2);
                        self.set_accent(hex, out);
                        return true;
                    }
                    if (H_SCHEME_BASE..H_SCHEME_BASE + SCHEME_OPTS.len() as u32).contains(&hit) {
                        let cur = self.value(SCHEME_ROW);
                        let cur_idx = SCHEME_OPTS
                            .iter()
                            .position(|(v, _)| v.eq_ignore_ascii_case(&cur))
                            .unwrap_or((hit - H_SCHEME_BASE) as usize);
                        let next = (cur_idx as isize + isize::from(*dir))
                            .clamp(0, SCHEME_OPTS.len() as isize - 1)
                            as usize;
                        let (val, _) = SCHEME_OPTS[next];
                        return self.set(SCHEME_ROW, val.to_string(), out);
                    }
                }
                if !(ROW..MORE).contains(&hit) {
                    return false;
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
                    LookControl::Slide { min, max, step, .. } if part == 1 => {
                        let cur = self.value(i).parse::<f64>().unwrap_or(min);
                        let next = snap(cur + f64::from(*dir) * step, min, max, step);
                        self.set(i, fmt_number(next), out)
                    }
                    LookControl::Step { min, max } if part == 1 || part == 2 => {
                        let v: i64 = self.value(i).parse().unwrap_or(min);
                        let n = (v + i64::from(*dir)).clamp(min, max);
                        self.set(i, n.to_string(), out)
                    }
                    _ => false,
                }
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

    pub(super) struct TempDir(pub(super) PathBuf);

    impl TempDir {
        pub(super) fn new(tag: &str) -> TempDir {
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

    pub(super) fn shipped_defaults() -> HashMap<String, String> {
        let text = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
            .unwrap();
        parse_config(&text)
    }

    pub(super) fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    pub(super) fn test_theme() -> Theme {
        Theme::from_config(&crate::config::Config::from_text(""))
    }

    pub(super) fn draw_card(c: &dyn Card, cx: &Cx, width: f64) -> (Hits, f64) {
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

    pub(super) fn cfg(now: &[(&str, &str)], progs: &[&str]) -> ConfigData {
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

    pub(super) fn init_appearance(
        now: &[(&str, &str)],
        wallpaper_bin: bool,
        bad: &[&str],
    ) -> LookCard {
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

    pub(super) fn init_wallpaper(
        now: &[(&str, &str)],
        has_bin: bool,
        files: &[(&str, &str)],
    ) -> LookCard {
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

    pub(super) fn init_fonts(now: &[(&str, &str)], progs: &[&str]) -> LookCard {
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
                    LookControl::Step { min, max } => {
                        let n: i64 = v.parse().unwrap_or_else(|_| panic!("{} = {v}", r.key));
                        assert!((min..=max).contains(&n), "{} = {n}", r.key);
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
                        Control::Text => unreachable!(),
                    },
                }
            }
        }
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

        // ICON_THEME="auto": Folders in the accent shows when the Papirus set it picks is installed
        let found = |names: &[&str]| FoundNames(names.iter().map(|n| (*n).to_string()).collect());
        let mut auto_dark = init_appearance(&[("ICON_THEME", "auto")], false, &[]);
        auto_dark.apply("icons", &found(&["Adwaita", "Papirus-Dark"]));
        let (r, _) = auto_dark.shown(&cx_m);
        assert!(r.iter().any(|&i| auto_dark.table[i].key == "FOLDER_COLOR"));
        let mut auto_none = init_appearance(&[("ICON_THEME", "auto")], false, &[]);
        auto_none.apply("icons", &found(&["Adwaita"]));
        let (r, _) = auto_none.shown(&cx_m);
        assert!(!r.iter().any(|&i| auto_none.table[i].key == "FOLDER_COLOR"));
        let mut auto_light = init_appearance(
            &[("ICON_THEME", "auto"), ("COLOR_SCHEME", "prefer-light")],
            false,
            &[],
        );
        auto_light.apply("icons", &found(&["Adwaita", "Papirus-Dark"]));
        let (r, _) = auto_light.shown(&cx_m);
        assert!(!r.iter().any(|&i| auto_light.table[i].key == "FOLDER_COLOR"));

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
                "sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" log COLOR_SCHEME - && \
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" set COLOR_SCHEME 'prefer-light' >/dev/null 2>&1"
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
                "sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" log FOLDER_COLOR - && \
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" set FOLDER_COLOR 'theme' >/dev/null 2>&1"
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
                "sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" log THEME_SCHEME - && \
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" set THEME_SCHEME 'tonal' >/dev/null 2>&1"
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
                "sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" log WINDOW_CORNERS - && \
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" set WINDOW_CORNERS '18' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // 5. Step: BORDER_WIDTH (row 6) + -> 2
        out.clear();
        assert!(c.event(&press(ROW + 62), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" log BORDER_WIDTH - && \
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" set BORDER_WIDTH '2' >/dev/null 2>&1"
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
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" log ACCENT - && \
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" set ACCENT \"$v\" >/dev/null 2>&1 ;; \
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
                "sh \"${{INFYNITAE_DIR:-$HOME/infynitae}}/live/settings.sh\" log WALLPAPER '/home/u/Pictures/Wallpapers/sky.png' && \
                 sh \"${{INFYNITAE_DIR:-$HOME/infynitae}}/live/settings.sh\" set WALLPAPER {} >/dev/null 2>&1",
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
                "sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" log WALLPAPER '/home/u/Pictures/Wallpapers/w01.png' && \
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" set WALLPAPER '/home/u/Pictures/Wallpapers/w25.png' >/dev/null 2>&1"
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
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" log WALLPAPER '/home/u/Pictures/Wallpapers/w01.png' && \
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" set WALLPAPER \"$v\" >/dev/null 2>&1"
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
        assert!(f.event(
            &Event::Scroll {
                hit: None,
                dy: 36.0,
                notches: 1.0,
            },
            &cx_m,
            &mut out
        ));
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
                "sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" log ACCENT - && \
                 sh \"${INFYNITAE_DIR:-$HOME/infynitae}/live/settings.sh\" set ACCENT '#44cc88' >/dev/null 2>&1"
                    .to_string()
            )]
        );
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
            // grey edges on text: a CI machine whose fonts are set to subpixel smoothing gave the alert
            // text blue fringes (34 "accent" pixels) on 2026-11-14
            crate::draw::grey_text_edges(&cr);
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

    pub(super) fn write_png_rgba(path: &Path, w: i32, h: i32, rgba: [u8; 4]) {
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

    pub(super) fn has_hit(hits: &Hits, id: u32) -> bool {
        hits.list.iter().any(|(h, _)| *h == id)
    }
}
