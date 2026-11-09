//! The clock module (`clock`, `clock:big`, `clock:calendar`).

use crate::config::Item;
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{calendar, Rect, World};
use crate::theme::Theme;
use cairo::Context;
use chrono::Timelike;

fn look_name(it: &Item) -> &'static str {
    let (names, def) = it.module.looks();
    names.get(it.look.unwrap_or(def)).copied().unwrap_or("time")
}

pub fn height(it: &Item) -> f64 {
    match look_name(it) {
        "time" => 2.0 * PAD_Y + 40.0,
        "big" => 2.0 * PAD_Y + 86.0,
        _ => calendar::height() + 2.0 * PAD_Y + 86.0 + draw::GAP,
    }
}

pub fn draw(cr: &Context, th: &Theme, it: &Item, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    let look = look_name(it);
    if look == "calendar" {
        let top = 2.0 * PAD_Y + 86.0;
        draw::card(cr, th, x, y, w, top);
        draw_face(cr, th, world, x, y, w, true);
        calendar::draw(
            cr,
            th,
            world,
            (x, y + top + draw::GAP, w, calendar::height()),
        );
    } else {
        draw::card(cr, th, x, y, w, h);
        draw_face(cr, th, world, x, y, w, look == "big");
    }
}

pub fn click(it: &Item, world: &mut World, size: (f64, f64), pos: (f64, f64)) -> bool {
    if look_name(it) == "calendar" {
        let top = 2.0 * PAD_Y + 86.0 + draw::GAP;
        calendar::click(world, size.0, (pos.0, pos.1 - top))
    } else {
        false
    }
}

fn draw_face(cr: &Context, th: &Theme, world: &World, x: f64, y: f64, w: f64, big: bool) {
    let t = format!("{:02}:{:02}", world.now.hour(), world.now.minute());
    let date = world.now.format("%A, %-d %B").to_string();
    if big {
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
    } else {
        draw::text(
            cr,
            x + PAD_X,
            y + PAD_Y + 2.0,
            &Text {
                size: 28.0,
                bold: true,
                color: th.text,
                text: &t,
            },
        );
        draw::text_right(
            cr,
            x + w - PAD_X,
            y + PAD_Y + 14.0,
            &Text {
                size: 13.0,
                bold: false,
                color: th.muted,
                text: &date,
            },
        );
    }
}
