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
/// Leaving animation: fade and drop when a panel closes.
pub const LEAVE_MS: f64 = 120.0;
pub const LEAVE_DROP: f64 = 6.0;
/// Pointer dwell before opening a hover panel, and grace before closing when left.
pub const HOVER_OPEN_MS: f64 = 120.0;
pub const HOVER_CLOSE_MS: f64 = 250.0;

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

struct ClosingPanel {
    idx: usize,
    transient: Option<PanelState>,
    started: Instant,
    ms: f64,
    scroll: f64,
}

pub const NOT_INSTALLED_TEXT: &str = "This app is not installed";

struct NotInstalledProvider;

impl provider::Provider for NotInstalledProvider {
    fn id(&self) -> &'static str {
        ""
    }
    fn name(&self) -> &'static str {
        ""
    }
    fn claims(&self) -> &'static [provider::Claim] {
        &[]
    }
    fn listed(&self) -> provider::Listed {
        provider::Listed::Never
    }
    fn cards(&self) -> &'static [CardKind] {
        &[]
    }
    fn card(&self, _: &str, _: Option<&str>) -> Option<Box<dyn Card>> {
        None
    }
    fn source(&self, _: &str) -> Option<Box<dyn crate::card::Source>> {
        None
    }
}

static NOT_INSTALLED_PROVIDER: NotInstalledProvider = NotInstalledProvider;
static NOT_INSTALLED_KIND: CardKind = CardKind {
    id: "",
    title: "",
    about: "",
    sizes: &[Size::S],
    default: Size::S,
    half: false,
    sources: &[],
};

struct NotInstalledCard {
    title: String,
}

impl Card for NotInstalledCard {
    fn apply(&mut self, _key: &str, _data: &dyn std::any::Any) -> bool {
        false
    }
    fn status(&self, _cx: &Cx) -> Status {
        Status::Ready
    }
    fn title(&self, _cx: &Cx) -> Option<String> {
        Some(self.title.clone())
    }
    fn height(&self, _cx: &Cx, width: f64) -> f64 {
        wrapped_text_height((width - 16.0).max(20.0), NOT_INSTALLED_TEXT) + 8.0 + 26.0
    }
    fn draw(&self, cr: &cairo::Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let max_w = (body.2 - 16.0).max(20.0);
        let l = missing_layout(cr, max_w, NOT_INSTALLED_TEXT);
        draw::set(cr, cx.theme.muted);
        cr.move_to((body.0 + 8.0).round(), body.1.round());
        pangocairo::functions::show_layout(cr, &l);
        let text_h = (l.pixel_size().1 as f64).max(20.0);
        crate::ui::button(cr, cx, hits, 0, body.0 + 8.0, body.1 + text_h + 8.0, "Remove");
    }
    fn event(&mut self, ev: &Event, _cx: &Cx, out: &mut Vec<Action>) -> bool {
        if matches!(
            ev,
            Event::Press {
                hit: 0,
                button: Button::Left,
                ..
            }
        ) {
            out.push(Action::RemoveCard);
            return true;
        }
        false
    }
}

fn place_one(w: CardRef, apps: &[App]) -> Placed {
    if let Some((prov, app)) = provider::resolve(&w.owner, apps) {
        if let (Some(kind), Some(card)) = (
            prov.kind(&w.kind),
            prov.card(&w.kind, w.argument.as_deref()),
        ) {
            let size = w
                .size
                .filter(|s| kind.sizes.contains(s))
                .unwrap_or(kind.default);
            return Placed {
                from: from_name(prov.name(), kind.title, app),
                app_id: app.map(|a| a.id.clone()),
                word: w,
                provider: prov,
                kind,
                size,
                card,
            };
        }
    }
    let from = provider::by_id(&w.owner).map_or_else(|| w.owner.clone(), |p| p.name().to_string());
    let title = provider::by_id(&w.owner)
        .and_then(|p| p.kind(&w.kind))
        .map_or_else(|| w.kind.clone(), |k| k.title.to_string());
    Placed {
        word: w,
        provider: &NOT_INSTALLED_PROVIDER,
        kind: &NOT_INSTALLED_KIND,
        size: Size::S,
        from,
        app_id: None,
        card: Box::new(NotInstalledCard { title }),
    }
}

/// The cards of `p`: unknown apps or card kinds stay as a built-in "This app is not installed"
/// card with Remove (docs/EDGE_APPS.md section 3.3).
pub fn place(p: &Panel, apps: &[App], favorites: &str) -> Vec<Placed> {
    crate::panel::cards_with(p, favorites)
        .into_iter()
        .map(|w| place_one(w, apps))
        .collect()
}

/// The name at the right of a card's header: the app it comes from, else its provider, and nothing
/// where that only repeats the title ("Notifications" from Notifications).
pub fn from_name(provider: &str, title: &str, app: Option<&App>) -> String {
    let name = app.map_or(provider, |a| a.name.as_str());
    if name.eq_ignore_ascii_case(title) {
        String::new()
    } else {
        name.to_string()
    }
}

/// A key the open panel handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Up,
    Down,
    Tab,
    BackTab,
    Enter,
    Backspace,
    /// a character typed
    Char(char),
    /// Super with a letter: a panel's key
    Super(char),
}

