//! Discord and Telegram (docs/EDGE_APPS.md section 6, docs/design/apps-edge/app-cards.html):
//! notifications by sender or channel (`cx.notes`) and open windows (`cx.windows`). No source of
//! its own; neither app gives an unread count, so none is shown.

use crate::app::App;
use crate::card::{Action, Button, Card, Cx, Event, Hits, Note, Rect, Source, Status, Window};
use crate::draw::{self, Text};
use crate::provider::{self, CardKind, Claim, Listed, Provider, Size};
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;

pub struct Chat;

const CLAIMS: &[Claim] = &[
    Claim::Id("discord"),
    Claim::Id("com.discordapp.Discord"),
    Claim::IdPrefix("org.telegram.desktop"),
];

const KINDS: &[CardKind] = &[CardKind {
    id: "messages",
    title: "Messages",
    about: "Recent notifications by sender and open windows",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &[],
}];

impl Provider for Chat {
    fn id(&self) -> &'static str {
        "chat"
    }
    fn name(&self) -> &'static str {
        "Chat"
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
    fn card(&self, kind: &str, argument: Option<&str>) -> Option<Box<dyn Card>> {
        match (kind, argument) {
            ("messages", None) => Some(Box::new(MessagesCard)),
            _ => None,
        }
    }
    fn source(&self, _key: &str) -> Option<Box<dyn Source>> {
        None
    }
}

const MAX_S: usize = 3;
const MAX_M: usize = 8;
const SUB_H: f64 = 18.0;
const GROUP_GAP: f64 = 6.0;
const ITEM_H: f64 = 36.0;
const X_BTN: f64 = 24.0;

const H_OPEN: u32 = 0;
const H_NOTE: u32 = 10;
const H_DISMISS: u32 = 30;
const H_WIN: u32 = 50;

fn inline_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn id_variants(s: &str) -> [Option<&str>; 3] {
    let s = s.trim();
    if s.is_empty() {
        return [None, None, None];
    }
    let no_ext = s.strip_suffix(".desktop").filter(|x| !x.is_empty());
    // Telegram's desktop entry carries an instance hash suffix (`org.telegram.desktop._<hash>`),
    // while notifications and windows often use the unhashed `org.telegram.desktop`.
    let base = no_ext
        .unwrap_or(s)
        .split_once("._")
        .map(|(b, _)| b)
        .filter(|x| !x.is_empty());
    [Some(s), no_ext, base]
}

fn any_variant_eq(a: &[Option<&str>; 3], b: &[Option<&str>; 3]) -> bool {
    a.iter()
        .flatten()
        .any(|x| b.iter().flatten().any(|y| x.eq_ignore_ascii_case(y)))
}

fn non_generic_last(s: &str) -> Option<&str> {
    let last = s.rsplit('.').next().unwrap_or(s);
    if last.is_empty() || last.eq_ignore_ascii_case("desktop") || last.starts_with('_') {
        None
    } else {
        Some(last)
    }
}

fn match_id_pair(w_vars: &[Option<&str>; 3], candidate: &str) -> bool {
    let c_vars = id_variants(candidate);
    if c_vars[0].is_none() {
        return false;
    }
    if any_variant_eq(w_vars, &c_vars) {
        return true;
    }
    w_vars
        .iter()
        .flatten()
        .filter_map(|w| non_generic_last(w))
        .any(|wl| {
            c_vars
                .iter()
                .flatten()
                .filter_map(|c| non_generic_last(c))
                .any(|cl| wl.eq_ignore_ascii_case(cl))
        })
}

/// Whether `window_app_id` belongs to `app` (reference: `app_matches_window_id` in
/// `live/edge-rs/src/modules/apps.rs`).
pub fn app_matches_window_id(app: &App, window_app_id: &str) -> bool {
    let w_vars = id_variants(window_app_id);
    if w_vars[0].is_none() {
        return false;
    }

    if match_id_pair(&w_vars, &app.id) {
        return true;
    }
    if let Some(wm) = &app.wm_class {
        if match_id_pair(&w_vars, wm) {
            return true;
        }
    }
    if !app.name.is_empty()
        && w_vars.iter().flatten().any(|w| {
            w.eq_ignore_ascii_case(&app.name)
                || non_generic_last(w).is_some_and(|wl| wl.eq_ignore_ascii_case(&app.name))
        })
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
        && !matches!(cmd_bin, "flatpak" | "env")
        && w_vars.iter().flatten().any(|w| {
            w.eq_ignore_ascii_case(cmd_bin)
                || non_generic_last(w).is_some_and(|wl| wl.eq_ignore_ascii_case(cmd_bin))
        })
}

