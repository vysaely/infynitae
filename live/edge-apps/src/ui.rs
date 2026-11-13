//! The shared parts every card draws with (docs/EDGE_APPS.md section 3.5), in the look of version 1 of
//! `docs/design/apps-edge/system-cards.html` ("rows and switches"): 40 px rows with a 16 px icon, the
//! name and its state; a 30 x 16 switch; More at a row's right while it is pointed at (Less while its
//! list is open); lists that open in place, under the row or over it in a panel at the bottom. Every
//! part registers its own hit, so hover and clicks come with it. Colours only from the theme.
//!
//! Rectangles are in the surface's pixels, the same as the card's `body`.

use crate::card::{Cx, Hits, Rect};
use crate::draw::{self, Text};
use crate::theme::Rgba;
use cairo::Context;
use std::f64::consts::PI;

pub const ROW_H: f64 = 40.0;
pub const LIST_ROW_H: f64 = 32.0;
/// How long a list takes to open or fold.
pub const LIST_MS: f64 = 220.0;
const ICON: f64 = 16.0;
const SWITCH_W: f64 = 30.0;
const SWITCH_H: f64 = 16.0;

/// The soft light under something pointed at (or the keyboard's selection).
pub fn light(cr: &Context, cx: &Cx, r: Rect) {
    draw::round_rect(cr, r.0, r.1, r.2, r.3, cx.theme.small_radius());
    draw::set(cr, cx.theme.text.alpha(0.08));
    let _ = cr.fill();
}

/// Line icons, 16 px, 1.6 px stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// a network port
    Wired,
    /// joined: 1 to 3 arcs bright
    Wifi(u8),
    /// Wi-Fi on, no network: all faint
    WifiSearching,
    /// off: faint, crossed out
    WifiOff,
    Bluetooth,
    BluetoothOff,
    /// a gauge
    Power,
    /// a padlock (a secured network)
    Lock,
    /// a bell (do not disturb: crossed out while on)
    Bell(bool),
    /// a crescent moon (night light)
    Moon,
    /// a keyboard (the layout)
    Keyboard,
    /// a keyboard on a screen (the screen keyboard)
    ScreenKeys,
    /// a ring with a dot (replay: the recorder keeping the last minutes)
    Record,
}

