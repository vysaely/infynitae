//! App cards: installed app list, favorites, search, and background tray items.
//!
//! Replaces `edge-rs`'s `apps`, `favorites`, `search`, and `tray` modules on the `Card` trait.
//! `BackgroundSource` reads `$XDG_RUNTIME_DIR/infynitae/capturing` and session D-Bus
//! `StatusNotifierWatcher` items off the UI thread (with `INFYNITAE_EDGE_RENDER_DATA` fixtures
//! supported for offline rendering).

use std::any::Any;
use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cairo::Context;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::app::App;
use crate::card::{run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::theme::{Rgba, Theme};
use crate::ui;

const ROW_H: f64 = ui::ROW_H;
const SUB_H: f64 = 22.0;
const TILE_SZ: f64 = 28.0;
const FAV_ICON_SZ: f64 = 44.0;
const FAV_CELL_H: f64 = 66.0;
const FAV_ROW_GAP: f64 = 8.0;
const MAX_FAVORITES: usize = 24;
const LIST_M_ROWS: usize = 6;
const LIST_L_ROWS: usize = 12;
const SEARCH_BAR_H: f64 = 42.0;
const SEARCH_GAP: f64 = 6.0;
const SEARCH_MAX_RESULTS: usize = 6;
const BG_MAX_H: f64 = 6.0 * ui::ROW_H;

const H_SEARCH_FIELD: u32 = 0;
const H_SEARCH_CLEAR: u32 = 1;
const H_SEARCH_RESULT_BASE: u32 = 10;

const CATEGORY_ORDER: &[&str] = &[
    "Games",
    "Internet",
    "Media",
    "Graphics",
    "Development",
    "Office",
    "Utilities",
    "System",
    "Settings",
    "Education",
    "Science",
    "Other",
];

const KINDS: &[CardKind] = &[
    CardKind {
        id: "list",
        title: "Apps",
        about: "Installed applications grouped by category",
        sizes: &[Size::M, Size::L],
        default: Size::M,
        half: false,
        sources: &[],
    },
    CardKind {
        id: "favorites",
        title: "Favorites",
        about: "Pinned apps as a row or grid of icons",
        sizes: &[Size::S, Size::M],
        default: Size::S,
        half: false,
        sources: &[],
    },
    CardKind {
        id: "search",
        title: "Search",
        about: "Filter and launch installed apps",
        sizes: &[Size::S, Size::M],
        default: Size::S,
        half: false,
        sources: &[],
    },
    CardKind {
        id: "background",
        title: "Background",
        about: "Apps running in the system tray or capturing the screen",
        sizes: &[Size::S],
        default: Size::S,
        half: false,
        sources: &["background"],
    },
    crate::cards::tray_menu::KIND,
    crate::cards::search::KIND,
];

pub struct Apps;

impl Provider for Apps {
    fn id(&self) -> &'static str {
        "apps"
    }

    fn name(&self) -> &'static str {
        "Apps"
    }

    fn claims(&self) -> &'static [Claim] {
        &[]
    }

    fn listed(&self) -> Listed {
        Listed::Always
    }

    fn cards(&self) -> &'static [CardKind] {
        KINDS
    }

    fn card(&self, kind: &str, argument: Option<&str>) -> Option<Box<dyn Card>> {
        match kind {
            "list" if argument.is_none() => Some(Box::new(AppsListCard::default())),
            "favorites" => Some(Box::new(FavoritesCard::new(argument))),
            "search" if argument.is_none() => Some(Box::new(SearchCard::default())),
            "search" if argument == Some("settings") => Some(Box::new(SearchCard {
                settings: true,
                ..SearchCard::default()
            })),
            "background" if argument.is_none() => Some(Box::new(BackgroundCard::default())),
            "results" if argument.is_none() => {
                Some(Box::new(crate::cards::search::ResultsCard::default()))
            }
            "menu" => argument
                .map(|a| Box::new(crate::cards::tray_menu::MenuCard::new(a)) as Box<dyn Card>),
            _ => None,
        }
    }

    fn source(&self, key: &str) -> Option<Box<dyn Source>> {
        match key {
            "background" | "tray" => Some(Box::new(BackgroundSource::default())),
            "traymenu" => Some(Box::new(crate::cards::tray_menu::MenusSource)),
            "search" => Some(Box::new(crate::cards::search::SearchSource::default())),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Shared drawing and matching helpers

/// Fallback icon tile when an app has no icon or its tile uses the accent.
fn draw_tile(
    cr: &Context,
    th: &Theme,
    label: &str,
    fallback: &str,
    sq: (f64, f64, f64),
    accent: bool,
) {
    let (x, y, size) = sq;
    draw::round_rect(cr, x, y, size, size, th.small_radius());
    draw::set(cr, if accent { th.accent } else { th.track });
    let _ = cr.fill();

    let ch = label
        .chars()
        .chain(fallback.chars())
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    let font_sz = (size * 0.5).round();
    let fg = if accent { th.on_accent } else { th.text };
    let t = Text {
        text: &ch,
        size: font_sz,
        bold: true,
        color: fg,
    };
    let (_, th_h) = draw::measure(cr, &t);
    draw::text_center(cr, x + size / 2.0, y + (size - th_h) / 2.0, &t);
}

pub(crate) fn draw_subhead(cr: &Context, th: &Theme, body: Rect, y: f64, label: &str) {
    draw::text_max(
        cr,
        body.0 + 8.0,
        y + 4.0,
        (body.2 - 16.0).max(0.0),
        &Text {
            text: label,
            size: 11.0,
            bold: true,
            color: th.muted,
        },
    );
}

struct EntryRow<'a> {
    hit_id: u32,
    row_r: Rect,
    hit_r: Rect,
    force_sel: bool,
    name: &'a str,
    fallback: &'a str,
    /// the icon's name from the desktop entry or the tray item ("" for none)
    icon: &'a str,
    tile_accent: bool,
    note: &'a str,
    note_accent: bool,
}

fn draw_row_entry(cr: &Context, cx: &Cx, hits: &mut Hits, e: &EntryRow<'_>) {
    let hot = hits.add(e.hit_id, e.hit_r);
    let sel = e.force_sel || hits.selected(e.hit_id);
    if hot || sel {
        ui::light(cr, cx, e.row_r);
    }
    let th = cx.theme;
    if sel {
        draw::round_rect(
            cr,
            e.row_r.0 + 2.0,
            e.row_r.1 + 9.0,
            2.5,
            e.row_r.3 - 18.0,
            1.25,
        );
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }

    let tx = e.row_r.0 + 8.0;
    let ty = e.row_r.1 + (e.row_r.3 - TILE_SZ) / 2.0;
    // the app's icon; a tile with its letter (in the accent when it asks for attention) without one
    if e.icon.is_empty() || e.tile_accent {
        draw_tile(cr, th, e.name, e.fallback, (tx, ty, TILE_SZ), e.tile_accent);
    } else {
        crate::icons::draw_app_icon(cr, cx, e.icon, e.name, tx, ty, TILE_SZ);
    }

    let text_x = tx + TILE_SZ + 10.0;
    let mut name_max = (e.row_r.0 + e.row_r.2 - 8.0 - text_x).max(0.0);

    if !e.note.is_empty() && name_max > 110.0 {
        let ns = Text {
            text: e.note,
            size: 11.0,
            bold: e.note_accent,
            color: if e.note_accent {
                th.accent_text
            } else {
                th.muted
            },
        };
        let (nw, nh) = draw::measure(cr, &ns);
        let note_w = nw.min((e.row_r.2 * 0.36).max(48.0));
        draw::text_max(
            cr,
            e.row_r.0 + e.row_r.2 - 8.0 - note_w,
            e.row_r.1 + (e.row_r.3 - nh) / 2.0,
            note_w,
            &ns,
        );
        name_max = (name_max - note_w - 10.0).max(0.0);
    }

    let ag = Text {
        text: "Ag",
        size: 13.0,
        bold: false,
        color: th.text,
    };
    let (_, nh) = draw::measure(cr, &ag);
    draw::text_max(
        cr,
        text_x,
        e.row_r.1 + (e.row_r.3 - nh) / 2.0,
        name_max,
        &Text {
            text: e.name,
            size: 13.0,
            bold: false,
            color: th.text,
        },
    );
}

fn clamped_scroll(scroll: f64, total_h: f64, cap_h: f64) -> f64 {
    scroll.clamp(0.0, (total_h - cap_h).max(0.0))
}

fn apply_scroll(scroll: &mut f64, dy: f64, total_h: f64, cap_h: f64) -> bool {
    let max = (total_h - cap_h).max(0.0);
    if max <= 0.0 {
        return false;
    }
    let prev = clamped_scroll(*scroll, total_h, cap_h);
    let next = (prev + dy).clamp(0.0, max);
    *scroll = next;
    (next - prev).abs() > f64::EPSILON
}

fn draw_search_icon(cr: &Context, cx_pos: f64, cy_pos: f64, r: f64, color: Rgba) {
    let _ = cr.save();
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.arc(cx_pos, cy_pos, r, 0.0, std::f64::consts::TAU);
    let _ = cr.stroke();
    let d = r * std::f64::consts::FRAC_1_SQRT_2;
    cr.move_to(cx_pos + d, cy_pos + d);
    cr.line_to(cx_pos + d + 4.5, cy_pos + d + 4.5);
    let _ = cr.stroke();
    let _ = cr.restore();
}

fn strip_desktop_suffix(s: &str) -> &str {
    s.strip_suffix(".desktop").unwrap_or(s)
}

/// Maps XDG desktop categories into one primary group (priority order prevents duplicate rows when
/// entries declare multiple categories such as `Network;FileTransfer;Game`).
fn app_category(app: &App) -> &'static str {
    let has = |needle: &str| {
        app.categories
            .iter()
            .any(|c| c.eq_ignore_ascii_case(needle))
    };
    if has("Game") {
        "Games"
    } else if has("AudioVideo") || has("Audio") || has("Video") {
        "Media"
    } else if has("Graphics") {
        "Graphics"
    } else if has("Development") {
        "Development"
    } else if has("Office") {
        "Office"
    } else if has("Education") {
        "Education"
    } else if has("Science") {
        "Science"
    } else if has("Network") || has("WebBrowser") {
        "Internet"
    } else if has("Settings") || has("DesktopSettings") {
        "Settings"
    } else if has("System") || has("TerminalEmulator") || has("Monitor") {
        "System"
    } else if has("Utility") || has("Accessories") || has("FileManager") {
        "Utilities"
    } else {
        "Other"
    }
}

