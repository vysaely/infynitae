//! infynitae-edge-rs: Infynitae's panels and desktop widgets, in Rust (docs/EDGE_RUST.md).
//!
//!   infynitae-edge-rs                           run the Wayland edge
//!   infynitae-edge-rs panel NAME                open or close panel NAME
//!   infynitae-edge-rs control | apps | clipboard | toggle | center | open
//!   infynitae-edge-rs stop                      stop the running edge
//!   infynitae-edge-rs --print-panels            each panel as its PANEL_ line (as the C edge prints)
//!   infynitae-edge-rs --print-theme             the resolved look as LOOK_* shell lines (stage 7)
//!   infynitae-edge-rs widgets                   edit the desktop's widgets (WIDGETS line, stage 5)
//!   infynitae-edge-rs --render NAME WxH OUT.png a panel open on a WxH screen, drawn to a PNG
//!
//! INFYNITAE_EDGE_RENDER_TIME="2026-11-09 17:23" fixes the clock for --render.

mod config;
mod draw;
mod modules;
mod theme;
mod wayland;

use config::{Config, Panel, Side};
use std::{path::PathBuf, process::ExitCode, time::SystemTime};

/// The width a panel takes on a screen w px wide (the C edge's panel_width).
pub fn panel_width(p: &Panel, w: f64) -> f64 {
    use config::Module::*;
    if p.desktop {
        return w;
    }
    let pw = if p.width > 0 {
        p.width as f64
    } else if p.has(Apps) || p.has(Windows) || p.has(Clipboard) || p.has(Favorites) {
        560.0
    } else {
        380.0
    };
    pw.min(w - 16.0)
}

/// Where a panel of pw x ph goes: 8 px from its edge, at its place along it (panel_origin).
pub fn panel_origin(p: &Panel, w: f64, h: f64, pw: f64, ph: f64) -> (f64, f64) {
    const M: f64 = 8.0;
    if p.desktop {
        return (0.0, 0.0);
    }
    let (along_w, size) = if p.side.horizontal() {
        (w, pw)
    } else {
        (h, ph)
    };
    let along = match p.align {
        a if a < 0 => M,
        a if a > 0 => along_w - size - M,
        _ => ((along_w - size) / 2.0).round(),
    }
    .max(M);
    let (x, y) = match p.side {
        Side::Top => (along, M),
        Side::Bottom => (along, h - ph - M),
        Side::Left => (M, along),
        Side::Right => (w - pw - M, along),
    };
    (x, y.max(M))
}

/// Each module's rectangle in the panel: where it was placed by hand, else stacked.
pub fn layout(p: &Panel, pw: f64) -> (Vec<(f64, f64, f64, f64)>, f64) {
    let world = modules::World::now();
    let th = theme::Theme::from_config(&Config::from_text(""));
    layout_in(p, pw, &world, &th)
}

/// Each module's rectangle in the panel given `world` and `th`.
pub fn layout_in(
    p: &Panel,
    pw: f64,
    world: &modules::World,
    th: &theme::Theme,
) -> (Vec<(f64, f64, f64, f64)>, f64) {
    if p.placed {
        let rects: Vec<_> = p
            .items
            .iter()
            .map(|it| {
                it.rect
                    .map(|(x, y, w, h)| (x as f64, y as f64, w as f64, h as f64))
                    .unwrap_or((0.0, 0.0, 0.0, 0.0))
            })
            .collect();
        let mut rects = rects;
        let mut bottom = rects.iter().map(|r| r.1 + r.3).fold(0.0, f64::max);
        // a module without a place of its own (it showed nothing when the panel was arranged):
        // under the placed ones, the panel's width, once it has something to show
        for (it, r) in p.items.iter().zip(rects.iter_mut()) {
            if it.rect.is_none() {
                let h = modules::height_in(it, world, th, pw);
                if h > 0.0 {
                    let y = if bottom > 0.0 {
                        bottom + draw::GAP
                    } else {
                        0.0
                    };
                    *r = (0.0, y, pw, h);
                    bottom = y + h;
                }
            }
        }
        return (
            rects,
            if p.height > 0 {
                p.height as f64
            } else {
                bottom
            },
        );
    }
    let mut y = 0.0;
    let mut rects = Vec::with_capacity(p.items.len());
    let mut i = 0;
    while i < p.items.len() {
        if p.items[i].module.is_switch() {
            let mut j = i + 1;
            while j < p.items.len() && p.items[j].module.is_switch() {
                j += 1;
            }
            let (run_rects, card_h) =
                modules::switches::layout_run(&p.items[i..j], world, &th.switch_look, pw, y);
            rects.extend(run_rects);
            if card_h > 0.0 {
                y += card_h + draw::GAP;
            }
            i = j;
        } else {
            let h = modules::height_in(&p.items[i], world, th, pw);
            rects.push((0.0, y, pw, h));
            if h > 0.0 {
                y += h + draw::GAP;
            }
            i += 1;
        }
    }
    (rects, (y - draw::GAP).max(0.0))
}

/// The maximum height a panel may take on a screen `h` px tall (screen height minus 16).
/// A panel's height on a screen h px tall: its content, at most the screen less 16 px; the
/// desktop's widgets: the whole screen (their canvas).
pub fn ph_of(p: &Panel, content_h: f64, h: f64) -> f64 {
    if p.desktop {
        h
    } else {
        content_h.min(max_panel_height(h))
    }
}

pub fn max_panel_height(h: f64) -> f64 {
    (h - 16.0).max(0.0)
}

/// The panel's rectangle `(x, y, w, h)` on an output of size `w x h`.
pub fn panel_rect(p: &Panel, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let pw = panel_width(p, w);
    let (_, content_h) = layout(p, pw);
    let ph = ph_of(p, content_h, h);
    let (ox, oy) = panel_origin(p, w, h, pw, ph);
    (ox, oy, pw, ph)
}

/// The panel's rectangle `(x, y, w, h)` given `world` and `th`.
pub fn panel_rect_in(
    p: &Panel,
    w: f64,
    h: f64,
    world: &modules::World,
    th: &theme::Theme,
) -> (f64, f64, f64, f64) {
    let pw = panel_width(p, w);
    let (_, content_h) = layout_in(p, pw, world, th);
    let ph = ph_of(p, content_h, h);
    let (ox, oy) = panel_origin(p, w, h, pw, ph);
    (ox, oy, pw, ph)
}

/// The 2 px hover strip at `p`'s place on its screen edge (its edge and alignment as in `panel_origin`),
/// measured with no data and the default look (tests).
#[cfg(test)]
pub fn hover_strip(p: &Panel, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let world = modules::World::now();
    let th = theme::Theme::from_config(&Config::from_text(""));
    hover_strip_in(p, w, h, &world, &th)
}

/// The hover strip given `world` and `th`, so it sits where the panel itself is drawn (review 22).
pub fn hover_strip_in(
    p: &Panel,
    w: f64,
    h: f64,
    world: &modules::World,
    th: &theme::Theme,
) -> (f64, f64, f64, f64) {
    let (ox, oy, pw, ph) = panel_rect_in(p, w, h, world, th);
    match p.side {
        Side::Top => (ox, 0.0, pw, 2.0),
        Side::Bottom => (ox, (h - 2.0).max(0.0), pw, 2.0),
        Side::Left => (0.0, oy, 2.0, ph),
        Side::Right => ((w - 2.0).max(0.0), oy, 2.0, ph),
    }
}

/// The panel's rectangle together with the 8 px gap to its screen edge (for hover tracking).
pub fn panel_hover_box(p: &Panel, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let (ox, oy, pw, ph) = panel_rect(p, w, h);
    match p.side {
        Side::Top => (ox, 0.0, pw, oy + ph),
        Side::Bottom => (ox, oy, pw, (h - oy).max(ph)),
        Side::Left => (0.0, oy, ox + pw, ph),
        Side::Right => (ox, oy, (w - ox).max(pw), ph),
    }
}

/// The panel's hover box given `world` and `th`.
pub fn panel_hover_box_in(
    p: &Panel,
    w: f64,
    h: f64,
    world: &modules::World,
    th: &theme::Theme,
) -> (f64, f64, f64, f64) {
    let (ox, oy, pw, ph) = panel_rect_in(p, w, h, world, th);
    match p.side {
        Side::Top => (ox, 0.0, pw, oy + ph),
        Side::Bottom => (ox, oy, pw, (h - oy).max(ph)),
        Side::Left => (0.0, oy, ox + pw, ph),
        Side::Right => (ox, oy, (w - ox).max(pw), ph),
    }
}

/// Command-line action (matching the C edge's `panel_of_args` and `main`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Run,
    Panel(String),
    Stop,
    PrintPanels,
    PrintTheme,
    Render {
        name: String,
        size: String,
        out: String,
    },
    Ask(Vec<String>),
}

pub fn parse_args<S: AsRef<str>>(args: &[S]) -> Result<Action, String> {
    let a: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    match a.as_slice() {
        [] => Ok(Action::Run),
        ["--print-panels"] => Ok(Action::PrintPanels),
        ["--print-theme"] => Ok(Action::PrintTheme),
        ["--render", name, size, out] => Ok(Action::Render {
            name: (*name).to_string(),
            size: (*size).to_string(),
            out: (*out).to_string(),
        }),
        ["ask", rest @ ..] => Ok(Action::Ask(rest.iter().map(|s| (*s).to_string()).collect())),
        ["stop", ..] => Ok(Action::Stop),
        ["panel", name, ..] if !name.is_empty() => Ok(Action::Panel((*name).to_string())),
        ["widgets", ..] => Ok(Action::Panel(wayland::WIDGETS_REQUEST.to_string())),
        ["control", ..] => Ok(Action::Panel("control".to_string())),
        ["apps", ..] => Ok(Action::Panel("apps".to_string())),
        ["clipboard", ..] => Ok(Action::Panel("clipboard".to_string())),
        ["toggle" | "center" | "open", ..] => Ok(Action::Panel("windows".to_string())),
        _ => Err(
            "usage: infynitae-edge-rs [panel NAME | control | apps | clipboard | toggle | widgets | stop | --print-panels | --print-theme | --render NAME|widgets WxH OUT.png]"
                .to_string(),
        ),
    }
}

/// Key input to the open panel state machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Super(char),
    ShiftReturn,
    Sym { keysym: u32, ch: Option<char> },
}

/// The open/close and input state machine for panels.
pub struct Shell {
    pub cfg: Config,
    pub theme: theme::Theme,
    pub config_path: PathBuf,
    pub config_mtime: Option<SystemTime>,
    pub open: Option<usize>,
    pub opened_by_hover: bool,
    pub pointer_down: bool,
    pub last_screen: (f64, f64),
    pub world: modules::World,
    pub edit: modules::edit::EditState,
    /// the desktop's widgets (stage 5): its one panel is the WIDGETS line, always open
    pub desktop: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    pub fn new() -> Shell {
        let path = config::config_path();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let cfg = Config::load();
        let theme = theme::Theme::from_config(&cfg);
        let mut world = modules::World::now();
        if world.apps.favorites.is_empty() && !cfg.favorites.is_empty() {
            world.apps.set_favorites(&cfg.favorites);
        }
        let edit = modules::edit::EditState {
            snap: cfg.snap,
            overlap: cfg.overlap,
            ..Default::default()
        };
        Shell {
            cfg,
            theme,
            config_path: path,
            config_mtime: mtime,
            open: None,
            opened_by_hover: false,
            pointer_down: false,
            last_screen: (1920.0, 1080.0),
            world,
            edit,
            desktop: false,
        }
    }