pub fn icon(cr: &Context, ic: Icon, x: f64, y: f64, color: Rgba) {
    // text drawn before leaves a current point: an arc would join it with a line
    cr.new_path();
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    let (cx, cy) = (x + 8.0, y + 8.0);
    let faint = color.alpha(color.3 * 0.3);
    draw::set(cr, color);
    match ic {
        Icon::Wired => {
            cr.move_to(cx - 6.0, cy - 4.5);
            cr.line_to(cx + 6.0, cy - 4.5);
            cr.line_to(cx + 6.0, cy + 2.5);
            cr.line_to(cx + 3.0, cy + 2.5);
            cr.line_to(cx + 3.0, cy + 5.5);
            cr.line_to(cx - 3.0, cy + 5.5);
            cr.line_to(cx - 3.0, cy + 2.5);
            cr.line_to(cx - 6.0, cy + 2.5);
            cr.close_path();
            let _ = cr.stroke();
            for dx in [-2.5, 0.0, 2.5] {
                cr.move_to(cx + dx, cy - 4.5);
                cr.line_to(cx + dx, cy - 2.0);
            }
            let _ = cr.stroke();
        }
        Icon::Wifi(_) | Icon::WifiSearching | Icon::WifiOff => {
            let bright = match ic {
                Icon::Wifi(n) => n.min(3) as usize,
                _ => 0,
            };
            for (i, r) in [7.0, 4.6, 2.2].iter().enumerate() {
                draw::set(cr, if 3 - i <= bright { color } else { faint });
                cr.new_sub_path();
                cr.arc(cx, cy + 4.5, *r, -0.78 * PI, -0.22 * PI);
                let _ = cr.stroke();
            }
            draw::set(cr, if bright > 0 { color } else { faint });
            cr.new_sub_path();
            cr.arc(cx, cy + 5.0, 1.1, 0.0, 2.0 * PI);
            let _ = cr.fill();
            if ic == Icon::WifiOff {
                draw::set(cr, color);
                cr.move_to(cx - 6.0, cy - 5.0);
                cr.line_to(cx + 6.0, cy + 6.0);
                let _ = cr.stroke();
            }
        }
        Icon::Bluetooth | Icon::BluetoothOff => {
            if ic == Icon::BluetoothOff {
                draw::set(cr, faint);
            }
            cr.move_to(cx - 3.0, cy - 3.0);
            cr.line_to(cx + 3.0, cy + 3.0);
            cr.line_to(cx, cy + 6.0);
            cr.line_to(cx, cy - 6.0);
            cr.line_to(cx + 3.0, cy - 3.0);
            cr.line_to(cx - 3.0, cy + 3.0);
            let _ = cr.stroke();
            if ic == Icon::BluetoothOff {
                draw::set(cr, color);
                cr.move_to(cx - 6.0, cy - 5.0);
                cr.line_to(cx + 6.0, cy + 6.0);
                let _ = cr.stroke();
            }
        }
        Icon::Power => {
            cr.new_sub_path();
            cr.arc(cx, cy + 2.0, 6.0, PI, 2.0 * PI);
            let _ = cr.stroke();
            cr.move_to(cx, cy + 2.0);
            cr.line_to(cx + 3.5, cy - 2.0);
            let _ = cr.stroke();
        }
        Icon::Lock => {
            draw::round_rect(cr, cx - 4.0, cy - 1.0, 8.0, 6.0, 1.0);
            let _ = cr.stroke();
            cr.new_sub_path();
            cr.arc(cx, cy - 1.0, 2.4, PI, 2.0 * PI);
            let _ = cr.stroke();
        }
        Icon::Bell(crossed) => {
            cr.move_to(cx - 5.0, cy + 3.5);
            cr.line_to(cx - 4.0, cy + 1.5);
            cr.line_to(cx - 4.0, cy - 1.5);
            cr.arc(cx, cy - 1.5, 4.0, PI, 2.0 * PI);
            cr.line_to(cx + 4.0, cy + 1.5);
            cr.line_to(cx + 5.0, cy + 3.5);
            cr.close_path();
            let _ = cr.stroke();
            cr.move_to(cx - 1.5, cy + 6.0);
            cr.line_to(cx + 1.5, cy + 6.0);
            let _ = cr.stroke();
            if crossed {
                cr.move_to(cx - 6.5, cy - 6.5);
                cr.line_to(cx + 6.5, cy + 6.5);
                let _ = cr.stroke();
            }
        }
        Icon::Moon => {
            cr.arc(cx, cy, 6.0, 0.35 * PI, 1.65 * PI);
            cr.arc_negative(cx + 3.0, cy - 1.2, 4.6, 1.38 * PI, 0.62 * PI);
            cr.close_path();
            let _ = cr.stroke();
        }
        Icon::Keyboard | Icon::ScreenKeys => {
            let (top, h) = if ic == Icon::ScreenKeys {
                (cy - 6.5, 10.0)
            } else {
                (cy - 4.5, 9.0)
            };
            draw::round_rect(cr, cx - 7.0, top, 14.0, h, 1.5);
            let _ = cr.stroke();
            for i in 0..4 {
                let kx = cx - 4.5 + i as f64 * 3.0;
                cr.move_to(kx, top + 3.2);
                cr.line_to(kx + 0.01, top + 3.2);
            }
            cr.move_to(cx - 3.0, top + h - 2.8);
            cr.line_to(cx + 3.0, top + h - 2.8);
            let _ = cr.stroke();
            if ic == Icon::ScreenKeys {
                cr.move_to(cx - 3.0, cy + 6.5);
                cr.line_to(cx + 3.0, cy + 6.5);
                let _ = cr.stroke();
            }
        }
        Icon::Record => {
            cr.arc(cx, cy, 6.0, 0.0, 2.0 * PI);
            let _ = cr.stroke();
            cr.arc(cx, cy, 2.4, 0.0, 2.0 * PI);
            let _ = cr.fill();
        }
    }
}

/// A row's content.
pub struct Row<'a> {
    pub icon: Option<Icon>,
    pub name: &'a str,
    /// the second line under the name
    pub state: &'a str,
    /// on: the icon and the state in the accent
    pub on: bool,
    /// room kept free at the right (for a switch, More)
    pub right: f64,
}

/// A row: icon, name, state under it. Returns whether it is lit (pointed at or selected).
pub fn row(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, r: Rect, row: &Row) -> bool {
    let lit = hits.add(id, r) || hits.selected(id);
    if lit {
        light(cr, cx, r);
    }
    let th = cx.theme;
    let mut tx = r.0 + 8.0;
    if let Some(ic) = row.icon {
        icon(
            cr,
            ic,
            tx,
            r.1 + (r.3 - ICON) / 2.0,
            if row.on { th.accent_text } else { th.text },
        );
        tx += ICON + 10.0;
    }
    let w = (r.0 + r.2 - row.right - 8.0 - tx).max(20.0);
    let two = !row.state.is_empty();
    let ty = if two {
        r.1 + (r.3 - 32.0) / 2.0
    } else {
        r.1 + (r.3 - 17.0) / 2.0
    };
    draw::text_max(
        cr,
        tx,
        ty,
        w,
        &Text {
            size: 13.0,
            bold: false,
            color: th.text,
            text: row.name,
        },
    );
    if two {
        draw::text_max(
            cr,
            tx,
            ty + 17.0,
            w,
            &Text {
                size: 11.0,
                bold: false,
                color: if row.on { th.accent_text } else { th.muted },
                text: row.state,
            },
        );
    }
    lit
}

