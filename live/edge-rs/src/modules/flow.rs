//! Editing a panel (2026-11-10, the owner: "fully rework it ... the edit mode"; design agreed on
//! the canvas https://claude.ai/artifact/37AMrAAnorStoE942nh3f1, reviewed by Fable 5.2).
//!
//! A panel is its modules in order, each as wide as the panel or half of it, each as tall as
//! what it shows. Editing keeps that look: nothing is placed by pixels, nothing freezes. The
//! module under the pointer gets a small bar (move up, move down, half or full width, its look,
//! take it out); a module is also moved by dragging it, a line showing where it lands. The run
//! of switches is one card (Quick settings) and moves as one; each switch in it has its own x.
//! A bar beside the panel holds Undo, Reset (asked twice), Add, the width, hover on or off,
//! Cancel and Done; Add lists what the panel does not hold yet, each with what it is for.
//!
//! The desktop's widgets keep the free canvas of `edit.rs`.

use crate::config::{Item, Module, Panel};
use crate::draw::{self, Text, GAP};
use crate::modules::edit::{module_display_name, EditState};
use crate::theme::Theme;
use cairo::Context;
use std::ops::Range;

pub type Rect = (f64, f64, f64, f64);

/// The narrowest and widest a panel is made with − and +, and the step.
pub const WIDTH_MIN: i32 = 300;
pub const WIDTH_MAX: i32 = 560;
pub const WIDTH_STEP: i32 = 20;
/// Undo keeps this many steps.
pub const UNDO_MAX: usize = 30;

/// The bar: two rows (the panel's name with Undo, Reset, Add and Done; the width and the rest).
const BAR_ROW: f64 = 36.0;
const BAR_H: f64 = 2.0 * BAR_ROW + 4.0;
/// Add's list: a card of its own beside the panel, a title row and a row per module (its name,
/// what it is for under it, an Add button).
const ADD_W: f64 = 300.0;
const ADD_TITLE_H: f64 = 40.0;
const ADD_ROW_H: f64 = 50.0;
/// While a panel is edited each card has this strip above it: its name, and on the card under the
/// pointer (or the one chosen) its buttons. Reserved in the layout, so a bar never covers a card.
pub const STRIP_H: f64 = 28.0;
const BTN: f64 = 24.0;

/// What a panel module is for, in Add's list.
pub fn module_description(m: Module) -> &'static str {
    match m {
        Module::Search => "Type to find apps, windows, clips",
        Module::Favorites => "Your pinned apps",
        Module::Apps => "Every installed app",
        Module::Windows => "The open windows",
        Module::Clipboard => "What you copied lately",
        Module::Clock => "Time and date; click: the calendar",
        Module::Calendar => "This month; click a day: an event",
        Module::Volume => "Volume, brightness, outputs, apps",
        Module::Media => "The song playing and its controls",
        Module::Notifications => "Recent notifications",
        Module::Tray => "Apps running in the background",
        Module::Session => "Lock, sleep, log out, restart, shut down",
        Module::Memory => "What uses memory; close what you do not need",
        Module::System => "Graphics card, video memory, heat",
        Module::Drives => "Each drive and its free space",
        Module::Wifi => "Wired or Wi-Fi, and the networks",
        Module::Bluetooth => "On or off, and the devices",
        Module::Dnd => "Hold notifications back",
        Module::Night => "Warmer colours in the evening",
        Module::Keyboard => "Switch the keyboard layout",
        Module::Osk => "The keyboard on the screen",
        Module::Power => "Quiet, balanced or fast",
        Module::Replay => "Keep the last minutes to save",
    }
}

/// The panel's blocks, in order: one module, or a run of switches (one card).
pub fn blocks(p: &Panel) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < p.items.len() {
        let mut j = i + 1;
        if p.items[i].module.is_switch() {
            while j < p.items.len() && p.items[j].module.is_switch() {
                j += 1;
            }
        }
        out.push(i..j);
        i = j;
    }
    out
}

