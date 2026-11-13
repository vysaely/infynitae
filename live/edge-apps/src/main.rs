//! The edge from apps (docs/EDGE_APPS.md): every card in a panel comes from an app. Stage 1, part 1:
//! the model (apps, providers, cards, panels), the shared parts, the system cards, and the command
//! line's `--print-panels`, `--print-theme` and `--render` (pictures from fixed data, for tests).
//! The Wayland side comes next; until then `infynitae-edge-rs` stays the running edge.

// Input, actions and the timers have no caller until the Wayland side (stage 1, part 2) lands;
// this goes with it.
#![allow(dead_code)]

mod app;
mod card;
mod cards;
mod config;
mod draw;
mod panel;
mod provider;
mod theme;
mod ui;

use card::{Card, Cx, Hits, Status};
use config::{Config, Panel, Side};
use draw::{Text, PAD_X};
use provider::Size;
use std::process::ExitCode;

/// From the screen's edge to a panel.
pub const PANEL_MARGIN: f64 = 24.0;
const PAD_TOP: f64 = 14.0;
const PAD_BOTTOM: f64 = 12.0;
const HEAD_H: f64 = 26.0;
const GAP: f64 = 12.0;

fn panel_width(p: &Panel, w: f64) -> f64 {
    let pw = if p.width > 0 { p.width as f64 } else { 380.0 };
    pw.min(w - 2.0 * PANEL_MARGIN)
}

fn panel_origin(p: &Panel, w: f64, h: f64, pw: f64, ph: f64) -> (f64, f64) {
    const M: f64 = PANEL_MARGIN;
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

/// One placed card, made from its word, with its provider's name for the header.
struct Placed {
    word: panel::CardRef,
    card: Box<dyn Card>,
    kind: &'static provider::CardKind,
    from: String,
    size: Size,
}

fn make_cards(p: &Panel, apps: &[app::App]) -> Vec<Placed> {
    let mut out = Vec::new();
    for w in panel::cards(p) {
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
        let from = app.map_or(prov.name().to_string(), |a| a.name.clone());
        out.push(Placed {
            word: w,
            card,
            kind,
            from,
            size,
        });
    }
    out
}

/// The card's header: its title at the left, the app it comes from at the right (the core's part).
fn draw_header(
    cr: &cairo::Context,
    th: &theme::Theme,
    x: f64,
    y: f64,
    w: f64,
    title: &str,
    from: &str,
) {
    let from_w = draw::text_right(
        cr,
        x + w - PAD_X,
        y + PAD_TOP + 3.0,
        &Text {
            size: 12.0,
            bold: false,
            color: th.muted,
            text: from,
        },
    );
    draw::text_max(
        cr,
        x + PAD_X,
        y + PAD_TOP,
        (w - 2.0 * PAD_X - from_w - 12.0).max(40.0),
        &Text {
            size: 15.0,
            bold: true,
            color: th.text,
            text: title,
        },
    );
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
    let apps = app::load(&app::dirs());
    let mut placed = make_cards(panel, &apps);
    // the data, from the fixture (INFYNITAE_EDGE_RENDER_DATA) through each card's sources
    for pl in &mut placed {
        let Some((prov, _)) = provider::resolve(&pl.word.owner, &apps) else {
            continue;
        };
        for key in pl.kind.sources {
            if let Some(mut src) = prov.source(key) {
                let data = src.fetch();
                pl.card.apply(key, data.as_ref());
            }
        }
    }
    // a list shown open (INFYNITAE_EDGE_RENDER_SUB names a card kind)
    if let Ok(sub) = std::env::var("INFYNITAE_EDGE_RENDER_SUB") {
        for pl in placed.iter_mut().filter(|p| p.word.kind == sub) {
            pl.card.show_list();
        }
    }
    let now = std::env::var("INFYNITAE_EDGE_RENDER_TIME")
        .ok()
        .and_then(|t| chrono::NaiveDateTime::parse_from_str(&t, "%Y-%m-%d %H:%M").ok())
        .unwrap_or_else(|| chrono::Local::now().naive_local());
    let pointer = std::env::var("INFYNITAE_EDGE_RENDER_POINTER")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once(',')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        });
    let list_above = panel.side == Side::Bottom;
    let pw = panel_width(panel, w as f64);
    let cx_for = |s: Size| Cx {
        theme: &th,
        now,
        size: s,
        app: None,
        apps: &apps,
        windows: &[],
        notes: &[],
        list_above,
        editing: false,
    };
    // heights first: the panel sits at its edge by its whole height
    let heights: Vec<f64> = placed
        .iter()
        .map(|pl| {
            let cx = cx_for(pl.size);
            match pl.card.status(&cx) {
                Status::Hidden => 0.0,
                Status::Ready => {
                    PAD_TOP + HEAD_H + pl.card.height(&cx, pw - 2.0 * (PAD_X - 8.0)) + PAD_BOTTOM
                }
                _ => PAD_TOP + HEAD_H + 20.0 + PAD_BOTTOM,
            }
        })
        .collect();
    let shown = heights.iter().filter(|h| **h > 0.0).count();
    let ph = heights.iter().sum::<f64>() + GAP * shown.saturating_sub(1) as f64;
    let (ox, oy) = panel_origin(panel, w as f64, h as f64, pw, ph);

    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).map_err(|e| e.to_string())?;
    let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
    cr.set_source_rgb(
        0x0e as f64 / 255.0,
        0x0a as f64 / 255.0,
        0x0c as f64 / 255.0,
    );
    let _ = cr.paint();
    let mut y = oy;
    for (pl, ch) in placed.iter().zip(&heights) {
        if *ch <= 0.0 {
            continue;
        }
        let cx = cx_for(pl.size);
        draw::card(&cr, &th, ox, y, pw, *ch);
        let title = pl
            .card
            .title(&cx)
            .unwrap_or_else(|| pl.kind.title.to_string());
        draw_header(&cr, &th, ox, y, pw, &title, &pl.from);
        let body = (
            ox + PAD_X - 8.0,
            y + PAD_TOP + HEAD_H,
            pw - 2.0 * (PAD_X - 8.0),
            ch - PAD_TOP - HEAD_H - PAD_BOTTOM,
        );
        match pl.card.status(&cx) {
            Status::Ready => {
                let mut hits = Hits::new(pointer, None);
                pl.card.draw(&cr, &cx, body, &mut hits);
            }
            Status::Loading => {
                draw::text(
                    &cr,
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
                    &cr,
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
        y += ch + GAP;
    }
    surface.flush();
    let mut f = std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?;
    surface.write_to_png(&mut f).map_err(|e| e.to_string())
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
        _ => {
            eprintln!("usage: infynitae-edge-apps --print-panels | --print-theme | --render PANEL WxH FILE.png");
            ExitCode::from(2)
        }
    }
}
