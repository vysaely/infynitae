//! Infynitae's panels, where each card comes from an app (`docs/EDGE_APPS.md`). Runs the Wayland
//! edge or one-shot subcommands (`ask`, `stop`, `--print-panels`, `--print-theme`, `--render`).

mod app;
mod ask;
mod boxes;
mod card;
mod cards;
mod config;
mod dbus;
mod draw;
mod edit;
mod icons;
mod notify;
mod notify_server;
mod panel;
mod paths;
mod provider;
mod recent;
mod shell;
mod theme;
mod tray_watcher;
mod ui;
mod wayland;
mod widgets;

use config::Config;
use std::process::ExitCode;

/// A still picture of panel `name` on a `WxH` screen, drawn by the same shell as the live edge,
/// from fixed data: `INFYNITAE_EDGE_RENDER_DATA` (the sources' fixture, `windows`, `notes`),
/// `_TIME`, `_POINTER`, `_SEARCH` (what was typed), `_SUB` (a card kind's list shown open). The
/// desktop's widgets are drawn under it, as on the screen; `widgets` draws them alone.
fn render(name: &str, size: &str, out: &str) -> Result<(), String> {
    render_with(Config::load(), name, size, out)
}

/// `INFYNITAE_EDGE_RENDER_POINTER` ("x,y" in screen pixels): the pointer's place for the hover light.
fn render_pointer() -> Option<(f64, f64)> {
    std::env::var("INFYNITAE_EDGE_RENDER_POINTER")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once(',')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
}

fn render_with(cfg: Config, name: &str, size: &str, out: &str) -> Result<(), String> {
    let (w, h) = size
        .split_once('x')
        .and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?)))
        .filter(|&(w, h)| (200..=8192).contains(&w) && (200..=8192).contains(&h))
        .ok_or("--render needs WxH, e.g. 1920x1080")?;
    let mut sh = shell::Shell::new(cfg, app::load(&app::dirs()));
    if name == "ask" {
        // the question card (INFYNITAE_EDGE_RENDER_ASK: "title|choice|choice...")
        sh.ask.ask.load_render_env();
        return paint(&sh, w, h, out);
    }
    sh.windows = card::render_windows();
    sh.notes = card::render_notes();
    if let Some(t) = std::env::var("INFYNITAE_EDGE_RENDER_TIME")
        .ok()
        .and_then(|t| chrono::NaiveDateTime::parse_from_str(&t, "%Y-%m-%d %H:%M").ok())
    {
        sh.now = t;
    }
    for (prov, key) in sh.widget_sources() {
        if let Some(mut src) = provider::by_id(prov).and_then(|p| p.source(key)) {
            let data = src.fetch();
            sh.apply(prov, key, data.as_ref());
        }
    }
    if name == "widgets" {
        if sh.widgets.list.is_empty() {
            return Err("no WIDGETS line with a placed widget".into());
        }
        sh.widgets.pointer = render_pointer();
        return paint(&sh, w, h, out);
    }
    if name == "window-menu"
        || name == "window_menu"
        || std::env::var_os("INFYNITAE_EDGE_RENDER_WINDOW_MENU").is_some_and(|v| !v.is_empty())
    {
        let raw = std::env::var("INFYNITAE_EDGE_RENDER_WINDOW_MENU").unwrap_or_default();
        sh.open_window_menu(shell::WindowMenu::from_render_env(&raw));
        sh.pointer = render_pointer();
        return paint(&sh, w, h, out);
    }
    let idx = sh
        .panels
        .iter()
        .position(|p| p.panel.name == name)
        .ok_or(format!("no panel {name}"))?;
    sh.open_panel(idx, false, std::time::Instant::now());
    // drawn as it is once the cards have arrived
    sh.open_ms = f64::INFINITY;
    sh.opened_at = None;
    // the data, from the fixture through each card's sources
    for (prov, key) in sh.sources() {
        if let Some(mut src) = provider::by_id(prov).and_then(|p| p.source(key)) {
            let data = src.fetch();
            sh.apply(prov, key, data.as_ref());
        }
    }
    // a tray app's menu and card in the panel's place (INFYNITAE_EDGE_RENDER_TRAY: their words,
    // space separated, menu first), then their data
    if let Ok(words) = std::env::var("INFYNITAE_EDGE_RENDER_TRAY") {
        sh.show_cards(idx, words.split_whitespace().map(String::from).collect());
        sh.open_ms = f64::INFINITY;
        sh.opened_at = None;
        for (prov, key) in sh.sources() {
            if let Some(mut src) = provider::by_id(prov).and_then(|p| p.source(key)) {
                let data = src.fetch();
                sh.apply(prov, key, data.as_ref());
            }
        }
    }
    if let Ok(q) = std::env::var("INFYNITAE_EDGE_RENDER_SEARCH") {
        sh.type_text(&q);
    }
    if let Ok(sub) = std::env::var("INFYNITAE_EDGE_RENDER_SUB") {
        for pc in sh.panels[idx]
            .cards
            .iter_mut()
            .filter(|p| p.word.kind == sub)
        {
            pc.card.show_list();
        }
    }
    sh.pointer = render_pointer();
    let edit_env = std::env::var("INFYNITAE_EDGE_RENDER_EDIT").ok();
    let add_env = std::env::var("INFYNITAE_EDGE_RENDER_ADD").ok();
    let is_adding = add_env
        .as_deref()
        .is_some_and(|v| !v.is_empty() && v != "0");
    let is_editing = edit_env
        .as_deref()
        .is_some_and(|v| !v.is_empty() && v != "0")
        || is_adding;
    if is_editing {
        sh.enter_edit(w as f64, h as f64);
        if let Some(n) = edit_env
            .as_deref()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n > 0 && n <= sh.panels[idx].cards.len())
        {
            sh.edit.hover_card = Some(n - 1);
        }
        sh.edit.adding = is_adding;
    }
    paint(&sh, w, h, out)
}

