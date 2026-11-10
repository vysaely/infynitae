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

const BAR_H: f64 = 36.0;
const ADD_ROW_H: f64 = 34.0;
const STRIP_H: f64 = 26.0;
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
    for (b, r) in blocks(p).into_iter().enumerate() {
        if p.items[r.start].module.is_switch() {
            let (_, card_h) = crate::modules::switches::layout_run(
                &p.items[r.clone()],
                world,
                &th.switch_look,
                pw,
                y,
            );
            if card_h > 0.0 {
                out.push((b, (0.0, y, pw, card_h)));
                y += card_h + GAP;
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

/// The block under `(px, py)` (panel coordinates).
pub fn block_at(brects: &[(usize, Rect)], px: f64, py: f64) -> Option<usize> {
    brects
        .iter()
        .find(|(_, r)| px >= r.0 && px <= r.0 + r.2 && py >= r.1 && py <= r.1 + r.3)
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
    let strip = (r.0 + 6.0, r.1 + 6.0, (r.2 - 12.0).max(0.0), STRIP_H);
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
    let left_limit = strip.0 + 40.0;
    for (k, w) in want {
        if x - w < left_limit {
            break;
        }
        x -= w;
        out.push((k, (x, strip.1 + 1.0, w, STRIP_H - 2.0)));
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
    Hover,
    Cancel,
    Done,
}

const BAR_BUTTONS: [(BarButton, f64); 8] = [
    (BarButton::Undo, 1.0),
    (BarButton::Reset, 1.0),
    (BarButton::Add, 0.9),
    (BarButton::Narrower, 0.55),
    (BarButton::Wider, 0.55),
    (BarButton::Hover, 1.15),
    (BarButton::Cancel, 1.1),
    (BarButton::Done, 1.0),
];

/// The bar's rectangle on the screen: beside the panel where there is room (above it, below
/// it, to its inner side), never over the screen's edge; taller while adding.
pub fn bar_rect(
    origin: (f64, f64),
    pw: f64,
    ph: f64,
    screen: (f64, f64),
    adding_rows: usize,
) -> Rect {
    let (ox, oy) = origin;
    let (sw, sh) = screen;
    let bw = pw.max(380.0).min(sw - 16.0);
    let bh = BAR_H
        + if adding_rows > 0 {
            6.0 + adding_rows as f64 * ADD_ROW_H
        } else {
            0.0
        };
    let bh = bh.min(sh - 16.0);
    let clamp_x = |x: f64| x.clamp(8.0, (sw - bw - 8.0).max(8.0));
    let clamp_y = |y: f64| y.clamp(8.0, (sh - bh - 8.0).max(8.0));
    // above, below
    if oy - bh - 8.0 >= 8.0 {
        return (clamp_x(ox + pw - bw), oy - bh - 8.0, bw, bh);
    }
    if oy + ph + 8.0 + bh <= sh - 8.0 {
        return (clamp_x(ox + pw - bw), oy + ph + 8.0, bw, bh);
    }
    // beside, on the side with room (the inner side of a panel at an edge)
    let y = clamp_y(oy);
    if ox - bw - 8.0 >= 8.0 {
        return (ox - bw - 8.0, y, bw, bh);
    }
    if ox + pw + 8.0 + bw <= sw - 8.0 {
        return (ox + pw + 8.0, y, bw, bh);
    }
    (clamp_x(ox), clamp_y(oy), bw, bh)
}

/// The bar's buttons with their rectangles on the screen.
pub fn bar_buttons(bar: Rect) -> Vec<(BarButton, Rect)> {
    let total: f64 = BAR_BUTTONS.iter().map(|(_, u)| u).sum();
    let inner = bar.2 - 10.0 - 4.0 * (BAR_BUTTONS.len() as f64 - 1.0);
    let mut x = bar.0 + 5.0;
    BAR_BUTTONS
        .iter()
        .map(|&(k, u)| {
            let w = inner * u / total;
            let r = (x, bar.1 + 5.0, w, BAR_H - 10.0);
            x += w + 4.0;
            (k, r)
        })
        .collect()
}

/// Add's rows under the bar: each module the panel can take, with its rectangle on the screen.
pub fn add_rows(bar: Rect, choices: &[Module]) -> Vec<(Module, Rect)> {
    choices
        .iter()
        .enumerate()
        .map(|(k, &m)| {
            (
                m,
                (
                    bar.0 + 5.0,
                    bar.1 + BAR_H + 3.0 + k as f64 * ADD_ROW_H,
                    bar.2 - 10.0,
                    ADD_ROW_H - 4.0,
                ),
            )
        })
        .filter(|(_, r)| r.1 + r.3 <= bar.1 + bar.3)
        .collect()
}

fn inside(r: Rect, x: f64, y: f64) -> bool {
    x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3
}

/// What a press at `(x, y)` on the screen hits in the bar.
pub fn bar_hit(bar: Rect, choices: &[Module], adding: bool, x: f64, y: f64) -> Option<BarHit> {
    if !inside(bar, x, y) {
        return None;
    }
    for (k, r) in bar_buttons(bar) {
        if inside(r, x, y) {
            return Some(BarHit::Button(k));
        }
    }
    if adding {
        for (m, r) in add_rows(bar, choices) {
            if inside(r, x, y) {
                return Some(BarHit::Add(m));
            }
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
        draw::set(cr, th.accent.alpha(0.22));
    } else {
        draw::set(cr, th.track.alpha(0.55));
    }
    let _ = cr.fill();
}

fn arrow(cr: &Context, r: Rect, up: bool, color: crate::theme::Rgba) {
    let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    let d = if up { -1.0 } else { 1.0 };
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
    pw: f64,
    state: &EditState,
) {
    let (ox, oy) = (origin.0, origin.1 - world.panel_scroll);
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

    // the card under the pointer: its bar
    if let (Some(b), false) = (state.hover_block, dragging) {
        if let Some((_, r)) = brects.iter().find(|(k, _)| *k == b) {
            let (strip, buttons) = card_buttons(panel, b, *r, pw);
            draw::round_rect(
                cr,
                ox + strip.0,
                oy + strip.1,
                strip.2,
                strip.3,
                th.small_radius(),
            );
            draw::set(cr, th.card.alpha(0.96));
            let _ = cr.fill();
            draw::round_rect(
                cr,
                ox + strip.0 + 0.5,
                oy + strip.1 + 0.5,
                strip.2 - 1.0,
                strip.3 - 1.0,
                th.small_radius(),
            );
            draw::set(cr, th.accent.alpha(0.55));
            cr.set_line_width(1.0);
            let _ = cr.stroke();
            // the grip and the name
            let first = &panel.items[bs[b].start];
            let name = if first.module.is_switch() {
                "Quick settings"
            } else {
                module_display_name(first.module)
            };
            draw::set(cr, th.muted);
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
            let name_right = buttons
                .last()
                .map(|(_, r)| r.0 - 6.0)
                .unwrap_or(strip.0 + strip.2 - 6.0);
            draw::text_max(
                cr,
                ox + strip.0 + 22.0,
                oy + strip.1 + (strip.3 - 13.0) / 2.0 - 1.0,
                (name_right - strip.0 - 22.0).max(10.0),
                &Text {
                    size: 12.0,
                    bold: true,
                    color: th.text,
                    text: name,
                },
            );
            let it = &panel.items[bs[b].start];
            for (k, r) in buttons {
                let r = (ox + r.0, oy + r.1, r.2, r.3);
                match k {
                    CardButton::Up | CardButton::Down => {
                        button_bg(cr, th, r, false, false);
                        arrow(cr, r, k == CardButton::Up, th.text);
                    }
                    CardButton::Half => {
                        button_bg(cr, th, r, it.half, false);
                        label(cr, r, if it.half { "Full" } else { "Half" }, th.text, false);
                    }
                    CardButton::Look => {
                        button_bg(cr, th, r, false, false);
                        label(cr, r, look_name(it).unwrap_or(""), th.text, false);
                    }
                    CardButton::Remove => {
                        button_bg(cr, th, r, false, false);
                        cross(cr, r, th.accent);
                    }
                }
            }
            // a run of switches: each switch's own x
            if first.module.is_switch() {
                for i in bs[b].clone() {
                    if let Some(&cell) = rects.get(i).filter(|c| c.2 > 0.0 && c.3 > 0.0) {
                        let x = switch_x(cell);
                        let x = (ox + x.0, oy + x.1, x.2, x.3);
                        draw::round_rect(cr, x.0, x.1, x.2, x.3, 9.0);
                        draw::set(cr, th.card.alpha(0.9));
                        let _ = cr.fill();
                        cross(cr, (x.0 - 2.0, x.1 - 2.0, x.2 + 4.0, x.3 + 4.0), th.muted);
                    }
                }
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
        draw::round_rect(cr, ox + 4.0, oy + y - 1.5, pw - 8.0, 3.0, 1.5);
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }

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
    for (k, r) in bar_buttons(bar) {
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
                true,
            ),
            BarButton::Add => ("Add", state.adding, false, true),
            BarButton::Narrower => ("-", false, false, true),
            BarButton::Wider => ("+", false, false, true),
            BarButton::Hover => (
                if panel.hover { "Hover on" } else { "Hover off" },
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
        let choices = crate::modules::edit::addable(panel);
        if choices.is_empty() {
            draw::text(
                cr,
                bar.0 + 12.0,
                bar.1 + BAR_H + 10.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: "The panel holds every module it can",
                },
            );
        }
        for (m, r) in add_rows(bar, &choices) {
            draw::round_rect(cr, r.0, r.1, r.2, r.3, th.small_radius());
            draw::set(cr, th.track.alpha(0.35));
            let _ = cr.fill();
            let name = Text {
                size: 13.0,
                bold: true,
                color: th.text,
                text: module_display_name(m),
            };
            let (nw, _) = draw::measure(cr, &name);
            draw::text(cr, r.0 + 10.0, r.1 + (r.3 - 15.0) / 2.0, &name);
            draw::text_max(
                cr,
                r.0 + 10.0 + nw + 10.0,
                r.1 + (r.3 - 13.0) / 2.0,
                (r.2 - nw - 30.0 - 40.0).max(10.0),
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: module_description(m),
                },
            );
            draw::text_right(
                cr,
                r.0 + r.2 - 10.0,
                r.1 + (r.3 - 13.0) / 2.0,
                &Text {
                    size: 12.0,
                    bold: true,
                    color: th.accent,
                    text: "Add",
                },
            );
        }
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
        assert_eq!(p.items.len(), 4, "once");
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
        let bar = bar_rect((1532.0, 600.0), 380.0, 472.0, (1920.0, 1080.0), 3);
        assert!(
            bar.1 + bar.3 <= 600.0 - 8.0 + 0.5,
            "{bar:?} above the panel"
        );
        assert_eq!(
            add_rows(bar, &[Module::Calendar, Module::Memory, Module::System]).len(),
            3
        );
    }

    #[test]
    fn card_buttons_fit_a_half_card() {
        let p = panel("clock@half system@half volume");
        let (strip, b) = card_buttons(&p, 0, (0.0, 0.0, 185.0, 60.0), 380.0);
        assert!(b.iter().all(|(_, r)| r.0 >= strip.0 + 40.0));
        assert!(b.iter().any(|(k, _)| *k == CardButton::Remove));
    }
}
