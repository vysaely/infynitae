//! infynitae-edge-rs: Infynitae's panels and desktop widgets, in Rust (docs/EDGE_RUST.md).
//!
//!   infynitae-edge-rs                           run the Wayland edge
//!   infynitae-edge-rs panel NAME                open or close panel NAME
//!   infynitae-edge-rs control | apps | clipboard | toggle | center | open
//!   infynitae-edge-rs stop                      stop the running edge
//!   infynitae-edge-rs --print-panels            each panel as its PANEL_ line (as the C edge prints)
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
        let bottom = rects.iter().map(|r| r.1 + r.3).fold(0.0, f64::max);
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
pub fn max_panel_height(h: f64) -> f64 {
    (h - 16.0).max(0.0)
}

/// The panel's rectangle `(x, y, w, h)` on an output of size `w x h`.
pub fn panel_rect(p: &Panel, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let pw = panel_width(p, w);
    let (_, content_h) = layout(p, pw);
    let ph = content_h.min(max_panel_height(h));
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
    let ph = content_h.min(max_panel_height(h));
    let (ox, oy) = panel_origin(p, w, h, pw, ph);
    (ox, oy, pw, ph)
}

/// The 2 px hover strip at `p`'s place on its screen edge (its edge and alignment as in `panel_origin`).
pub fn hover_strip(p: &Panel, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let (ox, oy, pw, ph) = panel_rect(p, w, h);
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
        ["--render", name, size, out] => Ok(Action::Render {
            name: (*name).to_string(),
            size: (*size).to_string(),
            out: (*out).to_string(),
        }),
        ["ask", rest @ ..] => Ok(Action::Ask(rest.iter().map(|s| (*s).to_string()).collect())),
        ["stop", ..] => Ok(Action::Stop),
        ["panel", name, ..] if !name.is_empty() => Ok(Action::Panel((*name).to_string())),
        ["control", ..] => Ok(Action::Panel("control".to_string())),
        ["apps", ..] => Ok(Action::Panel("apps".to_string())),
        ["clipboard", ..] => Ok(Action::Panel("clipboard".to_string())),
        ["toggle" | "center" | "open", ..] => Ok(Action::Panel("windows".to_string())),
        _ => Err(
            "usage: infynitae-edge-rs [panel NAME | control | apps | clipboard | toggle | stop | --print-panels | --render NAME WxH OUT.png]"
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
        }
    }

    #[cfg(test)]
    pub fn from_config(cfg: Config) -> Shell {
        let theme = theme::Theme::from_config(&cfg);
        let mut world = modules::World::now();
        if world.apps.favorites.is_empty() && !cfg.favorites.is_empty() {
            world.apps.set_favorites(&cfg.favorites);
        }
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
        }
    }

    /// Re-reads the config file if its modification time changed since the last load.
    pub fn reload_if_changed(&mut self) {
        if self.config_path.as_os_str().is_empty() {
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
                if let Some(idx) = self.open {
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

    pub fn close(&mut self) -> bool {
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
            Key::Escape => self.close(),
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
            Key::ShiftReturn => self.activate_selected(true),
            Key::Sym { keysym, ch } => {
                match keysym {
                    0xff1b => return self.close(),
                    0xff0d | 0xff8d => return self.activate_selected(false),
                    0xff67 => return self.activate_selected(true),
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
        let ph = content_h.min(max_panel_height(h));
        let (ox, oy) = panel_origin(panel, w, h, pw, ph);
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
        if x < ox || x > ox + pw || y < oy || y > oy + ph {
            self.world.tray.open_menu = None;
            return self.close();
        }
        if modules::is_left_button(button) {
            self.pointer_down = true;
        }
        let oy_scrolled = oy - self.world.panel_scroll;
        let mut changed = false;
        for (it, r) in panel.items.iter().zip(rects) {
            let (mx, my, mw, mh) = (ox + r.0, oy_scrolled + r.1, r.2, r.3);
            if mw > 0.0 && mh > 0.0 && x >= mx && x <= mx + mw && y >= my && y <= my + mh {
                changed |= modules::click(it, &mut self.world, (mw, mh), (x - mx, y - my), button);
            }
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

    pub fn pointer_release(&mut self) {
        self.pointer_down = false;
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
        let ph = content_h.min(max_panel_height(h));
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
        if let Some(open_idx) = self.open {
            if let Some(p) = self.cfg.panels.get(open_idx) {
                if self.opened_by_hover {
                    let (bx, by, bw, bh) = panel_hover_box_in(p, w, h, &self.world, &self.theme);
                    if x < bx || x > bx + bw || y < by || y > by + bh {
                        return self.close();
                    }
                }
                if self.pointer_down {
                    let pw = panel_width(p, w);
                    let (rects, content_h) = layout_in(p, pw, &self.world, &self.theme);
                    let ph = content_h.min(max_panel_height(h));
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
            let (sx, sy, sw, sh) = hover_strip(&self.cfg.panels[i], w, h);
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
        if self.world.ask.on {
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
    let cfg = Config::load();
    let th = theme::Theme::from_config(&cfg);
    let is_ask = name == "ask";
    let panel = if is_ask {
        None
    } else {
        Some(
            cfg.panels
                .iter()
                .find(|p| p.name == name)
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
    if let Some(panel) = panel {
        if panel.has(config::Module::Apps) || panel.has(config::Module::Favorites) {
            world.apps.ensure_loaded(&world.search);
        }
        if world.apps.favorites.is_empty() && !cfg.favorites.is_empty() {
            world.apps.set_favorites(&cfg.favorites);
        }
        let pw = panel_width(panel, w as f64);
        let (rects, content_h) = layout_in(panel, pw, &world, &th);
        let ph = content_h.min(max_panel_height(h as f64));
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
        if std::env::var_os("INFYNITAE_EDGE_RENDER_RECT").is_some() {
            println!(
                "{} {} {} {}",
                ox.round() as i32,
                oy.round() as i32,
                pw.round() as i32,
                ph.round() as i32
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
