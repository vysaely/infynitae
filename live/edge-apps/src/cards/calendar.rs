//! Calendar, Infynitae's own (no desktop entry): Clock and Month (docs/EDGE_APPS.md section 6),
//! ported from `live/edge-rs/src/modules/clock.rs` and `calendar.rs`. Both read `cx.now`; Month
//! events have no data source yet, so L draws the month only until one exists.

use crate::card::{Action, Button, Card, Cx, Event, Hits, Rect, Source, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::ui;
use cairo::Context;
use chrono::{Datelike, NaiveDate, Timelike, Weekday};
use std::any::Any;

pub struct Calendar;

const KINDS: &[CardKind] = &[
    CardKind {
        id: "clock",
        title: "Clock",
        about: "The time and the date",
        sizes: &[Size::S, Size::M],
        default: Size::S,
        half: true,
        sources: &[],
    },
    CardKind {
        id: "month",
        title: "Month",
        about: "This month's days, previous and next months",
        sizes: &[Size::M, Size::L],
        default: Size::M,
        half: true,
        sources: &[],
    },
];

impl Provider for Calendar {
    fn id(&self) -> &'static str {
        "calendar"
    }
    fn name(&self) -> &'static str {
        "Calendar"
    }
    fn claims(&self) -> &'static [Claim] {
        &[]
    }
    fn listed(&self) -> Listed {
        Listed::Always
    }
    fn cards(&self) -> &'static [CardKind] {
        KINDS
    }
    fn card(&self, kind: &str, argument: Option<&str>) -> Option<Box<dyn Card>> {
        if argument.is_some() {
            return None;
        }
        match kind {
            "clock" => Some(Box::new(ClockCard)),
            "month" => Some(Box::new(MonthCard::default())),
            _ => None,
        }
    }
    fn source(&self, _key: &str) -> Option<Box<dyn Source>> {
        None
    }
}

// ---------------------------------------------------------------------------------------------
// Clock

const CLOCK_S_H: f64 = 40.0;
const CLOCK_M_H: f64 = 92.0;

#[derive(Default)]
pub struct ClockCard;

impl Card for ClockCard {
    fn apply(&mut self, _key: &str, _data: &dyn Any) -> bool {
        false
    }
    fn status(&self, _cx: &Cx) -> Status {
        Status::Ready
    }
    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        match cx.size {
            Size::S => CLOCK_S_H,
            Size::M | Size::L => CLOCK_M_H,
        }
    }
    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, _hits: &mut Hits) {
        let th = cx.theme;
        let x = body.0 + 8.0;
        let x_right = body.0 + body.2 - 8.0;
        let inner_w = (body.2 - 16.0).max(20.0);
        let time = format!("{:02}:{:02}", cx.now.hour(), cx.now.minute());
        let date = cx.now.format("%A, %-d %B").to_string();

        let measure_bold = |size: f64| {
            draw::measure(
                cr,
                &Text {
                    size,
                    bold: true,
                    color: th.text,
                    text: &time,
                },
            )
        };

        if cx.size == Size::S {
            let mut size = 28.0;
            let (mut tw, mut th_px) = measure_bold(size);
            if tw > inner_w {
                size = (size * inner_w / tw).max(14.0);
                (tw, th_px) = measure_bold(size);
            }
            let ty = (body.1 + (body.3 - th_px) / 2.0).round();
            draw::text(
                cr,
                x,
                ty,
                &Text {
                    size,
                    bold: true,
                    color: th.text,
                    text: &time,
                },
            );
            let room = inner_w - tw - 12.0;
            if room > 40.0 {
                let ds = 13.0;
                let muted = |s| Text {
                    size: ds,
                    bold: false,
                    color: th.muted,
                    text: s,
                };
                let (dw, dh) = draw::measure(cr, &muted(&date));
                let dy = (body.1 + (body.3 - dh) / 2.0 + 1.0).round();
                if dw <= room {
                    draw::text_right(cr, x_right, dy, &muted(&date));
                } else {
                    let short = cx.now.format("%a %-d %b").to_string();
                    if draw::measure(cr, &muted(&short)).0 <= room {
                        draw::text_right(cr, x_right, dy, &muted(&short));
                    } else {
                        draw::text_max(cr, x + tw + 12.0, dy, room, &muted(&short));
                    }
                }
            }
            return;
        }

        let mut size = 64.0;
        let (tw, _) = measure_bold(size);
        if tw > inner_w {
            size = (size * inner_w / tw).max(20.0);
        }
        let (_, dh) = draw::measure(
            cr,
            &Text {
                size: 15.0,
                bold: false,
                color: th.muted,
                text: &date,
            },
        );
        let date_y = body.1 + body.3 - dh;
        let time_y = body.1.max(date_y - size * 1.12);
        draw::text(
            cr,
            x,
            time_y,
            &Text {
                size,
                bold: true,
                color: th.text,
                text: &time,
            },
        );
        draw::text_max(
            cr,
            x,
            date_y,
            inner_w,
            &Text {
                size: 15.0,
                bold: false,
                color: th.muted,
                text: &date,
            },
        );
    }
    fn event(&mut self, _ev: &Event, _cx: &Cx, _out: &mut Vec<Action>) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------------------------
