//! Settings index (`settings/all`, docs/EDGE_SETTINGS.md section 4): one row per visible group
//! (Personalization merges Appearance, Wallpaper and Fonts, icons, cursor into one row whose open
//! body stacks their non-hidden bodies in place under small headings), or all matching child
//! cards one by one plus `"In the file"` (`config/config` keys not on any card) while searching.
//!
//! Hit ID scheme:
//! - `1..50` (`HIT_GROUP_BASE + group_idx`): group rows in GROUPS' order (`1` Personalization,
//!   then one row per other card: Display, Keyboard and mouse, Windows, Keys, Apps, Games, Lock
//!   and sleep, Privacy, Recording).
//! - `50` (`HIT_OPEN_CONFIG`): `"Open the config file"` row.
//! - `100..CHILD_STRIDE` (`HIT_FILE_BASE + idx`): `"In the file"` search rows.
//! - `CHILD_STRIDE * (child_idx + 1) + child_hit` (`CHILD_STRIDE = 100_000`): child hits routed to
//!   `CHILD_KINDS[child_idx]` so multi-child groups and search never collide even for children
//!   emitting IDs >= 1000 (`MORE = 9000`, `LIST = 10_000`, `FOUND_LIST = 20_000`).

use crate::card::{Action, Button, Card, Cx, Event, Hits, Rect, Status};
use crate::cards::{
    settings, settings_apps, settings_display, settings_games, settings_input, settings_keys,
    settings_lock, settings_look, settings_privacy, settings_recording, settings_rows,
    settings_windows,
};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Size};
use crate::ui::{self, Row};
use cairo::Context;
use settings_display::ScreensData;
use settings_input::TouchpadData;
use settings_lock::BatteryLimitData;
use settings_privacy::SharesData;
use settings_rows::ConfigData;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const EDIT_COMMAND: &str =
    r#"sh "$HOME/.local/bin/xdg-terminal-exec" sh "$HOME/infynitae/live/settings.sh" edit"#;
const PLUS_LINE_EDITORS: &[&str] = &["nano", "vim", "nvim", "vi", "micro"];

const HIT_GROUP_BASE: u32 = 1;
const HIT_OPEN_CONFIG: u32 = 50;
const HIT_FILE_BASE: u32 = 100;
const CHILD_STRIDE: u32 = 100_000;

const HEADER_H: f64 = 24.0;
const SECTION_GAP: f64 = 8.0;
const INNER_X: f64 = 12.0;
const INNER_GAP: f64 = 6.0;

#[derive(Clone, Copy)]
struct Group {
    title: &'static str,
    children: &'static [usize],
}

const GROUPS: [Group; 10] = [
    Group {
        title: "Personalization",
        children: &[0, 1, 2],
    },
    Group {
        title: settings_display::KIND.title,
        children: &[3],
    },
    Group {
        title: settings_input::KIND.title,
        children: &[4],
    },
    Group {
        title: settings_windows::KIND.title,
        children: &[5],
    },
    Group {
        title: settings_keys::KIND.title,
        children: &[6],
    },
    Group {
        title: settings_apps::KIND.title,
        children: &[7],
    },
    Group {
        title: settings_games::KIND.title,
        children: &[8],
    },
    Group {
        title: settings_lock::KIND.title,
        children: &[9],
    },
    Group {
        title: settings_privacy::KIND.title,
        children: &[10],
    },
    Group {
        title: settings_recording::KIND.title,
        children: &[11],
    },
];

pub const KIND: CardKind = CardKind {
    id: "all",
    title: "Settings",
    about: "All settings by group, with search and the config file",
    sizes: &[Size::M],
    default: Size::M,
    half: false,
    sources: &[
        "config",
        "look",
        "wallpapers",
        "fonts",
        "fonts-mono",
        "icons",
        "cursors",
        "screens",
        "touchpad",
        "battery_limit",
        "shares",
        "recording",
        "key_capture",
    ],
};

/// Child cards owned by `settings/all`, in section 1's order.
pub const CHILD_KINDS: [CardKind; 12] = [
    settings_look::KIND_APPEARANCE,
    settings_look::KIND_WALLPAPER,
    settings_look::KIND_FONTS,
    settings_display::KIND,
    settings_input::KIND,
    settings_windows::KIND,
    settings_keys::KIND,
    settings_apps::KIND,
    settings_games::KIND,
    settings_lock::KIND,
    settings_privacy::KIND,
    settings_recording::KIND,
];

fn make_children() -> Vec<Box<dyn Card>> {
    vec![
        Box::new(settings_look::LookCard::appearance()),
        Box::new(settings_look::LookCard::wallpaper()),
        Box::new(settings_look::LookCard::fonts()),
        Box::new(settings_display::DisplayCard::new()),
        Box::new(settings_input::InputCard::default()),
        Box::new(settings_rows::TableCard::new(settings_windows::TABLE)),
        Box::new(settings_keys::KeysCard::new()),
        Box::new(settings_apps::DefaultsCard::new()),
        Box::new(settings_rows::TableCard::new(settings_games::TABLE)),
        Box::new(settings_lock::LockCard::default()),
        Box::new(settings_privacy::PrivacyCard::new()),
        Box::new(settings_recording::RecordingCard::new()),
    ]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileEntry {
    pub key: String,
    pub default_val: String,
    pub comment: String,
}

#[derive(Clone, Copy)]
struct DrawnHit {
    emitted: u32,
    child: usize,
    child_id: u32,
    rect: Rect,
}

pub struct SettingsAllCard {
    children: Vec<Box<dyn Card>>,
    open: Option<usize>,
    active_child: Option<usize>,
    query: String,
    config: Option<ConfigData>,
    screens: Option<ScreensData>,
    touchpad: Option<TouchpadData>,
    battery: Option<BatteryLimitData>,
    shares: Option<SharesData>,
    file_entries: Vec<FileEntry>,
    user_lines: HashMap<String, usize>,
    defaults_path: PathBuf,
    config_path: PathBuf,
    last_hits: RefCell<Vec<DrawnHit>>,
}

impl Default for SettingsAllCard {
    fn default() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        Self::with_paths(
            home.join("infynitae/config/config"),
            crate::config::config_path(),
        )
    }
}

