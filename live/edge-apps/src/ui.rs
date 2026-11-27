//! The shared parts every card draws with (`docs/EDGE_APPS.md` section 3.5), in the style of
//! `docs/design/apps-edge/system-cards.html`: 40 px rows with a 16 px icon, the name and its
//! state; a 30 x 16 switch; More at a row's right while pointed at (Less while its list is open);
//! lists that open in place, under the row or over it in a panel at the bottom. Every part
//! registers its own hit, so hover and clicks come with it. Colours only from the theme.
//!
//! Rectangles are in the surface's pixels, the same as the card's `body`.

use crate::card::{Cx, Hits, Rect};
use crate::draw::{self, Text};
use crate::provider::Size;
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::f64::consts::PI;
use std::time::{Duration, Instant};

pub const ROW_H: f64 = 40.0;
pub const LIST_ROW_H: f64 = 32.0;
pub const ERROR_H: f64 = 20.0;
pub const ERROR_TTL: Duration = Duration::from_secs(10);
/// How long a list takes to open or fold.
pub const LIST_MS: f64 = 175.0;
const ICON: f64 = 16.0;
const SWITCH_W: f64 = 30.0;
const SWITCH_H: f64 = 16.0;
pub const LIGHT_ALPHA: f64 = 0.08;
pub const CHOSEN_ALPHA: f64 = 0.16;
pub const CHOSEN_HOVER_ALPHA: f64 = 0.22;
pub const BOX_ALPHA: f64 = 0.055;

