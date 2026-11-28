//! The desktop's widgets (`WIDGETS`, docs/EDGE_APPS.md section 5): cards at their places in screen
//! pixels on a surface of their own under the windows, drawn and fed as a panel's cards are (the
//! card trait, `shell::draw_card`, their sources). Always shown while the line has a widget; not
//! edited here (the line is edited in the config). Plain state, as `shell.rs`.

use crate::app::App;
use crate::card::{Action, Button, Cx, Event, Hits, Rect, Status};
use crate::config::Config;
use crate::shell::{self, Placed, Shell, HEAD_H, PAD_BOTTOM, PAD_TOP};
use std::cell::RefCell;
use std::time::Instant;

/// A card on the desktop and its box.
pub struct Widget {
    pub rect: Rect,
    pub placed: Placed,
}

#[derive(Default)]
pub struct Widgets {
    pub list: Vec<Widget>,
    /// the pointer on the widgets' surface
    pub pointer: Option<(f64, f64)>,
    /// the widget and hit a button or finger went down on (drags and the release go to it)
    pressed: Option<(usize, u32)>,
    /// each drawn widget's hits in the last frame
    frame: RefCell<Vec<(usize, Hits)>>,
    last_tick: Option<Instant>,
}

/// The widgets of `cfg`'s WIDGETS line. A card whose app is not installed is left out: its
/// Remove button would have no panel to remove it from.
pub fn place(cfg: &Config, apps: &[App]) -> Widgets {
    let Some(p) = &cfg.widgets else {
        return Widgets::default();
    };
    let list = crate::panel::widget_cards(p, &cfg.favorites)
        .into_iter()
        .filter_map(|w| {
            let (x, y, pw, ph) = w.place?;
            let placed = shell::place_one(w, apps);
            (!placed.provider.id().is_empty()).then_some(Widget {
                rect: (x as f64, y as f64, pw as f64, ph as f64),
                placed,
            })
        })
        .collect();
    Widgets {
        list,
        ..Default::default()
    }
}