// ---------------------------------------------------------------------------------------------
// apps/list

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OrderedApp {
    category: &'static str,
    app_idx: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListItem {
    Header(&'static str),
    App(usize),
}

impl ListItem {
    fn height(self) -> f64 {
        match self {
            ListItem::Header(_) => SUB_H,
            ListItem::App(_) => ROW_H,
        }
    }
}

fn grouped_apps(apps: &[App]) -> Vec<OrderedApp> {
    let mut grouped: Vec<Vec<usize>> = vec![Vec::new(); CATEGORY_ORDER.len()];
    for (idx, app) in apps.iter().enumerate() {
        let cat = app_category(app);
        let b = CATEGORY_ORDER
            .iter()
            .position(|&c| c == cat)
            .unwrap_or(CATEGORY_ORDER.len() - 1);
        grouped[b].push(idx);
    }

    let mut out = Vec::with_capacity(apps.len());
    for (b, indices) in grouped.into_iter().enumerate() {
        for idx in indices {
            out.push(OrderedApp {
                category: CATEGORY_ORDER[b],
                app_idx: idx,
            });
        }
    }
    out
}

#[derive(Default)]
struct AppsListCard {
    scroll: usize,
    scroll_accum: f64,
    /// what was typed in the panel: the list shows the matches, best first, without categories
    query: String,
}

impl AppsListCard {
    fn max_rows(size: Size) -> usize {
        match size {
            Size::L => LIST_L_ROWS,
            Size::S | Size::M => LIST_M_ROWS,
        }
    }

    /// The apps matching the query, best first, by their index in `cx.apps`.
    fn matches(&self, cx: &Cx) -> Vec<usize> {
        search_apps(cx.apps, cx.recent, &self.query)
            .into_iter()
            .filter_map(|m| cx.apps.iter().position(|a| std::ptr::eq(a, m)))
            .collect()
    }

    fn visible_window(&self, cx: &Cx) -> (Vec<ListItem>, usize, usize) {
        if !self.query.is_empty() {
            let found = self.matches(cx);
            let total = found.len();
            let cap = Self::max_rows(cx.size);
            let start = self.scroll.min(total.saturating_sub(cap));
            let items = found[start..(start + cap).min(total)]
                .iter()
                .map(|&i| ListItem::App(i))
                .collect();
            return (items, total, start);
        }
        let ordered = grouped_apps(cx.apps);
        let total = ordered.len();
        let cap = Self::max_rows(cx.size);
        let max_scroll = total.saturating_sub(cap);
        let start = self.scroll.min(max_scroll);
        let slice = &ordered[start..(start + cap).min(total)];

        let mut items = Vec::with_capacity(slice.len() * 2);
        let mut prev_cat: Option<&'static str> = None;
        for entry in slice {
            if prev_cat != Some(entry.category) {
                items.push(ListItem::Header(entry.category));
                prev_cat = Some(entry.category);
            }
            items.push(ListItem::App(entry.app_idx));
        }
        (items, total, start)
    }

    fn is_app_visible(&self, cx: &Cx, app_idx: usize) -> bool {
        self.visible_window(cx)
            .0
            .into_iter()
            .any(|it| it == ListItem::App(app_idx))
    }
}

impl Card for AppsListCard {
    fn apply(&mut self, _key: &str, _data: &dyn Any) -> bool {
        false
    }

    fn status(&self, cx: &Cx) -> Status {
        if cx.apps.is_empty() {
            Status::Missing("No applications installed".into())
        } else if !self.query.is_empty() && self.matches(cx).is_empty() {
            // nothing matches what was typed: the list steps aside
            Status::Hidden
        } else {
            Status::Ready
        }
    }

    fn title(&self, cx: &Cx) -> Option<String> {
        if self.query.is_empty() {
            return None;
        }
        let n = self.matches(cx).len();
        Some(if n == 1 {
            "1 app".to_string()
        } else {
            format!("{n} apps")
        })
    }

    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        let (items, _, _) = self.visible_window(cx);
        items.iter().map(|it| it.height()).sum()
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let (items, total, start) = self.visible_window(cx);
        if items.is_empty() {
            return;
        }
        let mut y = body.1;
        for it in items {
            let h = it.height();
            match it {
                ListItem::Header(label) => draw_subhead(cr, cx.theme, body, y, label),
                ListItem::App(app_idx) => {
                    let Some(app) = cx.apps.get(app_idx) else {
                        y += h;
                        continue;
                    };
                    let row_r = (body.0, y, body.2, h);
                    draw_row_entry(
                        cr,
                        cx,
                        hits,
                        &EntryRow {
                            hit_id: app_idx as u32,
                            row_r,
                            hit_r: row_r,
                            // Enter launches the best match: it shows marked
                            force_sel: !self.query.is_empty() && start == 0 && y == body.1,
                            name: &app.name,
                            fallback: &app.id,
                            icon: &app.icon,
                            tile_accent: false,
                            note: "",
                            note_accent: false,
                        },
                    );
                }
            }
            y += h;
        }
        let cap = Self::max_rows(cx.size);
        if total > cap && cap > 0 {
            let scale = body.3 / cap as f64;
            ui::scrollbar(cr, cx, body, total as f64 * scale, start as f64 * scale);
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Text(q) => {
                if self.query == *q {
                    return false;
                }
                self.query = (*q).to_string();
                self.scroll = 0;
                true
            }
            // Enter with nothing selected: the best match
            Event::Enter => {
                let Some(&i) = self.matches(cx).first().filter(|_| !self.query.is_empty()) else {
                    return false;
                };
                out.push(Action::Launch(cx.apps[i].id.clone()));
                out.push(Action::ClosePanel);
                true
            }
            Event::Scroll { dy, notches, .. } => {
                let total = if self.query.is_empty() {
                    cx.apps.len()
                } else {
                    self.matches(cx).len()
                };
                let max = total.saturating_sub(Self::max_rows(cx.size));
                crate::card::scroll_rows(
                    &mut self.scroll,
                    &mut self.scroll_accum,
                    max,
                    *dy,
                    *notches,
                )
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } => {
                let idx = *hit as usize;
                let Some(app) = cx.apps.get(idx) else {
                    return false;
                };
                if !self.is_app_visible(cx, idx) {
                    return false;
                }
                out.push(Action::Launch(app.id.clone()));
                out.push(Action::ClosePanel);
                true
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// apps/favorites

fn parse_favorite_ids(argument: Option<&str>) -> Vec<String> {
    let Some(arg) = argument else {
        return Vec::new();
    };
    arg.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn find_favorite_app<'a>(apps: &'a [App], token: &str) -> Option<&'a App> {
    let want = strip_desktop_suffix(token.trim());
    if want.is_empty() {
        return None;
    }
    apps.iter()
        .find(|a| strip_desktop_suffix(&a.id).eq_ignore_ascii_case(want))
        .or_else(|| apps.iter().find(|a| a.name.eq_ignore_ascii_case(token)))
}

struct FavoritesCard {
    ids: Vec<String>,
    /// Tracks column count from the latest layout width so `event` enforces `Size::S` row cap.
    cols_hint: Cell<usize>,
}

impl FavoritesCard {
    fn new(argument: Option<&str>) -> Self {
        Self {
            ids: parse_favorite_ids(argument),
            cols_hint: Cell::new(5),
        }
    }

    fn resolved<'a>(&self, apps: &'a [App]) -> Vec<&'a App> {
        let mut out = Vec::new();
        for token in &self.ids {
            let Some(app) = find_favorite_app(apps, token) else {
                continue;
            };
            if out.iter().any(|existing: &&App| existing.id == app.id) {
                continue;
            }
            out.push(app);
            if out.len() >= MAX_FAVORITES {
                break;
            }
        }
        out
    }

    fn cols_for_width(width: f64) -> usize {
        ((width / 62.0).floor() as usize).clamp(3, 6)
    }

    fn visible<'a>(&self, cx: &Cx<'a>, cols: usize) -> Vec<&'a App> {
        let mut list = self.resolved(cx.apps);
        if cx.size == Size::S {
            list.truncate(cols);
        }
        list
    }
}