/// The soft light under something pointed at (or the keyboard's selection).
pub fn light(cr: &Context, cx: &Cx, r: Rect) {
    draw::round_rect(cr, r.0, r.1, r.2, r.3, cx.theme.small_radius());
    draw::set(cr, cx.theme.text.alpha(LIGHT_ALPHA));
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
    /// a check mark (a menu item that is on)
    Check,
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
        Icon::Check => {
            cr.move_to(cx - 5.0, cy + 0.5);
            cr.line_to(cx - 1.5, cy + 4.0);
            cr.line_to(cx + 5.0, cy - 3.5);
            let _ = cr.stroke();
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
    row_alert(cr, cx, hits, id, r, row, false)
}

/// Same as `row`, drawing the state line (and icon, if any) in `th.alert` when `alert` is true.
pub fn row_alert(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    id: u32,
    r: Rect,
    row: &Row,
    alert: bool,
) -> bool {
    let lit = hits.add(id, r) || hits.selected(id);
    if lit {
        light(cr, cx, r);
    }
    let th = cx.theme;
    let mut tx = r.0 + 8.0;
    if let Some(ic) = row.icon {
        let icon_color = if alert {
            th.alert
        } else if row.on {
            th.accent_text
        } else {
            th.text
        };
        icon(cr, ic, tx, r.1 + (r.3 - ICON) / 2.0, icon_color);
        tx += ICON + 10.0;
    }
    let w = (r.0 + r.2 - row.right - 8.0 - tx)
        .max(20.0)
        .min((r.0 + r.2 - 8.0 - tx).max(0.0));
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
            size: draw::SIZE_BODY,
            bold: false,
            color: th.text,
            text: row.name,
        },
    );
    if two {
        let state_color = if alert {
            th.alert
        } else if row.on {
            th.accent_text
        } else {
            th.muted
        };
        draw::text_max(
            cr,
            tx,
            ty + 17.0,
            w,
            &Text {
                size: draw::SIZE_NOTE,
                bold: false,
                color: state_color,
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
        draw::set(cr, th.text.alpha(LIGHT_ALPHA));
        let _ = cr.fill();
    }
    draw::round_rect(cr, x, y, SWITCH_W, SWITCH_H, SWITCH_H / 2.0);
    draw::set(cr, if on { th.accent } else { th.track });
    let _ = cr.fill();
    let kx = if on { x + SWITCH_W - 8.0 } else { x + 8.0 };
    cr.new_sub_path();
    cr.arc(kx, y + SWITCH_H / 2.0, 5.5, 0.0, 2.0 * PI);
    draw::set(cr, if on { th.on_accent } else { th.text });
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
    let more_t = Text {
        size: draw::SIZE_SMALL,
        bold: false,
        color: if lit || open { th.text } else { th.muted },
        text: if open { "Less" } else { "More" },
    };
    let (_, th_px) = draw::measure(cr, &more_t);
    draw::text_center(cr, r.0 + r.2 / 2.0, r.1 + (r.3 - th_px) / 2.0, &more_t);
}

/// Maximum width of a `button` so a long label fits inside a narrow (half) card.
pub const BUTTON_MAX_W: f64 = 140.0;

/// A text button on a soft fill ("Search again", "Remove"). Returns its width.
pub fn button(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, x: f64, y: f64, word: &str) -> f64 {
    let th = cx.theme;
    let t = Text {
        size: draw::SIZE_SMALL,
        bold: false,
        color: th.text,
        text: word,
    };
    let (tw, th_px) = draw::measure(cr, &t);
    let w = (tw + 18.0).min(BUTTON_MAX_W);
    let r = (x, y, w, 26.0);
    let lit = hits.add(id, r) || hits.selected(id);
    draw::round_rect(cr, x, y, w, 26.0, th.small_radius());
    draw::set(
        cr,
        th.text.alpha(if lit { CHOSEN_ALPHA } else { LIGHT_ALPHA }),
    );
    let _ = cr.fill();
    let ty = y + (26.0 - th_px) / 2.0;
    let max_tw = (w - 18.0).max(0.0);
    if tw <= max_tw {
        draw::text_center(cr, x + w / 2.0, ty, &t);
    } else {
        draw::text_max(cr, x + 9.0, ty, max_tw, &t);
    }
    w
}

/// The value of a row that opens a list: a small copy of the list's chosen option, shown in a
/// soft accent block with a chevron, so pressing it reads as that option opening into the list.
/// While the list is open only the muted word and upward chevron are drawn. `lit` deepens the
/// block. Returns the block's width, to keep the row's name clear of it.
pub fn value_box(
    cr: &Context,
    cx: &Cx,
    row: Rect,
    x_right: f64,
    word: &str,
    lit: bool,
    open: bool,
) -> f64 {
    let th = cx.theme;
    let t = Text {
        size: draw::SIZE_SMALL,
        bold: false,
        color: if open { th.muted } else { th.accent_text },
        text: word,
    };
    let (tw, th_px) = draw::measure(cr, &t);
    let (h, pad, chev) = (26.0, 10.0, 9.0);
    let w = (tw + pad * 2.0 + chev + 4.0).min(row.2 * 0.6);
    let (x, y) = (x_right - w, row.1 + (row.3 - h) / 2.0);
    if !open {
        draw::round_rect(cr, x, y, w, h, th.small_radius());
        draw::set(
            cr,
            th.accent.alpha(if lit {
                CHOSEN_HOVER_ALPHA
            } else {
                CHOSEN_ALPHA
            }),
        );
        let _ = cr.fill();
    }
    draw::text_max(
        cr,
        x + pad,
        y + (h - th_px) / 2.0,
        w - pad * 2.0 - chev - 4.0,
        &t,
    );
    // the chevron: down while closed, up while open
    let (cxp, cyp) = (x + w - pad - chev / 2.0, y + h / 2.0);
    let d = if open { -1.0 } else { 1.0 };
    cr.new_path();
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(cxp - 3.0, cyp - 1.5 * d);
    cr.line_to(cxp, cyp + 1.5 * d);
    cr.line_to(cxp + 3.0, cyp - 1.5 * d);
    draw::set(cr, if open { th.muted } else { th.accent_text });
    cr.set_line_width(1.4);
    let _ = cr.stroke();
    w
}

/// One option pill in a horizontal choice strip, drawn without an outline stroke.
pub fn choice_button(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    id: u32,
    r: Rect,
    word: &str,
    on: bool,
) {
    let th = cx.theme;
    let lit = hits.add(id, r) || hits.selected(id);
    draw::round_rect(cr, r.0, r.1, r.2, r.3, th.small_radius());
    draw::set(
        cr,
        if on {
            th.accent.alpha(if lit {
                CHOSEN_HOVER_ALPHA
            } else {
                CHOSEN_ALPHA
            })
        } else {
            th.text.alpha(if lit { CHOSEN_ALPHA } else { LIGHT_ALPHA })
        },
    );
    let _ = cr.fill();
    let t = Text {
        size: draw::SIZE_SMALL,
        bold: on,
        color: if on { th.accent_text } else { th.text },
        text: word,
    };
    let (tw, th_px) = draw::measure(cr, &t);
    let ty = r.1 + (r.3 - th_px) / 2.0;
    let max_tw = (r.2 - 12.0).max(0.0);
    if tw <= max_tw {
        draw::text_center(cr, r.0 + r.2 / 2.0, ty, &t);
    } else {
        draw::text_max(cr, r.0 + 6.0, ty, max_tw, &t);
    }
}

/// A choice of a few words in one line (power mode, picker tabs); each word is hit `first_id + i`,
/// the chosen one in the accent.
pub fn choice(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    first_id: u32,
    r: Rect,
    words: &[&str],
    chosen: usize,
) {
    if words.is_empty() {
        return;
    }
    let gap = 6.0;
    let w = (r.2 - gap * (words.len() as f64 - 1.0)) / words.len() as f64;
    for (i, word) in words.iter().enumerate() {
        let b = (r.0 + i as f64 * (w + gap), r.1, w, r.3);
        choice_button(cr, cx, hits, first_id + i as u32, b, word, i == chosen);
    }
}

/// A slider; thicker while pointed at. `part` 0 to 1.
pub fn slider(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, r: Rect, part: f64) {
    let th = cx.theme;
    let lit = hits.add(id, r) || hits.selected(id);
    hits.mark_draggable(id);
    let (bar, knob) = if lit { (6.0, 7.0) } else { (4.0, 6.0) };
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
    let kx = r.0 + r.2 * p;
    // crisp card-coloured collar separates the knob from the filled accent track
    cr.new_sub_path();
    cr.arc(kx, mid, knob + 1.5, 0.0, 2.0 * PI);
    draw::set(cr, th.card.alpha(1.0));
    let _ = cr.fill();
    cr.new_sub_path();
    cr.arc(kx, mid, knob, 0.0, 2.0 * PI);
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

/// Quartic ease-out: quick start, crisp settle with no sluggish tail.
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(4)
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
        self.height_for(rows.max(1) as f64 * LIST_ROW_H + 8.0)
    }
    /// Its animated height now for a custom content height `full_h`.
    pub fn height_for(&self, full_h: f64) -> f64 {
        if self.reveal <= 0.0 {
            return 0.0;
        }
        full_h * ease(self.reveal)
    }
    /// The row and the list inside `r` (the row's place with the list's height added): the list
    /// under the row, or over it when `above`, so the row stays under the pointer in a panel at the
    /// bottom of the screen.
    pub fn split(&self, r: Rect, rows: usize, above: bool) -> (Rect, Rect) {
        self.split_for(r, rows.max(1) as f64 * LIST_ROW_H + 8.0, above)
    }
    /// Same as `split`, for a custom list body height `full_h`.
    pub fn split_for(&self, r: Rect, full_h: f64, above: bool) -> (Rect, Rect) {
        let lh = self.height_for(full_h);
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

/// Transient command error state shared across cards that show a plain alert line under their row.
#[derive(Default)]
pub struct CardError {
    pub message: Option<String>,
    pub until: Option<Instant>,
}

impl CardError {
    pub fn set(&mut self, msg: String, now: Instant) -> bool {
        let next = Some(msg);
        let changed = self.message != next;
        self.message = next;
        self.until = Some(now + ERROR_TTL);
        changed
    }

    pub fn clear(&mut self) -> bool {
        let changed = self.message.is_some();
        self.message = None;
        self.until = None;
        changed
    }

    pub fn expire(&mut self, now: Instant) -> bool {
        if self.until.is_some_and(|u| now >= u) {
            return self.clear();
        }
        false
    }

    pub fn height(&self) -> f64 {
        if self.message.is_some() {
            ERROR_H
        } else {
            0.0
        }
    }
}

/// Formats `prefix` with `stderr_line` when non-empty.
pub fn command_error(prefix: &str, stderr_line: &str) -> String {
    if stderr_line.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}: {stderr_line}")
    }
}

