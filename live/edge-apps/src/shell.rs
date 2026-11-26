//! The edge's state (docs/EDGE_APPS.md sections 3.4, 5 and 8): the panels and their placed cards, which
//! panel is open and how, the layout of its cards, the pointer and the hits it is on, the cards'
//! arrival and their animations, and the actions they asked for. Plain state: no Wayland, no threads,
//! no process started here (`wayland.rs` does the I/O), so it is tested without a display.

use crate::app::App;
use crate::card::{
    Action, Button, Card, CommandResult, Cx, Event, Hits, Note, Rect, Status, Window,
};
use crate::config::{Config, Panel, Side};
use crate::draw::{self, Text, PAD_X};
use crate::panel::CardRef;
use crate::provider::{self, CardKind, Size};
use crate::theme::Theme;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

mod drawing;
mod edit;
mod keys;
mod pointer;
mod touch;

pub use drawing::draw_card;
use drawing::{missing_height, missing_layout, union_rect, wrapped_text_height};

/// Identifies an output surface inside `Shell` without exposing Wayland handles.
pub type OutputId = u64;

const SCREEN_KEY_TAG: u64 = 1 << 63;

fn screen_key(w: f64, h: f64) -> OutputId {
    SCREEN_KEY_TAG
        | ((w.round().clamp(0.0, 65535.0) as u64) << 16)
        | (h.round().clamp(0.0, 65535.0) as u64)
}

#[derive(Clone, Default)]
struct OutputFrame {
    hits: Vec<(usize, Rect, Hits)>,
    scroll: f64,
    screen: (f64, f64),
}

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
    output: Option<OutputId>,
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
        crate::ui::button(
            cr,
            cx,
            hits,
            0,
            body.0 + 8.0,
            body.1 + text_h + 8.0,
            "Remove",
        );
    }
    fn event(&mut self, ev: &Event, _cx: &Cx, out: &mut Vec<Action>) -> bool {
        if matches!(
            ev,
            Event::Press {
                hit: 0,
                button: Button::Left,
                ..
            } | Event::Delete { hit: 0 }
        ) {
            out.push(Action::RemoveCard);
            return true;
        }
        false
    }
}

pub fn place_one(w: CardRef, apps: &[App]) -> Placed {
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
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Tab,
    BackTab,
    Enter,
    Backspace,
    /// a character typed
    Char(char),
    /// Super with a letter: a panel's key
    Super(char),
}

const TOUCH_DRAG_PX: f64 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TouchMode {
    Pending,
    DragHit,
    Scroll,
    Moved,
}

#[derive(Clone, Copy, Debug)]
struct TouchState {
    id: i32,
    output: OutputId,
    w: f64,
    h: f64,
    start: (f64, f64),
    last: (f64, f64),
    mode: TouchMode,
}

/// One of the items in the compositor's window menu (`xdg_toplevel.show_window_menu` or
/// `BIND_WINDOW_MENU`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMenuAction {
    KeepOnTop,
    BackgroundKeep,
    Minimize,
    Maximize,
    Fullscreen,
    Close,
}

/// The window menu shown at `at` for `element` (`[instance, element]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowMenu {
    pub element: (u64, u64),
    pub at: (f64, f64),
    pub on_top: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    /// its app kept at full speed out of sight (`BACKGROUND_KEEP`); `None`: the window has no
    /// app id to keep it by, and the item is not shown
    pub background_keep: Option<bool>,
}

/// The window menu's width; wider with the full speed item, whose label is the longest.
const WINDOW_MENU_W: f64 = 180.0;
const WINDOW_MENU_W_KEEP: f64 = 300.0;

impl WindowMenu {
    /// The menu's items in order: `(action, label, checked)`.
    pub fn items(&self) -> Vec<(WindowMenuAction, &'static str, bool)> {
        let keep = self.background_keep.map(|on| {
            (
                WindowMenuAction::BackgroundKeep,
                "Keep at full speed in the background",
                on,
            )
        });
        [(WindowMenuAction::KeepOnTop, "Keep on top", self.on_top)]
            .into_iter()
            .chain(keep)
            .chain([
                (WindowMenuAction::Minimize, "Minimize", false),
                (
                    WindowMenuAction::Maximize,
                    if self.maximized {
                        "Restore"
                    } else {
                        "Maximize"
                    },
                    false,
                ),
                (WindowMenuAction::Fullscreen, "Fullscreen", false),
                (WindowMenuAction::Close, "Close", false),
            ])
            .collect()
    }

    /// The shell IPC command line for `action` on `self.element`.
    pub fn command_line(&self, action: WindowMenuAction) -> String {
        let (inst, elem) = self.element;
        match action {
            WindowMenuAction::KeepOnTop => format!(
                "{{\"id\":1,\"cmd\":\"keep_on_top\",\"element\":[{inst},{elem}],\"on\":{}}}\n",
                !self.on_top
            ),
            WindowMenuAction::BackgroundKeep => format!(
                "{{\"id\":1,\"cmd\":\"background_keep\",\"element\":[{inst},{elem}],\"on\":{}}}\n",
                self.background_keep != Some(true)
            ),
            WindowMenuAction::Minimize => {
                format!("{{\"id\":1,\"cmd\":\"minimize\",\"element\":[{inst},{elem}]}}\n")
            }
            WindowMenuAction::Maximize => format!(
                "{{\"id\":1,\"cmd\":\"maximize\",\"element\":[{inst},{elem}],\"maximized\":{}}}\n",
                !self.maximized
            ),
            WindowMenuAction::Fullscreen => format!(
                "{{\"id\":1,\"cmd\":\"fullscreen\",\"element\":[{inst},{elem}],\"fullscreen\":{}}}\n",
                !self.fullscreen
            ),
            WindowMenuAction::Close => {
                format!("{{\"id\":1,\"cmd\":\"close\",\"element\":[{inst},{elem}]}}\n")
            }
        }
    }

