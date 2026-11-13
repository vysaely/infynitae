//! The edge's state (docs/EDGE_APPS.md sections 3.4, 5 and 8): the panels and their placed cards, which
//! panel is open and how, the layout of its cards, the pointer and the hits it is on, the cards'
//! arrival and their animations, and the actions they asked for. Plain state: no Wayland, no threads,
//! no process started here (`wayland.rs` does the I/O), so it is tested without a display.

use crate::app::App;
use crate::card::{Action, Button, Card, Cx, Event, Hits, Note, Rect, Status, Window};
use crate::config::{Config, Panel, Side};
use crate::draw::{self, Text, PAD_X};
use crate::panel::CardRef;
use crate::provider::{self, CardKind, Size};
use crate::theme::Theme;
use std::cell::RefCell;
use std::time::Instant;

/// From the screen's edge to a panel.
pub const MARGIN: f64 = 24.0;
pub const PAD_TOP: f64 = 14.0;
pub const PAD_BOTTOM: f64 = 12.0;
pub const HEAD_H: f64 = 26.0;
pub const GAP: f64 = 12.0;
/// A card rises and fades in as the panel opens; the next one starts this much later.
pub const ARRIVE_MS: f64 = 200.0;
pub const ARRIVE_STAGGER_MS: f64 = 28.0;
pub const ARRIVE_LIFT: f64 = 14.0;
const ARRIVE_CARDS: f64 = 8.0;

/// A card in a panel: its word, provider, kind, the card itself, the app it comes from.
pub struct Placed {
    pub word: CardRef,
    pub provider: &'static dyn provider::Provider,
    pub kind: &'static CardKind,
    pub size: Size,
    /// the header's right side: the app's name, or the provider's
    pub from: String,
    pub app_id: Option<String>,
    pub card: Box<dyn Card>,
}

pub struct PanelState {
    pub panel: Panel,
    pub cards: Vec<Placed>,
}

/// The cards of `p` that can be made: a word whose app is not installed, or whose provider or kind
/// does not exist, is left out (docs section 3.3, "App not installed", comes with edit mode).
pub fn place(p: &Panel, apps: &[App]) -> Vec<Placed> {
    let mut out = Vec::new();
    for w in crate::panel::cards(p) {
        let Some((prov, app)) = provider::resolve(&w.owner, apps) else {
            continue;
        };
        let (Some(kind), Some(card)) = (
            prov.kind(&w.kind),
            prov.card(&w.kind, w.argument.as_deref()),
        ) else {
            continue;
        };
        let size = w
            .size
            .filter(|s| kind.sizes.contains(s))
            .unwrap_or(kind.default);
        out.push(Placed {
            from: app.map_or(prov.name().to_string(), |a| a.name.clone()),
            app_id: app.map(|a| a.id.clone()),
            word: w,
            provider: prov,
            kind,
            size,
            card,
        });
    }
    out
}

