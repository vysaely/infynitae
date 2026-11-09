//! The modules. Each one knows how tall it is at a width and draws itself into its card; a module
//! not ported yet draws a card that says so (docs/EDGE_RUST.md, stages 2 to 4).

use crate::config::{Item, Module};
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::theme::Theme;
use cairo::Context;
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, Timelike};

/// What a module needs from the world to draw (fixed in tests and `--render`).
pub struct World {
    pub now: NaiveDateTime,
}

impl World {
    pub fn now() -> World {
        let fixed = std::env::var("INFYNITAE_EDGE_RENDER_TIME")
            .ok()
            .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M").ok());
        World {
            now: fixed.unwrap_or_else(|| Local::now().naive_local()),
        }
    }
}

/// The module's height at width `w` (its card included).
pub fn height(it: &Item, w: f64) -> f64 {
    let _ = w;
    let (names, def) = it.module.looks();
    let look = it.look.unwrap_or(def);
    match it.module {
        Module::Clock => match names.get(look).copied().unwrap_or("time") {
            "time" => 2.0 * PAD_Y + 40.0,
            "big" => 2.0 * PAD_Y + 86.0,
            _ => calendar_height() + 2.0 * PAD_Y + 86.0 + draw::GAP,
        },
        Module::Calendar => calendar_height(),
        _ => 2.0 * PAD_Y + 40.0,
    }
}

/// A module drawn into its rectangle (x, y, w, h).
pub fn draw(cr: &Context, th: &Theme, it: &Item, world: &World, rect: (f64, f64, f64, f64)) {
    let (x, y, w, h) = rect;
    let (names, def) = it.module.looks();
    let look = names.get(it.look.unwrap_or(def)).copied().unwrap_or("");
    match it.module {
        Module::Clock if look == "calendar" => {
            let top = 2.0 * PAD_Y + 86.0;
            draw::card(cr, th, x, y, w, top);
            clock(cr, th, world, x, y, w, true);
            calendar(cr, th, world, x, y + top + draw::GAP, w);
        }
        Module::Clock => {
            draw::card(cr, th, x, y, w, h);
            clock(cr, th, world, x, y, w, look == "big");
        }
        Module::Calendar => calendar(cr, th, world, x, y, w),
        m => {
            draw::card(cr, th, x, y, w, h);
            draw::title(cr, th, x + PAD_X, y + PAD_Y, m.word());
            draw::text_right(
                cr,
                x + w - PAD_X,
                y + PAD_Y + 2.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: "not in Rust yet",
                },
            );
        }
    }
}

fn clock(cr: &Context, th: &Theme, world: &World, x: f64, y: f64, w: f64, big: bool) {
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

const CAL_ROW: f64 = 34.0;

fn calendar_height() -> f64 {
    2.0 * PAD_Y + 28.0 + 22.0 + 6.0 * CAL_ROW
}

/// The month: six weeks, Monday first, the days of the months before and after dimmed, today on
/// the accent.
fn calendar(cr: &Context, th: &Theme, world: &World, x: f64, y: f64, w: f64) {
    let h = calendar_height();
    draw::card(cr, th, x, y, w, h);
    let today = world.now.date();
    let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today);
    draw::title(
        cr,
        th,
        x + PAD_X,
        y + PAD_Y,
        &first.format("%B").to_string(),
    );
    draw::text_right(
        cr,
        x + w - PAD_X,
        y + PAD_Y + 2.0,
        &Text {
            size: 13.0,
            bold: false,
            color: th.muted,
            text: &first.format("%Y").to_string(),
        },
    );
    let col = (w - 2.0 * PAD_X) / 7.0;
    let top = y + PAD_Y + 28.0;
    for (i, d) in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
        .iter()
        .enumerate()
    {
        let cx = (x + PAD_X + col * (i as f64 + 0.5)).round();
        draw::text_center(
            cr,
            cx,
            top,
            &Text {
                size: 11.0,
                bold: false,
                color: th.muted,
                text: d,
            },
        );
    }
    let start = first - chrono::Duration::days(first.weekday().num_days_from_monday() as i64);
    for i in 0..42 {
        let day = start + chrono::Duration::days(i);
        let cx = (x + PAD_X + col * ((i % 7) as f64 + 0.5)).round();
        let cy = (top + 22.0 + CAL_ROW * (i / 7) as f64).round();
        let s = day.day().to_string();
        let color = if day == today {
            let r = 15.0;
            draw::round_rect(cr, cx - r, cy, 2.0 * r, 2.0 * r, th.small_radius().min(r));
            draw::set(cr, th.accent);
            let _ = cr.fill();
            th.on_accent
        } else if day.month() != today.month() {
            th.text.alpha(0.28)
        } else if i % 7 >= 5 {
            th.muted
        } else {
            th.text
        };
        draw::text_center(
            cr,
            cx,
            cy + 7.0,
            &Text {
                size: 13.0,
                bold: day == today,
                color,
                text: &s,
            },
        );
    }
}