/// The switch at a row's right end, `x_right` its right edge. On: the accent.
pub fn switch(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, row: Rect, x_right: f64, on: bool) {
    let th = cx.theme;
    let x = x_right - SWITCH_W;
    let y = row.1 + (row.3 - SWITCH_H) / 2.0;
    let r = (x - 4.0, y - 4.0, SWITCH_W + 8.0, SWITCH_H + 8.0);
    if hits.add(id, r) || hits.selected(id) {
        draw::round_rect(
            cr,
            r.0,
            r.1,
            r.2,
            r.3,
            (r.3 / 2.0).min(th.small_radius() + 4.0),
        );
        draw::set(cr, th.text.alpha(0.08));
        let _ = cr.fill();
    }
    draw::round_rect(cr, x, y, SWITCH_W, SWITCH_H, SWITCH_H / 2.0);
    draw::set(cr, if on { th.accent } else { th.track });
    let _ = cr.fill();
    let kx = if on { x + SWITCH_W - 8.0 } else { x + 8.0 };
    cr.new_sub_path();
    cr.arc(kx, y + SWITCH_H / 2.0, 6.0, 0.0, 2.0 * PI);
    draw::set(cr, if on { th.card.alpha(1.0) } else { th.text });
    let _ = cr.fill();
}

/// The width "More" (or "Less") takes, for a row's `right`.
pub const MORE_W: f64 = 46.0;

#[allow(clippy::too_many_arguments)]
/// "More" at a row's right while the row is lit or selected, "Less" while its list is open; `x_right`
/// its right edge. Drawn only then, but always a hit (a click there opens the list).
pub fn more(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    id: u32,
    row: Rect,
    x_right: f64,
    row_lit: bool,
    open: bool,
) {
    let th = cx.theme;
    let r = (x_right - MORE_W, row.1 + (row.3 - 24.0) / 2.0, MORE_W, 24.0);
    let lit = hits.add(id, r) || hits.selected(id);
    if !(row_lit || open || lit) {
        return;
    }
    if lit {
        light(cr, cx, r);
    }
    draw::text_center(
        cr,
        r.0 + r.2 / 2.0,
        r.1 + 4.0,
        &Text {
            size: 12.0,
            bold: false,
            color: if lit || open { th.text } else { th.muted },
            text: if open { "Less" } else { "More" },
        },
    );
}

/// A text button with an outline ("Search again", "Remove"). Returns its width.
pub fn button(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, x: f64, y: f64, word: &str) -> f64 {
    let th = cx.theme;
    let t = Text {
        size: 12.0,
        bold: false,
        color: th.text,
        text: word,
    };
    let w = draw::measure(cr, &t).0 + 18.0;
    let r = (x, y, w, 26.0);
    if hits.add(id, r) || hits.selected(id) {
        light(cr, cx, r);
    }
    draw::round_rect(cr, x + 0.5, y + 0.5, w - 1.0, 25.0, th.small_radius());
    draw::set(cr, th.edge);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
    draw::text_center(cr, x + w / 2.0, y + 5.0, &t);
    w
}

