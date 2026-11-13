//! Notifications, Infynitae's own: the history the edge's notification server keeps (`cx.notes`,
//! newest first; `crate::notify`). One app's notifications in a row stack under one name with
//! their count; a click opens one (its default action) and it goes, its x (on hover) dismisses it,
//! "Clear all" dismisses every one. With none the card is not shown. Ported from
//! live/edge-rs/src/modules/notifications.rs.

use crate::card::{Action, Button, Card, Cx, Event, Hits, Note, Rect, Source, Status};
use crate::draw::{self, Text, FONT};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::ui;
use cairo::Context;
use std::any::Any;

pub struct Notifications;

const KINDS: &[CardKind] = &[CardKind {
    id: "list",
    title: "Notifications",
    about: "What apps told you, newest first: open one, dismiss one, or clear all",
    sizes: &[Size::M, Size::L],
    default: Size::M,
    half: false,
    sources: &[],
}];

impl Provider for Notifications {
    fn id(&self) -> &'static str {
        "notifications"
    }
    fn name(&self) -> &'static str {
        "Notifications"
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
        (kind == "list" && argument.is_none()).then(|| Box::new(ListCard) as Box<dyn Card>)
    }
    fn source(&self, _: &str) -> Option<Box<dyn Source>> {
        None
    }
}

/// Hits: Clear all, then one per notification shown and one per x.
const H_CLEAR: u32 = 1;
const H_NOTE: u32 = 100;
const H_X: u32 = 1000;

/// How many show at each size (whole notifications only; the rest wait for room).
fn shown(cx: &Cx) -> usize {
    let most = match cx.size {
        Size::L => 8,
        _ => 4,
    };
    cx.notes.len().min(most)
}

/// The app's line above a notification's title.
const APP_LINE: f64 = 16.0;
const GAP: f64 = 10.0;
/// The "Clear all" button's line under the last one.
const FOOT_H: f64 = 34.0;

fn body_lines(body: &str) -> usize {
    if body.is_empty() {
        0
    } else if body.contains('\n') || body.chars().count() > 48 {
        2
    } else {
        1
    }
}

/// The first of a run from one app carries the app's name.
fn shows_app(notes: &[Note], i: usize) -> bool {
    i == 0 || notes[i - 1].app != notes[i].app
}

/// How many in a row come from the app of `i`.
fn run_len(notes: &[Note], i: usize) -> usize {
    notes[i..]
        .iter()
        .take_while(|n| n.app == notes[i].app)
        .count()
}

fn note_h(notes: &[Note], i: usize) -> f64 {
    let base = match body_lines(&notes[i].body) {
        0 => 36.0,
        1 => 54.0,
        _ => 70.0,
    };
    base - if shows_app(notes, i) { 0.0 } else { APP_LINE }
}

/// The x of a notification whose block starts at `y` in a body at `x` of width `w`.
fn x_rect(x: f64, w: f64, y: f64) -> Rect {
    (x + w - 20.0, y - 3.0, 20.0, 20.0)
}

