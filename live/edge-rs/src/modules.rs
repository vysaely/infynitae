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
    pub cal_month: i32,
}

impl World {
    pub fn now() -> World {
        let fixed = std::env::var("INFYNITAE_EDGE_RENDER_TIME")
            .ok()
            .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M").ok());
        World {
            now: fixed.unwrap_or_else(|| Local::now().naive_local()),
            cal_month: 0,
        }
    }
}

impl Module {
    /// A pointer click at `pos` inside this module's `size` rectangle; true when state changed.
    pub fn click(
        self,
        look: Option<usize>,
        world: &mut World,
        size: (f64, f64),
        pos: (f64, f64),
        button: u32,
    ) -> bool {
        let _ = button;
        let (w, _h) = size;
        let (x, y) = pos;
        let (names, def) = self.looks();
        let look_name = names.get(look.unwrap_or(def)).copied().unwrap_or("");
        match self {
            Module::Calendar => calendar_click(world, w, x, y),
            Module::Clock if look_name == "calendar" => {
                let top = 2.0 * PAD_Y + 86.0 + draw::GAP;
                calendar_click(world, w, x, y - top)
            }
            _ => false,
        }
    }

    /// A key press while this module's panel is open; true when state changed.
    pub fn key(
        self,
        look: Option<usize>,
        world: &mut World,
        keysym: u32,
        ch: Option<char>,
    ) -> bool {
        let _ = (self, look, world, keysym, ch);
        false
    }
}

/// Dispatches a pointer click at `pos` inside `it`'s `size` card.
pub fn click(it: &Item, world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    it.module.click(it.look, world, size, pos, button)
}

/// Dispatches a key press to `it`.
pub fn key(it: &Item, world: &mut World, keysym: u32, ch: Option<char>) -> bool {
    it.module.key(it.look, world, keysym, ch)
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

/// The first day of the month `delta` months away from `today`'s month.
pub fn shifted_month(today: NaiveDate, delta: i32) -> NaiveDate {
    let total = today.year() * 12 + (today.month() as i32 - 1) + delta;
    let y = total.div_euclid(12);
    let m = (total.rem_euclid(12) + 1) as u32;
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(today)
}

/// A rectangle `(x, y, w, h)`.
pub type Rect = (f64, f64, f64, f64);

/// The `(x, y, w, h)` hitboxes of the calendar's previous and next month arrows, inside its card.
pub fn calendar_arrows(w: f64) -> (Rect, Rect) {
    let h = PAD_Y + 26.0;
    let prev = (w - PAD_X - 40.0, 0.0, 20.0, h);
    let next = (w - PAD_X - 20.0, 0.0, 20.0 + PAD_X, h);
    (prev, next)
}

fn calendar_click(world: &mut World, w: f64, x: f64, y: f64) -> bool {
    let (prev, next) = calendar_arrows(w);
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
fn calendar(cr: &Context, th: &Theme, world: &World, x: f64, y: f64, w: f64) {
    let h = calendar_height();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_month_arrows_change_shown_month() {
        let now = NaiveDate::from_ymd_opt(2026, 11, 9)
            .unwrap()
            .and_hms_opt(17, 23, 0)
            .unwrap();
        let mut world = World { now, cal_month: 0 };
        let cal = Item {
            module: Module::Calendar,
            look: None,
            rect: None,
        };
        let w = 380.0;
        let h = height(&cal, w);
        let (prev, next) = calendar_arrows(w);

        assert!(!click(&cal, &mut world, (w, h), (40.0, 100.0), 1));
        assert_eq!(world.cal_month, 0);

        assert!(click(
            &cal,
            &mut world,
            (w, h),
            (prev.0 + 4.0, prev.1 + 10.0),
            1
        ));
        assert_eq!(world.cal_month, -1);
        assert_eq!(
            shifted_month(now.date(), world.cal_month),
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
        );

        assert!(click(
            &cal,
            &mut world,
            (w, h),
            (next.0 + 4.0, next.1 + 10.0),
            1
        ));
        assert!(click(
            &cal,
            &mut world,
            (w, h),
            (next.0 + 4.0, next.1 + 10.0),
            1
        ));
        assert!(click(
            &cal,
            &mut world,
            (w, h),
            (next.0 + 4.0, next.1 + 10.0),
            1
        ));
        assert_eq!(world.cal_month, 2);
        assert_eq!(
            shifted_month(now.date(), world.cal_month),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }

    #[test]
    fn clock_with_calendar_look_forwards_arrow_clicks() {
        let now = NaiveDate::from_ymd_opt(2026, 1, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let mut world = World { now, cal_month: 0 };
        let clock_cal = Item {
            module: Module::Clock,
            look: Some(2),
            rect: None,
        };
        let w = 380.0;
        let h = height(&clock_cal, w);
        let (prev, _) = calendar_arrows(w);
        let cal_top = 2.0 * PAD_Y + 86.0 + draw::GAP;

        assert!(click(
            &clock_cal,
            &mut world,
            (w, h),
            (prev.0 + 5.0, cal_top + 12.0),
            1
        ));
        assert_eq!(world.cal_month, -1);
        assert_eq!(
            shifted_month(now.date(), world.cal_month),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
    }
}