/// Backdrop colour for `--render`: `INFYNITAE_EDGE_RENDER_BG` (`#rrggbb`) if set, otherwise the
/// theme's card colour adjusted by polarity so light themes are not pictured on a dark desktop.
fn render_backdrop(th: &theme::Theme, light: bool, env_bg: Option<&str>) -> theme::Rgba {
    if let Some(rgb) = env_bg.and_then(config::parse_hex) {
        return theme::Rgba::rgb(rgb);
    }
    let c = th.card;
    let is_light = light || c.alpha(1.0).luminance() > 0.35;
    let factor = if is_light { 0.88 } else { 0.65 };
    theme::Rgba(c.0 * factor, c.1 * factor, c.2 * factor, 1.0)
}

/// Draws what the shell shows on a `w x h` screen into the PNG file `out`.
fn paint(sh: &shell::Shell, w: i32, h: i32, out: &str) -> Result<(), String> {
    if let Ok(scratch) = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h) {
        if let Ok(cr) = cairo::Context::new(&scratch) {
            sh.draw_widgets(&cr);
            sh.draw(&cr, w as f64, h as f64);
        }
    }
    icons::wait_icon_idle();
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).map_err(|e| e.to_string())?;
    let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
    let bg = render_backdrop(
        &sh.theme,
        sh.cfg.light,
        std::env::var("INFYNITAE_EDGE_RENDER_BG").ok().as_deref(),
    );
    cr.set_source_rgb(bg.0, bg.1, bg.2);
    let _ = cr.paint();
    sh.draw_widgets(&cr);
    sh.draw(&cr, w as f64, h as f64);
    crate::boxes::dump(sh, w as f64, h as f64);
    drop(cr);
    surface.flush();
    let mut f = std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?;
    surface.write_to_png(&mut f).map_err(|e| e.to_string())
}