impl SettingsAllCard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_paths(defaults_path: PathBuf, config_path: PathBuf) -> Self {
        Self {
            children: make_children(),
            open: None,
            active_child: None,
            query: String::new(),
            config: None,
            screens: None,
            touchpad: None,
            battery: None,
            shares: None,
            file_entries: Vec::new(),
            user_lines: HashMap::new(),
            defaults_path,
            config_path,
            last_hits: RefCell::new(Vec::new()),
        }
    }

    fn reload_files(&mut self) {
        let defaults_text = std::fs::read_to_string(&self.defaults_path).unwrap_or_default();
        let user_text = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        self.file_entries = parse_file_only_entries(&defaults_text);
        self.user_lines = parse_user_lines(&user_text);
    }

    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    fn child_cx<'a>(&self, cx: &'a Cx<'a>) -> Cx<'a> {
        Cx {
            size: Size::S,
            ..*cx
        }
    }

    fn matching_file_entries(&self) -> Vec<&FileEntry> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        if words.is_empty() {
            return Vec::new();
        }
        self.file_entries
            .iter()
            .filter(|e| entry_matches(e, &words))
            .collect()
    }

    fn file_value_and_changed<'a>(&'a self, e: &'a FileEntry) -> (&'a str, bool) {
        let Some(cfg) = self.config.as_ref() else {
            return (e.default_val.as_str(), false);
        };
        let def = cfg
            .default
            .get(&e.key)
            .map(String::as_str)
            .unwrap_or(e.default_val.as_str());
        if let Some(now) = cfg.now.get(&e.key) {
            (now.as_str(), now.as_str() != def)
        } else {
            (def, false)
        }
    }

    fn visible_group_children(&self, g: usize, child_cx: &Cx) -> Vec<(usize, Status)> {
        GROUPS[g]
            .children
            .iter()
            .filter_map(|&i| {
                let st = self.children[i].status(child_cx);
                (!matches!(st, Status::Hidden)).then_some((i, st))
            })
            .collect()
    }

    fn summary(&self, g: usize, cx: &Cx) -> String {
        let Some(cfg) = self.config.as_ref() else {
            return String::new();
        };
        if GROUPS[g].children.len() > 1 {
            let child_cx = self.child_cx(cx);
            let look = match cfg.value("COLOR_SCHEME").trim() {
                "prefer-light" => "Light".to_string(),
                "prefer-dark" => "Dark".to_string(),
                "default" | "" => "System look".to_string(),
                other => other.to_string(),
            };
            let theme_mode = cfg.value("THEME_FROM_WALLPAPER").trim();
            let accent = if theme_mode.eq_ignore_ascii_case("true")
                || theme_mode.eq_ignore_ascii_case("accent")
                || theme_mode.eq_ignore_ascii_case("all")
            {
                "from wallpaper".to_string()
            } else {
                non_empty(cfg.value("BORDER_ACTIVE"))
            };
            let wallpaper = if !matches!(self.children[1].status(&child_cx), Status::Hidden) {
                let raw = cfg.value("WALLPAPER").trim();
                if raw.is_empty() {
                    String::new()
                } else {
                    Path::new(raw)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(raw)
                        .to_string()
                }
            } else {
                String::new()
            };
            let tail = if !wallpaper.is_empty() {
                wallpaper
            } else {
                join_space(cfg.value("FONT_NAME").trim(), cfg.value("FONT_SIZE").trim())
            };
            return [look, accent, tail]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
        }
        let i = GROUPS[g].children[0];
        let parts: Vec<String> = match CHILD_KINDS[i].id {
            "appearance" => {
                let look = match cfg.value("COLOR_SCHEME").trim() {
                    "prefer-light" => "Light".to_string(),
                    "prefer-dark" => "Dark".to_string(),
                    "default" | "" => "System look".to_string(),
                    other => other.to_string(),
                };
                let accent = if cfg
                    .value("THEME_FROM_WALLPAPER")
                    .trim()
                    .eq_ignore_ascii_case("true")
                {
                    "from wallpaper".to_string()
                } else {
                    non_empty(cfg.value("BORDER_ACTIVE"))
                };
                let corners = {
                    let c = cfg.value("WINDOW_CORNERS").trim();
                    if c.is_empty() {
                        String::new()
                    } else {
                        format!("{c} px corners")
                    }
                };
                vec![look, accent, corners]
            }
            "wallpaper" => {
                let raw = cfg.value("WALLPAPER").trim();
                let pic = if raw.is_empty() {
                    "None".to_string()
                } else {
                    Path::new(raw)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(raw)
                        .to_string()
                };
                vec![pic, non_empty(cfg.value("WALLPAPER_MODE"))]
            }
            "fonts" => {
                let font = join_space(cfg.value("FONT_NAME").trim(), cfg.value("FONT_SIZE").trim());
                let icons = non_empty(cfg.value("ICON_THEME"));
                vec![font, icons]
            }
            "display" => self.display_summary(cfg),
            "input" => {
                let layout_raw = cfg.value("XKB_DEFAULT_LAYOUT").trim();
                let layout = if layout_raw.is_empty() {
                    "Default layout".to_string()
                } else {
                    layout_raw.to_string()
                };
                let mouse = match cfg.value("MOUSE_ACCEL_PROFILE").trim() {
                    "flat" => "flat mouse".to_string(),
                    "adaptive" => "adaptive mouse".to_string(),
                    other if !other.is_empty() => format!("{other} mouse"),
                    _ => String::new(),
                };
                let tap = if self.touchpad.is_some_and(|t| t.present)
                    && cfg
                        .value("TOUCHPAD_TAP")
                        .trim()
                        .eq_ignore_ascii_case("true")
                {
                    "touchpad tap".to_string()
                } else {
                    String::new()
                };
                vec![layout, mouse, tap]
            }
            "windows" => {
                let focus = {
                    let v = cfg.value("FOCUS_FOLLOWS_MOUSE");
                    shown_word("FOCUS_FOLLOWS_MOUSE", v.trim())
                        .map(|w| format!("Focus {}", w.to_lowercase()))
                        .unwrap_or_default()
                };
                let open = {
                    let v = cfg.value("PLACEMENT_POLICY");
                    shown_word("PLACEMENT_POLICY", v.trim())
                        .map(|w| format!("new windows {}", w.to_lowercase()))
                        .unwrap_or_default()
                };
                vec![focus, open]
            }
            "keys" => {
                let n = cfg
                    .now
                    .keys()
                    .filter(|k| settings_keys::KeysCard::owns_config_key(k))
                    .count();
                if n == 0 {
                    vec!["Defaults".to_string()]
                } else if n == 1 {
                    vec!["1 custom".to_string()]
                } else {
                    vec![format!("{n} custom")]
                }
            }
            "defaults" => {
                let custom: Vec<String> = ["TERMINAL", "BROWSER", "FILE_MANAGER"]
                    .into_iter()
                    .filter_map(|k| {
                        let v = cfg.value(k).trim();
                        if v.is_empty() || v.eq_ignore_ascii_case("auto") {
                            None
                        } else {
                            Some(
                                Path::new(v.split_whitespace().next().unwrap_or(v))
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(v)
                                    .to_string(),
                            )
                        }
                    })
                    .collect();
                let apps = if custom.is_empty() {
                    if cfg
                        .value("SESSION_RESTORE")
                        .trim()
                        .eq_ignore_ascii_case("true")
                    {
                        vec!["Automatic".to_string(), "restore session".to_string()]
                    } else {
                        vec!["Automatic".to_string()]
                    }
                } else {
                    custom
                };
                let _ = cx;
                apps
            }
            "games" => {
                let mode = if cfg.value("GAME_MODE").trim().eq_ignore_ascii_case("true") {
                    "Game mode"
                } else {
                    "Standard"
                }
                .to_string();
                let lat = shown_word("LATENCY", cfg.value("LATENCY").trim())
                    .map(|w| format!("frames {}", w.to_lowercase()))
                    .unwrap_or_default();
                let tear = match cfg.value("TEARING").trim() {
                    "" | "false" => String::new(),
                    t => shown_word("TEARING", t)
                        .map(|w| format!("tearing {}", w.to_lowercase()))
                        .unwrap_or_default(),
                };
                vec![mode, lat, tear]
            }
            "lock" => {
                let sleep = match cfg.value("IDLE_SLEEP_TIMEOUT_MIN").trim() {
                    "" | "0" => "Never sleeps".to_string(),
                    m => format!("{m} min sleep"),
                };
                let lock = if cfg.value("SLEEP_LOCK").trim().eq_ignore_ascii_case("true") {
                    "lock on sleep".to_string()
                } else {
                    String::new()
                };
                let limit = cfg.value("BATTERY_LIMIT").trim();
                let bat = if self.battery.is_some_and(|b| b.supported)
                    && !limit.is_empty()
                    && !limit.eq_ignore_ascii_case("off")
                {
                    format!("{limit}% charge")
                } else {
                    String::new()
                };
                vec![sleep, lock, bat]
            }
            "privacy" => {
                let ask = match cfg.value("SCREENSHARE_ASK").trim() {
                    "app" => "Ask per app".to_string(),
                    "always" => "Always ask".to_string(),
                    "never" => "Never ask".to_string(),
                    other => other.to_string(),
                };
                let shares = self.shares.as_ref().map_or(0, |s| s.apps.len());
                let sharing = if shares > 0 {
                    format!("{shares} sharing")
                } else {
                    String::new()
                };
                let hidden_n = cfg
                    .value("PRIVACY_APPS")
                    .split([',', ' ', '\t'])
                    .filter(|s| !s.trim().is_empty())
                    .count();
                let hidden = if hidden_n > 0 {
                    format!("{hidden_n} hidden")
                } else {
                    String::new()
                };
                vec![ask, sharing, hidden]
            }
            "recording" => {
                let fps = {
                    let f = cfg.value("RECORD_FPS").trim();
                    if f.is_empty() {
                        String::new()
                    } else {
                        format!("{f} fps")
                    }
                };
                let codec = non_empty(cfg.value("RECORD_CODEC"));
                let replay = {
                    let m = cfg.value("REPLAY_BUFFER_MINUTES").trim();
                    if m.is_empty() {
                        String::new()
                    } else {
                        format!("{m} min replay")
                    }
                };
                vec![fps, codec, replay]
            }
            _ => Vec::new(),
        };
        parts
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn display_summary(&self, cfg: &ConfigData) -> Vec<String> {
        if let Some(sc) = self.screens.as_ref().filter(|s| !s.screens.is_empty()) {
            if sc.screens.len() == 1 {
                let s = &sc.screens[0];
                let res = if s.width > 0 && s.height > 0 {
                    format!("{}x{}", s.width, s.height)
                } else {
                    s.name.clone()
                };
                let hz = if s.refresh_mhz > 0 {
                    fmt_hz(s.refresh_mhz)
                } else {
                    String::new()
                };
                let scale = non_empty(cfg.value("OUTPUT_SCALE"));
                let scale_str = if scale.is_empty() || scale == "1" || scale == "1.0" {
                    String::new()
                } else {
                    format!("{scale}x")
                };
                return vec![res, hz, scale_str];
            }
            return vec![format!("{} screens", sc.screens.len())];
        }
        let scale = non_empty(cfg.value("OUTPUT_SCALE"));
        let scale_str = if scale.is_empty() {
            String::new()
        } else {
            format!("Scale {scale}")
        };
        let night = non_empty(cfg.value("NIGHT_LIGHT_TEMP"));
        let night_str = if night.is_empty() {
            String::new()
        } else {
            format!("night light {night} K")
        };
        vec![scale_str, night_str]
    }

    fn resolve_child_hit(&self, emitted: u32, at: (f64, f64)) -> Option<(usize, u32)> {
        if emitted < CHILD_STRIDE {
            return None;
        }
        let hits = self.last_hits.borrow();
        if let Some(found) = hits
            .iter()
            .rev()
            .find(|h| h.emitted == emitted && rect_contains(at, h.rect))
            .or_else(|| hits.iter().rev().find(|h| h.emitted == emitted))
        {
            return Some((found.child, found.child_id));
        }
        let i = ((emitted / CHILD_STRIDE).checked_sub(1))? as usize;
        (i < self.children.len()).then_some((i, emitted % CHILD_STRIDE))
    }

    fn resolve_drag_or_release(&self, emitted: u32, active: Option<usize>) -> Option<(usize, u32)> {
        if emitted < CHILD_STRIDE {
            return None;
        }
        if let Some(found) = self
            .last_hits
            .borrow()
            .iter()
            .rev()
            .find(|h| h.emitted == emitted && active.is_none_or(|a| h.child == a))
        {
            return Some((found.child, found.child_id));
        }
        let i = ((emitted / CHILD_STRIDE).checked_sub(1))? as usize;
        (i < self.children.len() && active.is_none_or(|a| a == i))
            .then_some((i, emitted % CHILD_STRIDE))
    }
}

