//! Simple card (docs/EDGE_APPS.md section 6), the fallback provider for any installed app no
//! other provider claims, in the look of `docs/design/apps-edge/app-cards.html`. Reads `cx.app`,
//! `cx.windows` and `cx.notes`; no source of its own.

use crate::app::App;
use crate::card::{Action, Button, Card, Cx, Event, Hits, Note, Rect, Source, Status, Window};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::ui;
use cairo::Context;
use std::any::Any;

pub struct Simple;

const KINDS: &[CardKind] = &[CardKind {
    id: "app",
    title: "App",
    about: "Open the app, its windows and notifications",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: true,
    sources: &[],
}];

// Last in `provider::providers()`: `Claim::Categories(&[])` matches only in the category pass
// after every `Id`, `IdPrefix` and earlier `Categories` claim has been checked, so `provider::owner_of`
// falls back to `Simple` without changing `provider.rs`.
const CLAIMS: &[Claim] = &[Claim::Categories(&[])];

impl Provider for Simple {
    fn id(&self) -> &'static str {
        "simple"
    }
    fn name(&self) -> &'static str {
        "Simple card"
    }
    fn claims(&self) -> &'static [Claim] {
        CLAIMS
    }
    fn listed(&self) -> Listed {
        Listed::Never
    }
    fn cards(&self) -> &'static [CardKind] {
        KINDS
    }
    fn card(&self, kind: &str, _argument: Option<&str>) -> Option<Box<dyn Card>> {
        match kind {
            "app" => Some(Box::new(SimpleCard)),
            _ => None,
        }
    }
    fn source(&self, _key: &str) -> Option<Box<dyn Source>> {
        None
    }
}

#[cfg(test)]
/// The provider that owns `app`, falling back to `Simple` when no claim matched.
pub fn owner_of(app: &App) -> &'static dyn Provider {
    crate::provider::owner_of(app).unwrap_or(&Simple)
}

/// Checks whether a desktop `app` matches a window's `app_id` (by entry id without `.desktop`,
/// `StartupWMClass`, app name, or binary name from `Exec`, case-insensitive; from
/// `live/edge-rs/src/modules/apps.rs:548`).
pub fn app_matches_window_id(app: &App, window_app_id: &str) -> bool {
    let w_stem = window_app_id
        .trim()
        .strip_suffix(".desktop")
        .unwrap_or(window_app_id.trim());
    if w_stem.is_empty() {
        return false;
    }
    let w_last = w_stem.rsplit('.').next().unwrap_or(w_stem);
    let matches_token = |cand: &str| {
        let stem = cand.strip_suffix(".desktop").unwrap_or(cand);
        let last = stem.rsplit('.').next().unwrap_or(stem);
        (!stem.is_empty() && w_stem.eq_ignore_ascii_case(stem))
            || (!last.is_empty() && !w_last.is_empty() && w_last.eq_ignore_ascii_case(last))
    };

    if matches_token(&app.id) {
        return true;
    }
    if app.wm_class.as_deref().is_some_and(matches_token) {
        return true;
    }
    if !app.name.is_empty()
        && (w_stem.eq_ignore_ascii_case(&app.name) || w_last.eq_ignore_ascii_case(&app.name))
    {
        return true;
    }
    let cmd_bin = app
        .exec
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    !cmd_bin.is_empty()
        && (w_stem.eq_ignore_ascii_case(cmd_bin) || w_last.eq_ignore_ascii_case(cmd_bin))
}

/// Checks whether a notification belongs to `app` (by `app.id` or `app.name`, case-insensitive).
pub fn note_matches_app(note: &Note, app: &App) -> bool {
    let n = note.app.trim();
    if n.is_empty() {
        return false;
    }
    let n_stem = n.strip_suffix(".desktop").unwrap_or(n);
    let app_stem = app.id.strip_suffix(".desktop").unwrap_or(&app.id);
    (!app_stem.is_empty() && n_stem.eq_ignore_ascii_case(app_stem))
        || (!app.name.is_empty() && n.eq_ignore_ascii_case(&app.name))
}