/// The value of a row that opens a list (owner's choice, 2026-11-13: today's quiet value inside
/// the thin box of a drop-down): the word at the right in a faint outline with a small chevron.
/// `lit` (the row pointed at) brightens the outline; `open` (its list shown) draws it in the accent
/// with the chevron turned up; `changed` (not the default) puts the word in the accent. Returns the
/// box's width, to keep the row's name clear of it.
#[allow(clippy::too_many_arguments)]
pub fn value_box(
    cr: &Context,
    cx: &Cx,
    row: Rect,
    x_right: f64,
    word: &str,
    changed: bool,
    lit: bool,
    open: bool,
) -> f64 {
    let th = cx.theme;
    let t = Text {
        size: 12.0,
        bold: false,
        color: if changed { th.accent_text } else { th.muted },
        text: word,
    };
    let tw = draw::measure(cr, &t).0;
    let (h, pad, chev) = (24.0, 8.0, 10.0);
    let w = (tw + pad * 2.0 + chev + 4.0).min(row.2 * 0.6);
    let (x, y) = (x_right - w, row.1 + (row.3 - h) / 2.0);
    draw::round_rect(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, th.small_radius());
    draw::set(
        cr,
        if open {
            th.accent
        } else if lit {
            th.text.alpha(0.28)
        } else {
            th.text.alpha(0.12)
        },
    );
    cr.set_line_width(1.0);
    let _ = cr.stroke();
    draw::text_max(
        cr,
        x + pad,
        y + (h - 15.0) / 2.0,
        w - pad * 2.0 - chev - 4.0,
        &t,
    );
    // the chevron: down while closed, up while open
    let (cxp, cyp) = (x + w - pad - chev / 2.0, y + h / 2.0);
    let d = if open { -1.0 } else { 1.0 };
    cr.move_to(cxp - 3.5, cyp - 1.75 * d);
    cr.line_to(cxp, cyp + 1.75 * d);
    cr.line_to(cxp + 3.5, cyp - 1.75 * d);
    draw::set(cr, if open { th.accent } else { th.muted });
    cr.set_line_width(1.4);
    let _ = cr.stroke();
    w
}

/// A choice of a few words in one line (power mode); each word is hit `first_id + i`, the chosen
/// one in the accent.
#[allow(clippy::too_many_arguments)]
pub fn choice(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    first_id: u32,
    r: Rect,
    words: &[&str],
    chosen: usize,
) {
    let th = cx.theme;
    if words.is_empty() {
        return;
    }
    let gap = 6.0;
    let w = (r.2 - gap * (words.len() as f64 - 1.0)) / words.len() as f64;
    for (i, word) in words.iter().enumerate() {
        let b = (r.0 + i as f64 * (w + gap), r.1, w, r.3);
        let id = first_id + i as u32;
        let lit = hits.add(id, b) || hits.selected(id);
        let on = i == chosen;
        draw::round_rect(
            cr,
            b.0 + 0.5,
            b.1 + 0.5,
            b.2 - 1.0,
            b.3 - 1.0,
            th.small_radius(),
        );
        if on {
            draw::set(cr, th.accent.alpha(0.18));
            let _ = cr.fill_preserve();
        } else if lit {
            draw::set(cr, th.text.alpha(0.08));
            let _ = cr.fill_preserve();
        }
        draw::set(cr, if on { th.accent } else { th.edge });
        cr.set_line_width(1.0);
        let _ = cr.stroke();
        draw::text_center(
            cr,
            b.0 + b.2 / 2.0,
            b.1 + (b.3 - 16.0) / 2.0,
            &Text {
                size: 12.0,
                bold: on,
                color: if on { th.accent_text } else { th.text },
                text: word,
            },
        );
    }
}

/// A slider; thicker while pointed at. `part` 0 to 1.
pub fn slider(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, r: Rect, part: f64) {
    let th = cx.theme;
    let lit = hits.add(id, r) || hits.selected(id);
    let (bar, knob) = if lit { (6.0, 7.0) } else { (3.0, 6.0) };
    let mid = r.1 + r.3 / 2.0;
    let p = part.clamp(0.0, 1.0);
    draw::round_rect(cr, r.0, mid - bar / 2.0, r.2, bar, bar / 2.0);
    draw::set(cr, th.track);
    let _ = cr.fill();
    if p > 0.0 {
        draw::round_rect(cr, r.0, mid - bar / 2.0, (r.2 * p).max(bar), bar, bar / 2.0);
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }
    cr.new_sub_path();
    cr.arc(r.0 + r.2 * p, mid, knob, 0.0, 2.0 * PI);
    draw::set(cr, th.text);
    let _ = cr.fill();
}

/// The value under the pointer on a slider at `r`, 0 to 1.
pub fn slider_part(r: Rect, at: (f64, f64)) -> f64 {
    if r.2 <= 0.0 {
        return 0.0;
    }
    ((at.0 - r.0) / r.2).clamp(0.0, 1.0)
}

/// Eased out (cubic): fast at first, settling softly.
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// A list that opens in place under (or over) a row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct List {
    pub open: bool,
    /// how far open, 0 to 1
    reveal: f64,
}

