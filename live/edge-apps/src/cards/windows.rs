//! Open windows card (`windows/list`). Reads live toplevels from `cx.windows` (populated by the
//! Wayland core via `wlr-foreign-toplevel`), resolves each window's app name via `cx.apps`, marks
//! the focused window with the theme accent, filters via `Event::Text`, and emits `Action::Focus`,
//! `Action::ClosePanel`, and `Action::CloseWindow`.

use crate::app::App;
use crate::card::{Action, Button, Card, Cx, Event, Hits, Rect, Source, Status, Window};
use crate::cards::simple::app_matches_window_id;
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::ui;
use cairo::Context;
use std::any::Any;

const ROW_H: f64 = 44.0;
const EMPTY_H: f64 = 26.0;
const ICON_SZ: f64 = 28.0;
const CLOSE_SZ: f64 = 24.0;
const MAX_ROWS_S: usize = 4;
const MAX_ROWS_M: usize = 6;
const MAX_ROWS_L: usize = 12;

/// Provider for the open windows list (`windows/list`).
pub struct Windows;

const KINDS: [CardKind; 1] = [CardKind {
    id: "list",
    title: "Windows",
    about: "Switch or close open windows.",
    sizes: &[Size::M, Size::L],
    default: Size::M,
    half: false,
    sources: &[],
}];

impl Provider for Windows {
    fn id(&self) -> &'static str {
        "windows"
    }

    fn name(&self) -> &'static str {
        "Windows"
    }

    fn claims(&self) -> &'static [Claim] {
        &[]
    }

    fn listed(&self) -> Listed {
        Listed::Always
    }

    fn cards(&self) -> &'static [CardKind] {
        &KINDS
    }

    fn card(&self, kind: &str, argument: Option<&str>) -> Option<Box<dyn Card>> {
        if kind == "list" && argument.is_none() {
            Some(Box::new(WindowsCard::default()))
        } else {
            None
        }
    }

    fn source(&self, _key: &str) -> Option<Box<dyn Source>> {
        None
    }
}

/// Interactive open windows list card.
#[derive(Default)]
pub struct WindowsCard {
    scroll: usize,
    filter: String,
}

fn max_rows(size: Size) -> usize {
    match size {
        Size::S => MAX_ROWS_S,
        Size::M => MAX_ROWS_M,
        Size::L => MAX_ROWS_L,
    }
}

fn hit_row(idx: usize) -> u32 {
    (idx as u32).saturating_mul(2).saturating_add(1)
}

fn hit_close(idx: usize) -> u32 {
    (idx as u32).saturating_mul(2).saturating_add(2)
}

fn decode_hit(hit: u32) -> Option<(usize, bool)> {
    if hit == 0 {
        return None;
    }
    let zero_based = hit - 1;
    let idx = (zero_based / 2) as usize;
    let is_close = zero_based % 2 == 1;
    Some((idx, is_close))
}

fn resolve_app_name<'a>(apps: &'a [App], win: &'a Window) -> &'a str {
    apps.iter()
        .find(|a| app_matches_window_id(a, &win.app_id))
        .map(|a| a.name.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or(win.app_id.as_str())
}

fn matches_filter(win: &Window, app_name: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let title_low = win.title.to_lowercase();
    let app_low = app_name.to_lowercase();
    let id_low = win.app_id.to_lowercase();
    q.split_whitespace()
        .all(|word| title_low.contains(word) || app_low.contains(word) || id_low.contains(word))
}

fn filtered_windows<'a>(cx: &'a Cx, query: &str) -> Vec<(&'a Window, &'a str)> {
    cx.windows
        .iter()
        .filter_map(|win| {
            let app_name = resolve_app_name(cx.apps, win);
            matches_filter(win, app_name, query).then_some((win, app_name))
        })
        .collect()
}