/// Splits `body` into `(row_rect, error_rect, list_rect)` so any error sits directly under the row.
pub fn split_row_error_list(
    list: &List,
    body: Rect,
    has_list: bool,
    size: Size,
    rows: usize,
    list_above: bool,
    has_error: bool,
) -> (Rect, Option<Rect>, Rect) {
    let err_h = if has_error { ERROR_H } else { 0.0 };
    if !has_list {
        let row_r = (body.0, body.1, body.2, ROW_H);
        let err_r = has_error.then_some((body.0, body.1 + ROW_H, body.2, err_h));
        return (row_r, err_r, (0.0, 0.0, 0.0, 0.0));
    }
    if size == Size::M {
        let row_r = (body.0, body.1, body.2, ROW_H);
        let err_r = has_error.then_some((body.0, body.1 + ROW_H, body.2, err_h));
        let list_r = (
            body.0,
            body.1 + ROW_H + err_h,
            body.2,
            (body.3 - ROW_H - err_h).max(0.0),
        );
        return (row_r, err_r, list_r);
    }
    let avail = (body.0, body.1, body.2, (body.3 - err_h).max(ROW_H));
    let (row_r, mut list_r) = list.split(avail, rows, list_above);
    let err_r = has_error.then_some((row_r.0, row_r.1 + ROW_H, row_r.2, err_h));
    if !list_above && list_r.3 > 0.0 {
        list_r.1 += err_h;
    }
    (row_r, err_r, list_r)
}

