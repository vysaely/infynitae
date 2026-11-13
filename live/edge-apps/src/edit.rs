//! Flow editor for panels and the "Add a card" list: pure list operations, layout and hit testing,
//! overlay drawing, and saving a panel's line to config with backup.

use crate::app::App;
use crate::card::{Cx, Rect};
use crate::config::{Config, Panel};
use crate::draw::{self, Text};
use crate::icons;
use crate::panel::{self, CardRef};
use crate::provider::{self, CardKind, Listed, Provider, Size};
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::path::Path;

pub const WIDTH_MIN: i32 = 300;
pub const WIDTH_MAX: i32 = 560;
pub const WIDTH_STEP: i32 = 20;
pub const DEFAULT_WIDTH: i32 = 380;
pub const UNDO_MAX: usize = 30;
pub const MAX_CARDS: usize = 24;
pub const HALF_MIN_PANEL: f64 = 340.0;
pub const STRIP_H: f64 = 28.0;
pub const BTN: f64 = 24.0;
pub const BAR_ROW: f64 = 36.0;
pub const BAR_H: f64 = 76.0;
pub const ADD_W: f64 = 300.0;
pub const ADD_TITLE_H: f64 = 40.0;
pub const ADD_GROUP_H: f64 = 26.0;
pub const ADD_ROW_H: f64 = 44.0;
pub const ADD_KIND_H: f64 = 52.0;
pub const PLACEHOLDER_H: f64 = 52.0;

const GAP: f64 = 12.0;
const BAR_MIN_W: f64 = 380.0;
const WIDTH_LABEL_W: f64 = 46.0;
const WIDTH_VALUE_W: f64 = 58.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub cards: Vec<CardRef>,
    pub width: i32,
    pub hover: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardDrag {
    pub card: usize,
    pub start_y: f64,
    pub moved: bool,
}