fn draw_close_mark(cr: &Context, xr: Rect, color: crate::theme::Rgba) {
    let cx = xr.0 + xr.2 / 2.0;
    let cy = xr.1 + xr.3 / 2.0;
    let d = 4.0;
    draw::set(cr, color);
    cr.set_line_width(1.5);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.move_to(cx - d, cy - d);
    cr.line_to(cx + d, cy + d);
    let _ = cr.stroke();
    cr.move_to(cx + d, cy - d);
    cr.line_to(cx - d, cy + d);
    let _ = cr.stroke();
}

impl Card for WindowsCard {
    fn apply(&mut self, _key: &str, _data: &dyn Any) -> bool {
        false
    }

    fn status(&self, _cx: &Cx) -> Status {
        Status::Ready
    }

    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        let filtered = filtered_windows(cx, &self.filter);
        if filtered.is_empty() {
            EMPTY_H
        } else {
            filtered.len().min(max_rows(cx.size)) as f64 * ROW_H
        }
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let th = cx.theme;
        let filtered = filtered_windows(cx, &self.filter);
        if filtered.is_empty() {
            let msg = if cx.windows.is_empty() || self.filter.trim().is_empty() {
                "No open windows"
            } else {
                "No matching windows"
            };
            draw::text(
                cr,
                body.0 + 8.0,
                body.1 + 3.0,
                &Text {
                    size: 13.0,
                    bold: false,
                    color: th.muted,
                    text: msg,
                },
            );
            return;
        }

        let max_vis = max_rows(cx.size);
        let max_scroll = filtered.len().saturating_sub(max_vis);
        let start = self.scroll.min(max_scroll);
        let end = (start + max_vis).min(filtered.len());