/// Whether `note` belongs to `app`: by its `desktop-entry` id, else by app name without case
/// (`card::Note` keeps one field from both).
pub fn note_matches_app(note: &Note, app: &App) -> bool {
    let n_vars = id_variants(&note.app);
    if n_vars[0].is_none() {
        return false;
    }
    let a_vars = id_variants(&app.id);
    if any_variant_eq(&n_vars, &a_vars) {
        return true;
    }
    !app.name.is_empty()
        && n_vars
            .iter()
            .flatten()
            .any(|n| n.eq_ignore_ascii_case(&app.name))
}

fn target_apps<'a>(cx: &'a Cx<'a>) -> Vec<&'a App> {
    if let Some(app) = cx.app {
        return vec![app];
    }
    cx.apps
        .iter()
        .filter(|a| provider::owner_of(a).is_some_and(|p| p.id() == "chat"))
        .collect()
}

struct Group<'a> {
    heading: String,
    notes: Vec<&'a Note>,
}

struct Plan<'a> {
    apps: Vec<&'a App>,
    groups: Vec<Group<'a>>,
    note_ids: Vec<u32>,
    windows: Vec<&'a Window>,
    open_apps: Vec<&'a App>,
}

impl<'a> Plan<'a> {
    fn build(cx: &'a Cx<'a>) -> Self {
        let apps = target_apps(cx);
        let limit = match cx.size {
            Size::S => MAX_S,
            Size::M | Size::L => MAX_M,
        };
        let matched_notes: Vec<(&Note, &App)> = cx
            .notes
            .iter()
            .filter_map(|n| {
                apps.iter()
                    .find(|a| note_matches_app(n, a))
                    .map(|&a| (n, a))
            })
            .take(limit)
            .collect();

        let mut groups: Vec<Group<'a>> = Vec::new();
        for (n, app) in matched_notes {
            let sum = inline_text(&n.summary);
            let heading = if sum.is_empty() {
                app.name.clone()
            } else {
                sum
            };
            if let Some(g) = groups.iter_mut().find(|g| g.heading == heading) {
                g.notes.push(n);
            } else {
                groups.push(Group {
                    heading,
                    notes: vec![n],
                });
            }
        }
        let note_ids: Vec<u32> = groups
            .iter()
            .flat_map(|g| g.notes.iter().map(|n| n.id))
            .collect();

        let windows: Vec<&'a Window> = cx
            .windows
            .iter()
            .filter(|w| apps.iter().any(|a| app_matches_window_id(a, &w.app_id)))
            .take(limit)
            .collect();

        let open_apps = if groups.is_empty() && windows.is_empty() {
            apps.iter().copied().take(limit).collect()
        } else {
            Vec::new()
        };

        Plan {
            apps,
            groups,
            note_ids,
            windows,
            open_apps,
        }
    }

    fn height(&self) -> f64 {
        if !self.open_apps.is_empty() {
            return self.open_apps.len() as f64 * ui::ROW_H;
        }
        let mut h = 0.0;
        let mut first = true;
        for g in &self.groups {
            if !first {
                h += GROUP_GAP;
            }
            first = false;
            h += SUB_H + g.notes.len() as f64 * ITEM_H;
        }
        if !self.windows.is_empty() {
            if !first {
                h += GROUP_GAP;
            }
            h += SUB_H + self.windows.len() as f64 * ITEM_H;
        }
        h
    }
}

fn draw_sub(cr: &Context, cx: &Cx, x: f64, y: f64, max_w: f64, label: &str) {
    draw::text_max(
        cr,
        x,
        y + 2.0,
        max_w,
        &Text {
            size: 11.0,
            bold: false,
            color: cx.theme.muted,
            text: label,
        },
    );
}