// Month

const NAV_H: f64 = 24.0;
const NAV_GAP: f64 = 4.0;
const WEEK_H: f64 = 20.0;
const DAY_ROW_H: f64 = 30.0;
const MONTH_H: f64 = NAV_H + NAV_GAP + WEEK_H + 6.0 * DAY_ROW_H;

const H_PREV: u32 = 0;
const H_NEXT: u32 = 1;
const H_TODAY: u32 = 2;

/// The first day of the month `delta` months away from `today`'s month.
pub fn shifted_month(today: NaiveDate, delta: i32) -> NaiveDate {
    let total = today.year() * 12 + (today.month() as i32 - 1) + delta;
    let y = total.div_euclid(12);
    let m = (total.rem_euclid(12) + 1) as u32;
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(today)
}

/// The first day of the displayed month and the 42 dates of its six Monday-first weeks.
pub fn month_grid(today: NaiveDate, month_offset: i32) -> (NaiveDate, [NaiveDate; 42]) {
    let first = shifted_month(today, month_offset);
    let start = first - chrono::Duration::days(first.weekday().num_days_from_monday() as i64);
    let days = std::array::from_fn(|i| start + chrono::Duration::days(i as i64));
    (first, days)
}

#[derive(Default)]
pub struct MonthCard {
    pub month_offset: i32,
}

impl MonthCard {
    fn nav_rects(body: Rect) -> (Rect, Rect) {
        let x_right = body.0 + body.2 - 8.0;
        let prev = (x_right - 50.0, body.1, 24.0, NAV_H);
        let next = (x_right - 24.0, body.1, 24.0, NAV_H);
        (prev, next)
    }
}

fn draw_chevron(cr: &Context, r: Rect, forward: bool, color: crate::theme::Rgba) {
    let cx = r.0 + r.2 / 2.0;
    let cy = r.1 + r.3 / 2.0;
    let dx = if forward { 2.5 } else { -2.5 };
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(cx - dx, cy - 4.5);
    cr.line_to(cx + dx, cy);
    cr.line_to(cx - dx, cy + 4.5);
    let _ = cr.stroke();
}

impl Card for MonthCard {
    fn apply(&mut self, _key: &str, _data: &dyn Any) -> bool {
        false
    }
    fn status(&self, _cx: &Cx) -> Status {
        Status::Ready
    }
    fn title(&self, cx: &Cx) -> Option<String> {
        let first = shifted_month(cx.now.date(), self.month_offset);
        Some(first.format("%B %Y").to_string())
    }
    fn height(&self, _cx: &Cx, _width: f64) -> f64 {
        MONTH_H
    }
    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let th = cx.theme;
        let x = body.0 + 8.0;
        let inner_w = (body.2 - 16.0).max(20.0);
        let (prev_r, next_r) = Self::nav_rects(body);