#[derive(Clone, Debug, Default)]
pub struct EditState {
    pub active: bool,
    pub adding: bool,
    pub panel_idx: usize,
    pub before_line: String,
    pub before_cards: Vec<CardRef>,
    pub before_width: i32,
    pub before_hover: bool,
    pub undo: Vec<Snapshot>,
    pub hover_card: Option<usize>,
    pub drag: Option<CardDrag>,
    pub drop_at: Option<usize>,
    pub confirm_reset: bool,
    pub add_scroll: f64,
    pub expanded_add: Option<String>,
    pub bar_rect: Rect,
    pub add_rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardButton {
    Up,
    Down,
    Half,
    Size,
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarButton {
    Undo,
    Reset,
    Add,
    Narrower,
    Wider,
    Hover,
    Cancel,
    Done,
}

const ROW1: [(BarButton, f64); 4] = [
    (BarButton::Done, 56.0),
    (BarButton::Add, 46.0),
    (BarButton::Reset, 52.0),
    (BarButton::Undo, 50.0),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarHit {
    Button(BarButton),
    ChooseAdd(String),
    AddKind(String, String),
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddGroup {
    Infynitae,
    RichCards,
    SimpleCard,
}

impl AddGroup {
    pub fn title(self) -> &'static str {
        match self {
            AddGroup::Infynitae => "Infynitae",
            AddGroup::RichCards => "Rich cards",
            AddGroup::SimpleCard => "Simple card",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AddEntry {
    pub group: AddGroup,
    pub owner: String,
    pub name: String,
    pub icon: Option<String>,
    pub kinds: Vec<&'static CardKind>,
}

#[derive(Clone, Debug)]
pub enum AddItem {
    Group {
        title: &'static str,
        rect: Rect,
    },
    Entry {
        owner: String,
        name: String,
        icon: Option<String>,
        expanded: bool,
        first_in_group: bool,
        rect: Rect,
    },
    Kind {
        owner: String,
        kind: &'static CardKind,
        rect: Rect,
        button: Rect,
    },
}

pub fn inside(r: Rect, x: f64, y: f64) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

pub fn display_width(width: i32) -> i32 {
    if width > 0 {
        width
    } else {
        DEFAULT_WIDTH
    }
}

pub fn adjust_width(width: i32, delta: i32) -> Option<i32> {
    let cur = display_width(width);
    let next = (cur + delta).clamp(WIDTH_MIN, WIDTH_MAX);
    if next == cur {
        None
    } else {
        Some(next)
    }
}

pub fn resolve_kind(
    card: &CardRef,
    apps: &[App],
) -> Option<(&'static dyn Provider, &'static CardKind)> {
    let (prov, _) = provider::resolve(&card.owner, apps)?;
    let kind = prov.kind(&card.kind)?;
    Some((prov, kind))
}

pub fn card_title(card: &CardRef, apps: &[App]) -> String {
    let Some((prov, app)) = provider::resolve(&card.owner, apps) else {
        return card.owner.clone();
    };
    let kind_title = prov.kind(&card.kind).map_or("", |k| k.title);
    match app {
        Some(a) if prov.id() == "simple" || kind_title.eq_ignore_ascii_case(&a.name) => {
            a.name.clone()
        }
        Some(a) => format!("{} \u{00b7} {}", a.name, kind_title),
        None => {
            if kind_title.is_empty() {
                prov.name().to_string()
            } else {
                kind_title.to_string()
            }
        }
    }
}

fn strip_places(cards: &mut [CardRef]) {
    for c in cards {
        c.place = None;
    }
}

/// Adjacent half-width pairs formed from left to right (`(i, i + 1)`).
pub fn half_pairs(cards: &[CardRef], apps: &[App]) -> Vec<Option<usize>> {
    let mut pairs = vec![None; cards.len()];
    let can_half = |c: &CardRef| c.half && resolve_kind(c, apps).is_some_and(|(_, kind)| kind.half);
    let mut i = 0;
    while i < cards.len() {
        if can_half(&cards[i]) && i + 1 < cards.len() && can_half(&cards[i + 1]) {
            pairs[i] = Some(i + 1);
            pairs[i + 1] = Some(i);
            i += 2;
        } else {
            i += 1;
        }
    }
    pairs
}

/// Moves card `from` to gap `to` (`0..=cards.len()` in pre-removal coordinates).
pub fn move_card(cards: &mut Vec<CardRef>, from: usize, to: usize) -> bool {
    if from >= cards.len() || to > cards.len() || to == from || to == from + 1 {
        return false;
    }
    let item = cards.remove(from);
    let dest = if to > from { to - 1 } else { to };
    cards.insert(dest, item);
    strip_places(cards);
    true
}

pub fn move_up(cards: &mut Vec<CardRef>, idx: usize) -> bool {
    idx > 0 && move_card(cards, idx, idx - 1)
}

pub fn move_down(cards: &mut Vec<CardRef>, idx: usize) -> bool {
    idx + 1 < cards.len() && move_card(cards, idx, idx + 2)
}

pub fn remove_card(cards: &mut Vec<CardRef>, idx: usize, apps: &[App]) -> bool {
    if idx >= cards.len() {
        return false;
    }
    let partner = half_pairs(cards, apps)[idx];
    if let Some(p) = partner {
        cards[p].half = false;
    }
    cards.remove(idx);
    strip_places(cards);
    true
}

/// Toggles half-width on `idx` when its kind allows `half: true`. When turning on, also pairs with
/// an eligible adjacent neighbour if one is unpaired; when turning off, also clears its paired partner.
pub fn toggle_half(cards: &mut [CardRef], idx: usize, apps: &[App]) -> bool {
    if idx >= cards.len() {
        return false;
    }
    let supports_half =
        |i: usize, cards: &[CardRef]| resolve_kind(&cards[i], apps).is_some_and(|(_, k)| k.half);
    if !supports_half(idx, cards) {
        return false;
    }
    let pairs = half_pairs(cards, apps);
    if cards[idx].half {
        cards[idx].half = false;
        if let Some(p) = pairs[idx] {
            cards[p].half = false;
        }
    } else {
        let partner = [idx.checked_add(1), idx.checked_sub(1)]
            .into_iter()
            .flatten()
            .find(|&j| j < cards.len() && !cards[j].half && supports_half(j, cards));
        cards[idx].half = true;
        if let Some(p) = partner {
            cards[p].half = true;
        }
    }
    strip_places(cards);
    true
}

pub fn next_size(cards: &mut [CardRef], idx: usize, apps: &[App]) -> bool {
    if idx >= cards.len() {
        return false;
    }
    let Some((_, kind)) = resolve_kind(&cards[idx], apps) else {
        return false;
    };
    if kind.sizes.len() <= 1 {
        return false;
    }
    let cur = cards[idx].size.unwrap_or(kind.default);
    let pos = kind.sizes.iter().position(|&s| s == cur).unwrap_or(0);
    let next = kind.sizes[(pos + 1) % kind.sizes.len()];
    cards[idx].size = if next == kind.default {
        None
    } else {
        Some(next)
    };
    strip_places(cards);
    true
}

pub fn add_card(cards: &mut Vec<CardRef>, mut card: CardRef) -> bool {
    if cards.len() >= MAX_CARDS {
        return false;
    }
    if cards
        .iter()
        .any(|c| c.owner == card.owner && c.kind == card.kind)
    {
        return false;
    }
    card.place = None;
    cards.push(card);
    strip_places(cards);
    true
}

pub fn size_label(s: Size, compact: bool) -> &'static str {
    if compact {
        match s {
            Size::S => "S",
            Size::M => "M",
            Size::L => "L",
        }
    } else {
        match s {
            Size::S => "Small",
            Size::M => "Medium",
            Size::L => "Large",
        }
    }
}

pub fn has_default(name: &str) -> bool {
    matches!(name, "control" | "apps" | "windows" | "clipboard")
}

/// Built-in default `(cards, width, hover)` of a default panel, if any.
pub fn default_panel_state(name: &str) -> Option<(Vec<CardRef>, i32, bool)> {
    if !has_default(name) {
        return None;
    }
    let cfg = Config::from_text("");
    let p = cfg.panels.into_iter().find(|p| p.name == name)?;
    let mut cards = panel::cards(&p);
    strip_places(&mut cards);
    Some((cards, p.width, p.hover))
}

impl EditState {
    pub fn push_undo(&mut self, snap: Snapshot) {
        if self.undo.len() >= UNDO_MAX {
            self.undo.remove(0);
        }
        self.undo.push(snap);
    }

    pub fn pop_undo(&mut self) -> Option<Snapshot> {
        self.undo.pop()
    }
}

/// Rows of laid-out card rectangles (1 full-width card, or 2 half-width cards sharing `y`).
pub fn card_rows(rects: &[(usize, Rect)]) -> Vec<Vec<(usize, Rect)>> {
    let mut rows: Vec<Vec<(usize, Rect)>> = Vec::new();
    for &(ci, r) in rects {
        let same_row = rows
            .last()
            .and_then(|row| row.last())
            .is_some_and(|(_, pr)| (pr.1 - r.1).abs() < 2.0);
        if same_row {
            if let Some(row) = rows.last_mut() {
                row.push((ci, r));
            }
        } else {
            rows.push(vec![(ci, r)]);
        }
    }
    rows
}

/// Drop gap index `0..=cards_len` in panel-local coordinates for dragging `drag_idx`.
pub fn drop_index(
    rects: &[(usize, Rect)],
    cards_len: usize,
    drag_idx: usize,
    local_x: f64,
    local_y: f64,
) -> Option<usize> {
    if cards_len < 2 || rects.is_empty() {
        return None;
    }
    let rows = card_rows(rects);
    for row in &rows {
        let row_top = row.iter().map(|(_, r)| r.1).fold(f64::INFINITY, f64::min) - STRIP_H;
        let row_bot = row
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(f64::NEG_INFINITY, f64::max);
        if row.len() == 2 {
            let (i0, r0) = row[0];
            let (i1, r1) = row[1];
            if local_y < row_top + (STRIP_H * 0.65) {
                return (i0 != drag_idx && i0 != drag_idx + 1).then_some(i0);
            }
            if local_y <= row_bot - 6.0 {
                let slot = if local_x < r0.0 + r0.2 * 0.4 {
                    i0
                } else if local_x < r1.0 + r1.2 * 0.6 {
                    i1
                } else {
                    i1 + 1
                };
                return (slot != drag_idx && slot != drag_idx + 1).then_some(slot);
            }
            if local_y <= row_bot + GAP * 0.5 {
                let slot = i1 + 1;
                return (slot != drag_idx && slot != drag_idx + 1).then_some(slot);
            }
        } else {
            let (i0, r0) = row[0];
            let mid = r0.1 + r0.3 * 0.5;
            if local_y < mid {
                return (i0 != drag_idx && i0 != drag_idx + 1).then_some(i0);
            }
            if local_y <= r0.1 + r0.3 + GAP * 0.5 {
                let slot = i0 + 1;
                return (slot != drag_idx && slot != drag_idx + 1).then_some(slot);
            }
        }
    }
    let last = rows
        .last()
        .and_then(|r| r.last())
        .map(|(ci, _)| ci + 1)
        .unwrap_or(cards_len);
    (last != drag_idx && last != drag_idx + 1).then_some(last)
}

#[derive(Clone, Copy, Debug)]
enum DropLine {
    Horizontal { x: f64, y: f64, w: f64 },
    Vertical { x: f64, y: f64, h: f64 },
}

fn drop_line_geom(rects: &[(usize, Rect)], pw: f64, at: usize) -> Option<DropLine> {
    let rows = card_rows(rects);
    for row in &rows {
        if row.len() == 2 {
            let (i0, r0) = row[0];
            let (i1, r1) = row[1];
            if at == i1 {
                let mid_x = (r0.0 + r0.2 + r1.0) * 0.5;
                let top = r0.1.min(r1.1) - STRIP_H + 4.0;
                let bot = (r0.1 + r0.3).max(r1.1 + r1.3);
                return Some(DropLine::Vertical {
                    x: mid_x,
                    y: top,
                    h: (bot - top).max(16.0),
                });
            }
            if at == i0 {
                return Some(DropLine::Horizontal {
                    x: 4.0,
                    y: r0.1.min(r1.1) - STRIP_H - GAP * 0.3,
                    w: (pw - 8.0).max(12.0),
                });
            }
        } else if row[0].0 == at {
            return Some(DropLine::Horizontal {
                x: 4.0,
                y: row[0].1 .1 - STRIP_H - GAP * 0.3,
                w: (pw - 8.0).max(12.0),
            });
        }
    }
    rows.last().map(|row| {
        let bot = row.iter().map(|(_, r)| r.1 + r.3).fold(0.0_f64, f64::max);
        DropLine::Horizontal {
            x: 4.0,
            y: bot + GAP * 0.5,
            w: (pw - 8.0).max(12.0),
        }
    })
}

/// Strip rectangle above `r` and its action buttons in panel-local coordinates.
pub fn card_buttons(
    cards: &[CardRef],
    apps: &[App],
    ci: usize,
    r: Rect,
    pw: f64,
) -> (Rect, Vec<(CardButton, Rect)>) {
    let strip = (r.0, r.1 - STRIP_H + 2.0, r.2, STRIP_H - 4.0);
    let Some(card) = cards.get(ci) else {
        return (strip, Vec::new());
    };
    let kind = resolve_kind(card, apps).map(|(_, k)| k);
    let compact = strip.2 < 210.0;
    let btn_w = if compact { 22.0 } else { BTN };
    let gap = if compact { 3.0 } else { 4.0 };
    let mut want = vec![(CardButton::Remove, btn_w)];
    if ci > 0 {
        want.push((CardButton::Up, btn_w));
    }
    if ci + 1 < cards.len() {
        want.push((CardButton::Down, btn_w));
    }
    if pw >= HALF_MIN_PANEL && kind.is_some_and(|k| k.half) {
        want.push((CardButton::Half, if compact { 36.0 } else { 40.0 }));
    }
    if kind.is_some_and(|k| k.sizes.len() > 1) {
        want.push((CardButton::Size, if compact { 24.0 } else { 54.0 }));
    }
    let mut out = Vec::new();
    let mut x = strip.0 + strip.2 - 1.0;
    let left_limit = strip.0 + if compact { 18.0 } else { 22.0 };
    for (k, w) in want {
        if x - w < left_limit {
            break;
        }
        x -= w;
        out.push((k, (x, strip.1 + 1.0, w, strip.3 - 2.0)));
        x -= gap;
    }
    (strip, out)
}

/// Card index whose strip or card body holds `(local_x, local_y)`.
pub fn card_at(rects: &[(usize, Rect)], local_x: f64, local_y: f64) -> Option<usize> {
    for &(ci, r) in rects {
        let strip = (r.0, r.1 - STRIP_H + 2.0, r.2, STRIP_H - 4.0);
        if inside(strip, local_x, local_y) || inside(r, local_x, local_y) {
            return Some(ci);
        }
    }
    None
}

/// Bar rectangle on screen: above the panel, below if room for Add, else beside on the inner side.
pub fn bar_rect(origin: (f64, f64), pw: f64, ph: f64, screen: (f64, f64)) -> Rect {
    let (ox, oy) = origin;
    let (sw, sh) = screen;
    let bw = pw.max(BAR_MIN_W).min((sw - 16.0).max(BAR_MIN_W));
    let bh = BAR_H;
    let clamp_x = |x: f64| x.clamp(8.0, (sw - bw - 8.0).max(8.0));
    let clamp_y = |y: f64| y.clamp(8.0, (sh - bh - 8.0).max(8.0));
    let room = bh + 6.0 + 6.0 * ADD_ROW_H;
    let (x, y) = if oy - bh - 8.0 >= 8.0 {
        (clamp_x(ox + pw - bw), oy - bh - 8.0)
    } else if oy + ph + 8.0 + room <= sh - 8.0 {
        (clamp_x(ox + pw - bw), oy + ph + 8.0)
    } else if ox - bw - 8.0 >= 8.0 {
        (ox - bw - 8.0, clamp_y(oy))
    } else if ox + pw + 8.0 + bw <= sw - 8.0 {
        (ox + pw + 8.0, clamp_y(oy))
    } else {
        (clamp_x(ox), clamp_y(oy))
    };
    (x, y, bw, bh)
}

pub fn bar_buttons(bar: Rect) -> Vec<(BarButton, Rect)> {
    let mut out = Vec::new();
    let row = |i: f64| bar.1 + 4.0 + i * BAR_ROW + 4.0;
    let h = BAR_ROW - 8.0;
    let mut x = bar.0 + bar.2 - 8.0;
    for (k, w) in ROW1 {
        x -= w;
        out.push((k, (x, row(0.0), w, h)));
        x -= 4.0;
    }
    let left = bar.0 + 12.0 + WIDTH_LABEL_W;
    out.push((BarButton::Narrower, (left, row(1.0), BTN + 4.0, h)));
    out.push((
        BarButton::Wider,
        (left + BTN + 4.0 + WIDTH_VALUE_W, row(1.0), BTN + 4.0, h),
    ));
    let mut x = bar.0 + bar.2 - 8.0;
    for (k, w) in [(BarButton::Cancel, 62.0), (BarButton::Hover, 84.0)] {
        x -= w;
        out.push((k, (x, row(1.0), w, h)));
        x -= 4.0;
    }
    out
}

/// Available entries grouped as Infynitae, Rich cards, and Simple card, excluding cards already in
/// `cards`. Each group is sorted by name.
pub fn add_entries(cards: &[CardRef], apps: &[App]) -> Vec<AddEntry> {
    let has_card =
        |owner: &str, kind: &str| cards.iter().any(|c| c.owner == owner && c.kind == kind);

    let mut infynitae = Vec::new();
    for prov in provider::providers() {
        let include = match prov.listed() {
            Listed::Always => true,
            Listed::WithoutApp => !apps
                .iter()
                .any(|a| provider::owner_of(a).map(|p| p.id()) == Some(prov.id())),
            Listed::Never => false,
        };
        if !include {
            continue;
        }
        let kinds: Vec<&'static CardKind> = prov
            .cards()
            .iter()
            .filter(|k| !has_card(prov.id(), k.id))
            .collect();
        if !kinds.is_empty() {
            infynitae.push(AddEntry {
                group: AddGroup::Infynitae,
                owner: prov.id().to_string(),
                name: prov.name().to_string(),
                icon: None,
                kinds,
            });
        }
    }
    infynitae.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.owner.cmp(&b.owner))
    });

    let mut rich = Vec::new();
    let mut simple = Vec::new();
    let mut seen_apps = std::collections::HashSet::new();
    for a in apps {
        if !seen_apps.insert(a.id.as_str()) {
            continue;
        }
        let Some(prov) = provider::owner_of(a).or_else(|| provider::by_id("simple")) else {
            continue;
        };
        let kinds: Vec<&'static CardKind> = prov
            .cards()
            .iter()
            .filter(|k| !has_card(&a.id, k.id))
            .collect();
        if kinds.is_empty() {
            continue;
        }
        let group = if prov.id() != "simple" {
            AddGroup::RichCards
        } else {
            AddGroup::SimpleCard
        };
        let entry = AddEntry {
            group,
            owner: a.id.clone(),
            name: a.name.clone(),
            icon: Some(a.icon.clone()),
            kinds,
        };
        match group {
            AddGroup::RichCards => rich.push(entry),
            AddGroup::SimpleCard => simple.push(entry),
            AddGroup::Infynitae => {}
        }
    }
    let cmp_name = |a: &AddEntry, b: &AddEntry| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.owner.cmp(&b.owner))
    };
    rich.sort_by(cmp_name);
    simple.sort_by(cmp_name);