impl Card for FavoritesCard {
    fn apply(&mut self, _key: &str, _data: &dyn Any) -> bool {
        false
    }

    fn status(&self, cx: &Cx) -> Status {
        if self.resolved(cx.apps).is_empty() {
            Status::Hidden
        } else {
            Status::Ready
        }
    }

    fn height(&self, cx: &Cx, width: f64) -> f64 {
        let cols = Self::cols_for_width(width);
        self.cols_hint.set(cols);
        let count = self.visible(cx, cols).len();
        if count == 0 {
            return 0.0;
        }
        let rows = count.div_ceil(cols);
        rows as f64 * FAV_CELL_H + (rows.saturating_sub(1)) as f64 * FAV_ROW_GAP
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let cols = Self::cols_for_width(body.2);
        self.cols_hint.set(cols);
        let vis = self.visible(cx, cols);
        if vis.is_empty() {
            return;
        }
        let cell_w = body.2 / cols as f64;
        for (i, app) in vis.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let x = body.0 + col as f64 * cell_w;
            let y = body.1 + row as f64 * (FAV_CELL_H + FAV_ROW_GAP);
            let cell = (x, y, cell_w, FAV_CELL_H);
            let hit_id = i as u32;
            let hot = hits.add(hit_id, cell);
            let sel = hits.selected(hit_id);
            if hot || sel {
                ui::light(cr, cx, cell);
            }

            let icon_x = x + (cell_w - FAV_ICON_SZ) / 2.0;
            let icon_y = y + 3.0;
            crate::icons::draw_app_icon(cr, cx, &app.icon, &app.name, icon_x, icon_y, FAV_ICON_SZ);

            let lbl = Text {
                text: &app.name,
                size: draw::SIZE_NOTE,
                bold: false,
                color: if hot || sel {
                    cx.theme.text
                } else {
                    cx.theme.muted
                },
            };
            let (lw, _) = draw::measure(cr, &lbl);
            let max_w = (cell_w - 6.0).max(0.0);
            let w = lw.min(max_w);
            draw::text_max(
                cr,
                x + (cell_w - w) / 2.0,
                icon_y + FAV_ICON_SZ + 4.0,
                w,
                &lbl,
            );
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Event::Press {
            hit,
            button: Button::Left,
            ..
        } = ev
        else {
            return false;
        };
        let cols = self.cols_hint.get();
        let vis = self.visible(cx, cols);
        let Some(app) = vis.get(*hit as usize) else {
            return false;
        };
        out.push(Action::Launch(app.id.clone()));
        out.push(Action::ClosePanel);
        true
    }
}

// ---------------------------------------------------------------------------------------------
// apps/search

fn word_starts_with(hay: &str, needle: &str) -> bool {
    hay.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(needle))
}

fn subseq_match(needle: &str, hay: &str) -> bool {
    let mut chars = needle.chars();
    let Some(mut cur) = chars.next() else {
        return false;
    };
    for h in hay.chars() {
        if h == cur {
            let Some(next) = chars.next() else {
                return true;
            };
            cur = next;
        }
    }
    false
}

fn score_term(app: &App, term: &str) -> Option<u8> {
    let name = app.name.to_lowercase();
    if name == term {
        return Some(0);
    }
    if name.starts_with(term) {
        return Some(1);
    }
    if word_starts_with(&name, term) {
        return Some(2);
    }
    if name.contains(term) {
        return Some(3);
    }

    let id = strip_desktop_suffix(&app.id).to_lowercase();
    if id == term || id.starts_with(term) {
        return Some(4);
    }
    if id.contains(term) {
        return Some(5);
    }

    let cat_group = app_category(app).to_lowercase();
    if cat_group.starts_with(term)
        || app
            .categories
            .iter()
            .any(|c| c.to_lowercase().starts_with(term))
    {
        return Some(6);
    }
    if cat_group.contains(term)
        || app
            .categories
            .iter()
            .any(|c| c.to_lowercase().contains(term))
    {
        return Some(7);
    }

    if subseq_match(term, &name) {
        return Some(8);
    }
    None
}

/// The apps matching every word of `query`, best match first; within one match quality the apps
/// started often and lately (`Recent::scores`) first, then by name. Nothing typed: none.
pub fn search_apps<'a>(
    apps: &'a [App],
    recent: &crate::recent::Recent,
    query: &str,
) -> Vec<&'a App> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let terms: Vec<String> = trimmed.split_whitespace().map(str::to_lowercase).collect();
    if terms.is_empty() {
        return Vec::new();
    }

    let started = recent.scores();
    let mut ranked: Vec<(u16, f64, String, &App)> = Vec::new();
    for app in apps {
        let mut total_score = 0u16;
        let mut matched = true;
        for term in &terms {
            let Some(s) = score_term(app, term) else {
                matched = false;
                break;
            };
            total_score += u16::from(s);
        }
        if matched {
            let lately = started.get(app.id.as_str()).copied().unwrap_or(0.0);
            ranked.push((total_score, lately, app.name.to_lowercase(), app));
        }
    }
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.1.total_cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.3.id.cmp(&b.3.id))
    });
    ranked.into_iter().map(|(_, _, _, app)| app).collect()
}

#[derive(Default)]
struct SearchCard {
    query: String,
    /// "settings" (`apps/search=settings`, the Settings panel): the field says what it searches
    settings: bool,
}

impl Card for SearchCard {
    fn apply(&mut self, _key: &str, _data: &dyn Any) -> bool {
        false
    }

    /// The field says what it is: at its smallest it goes without the header.
    fn header(&self, cx: &Cx) -> bool {
        cx.size != Size::S
    }

    fn status(&self, _cx: &Cx) -> Status {
        Status::Ready
    }

    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        if cx.size == Size::S || self.query.trim().is_empty() {
            return SEARCH_BAR_H;
        }
        let matches = search_apps(cx.apps, cx.recent, &self.query);
        let results_h = if matches.is_empty() {
            ui::LIST_ROW_H
        } else {
            matches.len().min(SEARCH_MAX_RESULTS) as f64 * ROW_H
        };
        SEARCH_BAR_H + SEARCH_GAP + results_h
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let th = cx.theme;
        let trimmed = self.query.trim();
        let active = !trimmed.is_empty();
        let matches = if cx.size != Size::S && active {
            search_apps(cx.apps, cx.recent, &self.query)
        } else {
            Vec::new()
        };

        // Register results first (disjoint vertically from the field) so result 0 sits first in hits.
        let mut y = body.1 + SEARCH_BAR_H + SEARCH_GAP;
        if cx.size != Size::S && active {
            if matches.is_empty() {
                let empty_t = Text {
                    text: "No matching apps",
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                };
                let (_, eh) = draw::measure(cr, &empty_t);
                draw::text_max(
                    cr,
                    body.0 + 10.0,
                    y + (ui::LIST_ROW_H - eh) / 2.0,
                    (body.2 - 20.0).max(0.0),
                    &empty_t,
                );
            } else {
                for (i, app) in matches.iter().take(SEARCH_MAX_RESULTS).enumerate() {
                    let row_r = (body.0, y, body.2, ROW_H);
                    let hit_id = H_SEARCH_RESULT_BASE + i as u32;
                    draw_row_entry(
                        cr,
                        cx,
                        hits,
                        &EntryRow {
                            hit_id,
                            row_r,
                            hit_r: row_r,
                            force_sel: i == 0,
                            name: &app.name,
                            fallback: &app.id,
                            icon: &app.icon,
                            tile_accent: false,
                            note: "",
                            note_accent: false,
                        },
                    );
                    y += ROW_H;
                }
            }
        }

        let bar = (body.0, body.1, body.2, SEARCH_BAR_H);
        let field_hot = hits.add(H_SEARCH_FIELD, bar);
        let field_sel = hits.selected(H_SEARCH_FIELD);

        let clear_w = if self.query.is_empty() { 0.0 } else { 48.0 };
        let placeholder = if self.settings {
            "Search settings"
        } else {
            "Search apps"
        };
        ui::text_field(
            cr,
            th,
            bar,
            &ui::TextField {
                value: &self.query,
                placeholder,
                focused: active || field_hot || field_sel,
                show_caret: !self.query.is_empty(),
                left_pad: 34.0,
                right_pad: 10.0 + clear_w,
            },
        );

        let icon_col = if active { th.accent_text } else { th.muted };
        draw_search_icon(cr, bar.0 + 18.0, bar.1 + bar.3 / 2.0 - 1.5, 5.5, icon_col);