/// Draws a plain one-line error under a card's row using the theme's alert colour.
pub fn error_line(cr: &Context, cx: &Cx, r: Rect, msg: &str) {
    draw::text_max(
        cr,
        r.0 + 8.0,
        r.1 + 2.0,
        (r.2 - 16.0).max(0.0),
        &Text {
            size: draw::SIZE_NOTE,
            bold: false,
            color: cx.theme.alert,
            text: msg,
        },
    );
}

/// The soft box a list sits in.
pub fn list_box(cr: &Context, cx: &Cx, r: Rect) {
    if r.3 <= 1.0 {
        return;
    }
    draw::round_rect(cr, r.0, r.1 + 2.0, r.2, r.3 - 4.0, cx.theme.small_radius());
    draw::set(cr, cx.theme.text.alpha(BOX_ALPHA));
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
        draw::set(
            cr,
            th.accent.alpha(if lit {
                CHOSEN_HOVER_ALPHA
            } else {
                CHOSEN_ALPHA
            }),
        );
        let _ = cr.fill();
    } else if lit {
        light(cr, cx, r);
    }
    let icon_w = if icon_at_right.is_some() {
        ICON + 6.0
    } else {
        0.0
    };
    let note_w = if note.is_empty() {
        0.0
    } else {
        let nt = Text {
            size: draw::SIZE_NOTE,
            bold: false,
            color: th.muted,
            text: note,
        };
        let max_nw = ((r.2 - 20.0 - icon_w) * 0.45).max(0.0);
        let (measured_nw, nh) = draw::measure(cr, &nt);
        let nw = measured_nw.min(max_nw);
        draw::text_max(cr, r.0 + r.2 - 10.0 - nw, r.1 + (r.3 - nh) / 2.0, nw, &nt) + 8.0
    };
    if let Some(ic) = icon_at_right {
        icon(
            cr,
            ic,
            r.0 + r.2 - 10.0 - note_w - ICON,
            r.1 + (r.3 - ICON) / 2.0,
            if active && ic == Icon::Check {
                th.accent_text
            } else {
                th.muted
            },
        );
    }
    let label_t = Text {
        size: draw::SIZE_BODY,
        bold: active,
        color: if active { th.accent_text } else { th.text },
        text,
    };
    let (_, lh) = draw::measure(cr, &label_t);
    draw::text_max(
        cr,
        r.0 + 10.0,
        r.1 + (r.3 - lh) / 2.0,
        (r.2 - 20.0 - note_w - icon_w)
            .max(20.0)
            .min((r.2 - 20.0).max(0.0)),
        &label_t,
    );
    lit
}

/// A thin scroll bar on the right edge of `view` when `content_h` exceeds `view.3`.
pub fn scrollbar(cr: &Context, cx: &Cx, view: Rect, content_h: f64, scroll: f64) {
    let max_scroll = content_h - view.3;
    if max_scroll <= 0.01 || view.3 <= 0.0 {
        return;
    }
    let thumb_h = ((view.3 / content_h) * view.3).clamp(24.0_f64.min(view.3), view.3);
    let track = (view.3 - thumb_h).max(1.0);
    let frac = (scroll / max_scroll).clamp(0.0, 1.0);
    let thumb_y = view.1 + frac * track;
    draw::round_rect(cr, view.0 + view.2 - 4.0, thumb_y, 3.0, thumb_h, 1.5);
    draw::set(cr, cx.theme.muted.alpha(0.35));
    let _ = cr.fill();
}