    /// Parses `INFYNITAE_EDGE_RENDER_WINDOW_MENU`
    /// (`"x,y[,on_top|off_top|maximized|fullscreen|app|full_speed]"`: `app` shows the full speed
    /// item off, `full_speed` on).
    pub fn from_render_env(raw: &str) -> Self {
        let mut wm = Self {
            element: (1, 1),
            at: (200.0, 200.0),
            on_top: true,
            maximized: false,
            fullscreen: false,
            background_keep: None,
        };
        let parts: Vec<&str> = raw
            .split([',', '|'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() >= 2 {
            if let (Ok(x), Ok(y)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                wm.at = (x, y);
            }
        }
        for p in parts {
            match p {
                "on_top" | "on-top" => wm.on_top = true,
                "off_top" | "off-top" | "normal" => wm.on_top = false,
                "maximized" | "restore" => wm.maximized = true,
                "fullscreen" => wm.fullscreen = true,
                "app" => wm.background_keep = Some(false),
                "full_speed" => wm.background_keep = Some(true),
                _ => {}
            }
        }
        wm
    }
}

/// Parses a `"window_menu"` event line from `$XDG_RUNTIME_DIR/infynitae/shell.sock`.
pub fn parse_window_menu_event(line: &str) -> Option<WindowMenu> {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.contains("\"event\":\"window_menu\"") {
        return None;
    }
    let pair = |key: &str| -> Option<(&str, &str)> {
        let after = compact.split_once(&format!("\"{key}\":["))?.1;
        let inside = after.split_once(']')?.0;
        inside.split_once(',')
    };
    let bool_of = |key: &str| -> bool {
        compact
            .split_once(&format!("\"{key}\":"))
            .is_some_and(|(_, r)| r.starts_with("true"))
    };
    let (inst_s, elem_s) = pair("element")?;
    let (x_s, y_s) = pair("position")?;
    Some(WindowMenu {
        element: (inst_s.parse().ok()?, elem_s.parse().ok()?),
        at: (x_s.parse().ok()?, y_s.parse().ok()?),
        on_top: bool_of("on_top"),
        maximized: bool_of("maximized"),
        fullscreen: bool_of("fullscreen"),
        // true or false; null (no app id) or missing (an older compositor): no item
        background_keep: compact
            .split_once("\"background_keep\":")
            .and_then(|(_, r)| {
                if r.starts_with("true") {
                    Some(true)
                } else if r.starts_with("false") {
                    Some(false)
                } else {
                    None
                }
            }),
    })
}

pub struct Shell {
    pub cfg: Config,
    pub theme: Theme,
    pub apps: Vec<App>,
    /// the apps started lately and the file they are kept in (`None` until `load_recent`)
    pub recent: crate::recent::Recent,
    recent_path: Option<std::path::PathBuf>,
    pub windows: Vec<Window>,
    pub notes: Vec<Note>,
    /// the edge's own notifications: pop-ups and history (`notes` is the history as cards read it)
    pub notify: crate::notify::Notify,
    pub panels: Vec<PanelState>,
    /// the desktop's widgets (`WIDGETS`), shown whether a panel is open or not
    pub widgets: crate::widgets::Widgets,
    pub open: Option<usize>,
    pub open_output: Option<OutputId>,
    ask_output: Option<OutputId>,
    pub opened_by_hover: bool,
    /// A panel opened by its key has had the keyboard: from then on it holds it on demand, so a
    /// window the pointer goes to or clicks can take it (the panel stays open meanwhile).
    pub keyboard_held: bool,
    hover_open: Option<(OutputId, usize, Instant)>,
    hover_close: Option<Instant>,
    /// the pointer on the active output (or from `--render`), and per-output pointers
    pub pointer: Option<(f64, f64)>,
    pointers: HashMap<OutputId, (f64, f64)>,
    hovered: HashMap<OutputId, (usize, u32)>,
    /// the output, card and hit a button went down on (drags go to it)
    pressed: Option<(OutputId, usize, u32)>,
    touch: Option<TouchState>,
    /// each shown card's rectangle and hits in the last frame, plus per-output frames
    pub frame: RefCell<Vec<(usize, Rect, Hits)>>,
    frame_scroll: std::cell::Cell<f64>,
    frame_screen: std::cell::Cell<(f64, f64)>,
    frames: RefCell<HashMap<OutputId, OutputFrame>>,
    last_output: Option<OutputId>,
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
    /// the compositor's window menu (`xdg_toplevel.show_window_menu` or `BIND_WINDOW_MENU`)
    pub window_menu: Option<WindowMenu>,
    /// the config file's time when it was last read (None: not looked at yet)
    pub config_mtime: Option<std::time::SystemTime>,
    /// desktop-entry folders and their modification times when `apps` was last read
    pub app_dirs: Vec<std::path::PathBuf>,
    pub apps_mtime: Vec<Option<std::time::SystemTime>>,
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
        let path = crate::config::config_path();
        let mut sh = Self::new_with_config_path(cfg, apps, path.as_deref());
        sh.set_app_dirs(crate::app::dirs());
        sh
    }

    #[cfg(test)]
    pub fn new_at(cfg: Config, apps: Vec<App>, config_path: &std::path::Path) -> Shell {
        Self::new_with_config_path(cfg, apps, Some(config_path))
    }

    fn new_with_config_path(
        cfg: Config,
        apps: Vec<App>,
        config_path: Option<&std::path::Path>,
    ) -> Shell {
        let theme = Theme::from_config(&cfg);
        let panels = cfg
            .panels
            .iter()
            .map(|p| PanelState {
                cards: place(p, &apps, &cfg.favorites),
                panel: p.clone(),
            })
            .collect();
        let widgets = crate::widgets::place(&cfg, &apps);
        Shell {
            widgets,
            cfg,
            theme,
            apps,
            recent: Default::default(),
            recent_path: None,
            windows: Vec::new(),
            notes: Vec::new(),
            notify: Default::default(),
            ask: Default::default(),
            config_mtime: config_path.and_then(file_mtime),
            app_dirs: Vec::new(),
            apps_mtime: Vec::new(),
            transient: None,
            edit_menu: None,
            keyboard_held: false,
            window_menu: None,
            query: String::new(),
            selected: None,
            keyboard: false,
            panels,
            open: None,
            open_output: None,
            ask_output: None,
            opened_by_hover: false,
            hover_open: None,
            hover_close: None,
            pointer: None,
            pointers: HashMap::new(),
            hovered: HashMap::new(),
            pressed: None,
            touch: None,
            frame: RefCell::new(Vec::new()),
            frame_scroll: std::cell::Cell::new(0.0),
            frame_screen: std::cell::Cell::new((1920.0, 1080.0)),
            frames: RefCell::new(HashMap::new()),
            last_output: None,
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

    /// Reads the recent apps from `path` and writes each start to it from now on.
    pub fn load_recent(&mut self, path: std::path::PathBuf) {
        match crate::recent::Recent::load(&path) {
            Ok(r) => self.recent = r,
            Err(e) => eprintln!("infynitae-edge-apps: read {}: {e}", path.display()),
        }
        self.recent_path = Some(path);
    }

    /// App `id` was started (`Action::Launch`): the newest line of the recent apps' file.
    pub fn note_launch(&mut self, id: &str) {
        self.recent.note(id, chrono::Utc::now().timestamp());
        if let Some(p) = &self.recent_path {
            if let Err(e) = self.recent.save(p) {
                eprintln!("infynitae-edge-apps: write {}: {e}", p.display());
            }
        }
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
            recent: &self.recent,
            windows: &self.windows,
            notes: &self.notes,
            list_above,
            editing: self.editing(),
            room: f64::INFINITY,
        }
    }

    fn panel_width(ps: &PanelState, w: f64) -> f64 {
        if ps.panel.width > 0 {
            ps.panel.width as f64
        } else {
            380.0
        }
        .min((w - 2.0 * MARGIN).max(0.0))
    }

    /// A card's whole height (its box: header, body, padding) at `width`; 0 when it hides, or a
    /// placeholder's height while editing.
    fn card_h(&self, pc: &Placed, width: f64, list_above: bool, room: f64) -> f64 {
        let cx = Cx {
            room,
            ..self.cx(pc.size, pc.app_id.as_deref(), list_above)
        };
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
        draw::with_font(&self.theme.font, || {
            self.layout_panel_local(&self.panels[idx], w, h)
        })
    }

    /// The body room of each card of panel `idx` (by card index): infinite, unless the panel
    /// would be taller than the screen; then the flexible cards, bottom one first, give up what
    /// is over (a card is never cut at the screen's edge just because it is long: 2026-11-14,
    /// the control panel's notifications ran under the screen's bottom).
    pub fn rooms(&self, idx: usize, w: f64, h: f64) -> Vec<f64> {
        draw::with_font(&self.theme.font, || {
            self.panel_rooms(&self.panels[idx], w, h)
        })
    }

    fn panel_rooms(&self, ps: &PanelState, w: f64, h: f64) -> Vec<f64> {
        let mut rooms = vec![f64::INFINITY; ps.cards.len()];
        if self.editing() {
            return rooms;
        }
        let (pbox, _, content_h) = self.layout_panel_with(ps, w, h, &rooms);
        let mut over = content_h - pbox.3;
        if over <= 0.0 {
            return rooms;
        }
        let above = ps.panel.side == Side::Bottom;
        let pw = Self::panel_width(ps, w);
        let body_w = pw - 2.0 * (PAD_X - 8.0);
        for (i, pc) in ps.cards.iter().enumerate().rev() {
            if over <= 0.0 {
                break;
            }
            if !pc.card.flexible() {
                continue;
            }
            let cx = self.cx(pc.size, pc.app_id.as_deref(), above);
            if pc.card.status(&cx) != Status::Ready {
                continue;
            }
            let body = pc.card.height(&cx, body_w);
            let room = (body - over).max(0.0);
            let given = pc.card.height(&Cx { room, ..cx }, body_w);
            rooms[i] = room;
            over -= body - given;
        }
        rooms
    }

    fn layout_panel_local(
        &self,
        ps: &PanelState,
        w: f64,
        h: f64,
    ) -> (Rect, Vec<(usize, Rect)>, f64) {
        let rooms = self.panel_rooms(ps, w, h);
        self.layout_panel_with(ps, w, h, &rooms)
    }

    /// `layout_panel_local` with each card's body room given.
    fn layout_panel_with(
        &self,
        ps: &PanelState,
        w: f64,
        h: f64,
        rooms: &[f64],
    ) -> (Rect, Vec<(usize, Rect)>, f64) {
        let editing = self.editing();
        let pw = Self::panel_width(ps, w);
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
                    (i, self.card_h(a, half, above, rooms[i])),
                    (i + 1, self.card_h(next.unwrap(), half, above, rooms[i + 1])),
                ]);
                i += 2;
            } else {
                rows.push(vec![(i, self.card_h(a, pw, above, rooms[i]))]);
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
        // Taller than the screen: one column that scrolls (owner, 2026-11-22: "expected it to scroll
        // down for other widgets and not them being on the side"); editing the same.
        let avail = (h - 2.0 * MARGIN).max(0.0);
        // Scrolling, the panel runs from the screen's top to its bottom and the cards pass under
        // both edges; the margin stays before the first card and after the last (owner,
        // 2026-11-23: "should be at the edge of the screen not a lil higher").
        let edge_to_edge = !editing && content_h > avail;
        let (lead, content_h, avail) = if edge_to_edge {
            (MARGIN, content_h + 2.0 * MARGIN, h)
        } else {
            (0.0, content_h, avail)
        };
        let tw = pw;
        let ph = content_h.min(avail);
        let p = &ps.panel;
        let (along_w, size) = if p.side.horizontal() {
            (w, tw)
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
            Side::Left if edge_to_edge => (MARGIN, 0.0),
            Side::Right if edge_to_edge => (w - tw - MARGIN, 0.0),
            _ if edge_to_edge => (along, 0.0),
            Side::Top => (along, MARGIN),
            Side::Bottom => (along, h - ph - MARGIN),
            Side::Left => (MARGIN, along),
            Side::Right => (w - tw - MARGIN, along),
        };
        let mut out = Vec::new();
        let mut y = lead + strip;
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
        ((ox, oy, tw, ph), out, content_h)
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

    /// Opens the window menu for `menu.element` at `menu.at`, closing any open panel.
    pub fn open_window_menu(&mut self, menu: WindowMenu) -> bool {
        if self.ask.ask.on {
            crate::ask::answer(&mut self.ask, None);
        }
        if self.open.is_some() {
            self.close();
        }
        self.window_menu = Some(menu);
        self.hovered.clear();
        self.pressed = None;
        self.touch = None;
        self.selected = None;
        self.keyboard = false;
        true
    }

    /// The window menu's card rectangle, kept on the screen.
    pub fn window_menu_rect(&self, w: f64, h: f64) -> Option<Rect> {
        let wm = self.window_menu?;
        let (mw, mh) = (
            if wm.background_keep.is_some() {
                WINDOW_MENU_W_KEEP
            } else {
                WINDOW_MENU_W
            },
            PAD_TOP + wm.items().len() as f64 * crate::ui::LIST_ROW_H + PAD_BOTTOM,
        );
        Some((
            wm.at.0.min(w - mw - 4.0).max(4.0),
            wm.at.1.min(h - mh - 4.0).max(4.0),
            mw,
            mh,
        ))
    }

    /// The config file changed (a Settings card, `settings.sh set`, the owner's editor): read it
    /// again and rebuild the panels, the open one staying open. Never while a panel is edited
    /// (Done saves that one). True when it reloaded.
    pub fn reload_if_changed(&mut self) -> bool {
        let Some(path) = crate::config::config_path() else {
            return false;
        };
        self.reload_if_changed_at(&path)
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
        crate::icons::clear_icon_cache();
        if cfg.panels == self.cfg.panels
            && cfg.favorites == self.cfg.favorites
            && cfg.widgets == self.cfg.widgets
        {
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

    pub fn set_app_dirs(&mut self, dirs: Vec<std::path::PathBuf>) {
        let mtimes = crate::app::dir_mtimes(&dirs);
        self.set_app_dirs_with_mtimes(dirs, mtimes);
    }

    pub fn set_app_dirs_with_mtimes(
        &mut self,
        dirs: Vec<std::path::PathBuf>,
        mtimes: Vec<Option<std::time::SystemTime>>,
    ) {
        self.app_dirs = dirs;
        self.apps_mtime = mtimes;
    }

    /// Replaces `self.apps` and rebuilds panels when the app list changed, keeping any open
    /// non-transient panel open. Returns true when panels were rebuilt.
    pub fn apply_apps(&mut self, apps: Vec<App>) -> bool {
        if apps == self.apps || self.edit.active || self.transient.is_some() {
            return false;
        }
        let open_name = self
            .open
            .and_then(|i| self.panels.get(i))
            .map(|ps| ps.panel.name.clone());
        self.rebuild_panels(self.cfg.clone(), Some(apps));
        self.open = open_name.and_then(|n| self.panels.iter().position(|ps| ps.panel.name == n));
        self.fetch_needed = true;
        true
    }

    /// Stats `self.app_dirs` and reloads `self.apps` synchronously when any folder's modification
    /// time changed. Used outside the Wayland loop (`!self.live`).
    pub fn refresh_apps_if_changed(&mut self) -> bool {
        if self.edit.active || self.app_dirs.is_empty() {
            return false;
        }
        let mtimes = crate::app::dir_mtimes(&self.app_dirs);
        if mtimes == self.apps_mtime {
            return false;
        }
        self.apps_mtime = mtimes;
        let apps = crate::app::load(&self.app_dirs);
        self.apply_apps(apps)
    }

    pub fn rebuild_panels(&mut self, cfg: Config, apps: Option<Vec<App>>) {
        self.transient = None;
        self.closing = None;
        if let Some(a) = apps {
            if a != self.apps {
                crate::icons::clear_icon_cache();
            }
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
        self.widgets = crate::widgets::place(&cfg, &self.apps);
        self.cfg = cfg;
    }

    fn pointer_for(&self, output: OutputId) -> Option<(f64, f64)> {
        self.pointers
            .get(&output)
            .copied()
            .or_else(|| self.pointers.is_empty().then_some(self.pointer).flatten())
    }

    fn active_output_or_last(&self) -> OutputId {
        self.open_output
            .or(self.last_output)
            .unwrap_or_else(|| screen_key(self.last_screen.0, self.last_screen.1))
    }

    fn screen_for(&self, output: OutputId) -> (f64, f64) {
        self.frames
            .borrow()
            .get(&output)
            .map(|f| f.screen)
            .unwrap_or(self.last_screen)
    }

    pub fn open_panel(&mut self, idx: usize, by_hover: bool, now: Instant) -> bool {
        self.open_panel_on(None, idx, by_hover, now)
    }

    pub fn open_panel_on(
        &mut self,
        output: Option<OutputId>,
        idx: usize,
        by_hover: bool,
        now: Instant,
    ) -> bool {
        // what changed in the file shows from this opening on (a reload keeps the order of panels)
        self.reload_if_changed();
        if idx >= self.panels.len() || (by_hover && self.ask.ask.on) {
            return false;
        }
        // a panel asked for by its key: the question is put aside (cancelled), as in the old edge
        if self.ask.ask.on {
            crate::ask::answer(&mut self.ask, None);
            self.ask_output = None;
        }
        if self.open == Some(idx) {
            return if by_hover { false } else { self.close_at(now) };
        }
        if !self.live {
            self.refresh_apps_if_changed();
        }
        self.window_menu = None;
        if self.editing() {
            self.cancel_edit();
        }
        self.open = Some(idx);
        self.open_output = output;
        if let Some(o) = output {
            self.last_output = Some(o);
        }
        self.closing = None;
        self.panel_scroll = 0.0;
        self.reset_keys(idx);
        self.opened_by_hover = by_hover;
        self.keyboard_held = false;
        self.hover_open = None;
        self.hover_close = None;
        self.opened_at = Some(now);
        self.open_ms = 0.0;
        self.hovered.clear();
        self.pressed = None;
        self.touch = None;
        self.expire_cards(now);
        true
    }

    pub fn toggle_named_on(&mut self, output: Option<OutputId>, name: &str) -> bool {
        match self.panels.iter().position(|p| p.panel.name == name) {
            Some(i) => self.open_panel_on(output, i, false, Instant::now()),
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
        let output = self.open_output.take();
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
        let had_wm = self.window_menu.take().is_some();
        self.edit_menu = None;
        self.query.clear();
        self.selected = None;
        self.keyboard = false;
        self.opened_by_hover = false;
        self.keyboard_held = false;
        self.hover_open = None;
        self.hover_close = None;
        self.hovered.clear();
        self.pressed = None;
        self.touch = None;
        if let Some(o) = output {
            self.pointers.remove(&o);
        } else {
            self.pointers.clear();
        }
        self.pointer = None;
        self.panel_scroll = 0.0;
        self.opened_at = None;
        self.open_ms = f64::INFINITY;
        self.frame.borrow_mut().clear();
        self.frames.borrow_mut().clear();
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
                output,
            });
        }
        was || had_wm
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
        let output = self.open_output;
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
        self.open_output = output;
        self.opened_at = Some(Instant::now());
        self.open_ms = 0.0;
        self.fetch_needed = true;
    }

    /// The card and hit under `p` in the last frame drawn on `output`.
    fn hit_at_on(&self, output: OutputId, p: (f64, f64)) -> Option<(usize, u32)> {
        let frames = self.frames.borrow();
        if let Some(f) = frames.get(&output) {
            return f
                .hits
                .iter()
                .rev()
                .find_map(|(i, _, hits)| hits.top_at(p).map(|h| (*i, h)));
        }
        self.frame
            .borrow()
            .iter()
            .rev()
            .find_map(|(i, _, hits)| hits.top_at(p).map(|h| (*i, h)))
    }

    fn card_at_on(&self, output: OutputId, p: (f64, f64)) -> Option<usize> {
        let frames = self.frames.borrow();
        if let Some(f) = frames.get(&output) {
            return f
                .hits
                .iter()
                .find(|(_, r, _)| p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3)
                .map(|(i, _, _)| *i);
        }
        self.frame
            .borrow()
            .iter()
            .find(|(_, r, _)| p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3)
            .map(|(i, _, _)| *i)
    }

    fn hit_is_draggable_on(&self, output: OutputId, ci: usize, hit: u32) -> bool {
        let frames = self.frames.borrow();
        if let Some(f) = frames.get(&output) {
            return f
                .hits
                .iter()
                .rev()
                .find(|(c, _, _)| *c == ci)
                .is_some_and(|(_, _, hits)| hits.is_draggable(hit));
        }
        self.frame
            .borrow()
            .iter()
            .rev()
            .find(|(c, _, _)| *c == ci)
            .is_some_and(|(_, _, hits)| hits.is_draggable(hit))
    }

    fn open_screen(&self) -> (f64, f64) {
        let frames = self.frames.borrow();
        if let Some(f) = self.open_output.and_then(|id| frames.get(&id)) {
            f.screen
        } else if let Some(id) = self.open_output {
            self.screen_for(id)
        } else {
            self.frame_screen.get()
        }
    }

    /// Hands `ev` to card `ci` of panel `idx`; its actions are queued with what to fetch after.
    fn card_event(&mut self, idx: usize, ci: usize, ev: &Event) -> bool {
        let editing = self.editing();
        let above = self.panels[idx].panel.side == Side::Bottom;
        let (w, h) = self
            .open_output
            .or(self.last_output)
            .and_then(|id| self.frames.borrow().get(&id).map(|f| f.screen))
            .unwrap_or_else(|| {
                if self.last_output.is_some() {
                    self.last_screen
                } else {
                    self.frame_screen.get()
                }
            });
        let room = self
            .rooms(idx, w, h)
            .get(ci)
            .copied()
            .unwrap_or(f64::INFINITY);
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
                recent: &self.recent,
                windows: &self.windows,
                notes: &self.notes,
                list_above: above,
                editing,
                room,
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

    /// Something shows: a panel, the window menu, or the question.
    pub fn active(&self) -> bool {
        self.open.is_some() || self.ask.ask.on || self.window_menu.is_some()
    }

    pub fn is_closing(&self) -> bool {
        self.closing.is_some()
    }

    /// Something must be drawn on screen (open, asking, or fading out after close).
    pub fn visible(&self) -> bool {
        self.active() || self.closing.is_some()
    }

    pub fn set_ask_output(&mut self, output: Option<OutputId>) {
        self.ask_output = output;
    }

    pub fn visible_output(&self) -> Option<OutputId> {
        if self.ask.ask.on {
            self.ask_output
        } else if self.open.is_some() {
            self.open_output
        } else {
            self.closing.as_ref().and_then(|c| c.output)
        }
    }

    pub fn visible_on(&self, output: OutputId) -> bool {
        self.visible() && self.visible_output().is_none_or(|id| id == output)
    }

    pub fn frame_hits(&self, output: Option<OutputId>) -> Vec<(usize, Rect, Hits)> {
        if let Some(id) = output {
            if let Some(f) = self.frames.borrow().get(&id) {
                return f.hits.clone();
            }
        }
        self.frame.borrow().clone()
    }

    /// What is drawn on a `w x h` screen: the question's card while it asks, the window menu,
    /// else the open panel's box (plus the edit bar and Add list while editing), or a closing
    /// panel's box.
    pub fn shown_box(&self, w: f64, h: f64) -> Option<Rect> {
        if self.ask.ask.on {
            return Some(self.ask.ask.geometry(w, h));
        }
        if let Some(r) = self.window_menu_rect(w, h) {
            return Some(r);
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
        Some(draw::with_font(&self.theme.font, || {
            self.layout_panel_local(ps, w, h).0
        }))
    }

    /// A request in the ask file (SIGRTMIN+3), or a cancel: loaded, queued after one already
    /// asking. True when what shows changed. A question closes the open panel.
    pub fn ask_load(&mut self) -> bool {
        let cancel = crate::paths::runtime_path(crate::ask::ASK_CANCEL_FILE);
        let mut changed = crate::ask::handle_cancel_from_path(&mut self.ask, &cancel);
        let req = crate::paths::runtime_path(crate::ask::ASK_FILE);
        changed |= crate::ask::load_from_path(&mut self.ask, &req);
        changed |= crate::ask::check_asker_gone(&mut self.ask);
        if self.ask.ask.on {
            if self.open.is_some() {
                let prev_out = self.open_output;
                self.close();
                if self.ask_output.is_none() {
                    self.ask_output = prev_out;
                }
                changed = true;
            }
            self.closing = None;
        } else {
            self.ask_output = None;
        }
        changed
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
        let out = self.active_output_or_last();
        let (w, h) = self.screen_for(out);
        running |= self.drag_autoscroll(out, w, h, dt);
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

    pub fn wants_clock(&self) -> bool {
        self.open.is_some_and(|idx| {
            self.panels[idx]
                .cards
                .iter()
                .any(|pc| pc.card.wants_clock())
        })
    }

    /// Every placed card: the panels' and the desktop widgets'.
    fn all_cards_mut(&mut self) -> impl Iterator<Item = &mut Placed> {
        self.panels
            .iter_mut()
            .flat_map(|ps| ps.cards.iter_mut())
            .chain(self.widgets.list.iter_mut().map(|w| &mut w.placed))
    }

    /// New data for `(provider, key)`: every card that lists it (in a panel or on the desktop).
    /// True when one changed.
    pub fn apply(&mut self, provider: &str, key: &str, data: &dyn std::any::Any) -> bool {
        let mut changed = false;
        for pc in self.all_cards_mut() {
            if pc.provider.id() == provider && pc.kind.sources.contains(&key) {
                changed |= pc.card.apply(key, data);
            }
        }
        changed
    }

    /// Delivers a finished card command's result to matching cards (`owner/kind`).
    pub fn command_done(&mut self, card: &str, res: &CommandResult, now: Instant) -> bool {
        let Some((owner, kind)) = card.split_once('/') else {
            return false;
        };
        let mut changed = false;
        for pc in self.all_cards_mut() {
            if pc.word.owner == owner && pc.word.kind == kind {
                changed |= pc.card.command_done(res, now);
            }
        }
        changed
    }

    /// Earliest expiry deadline across cards with transient feedback (e.g. failed command lines).
    pub fn card_deadline(&self) -> Option<Instant> {
        self.panels
            .iter()
            .flat_map(|ps| ps.cards.iter())
            .chain(self.widgets.list.iter().map(|w| &w.placed))
            .filter_map(|pc| pc.card.next_deadline())
            .min()
    }

    /// Expires transient card feedback whose deadline passed by `now`.
    pub fn expire_cards(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for pc in self.all_cards_mut() {
            changed |= pc.card.expire(now);
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
            let out = self.active_output_or_last();
            if let Some((x, y)) = self.pointer_for(out) {
                let (w, h) = self.screen_for(out);
                let (pbox, _, content_h) = self.layout_local(idx, w, h);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::parse_power;

    pub(super) fn shell(line: &str) -> Shell {
        Shell::new(
            Config::from_text(&format!("PANEL_CONTROL=\"{line}\"\n")),
            Vec::new(),
        )
    }

    pub(super) fn windows_shell() -> (Shell, usize) {
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
    pub(super) fn frame(sh: &Shell) {
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);
    }

    #[test]
    fn a_panel_taller_than_the_screen_shows_fewer_notifications_instead_of_cutting_the_card() {
        // 2026-11-14: the control panel ran under the screen's bottom edge and cut the
        // notifications card in half (the panel scrolled, but nothing showed it)
        let mut sh = shell("SUPER+A bottom right 380 notifications/list");
        sh.notes = (0..8)
            .map(|i| crate::card::Note {
                id: i,
                app: format!("App{i}"),
                summary: format!("Title {i}"),
                body: "body".into(),
                actions: vec![],
                urgency: 1,
                time: "12:00".into(),
            })
            .collect();
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        // room for everything: nothing is limited
        assert!(sh
            .rooms(idx, 1920.0, 1080.0)
            .iter()
            .all(|r| r.is_infinite()));
        let (_, _, tall) = sh.layout_local(idx, 1920.0, 1080.0);
        // a short screen: the list gives up whole notifications until the panel fits
        let h = 300.0;
        let rooms = sh.rooms(idx, 1920.0, h);
        assert!(rooms[0].is_finite());
        let (pbox, rects, content_h) = sh.layout_local(idx, 1920.0, h);
        assert!(content_h < tall);
        assert!(
            content_h <= h - 2.0 * MARGIN,
            "{content_h} > {}",
            h - 2.0 * MARGIN
        );
        assert!(rects.iter().all(|(_, r)| r.1 + r.3 <= pbox.3 + 0.5));
        // the title counts the ones left out
        let pc = &sh.panels[idx].cards[0];
        let cx = Cx {
            room: rooms[0],
            ..sh.cx(pc.size, None, true)
        };
        assert!(pc.card.title(&cx).is_some_and(|t| t.contains("more")));
    }

    #[test]
    fn a_panel_taller_than_the_screen_scrolls_in_one_column() {
        let mut sh = shell("SUPER+A bottom right 380 calendar/clock settings/network settings/bluetooth settings/power settings/session");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        let (one, _, h1) = sh.layout_local(idx, 1920.0, 1080.0);
        // a screen too short for the cards: the same one column, the panel from the screen's top
        // to its bottom, the rest reached by scrolling (owner, 2026-11-22: no second column;
        // 2026-11-23: to the screen's edge)
        let h = h1 * 0.7 + 2.0 * MARGIN;
        let (pbox, rects, content_h) = sh.layout_local(idx, 1920.0, h);
        assert!(
            rects.iter().all(|(_, r)| r.0 == 0.0),
            "one column: {rects:?}"
        );
        assert_eq!(pbox.2, one.2, "the panel keeps its width");
        assert_eq!((pbox.1, pbox.3), (0.0, h), "edge to edge");
        assert_eq!(
            rects[0].1 .1, MARGIN,
            "the first card a margin below the top"
        );
        assert!(content_h > pbox.3, "more than the panel shows");
        assert!((pbox.0 + pbox.2 - (1920.0 - MARGIN)).abs() < 0.5);
        // scrolled to the end, the last card ends a margin above the screen's bottom
        sh.panel_scroll = content_h - pbox.3;
        let (_, on_screen) = sh.layout(idx, 1920.0, h);
        let last = on_screen.last().unwrap().1;
        assert!(
            (last.1 + last.3 - (h - MARGIN)).abs() < 0.5,
            "{last:?} {pbox:?}"
        );
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

    pub(super) fn parse_box_center(dump: &str, prefix: &str) -> (f64, f64) {
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

    pub(super) fn box_rect(dump: &str, prefix: &str) -> Rect {
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

    /// The full speed item shows for a window with an app id, checked as the compositor says,
    /// and asks for the opposite; without an app id (or from a compositor that does not say) it
    /// is not there.
    #[test]
    fn window_menu_full_speed_item() {
        let ev = |keep: &str| {
            parse_window_menu_event(&format!(
                r#"{{"event":"window_menu","element":[42,7],"position":[10,10],"on_top":false,"maximized":false,"fullscreen":false{keep},"generation":3}}"#
            ))
            .unwrap()
        };
        let item = (
            WindowMenuAction::BackgroundKeep,
            "Keep at full speed in the background",
        );
        for (keep, want) in [
            (r#","background_keep":null"#, None),
            ("", None),
            (r#","background_keep":false"#, Some(false)),
            (r#","background_keep":true"#, Some(true)),
        ] {
            let wm = ev(keep);
            assert_eq!(wm.background_keep, want, "{keep}");
            let found = wm.items().iter().find(|i| i.0 == item.0).copied();
            assert_eq!(found, want.map(|on| (item.0, item.1, on)), "{keep}");
            assert_eq!(wm.items().len(), if want.is_some() { 6 } else { 5 });
        }
        assert_eq!(
            ev(r#","background_keep":false"#).command_line(item.0),
            "{\"id\":1,\"cmd\":\"background_keep\",\"element\":[42,7],\"on\":true}\n"
        );
        assert_eq!(
            ev(r#","background_keep":true"#).command_line(item.0),
            "{\"id\":1,\"cmd\":\"background_keep\",\"element\":[42,7],\"on\":false}\n"
        );

        // second in the menu, a click on it sends the command
        let mut sh = Shell::new(Config::from_text(""), Vec::new());
        assert!(sh.open_window_menu(ev(r#","background_keep":false"#)));
        frame(&sh);
        let dump = crate::boxes::text(&sh, 1920.0, 1080.0);
        let (x, y) = parse_box_center(&dump, "hit window/menu 1 ");
        assert!(sh.click(1920.0, 1080.0, x, y, Button::Left));
        assert_eq!(
            std::mem::take(&mut sh.actions)[0].0,
            Action::ShellCommand(
                "{\"id\":1,\"cmd\":\"background_keep\",\"element\":[42,7],\"on\":true}\n".into()
            )
        );
    }

    #[test]
    fn window_menu_items_labels_ipc_parsing_and_actions() {
        let ev = r#"{"event":"window_menu","element":[42,7],"position":[320.5,180.0],"on_top":true,"maximized":false,"fullscreen":false,"generation":3}"#;
        let wm = parse_window_menu_event(ev).expect("parsed window_menu event");
        assert_eq!(wm.element, (42, 7));
        assert_eq!(wm.at, (320.5, 180.0));
        assert!(wm.on_top && !wm.maximized && !wm.fullscreen);
        assert_eq!(
            wm.items(),
            [
                (WindowMenuAction::KeepOnTop, "Keep on top", true),
                (WindowMenuAction::Minimize, "Minimize", false),
                (WindowMenuAction::Maximize, "Maximize", false),
                (WindowMenuAction::Fullscreen, "Fullscreen", false),
                (WindowMenuAction::Close, "Close", false),
            ]
        );

        let wm_max = WindowMenu {
            maximized: true,
            on_top: false,
            ..wm
        };
        assert_eq!(
            wm_max.items(),
            [
                (WindowMenuAction::KeepOnTop, "Keep on top", false),
                (WindowMenuAction::Minimize, "Minimize", false),
                (WindowMenuAction::Maximize, "Restore", false),
                (WindowMenuAction::Fullscreen, "Fullscreen", false),
                (WindowMenuAction::Close, "Close", false),
            ]
        );

        let mut sh = Shell::new(Config::from_text(""), Vec::new());
        assert!(sh.open_window_menu(wm));
        assert!(sh.active());
        frame(&sh);
        let dump = crate::boxes::text(&sh, 1920.0, 1080.0);
        assert!(dump.contains("windowmenu 320 180 180 186\n"));
        assert!(dump.contains("hit window/menu 0 "));
        assert!(dump.contains("hit window/menu 4 "));

        // Clicking row 0 ("Keep on top") sends keep_on_top with on:false (toggling off) and closes
        let (x0, y0) = parse_box_center(&dump, "hit window/menu 0 ");
        assert!(sh.click(1920.0, 1080.0, x0, y0, Button::Left));
        assert!(!sh.active());
        assert_eq!(
            std::mem::take(&mut sh.actions)[0].0,
            Action::ShellCommand(
                "{\"id\":1,\"cmd\":\"keep_on_top\",\"element\":[42,7],\"on\":false}\n".into()
            )
        );

        // Keyboard Down + Enter activates item 2, Escape closes
        sh.open_window_menu(wm_max);
        frame(&sh);
        assert!(sh.key(Key::Down));
        assert!(sh.key(Key::Down));
        assert!(sh.key(Key::Down));
        assert!(sh.key(Key::Enter));
        assert!(!sh.active());
        assert_eq!(
            std::mem::take(&mut sh.actions)[0].0,
            Action::ShellCommand(
                "{\"id\":1,\"cmd\":\"maximize\",\"element\":[42,7],\"maximized\":false}\n".into()
            )
        );

        sh.open_window_menu(wm);
        frame(&sh);
        assert!(sh.key(Key::Escape));
        assert!(!sh.active());
    }

    #[test]
    fn small_screen_clamps_panel_width_height_and_scrolls() {
        let mut sh = shell(
            "SUPER+A top right 380 settings/power settings/session settings/power settings/session",
        );
        sh.apply(
            "settings",
            "power",
            &parse_power("* balanced:\n  power-saver:\n  performance:\n") as &dyn std::any::Any,
        );
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        // Screen narrower and shorter than the 380 px panel: it scrolls from the screen's top to
        // its bottom.
        let (pbox, _, content_h) = sh.layout_local(idx, 320.0, 240.0);
        assert_eq!(pbox, (24.0, 0.0, 272.0, 240.0));
        assert!(content_h > pbox.3);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 320, 240).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        assert_eq!(sh.draw(&cr, 320.0, 240.0), Some(pbox));

        // Scrolling inside the clamped panel moves content into view and clicks hit accurately.
        sh.pointer_motion(320.0, 240.0, pbox.0 + 20.0, pbox.1 + 20.0);
        assert!(sh.scroll(pbox.0 + 20.0, pbox.1 + 20.0, 1.0));
        assert!(sh.panel_scroll > 0.0);
        sh.draw(&cr, 320.0, 240.0);
        let hit_after = sh.frame.borrow()[0].2.list[0].1;
        assert!(sh.click(
            320.0,
            240.0,
            hit_after.0 + 4.0,
            hit_after.1 + 4.0,
            Button::Left
        ));
        assert!(!sh.actions.is_empty());

        // Even an extreme screen smaller than 2*MARGIN stays non-negative without panicking.
        let (tiny_box, _, _) = sh.layout_local(idx, 32.0, 32.0);
        assert!(tiny_box.2 >= 0.0 && tiny_box.3 >= 0.0);
    }

    #[test]
    fn new_app_in_desktop_folder_appears_when_panel_opens() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-shell-apps-refresh-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let apps_dir = dir.join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();
        let cfg =
            Config::from_text("PANEL_APPS=\"SUPER+A top right 380 apps/apps discord/messages\"\n");
        let mut sh = Shell::new(cfg, crate::app::load(std::slice::from_ref(&apps_dir)));
        sh.set_app_dirs(vec![apps_dir.clone()]);
        assert!(sh.apps.is_empty());
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "apps")
            .unwrap();
        assert_eq!(sh.panels[idx].cards[1].provider.id(), "");
        assert_eq!(sh.panels[idx].cards[1].app_id, None);

        // Add a new .desktop file while all panels are closed.
        std::fs::write(
            apps_dir.join("discord.desktop"),
            "[Desktop Entry]\nName=Discord\nExec=discord\n",
        )
        .unwrap();
        let t1 = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::open(&apps_dir)
            .unwrap()
            .set_modified(t1)
            .unwrap();
        assert!(
            sh.apps.is_empty(),
            "apps stay unchanged while panels are closed"
        );

        assert!(sh.open_panel(idx, false, Instant::now()));
        assert_eq!(
            sh.apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            vec!["discord"]
        );
        assert_eq!(sh.panels[idx].cards[1].provider.id(), "chat");
        assert_eq!(sh.panels[idx].cards[1].app_id.as_deref(), Some("discord"));

        // Removing the .desktop file updates sh.apps and card ownership on the next open too.
        assert!(sh.close_at(Instant::now()));
        std::fs::remove_file(apps_dir.join("discord.desktop")).unwrap();
        let t2 = t1 + std::time::Duration::from_secs(5);
        std::fs::File::open(&apps_dir)
            .unwrap()
            .set_modified(t2)
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        assert!(sh.apps.is_empty());
        assert_eq!(sh.panels[idx].cards[1].provider.id(), "");
        assert_eq!(sh.panels[idx].cards[1].app_id, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn command_done_routes_error_to_card_and_reopen_expires_after_ttl() {
        let mut sh = shell("SUPER+A top right 380 settings/network settings/bluetooth");
        let t0 = Instant::now();
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        sh.apply(
            "settings",
            "network",
            &crate::cards::settings::parse_network(
                "enabled\n---\nwifi:disconnected\n---\n :80:WPA2:CafeSec\n---\n",
            ),
        );
        assert!(sh.open_panel_on(None, idx, false, t0));
        assert!(sh.command_done(
            "settings/network",
            &CommandResult {
                command: "nmcli connection up id 'CafeSec' >/dev/null".into(),
                status: Some(4),
                stderr_line:
                    "Error: Connection activation failed: Secrets were required, but not provided."
                        .into(),
            },
            t0,
        ));
        assert_eq!(sh.card_deadline(), Some(t0 + crate::ui::ERROR_TTL));
        assert!(sh.close_at(t0 + std::time::Duration::from_secs(2)));
        assert!(sh.open_panel_on(None, idx, false, t0 + crate::ui::ERROR_TTL));
        assert_eq!(
            sh.card_deadline(),
            None,
            "expired card errors must clear when reopening the panel"
        );
    }

    #[test]
    fn a_start_is_written_first_in_the_recent_apps_and_the_history_stays() {
        let dir =
            std::env::temp_dir().join(format!("edge-apps-shell-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("recent-apps");
        std::fs::write(
            &path,
            "1700000200\tfirefox.desktop\n1700000100\tsteam.desktop\n",
        )
        .unwrap();
        let mut sh = shell("SUPER+W top center 440 apps/search");
        sh.load_recent(path.clone());
        sh.note_launch("foot");
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        let (when, name) = lines[0].split_once('\t').unwrap();
        assert_eq!(name, "foot.desktop");
        assert!(when.parse::<i64>().unwrap() > 1700000200);
        assert_eq!(
            lines[1..],
            ["1700000200\tfirefox.desktop", "1700000100\tsteam.desktop"]
        );
        // the cards see it at once
        assert!(sh
            .cx(Size::M, None, false)
            .recent
            .scores()
            .contains_key("foot"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
