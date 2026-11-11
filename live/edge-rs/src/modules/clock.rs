//! The clock module (`clock`, `clock:big`, `clock:calendar`).
//!
//! # Sizes
//!
//! In placed panels or during resizing, size picks the look dynamically:
//! - **time** (`h < 112`): time and date on a line.
//! - **big** (`112 <= h`): large time digits with date underneath.
//! - **calendar** (`h >= 200.0 && w >= 250.0`): time face with the month calendar grid.

use crate::config::Item;
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{calendar, Rect, World};
use crate::theme::Theme;
use cairo::Context;
use chrono::Timelike;

pub fn size_look(w: f64, h: f64) -> &'static str {
    if h >= 200.0 && w >= 250.0 {
        "calendar"
    } else if h >= 2.0 * PAD_Y + 80.0 {
        "big" // the big face needs 64 px digits and the date: 2 * PAD_Y + 86, a little less is fine
    } else {
        "time"
    }
}

fn look_name(it: &Item, rect: Rect) -> &'static str {
    // the size picks the look only for a module with a size of its own; otherwise its natural
    // height (from its look) would pick a bigger look than the card has room for
    if it.rect.is_some() && rect.2 > 0.0 && rect.3 > 0.0 {
        size_look(rect.2, rect.3)
    } else {
        let (names, def) = it.module.looks();
        names.get(it.look.unwrap_or(def)).copied().unwrap_or("time")
    }
}

pub fn height(it: &Item) -> f64 {
    if let Some((_, _, _, h)) = it.rect {
        if h > 0 {
            return h as f64;
        }
    }
    let (names, def) = it.module.looks();
    let name = names.get(it.look.unwrap_or(def)).copied().unwrap_or("time");
    match name {
        "time" => 2.0 * PAD_Y + 40.0,
        "big" => 2.0 * PAD_Y + 86.0,
        _ => calendar::height() + 2.0 * PAD_Y + 86.0 + draw::GAP,
    }
}

pub fn draw(cr: &Context, th: &Theme, it: &Item, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    let look = look_name(it, rect);
    if look == "calendar" {
        let top = 2.0 * PAD_Y + 86.0;
        draw::card(cr, th, x, y, w, h.max(top));
        draw_face(cr, th, world, x, y);
        calendar::draw_content(
            cr,
            th,
            world,
            (x, y + top + draw::GAP, w, (h - top - draw::GAP).max(40.0)),
        );
    } else {
        draw::card(cr, th, x, y, w, h);
        if look == "big" {
            draw_face(cr, th, world, x, y);
        } else {
            draw_line(cr, th, world, (x, y, w, h));
        }
    }
}

pub fn click(it: &Item, world: &mut World, size: (f64, f64), pos: (f64, f64)) -> bool {
    let look = look_name(it, (0.0, 0.0, size.0, size.1));
    if look == "calendar" {
        let top = 2.0 * PAD_Y + 86.0 + draw::GAP;
        calendar::click(
            world,
            (size.0, (size.1 - top).max(0.0)),
            (pos.0, pos.1 - top),
        )
    } else {
        false
    }
}

/// The time and the date on one line, centred in the card's height: 28 px digits when there is room
/// (the natural 72 px), smaller down to 16 px in a short placed card (the C edge's 34 px clock).
fn draw_line(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    let t = format!("{:02}:{:02}", world.now.hour(), world.now.minute());
    let date = world.now.format("%A, %-d %B").to_string();
    let size = ((h - 12.0) * 0.75).clamp(16.0, 28.0);
    let line = size * 1.35; // pango's line height for these sizes
    let ty = (y + (h - line) / 2.0).round();
    let tw = draw::text(
        cr,
        x + PAD_X,
        ty,
        &Text {
            size,
            bold: true,
            color: th.text,
            text: &t,
        },
    );
    let ds = (size * 0.5).clamp(12.0, 13.0);
    let dy = (y + (h - ds * 1.35) / 2.0 + 1.0).round();
    let room = w - 2.0 * PAD_X - tw - 12.0;
    if room > 40.0 {
        draw::text_right(
            cr,
            x + w - PAD_X,
            dy,
            &Text {
                size: ds,
                bold: false,
                color: th.muted,
                text: &date,
            },
        );
    }
}

/// The big face: 64 px digits and the date under them.
fn draw_face(cr: &Context, th: &Theme, world: &World, x: f64, y: f64) {
    let t = format!("{:02}:{:02}", world.now.hour(), world.now.minute());
    let date = world.now.format("%A, %-d %B").to_string();
    draw::text(
        cr,
        x + PAD_X,
        y + PAD_Y - 6.0,
        &Text {
            size: 64.0,
            bold: true,
            color: th.text,
            text: &t,
        },
    );
    draw::text(
        cr,
        x + PAD_X,
        y + PAD_Y + 66.0,
        &Text {
            size: 15.0,
            bold: false,
            color: th.muted,
            text: &date,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_look_picks_expected_mode() {
        assert_eq!(size_look(300.0, 50.0), "time");
        assert_eq!(size_look(300.0, 72.0), "time"); // the natural height of the time look
        assert_eq!(size_look(300.0, 118.0), "big");
        assert_eq!(size_look(200.0, 250.0), "big"); // w < 250 stays big
        assert_eq!(size_look(300.0, 250.0), "calendar");
    }

    /// The bug from the stage 4 VM test: an unplaced clock at its natural height (72 px) picked
    /// the big look by size and drew 64 px digits out of its card. Only a placed clock follows its size.
    #[test]
    fn an_unplaced_clock_keeps_its_look() {
        let c = crate::config::Config::from_text("PANEL_T=\"SUPER+T bottom right clock\"\n");
        let it = c
            .panels
            .iter()
            .flat_map(|p| &p.items)
            .find(|i| i.module == crate::modules::Module::Clock && i.rect.is_none())
            .expect("the clock item");
        assert_eq!(height(it), 2.0 * PAD_Y + 40.0);
        assert_eq!(look_name(it, (0.0, 0.0, 344.0, height(it))), "time");
    }
}