fn two_lines(cr: &Context, x: f64, y: f64, max_w: f64, t: &Text) {
    let l = pangocairo::functions::create_layout(cr);
    let mut fd = pango::FontDescription::from_string(FONT);
    fd.set_absolute_size(t.size * pango::SCALE as f64);
    l.set_font_description(Some(&fd));
    l.set_width((max_w.max(1.0) * pango::SCALE as f64) as i32);
    l.set_wrap(pango::WrapMode::WordChar);
    l.set_ellipsize(pango::EllipsizeMode::End);
    l.set_height(-2);
    l.set_text(t.text);
    draw::set(cr, t.color);
    cr.move_to(x.round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
}

fn draw_x(cr: &Context, cx: &Cx, r: Rect, lit: bool) {
    let (mx, my) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    draw::set(cr, if lit { cx.theme.text } else { cx.theme.muted });
    cr.set_line_width(1.5);
    cr.move_to(mx - 4.0, my - 4.0);
    cr.line_to(mx + 4.0, my + 4.0);
    cr.move_to(mx + 4.0, my - 4.0);
    cr.line_to(mx - 4.0, my + 4.0);
    let _ = cr.stroke();
}

pub struct ListCard;

impl Card for ListCard {
    fn apply(&mut self, _: &str, _: &dyn Any) -> bool {
        false
    }

    fn status(&self, cx: &Cx) -> Status {
        if cx.notes.is_empty() {
            Status::Hidden
        } else {
            Status::Ready
        }
    }

    fn title(&self, cx: &Cx) -> Option<String> {
        let more = cx.notes.len().saturating_sub(shown(cx));
        (more > 0).then(|| format!("Notifications \u{b7} {} more", more))
    }

    fn height(&self, cx: &Cx, _: f64) -> f64 {
        let n = shown(cx);
        if n == 0 {
            return 0.0;
        }
        (0..n).map(|i| note_h(cx.notes, i)).sum::<f64>() + (n - 1) as f64 * GAP + FOOT_H
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        // the body keeps 8 px each side for the lights; the text lines up with the header's
        let (x, mut y, w) = (body.0 + 8.0, body.1, body.2 - 16.0);
        let th = cx.theme;
        let notes = cx.notes;
        for i in 0..shown(cx) {
            let note = &notes[i];
            let h = note_h(notes, i);
            let head = shows_app(notes, i);
            let block = (x - 8.0, y - 5.0, w + 16.0, h + GAP);
            let id = i as u32;
            let pointed = hits.add(H_NOTE + id, block);
            if pointed || hits.selected(H_NOTE + id) {
                ui::light(cr, cx, block);
            }
            // its x on top of the block: under the pointer it is lit, not the notification
            let xr = x_rect(x, w, y);
            let x_lit = hits.add(H_X + id, xr);
            let time_w = if pointed || x_lit || hits.selected(H_X + id) {
                draw_x(cr, cx, xr, x_lit || hits.selected(H_X + id));
                24.0
            } else if !note.time.is_empty() {
                draw::text_right(
                    cr,
                    x + w,
                    y,
                    &Text {
                        size: 11.0,
                        bold: false,
                        color: th.muted,
                        text: &note.time,
                    },
                ) + 8.0
            } else {
                0.0
            };
            if head {
                let run = run_len(notes, i);
                let line = if run > 1 {
                    format!("{} \u{b7} {run}", note.app)
                } else {
                    note.app.clone()
                };
                draw::text_max(
                    cr,
                    x,
                    y,
                    (w - time_w).max(20.0),
                    &Text {
                        size: 11.0,
                        bold: false,
                        color: th.muted,
                        text: &line,
                    },
                );
            }
            // inside a run the title takes the app line's place
            let ty = if head { y } else { y - APP_LINE };
            let title_w = if head { w } else { (w - time_w).max(20.0) };
            draw::text_max(
                cr,
                x,
                ty + 16.0,
                title_w,
                &Text {
                    size: 13.0,
                    bold: true,
                    color: th.text,
                    text: &note.summary,
                },
            );
            if !note.body.is_empty() {
                two_lines(
                    cr,
                    x,
                    ty + 36.0,
                    w,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: th.muted,
                        text: &note.body,
                    },
                );
            }
            y += h + GAP;
        }
        // "Clear all" at the bottom right, under the last one
        let t = Text {
            size: 12.0,
            bold: false,
            color: th.text,
            text: "Clear all",
        };
        let bw = draw::measure(cr, &t).0 + 18.0;
        ui::button(
            cr,
            cx,
            hits,
            H_CLEAR,
            x + w - bw,
            y - GAP + 8.0,
            "Clear all",
        );
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Event::Press {
            hit,
            button: Button::Left,
            ..
        } = ev
        else {
            return false;
        };
        let n = shown(cx);
        match *hit {
            H_CLEAR => {
                out.extend(cx.notes.iter().map(|note| Action::Dismiss(note.id)));
                true
            }
            h if (H_X..H_X + n as u32).contains(&h) => {
                out.push(Action::Dismiss(cx.notes[(h - H_X) as usize].id));
                true
            }
            h if (H_NOTE..H_NOTE + n as u32).contains(&h) => {
                // opened: its default action (if it has one), and it goes unless resident
                out.push(Action::Invoke(
                    cx.notes[(h - H_NOTE) as usize].id,
                    "default".into(),
                ));
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::theme::Theme;

    fn note(id: u32, app: &str, body: &str) -> Note {
        Note {
            id,
            app: app.into(),
            summary: format!("Title {id}"),
            body: body.into(),
            actions: vec![],
            urgency: 1,
            time: "12:00".into(),
        }
    }

    fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    #[test]
    fn hidden_with_none_and_ready_with_some() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        assert_eq!(ListCard.status(&cx), Status::Hidden);
        assert_eq!(ListCard.height(&cx, 300.0), 0.0);
        let notes = [note(1, "Firefox", "")];
        let cx = Cx {
            notes: &notes,
            ..test_cx(&th)
        };
        assert_eq!(ListCard.status(&cx), Status::Ready);
    }

    #[test]
    fn one_apps_run_stacks_under_one_name() {
        let notes = [
            note(3, "Discord", "hi"),
            note(2, "Discord", ""),
            note(1, "Mail", ""),
        ];
        assert!(shows_app(&notes, 0));
        assert!(!shows_app(&notes, 1));
        assert!(shows_app(&notes, 2));
        assert_eq!(run_len(&notes, 0), 2);
        // the second in the run has no app line
        assert_eq!(note_h(&notes, 0), 54.0);
        assert_eq!(note_h(&notes, 1), 36.0 - APP_LINE);
    }

    #[test]
    fn clicks_open_dismiss_and_clear_by_id() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let notes = [note(7, "A", ""), note(5, "B", "")];
        let cx = Cx {
            notes: &notes,
            ..test_cx(&th)
        };
        let mut out = Vec::new();
        assert!(ListCard.event(&press(H_NOTE + 1), &cx, &mut out));
        assert_eq!(out, vec![Action::Invoke(5, "default".into())]);
        out.clear();
        assert!(ListCard.event(&press(H_X), &cx, &mut out));
        assert_eq!(out, vec![Action::Dismiss(7)]);
        out.clear();
        assert!(ListCard.event(&press(H_CLEAR), &cx, &mut out));
        assert_eq!(out, vec![Action::Dismiss(7), Action::Dismiss(5)]);
        out.clear();
        // a hit past the shown ones does nothing
        assert!(!ListCard.event(&press(H_NOTE + 2), &cx, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn size_limits_how_many_and_the_title_counts_the_rest() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let notes: Vec<Note> = (0..6).map(|i| note(i, &format!("App{i}"), "")).collect();
        let cx = Cx {
            notes: &notes,
            ..test_cx(&th)
        };
        assert_eq!(shown(&cx), 4);
        assert_eq!(
            ListCard.title(&cx).as_deref(),
            Some("Notifications \u{b7} 2 more")
        );
        let cx = Cx {
            notes: &notes,
            size: Size::L,
            ..test_cx(&th)
        };
        assert_eq!(shown(&cx), 6);
        assert_eq!(ListCard.title(&cx), None);
        assert_eq!(ListCard.height(&cx, 300.0), 6.0 * 36.0 + 5.0 * GAP + FOOT_H);
    }

    #[test]
    fn draw_adds_a_hit_for_each_shown_and_clear_all() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let notes = [note(1, "A", "a body"), note(2, "A", "")];
        let cx = Cx {
            notes: &notes,
            ..test_cx(&th)
        };
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 300).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        ListCard.draw(&cr, &cx, (10.0, 10.0, 320.0, 200.0), &mut hits);
        let ids: Vec<u32> = hits.list.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![H_NOTE, H_X, H_NOTE + 1, H_X + 1, H_CLEAR]);
    }
}