fn draw_x(cr: &Context, cx: &Cx, r: Rect, lit: bool) {
    let (cx_x, cy_x) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    draw::set(cr, if lit { cx.theme.text } else { cx.theme.muted });
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.move_to(cx_x - 4.0, cy_x - 4.0);
    cr.line_to(cx_x + 4.0, cy_x + 4.0);
    cr.move_to(cx_x + 4.0, cy_x - 4.0);
    cr.line_to(cx_x - 4.0, cy_x + 4.0);
    let _ = cr.stroke();
}

#[derive(Default)]
pub struct MessagesCard;

impl Card for MessagesCard {
    fn apply(&mut self, _key: &str, _data: &dyn Any) -> bool {
        false
    }

    fn status(&self, cx: &Cx) -> Status {
        if target_apps(cx).is_empty() {
            Status::Missing("No chat app installed".into())
        } else {
            Status::Ready
        }
    }

    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        Plan::build(cx).height()
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let plan = Plan::build(cx);
        if !plan.open_apps.is_empty() {
            let mut y = body.1;
            for (i, app) in plan.open_apps.iter().enumerate() {
                let label = format!("Open {}", app.name);
                ui::row(
                    cr,
                    cx,
                    hits,
                    H_OPEN + i as u32,
                    (body.0, y, body.2, ui::ROW_H),
                    &Row {
                        icon: None,
                        name: &label,
                        state: "",
                        on: false,
                        right: 0.0,
                    },
                );
                y += ui::ROW_H;
            }
            return;
        }

        let th = cx.theme;
        let mut y = body.1;
        let mut first = true;
        let mut note_idx = 0u32;

        for g in &plan.groups {
            if !first {
                y += GROUP_GAP;
            }
            first = false;
            draw_sub(
                cr,
                cx,
                body.0 + 8.0,
                y,
                (body.2 - 16.0).max(20.0),
                &g.heading,
            );
            y += SUB_H;
            for n in &g.notes {
                let r = (body.0, y, body.2, ITEM_H);
                let row_lit = hits.add(H_NOTE + note_idx, r) || hits.selected(H_NOTE + note_idx);
                let xr = (
                    r.0 + r.2 - X_BTN - 2.0,
                    r.1 + (r.3 - X_BTN) / 2.0,
                    X_BTN,
                    X_BTN,
                );
                let x_lit =
                    hits.add(H_DISMISS + note_idx, xr) || hits.selected(H_DISMISS + note_idx);
                if row_lit || x_lit {
                    ui::light(cr, cx, r);
                }
                let time = inline_text(&n.time);
                let right_used = if row_lit || x_lit {
                    if x_lit {
                        ui::light(cr, cx, xr);
                    }
                    draw_x(cr, cx, xr, x_lit);
                    X_BTN + 6.0
                } else if !time.is_empty() {
                    draw::text_right(
                        cr,
                        r.0 + r.2 - 8.0,
                        r.1 + (r.3 - 15.0) / 2.0,
                        &Text {
                            size: draw::SIZE_NOTE,
                            bold: false,
                            color: th.muted,
                            text: &time,
                        },
                    ) + 8.0
                } else {
                    0.0
                };
                let body_text = inline_text(&n.body);
                let line = if body_text.is_empty() {
                    &g.heading
                } else {
                    &body_text
                };
                draw::text_max(
                    cr,
                    r.0 + 8.0,
                    r.1 + (r.3 - 17.0) / 2.0,
                    (r.2 - 16.0 - right_used).max(20.0),
                    &Text {
                        size: 13.0,
                        bold: false,
                        color: th.text,
                        text: line,
                    },
                );
                note_idx += 1;
                y += ITEM_H;
            }
        }