/// Content and padding for a single-line text field.
pub struct TextField<'a> {
    pub value: &'a str,
    pub placeholder: &'a str,
    pub focused: bool,
    pub show_caret: bool,
    pub left_pad: f64,
    pub right_pad: f64,
}

/// A single-line text field with rounded background, focus border, placeholder, and optional
/// caret. Returns the caret's x coordinate.
pub fn text_field(cr: &Context, th: &Theme, r: Rect, f: &TextField) -> f64 {
    let rad = th.small_radius().max(4.0);
    draw::round_rect(cr, r.0.round(), r.1.round(), r.2.round(), r.3.round(), rad);
    draw::set(cr, th.track);
    let _ = cr.fill();
    draw::round_rect(
        cr,
        r.0.round() + 0.5,
        r.1.round() + 0.5,
        (r.2.round() - 1.0).max(0.0),
        (r.3.round() - 1.0).max(0.0),
        (rad - 0.5).max(0.0),
    );
    draw::set(cr, if f.focused { th.accent } else { th.edge });
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    let tx = r.0 + f.left_pad;
    let max_w = (r.2 - f.left_pad - f.right_pad).max(10.0);
    let caret_x = if f.value.is_empty() {
        if !f.placeholder.is_empty() {
            let ph_t = Text {
                size: draw::SIZE_BODY,
                bold: false,
                color: th.muted,
                text: f.placeholder,
            };
            let (_, ph) = draw::measure(cr, &ph_t);
            draw::text_max(cr, tx, r.1 + (r.3 - ph) / 2.0, max_w, &ph_t);
        }
        tx
    } else {
        let val_t = Text {
            size: draw::SIZE_BODY,
            bold: false,
            color: th.text,
            text: f.value,
        };
        let (vw, vh) = draw::measure(cr, &val_t);
        draw::text_max(cr, tx, r.1 + (r.3 - vh) / 2.0, max_w, &val_t);
        (tx + vw.min(max_w) + 1.5).min(r.0 + r.2 - f.right_pad)
    }
    .round();

    if f.show_caret {
        draw::set(cr, th.accent);
        cr.rectangle(caret_x, (r.1 + (r.3 - 14.0) / 2.0).round(), 1.5, 14.0);
        let _ = cr.fill();
    }
    caret_x
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

    fn test_cx(theme: &crate::theme::Theme) -> Cx<'_> {
        Cx {
            theme,
            now: chrono::NaiveDateTime::default(),
            size: crate::provider::Size::M,
            app: None,
            apps: &[],
            recent: &crate::recent::NONE,
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
            room: f64::INFINITY,
        }
    }

    fn render_pixel<F>(w: i32, h: i32, draw_fn: F, px: (usize, usize)) -> [u8; 4]
    where
        F: FnOnce(&Context),
    {
        let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).unwrap();
        {
            let cr = Context::new(&surf).unwrap();
            draw_fn(&cr);
        }
        surf.flush();
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        let off = px.1 * stride + px.0 * 4;
        [data[off], data[off + 1], data[off + 2], data[off + 3]]
    }

    #[test]
    fn button_uses_soft_fill_without_outline_and_deepens_on_hover() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let rest_edge = render_pixel(
            180,
            60,
            |cr| {
                let mut hits = Hits::new(None, None);
                button(cr, &cx, &mut hits, 1, 10.0, 10.0, "Remove");
            },
            (24, 10),
        );
        let rest_inside = render_pixel(
            180,
            60,
            |cr| {
                let mut hits = Hits::new(None, None);
                button(cr, &cx, &mut hits, 1, 10.0, 10.0, "Remove");
            },
            (24, 12),
        );
        assert!(rest_inside[3] > 0, "button should have a visible rest fill");
        assert_eq!(
            rest_edge, rest_inside,
            "button edge should match interior (no 1 px outline)"
        );

        let hovered_inside = render_pixel(
            180,
            60,
            |cr| {
                let mut hits = Hits::new(None, Some(1));
                button(cr, &cx, &mut hits, 1, 10.0, 10.0, "Remove");
            },
            (24, 12),
        );
        assert!(
            hovered_inside[3] > rest_inside[3],
            "hovered/selected button fill should deepen"
        );
    }

    #[test]
    fn value_box_fills_when_closed_and_hides_block_when_open() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let row = (0.0, 10.0, 300.0, ROW_H);
        let mut closed_w = 0.0;
        let closed_bg = render_pixel(
            320,
            60,
            |cr| {
                closed_w = value_box(cr, &cx, row, 290.0, "Open window", false, false);
            },
            (286, 18),
        );
        let hovered_bg = render_pixel(
            320,
            60,
            |cr| {
                value_box(cr, &cx, row, 290.0, "Open window", true, false);
            },
            (286, 18),
        );
        let open_bg = render_pixel(
            320,
            60,
            |cr| {
                value_box(cr, &cx, row, 290.0, "Open window", false, true);
            },
            (286, 18),
        );
        assert!(closed_w > 40.0);
        assert!(closed_bg[3] > 0, "closed value_box draws soft accent block");
        assert!(hovered_bg[3] > closed_bg[3], "hovered value_box deepens");
        assert_eq!(
            open_bg[3], 0,
            "open value_box hides background block behind label and chevron"
        );
    }

    #[test]
    fn choice_uses_soft_fills_without_outline_and_highlights_chosen() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let r = (10.0, 10.0, 290.0, 34.0);
        // With 3 items across 290 px, item 0 is at x=10..102.6, item 1 is at x=108.6..201.3.
        let unchosen_edge = render_pixel(
            320,
            60,
            |cr| {
                let mut hits = Hits::new(None, None);
                choice(cr, &cx, &mut hits, 0, r, &["Low", "Balanced", "High"], 1);
            },
            (24, 10),
        );
        let unchosen_in = render_pixel(
            320,
            60,
            |cr| {
                let mut hits = Hits::new(None, None);
                choice(cr, &cx, &mut hits, 0, r, &["Low", "Balanced", "High"], 1);
            },
            (24, 12),
        );
        let chosen_edge = render_pixel(
            320,
            60,
            |cr| {
                let mut hits = Hits::new(None, None);
                choice(cr, &cx, &mut hits, 0, r, &["Low", "Balanced", "High"], 1);
            },
            (122, 10),
        );
        let chosen_in = render_pixel(
            320,
            60,
            |cr| {
                let mut hits = Hits::new(None, None);
                choice(cr, &cx, &mut hits, 0, r, &["Low", "Balanced", "High"], 1);
            },
            (122, 12),
        );
        assert!(unchosen_in[3] > 0);
        assert_eq!(unchosen_edge, unchosen_in);
        assert_eq!(chosen_edge, chosen_in);
        assert_ne!(chosen_in, unchosen_in);

        let hovered_unchosen = render_pixel(
            320,
            60,
            |cr| {
                let mut hits = Hits::new(None, Some(0));
                choice(cr, &cx, &mut hits, 0, r, &["Low", "Balanced", "High"], 1);
            },
            (24, 12),
        );
        assert!(hovered_unchosen[3] > unchosen_in[3]);
    }

    #[test]
    fn row_alert_draws_state_in_alert_colour_instead_of_accent() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        assert!(th.alert.0 > th.alert.2 && th.accent_text.2 > th.accent_text.0);

        let max_rb = |alert: bool, on: bool| {
            let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 260, 44).unwrap();
            {
                let cr = Context::new(&surf).unwrap();
                // grey edges on text: a CI machine whose fonts are set to subpixel smoothing gave the alert
                // text blue fringes (34 "accent" pixels) on 2026-11-14
                crate::draw::grey_text_edges(&cr);
                let mut hits = Hits::new(None, None);
                let r = (0.0, 0.0, 260.0, ROW_H);
                let data = Row {
                    icon: None,
                    name: "Accent",
                    state: "Not a colour",
                    on,
                    right: 0.0,
                };
                row_alert(&cr, &cx, &mut hits, 1, r, &data, alert);
            }
            surf.flush();
            let stride = surf.stride() as usize;
            let data = surf.data().unwrap();
            let (mut red_dom, mut blue_dom) = (0usize, 0usize);
            for y in 22..44 {
                for x in 0..260 {
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
            (red_dom, blue_dom)
        };

        let (alert_red, alert_blue) = max_rb(true, true);
        assert!(
            alert_red > 10,
            "expected alert state pixels, got {alert_red}"
        );
        assert_eq!(
            alert_blue, 0,
            "expected no accent blue pixels on alert state"
        );

        let (accent_red, accent_blue) = max_rb(false, true);
        assert!(
            accent_blue > 10,
            "expected accent state pixels, got {accent_blue}"
        );
        assert_eq!(
            accent_red, 0,
            "expected no red alert pixels on accent state"
        );
    }

    #[test]
    fn scrollbar_draws_only_when_content_overflows_and_tracks_scroll() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let view = (0.0, 10.0, 200.0, 100.0);

        let no_overflow = render_pixel(
            210,
            120,
            |cr| scrollbar(cr, &cx, view, 100.0, 0.0),
            (197, 15),
        );
        assert_eq!(no_overflow[3], 0, "no scrollbar when content fits");

        let top_start = render_pixel(
            210,
            120,
            |cr| scrollbar(cr, &cx, view, 400.0, 0.0),
            (197, 15),
        );
        let top_end = render_pixel(
            210,
            120,
            |cr| scrollbar(cr, &cx, view, 400.0, 0.0),
            (197, 105),
        );
        assert!(top_start[3] > 0, "thumb drawn at top when scroll is 0");
        assert_eq!(top_end[3], 0, "bottom clear when scroll is 0");

        let bot_start = render_pixel(
            210,
            120,
            |cr| scrollbar(cr, &cx, view, 400.0, 300.0),
            (197, 15),
        );
        let bot_end = render_pixel(
            210,
            120,
            |cr| scrollbar(cr, &cx, view, 400.0, 300.0),
            (197, 105),
        );
        assert_eq!(bot_start[3], 0, "top clear at bottom of scroll");
        assert!(bot_end[3] > 0, "thumb drawn at bottom at max scroll");
    }

    #[test]
    fn text_field_returns_caret_position_and_draws_caret_when_shown() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(
            "ACCENT=\"#3366ee\"\n",
        ));
        let r = (10.0, 10.0, 240.0, 40.0);
        let mut caret_empty = 0.0;
        let mut caret_abc = 0.0;
        let caret_px = render_pixel(
            260,
            60,
            |cr| {
                caret_empty = text_field(
                    cr,
                    &th,
                    r,
                    &TextField {
                        value: "",
                        placeholder: "Search",
                        focused: false,
                        show_caret: false,
                        left_pad: 12.0,
                        right_pad: 12.0,
                    },
                );
            },
            (22, 30),
        );
        let with_text = render_pixel(
            260,
            60,
            |cr| {
                caret_abc = text_field(
                    cr,
                    &th,
                    r,
                    &TextField {
                        value: "abc",
                        placeholder: "Search",
                        focused: true,
                        show_caret: true,
                        left_pad: 12.0,
                        right_pad: 12.0,
                    },
                );
            },
            // Sample at returned caret_abc after computing it below; we verify via second render.
            (22, 30),
        );
        assert_eq!(caret_empty, 22.0);
        assert!(caret_abc > caret_empty);
        let caret_drawn = render_pixel(
            260,
            60,
            |cr| {
                text_field(
                    cr,
                    &th,
                    r,
                    &TextField {
                        value: "abc",
                        placeholder: "Search",
                        focused: true,
                        show_caret: true,
                        left_pad: 12.0,
                        right_pad: 12.0,
                    },
                );
            },
            (caret_abc as usize, 30),
        );
        assert!(
            caret_drawn[0] > caret_drawn[2] && caret_drawn[3] > 200,
            "caret should be drawn in accent blue at caret_abc={caret_abc}: {caret_drawn:?}"
        );
        let _ = (caret_px, with_text);
    }

    fn assert_drawn_only_inside<F>(surf_w: i32, surf_h: i32, r: Rect, label: &str, draw_fn: F)
    where
        F: FnOnce(&Context),
    {
        let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, surf_w, surf_h).unwrap();
        {
            let cr = Context::new(&surf).unwrap();
            draw_fn(&cr);
        }
        surf.flush();
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        let x0 = r.0.floor() as usize;
        let y0 = r.1.floor() as usize;
        let x1 = (r.0 + r.2).ceil() as usize;
        let y1 = (r.1 + r.3).ceil() as usize;
        let mut inside_ink = 0usize;
        for y in 0..surf_h as usize {
            for x in 0..surf_w as usize {
                let off = y * stride + x * 4;
                let px = [data[off], data[off + 1], data[off + 2], data[off + 3]];
                if x >= x0 && x < x1 && y >= y0 && y < y1 {
                    if px[3] > 0 {
                        inside_ink += 1;
                    }
                } else {
                    assert_eq!(
                        px,
                        [0, 0, 0, 0],
                        "{label}: pixel outside rect {r:?} touched at ({x}, {y}): {px:?}"
                    );
                }
            }
        }
        assert!(inside_ink > 0, "{label}: nothing drawn inside {r:?}");
    }

    #[test]
    fn narrow_140px_helpers_keep_60_char_text_inside_rect() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let long = "W".repeat(60);
        assert_eq!(long.chars().count(), 60);

        // 1. ui::row (with icon, 60-char name, 60-char state, and right reserved width)
        let row_r = (20.0, 20.0, 140.0, ROW_H);
        assert_drawn_only_inside(200, 80, row_r, "row", |cr| {
            let mut hits = Hits::new(None, None);
            row(
                cr,
                &cx,
                &mut hits,
                1,
                row_r,
                &Row {
                    icon: Some(Icon::Wifi(3)),
                    name: &long,
                    state: &long,
                    on: true,
                    right: MORE_W,
                },
            );
        });

        // 2. ui::list_row (60-char text, 60-char note, and right icon)
        let list_r = (20.0, 20.0, 140.0, LIST_ROW_H);
        assert_drawn_only_inside(200, 80, list_r, "list_row", |cr| {
            let mut hits = Hits::new(None, None);
            list_row(
                cr,
                &cx,
                &mut hits,
                1,
                list_r,
                &long,
                &long,
                true,
                Some(Icon::Check),
            );
        });

        // 3. ui::value_box (closed and open inside a 140 px row)
        assert_drawn_only_inside(200, 80, row_r, "value_box closed", |cr| {
            let w = value_box(cr, &cx, row_r, row_r.0 + row_r.2, &long, false, false);
            assert!(w <= row_r.2 * 0.6 + 1e-6);
        });
        assert_drawn_only_inside(200, 80, row_r, "value_box open", |cr| {
            let w = value_box(cr, &cx, row_r, row_r.0 + row_r.2, &long, false, true);
            assert!(w <= row_r.2 * 0.6 + 1e-6);
        });

        // 4. ui::button (at 140 px max width with a 60-char label)
        let btn_r = (20.0, 20.0, 140.0, 26.0);
        assert_drawn_only_inside(200, 80, btn_r, "button", |cr| {
            let mut hits = Hits::new(None, None);
            let w = button(cr, &cx, &mut hits, 1, btn_r.0, btn_r.1, &long);
            assert!(w <= 140.0, "button width {w} exceeded 140 px");
        });

        // 5. ui::choice_button (at 140 px width with a 60-char label)
        assert_drawn_only_inside(200, 80, btn_r, "choice_button", |cr| {
            let mut hits = Hits::new(None, None);
            choice_button(cr, &cx, &mut hits, 1, btn_r, &long, true);
        });
    }

    #[test]
    fn active_list_row_deepens_on_hover_and_switch_uses_on_accent_knob() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(
            "SURFACE_COLOR=\"#111317\"\nTEXT_COLOR=\"#eef2f8\"\nACCENT=\"#22242a\"\n",
        ));
        let cx = test_cx(&th);
        let row_r = (10.0, 10.0, 220.0, LIST_ROW_H);

        let rest = render_pixel(
            260,
            60,
            |cr| {
                let mut hits = Hits::new(None, None);
                list_row(cr, &cx, &mut hits, 1, row_r, "Headphones", "", true, None);
            },
            (200, 24),
        );
        let hovered = render_pixel(
            260,
            60,
            |cr| {
                let mut hits = Hits::new(None, Some(1));
                list_row(cr, &cx, &mut hits, 1, row_r, "Headphones", "", true, None);
            },
            (200, 24),
        );
        assert!(
            hovered[3] > rest[3],
            "active list row should deepen on hover/selection"
        );

        // On a dark accent on a dark card, the ON switch knob must use light on_accent, not dark card.
        let sw_row = (0.0, 0.0, 100.0, ROW_H);
        let knob_px = render_pixel(
            120,
            40,
            |cr| {
                let mut hits = Hits::new(None, None);
                switch(cr, &cx, &mut hits, 1, sw_row, 90.0, true);
            },
            (82, 20),
        );
        assert!(
            knob_px[2] > 180 && knob_px[1] > 180 && knob_px[0] > 180,
            "ON switch knob should use readable on_accent colour, got {knob_px:?}"
        );
    }
}