const MAX_NOTES: usize = 3;
const ICON_SZ: f64 = 28.0;
const SUB_H: f64 = 24.0;
const ITEM_H: f64 = ui::LIST_ROW_H;

const H_APP: u32 = 0;
// Up to 3 notifications use hits 10..16 (row = +2*i, dismiss x = +2*i + 1).
const H_NOTE_BASE: u32 = 10;
// Windows start at 100 (row = +2*i, close x = +2*i + 1) so they never collide with notes.
const H_WIN_BASE: u32 = 100;

struct NoteGroup<'a> {
    heading: &'a str,
    items: Vec<&'a Note>,
}

fn app_windows<'a>(cx: &'a Cx, app: &App) -> Vec<&'a Window> {
    cx.windows
        .iter()
        .filter(|w| app_matches_window_id(app, &w.app_id))
        .collect()
}

fn note_groups<'a>(cx: &'a Cx, app: &App) -> Vec<NoteGroup<'a>> {
    let mut groups: Vec<NoteGroup<'a>> = Vec::new();
    for note in cx
        .notes
        .iter()
        .filter(|n| note_matches_app(n, app))
        .take(MAX_NOTES)
    {
        let heading = if !note.summary.trim().is_empty() && !note.body.trim().is_empty() {
            note.summary.as_str()
        } else {
            "Notifications"
        };
        if let Some(g) = groups.iter_mut().find(|g| g.heading == heading) {
            g.items.push(note);
        } else {
            groups.push(NoteGroup {
                heading,
                items: vec![note],
            });
        }
    }
    groups
}

fn draw_app_tile(cr: &Context, cx: &Cx, app: &App, x: f64, y: f64) {
    crate::icons::draw_app_icon(cr, cx, &app.icon, &app.name, x, y, ICON_SZ);
}

fn draw_sub(cr: &Context, cx: &Cx, x: f64, y: f64, w: f64, label: &str) {
    draw::text_max(
        cr,
        x + 8.0,
        y + 8.0,
        (w - 16.0).max(20.0),
        &Text {
            size: 11.0,
            bold: false,
            color: cx.theme.muted,
            text: label,
        },
    );
}

fn draw_item_row(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    ids: (u32, u32),
    r: Rect,
    label: &str,
    right_note: &str,
) {
    let (row_id, x_id) = ids;
    let xr = (r.0 + r.2 - 32.0, r.1 + (r.3 - 24.0) / 2.0, 24.0, 24.0);
    let row_lit = hits.add(row_id, r) || hits.selected(row_id);
    let x_lit = hits.add(x_id, xr) || hits.selected(x_id);
    if row_lit {
        ui::light(cr, cx, r);
    }
    let th = cx.theme;
    let right_w = if row_lit || x_lit {
        if x_lit {
            ui::light(cr, cx, xr);
        }
        draw::text_center(
            cr,
            xr.0 + xr.2 / 2.0,
            xr.1 + 4.0,
            &Text {
                size: 12.0,
                bold: false,
                color: if x_lit { th.text } else { th.muted },
                text: "x",
            },
        );
        xr.2 + 8.0
    } else if !right_note.is_empty() {
        draw::text_right(
            cr,
            r.0 + r.2 - 8.0,
            r.1 + (r.3 - 15.0) / 2.0,
            &Text {
                size: draw::SIZE_NOTE,
                bold: false,
                color: th.muted,
                text: right_note,
            },
        ) + 8.0
    } else {
        0.0
    };
    draw::text_max(
        cr,
        r.0 + 8.0,
        r.1 + (r.3 - 17.0) / 2.0,
        (r.2 - 16.0 - right_w).max(20.0),
        &Text {
            size: 13.0,
            bold: false,
            color: th.text,
            text: label,
        },
    );
}

#[derive(Default)]
pub struct SimpleCard;

impl Card for SimpleCard {
    fn apply(&mut self, _key: &str, _data: &dyn Any) -> bool {
        false
    }