        if !plan.windows.is_empty() {
            if !first {
                y += GROUP_GAP;
            }
            draw_sub(
                cr,
                cx,
                body.0 + 8.0,
                y,
                (body.2 - 16.0).max(20.0),
                "Windows",
            );
            y += SUB_H;
            for (i, win) in plan.windows.iter().enumerate() {
                let r = (body.0, y, body.2, ITEM_H);
                let lit = hits.add(H_WIN + i as u32, r) || hits.selected(H_WIN + i as u32);
                if lit {
                    ui::light(cr, cx, r);
                }
                if win.focused {
                    draw::round_rect(cr, r.0 + 8.0, r.1 + 7.0, 3.0, r.3 - 14.0, 1.5);
                    draw::set(cr, th.accent);
                    let _ = cr.fill();
                }
                let right_used = if lit {
                    draw::text_right(
                        cr,
                        r.0 + r.2 - 8.0,
                        r.1 + (r.3 - 16.0) / 2.0,
                        &Text {
                            size: 12.0,
                            bold: false,
                            color: th.muted,
                            text: "Go to window",
                        },
                    ) + 8.0
                } else {
                    0.0
                };
                let title = inline_text(&win.title);
                let fallback = plan
                    .apps
                    .iter()
                    .find(|a| app_matches_window_id(a, &win.app_id))
                    .map(|a| a.name.as_str())
                    .unwrap_or(win.app_id.as_str());
                let line = if title.is_empty() { fallback } else { &title };
                let tx = r.0 + 8.0 + if win.focused { 11.0 } else { 0.0 };
                draw::text_max(
                    cr,
                    tx,
                    r.1 + (r.3 - 17.0) / 2.0,
                    (r.0 + r.2 - 8.0 - right_used - tx).max(20.0),
                    &Text {
                        size: 13.0,
                        bold: false,
                        color: th.text,
                        text: line,
                    },
                );
                y += ITEM_H;
            }
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Event::Press {
            hit,
            button: Button::Left,
            ..
        } = ev
        else {
            return false;
        };
        let plan = Plan::build(cx);
        let h = *hit;
        if !plan.open_apps.is_empty() && h < H_NOTE {
            let Some(app) = plan.open_apps.get((h - H_OPEN) as usize) else {
                return false;
            };
            out.push(Action::Launch(app.id.clone()));
            return true;
        }
        if (H_NOTE..H_DISMISS).contains(&h) {
            let Some(&id) = plan.note_ids.get((h - H_NOTE) as usize) else {
                return false;
            };
            out.push(Action::Invoke(id, "default".into()));
            return true;
        }
        if (H_DISMISS..H_WIN).contains(&h) {
            let Some(&id) = plan.note_ids.get((h - H_DISMISS) as usize) else {
                return false;
            };
            out.push(Action::Dismiss(id));
            return true;
        }
        if h >= H_WIN {
            let Some(win) = plan.windows.get((h - H_WIN) as usize) else {
                return false;
            };
            out.push(Action::Focus(win.id));
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cx(th: &crate::theme::Theme) -> Cx<'_> {
        Cx {
            theme: th,
            now: chrono::NaiveDate::from_ymd_opt(2026, 11, 13)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            size: Size::S,
            app: None,
            apps: &[],
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
        }
    }

    fn discord_app() -> App {
        App {
            id: "discord".into(),
            name: "Discord".into(),
            icon: "discord".into(),
            exec: "/usr/bin/discord".into(),
            categories: vec!["Network".into(), "InstantMessaging".into()],
            wm_class: Some("discord".into()),
            terminal: false,
        }
    }

    fn telegram_app() -> App {
        App {
            id: "org.telegram.desktop._8cd7908c5e89c6139d26e96aae2e8a4a".into(),
            name: "Telegram".into(),
            icon: "telegram".into(),
            exec: "/home/user/Telegram/Telegram --".into(),
            categories: vec![
                "Chat".into(),
                "Network".into(),
                "InstantMessaging".into(),
                "Qt".into(),
            ],
            wm_class: Some("TelegramDesktop".into()),
            terminal: false,
        }
    }

    fn note(id: u32, app: &str, summary: &str, body: &str, time: &str) -> Note {
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

    fn window(id: u64, app_id: &str, title: &str, focused: bool) -> Window {
        Window {
            id,
            title: title.into(),
            app_id: app_id.into(),
            focused,
        }
    }

    #[test]
    fn provider_claims_discord_and_telegram_only_and_has_no_source() {
        let discord = discord_app();
        let flatpak_discord = App {
            id: "com.discordapp.Discord".into(),
            ..discord_app()
        };
        let telegram = telegram_app();
        let firefox = App {
            id: "firefox".into(),
            name: "Firefox".into(),
            ..Default::default()
        };

        assert_eq!(Chat.id(), "chat");
        assert_eq!(Chat.listed(), Listed::Never);
        assert_eq!(Chat.cards()[0].sizes, &[Size::S, Size::M]);
        assert_eq!(Chat.cards()[0].sources, &[] as &[&str]);
        assert!(Chat.source("messages").is_none());
        assert!(Chat.card("messages", None).is_some());
        assert!(Chat.card("messages", Some("extra")).is_none());
        assert!(Chat.card("other", None).is_none());

        assert_eq!(provider::owner_of(&discord).unwrap().id(), "chat");
        assert_eq!(provider::owner_of(&flatpak_discord).unwrap().id(), "chat");
        assert_eq!(provider::owner_of(&telegram).unwrap().id(), "chat");
        assert_eq!(provider::owner_of(&firefox).unwrap().id(), "simple");
    }

    #[test]
    fn note_and_window_matching_follow_id_wm_class_and_name() {
        let discord = discord_app();
        let flatpak_discord = App {
            id: "com.discordapp.Discord".into(),
            exec: "flatpak run com.discordapp.Discord".into(),
            ..discord_app()
        };
        let telegram = telegram_app();
        let telegram_renamed = App {
            name: "Telegram Desktop".into(),
            ..telegram_app()
        };

        assert!(note_matches_app(
            &note(1, "discord", "Friend", "hi", "12:00"),
            &discord
        ));
        assert!(note_matches_app(
            &note(2, "Discord", "Friend", "hi", "12:00"),
            &flatpak_discord
        ));
        assert!(note_matches_app(
            &note(3, "org.telegram.desktop", "Alice", "yo", "12:01"),
            &telegram
        ));
        assert!(note_matches_app(
            &note(3, "org.telegram.desktop", "Alice", "yo", "12:01"),
            &telegram_renamed
        ));
        assert!(note_matches_app(
            &note(
                4,
                "org.telegram.desktop._8cd7908c5e89c6139d26e96aae2e8a4a",
                "Alice",
                "yo",
                "12:01"
            ),
            &telegram
        ));
        assert!(note_matches_app(
            &note(5, "TELEGRAM", "Alice", "yo", "12:01"),
            &telegram
        ));
        assert!(!note_matches_app(
            &note(6, "Steam", "Friend", "hi", "12:00"),
            &discord
        ));
        assert!(!note_matches_app(
            &note(7, "", "Friend", "hi", "12:00"),
            &discord
        ));

        assert!(app_matches_window_id(&discord, "discord"));
        assert!(app_matches_window_id(&discord, "Discord"));
        assert!(app_matches_window_id(&flatpak_discord, "discord"));
        assert!(app_matches_window_id(
            &flatpak_discord,
            "com.discordapp.Discord"
        ));
        assert!(app_matches_window_id(&telegram, "org.telegram.desktop"));
        assert!(app_matches_window_id(
            &telegram_renamed,
            "org.telegram.desktop"
        ));
        assert!(app_matches_window_id(
            &telegram,
            "org.telegram.desktop._8cd7908c5e89c6139d26e96aae2e8a4a"
        ));
        assert!(app_matches_window_id(&telegram, "telegramdesktop"));
        assert!(!app_matches_window_id(&telegram, "org.other.desktop"));
        assert!(!app_matches_window_id(&flatpak_discord, "flatpak"));
        assert!(!app_matches_window_id(&discord, ""));
        assert!(!app_matches_window_id(&discord, "firefox"));
    }

    #[test]
    fn status_and_apply_cover_empty_tied_and_missing_states() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let mut card = MessagesCard;
        assert!(!card.apply("messages", &() as &dyn Any));

        let cx_none = test_cx(&th);
        assert_eq!(
            card.status(&cx_none),
            Status::Missing("No chat app installed".into())
        );

        let unrelated = [App {
            id: "firefox".into(),
            name: "Firefox".into(),
            ..Default::default()
        }];
        let mut cx_unrelated = test_cx(&th);
        cx_unrelated.apps = &unrelated;
        assert_eq!(
            card.status(&cx_unrelated),
            Status::Missing("No chat app installed".into())
        );

        let installed = [unrelated[0].clone(), telegram_app()];
        let mut cx_installed = test_cx(&th);
        cx_installed.apps = &installed;
        assert_eq!(card.status(&cx_installed), Status::Ready);

        let discord = discord_app();
        let mut cx_discord = test_cx(&th);
        cx_discord.app = Some(&discord);
        assert_eq!(card.status(&cx_discord), Status::Ready);
        assert_eq!(card.title(&cx_discord), None);
    }

    #[test]
    fn claims_desktop_entries_loaded_from_folder() {
        let dir = std::env::temp_dir().join(format!("edge_apps_chat_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("discord.desktop"),
            "[Desktop Entry]\nType=Application\nName=Discord\nExec=/usr/bin/discord\nStartupWMClass=discord\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("org.telegram.desktop._8cd7908c5e89c6139d26e96aae2e8a4a.desktop"),
            "[Desktop Entry]\nType=Application\nName=Telegram\nExec=/usr/bin/telegram-desktop --\nStartupWMClass=TelegramDesktop\n",
        )
        .unwrap();

        let apps = crate::app::load(std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);

        let (p_d, a_d) = provider::resolve("discord", &apps).expect("discord claimed");
        assert_eq!(p_d.id(), "chat");
        assert_eq!(a_d.expect("discord app").name, "Discord");

        let (p_t, a_t) = provider::resolve(
            "org.telegram.desktop._8cd7908c5e89c6139d26e96aae2e8a4a",
            &apps,
        )
        .expect("telegram claimed");
        assert_eq!(p_t.id(), "chat");
        assert_eq!(a_t.expect("telegram app").name, "Telegram");
    }

    #[test]
    fn groups_by_summary_newest_first_and_respects_size_limits() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let discord = discord_app();
        let notes: Vec<Note> = (0..10)
            .map(|i| {
                let who = if i % 2 == 0 { "Alice" } else { "#general" };
                note(100 - i, "discord", who, &format!("msg {i}"), "12:00")
            })
            .chain(std::iter::once(note(
                999, "Telegram", "Other", "ignored", "12:05",
            )))
            .collect();

        let mut cx_s = test_cx(&th);
        cx_s.app = Some(&discord);
        cx_s.size = Size::S;
        cx_s.notes = &notes;
        let p_s = Plan::build(&cx_s);
        assert_eq!(p_s.note_ids, vec![100, 98, 99]);
        assert_eq!(
            p_s.groups
                .iter()
                .map(|g| g.heading.as_str())
                .collect::<Vec<_>>(),
            vec!["Alice", "#general"]
        );

        let mut cx_m = test_cx(&th);
        cx_m.app = Some(&discord);
        cx_m.size = Size::M;
        cx_m.notes = &notes;
        let p_m = Plan::build(&cx_m);
        assert_eq!(p_m.note_ids, vec![100, 98, 96, 94, 99, 97, 95, 93]);
    }

    #[test]
    fn clicks_emit_exact_actions_and_hits_match_draw() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let discord = discord_app();
        let mut card = MessagesCard;

        // Empty: one row "Open Discord" -> Action::Launch
        let mut cx = test_cx(&th);
        cx.app = Some(&discord);
        let h_empty = card.height(&cx, 360.0);
        assert_eq!(h_empty, ui::ROW_H);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 300).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::default();
        card.draw(&cr, &cx, (0.0, 0.0, 360.0, h_empty), &mut hits);
        assert_eq!(hits.list.len(), 1);
        assert_eq!(hits.top_at((40.0, 20.0)), Some(H_OPEN));
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_OPEN,
                button: Button::Left,
                at: (40.0, 20.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Launch("discord".into())]);

        // Notifications + Windows: grouped notes, x dismisses, row invokes default, window focuses
        let notes = vec![
            note(11, "discord", "A friend", "Second message\nline", "12:04"),
            note(12, "discord", "#a-channel", "Channel post", "11:50"),
            note(10, "discord", "A friend", "First message", "12:03"),
            note(99, "Steam", "Download", "Done", "11:00"),
        ];
        let windows = vec![
            window(1, "firefox", "Browser", false),
            window(42, "discord", "Discord — #a-channel", true),
        ];
        let mut cx = test_cx(&th);
        cx.app = Some(&discord);
        cx.size = Size::M;
        cx.notes = &notes;
        cx.windows = &windows;

        let h = card.height(&cx, 360.0);
        // 2 note groups (18 + 2*36, gap 6, 18 + 1*36) + gap 6 + Windows (18 + 1*36) = 210.0
        assert_eq!(h, 210.0);

        let mut hits = Hits::default();
        card.draw(&cr, &cx, (10.0, 20.0, 360.0, h), &mut hits);
        // Last hit (Window 0) must end exactly at body.1 + h
        let last_rect = hits.list.last().unwrap().1;
        assert_eq!(last_rect.1 + last_rect.3, 20.0 + h);

        // First note row (id 11) and its x button
        let note0_rect = hits.list.iter().find(|(id, _)| *id == H_NOTE).unwrap().1;
        let x0_rect = hits.list.iter().find(|(id, _)| *id == H_DISMISS).unwrap().1;
        assert_eq!(
            hits.top_at((note0_rect.0 + 20.0, note0_rect.1 + 10.0)),
            Some(H_NOTE)
        );
        assert_eq!(
            hits.top_at((x0_rect.0 + 12.0, x0_rect.1 + 12.0)),
            Some(H_DISMISS)
        );

        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_NOTE,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Invoke(11, "default".into())]);

        // Second rendered note in group "A friend" is id 10
        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: H_DISMISS + 1,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Dismiss(10)]);

        // Third rendered note (in "#a-channel") is id 12
        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: H_NOTE + 2,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Invoke(12, "default".into())]);

        // Window row (id 42)
        let win_rect = hits.list.iter().find(|(id, _)| *id == H_WIN).unwrap().1;
        assert_eq!(
            hits.top_at((win_rect.0 + 20.0, win_rect.1 + 10.0)),
            Some(H_WIN)
        );
        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: H_WIN,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Focus(42)]);

        // Right click or bogus hit does nothing
        out.clear();
        assert!(!card.event(
            &Event::Press {
                hit: H_WIN,
                button: Button::Right,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(!card.event(
            &Event::Press {
                hit: 999,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
    }

    #[test]
    fn height_matches_draw_bounds_in_all_layouts_and_sizes() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let telegram = telegram_app();
        let notes = [
            note(1, "Telegram", "Bob", "first", "09:10"),
            note(2, "Telegram", "", "", ""),
            note(3, "Telegram", "Channel", "second", "09:12"),
            note(4, "Telegram", "Bob", "third", "09:13"),
        ];
        let windows = [
            window(10, "org.telegram.desktop", "Telegram", false),
            window(11, "TelegramDesktop", "", true),
        ];

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 600).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let card = MessagesCard;

        for size in [Size::S, Size::M] {
            for (ns, ws) in [
                (&notes[..0], &windows[..0]),
                (&notes[..], &windows[..0]),
                (&notes[..0], &windows[..]),
                (&notes[..], &windows[..]),
            ] {
                let mut cx = test_cx(&th);
                cx.app = Some(&telegram);
                cx.size = size;
                cx.notes = ns;
                cx.windows = ws;

                let h = card.height(&cx, 360.0);
                let mut hits = Hits::new(Some((340.0, 30.0)), None);
                card.draw(&cr, &cx, (0.0, 10.0, 360.0, h), &mut hits);
                let min_y = hits
                    .list
                    .iter()
                    .map(|(_, r)| r.1)
                    .fold(f64::INFINITY, f64::min);
                let max_y = hits
                    .list
                    .iter()
                    .map(|(_, r)| r.1 + r.3)
                    .fold(0.0_f64, f64::max);
                assert!(min_y >= 10.0);
                assert_eq!(max_y, 10.0 + h);
            }
        }
    }
}