    #[cfg(test)]
    pub fn from_config(cfg: Config) -> Shell {
        let theme = theme::Theme::from_config(&cfg);
        let mut world = modules::World::now();
        if world.apps.favorites.is_empty() && !cfg.favorites.is_empty() {
            world.apps.set_favorites(&cfg.favorites);
        }
        let edit = modules::edit::EditState {
            snap: cfg.snap,
            overlap: cfg.overlap,
            ..Default::default()
        };
        Shell {
            cfg,
            theme,
            config_path: PathBuf::new(),
            config_mtime: None,
            open: None,
            opened_by_hover: false,
            pointer_down: false,
            last_screen: (1920.0, 1080.0),
            world,
            edit,
            desktop: false,
        }
    }

    /// The desktop's widgets: a shell whose one panel is the WIDGETS line, always open.
    pub fn widgets() -> Shell {
        let mut s = Shell::new();
        s.desktop = true;
        s.use_widgets();
        s
    }

    fn use_widgets(&mut self) {
        self.cfg.panels = vec![self.cfg.widgets.clone().unwrap_or_else(Panel::no_widgets)];
        self.open = Some(0);
    }

    /// Widgets to show (or the desktop being edited, even with none yet).
    pub fn shows_widgets(&self) -> bool {
        self.desktop
            && (self.edit.active || self.cfg.panels.first().is_some_and(|p| !p.items.is_empty()))
    }

    /// A panel opened by hover closes when the pointer leaves it, unless something opened from
    /// it is in use: a card beside it, a tray app's menu, the editing (the pointer has to cross
    /// to them, or works outside the panel).
    pub fn hover_closes(&self) -> bool {
        self.opened_by_hover
            && !self.edit.active
            && !self.world.child.open
            && self.world.tray.open_menu.is_none()
    }

    /// reload_if_changed, saying whether the config was read again.
    pub fn reload_config_changed(&mut self) -> bool {
        let before = self.config_mtime;
        self.reload_if_changed();
        before != self.config_mtime
    }

    /// Re-reads the config file if its modification time changed since the last load.
    pub fn reload_if_changed(&mut self) {
        // never while arranging: the panel being edited is the one Done saves (Snap and Overlap
        // write the config themselves, and a reload then put another panel in its place)
        if self.config_path.as_os_str().is_empty() || self.edit.active {
            return;
        }
        let mtime = std::fs::metadata(&self.config_path)
            .and_then(|m| m.modified())
            .ok();
        if mtime.is_some() && mtime != self.config_mtime {
            if let Ok(text) = std::fs::read_to_string(&self.config_path) {
                self.cfg = Config::from_text(&text);
                self.theme = theme::Theme::from_config(&self.cfg);
                self.world.apps.set_favorites(&self.cfg.favorites);
                self.config_mtime = mtime;
                if !self.edit.active {
                    self.edit.snap = self.cfg.snap;
                    self.edit.overlap = self.cfg.overlap;
                }
                if self.desktop {
                    if !self.edit.active {
                        self.use_widgets();
                    }
                } else if let Some(idx) = self.open {
                    if idx >= self.cfg.panels.len() {
                        self.open = None;
                        self.opened_by_hover = false;
                        self.pointer_down = false;
                    }
                }
            }
        }
    }