fn run(panel: Option<&str>) -> ExitCode {
    match wayland::run(panel) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("infynitae-edge-apps: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["--print-panels"] => {
            print!("{}", panel::print(&Config::load()));
            ExitCode::SUCCESS
        }
        ["--print-theme"] => {
            print!(
                "{}",
                theme::Theme::from_config(&Config::load()).shell_lines()
            );
            ExitCode::SUCCESS
        }
        ["--render", name, size, out] => match render(name, size, out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("infynitae-edge-apps: {e}");
                ExitCode::from(1)
            }
        },
        ["--crop-face", src, dst] => {
            match icons::write_face_png(std::path::Path::new(src), std::path::Path::new(dst)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("infynitae-edge-apps: {e}");
                    ExitCode::from(1)
                }
            }
        }
        // a question for the running edge (the keyring, the drive formatter, the share picker):
        // exit 0 answered, 1 cancelled, 2 wrong use, 3 no edge running
        ["ask", rest @ ..] => ExitCode::from(ask::run_client(rest)),
        ["stop"] => {
            wayland::stop_running();
            ExitCode::SUCCESS
        }
        [] => run(None),
        ["panel", name] | [name] if !name.starts_with('-') => {
            // the windows panel answers to its older names too
            let name = match *name {
                "toggle" | "center" | "open" => "windows",
                n => n,
            };
            if wayland::send_panel(name) {
                ExitCode::SUCCESS
            } else {
                run(Some(name))
            }
        }
        _ => {
            eprintln!("usage: infynitae-edge-apps [PANEL | ask ... | stop | --print-panels | --print-theme | --render PANEL|widgets WxH FILE.png]");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_backdrop_follows_theme_polarity_and_env_override() {
        let dark_cfg = Config::from_text("THEME_SCHEME=\"tonal\"\n");
        let dark_th = theme::Theme::from_config(&dark_cfg);
        let dark_bg = render_backdrop(&dark_th, dark_cfg.light, None);
        assert!(
            dark_bg.luminance() < 0.15,
            "dark lum={}",
            dark_bg.luminance()
        );

        let light_cfg =
            Config::from_text("COLOR_SCHEME=\"prefer-light\"\nTHEME_SCHEME=\"tonal\"\n");
        let light_th = theme::Theme::from_config(&light_cfg);
        let light_bg = render_backdrop(&light_th, light_cfg.light, None);
        assert!(
            light_bg.luminance() > 0.5,
            "light lum={}",
            light_bg.luminance()
        );

        let override_bg = render_backdrop(&light_th, light_cfg.light, Some("#123456"));
        assert_eq!(override_bg, theme::Rgba::rgb([0x12, 0x34, 0x56]));
    }

    #[test]
    fn render_widgets_draws_the_widgets_line_or_says_there_is_none() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-edge-apps-render-widgets-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("widgets.png");
        let png = png.to_str().unwrap();
        let cfg = |t: &str| Config::from_text(t);
        assert_eq!(
            render_with(cfg(""), "widgets", "1280x720", png),
            Err("no WIDGETS line with a placed widget".into())
        );
        let mut pixels = Vec::new();
        for line in [
            "WIDGETS=\"clock:big@60,60,380x150\"\n",
            "WIDGETS=\"clock:big@60,60,380x150 calendar@60,230,380x330\"\n",
        ] {
            render_with(cfg(line), "widgets", "1280x720", png).unwrap();
            let mut f = std::fs::File::open(png).unwrap();
            let mut img = cairo::ImageSurface::create_from_png(&mut f).unwrap();
            assert_eq!((img.width(), img.height()), (1280, 720));
            let stride = img.stride() as usize;
            let data = img.data().unwrap();
            // the pixel at (x, y) as its four bytes; the points are in the boxes' bottom padding
            let at = |x: usize, y: usize| data[y * stride + x * 4..y * stride + x * 4 + 4].to_vec();
            pixels.push((at(250, 205), at(250, 555), at(700, 555)));
        }
        let (one, two) = (&pixels[0], &pixels[1]);
        // the clock's box is drawn the same in both; the calendar's place only in the second; a
        // place without a widget is the backdrop in both
        assert_eq!(one.0, two.0);
        assert_eq!(one.1, one.2);
        assert_ne!(two.1, two.2);
        assert_ne!(one.0, one.2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_window_menu_writes_png() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-edge-apps-render-wm-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("window-menu.png");

        std::env::set_var(
            "INFYNITAE_EDGE_RENDER_WINDOW_MENU",
            "240,160,on_top,maximized",
        );
        render("window-menu", "1280x800", png.to_str().unwrap()).unwrap();
        std::env::remove_var("INFYNITAE_EDGE_RENDER_WINDOW_MENU");

        let meta = std::fs::metadata(&png).unwrap();
        assert!(meta.len() > 200);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