    fn status(&self, cx: &Cx) -> Status {
        if cx.app.is_some() {
            Status::Ready
        } else {
            Status::Missing("No app for this card".into())
        }
    }

    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        let Some(app) = cx.app else {
            return ui::ROW_H;
        };
        let mut h = ui::ROW_H;
        if cx.size == Size::M {
            let wins = app_windows(cx, app);
            if !wins.is_empty() {
                h += SUB_H + wins.len() as f64 * ITEM_H;
            }
            for g in note_groups(cx, app) {
                h += SUB_H + g.items.len() as f64 * ITEM_H;
            }
        }
        h
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(app) = cx.app else { return };
        let wins = app_windows(cx, app);
        let th = cx.theme;

        let mut y = body.1;
        let first_r = (body.0, y, body.2, ui::ROW_H);
        let lit = hits.add(H_APP, first_r) || hits.selected(H_APP);
        if lit {
            ui::light(cr, cx, first_r);
        }
        let ix = first_r.0 + 8.0;
        let iy = first_r.1 + (first_r.3 - ICON_SZ) / 2.0;
        draw_app_tile(cr, cx, app, ix, iy);

        let right_w = if lit {
            draw::text_right(
                cr,
                first_r.0 + first_r.2 - 8.0,
                first_r.1 + (first_r.3 - 16.0) / 2.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: if wins.is_empty() {
                        "Open"
                    } else {
                        "Go to window"
                    },
                },
            ) + 8.0
        } else {
            0.0
        };

        let tx = ix + ICON_SZ + 10.0;
        let tw = (first_r.0 + first_r.2 - 8.0 - right_w - tx).max(20.0);
        let ty = first_r.1 + (first_r.3 - 32.0) / 2.0;
        draw::text_max(
            cr,
            tx,
            ty,
            tw,
            &Text {
                size: 13.0,
                bold: false,
                color: th.text,
                text: &app.name,
            },
        );
        let state = match wins.len() {
            0 => "Not running".to_string(),
            1 => "1 window".to_string(),
            n => format!("{n} windows"),
        };
        draw::text_max(
            cr,
            tx,
            ty + 17.0,
            tw,
            &Text {
                size: 11.0,
                bold: false,
                color: th.muted,
                text: &state,
            },
        );
        y += ui::ROW_H;

        if cx.size != Size::M {
            return;
        }
        if !wins.is_empty() {
            draw_sub(cr, cx, body.0, y, body.2, "Windows");
            y += SUB_H;
            for (i, w) in wins.iter().enumerate() {
                let label = if w.title.is_empty() {
                    &w.app_id
                } else {
                    &w.title
                };
                let id = H_WIN_BASE + i as u32 * 2;
                draw_item_row(
                    cr,
                    cx,
                    hits,
                    (id, id + 1),
                    (body.0, y, body.2, ITEM_H),
                    label,
                    "",
                );
                y += ITEM_H;
            }
        }
        let mut note_idx = 0u32;
        for g in note_groups(cx, app) {
            draw_sub(cr, cx, body.0, y, body.2, g.heading);
            y += SUB_H;
            for n in g.items {
                let label = if !n.body.trim().is_empty() {
                    n.body.as_str()
                } else if !n.summary.trim().is_empty() {
                    n.summary.as_str()
                } else {
                    "Notification"
                };
                let id = H_NOTE_BASE + note_idx * 2;
                draw_item_row(
                    cr,
                    cx,
                    hits,
                    (id, id + 1),
                    (body.0, y, body.2, ITEM_H),
                    label,
                    &n.time,
                );
                note_idx += 1;
                y += ITEM_H;
            }
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Some(app) = cx.app else { return false };
        let Event::Press {
            hit,
            button: Button::Left,
            ..
        } = ev
        else {
            return false;
        };
        if *hit == H_APP {
            let wins = app_windows(cx, app);
            if let Some(newest) = wins.iter().max_by_key(|w| w.id) {
                out.push(Action::Focus(newest.id));
            } else {
                out.push(Action::Launch(app.id.clone()));
            }
            return true;
        }
        if cx.size != Size::M {
            return false;
        }
        if *hit >= H_WIN_BASE {
            let off = *hit - H_WIN_BASE;
            let wins = app_windows(cx, app);
            let Some(w) = wins.get((off / 2) as usize) else {
                return false;
            };
            if off % 2 == 1 {
                out.push(Action::CloseWindow(w.id));
            } else {
                out.push(Action::Focus(w.id));
            }
            return true;
        }
        if *hit >= H_NOTE_BASE {
            let off = *hit - H_NOTE_BASE;
            let notes: Vec<&Note> = note_groups(cx, app)
                .into_iter()
                .flat_map(|g| g.items)
                .collect();
            let Some(n) = notes.get((off / 2) as usize) else {
                return false;
            };
            if off % 2 == 1 {
                out.push(Action::Dismiss(n.id));
            } else {
                out.push(Action::Invoke(n.id, "default".to_string()));
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app;
    use crate::config::Config;
    use crate::provider;
    use crate::shell::Shell;
    use crate::theme::Theme;

    fn make_app(id: &str, name: &str, cats: &[&str]) -> App {
        App {
            id: id.into(),
            name: name.into(),
            icon: id.into(),
            exec: id.into(),
            categories: cats.iter().map(|s| s.to_string()).collect(),
            wm_class: None,
            terminal: false,
        }
    }

    fn make_win(id: u64, app_id: &str, title: &str, focused: bool) -> Window {
        Window {
            id,
            title: title.into(),
            app_id: app_id.into(),
            focused,
        }
    }

    fn make_note(id: u32, app: &str, summary: &str, body: &str, time: &str) -> Note {
        Note {
            id,
            app: app.into(),
            summary: summary.into(),
            body: body.into(),
            actions: vec![("default".into(), "Open".into())],
            urgency: 1,
            time: time.into(),
        }
    }

    fn cx<'a>(
        th: &'a Theme,
        size: Size,
        app: Option<&'a App>,
        apps: &'a [App],
        windows: &'a [Window],
        notes: &'a [Note],
    ) -> Cx<'a> {
        Cx {
            theme: th,
            now: chrono::NaiveDate::from_ymd_opt(2026, 11, 13)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            size,
            app,
            apps,
            windows,
            notes,
            list_above: false,
            editing: false,
        }
    }

    #[test]
    fn provider_metadata_and_fallback_claiming() {
        let provs = provider::providers();
        assert_eq!(provs.last().unwrap().id(), "simple");
        assert_eq!(Simple.name(), "Simple card");
        assert_eq!(Simple.listed(), Listed::Never);
        assert!(Simple.source("any").is_none());
        assert!(Simple.card("unknown", None).is_none());
        let kind = Simple.kind("app").unwrap();
        assert_eq!(kind.sizes, &[Size::S, Size::M]);
        assert!(kind.sources.is_empty());

        let firefox = make_app("firefox", "Firefox", &["Network", "WebBrowser"]);
        let bare = make_app("custom-tool", "Custom Tool", &[]);
        assert_eq!(provider::owner_of(&firefox).unwrap().id(), "simple");
        assert_eq!(provider::owner_of(&bare).unwrap().id(), "simple");
        assert_eq!(owner_of(&bare).id(), "simple");

        let apps = [firefox.clone(), bare];
        let (p, a) = provider::resolve("firefox", &apps).unwrap();
        assert_eq!(p.id(), "simple");
        assert_eq!(a.unwrap().name, "Firefox");
        assert!(provider::resolve("missing", &apps).is_none());
    }

    #[test]
    fn loads_desktop_entries_from_temp_folder_usual_empty_broken() {
        let base = std::env::temp_dir().join(format!("edge-apps-simple-{}", std::process::id()));
        let empty = base.join("empty");
        let usual = base.join("usual");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&usual).unwrap();
        std::fs::write(
            usual.join("firefox.desktop"),
            "[Desktop Entry]\nName=Firefox\nExec=firefox %u\nIcon=firefox\nCategories=Network;WebBrowser;\n",
        )
        .unwrap();
        std::fs::write(
            usual.join("kitty.desktop"),
            "[Desktop Entry]\nName=kitty\nExec=kitty\nIcon=kitty\nCategories=System;TerminalEmulator;\n",
        )
        .unwrap();
        std::fs::write(
            usual.join("broken.desktop"),
            "[Desktop Entry]\nName=Broken\n",
        )
        .unwrap();
        std::fs::write(
            usual.join("hidden.desktop"),
            "[Desktop Entry]\nName=Hidden\nExec=hidden\nNoDisplay=true\n",
        )
        .unwrap();

        assert!(app::load(&[empty]).is_empty());
        let loaded = app::load(&[usual]);
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!(
            loaded.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            vec!["firefox", "kitty"]
        );
        for a in &loaded {
            assert_eq!(provider::owner_of(a).unwrap().id(), "simple");
        }
    }

    #[test]
    fn matches_windows_and_notifications() {
        let mut qb = make_app("org.qbittorrent.qBittorrent", "qBittorrent", &[]);
        qb.exec = "/usr/bin/qbittorrent %U".into();
        qb.wm_class = Some("qBittorrent".into());

        assert!(app_matches_window_id(&qb, "org.qbittorrent.qBittorrent"));
        assert!(app_matches_window_id(&qb, "qbittorrent.desktop"));
        assert!(app_matches_window_id(&qb, "QBITTORRENT"));
        assert!(!app_matches_window_id(&qb, ""));
        assert!(!app_matches_window_id(&qb, "firefox"));

        let mut custom = make_app("my-browser", "Firefox", &[]);
        custom.exec = "/opt/ff/firefox-bin --new-window".into();
        assert!(app_matches_window_id(&custom, "firefox-bin"));
        assert!(app_matches_window_id(&custom, "Firefox"));

        assert!(note_matches_app(
            &make_note(1, "org.qbittorrent.qBittorrent.desktop", "s", "b", "12:00"),
            &qb
        ));
        assert!(note_matches_app(
            &make_note(2, "QBITTORRENT", "s", "b", "12:00"),
            &qb
        ));
        assert!(!note_matches_app(&make_note(3, "", "s", "b", "12:00"), &qb));
        assert!(!note_matches_app(
            &make_note(4, "kitty", "s", "b", "12:00"),
            &qb
        ));
    }

    #[test]
    fn apply_and_status() {
        let th = Theme::from_config(&Config::from_text(""));
        let mut card = SimpleCard;
        assert!(!card.apply("anything", &()));

        let without_app = cx(&th, Size::S, None, &[], &[], &[]);
        assert!(matches!(card.status(&without_app), Status::Missing(_)));

        let firefox = make_app("firefox", "Firefox", &[]);
        let with_app = cx(&th, Size::S, Some(&firefox), &[], &[], &[]);
        assert_eq!(card.status(&with_app), Status::Ready);
    }

    #[test]
    fn first_row_focuses_newest_window_or_launches_when_not_running() {
        let th = Theme::from_config(&Config::from_text(""));
        let firefox = make_app("firefox", "Firefox", &[]);
        let mut card = SimpleCard;
        let mut out = Vec::new();

        // Not running: Launch
        let idle = cx(&th, Size::S, Some(&firefox), &[], &[], &[]);
        assert!(card.event(
            &Event::Press {
                hit: H_APP,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &idle,
            &mut out,
        ));
        assert_eq!(out, vec![Action::Launch("firefox".into())]);

        // Two windows: newest (highest id) is focused
        out.clear();
        let wins = [
            make_win(10, "firefox", "First tab", false),
            make_win(25, "firefox", "Second tab", true),
            make_win(30, "kitty", "Other app", false),
        ];
        let running = cx(&th, Size::S, Some(&firefox), &[], &wins, &[]);
        assert!(card.event(
            &Event::Press {
                hit: H_APP,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &running,
            &mut out,
        ));
        assert_eq!(out, vec![Action::Focus(25)]);

        // Size S ignores window/notification hits, right-click, and missing app
        out.clear();
        assert!(!card.event(
            &Event::Press {
                hit: H_WIN_BASE,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &running,
            &mut out,
        ));
        assert!(!card.event(
            &Event::Press {
                hit: H_APP,
                button: Button::Right,
                at: (10.0, 10.0),
            },
            &running,
            &mut out,
        ));
        let missing = cx(&th, Size::M, None, &[], &wins, &[]);
        assert!(!card.event(
            &Event::Press {
                hit: H_APP,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &missing,
            &mut out,
        ));
        assert!(out.is_empty());
    }

    #[test]
    fn medium_size_clicks_windows_and_grouped_notifications() {
        let th = Theme::from_config(&Config::from_text(""));
        let firefox = make_app("firefox", "Firefox", &[]);
        let wins = [
            make_win(10, "firefox", "Infynitae on GitHub", true),
            make_win(20, "firefox", "Second window", false),
        ];
        // Newest-first, interleaved summaries; 4th matching note must be omitted.
        let notes = [
            make_note(1, "firefox", "Alice", "Newest from Alice", "12:05"),
            make_note(99, "kitty", "Other", "Ignored", "12:04"),
            make_note(2, "Firefox", "Bob", "From Bob", "12:03"),
            make_note(3, "firefox", "Alice", "Older from Alice", "12:02"),
            make_note(4, "firefox", "Alice", "Fourth dropped", "12:01"),
        ];
        let c = cx(&th, Size::M, Some(&firefox), &[], &wins, &notes);
        let mut card = SimpleCard;
        let mut out = Vec::new();

        // Grouped visual order of notes: Alice (1, 3), then Bob (2).
        for (hit, expected) in [
            (H_APP, Action::Focus(20)),
            (H_WIN_BASE, Action::Focus(10)),
            (H_WIN_BASE + 1, Action::CloseWindow(10)),
            (H_WIN_BASE + 2, Action::Focus(20)),
            (H_WIN_BASE + 3, Action::CloseWindow(20)),
            (H_NOTE_BASE, Action::Invoke(1, "default".into())),
            (H_NOTE_BASE + 1, Action::Dismiss(1)),
            (H_NOTE_BASE + 2, Action::Invoke(3, "default".into())),
            (H_NOTE_BASE + 3, Action::Dismiss(3)),
            (H_NOTE_BASE + 4, Action::Invoke(2, "default".into())),
            (H_NOTE_BASE + 5, Action::Dismiss(2)),
        ] {
            out.clear();
            assert!(
                card.event(
                    &Event::Press {
                        hit,
                        button: Button::Left,
                        at: (10.0, 10.0),
                    },
                    &c,
                    &mut out,
                ),
                "hit {hit}"
            );
            assert_eq!(out, vec![expected], "hit {hit}");
        }

        // 4th note (index 3 -> H_NOTE_BASE + 6) does not exist
        out.clear();
        assert!(!card.event(
            &Event::Press {
                hit: H_NOTE_BASE + 6,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &c,
            &mut out,
        ));
        assert!(out.is_empty());
    }

    #[test]
    fn height_matches_drawn_layout_and_hits() {
        let th = Theme::from_config(&Config::from_text(""));
        let firefox = make_app("firefox", "Firefox", &[]);
        let wins = [
            make_win(1, "firefox", "Tab one", true),
            make_win(2, "firefox", "Tab two", false),
        ];
        let notes = [
            make_note(1, "firefox", "Alice", "Hi", "12:05"),
            make_note(2, "firefox", "Bob", "Hey", "12:04"),
            make_note(3, "firefox", "Download done", "", "12:03"),
            make_note(4, "firefox", "Dropped", "Ignored", "12:02"),
        ];
        let card = SimpleCard;
        let w = 360.0;

        let cx_s = cx(&th, Size::S, Some(&firefox), &[], &wins, &notes);
        assert_eq!(card.height(&cx_s, w), ui::ROW_H);

        let cx_empty = cx(&th, Size::M, Some(&firefox), &[], &[], &[]);
        assert_eq!(card.height(&cx_empty, w), ui::ROW_H);

        let cx_m = cx(&th, Size::M, Some(&firefox), &[], &wins, &notes);
        // Main row + "Windows" (2 wins) + "Alice" (1) + "Bob" (1) + "Notifications" (1)
        let want_h = ui::ROW_H + (SUB_H + 2.0 * ITEM_H) + 3.0 * (SUB_H + ITEM_H);
        assert_eq!(card.height(&cx_m, w), want_h);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 500).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(Some((w - 15.0, ui::ROW_H + SUB_H + 10.0)), None);
        card.draw(&cr, &cx_m, (0.0, 0.0, w, want_h), &mut hits);

        // Every hit sits inside (0, 0, w, want_h) and the bottom-most hit reaches want_h.
        let max_y = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        assert_eq!(max_y, want_h);
        assert!(hits
            .list
            .iter()
            .all(|(_, r)| r.0 >= 0.0 && r.1 >= 0.0 && r.0 + r.2 <= w));

        // The close/dismiss x sits on top of its row hit.
        let win0_x = hits
            .list
            .iter()
            .find(|(id, _)| *id == H_WIN_BASE + 1)
            .unwrap()
            .1;
        assert_eq!(
            hits.top_at((win0_x.0 + 5.0, win0_x.1 + 5.0)),
            Some(H_WIN_BASE + 1)
        );
        assert_eq!(hits.top_at((20.0, win0_x.1 + 5.0)), Some(H_WIN_BASE));

        let note0_x = hits
            .list
            .iter()
            .find(|(id, _)| *id == H_NOTE_BASE + 1)
            .unwrap()
            .1;
        assert_eq!(
            hits.top_at((note0_x.0 + 5.0, note0_x.1 + 5.0)),
            Some(H_NOTE_BASE + 1)
        );
        assert_eq!(hits.top_at((20.0, note0_x.1 + 5.0)), Some(H_NOTE_BASE));

        // Exercise hover state on H_APP as well
        let mut app_hover = Hits::new(Some((20.0, 20.0)), None);
        card.draw(&cr, &cx_s, (0.0, 0.0, w, ui::ROW_H), &mut app_hover);
        assert_eq!(app_hover.top_at((20.0, 20.0)), Some(H_APP));
    }

    #[test]
    fn shell_places_draws_and_clicks_simple_card() {
        let cfg = Config::from_text(
            "PANEL_CONTROL=\"SUPER+A top right 380 firefox/app:m kitty/app:s\"\n",
        );
        let apps = vec![
            make_app("firefox", "Firefox", &["Network", "WebBrowser"]),
            make_app("kitty", "kitty", &["System", "TerminalEmulator"]),
        ];
        let mut sh = Shell::new(cfg, apps);
        sh.windows = vec![make_win(7, "firefox", "Home", false)];
        sh.notes = vec![make_note(
            5,
            "firefox",
            "Downloads",
            "Saved file.tar.gz",
            "12:10",
        )];
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert_eq!(sh.panels[idx].cards.len(), 2);
        assert_eq!(sh.panels[idx].cards[0].from, "Firefox");
        assert_eq!(sh.panels[idx].cards[1].from, "kitty");

        assert!(sh.open_panel(idx, false, std::time::Instant::now()));
        sh.open_ms = f64::INFINITY;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);

        let (ff_close, kitty_main) = {
            let f = sh.frame.borrow();
            let ff_close = f[0]
                .2
                .list
                .iter()
                .find(|(id, _)| *id == H_WIN_BASE + 1)
                .unwrap()
                .1;
            let kitty_main = f[1].2.list.iter().find(|(id, _)| *id == H_APP).unwrap().1;
            (ff_close, kitty_main)
        };
        assert!(sh.click(
            1280.0,
            800.0,
            ff_close.0 + 4.0,
            ff_close.1 + 4.0,
            Button::Left
        ));
        assert!(sh.click(
            1280.0,
            800.0,
            kitty_main.0 + 4.0,
            kitty_main.1 + 4.0,
            Button::Left
        ));
        assert_eq!(
            sh.actions
                .iter()
                .map(|(a, _, _, _)| a.clone())
                .collect::<Vec<_>>(),
            vec![Action::CloseWindow(7), Action::Launch("kitty".into())]
        );
    }
}