pub struct Shell {
    pub cfg: Config,
    pub theme: Theme,
    pub apps: Vec<App>,
    pub windows: Vec<Window>,
    pub notes: Vec<Note>,
    /// the edge's own notifications: pop-ups and history (`notes` is the history as cards read it)
    pub notify: crate::notify::Notify,
    pub panels: Vec<PanelState>,
    pub open: Option<usize>,
    pub opened_by_hover: bool,
    pub clicked_inside: bool,
    hover_open: Option<(usize, Instant)>,
    hover_close: Option<Instant>,
    /// the pointer, in surface pixels, and the card and hit under it in the last frame
    pub pointer: Option<(f64, f64)>,
    pub hovered: Option<(usize, u32)>,
    /// the card and hit a button went down on (drags go to it)
    pub pressed: Option<(usize, u32)>,
    /// each shown card's rectangle and hits in the last frame, plus the scroll and screen at which
    /// they were drawn
    pub frame: RefCell<Vec<(usize, Rect, Hits)>>,
    frame_scroll: std::cell::Cell<f64>,
    frame_screen: std::cell::Cell<(f64, f64)>,
    /// what the cards asked for: the action, its card ("owner/kind": one command at a time per
    /// card), and the provider and source keys to fetch again after it ran
    pub actions: Vec<(Action, String, &'static str, &'static [&'static str])>,
    /// the ask prompt (`infynitae-edge ask`): over everything while it asks
    pub ask: crate::ask::World,
    /// what was typed while the panel is open (the cards with a search field read it)
    pub query: String,
    /// the keyboard's selection: a card and one of its hits; shown from the first arrow or Tab
    /// until the pointer moves
    pub selected: Option<(usize, u32)>,
    pub keyboard: bool,
    pub opened_at: Option<Instant>,
    pub open_ms: f64,
    closing: Option<ClosingPanel>,
    pub last_tick: Option<Instant>,
    pub last_screen: (f64, f64),
    pub now: chrono::NaiveDateTime,
    pub edit: crate::edit::EditState,
    /// the panel ShowCards put in the open one's place (a tray app's menu and card), removed when
    /// it closes
    pub transient: Option<usize>,
    /// a right click on a free spot: small menu with "Edit panel" at that point (a right click
    /// alone opened edit mode too easily)
    pub edit_menu: Option<(f64, f64)>,
    /// the config file's time when it was last read (None: not looked at yet)
    pub config_mtime: Option<std::time::SystemTime>,
    pub panel_scroll: f64,
    pub live: bool,
    pub save_paths: Option<(std::path::PathBuf, std::path::PathBuf)>,
    pub fetch_needed: bool,
}

fn file_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Shell {
    pub fn new(cfg: Config, apps: Vec<App>) -> Shell {
        Self::new_at(cfg, apps, &crate::config::config_path())
    }

    pub fn new_at(cfg: Config, apps: Vec<App>, config_path: &std::path::Path) -> Shell {
        let theme = Theme::from_config(&cfg);
        let panels = cfg
            .panels
            .iter()
            .map(|p| PanelState {
                cards: place(p, &apps, &cfg.favorites),
                panel: p.clone(),
            })
            .collect();
        Shell {
            cfg,
            theme,
            apps,
            windows: Vec::new(),
            notes: Vec::new(),
            notify: Default::default(),
            ask: Default::default(),
            config_mtime: file_mtime(config_path),
            transient: None,
            edit_menu: None,
            query: String::new(),
            selected: None,
            keyboard: false,
            panels,
            open: None,
            opened_by_hover: false,
            clicked_inside: false,
            hover_open: None,
            hover_close: None,
            pointer: None,
            hovered: None,
            pressed: None,
            frame: RefCell::new(Vec::new()),
            frame_scroll: std::cell::Cell::new(0.0),
            frame_screen: std::cell::Cell::new((1920.0, 1080.0)),
            actions: Vec::new(),
            opened_at: None,
            open_ms: f64::INFINITY,
            closing: None,
            last_tick: None,
            last_screen: (1920.0, 1080.0),
            now: chrono::Local::now().naive_local(),
            edit: Default::default(),
            panel_scroll: 0.0,
            live: false,
            save_paths: None,
            fetch_needed: false,
        }
    }

    pub fn editing(&self) -> bool {
        self.edit.active
    }

    pub fn take_fetch_needed(&mut self) -> bool {
        std::mem::take(&mut self.fetch_needed)
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
            editing: self.editing(),
        }
    }

    /// A card's whole height (its box: header, body, padding) at `width`; 0 when it hides, or a
    /// placeholder's height while editing.
    fn card_h(&self, pc: &Placed, width: f64, list_above: bool) -> f64 {
        let cx = self.cx(pc.size, pc.app_id.as_deref(), list_above);
        let head = if pc.card.header(&cx) { HEAD_H } else { 0.0 };
        match pc.card.status(&cx) {
            Status::Hidden if cx.editing => crate::edit::PLACEHOLDER_H,
            Status::Hidden => 0.0,
            Status::Ready => {
                PAD_TOP + head + pc.card.height(&cx, width - 2.0 * (PAD_X - 8.0)) + PAD_BOTTOM
            }
            Status::Missing(why) => PAD_TOP + HEAD_H + missing_height(width, &why) + PAD_BOTTOM,
            _ => PAD_TOP + HEAD_H + 20.0 + PAD_BOTTOM,
        }
    }

    /// Panel `idx` on a screen of `w x h`: its box on screen, panel-local card rectangles (without
    /// `panel_scroll`), and total content height.
    pub fn layout_local(&self, idx: usize, w: f64, h: f64) -> (Rect, Vec<(usize, Rect)>, f64) {
        self.layout_panel_local(&self.panels[idx], w, h)
    }

    fn layout_panel_local(
        &self,
        ps: &PanelState,
        w: f64,
        h: f64,
    ) -> (Rect, Vec<(usize, Rect)>, f64) {
        let editing = self.editing();
        let pw = if ps.panel.width > 0 {
            ps.panel.width as f64
        } else {
            380.0
        }
        .min(w - 2.0 * MARGIN);
        let above = ps.panel.side == Side::Bottom;
        let mut rows: Vec<Vec<(usize, f64)>> = Vec::new();
        let mut i = 0;
        while i < ps.cards.len() {
            let a = &ps.cards[i];
            let next = ps.cards.get(i + 1);
            if pw >= crate::edit::HALF_MIN_PANEL
                && a.word.half
                && a.kind.half
                && next.is_some_and(|b| b.word.half && b.kind.half)
            {
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
        let strip = if editing { crate::edit::STRIP_H } else { 0.0 };
        let heights: Vec<f64> = rows
            .iter()
            .map(|r| r.iter().map(|(_, ch)| *ch).fold(0.0, f64::max))
            .collect();
        let content_h = if rows.is_empty() {
            if editing {
                64.0
            } else {
                0.0
            }
        } else {
            strip
                + heights.iter().sum::<f64>()
                + (GAP + strip) * rows.len().saturating_sub(1) as f64
        };
        let ph = content_h.min(h - 2.0 * MARGIN);
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
        let mut y = strip;
        for (row, rh) in rows.iter().zip(&heights) {
            if row.len() == 2 {
                let half = (pw - GAP) / 2.0;
                out.push((row[0].0, (0.0, y, half, row[0].1)));
                out.push((row[1].0, (half + GAP, y, half, row[1].1)));
            } else {
                out.push((row[0].0, (0.0, y, pw, row[0].1)));
            }
            y += rh + GAP + strip;
        }
        ((ox, oy, pw, ph), out, content_h)
    }

    /// Panel `idx` on a screen of `w x h`: its box, and each shown card's screen rectangle (halves
    /// side by side), top to bottom.
    pub fn layout(&self, idx: usize, w: f64, h: f64) -> (Rect, Vec<(usize, Rect)>) {
        let (pbox, local, content_h) = self.layout_local(idx, w, h);
        let scroll = self.panel_scroll.clamp(0.0, (content_h - pbox.3).max(0.0));
        let out = local
            .into_iter()
            .map(|(ci, r)| (ci, (pbox.0 + r.0, pbox.1 + r.1 - scroll, r.2, r.3)))
            .collect();
        (pbox, out)
    }

    pub fn edit_cards(&self, idx: usize) -> Vec<CardRef> {
        let Some(ps) = self.panels.get(idx) else {
            return Vec::new();
        };
        crate::panel::cards(&ps.panel)
            .into_iter()
            .map(|mut c| {
                c.place = None;
                c
            })
            .collect()
    }

    pub fn edit_rects(&self, idx: usize, w: f64, h: f64) -> (Rect, Rect) {
        let (pbox, _, _) = self.layout_local(idx, w, h);
        let bar = crate::edit::bar_rect((pbox.0, pbox.1), pbox.2, pbox.3, (w, h));
        let cards = self.edit_cards(idx);
        let entries = crate::edit::add_entries(&cards, &self.apps);
        let ch = crate::edit::add_content_height(&entries, self.edit.expanded_add.as_deref());
        let add = crate::edit::add_rect((pbox.0, pbox.1), pbox.2, (w, h), ch, bar);
        (bar, add)
    }

    fn refresh_edit_rects(&mut self, w: f64, h: f64) {
        self.last_screen = (w, h);
        if let Some(idx) = self.open.filter(|_| self.editing()) {
            let (bar, add) = self.edit_rects(idx, w, h);
            self.edit.bar_rect = bar;
            self.edit.add_rect = add;
        }
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

    fn apply_panel_state(&mut self, idx: usize, cards: Vec<CardRef>, width: i32, hover: bool) {
        let old_cards = std::mem::take(&mut self.panels[idx].cards);
        let favorites = self.cfg.favorites.clone();
        let apps = &self.apps;
        {
            let p = &mut self.panels[idx].panel;
            p.width = width;
            p.hover = hover;
            p.placed = false;
            p.height = 0;
            p.items.clear();
            p.words = cards.iter().map(CardRef::word).collect();
        }
        let mut pool: Vec<Option<Placed>> = old_cards.into_iter().map(Some).collect();
        let desired = crate::panel::cards_with(&self.panels[idx].panel, &favorites);
        let mut next_cards = Vec::with_capacity(desired.len());
        let mut added_fresh = false;
        for mut w in desired {
            w.place = None;
            if let Some(slot) = pool.iter_mut().find(|slot| {
                slot.as_ref().is_some_and(|pc| {
                    pc.word.owner == w.owner
                        && pc.word.kind == w.kind
                        && pc.word.argument == w.argument
                })
            }) {
                let mut pc = slot.take().unwrap();
                pc.size = w
                    .size
                    .filter(|s| pc.kind.sizes.contains(s))
                    .unwrap_or(pc.kind.default);
                pc.word = w;
                next_cards.push(pc);
            } else {
                next_cards.push(place_one(w, apps));
                added_fresh = true;
            }
        }
        self.panels[idx].cards = next_cards;
        if added_fresh {
            self.fetch_needed = true;
        }
    }

    /// A right click on a free spot of a panel of the config: the "Edit panel" menu at that point.
    fn offer_edit(&mut self, idx: usize, x: f64, y: f64) -> bool {
        if self.transient == Some(idx) {
            return false;
        }
        self.edit_menu = Some((x, y));
        true
    }

    /// The menu's one item, kept on the screen.
    pub fn edit_menu_rect(&self, w: f64, h: f64) -> Option<Rect> {
        let (x, y) = self.edit_menu?;
        let (mw, mh) = (150.0, 34.0);
        Some((
            (x).min(w - mw - 4.0).max(4.0),
            (y).min(h - mh - 4.0).max(4.0),
            mw,
            mh,
        ))
    }

    pub fn enter_edit(&mut self, w: f64, h: f64) -> bool {
        let Some(idx) = self.open else {
            return false;
        };
        // a tray app's menu and card are not a panel of the config
        if self.transient == Some(idx) {
            return false;
        }
        if self.editing() {
            return false;
        }
        self.opened_by_hover = false;
        self.clicked_inside = false;
        self.hover_open = None;
        self.hover_close = None;
        self.hovered = None;
        self.pressed = None;
        self.keyboard = false;
        self.selected = None;
        self.opened_at = None;
        self.open_ms = f64::INFINITY;
        self.query.clear();
        self.reset_keys(idx);

        let before_line = crate::panel::line(&self.panels[idx].panel);
        let before_cards = self.edit_cards(idx);
        let p = &self.panels[idx].panel;
        let before_width = p.width;
        let before_hover = p.hover;
        self.edit = crate::edit::EditState {
            active: true,
            panel_idx: idx,
            before_line,
            before_cards,
            before_width,
            before_hover,
            ..Default::default()
        };
        self.refresh_edit_rects(w, h);
        self.update_edit_hover(w, h);
        true
    }

    pub fn cancel_edit(&mut self) -> bool {
        if !self.editing() {
            return false;
        }
        let idx = self.edit.panel_idx;
        let cards = std::mem::take(&mut self.edit.before_cards);
        let width = self.edit.before_width;
        let hover = self.edit.before_hover;
        self.edit = Default::default();
        if idx < self.panels.len() {
            self.apply_panel_state(idx, cards, width, hover);
        }
        self.panel_scroll = 0.0;
        true
    }

    fn save_and_rebuild_panel(&mut self, idx: usize, cards: Vec<CardRef>, before_line: String) {
        let p = &mut self.panels[idx].panel;
        p.placed = false;
        p.height = 0;
        p.items.clear();
        p.words = cards.iter().map(CardRef::word).collect();
        let panel_clone = p.clone();
        let panel_name = panel_clone.name.clone();

        let new_cfg = if let Some((cfg_path, bak_path)) = self.save_paths.clone() {
            if let Err(e) =
                crate::edit::save_panel_to(&cfg_path, &bak_path, &panel_clone, &before_line)
            {
                eprintln!("infynitae-edge-apps: save panel: {e}");
                None
            } else {
                std::fs::read_to_string(&cfg_path)
                    .ok()
                    .map(|t| Config::from_text(&t))
            }
        } else if self.live {
            if let Err(e) = crate::edit::save_panel(&panel_clone, &before_line) {
                eprintln!("infynitae-edge-apps: save panel: {e}");
                None
            } else {
                Some(Config::load())
            }
        } else {
            let mut next = self.cfg.clone();
            if let Some(cp) = next.panels.iter_mut().find(|x| x.name == panel_name) {
                *cp = panel_clone.clone();
            }
            Some(next)
        };

        if let Some(cfg) = new_cfg {
            self.rebuild_panels(cfg, None);
        } else if let Some(cp) = self.cfg.panels.iter_mut().find(|x| x.name == panel_name) {
            *cp = panel_clone;
            self.rebuild_panels(self.cfg.clone(), None);
        }
        self.open = self
            .panels
            .iter()
            .position(|ps| ps.panel.name == panel_name);
        self.fetch_needed = true;
    }

    fn remove_placed_card(&mut self, idx: usize, ci: usize) -> bool {
        let Some(ps) = self.panels.get(idx) else {
            return false;
        };
        let before_line = crate::panel::line(&ps.panel);
        let mut cards = self.edit_cards(idx);
        if !crate::edit::remove_card(&mut cards, ci, &self.apps) {
            return false;
        }
        self.selected = None;
        self.hovered = None;
        self.pressed = None;
        self.save_and_rebuild_panel(idx, cards, before_line);
        true
    }

    pub fn finish_edit(&mut self) -> bool {
        if !self.editing() {
            return false;
        }
        let idx = self.edit.panel_idx;
        let before_line = std::mem::take(&mut self.edit.before_line);
        let cards = self.edit_cards(idx);
        self.edit = Default::default();
        self.panel_scroll = 0.0;
        self.save_and_rebuild_panel(idx, cards, before_line);
        true
    }

    /// The config file changed (a Settings card, `settings.sh set`, the owner's editor): read it
    /// again and rebuild the panels, the open one staying open. Never while a panel is edited
    /// (Done saves that one). True when it reloaded.
    pub fn reload_if_changed(&mut self) -> bool {
        self.reload_if_changed_at(&crate::config::config_path())
    }

    pub fn reload_if_changed_at(&mut self, path: &std::path::Path) -> bool {
        if self.edit.active {
            return false;
        }
        let mtime = file_mtime(path);
        if mtime == self.config_mtime {
            return false;
        }
        self.config_mtime = mtime;
        if mtime.is_none() {
            return false;
        }
        let cfg = Config::from_text(&std::fs::read_to_string(path).unwrap_or_default());
        if cfg.panels == self.cfg.panels && cfg.favorites == self.cfg.favorites {
            // the same panels and cards: keep them (an open list, the accent picker stay as they
            // are while a setting is written); the colours and the cards' data follow
            self.theme = Theme::from_config(&cfg);
            self.cfg = cfg;
            self.fetch_needed = true;
            return true;
        }
        let open = self.open.map(|i| self.panels[i].panel.name.clone());
        self.rebuild_panels(cfg, None);
        self.open = open.and_then(|n| self.panels.iter().position(|ps| ps.panel.name == n));
        self.fetch_needed = true;
        true
    }

    pub fn rebuild_panels(&mut self, cfg: Config, apps: Option<Vec<App>>) {
        self.transient = None;
        self.closing = None;
        if let Some(a) = apps {
            self.apps = a;
        }
        self.theme = Theme::from_config(&cfg);
        self.panels = cfg
            .panels
            .iter()
            .map(|p| PanelState {
                cards: place(p, &self.apps, &cfg.favorites),
                panel: p.clone(),
            })
            .collect();
        self.cfg = cfg;
    }

    fn push_edit_snapshot(&mut self, idx: usize) {
        let cards = self.edit_cards(idx);
        let p = &self.panels[idx].panel;
        self.edit.push_undo(crate::edit::Snapshot {
            cards,
            width: p.width,
            hover: p.hover,
        });
    }

    fn edit_change<F>(&mut self, idx: usize, f: F) -> bool
    where
        F: FnOnce(&mut Vec<CardRef>, &mut i32, &mut bool, &[App]) -> bool,
    {
        self.edit.confirm_reset = false;
        let mut cards = self.edit_cards(idx);
        let snap = crate::edit::Snapshot {
            cards: cards.clone(),
            width: self.panels[idx].panel.width,
            hover: self.panels[idx].panel.hover,
        };
        let mut width = snap.width;
        let mut hover = snap.hover;
        if !f(&mut cards, &mut width, &mut hover, &self.apps) {
            return false;
        }
        self.edit.push_undo(snap);
        self.apply_panel_state(idx, cards, width, hover);
        let (w, h) = self.last_screen;
        let (pbox, _, content_h) = self.layout_local(idx, w, h);
        self.panel_scroll = self.panel_scroll.clamp(0.0, (content_h - pbox.3).max(0.0));
        self.refresh_edit_rects(w, h);
        self.update_edit_hover(w, h);
        true
    }

    fn scroll_to_last(&mut self, idx: usize, w: f64, h: f64) {
        let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
        let max_scroll = (content_h - pbox.3).max(0.0);
        if max_scroll <= 0.0 {
            return;
        }
        if let Some((_, r)) = local_rects.last() {
            let bot = r.1 + r.3;
            if bot > self.panel_scroll + pbox.3 {
                self.panel_scroll = (bot - pbox.3).clamp(0.0, max_scroll);
            }
        }
    }

    fn update_edit_hover(&mut self, w: f64, h: f64) {
        let Some(idx) = self.open.filter(|_| self.editing()) else {
            self.edit.hover_card = None;
            return;
        };
        let Some((x, y)) = self.pointer else {
            return;
        };
        let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
        let (bar, add) = self.edit_rects(idx, w, h);
        let over_ui =
            crate::edit::inside(bar, x, y) || (self.edit.adding && crate::edit::inside(add, x, y));
        if over_ui || !crate::edit::inside(pbox, x, y) {
            self.edit.hover_card = None;
            return;
        }
        let scroll = self.panel_scroll.clamp(0.0, (content_h - pbox.3).max(0.0));
        self.edit.hover_card = crate::edit::card_at(&local_rects, x - pbox.0, y - pbox.1 + scroll);
    }

    pub fn open_panel(&mut self, idx: usize, by_hover: bool, now: Instant) -> bool {
        // what changed in the file shows from this opening on (a reload keeps the order of panels)
        self.reload_if_changed();
        if idx >= self.panels.len() || (by_hover && self.ask.ask.on) {
            return false;
        }
        // a panel asked for by its key: the question is put aside (cancelled), as in the old edge
        if self.ask.ask.on {
            crate::ask::answer(&mut self.ask, None);
        }
        if self.open == Some(idx) {
            return if by_hover { false } else { self.close_at(now) };
        }
        if self.editing() {
            self.cancel_edit();
        }
        self.open = Some(idx);
        self.closing = None;
        self.panel_scroll = 0.0;
        self.reset_keys(idx);
        self.opened_by_hover = by_hover;
        self.clicked_inside = false;
        self.hover_open = None;
        self.hover_close = None;
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
        self.close_at(Instant::now())
    }

    pub fn close_at(&mut self, now: Instant) -> bool {
        if self.editing() {
            self.cancel_edit();
        }
        let open = self.open.take();
        let was = open.is_some();
        let scroll = self.panel_scroll;
        if let Some(idx) = open {
            if let Some(panel) = self.panels.get_mut(idx) {
                for pc in &mut panel.cards {
                    pc.card.panel_closed();
                }
            }
        }
        if was && self.live {
            crate::cards::settings_about::clear_load_state();
        }
        self.edit_menu = None;
        self.query.clear();
        self.selected = None;
        self.keyboard = false;
        self.opened_by_hover = false;
        self.clicked_inside = false;
        self.hover_open = None;
        self.hover_close = None;
        self.hovered = None;
        self.pressed = None;
        self.pointer = None;
        self.panel_scroll = 0.0;
        self.opened_at = None;
        self.open_ms = f64::INFINITY;
        self.frame.borrow_mut().clear();
        self.frame_scroll.set(0.0);
        let transient = self
            .transient
            .take()
            .and_then(|t| (t < self.panels.len()).then(|| self.panels.remove(t)));
        if let Some(idx) = open {
            self.closing = (!self.ask.ask.on).then_some(ClosingPanel {
                idx,
                transient,
                started: now,
                ms: 0.0,
                scroll,
            });
        }
        was
    }

    /// `words` (a tray app's menu, then its card) in the open panel's place: the same edge, side
    /// and width, until it closes. TRAY_CARD orders them.
    pub fn show_cards(&mut self, idx: usize, mut words: Vec<String>) {
        match self.cfg.tray_card.as_str() {
            "card-first" => words.reverse(),
            "menu" => words.retain(|w| w.starts_with("apps/menu=")),
            _ => {}
        }
        if words.is_empty() {
            return;
        }
        let mut p = self.panels[idx].panel.clone();
        p.name = "tray".into();
        p.key = None;
        p.hover = false;
        p.items.clear();
        p.words = words;
        let cards = place(&p, &self.apps, &self.cfg.favorites);
        self.close();
        self.closing = None;
        self.panels.push(PanelState { panel: p, cards });
        let t = self.panels.len() - 1;
        self.transient = Some(t);
        self.open = Some(t);
        self.opened_at = Some(Instant::now());
        self.open_ms = 0.0;
        self.fetch_needed = true;
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

    /// The pointer moved: a hover panel opens from its strip after a short dwell and closes after
    /// a grace time when left; the light follows the hit under it; a held button drags. True when
    /// something must be drawn again.
    pub fn pointer_motion(&mut self, w: f64, h: f64, x: f64, y: f64) -> bool {
        self.pointer_motion_at(w, h, x, y, Instant::now())
    }

    pub fn pointer_motion_at(&mut self, w: f64, h: f64, x: f64, y: f64, now: Instant) -> bool {
        self.last_screen = (w, h);
        if self.ask.ask.on {
            return crate::ask::hover(&mut self.ask, w, h, x, y);
        }
        // the pointer moved: the keyboard's selection hides (the light follows the pointer)
        let hid = std::mem::take(&mut self.keyboard);
        let Some(idx) = self.open else {
            let hit_strip = (0..self.panels.len()).find(|&i| {
                if !self.panels[i].panel.hover {
                    return false;
                }
                let s = self.hover_strip(i, w, h);
                x >= s.0 && x <= s.0 + s.2 && y >= s.1 && y <= s.1 + s.3
            });
            if let Some(i) = hit_strip {
                match self.hover_open {
                    Some((prev, t0)) if prev == i => {
                        if now.saturating_duration_since(t0).as_secs_f64() * 1000.0
                            >= HOVER_OPEN_MS
                        {
                            self.hover_open = None;
                            return self.open_panel(i, true, now);
                        }
                    }
                    _ => self.hover_open = Some((i, now)),
                }
            } else {
                self.hover_open = None;
            }
            return false;
        };
        if self.editing() {
            self.pointer = Some((x, y));
            if let Some(mut d) = self.edit.drag {
                if !d.moved && (y - d.start_y).abs() >= 6.0 {
                    d.moved = true;
                    self.edit.hover_card = None;
                }
                self.edit.drag = Some(d);
                if d.moved {
                    let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
                    let scroll = self.panel_scroll.clamp(0.0, (content_h - pbox.3).max(0.0));
                    let cards = self.edit_cards(idx);
                    self.edit.drop_at = crate::edit::drop_index(
                        &local_rects,
                        cards.len(),
                        d.card,
                        x - pbox.0,
                        y - pbox.1 + scroll,
                    );
                    return true;
                }
                return false;
            }
            let prev = self.edit.hover_card;
            self.update_edit_hover(w, h);
            let (bar, add) = self.edit_rects(idx, w, h);
            let over_bar = crate::edit::inside(bar, x, y)
                || (self.edit.adding && crate::edit::inside(add, x, y));
            return self.edit.hover_card != prev || over_bar || hid;
        }
        if self.opened_by_hover {
            let b = self.hover_box(idx, w, h);
            if x < b.0 || x > b.0 + b.2 || y < b.1 || y > b.1 + b.3 {
                self.hover_close.get_or_insert(now);
                self.pointer = None;
                let lit = self.hovered.take().is_some();
                return lit || hid;
            }
            self.hover_close = None;
        }
        self.pointer = Some((x, y));
        if let Some((ci, hit)) = self.pressed {
            return self.card_event(idx, ci, &Event::Drag { hit, at: (x, y) });
        }
        let hit_now = self.hit_at((x, y));
        let changed = hit_now != self.hovered;
        self.hovered = hit_now;
        changed || hid
    }

    pub fn pointer_leave(&mut self) -> bool {
        self.pointer_leave_at(Instant::now())
    }

    pub fn pointer_leave_at(&mut self, now: Instant) -> bool {
        if self.ask.ask.on {
            return false;
        }
        self.pointer = None;
        self.hover_open = None;
        if self.editing() {
            return self.edit.hover_card.take().is_some();
        }
        let lit = self.hovered.take().is_some();
        if self.opened_by_hover && self.open.is_some() {
            self.hover_close.get_or_insert(now);
            return lit;
        }
        lit
    }

    /// Advances hover dwell / close grace timers; returns true if a panel opened or closed.
    pub fn hover_tick(&mut self, now: Instant) -> bool {
        if self.ask.ask.on || self.editing() {
            self.hover_open = None;
            self.hover_close = None;
            return false;
        }
        if self.open.is_none() {
            if let Some((i, t0)) = self.hover_open {
                if now.saturating_duration_since(t0).as_secs_f64() * 1000.0 >= HOVER_OPEN_MS {
                    self.hover_open = None;
                    return self.open_panel(i, true, now);
                }
            }
            return false;
        }
        if self.opened_by_hover {
            if let Some(t0) = self.hover_close {
                if now.saturating_duration_since(t0).as_secs_f64() * 1000.0 >= HOVER_CLOSE_MS {
                    self.hover_close = None;
                    return self.close_at(now);
                }
            }
        }
        false
    }

    pub fn hover_timeout(&self, now: Instant) -> Option<std::time::Duration> {
        if self.ask.ask.on || self.editing() {
            return None;
        }
        if self.open.is_none() {
            self.hover_open.map(|(_, t0)| {
                std::time::Duration::from_secs_f64(HOVER_OPEN_MS / 1000.0)
                    .saturating_sub(now.saturating_duration_since(t0))
            })
        } else if self.opened_by_hover {
            self.hover_close.map(|t0| {
                std::time::Duration::from_secs_f64(HOVER_CLOSE_MS / 1000.0)
                    .saturating_sub(now.saturating_duration_since(t0))
            })
        } else {
            None
        }
    }

    fn edit_press(&mut self, idx: usize, w: f64, h: f64, x: f64, y: f64, button: Button) -> bool {
        self.last_screen = (w, h);
        self.pointer = Some((x, y));
        let (bar, add) = self.edit_rects(idx, w, h);
        self.edit.bar_rect = bar;
        self.edit.add_rect = add;
        let cards_now = self.edit_cards(idx);
        let entries = crate::edit::add_entries(&cards_now, &self.apps);

        if let Some(hit) = crate::edit::bar_hit(&self.edit, &entries, x, y) {
            if !matches!(
                hit,
                crate::edit::BarHit::Button(crate::edit::BarButton::Reset)
            ) {
                self.edit.confirm_reset = false;
            }
            match hit {
                crate::edit::BarHit::Nothing => return false,
                crate::edit::BarHit::Button(btn) => match btn {
                    crate::edit::BarButton::Undo => {
                        let Some(snap) = self.edit.pop_undo() else {
                            return false;
                        };
                        self.apply_panel_state(idx, snap.cards, snap.width, snap.hover);
                        self.refresh_edit_rects(w, h);
                        self.update_edit_hover(w, h);
                        return true;
                    }
                    crate::edit::BarButton::Reset => {
                        if !crate::edit::has_default(&self.panels[idx].panel.name) {
                            return false;
                        }
                        if !self.edit.confirm_reset {
                            self.edit.confirm_reset = true;
                            return true;
                        }
                        self.edit.confirm_reset = false;
                        let name = self.panels[idx].panel.name.clone();
                        let Some((def_cards, def_w, def_hover)) =
                            crate::edit::default_panel_state(&name)
                        else {
                            return false;
                        };
                        self.push_edit_snapshot(idx);
                        self.apply_panel_state(idx, def_cards, def_w, def_hover);
                        self.panel_scroll = 0.0;
                        self.refresh_edit_rects(w, h);
                        self.update_edit_hover(w, h);
                        return true;
                    }
                    crate::edit::BarButton::Add => {
                        self.edit.adding = !self.edit.adding;
                        if !self.edit.adding {
                            self.edit.expanded_add = None;
                            self.edit.add_scroll = 0.0;
                        }
                        self.refresh_edit_rects(w, h);
                        return true;
                    }
                    crate::edit::BarButton::Narrower => {
                        return self.edit_change(idx, |_, pw, _, _| {
                            if let Some(nw) =
                                crate::edit::adjust_width(*pw, -crate::edit::WIDTH_STEP)
                            {
                                *pw = nw;
                                true
                            } else {
                                false
                            }
                        });
                    }
                    crate::edit::BarButton::Wider => {
                        return self.edit_change(idx, |_, pw, _, _| {
                            if let Some(nw) =
                                crate::edit::adjust_width(*pw, crate::edit::WIDTH_STEP)
                            {
                                *pw = nw;
                                true
                            } else {
                                false
                            }
                        });
                    }
                    crate::edit::BarButton::Hover => {
                        return self.edit_change(idx, |_, _, hv, _| {
                            *hv = !*hv;
                            true
                        });
                    }
                    crate::edit::BarButton::Cancel => return self.cancel_edit(),
                    crate::edit::BarButton::Done => return self.finish_edit(),
                },
                crate::edit::BarHit::ChooseAdd(owner) => {
                    if self.edit.expanded_add.as_deref() == Some(owner.as_str()) {
                        self.edit.expanded_add = None;
                    } else {
                        self.edit.expanded_add = Some(owner.clone());
                        let new_ch = crate::edit::add_content_height(
                            &entries,
                            self.edit.expanded_add.as_deref(),
                        );
                        let (pbox, _, _) = self.layout_local(idx, w, h);
                        let new_add =
                            crate::edit::add_rect((pbox.0, pbox.1), pbox.2, (w, h), new_ch, bar);
                        self.edit.add_scroll = crate::edit::reveal_expanded(
                            new_add,
                            &entries,
                            &owner,
                            self.edit.add_scroll,
                        );
                    }
                    self.refresh_edit_rects(w, h);
                    return true;
                }
                crate::edit::BarHit::AddKind(owner, kind_id) => {
                    let changed = self.edit_change(idx, |cards, _, _, _| {
                        crate::edit::add_card(cards, CardRef::new(&owner, &kind_id))
                    });
                    if changed {
                        let new_cards = self.edit_cards(idx);
                        let new_entries = crate::edit::add_entries(&new_cards, &self.apps);
                        if !new_entries.iter().any(|e| e.owner == owner) {
                            self.edit.expanded_add = None;
                        }
                        self.scroll_to_last(idx, w, h);
                        self.refresh_edit_rects(w, h);
                    }
                    return changed;
                }
            }
        }

        self.edit.confirm_reset = false;
        let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
        if !crate::edit::inside(pbox, x, y) {
            return false;
        }
        let scroll = self.panel_scroll.clamp(0.0, (content_h - pbox.3).max(0.0));
        let (lx, ly) = (x - pbox.0, y - pbox.1 + scroll);
        let Some(ci) = crate::edit::card_at(&local_rects, lx, ly) else {
            return false;
        };
        if let Some((_, r)) = local_rects.iter().find(|&&(i, _)| i == ci) {
            let (_, btns) = crate::edit::card_buttons(&cards_now, &self.apps, ci, *r, pbox.2);
            for (kind, br) in btns {
                if crate::edit::inside(br, lx, ly) {
                    return match kind {
                        crate::edit::CardButton::Up => {
                            self.edit_change(idx, |c, _, _, _| crate::edit::move_up(c, ci))
                        }
                        crate::edit::CardButton::Down => {
                            self.edit_change(idx, |c, _, _, _| crate::edit::move_down(c, ci))
                        }
                        crate::edit::CardButton::Half => self.edit_change(idx, |c, _, _, apps| {
                            crate::edit::toggle_half(c, ci, apps)
                        }),
                        crate::edit::CardButton::Size => self
                            .edit_change(idx, |c, _, _, apps| crate::edit::next_size(c, ci, apps)),
                        crate::edit::CardButton::Remove => self
                            .edit_change(idx, |c, _, _, apps| {
                                crate::edit::remove_card(c, ci, apps)
                            }),
                    };
                }
            }
        }
        if button == Button::Left {
            self.edit.drag = Some(crate::edit::CardDrag {
                card: ci,
                start_y: y,
                moved: false,
            });
            return true;
        }
        false
    }

    /// A button went down: on a hit, the card's Press; an unused right click starts editing; outside
    /// an open panel, it closes.
    pub fn click(&mut self, w: f64, h: f64, x: f64, y: f64, button: Button) -> bool {
        if self.ask.ask.on {
            let raw = match button {
                Button::Left => 0x110,
                Button::Right => 0x111,
                Button::Middle => 0x112,
            };
            return crate::ask::click(&mut self.ask, w, h, x, y, raw);
        }
        let Some(idx) = self.open else {
            return false;
        };
        if self.edit_menu.is_some() {
            let item = self.edit_menu_rect(w, h);
            self.edit_menu = None;
            if button == Button::Left
                && item.is_some_and(|r| x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3)
            {
                return self.enter_edit(w, h);
            }
            return true;
        }
        if self.editing() {
            return self.edit_press(idx, w, h, x, y, button);
        }
        let ((ox, oy, pw, ph), _) = self.layout(idx, w, h);
        if x < ox || x > ox + pw || y < oy || y > oy + ph {
            return self.close();
        }
        self.hover_close = None;
        let promoted = self.opened_by_hover && !std::mem::replace(&mut self.clicked_inside, true);
        let Some((ci, hit)) = self.hit_at((x, y)) else {
            if button == Button::Right {
                return self.offer_edit(idx, x, y) || promoted;
            }
            return promoted;
        };
        if button == Button::Left {
            self.pressed = Some((ci, hit));
        }
        let used = self.card_event(
            idx,
            ci,
            &Event::Press {
                hit,
                button,
                at: (x, y),
            },
        );
        if !used && button == Button::Right && self.open == Some(idx) {
            return self.offer_edit(idx, x, y) || promoted;
        }
        used || promoted
    }

    pub fn release(&mut self) -> bool {
        let Some(idx) = self.open else {
            return false;
        };
        if self.editing() {
            let Some(d) = self.edit.drag.take() else {
                return false;
            };
            let at = self.edit.drop_at.take();
            if d.moved {
                let changed = if let Some(to) = at {
                    self.edit_change(idx, |c, _, _, _| crate::edit::move_card(c, d.card, to))
                } else {
                    false
                };
                if !changed {
                    let (w, h) = self.last_screen;
                    self.update_edit_hover(w, h);
                }
                return true;
            }
            return false;
        }
        let Some((ci, hit)) = self.pressed.take() else {
            return false;
        };
        self.card_event(idx, ci, &Event::Release { hit })
    }

    pub fn scroll(&mut self, x: f64, y: f64, dy: f64) -> bool {
        let Some(idx) = self.open else {
            return false;
        };
        let (w, h) = self.last_screen;
        let step = if dy.abs() <= 3.0 { dy * 36.0 } else { dy * 3.0 };
        if self.editing() {
            let (pbox, _, content_h) = self.layout_local(idx, w, h);
            let (bar, add) = self.edit_rects(idx, w, h);
            if self.edit.adding && crate::edit::inside(add, x, y) {
                let cards = self.edit_cards(idx);
                let entries = crate::edit::add_entries(&cards, &self.apps);
                let total_h =
                    crate::edit::add_content_height(&entries, self.edit.expanded_add.as_deref());
                let view_h = crate::edit::add_viewport(add).3;
                let max = (total_h - view_h).max(0.0);
                let next = (self.edit.add_scroll + step).clamp(0.0, max);
                if (next - self.edit.add_scroll).abs() > 0.01 {
                    self.edit.add_scroll = next;
                    return true;
                }
                return false;
            }
            if crate::edit::inside(bar, x, y) {
                return false;
            }
            if crate::edit::inside(pbox, x, y) {
                let max = (content_h - pbox.3).max(0.0);
                let next = (self.panel_scroll + step).clamp(0.0, max);
                if (next - self.panel_scroll).abs() > 0.01 {
                    self.panel_scroll = next;
                    self.update_edit_hover(w, h);
                    return true;
                }
            }
            return false;
        }
        if let Some(ci) = self.card_at((x, y)) {
            let hit = self
                .hit_at((x, y))
                .filter(|(c, _)| *c == ci)
                .map(|(_, h)| h);
            if self.card_event(idx, ci, &Event::Scroll { hit, dy }) {
                return true;
            }
        }
        let (pbox, _, content_h) = self.layout_local(idx, w, h);
        if crate::edit::inside(pbox, x, y) {
            let max = (content_h - pbox.3).max(0.0);
            let next = (self.panel_scroll + step).clamp(0.0, max);
            if (next - self.panel_scroll).abs() > 0.01 {
                self.panel_scroll = next;
                return true;
            }
        }
        false
    }

    /// Hands `ev` to card `ci` of panel `idx`; its actions are queued with what to fetch after.
    fn card_event(&mut self, idx: usize, ci: usize, ev: &Event) -> bool {
        let editing = self.editing();
        let above = self.panels[idx].panel.side == Side::Bottom;
        let pc = &mut self.panels[idx].cards[ci];
        let mut out = Vec::new();
        let changed = {
            let cx = Cx {
                theme: &self.theme,
                now: self.now,
                size: pc.size,
                app: pc
                    .app_id
                    .as_deref()
                    .and_then(|id| self.apps.iter().find(|a| a.id == id)),
                apps: &self.apps,
                windows: &self.windows,
                notes: &self.notes,
                list_above: above,
                editing,
            };
            pc.card.event(ev, &cx, &mut out)
        };
        let (prov, keys) = (pc.provider.id(), pc.kind.sources);
        let card = format!("{}/{}", pc.word.owner, pc.word.kind);
        let mut requery = None;
        let mut show = None;
        let mut remove = false;
        for a in out {
            match a {
                Action::ShowCards(w) => show = Some(w),
                Action::ClosePanel => {
                    self.close();
                    return true;
                }
                Action::RemoveCard => remove = true,
                Action::SetQuery(q) => requery = Some(q),
                a => self.actions.push((a, card.clone(), prov, keys)),
            }
        }
        if remove {
            return self.remove_placed_card(idx, ci);
        }
        if let Some(w) = show {
            self.show_cards(idx, w);
            return true;
        }
        if let Some(q) = requery {
            // every card hears it, the one that asked too (a Text never asks again)
            self.query = q;
            self.send_text(idx);
            return true;
        }
        changed
    }

    /// Something shows: a panel, or the question.
    pub fn active(&self) -> bool {
        self.open.is_some() || self.ask.ask.on
    }

    pub fn is_closing(&self) -> bool {
        self.closing.is_some()
    }

    /// Something must be drawn on screen (open, asking, or fading out after close).
    pub fn visible(&self) -> bool {
        self.active() || self.closing.is_some()
    }

    /// What is drawn on a `w x h` screen: the question's card while it asks, else the open
    /// panel's box (plus the edit bar and Add list while editing), or a closing panel's box.
    pub fn shown_box(&self, w: f64, h: f64) -> Option<Rect> {
        if self.ask.ask.on {
            return Some(self.ask.ask.geometry(w, h));
        }
        if let Some(idx) = self.open {
            let pbox = self.layout_local(idx, w, h).0;
            if !self.editing() {
                return Some(pbox);
            }
            let (bar, add) = self.edit_rects(idx, w, h);
            let mut out = union_rect(pbox, bar);
            if self.edit.adding {
                out = union_rect(out, add);
            }
            return Some(out);
        }
        let c = self.closing.as_ref()?;
        let ps = c.transient.as_ref().or_else(|| self.panels.get(c.idx))?;
        Some(self.layout_panel_local(ps, w, h).0)
    }

    /// A request in the ask file (SIGRTMIN+3), or a cancel: loaded, queued after one already
    /// asking. True when what shows changed. A question closes the open panel.
    pub fn ask_load(&mut self) -> bool {
        let cancel = crate::wayland::runtime_path(crate::ask::ASK_CANCEL_FILE);
        let mut changed = crate::ask::handle_cancel_from_path(&mut self.ask, &cancel);
        let req = crate::wayland::runtime_path(crate::ask::ASK_FILE);
        changed |= crate::ask::load_from_path(&mut self.ask, &req);
        changed |= crate::ask::check_asker_gone(&mut self.ask);
        if self.ask.ask.on {
            if self.open.is_some() {
                self.close();
                changed = true;
            }
            self.closing = None;
        }
        changed
    }

    /// What was typed, as if typed now (renders and tests).
    pub fn type_text(&mut self, q: &str) {
        if let Some(idx) = self.open {
            self.query = q.to_string();
            self.send_text(idx);
        }
    }

    /// A new panel's typing starts empty, and its cards hear so (a search shows everything again).
    fn reset_keys(&mut self, idx: usize) {
        self.query.clear();
        self.selected = None;
        self.keyboard = false;
        for ci in 0..self.panels[idx].cards.len() {
            self.card_event(idx, ci, &Event::Text(""));
        }
    }

    /// The query to every card of the open panel; the selection starts over. True when one
    /// changed.
    fn send_text(&mut self, idx: usize) -> bool {
        self.selected = None;
        let q = self.query.clone();
        let mut changed = false;
        for ci in 0..self.panels[idx].cards.len() {
            changed |= self.card_event(idx, ci, &Event::Text(&q));
            if self.open != Some(idx) {
                break;
            }
        }
        changed
    }

    /// Every hit of the last frame in order (the cards top to bottom, each card's in the order it
    /// added them), once each.
    fn hit_order(&self) -> Vec<(usize, u32, Rect)> {
        let mut out: Vec<(usize, u32, Rect)> = Vec::new();
        for (ci, _, hits) in self.frame.borrow().iter() {
            for (id, r) in &hits.list {
                if !out.iter().any(|(c, i, _)| c == ci && i == id) {
                    out.push((*ci, *id, *r));
                }
            }
        }
        out
    }

    fn scroll_to_hit(&mut self, idx: usize, r: Rect, is_first: bool) {
        let (w, h) = self.frame_screen.get();
        let (pbox, _, content_h) = self.layout_local(idx, w, h);
        let max_scroll = (content_h - pbox.3).max(0.0);
        if max_scroll <= 0.0 {
            self.panel_scroll = 0.0;
            return;
        }
        let cur_scroll = self.panel_scroll.clamp(0.0, max_scroll);
        let frame_scroll = self.frame_scroll.get();
        let ry = r.1 + frame_scroll - cur_scroll;
        let top = pbox.1;
        let bot = pbox.1 + pbox.3;
        let mut next = cur_scroll;
        if ry + r.3 > bot {
            next = cur_scroll + (ry + r.3 - bot);
        }
        let ry_after = r.1 + frame_scroll - next;
        if ry_after < top {
            next = cur_scroll + (ry - top);
        }
        if is_first && (r.1 + frame_scroll + r.3) <= bot {
            next = 0.0;
        }
        self.panel_scroll = next.clamp(0.0, max_scroll);
    }

    /// A key while a panel is open. True when something must be drawn again.
    pub fn key(&mut self, k: Key) -> bool {
        // the question first: it takes the keyboard while it asks, panel or not
        if self.ask.ask.on {
            let (sym, ch) = match k {
                Key::Escape => (0xff1b, None),
                Key::Enter => (0xff0d, None),
                Key::Up => (0xff52, None),
                Key::Down => (0xff54, None),
                Key::Tab => (0xff09, None),
                Key::BackTab => (0xfe20, None),
                Key::Backspace => (0xff08, None),
                Key::Char(c) => (0, Some(c)),
                Key::Super(_) => return false,
            };
            return crate::ask::key(&mut self.ask, sym, ch);
        }
        let Some(idx) = self.open else {
            return false;
        };
        if self.editing() {
            return match k {
                Key::Escape => {
                    if self.edit.adding {
                        self.edit.adding = false;
                        self.edit.expanded_add = None;
                        self.edit.add_scroll = 0.0;
                        true
                    } else {
                        self.finish_edit()
                    }
                }
                Key::Super(c) => {
                    let c = c.to_ascii_lowercase();
                    self.finish_edit();
                    if self
                        .open
                        .is_some_and(|i| self.panels[i].panel.key == Some(c))
                    {
                        self.close()
                    } else if let Some(other) =
                        self.panels.iter().position(|p| p.panel.key == Some(c))
                    {
                        self.open_panel(other, false, Instant::now())
                    } else {
                        true
                    }
                }
                _ => false,
            };
        }
        if k == Key::Escape && self.edit_menu.take().is_some() {
            return true;
        }
        match k {
            Key::Escape => {
                if self.query.is_empty() {
                    self.close()
                } else {
                    self.query.clear();
                    self.send_text(idx);
                    true
                }
            }
            Key::Super(c) => {
                let c = c.to_ascii_lowercase();
                if self.panels[idx].panel.key == Some(c) {
                    self.close()
                } else if let Some(other) = self.panels.iter().position(|p| p.panel.key == Some(c))
                {
                    self.open_panel(other, false, Instant::now())
                } else {
                    false
                }
            }
            Key::Up | Key::Down | Key::Tab | Key::BackTab => {
                let order = self.hit_order();
                if order.is_empty() {
                    return false;
                }
                let at = self
                    .selected
                    .and_then(|(c, h)| order.iter().position(|(oc, oh, _)| *oc == c && *oh == h));
                // the first arrow while the selection is hidden shows it where it is
                let next = match (at, self.keyboard) {
                    (Some(i), false) => i,
                    (None, _) => {
                        if matches!(k, Key::Up | Key::BackTab) {
                            order.len() - 1
                        } else {
                            0
                        }
                    }
                    (Some(i), true) => {
                        if matches!(k, Key::Up | Key::BackTab) {
                            (i + order.len() - 1) % order.len()
                        } else {
                            (i + 1) % order.len()
                        }
                    }
                };
                self.keyboard = true;
                self.selected = Some((order[next].0, order[next].1));
                self.scroll_to_hit(idx, order[next].2, next == 0);
                true
            }
            Key::Enter => {
                if let (true, Some((ci, hit))) = (self.keyboard, self.selected) {
                    let r = self
                        .hit_order()
                        .into_iter()
                        .find(|(c, h, _)| *c == ci && *h == hit)
                        .map_or((0.0, 0.0, 0.0, 0.0), |(_, _, r)| r);
                    let dy = self.frame_scroll.get() - self.panel_scroll;
                    let ev = Event::Press {
                        hit,
                        button: Button::Left,
                        at: (r.0 + r.2 / 2.0, r.1 + dy + r.3 / 2.0),
                    };
                    let changed = self.card_event(idx, ci, &ev);
                    if self.open == Some(idx) {
                        self.card_event(idx, ci, &Event::Release { hit });
                    }
                    return changed;
                }
                // nothing selected: the first card that has a default takes it
                for ci in 0..self.panels[idx].cards.len() {
                    if self.card_event(idx, ci, &Event::Enter) {
                        return true;
                    }
                    if self.open != Some(idx) {
                        return true;
                    }
                }
                false
            }
            Key::Backspace => {
                if self.query.pop().is_some() {
                    self.send_text(idx);
                    true
                } else {
                    false
                }
            }
            Key::Char(c) => {
                if c.is_control() {
                    return false;
                }
                self.query.push(c);
                self.send_text(idx);
                true
            }
        }
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
        if let Some(c) = &mut self.closing {
            let ms = now.duration_since(c.started).as_secs_f64() * 1000.0;
            if ms < LEAVE_MS {
                c.ms = ms;
                running = true;
            } else {
                self.closing = None;
            }
        }
        if let Some(idx) = self.open {
            for pc in &mut self.panels[idx].cards {
                running |= pc.card.tick(dt);
            }
        }
        running |= self.drag_autoscroll(self.last_screen.0, self.last_screen.1, dt);
        if !running {
            self.last_tick = None;
        }
        running
    }

    fn drag_autoscroll(&mut self, w: f64, h: f64, dt_ms: f64) -> bool {
        let Some(idx) = self.open else {
            return false;
        };
        if !self.editing() || !self.edit.drag.is_some_and(|d| d.moved) {
            return false;
        }
        let Some((x, y)) = self.pointer else {
            return false;
        };
        let (pbox, local_rects, content_h) = self.layout_local(idx, w, h);
        let max_scroll = (content_h - pbox.3).max(0.0);
        if max_scroll <= 0.0 || x < pbox.0 || x > pbox.0 + pbox.2 {
            return false;
        }
        let edge = 36.0;
        let speed = if y < pbox.1 + edge {
            -((1.0 - ((y - pbox.1).max(0.0) / edge)) * 420.0)
        } else if y > pbox.1 + pbox.3 - edge {
            (1.0 - ((pbox.1 + pbox.3 - y).max(0.0) / edge)) * 420.0
        } else {
            0.0
        };
        if speed == 0.0 {
            return false;
        }
        let next = (self.panel_scroll + speed * (dt_ms / 1000.0)).clamp(0.0, max_scroll);
        if (next - self.panel_scroll).abs() < 0.01 {
            return false;
        }
        self.panel_scroll = next;
        let drag_idx = self.edit.drag.unwrap().card;
        let cards = self.edit_cards(idx);
        self.edit.drop_at = crate::edit::drop_index(
            &local_rects,
            cards.len(),
            drag_idx,
            x - pbox.0,
            y - pbox.1 + next,
        );
        true
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

    pub fn wants_clock(&self) -> bool {
        self.open
            .is_some_and(|idx| self.panels[idx].cards.iter().any(|pc| pc.card.wants_clock()))
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

    pub fn wants_frame(&self) -> bool {
        if self.opened_at.is_some() || self.closing.is_some() {
            return true;
        }
        let Some(idx) = self.open else {
            return false;
        };
        if self.editing() && self.edit.drag.is_some_and(|d| d.moved) {
            if let Some((x, y)) = self.pointer {
                let (pbox, _, content_h) =
                    self.layout_local(idx, self.last_screen.0, self.last_screen.1);
                if content_h > pbox.3
                    && x >= pbox.0
                    && x <= pbox.0 + pbox.2
                    && (y < pbox.1 + 36.0 || y > pbox.1 + pbox.3 - 36.0)
                {
                    return true;
                }
            }
        }
        false
    }

    /// Draws the open (or closing) panel on a screen of `w x h`: each card's box, header and body,
    /// rising in as the panel opens or fading out as it closes; keeps each card's hits for the
    /// pointer while open. Returns the panel's box.
    pub fn draw(&self, cr: &cairo::Context, w: f64, h: f64) -> Option<Rect> {
        if self.ask.ask.on {
            return Some(crate::ask::draw(cr, &self.theme, &self.ask.ask, (w, h)));
        }
        if let Some(idx) = self.open {
            return Some(self.draw_panel(cr, w, h, &self.panels[idx], idx, self.panel_scroll, None));
        }
        if let Some(c) = &self.closing {
            let ps = c.transient.as_ref().or_else(|| self.panels.get(c.idx))?;
            let cp = (1.0 - crate::ui::ease((c.ms / LEAVE_MS).clamp(0.0, 1.0))).clamp(0.0, 1.0);
            return Some(self.draw_panel(cr, w, h, ps, c.idx, c.scroll, Some(cp)));
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_panel(
        &self,
        cr: &cairo::Context,
        w: f64,
        h: f64,
        ps: &PanelState,
        idx: usize,
        scroll_pos: f64,
        closing_p: Option<f64>,
    ) -> Rect {
        let interactive = closing_p.is_none();
        let editing = interactive && self.editing();
        let (pbox, local_rects, content_h) = self.layout_panel_local(ps, w, h);
        let max_scroll = (content_h - pbox.3).max(0.0);
        let scroll = scroll_pos.clamp(0.0, max_scroll);
        let rects: Vec<(usize, Rect)> = local_rects
            .iter()
            .map(|&(ci, r)| (ci, (pbox.0 + r.0, pbox.1 + r.1 - scroll, r.2, r.3)))
            .collect();
        let above = ps.panel.side == Side::Bottom;
        let th = &self.theme;
        let mut frame = Vec::new();
        let bottom = rects.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        cr.save().ok();
        let clip_lift = if closing_p.is_some() {
            LEAVE_DROP
        } else if self.open_ms.is_finite() {
            ARRIVE_LIFT
        } else {
            0.0
        };
        cr.rectangle(pbox.0, pbox.1, pbox.2, pbox.3 + clip_lift);
        cr.clip();
        for (ci, r) in &rects {
            let pc = &ps.cards[*ci];
            let cx = self.cx(pc.size, pc.app_id.as_deref(), above);
            let status = pc.card.status(&cx);
            let sel = if !interactive || editing {
                None
            } else {
                self.selected
                    .filter(|(c, _)| self.keyboard && c == ci)
                    .map(|(_, h)| h)
            };
            let mut hits = Hits::new(
                if !interactive || editing {
                    None
                } else {
                    self.pointer
                },
                sel,
            );
            // the bottom card arrives first (about one card per 90 px up)
            let k = ((bottom - (r.1 + r.3)) / 90.0).clamp(0.0, ARRIVE_CARDS);
            let p = if let Some(cp) = closing_p {
                cp
            } else if self.open_ms.is_finite() {
                crate::ui::ease((self.open_ms - k * ARRIVE_STAGGER_MS) / ARRIVE_MS)
            } else {
                1.0
            };
            if p <= 0.0 {
                if interactive {
                    frame.push((*ci, *r, hits));
                }
                continue;
            }
            if editing && status == Status::Hidden {
                let dash = [4.0, 3.0];
                draw::round_rect(
                    cr,
                    r.0 + 0.5,
                    r.1 + 0.5,
                    r.2 - 1.0,
                    r.3 - 1.0,
                    th.corners.min(7.0),
                );
                draw::set(cr, th.muted.alpha(0.35));
                cr.set_line_width(1.0);
                cr.set_dash(&dash, 0.0);
                let _ = cr.stroke();
                cr.set_dash(&[], 0.0);
                let title = pc
                    .card
                    .title(&cx)
                    .unwrap_or_else(|| pc.kind.title.to_string());
                let name = Text {
                    size: 13.0,
                    bold: true,
                    color: th.text.alpha(0.75),
                    text: &title,
                };
                let (nw, _) = draw::measure(cr, &name);
                draw::text(cr, r.0 + PAD_X, r.1 + (r.3.min(52.0) - 15.0) / 2.0, &name);
                draw::text_max(
                    cr,
                    r.0 + PAD_X + nw + 10.0,
                    r.1 + (r.3.min(52.0) - 13.0) / 2.0 + 1.0,
                    (r.2 - 2.0 * PAD_X - nw - 10.0).max(10.0),
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: th.muted,
                        text: "nothing to show now",
                    },
                );
                frame.push((*ci, *r, hits));
                continue;
            }
            // a card without its header only while it shows itself (Loading and Missing say
            // whose they are)
            let headed = pc.card.header(&cx) || status != Status::Ready;
            let head = if headed { HEAD_H } else { 0.0 };
            let body = (
                r.0 + PAD_X - 8.0,
                r.1 + PAD_TOP + head,
                r.2 - 2.0 * (PAD_X - 8.0),
                r.3 - PAD_TOP - head - PAD_BOTTOM,
            );
            let lifted = p < 1.0;
            if lifted {
                let drop_y = if closing_p.is_some() {
                    LEAVE_DROP
                } else {
                    ARRIVE_LIFT
                };
                cr.push_group();
                cr.translate(0.0, (1.0 - p) * drop_y);
            }
            draw::card(cr, th, r.0, r.1, r.2, r.3);
            if headed {
                let title = pc
                    .card
                    .title(&cx)
                    .unwrap_or_else(|| pc.kind.title.to_string());
                draw_header(cr, th, *r, &title, &pc.from);
            }
            match status {
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
                    let l = missing_layout(cr, body.2 - 16.0, &why);
                    draw::set(cr, th.muted);
                    cr.move_to((body.0 + 8.0).round(), body.1.round());
                    pangocairo::functions::show_layout(cr, &l);
                }
                Status::Hidden => {}
            }
            if lifted {
                let _ = cr.pop_group_to_source();
                let _ = cr.paint_with_alpha(p);
            }
            if interactive {
                frame.push((*ci, *r, hits));
            }
        }
        if max_scroll > 0.0 {
            let thumb_h = ((pbox.3 / content_h) * pbox.3).clamp(24.0, pbox.3);
            let track = (pbox.3 - thumb_h).max(1.0);
            let thumb_y = pbox.1 + (scroll / max_scroll) * track;
            draw::round_rect(cr, pbox.0 + pbox.2 - 4.0, thumb_y, 3.0, thumb_h, 1.5);
            draw::set(cr, th.muted.alpha(0.35));
            let _ = cr.fill();
        }
        cr.restore().ok();
        if editing {
            let cards = self.edit_cards(idx);
            let (bar, add) = self.edit_rects(idx, w, h);
            let mut overlay_state = self.edit.clone();
            overlay_state.bar_rect = bar;
            overlay_state.add_rect = add;
            let overlay_cx = self.cx(Size::M, None, above);
            crate::edit::draw_overlay(
                cr,
                &overlay_cx,
                &ps.panel,
                &cards,
                &local_rects,
                (pbox.0, pbox.1),
                (pbox.2, pbox.3),
                scroll,
                self.pointer,
                &overlay_state,
            );
        }
        if interactive {
            if let Some(r) = self.edit_menu_rect(w, h) {
                let th = &self.theme;
                crate::draw::round_rect(cr, r.0, r.1, r.2, r.3, th.small_radius());
                crate::draw::set(cr, th.card.alpha(1.0));
                let _ = cr.fill_preserve();
                crate::draw::set(cr, th.edge);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
                let lit = self.pointer.is_some_and(|(px, py)| {
                    px >= r.0 && px <= r.0 + r.2 && py >= r.1 && py <= r.1 + r.3
                });
                if lit {
                    crate::draw::round_rect(
                        cr,
                        r.0 + 3.0,
                        r.1 + 3.0,
                        r.2 - 6.0,
                        r.3 - 6.0,
                        th.small_radius(),
                    );
                    crate::draw::set(cr, th.text.alpha(0.08));
                    let _ = cr.fill();
                }
                crate::draw::text_max(
                    cr,
                    r.0 + 12.0,
                    r.1 + (r.3 - 17.0) / 2.0,
                    r.2 - 24.0,
                    &crate::draw::Text {
                        size: 13.0,
                        bold: false,
                        color: th.text,
                        text: "Edit panel",
                    },
                );
            }
            *self.frame.borrow_mut() = frame;
            self.frame_scroll.set(scroll);
            self.frame_screen.set((w, h));
        }
        pbox
    }
}

fn header_title_max_w(card_w: f64, from_w: f64) -> (f64, f64) {
    let head_w = (card_w - 2.0 * PAD_X).max(40.0);
    // Keep at least half of the header for the title when the app name is long.
    let max_from = ((head_w - 12.0) * 0.45).max(0.0);
    let capped_from = from_w.min(max_from);
    let title_w = if capped_from > 0.0 {
        (head_w - capped_from - 12.0).max(40.0)
    } else {
        head_w
    };
    (capped_from, title_w)
}

fn draw_header(cr: &cairo::Context, th: &Theme, r: Rect, title: &str, from: &str) {
    let from_w = if from.is_empty() {
        0.0
    } else {
        let from_t = Text {
            size: 12.0,
            bold: false,
            color: th.muted,
            text: from,
        };
        let full_w = draw::measure(cr, &from_t).0;
        let (capped, _) = header_title_max_w(r.2, full_w);
        if full_w <= capped {
            draw::text_right(cr, r.0 + r.2 - PAD_X, r.1 + PAD_TOP + 3.0, &from_t)
        } else {
            draw::text_max(
                cr,
                r.0 + r.2 - PAD_X - capped,
                r.1 + PAD_TOP + 3.0,
                capped,
                &from_t,
            );
            capped
        }
    };
    let (_, title_w) = header_title_max_w(r.2, from_w);
    draw::text_max(
        cr,
        r.0 + PAD_X,
        r.1 + PAD_TOP,
        title_w,
        &Text {
            size: 15.0,
            bold: true,
            color: th.text,
            text: title,
        },
    );
}

fn missing_layout(cr: &cairo::Context, max_w: f64, why: &str) -> pango::Layout {
    let l = pangocairo::functions::create_layout(cr);
    let mut fd = pango::FontDescription::from_string(draw::FONT);
    fd.set_absolute_size(draw::SIZE_SMALL * pango::SCALE as f64);
    l.set_font_description(Some(&fd));
    l.set_width((max_w.max(20.0) * pango::SCALE as f64) as i32);
    l.set_wrap(pango::WrapMode::WordChar);
    l.set_ellipsize(pango::EllipsizeMode::End);
    l.set_height(-2);
    l.set_text(why);
    l
}

fn wrapped_text_height(max_w: f64, text: &str) -> f64 {
    let Ok(surf) = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1) else {
        return 20.0;
    };
    let Ok(cr) = cairo::Context::new(&surf) else {
        return 20.0;
    };
    let l = missing_layout(&cr, max_w, text);
    (l.pixel_size().1 as f64).max(20.0)
}

fn missing_height(card_width: f64, why: &str) -> f64 {
    wrapped_text_height(card_width - 2.0 * PAD_X, why)
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.0.min(b.0);
    let y0 = a.1.min(b.1);
    let x1 = (a.0 + a.2).max(b.0 + b.2);
    let y1 = (a.1 + a.3).max(b.1 + b.3);
    (x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::parse_power;

    fn shell(line: &str) -> Shell {
        Shell::new(
            Config::from_text(&format!("PANEL_CONTROL=\"{line}\"\n")),
            Vec::new(),
        )
    }

    fn windows_shell() -> (Shell, usize) {
        let mut sh = shell("SUPER+A top right 380 windows/list");
        sh.windows = vec![
            Window {
                id: 1,
                title: "Firefox".into(),
                app_id: "firefox".into(),
                focused: true,
            },
            Window {
                id: 3,
                title: "Steam".into(),
                app_id: "steam".into(),
                focused: false,
            },
        ];
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;
        (sh, idx)
    }

    /// Draws once, as a frame would: the hits keyboard moves go over.
    fn frame(sh: &Shell) {
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);
    }

    #[test]
    fn a_tray_apps_cards_take_the_panels_place_in_the_chosen_order_and_go_on_close() {
        for (choice, want) in [
            ("", vec!["apps/menu"]),
            (
                "TRAY_CARD=\"menu-first\"\n",
                vec!["apps/menu", "calendar/clock"],
            ),
            (
                "TRAY_CARD=\"card-first\"\n",
                vec!["calendar/clock", "apps/menu"],
            ),
            ("TRAY_CARD=\"menu\"\n", vec!["apps/menu"]),
        ] {
            let mut sh = Shell::new(
                Config::from_text(&format!(
                    "PANEL_CONTROL=\"SUPER+A top right 380 apps/background\"\n{choice}"
                )),
                Vec::new(),
            );
            let n = sh.panels.len();
            let idx = sh
                .panels
                .iter()
                .position(|p| p.panel.name == "control")
                .unwrap();
            sh.open_panel(idx, false, Instant::now());
            sh.show_cards(idx, vec!["apps/menu=steam".into(), "calendar/clock".into()]);
            let t = sh.open.unwrap();
            assert_eq!(sh.transient, Some(t));
            let words: Vec<String> = sh.panels[t]
                .cards
                .iter()
                .map(|c| format!("{}/{}", c.word.owner, c.word.kind))
                .collect();
            assert_eq!(words, want, "{choice}");
            assert!(!sh.enter_edit(1920.0, 1080.0), "not editable");
            sh.close();
            assert_eq!(sh.panels.len(), n);
            assert_eq!(sh.transient, None);
        }
    }

    #[test]
    fn typing_filters_and_enter_takes_the_first_match() {
        let (mut sh, _) = windows_shell();
        frame(&sh);
        for c in "ste".chars() {
            assert!(sh.key(Key::Char(c)));
        }
        assert_eq!(sh.query, "ste");
        assert!(sh.key(Key::Enter));
        let acts: Vec<&Action> = sh.actions.iter().map(|(a, ..)| a).collect();
        assert_eq!(acts, vec![&Action::Focus(3)]);
        // the Focus closes the panel through the core, ClosePanel already did
        assert!(sh.open.is_none());
        assert!(sh.query.is_empty());
    }

    #[test]
    fn escape_clears_the_query_first_then_closes() {
        let (mut sh, _) = windows_shell();
        sh.key(Key::Char('x'));
        assert!(sh.key(Key::Backspace));
        assert!(sh.query.is_empty());
        assert!(!sh.key(Key::Backspace));
        sh.key(Key::Char('x'));
        assert!(sh.key(Key::Escape));
        assert!(sh.open.is_some());
        assert!(sh.key(Key::Escape));
        assert!(sh.open.is_none());
    }

    #[test]
    fn arrows_show_the_selection_then_move_it_and_enter_presses_it() {
        let (mut sh, _) = windows_shell();
        frame(&sh);
        let order = sh.hit_order();
        assert!(order.len() >= 2);
        // the first arrow shows it on the first hit, not past it
        assert!(sh.key(Key::Down));
        assert!(sh.keyboard);
        assert_eq!(sh.selected, Some((order[0].0, order[0].1)));
        sh.key(Key::Down);
        assert_eq!(sh.selected, Some((order[1].0, order[1].1)));
        sh.key(Key::Up);
        sh.key(Key::Up);
        // wraps to the last
        let last = order.last().unwrap();
        assert_eq!(sh.selected, Some((last.0, last.1)));
        // the pointer moving hides it
        assert!(sh.pointer_motion(1280.0, 800.0, 1100.0, 60.0));
        assert!(!sh.keyboard);
        // first row, Enter: its window
        sh.key(Key::Tab);
        sh.selected = Some((order[0].0, order[0].1));
        assert!(sh.key(Key::Enter));
        let acts: Vec<&Action> = sh.actions.iter().map(|(a, ..)| a).collect();
        assert_eq!(acts, vec![&Action::Focus(1)]);
    }

    #[test]
    fn super_with_another_panels_key_switches_to_it() {
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 windows/list\"\nPANEL_CLIP=\"SUPER+K top center 380 clipboard/history\"\n",
            ),
            Vec::new(),
        );
        let a = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let v = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "clip")
            .unwrap();
        sh.open_panel(a, false, Instant::now());
        assert!(sh.key(Key::Super('K')));
        assert_eq!(sh.open, Some(v));
        assert!(sh.key(Key::Super('k')));
        assert!(sh.open.is_none());
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
        let t0 = Instant::now();
        assert!(!sh.pointer_motion_at(1920.0, 1080.0, s.0 + 10.0, s.1 + 1.0, t0));
        assert!(sh.open.is_none());
        let t1 = t0 + std::time::Duration::from_millis(HOVER_OPEN_MS as u64);
        assert!(sh.hover_tick(t1));
        assert_eq!(sh.open, Some(idx));
        assert!(sh.opened_by_hover);
        assert!(!sh.pointer_motion_at(1920.0, 1080.0, 100.0, 100.0, t1));
        assert_eq!(sh.open, Some(idx));
        assert!(
            sh.hover_tick(t1 + std::time::Duration::from_millis(HOVER_CLOSE_MS as u64)),
            "left past grace: closed"
        );
        assert!(sh.open.is_none());
    }

    #[test]
    fn hover_dwell_ignores_fast_pass_and_grace_cancels_on_reentry() {
        let mut sh = shell("SUPER+A bottom right 380 hover settings/power");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let s = sh.hover_strip(idx, 1920.0, 1080.0);
        let t0 = Instant::now();

        // Fast pass through the 2 px strip (leaves within 40 ms) does not open.
        assert!(!sh.pointer_motion_at(1920.0, 1080.0, s.0 + 10.0, s.1 + 1.0, t0));
        assert!(sh.hover_timeout(t0).is_some());
        assert!(!sh.hover_tick(t0 + std::time::Duration::from_millis(40)));
        assert!(!sh.pointer_motion_at(
            1920.0,
            1080.0,
            s.0 + 10.0,
            s.1 - 20.0,
            t0 + std::time::Duration::from_millis(50)
        ));
        assert!(sh.hover_timeout(t0 + std::time::Duration::from_millis(50)).is_none());
        assert!(!sh.hover_tick(t0 + std::time::Duration::from_millis(300)));
        assert!(sh.open.is_none());

        // Leaving via pointer_leave_at while dwelling also cancels opening.
        let t_enter = t0 + std::time::Duration::from_millis(400);
        sh.pointer_motion_at(1920.0, 1080.0, s.0 + 10.0, s.1 + 1.0, t_enter);
        sh.pointer_leave_at(t_enter + std::time::Duration::from_millis(30));
        assert!(!sh.hover_tick(t_enter + std::time::Duration::from_millis(200)));
        assert!(sh.open.is_none());

        // Resting in the strip for HOVER_OPEN_MS opens it.
        let t_dwell = t_enter + std::time::Duration::from_millis(300);
        sh.pointer_motion_at(1920.0, 1080.0, s.0 + 10.0, s.1 + 1.0, t_dwell);
        assert!(!sh.hover_tick(t_dwell + std::time::Duration::from_millis(119)));
        assert!(sh.hover_tick(t_dwell + std::time::Duration::from_millis(120)));
        assert_eq!(sh.open, Some(idx));

        // Leave the hover box, then return within HOVER_CLOSE_MS: stays open.
        let t_out = t_dwell + std::time::Duration::from_millis(200);
        let b = sh.hover_box(idx, 1920.0, 1080.0);
        sh.pointer_motion_at(1920.0, 1080.0, 50.0, 50.0, t_out);
        assert!(!sh.hover_tick(t_out + std::time::Duration::from_millis(200)));
        assert_eq!(sh.open, Some(idx));
        sh.pointer_motion_at(
            1920.0,
            1080.0,
            b.0 + 10.0,
            b.1 + 10.0,
            t_out + std::time::Duration::from_millis(210),
        );
        assert!(!sh.hover_tick(t_out + std::time::Duration::from_millis(500)));
        assert_eq!(sh.open, Some(idx));

        // Leave again (via pointer_leave_at) and stay out past HOVER_CLOSE_MS: closes.
        let t_leave = t_out + std::time::Duration::from_millis(600);
        sh.pointer_leave_at(t_leave);
        assert!(!sh.hover_tick(t_leave + std::time::Duration::from_millis(249)));
        assert!(sh.hover_tick(t_leave + std::time::Duration::from_millis(250)));
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

    #[test]
    fn close_runs_leaving_animation_for_leave_ms_then_stops() {
        let mut sh = shell("SUPER+A top right settings/session");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let t0 = Instant::now();
        assert!(sh.open_panel(idx, false, t0));
        sh.open_ms = f64::INFINITY;
        sh.opened_at = None;

        let t_close = t0 + std::time::Duration::from_millis(500);
        assert!(sh.close_at(t_close));
        assert!(sh.open.is_none(), "logically closed at once");
        assert!(!sh.active(), "input/keyboard cleared at once");
        assert!(sh.is_closing());
        assert!(sh.visible());
        assert!(sh.wants_frame());

        assert!(sh.tick(t_close));
        assert!(sh.tick(t_close + std::time::Duration::from_millis(60)));
        assert!(sh.shown_box(1280.0, 800.0).is_some());
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        assert!(sh.draw(&cr, 1280.0, 800.0).is_some());

        assert!(
            !sh.tick(t_close + std::time::Duration::from_millis(LEAVE_MS as u64)),
            "stops after LEAVE_MS"
        );
        assert!(!sh.is_closing());
        assert!(!sh.visible());
        assert!(sh.shown_box(1280.0, 800.0).is_none());
    }

    fn parse_box_center(dump: &str, prefix: &str) -> (f64, f64) {
        let line = dump
            .lines()
            .find(|l| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing box line starting with {prefix:?} in:\n{dump}"));
        let nums: Vec<f64> = line
            .split_whitespace()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|s| s.parse().unwrap())
            .collect();
        (nums[0] + nums[2] / 2.0, nums[1] + nums[3] / 2.0)
    }

    fn box_rect(dump: &str, prefix: &str) -> Rect {
        let line = dump
            .lines()
            .find(|l| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing box line starting with {prefix:?} in:\n{dump}"));
        let nums: Vec<f64> = line
            .split_whitespace()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|s| s.parse().unwrap())
            .collect();
        (nums[0], nums[1], nums[2], nums[3])
    }

    #[test]
    fn right_click_starts_editing_shows_hidden_placeholder_and_ignores_card_clicks() {
        // notifications/list is Status::Hidden when notes are empty
        let mut sh = shell("SUPER+A top right 380 hover notifications/list settings/power");
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n") as &dyn std::any::Any,
        );
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let strip = sh.hover_strip(idx, 1920.0, 1080.0);
        let t0 = Instant::now();
        assert!(!sh.pointer_motion_at(1920.0, 1080.0, strip.0 + 10.0, strip.1 + 1.0, t0));
        assert!(sh.hover_tick(t0 + std::time::Duration::from_millis(HOVER_OPEN_MS as u64)));
        assert!(sh.opened_by_hover);
        sh.open_ms = f64::INFINITY;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1920, 1080).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1920.0, 1080.0);
        // Before editing, notifications/list is Hidden (only 1 card in layout)
        assert_eq!(sh.layout(idx, 1920.0, 1080.0).1.len(), 1);

        // Right-click on the settings/power hit (which does not use Button::Right) starts edit
        let (_, power_r) = {
            let f = sh.frame.borrow();
            *f[0].2.list.first().unwrap()
        };
        assert!(sh.click(
            1920.0,
            1080.0,
            power_r.0 + 4.0,
            power_r.1 + 4.0,
            Button::Right
        ));
        // a right click alone only offers the menu; Escape takes it away without editing
        assert!(!sh.editing());
        assert!(sh.edit_menu.is_some());
        assert!(sh.key(Key::Escape));
        assert!(sh.edit_menu.is_none() && !sh.editing() && sh.open.is_some());
        sh.click(
            1920.0,
            1080.0,
            power_r.0 + 4.0,
            power_r.1 + 4.0,
            Button::Right,
        );
        let m = sh.edit_menu_rect(1920.0, 1080.0).unwrap();
        assert!(sh.click(1920.0, 1080.0, m.0 + 10.0, m.1 + 10.0, Button::Left));
        assert!(sh.editing());
        assert!(!sh.opened_by_hover);

        // Hover-to-close is off while editing
        sh.pointer_motion(1920.0, 1080.0, 10.0, 10.0);
        assert_eq!(sh.open, Some(idx));

        // While editing: Status::Hidden notifications/list now shows placeholder height
        sh.draw(&cr, 1920.0, 1080.0);
        let rects = sh.layout(idx, 1920.0, 1080.0).1;
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].1 .3, crate::edit::PLACEHOLDER_H);