    let mut out = Vec::with_capacity(infynitae.len() + rich.len() + simple.len());
    out.extend(infynitae);
    out.extend(rich);
    out.extend(simple);
    out
}

pub fn add_content_height(entries: &[AddEntry], expanded: Option<&str>) -> f64 {
    let mut h = 0.0;
    let mut last_group: Option<AddGroup> = None;
    for e in entries {
        if last_group != Some(e.group) {
            h += ADD_GROUP_H;
            last_group = Some(e.group);
        }
        h += ADD_ROW_H;
        if expanded == Some(e.owner.as_str()) {
            h += e.kinds.len() as f64 * ADD_KIND_H;
        }
    }
    h
}

pub fn add_rect(
    origin: (f64, f64),
    pw: f64,
    screen: (f64, f64),
    content_h: f64,
    bar: Rect,
) -> Rect {
    let (ox, oy) = origin;
    let (sw, sh) = screen;
    let x = if ox - ADD_W - 8.0 >= 8.0 {
        ox - ADD_W - 8.0
    } else if ox + pw + 8.0 + ADD_W <= sw - 8.0 {
        ox + pw + 8.0
    } else {
        (sw - ADD_W - 8.0).max(8.0)
    };
    let beside_bar = x < bar.0 + bar.2 && bar.0 < x + ADD_W;
    let top = if beside_bar {
        oy.max(bar.1 + bar.3 + 8.0)
    } else {
        oy
    };
    let want = ADD_TITLE_H + content_h.max(ADD_ROW_H) + 8.0;
    let h = want.min((sh - 8.0 - top).max(ADD_TITLE_H + ADD_ROW_H));
    let y = top.clamp(8.0, (sh - h - 8.0).max(8.0));
    (x, y, ADD_W, h)
}

