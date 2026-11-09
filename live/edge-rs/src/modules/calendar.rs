//! The calendar module: six weeks, Monday first, previous/next month arrows.

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{Rect, World};
use crate::theme::Theme;
use cairo::Context;
use chrono::{Datelike, NaiveDate};

const CAL_ROW: f64 = 34.0;

pub fn height() -> f64 {
    2.0 * PAD_Y + 28.0 + 22.0 + 6.0 * CAL_ROW
}

/// The first day of the month `delta` months away from `today`'s month.
pub fn shifted_month(today: NaiveDate, delta: i32) -> NaiveDate {
    let total = today.year() * 12 + (today.month() as i32 - 1) + delta;
    let y = total.div_euclid(12);
    let m = (total.rem_euclid(12) + 1) as u32;
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(today)
}

/// The `(x, y, w, h)` hitboxes of the calendar's previous and next month arrows, inside its card.
pub fn arrows(w: f64) -> (Rect, Rect) {
    let h = PAD_Y + 26.0;
    let prev = (w - PAD_X - 40.0, 0.0, 20.0, h);
    let next = (w - PAD_X - 20.0, 0.0, 20.0 + PAD_X, h);
    (prev, next)
}

pub fn click(world: &mut World, w: f64, pos: (f64, f64)) -> bool {
    let (x, y) = pos;
    let (prev, next) = arrows(w);
    if x >= prev.0 && x < prev.0 + prev.2 && y >= prev.1 && y <= prev.1 + prev.3 {
        world.cal_month -= 1;
        true
    } else if x >= next.0 && x <= next.0 + next.2 && y >= next.1 && y <= next.1 + next.3 {
        world.cal_month += 1;
        true
    } else {
        false
    }
}

/// The month: six weeks, Monday first, the days of the months before and after dimmed, today on
/// the accent.
pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, _) = rect;
    let h = height();
    draw::card(cr, th, x, y, w, h);
    let today = world.now.date();
    let first = shifted_month(today, world.cal_month);
    draw::title(
        cr,
        th,
        x + PAD_X,
        y + PAD_Y,
        &first.format("%B").to_string(),
    );
    draw::text_right(
        cr,
        x + w - PAD_X - 44.0,
        y + PAD_Y + 2.0,
        &Text {
            size: 13.0,
            bold: false,
            color: th.muted,
            text: &first.format("%Y").to_string(),
        },
    );
    draw::set(cr, th.muted);
    cr.set_line_width(1.5);
    let ay = y + PAD_Y + 10.0;
    let px = x + w - PAD_X - 30.0;
    cr.move_to(px + 2.0, ay - 4.0);
    cr.line_to(px - 2.0, ay);
    cr.line_to(px + 2.0, ay + 4.0);
    let _ = cr.stroke();
    let nx = x + w - PAD_X - 10.0;
    cr.move_to(nx - 2.0, ay - 4.0);
    cr.line_to(nx + 2.0, ay);
    cr.line_to(nx - 2.0, ay + 4.0);
    let _ = cr.stroke();
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
        } else if day.month() != first.month() {
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