pub struct Shell {
    pub cfg: Config,
    pub theme: Theme,
    pub apps: Vec<App>,
    pub windows: Vec<Window>,
    pub notes: Vec<Note>,
    pub panels: Vec<PanelState>,
    pub open: Option<usize>,
    pub opened_by_hover: bool,
    /// the pointer, in surface pixels, and the card and hit under it in the last frame
    pub pointer: Option<(f64, f64)>,
    pub hovered: Option<(usize, u32)>,
    /// the card and hit a button went down on (drags go to it)
    pub pressed: Option<(usize, u32)>,
    /// each shown card's rectangle and hits in the last frame
    pub frame: RefCell<Vec<(usize, Rect, Hits)>>,
    /// what the cards asked for: the action, its card ("owner/kind": one command at a time per
    /// card), and the provider and source keys to fetch again after it ran
    pub actions: Vec<(Action, String, &'static str, &'static [&'static str])>,
    pub opened_at: Option<Instant>,
    pub open_ms: f64,
    pub last_tick: Option<Instant>,
    pub last_screen: (f64, f64),
    pub now: chrono::NaiveDateTime,
}

impl Shell {
    pub fn new(cfg: Config, apps: Vec<App>) -> Shell {
        let theme = Theme::from_config(&cfg);
        let panels = cfg
            .panels
            .iter()
            .map(|p| PanelState {
                cards: place(p, &apps),
                panel: p.clone(),
            })
            .collect();
        Shell {
            cfg,
            theme,
            apps,
            windows: Vec::new(),
            notes: Vec::new(),
            panels,
            open: None,
            opened_by_hover: false,
            pointer: None,
            hovered: None,
            pressed: None,
            frame: RefCell::new(Vec::new()),
            actions: Vec::new(),
            opened_at: None,
            open_ms: f64::INFINITY,
            last_tick: None,
            last_screen: (1920.0, 1080.0),
            now: chrono::Local::now().naive_local(),
        }
    }

    pub fn cx(&self, size: Size, app_id: Option<&str>, list_above: bool) -> Cx<'_> {
        Cx {
            theme: &self.theme,
            now: self.now,
            size,
            app: app_id.and_then(|id| self.apps.iter().find(|a| a.id == id)),
            apps: &self.apps,
            windows: &self.windows,
            notes: &self.notes,
            list_above,
            editing: false,
        }
    }

    fn width_of(&self, idx: usize, w: f64) -> f64 {
        let p = &self.panels[idx].panel;
        let pw = if p.width > 0 { p.width as f64 } else { 380.0 };
        pw.min(w - 2.0 * MARGIN)
    }

    /// A card's whole height (its box: header, body, padding) at `width`; 0 when it hides.
    fn card_h(&self, pc: &Placed, width: f64, list_above: bool) -> f64 {
        let cx = self.cx(pc.size, pc.app_id.as_deref(), list_above);
        match pc.card.status(&cx) {
            Status::Hidden => 0.0,
            Status::Ready => {
                PAD_TOP + HEAD_H + pc.card.height(&cx, width - 2.0 * (PAD_X - 8.0)) + PAD_BOTTOM
            }
            _ => PAD_TOP + HEAD_H + 20.0 + PAD_BOTTOM,
        }
    }

    /// Panel `idx` on a screen of `w x h`: its box, and each shown card's rectangle (halves side by
    /// side), top to bottom.
    pub fn layout(&self, idx: usize, w: f64, h: f64) -> (Rect, Vec<(usize, Rect)>) {
        let ps = &self.panels[idx];
        let pw = self.width_of(idx, w);
        let above = ps.panel.side == Side::Bottom;
        let mut rows: Vec<Vec<(usize, f64)>> = Vec::new();
        let mut i = 0;
        while i < ps.cards.len() {
            let a = &ps.cards[i];
            let next = ps.cards.get(i + 1);
            if a.word.half && a.kind.half && next.is_some_and(|b| b.word.half && b.kind.half) {
                let half = (pw - GAP) / 2.0;
                rows.push(vec![
                    (i, self.card_h(a, half, above)),
                    (i + 1, self.card_h(next.unwrap(), half, above)),
                ]);
                i += 2;
            } else {
                rows.push(vec![(i, self.card_h(a, pw, above))]);
                i += 1;
            }
        }
        rows.retain(|r| r.iter().any(|(_, ch)| *ch > 0.0));
        let heights: Vec<f64> = rows
            .iter()
            .map(|r| r.iter().map(|(_, ch)| *ch).fold(0.0, f64::max))
            .collect();
        let ph = (heights.iter().sum::<f64>() + GAP * rows.len().saturating_sub(1) as f64)
            .min(h - 2.0 * MARGIN);
        let p = &ps.panel;
        let (along_w, size) = if p.side.horizontal() {
            (w, pw)
        } else {
            (h, ph)
        };
        let along = match p.align {
            a if a < 0 => MARGIN,
            a if a > 0 => along_w - size - MARGIN,
            _ => ((along_w - size) / 2.0).round(),
        }
        .max(MARGIN);
        let (ox, oy) = match p.side {
            Side::Top => (along, MARGIN),
            Side::Bottom => (along, h - ph - MARGIN),
            Side::Left => (MARGIN, along),
            Side::Right => (w - pw - MARGIN, along),
        };
        let mut out = Vec::new();
        let mut y = oy;
        for (row, rh) in rows.iter().zip(&heights) {
            if row.len() == 2 {
                let half = (pw - GAP) / 2.0;
                out.push((row[0].0, (ox, y, half, row[0].1)));
                out.push((row[1].0, (ox + half + GAP, y, half, row[1].1)));
            } else {
                out.push((row[0].0, (ox, y, pw, row[0].1)));
            }
            y += rh + GAP;
        }
        ((ox, oy, pw, ph), out)
    }

    /// The 2 px strip at the screen's edge where pointing opens a hover panel.
    pub fn hover_strip(&self, idx: usize, w: f64, h: f64) -> Rect {
        let ((ox, oy, pw, ph), _) = self.layout(idx, w, h);
        match self.panels[idx].panel.side {
            Side::Top => (ox, 0.0, pw, 2.0),
            Side::Bottom => (ox, (h - 2.0).max(0.0), pw, 2.0),
            Side::Left => (0.0, oy, 2.0, ph),
            Side::Right => ((w - 2.0).max(0.0), oy, 2.0, ph),
        }
    }

    /// The panel with the gap to its edge: leaving it closes a panel opened by hover.
    pub fn hover_box(&self, idx: usize, w: f64, h: f64) -> Rect {
        let ((ox, oy, pw, ph), _) = self.layout(idx, w, h);
        match self.panels[idx].panel.side {
            Side::Top => (ox, 0.0, pw, oy + ph),
            Side::Bottom => (ox, oy, pw, (h - oy).max(ph)),
            Side::Left => (0.0, oy, ox + pw, ph),
            Side::Right => (ox, oy, (w - ox).max(pw), ph),
        }
    }

    pub fn open_panel(&mut self, idx: usize, by_hover: bool, now: Instant) -> bool {
        if idx >= self.panels.len() {
            return false;
        }
        if self.open == Some(idx) {
            return if by_hover { false } else { self.close() };
        }
        self.open = Some(idx);
        self.opened_by_hover = by_hover;
        self.opened_at = Some(now);
        self.open_ms = 0.0;
        self.hovered = None;
        self.pressed = None;
        true
    }

    pub fn toggle_named(&mut self, name: &str) -> bool {
        match self.panels.iter().position(|p| p.panel.name == name) {
            Some(i) => self.open_panel(i, false, Instant::now()),
            None => false,
        }
    }

    pub fn close(&mut self) -> bool {
        let was = self.open.take().is_some();
        self.opened_by_hover = false;
        self.hovered = None;
        self.pressed = None;
        self.pointer = None;
        self.opened_at = None;
        self.open_ms = f64::INFINITY;
        self.frame.borrow_mut().clear();
        was
    }

    /// The card and hit under `p` in the last frame.
    fn hit_at(&self, p: (f64, f64)) -> Option<(usize, u32)> {
        self.frame
            .borrow()
            .iter()
            .rev()
            .find_map(|(i, _, hits)| hits.top_at(p).map(|h| (*i, h)))
    }

    fn card_at(&self, p: (f64, f64)) -> Option<usize> {
        self.frame
            .borrow()
            .iter()
            .find(|(_, r, _)| p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3)
            .map(|(i, _, _)| *i)
    }

    /// The pointer moved: a hover panel opens from its strip and closes when left; the light follows
    /// the hit under it; a held button drags. True when something must be drawn again.
    pub fn pointer_motion(&mut self, w: f64, h: f64, x: f64, y: f64) -> bool {
        self.last_screen = (w, h);
        let Some(idx) = self.open else {
            for i in 0..self.panels.len() {
                if self.panels[i].panel.hover {
                    let s = self.hover_strip(i, w, h);
                    if x >= s.0 && x <= s.0 + s.2 && y >= s.1 && y <= s.1 + s.3 {
                        return self.open_panel(i, true, Instant::now());
                    }
                }
            }
            return false;
        };
        if self.opened_by_hover {
            let b = self.hover_box(idx, w, h);
            if x < b.0 || x > b.0 + b.2 || y < b.1 || y > b.1 + b.3 {
                return self.close();
            }
        }
        self.pointer = Some((x, y));
        if let Some((ci, hit)) = self.pressed {
            return self.card_event(idx, ci, &Event::Drag { hit, at: (x, y) });
        }
        let now = self.hit_at((x, y));
        let changed = now != self.hovered;
        self.hovered = now;
        changed
    }

    pub fn pointer_leave(&mut self) -> bool {
        self.pointer = None;
        let lit = self.hovered.take().is_some();
        if self.opened_by_hover && self.open.is_some() {
            return self.close();
        }
        lit
    }

    /// A button went down: on a hit, the card's Press; outside an open panel, it closes.
    pub fn click(&mut self, w: f64, h: f64, x: f64, y: f64, button: Button) -> bool {
        let Some(idx) = self.open else {
            return false;
        };
        let ((ox, oy, pw, ph), _) = self.layout(idx, w, h);
        if x < ox || x > ox + pw || y < oy || y > oy + ph {
            return self.close();
        }
        let Some((ci, hit)) = self.hit_at((x, y)) else {
            return false;
        };
        if button == Button::Left {
            self.pressed = Some((ci, hit));
        }
        self.card_event(
            idx,
            ci,
            &Event::Press {
                hit,
                button,
                at: (x, y),
            },
        )
    }

    pub fn release(&mut self) -> bool {
        let (Some(idx), Some((ci, hit))) = (self.open, self.pressed.take()) else {
            return false;
        };
        self.card_event(idx, ci, &Event::Release { hit })
    }

    pub fn scroll(&mut self, x: f64, y: f64, dy: f64) -> bool {
        let Some(idx) = self.open else {
            return false;
        };
        let Some(ci) = self.card_at((x, y)) else {
            return false;
        };
        let hit = self
            .hit_at((x, y))
            .filter(|(c, _)| *c == ci)
            .map(|(_, h)| h);
        self.card_event(idx, ci, &Event::Scroll { hit, dy })
    }

    /// Hands `ev` to card `ci` of panel `idx`; its actions are queued with what to fetch after.
    fn card_event(&mut self, idx: usize, ci: usize, ev: &Event) -> bool {
        let above = self.panels[idx].panel.side == Side::Bottom;
        let (size, app_id) = {
            let pc = &self.panels[idx].cards[ci];
            (pc.size, pc.app_id.clone())
        };
        let theme = self.theme.clone();
        let cx = Cx {
            theme: &theme,
            now: self.now,
            size,
            app: app_id
                .as_deref()
                .and_then(|id| self.apps.iter().find(|a| a.id == id)),
            apps: &self.apps,
            windows: &self.windows,
            notes: &self.notes,
            list_above: above,
            editing: false,
        };
        let mut out = Vec::new();
        let pc = &mut self.panels[idx].cards[ci];
        let changed = pc.card.event(ev, &cx, &mut out);
        let (prov, keys) = (pc.provider.id(), pc.kind.sources);
        let card = format!("{}/{}", pc.word.owner, pc.word.kind);
        for a in out {
            if a == Action::ClosePanel {
                self.close();
                return true;
            }
            self.actions.push((a, card.clone(), prov, keys));
        }
        changed
    }

    /// Escape closes the open panel. True when it did.
    pub fn escape(&mut self) -> bool {
        self.close()
    }

    /// Moves the animations on: the cards' arrival and every shown card's own. True while one runs.
    pub fn tick(&mut self, now: Instant) -> bool {
        let dt = self
            .last_tick
            .map_or(0.0, |t| now.duration_since(t).as_secs_f64() * 1000.0)
            .min(50.0);
        self.last_tick = Some(now);
        let mut running = false;
        if let Some(t0) = self.opened_at {
            let ms = now.duration_since(t0).as_secs_f64() * 1000.0;
            if ms < ARRIVE_MS + ARRIVE_STAGGER_MS * ARRIVE_CARDS {
                self.open_ms = ms;
            } else {
                self.opened_at = None;
                self.open_ms = f64::INFINITY;
            }
            running = true;
        }
        if let Some(idx) = self.open {
            for pc in &mut self.panels[idx].cards {
                running |= pc.card.tick(dt);
            }
        }
        if !running {
            self.last_tick = None;
        }
        running
    }

    /// The (provider, source key) pairs the open panel's cards need.
    pub fn sources(&self) -> Vec<(&'static str, &'static str)> {
        let mut out: Vec<(&'static str, &'static str)> = Vec::new();
        if let Some(idx) = self.open {
            for pc in &self.panels[idx].cards {
                for k in pc.kind.sources {
                    if !out.contains(&(pc.provider.id(), *k)) {
                        out.push((pc.provider.id(), k));
                    }
                }
            }
        }
        out
    }

    /// New data for `(provider, key)`: every card that lists it. True when one changed.
    pub fn apply(&mut self, provider: &str, key: &str, data: &dyn std::any::Any) -> bool {
        let mut changed = false;
        for ps in &mut self.panels {
            for pc in &mut ps.cards {
                if pc.provider.id() == provider && pc.kind.sources.contains(&key) {
                    changed |= pc.card.apply(key, data);
                }
            }
        }
        changed
    }

    /// Draws the open panel on a screen of `w x h`: each card's box, header and body, rising in as
    /// the panel opens; keeps each card's hits for the pointer. Returns the panel's box.
    pub fn draw(&self, cr: &cairo::Context, w: f64, h: f64) -> Option<Rect> {
        let idx = self.open?;
        let (pbox, rects) = self.layout(idx, w, h);
        let ps = &self.panels[idx];
        let above = ps.panel.side == Side::Bottom;
        let th = &self.theme;
        let mut frame = Vec::new();
        let bottom = rects.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        cr.save().ok();
        cr.rectangle(pbox.0, pbox.1, pbox.2, pbox.3 + ARRIVE_LIFT);
        cr.clip();
        for (ci, r) in &rects {
            let pc = &ps.cards[*ci];
            let cx = self.cx(pc.size, pc.app_id.as_deref(), above);
            let body = (
                r.0 + PAD_X - 8.0,
                r.1 + PAD_TOP + HEAD_H,
                r.2 - 2.0 * (PAD_X - 8.0),
                r.3 - PAD_TOP - HEAD_H - PAD_BOTTOM,
            );
            let mut hits = Hits::new(self.pointer, None);
            // the bottom card arrives first (about one card per 90 px up)
            let k = ((bottom - (r.1 + r.3)) / 90.0).clamp(0.0, ARRIVE_CARDS);
            let p = if self.open_ms.is_finite() {
                crate::ui::ease((self.open_ms - k * ARRIVE_STAGGER_MS) / ARRIVE_MS)
            } else {
                1.0
            };
            if p <= 0.0 {
                frame.push((*ci, *r, hits));
                continue;
            }
            let lifted = p < 1.0;
            if lifted {
                cr.push_group();
                cr.translate(0.0, (1.0 - p) * ARRIVE_LIFT);
            }
            draw::card(cr, th, r.0, r.1, r.2, r.3);
            let title = pc
                .card
                .title(&cx)
                .unwrap_or_else(|| pc.kind.title.to_string());
            let from_w = draw::text_right(
                cr,
                r.0 + r.2 - PAD_X,
                r.1 + PAD_TOP + 3.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: &pc.from,
                },
            );
            draw::text_max(
                cr,
                r.0 + PAD_X,
                r.1 + PAD_TOP,
                (r.2 - 2.0 * PAD_X - from_w - 12.0).max(40.0),
                &Text {
                    size: 15.0,
                    bold: true,
                    color: th.text,
                    text: &title,
                },
            );
            match pc.card.status(&cx) {
                Status::Ready => {
                    cr.save().ok();
                    cr.rectangle(body.0 - 1.0, body.1 - 1.0, body.2 + 2.0, body.3 + 2.0);
                    cr.clip();
                    pc.card.draw(cr, &cx, body, &mut hits);
                    cr.restore().ok();
                }
                Status::Loading => {
                    draw::text(
                        cr,
                        body.0 + 8.0,
                        body.1,
                        &Text {
                            size: 12.0,
                            bold: false,
                            color: th.muted,
                            text: "Loading",
                        },
                    );
                }
                Status::Missing(why) => {
                    draw::text_max(
                        cr,
                        body.0 + 8.0,
                        body.1,
                        body.2 - 16.0,
                        &Text {
                            size: 12.0,
                            bold: false,
                            color: th.muted,
                            text: &why,
                        },
                    );
                }
                Status::Hidden => {}
            }
            if lifted {
                let _ = cr.pop_group_to_source();
                let _ = cr.paint_with_alpha(p);
            }
            frame.push((*ci, *r, hits));
        }
        cr.restore().ok();
        *self.frame.borrow_mut() = frame;
        Some(pbox)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::{parse_power, PowerData};

    fn shell(line: &str) -> Shell {
        Shell::new(
            Config::from_text(&format!("PANEL_CONTROL=\"{line}\"\n")),
            Vec::new(),
        )
    }

    #[test]
    fn a_bottom_panel_sits_on_its_edge_and_its_cards_stack() {
        let mut sh = shell("SUPER+A bottom right 380 settings/power settings/power");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert_eq!(sh.panels[idx].cards.len(), 1, "a card comes once");
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n") as &dyn std::any::Any,
        );
        let ((ox, oy, pw, ph), rects) = sh.layout(idx, 1920.0, 1080.0);
        assert_eq!((ox, pw), (1920.0 - MARGIN - 380.0, 380.0));
        assert_eq!(oy + ph, 1080.0 - MARGIN);
        assert_eq!(rects.len(), 1);
    }