        if !self.query.is_empty() {
            let clear_r = (
                bar.0 + bar.2 - clear_w - 4.0,
                bar.1 + 4.0,
                clear_w,
                bar.3 - 8.0,
            );
            let clear_hot = hits.add(H_SEARCH_CLEAR, clear_r);
            let clear_sel = hits.selected(H_SEARCH_CLEAR);
            if clear_hot || clear_sel {
                ui::light(cr, cx, clear_r);
            }
            let ct = Text {
                text: "Clear",
                size: draw::SIZE_SMALL,
                bold: false,
                color: if clear_hot || clear_sel {
                    th.text
                } else {
                    th.muted
                },
            };
            let (_, ch) = draw::measure(cr, &ct);
            draw::text_center(
                cr,
                clear_r.0 + clear_r.2 / 2.0,
                clear_r.1 + (clear_r.3 - ch) / 2.0,
                &ct,
            );
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Text(s) => {
                if self.query == *s {
                    false
                } else {
                    self.query = (*s).to_string();
                    true
                }
            }
            Event::Press {
                hit: H_SEARCH_CLEAR,
                button: Button::Left,
                ..
            } => {
                if self.query.is_empty() {
                    false
                } else {
                    // the panel's query too, or the next letter brings the old text back
                    self.query.clear();
                    out.push(Action::SetQuery(String::new()));
                    true
                }
            }
            Event::Press {
                hit: H_SEARCH_FIELD,
                button: Button::Right,
                ..
            } => {
                if self.query.is_empty() {
                    false
                } else {
                    // the panel's query too, or the next letter brings the old text back
                    self.query.clear();
                    out.push(Action::SetQuery(String::new()));
                    true
                }
            }
            // Enter with nothing selected: the first result
            Event::Enter => {
                if self.query.is_empty() {
                    return false;
                }
                let Some(app) = search_apps(cx.apps, cx.recent, &self.query)
                    .into_iter()
                    .next()
                else {
                    return false;
                };
                out.push(Action::Launch(app.id.clone()));
                out.push(Action::ClosePanel);
                true
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } if *hit >= H_SEARCH_RESULT_BASE => {
                if cx.size == Size::S {
                    return false;
                }
                let idx = (*hit - H_SEARCH_RESULT_BASE) as usize;
                if idx >= SEARCH_MAX_RESULTS {
                    return false;
                }
                let matches = search_apps(cx.apps, cx.recent, &self.query);
                let Some(app) = matches.get(idx) else {
                    return false;
                };
                out.push(Action::Launch(app.id.clone()));
                out.push(Action::ClosePanel);
                true
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// apps/background (tray)

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub tooltip: String,
    pub icon_name: String,
    pub bus_name: String,
    pub obj_path: String,
    pub menu_path: String,
    pub item_is_menu: bool,
}

impl TrayItem {
    fn label(&self) -> &str {
        if !self.title.is_empty() {
            &self.title
        } else if !self.id.is_empty() {
            &self.id
        } else {
            &self.bus_name
        }
    }

    fn note(&self) -> (&str, bool) {
        if self.id == "screen-capture" {
            ("Capturing", true)
        } else if self.status.eq_ignore_ascii_case("NeedsAttention") {
            ("Needs attention", true)
        } else if !self.tooltip.is_empty() && self.tooltip != self.label() {
            (&self.tooltip, false)
        } else {
            ("", false)
        }
    }

    fn needs_attention(&self) -> bool {
        self.id == "screen-capture" || self.status.eq_ignore_ascii_case("NeedsAttention")
    }

    fn can_activate(&self) -> bool {
        self.id != "screen-capture"
            && is_safe_dbus_token(&self.bus_name)
            && is_safe_dbus_token(&self.obj_path)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayData {
    pub items: Vec<TrayItem>,
    pub watcher_ok: bool,
}

fn is_safe_dbus_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':' | b'/'))
}

fn dbus_activate_cmd(bus: &str, path: &str) -> String {
    format!(
        "dbus-send --session --type=method_call --dest='{bus}' '{path}' org.kde.StatusNotifierItem.Activate int32:0 int32:0"
    )
}

fn finish_kv_item(cur: &mut Option<TrayItem>, out: &mut Vec<TrayItem>) {
    let Some(item) = cur.take() else {
        return;
    };
    if item.id.is_empty() && item.title.is_empty() && item.bus_name.is_empty() {
        return;
    }
    if item.status.eq_ignore_ascii_case("Passive") {
        return;
    }
    out.push(item);
}

fn parse_delimited_item(line: &str, sep: char) -> Option<TrayItem> {
    let cols: Vec<&str> = line.split(sep).map(str::trim).collect();
    if cols.len() < 2 {
        return None;
    }
    let id = cols[0];
    let title = cols[1];
    if id.is_empty() && title.is_empty() {
        return None;
    }
    let status = cols.get(2).copied().unwrap_or("Active");
    if status.eq_ignore_ascii_case("Passive") {
        return None;
    }
    Some(TrayItem {
        id: id.to_string(),
        title: title.to_string(),
        status: status.to_string(),
        tooltip: cols.get(3).copied().unwrap_or("").to_string(),
        icon_name: cols.get(4).copied().unwrap_or("").to_string(),
        bus_name: cols.get(5).copied().unwrap_or("").to_string(),
        obj_path: cols.get(6).copied().unwrap_or("").to_string(),
        menu_path: cols.get(7).copied().unwrap_or("").to_string(),
        item_is_menu: cols
            .get(8)
            .is_some_and(|v| matches!(*v, "1" | "true" | "yes")),
    })
}

/// Parses tray fixture output (`---`-separated `key=value` records, tab-separated lines, or
/// pipe-separated lines). Ignores comments, blank lines, and malformed records.
pub fn parse_tray_data(raw: &str) -> TrayData {
    let mut items = Vec::new();
    let mut cur: Option<TrayItem> = None;

    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "---" {
            finish_kv_item(&mut cur, &mut items);
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let val = v.trim();
            if key == "id" && cur.is_some() {
                finish_kv_item(&mut cur, &mut items);
            }
            let item = cur.get_or_insert_with(TrayItem::default);
            match key {
                "id" => item.id = val.to_string(),
                "title" => item.title = val.to_string(),
                "status" => item.status = val.to_string(),
                "tooltip" => item.tooltip = val.to_string(),
                "icon" | "icon_name" => item.icon_name = val.to_string(),
                "bus" | "bus_name" => item.bus_name = val.to_string(),
                "path" | "obj_path" => item.obj_path = val.to_string(),
                "menu" | "menu_path" => item.menu_path = val.to_string(),
                "item_is_menu" | "is_menu" => {
                    item.item_is_menu = matches!(val, "1" | "true" | "yes");
                }
                _ => {}
            }
            continue;
        }
        finish_kv_item(&mut cur, &mut items);
        if line.contains('\t') {
            if let Some(it) = parse_delimited_item(line, '\t') {
                items.push(it);
            }
        } else if line.contains('|') {
            if let Some(it) = parse_delimited_item(line, '|') {
                items.push(it);
            }
        }
    }
    finish_kv_item(&mut cur, &mut items);
    TrayData {
        items,
        watcher_ok: true,
    }
}

fn capture_item(runtime_dir: &Path) -> Option<TrayItem> {
    let path = runtime_dir.join("infynitae").join("capturing");
    let raw = std::fs::read_to_string(path).ok()?;
    let mut lines = raw.lines().map(str::trim);
    let target = lines.next().unwrap_or("");
    if target.is_empty() {
        return None;
    }
    if let Some(pid_str) = lines.next() {
        let pid: u32 = pid_str.parse().ok()?;
        if pid == 0 || !Path::new(&format!("/proc/{pid}")).exists() {
            return None;
        }
    }
    Some(TrayItem {
        id: "screen-capture".to_string(),
        title: format!("Screen capture ({target})"),
        status: "Active".to_string(),
        tooltip: String::new(),
        icon_name: String::new(),
        bus_name: String::new(),
        obj_path: String::new(),
        menu_path: String::new(),
        item_is_menu: false,
    })
}

fn owned_str(v: &OwnedValue) -> String {
    let val: &Value<'_> = v;
    match val {
        Value::Str(s) => s.as_str().to_string(),
        Value::ObjectPath(p) => p.as_str().to_string(),
        _ => String::new(),
    }
}

fn owned_bool(v: &OwnedValue) -> bool {
    let val: &Value<'_> = v;
    matches!(val, Value::Bool(true))
}