pub fn add_viewport(add: Rect) -> Rect {
    (
        add.0,
        add.1 + ADD_TITLE_H,
        add.2,
        (add.3 - ADD_TITLE_H - 6.0).max(1.0),
    )
}

pub fn build_add_items(
    add: Rect,
    entries: &[AddEntry],
    expanded: Option<&str>,
    scroll: f64,
) -> (Vec<AddItem>, f64, f64) {
    let total_h = add_content_height(entries, expanded);
    let view = add_viewport(add);
    let max_scroll = (total_h - view.3).max(0.0);
    let scroll = scroll.clamp(0.0, max_scroll);
    let mut y = view.1 - scroll;
    let mut items = Vec::new();
    let mut last_group: Option<AddGroup> = None;
    for e in entries {
        let first_in_group = last_group != Some(e.group);
        if first_in_group {
            items.push(AddItem::Group {
                title: e.group.title(),
                rect: (add.0 + 12.0, y, add.2 - 24.0, ADD_GROUP_H),
            });
            y += ADD_GROUP_H;
            last_group = Some(e.group);
        }
        let is_exp = expanded == Some(e.owner.as_str());
        items.push(AddItem::Entry {
            owner: e.owner.clone(),
            name: e.name.clone(),
            icon: e.icon.clone(),
            expanded: is_exp,
            first_in_group,
            rect: (add.0 + 8.0, y, add.2 - 16.0, ADD_ROW_H),
        });
        y += ADD_ROW_H;
        if is_exp {
            for &k in &e.kinds {
                let row = (add.0 + 18.0, y, add.2 - 28.0, ADD_KIND_H);
                let btn = (
                    row.0 + row.2 - 52.0,
                    row.1 + (row.3 - 26.0) / 2.0,
                    48.0,
                    26.0,
                );
                items.push(AddItem::Kind {
                    owner: e.owner.clone(),
                    kind: k,
                    rect: row,
                    button: btn,
                });
                y += ADD_KIND_H;
            }
        }
    }
    (items, scroll, max_scroll)
}

/// Adjusts `scroll` so the expanded entry and its first kind(s) stay inside the Add viewport.
pub fn reveal_expanded(add: Rect, entries: &[AddEntry], owner: &str, scroll: f64) -> f64 {
    let total_h = add_content_height(entries, Some(owner));
    let view_h = add_viewport(add).3;
    let max_scroll = (total_h - view_h).max(0.0);
    let mut y = 0.0;
    let mut last_group: Option<AddGroup> = None;
    for e in entries {
        if last_group != Some(e.group) {
            y += ADD_GROUP_H;
            last_group = Some(e.group);
        }
        let top = y;
        y += ADD_ROW_H;
        if e.owner == owner {
            let bot = (top + ADD_ROW_H + e.kinds.len() as f64 * ADD_KIND_H).min(top + view_h);
            let mut next = scroll.clamp(0.0, max_scroll);
            if bot > next + view_h {
                next = (bot - view_h).min(max_scroll);
            }
            if top < next {
                next = top;
            }
            return next.clamp(0.0, max_scroll);
        }
    }
    scroll.clamp(0.0, max_scroll)
}

pub fn bar_hit(state: &EditState, entries: &[AddEntry], x: f64, y: f64) -> Option<BarHit> {
    let (bar, add) = (state.bar_rect, state.add_rect);
    if state.adding && inside(add, x, y) {
        let view = add_viewport(add);
        if inside(view, x, y) {
            let (items, _, _) = build_add_items(
                add,
                entries,
                state.expanded_add.as_deref(),
                state.add_scroll,
            );
            for item in items {
                match item {
                    AddItem::Kind {
                        owner,
                        kind,
                        button,
                        ..
                    } if inside(button, x, y) => {
                        return Some(BarHit::AddKind(owner, kind.id.to_string()));
                    }
                    AddItem::Entry { owner, rect, .. } if inside(rect, x, y) => {
                        return Some(BarHit::ChooseAdd(owner));
                    }
                    _ => {}
                }
            }
        }
        return Some(BarHit::Nothing);
    }
    if !inside(bar, x, y) {
        return None;
    }
    for (k, r) in bar_buttons(bar) {
        if inside(r, x, y) {
            return Some(BarHit::Button(k));
        }
    }
    Some(BarHit::Nothing)
}

fn label(cr: &Context, r: Rect, s: &str, color: Rgba, bold: bool) {
    draw::text_center(
        cr,
        r.0 + r.2 / 2.0,
        r.1 + (r.3 - 13.0) / 2.0 - 1.0,
        &Text {
            size: 12.0,
            bold,
            color,
            text: s,
        },
    );
}

fn button_bg(cr: &Context, th: &Theme, r: Rect, on: bool, hot: bool, primary: bool) {
    draw::round_rect(cr, r.0, r.1, r.2, r.3, th.small_radius());
    if primary {
        draw::set(cr, th.accent);
    } else if on {
        draw::set(cr, th.accent.alpha(if hot { 0.32 } else { 0.24 }));
    } else {
        draw::set(cr, th.text.alpha(if hot { 0.14 } else { 0.08 }));
    }
    let _ = cr.fill();
}

fn arrow(cr: &Context, r: Rect, up: bool, color: Rgba) {
    let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    let d = if up { 1.0 } else { -1.0 };
    draw::set(cr, color);
    cr.set_line_width(1.8);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(cx, cy - 5.0 * d);
    cr.line_to(cx, cy + 5.0 * d);
    cr.move_to(cx - 4.0, cy - 1.0 * d);
    cr.line_to(cx, cy - 5.0 * d);
    cr.line_to(cx + 4.0, cy - 1.0 * d);
    let _ = cr.stroke();
}