/// Moves block `from` so that it lands before block `to` (`to` = the number of blocks: last).
/// Two runs of switches that end up next to each other become one card.
pub fn move_block(p: &mut Panel, from: usize, to: usize) {
    let bs = blocks(p);
    if from >= bs.len() || to > bs.len() || to == from || to == from + 1 {
        return;
    }
    let moving: Vec<Item> = p.items[bs[from].clone()].to_vec();
    let mut rest: Vec<Vec<Item>> = bs
        .iter()
        .enumerate()
        .filter(|(k, _)| *k != from)
        .map(|(_, r)| p.items[r.clone()].to_vec())
        .collect();
    let at = if to > from { to - 1 } else { to };
    rest.insert(at.min(rest.len()), moving);
    p.items = rest.into_iter().flatten().collect();
}

/// Takes module `i` out (a switch: only that switch).
pub fn remove_item(p: &mut Panel, i: usize) {
    if i < p.items.len() {
        p.items.remove(i);
    }
}

/// Takes block `b` out (the whole Quick settings card for a run of switches).
pub fn remove_block(p: &mut Panel, b: usize) {
    if let Some(r) = blocks(p).get(b).cloned() {
        p.items.drain(r);
    }
}

/// Adds `m` to the panel: a switch joins the first run of switches (or starts one at the end),
/// any other module goes last. Nothing when it is there already or the panel is full.
pub fn add_module(p: &mut Panel, m: Module) {
    if p.has(m) || p.items.len() >= 16 {
        return;
    }
    let it = Item {
        module: m,
        look: None,
        rect: None,
        half: false,
    };
    if m.is_switch() {
        if let Some(r) = blocks(p)
            .into_iter()
            .find(|r| p.items[r.start].module.is_switch())
        {
            p.items.insert(r.end, it);
            return;
        }
    }
    p.items.push(it);
}

/// Half width on or off for module `i`. A half needs a half beside it, so turning a lone card
/// half also makes the next card (or else the one before) half: the change shows at once.
/// Turning it off turns its partner off too.
pub fn toggle_half(p: &mut Panel, i: usize) {
    let Some(it) = p.items.get(i) else { return };
    if it.module.is_switch() {
        return;
    }
    let on = !it.half;
    let card = |p: &Panel, k: usize| p.items.get(k).is_some_and(|x| !x.module.is_switch());
    // its partner: the halves pair up in order, so count the halves before it
    let halves_before = p.items[..i]
        .iter()
        .rev()
        .take_while(|x| x.half && !x.module.is_switch())
        .count();
    let partner = if halves_before % 2 == 1 {
        Some(i - 1)
    } else if card(p, i + 1) && p.items[i + 1].half {
        Some(i + 1)
    } else {
        None
    };
    p.items[i].half = on;
    if on {
        if partner.is_none() {
            if card(p, i + 1) {
                p.items[i + 1].half = true;
            } else if i > 0 && card(p, i - 1) {
                p.items[i - 1].half = true;
            }
        }
    } else if let Some(k) = partner {
        p.items[k].half = false;
    }
}

/// The next look of module `i` (its looks in turn; the first after the last).
pub fn next_look(p: &mut Panel, i: usize) {
    let Some(it) = p.items.get_mut(i) else {
        return;
    };
    let (names, def) = it.module.looks();
    if names.is_empty() {
        return;
    }
    let cur = it.look.unwrap_or(def);
    let next = (cur + 1) % names.len();
    it.look = if next == def { None } else { Some(next) };
}

/// The name of module `i`'s look, None when it has one look only.
pub fn look_name(it: &Item) -> Option<&'static str> {
    let (names, def) = it.module.looks();
    (!names.is_empty() && !it.module.is_switch()).then(|| names[it.look.unwrap_or(def)])
}

/// Each block with its card's rectangle in the panel (`rects`: `layout_in`'s, one per module);
/// blocks showing nothing are left out.
pub fn block_rects(
    p: &Panel,
    rects: &[Rect],
    world: &crate::modules::World,
    th: &Theme,
    pw: f64,
) -> Vec<(usize, Rect)> {
    let mut out = Vec::new();
    let mut y = 0.0_f64;
    let strip = if world.edit_preview && !p.desktop {
        STRIP_H
    } else {
        0.0
    };
    for (b, r) in blocks(p).into_iter().enumerate() {
        if p.items[r.start].module.is_switch() {
            let (_, card_h) = crate::modules::switches::layout_run(
                &p.items[r.clone()],
                world,
                &th.switch_look,
                pw,
                y + strip,
            );
            if card_h > 0.0 {
                out.push((b, (0.0, y + strip, pw, card_h)));
                y += strip + card_h + GAP;
            }
        } else if let Some(&rect) = rects.get(r.start) {
            if rect.2 > 0.0 && rect.3 > 0.0 {
                out.push((b, rect));
                y = y.max(rect.1 + rect.3 + GAP);
            }
        }
    }
    out
}