impl Card for SettingsAllCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let mut changed = false;
        match key {
            "config" => {
                if let Some(d) = data.downcast_ref::<ConfigData>() {
                    changed |= self.config.as_ref() != Some(d);
                    self.config = Some(d.clone());
                    let prev_entries = self.file_entries.clone();
                    let prev_lines = self.user_lines.clone();
                    self.reload_files();
                    changed |= prev_entries != self.file_entries || prev_lines != self.user_lines;
                }
            }
            "screens" => {
                if let Some(d) = data.downcast_ref::<ScreensData>() {
                    changed |= self.screens.as_ref() != Some(d);
                    self.screens = Some(d.clone());
                }
            }
            "touchpad" => {
                if let Some(d) = data.downcast_ref::<TouchpadData>() {
                    changed |= self.touchpad.as_ref() != Some(d);
                    self.touchpad = Some(*d);
                }
            }
            "battery_limit" => {
                if let Some(d) = data.downcast_ref::<BatteryLimitData>() {
                    changed |= self.battery.as_ref() != Some(d);
                    self.battery = Some(*d);
                }
            }
            "shares" => {
                if let Some(d) = data.downcast_ref::<SharesData>() {
                    changed |= self.shares.as_ref() != Some(d);
                    self.shares = Some(d.clone());
                }
            }
            _ => {}
        }
        for (i, child) in self.children.iter_mut().enumerate() {
            if CHILD_KINDS[i].sources.contains(&key) {
                changed |= child.apply(key, data);
            }
        }
        changed
    }

    fn status(&self, cx: &Cx) -> Status {
        if self.config.is_none() {
            return Status::Loading;
        }
        let child_cx = self.child_cx(cx);
        if self.searching() {
            let any_child = self
                .children
                .iter()
                .any(|c| c.status(&child_cx) == Status::Ready);
            if any_child || !self.matching_file_entries().is_empty() {
                Status::Ready
            } else {
                Status::Hidden
            }
        } else {
            Status::Ready
        }
    }

    fn height(&self, cx: &Cx, width: f64) -> f64 {
        let child_cx = self.child_cx(cx);
        if self.searching() {
            let mut h = 0.0;
            let mut first = true;
            for c in &self.children {
                if c.status(&child_cx) != Status::Ready {
                    continue;
                }
                if !first {
                    h += SECTION_GAP;
                }
                first = false;
                h += HEADER_H + c.height(&child_cx, width);
            }
            let file_matches = self.matching_file_entries();
            if !file_matches.is_empty() {
                if !first {
                    h += SECTION_GAP;
                }
                h += HEADER_H + file_matches.len() as f64 * ui::ROW_H;
            }
            return h;
        }

        let mut h = 0.0;
        let inner_w = (width - INNER_X).max(0.0);
        for (g, group) in GROUPS.iter().enumerate() {
            let vis = self.visible_group_children(g, &child_cx);
            if vis.is_empty() {
                continue;
            }
            h += ui::ROW_H;
            if self.open == Some(g) {
                let multi = group.children.len() > 1;
                for (pos, (i, st)) in vis.into_iter().enumerate() {
                    if multi {
                        if pos > 0 {
                            h += SECTION_GAP;
                        }
                        h += HEADER_H;
                    }
                    let ch = if matches!(st, Status::Loading) {
                        ui::ROW_H
                    } else {
                        self.children[i].height(&child_cx, inner_w)
                    };
                    h += ch;
                }
                h += INNER_GAP;
            }
        }
        h + ui::ROW_H
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let child_cx = self.child_cx(cx);
        let selected_target = hits.selected.and_then(|sel| {
            if sel < CHILD_STRIDE {
                return None;
            }
            let prev = self.last_hits.borrow();
            prev.iter()
                .rev()
                .find(|h| h.emitted == sel && hits.pointer.is_none_or(|p| rect_contains(p, h.rect)))
                .or_else(|| prev.iter().rev().find(|h| h.emitted == sel))
                .map(|h| (h.child, h.child_id))
                .or_else(|| {
                    let i = ((sel / CHILD_STRIDE).checked_sub(1))? as usize;
                    (i < self.children.len()).then_some((i, sel % CHILD_STRIDE))
                })
        });

        let mut recorded_hits = Vec::new();
        let mut y = body.1;

        if self.searching() {
            let mut first = true;
            for (i, child) in self.children.iter().enumerate() {
                if child.status(&child_cx) != Status::Ready {
                    continue;
                }
                if !first {
                    y += SECTION_GAP;
                }
                first = false;
                draw_section_header(cr, cx, body, y, CHILD_KINDS[i].title);
                y += HEADER_H;

                let ch = child.height(&child_cx, body.2);
                let child_sel = selected_target.and_then(|(c, cid)| (c == i).then_some(cid));
                let mut child_hits = Hits::new(hits.pointer, child_sel);
                child.draw(cr, &child_cx, (body.0, y, body.2, ch), &mut child_hits);
                let base = CHILD_STRIDE * (i as u32 + 1);
                for (cid, rect) in child_hits.list {
                    let emitted = base + cid;
                    let _ = hits.add(emitted, rect);
                    recorded_hits.push(DrawnHit {
                        emitted,
                        child: i,
                        child_id: cid,
                        rect,
                    });
                }
                y += ch;
            }

            let file_matches = self.matching_file_entries();
            if !file_matches.is_empty() {
                if !first {
                    y += SECTION_GAP;
                }
                draw_section_header(cr, cx, body, y, "In the file");
                y += HEADER_H;
                let right = body.0 + body.2 - 8.0;
                for (idx, entry) in file_matches.into_iter().enumerate() {
                    let r = (body.0, y, body.2, ui::ROW_H);
                    let (raw_val, changed) = self.file_value_and_changed(entry);
                    let shown_val = if raw_val.is_empty() {
                        "Not set"
                    } else {
                        raw_val
                    };
                    let val_t = Text {
                        size: 12.0,
                        bold: false,
                        color: if changed {
                            cx.theme.accent_text
                        } else {
                            cx.theme.muted
                        },
                        text: shown_val,
                    };
                    let max_val_w = (body.2 * 0.42).max(24.0);
                    let vw = draw::measure(cr, &val_t).0.min(max_val_w);
                    if idx < (CHILD_STRIDE - HIT_FILE_BASE) as usize {
                        ui::row(
                            cr,
                            cx,
                            hits,
                            HIT_FILE_BASE + idx as u32,
                            r,
                            &Row {
                                icon: None,
                                name: &entry.key,
                                state: &entry.comment,
                                on: false,
                                right: vw + 12.0,
                            },
                        );
                    } else {
                        let mut no_hits = Hits::default();
                        ui::row(
                            cr,
                            cx,
                            &mut no_hits,
                            0,
                            r,
                            &Row {
                                icon: None,
                                name: &entry.key,
                                state: &entry.comment,
                                on: false,
                                right: vw + 12.0,
                            },
                        );
                    }
                    draw::text_max(
                        cr,
                        right - vw,
                        y + (ui::ROW_H - 15.0) / 2.0,
                        max_val_w,
                        &val_t,
                    );
                    y += ui::ROW_H;
                }
            }
            *self.last_hits.borrow_mut() = recorded_hits;
            return;
        }

        let inner_w = (body.2 - INNER_X).max(0.0);
        for (g, group) in GROUPS.iter().enumerate() {
            let vis = self.visible_group_children(g, &child_cx);
            if vis.is_empty() {
                continue;
            }
            let r = (body.0, y, body.2, ui::ROW_H);
            let sum = self.summary(g, cx);
            ui::row(
                cr,
                cx,
                hits,
                HIT_GROUP_BASE + g as u32,
                r,
                &Row {
                    icon: None,
                    name: group.title,
                    state: &sum,
                    on: self.open == Some(g),
                    right: 8.0,
                },
            );
            y += ui::ROW_H;

            if self.open == Some(g) {
                let multi = group.children.len() > 1;
                let content_top = y;
                for (pos, (i, st)) in vis.into_iter().enumerate() {
                    if multi {
                        if pos > 0 {
                            y += SECTION_GAP;
                        }
                        draw_section_header(
                            cr,
                            cx,
                            (body.0 + INNER_X, y, inner_w, HEADER_H),
                            y,
                            CHILD_KINDS[i].title,
                        );
                        y += HEADER_H;
                    }
                    let ch = if matches!(st, Status::Loading) {
                        draw::text_max(
                            cr,
                            body.0 + INNER_X + 8.0,
                            y + (ui::ROW_H - 15.0) / 2.0,
                            (inner_w - 16.0).max(0.0),
                            &Text {
                                size: 12.0,
                                bold: false,
                                color: cx.theme.muted,
                                text: "Loading…",
                            },
                        );
                        ui::ROW_H
                    } else {
                        let h = self.children[i].height(&child_cx, inner_w);
                        let child_sel =
                            selected_target.and_then(|(c, cid)| (c == i).then_some(cid));
                        let mut child_hits = Hits::new(hits.pointer, child_sel);
                        self.children[i].draw(
                            cr,
                            &child_cx,
                            (body.0 + INNER_X, y, inner_w, h),
                            &mut child_hits,
                        );
                        let base = CHILD_STRIDE * (i as u32 + 1);
                        for (cid, rect) in child_hits.list {
                            let emitted = base + cid;
                            let _ = hits.add(emitted, rect);
                            recorded_hits.push(DrawnHit {
                                emitted,
                                child: i,
                                child_id: cid,
                                rect,
                            });
                        }
                        h
                    };
                    y += ch;
                }
                let content_h = y - content_top;
                if content_h > 4.0 {
                    draw::set(cr, cx.theme.edge);
                    cr.set_line_width(1.0);
                    cr.move_to(body.0 + 4.5, content_top + 2.0);
                    cr.line_to(body.0 + 4.5, content_top + content_h - 2.0);
                    let _ = cr.stroke();
                }
                y += INNER_GAP;
            }
        }

        let note = "every other setting";
        let note_t = Text {
            size: 12.0,
            bold: false,
            color: cx.theme.muted,
            text: note,
        };
        let note_w = draw::measure(cr, &note_t).0.min((body.2 * 0.45).max(24.0));
        let r = (body.0, y, body.2, ui::ROW_H);
        ui::row(
            cr,
            cx,
            hits,
            HIT_OPEN_CONFIG,
            r,
            &Row {
                icon: None,
                name: "Open the config file",
                state: "",
                on: false,
                right: note_w + 12.0,
            },
        );
        draw::text_max(
            cr,
            body.0 + body.2 - 8.0 - note_w,
            y + (ui::ROW_H - 15.0) / 2.0,
            note_w,
            &note_t,
        );

        *self.last_hits.borrow_mut() = recorded_hits;
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let child_cx = self.child_cx(cx);
        match ev {
            Event::Text(q) => {
                let mut changed = self.query != *q;
                self.query = (*q).to_string();
                self.active_child = None;
                for c in &mut self.children {
                    changed |= c.event(ev, &child_cx, out);
                }
                changed
            }
            Event::Press { hit, button, at } => {
                let hit = *hit;
                if !self.searching() {
                    if button == &Button::Left && hit == HIT_OPEN_CONFIG {
                        out.push(Action::Run(EDIT_COMMAND.to_string()));
                        out.push(Action::ClosePanel);
                        return true;
                    }
                    if button == &Button::Left
                        && (HIT_GROUP_BASE..HIT_GROUP_BASE + GROUPS.len() as u32).contains(&hit)
                    {
                        let idx = (hit - HIT_GROUP_BASE) as usize;
                        if self.visible_group_children(idx, &child_cx).is_empty() {
                            return false;
                        }
                        self.open = if self.open == Some(idx) {
                            None
                        } else {
                            Some(idx)
                        };
                        self.active_child = None;
                        return true;
                    }
                    if hit >= CHILD_STRIDE {
                        if let Some((i, child_id)) = self.resolve_child_hit(hit, *at) {
                            self.active_child = Some(i);
                            return self.children[i].event(
                                &Event::Press {
                                    hit: child_id,
                                    button: *button,
                                    at: *at,
                                },
                                &child_cx,
                                out,
                            );
                        }
                    }
                    return false;
                }

                if button == &Button::Left && (HIT_FILE_BASE..CHILD_STRIDE).contains(&hit) {
                    let idx = (hit - HIT_FILE_BASE) as usize;
                    let matches = self.matching_file_entries();
                    let Some(entry) = matches.get(idx) else {
                        return false;
                    };
                    let editor = std::env::var("EDITOR").ok();
                    let line = self.user_lines.get(&entry.key).copied();
                    out.push(Action::Run(open_file_command(
                        editor.as_deref(),
                        &self.config_path,
                        line,
                    )));
                    out.push(Action::ClosePanel);
                    return true;
                }

                if hit >= CHILD_STRIDE {
                    if let Some((i, child_id)) = self.resolve_child_hit(hit, *at) {
                        self.active_child = Some(i);
                        return self.children[i].event(
                            &Event::Press {
                                hit: child_id,
                                button: *button,
                                at: *at,
                            },
                            &child_cx,
                            out,
                        );
                    }
                }
                false
            }
            Event::Scroll { hit, dy } => {
                if let Some(h) = *hit {
                    if let Some((i, child_id)) = self.resolve_child_hit(h, (0.0, 0.0)) {
                        return self.children[i].event(
                            &Event::Scroll {
                                hit: Some(child_id),
                                dy: *dy,
                            },
                            &child_cx,
                            out,
                        );
                    }
                }
                if !self.searching() {
                    if let Some(g) = self.open {
                        for &i in GROUPS[g].children {
                            if self.children[i].event(ev, &child_cx, out) {
                                return true;
                            }
                        }
                    }
                    return false;
                }
                for child in &mut self.children {
                    if child.event(ev, &child_cx, out) {
                        return true;
                    }
                }
                false
            }
            Event::Drag { hit, at } => {
                let Some((i, child_id)) = self.resolve_drag_or_release(*hit, self.active_child)
                else {
                    return false;
                };
                self.children[i].event(
                    &Event::Drag {
                        hit: child_id,
                        at: *at,
                    },
                    &child_cx,
                    out,
                )
            }
            Event::Release { hit } => {
                let active = self.active_child.take();
                let Some((i, child_id)) = self.resolve_drag_or_release(*hit, active) else {
                    return false;
                };
                self.children[i].event(&Event::Release { hit: child_id }, &child_cx, out)
            }
            _ => false,
        }
    }

    fn tick(&mut self, dt_ms: f64) -> bool {
        let mut changed = false;
        for c in &mut self.children {
            changed |= c.tick(dt_ms);
        }
        changed
    }

    fn show_list(&mut self) {
        if let Some(g) = self.open {
            if let Some(&i) = GROUPS[g].children.first() {
                self.children[i].show_list();
            }
            return;
        }
        if !GROUPS.is_empty() {
            self.open = Some(0);
        }
    }
}