fn cross(cr: &Context, r: Rect, color: Rgba) {
    let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    draw::set(cr, color);
    cr.set_line_width(1.8);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.move_to(cx - 4.0, cy - 4.0);
    cr.line_to(cx + 4.0, cy + 4.0);
    cr.move_to(cx + 4.0, cy - 4.0);
    cr.line_to(cx - 4.0, cy + 4.0);
    let _ = cr.stroke();
}

fn draw_provider_badge(cr: &Context, th: &Theme, name: &str, x: f64, y: f64, size: f64) {
    draw::round_rect(cr, x, y, size, size, th.small_radius().min(size * 0.28));
    draw::set(cr, th.accent.alpha(0.18));
    let _ = cr.fill();
    let ch: String = name.chars().take(1).collect();
    draw::text_center(
        cr,
        x + size / 2.0,
        y + (size - 13.0) / 2.0 - 1.0,
        &Text {
            size: 12.0,
            bold: true,
            color: th.accent_text,
            text: &ch,
        },
    );
}

fn panel_title(panel: &Panel) -> String {
    let mut cs = panel.name.chars();
    match cs.next() {
        Some(c) => c.to_uppercase().chain(cs).collect(),
        None => "panel".to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_overlay(
    cr: &Context,
    cx: &Cx<'_>,
    panel: &Panel,
    cards: &[CardRef],
    rects: &[(usize, Rect)],
    origin: (f64, f64),
    size: (f64, f64),
    panel_scroll: f64,
    pointer: Option<(f64, f64)>,
    state: &EditState,
) {
    let th = cx.theme;
    let apps = cx.apps;
    let (pw, ph) = size;
    let (ox, oy) = (origin.0, origin.1 - panel_scroll);
    let dragging = state.drag.is_some_and(|d| d.moved);

    // keep card outlines, strips and drop indicator clipped to the panel box
    let _ = cr.save();
    cr.rectangle(origin.0 - 2.0, origin.1 - 2.0, pw + 4.0, ph + 4.0);
    cr.clip();

    for &(ci, r) in rects {
        let held = state.drag.is_some_and(|d| d.card == ci && d.moved);
        draw::round_rect(
            cr,
            ox + r.0 + 0.5,
            oy + r.1 + 0.5,
            r.2 - 1.0,
            r.3 - 1.0,
            th.corners,
        );
        draw::set(cr, th.accent.alpha(if held { 0.95 } else { 0.40 }));
        cr.set_line_width(if held { 2.0 } else { 1.0 });
        let _ = cr.stroke();
    }

    for &(ci, r) in rects {
        let Some(card) = cards.get(ci) else {
            continue;
        };
        let title = card_title(card, apps);
        let shown = state.hover_card == Some(ci) && !dragging;
        let (strip, buttons) = card_buttons(cards, apps, ci, r, pw);
        if shown {
            draw::round_rect(
                cr,
                ox + strip.0,
                oy + strip.1,
                strip.2,
                strip.3,
                th.small_radius(),
            );
            draw::set(cr, th.card.alpha(0.95));
            let _ = cr.fill();
        }
        draw::set(cr, if shown { th.text } else { th.muted });
        for dy in [-4.0, 0.0, 4.0] {
            for dx in [0.0, 4.0] {
                cr.new_sub_path();
                cr.arc(
                    ox + strip.0 + 9.0 + dx,
                    oy + strip.1 + strip.3 / 2.0 + dy,
                    1.1,
                    0.0,
                    std::f64::consts::TAU,
                );
            }
        }
        let _ = cr.fill();

        let name_right = if shown {
            buttons
                .last()
                .map(|(_, br)| br.0 - 6.0)
                .unwrap_or(strip.0 + strip.2 - 6.0)
        } else {
            strip.0 + strip.2 - 6.0
        };
        let name_room = name_right - strip.0 - 22.0;
        if name_room >= 30.0 {
            draw::text_max(
                cr,
                ox + strip.0 + 22.0,
                oy + strip.1 + (strip.3 - 13.0) / 2.0 - 1.0,
                name_room,
                &Text {
                    size: 12.0,
                    bold: shown,
                    color: if shown { th.text } else { th.muted },
                    text: &title,
                },
            );
        }
        if !shown {
            continue;
        }
        let kind = resolve_kind(card, apps).map(|(_, k)| k);
        let compact = strip.2 < 210.0;
        for (k, br) in buttons {
            let sbr = (ox + br.0, oy + br.1, br.2, br.3);
            let hot = pointer.is_some_and(|p| inside(sbr, p.0, p.1));
            match k {
                CardButton::Up | CardButton::Down => {
                    button_bg(cr, th, sbr, false, hot, false);
                    arrow(cr, sbr, k == CardButton::Up, th.text);
                }
                CardButton::Half => {
                    button_bg(cr, th, sbr, card.half, hot, false);
                    label(
                        cr,
                        sbr,
                        if card.half { "Full" } else { "Half" },
                        th.text,
                        false,
                    );
                }
                CardButton::Size => {
                    let s = card
                        .size
                        .unwrap_or_else(|| kind.map_or(Size::M, |kd| kd.default));
                    button_bg(cr, th, sbr, false, hot, false);
                    label(cr, sbr, size_label(s, compact), th.text, false);
                }
                CardButton::Remove => {
                    button_bg(cr, th, sbr, false, hot, false);
                    cross(cr, sbr, th.accent);
                }
            }
        }
    }

    if let (true, Some(at)) = (dragging, state.drop_at) {
        if let Some(geom) = drop_line_geom(rects, pw, at) {
            draw::set(cr, th.accent);
            match geom {
                DropLine::Horizontal { x, y, w } => {
                    let sy = (oy + y).clamp(origin.1 + 2.0, origin.1 + ph - 3.0);
                    draw::round_rect(cr, ox + x, sy - 1.5, w, 3.0, 1.5);
                    let _ = cr.fill();
                }
                DropLine::Vertical { x, y, h } => {
                    let sy = (oy + y).clamp(origin.1 + 2.0, origin.1 + ph - 18.0);
                    draw::round_rect(
                        cr,
                        ox + x - 1.5,
                        sy,
                        3.0,
                        h.min(origin.1 + ph - sy - 2.0),
                        1.5,
                    );
                    let _ = cr.fill();
                }
            }
        }
    }
    let _ = cr.restore();

    // the floating edit bar
    let bar = state.bar_rect;
    draw::card(cr, th, bar.0, bar.1, bar.2, bar.3);
    draw::round_rect(
        cr,
        bar.0 + 0.5,
        bar.1 + 0.5,
        bar.2 - 1.0,
        bar.3 - 1.0,
        th.corners,
    );
    draw::set(cr, th.accent.alpha(0.7));
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    let buttons = bar_buttons(bar);
    let row0 = buttons
        .iter()
        .find(|(k, _)| *k == BarButton::Undo)
        .map(|(_, r)| *r)
        .unwrap_or(bar);
    let heading = format!("Editing {}", panel_title(panel));
    draw::text_max(
        cr,
        bar.0 + 12.0,
        row0.1 + (row0.3 - 15.0) / 2.0,
        (row0.0 - bar.0 - 20.0).max(20.0),
        &Text {
            size: 13.0,
            bold: true,
            color: th.text,
            text: &heading,
        },
    );
    let shown_w = display_width(panel.width);
    if let Some((_, nr)) = buttons.iter().find(|(k, _)| *k == BarButton::Narrower) {
        let y = nr.1 + (nr.3 - 13.0) / 2.0;
        draw::text(
            cr,
            bar.0 + 12.0,
            y,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: "Width",
            },
        );
        let value = format!("{shown_w} px");
        draw::text_center(
            cr,
            nr.0 + nr.2 + WIDTH_VALUE_W / 2.0,
            y,
            &Text {
                size: 12.0,
                bold: false,
                color: th.text,
                text: &value,
            },
        );
    }
    for (k, r) in buttons {
        let (text, on, primary, enabled) = match k {
            BarButton::Undo => ("Undo", false, false, !state.undo.is_empty()),
            BarButton::Reset => (
                if state.confirm_reset {
                    "Sure?"
                } else {
                    "Reset"
                },
                state.confirm_reset,
                false,
                has_default(&panel.name),
            ),
            BarButton::Add => ("Add", state.adding, false, true),
            BarButton::Narrower => ("\u{2212}", false, false, shown_w > WIDTH_MIN),
            BarButton::Wider => ("+", false, false, shown_w < WIDTH_MAX),
            BarButton::Hover => (
                if panel.hover {
                    "Hover: on"
                } else {
                    "Hover: off"
                },
                panel.hover,
                false,
                true,
            ),
            BarButton::Cancel => ("Cancel", false, false, true),
            BarButton::Done => ("Done", false, true, true),
        };
        let hot = enabled && pointer.is_some_and(|p| inside(r, p.0, p.1));
        button_bg(cr, th, r, on, hot, primary);
        let color = if primary {
            th.on_accent
        } else if !enabled {
            th.muted.alpha(0.5)
        } else {
            th.text
        };
        label(cr, r, text, color, primary);
    }

    if state.adding {
        draw_add_list(cr, cx, cards, pointer, state);
    }
}

fn draw_add_list(
    cr: &Context,
    cx: &Cx<'_>,
    cards: &[CardRef],
    pointer: Option<(f64, f64)>,
    state: &EditState,
) {
    let th = cx.theme;
    let apps = cx.apps;
    let add = state.add_rect;
    draw::card(cr, th, add.0, add.1, add.2, add.3);
    draw::round_rect(
        cr,
        add.0 + 0.5,
        add.1 + 0.5,
        add.2 - 1.0,
        add.3 - 1.0,
        th.corners,
    );
    draw::set(cr, th.accent.alpha(0.5));
    cr.set_line_width(1.0);
    let _ = cr.stroke();
    draw::title(cr, th, add.0 + 14.0, add.1 + 12.0, "Add a card");

    let entries = add_entries(cards, apps);
    if entries.is_empty() {
        draw::text(
            cr,
            add.0 + 14.0,
            add.1 + ADD_TITLE_H + 6.0,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: "Every card is in the panel.",
            },
        );
        return;
    }

    let view = add_viewport(add);
    let (items, scroll, max_scroll) = build_add_items(
        add,
        &entries,
        state.expanded_add.as_deref(),
        state.add_scroll,
    );

    let _ = cr.save();
    cr.rectangle(view.0 + 1.0, view.1, view.2 - 2.0, view.3);
    cr.clip();

    for item in items {
        match item {
            AddItem::Group { title, rect } => {
                draw::text_max(
                    cr,
                    rect.0,
                    rect.1 + (rect.3 - 12.0) / 2.0,
                    rect.2,
                    &Text {
                        size: 11.0,
                        bold: true,
                        color: th.muted,
                        text: title,
                    },
                );
            }
            AddItem::Entry {
                owner: _,
                name,
                icon,
                expanded,
                first_in_group,
                rect,
            } => {
                if !first_in_group {
                    draw::set(cr, th.text.alpha(0.06));
                    cr.rectangle(rect.0 + 6.0, rect.1, rect.2 - 12.0, 1.0);
                    let _ = cr.fill();
                }
                let hot = pointer.is_some_and(|p| inside(view, p.0, p.1) && inside(rect, p.0, p.1));
                if hot || expanded {
                    draw::round_rect(
                        cr,
                        rect.0,
                        rect.1 + 2.0,
                        rect.2,
                        rect.3 - 4.0,
                        th.small_radius(),
                    );
                    draw::set(
                        cr,
                        if expanded {
                            th.accent.alpha(0.14)
                        } else {
                            th.text.alpha(0.07)
                        },
                    );
                    let _ = cr.fill();
                }
                let icon_size = 24.0;
                let ix = rect.0 + 8.0;
                let iy = rect.1 + (rect.3 - icon_size) / 2.0;
                match icon {
                    Some(ic) => icons::draw_app_icon(cr, cx, &ic, &name, ix, iy, icon_size),
                    None => draw_provider_badge(cr, th, &name, ix, iy, icon_size),
                }
                let tx = ix + icon_size + 10.0;
                let tw = (rect.0 + rect.2 - 12.0 - tx).max(20.0);
                draw::text_max(
                    cr,
                    tx,
                    rect.1 + (rect.3 - 14.0) / 2.0,
                    tw,
                    &Text {
                        size: 13.0,
                        bold: expanded,
                        color: th.text,
                        text: &name,
                    },
                );
            }
            AddItem::Kind {
                owner: _,
                kind,
                rect,
                button,
            } => {
                draw::set(cr, th.text.alpha(0.05));
                cr.rectangle(rect.0, rect.1, rect.2 - 6.0, 1.0);
                let _ = cr.fill();
                let tw = (button.0 - rect.0 - 10.0).max(20.0);
                draw::text_max(
                    cr,
                    rect.0 + 4.0,
                    rect.1 + 7.0,
                    tw,
                    &Text {
                        size: 12.5,
                        bold: true,
                        color: th.text,
                        text: kind.title,
                    },
                );
                draw::text_max(
                    cr,
                    rect.0 + 4.0,
                    rect.1 + 26.0,
                    tw,
                    &Text {
                        size: 11.5,
                        bold: false,
                        color: th.muted,
                        text: kind.about,
                    },
                );
                let hot =
                    pointer.is_some_and(|p| inside(view, p.0, p.1) && inside(button, p.0, p.1));
                if hot {
                    draw::round_rect(
                        cr,
                        button.0,
                        button.1,
                        button.2,
                        button.3,
                        th.small_radius(),
                    );
                    draw::set(cr, th.accent.alpha(0.18));
                    let _ = cr.fill();
                }
                draw::round_rect(
                    cr,
                    button.0 + 0.5,
                    button.1 + 0.5,
                    button.2 - 1.0,
                    button.3 - 1.0,
                    th.small_radius(),
                );
                draw::set(cr, th.accent.alpha(0.65));
                cr.set_line_width(1.0);
                let _ = cr.stroke();
                label(cr, button, "Add", th.accent_text, true);
            }
        }
    }

    if max_scroll > 0.0 {
        let total_h = view.3 + max_scroll;
        let thumb_h = ((view.3 / total_h) * view.3).clamp(20.0, view.3);
        let track = (view.3 - thumb_h).max(1.0);
        let thumb_y = view.1 + (scroll / max_scroll) * track;
        draw::round_rect(cr, add.0 + add.2 - 6.0, thumb_y, 3.0, thumb_h, 1.5);
        draw::set(cr, th.muted.alpha(0.45));
        let _ = cr.fill();
    }
    let _ = cr.restore();
}

