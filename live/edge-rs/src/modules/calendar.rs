//! The calendar module: six weeks, Monday first, previous/next month arrows.
//!
//! # Sizes
//!
//! - **today only** (`h < 140.0`):
//!   Card showing today's day of week, day number, and month/year.
//! - **the month** (`140.0 <= h < 310.0`):
//!   The standard 6-week calendar month grid with previous/next month arrows.
//! - **the month and events** (`h >= 310.0`):
//!   The month grid with an "Events" agenda list below.

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{Rect, World};
use crate::theme::Theme;
use cairo::Context;
use chrono::{Datelike, NaiveDate};

const CAL_ROW: f64 = 34.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarEvent {
    pub time: String,
    pub title: String,
}

pub fn height() -> f64 {
    2.0 * PAD_Y + 28.0 + 22.0 + 6.0 * CAL_ROW
}

/// Parses raw event lines (e.g. `time \t title` or `time title`) into `CalendarEvent`s.
pub fn parse_calendar_events(out: &str) -> Vec<CalendarEvent> {
    let mut events = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((time, title)) = line.split_once('\t') {
            events.push(CalendarEvent {
                time: time.trim().to_string(),
                title: title.trim().to_string(),
            });
        } else if let Some((time, title)) = line.split_once(' ') {
            if time.contains(':') && time.chars().any(|c| c.is_ascii_digit()) {
                events.push(CalendarEvent {
                    time: time.trim().to_string(),
                    title: title.trim().to_string(),
                });
            } else {
                events.push(CalendarEvent {
                    time: String::new(),
                    title: line.to_string(),
                });
            }
        } else {
            events.push(CalendarEvent {
                time: String::new(),
                title: line.to_string(),
            });
        }
    }
    events
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

/// Draws the calendar module choosing size according to `h`:
/// - `h < 140.0`: today only
/// - `h >= 310.0`: the month and events
/// - otherwise: the month
pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    draw_inner(cr, th, world, rect, true);
}

/// Draws the calendar content without rendering an outer card background (for embedding in clock).
pub fn draw_content(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    draw_inner(cr, th, world, rect, false);
}

fn draw_inner(cr: &Context, th: &Theme, world: &World, rect: Rect, with_card: bool) {
    let (x, y, w, h) = rect;
    let effective_h = if h > 0.0 { h } else { height() };

    if effective_h < 140.0 {
        draw_today_only(cr, th, world, (x, y, w, effective_h), with_card);
    } else if effective_h >= 310.0 {
        draw_month_and_events(cr, th, world, (x, y, w, effective_h), with_card);
    } else {
        draw_month(cr, th, world, (x, y, w, effective_h), with_card);
    }
}

/// Draws "today only" view (compact).
fn draw_today_only(cr: &Context, th: &Theme, world: &World, rect: Rect, with_card: bool) {
    let (x, y, w, h) = rect;
    if with_card {
        draw::card(cr, th, x, y, w, h);
    }
    let today = world.now.date();

    let weekday = today.format("%A").to_string();
    let mday = today.day().to_string();
    let my = today.format("%B %Y").to_string();

    draw::title(cr, th, x + PAD_X, y + PAD_Y, &weekday);

    draw::text(
        cr,
        x + PAD_X,
        y + PAD_Y + 24.0,
        &Text {
            size: 32.0,
            bold: true,
            color: th.accent,
            text: &mday,
        },
    );

    draw::text(
        cr,
        x + PAD_X + 54.0,
        y + PAD_Y + 34.0,
        &Text {
            size: 13.0,
            bold: false,
            color: th.muted,
            text: &my,
        },
    );
}

/// Draws the month grid.
fn draw_month_grid(cr: &Context, th: &Theme, world: &World, x: f64, y: f64, w: f64) {
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

/// Draws the standard month view.
fn draw_month(cr: &Context, th: &Theme, world: &World, rect: Rect, with_card: bool) {
    let (x, y, w, h) = rect;
    if with_card {
        draw::card(cr, th, x, y, w, h);
    }
    draw_month_grid(cr, th, world, x, y, w);
}

/// Draws the month grid and the events section below.
fn draw_month_and_events(cr: &Context, th: &Theme, world: &World, rect: Rect, with_card: bool) {
    let (x, y, w, h) = rect;
    if with_card {
        draw::card(cr, th, x, y, w, h);
    }
    draw_month_grid(cr, th, world, x, y, w);

    let events_y = y + PAD_Y + 28.0 + 22.0 + 6.0 * CAL_ROW + 12.0;

    // Events section header
    draw::text(
        cr,
        x + PAD_X,
        events_y,
        &Text {
            size: 13.0,
            bold: true,
            color: th.text,
            text: "Events",
        },
    );

    let item_y = events_y + 20.0;
    if world.calendar_events.is_empty() {
        draw::text(
            cr,
            x + PAD_X,
            item_y,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: "No events today",
            },
        );
    } else {
        let mut cur_y = item_y;
        for ev in &world.calendar_events {
            if cur_y + 16.0 > y + h - PAD_Y {
                break;
            }
            if !ev.time.is_empty() {
                let tw = draw::text(
                    cr,
                    x + PAD_X,
                    cur_y,
                    &Text {
                        size: 12.0,
                        bold: true,
                        color: th.accent,
                        text: &ev.time,
                    },
                );
                draw::text_max(
                    cr,
                    x + PAD_X + tw + 8.0,
                    cur_y,
                    w - 2.0 * PAD_X - tw - 8.0,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: th.text,
                        text: &ev.title,
                    },
                );
            } else {
                draw::text_max(
                    cr,
                    x + PAD_X,
                    cur_y,
                    w - 2.0 * PAD_X,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: th.text,
                        text: &ev.title,
                    },
                );
            }
            cur_y += 20.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_calendar_events_tsv_and_space() {
        let sample = "10:00\tTeam Standup\n14:30 Code Review\nAll-day Holiday\n";
        let evs = parse_calendar_events(sample);
        assert_eq!(evs.len(), 3);
        assert_eq!(evs[0].time, "10:00");
        assert_eq!(evs[0].title, "Team Standup");
        assert_eq!(evs[1].time, "14:30");
        assert_eq!(evs[1].title, "Code Review");
        assert_eq!(evs[2].time, "");
        assert_eq!(evs[2].title, "All-day Holiday");
    }

    #[test]
    fn calendar_draw_content_renders_without_card_background() {
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 380, 240).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let world = World::now();
        draw_content(&cr, &th, &world, (0.0, 0.0, 380.0, 240.0));
    }
}