pub(crate) fn on_any_card(key: &str) -> bool {
    [
        settings_display::TABLE,
        settings_input::TABLE,
        settings_windows::TABLE,
        settings_apps::TABLE,
        settings_games::TABLE,
        settings_lock::TABLE,
        settings_privacy::TABLE,
        settings_recording::TABLE,
    ]
    .into_iter()
    .any(|t| t.iter().any(|s| s.key == key))
        || [
            settings_look::KEYS,
            settings_display::KEYS,
            settings_apps::KEYS,
            settings_privacy::KEYS,
            settings_recording::KEYS,
        ]
        .into_iter()
        .any(|ks| ks.contains(&key))
        || settings_keys::KeysCard::owns_config_key(key)
}

fn parse_key_value(line: &str) -> Option<(&str, String)> {
    let l = line.trim_start();
    if l.starts_with('#') {
        return None;
    }
    let (k, v) = l.split_once('=')?;
    let first = k.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_')
        || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some((k, crate::config::value_text(v)))
}

/// Parses non-card keys from `config/config` preserving file order and capturing each key's first
/// comment line immediately beneath it.
pub(crate) fn parse_file_only_entries(defaults_text: &str) -> Vec<FileEntry> {
    let lines: Vec<&str> = defaults_text.lines().collect();
    let mut out: Vec<FileEntry> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();

    for (i, line) in lines.iter().enumerate() {
        let Some((key, default_val)) = parse_key_value(line) else {
            continue;
        };
        if on_any_card(key) {
            continue;
        }
        let mut comment = String::new();
        for next in lines.iter().skip(i + 1) {
            let trimmed = next.trim();
            let Some(after_hash) = trimmed.strip_prefix('#') else {
                break;
            };
            let text = after_hash.trim();
            if !text.is_empty() {
                comment = text.to_string();
                break;
            }
        }
        if let Some(&pos) = positions.get(key) {
            out[pos].default_val = default_val;
            out[pos].comment = comment;
        } else {
            positions.insert(key.to_string(), out.len());
            out.push(FileEntry {
                key: key.to_string(),
                default_val,
                comment,
            });
        }
    }
    out
}