    pub fn panel_index(&self, name: &str) -> Option<usize> {
        self.cfg
            .panels
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(name))
    }

    fn prepare_panel(&mut self, idx: usize) {
        self.world.on_open();
        if let Some(p) = self.cfg.panels.get(idx) {
            if p.has(config::Module::Apps) || p.has(config::Module::Favorites) {
                self.world.apps.ensure_loaded(&self.world.search);
            }
            if self.world.apps.favorites.is_empty() && !self.cfg.favorites.is_empty() {
                self.world.apps.set_favorites(&self.cfg.favorites);
            } else {
                self.world.apps.resolve_favorites();
            }
        }
        self.sync_selection();
    }

    fn sync_selection(&mut self) {
        let Some(open_idx) = self.open else {
            return;
        };
        let Some(panel) = self.cfg.panels.get(open_idx).cloned() else {
            return;
        };
        let (w, h) = self.last_screen;
        let pw = panel_width(&panel, w);
        let (rects, content_h) = layout_in(&panel, pw, &self.world, &self.theme);
        self.world
            .ensure_selection_visible(&panel, &rects, content_h, max_panel_height(h));
    }

    fn move_selection(&mut self, delta: isize) -> bool {
        let Some(open_idx) = self.open else {
            return false;
        };
        let Some(panel) = self.cfg.panels.get(open_idx).cloned() else {
            return false;
        };
        let (w, h) = self.last_screen;
        let pw = panel_width(&panel, w);
        let (rects, content_h) = layout_in(&panel, pw, &self.world, &self.theme);
        let nav = self.world.nav_items(&panel, &rects);
        if nav.is_empty() {
            return false;
        }
        let max_idx = (nav.len() - 1) as isize;
        let cur = (self.world.selected as isize).clamp(0, max_idx);
        let next = (cur + delta).clamp(0, max_idx) as usize;
        self.world.selected = next;
        self.world
            .ensure_selection_visible(&panel, &rects, content_h, max_panel_height(h));
        true
    }

    fn activate_selected(&mut self, secondary: bool) -> bool {
        self.sync_selection();
        let changed = match self.world.active_target {
            Some(modules::NavTarget::Favorite(slot)) => {
                if let Some(&app_idx) = self.world.apps.fav_indices.get(slot) {
                    if secondary {
                        modules::favorites::toggle_favorite(&mut self.world, app_idx)
                    } else {
                        modules::apps::launch_app(&mut self.world, app_idx)
                    }
                } else {
                    false
                }
            }
            Some(modules::NavTarget::App(row)) => {
                if let Some(&app_idx) = self.world.apps.filtered.get(row) {
                    if secondary {
                        modules::favorites::toggle_favorite(&mut self.world, app_idx)
                    } else {
                        modules::apps::launch_app(&mut self.world, app_idx)
                    }
                } else {
                    false
                }
            }
            Some(modules::NavTarget::Window(row)) => {
                if let Some(&win_idx) = self.world.windows.filtered.get(row) {
                    if secondary {
                        modules::windows::close_window(&mut self.world, win_idx)
                    } else {
                        modules::windows::activate_window(&mut self.world, win_idx)
                    }
                } else {
                    false
                }
            }
            Some(modules::NavTarget::Module(m)) => {
                if secondary {
                    match m {
                        config::Module::Wifi => {
                            modules::toggle_child(&mut self.world, modules::child::ChildKind::Wifi)
                        }
                        config::Module::Bluetooth => modules::toggle_child(
                            &mut self.world,
                            modules::child::ChildKind::Bluetooth,
                        ),
                        config::Module::Volume => {
                            modules::toggle_child(&mut self.world, modules::child::ChildKind::Audio)
                        }
                        config::Module::Power => {
                            modules::toggle_child(&mut self.world, modules::child::ChildKind::Power)
                        }
                        config::Module::Clock => modules::toggle_child(
                            &mut self.world,
                            modules::child::ChildKind::Calendar,
                        ),
                        _ => false,
                    }
                } else if m.is_switch() {
                    modules::switches::click(m, &mut self.world, 1)
                } else {
                    false
                }
            }
            None => false,
        };
        self.cfg.favorites = self.world.apps.favorites.clone();
        if self.world.close_requested {
            self.world.close_requested = false;
            self.close();
        } else if changed {
            self.sync_selection();
        }
        changed
    }

    fn close_selected_item(&mut self) -> bool {
        self.sync_selection();
        if let Some(modules::NavTarget::Window(row)) = self.world.active_target {
            if let Some(&win_idx) = self.world.windows.filtered.get(row) {
                let changed = modules::windows::close_window(&mut self.world, win_idx);
                self.sync_selection();
                return changed;
            }
        }
        false
    }

    pub fn ask_load(&mut self) -> bool {
        self.reload_if_changed();
        let req_path = wayland::runtime_path(modules::ask::ASK_FILE);
        let changed = modules::ask::load_from_path(&mut self.world, &req_path);
        if self.world.ask.on {
            self.close();
        }
        changed
    }

    /// Does the open panel (or its card) show windows: the windows list, or apps and favorites
    /// with their running marks.
    pub fn shows_windows(&self) -> bool {
        let Some(i) = self.open else { return false };
        self.world.child.open
            || self.cfg.panels.get(i).is_some_and(|p| {
                use config::Module::{Apps, Favorites, Windows};
                p.has(Windows) || p.has(Apps) || p.has(Favorites)
            })
    }

    pub fn open_panel(&mut self, idx: usize, by_hover: bool) -> bool {
        if by_hover && self.world.ask.on {
            return false;
        }
        self.reload_if_changed();
        if idx >= self.cfg.panels.len() {
            return false;
        }
        if self.world.ask.on {
            modules::ask::answer(&mut self.world, None);
        }
        if self.open == Some(idx) {
            if by_hover {
                return false;
            }
            return self.close();
        }
        self.open = Some(idx);
        self.opened_by_hover = by_hover;
        self.pointer_down = false;
        self.prepare_panel(idx);
        true
    }

    pub fn toggle_named(&mut self, name: &str) -> bool {
        self.reload_if_changed();
        let Some(idx) = self.panel_index(name) else {
            return false;
        };
        if self.world.ask.on {
            modules::ask::answer(&mut self.world, None);
        }
        if self.open == Some(idx) {
            self.close()
        } else {
            self.open = Some(idx);
            self.opened_by_hover = false;
            self.pointer_down = false;
            self.prepare_panel(idx);
            true
        }
    }

    /// The edit bar where it is drawn now: under, over or beside the panel as it is after a
    /// move or resize (a bar left at the panel's old height took the clicks meant for it
    /// somewhere else).
    fn refresh_bar(&mut self) {
        if !self.edit.active {
            return;
        }
        let Some(panel) = self.open.and_then(|i| self.cfg.panels.get(i)) else {
            return;
        };
        let (w, h) = self.last_screen;
        let pw = panel_width(panel, w);
        let (_, content_h) = layout_in(panel, pw, &self.world, &self.theme);
        let ph = ph_of(panel, content_h, h);
        let (ox, oy) = panel_origin(panel, w, h, pw, ph);
        self.edit.bar_rect =
            modules::edit::calculate_bar_rect(panel, (ox, oy), pw, ph, (w, h), self.edit.adding);
    }

    pub fn edit_enter(&mut self) -> bool {
        let Some(open_idx) = self.open else {
            return false;
        };
        if self.edit.active {
            return false;
        }
        self.world.child.open = false;
        self.world.tray.open_menu = None;
        let (w, h) = self.last_screen;
        if let Some(panel) = self.cfg.panels.get_mut(open_idx) {
            let pw = panel_width(panel, w);
            let (rects, content_h) = layout_in(panel, pw, &self.world, &self.theme);
            let ph = ph_of(panel, content_h, h);
            let (ox, oy) = panel_origin(panel, w, h, pw, ph);
            self.edit.before = panel.value();
            if !panel.placed {
                modules::edit::freeze_layout(panel, &rects, pw);
            }
            self.edit.active = true;
            self.edit.changed = false;
            self.edit.adding = false;
            self.edit.undo.clear();
            self.edit.drag = None;
            self.edit.live = false;
            self.edit.selected = None;
            self.edit.bar_rect =
                modules::edit::calculate_bar_rect(panel, (ox, oy), pw, ph, (w, h), false);
            self.opened_by_hover = false;
            return true;
        }
        false
    }

    pub fn edit_exit(&mut self) -> bool {
        if !self.edit.active {
            return false;
        }
        self.edit.active = false;
        self.edit.adding = false;
        self.edit.drag = None;
        self.edit.live = false;
        if let Some(open_idx) = self.open {
            if self.edit.changed {
                if let Some(panel) = self.cfg.panels.get_mut(open_idx) {
                    panel.items.sort_by(|a, b| {
                        let ra = a.rect.unwrap_or((0, 0, 0, 0));
                        let rb = b.rect.unwrap_or((0, 0, 0, 0));
                        ra.1.cmp(&rb.1).then_with(|| ra.0.cmp(&rb.0))
                    });
                    let before = self.edit.before.clone();
                    modules::edit::save_panel(&mut self.world, panel, &before);
                }
            }
        }
        true
    }

    pub fn edit_press(&mut self, x: f64, y: f64, button: u32) -> bool {
        let r = self.edit_press_inner(x, y, button);
        self.refresh_bar();
        r
    }

    fn edit_press_inner(&mut self, x: f64, y: f64, button: u32) -> bool {
        let Some(open_idx) = self.open else {
            return false;
        };
        let (w, h) = self.last_screen;
        let Some(panel) = self.cfg.panels.get(open_idx).cloned() else {
            return false;
        };
        let pw = panel_width(&panel, w);
        let (_rects, content_h) = layout_in(&panel, pw, &self.world, &self.theme);
        let ph = ph_of(&panel, content_h, h);
        let (ox, oy) = panel_origin(&panel, w, h, pw, ph);

        let is_left = modules::is_left_button(button);
        let (bx, by, bw, bh) =
            modules::edit::calculate_bar_rect(&panel, (ox, oy), pw, ph, (w, h), self.edit.adding);
        self.edit.bar_rect = (bx, by, bw, bh);

        // Edit bar buttons (left click only)
        if is_left && x >= bx && x <= bx + bw && y >= by && y <= by + bh {
            let w6 = (bw - 10.0 - 5.0 * 4.0) / 6.0;
            if y >= by + 5.0 && y <= by + 29.0 {
                for i in 0..6 {
                    let btn_x = bx + 5.0 + i as f64 * (w6 + 4.0);
                    if x >= btn_x && x <= btn_x + w6 {
                        match i {
                            0 => {
                                // Undo
                                if let Some(prev) = self.edit.undo.pop() {
                                    if let Some(p) = self.cfg.panels.get_mut(open_idx) {
                                        *p = prev;
                                        self.edit.changed = true;
                                    }
                                }
                            }
                            1 => {
                                // Reset
                                if let Some(p) =
                                    self.cfg.panels.get_mut(open_idx).filter(|p| p.desktop)
                                {
                                    // the desktop's Reset: no widgets
                                    self.edit.undo.push(p.clone());
                                    p.items.clear();
                                    self.edit.changed = true;
                                } else if let Some(p) = self.cfg.panels.get_mut(open_idx) {
                                    self.edit.undo.push(p.clone());
                                    let mods = match p.name.as_str() {
                                        "control" => modules::edit::DEFAULT_CONTROL,
                                        "apps" => modules::edit::DEFAULT_APPS,
                                        "windows" => modules::edit::DEFAULT_WINDOWS,
                                        "clipboard" => modules::edit::DEFAULT_CLIPBOARD,
                                        _ => &[],
                                    };
                                    if !mods.is_empty() {
                                        p.items = mods
                                            .iter()
                                            .map(|&m| config::Item {
                                                module: m,
                                                look: None,
                                                rect: None,
                                            })
                                            .collect();
                                    }
                                    p.placed = false;
                                    p.width = 0;
                                    p.height = 0;
                                    let pw = panel_width(p, w);
                                    let (rects, _) = layout_in(p, pw, &self.world, &self.theme);
                                    modules::edit::freeze_layout(p, &rects, pw);
                                    self.edit.changed = true;
                                }
                            }
                            2 => {
                                // Snap
                                self.edit.snap = !self.edit.snap;
                                let v = if self.edit.snap { "true" } else { "false" };
                                self.world.set_config("PANELS_SNAP", v);
                            }
                            3 => {
                                // Overlap
                                self.edit.overlap = !self.edit.overlap;
                                let v = if self.edit.overlap { "true" } else { "false" };
                                self.world.set_config("PANELS_OVERLAP", v);
                            }
                            4 => {
                                // Add
                                self.edit.adding = !self.edit.adding;
                                let (bx, by, bw, bh) = modules::edit::calculate_bar_rect(
                                    &panel,
                                    (ox, oy),
                                    pw,
                                    ph,
                                    (w, h),
                                    self.edit.adding,
                                );
                                self.edit.bar_rect = (bx, by, bw, bh);
                            }
                            5 => {
                                // Done
                                self.edit_exit();
                            }
                            _ => {}
                        }
                        return true;
                    }
                }
            }

            // If Add menu is open
            if self.edit.adding && y >= by + 34.0 {
                let available = modules::edit::addable(&panel);
                let w4 = (bw - 10.0 - 3.0 * 4.0) / 4.0;
                for (k, &m) in available.iter().enumerate() {
                    let col = k % 4;
                    let row = k / 4;
                    let ax = bx + 5.0 + col as f64 * (w4 + 4.0);
                    let ay = by + 34.0 + 4.0 + row as f64 * 30.0;
                    if x >= ax && x <= ax + w4 && y >= ay && y <= ay + 26.0 {
                        if let Some(p) = self.cfg.panels.get_mut(open_idx) {
                            self.edit.undo.push(p.clone());
                            let (dw, dh) = if p.desktop {
                                modules::edit::widget_default_size(m)
                            } else {
                                modules::edit::module_default_size(m, pw)
                            };
                            let (fx, fy) =
                                modules::edit::nearest_free(p, p.items.len(), dw, dh, pw);
                            p.items.push(config::Item {
                                module: m,
                                look: None,
                                rect: Some((fx, fy, dw, dh)),
                            });
                            self.edit.changed = true;
                            self.edit.selected = Some(p.items.len() - 1);
                        }
                        return true;
                    }
                }
            }
            return true;
        }

        // Remove button for selected module
        if is_left {
            if let Some(sel) = self.edit.selected {
                if let Some(it) = panel.items.get(sel) {
                    if let Some((mx, my, mw, mh)) = it.rect {
                        let rx = ox + mx as f64 + mw as f64 - 58.0 - 4.0;
                        let ry = if mw < 68 || mh < 28 {
                            oy + my as f64 - 18.0 - 3.0
                        } else {
                            oy + my as f64 + 4.0
                        };
                        if x >= rx && x <= rx + 58.0 && y >= ry && y <= ry + 18.0 {
                            if let Some(p) = self.cfg.panels.get_mut(open_idx) {
                                self.edit.undo.push(p.clone());
                                p.items.remove(sel);
                                self.edit.selected = None;
                                self.edit.changed = true;
                            }
                            return true;
                        }
                    }
                }
            }
        }

        // Outside panel and outside edit bar -> edit_exit()
        let in_panel = x >= ox && x <= ox + pw && y >= oy && y <= oy + ph;
        if !in_panel {
            self.edit_exit();
            return true;
        }

        let px_in = x - ox;
        let py_in = y - oy;
        let mi = modules::edit::module_under(&panel, px_in, py_in);

        let is_right = modules::is_right_button(button);
        if let Some(mi) = mi {
            let it = &panel.items[mi];
            let (mx, my, mw, mh) = it.rect.unwrap();
            let corner = px_in > (mx + mw) as f64 - 16.0 && py_in > (my + mh) as f64 - 16.0;
            let kind = if is_right || corner {
                modules::edit::DragKind::ResizeModule(mi)
            } else {
                modules::edit::DragKind::MoveModule(mi)
            };
            self.edit.drag = Some(modules::edit::DragState {
                kind,
                start_pos: (x, y),
                initial_rect: (mx, my, mw, mh),
            });
            self.edit.live = false;
            self.pointer_down = true;
        } else if panel.desktop {
            // the desktop: no panel to move or resize; a press beside the widgets unselects
            self.edit.selected = None;
        } else {
            let cx = if panel.align > 0 && panel.side.horizontal() {
                0.0
            } else {
                pw
            };
            let cy = if panel.side == config::Side::Bottom {
                0.0
            } else {
                ph
            };
            let corner = (px_in - cx).abs() < 16.0 && (py_in - cy).abs() < 16.0;
            let kind = if is_right || corner {
                modules::edit::DragKind::ResizePanel
            } else {
                modules::edit::DragKind::MovePanel
            };
            self.edit.drag = Some(modules::edit::DragState {
                kind,
                start_pos: (x, y),
                initial_rect: (0, 0, pw.round() as i32, ph.round() as i32),
            });
            self.edit.live = false;
            self.pointer_down = true;
        }
        true
    }

    pub fn edit_drag(&mut self, x: f64, y: f64) -> bool {
        let r = self.edit_drag_inner(x, y);
        self.refresh_bar();
        r
    }

    fn edit_drag_inner(&mut self, x: f64, y: f64) -> bool {
        let Some(open_idx) = self.open else {
            return false;
        };
        let Some(drag) = self.edit.drag.clone() else {
            return false;
        };
        let (w, h) = self.last_screen;
        let (x0, y0) = drag.start_pos;
        let (dx, dy) = (x - x0, y - y0);
        if !self.edit.live {
            if dx.hypot(dy) < 6.0 {
                return false;
            }
            self.edit.live = true;
            if let Some(p) = self.cfg.panels.get(open_idx) {
                self.edit.undo.push(p.clone());
            }
        }

        let Some(panel) = self.cfg.panels.get_mut(open_idx) else {
            return false;
        };
        let pw = panel_width(panel, w);

        match drag.kind {
            modules::edit::DragKind::MoveModule(mi) => {
                if mi >= panel.items.len() {
                    return false;
                }
                let (r0_x, r0_y, mw, mh) = drag.initial_rect;
                let mut nx = r0_x + dx.round() as i32;
                let mut ny = r0_y + dy.round() as i32;
                if self.edit.snap {
                    modules::edit::snap_move(panel, mi, pw, &mut nx, &mut ny, mw, mh);
                }
                let max_x = (pw - modules::edit::PAD).round() as i32 - mw;
                nx = nx.clamp(
                    modules::edit::PAD as i32,
                    max_x.max(modules::edit::PAD as i32),
                );
                ny = ny.max(modules::edit::PAD as i32);
                panel.items[mi].rect = Some((nx, ny, mw, mh));
                self.edit.changed = true;
            }
            modules::edit::DragKind::ResizeModule(mi) => {
                if mi >= panel.items.len() {
                    return false;
                }
                let (r0_x, r0_y, r0_w, r0_h) = drag.initial_rect;
                let mut nw = r0_w + dx.round() as i32;
                let mut nh = r0_h + dy.round() as i32;
                if self.edit.snap {
                    modules::edit::snap_size(panel, mi, pw, r0_x, r0_y, &mut nw, &mut nh);
                }
                let max_w = (pw - modules::edit::PAD).round() as i32 - r0_x;
                nw = nw.clamp(40, max_w.max(40));
                nh = nh.max(26);

                if !self.edit.overlap {
                    for (k, it) in panel.items.iter().enumerate() {
                        if k == mi {
                            continue;
                        }
                        if let Some((ox, oy, ow, oh)) = it.rect {
                            if ow <= 0 || oh <= 0 {
                                continue;
                            }
                            if r0_x < ox + ow && ox < r0_x + nw && oy >= r0_y && oy < r0_y + nh {
                                let gap_limit = oy - modules::edit::GAP as i32 - r0_y;
                                if gap_limit >= 26 {
                                    nh = nh.min(gap_limit);
                                }
                            }
                            if r0_y < oy + oh && oy < r0_y + nh && ox >= r0_x && ox < r0_x + nw {
                                let gap_limit = ox - modules::edit::GAP as i32 - r0_x;
                                if gap_limit >= 40 {
                                    nw = nw.min(gap_limit);
                                }
                            }
                        }
                    }
                }

                panel.items[mi].rect = Some((r0_x, r0_y, nw, nh));
                self.edit.changed = true;
            }
            modules::edit::DragKind::MovePanel => {
                let d = [y, h - y, x, w - x];
                let mut side_idx = 0;
                for i in 1..4 {
                    if d[i] < d[side_idx] {
                        side_idx = i;
                    }
                }
                let sides = [
                    config::Side::Top,
                    config::Side::Bottom,
                    config::Side::Left,
                    config::Side::Right,
                ];
                let along = if side_idx < 2 { x / w } else { y / h };
                panel.side = sides[side_idx];
                panel.align = if along < 1.0 / 3.0 {
                    -1
                } else if along > 2.0 / 3.0 {
                    1
                } else {
                    0
                };
                self.edit.changed = true;
            }
            modules::edit::DragKind::ResizePanel => {
                let horizontal = panel.side.horizontal();
                let gw = if horizontal {
                    if panel.align < 0 {
                        dx
                    } else if panel.align > 0 {
                        -dx
                    } else {
                        2.0 * dx
                    }
                } else {
                    if panel.side == config::Side::Left {
                        dx
                    } else {
                        -dx
                    }
                };
                let gh = if horizontal {
                    if panel.side == config::Side::Top {
                        dy
                    } else {
                        -dy
                    }
                } else {
                    if panel.align < 0 {
                        dy
                    } else if panel.align > 0 {
                        -dy
                    } else {
                        2.0 * dy
                    }
                };
                let mut need_w = 160;
                let mut need_h = 60;
                for it in &panel.items {
                    if let Some((mx, my, mw, mh)) = it.rect {
                        if mw <= 0 || mh <= 0 {
                            continue;
                        }
                        need_w = need_w.max(mx + mw + modules::edit::PAD as i32);
                        need_h = need_h.max(my + mh + modules::edit::PAD as i32);
                    }
                }
                let nw = (drag.initial_rect.2 as f64 + gw).round() as i32;
                let nh = (drag.initial_rect.3 as f64 + gh).round() as i32;
                panel.width = nw.max(need_w);
                panel.height = nh.max(need_h);
                self.edit.changed = true;
            }
        }
        true
    }

    pub fn edit_release(&mut self) -> bool {
        let r = self.edit_release_inner();
        self.refresh_bar();
        r
    }

    fn edit_release_inner(&mut self) -> bool {
        if !self.edit.active {
            return false;
        }
        let Some(open_idx) = self.open else {
            return false;
        };
        let drag = self.edit.drag.take();
        self.pointer_down = false;
        let (w, _h) = self.last_screen;

        if let Some(drag) = drag {
            if !self.edit.live {
                match drag.kind {
                    modules::edit::DragKind::MoveModule(mi) => {
                        self.edit.selected = if self.edit.selected == Some(mi) {
                            None
                        } else {
                            Some(mi)
                        };
                    }
                    modules::edit::DragKind::MovePanel => {
                        self.edit.selected = None;
                    }
                    _ => {}
                }
            } else {
                if let modules::edit::DragKind::MoveModule(mi) = drag.kind {
                    if !self.edit.overlap {
                        if let Some(panel) = self.cfg.panels.get_mut(open_idx) {
                            let pw = panel_width(panel, w);
                            if let Some(it) = panel.items.get(mi) {
                                if let Some((mx, my, mw, mh)) = it.rect {
                                    if modules::edit::covers_other(panel, mi, mx, my, mw, mh) {
                                        let (mut fx, mut fy) =
                                            modules::edit::nearest_free(panel, mi, mw, mh, pw);
                                        if self.edit.snap {
                                            let (mut sx, mut sy) = (fx, fy);
                                            modules::edit::snap_move(
                                                panel, mi, pw, &mut sx, &mut sy, mw, mh,
                                            );
                                            if !modules::edit::covers_other(
                                                panel, mi, sx, sy, mw, mh,
                                            ) && sx >= modules::edit::PAD as i32
                                            {
                                                fx = sx;
                                                fy = sy;
                                            }
                                        }
                                        panel.items[mi].rect = Some((fx, fy, mw, mh));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            self.edit.live = false;
            return true;
        }
        false
    }

    pub fn close(&mut self) -> bool {
        if self.desktop {
            // the widgets stay: closing ends editing (saving) and what was open in them
            let was =
                self.edit.active || self.world.child.open || self.world.tray.open_menu.is_some();
            self.edit_exit();
            self.world.child.open = false;
            self.world.tray.open_menu = None;
            self.pointer_down = false;
            return was;
        }
        if self.edit.active {
            self.edit_exit();
        }
        self.world.child.open = false;
        self.world.tray.open_menu = None;
        self.pointer_down = false;
        if self.open.is_some() {
            self.open = None;
            self.opened_by_hover = false;
            true
        } else {
            false
        }
    }

    pub fn key(&mut self, k: Key) -> bool {
        if self.world.ask.on {
            return match k {
                Key::Escape => modules::ask::key(&mut self.world, 0xff1b, None),
                Key::ShiftReturn => modules::ask::key(&mut self.world, 0xff0d, None),
                Key::Super(_) => false,
                Key::Sym { keysym, ch } => modules::ask::key(&mut self.world, keysym, ch),
            };
        }
        let Some(open_idx) = self.open else {
            return false;
        };
        match k {
            Key::Escape => {
                if self.edit.active {
                    self.edit_exit();
                    return true;
                }
                if self.world.child.open {
                    self.world.child.open = false;
                    return true;
                }
                self.close()
            }
            Key::Super(c) => {
                let c = c.to_ascii_lowercase();
                if self.cfg.panels.get(open_idx).and_then(|p| p.key) == Some(c) {
                    self.close()
                } else if let Some(other) = self.cfg.panels.iter().position(|p| p.key == Some(c)) {
                    self.open_panel(other, false)
                } else {
                    false
                }
            }
            Key::ShiftReturn => {
                if let Some(modules::NavTarget::Module(m)) = self.world.active_target {
                    match m {
                        config::Module::Wifi => {
                            return modules::toggle_child(
                                &mut self.world,
                                modules::child::ChildKind::Wifi,
                            )
                        }
                        config::Module::Bluetooth => {
                            return modules::toggle_child(
                                &mut self.world,
                                modules::child::ChildKind::Bluetooth,
                            )
                        }
                        config::Module::Volume => {
                            return modules::toggle_child(
                                &mut self.world,
                                modules::child::ChildKind::Audio,
                            )
                        }
                        config::Module::Power => {
                            return modules::toggle_child(
                                &mut self.world,
                                modules::child::ChildKind::Power,
                            )
                        }
                        config::Module::Clock => {
                            return modules::toggle_child(
                                &mut self.world,
                                modules::child::ChildKind::Calendar,
                            )
                        }
                        _ => {}
                    }
                }
                self.activate_selected(true)
            }
            Key::Sym { keysym, ch } => {
                match keysym {
                    0xff1b => {
                        if self.edit.active {
                            self.edit_exit();
                            return true;
                        }
                        if self.world.child.open {
                            self.world.child.open = false;
                            return true;
                        }
                        return self.close();
                    }
                    0xff0d | 0xff8d => return self.activate_selected(false),
                    0xff67 => {
                        if let Some(modules::NavTarget::Module(m)) = self.world.active_target {
                            match m {
                                config::Module::Wifi => {
                                    return modules::toggle_child(
                                        &mut self.world,
                                        modules::child::ChildKind::Wifi,
                                    )
                                }
                                config::Module::Bluetooth => {
                                    return modules::toggle_child(
                                        &mut self.world,
                                        modules::child::ChildKind::Bluetooth,
                                    )
                                }
                                config::Module::Volume => {
                                    return modules::toggle_child(
                                        &mut self.world,
                                        modules::child::ChildKind::Audio,
                                    )
                                }
                                config::Module::Power => {
                                    return modules::toggle_child(
                                        &mut self.world,
                                        modules::child::ChildKind::Power,
                                    )
                                }
                                config::Module::Clock => {
                                    return modules::toggle_child(
                                        &mut self.world,
                                        modules::child::ChildKind::Calendar,
                                    )
                                }
                                _ => {}
                            }
                        }
                        return self.activate_selected(true);
                    }
                    0xffff => return self.close_selected_item(),
                    0xff54 | 0xff09 | 0xff53 => return self.move_selection(1),
                    0xff52 | 0xfe20 | 0xff51 => return self.move_selection(-1),
                    0xff56 => return self.move_selection(8),
                    0xff55 => return self.move_selection(-8),
                    0xff08 => {
                        if !self.world.search.is_empty() {
                            let mut next = self.world.search.clone();
                            next.pop();
                            self.world.set_search(next);
                            self.sync_selection();
                            return true;
                        }
                        return false;
                    }
                    _ => {}
                }
                let mut changed = false;
                if let Some(p) = self.cfg.panels.get(open_idx) {
                    for it in &p.items {
                        changed |= modules::key(it, &mut self.world, keysym, ch);
                    }
                }
                if !changed {
                    if let Some(c) = ch.filter(|c| !c.is_control()) {
                        let mut next = self.world.search.clone();
                        next.push(c);
                        self.world.set_search(next);
                        self.sync_selection();
                        changed = true;
                    }
                }
                changed
            }
        }
    }

    pub fn click(&mut self, w: f64, h: f64, x: f64, y: f64, button: u32) -> bool {
        self.last_screen = (w, h);
        if self.world.ask.on {
            return modules::ask::click(&mut self.world, w, h, x, y, button);
        }
        let Some(open_idx) = self.open else {
            return self.pointer_motion(w, h, x, y);
        };
        let Some(panel) = self.cfg.panels.get(open_idx) else {
            return self.close();
        };
        let pw = panel_width(panel, w);
        let (rects, content_h) = layout_in(panel, pw, &self.world, &self.theme);
        let ph = ph_of(panel, content_h, h);
        let (ox, oy) = panel_origin(panel, w, h, pw, ph);

        // 1. Child list interaction
        if self.world.child.open {
            let child_kind = self
                .world
                .child
                .kind
                .unwrap_or(modules::child::ChildKind::Wifi);
            let parent_rect = modules::child::find_parent_rect(panel, &rects, child_kind);
            let parent_y = parent_rect.map(|r| r.1).unwrap_or(0.0);
            let cr_rect = modules::child::child_card_geometry(
                child_kind,
                (ox, oy),
                pw,
                parent_y,
                self.world.child.items.len(),
                (w, h),
            );
            if x >= cr_rect.0
                && x <= cr_rect.0 + cr_rect.2
                && y >= cr_rect.1
                && y <= cr_rect.1 + cr_rect.3
            {
                let (changed, _) =
                    modules::child::click_child(&mut self.world, cr_rect, (x, y), button);
                return changed;
            } else {
                self.world.child.open = false;
                return true;
            }
        }

        // 2. Tray menu interaction
        if let Some(open_tray_idx) = self.world.tray.open_menu {
            if let Some((_, r)) = panel
                .items
                .iter()
                .zip(&rects)
                .find(|(it, _)| it.module == config::Module::Tray)
            {
                let item_count = self
                    .world
                    .tray
                    .items
                    .get(open_tray_idx)
                    .map(|it| it.effective_menu_items().len())
                    .unwrap_or(3);
                let m_rect = modules::tray::menu_rect((ox, oy), (pw, ph), *r, item_count, (w, h));
                if x >= m_rect.0
                    && x <= m_rect.0 + m_rect.2
                    && y >= m_rect.1
                    && y <= m_rect.1 + m_rect.3
                {
                    return modules::tray::click_menu(&mut self.world, m_rect, (x, y));
                }
            }
        }

        // 3. Edit mode interaction
        if self.edit.active {
            return self.edit_press(x, y, button);
        }

        // 4. Click outside panel closes it
        if x < ox || x > ox + pw || y < oy || y > oy + ph {
            self.world.tray.open_menu = None;
            return self.close();
        }

        // a right click on a widget starts editing the desktop's widgets
        if panel.desktop && modules::is_right_button(button) {
            return self.edit_enter();
        }

        // 5. Click inside panel
        if modules::is_left_button(button) {
            self.pointer_down = true;
        }
        let oy_scrolled = oy - self.world.panel_scroll;
        let mut clicked_module = false;
        let mut changed = false;
        for (it, r) in panel.items.iter().zip(rects) {
            let (mx, my, mw, mh) = (ox + r.0, oy_scrolled + r.1, r.2, r.3);
            if mw > 0.0 && mh > 0.0 && x >= mx && x <= mx + mw && y >= my && y <= my + mh {
                clicked_module = true;
                changed |= modules::click(it, &mut self.world, (mw, mh), (x - mx, y - my), button);
            }
        }

        // the desktop's widgets open no card beside them (placed for a panel, it went to the
        // screen's far edge, outside the widgets' input region where it could not be clicked)
        if panel.desktop && self.world.child.open {
            self.world.child.open = false;
        }

        // Right click on empty background enters edit mode
        if !clicked_module && modules::is_right_button(button) {
            return self.edit_enter();
        }

        self.cfg.favorites = self.world.apps.favorites.clone();
        if self.world.close_requested {
            self.world.close_requested = false;
            self.close();
            return true;
        }
        if changed {
            self.sync_selection();
        }
        changed
    }

    pub fn pointer_release(&mut self) -> bool {
        self.pointer_down = false;
        if self.edit.active {
            return self.edit_release();
        }
        false
    }

    pub fn scroll(&mut self, w: f64, h: f64, x: f64, y: f64, dy: f64) -> bool {
        self.last_screen = (w, h);
        let Some(open_idx) = self.open else {
            return false;
        };
        let Some(panel) = self.cfg.panels.get(open_idx) else {
            return false;
        };
        let pw = panel_width(panel, w);
        let (rects, content_h) = layout_in(panel, pw, &self.world, &self.theme);
        let ph = ph_of(panel, content_h, h);
        let (ox, oy) = panel_origin(panel, w, h, pw, ph);
        let oy_scrolled = oy - self.world.panel_scroll;
        let mut changed = false;
        for (it, r) in panel.items.iter().zip(&rects) {
            let (mx, my, mw, mh) = (ox + r.0, oy_scrolled + r.1, r.2, r.3);
            if mw > 0.0 && mh > 0.0 && x >= mx && x <= mx + mw && y >= my && y <= my + mh {
                changed |= modules::scroll(it, &mut self.world, (mw, mh), (x - mx, y - my), dy);
            }
        }
        if !changed && content_h > ph && x >= ox && x <= ox + pw && y >= oy && y <= oy + ph {
            let max_scroll = (content_h - ph).max(0.0);
            let step = if dy > 0.0 {
                40.0
            } else if dy < 0.0 {
                -40.0
            } else {
                0.0
            };
            let next = (self.world.panel_scroll + step).clamp(0.0, max_scroll);
            if (next - self.world.panel_scroll).abs() > 0.01 {
                self.world.panel_scroll = next;
                changed = true;
            }
        }
        changed
    }

    pub fn pointer_motion(&mut self, w: f64, h: f64, x: f64, y: f64) -> bool {
        self.last_screen = (w, h);
        if self.world.ask.on {
            return modules::ask::hover(&mut self.world, w, h, x, y);
        }
        if self.edit.active {
            if self.pointer_down {
                return self.edit_drag(x, y);
            } else if let Some(open_idx) = self.open {
                if let Some(panel) = self.cfg.panels.get(open_idx) {
                    let pw = panel_width(panel, w);
                    let (_, content_h) = layout_in(panel, pw, &self.world, &self.theme);
                    let ph = ph_of(panel, content_h, h);
                    let (ox, oy) = panel_origin(panel, w, h, pw, ph);
                    let px = x - ox;
                    let py = y - oy;
                    let next_hover = modules::edit::module_under(panel, px, py);
                    if next_hover != self.edit.hover_module {
                        self.edit.hover_module = next_hover;
                        return true;
                    }
                }
            }
            return false;
        }
        if let Some(open_idx) = self.open {
            if let Some(p) = self.cfg.panels.get(open_idx) {
                if self.hover_closes() {
                    let (bx, by, bw, bh) = panel_hover_box_in(p, w, h, &self.world, &self.theme);
                    if x < bx || x > bx + bw || y < by || y > by + bh {
                        return self.close();
                    }
                }
                if self.pointer_down {
                    let pw = panel_width(p, w);
                    let (rects, content_h) = layout_in(p, pw, &self.world, &self.theme);
                    let ph = ph_of(p, content_h, h);
                    let (ox, oy) = panel_origin(p, w, h, pw, ph);
                    let oy_scrolled = oy - self.world.panel_scroll;
                    let mut changed = false;
                    for (it, r) in p.items.iter().zip(rects) {
                        let (mx, my, mw, mh) = (ox + r.0, oy_scrolled + r.1, r.2, r.3);
                        if mw > 0.0
                            && mh > 0.0
                            && x >= mx
                            && x <= mx + mw
                            && y >= my
                            && y <= my + mh
                        {
                            changed |=
                                modules::drag(it, &mut self.world, (mw, mh), (x - mx, y - my));
                        }
                    }
                    return changed;
                }
            }
            return false;
        }
        for i in 0..self.cfg.panels.len() {
            if !self.cfg.panels[i].hover {
                continue;
            }
            let (sx, sy, sw, sh) =
                hover_strip_in(&self.cfg.panels[i], w, h, &self.world, &self.theme);
            if x >= sx && x <= sx + sw && y >= sy && y <= sy + sh {
                let name = self.cfg.panels[i].name.clone();
                self.reload_if_changed();
                let idx = self.panel_index(&name).unwrap_or(i);
                return self.open_panel(idx, true);
            }
        }
        false
    }

    pub fn pointer_leave(&mut self) -> bool {
        self.pointer_down = false;
        if self.world.ask.on
            || self.edit.active
            || self.world.child.open
            || self.world.tray.open_menu.is_some()
        {
            return false;
        }
        let Some(open_idx) = self.open else {
            return false;
        };
        if self.opened_by_hover || self.cfg.panels.get(open_idx).is_some_and(|p| p.hover) {
            self.close()
        } else {
            false
        }
    }
}

fn render(name: &str, size: &str, out: &str) -> Result<(), String> {
    let (w, h) = size
        .split_once('x')
        .and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?)))
        .filter(|&(w, h)| (200..=8192).contains(&w) && (200..=8192).contains(&h))
        .ok_or("--render needs WxH, e.g. 1920x1080")?;
    let mut cfg = Config::load();
    let th = theme::Theme::from_config(&cfg);
    // the desktop's widgets: their one panel, the whole screen
    if name == "widgets" {
        cfg.panels = vec![cfg
            .widgets
            .clone()
            .ok_or("no WIDGETS line with a placed widget")?];
    }
    let is_ask = name == "ask";
    let panel_idx = if is_ask {
        None
    } else {
        Some(
            cfg.panels
                .iter()
                .position(|p| p.name == name)
                .ok_or(format!("no panel {name}"))?,
        )
    };
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).map_err(|e| e.to_string())?;
    let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
    // the canvas behind, as the desktop's: the cards are see-through
    cr.set_source_rgb(
        0x0e as f64 / 255.0,
        0x0a as f64 / 255.0,
        0x0c as f64 / 255.0,
    );
    let _ = cr.paint();
    let mut world = modules::World::now();
    if let Some(open_idx) = panel_idx {
        let is_editing = std::env::var_os("INFYNITAE_EDGE_RENDER_EDIT").is_some();
        let is_adding = std::env::var_os("INFYNITAE_EDGE_RENDER_ADD").is_some();

        let panel = &mut cfg.panels[open_idx];
        if panel.has(config::Module::Apps) || panel.has(config::Module::Favorites) {
            world.apps.ensure_loaded(&world.search);
        }
        if world.apps.favorites.is_empty() && !cfg.favorites.is_empty() {
            world.apps.set_favorites(&cfg.favorites);
        }

        let pw = panel_width(panel, w as f64);
        let (rects_init, content_h_init) = layout_in(panel, pw, &world, &th);
        let ph_init = ph_of(panel, content_h_init, h as f64);
        let (ox_init, oy_init) = panel_origin(panel, w as f64, h as f64, pw, ph_init);

        let mut edit_state = modules::edit::EditState {
            active: is_editing,
            adding: is_adding,
            snap: cfg.snap,
            overlap: cfg.overlap,
            bar_rect: modules::edit::calculate_bar_rect(
                panel,
                (ox_init, oy_init),
                pw,
                ph_init,
                (w as f64, h as f64),
                is_adding,
            ),
            ..Default::default()
        };

        if is_editing {
            if !panel.placed {
                modules::edit::freeze_layout(panel, &rects_init, pw);
            }
            if let Ok(v) = std::env::var("INFYNITAE_EDGE_RENDER_EDIT") {
                if let Ok(n) = v.parse::<usize>() {
                    if n > 0 && n <= panel.items.len() {
                        edit_state.selected = Some(n - 1);
                        edit_state.hover_module = Some(n - 1);
                    }
                }
            }
        }

        // Child subpanel via env
        let mut child_kind = None;
        if let Ok(sub) = std::env::var("INFYNITAE_EDGE_RENDER_SUB") {
            let k = match sub.as_str() {
                "cal" => modules::child::ChildKind::Calendar,
                "bt" => modules::child::ChildKind::Bluetooth,
                "audio" => modules::child::ChildKind::Audio,
                "power" => modules::child::ChildKind::Power,
                _ => modules::child::ChildKind::Wifi,
            };
            modules::toggle_child(&mut world, k);
            child_kind = Some(k);
        }

        let (rects, content_h) = layout_in(panel, pw, &world, &th);
        let ph = ph_of(panel, content_h, h as f64);
        let (ox, oy) = panel_origin(panel, w as f64, h as f64, pw, ph);

        if content_h > ph {
            let _ = cr.save();
            cr.rectangle(ox, oy, pw, ph);
            cr.clip();
            modules::draw_panel(&cr, &th, panel, &rects, &world, (ox, oy), pw);
            let _ = cr.restore();
        } else {
            modules::draw_panel(&cr, &th, panel, &rects, &world, (ox, oy), pw);
        }

        if let Some(open_idx) = world.tray.open_menu {
            if let Some((_, r)) = panel
                .items
                .iter()
                .zip(&rects)
                .find(|(it, _)| it.module == config::Module::Tray)
            {
                let item_count = world
                    .tray
                    .items
                    .get(open_idx)
                    .map(|it| it.effective_menu_items().len())
                    .unwrap_or(3);
                let m_rect = modules::tray::menu_rect(
                    (ox, oy),
                    (pw, ph),
                    *r,
                    item_count,
                    (w as f64, h as f64),
                );
                modules::tray::draw_menu(&cr, &th, &world, m_rect);
            }
        }

        let mut total_rect = (
            ox.round() as i32,
            oy.round() as i32,
            pw.round() as i32,
            ph.round() as i32,
        );

        if is_editing {
            edit_state.bar_rect = modules::edit::calculate_bar_rect(
                panel,
                (ox, oy),
                pw,
                ph,
                (w as f64, h as f64),
                is_adding,
            );
            modules::edit::draw_edit_overlay(
                &cr,
                &th,
                panel,
                (ox, oy),
                pw,
                ph,
                (w as f64, h as f64),
                &edit_state,
            );
            let (bx, by, bw, bh) = edit_state.bar_rect;
            let min_x = total_rect.0.min(bx.round() as i32 - 2);
            let min_y = total_rect.1.min(by.round() as i32 - 2);
            let max_x = (total_rect.0 + total_rect.2).max((bx + bw).round() as i32 + 2);
            let max_y = (total_rect.1 + total_rect.3).max((by + bh).round() as i32 + 2);
            total_rect = (min_x, min_y, max_x - min_x, max_y - min_y);
        }

        let mut drawn_child_rect = None;
        if let Some(k) = child_kind {
            if !is_editing {
                let parent_rect = modules::child::find_parent_rect(panel, &rects, k);
                let parent_y = parent_rect.map(|r| r.1).unwrap_or(0.0);
                let c_rect = modules::child::child_card_geometry(
                    k,
                    (ox, oy),
                    pw,
                    parent_y,
                    world.child.items.len(),
                    (w as f64, h as f64),
                );
                modules::child::draw_child_card(&cr, &th, &world, &world.child, c_rect);
                drawn_child_rect = Some(c_rect);
                let (cx, cy, cw, ch) = c_rect;
                let min_x = total_rect.0.min(cx.round() as i32 - 2);
                let min_y = total_rect.1.min(cy.round() as i32 - 2);
                let max_x = (total_rect.0 + total_rect.2).max((cx + cw).round() as i32 + 2);
                let max_y = (total_rect.1 + total_rect.3).max((cy + ch).round() as i32 + 2);
                total_rect = (min_x, min_y, max_x - min_x, max_y - min_y);
            }
        }

        modules::dump_boxes(
            panel,
            &rects,
            (ox, oy, pw, ph),
            is_editing.then_some(edit_state.bar_rect),
            drawn_child_rect,
        );

        if std::env::var_os("INFYNITAE_EDGE_RENDER_RECT").is_some() {
            println!(
                "{} {} {} {}",
                total_rect.0, total_rect.1, total_rect.2, total_rect.3
            );
        }
    } else {
        if std::env::var_os("INFYNITAE_EDGE_RENDER_ASK").is_some() || !world.ask.on {
            world.ask.load_render_env();
        }
        let (gx, gy, gw, gh) = modules::ask::draw(&cr, &th, &world.ask, (w as f64, h as f64));
        if std::env::var_os("INFYNITAE_EDGE_RENDER_RECT").is_some() {
            println!(
                "{} {} {} {}",
                gx.round() as i32,
                gy.round() as i32,
                gw.round() as i32,
                gh.round() as i32
            );
        }
    }
    drop(cr);
    let mut f = std::fs::File::create(out).map_err(|e| e.to_string())?;
    surface.write_to_png(&mut f).map_err(|e| e.to_string())?;
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Ok(Action::PrintPanels) => {
            for p in Config::load().panels {
                println!("{} {}", p.name, p.value());
            }
            ExitCode::SUCCESS
        }
        Ok(Action::PrintTheme) => {
            print!(
                "{}",
                theme::Theme::from_config(&Config::load()).shell_lines()
            );
            ExitCode::SUCCESS
        }
        Ok(Action::Render { name, size, out }) => match render(&name, &size, &out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[infynitae-edge-rs] {e}");
                ExitCode::from(2)
            }
        },
        Ok(Action::Stop) => {
            wayland::stop_running();
            ExitCode::SUCCESS
        }
        Ok(Action::Ask(ask_args)) => ExitCode::from(modules::ask::run_client(&ask_args)),
        Ok(Action::Panel(name)) => {
            if wayland::send_panel(&name) {
                return ExitCode::SUCCESS;
            }
            match wayland::run(Some(&name)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("[infynitae-edge-rs] {e}");
                    ExitCode::from(1)
                }
            }
        }
        Ok(Action::Run) => match wayland::run(None) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[infynitae-edge-rs] {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widgets_shell(line: &str) -> Shell {
        let mut s = Shell::from_config(Config::from_text(line));
        s.desktop = true;
        s.use_widgets();
        s
    }

    #[test]
    fn the_widgets_line_reads_and_writes_back() {
        let c = Config::from_text(
            "WIDGETS=\"clock:big@60,60,380x150 calendar@60,230,380x330 media wifi@500,40,180x46\"\n",
        );
        let w = c.widgets.expect("widgets");
        assert!(w.desktop && w.placed);
        // a widget without its place is left out
        assert_eq!(
            w.value(),
            "clock:big@60,60,380x150 calendar@60,230,380x330 wifi@500,40,180x46"
        );
        assert!(Config::from_text("WIDGETS=\"\"\n").widgets.is_none());
        assert!(Config::from_text("").widgets.is_none());
    }

    #[test]
    fn widgets_are_the_whole_screen_and_their_region_is_the_widgets() {
        let s = widgets_shell("WIDGETS=\"clock@60,60,380x150 calendar@60,230,380x330\"\n");
        assert!(s.shows_widgets() && s.open == Some(0));
        let p = &s.cfg.panels[0];
        assert_eq!(panel_width(p, 1920.0), 1920.0);
        assert_eq!(panel_origin(p, 1920.0, 1080.0, 1920.0, 1080.0), (0.0, 0.0));
        assert_eq!(ph_of(p, 500.0, 1080.0), 1080.0);
        let (rects, _) = layout_in(p, 1920.0, &s.world, &s.theme);
        assert_eq!(
            wayland::widget_region(&rects),
            vec![(60, 60, 380, 150), (60, 230, 380, 330)]
        );
        // no line: nothing shown until editing starts
        let mut none = widgets_shell("");
        assert!(!none.shows_widgets());
        assert!(none.edit_enter());
        assert!(none.shows_widgets());
    }

    #[test]
    fn a_click_reaches_the_widget_under_it_and_a_right_click_edits() {
        let mut s = widgets_shell("WIDGETS=\"calendar@100,100,380x330\"\n");
        let before = s.world.cal_month;
        // the next-month arrow, at the calendar's top right
        assert!(s.click(1920.0, 1080.0, 100.0 + 380.0 - 26.0, 100.0 + 26.0, 0x110));
        assert_eq!(s.world.cal_month, before + 1);
        assert!(!s.edit.active);
        assert!(s.click(1920.0, 1080.0, 200.0, 300.0, 0x111));
        assert!(s.edit.active);
        // Escape ends editing; the widgets stay
        assert!(s.close());
        assert!(!s.edit.active && s.open == Some(0));
    }

    #[test]
    fn done_writes_the_widgets_line() {
        let p = Config::from_text("WIDGETS=\"clock@60,60,380x150\"\n")
            .widgets
            .unwrap();
        let mut world = modules::World::now();
        modules::edit::save_panel(&mut world, &p, "clock@0,0,380x150");
        assert_eq!(
            world.actions,
            vec![
                "backup WIDGETS=\"clock@0,0,380x150\"".to_string(),
                "set WIDGETS=\"clock@60,60,380x150\"".to_string()
            ]
        );
    }

    #[test]
    fn the_widgets_clock_wakes_on_the_minute() {
        use std::time::Duration;
        assert_eq!(
            wayland::until_next_minute(60_000 * 7 + 59_000),
            Duration::from_millis(1_005)
        );
        assert_eq!(
            wayland::until_next_minute(60_000 * 7),
            Duration::from_millis(60_005)
        );
    }

    #[test]
    fn command_line_parses_into_actions() {
        let empty: &[&str] = &[];
        assert_eq!(parse_args(empty), Ok(Action::Run));
        assert_eq!(
            parse_args(&["panel", "control"]),
            Ok(Action::Panel("control".into()))
        );
        assert_eq!(
            parse_args(&["panel", "media"]),
            Ok(Action::Panel("media".into()))
        );
        assert_eq!(
            parse_args(&["control"]),
            Ok(Action::Panel("control".into()))
        );
        assert_eq!(parse_args(&["apps"]), Ok(Action::Panel("apps".into())));
        assert_eq!(
            parse_args(&["clipboard"]),
            Ok(Action::Panel("clipboard".into()))
        );
        for alias in ["toggle", "center", "open"] {
            assert_eq!(parse_args(&[alias]), Ok(Action::Panel("windows".into())));
        }
        assert_eq!(parse_args(&["stop"]), Ok(Action::Stop));
        assert_eq!(parse_args(&["--print-panels"]), Ok(Action::PrintPanels));
        assert_eq!(parse_args(&["--print-theme"]), Ok(Action::PrintTheme));
        assert_eq!(
            parse_args(&["widgets"]),
            Ok(Action::Panel(wayland::WIDGETS_REQUEST.into()))
        );
        assert_eq!(
            parse_args(&["--render", "control", "1920x1080", "/tmp/out.png"]),
            Ok(Action::Render {
                name: "control".into(),
                size: "1920x1080".into(),
                out: "/tmp/out.png".into(),
            })
        );
        assert_eq!(
            parse_args(&["ask", "--text", "Name for the drive"]),
            Ok(Action::Ask(vec![
                "--text".into(),
                "Name for the drive".into()
            ]))
        );
        assert!(parse_args(&["panel"]).is_err());
        assert!(parse_args(&["--unknown"]).is_err());
    }

    #[test]
    fn window_changes_redraw_only_panels_that_show_windows() {
        let mut sh = Shell::from_config(Config::from_text(
            "PANEL_A=\"top left 300 clock\"\nPANEL_B=\"top right 300 windows\"\n",
        ));
        assert!(!sh.shows_windows(), "nothing open");
        sh.open = sh.panel_index("a");
        assert!(!sh.shows_windows(), "a clock-only panel");
        sh.open = sh.panel_index("b");
        assert!(sh.shows_windows(), "a windows list");
    }

    #[test]
    fn hover_strip_rectangles_per_side_and_alignment() {
        let cfg_h = Config::from_text(
            "PANEL_TL=\"top left 300 hover clock\"\n\
             PANEL_TC=\"top center 300 hover clock\"\n\
             PANEL_TR=\"top right 300 hover clock\"\n\
             PANEL_BL=\"bottom left 300 hover clock\"\n\
             PANEL_BC=\"bottom center 300 hover clock\"\n\
             PANEL_BR=\"bottom right 300 hover clock\"\n",
        );
        let find_h = |name: &str| cfg_h.panels.iter().find(|p| p.name == name).unwrap();
        let (w, h) = (1920.0, 1080.0);
        let (_, ph) = layout(find_h("tl"), 300.0);

        assert_eq!(hover_strip(find_h("tl"), w, h), (8.0, 0.0, 300.0, 2.0));
        assert_eq!(hover_strip(find_h("tc"), w, h), (810.0, 0.0, 300.0, 2.0));
        assert_eq!(hover_strip(find_h("tr"), w, h), (1612.0, 0.0, 300.0, 2.0));

        assert_eq!(hover_strip(find_h("bl"), w, h), (8.0, 1078.0, 300.0, 2.0));
        assert_eq!(hover_strip(find_h("bc"), w, h), (810.0, 1078.0, 300.0, 2.0));
        assert_eq!(
            hover_strip(find_h("br"), w, h),
            (1612.0, 1078.0, 300.0, 2.0)
        );

        let cfg_v = Config::from_text(
            "PANEL_LT=\"left top 300 hover clock\"\n\
             PANEL_LC=\"left center 300 hover clock\"\n\
             PANEL_LB=\"left bottom 300 hover clock\"\n\
             PANEL_RT=\"right top 300 hover clock\"\n\
             PANEL_RC=\"right center 300 hover clock\"\n\
             PANEL_RB=\"right bottom 300 hover clock\"\n",
        );
        let find_v = |name: &str| cfg_v.panels.iter().find(|p| p.name == name).unwrap();
        let mid_y = ((h - ph) / 2.0).round();
        let bot_y = h - ph - 8.0;
        assert_eq!(hover_strip(find_v("lt"), w, h), (0.0, 8.0, 2.0, ph));
        assert_eq!(hover_strip(find_v("lc"), w, h), (0.0, mid_y, 2.0, ph));
        assert_eq!(hover_strip(find_v("lb"), w, h), (0.0, bot_y, 2.0, ph));
        assert_eq!(hover_strip(find_v("rt"), w, h), (1918.0, 8.0, 2.0, ph));
        assert_eq!(hover_strip(find_v("rc"), w, h), (1918.0, mid_y, 2.0, ph));
        assert_eq!(hover_strip(find_v("rb"), w, h), (1918.0, bot_y, 2.0, ph));
    }

    #[test]
    fn open_close_state_machine_transitions() {
        let cfg = Config::from_text(
            "EDGE_POSITION=\"top\"\nEDGE_CONTROL_SIDE=\"right\"\nPANEL_CONTROL=\"SUPER+A top right hover calendar\"\n",
        );
        let mut sh = Shell::from_config(cfg);
        let (w, h) = (1920.0, 1080.0);

        // open by name, toggle same panel closes
        assert_eq!(sh.open, None);
        assert!(sh.toggle_named("control"));
        let ctrl_idx = sh.panel_index("control").unwrap();
        let apps_idx = sh.panel_index("apps").unwrap();
        assert_eq!(sh.open, Some(ctrl_idx));
        assert!(sh.toggle_named("control"));
        assert_eq!(sh.open, None);

        // open, same key (SUPER+A) closes
        assert!(sh.toggle_named("control"));
        assert_eq!(sh.open, Some(ctrl_idx));
        assert!(sh.key(Key::Super('a')));
        assert_eq!(sh.open, None);

        // open, other panel's key (SUPER+W) switches to that panel
        assert!(sh.toggle_named("control"));
        assert!(sh.key(Key::Super('W')));
        assert_eq!(sh.open, Some(apps_idx));

        // Escape closes
        assert!(sh.key(Key::Escape));
        assert_eq!(sh.open, None);

        // click inside calendar arrow changes month and stays open; click outside closes
        assert!(sh.toggle_named("control"));
        let (ox, oy, pw, _ph) = panel_rect(&sh.cfg.panels[ctrl_idx], w, h);
        let (prev, _) = modules::calendar_arrows(pw);
        assert!(sh.click(w, h, ox + prev.0 + 4.0, oy + prev.1 + 10.0, 1));
        assert_eq!(sh.open, Some(ctrl_idx));
        assert_eq!(sh.world.cal_month, -1);

        // click outside closes
        assert!(sh.click(w, h, 20.0, 500.0, 1));
        assert_eq!(sh.open, None);

        // hover strip opens hover=true panel, pointer leave closes it
        let strip = hover_strip(&sh.cfg.panels[ctrl_idx], w, h);
        assert!(sh.pointer_motion(w, h, strip.0 + 10.0, strip.1 + 1.0));
        assert_eq!(sh.open, Some(ctrl_idx));
        assert!(sh.opened_by_hover);
        assert!(sh.pointer_leave());
        assert_eq!(sh.open, None);
    }

    #[test]
    fn config_reloads_when_mtime_changes_on_open() {
        let path =
            std::env::temp_dir().join(format!("infynitae-edge-rs-test-cfg-{}", std::process::id()));
        std::fs::write(&path, "EDGE_POSITION=\"top\"\n").unwrap();
        let mtime1 = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let mut sh = Shell::from_config(Config::from_text("EDGE_POSITION=\"top\"\n"));
        sh.config_path = path.clone();
        sh.config_mtime = mtime1;
        assert_eq!(sh.cfg.panels[0].side, Side::Top);

        // Simulate a newer mtime by backdating sh.config_mtime by 2 seconds
        sh.config_mtime = mtime1.map(|t| t - std::time::Duration::from_secs(2));
        std::fs::write(&path, "EDGE_POSITION=\"bottom\"\n").unwrap();

        assert!(sh.toggle_named("control"));
        assert_eq!(sh.cfg.panels[0].side, Side::Bottom);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn layout_and_clicks_work_for_switches_and_volume() {
        for look in ["rows", "icons", "tiles"] {
            let cfg = Config::from_text(&format!(
                "SWITCH_LOOK=\"{look}\"\n\
                 PANEL_CONTROL=\"SUPER+A top right wifi bluetooth dnd nightlight keyboard osk power replay volume\"\n"
            ));
            let mut sh = Shell::from_config(cfg);
            sh.world.apply("wifi", "enabled\nyes:HomeNet");
            sh.world.apply("bluetooth", "Powered: no\n");
            sh.world.apply("keyboard", "US\tEnglish (US)\t2\n");
            sh.world.apply("osk", "hidden\n");
            sh.world.apply("power", "balanced\n");
            sh.world
                .apply("volume", "Volume: 50%\nMute: no\nDescription: Speakers\n");
            sh.world
                .apply("brightness", "amdgpu_bl1,backlight,128,50%,255\n");

            assert!(sh.toggle_named("control"));
            let (w, h) = (1920.0, 1080.0);
            let panel = sh.cfg.panels[sh.open.unwrap()].clone();
            let pw = panel_width(&panel, w);
            let (rects, ph) = layout_in(&panel, pw, &sh.world, &sh.theme);
            let (ox, oy) = panel_origin(&panel, w, h, pw, ph);
            assert!(ph > 100.0);

            // Click wifi (index 0)
            let r_wifi = rects[0];
            assert!(sh.click(w, h, ox + r_wifi.0 + 10.0, oy + r_wifi.1 + 10.0, 1));
            assert!(!sh.world.switches.wifi_on);
            assert_eq!(
                sh.world.actions.last().map(String::as_str),
                Some("nmcli radio wifi off")
            );
            sh.pointer_release();

            // Click and drag volume (index 8)
            let r_vol = rects[8];
            assert!(sh.click(w, h, ox + r_vol.0 + r_vol.2 - 14.0, oy + r_vol.1 + 36.0, 1));
            assert_eq!(sh.world.volume.pct, 100);
            assert!(sh.pointer_motion(w, h, ox + r_vol.0 + 14.0, oy + r_vol.1 + 36.0));
            assert_eq!(sh.world.volume.pct, 0);
            sh.pointer_release();

            // Scroll volume up by 1 notch (-1.0)
            assert!(sh.scroll(w, h, ox + r_vol.0 + 40.0, oy + r_vol.1 + 25.0, -1.0));
            assert_eq!(sh.world.volume.pct, 5);
        }
    }

    #[test]
    fn layout_and_clicks_work_for_session_clipboard_and_notifications() {
        let cfg = Config::from_text(
            "PANEL_CONTROL=\"SUPER+A top right notifications session\"\n\
             PANEL_CLIPBOARD=\"SUPER+V right center clipboard\"\n",
        );
        let mut sh = Shell::from_config(cfg);
        sh.world.apply(
            "notifications",
            "42\tFirefox\t2m\tBuild finished\tAll checks passed\n",
        );
        sh.world.apply("clipboard", "108\tgit push origin main\n");

        let (w, h) = (1920.0, 1080.0);
        assert!(sh.toggle_named("control"));
        let panel = sh.cfg.panels[sh.open.unwrap()].clone();
        let pw = panel_width(&panel, w);
        let (rects, ph) = layout_in(&panel, pw, &sh.world, &sh.theme);
        let (ox, oy) = panel_origin(&panel, w, h, pw, ph);
        assert_eq!(rects.len(), 2);
        assert!(rects[0].3 > 0.0);
        assert!(rects[1].3 > 0.0);

        // Click notification item -> invokes default action, dismisses, and hides notifications
        let r_notif = rects[0];
        assert!(sh.click(w, h, ox + r_notif.0 + 40.0, oy + r_notif.1 + 60.0, 1));
        sh.pointer_release();
        assert!(sh.world.notifications.items.is_empty());
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some("makoctl invoke -n 42 default 2>/dev/null; makoctl dismiss -n 42 2>/dev/null")
        );

        // Now notifications height is 0; click session Lock button
        let (rects2, ph2) = layout_in(&panel, pw, &sh.world, &sh.theme);
        let (ox2, oy2) = panel_origin(&panel, w, h, pw, ph2);
        assert_eq!(rects2[0].3, 0.0);
        let r_sess = rects2[1];
        assert!(sh.click(w, h, ox2 + r_sess.0 + 30.0, oy2 + r_sess.1 + 30.0, 1));
        sh.pointer_release();
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some(
                "infynitae msg lock >/dev/null 2>&1 || setsid -f ~/.local/bin/infynitae-lock >/dev/null 2>&1 || setsid -f ~/infynitae/live/lock/infynitae-lock >/dev/null 2>&1"
            )
        );

        // Open clipboard panel and click first row -> copies and closes panel
        assert!(sh.toggle_named("clipboard"));
        let clip_panel = sh.cfg.panels[sh.open.unwrap()].clone();
        let cpw = panel_width(&clip_panel, w);
        let (crects, cph) = layout_in(&clip_panel, cpw, &sh.world, &sh.theme);
        let (cox, coy) = panel_origin(&clip_panel, w, h, cpw, cph);
        let r_clip = crects[0];
        assert!(sh.click(w, h, cox + r_clip.0 + 40.0, coy + r_clip.1 + 55.0, 1));
        assert_eq!(sh.open, None);
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some("printf '%s\\n' '108\tgit push origin main' | cliphist decode 2>/dev/null | wl-copy")
        );
    }

    #[test]
    fn layout_and_clicks_work_for_tray_and_media() {
        let cfg = Config::from_text("PANEL_TRAY_MEDIA=\"SUPER+M top right media tray\"\n");
        let mut sh = Shell::from_config(cfg);
        sh.world.apply(
            "media",
            "status=Playing\ntitle=Starless\nartist=King Crimson\nalbum=Red\nplayer=Melodyae\nposition=234000000\nduration=742000000\nlyrics_current=Starless and bible black\nqueue=Starless\tKing Crimson\t\t742000000\n",
        );
        sh.world.apply(
            "tray",
            "Steam\t2159 MB\nDiscord\t2 new\tattention\nmenu:10\tSettings\nmenu:20\tQuit\n",
        );

        assert!(sh.toggle_named("tray_media"));
        let (w, h) = (1920.0, 1080.0);
        let panel = sh.cfg.panels[sh.open.unwrap()].clone();
        let pw = panel_width(&panel, w);
        let (rects, ph) = layout_in(&panel, pw, &sh.world, &sh.theme);
        let (ox, oy) = panel_origin(&panel, w, h, pw, ph);

        assert_eq!(rects.len(), 2);
        let r_media = rects[0];
        let r_tray = rects[1];
        assert!(r_media.3 > 150.0);
        assert!(r_tray.3 > 60.0);

        // Click play/pause in media (medium size)
        let play_pos = (ox + pw / 2.0, oy + r_media.1 + 146.0);
        assert!(sh.click(w, h, play_pos.0, play_pos.1, 1));
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some("media:play-pause")
        );

        // Left click Steam in tray activates Steam
        let slot0 = modules::tray::item_slot((r_tray.2, r_tray.3), 0).expect("slot0");
        assert!(sh.click(
            w,
            h,
            ox + r_tray.0 + slot0.0 + 10.0,
            oy + r_tray.1 + slot0.1 + 10.0,
            1
        ));
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some("tray:activate:steam")
        );

        // Right click Discord in tray opens menu beside the panel
        let slot1 = modules::tray::item_slot((r_tray.2, r_tray.3), 1).expect("slot1");
        assert!(sh.click(
            w,
            h,
            ox + r_tray.0 + slot1.0 + 10.0,
            oy + r_tray.1 + slot1.1 + 10.0,
            3
        ));
        assert_eq!(sh.world.tray.open_menu, Some(1));

        // Click menu item in menu beside panel
        let m_rect = modules::tray::menu_rect((ox, oy), (pw, ph), r_tray, 2, (w, h));
        // Click Quit (id 20)
        let menu_quit_y = m_rect.1 + 8.0 + 26.0 + 5.0;
        assert!(sh.click(w, h, m_rect.0 + 20.0, menu_quit_y, 1));
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some("tray:menu-action:discord:20")
        );
        assert_eq!(sh.world.tray.open_menu, None);
    }

    #[test]
    fn typing_in_open_panel_searches_and_enter_or_shift_enter_acts_on_selection() {
        let cfg = Config::from_text(
            "FAVORITES=\"foot\"\n\
             PANEL_APPS=\"SUPER+W top center search favorites apps\"\n",
        );
        let mut sh = Shell::from_config(cfg);
        sh.world.apply(
            "apps",
            "Firefox\tfirefox\tfirefox\torg.mozilla.firefox.desktop\tWeb Browser\tweb;\n\
             Foot\tfoot\tfoot\tfoot.desktop\tTerminal\tshell;\n\
             Files\tnautilus\tnautilus\torg.gnome.Nautilus.desktop\tFile Manager\tfolder;\n",
        );
        assert!(sh.toggle_named("apps"));

        // Initially Favorites has 1 item (foot), Apps has 3 items
        assert_eq!(sh.world.apps.fav_indices, vec![1]);
        assert_eq!(sh.world.apps.filtered.len(), 3);

        // Typing 'f', 'i', 'l' anywhere in the open panel filters apps to Files and hides favorites
        for c in ['f', 'i', 'l'] {
            assert!(sh.key(Key::Sym {
                keysym: c as u32,
                ch: Some(c),
            }));
        }
        assert_eq!(sh.world.search, "fil");
        assert_eq!(sh.world.apps.filtered, vec![2]);

        // Shift+Enter toggles Files into FAVORITES
        assert!(sh.key(Key::ShiftReturn));
        assert_eq!(sh.world.apps.favorites, "foot org.gnome.Nautilus");
        assert_eq!(sh.cfg.favorites, "foot org.gnome.Nautilus");

        // Enter launches the selected app (Files) and closes the panel
        assert!(sh.key(Key::Sym {
            keysym: 0xff0d,
            ch: Some('\r'),
        }));
        assert_eq!(sh.open, None);
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some("nautilus")
        );
    }

    #[test]
    fn tall_panel_clamps_to_screen_height_minus_16_and_scrolls_by_wheel_and_keys() {
        let cfg = Config::from_text(
            "FAVORITES=\"app0 app1 app2 app3 app4 app5\"\n\
             PANEL_APPS=\"SUPER+W top center search favorites apps windows\"\n",
        );
        let mut sh = Shell::from_config(cfg);
        let mut apps_tsv = String::new();
        for i in 0..12 {
            apps_tsv.push_str(&format!(
                "App{i}\tapp-cmd-{i}\tapp{i}\tapp{i}.desktop\tTool\ttest;\n"
            ));
        }
        sh.world.apply("apps", &apps_tsv);
        sh.world.apply(
            "windows",
            "1\tapp0\tWindow Zero\tfocused\n2\tapp1\tWindow One\t\n3\tapp2\tWindow Two\t\n",
        );
        assert!(sh.toggle_named("apps"));

        // Screen shorter than the panel's unclipped content height (e.g. 1920x420)
        let (w, h) = (1920.0, 420.0);
        sh.last_screen = (w, h);
        let panel = sh.cfg.panels[sh.open.unwrap()].clone();
        let (_, _, _, ph) = panel_rect_in(&panel, w, h, &sh.world, &sh.theme);
        assert_eq!(ph, h - 16.0);

        // Arrow Down past the 8 visible rows of Apps scrolls the Apps card and the panel
        for _ in 0..16 {
            assert!(sh.key(Key::Sym {
                keysym: 0xff54,
                ch: None,
            }));
        }
        // Selection is now at row 10 of Apps (after 6 favorites: index 16 -> App(10))
        assert_eq!(sh.world.active_target, Some(modules::NavTarget::App(10)));
        // Apps list scrolled inside its own card to keep row 10 in view (8 visible rows -> scroll = 3)
        assert_eq!(sh.world.apps.scroll, 3);
        // Panel scrolled to keep the Apps card in view inside the 404 px tall panel
        assert!(sh.world.panel_scroll > 0.0);

        // Scrolling with the wheel over the search bar (which does not scroll internally) scrolls the panel
        let prev_panel_scroll = sh.world.panel_scroll;
        assert!(sh.scroll(w, h, 960.0, 12.0, -1.0));
        assert!(sh.world.panel_scroll < prev_panel_scroll);
    }

    #[test]
    fn shell_ask_prompt_handles_keys_hover_and_clicks() {
        let cfg = Config::from_text("PANEL_CONTROL=\"SUPER+A top right hover clock\"\n");
        let mut sh = Shell::from_config(cfg);
        sh.world
            .apply("ask", "Format the drive as|exFAT|FAT32|ext4");
        assert!(sh.world.ask.on);

        let (w, h) = (1920.0, 1080.0);
        let (gx, gy, gw, _gh) = sh.world.ask.geometry(w, h);
        // Hovering over the panel's hover strip does not open the panel while asking
        assert!(!sh.pointer_motion(w, h, 1800.0, 1.0));
        assert_eq!(sh.open, None);
        // Pointer leave does not close the ask prompt
        assert!(!sh.pointer_leave());
        assert!(sh.world.ask.on);

        // Hover over row 1 selects FAT32, Enter confirms
        let row1_y = gy + draw::PAD_Y + modules::ask::TITLE_H + modules::ask::ROW_H * 1.5;
        assert!(sh.pointer_motion(w, h, gx + gw / 2.0, row1_y));
        assert_eq!(sh.world.ask.sel, 1);
        assert!(sh.key(Key::Sym {
            keysym: 0xff0d,
            ch: Some('\r'),
        }));
        assert!(!sh.world.ask.on);
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some("ask:answer:FAT32")
        );
    }
}