impl Shell {
    /// A widget's view of the edge: never edited, its lists open in place, its body's room the
    /// box it was given.
    fn widget_cx(&self, w: &Widget) -> Cx<'_> {
        let pc = &w.placed;
        let cx = Cx {
            editing: false,
            ..self.cx(pc.size, pc.app_id.as_deref(), false)
        };
        let head = if pc.card.header(&cx) { HEAD_H } else { 0.0 };
        Cx {
            room: (w.rect.3 - PAD_TOP - head - PAD_BOTTOM).max(0.0),
            ..cx
        }
    }

    /// The widgets that show something now (a card with nothing to show hides, as in a panel).
    fn shown_widgets(&self) -> impl Iterator<Item = (usize, &Widget, Cx<'_>, Status)> {
        self.widgets.list.iter().enumerate().filter_map(|(i, w)| {
            let cx = self.widget_cx(w);
            let status = w.placed.card.status(&cx);
            (status != Status::Hidden).then_some((i, w, cx, status))
        })
    }

    /// Draws every shown widget at its place, keeping their hits for the pointer.
    pub fn draw_widgets(&self, cr: &cairo::Context) {
        self.drawn_private.borrow_mut().clear();
        crate::draw::with_font(&self.theme.font, || {
            let mut frame = Vec::new();
            for (i, w, cx, status) in self.shown_widgets() {
                let mut hits = Hits::new(self.widgets.pointer, None);
                // a list opened in place stays inside the widget's box
                cr.save().ok();
                cr.rectangle(w.rect.0, w.rect.1, w.rect.2, w.rect.3);
                cr.clip();
                shell::draw_card(cr, &self.theme, &w.placed, &cx, status, w.rect, &mut hits);
                cr.restore().ok();
                self.drawn_private
                    .borrow_mut()
                    .extend_from_slice(&hits.private);
                frame.push((i, hits));
            }
            *self.widgets.frame.borrow_mut() = frame;
        })
    }

    /// The input region of the widgets' surface: the shown widgets' boxes on whole pixels; a click
    /// anywhere else is the desktop's.
    pub fn widget_region(&self) -> Vec<(i32, i32, i32, i32)> {
        self.shown_widgets()
            .map(|(_, w, _, _)| {
                let r = w.rect;
                let (x0, y0) = (r.0.floor() as i32, r.1.floor() as i32);
                (
                    x0,
                    y0,
                    (r.0 + r.2).ceil() as i32 - x0,
                    (r.1 + r.3).ceil() as i32 - y0,
                )
            })
            .collect()
    }

    /// The (provider, source key) pairs the widgets' cards need: fetched while they are shown,
    /// whether a panel is open or not.
    pub fn widget_sources(&self) -> Vec<(&'static str, &'static str)> {
        let mut out: Vec<(&'static str, &'static str)> = Vec::new();
        for w in &self.widgets.list {
            let pc = &w.placed;
            for k in pc.kind.sources {
                if !out.contains(&(pc.provider.id(), *k)) {
                    out.push((pc.provider.id(), k));
                }
            }
        }
        out
    }

    pub fn widgets_want_clock(&self) -> bool {
        self.widgets
            .list
            .iter()
            .any(|w| w.placed.card.wants_clock())
    }

    /// Moves the widgets' animations on (a list opening). True while one runs.
    pub fn widgets_tick(&mut self, now: Instant) -> bool {
        let dt = self
            .widgets
            .last_tick
            .map_or(0.0, |t| now.duration_since(t).as_secs_f64() * 1000.0)
            .min(50.0);
        let mut running = false;
        for w in &mut self.widgets.list {
            running |= w.placed.card.tick(dt);
        }
        self.widgets.last_tick = running.then_some(now);
        running
    }

    /// Tests: each drawn widget's hits in the last frame.
    #[cfg(test)]
    pub fn widget_frame(&self) -> Vec<(usize, Hits)> {
        self.widgets.frame.borrow().clone()
    }

    fn widget_hit_at(&self, p: Option<(f64, f64)>) -> Option<(usize, u32)> {
        let p = p?;
        self.widgets
            .frame
            .borrow()
            .iter()
            .rev()
            .find_map(|(i, hits)| hits.top_at(p).map(|h| (*i, h)))
    }

    /// The pointer moved on the widgets: a drag goes to the hit it started on; true when what is
    /// lit changed.
    pub fn widget_motion(&mut self, x: f64, y: f64) -> bool {
        let before = self.widget_hit_at(self.widgets.pointer);
        self.widgets.pointer = Some((x, y));
        if let Some((i, hit)) = self.widgets.pressed {
            return self.widget_event(i, &Event::Drag { hit, at: (x, y) });
        }
        self.widget_hit_at(Some((x, y))) != before
    }

    pub fn widget_leave(&mut self) -> bool {
        let lit = self.widget_hit_at(self.widgets.pointer).is_some();
        self.widgets.pointer = None;
        lit
    }

    /// A button (or a finger, as the left button) went down on the widgets.
    pub fn widget_press(&mut self, x: f64, y: f64, button: Button) -> bool {
        let Some((i, hit)) = self.widget_hit_at(Some((x, y))) else {
            return false;
        };
        if button == Button::Left {
            self.widgets.pressed = Some((i, hit));
        }
        self.widget_event(
            i,
            &Event::Press {
                hit,
                button,
                at: (x, y),
            },
        )
    }

    pub fn widget_release(&mut self) -> bool {
        match self.widgets.pressed.take() {
            Some((i, hit)) => self.widget_event(i, &Event::Release { hit }),
            None => false,
        }
    }

    /// The compositor took a touch sequence back.
    pub fn widget_cancel(&mut self) -> bool {
        match self.widgets.pressed.take() {
            Some((i, hit)) => self.widget_event(i, &Event::Cancel { hit }),
            None => false,
        }
    }

    fn widget_event(&mut self, i: usize, ev: &Event) -> bool {
        let room = self.widget_cx(&self.widgets.list[i]).room;
        let w = &mut self.widgets.list[i];
        let mut out = Vec::new();
        let changed = {
            let pc = &w.placed;
            let cx = Cx {
                theme: &self.theme,
                now: self.now,
                size: pc.size,
                app: pc
                    .app_id
                    .as_deref()
                    .and_then(|id| self.apps.iter().find(|a| a.id == id)),
                apps: &self.apps,
                recent: &self.recent,
                windows: &self.windows,
                notes: &self.notes,
                list_above: false,
                editing: false,
                room,
            };
            w.placed.card.event(ev, &cx, &mut out)
        };
        let pc = &w.placed;
        let (prov, keys) = (pc.provider.id(), pc.kind.sources);
        let card = format!("{}/{}", pc.word.owner, pc.word.kind);
        for a in out {
            match a {
                // a widget has no panel to show cards in or to close, no keyboard to type a
                // query with, and is not removed from here (its card is always installed)
                Action::ShowCards(_)
                | Action::ClosePanel
                | Action::SetQuery(_)
                | Action::RemoveCard => {}
                a => self.actions.push((a, card.clone(), prov, keys)),
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Note, Window};

    fn shell(text: &str) -> Shell {
        Shell::new(Config::from_text(text), Vec::new())
    }

    /// Whether any pixel inside `r` is drawn (not clear) on a 640-pixel-wide picture.
    fn drawn_in(px: &[u8], r: Rect) -> bool {
        (r.1 as usize..(r.1 + r.3) as usize).any(|y| {
            let row = &px[y * 640 * 4..(y + 1) * 640 * 4];
            row[r.0 as usize * 4..(r.0 + r.2) as usize * 4]
                .iter()
                .any(|&b| b != 0)
        })
    }

    #[test]
    fn widgets_come_from_the_line_only_and_at_their_places() {
        assert!(shell("").widgets.list.is_empty());
        assert!(shell("WIDGETS=\"\"\n").widgets.list.is_empty());
        let s =
            shell("WIDGETS=\"clock@60,60,380x150 media notinstalled.app/thing@500,40,300x100\"\n");
        assert_eq!(s.widgets.list.len(), 1);
        assert_eq!(s.widgets.list[0].rect, (60.0, 60.0, 380.0, 150.0));
        assert_eq!(s.widgets.list[0].placed.word.kind, "clock");
        // the clock card reads the time: no source, but the minute wakes it
        assert!(s.widget_sources().is_empty() && s.widgets_want_clock());
        let s = shell("WIDGETS=\"memory@0,0,380x120\"\n");
        assert_eq!(s.widget_sources(), vec![("monitor", "memory")]);
    }

    #[test]
    fn a_widget_is_drawn_at_its_place_and_nowhere_else() {
        let mut s = shell("WIDGETS=\"clock@100,80,300x100\"\n");
        s.now = chrono::NaiveDate::from_ymd_opt(2026, 11, 22)
            .unwrap()
            .and_hms_opt(17, 23, 0)
            .unwrap();
        let mut img = cairo::ImageSurface::create(cairo::Format::ARgb32, 640, 400).unwrap();
        s.draw_widgets(&cairo::Context::new(&img).unwrap());
        assert_eq!(img.stride(), 640 * 4);
        let px = img.data().unwrap().to_vec();
        assert!(drawn_in(&px, (100.0, 80.0, 300.0, 100.0)));
        for outside in [
            (0.0, 0.0, 640.0, 78.0),
            (0.0, 182.0, 640.0, 218.0),
            (0.0, 0.0, 98.0, 400.0),
            (402.0, 0.0, 238.0, 400.0),
        ] {
            assert!(!drawn_in(&px, outside), "{outside:?}");
        }
    }

    #[test]
    fn the_input_region_is_the_shown_widgets_only() {
        let s = shell("WIDGETS=\"clock@60.5,60,380x150 calendar@60,230,380x330\"\n");
        // 60.5 is not a place: that word is left out
        assert_eq!(s.widget_region(), vec![(60, 230, 380, 330)]);
        // a card with nothing to show takes no clicks: notifications with none
        let mut s = shell("WIDGETS=\"notifications@10,10,380x200 clock@10,300,380x100\"\n");
        assert_eq!(s.widget_region(), vec![(10, 300, 380, 100)]);
        s.notes = vec![Note {
            id: 1,
            app: "Mail".into(),
            summary: "Invoice".into(),
            body: String::new(),
            actions: vec![],
            urgency: 1,
            time: "17:20".into(),
            private: false,
        }];
        assert_eq!(
            s.widget_region(),
            vec![(10, 10, 380, 200), (10, 300, 380, 100)]
        );
    }

    #[test]
    fn a_click_on_a_widget_reaches_its_card_and_beside_it_nothing() {
        let mut s = shell("WIDGETS=\"windows@10,10,380x200\"\n");
        s.windows = vec![Window {
            id: 7,
            title: "Notes".into(),
            app_id: "notes".into(),
            focused: false,
        }];
        let img = cairo::ImageSurface::create(cairo::Format::ARgb32, 640, 400).unwrap();
        s.draw_widgets(&cairo::Context::new(&img).unwrap());
        assert!(!s.widget_press(500.0, 300.0, Button::Left));
        assert!(s.actions.is_empty());
        // the window's row: its first hit
        let r = s.widgets.frame.borrow()[0].1.list[0].1;
        s.widget_press(r.0 + 4.0, r.1 + 4.0, Button::Left);
        s.widget_release();
        assert!(
            s.actions
                .iter()
                .any(|(a, card, _, _)| matches!(a, Action::Focus(7)) && card == "windows/list"),
            "{:?}",
            s.actions.iter().map(|a| &a.1).collect::<Vec<_>>()
        );
    }
}