/// Maps each active `KEY=...` in the owner's config file to its 1-based line number.
pub(crate) fn parse_user_lines(user_text: &str) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (idx, line) in user_text.lines().enumerate() {
        if let Some((key, _)) = parse_key_value(line) {
            map.insert(key.to_string(), idx + 1);
        }
    }
    map
}

/// Builds the terminal editor shell command, jumping to `+N` for editors that accept it and quoting
/// every value with `settings::sq`.
/// The word a card shows for a value (its Pick or Words label), so a summary says what the card says.
fn shown_word(key: &str, value: &str) -> Option<&'static str> {
    use super::settings_rows::Control;
    let tables: [&[super::settings_rows::Setting]; 8] = [
        super::settings_games::TABLE,
        super::settings_windows::TABLE,
        super::settings_input::TABLE,
        super::settings_lock::TABLE,
        super::settings_privacy::TABLE,
        super::settings_display::TABLE,
        super::settings_apps::TABLE,
        super::settings_recording::TABLE,
    ];
    tables
        .iter()
        .flat_map(|t| t.iter())
        .find(|s| s.key == key)
        .and_then(|s| match s.control {
            Control::Pick(o) | Control::Words(o) => {
                o.iter().find(|(v, _)| *v == value).map(|(_, w)| *w)
            }
            _ => None,
        })
}

pub(crate) fn open_file_command(editor: Option<&str>, path: &Path, line: Option<usize>) -> String {
    let ed = editor
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("nano");
    // EDITOR can carry its own arguments ("code --wait", "nvim -p"): each word quoted on its own
    let words: Vec<&str> = ed.split_whitespace().collect();
    let ed_words = words
        .iter()
        .map(|w| settings::sq(w))
        .collect::<Vec<_>>()
        .join(" ");
    let first = words.first().copied().unwrap_or("nano");
    let bin = Path::new(first)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(first);
    let path_str = path.to_string_lossy();
    if let Some(n) = line.filter(|_| PLUS_LINE_EDITORS.contains(&bin)) {
        format!(
            "sh \"$HOME/.local/bin/xdg-terminal-exec\" {} {} {}",
            ed_words,
            settings::sq(&format!("+{n}")),
            settings::sq(&path_str),
        )
    } else {
        format!(
            "sh \"$HOME/.local/bin/xdg-terminal-exec\" {} {}",
            ed_words,
            settings::sq(&path_str),
        )
    }
}

fn entry_matches(e: &FileEntry, words: &[String]) -> bool {
    let hay = format!("{} {}", e.key, e.comment).to_lowercase();
    words.iter().all(|w| hay.contains(w))
}

fn non_empty(s: &str) -> String {
    s.trim().to_string()
}

fn join_space(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => String::new(),
        (false, true) => a.to_string(),
        (true, false) => b.to_string(),
        (false, false) => format!("{a} {b}"),
    }
}

fn fmt_hz(mhz: u32) -> String {
    if mhz.is_multiple_of(1000) {
        format!("{} Hz", mhz / 1000)
    } else {
        format!("{:.2} Hz", mhz as f64 / 1000.0)
    }
}

fn rect_contains(p: (f64, f64), r: Rect) -> bool {
    p.0 >= r.0 && p.0 <= r.0 + r.2 && p.1 >= r.1 && p.1 <= r.1 + r.3
}