fn owned_tooltip_title(v: &OwnedValue) -> String {
    let val: &Value<'_> = v;
    let Value::Structure(st) = val else {
        return String::new();
    };
    st.fields()
        .get(2)
        .and_then(|f| match f {
            Value::Str(s) => Some(s.as_str().trim().to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

/// The cards a right click on a tray app shows: its own menu, then its card (its
/// provider's first kind; the simple card for an app nobody knows). The shell orders them by
/// TRAY_CARD.
pub fn tray_words(item: &TrayItem, apps: &[App]) -> Vec<String> {
    let mut words = Vec::new();
    let id = crate::cards::tray_menu::word_id(&item.id);
    if !id.is_empty() && !item.menu_path.is_empty() {
        words.push(format!("apps/menu={id}"));
    }
    let want = |s: &str| !s.is_empty();
    let app = apps.iter().find(|a| {
        let keys = [item.id.as_str(), item.title.as_str()];
        keys.iter().filter(|k| want(k)).any(|k| {
            a.id.eq_ignore_ascii_case(k)
                || a.id
                    .rsplit('.')
                    .next()
                    .is_some_and(|l| l.eq_ignore_ascii_case(k))
                || a.wm_class
                    .as_deref()
                    .is_some_and(|w| w.eq_ignore_ascii_case(k))
                || a.name.eq_ignore_ascii_case(k)
        })
    });
    if let Some((p, a)) = app.and_then(|a| crate::provider::owner_of(a).map(|p| (p, a))) {
        if let Some(k) = p.cards().first() {
            words.push(format!("{}/{}", a.id, k.id));
        }
    }
    words
}

fn fetch_dbus_tray() -> (Vec<TrayItem>, bool) {
    let Ok(conn) = crate::dbus::session() else {
        return (Vec::new(), false);
    };
    fetch_dbus_tray_on(&conn)
}

pub(super) fn fetch_dbus_tray_on(conn: &zbus::blocking::Connection) -> (Vec<TrayItem>, bool) {
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(conn) else {
        return (Vec::new(), false);
    };
    let Ok(watcher_name) = zbus::names::BusName::try_from("org.kde.StatusNotifierWatcher") else {
        return (Vec::new(), false);
    };
    if !dbus.name_has_owner(watcher_name).unwrap_or(false) {
        return (Vec::new(), false);
    }

    let Ok(watcher) = zbus::blocking::Proxy::new(
        conn,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    ) else {
        return (Vec::new(), false);
    };
    let Ok(entries) = watcher.get_property::<Vec<String>>("RegisteredStatusNotifierItems") else {
        return (Vec::new(), false);
    };

    let mut items = Vec::new();
    for entry in entries {
        let (bus, path) = match entry.find('/') {
            Some(pos) => (&entry[..pos], &entry[pos..]),
            None => (entry.as_str(), "/StatusNotifierItem"),
        };
        if bus.is_empty() {
            continue;
        }
        let Ok(props) =
            zbus::blocking::Proxy::new(conn, bus, path, "org.freedesktop.DBus.Properties")
        else {
            continue;
        };
        let Ok(map) = props
            .call::<_, _, HashMap<String, OwnedValue>>("GetAll", &("org.kde.StatusNotifierItem",))
        else {
            continue;
        };
        let id = map.get("Id").map(owned_str).unwrap_or_default();
        let title = map.get("Title").map(owned_str).unwrap_or_default();
        let status = map.get("Status").map(owned_str).unwrap_or_default();
        if status.eq_ignore_ascii_case("Passive") {
            continue;
        }
        let icon_name = map.get("IconName").map(owned_str).unwrap_or_default();
        let tooltip = map
            .get("ToolTip")
            .map(owned_tooltip_title)
            .unwrap_or_default();
        let menu_path = map
            .get("Menu")
            .and_then(|v| {
                OwnedObjectPath::try_from(v.clone())
                    .ok()
                    .map(|p| p.to_string())
            })
            .unwrap_or_default();
        let item_is_menu = map.get("ItemIsMenu").is_some_and(owned_bool);
        if id.is_empty() && title.is_empty() {
            continue;
        }
        items.push(TrayItem {
            id,
            title,
            status,
            tooltip,
            icon_name,
            bus_name: bus.to_string(),
            obj_path: path.to_string(),
            menu_path,
            item_is_menu,
        });
    }
    (items, true)
}

pub struct BackgroundSource {
    pub runtime_dir: Option<PathBuf>,
}

impl Default for BackgroundSource {
    fn default() -> Self {
        Self {
            runtime_dir: Some(crate::paths::runtime_dir()),
        }
    }
}

impl BackgroundSource {
    fn collect_with_bus(&self, bus_items: Vec<TrayItem>, watcher_ok: bool) -> TrayData {
        let mut items = Vec::new();
        if let Some(dir) = &self.runtime_dir {
            if let Some(cap) = capture_item(dir) {
                items.push(cap);
            }
        }
        items.extend(bus_items);
        TrayData { items, watcher_ok }
    }
}

impl Source for BackgroundSource {
    fn fetch(&mut self) -> Data {
        if std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_some_and(|p| !p.is_empty()) {
            let raw = run_sh("background", "");
            let raw = if raw.is_empty() {
                run_sh("tray", "")
            } else {
                raw
            };
            return Box::new(parse_tray_data(&raw));
        }
        let (bus_items, watcher_ok) = fetch_dbus_tray();
        Box::new(self.collect_with_bus(bus_items, watcher_ok))
    }

    fn every(&self) -> Option<Duration> {
        Some(Duration::from_secs(2))
    }
}

#[derive(Default)]
struct BackgroundCard {
    data: Option<TrayData>,
    scroll: f64,
}

impl BackgroundCard {
    fn bounds(&self) -> (f64, f64) {
        let n = self.data.as_ref().map_or(0, |d| d.items.len());
        (n as f64 * ROW_H, BG_MAX_H)
    }

    fn is_item_visible(&self, idx: usize) -> bool {
        let (total_h, cap_h) = self.bounds();
        let view_h = total_h.min(cap_h);
        let scroll = clamped_scroll(self.scroll, total_h, cap_h);
        let top = idx as f64 * ROW_H - scroll;
        let bot = top + ROW_H;
        bot > 0.0 && top < view_h
    }
}

impl Card for BackgroundCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (
            key == "background" || key == "tray",
            data.downcast_ref::<TrayData>(),
        ) else {
            return false;
        };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
    }

    fn status(&self, _cx: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if !d.items.is_empty() => Status::Ready,
            Some(d) if d.watcher_ok => Status::Hidden,
            Some(_) => Status::Missing("No tray watcher service is running".into()),
        }
    }

    fn height(&self, _cx: &Cx, _width: f64) -> f64 {
        let (total_h, cap_h) = self.bounds();
        total_h.min(cap_h)
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else {
            return;
        };
        if d.items.is_empty() {
            return;
        }
        let (total_h, cap_h) = self.bounds();
        let scroll = clamped_scroll(self.scroll, total_h, cap_h);
        let _ = cr.save();
        cr.rectangle(body.0, body.1, body.2, body.3);
        cr.clip();

        let body_bot = body.1 + body.3;
        for (i, item) in d.items.iter().enumerate() {
            let top = body.1 + i as f64 * ROW_H - scroll;
            let bot = top + ROW_H;
            if bot <= body.1 || top >= body_bot {
                continue;
            }
            let hit_top = top.max(body.1);
            let hit_bot = bot.min(body_bot);
            let row_r = (body.0, top, body.2, ROW_H);
            let hit_r = (body.0, hit_top, body.2, (hit_bot - hit_top).max(0.0));
            let (note, note_acc) = item.note();
            draw_row_entry(
                cr,
                cx,
                hits,
                &EntryRow {
                    hit_id: i as u32,
                    row_r,
                    hit_r,
                    force_sel: false,
                    name: item.label(),
                    fallback: &item.id,
                    icon: &item.icon_name,
                    tile_accent: item.needs_attention(),
                    note,
                    note_accent: note_acc,
                },
            );
        }
        ui::scrollbar(cr, cx, body, total_h, scroll);
        let _ = cr.restore();
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            // a right click: the app's own menu and its card, in the panel's place
            Event::Press {
                hit,
                button: Button::Right,
                ..
            } => {
                let Some(item) = self.data.as_ref().and_then(|d| d.items.get(*hit as usize)) else {
                    return false;
                };
                let words = tray_words(item, cx.apps);
                if words.is_empty() {
                    return false;
                }
                out.push(Action::ShowCards(words));
                true
            }
            Event::Scroll { dy, .. } => {
                let (total_h, cap_h) = self.bounds();
                apply_scroll(&mut self.scroll, *dy, total_h, cap_h)
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } => {
                let idx = *hit as usize;
                let Some(d) = &self.data else {
                    return false;
                };
                let Some(item) = d.items.get(idx) else {
                    return false;
                };
                if !self.is_item_visible(idx) || !item.can_activate() {
                    return false;
                }
                out.push(Action::Run(dbus_activate_cmd(
                    &item.bus_name,
                    &item.obj_path,
                )));
                out.push(Action::ClosePanel);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use chrono::NaiveDate;

    fn make_app(id: &str, name: &str, categories: &[&str]) -> App {
        App {
            id: id.into(),
            name: name.into(),
            icon: id.into(),
            exec: id.into(),
            categories: categories.iter().map(|s| (*s).to_string()).collect(),
            wm_class: None,
            terminal: false,
        }
    }

    fn test_cx<'a>(th: &'a Theme, size: Size, apps: &'a [App]) -> Cx<'a> {
        Cx {
            theme: th,
            now: NaiveDate::from_ymd_opt(2026, 11, 13)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            size,
            app: None,
            apps,
            recent: &crate::recent::NONE,
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
            room: f64::INFINITY,
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "edge-apps-apps-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn provider_metadata_claims_and_arguments() {
        assert_eq!(Apps.id(), "apps");
        assert_eq!(Apps.name(), "Apps");
        assert_eq!(Apps.listed(), Listed::Always);
        assert!(Apps.claims().is_empty());

        let ids: Vec<&str> = Apps.cards().iter().map(|k| k.id).collect();
        assert_eq!(
            ids,
            vec![
                "list",
                "favorites",
                "search",
                "background",
                "menu",
                "results"
            ]
        );
        // the menu is a tray app's: only with its id
        assert!(Apps.card("menu", None).is_none());
        assert!(Apps.card("menu", Some("steam")).is_some());

        assert!(Apps.card("list", None).is_some());
        assert!(Apps.card("list", Some("arg")).is_none());
        assert!(Apps.card("favorites", None).is_some());
        assert!(Apps.card("favorites", Some("firefox,steam")).is_some());
        assert!(Apps.card("search", None).is_some());
        assert!(Apps.card("search", Some("arg")).is_none());
        assert!(Apps.card("search", Some("settings")).is_some());
        assert!(Apps.card("background", None).is_some());
        assert!(Apps.card("background", Some("arg")).is_none());
        assert!(Apps.card("results", None).is_some());
        assert!(Apps.card("results", Some("arg")).is_none());
        assert!(Apps.card("unknown", None).is_none());

        assert!(Apps.source("background").is_some());
        assert!(Apps.source("tray").is_some());
        assert!(Apps.source("search").is_some());
        // plocate's answers come under it; nothing fetches it
        assert!(Apps.source("located").is_none());
        assert!(Apps.source("other").is_none());
    }

    #[test]
    fn list_card_groups_categories_scrolls_and_launches() {
        let th = Theme::from_config(&Config::from_text(""));
        let empty: Vec<App> = Vec::new();
        let cx_empty = test_cx(&th, Size::M, &empty);
        let mut card = AppsListCard::default();
        assert_eq!(
            card.status(&cx_empty),
            Status::Missing("No applications installed".into())
        );

        let apps: Vec<App> = (0..18)
            .map(|i| {
                let cats: &[&str] = match i % 3 {
                    0 => &["Network", "WebBrowser"],
                    1 => &["System", "Monitor"],
                    _ => &[],
                };
                make_app(&format!("app{i}"), &format!("App {i:02}"), cats)
            })
            .collect();
        let cx_m = test_cx(&th, Size::M, &apps);
        let cx_l = test_cx(&th, Size::L, &apps);
        assert_eq!(card.status(&cx_m), Status::Ready);

        // 18 apps across 3 categories (6 each): M shows 6 apps (Internet), L shows 12 (Internet + System).
        assert_eq!(card.height(&cx_m, 340.0), SUB_H + 6.0 * ROW_H);
        assert_eq!(card.height(&cx_l, 340.0), 2.0 * SUB_H + 12.0 * ROW_H);
        assert!(card.height(&cx_l, 340.0) > card.height(&cx_m, 340.0));

        // Draw checks every registered hit stays within body height and reaches bottom.
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 600).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        for (size, cx, want_rows) in [(Size::M, &cx_m, 6), (Size::L, &cx_l, 12)] {
            let h = card.height(cx, 340.0);
            let body = (10.0, 20.0, 340.0, h);
            let mut hits = Hits::new(None, None);
            card.draw(&cr, cx, body, &mut hits);
            assert_eq!(hits.list.len(), want_rows, "{size:?} hit count");
            let max_y = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
            assert_eq!(max_y, body.1 + h, "{size:?} reaches bottom");
            for (_, r) in &hits.list {
                assert!(r.1 >= body.1 && r.1 + r.3 <= body.1 + h);
            }
        }

        // Scroll up at 0 is a no-op; scrolling down crosses categories and clamps at bottom (18 - 6 = 12).
        let mut out = Vec::new();
        assert!(!card.event(
            &Event::Scroll {
                hit: None,
                dy: -36.0,
                notches: -1.0,
            },
            &cx_m,
            &mut out
        ));
        for _ in 0..4 {
            assert!(card.event(
                &Event::Scroll {
                    hit: None,
                    dy: 36.0,
                    notches: 1.0,
                },
                &cx_m,
                &mut out
            ));
        }
        assert_eq!(card.scroll, 4);
        let h_split = card.height(&cx_m, 340.0);
        assert_eq!(h_split, 2.0 * SUB_H + 6.0 * ROW_H);
        let mut hits_split = Hits::new(None, None);
        card.draw(&cr, &cx_m, (10.0, 20.0, 340.0, h_split), &mut hits_split);
        assert_eq!(hits_split.list.len(), 6);

        for _ in 0..20 {
            card.event(
                &Event::Scroll {
                    hit: None,
                    dy: 36.0,
                    notches: 1.0,
                },
                &cx_m,
                &mut out,
            );
        }
        assert_eq!(card.scroll, 12);
        assert!(!card.event(
            &Event::Scroll {
                hit: None,
                dy: 36.0,
                notches: 1.0,
            },
            &cx_m,
            &mut out
        ));

        // Scroll back to top: first visible app is clickable; hidden app is not.
        for _ in 0..20 {
            card.event(
                &Event::Scroll {
                    hit: None,
                    dy: -36.0,
                    notches: -1.0,
                },
                &cx_m,
                &mut out,
            );
        }
        assert_eq!(card.scroll, 0);

        let mut hits_top = Hits::new(None, None);
        let h_m = card.height(&cx_m, 340.0);
        card.draw(&cr, &cx_m, (0.0, 0.0, 340.0, h_m), &mut hits_top);
        let first_hit = hits_top.list[0].0;
        let hidden_hit = (0..18u32)
            .find(|id| !hits_top.list.iter().any(|(hid, _)| hid == id))
            .unwrap();

        let mut out = Vec::new();
        assert!(!card.event(
            &Event::Press {
                hit: hidden_hit,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_m,
            &mut out
        ));
        assert!(out.is_empty());

        assert!(card.event(
            &Event::Press {
                hit: first_hit,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(
            out,
            vec![
                Action::Launch(apps[first_hit as usize].id.clone()),
                Action::ClosePanel,
            ]
        );
    }

    #[test]
    fn favorites_skips_missing_ids_hides_when_empty_and_respects_sizes() {
        let th = Theme::from_config(&Config::from_text(""));
        let apps = vec![
            make_app("firefox", "Firefox", &["Network", "WebBrowser"]),
            make_app("kitty", "kitty", &["System"]),
            make_app("steam", "Steam", &["Game"]),
            make_app("discord", "Discord", &["Network"]),
            make_app("mpv", "mpv", &["AudioVideo"]),
            make_app("btop", "btop++", &["System"]),
            make_app("pcmanfm-qt", "PCManFM-Qt", &["Utility"]),
        ];
        let cx_s = test_cx(&th, Size::S, &apps);
        let cx_m = test_cx(&th, Size::M, &apps);

        let none_card = FavoritesCard::new(Some("missing1, nonexistent"));
        assert_eq!(none_card.status(&cx_s), Status::Hidden);
        assert_eq!(none_card.height(&cx_s, 340.0), 0.0);

        let empty_card = FavoritesCard::new(None);
        assert_eq!(empty_card.status(&cx_s), Status::Hidden);

        let mut card = FavoritesCard::new(Some(
            "firefox, nonexistent, steam.desktop, firefox, kitty, discord, mpv, btop, pcmanfm-qt",
        ));
        assert_eq!(card.status(&cx_s), Status::Ready);
        let resolved: Vec<&str> = card.resolved(&apps).iter().map(|a| a.id.as_str()).collect();
        assert_eq!(
            resolved,
            vec![
                "firefox",
                "steam",
                "kitty",
                "discord",
                "mpv",
                "btop",
                "pcmanfm-qt"
            ]
        );

        let h_s = card.height(&cx_s, 340.0);
        let h_m = card.height(&cx_m, 340.0);
        assert_eq!(h_s, FAV_CELL_H);
        assert_eq!(h_m, 2.0 * FAV_CELL_H + FAV_ROW_GAP);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 300).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits_s = Hits::new(None, None);
        card.draw(&cr, &cx_s, (10.0, 20.0, 340.0, h_s), &mut hits_s);
        assert_eq!(hits_s.list.len(), 5);
        assert!(hits_s
            .list
            .iter()
            .all(|(_, r)| r.1 >= 20.0 && r.1 + r.3 <= 20.0 + h_s));

        let mut hits_m = Hits::new(None, None);
        card.draw(&cr, &cx_m, (10.0, 20.0, 340.0, h_m), &mut hits_m);
        assert_eq!(hits_m.list.len(), 7);
        let max_y = hits_m
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0, f64::max);
        assert_eq!(max_y, 20.0 + h_m);

        let mut out = Vec::new();
        assert!(!card.event(
            &Event::Press {
                hit: 5,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_s,
            &mut out
        ));
        assert!(card.event(
            &Event::Press {
                hit: 5,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(out, vec![Action::Launch("btop".into()), Action::ClosePanel]);
    }

    #[test]
    fn search_ranking_text_events_and_heights() {
        let th = Theme::from_config(&Config::from_text(""));
        let apps = vec![
            make_app("com.example.web", "Web App", &["Utility"]),
            make_app("ff-dev", "Firefox Developer Edition", &["Network"]),
            make_app("firefox", "Firefox", &["Network", "WebBrowser"]),
            make_app("my-firefox-helper", "Helper for Firefox", &["System"]),
            make_app("org.mozilla.fx", "Nightly", &["Network", "Firefox"]),
            make_app("librewolf", "LibreWolf", &["Network", "WebBrowser"]),
            make_app("kitty", "kitty", &["System", "TerminalEmulator"]),
            make_app("btop", "btop++", &["System", "Monitor"]),
            make_app("steam", "Steam", &["Game"]),
        ];

        let ids: Vec<&str> = search_apps(&apps, &crate::recent::NONE, "firefox")
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["firefox", "ff-dev", "my-firefox-helper", "org.mozilla.fx",]
        );
        let sub_ids: Vec<&str> = search_apps(&apps, &crate::recent::NONE, "frx")
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(sub_ids[0], "firefox");

        let cx_s = test_cx(&th, Size::S, &apps);
        let cx_m = test_cx(&th, Size::M, &apps);
        let mut card = SearchCard::default();
        assert_eq!(card.height(&cx_m, 340.0), SEARCH_BAR_H);

        let mut out = Vec::new();
        assert!(card.event(&Event::Text("e"), &cx_m, &mut out));
        assert!(!card.event(&Event::Text("e"), &cx_m, &mut out));
        assert_eq!(card.height(&cx_s, 340.0), SEARCH_BAR_H);
        let h_m = card.height(&cx_m, 340.0);
        assert_eq!(h_m, SEARCH_BAR_H + SEARCH_GAP + 6.0 * ROW_H);

        assert!(card.event(&Event::Text("zzzzz"), &cx_m, &mut out));
        assert_eq!(
            card.height(&cx_m, 340.0),
            SEARCH_BAR_H + SEARCH_GAP + ui::LIST_ROW_H
        );

        assert!(card.event(&Event::Text("firefox"), &cx_m, &mut out));
        let h_ff = card.height(&cx_m, 340.0);
        assert_eq!(h_ff, SEARCH_BAR_H + SEARCH_GAP + 4.0 * ROW_H);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 400).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        card.draw(&cr, &cx_m, (10.0, 20.0, 340.0, h_ff), &mut hits);
        assert_eq!(hits.list.len(), 6);
        assert_eq!(hits.list[0].0, H_SEARCH_RESULT_BASE);
        for (_, r) in &hits.list {
            assert!(r.1 >= 20.0 && r.1 + r.3 <= 20.0 + h_ff);
        }

        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_SEARCH_RESULT_BASE,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Launch("firefox".into()), Action::ClosePanel]
        );

        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_SEARCH_CLEAR,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(card.height(&cx_m, 340.0), SEARCH_BAR_H);
    }

    /// Starts, oldest first: "KDE Find" three times, Firefox twice, then Foot once.
    fn recent_starts() -> crate::recent::Recent {
        let mut r = crate::recent::Recent::default();
        for (when, id) in [
            (1, "kfind"),
            (2, "kfind"),
            (3, "kfind"),
            (4, "firefox"),
            (5, "firefox"),
            (6, "foot"),
        ] {
            r.note(id, when);
        }
        r
    }

    fn launcher_apps() -> Vec<App> {
        vec![
            make_app("files", "Files", &["System"]),
            make_app("firefox", "Firefox", &["Network"]),
            make_app("foot", "Foot", &["System"]),
            make_app("kfind", "KDE Find", &["Utility"]),
        ]
    }

    #[test]
    fn search_puts_apps_started_often_and_lately_first_within_a_match() {
        let apps = launcher_apps();
        let recent = recent_starts();
        let ids = |q: &str| -> Vec<String> {
            search_apps(&apps, &recent, q)
                .iter()
                .map(|a| a.id.clone())
                .collect()
        };
        // names starting with "f": Firefox (twice) before Foot (once, latest) before Files (never);
        // "KDE Find", started most, only has a word starting with it and stays after them
        assert_eq!(ids("f"), ["firefox", "foot", "files", "kfind"]);
        // a name starting with the query beats letters in order, however often that app started
        assert_eq!(ids("fo"), ["foot", "firefox"]);
        assert_eq!(ids("kde find"), ["kfind"]);

        // Enter in the launcher's list starts the first of them
        let th = Theme::from_config(&Config::from_text(""));
        let cx = Cx {
            recent: &recent,
            ..test_cx(&th, Size::M, &apps)
        };
        let mut card = AppsListCard::default();
        let mut out = Vec::new();
        card.event(&Event::Text("f"), &cx, &mut out);
        assert!(card.event(&Event::Enter, &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Launch("firefox".into()), Action::ClosePanel]
        );
    }

    #[test]
    fn nothing_typed_the_launcher_is_the_same_and_enter_starts_nothing() {
        let th = Theme::from_config(&Config::from_text(""));
        let apps = launcher_apps();
        let recent = recent_starts();
        let plain = test_cx(&th, Size::M, &apps);
        let cx = Cx {
            recent: &recent,
            ..test_cx(&th, Size::M, &apps)
        };
        let mut list = AppsListCard::default();
        assert_eq!(list.visible_window(&cx), list.visible_window(&plain));
        let mut search = SearchCard::default();
        assert_eq!(search.height(&cx, 340.0), SEARCH_BAR_H);
        let mut out = Vec::new();
        assert!(!list.event(&Event::Enter, &cx, &mut out));
        assert!(!search.event(&Event::Enter, &cx, &mut out));
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn background_parser_captures_folder_statuses_and_activation() {
        let usual = "id=discord\ntitle=Discord\nstatus=NeedsAttention\ntooltip=1 mention\n\
                     bus=:1.42\npath=/org/ayatana/NotificationItem/discord\n---\n\
                     steam\tSteam\tActive\tDownloading\tsteam\t:1.77\t/org/ayatana/NotificationItem/steam\n\
                     passive_app\tHidden\tPassive\t\t\t:1.99\t/Item\n";
        let parsed = parse_tray_data(usual);
        assert!(parsed.watcher_ok);
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].id, "discord");
        assert!(parsed.items[0].needs_attention());
        assert_eq!(parsed.items[1].title, "Steam");
        assert_eq!(parsed.items[1].bus_name, ":1.77");

        assert!(parse_tray_data("").items.is_empty());
        assert!(parse_tray_data("# comment\nbad line\njustone\n---\n")
            .items
            .is_empty());

        let dir = temp_dir("capturing");
        let inf_dir = dir.join("infynitae");
        std::fs::create_dir_all(&inf_dir).unwrap();
        let src = BackgroundSource {
            runtime_dir: Some(dir.clone()),
        };
        assert!(src.collect_with_bus(Vec::new(), true).items.is_empty());

        let cap_file = inf_dir.join("capturing");
        std::fs::write(&cap_file, format!("DP-1\n{}\n", std::process::id())).unwrap();
        let active = src.collect_with_bus(Vec::new(), false);
        assert_eq!(active.items.len(), 1);
        assert_eq!(active.items[0].id, "screen-capture");
        assert_eq!(active.items[0].title, "Screen capture (DP-1)");

        std::fs::write(&cap_file, "DP-1\n4294967294\n").unwrap();
        assert!(src.collect_with_bus(Vec::new(), false).items.is_empty());
        let _ = std::fs::remove_dir_all(&dir);

        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th, Size::S, &[]);
        let mut card = BackgroundCard::default();
        assert_eq!(card.status(&cx), Status::Loading);

        assert!(card.apply(
            "background",
            &TrayData {
                items: Vec::new(),
                watcher_ok: false,
            } as &dyn Any
        ));
        assert_eq!(
            card.status(&cx),
            Status::Missing("No tray watcher service is running".into())
        );

        assert!(card.apply(
            "tray",
            &TrayData {
                items: Vec::new(),
                watcher_ok: true,
            } as &dyn Any
        ));
        assert_eq!(card.status(&cx), Status::Hidden);

        assert!(card.apply("background", &parsed as &dyn Any));
        assert!(!card.apply("background", &parsed as &dyn Any));
        assert_eq!(card.status(&cx), Status::Ready);
        let h = card.height(&cx, 340.0);
        assert_eq!(h, 2.0 * ROW_H);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 200).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        card.draw(&cr, &cx, (10.0, 20.0, 340.0, h), &mut hits);
        assert_eq!(hits.list.len(), 2);
        assert_eq!(hits.list[1].1 .1 + hits.list[1].1 .3, 20.0 + h);

        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: 0,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(
            out,
            vec![
                Action::Run(
                    "dbus-send --session --type=method_call --dest=':1.42' '/org/ayatana/NotificationItem/discord' org.kde.StatusNotifierItem.Activate int32:0 int32:0".into()
                ),
                Action::ClosePanel,
            ]
        );
        // Right click and scrolling when <= 6 items return false.
        let mut out = Vec::new();
        assert!(!card.event(
            &Event::Press {
                hit: 0,
                button: Button::Right,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(!card.event(
            &Event::Scroll {
                hit: None,
                dy: 36.0,
                notches: 1.0,
            },
            &cx,
            &mut out
        ));

        // 8 items clamp at BG_MAX_H and scroll within 0..=(2 * ROW_H).
        let many = TrayData {
            watcher_ok: true,
            items: (0..8)
                .map(|i| TrayItem {
                    id: if i == 0 {
                        "screen-capture".into()
                    } else {
                        format!("item{i}")
                    },
                    title: format!("Item {i}"),
                    status: "Active".into(),
                    bus_name: format!(":1.{i}"),
                    obj_path: "/Item".into(),
                    ..TrayItem::default()
                })
                .collect(),
        };
        assert!(card.apply("background", &many as &dyn Any));
        assert_eq!(card.height(&cx, 340.0), BG_MAX_H);
        assert!(!card.event(
            &Event::Press {
                hit: 0,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(card.event(
            &Event::Scroll {
                hit: None,
                dy: 2.5,
                notches: 0.0,
            },
            &cx,
            &mut out
        ));
        assert_eq!(card.scroll, 2.5);
        assert!(card.event(
            &Event::Scroll {
                hit: None,
                dy: 36.0,
                notches: 1.0,
            },
            &cx,
            &mut out
        ));
        assert!(card.event(
            &Event::Scroll {
                hit: None,
                dy: 36.0,
                notches: 1.0,
            },
            &cx,
            &mut out
        ));
        assert!(card.event(
            &Event::Scroll {
                hit: None,
                dy: 36.0,
                notches: 1.0,
            },
            &cx,
            &mut out
        ));
        assert_eq!(card.scroll, 2.0 * ROW_H);
        assert!(!card.event(
            &Event::Scroll {
                hit: None,
                dy: 36.0,
                notches: 1.0,
            },
            &cx,
            &mut out
        ));
    }

    #[test]
    #[ignore = "wall-clock limits: run by hand with --ignored --nocapture"]
    fn first_draw_of_apps_card_with_200_entries_does_not_stall_on_icon_io() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let apps: Vec<App> = (0..200)
            .map(|i| App {
                id: format!("bench-app-{i:03}"),
                name: format!("Bench App {i:03}"),
                icon: format!("nonexistent-bench-icon-{i:03}"),
                exec: "true".into(),
                categories: vec!["Utility".into()],
                ..Default::default()
            })
            .collect();
        let cx = test_cx(&th, Size::L, &apps);
        let card = AppsListCard::default();
        let h = card.height(&cx, 340.0);
        let surf =
            cairo::ImageSurface::create(cairo::Format::ARgb32, 360, h.ceil() as i32 + 20).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);

        let t0 = std::time::Instant::now();
        card.draw(&cr, &cx, (10.0, 10.0, 340.0, h), &mut hits);
        let apps_card_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // Also time queuing all 200 distinct uncached icons directly on the UI thread.
        let cache =
            crate::icons::IconCache::new(|| crate::icons::IconSearch::with_dirs(Vec::new(), None));
        let t1 = std::time::Instant::now();
        for (i, app) in apps.iter().enumerate() {
            crate::icons::draw_app_icon_with(
                &cache,
                &cr,
                &cx,
                &app.icon,
                &app.name,
                10.0,
                10.0 + (i % 10) as f64 * 32.0,
                28.0,
            );
        }
        let all_200_ms = t1.elapsed().as_secs_f64() * 1000.0;
        eprintln!("apps_200_first_draw: card={apps_card_ms:.3} ms, 200_icons={all_200_ms:.3} ms");
        assert!(
            apps_card_ms < 100.0,
            "first AppsListCard::draw stalled: {apps_card_ms:.3} ms"
        );
        assert!(
            all_200_ms < 250.0,
            "first 200-icon draw stalled: {all_200_ms:.3} ms"
        );
        cache.wait_idle();
    }

    /// A session bus of the test's own, gone with the value; None (with a message) on a machine
    /// without dbus-daemon.
    struct Bus {
        daemon: std::process::Child,
        address: String,
        dir: PathBuf,
    }

    impl Bus {
        fn start(tag: &str) -> Option<Bus> {
            use std::io::BufRead;
            let dir = temp_dir(tag);
            let spawned = std::process::Command::new("dbus-daemon")
                .arg("--session")
                .arg("--nofork")
                .arg("--print-address=1")
                .arg(format!("--address=unix:dir={}", dir.display()))
                .stdout(std::process::Stdio::piped())
                .spawn();
            let mut daemon = match spawned {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("skipped: no dbus-daemon to run a private bus: {e}");
                    let _ = std::fs::remove_dir_all(&dir);
                    return None;
                }
            };
            let mut address = String::new();
            std::io::BufReader::new(daemon.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            let address = address.trim().to_string();
            assert!(!address.is_empty(), "dbus-daemon printed no address");
            Some(Bus {
                daemon,
                address,
                dir,
            })
        }

        fn connect(&self) -> zbus::blocking::connection::Builder<'static> {
            let addr = zbus::Address::try_from(self.address.as_str()).unwrap();
            crate::dbus::builder_for(&addr).unwrap()
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Polls `done` for up to five seconds (the watcher hears of a name going away from the bus).
    fn eventually(mut done: impl FnMut() -> bool) -> bool {
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < until {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        done()
    }

    struct FakeItem;

    #[zbus::interface(name = "org.kde.StatusNotifierItem")]
    impl FakeItem {
        #[zbus(property)]
        fn id(&self) -> String {
            "discord".into()
        }
        #[zbus(property)]
        fn title(&self) -> String {
            "Discord".into()
        }
        #[zbus(property)]
        fn status(&self) -> String {
            "Active".into()
        }
    }

    /// A watcher of a bar or desktop, as far as the edge sees it: it takes hosts.
    struct OtherWatcher {
        host: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
    impl OtherWatcher {
        fn register_status_notifier_host(&self, _service: String) {
            self.host.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn host_registered(conn: &zbus::blocking::Connection) -> bool {
        zbus::blocking::Proxy::new(
            conn,
            crate::tray_watcher::NAME,
            crate::tray_watcher::PATH,
            crate::tray_watcher::NAME,
        )
        .and_then(|p| p.get_property::<bool>("IsStatusNotifierHostRegistered"))
        .unwrap_or(false)
    }

    #[test]
    fn a_tray_app_registers_with_the_edge_s_watcher_and_leaves_with_its_name() {
        use crate::tray_watcher::{NAME, PATH};
        let Some(bus) = Bus::start("tray") else {
            return;
        };
        let _watcher = crate::tray_watcher::start_on(bus.connect()).unwrap();
        let reader = bus.connect().build().unwrap();
        assert_eq!(fetch_dbus_tray_on(&reader), (Vec::new(), true));
        assert!(
            eventually(|| host_registered(&reader)),
            "the edge is a host of its own watcher"
        );

        let item_path = "/org/ayatana/NotificationItem/discord";
        let item = bus
            .connect()
            .serve_at(item_path, FakeItem)
            .unwrap()
            .build()
            .unwrap();
        item.call_method(
            Some(NAME),
            PATH,
            Some(NAME),
            "RegisterStatusNotifierItem",
            &(item_path,),
        )
        .unwrap();
        let item_bus = item.unique_name().unwrap().to_string();

        let (items, ok) = fetch_dbus_tray_on(&reader);
        assert!(ok);
        assert_eq!(items.len(), 1);
        assert_eq!(
            (items[0].id.as_str(), items[0].title.as_str()),
            ("discord", "Discord")
        );
        assert_eq!(
            (items[0].bus_name.as_str(), items[0].obj_path.as_str()),
            (item_bus.as_str(), item_path)
        );

        // the Background card shows it
        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th, Size::S, &[]);
        let mut card = BackgroundCard::default();
        let src = BackgroundSource { runtime_dir: None };
        assert!(card.apply("background", &src.collect_with_bus(items, ok) as &dyn Any));
        assert_eq!(card.status(&cx), Status::Ready);
        let h = card.height(&cx, 340.0);
        assert_eq!(h, ROW_H);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 200).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        card.draw(&cr, &cx, (10.0, 20.0, 340.0, h), &mut hits);
        assert_eq!(hits.list.len(), 1);

        drop(item);
        assert!(
            eventually(|| fetch_dbus_tray_on(&reader) == (Vec::new(), true)),
            "the item is still listed after its connection closed"
        );
    }

    #[test]
    fn the_edge_leaves_another_watcher_the_name_and_takes_it_when_that_one_quits() {
        use crate::tray_watcher::{NAME, PATH};
        let Some(bus) = Bus::start("tray-other") else {
            return;
        };
        let host = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let other = bus
            .connect()
            .serve_at(PATH, OtherWatcher { host: host.clone() })
            .unwrap()
            .name(NAME)
            .unwrap()
            .build()
            .unwrap();
        let other_bus = other.unique_name().unwrap().to_string();
        let edge = crate::tray_watcher::start_on(bus.connect()).unwrap();
        let edge_bus = edge.unique_name().unwrap().to_string();
        let reader = bus.connect().build().unwrap();
        let dbus = zbus::blocking::fdo::DBusProxy::new(&reader).unwrap();
        let owner = || {
            dbus.get_name_owner(zbus::names::BusName::try_from(NAME).unwrap())
                .map(|o| o.to_string())
                .ok()
        };
        assert_eq!(owner(), Some(other_bus));
        assert!(
            eventually(|| host.load(std::sync::atomic::Ordering::SeqCst)),
            "the edge registers as a host with the other watcher"
        );

        drop(other);
        assert!(eventually(|| owner().as_deref() == Some(edge_bus.as_str())));
        assert!(
            eventually(|| host_registered(&reader)),
            "the edge registers as a host with itself once it serves"
        );
    }
}