        for (slot, idx) in (start..end).enumerate() {
            let (win, app_name) = filtered[idx];
            let r = (body.0, body.1 + slot as f64 * ROW_H, body.2, ROW_H);
            let xr = (
                r.0 + r.2 - CLOSE_SZ - 4.0,
                r.1 + (r.3 - CLOSE_SZ) / 2.0,
                CLOSE_SZ,
                CLOSE_SZ,
            );

            // Register the row hit first so the close mark sits on top inside the row.
            let row_id = hit_row(idx);
            let close_id = hit_close(idx);
            let row_lit = hits.add(row_id, r) || hits.selected(row_id);
            let close_lit = hits.add(close_id, xr) || hits.selected(close_id);
            let hovered = row_lit || close_lit;
            if hovered {
                ui::light(cr, cx, r);
            }

            if win.focused {
                draw::round_rect(cr, r.0 + 2.0, r.1 + 8.0, 3.0, r.3 - 16.0, 1.5);
                draw::set(cr, th.accent);
                let _ = cr.fill();
            }

            let ix = r.0 + 9.0;
            let iy = r.1 + (r.3 - ICON_SZ) / 2.0;
            let tile_label = if !app_name.is_empty() {
                app_name
            } else {
                win.title.as_str()
            };
            // the app's icon; a window with no entry tries its app id as an icon name
            let icon = cx
                .apps
                .iter()
                .find(|a| crate::cards::simple::app_matches_window_id(a, &win.app_id))
                .map_or(win.app_id.as_str(), |a| a.icon.as_str());
            crate::icons::draw_app_icon(cr, cx, icon, tile_label, ix, iy, ICON_SZ);

            let tx = ix + ICON_SZ + 10.0;
            let right_edge = if hovered { xr.0 - 4.0 } else { r.0 + r.2 - 8.0 };
            let max_w = (right_edge - tx).max(20.0);

            let title = if !win.title.is_empty() {
                win.title.as_str()
            } else if !app_name.is_empty() {
                app_name
            } else {
                "Window"
            };

            if !app_name.is_empty() {
                let ty = r.1 + (r.3 - 32.0) / 2.0;
                draw::text_max(
                    cr,
                    tx,
                    ty,
                    max_w,
                    &Text {
                        size: 13.0,
                        bold: win.focused,
                        color: th.text,
                        text: title,
                    },
                );
                draw::text_max(
                    cr,
                    tx,
                    ty + 17.0,
                    max_w,
                    &Text {
                        size: 11.0,
                        bold: false,
                        color: th.muted,
                        text: app_name,
                    },
                );
            } else {
                let t = Text {
                    size: 13.0,
                    bold: win.focused,
                    color: th.text,
                    text: title,
                };
                let (_, lh) = draw::measure(cr, &t);
                draw::text_max(cr, tx, r.1 + (r.3 - lh) / 2.0, max_w, &t);
            }

            if hovered {
                if close_lit {
                    ui::light(cr, cx, xr);
                }
                draw_close_mark(cr, xr, if close_lit { th.text } else { th.muted });
            }
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Text(q) => {
                if self.filter == *q {
                    return false;
                }
                self.filter = (*q).to_string();
                self.scroll = 0;
                true
            }
            Event::Scroll { dy, .. } => {
                let total = filtered_windows(cx, &self.filter).len();
                let max_scroll = total.saturating_sub(max_rows(cx.size));
                if max_scroll == 0 {
                    return false;
                }
                let cur = self.scroll.min(max_scroll);
                let next = if *dy > 0.0 {
                    (cur + 1).min(max_scroll)
                } else if *dy < 0.0 {
                    cur.saturating_sub(1)
                } else {
                    return false;
                };
                if next == self.scroll {
                    return false;
                }
                self.scroll = next;
                true
            }
            Event::Press { hit, button, .. } => {
                if *button != Button::Left {
                    return false;
                }
                let Some((idx, is_close)) = decode_hit(*hit) else {
                    return false;
                };
                let filtered = filtered_windows(cx, &self.filter);
                let Some((win, _)) = filtered.get(idx) else {
                    return false;
                };
                if is_close {
                    out.push(Action::CloseWindow(win.id));
                } else {
                    out.push(Action::Focus(win.id));
                    out.push(Action::ClosePanel);
                }
                true
            }
            // typed a name, then Enter: the first window that matches
            Event::Enter => {
                if self.filter.is_empty() {
                    return false;
                }
                let filtered = filtered_windows(cx, &self.filter);
                let Some((win, _)) = filtered.first() else {
                    return false;
                };
                out.push(Action::Focus(win.id));
                out.push(Action::ClosePanel);
                true
            }
            Event::Drag { .. } | Event::Release { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Button, Rect};
    use crate::config::Config;
    use crate::provider::providers;
    use crate::theme::Theme;

    fn test_app(id: &str, name: &str, wm_class: Option<&str>, exec: &str) -> App {
        App {
            id: id.into(),
            name: name.into(),
            icon: id.into(),
            exec: exec.into(),
            categories: Vec::new(),
            wm_class: wm_class.map(str::to_string),
            terminal: false,
        }
    }

    fn test_win(id: u64, title: &str, app_id: &str, focused: bool) -> Window {
        Window {
            id,
            title: title.into(),
            app_id: app_id.into(),
            focused,
        }
    }

    fn test_theme() -> Theme {
        Theme::from_config(&Config::from_text(""))
    }

    fn make_cx<'a>(size: Size, apps: &'a [App], windows: &'a [Window], theme: &'a Theme) -> Cx<'a> {
        let mut cx = crate::cards::settings::tests::test_cx(theme);
        cx.size = size;
        cx.apps = apps;
        cx.windows = windows;
        cx
    }

    fn draw_card(card: &WindowsCard, cx: &Cx, pointer: Option<(f64, f64)>) -> (f64, Hits) {
        let h = card.height(cx, 360.0);
        let surf =
            cairo::ImageSurface::create(cairo::Format::ARgb32, 360, (h as i32).max(1)).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(pointer, None);
        card.draw(&cr, cx, (0.0, 0.0, 360.0, h), &mut hits);
        (h, hits)
    }

    #[test]
    fn provider_metadata_and_registration() {
        let p = &Windows;
        assert_eq!(p.id(), "windows");
        assert_eq!(p.name(), "Windows");
        assert_eq!(p.listed(), Listed::Always);
        assert!(p.claims().is_empty());
        assert!(p.source("any").is_none());

        assert_eq!(p.cards().len(), 1);
        let kind = &p.cards()[0];
        assert_eq!(kind.id, "list");
        assert_eq!(kind.sizes, &[Size::M, Size::L]);
        assert_eq!(kind.default, Size::M);
        assert!(!kind.half);
        assert!(kind.sources.is_empty());

        assert!(p.card("list", None).is_some());
        assert!(p.card("list", Some("arg")).is_none());
        assert!(p.card("other", None).is_none());

        let all = providers();
        let win_pos = all.iter().position(|x| x.id() == "windows").unwrap();
        let simple_pos = all.iter().position(|x| x.id() == "simple").unwrap();
        assert!(
            win_pos < simple_pos,
            "windows must be registered before simple fallback"
        );
    }

    #[test]
    fn apply_and_status() {
        let th = test_theme();
        let cx = make_cx(Size::M, &[], &[], &th);
        let mut c = Windows.card("list", None).unwrap();
        assert_eq!(c.status(&cx), Status::Ready);
        assert!(!c.apply("anything", &() as &dyn Any));
        assert_eq!(c.status(&cx), Status::Ready);
    }

    #[test]
    fn resolves_app_name_by_id_wm_class_exec_or_raw() {
        let apps = [
            test_app(
                "org.mozilla.firefox",
                "Firefox",
                Some("firefox-bin"),
                "firefox %u",
            ),
            test_app("foot", "Foot Terminal", None, "/usr/bin/foot"),
        ];
        let w_wm = test_win(1, "Docs", "firefox-bin", true);
        let w_exec = test_win(2, "~", "foot", false);
        let w_raw = test_win(3, "Custom", "com.example.App", false);

        assert_eq!(resolve_app_name(&apps, &w_wm), "Firefox");
        assert_eq!(resolve_app_name(&apps, &w_exec), "Foot Terminal");
        assert_eq!(resolve_app_name(&apps, &w_raw), "com.example.App");
    }

    #[test]
    fn empty_state_height_and_no_hits() {
        let th = test_theme();
        let cx = make_cx(Size::M, &[], &[], &th);
        let c = WindowsCard::default();
        let (h, hits) = draw_card(&c, &cx, None);
        assert_eq!(h, EMPTY_H);
        assert!(hits.list.is_empty());
    }

    #[test]
    fn height_matches_drawn_rows_for_m_and_l() {
        let th = test_theme();
        let windows: Vec<Window> = (1..=15)
            .map(|i| test_win(i, &format!("Window {i}"), "term", i == 1))
            .collect();

        let c = WindowsCard::default();
        let cx_m = make_cx(Size::M, &[], &windows, &th);
        let (hm, hits_m) = draw_card(&c, &cx_m, None);
        assert_eq!(hm, MAX_ROWS_M as f64 * ROW_H);
        assert_eq!(hits_m.list.len(), MAX_ROWS_M * 2);
        for (_, r) in &hits_m.list {
            assert!(r.1 >= 0.0 && r.1 + r.3 <= hm);
        }

        let cx_l = make_cx(Size::L, &[], &windows, &th);
        let (hl, hits_l) = draw_card(&c, &cx_l, None);
        assert_eq!(hl, MAX_ROWS_L as f64 * ROW_H);
        assert_eq!(hits_l.list.len(), MAX_ROWS_L * 2);
        for (_, r) in &hits_l.list {
            assert!(r.1 >= 0.0 && r.1 + r.3 <= hl);
        }

        // Fewer windows than max_rows shrinks to fit.
        let cx_few = make_cx(Size::M, &[], &windows[..2], &th);
        let (hf, hits_f) = draw_card(&c, &cx_few, None);
        assert_eq!(hf, 2.0 * ROW_H);
        assert_eq!(hits_f.list.len(), 4);
        assert!(hits_f
            .list
            .iter()
            .all(|(_, r)| r.1 >= 0.0 && r.1 + r.3 <= hf));
        assert_eq!(
            hits_f
                .list
                .iter()
                .filter(|(id, _)| *id % 2 == 1)
                .map(|(_, r)| r.1 + r.3)
                .next_back(),
            Some(hf)
        );
    }

    #[test]
    fn focus_and_close_hits_and_actions() {
        let th = test_theme();
        let apps = [test_app("org.mozilla.firefox", "Firefox", None, "firefox")];
        let windows = [
            test_win(10, "Cargo Docs", "firefox", true),
            test_win(20, "Notes", "mousepad", false),
        ];
        let cx = make_cx(Size::M, &apps, &windows, &th);
        let mut c = WindowsCard::default();

        // Draw to inspect exact hit rects for row 1 (second window, id 20).
        let (_, hits) = draw_card(&c, &cx, None);
        let row1_rect: Rect = hits
            .list
            .iter()
            .find(|(id, _)| *id == hit_row(1))
            .unwrap()
            .1;
        let close1_rect: Rect = hits
            .list
            .iter()
            .find(|(id, _)| *id == hit_close(1))
            .unwrap()
            .1;

        // Clicking left side of row 1 hits row_hit (focus + close panel).
        let row_pt = (row1_rect.0 + 40.0, row1_rect.1 + ROW_H / 2.0);
        let (_, row_hits) = draw_card(&c, &cx, Some(row_pt));
        let row_hit_id = row_hits.top_at(row_pt).unwrap();
        assert_eq!(row_hit_id, hit_row(1));
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: row_hit_id,
                button: Button::Left,
                at: row_pt,
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Focus(20), Action::ClosePanel]);

        // Clicking inside close mark hits close_hit on top of row_hit (CloseWindow only).
        let close_pt = (
            close1_rect.0 + close1_rect.2 / 2.0,
            close1_rect.1 + close1_rect.3 / 2.0,
        );
        let (_, close_hits) = draw_card(&c, &cx, Some(close_pt));
        let close_hit_id = close_hits.top_at(close_pt).unwrap();
        assert_eq!(close_hit_id, hit_close(1));
        out.clear();
        assert!(c.event(
            &Event::Press {
                hit: close_hit_id,
                button: Button::Left,
                at: close_pt,
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::CloseWindow(20)]);

        // Right click does nothing.
        out.clear();
        assert!(!c.event(
            &Event::Press {
                hit: row_hit_id,
                button: Button::Right,
                at: row_pt,
            },
            &cx,
            &mut out
        ));
        assert!(out.is_empty());
    }

    #[test]
    fn text_filter_by_title_and_app_name_and_empty_match() {
        let th = test_theme();
        let apps = [
            test_app("org.mozilla.firefox", "Firefox", None, "firefox"),
            test_app("Alacritty", "Alacritty", None, "alacritty"),
        ];
        let windows = [
            test_win(1, "Crate docs", "org.mozilla.firefox", false),
            test_win(2, "shell", "Alacritty", true),
            test_win(3, "README", "raw.editor", false),
        ];
        let cx = make_cx(Size::M, &apps, &windows, &th);
        let mut c = WindowsCard::default();
        let mut out = Vec::new();

        // Filter by app name ("fire").
        assert!(c.event(&Event::Text("fire"), &cx, &mut out));
        assert!(!c.event(&Event::Text("fire"), &cx, &mut out));
        assert_eq!(c.height(&cx, 360.0), ROW_H);
        let (_, hits) = draw_card(&c, &cx, None);
        assert_eq!(hits.list.len(), 2);
        c.event(
            &Event::Press {
                hit: hit_row(0),
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out,
        );
        assert_eq!(out, vec![Action::Focus(1), Action::ClosePanel]);

        // Multi-word filter across title + app name ("alac shell").
        assert!(c.event(&Event::Text("alac shell"), &cx, &mut out));
        assert_eq!(c.height(&cx, 360.0), ROW_H);
        out.clear();
        c.event(
            &Event::Press {
                hit: hit_row(0),
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out,
        );
        assert_eq!(out, vec![Action::Focus(2), Action::ClosePanel]);

        // Unmatched filter shows single muted line height and no hits.
        assert!(c.event(&Event::Text("nomatch"), &cx, &mut out));
        assert_eq!(c.height(&cx, 360.0), EMPTY_H);
        let (_, empty_hits) = draw_card(&c, &cx, None);
        assert!(empty_hits.list.is_empty());

        // Clearing filter restores all 3 rows.
        assert!(c.event(&Event::Text(""), &cx, &mut out));
        assert_eq!(c.height(&cx, 360.0), 3.0 * ROW_H);
    }

    #[test]
    fn scroll_past_size_for_m_and_l() {
        let th = test_theme();
        let windows: Vec<Window> = (1..=14)
            .map(|i| test_win(i, &format!("Win {i}"), "term", false))
            .collect();

        // Size::M holds 6 rows -> 14 windows give max_scroll = 8.
        let cx_m = make_cx(Size::M, &[], &windows, &th);
        let mut c = WindowsCard::default();
        let mut out = Vec::new();
        // Scroll up at top is a no-op.
        assert!(!c.event(
            &Event::Scroll {
                hit: None,
                dy: -1.0
            },
            &cx_m,
            &mut out
        ));
        // Scroll down advances to offset 1 (windows 2..8).
        assert!(c.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_m, &mut out));
        assert_eq!(c.scroll, 1);
        let (_, hits) = draw_card(&c, &cx_m, None);
        // First visible row hit belongs to filtered index 1 (Window id 2).
        assert_eq!(hits.list[0].0, hit_row(1));
        assert!(c.event(
            &Event::Press {
                hit: hits.list[0].0,
                button: Button::Left,
                at: (20.0, 20.0),
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(out, vec![Action::Focus(2), Action::ClosePanel]);

        // Typing a filter resets scroll back to 0.
        assert!(c.event(&Event::Text("Win 14"), &cx_m, &mut out));
        assert_eq!(c.scroll, 0);
        // Only 1 window matches -> scroll is a no-op.
        assert!(!c.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_m, &mut out));

        // Size::L holds 12 rows -> 14 windows give max_scroll = 2.
        assert!(c.event(&Event::Text(""), &cx_m, &mut out));
        let cx_l = make_cx(Size::L, &[], &windows, &th);
        assert!(c.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out));
        assert!(c.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out));
        assert!(!c.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out));
        assert_eq!(c.scroll, 2);
    }

    #[test]
    fn shell_resolves_old_word_and_clicks_focus_and_close() {
        let cfg = Config::from_text("PANEL_CONTROL=\"SUPER+L left top 380 windows\"\n");
        let apps = vec![test_app(
            "org.mozilla.firefox",
            "Firefox",
            None,
            "firefox %u",
        )];
        let mut sh = crate::shell::Shell::new(cfg, apps);
        sh.windows = vec![
            test_win(11, "Home", "org.mozilla.firefox", true),
            test_win(12, "Notes", "mousepad", false),
        ];
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert_eq!(sh.panels[idx].cards.len(), 1);
        assert_eq!(sh.panels[idx].cards[0].provider.id(), "windows");
        assert_eq!(sh.panels[idx].cards[0].kind.id, "list");

        assert!(sh.open_panel(idx, false, std::time::Instant::now()));
        sh.open_ms = f64::INFINITY;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);

        let (close_rect, row_rect) = {
            let f = sh.frame.borrow();
            let list = &f[0].2.list;
            let close_rect = list.iter().find(|(id, _)| *id == hit_close(1)).unwrap().1;
            let row_rect = list.iter().find(|(id, _)| *id == hit_row(0)).unwrap().1;
            (close_rect, row_rect)
        };

        assert!(sh.click(
            1280.0,
            800.0,
            close_rect.0 + 4.0,
            close_rect.1 + 4.0,
            Button::Left
        ));
        assert!(sh.open.is_some(), "closing a window leaves the panel open");

        assert!(sh.click(
            1280.0,
            800.0,
            row_rect.0 + 40.0,
            row_rect.1 + 10.0,
            Button::Left
        ));
        assert!(sh.open.is_none(), "focusing a window closes the panel");
        let actions: Vec<Action> = sh.actions.iter().map(|(a, _, _, _)| a.clone()).collect();
        assert_eq!(actions, vec![Action::CloseWindow(12), Action::Focus(11)]);
    }
}
