//! The edge from apps (docs/EDGE_APPS.md): every card in a panel comes from an app. Stage 1, part 1:
//! the model (apps, providers, cards, panels), the shared parts, the system cards, and the command
//! line's `--print-panels`, `--print-theme` and `--render` (pictures from fixed data, for tests).
//! The Wayland side comes next; until then `infynitae-edge-rs` stays the running edge.

// Input, actions and the timers have no caller until the Wayland side (stage 1, part 2) lands;
// this goes with it.
#![allow(dead_code)]

mod app;
mod boxes;
mod card;
mod cards;
mod config;
mod draw;
mod icons;
mod notify;
mod notify_server;
mod panel;
mod provider;
mod shell;
mod theme;
mod ui;
mod wayland;

use config::Config;
use std::process::ExitCode;

/// A still picture of panel `name` on a `WxH` screen, drawn by the same shell as the live edge,
/// from fixed data: `INFYNITAE_EDGE_RENDER_DATA` (the sources' fixture, `windows`, `notes`),
/// `_TIME`, `_POINTER`, `_SEARCH` (what was typed), `_SUB` (a card kind's list shown open).
fn render(name: &str, size: &str, out: &str) -> Result<(), String> {
    let (w, h) = size
        .split_once('x')
        .and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?)))
        .filter(|&(w, h)| (200..=8192).contains(&w) && (200..=8192).contains(&h))
        .ok_or("--render needs WxH, e.g. 1920x1080")?;
    let mut sh = shell::Shell::new(Config::load(), app::load(&app::dirs()));
    let idx = sh
        .panels
        .iter()
        .position(|p| p.panel.name == name)
        .ok_or(format!("no panel {name}"))?;
    sh.windows = card::render_windows();
    sh.notes = card::render_notes();
    if let Some(t) = std::env::var("INFYNITAE_EDGE_RENDER_TIME")
        .ok()
        .and_then(|t| chrono::NaiveDateTime::parse_from_str(&t, "%Y-%m-%d %H:%M").ok())
    {
        sh.now = t;
    }
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
    sh.pointer = std::env::var("INFYNITAE_EDGE_RENDER_POINTER")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once(',')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        });
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).map_err(|e| e.to_string())?;
    let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
    // a dark desktop behind (the picture is for looking at the panel, not the wallpaper)
    cr.set_source_rgb(
        0x0e as f64 / 255.0,
        0x0a as f64 / 255.0,
        0x0c as f64 / 255.0,
    );
    let _ = cr.paint();
    sh.draw(&cr, w as f64, h as f64);
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
        // the ask card comes in the next part of stage 1: until then "no edge running", so its
        // callers (the keyring, the drive formatter, the share picker) use their fallbacks
        ["ask", ..] => ExitCode::from(3),
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
            eprintln!("usage: infynitae-edge-apps [PANEL | stop | --print-panels | --print-theme | --render PANEL WxH FILE.png]");
            ExitCode::from(2)
        }
    }
}