    #[test]
    fn a_click_on_a_mode_queues_its_command_and_a_click_outside_closes() {
        let mut sh = shell("SUPER+A bottom right 380 settings/power");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1920, 1080).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        sh.draw(&cr, 1920.0, 1080.0);
        // the third word, Performance, of the choice: find it among the hits
        let (ci, r) = {
            let f = sh.frame.borrow();
            let (ci, _, hits) = &f[0];
            (*ci, hits.list.iter().find(|(id, _)| *id == 2).unwrap().1)
        };
        assert_eq!(ci, 0);
        assert!(sh.click(1920.0, 1080.0, r.0 + 5.0, r.1 + 5.0, Button::Left));
        assert_eq!(sh.actions.len(), 1);
        assert_eq!(
            sh.actions[0].0,
            Action::Run("powerprofilesctl set 'performance' >/dev/null 2>&1".into())
        );
        assert_eq!(
            (sh.actions[0].1.as_str(), sh.actions[0].2),
            ("settings/power", "settings")
        );
        assert!(
            sh.click(1920.0, 1080.0, 10.0, 10.0, Button::Left),
            "outside: closes"
        );
        assert!(sh.open.is_none());
        let _ = PowerData::default();
    }

    #[test]
    fn a_hover_panel_opens_from_its_strip_and_closes_when_left() {
        let mut sh = shell("SUPER+A bottom right 380 hover settings/power");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let s = sh.hover_strip(idx, 1920.0, 1080.0);
        assert!(sh.pointer_motion(1920.0, 1080.0, s.0 + 10.0, s.1 + 1.0));
        assert_eq!(sh.open, Some(idx));
        assert!(sh.opened_by_hover);
        assert!(
            sh.pointer_motion(1920.0, 1080.0, 100.0, 100.0),
            "left: closed"
        );
        assert!(sh.open.is_none());
    }

    #[test]
    fn the_cards_arrive_then_stop_asking_for_frames() {
        let mut sh = shell("SUPER+A top right settings/power");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let t0 = Instant::now();
        sh.open_panel(idx, false, t0);
        assert!(sh.tick(t0));
        assert!(sh.tick(t0 + std::time::Duration::from_millis(100)));
        assert!(
            sh.tick(t0 + std::time::Duration::from_millis(1000)),
            "the step that ends it"
        );
        assert!(!sh.tick(t0 + std::time::Duration::from_millis(1016)));
        assert_eq!(sh.sources(), vec![("settings", "power")]);
    }
}