fn draw_section_header(cr: &Context, cx: &Cx, body: Rect, y: f64, title: &str) {
    draw::text_max(
        cr,
        body.0 + 8.0,
        y + (HEADER_H - 15.0) / 2.0,
        (body.2 - 16.0).max(0.0),
        &Text {
            size: 12.0,
            bold: true,
            color: cx.theme.muted,
            text: title,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_look::{FoundNames, LookData, WallpapersData};
    use crate::cards::settings_recording::RecordingEnv;
    use crate::cards::settings_rows::{parse_config, FILE_ONLY};
    use crate::config::Config;
    use crate::provider::Provider;
    use crate::theme::Theme;
    use std::collections::HashSet;

    fn full_config_data() -> ConfigData {
        let def_txt =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap();
        ConfigData {
            now: HashMap::new(),
            default: parse_config(&def_txt),
            programs: ["fc-list", "gamemoded", "udisksctl"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }

    fn apply_all_sources(card: &mut SettingsAllCard, home: &Path, cfg: &ConfigData) {
        card.apply("config", cfg as &dyn Any);
        card.apply(
            "look",
            &LookData {
                wallpaper_bin: true,
                ..LookData::default()
            } as &dyn Any,
        );
        card.apply(
            "wallpapers",
            &WallpapersData {
                has_bin: true,
                ..WallpapersData::default()
            } as &dyn Any,
        );
        for k in ["fonts", "fonts-mono", "icons", "cursors"] {
            card.apply(k, &FoundNames(vec!["Adwaita".into()]) as &dyn Any);
        }
        card.apply("screens", &ScreensData::default() as &dyn Any);
        card.apply("touchpad", &TouchpadData { present: true } as &dyn Any);
        card.apply(
            "battery_limit",
            &BatteryLimitData { supported: true } as &dyn Any,
        );
        card.apply("shares", &SharesData::default() as &dyn Any);
        card.apply(
            "recording",
            &RecordingEnv {
                home: home.to_path_buf(),
                local_recorder: true,
            } as &dyn Any,
        );
    }

    fn draw_card(card: &SettingsAllCard, cx: &Cx, width: f64) -> (f64, Hits) {
        let h = card.height(cx, width);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 3200).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        card.draw(&cr, cx, (0.0, 0.0, width, h), &mut hits);
        (h, hits)
    }

    fn temp_home(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "infynitae_edge_apps_settings_all_{}_{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn registration_order_sources_and_size_m() {
        let p = settings::Settings;
        let kind = p.cards().iter().find(|k| k.id == "all").unwrap();
        assert_eq!(kind.title, "Settings");
        assert_eq!(kind.sizes, &[Size::M]);
        assert_eq!(kind.default, Size::M);
        assert!(!kind.half);
        assert!(p.card("all", None).is_some());

        let expected_order = [
            "appearance",
            "wallpaper",
            "fonts",
            "display",
            "input",
            "windows",
            "keys",
            "defaults",
            "games",
            "lock",
            "privacy",
            "recording",
        ];
        let actual_order: Vec<&str> = CHILD_KINDS.iter().map(|k| k.id).collect();
        assert_eq!(actual_order, expected_order);

        for ck in CHILD_KINDS {
            for s in ck.sources {
                assert!(
                    kind.sources.contains(s),
                    "child {} source {s} missing from KIND.sources",
                    ck.id
                );
            }
        }
    }

    #[test]
    fn parse_file_only_from_real_usual_empty_and_broken_configs() {
        let real =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap();
        let real_entries = parse_file_only_entries(&real);
        let parsed_keys: HashSet<&str> = real_entries.iter().map(|e| e.key.as_str()).collect();
        let expected_keys: HashSet<&str> = FILE_ONLY.iter().copied().collect();
        assert_eq!(parsed_keys, expected_keys);

        let keep = real_entries
            .iter()
            .find(|e| e.key == "KEEP_ON_TOP")
            .unwrap();
        assert_eq!(keep.default_val, "Picture-in-Picture, Picture in picture");
        assert!(keep.comment.contains("Windows kept on top"));

        assert!(parse_file_only_entries("").is_empty());

        let broken = "# comment only\nnot an assignment\n123BAD=oops\nGAME_MODE=\"true\"\nCUSTOM_UNLISTED=\"val\"\n  # Custom comment line\n  # second line ignored\nUNTERMINATED=\"abc\n";
        let broken_entries = parse_file_only_entries(broken);
        assert_eq!(broken_entries.len(), 2);
        assert_eq!(broken_entries[0].key, "CUSTOM_UNLISTED");
        assert_eq!(broken_entries[0].default_val, "val");
        assert_eq!(broken_entries[0].comment, "Custom comment line");
        assert_eq!(broken_entries[1].key, "UNTERMINATED");

        let lines = parse_user_lines("# commented\n#KEEP_ON_TOP=\"no\"\nKEEP_ON_TOP=\"VLC\"\n");
        assert_eq!(lines.get("KEEP_ON_TOP"), Some(&3));
        assert!(!lines.contains_key("FRAME_GUARD"));
    }

    #[test]
    fn opens_one_group_at_a_time_hides_hidden_children_and_forwards_hits_and_tick() {
        let dir = temp_home("groups");
        let cfg_file = dir.join("config");
        let def_file = dir.join("defaults");
        std::fs::write(
            &def_file,
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap(),
        )
        .unwrap();
        std::fs::write(
            &cfg_file,
            "PANEL_SETTINGS=\"SUPER+I top center 440 settings/all\"\n",
        )
        .unwrap();

        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th);
        let cfg = full_config_data();
        let mut card = SettingsAllCard::with_paths(def_file, cfg_file);
        assert_eq!(card.status(&cx), Status::Loading);

        // First with all 10 groups visible: 10 groups + Open the config file = 11 rows
        apply_all_sources(&mut card, &dir, &cfg);
        assert_eq!(draw_card(&card, &cx, 404.0).0, 11.0 * ui::ROW_H);

        // Omit local_recorder -> recording returns Status::Hidden and its row disappears
        card.apply(
            "recording",
            &RecordingEnv {
                home: dir.clone(),
                local_recorder: false,
            } as &dyn Any,
        );
        assert_eq!(card.status(&cx), Status::Ready);

        let (h_closed, hits_closed) = draw_card(&card, &cx, 404.0);
        // 9 visible groups + Open the config file
        assert_eq!(h_closed, 10.0 * ui::ROW_H);
        assert!(!hits_closed
            .list
            .iter()
            .any(|(id, _)| *id == HIT_GROUP_BASE + 9));
        let bottom = hits_closed
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0, f64::max);
        assert!((bottom - h_closed).abs() < 0.5);

        // Click Games (group index 6, child index 8) -> opens in place at Size::S with its More row working
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: HIT_GROUP_BASE + 6,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out
        ));
        let (h_open_games, hits_games) = draw_card(&card, &cx, 404.0);
        assert!(h_open_games > h_closed);
        // Games child index 8 -> hits are CHILD_STRIDE * (8 + 1) + child_id = 900_000 + child_id.
        // Clicking Games More (9000) -> 909_000 expands Games in place.
        let games_base = CHILD_STRIDE * 9;
        assert!(hits_games
            .list
            .iter()
            .any(|(id, _)| *id == games_base + 9000));
        assert!(card.event(
            &Event::Press {
                hit: games_base + 9000,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out
        ));
        let (h_more_games, hits_games_more) = draw_card(&card, &cx, 404.0);
        assert!(h_more_games > h_open_games);
        // Toggle GAME_MODE (Games row 0 -> child hit 100 -> 900_100)
        assert!(hits_games_more
            .list
            .iter()
            .any(|(id, _)| *id == games_base + 100));
        assert!(card.event(
            &Event::Press {
                hit: games_base + 100,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(out
            .iter()
            .any(|a| matches!(a, Action::Run(cmd) if cmd.contains("set GAME_MODE 'false'"))));

        // Click Windows (group index 3, child index 5) -> closes Games, opens Windows (one open at a time)
        assert!(card.event(
            &Event::Press {
                hit: HIT_GROUP_BASE + 3,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(card.open, Some(3));
        let win_base = CHILD_STRIDE * 6;
        let (h_open_windows, hits_win) = draw_card(&card, &cx, 404.0);
        assert!(!hits_win
            .list
            .iter()
            .any(|(id, _)| (games_base..games_base + CHILD_STRIDE).contains(id)));
        assert!(hits_win
            .list
            .iter()
            .any(|(id, _)| (win_base..win_base + CHILD_STRIDE).contains(id)));

        // Windows Pick row (win_base + 100) opens a list and tick advances its reveal
        assert!(card.event(
            &Event::Press {
                hit: win_base + 100,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(card.tick(16.0));
        let _ = h_open_windows;

        // Open the config file row emits EDIT_COMMAND + ClosePanel
        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: HIT_OPEN_CONFIG,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(matches!(out.first(), Some(Action::Run(cmd)) if cmd == EDIT_COMMAND));
        assert!(matches!(out.last(), Some(Action::ClosePanel)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_replaces_rows_with_children_and_in_the_file_and_quotes_editor_commands() {
        let dir = temp_home("search");
        let cfg_file = dir.join("config'with spaces");
        let def_file = dir.join("defaults");
        let real_cfg =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap();
        std::fs::write(&def_file, &real_cfg).unwrap();
        std::fs::write(
            &cfg_file,
            "GAME_MODE=\"true\"\nKEEP_ON_TOP=\"VLC\"\nCUSTOM_LINE=\"x\"\n",
        )
        .unwrap();

        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th);
        let cfg = full_config_data();
        let mut card = SettingsAllCard::with_paths(def_file, cfg_file.clone());
        apply_all_sources(&mut card, &dir, &cfg);

        let mut out = Vec::new();
        // Matches KEEP_ON_TOP via first comment line ("Windows kept on top of the others...")
        card.event(&Event::Text("kept on top"), &cx, &mut out);
        assert_eq!(card.status(&cx), Status::Ready);
        let (h_search, hits_search) = draw_card(&card, &cx, 404.0);
        assert!(h_search > 0.0);
        // Should not draw Open the config file row while searching
        assert!(!hits_search
            .list
            .iter()
            .any(|(id, _)| *id == HIT_OPEN_CONFIG));
        // KEEP_ON_TOP is in "In the file"
        assert!(hits_search.list.iter().any(|(id, _)| *id == HIT_FILE_BASE));

        let matches = card.matching_file_entries();
        assert_eq!(matches[0].key, "KEEP_ON_TOP");

        // Also test key-name search (e.g. OVERLAY_PLANES)
        card.event(&Event::Text("OVERLAY_PLANES"), &cx, &mut out);
        assert_eq!(card.matching_file_entries()[0].key, "OVERLAY_PLANES");
        card.event(&Event::Text("kept on top"), &cx, &mut out);
        let _ = draw_card(&card, &cx, 404.0);

        // Verify exact editor commands for +N editors vs non-+N editors (including shell metacharacters)
        let evil_path = dir.join("cfg $(touch pwn) 'q'");
        for ed in PLUS_LINE_EDITORS {
            let cmd = open_file_command(Some(ed), &evil_path, Some(2));
            let expected = format!(
                "sh \"$HOME/.local/bin/xdg-terminal-exec\" '{}' '+2' {}",
                ed,
                settings::sq(&evil_path.to_string_lossy())
            );
            assert_eq!(cmd, expected);
        }
        let nvim_path_cmd = open_file_command(Some("/usr/bin/nvim"), &cfg_file, Some(15));
        assert_eq!(
            nvim_path_cmd,
            format!(
                "sh \"$HOME/.local/bin/xdg-terminal-exec\" '/usr/bin/nvim' '+15' {}",
                settings::sq(&cfg_file.to_string_lossy())
            )
        );
        let hx_cmd = open_file_command(Some("/usr/bin/hx"), &cfg_file, Some(2));
        assert_eq!(
            hx_cmd,
            format!(
                "sh \"$HOME/.local/bin/xdg-terminal-exec\" '/usr/bin/hx' {}",
                settings::sq(&cfg_file.to_string_lossy())
            )
        );
        assert!(
            open_file_command(Some("nvim -p"), &cfg_file, Some(3)).contains("'nvim' '-p' '+3' ")
        );
        let top_cmd = open_file_command(None, &cfg_file, None);
        assert_eq!(
            top_cmd,
            format!(
                "sh \"$HOME/.local/bin/xdg-terminal-exec\" 'nano' {}",
                settings::sq(&cfg_file.to_string_lossy())
            )
        );

        // Clicking the "In the file" hit runs command + ClosePanel
        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: HIT_FILE_BASE,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(matches!(
            out.as_slice(),
            [Action::Run(cmd), Action::ClosePanel] if cmd.contains("xdg-terminal-exec")
                && cmd.contains(&settings::sq(&cfg_file.to_string_lossy()))
        ));

        // Searching "tearing" (a setting on Games card) matches Games child, not "In the file"
        card.event(&Event::Text("tearing"), &cx, &mut out);
        assert!(card.matching_file_entries().is_empty());
        assert_eq!(card.status(&cx), Status::Ready);
        let (h_child_only, hits_child) = draw_card(&card, &cx, 404.0);
        assert!(h_child_only > HEADER_H);
        let games_base = CHILD_STRIDE * 9;
        assert!(hits_child
            .list
            .iter()
            .any(|(id, _)| (games_base..games_base + CHILD_STRIDE).contains(id)));
        let bottom_search = hits_child
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0, f64::max);
        assert!((bottom_search - h_child_only).abs() < 0.5);

        // Searching rows inside Personalization finds each child's rows individually (Appearance vs Wallpaper vs Fonts)
        card.event(&Event::Text("corners"), &cx, &mut out);
        let (_, hits_corners) = draw_card(&card, &cx, 404.0);
        assert!(hits_corners
            .list
            .iter()
            .any(|(id, _)| (CHILD_STRIDE..2 * CHILD_STRIDE).contains(id)));
        assert!(!hits_corners
            .list
            .iter()
            .any(|(id, _)| (2 * CHILD_STRIDE..4 * CHILD_STRIDE).contains(id)));

        card.event(&Event::Text("fit"), &cx, &mut out);
        let (_, hits_fit) = draw_card(&card, &cx, 404.0);
        assert!(hits_fit
            .list
            .iter()
            .any(|(id, _)| (2 * CHILD_STRIDE..3 * CHILD_STRIDE).contains(id)));

        card.event(&Event::Text("cursor"), &cx, &mut out);
        let (_, hits_cursor) = draw_card(&card, &cx, 404.0);
        assert!(hits_cursor
            .list
            .iter()
            .any(|(id, _)| (3 * CHILD_STRIDE..4 * CHILD_STRIDE).contains(id)));

        // Unmatched query hides card
        card.event(&Event::Text("zzzz-no-such-setting-xyz"), &cx, &mut out);
        assert_eq!(card.status(&cx), Status::Hidden);

        // Clearing query returns to normal mode
        card.event(&Event::Text(""), &cx, &mut out);
        assert_eq!(card.status(&cx), Status::Ready);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn builtin_panel_drag_release_and_safe_shell_invocation() {
        let cfg_builtin = Config::from_text("");
        let panel = cfg_builtin
            .panels
            .iter()
            .find(|p| p.name == "settings")
            .unwrap();
        assert_eq!(
            panel.words,
            vec!["apps/search=settings", "settings/all", "settings/about"]
        );

        let dir = temp_home("exec");
        let bin_dir = dir.join(".local/bin");
        let marker = dir.join("pwned");
        let args_log = dir.join("args.log");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let xte = bin_dir.join("xdg-terminal-exec");
        std::fs::write(
            &xte,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n",
                settings::sq(&args_log.to_string_lossy())
            ),
        )
        .unwrap();

        let evil_file = dir.join(format!("config $(touch {}) 'q'", marker.display()));
        let cmd = open_file_command(Some("nvim"), &evil_file, Some(77));
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .env("HOME", &dir)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!marker.exists(), "metacharacters executed in shell");
        let logged = std::fs::read_to_string(&args_log).unwrap();
        let lines: Vec<&str> = logged.lines().collect();
        assert_eq!(lines, vec!["nvim", "+77", &evil_file.to_string_lossy()]);

        // Appearance open -> right-click returns false, slider Press+Drag+Release forwards to child
        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th);
        let cfg = full_config_data();
        let mut card = SettingsAllCard::with_paths(dir.join("missing"), dir.join("missing_cfg"));
        apply_all_sources(&mut card, &dir, &cfg);
        let mut out = Vec::new();
        assert!(!card.event(
            &Event::Press {
                hit: HIT_GROUP_BASE,
                button: Button::Right,
                at: (5.0, 5.0),
            },
            &cx,
            &mut out
        ));
        assert!(card.event(
            &Event::Press {
                hit: HIT_GROUP_BASE,
                button: Button::Left,
                at: (5.0, 5.0),
            },
            &cx,
            &mut out
        ));
        let (_, hits) = draw_card(&card, &cx, 404.0);
        // Appearance WINDOW_CORNERS slider: row 3 -> 100 + 30 + 1 = 131 -> 100_000 + 131 = 100_131
        let slide_hit = CHILD_STRIDE + 131;
        let slide_rect = hits
            .list
            .iter()
            .find(|(id, _)| *id == slide_hit)
            .map(|(_, r)| *r)
            .expect("corners slider hit");
        let at = (slide_rect.0 + slide_rect.2 * 0.5, slide_rect.1 + 4.0);
        assert!(card.event(
            &Event::Press {
                hit: slide_hit,
                button: Button::Left,
                at,
            },
            &cx,
            &mut out
        ));
        let _ = card.event(
            &Event::Drag {
                hit: slide_hit,
                at: (slide_rect.0 + slide_rect.2 * 0.8, slide_rect.1 + 4.0),
            },
            &cx,
            &mut out,
        );
        let _ = card.event(&Event::Release { hit: slide_hit }, &cx, &mut out);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn personalization_groups_three_cards_builds_summary_and_keeps_hits_disjoint() {
        let dir = temp_home("personalization");
        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th);
        let mut cfg = full_config_data();
        cfg.now.insert("COLOR_SCHEME".into(), "prefer-dark".into());
        cfg.now.insert("BORDER_ACTIVE".into(), "#cc70a9".into());
        cfg.now.insert(
            "WALLPAPER".into(),
            "~/Pictures/Wallpapers/aurora.jpg".into(),
        );

        let mut card = SettingsAllCard::with_paths(dir.join("missing"), dir.join("missing_cfg"));
        apply_all_sources(&mut card, &dir, &cfg);
        card.apply(
            "wallpapers",
            &WallpapersData {
                has_bin: true,
                files: vec![
                    crate::cards::settings_look::WallpaperFile {
                        name: "aurora.jpg".into(),
                        path: dir
                            .join("Pictures/Wallpapers/aurora.jpg")
                            .to_string_lossy()
                            .into_owned(),
                    },
                    crate::cards::settings_look::WallpaperFile {
                        name: "dusk.png".into(),
                        path: dir
                            .join("Pictures/Wallpapers/dusk.png")
                            .to_string_lossy()
                            .into_owned(),
                    },
                ],
                ..WallpapersData::default()
            } as &dyn Any,
        );

        assert_eq!(card.summary(0, &cx), "Dark · #cc70a9 · aurora.jpg");

        // Open Personalization (group 0).
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: HIT_GROUP_BASE,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out
        ));
        let (h_open, hits) = draw_card(&card, &cx, 420.0);
        // All three child ranges must appear, and every emitted hit ID must be unique.
        let mut seen = HashSet::new();
        for (id, _) in &hits.list {
            assert!(
                seen.insert(*id),
                "duplicate hit id {id} when Personalization is open"
            );
        }
        assert!(hits
            .list
            .iter()
            .any(|(id, _)| (CHILD_STRIDE..2 * CHILD_STRIDE).contains(id)));
        assert!(hits
            .list
            .iter()
            .any(|(id, _)| (2 * CHILD_STRIDE..3 * CHILD_STRIDE).contains(id)));
        assert!(hits
            .list
            .iter()
            .any(|(id, _)| (3 * CHILD_STRIDE..4 * CHILD_STRIDE).contains(id)));
        let bottom = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        assert!((bottom - h_open).abs() < 1.0);

        // Expand Fonts' More (child 2 -> 300_000 + 9000) and Appearance's More (child 0 -> 109_000) independently.
        assert!(card.event(
            &Event::Press {
                hit: 3 * CHILD_STRIDE + 9000,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(card.event(
            &Event::Press {
                hit: CHILD_STRIDE + 9000,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));

        // Click second Wallpaper thumbnail (child 1 -> 200_000 + 1001) sets WALLPAPER.
        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: 2 * CHILD_STRIDE + 1001,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(out
            .iter()
            .any(|a| matches!(a, Action::Run(cmd) if cmd.contains("set WALLPAPER") && cmd.contains("dusk.png"))));

        // Without infynitae-wallpaper, Wallpaper hides while Personalization keeps Appearance + Fonts.
        card.apply(
            "wallpapers",
            &WallpapersData {
                has_bin: false,
                ..WallpapersData::default()
            } as &dyn Any,
        );
        let (_, hits_no_wp) = draw_card(&card, &cx, 420.0);
        assert!(hits_no_wp
            .list
            .iter()
            .any(|(id, _)| (CHILD_STRIDE..2 * CHILD_STRIDE).contains(id)));
        assert!(!hits_no_wp
            .list
            .iter()
            .any(|(id, _)| (2 * CHILD_STRIDE..3 * CHILD_STRIDE).contains(id)));
        assert!(hits_no_wp
            .list
            .iter()
            .any(|(id, _)| (3 * CHILD_STRIDE..4 * CHILD_STRIDE).contains(id)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keys_group_is_registered_after_windows_and_covers_bind_and_exec() {
        assert!(on_any_card("BIND_TERMINAL"));
        assert!(on_any_card("EXEC_CUSTOM"));

        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th);
        let cfg = crate::cards::settings_rows::ConfigData {
            now: parse_config(""),
            default: HashMap::new(),
            programs: Default::default(),
        };
        let mut card = SettingsAllCard::with_paths(
            PathBuf::from("/nonexistent/d"),
            PathBuf::from("/nonexistent/c"),
        );
        card.apply("config", &cfg);
        assert_eq!(card.summary(4, &cx), "Defaults");

        let cfg2 = crate::cards::settings_rows::ConfigData {
            now: parse_config("BIND_TERMINAL=\"SUPER+T\"\n"),
            default: HashMap::new(),
            programs: Default::default(),
        };
        card.apply("config", &cfg2);
        assert_eq!(card.summary(4, &cx), "1 custom");

        // Click Keys group (group 4, child 6 -> child hits from CHILD_STRIDE * 7).
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: HIT_GROUP_BASE + 4,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out
        ));
        let (_, hits) = draw_card(&card, &cx, 404.0);
        // Keys row 0 hit is HIT_ROW_BASE (10) -> CHILD_STRIDE * 7 + 10.
        assert!(hits.list.iter().any(|(id, _)| *id == CHILD_STRIDE * 7 + 10));
        assert!(card.event(
            &Event::Press {
                hit: CHILD_STRIDE * 7 + 10,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out
        ));
        assert!(out
            .iter()
            .any(|a| matches!(a, Action::Run(cmd) if cmd.contains("capture-chord"))));
    }

    /// Each one-card group opens the card it is named after, and every card is in one group.
    #[test]
    fn groups_point_at_the_cards_they_name() {
        let mut seen = Vec::new();
        for g in GROUPS.iter() {
            if g.children.len() == 1 {
                assert_eq!(g.title, CHILD_KINDS[g.children[0]].title);
            }
            seen.extend_from_slice(g.children);
        }
        seen.sort_unstable();
        assert_eq!(seen, (0..CHILD_KINDS.len()).collect::<Vec<_>>());
    }
}
