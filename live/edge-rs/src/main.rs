//! infynitae-edge-rs: Infynitae's panels and desktop widgets, in Rust (docs/EDGE_RUST.md). Stage 0:
//! the config, the look and offline drawing; the Wayland side comes in stage 1.
//!
//!   infynitae-edge-rs --print-panels            each panel as its PANEL_ line (as the C edge prints)
//!   infynitae-edge-rs --render NAME WxH OUT.png  a panel open on a WxH screen, drawn to a PNG
//!
//! INFYNITAE_EDGE_RENDER_TIME="2026-11-09 17:23" fixes the clock for --render.

mod config;
mod draw;
mod modules;
mod theme;

use config::{Config, Panel, Side};
use std::process::ExitCode;

/// The width a panel takes on a screen w px wide (the C edge's panel_width).
fn panel_width(p: &Panel, w: f64) -> f64 {
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
fn panel_origin(p: &Panel, w: f64, h: f64, pw: f64, ph: f64) -> (f64, f64) {
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
fn layout(p: &Panel, pw: f64) -> (Vec<(f64, f64, f64, f64)>, f64) {
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
    let mut rects = Vec::new();
    for it in &p.items {
        let h = modules::height(it, pw);
        rects.push((0.0, y, pw, h));
        y += h + draw::GAP;
    }
    (rects, (y - draw::GAP).max(0.0))
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
    let pw = panel_width(panel, w as f64);
    let (rects, ph) = layout(panel, pw);
    let (ox, oy) = panel_origin(panel, w as f64, h as f64, pw, ph);
    let world = modules::World::now();
    for (it, r) in panel.items.iter().zip(rects) {
        if r.2 > 0.0 && r.3 > 0.0 {
            modules::draw(&cr, &th, it, &world, ox + r.0, oy + r.1, r.2, r.3);
        }
    }
    drop(cr);
    let mut f = std::fs::File::create(out).map_err(|e| e.to_string())?;
    surface.write_to_png(&mut f).map_err(|e| e.to_string())?;
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--print-panels") if args.len() == 2 => {
            for p in Config::load().panels {
                println!("{} {}", p.name, p.value());
            }
            ExitCode::SUCCESS
        }
        Some("--render") if args.len() == 5 => match render(&args[2], &args[3], &args[4]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[infynitae-edge-rs] {e}");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("usage: infynitae-edge-rs --print-panels | --render NAME WxH OUT.png");
            ExitCode::from(2)
        }
    }
}
