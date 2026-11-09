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

/// The panel's rectangle `(x, y, w, h)` on an output of size `w x h`.
pub fn panel_rect(p: &Panel, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let pw = panel_width(p, w);
    let (_, ph) = layout(p, pw);
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
    let (_, ph) = layout_in(p, pw, world, th);
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
        Shell {
            cfg,
            theme,
            config_path: path,
            config_mtime: mtime,
            open: None,
            opened_by_hover: false,
            pointer_down: false,
            world: modules::World::now(),
        }
    }

    #[cfg(test)]
    pub fn from_config(cfg: Config) -> Shell {
        let theme = theme::Theme::from_config(&cfg);
        Shell {
            cfg,
            theme,
            config_path: PathBuf::new(),
            config_mtime: None,
            open: None,
            opened_by_hover: false,
            pointer_down: false,
            world: modules::World::now(),
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

    pub fn open_panel(&mut self, idx: usize, by_hover: bool) -> bool {
        self.reload_if_changed();
        if idx >= self.cfg.panels.len() {
            return false;
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
        self.world.on_open();
        true
    }

    pub fn toggle_named(&mut self, name: &str) -> bool {
        self.reload_if_changed();
        let Some(idx) = self.panel_index(name) else {
            return false;
        };
        if self.open == Some(idx) {
            self.close()
        } else {
            self.open = Some(idx);
            self.opened_by_hover = false;
            self.pointer_down = false;
            self.world.on_open();
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
            Key::Sym { keysym, ch } => {
                if keysym == 0xff1b {
                    return self.close();
                }
                let mut changed = false;
                if let Some(p) = self.cfg.panels.get(open_idx) {
                    for it in &p.items {
                        changed |= modules::key(it, &mut self.world, keysym, ch);
                    }
                }
                changed
            }
        }
    }

    pub fn click(&mut self, w: f64, h: f64, x: f64, y: f64, button: u32) -> bool {
        let Some(open_idx) = self.open else {
            return self.pointer_motion(w, h, x, y);
        };
        let Some(panel) = self.cfg.panels.get(open_idx) else {
            return self.close();
        };
        let pw = panel_width(panel, w);
        let (rects, ph) = layout_in(panel, pw, &self.world, &self.theme);
        let (ox, oy) = panel_origin(panel, w, h, pw, ph);
        if x < ox || x > ox + pw || y < oy || y > oy + ph {
            return self.close();
        }
        if modules::is_left_button(button) {
            self.pointer_down = true;
        }
        let mut changed = false;
        for (it, r) in panel.items.iter().zip(rects) {
            let (mx, my, mw, mh) = (ox + r.0, oy + r.1, r.2, r.3);
            if mw > 0.0 && mh > 0.0 && x >= mx && x <= mx + mw && y >= my && y <= my + mh {
                changed |= modules::click(it, &mut self.world, (mw, mh), (x - mx, y - my), button);
            }
        }
        changed
    }

    pub fn pointer_release(&mut self) {
        self.pointer_down = false;
    }

    pub fn scroll(&mut self, w: f64, h: f64, x: f64, y: f64, dy: f64) -> bool {
        let Some(open_idx) = self.open else {
            return false;
        };
        let Some(panel) = self.cfg.panels.get(open_idx) else {
            return false;
        };
        let pw = panel_width(panel, w);
        let (rects, ph) = layout_in(panel, pw, &self.world, &self.theme);
        let (ox, oy) = panel_origin(panel, w, h, pw, ph);
        let mut changed = false;
        for (it, r) in panel.items.iter().zip(rects) {
            let (mx, my, mw, mh) = (ox + r.0, oy + r.1, r.2, r.3);
            if mw > 0.0 && mh > 0.0 && x >= mx && x <= mx + mw && y >= my && y <= my + mh {
                changed |= modules::scroll(it, &mut self.world, (mw, mh), (x - mx, y - my), dy);
            }
        }
        changed
    }

    pub fn pointer_motion(&mut self, w: f64, h: f64, x: f64, y: f64) -> bool {
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
                    let (rects, ph) = layout_in(p, pw, &self.world, &self.theme);
                    let (ox, oy) = panel_origin(p, w, h, pw, ph);
                    let mut changed = false;
                    for (it, r) in p.items.iter().zip(rects) {
                        let (mx, my, mw, mh) = (ox + r.0, oy + r.1, r.2, r.3);
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
    let panel = cfg
        .panels
        .iter()
        .find(|p| p.name == name)
        .ok_or(format!("no panel {name}"))?;
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
    let world = modules::World::now();
    let pw = panel_width(panel, w as f64);
    let (rects, ph) = layout_in(panel, pw, &world, &th);
    let (ox, oy) = panel_origin(panel, w as f64, h as f64, pw, ph);
    modules::draw_panel(&cr, &th, panel, &rects, &world, (ox, oy), pw);
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
}