        if self.month_offset != 0 {
            let label = "Today";
            let (lw, lh) = draw::measure(
                cr,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: label,
                },
            );
            let avail = (prev_r.0 - x - 6.0).max(0.0);
            let tw = (lw + 14.0).min(avail);
            if tw >= 24.0 {
                let today_r = (x, body.1, tw, NAV_H);
                let lit = hits.add(H_TODAY, today_r) || hits.selected(H_TODAY);
                if lit {
                    ui::light(cr, cx, today_r);
                }
                draw::text_center(
                    cr,
                    today_r.0 + today_r.2 / 2.0,
                    today_r.1 + (NAV_H - lh) / 2.0,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: if lit { th.text } else { th.muted },
                        text: label,
                    },
                );
            }
        }

        for (id, r, forward) in [(H_PREV, prev_r, false), (H_NEXT, next_r, true)] {
            let lit = hits.add(id, r) || hits.selected(id);
            if lit {
                ui::light(cr, cx, r);
            }
            draw_chevron(cr, r, forward, if lit { th.text } else { th.muted });
        }

        let col = inner_w / 7.0;
        let week_y = body.1 + NAV_H + NAV_GAP;
        let labels = if col >= 26.0 {
            ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
        } else {
            ["M", "T", "W", "T", "F", "S", "S"]
        };
        for (i, d) in labels.iter().enumerate() {
            let cx_day = (x + col * (i as f64 + 0.5)).round();
            draw::text_center(
                cr,
                cx_day,
                week_y + 2.0,
                &Text {
                    size: 11.0,
                    bold: false,
                    color: th.muted,
                    text: d,
                },
            );
        }

        let today = cx.now.date();
        let (first, days) = month_grid(today, self.month_offset);
        let grid_y = week_y + WEEK_H;
        let box_size = (DAY_ROW_H - 4.0).min(col - 2.0).max(16.0);
        let day_font = if col < 24.0 { 11.5 } else { 13.0 };

        for (i, &day) in days.iter().enumerate() {
            let cx_day = (x + col * ((i % 7) as f64 + 0.5)).round();
            let ry = grid_y + (i / 7) as f64 * DAY_ROW_H;
            let s = day.day().to_string();
            let is_today = day == today;
            let color = if is_today {
                let bx = cx_day - box_size / 2.0;
                let by = ry + (DAY_ROW_H - box_size) / 2.0;
                draw::round_rect(
                    cr,
                    bx,
                    by,
                    box_size,
                    box_size,
                    th.small_radius().min(box_size / 2.0),
                );
                draw::set(cr, th.accent);
                let _ = cr.fill();
                th.on_accent
            } else if day.month() != first.month() {
                th.text.alpha(0.28)
            } else if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                th.muted
            } else {
                th.text
            };
            let (_, dh) = draw::measure(
                cr,
                &Text {
                    size: day_font,
                    bold: is_today,
                    color,
                    text: &s,
                },
            );
            draw::text_center(
                cr,
                cx_day,
                ry + (DAY_ROW_H - dh) / 2.0,
                &Text {
                    size: day_font,
                    bold: is_today,
                    color,
                    text: &s,
                },
            );
        }
    }
    fn event(&mut self, ev: &Event, _cx: &Cx, _out: &mut Vec<Action>) -> bool {
        let Event::Press {
            hit,
            button: Button::Left,
            ..
        } = ev
        else {
            return false;
        };
        match *hit {
            H_PREV => {
                self.month_offset = self.month_offset.saturating_sub(1);
                true
            }
            H_NEXT => {
                self.month_offset = self.month_offset.saturating_add(1);
                true
            }
            H_TODAY if self.month_offset != 0 => {
                self.month_offset = 0;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider;

    fn test_cx(th: &crate::theme::Theme, size: Size) -> Cx<'_> {
        Cx {
            theme: th,
            now: NaiveDate::from_ymd_opt(2026, 11, 13)
                .unwrap()
                .and_hms_opt(12, 34, 0)
                .unwrap(),
            size,
            app: None,
            apps: &[],
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
        }
    }

    #[test]
    fn provider_metadata_and_resolution_match_the_spec() {
        let (p, app) = provider::resolve("calendar", &[]).unwrap();
        assert_eq!(p.id(), "calendar");
        assert_eq!(p.name(), "Calendar");
        assert_eq!(p.listed(), Listed::Always);
        assert!(p.claims().is_empty());
        assert!(app.is_none());

        let clock = p.kind("clock").unwrap();
        assert_eq!(clock.sizes, &[Size::S, Size::M]);
        assert_eq!(clock.default, Size::S);
        assert!(clock.half);
        assert!(clock.sources.is_empty());

        let month = p.kind("month").unwrap();
        assert_eq!(month.sizes, &[Size::M, Size::L]);
        assert_eq!(month.default, Size::M);
        assert!(month.half);
        assert!(month.sources.is_empty());

        assert!(p.card("clock", None).is_some());
        assert!(p.card("month", None).is_some());
        assert!(p.card("clock", Some("arg")).is_none());
        assert!(p.card("unknown", None).is_none());
        assert!(p.source("anything").is_none());
    }

    #[test]
    fn shifted_month_steps_across_years() {
        let nov = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        assert_eq!(
            shifted_month(nov, 0),
            NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()
        );
        assert_eq!(
            shifted_month(nov, -1),
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
        );
        assert_eq!(
            shifted_month(nov, 2),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );

        let jan = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        assert_eq!(
            shifted_month(jan, -1),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
        assert_eq!(
            shifted_month(jan, -13),
            NaiveDate::from_ymd_opt(2024, 12, 1).unwrap()
        );
    }

    #[test]
    fn month_grid_starts_monday_and_covers_six_weeks() {
        let nov_13 = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        // 2026-11-01 is a Sunday, so the Monday-first grid starts on 2026-10-26.
        let (first, days) = month_grid(nov_13, 0);
        assert_eq!(first, NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
        assert_eq!(days[0], NaiveDate::from_ymd_opt(2026, 10, 26).unwrap());
        assert_eq!(days[0].weekday(), Weekday::Mon);
        assert_eq!(days[6], first);
        assert_eq!(days[41], NaiveDate::from_ymd_opt(2026, 12, 6).unwrap());
    }

    #[test]
    fn month_arrows_and_today_reset_change_shown_month() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th, Size::M);
        let mut card = MonthCard::default();

        assert_eq!(card.status(&cx), Status::Ready);
        assert!(!card.apply("anything", &() as &dyn Any));
        assert_eq!(card.title(&cx).as_deref(), Some("November 2026"));

        let mut out = Vec::new();
        // Clicking H_TODAY while already on this month does nothing.
        assert!(!card.event(
            &Event::Press {
                hit: H_TODAY,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx,
            &mut out
        ));
        assert_eq!(card.month_offset, 0);
        assert!(out.is_empty());

        assert!(card.event(
            &Event::Press {
                hit: H_PREV,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx,
            &mut out
        ));
        assert_eq!(card.month_offset, -1);
        assert_eq!(card.title(&cx).as_deref(), Some("October 2026"));

        for _ in 0..3 {
            assert!(card.event(
                &Event::Press {
                    hit: H_NEXT,
                    button: Button::Left,
                    at: (0.0, 0.0)
                },
                &cx,
                &mut out
            ));
        }
        assert_eq!(card.month_offset, 2);
        assert_eq!(card.title(&cx).as_deref(), Some("January 2027"));

        // Right click does nothing.
        assert!(!card.event(
            &Event::Press {
                hit: H_NEXT,
                button: Button::Right,
                at: (0.0, 0.0)
            },
            &cx,
            &mut out
        ));
        assert_eq!(card.month_offset, 2);

        // Clicking H_TODAY goes back to the current month.
        assert!(card.event(
            &Event::Press {
                hit: H_TODAY,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx,
            &mut out
        ));
        assert_eq!(card.month_offset, 0);
        assert_eq!(card.title(&cx).as_deref(), Some("November 2026"));
        assert!(out.is_empty());
    }

    #[test]
    fn month_draw_registers_hits_within_body_height_at_both_sizes_and_widths() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 300).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();

        for size in [Size::M, Size::L] {
            let cx = test_cx(&th, size);
            for width in [360.0, 164.0] {
                let mut card = MonthCard::default();
                let h = card.height(&cx, width);
                assert_eq!(h, MONTH_H);
                let body = (10.0, 40.0, width, h);

                let mut hits = Hits::default();
                card.draw(&cr, &cx, body, &mut hits);
                assert_eq!(
                    hits.list.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                    vec![H_PREV, H_NEXT],
                    "no Today hit when already on current month"
                );
                for (_, r) in &hits.list {
                    assert!(r.0 >= body.0 && r.0 + r.2 <= body.0 + body.2);
                    assert!(r.1 >= body.1 && r.1 + r.3 <= body.1 + h);
                }

                card.month_offset = 1;
                let mut shifted_hits = Hits::default();
                card.draw(&cr, &cx, body, &mut shifted_hits);
                assert_eq!(
                    shifted_hits
                        .list
                        .iter()
                        .map(|(id, _)| *id)
                        .collect::<Vec<_>>(),
                    vec![H_TODAY, H_PREV, H_NEXT]
                );
                for (_, r) in &shifted_hits.list {
                    assert!(r.0 >= body.0 && r.0 + r.2 <= body.0 + body.2);
                    assert!(r.1 >= body.1 && r.1 + r.3 <= body.1 + h);
                }
            }
        }
    }

    #[test]
    fn clock_renders_both_sizes_and_widths_without_hits_or_actions() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 200).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();

        for (size, want_h) in [(Size::S, CLOCK_S_H), (Size::M, CLOCK_M_H)] {
            let cx = test_cx(&th, size);
            let mut card = ClockCard;
            assert_eq!(card.status(&cx), Status::Ready);
            assert!(!card.apply("clock", &() as &dyn Any));
            assert!(card.title(&cx).is_none());

            for width in [360.0, 164.0, 90.0] {
                let h = card.height(&cx, width);
                assert_eq!(h, want_h);
                let mut hits = Hits::default();
                card.draw(&cr, &cx, (10.0, 40.0, width, h), &mut hits);
                assert!(hits.list.is_empty());
            }

            let mut out = Vec::new();
            assert!(!card.event(
                &Event::Press {
                    hit: 0,
                    button: Button::Left,
                    at: (0.0, 0.0)
                },
                &cx,
                &mut out
            ));
            assert!(out.is_empty());
        }
    }

    #[test]
    fn shell_resolves_old_words_halves_and_clicks_month_hits() {
        let cfg = crate::config::Config::from_text(
            "PANEL_CONTROL=\"SUPER+A top right 380 clock@half calendar@half\"\n",
        );
        let mut sh = crate::shell::Shell::new(cfg, Vec::new());
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        sh.now = NaiveDate::from_ymd_opt(2026, 11, 13)
            .unwrap()
            .and_hms_opt(14, 25, 0)
            .unwrap();
        assert_eq!(sh.panels[idx].cards.len(), 2);

        let ((_, _, pw, _), rects) = sh.layout(idx, 1920.0, 1080.0);
        assert_eq!(pw, 380.0);
        assert!(
            rects[0].1 .2 < 200.0 && rects[1].1 .2 < 200.0,
            "both cards allow half width"
        );
        assert_eq!(rects[0].1 .1, rects[1].1 .1, "paired on the same row");

        assert!(sh.open_panel(idx, false, std::time::Instant::now()));
        sh.open_ms = f64::INFINITY;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1920, 1080).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1920.0, 1080.0);

        let hit_rect = |sh: &crate::shell::Shell, id: u32| -> Rect {
            let f = sh.frame.borrow();
            f[1].2.list.iter().find(|(hid, _)| *hid == id).unwrap().1
        };

        let next_r = hit_rect(&sh, H_NEXT);
        assert!(sh.click(1920.0, 1080.0, next_r.0 + 4.0, next_r.1 + 4.0, Button::Left));
        assert!(sh.actions.is_empty());
        let cx = sh.cx(Size::M, None, false);
        assert_eq!(
            sh.panels[idx].cards[1].card.title(&cx).as_deref(),
            Some("December 2026")
        );

        sh.draw(&cr, 1920.0, 1080.0);
        let today_r = hit_rect(&sh, H_TODAY);
        assert!(sh.click(
            1920.0,
            1080.0,
            today_r.0 + 4.0,
            today_r.1 + 4.0,
            Button::Left
        ));
        let cx = sh.cx(Size::M, None, false);
        assert_eq!(
            sh.panels[idx].cards[1].card.title(&cx).as_deref(),
            Some("November 2026")
        );
    }
}