        // Clicking inside a card's own controls does nothing while editing (no actions queued)
        assert!(sh.click(
            1920.0,
            1080.0,
            rects[1].1 .0 + 20.0,
            rects[1].1 .1 + 40.0,
            Button::Left
        ));
        assert!(sh.release() || !sh.release());
        assert!(sh.actions.is_empty());

        let dump = crate::boxes::text(&sh, 1920.0, 1080.0);
        assert!(dump.contains("panel control "));
        assert!(dump.contains(" editing top"));
        assert!(dump.contains("barbutton Undo "));
        assert!(dump.contains("barbutton Done "));
    }

    #[test]
    fn card_buttons_drag_undo_reset_and_add_work_via_boxes() {
        let apps = vec![App {
            id: "alacritty".into(),
            name: "Alacritty".into(),
            wm_class: None,
            exec: "alacritty".into(),
            icon: "utilities-terminal".into(),
            terminal: true,
            categories: vec!["System".into()],
        }];
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 calendar/clock:s settings/network settings/bluetooth\"\n",
            ),
            apps,
        );
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        sh.open_panel(idx, false, Instant::now());
        sh.open_ms = f64::INFINITY;
        frame(&sh);

        // Right-click empty header area starts editing
        let (pbox, _) = sh.layout(idx, 1920.0, 1080.0);
        assert!(sh.click(1920.0, 1080.0, pbox.0 + 10.0, pbox.1 + 10.0, Button::Right));
        // a click elsewhere takes the menu away without editing
        assert!(sh.click(1920.0, 1080.0, pbox.0 + 30.0, pbox.1 + 60.0, Button::Left));
        assert!(!sh.editing() && sh.edit_menu.is_none());
        sh.click(1920.0, 1080.0, pbox.0 + 10.0, pbox.1 + 10.0, Button::Right);
        let m = sh.edit_menu_rect(1920.0, 1080.0).unwrap();
        assert!(sh.click(1920.0, 1080.0, m.0 + 10.0, m.1 + 10.0, Button::Left));
        assert!(sh.editing());

        // Use cardbutton Down on calendar/clock
        let d1 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (cx, cy) = parse_box_center(&d1, "cardbutton calendar/clock Down ");
        assert!(sh.click(1920.0, 1080.0, cx, cy, Button::Left));
        assert_eq!(sh.edit_cards(idx)[1].owner, "calendar");

        // Size cycles calendar/clock
        let d2 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (sx, sy) = parse_box_center(&d2, "cardbutton calendar/clock Size ");
        let before_size = sh.edit_cards(idx)[1].size;
        assert!(sh.click(1920.0, 1080.0, sx, sy, Button::Left));
        assert_ne!(sh.edit_cards(idx)[1].size, before_size);

        // Drag card 2 (settings/bluetooth) above card 0 (settings/network)
        let d3 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let c2 = box_rect(&d3, "card settings/bluetooth ");
        let c0 = box_rect(&d3, "card settings/network ");
        assert!(sh.pointer_motion(1920.0, 1080.0, c2.0 + 40.0, c2.1 + 20.0));
        assert!(sh.click(1920.0, 1080.0, c2.0 + 40.0, c2.1 + 20.0, Button::Left));
        assert!(sh.pointer_motion(1920.0, 1080.0, c0.0 + 40.0, c0.1 + 4.0));
        assert_eq!(sh.edit.drop_at, Some(0));
        assert!(sh.release());
        assert_eq!(sh.edit_cards(idx)[0].kind, "bluetooth");

        // Half pairs two adjacent half cards side by side, then toggles both back to full
        let d_half = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (hx, hy) = parse_box_center(&d_half, "cardbutton settings/bluetooth Half ");
        assert!(sh.click(1920.0, 1080.0, hx, hy, Button::Left));
        let cards_half = sh.edit_cards(idx);
        assert!(cards_half[0].half && cards_half[1].half);
        let d_half2 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (h2x, h2y) = parse_box_center(&d_half2, "cardbutton settings/bluetooth Half ");
        assert!(sh.click(1920.0, 1080.0, h2x, h2y, Button::Left));
        let cards_full = sh.edit_cards(idx);
        assert!(!cards_full[0].half && !cards_full[1].half);

        // Width + and -
        let d3b = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (wx, wy) = parse_box_center(&d3b, "barbutton Wider ");
        assert!(sh.click(1920.0, 1080.0, wx, wy, Button::Left));
        assert_eq!(sh.panels[idx].panel.width, 400);

        // Undo reverts width change
        let d4 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (ux, uy) = parse_box_center(&d4, "barbutton Undo ");
        assert!(sh.click(1920.0, 1080.0, ux, uy, Button::Left));
        assert_eq!(sh.panels[idx].panel.width, 380);

        // Add opens list, pick Alacritty then add alacritty/app
        let d4b = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (ax, ay) = parse_box_center(&d4b, "barbutton Add ");
        assert!(sh.click(1920.0, 1080.0, ax, ay, Button::Left));
        assert!(sh.edit.adding);
        let d5 = crate::boxes::text(&sh, 1920.0, 1080.0);
        assert!(d5.contains("add "));
        assert!(d5.contains("addrow alacritty "));
        // Scroll Add down so alacritty is visible if list is tall
        let add_r = box_rect(&d5, "add ");
        for _ in 0..20 {
            sh.scroll(add_r.0 + 20.0, add_r.1 + 50.0, 1.0);
        }
        let d5s = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (alx, aly) = parse_box_center(&d5s, "addrow alacritty ");
        assert!(sh.click(1920.0, 1080.0, alx, aly, Button::Left));
        let d6 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (akx, aky) = parse_box_center(&d6, "addrow alacritty/app ");
        assert!(sh.click(1920.0, 1080.0, akx, aky, Button::Left));
        assert_eq!(sh.edit_cards(idx).last().unwrap().owner, "alacritty");
        let d7 = crate::boxes::text(&sh, 1920.0, 1080.0);
        assert!(!d7.contains("addrow alacritty/app "));

        // Reset asks twice: first sets confirm_reset, second restores built-in control defaults
        let (rx, ry) = parse_box_center(&d7, "barbutton Reset ");
        assert!(sh.click(1920.0, 1080.0, rx, ry, Button::Left));
        assert!(sh.edit.confirm_reset);
        assert!(sh.click(1920.0, 1080.0, rx, ry, Button::Left));
        assert!(!sh.edit.confirm_reset);
        assert!(sh
            .edit_cards(idx)
            .iter()
            .any(|c| c.owner == "sound" && c.kind == "volume"));
    }

    #[test]
    fn cancel_restores_and_done_or_escape_saves_rewrites_and_rebuilds() {
        let dir = std::env::temp_dir().join(format!("infynitae-shell-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config");
        let bak_path = dir.join("panels.backup");
        std::fs::write(
            &cfg_path,
            "PANEL_CONTROL=\"SUPER+A top right 380 settings/network settings/bluetooth\"\n",
        )
        .unwrap();

        let mut sh = Shell::new(
            Config::from_text(&std::fs::read_to_string(&cfg_path).unwrap()),
            Vec::new(),
        );
        sh.save_paths = Some((cfg_path.clone(), bak_path.clone()));
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        sh.open_panel(idx, false, Instant::now());
        sh.open_ms = f64::INFINITY;
        let orig_line = crate::panel::line(&sh.panels[idx].panel);

        // Enter edit, change width and hover, Cancel restores original state without touching disk
        assert!(sh.enter_edit(1920.0, 1080.0));
        let dump = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (wx, wy) = parse_box_center(&dump, "barbutton Wider ");
        assert!(sh.click(1920.0, 1080.0, wx, wy, Button::Left));
        assert_eq!(sh.panels[idx].panel.width, 400);
        let (cx, cy) = parse_box_center(&dump, "barbutton Cancel ");
        assert!(sh.click(1920.0, 1080.0, cx, cy, Button::Left));
        assert!(!sh.editing());
        assert_eq!(crate::panel::line(&sh.panels[idx].panel), orig_line);
        assert!(!bak_path.exists());

        // Enter edit again, remove second card, open Add, Escape closes Add first, second Escape saves
        assert!(sh.enter_edit(1920.0, 1080.0));
        let dump2 = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (rmx, rmy) = parse_box_center(&dump2, "cardbutton settings/bluetooth Remove ");
        assert!(sh.click(1920.0, 1080.0, rmx, rmy, Button::Left));
        let dump2b = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (ax, ay) = parse_box_center(&dump2b, "barbutton Add ");
        assert!(sh.click(1920.0, 1080.0, ax, ay, Button::Left));
        assert!(sh.edit.adding);
        assert!(sh.key(Key::Escape));
        assert!(sh.editing() && !sh.edit.adding);
        assert!(sh.key(Key::Escape));
        assert!(!sh.editing());
        assert!(sh.take_fetch_needed());
        assert_eq!(sh.panels[sh.open.unwrap()].cards.len(), 1);
        assert_eq!(sh.panels[sh.open.unwrap()].cards[0].word.kind, "network");

        let cfg_written = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(cfg_written.contains("settings/network"));
        assert!(!cfg_written.contains("settings/bluetooth"));
        let bak_written = std::fs::read_to_string(&bak_path).unwrap();
        assert!(bak_written.contains(&format!("PANEL_CONTROL=\"{orig_line}\"")));

        // A built-in panel with no line yet in config ("apps") gets a PANEL_APPS= line when Done
        let apps_idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "apps")
            .unwrap();
        assert!(!cfg_written.contains("PANEL_APPS="));
        sh.open_panel(apps_idx, false, Instant::now());
        sh.open_ms = f64::INFINITY;
        assert!(sh.enter_edit(1920.0, 1080.0));
        let dump_apps = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (dx, dy) = parse_box_center(&dump_apps, "barbutton Done ");
        assert!(sh.click(1920.0, 1080.0, dx, dy, Button::Left));
        let cfg_written2 = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(cfg_written2.contains("PANEL_APPS="));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reload_if_changed_catches_first_poll_writes_and_post_start_creation() {
        let dir =
            std::env::temp_dir().join(format!("infynitae-shell-mtime-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config");

        // 1. Config exists at start, then changes before the very first reload_if_changed poll.
        std::fs::write(&cfg_path, "ACCENT=\"#112233\"\n").unwrap();
        let mut sh = Shell::new_at(
            Config::from_text(&std::fs::read_to_string(&cfg_path).unwrap()),
            Vec::new(),
            &cfg_path,
        );
        assert!(!sh.reload_if_changed_at(&cfg_path));
        std::fs::write(&cfg_path, "ACCENT=\"#445566\"\n").unwrap();
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&cfg_path)
            .unwrap()
            .set_modified(future)
            .unwrap();
        assert!(sh.reload_if_changed_at(&cfg_path));
        assert_eq!(sh.cfg.accent, [0x44, 0x55, 0x66]);

        // 2. Config does not exist at start, then is created after start.
        let new_cfg_path = dir.join("config-new");
        let mut sh_missing = Shell::new_at(Config::from_text(""), Vec::new(), &new_cfg_path);
        assert!(!sh_missing.reload_if_changed_at(&new_cfg_path));
        std::fs::write(&new_cfg_path, "ACCENT=\"#778899\"\n").unwrap();
        assert!(sh_missing.reload_if_changed_at(&new_cfg_path));
        assert_eq!(sh_missing.cfg.accent, [0x77, 0x88, 0x99]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_app_name_on_half_card_leaves_at_least_half_header_for_title() {
        let half_w = (380.0 - GAP) / 2.0;
        let head_w = half_w - 2.0 * PAD_X;
        let long_name = "Very Long Application Name Forty Charact";
        assert_eq!(long_name.chars().count(), 40);

        let apps = vec![
            App {
                id: "longapp".into(),
                name: long_name.into(),
                exec: "true".into(),
                ..Default::default()
            },
            App {
                id: "secondapp".into(),
                name: "Second".into(),
                exec: "true".into(),
                ..Default::default()
            },
        ];
        let mut sh = Shell::new(
            Config::from_text("PANEL_CONTROL=\"SUPER+A top right 380 longapp/app@half secondapp/app@half\"\n"),
            apps,
        );
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        let (_, rects) = sh.layout(idx, 1280.0, 800.0);
        assert_eq!(rects.len(), 2);
        let r0 = rects[0].1;
        assert!((r0.2 - half_w).abs() < 0.01);

        let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        {
            let cr = cairo::Context::new(&surf).unwrap();
            draw::grey_text_edges(&cr);
            let full_from_w = draw::measure(
                &cr,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: sh.theme.muted,
                    text: long_name,
                },
            )
            .0;
            let (capped_from, title_w) = header_title_max_w(half_w, full_from_w);
            assert!(capped_from <= head_w * 0.45);
            assert!(
                title_w >= head_w * 0.5,
                "title width {title_w} must be at least half of header {head_w}"
            );
            sh.draw(&cr, 1280.0, 800.0);
        }
        surf.flush();
        // Verify no header text spills left of the card's left edge.
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        let out_x = (r0.0 - 4.0).max(0.0) as usize;
        let y_start = (r0.1 + PAD_TOP) as usize;
        let y_end = (r0.1 + PAD_TOP + HEAD_H) as usize;
        for y in y_start..y_end {
            let px = &data[y * stride + out_x * 4..y * stride + out_x * 4 + 4];
            assert_eq!(px, &[0, 0, 0, 0], "header overflowed left of card at y={y}");
        }
    }

    #[test]
    fn missing_reason_wraps_and_fits_at_half_width() {
        let longest = "Memory information is not available";
        let half_w = (380.0 - GAP) / 2.0;
        let body_text_w = half_w - 2.0 * PAD_X;
        let mh = missing_height(half_w, longest);
        assert!(
            mh > 20.0,
            "wrapped missing text height ({mh}) should exceed single-line 20 px"
        );

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 200).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let l = missing_layout(&cr, body_text_w, longest);
        assert_eq!(l.line_count(), 2);
        assert!(
            !l.is_ellipsized(),
            "longest Missing string should fit in 2 lines without ellipsis at half width"
        );

        // Verify via Shell layout + draw with monitor/memory in Missing state at half width.
        let mut sh = shell("SUPER+A top right 380 monitor/memory@half calendar/clock@half");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        // Send empty MemoryData so monitor/memory enters Status::Missing(longest).
        sh.apply(
            "monitor",
            "memory",
            &crate::cards::monitor::MemoryData::default() as &dyn std::any::Any,
        );
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;
        let (_, rects) = sh.layout(idx, 1280.0, 800.0);
        assert_eq!(
            rects[0].1 .3,
            PAD_TOP + HEAD_H + mh + PAD_BOTTOM,
            "card height grows to fit wrapped Missing sentence"
        );
        sh.draw(&cr, 1280.0, 800.0);
    }

    #[test]
    fn keyboard_navigation_scrolls_selected_hit_into_panel_box() {
        let mut sh = shell("SUPER+A top right 380 settings/power settings/session windows/list");
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        sh.windows = (1..=6)
            .map(|i| Window {
                id: i,
                title: format!("Win {i}"),
                app_id: "kitty".into(),
                focused: i == 1,
            })
            .collect();
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 400).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let pbox = sh.draw(&cr, 1280.0, 400.0).unwrap();
        let (_, _, content_h) = sh.layout_local(idx, 1280.0, 400.0);
        assert!(content_h > pbox.3, "precondition: panel is taller than screen");

        // Move Down repeatedly without redrawing in between until past the fold.
        let order = sh.hit_order();
        let past_fold = order
            .iter()
            .position(|(_, _, r)| r.1 + r.3 > pbox.1 + pbox.3)
            .expect("at least one hit starts below the fold");
        for _ in 0..=past_fold {
            assert!(sh.key(Key::Down));
        }
        assert!(
            sh.panel_scroll > 0.0,
            "panel_scroll should advance past the fold"
        );
        sh.draw(&cr, 1280.0, 400.0);
        let (ci, hit) = sh.selected.unwrap();
        let r = sh
            .hit_order()
            .into_iter()
            .find(|(c, h, _)| *c == ci && *h == hit)
            .unwrap()
            .2;
        assert!(
            r.1 >= pbox.1 && r.1 + r.3 <= pbox.1 + pbox.3,
            "selected rect {r:?} must lie inside pbox {pbox:?}"
        );
    }

    #[test]
    fn tall_scrolled_panel_with_hidden_and_half_cards_walks_all_hits_in_view() {
        let apps = vec![
            App {
                id: "app1".into(),
                name: "App One".into(),
                exec: "true".into(),
                ..Default::default()
            },
            App {
                id: "app2".into(),
                name: "App Two".into(),
                exec: "true".into(),
                ..Default::default()
            },
        ];
        // Includes a hidden card (notifications/list with no notes), a half-width pair,
        // and multiple tall cards that overflow a 400 px high screen.
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 notifications/list app1/app@half app2/app@half settings/power settings/session windows/list\"\n",
            ),
            apps,
        );
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        sh.windows = (1..=6)
            .map(|i| Window {
                id: i,
                title: format!("Window {i}"),
                app_id: "firefox".into(),
                focused: i == 1,
            })
            .collect();
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 400).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let pbox = sh.draw(&cr, 1280.0, 400.0).unwrap();
        let (_, layout_rects) = sh.layout(idx, 1280.0, 400.0);
        // notifications/list (index 0) is Hidden, app1/app and app2/app share one row (half-width).
        assert_eq!(layout_rects[0].0, 1);
        assert_eq!(layout_rects[0].1 .1, layout_rects[1].1 .1);

        let total_hits = sh.hit_order().len();
        assert!(total_hits >= 10);
        let mut saw_scrolled = false;
        for step in 0..total_hits {
            assert!(sh.key(Key::Down));
            sh.draw(&cr, 1280.0, 400.0);
            if sh.panel_scroll > 0.0 {
                saw_scrolled = true;
            }
            let (ci, hit) = sh.selected.unwrap();
            let r = sh
                .hit_order()
                .into_iter()
                .find(|(c, h, _)| *c == ci && *h == hit)
                .expect("selected hit present in frame")
                .2;
            assert!(
                r.0 >= pbox.0
                    && r.0 + r.2 <= pbox.0 + pbox.2
                    && r.1 >= pbox.1
                    && r.1 + r.3 <= pbox.1 + pbox.3,
                "step {step}: selected rect {r:?} not inside pbox {pbox:?}"
            );
        }
        assert!(saw_scrolled, "walking all hits must scroll past the fold");

        // Wrapping past the last hit returns to the first hit and scrolls back to top.
        assert!(sh.key(Key::Down));
        sh.draw(&cr, 1280.0, 400.0);
        assert_eq!(sh.panel_scroll, 0.0);
        let first = sh.hit_order()[0];
        assert_eq!(sh.selected, Some((first.0, first.1)));
    }

    #[test]
    fn missing_app_shows_not_installed_card_and_remove_updates_panel_line() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-shell-not-installed-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config");
        let bak_path = dir.join("panels.backup");
        std::fs::write(
            &cfg_path,
            "PANEL_X=\"SUPER+X top right 380 nosuchapp/card settings/bogus settings/session\"\n",
        )
        .unwrap();

        let mut sh = Shell::new(
            Config::from_text(&std::fs::read_to_string(&cfg_path).unwrap()),
            Vec::new(),
        );
        sh.save_paths = Some((cfg_path.clone(), bak_path.clone()));
        let idx = sh.panels.iter().position(|p| p.panel.name == "x").unwrap();
        assert_eq!(sh.panels[idx].cards.len(), 3);
        assert_eq!(sh.panels[idx].cards[0].from, "nosuchapp");
        assert_eq!(sh.panels[idx].cards[1].from, "Settings");
        let cx = sh.cx(Size::S, None, false);
        assert_eq!(sh.panels[idx].cards[0].card.status(&cx), Status::Ready);
        assert_eq!(
            sh.panels[idx].cards[0].card.title(&cx).as_deref(),
            Some("card")
        );
        assert_eq!(NOT_INSTALLED_TEXT, "This app is not installed");

        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);

        let remove_r = {
            let f = sh.frame.borrow();
            assert_eq!(f[0].2.list.len(), 1, "not-installed card has Remove button");
            f[0].2.list[0].1
        };
        assert!(sh.click(
            1280.0,
            800.0,
            remove_r.0 + 4.0,
            remove_r.1 + 4.0,
            Button::Left
        ));
        let idx = sh.panels.iter().position(|p| p.panel.name == "x").unwrap();
        assert_eq!(sh.open, Some(idx));
        assert_eq!(sh.panels[idx].cards.len(), 2);
        assert_eq!(sh.panels[idx].cards[0].word.kind, "bogus");
        assert_eq!(sh.panels[idx].cards[1].word.kind, "session");

        let saved = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(!saved.contains("nosuchapp/card"));
        assert!(saved.contains("settings/bogus"));
        assert!(saved.contains("settings/session"));
        let backup = std::fs::read_to_string(&bak_path).unwrap();
        assert!(backup.contains("nosuchapp/card"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