/// Only safe shell tokens (card words, places, integers, `hover`/`nohover`, keys) may be written
/// into `~/.config/infynitae/config`, which the session shell sources.
pub fn writable(value: &str) -> bool {
    value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " +:_@,.-/=".contains(c))
}

pub fn panel_key(name: &str) -> String {
    format!("PANEL_{}", name.to_ascii_uppercase())
}

pub fn rewrite_config(config_text: &str, key: &str, value: &str) -> String {
    crate::config::with_value(config_text, key, value)
}

pub fn backup_line(path: &Path, key: &str, before_line: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let clean: String = before_line
        .chars()
        .filter(|&c| c != '\n' && c != '\r')
        .collect();
    let now = chrono::Local::now().format("%F %T");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{now} {key}=\"{clean}\"")
}

pub fn write_config_value(path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    if !writable(key) || !writable(value) || key.contains(' ') {
        return Err(std::io::Error::other("not a value the edge writes"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let updated = rewrite_config(&text, key, value);
    let tmp = path.with_extension(format!("edge-tmp.{}", std::process::id()));
    std::fs::write(&tmp, updated)?;
    std::fs::rename(&tmp, path)
}

pub fn save_panel_to(
    config_path: &Path,
    backup_path: &Path,
    panel: &Panel,
    before_line: &str,
) -> std::io::Result<String> {
    let key = panel_key(&panel.name);
    let new_line = panel::line(panel);
    backup_line(backup_path, &key, before_line)?;
    write_config_value(config_path, &key, &new_line)?;
    Ok(new_line)
}

pub fn save_panel(panel: &Panel, before_line: &str) -> std::io::Result<String> {
    let cfg = crate::config::config_path();
    let bak = cfg.with_file_name("panels.backup");
    save_panel_to(&cfg, &bak, panel, before_line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel_cards(line: &str) -> Vec<CardRef> {
        let cfg = Config::from_text(&format!("PANEL_T=\"{line}\"\n"));
        let p = cfg.panels.iter().find(|p| p.name == "t").expect("panel t");
        panel::cards(p)
    }

    fn words(cards: &[CardRef]) -> String {
        cards
            .iter()
            .map(CardRef::word)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn sample_apps() -> Vec<App> {
        vec![
            App {
                id: "steam".into(),
                name: "Steam".into(),
                wm_class: None,
                exec: "steam".into(),
                icon: "steam".into(),
                terminal: false,
                categories: vec!["Game".into()],
            },
            App {
                id: "zebra".into(),
                name: "ZebraPad".into(),
                wm_class: None,
                exec: "zebrapad".into(),
                icon: "text-editor".into(),
                terminal: false,
                categories: vec!["Utility".into()],
            },
            App {
                id: "alpha".into(),
                name: "AlphaNote".into(),
                wm_class: None,
                exec: "alphanote".into(),
                icon: "accessories-text-editor".into(),
                terminal: false,
                categories: vec!["Utility".into()],
            },
        ]
    }

    #[test]
    fn move_up_down_drag_and_remove() {
        let apps = Vec::new();
        let mut cards =
            panel_cards("SUPER+T right 380 calendar/clock:s sound/volume settings/network");
        assert!(move_up(&mut cards, 1));
        assert_eq!(
            words(&cards),
            "sound/volume calendar/clock:s settings/network"
        );
        assert!(!move_up(&mut cards, 0));

        assert!(move_down(&mut cards, 1));
        assert_eq!(
            words(&cards),
            "sound/volume settings/network calendar/clock:s"
        );
        assert!(!move_down(&mut cards, 2));

        // drag last card (index 2) to top (slot 0)
        assert!(move_card(&mut cards, 2, 0));
        assert_eq!(
            words(&cards),
            "calendar/clock:s sound/volume settings/network"
        );
        // no-op slots right before/after dragged card
        assert!(!move_card(&mut cards, 1, 1));
        assert!(!move_card(&mut cards, 1, 2));

        assert!(remove_card(&mut cards, 1, &apps));
        assert_eq!(words(&cards), "calendar/clock:s settings/network");
    }

    #[test]
    fn half_width_requires_support_and_pairs_with_neighbour() {
        let apps = Vec::new();
        // sound/volume does not support half; calendar/clock and monitor/system do? Check kinds
        // Wait: calendar/clock (half: true), music/playing (half: true?), settings/network (half)
        let mut cards = panel_cards("SUPER+T right 380 sound/volume settings/network");
        let vol_half = resolve_kind(&cards[0], &apps).is_some_and(|(_, k)| k.half);
        assert!(!vol_half);
        assert!(!toggle_half(&mut cards, 0, &apps));

        // pair two half-capable cards when available
        let mut pair_cards = Vec::new();
        for p in provider::providers() {
            for k in p.cards() {
                if k.half && pair_cards.len() < 2 {
                    pair_cards.push(CardRef::new(p.id(), k.id));
                }
            }
        }
        assert_eq!(pair_cards.len(), 2, "need at least two half-capable cards");
        assert!(toggle_half(&mut pair_cards, 0, &apps));
        assert!(pair_cards[0].half && pair_cards[1].half);
        assert!(toggle_half(&mut pair_cards, 0, &apps));
        assert!(!pair_cards[0].half && !pair_cards[1].half);
    }

    #[test]
    fn next_size_cycles_kind_sizes() {
        let apps = Vec::new();
        // find a kind with >1 sizes and one with 1 size
        let mut cards = panel_cards("SUPER+T right 380 calendar/clock sound/volume");
        assert!(next_size(&mut cards, 0, &apps));
        let first = cards[0].size;
        assert!(first.is_some());
        // cycling enough times returns to default (None)
        let (_, clock_kind) = resolve_kind(&cards[0], &apps).unwrap();
        for _ in 1..clock_kind.sizes.len() {
            assert!(next_size(&mut cards, 0, &apps));
        }
        assert_eq!(cards[0].size, None);

        // sound/volume only has one size
        let (_, vol_kind) = resolve_kind(&cards[1], &apps).unwrap();
        assert_eq!(vol_kind.sizes.len(), 1);
        assert!(!next_size(&mut cards, 1, &apps));
    }

    #[test]
    fn add_card_appends_at_end_and_dedups() {
        let mut cards = panel_cards("SUPER+T right 380 calendar/clock");
        assert!(add_card(&mut cards, CardRef::new("sound", "volume")));
        assert_eq!(words(&cards), "calendar/clock sound/volume");
        assert!(!add_card(&mut cards, CardRef::new("sound", "volume")));
    }

    #[test]
    fn undo_history_is_capped_at_thirty() {
        let mut st = EditState::default();
        for i in 0..35 {
            st.push_undo(Snapshot {
                cards: Vec::new(),
                width: 300 + i,
                hover: false,
            });
        }
        assert_eq!(st.undo.len(), UNDO_MAX);
        assert_eq!(st.pop_undo().unwrap().width, 334);
    }

    #[test]
    fn defaults_exist_for_builtin_panels_and_width_clamps() {
        for name in ["control", "apps", "windows", "clipboard"] {
            assert!(has_default(name));
            let (cards, _, _) = default_panel_state(name).unwrap();
            assert!(!cards.is_empty(), "empty default for {name}");
        }
        assert!(!has_default("custom"));
        assert_eq!(adjust_width(0, 20), Some(400));
        assert_eq!(adjust_width(300, -20), None);
        assert_eq!(adjust_width(560, 20), None);
        assert_eq!(adjust_width(320, -20), Some(300));
    }

    #[test]
    fn add_list_groups_orders_filters_and_resolves() {
        let apps = sample_apps();
        let cards = panel_cards("SUPER+T right 380 settings/network alpha/app");
        let entries = add_entries(&cards, &apps);

        let groups: Vec<AddGroup> = entries.iter().map(|e| e.group).collect();
        // Infynitae must precede RichCards, which precedes SimpleCard
        let first_rich = groups
            .iter()
            .position(|g| *g == AddGroup::RichCards)
            .unwrap();
        let first_simple = groups
            .iter()
            .position(|g| *g == AddGroup::SimpleCard)
            .unwrap();
        assert!(first_rich > 0 && first_simple > first_rich);

        // Steam is claimed -> listed in Rich cards, NOT as Listed::WithoutApp under Infynitae
        assert!(!entries
            .iter()
            .any(|e| e.group == AddGroup::Infynitae && e.owner == "steam"));
        assert!(entries
            .iter()
            .any(|e| e.group == AddGroup::RichCards && e.owner == "steam"));

        // monitor and files have Listed::WithoutApp and no installed app claiming them -> in Infynitae
        assert!(entries
            .iter()
            .any(|e| e.group == AddGroup::Infynitae && e.owner == "monitor"));
        assert!(entries
            .iter()
            .any(|e| e.group == AddGroup::Infynitae && e.owner == "files"));

        // Claiming `files` with an installed file manager moves `files` from Infynitae to Rich cards
        let mut with_files = apps.clone();
        with_files.push(App {
            id: "org.gnome.Nautilus".into(),
            name: "Files".into(),
            wm_class: None,
            exec: "nautilus".into(),
            icon: "system-file-manager".into(),
            terminal: false,
            categories: vec!["Utility".into(), "FileManager".into()],
        });
        let entries_with_files = add_entries(&cards, &with_files);
        assert!(!entries_with_files
            .iter()
            .any(|e| e.group == AddGroup::Infynitae && e.owner == "files"));
        assert!(entries_with_files
            .iter()
            .any(|e| e.group == AddGroup::RichCards && e.owner == "org.gnome.Nautilus"));

        // AlphaNote (`alpha/app`) is already in the panel, so only ZebraPad stays under Simple card
        let simple: Vec<&str> = entries
            .iter()
            .filter(|e| e.group == AddGroup::SimpleCard)
            .map(|e| e.owner.as_str())
            .collect();
        assert_eq!(simple, vec!["zebra"]);

        // Infynitae is sorted by provider name
        let inf_names: Vec<String> = entries
            .iter()
            .filter(|e| e.group == AddGroup::Infynitae)
            .map(|e| e.name.to_lowercase())
            .collect();
        let mut sorted = inf_names.clone();
        sorted.sort();
        assert_eq!(inf_names, sorted);

        // Settings already has `network` in the panel, so `network` is omitted from its kinds
        let settings = entries.iter().find(|e| e.owner == "settings").unwrap();
        assert!(!settings.kinds.iter().any(|k| k.id == "network"));
        assert!(settings.kinds.iter().any(|k| k.id == "bluetooth"));
    }

    #[test]
    fn drop_index_matches_full_and_half_rows() {
        // full rows: 0 at y=28..128, 1 at y=168..268, 2 at y=308..408
        let rects = vec![
            (0, (12.0, 28.0, 356.0, 100.0)),
            (1, (12.0, 168.0, 356.0, 100.0)),
            (2, (12.0, 308.0, 356.0, 100.0)),
        ];
        // Dragging card 0: near middle of card 1's bottom moves to gap 2 (after card 1)
        assert_eq!(drop_index(&rects, 3, 0, 100.0, 240.0), Some(2));
        // Dragging card 2: above card 0's middle moves to gap 0 (top)
        assert_eq!(drop_index(&rects, 3, 2, 100.0, 50.0), Some(0));
        // Dragging card 2 into slot right above itself is a no-op
        assert_eq!(drop_index(&rects, 3, 2, 100.0, 320.0), None);

        // Two-card half row [0, 1] followed by full row [2]
        let half_rects = vec![
            (0, (12.0, 28.0, 172.0, 80.0)),
            (1, (196.0, 28.0, 172.0, 80.0)),
            (2, (12.0, 148.0, 356.0, 100.0)),
        ];
        assert_eq!(drop_index(&half_rects, 3, 2, 190.0, 65.0), Some(1));
    }

    #[test]
    fn bar_and_add_never_overlap_and_stay_on_screen() {
        let screen = (1920.0, 1080.0);
        let origin = (1528.0, 12.0);
        let bar = bar_rect(origin, 380.0, 1056.0, screen);
        let entries = add_entries(&[], &sample_apps());
        let ch = add_content_height(&entries, None);
        let add = add_rect(origin, 380.0, screen, ch, bar);
        assert!(bar.0 >= 8.0 && bar.0 + bar.2 <= origin.0);
        assert!(add.1 >= bar.1 + bar.3 + 8.0);
        assert!(add.1 + add.3 <= screen.1 - 8.0);
    }

    #[test]
    fn save_rewrites_panel_line_adds_missing_line_and_appends_backup() {
        let dir = std::env::temp_dir().join(format!("infynitae-edit-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config");
        let bak_path = dir.join("panels.backup");
        std::fs::write(
            &cfg_path,
            "# keep comment\nACCENT=\"#cc3388\"\nPANEL_CONTROL=\"SUPER+A right 380 Clock\"\n",
        )
        .unwrap();

        let cfg = Config::from_text(&std::fs::read_to_string(&cfg_path).unwrap());
        let mut p = cfg
            .panels
            .iter()
            .find(|p| p.name == "control")
            .unwrap()
            .clone();
        let before = panel::line(&p);
        p.words = vec![
            "calendar/clock:s".into(),
            "settings/quick=dnd,nightlight".into(),
        ];
        p.width = 420;
        p.hover = true;
        p.placed = false;

        let saved = save_panel_to(&cfg_path, &bak_path, &p, &before).unwrap();
        let text = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(text.contains("# keep comment"));
        assert!(text.contains("ACCENT=\"#cc3388\""));
        assert!(text.contains(&format!("PANEL_CONTROL=\"{saved}\"")));

        let bak = std::fs::read_to_string(&bak_path).unwrap();
        assert!(bak.contains(&format!("PANEL_CONTROL=\"{before}\"")));

        // A built-in panel with no line yet gets one appended
        let apps_panel = cfg
            .panels
            .iter()
            .find(|p| p.name == "apps")
            .unwrap()
            .clone();
        let before_apps = panel::line(&apps_panel);
        save_panel_to(&cfg_path, &bak_path, &apps_panel, &before_apps).unwrap();
        let text2 = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(text2.contains(&format!("PANEL_APPS=\"{before_apps}\"")));

        // Shell safety rejects metacharacters
        assert!(!writable("bad`cmd`"));
        assert!(!writable("bad$VAR"));
        assert!(!writable("bad\"quote"));
        assert!(writable(
            "SUPER+A right 420 hover settings/quick=dnd,nightlight@half"
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