/// The block under `(px, py)` while editing (panel coordinates): its card or the strip above it
/// (where its name and buttons are).
pub fn block_at(brects: &[(usize, Rect)], px: f64, py: f64) -> Option<usize> {
    brects
        .iter()
        .find(|(_, r)| px >= r.0 && px <= r.0 + r.2 && py >= r.1 - STRIP_H && py <= r.1 + r.3)
        .map(|(b, _)| *b)
}

/// Where a block dragged to `py` (panel coordinates) lands: before the first card whose middle
/// is below it, else last.
pub fn drop_index(brects: &[(usize, Rect)], py: f64, n_blocks: usize) -> usize {
    let mut seen_y = f64::NEG_INFINITY;
    for (b, r) in brects {
        // the second half of a pair shares its row: one slot per row
        if (r.1 - seen_y).abs() < 0.5 {
            continue;
        }
        seen_y = r.1;
        if py < r.1 + r.3 / 2.0 {
            return *b;
        }
    }
    n_blocks
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardButton {
    Up,
    Down,
    Half,
    Look,
    Remove,
}

/// The buttons on block `b`'s bar, right to left as they fit, each with its rectangle in the
/// panel; the bar's own rectangle first.
pub fn card_buttons(p: &Panel, b: usize, r: Rect, pw: f64) -> (Rect, Vec<(CardButton, Rect)>) {
    let strip = (r.0, r.1 - STRIP_H, r.2.max(0.0), STRIP_H - 4.0);
    let bs = blocks(p);
    let Some(range) = bs.get(b) else {
        return (strip, Vec::new());
    };
    let it = &p.items[range.start];
    let mut want = vec![(CardButton::Remove, BTN)];
    if b > 0 {
        want.push((CardButton::Up, BTN));
    }
    if b + 1 < bs.len() {
        want.push((CardButton::Down, BTN));
    }
    if !it.module.is_switch() && pw >= crate::HALF_MIN_PANEL {
        want.push((CardButton::Half, 40.0));
    }
    if look_name(it).is_some() {
        want.push((CardButton::Look, 66.0));
    }
    let mut out = Vec::new();
    let mut x = strip.0 + strip.2 - 1.0;
    let left_limit = strip.0 + 22.0;
    for (k, w) in want {
        if x - w < left_limit {
            break;
        }
        x -= w;
        out.push((k, (x, strip.1 + 1.0, w, strip.3 - 2.0)));
        x -= 4.0;
    }
    (strip, out)
}

/// The x on a switch's cell, in the panel: its rectangle.
pub fn switch_x(cell: Rect) -> Rect {
    (
        cell.0 + cell.2 - 22.0,
        cell.1 + (cell.3 - 18.0) / 2.0,
        18.0,
        18.0,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarButton {
    Undo,
    Reset,
    Add,
    Narrower,
    Wider,
    Edge,
    Hover,
    Cancel,
    Done,
}

/// The first row's buttons from its right end, with their widths: Done last (furthest right).
const ROW1: [(BarButton, f64); 4] = [
    (BarButton::Done, 56.0),
    (BarButton::Add, 46.0),
    (BarButton::Reset, 52.0),
    (BarButton::Undo, 50.0),
];

/// The next screen edge for the Edge button: top, right, bottom, left, round.
pub fn next_side(s: crate::config::Side) -> crate::config::Side {
    use crate::config::Side::*;
    match s {
        Top => Right,
        Right => Bottom,
        Bottom => Left,
        Left => Top,
    }
}

/// Whether Reset has a default to go back to (the four built-in panels; a panel of your own
/// has none).
pub fn has_default(p: &Panel) -> bool {
    matches!(
        p.name.as_str(),
        "control" | "apps" | "windows" | "clipboard"
    )
}

/// The bar's rectangle on the screen: beside the panel where there is room (above it, below
/// it, to its inner side), never over the screen's edge. It is placed as it is closed; while
/// adding, Add's list hangs under its buttons as far as the screen goes (it scrolls for the
/// rest), so opening Add never moves the buttons.
pub fn bar_rect(
    origin: (f64, f64),
    pw: f64,
    ph: f64,
    screen: (f64, f64),
    adding_rows: usize,
) -> Rect {
    let (ox, oy) = origin;
    let (sw, sh) = screen;
    // wide enough for the second row (the width, the edge, hover, Cancel) whatever the panel's width
    let bw = pw.max(BAR_MIN_W).min(sw - 16.0);
    let bh = BAR_H;
    let clamp_x = |x: f64| x.clamp(8.0, (sw - bw - 8.0).max(8.0));
    let clamp_y = |y: f64| y.clamp(8.0, (sh - bh - 8.0).max(8.0));
    // above the panel (Add's list then hangs over it), below it only with room for six of
    // Add's rows, else beside it, where the screen's height is free (a tall panel left a strip
    // under it that showed three rows)
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
    let _ = adding_rows;
    (x, y, bw, BAR_H)
}

/// How many of Add's rows its card shows at once.
pub fn add_rows_shown(add: Rect) -> usize {
    ((add.3 - ADD_TITLE_H - 8.0).max(0.0) / ADD_ROW_H).floor() as usize
}

/// The bar's buttons with their rectangles on the screen: the first row's from the right (Done,
/// Add, Reset, Undo), the second row's from the left after the word "Width" (narrower, the width,
/// wider) and from the right (Cancel, Hover, the edge).
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
    for (k, w) in [
        (BarButton::Cancel, 62.0),
        (BarButton::Hover, 84.0),
        (BarButton::Edge, 92.0),
    ] {
        x -= w;
        out.push((k, (x, row(1.0), w, h)));
        x -= 4.0;
    }
    out
}

/// The bar is at least this wide: its second row's words fit.
const BAR_MIN_W: f64 = 440.0;

/// The word "Width" on the bar's second row, and the width between − and +.
const WIDTH_LABEL_W: f64 = 46.0;
const WIDTH_VALUE_W: f64 = 58.0;

/// Add's list: a card beside the panel, on its inner side (towards the screen's middle), its top
/// with the panel's; over the panel only when the screen has no room beside it.
pub fn add_rect(origin: (f64, f64), pw: f64, screen: (f64, f64), rows: usize, bar: Rect) -> Rect {
    let (ox, oy) = origin;
    let (sw, sh) = screen;
    let x = if ox - ADD_W - 8.0 >= 8.0 {
        ox - ADD_W - 8.0
    } else if ox + pw + 8.0 + ADD_W <= sw - 8.0 {
        ox + pw + 8.0
    } else {
        (sw - ADD_W - 8.0).max(8.0)
    };
    // under the bar when the bar is on the same side (a panel as tall as the screen has its bar beside
    // it): the two never overlap, so the bar's Add button closes the list again
    let beside_bar = x < bar.0 + bar.2 && bar.0 < x + ADD_W;
    let top = if beside_bar {
        oy.max(bar.1 + bar.3 + 8.0)
    } else {
        oy
    };
    let want = ADD_TITLE_H + rows.max(1) as f64 * ADD_ROW_H + 8.0;
    let h = want.min((sh - 8.0 - top).max(ADD_TITLE_H + ADD_ROW_H));
    let y = top.clamp(8.0, (sh - h - 8.0).max(8.0));
    (x, y, ADD_W, h)
}

/// Add's rows: each module the panel can take, with its rectangle on the screen, and the Add
/// button's rectangle in it.
pub fn add_rows(add: Rect, choices: &[Module], scroll: usize) -> Vec<(Module, Rect)> {
    choices
        .iter()
        .skip(scroll)
        .enumerate()
        .map(|(k, &m)| {
            (
                m,
                (
                    add.0 + 8.0,
                    add.1 + ADD_TITLE_H + k as f64 * ADD_ROW_H,
                    add.2 - 16.0,
                    ADD_ROW_H - 4.0,
                ),
            )
        })
        .filter(|(_, r)| r.1 + r.3 <= add.1 + add.3)
        .collect()
}

/// The Add button in an Add row.
fn add_button(row: Rect) -> Rect {
    (
        row.0 + row.2 - 54.0,
        row.1 + (row.3 - 26.0) / 2.0,
        50.0,
        26.0,
    )
}

/// Whether two rectangles share any area.
pub fn overlaps(a: Rect, b: Rect) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

fn inside(r: Rect, x: f64, y: f64) -> bool {
    x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3
}

/// What a press at `(x, y)` on the screen hits in the bar.
pub fn bar_hit(
    bar: Rect,
    add: Rect,
    choices: &[Module],
    adding: bool,
    scroll: usize,
    x: f64,
    y: f64,
) -> Option<BarHit> {
    if adding && inside(add, x, y) {
        // a whole row adds (the button shows where; a near miss still counts)
        for (m, r) in add_rows(add, choices, scroll) {
            if inside(r, x, y) {
                return Some(BarHit::Add(m));
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarHit {
    Button(BarButton),
    Add(Module),
    Nothing,
}

fn label(cr: &Context, r: Rect, s: &str, color: crate::theme::Rgba, bold: bool) {
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

fn button_bg(cr: &Context, th: &Theme, r: Rect, on: bool, primary: bool) {
    draw::round_rect(cr, r.0, r.1, r.2, r.3, th.small_radius());
    if primary {
        draw::set(cr, th.accent);
    } else if on {
        draw::set(cr, th.accent.alpha(0.24));
    } else {
        draw::set(cr, th.text.alpha(0.08));
    }
    let _ = cr.fill();
}

fn arrow(cr: &Context, r: Rect, up: bool, color: crate::theme::Rgba) {
    let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    // the head at cy - 5 * d: up for Up
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

fn cross(cr: &Context, r: Rect, color: crate::theme::Rgba) {
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

/// Draws the editing over the panel and its bar: each card's outline, the bar of the card under
/// the pointer, the x of each switch in a hovered Quick settings card, the line where a dragged
/// card lands, and the bar (with Add's list).
#[allow(clippy::too_many_arguments)]
pub fn draw_overlay(
    cr: &Context,
    th: &Theme,
    panel: &Panel,
    rects: &[Rect],
    world: &crate::modules::World,
    origin: (f64, f64),
    size: (f64, f64),
    state: &EditState,
) {
    let (pw, ph) = size;
    let (ox, oy) = (origin.0, origin.1 - world.panel_scroll);
    // the cards' marks stay inside the panel (a scrolled panel clips its cards; marks drawn
    // outside it were left on the screen)
    let _ = cr.save();
    cr.rectangle(origin.0 - 2.0, origin.1 - 2.0, pw + 4.0, ph + 4.0);
    cr.clip();
    let brects = block_rects(panel, rects, world, th, pw);
    let bs = blocks(panel);
    let dragging = state.flow_drag.is_some_and(|d| d.moved);

    // every card: a thin accent outline, the one held a stronger one
    for (b, r) in &brects {
        let held = state.flow_drag.is_some_and(|d| d.block == *b && d.moved);
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

    // above every card its name; above the one under the pointer also its buttons
    for (b, r) in &brects {
        let first = &panel.items[bs[*b].start];
        let name = if first.module.is_switch() {
            "Quick settings"
        } else {
            module_display_name(first.module)
        };
        let shown = state.hover_block == Some(*b) && !dragging;
        let (strip, buttons) = card_buttons(panel, *b, *r, pw);
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
        // the grip (a card is moved by dragging it) and the name
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
                .map(|(_, r)| r.0 - 6.0)
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
                    text: name,
                },
            );
        }
        if !shown {
            continue;
        }
        let it = &panel.items[bs[*b].start];
        for (k, br) in buttons {
            let br = (ox + br.0, oy + br.1, br.2, br.3);
            match k {
                CardButton::Up | CardButton::Down => {
                    button_bg(cr, th, br, false, false);
                    arrow(cr, br, k == CardButton::Up, th.text);
                }
                CardButton::Half => {
                    button_bg(cr, th, br, it.half, false);
                    label(
                        cr,
                        br,
                        if it.half { "Full" } else { "Half" },
                        th.text,
                        false,
                    );
                }
                CardButton::Look => {
                    button_bg(cr, th, br, false, false);
                    label(cr, br, look_name(it).unwrap_or(""), th.text, false);
                }
                CardButton::Remove => {
                    button_bg(cr, th, br, false, false);
                    cross(cr, br, th.accent);
                }
            }
        }
    }
    // each switch's own x, on every run of switches
    for (b, _) in &brects {
        if !panel.items[bs[*b].start].module.is_switch() || dragging {
            continue;
        }
        for i in bs[*b].clone() {
            if let Some(&cell) = rects.get(i).filter(|c| c.2 > 0.0 && c.3 > 0.0) {
                let x = switch_x(cell);
                let x = (ox + x.0, oy + x.1, x.2, x.3);
                draw::round_rect(cr, x.0, x.1, x.2, x.3, th.small_radius().min(x.2 / 2.0));
                draw::set(cr, th.card.alpha(0.9));
                let _ = cr.fill();
                cross(cr, (x.0 - 2.0, x.1 - 2.0, x.2 + 4.0, x.3 + 4.0), th.muted);
            }
        }
    }

    // where a dragged card lands: a line
    if let (true, Some(at)) = (dragging, state.drop_at) {
        let y = match brects.iter().find(|(b, _)| *b == at) {
            Some((_, r)) => r.1 - GAP / 2.0,
            None => brects
                .last()
                .map(|(_, r)| r.1 + r.3 + GAP / 2.0)
                .unwrap_or(0.0),
        };
        let y = (oy + y).clamp(origin.1 + 1.0, origin.1 + ph - 2.0);
        draw::round_rect(cr, ox + 4.0, y - 1.5, pw - 8.0, 3.0, 1.5);
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }

    let _ = cr.restore();

    // the bar
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
    // first row: what is edited; second row: "Width" and the width between − and +
    let buttons = bar_buttons(bar);
    let row0 = buttons
        .iter()
        .find(|(k, _)| *k == BarButton::Undo)
        .map(|(_, r)| *r)
        .unwrap_or(bar);
    let title = format!("Editing {}", panel_title(panel));
    draw::text_max(
        cr,
        bar.0 + 12.0,
        row0.1 + (row0.3 - 15.0) / 2.0,
        (row0.0 - bar.0 - 20.0).max(20.0),
        &Text {
            size: 13.0,
            bold: true,
            color: th.text,
            text: &title,
        },
    );
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
        let value = format!("{} px", panel.width.max(0));
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
                has_default(panel),
            ),
            BarButton::Add => ("Add", state.adding, false, true),
            BarButton::Narrower => ("\u{2212}", false, false, panel.width > WIDTH_MIN),
            BarButton::Wider => ("+", false, false, panel.width < WIDTH_MAX),
            BarButton::Edge => (
                match panel.side {
                    crate::config::Side::Top => "Edge: top",
                    crate::config::Side::Right => "Edge: right",
                    crate::config::Side::Bottom => "Edge: bottom",
                    crate::config::Side::Left => "Edge: left",
                },
                false,
                false,
                true,
            ),
            // on: lit (touching the screen edge opens it)
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
        button_bg(cr, th, r, on, primary);
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
        let add = state.add_rect;
        draw::card(cr, th, add.0, add.1, add.2, add.3);
        draw::title(cr, th, add.0 + 14.0, add.1 + 12.0, "Add a module");
        let choices = crate::modules::edit::addable(panel);
        if choices.is_empty() {
            draw::text(
                cr,
                add.0 + 14.0,
                add.1 + ADD_TITLE_H + 6.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: "Every module is in the panel.",
                },
            );
        }
        for (m, r) in add_rows(add, &choices, state.add_scroll) {
            // a hairline between rows, the name over what it is for, an Add button
            draw::set(cr, th.text.alpha(0.07));
            cr.rectangle(r.0 + 6.0, r.1, r.2 - 12.0, 1.0);
            let _ = cr.fill();
            let btn = add_button(r);
            let text_w = (btn.0 - r.0 - 16.0).max(20.0);
            draw::text_max(
                cr,
                r.0 + 6.0,
                r.1 + 6.0,
                text_w,
                &Text {
                    size: 13.0,
                    bold: true,
                    color: th.text,
                    text: module_display_name(m),
                },
            );
            draw::text_max(
                cr,
                r.0 + 6.0,
                r.1 + 25.0,
                text_w,
                &Text {
                    size: 11.5,
                    bold: false,
                    color: th.muted,
                    text: module_description(m),
                },
            );
            draw::round_rect(
                cr,
                btn.0 + 0.5,
                btn.1 + 0.5,
                btn.2 - 1.0,
                btn.3 - 1.0,
                th.small_radius(),
            );
            draw::set(cr, th.accent.alpha(0.6));
            cr.set_line_width(1.0);
            let _ = cr.stroke();
            label(cr, btn, "Add", th.accent, true);
        }
    }
}

/// The panel's name as the bar says it ("Editing Control").
fn panel_title(panel: &Panel) -> String {
    let mut cs = panel.name.chars();
    match cs.next() {
        Some(c) => c.to_uppercase().chain(cs).collect(),
        None => "the panel".to_string(),
    }
}

/// How many rows Add's list takes.
pub fn add_row_count(panel: &Panel) -> usize {
    crate::modules::edit::addable(panel).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn panel(line: &str) -> Panel {
        let cfg = Config::from_text(&format!("PANEL_T=\"{line}\"\n"));
        cfg.panels
            .into_iter()
            .find(|p| p.name == "t")
            .unwrap()
            .flowed()
    }

    fn words(p: &Panel) -> String {
        p.items
            .iter()
            .map(|i| format!("{}{}", i.module.word(), if i.half { "@half" } else { "" }))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_run_of_switches_is_one_block_and_moves_as_one() {
        let mut p = panel("clock wifi dnd power volume session");
        assert_eq!(blocks(&p).len(), 4);
        move_block(&mut p, 1, 4);
        assert_eq!(words(&p), "clock volume session wifi dnd power");
        move_block(&mut p, 3, 0);
        assert_eq!(words(&p), "wifi dnd power clock volume session");
        move_block(&mut p, 0, 1); // onto itself: nothing
        assert_eq!(words(&p), "wifi dnd power clock volume session");
    }

    #[test]
    fn a_switch_added_joins_the_card_and_one_taken_out_leaves_the_rest() {
        let mut p = panel("clock wifi dnd volume");
        add_module(&mut p, Module::Night);
        assert_eq!(words(&p), "clock wifi dnd nightlight volume");
        add_module(&mut p, Module::Night);
        assert_eq!(p.items.len(), 5, "once");
        remove_item(&mut p, 2);
        assert_eq!(words(&p), "clock wifi nightlight volume");
        remove_block(&mut p, 1);
        assert_eq!(words(&p), "clock volume");
        add_module(&mut p, Module::Replay);
        assert_eq!(words(&p), "clock volume replay");
    }

    #[test]
    fn a_line_placed_by_hand_keeps_its_order_and_pairs_become_halves() {
        let p = panel("SUPER+A bottom right 361x708 hover clock@8,8,344x34 media@8,438,344x76 wifi@8,86,169x46 bluetooth@183,86,169x46 notifications@9,288,170x144 tray@183,290,170x143");
        assert!(!p.placed);
        assert_eq!(p.height, 0);
        assert_eq!(
            words(&p),
            "clock wifi bluetooth notifications@half tray@half media"
        );
        assert!(p.items.iter().all(|i| i.rect.is_none()));
        assert!(p.value().contains("notifications@half tray@half media"));
        assert!(!p.value().contains("361x708"));
    }

    #[test]
    fn halves_are_read_and_written() {
        let p = panel("top right 380 clock@half system@half volume");
        assert_eq!(words(&p), "clock@half system@half volume");
        assert!(p.value().ends_with("clock@half system@half volume"));
    }

    #[test]
    fn half_on_a_lone_card_pairs_it_and_off_unpairs_both() {
        let mut p = panel("clock volume system session");
        toggle_half(&mut p, 0);
        assert_eq!(words(&p), "clock@half volume@half system session");
        toggle_half(&mut p, 1);
        assert_eq!(words(&p), "clock volume system session");
        toggle_half(&mut p, 3);
        assert_eq!(
            words(&p),
            "clock volume system@half session@half",
            "the last pairs back"
        );
    }

    #[test]
    fn looks_go_round() {
        let mut p = panel("clock volume");
        assert_eq!(look_name(&p.items[0]), Some("time"));
        next_look(&mut p, 0);
        assert_eq!(look_name(&p.items[0]), Some("big"));
        next_look(&mut p, 0);
        next_look(&mut p, 0);
        assert_eq!(p.items[0].look, None, "back to its own look");
        assert_eq!(look_name(&p.items[1]), None);
    }

    #[test]
    fn a_drop_lands_before_the_card_whose_middle_is_below() {
        let br = vec![
            (0, (0.0, 0.0, 380.0, 60.0)),
            (1, (0.0, 70.0, 185.0, 100.0)),
            (2, (195.0, 70.0, 185.0, 80.0)),
            (3, (0.0, 180.0, 380.0, 40.0)),
        ];
        assert_eq!(drop_index(&br, 10.0, 4), 0);
        assert_eq!(drop_index(&br, 40.0, 4), 1);
        assert_eq!(drop_index(&br, 150.0, 4), 3, "a pair is one row");
        assert_eq!(drop_index(&br, 500.0, 4), 4);
    }

    #[test]
    fn the_bar_stays_on_the_screen_beside_a_full_height_panel() {
        let bar = bar_rect((1532.0, 8.0), 380.0, 1064.0, (1920.0, 1080.0), 0);
        assert!(
            bar.0 + bar.2 <= 1532.0 - 8.0 + 0.5,
            "{bar:?} left of the panel"
        );
        assert!(bar.1 >= 8.0 && bar.1 + bar.3 <= 1072.0);
        // Add's list: a card of its own beside the panel (on the screen's side of it), never over
        // the bar, its top with the panel's
        let bar = bar_rect((1532.0, 600.0), 380.0, 472.0, (1920.0, 1080.0), 0);
        let add = add_rect((1532.0, 600.0), 380.0, (1920.0, 1080.0), 3, bar);
        assert!(
            add.0 + add.2 <= 1532.0 - 8.0 + 0.5,
            "{add:?} left of the panel"
        );
        assert_eq!(add.1, 600.0);
        assert_eq!(
            add_rows(add, &[Module::Calendar, Module::Memory, Module::System], 0).len(),
            3
        );
        // a short screen: as many rows as fit, the rest by scrolling
        let short = add_rect(
            (1532.0, 8.0),
            380.0,
            (1920.0, 600.0),
            20,
            (0.0, 0.0, 0.0, 0.0),
        );
        assert!(short.1 + short.3 <= 600.0);
        let shown = add_rows_shown(short);
        assert!(shown > 0 && shown < 20);
        let all: Vec<Module> = crate::modules::edit::addable(&panel("clock"))
            .into_iter()
            .collect();
        let first = add_rows(short, &all, 0)[0].0;
        assert_ne!(add_rows(short, &all, 1)[0].0, first, "scrolled one row");
        // a panel as tall as the screen: its bar beside it, the list under the bar, never over it
        let tall_bar = bar_rect((1532.0, 8.0), 380.0, 1064.0, (1920.0, 1080.0), 0);
        let under = add_rect((1532.0, 8.0), 380.0, (1920.0, 1080.0), 3, tall_bar);
        assert!(!overlaps(under, tall_bar), "{under:?} {tall_bar:?}");
        // a panel at the left edge: the list on its right
        let left = add_rect((8.0, 8.0), 380.0, (1920.0, 1080.0), 3, (0.0, 0.0, 0.0, 0.0));
        assert!(left.0 >= 8.0 + 380.0 + 8.0 - 0.5, "{left:?}");
    }

    #[test]
    fn the_edge_button_goes_round_and_reset_needs_a_default() {
        use crate::config::Side::*;
        assert_eq!(next_side(Top), Right);
        assert_eq!(next_side(Left), Top);
        assert!(!has_default(&panel("clock")), "a panel of your own");
    }

    #[test]
    fn card_buttons_fit_a_half_card() {
        let p = panel("clock@half system@half volume");
        let (strip, b) = card_buttons(&p, 0, (0.0, 0.0, 185.0, 60.0), 380.0);
        assert!(b.iter().all(|(_, r)| r.0 >= strip.0 + 40.0));
        assert!(b.iter().any(|(k, _)| *k == CardButton::Remove));
    }
}