impl List {
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }
    /// Shown fully open or closed at once (a still picture, a test).
    pub fn settle(&mut self) {
        self.reveal = if self.open { 1.0 } else { 0.0 };
    }
    /// Moves it on by `dt_ms` (LIST_MS to open or fold). True while moving.
    pub fn tick(&mut self, dt_ms: f64) -> bool {
        let target = if self.open { 1.0 } else { 0.0 };
        if self.reveal == target {
            return false;
        }
        let step = dt_ms / LIST_MS;
        self.reveal = if target > self.reveal {
            (self.reveal + step).min(1.0)
        } else {
            (self.reveal - step).max(0.0)
        };
        true
    }
    /// Its height now for `rows` rows: add it to `Card::height`.
    pub fn height(&self, rows: usize) -> f64 {
        if self.reveal <= 0.0 {
            return 0.0;
        }
        (rows.max(1) as f64 * LIST_ROW_H + 8.0) * ease(self.reveal)
    }
    /// The row and the list inside `r` (the row's place with the list's height added): the list
    /// under the row, or over it when `above`, so the row stays under the pointer in a panel at the
    /// bottom of the screen.
    pub fn split(&self, r: Rect, rows: usize, above: bool) -> (Rect, Rect) {
        let lh = self.height(rows);
        if above {
            ((r.0, r.1 + lh, r.2, r.3 - lh), (r.0, r.1, r.2, lh))
        } else {
            ((r.0, r.1, r.2, r.3 - lh), (r.0, r.1 + r.3 - lh, r.2, lh))
        }
    }
    pub fn shown(&self) -> bool {
        self.reveal > 0.0
    }
}

/// The soft box a list sits in.
pub fn list_box(cr: &Context, cx: &Cx, r: Rect) {
    if r.3 <= 1.0 {
        return;
    }
    draw::round_rect(cr, r.0, r.1 + 2.0, r.2, r.3 - 4.0, cx.theme.small_radius());
    draw::set(cr, cx.theme.text.alpha(0.04));
    let _ = cr.fill();
}

/// One line of a list: its text, a quieter note at the right, an accent mark when it is the active
/// one. Returns whether it is lit.
#[allow(clippy::too_many_arguments)]
pub fn list_row(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    id: u32,
    r: Rect,
    text: &str,
    note: &str,
    active: bool,
    icon_at_right: Option<Icon>,
) -> bool {
    let th = cx.theme;
    let lit = hits.add(id, r) || hits.selected(id);
    if active {
        draw::round_rect(cr, r.0, r.1, r.2, r.3, th.small_radius());
        draw::set(cr, th.accent.alpha(0.16));
        let _ = cr.fill();
    } else if lit {
        light(cr, cx, r);
    }
    let note_w = if note.is_empty() {
        0.0
    } else {
        draw::text_right(
            cr,
            r.0 + r.2 - 10.0,
            r.1 + (r.3 - 15.0) / 2.0,
            &Text {
                size: 11.5,
                bold: false,
                color: th.muted,
                text: note,
            },
        ) + 8.0
    };
    let icon_w = if let Some(ic) = icon_at_right {
        icon(
            cr,
            ic,
            r.0 + r.2 - 10.0 - note_w - ICON,
            r.1 + (r.3 - ICON) / 2.0,
            th.muted,
        );
        ICON + 6.0
    } else {
        0.0
    };
    draw::text_max(
        cr,
        r.0 + 10.0,
        r.1 + (r.3 - 17.0) / 2.0,
        (r.2 - 20.0 - note_w - icon_w).max(20.0),
        &Text {
            size: 13.0,
            bold: active,
            color: if active { th.accent_text } else { th.text },
            text,
        },
    );
    lit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_opens_folds_and_splits_over_or_under_its_row() {
        let mut l = List::default();
        assert_eq!(l.height(3), 0.0);
        l.toggle();
        assert!(l.tick(110.0));
        assert!(l.height(3) > 0.0 && l.height(3) < 3.0 * LIST_ROW_H + 8.0);
        while l.tick(16.0) {}
        assert_eq!(l.height(3), 3.0 * LIST_ROW_H + 8.0);
        let full = (0.0, 0.0, 300.0, ROW_H + l.height(3));
        let (row, list) = l.split(full, 3, false);
        assert_eq!((row.1, row.3, list.1), (0.0, ROW_H, ROW_H));
        let (row, list) = l.split(full, 3, true);
        assert_eq!(
            (list.1, row.1, row.3),
            (0.0, l.height(3), ROW_H),
            "over the row: the row stays at the bottom"
        );
        l.toggle();
        while l.tick(16.0) {}
        assert!(!l.shown());
    }

    #[test]
    fn a_slider_gives_the_value_under_the_pointer() {
        let r = (100.0, 0.0, 200.0, 20.0);
        assert_eq!(slider_part(r, (200.0, 5.0)), 0.5);
        assert_eq!(slider_part(r, (50.0, 5.0)), 0.0);
        assert_eq!(slider_part(r, (999.0, 5.0)), 1.0);
    }
}
