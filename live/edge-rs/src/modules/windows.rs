//! The `windows` module: open windows from `wlr-foreign-toplevel` (or snapshot / fixed render
//! data), filtered by `world.search`. Focused window has a 3 px accent bar on the left, title with
//! muted app name under it, click activates, and a small `x` on the right closes.

use super::{is_left_button, Rect, World};
use crate::config::Item;
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::theme::{Rgba, Theme};
use cairo::Context;

pub const TITLE_H: f64 = 28.0;
pub const ROW_H: f64 = 44.0;
pub const MAX_VISIBLE_ROWS: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowItem {
    pub id: u64,
    pub inst: Option<u64>,
    pub app_id: String,
    pub title: String,
    pub focused: bool,
    pub attention: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowOp {
    Activate(u64),
    Close(u64),
}

#[derive(Clone, Debug, Default)]
pub struct WindowsData {
    pub windows: Vec<WindowItem>,
    pub filtered: Vec<usize>,
    pub scroll: usize,
    pub pending_ops: Vec<WindowOp>,
}

impl WindowsData {
    pub fn update_filter(&mut self, query: &str) {
        self.filtered = filter_windows(&self.windows, query);
        if self.scroll >= self.filtered.len() {
            self.scroll = 0;
        }
    }

    pub fn set_windows(&mut self, windows: Vec<WindowItem>, query: &str) {
        self.windows = windows;
        self.update_filter(query);
    }
}

fn extract_json_str(block: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = block.find(&needle)? + needle.len();
    let mut out = String::new();
    let mut chars = block[start..].chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            return Some(out);
        }
        if c == '\\' {
            if let Some(esc) = chars.next() {
                out.push(esc);
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn parse_snapshot_json(buf: &str) -> Vec<WindowItem> {
    let mut out = Vec::new();
    let Some(elems_idx) = buf.find("\"elements\":[") else {
        return out;
    };
    let rest = &buf[elems_idx + 12..];
    let parts: Vec<&str> = rest.split("\"id\":[").skip(1).collect();
    for part in parts {
        let Some(end_bracket) = part.find(']') else {
            continue;
        };
        let mut nums = part[..end_bracket].split(',');
        let Some(inst) = nums.next().and_then(|s| s.trim().parse::<u64>().ok()) else {
            continue;
        };
        let Some(elem) = nums.next().and_then(|s| s.trim().parse::<u64>().ok()) else {
            continue;
        };
        let state = extract_json_str(part, "state").unwrap_or_default();
        if state != "mapped" {
            continue;
        }
        let mut app_id = extract_json_str(part, "app_id").unwrap_or_default();
        let mut title = extract_json_str(part, "title").unwrap_or_default();
        if app_id.is_empty() {
            app_id = "Window".to_string();
        }
        if title.is_empty() {
            title = app_id.clone();
        }
        let focused = part.contains("\"focused\":true") || out.is_empty();
        let attention = part.contains("\"attention\":true");
        out.push(WindowItem {
            id: elem,
            inst: Some(inst),
            app_id,
            title,
            focused,
            attention,
        });
    }
    out
}

fn parse_kv_quoted(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    let pos = line.find(&prefix)? + prefix.len();
    let rest = &line[pos..];
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        Some(rest.split_whitespace().next().unwrap_or("").to_string())
    }
}

fn parse_state_flags(s: &str) -> (bool, bool) {
    let lower = s.to_ascii_lowercase();
    let focused = lower.contains("focused")
        || lower.contains("activated")
        || lower.contains("active")
        || lower == "*"
        || lower == "1"
        || lower == "true";
    let attention = lower.contains("attention") || lower.contains("urgent");
    (focused, attention)
}

/// Parses window list output from TSV render data, `wlr-foreign-toplevel` dumps, or
/// `infynitae msg snapshot` JSON.
pub fn parse_windows(out: &str) -> Vec<WindowItem> {
    if out.contains("\"elements\":[") {
        return parse_snapshot_json(out);
    }

    let mut windows = Vec::new();
    let mut next_id = 1u64;
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains("app_id=") || line.contains("title=") {
            let id = line
                .split([' ', ':'])
                .find_map(|tok| tok.parse::<u64>().ok())
                .unwrap_or(next_id);
            next_id = id.saturating_add(1);
            let mut app_id = parse_kv_quoted(line, "app_id").unwrap_or_default();
            let mut title = parse_kv_quoted(line, "title").unwrap_or_default();
            let state = parse_kv_quoted(line, "state").unwrap_or_default();
            let (focused, attention) = parse_state_flags(&state);
            if app_id.is_empty() && title.is_empty() {
                continue;
            }
            if app_id.is_empty() {
                app_id = "Window".to_string();
            }
            if title.is_empty() {
                title = app_id.clone();
            }
            windows.push(WindowItem {
                id,
                inst: None,
                app_id,
                title,
                focused,
                attention,
            });
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        let (id, app_id, title, state_s) = match parts.as_slice() {
            [p0, p1, p2, p3, ..] => {
                let id = p0.trim().parse::<u64>().unwrap_or(next_id);
                (id, p1.trim(), p2.trim(), p3.trim())
            }
            [p0, p1, p2] => {
                if let Ok(id) = p0.trim().parse::<u64>() {
                    (id, p1.trim(), p2.trim(), "")
                } else {
                    (next_id, p0.trim(), p1.trim(), p2.trim())
                }
            }
            [p0, p1] => (next_id, p0.trim(), p1.trim(), ""),
            [p0] if !p0.trim().is_empty() => (next_id, p0.trim(), p0.trim(), ""),
            _ => continue,
        };
        next_id = id.saturating_add(1);
        let (focused, attention) = parse_state_flags(state_s);
        let app_id = if app_id.is_empty() { "Window" } else { app_id }.to_string();
        let title = if title.is_empty() {
            app_id.clone()
        } else {
            title.to_string()
        };
        windows.push(WindowItem {
            id,
            inst: None,
            app_id,
            title,
            focused,
            attention,
        });
    }
    windows
}

/// Filters `windows` by `query` (matching `title` or `app_id` case-insensitively).
pub fn filter_windows(windows: &[WindowItem], query: &str) -> Vec<usize> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return (0..windows.len()).collect();
    }
    let words: Vec<&str> = q.split_ascii_whitespace().collect();
    windows
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let title_l = w.title.to_ascii_lowercase();
            let app_l = w.app_id.to_ascii_lowercase();
            words
                .iter()
                .all(|&word| title_l.contains(word) || app_l.contains(word))
        })
        .map(|(i, _)| i)
        .collect()
}

pub fn apply(data: &mut WindowsData, key: &str, out: &str, query: &str) -> bool {
    if key == "windows" {
        data.set_windows(parse_windows(out), query);
        true
    } else {
        false
    }
}

pub fn visible_rows_for_height(h: f64) -> usize {
    let inner = (h - 2.0 * PAD_Y - TITLE_H).max(ROW_H);
    (inner / ROW_H).floor().max(1.0) as usize
}

pub fn height(it: &Item, world: &World) -> f64 {
    if let Some((_, _, _, rh)) = it.rect {
        return rh as f64;
    }
    let count = world.windows.filtered.len();
    let rows = count.clamp(1, MAX_VISIBLE_ROWS);
    2.0 * PAD_Y + TITLE_H + rows as f64 * ROW_H
}

pub fn activate_window(world: &mut World, win_idx: usize) -> bool {
    let Some(win) = world.windows.windows.get(win_idx) else {
        return false;
    };
    let id = win.id;
    let inst = win.inst;
    for (i, w) in world.windows.windows.iter_mut().enumerate() {
        w.focused = i == win_idx;
    }
    world.windows.pending_ops.push(WindowOp::Activate(id));
    world.close_requested = true;
    if let Some(inst) = inst {
        world.run(format!(
            "infynitae msg navigate element {inst} {id} && infynitae msg focus {inst} {id}"
        ));
    } else {
        world
            .actions
            .push(format!("wlr-foreign-toplevel activate {id}"));
    }
    true
}

pub fn close_window(world: &mut World, win_idx: usize) -> bool {
    let Some(win) = world.windows.windows.get(win_idx) else {
        return false;
    };
    let id = win.id;
    let inst = win.inst;
    world.windows.pending_ops.push(WindowOp::Close(id));
    if let Some(inst) = inst {
        world.run(format!("infynitae msg close {inst} {id}"));
    } else {
        world
            .actions
            .push(format!("wlr-foreign-toplevel close {id}"));
    }
    world.windows.windows.remove(win_idx);
    world.windows.update_filter(&world.search);
    true
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    let (w, h) = size;
    let (px, py) = pos;
    if py < PAD_Y + TITLE_H || world.windows.filtered.is_empty() {
        return false;
    }
    let vis = visible_rows_for_height(h).min(world.windows.filtered.len());
    let max_scroll = world.windows.filtered.len().saturating_sub(vis);
    let scroll = world.windows.scroll.min(max_scroll);
    let row = ((py - (PAD_Y + TITLE_H)) / ROW_H).floor() as isize;
    if row < 0 || (row as usize) >= vis {
        return false;
    }
    let idx = scroll + row as usize;
    let Some(&win_idx) = world.windows.filtered.get(idx) else {
        return false;
    };
    let on_close_x = px >= (w - PAD_X - 24.0).max(0.0);
    if on_close_x || button == 2 || button == 3 || button == 0x111 || button == 0x112 {
        return close_window(world, win_idx);
    }
    if is_left_button(button) {
        return activate_window(world, win_idx);
    }
    false
}

pub fn scroll(world: &mut World, size: (f64, f64), _pos: (f64, f64), dy: f64) -> bool {
    let count = world.windows.filtered.len();
    let vis = visible_rows_for_height(size.1);
    if count <= vis {
        return false;
    }
    let max_scroll = count - vis;
    let cur = world.windows.scroll.min(max_scroll);
    let next = if dy > 0.0 {
        (cur + 1).min(max_scroll)
    } else if dy < 0.0 {
        cur.saturating_sub(1)
    } else {
        cur
    };
    if next != world.windows.scroll {
        world.windows.scroll = next;
        true
    } else {
        false
    }
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    draw::card(cr, th, x, y, w, h);
    draw::title(cr, th, x + PAD_X, y + PAD_Y, "Windows");

    let count = world.windows.filtered.len();
    if count == 0 {
        let empty = if world.search.is_empty() {
            "No open windows"
        } else {
            "No matching windows"
        };
        draw::text_max(
            cr,
            x + PAD_X,
            y + PAD_Y + TITLE_H + 12.0,
            (w - 2.0 * PAD_X).max(0.0),
            &Text {
                size: 13.0,
                bold: false,
                color: th.muted,
                text: empty,
            },
        );
        return;
    }

    let vis = visible_rows_for_height(h).min(count);
    let max_scroll = count.saturating_sub(vis);
    let scroll = world.windows.scroll.min(max_scroll);
    let sel_win_row = world.selected_window_row();

    for k in 0..vis {
        let i = scroll + k;
        let win_idx = world.windows.filtered[i];
        let Some(win) = world.windows.windows.get(win_idx) else {
            continue;
        };
        let ry = y + PAD_Y + TITLE_H + k as f64 * ROW_H;
        let is_sel = sel_win_row == Some(i);

        if is_sel {
            draw::round_rect(
                cr,
                x + PAD_X - 8.0,
                ry + 1.0,
                (w - 2.0 * (PAD_X - 8.0)).max(0.0),
                ROW_H - 2.0,
                6.0,
            );
            draw::set(cr, Rgba(th.accent.0, th.accent.1, th.accent.2, 0.18));
            let _ = cr.fill();
        }

        if win.focused {
            draw::round_rect(cr, x + PAD_X, ry + 7.0, 3.0, ROW_H - 14.0, 1.5);
            draw::set(cr, th.accent);
            let _ = cr.fill();
        }

        let tx = x + PAD_X + if win.focused { 11.0 } else { 0.0 };
        let close_cx = x + w - PAD_X - 8.0;
        let close_cy = ry + ROW_H / 2.0;
        let max_w = (close_cx - 16.0 - tx).max(20.0);

        let primary = if win.title.is_empty() {
            &win.app_id
        } else {
            &win.title
        };
        draw::text_max(
            cr,
            tx,
            ry + 5.0,
            max_w,
            &Text {
                size: 13.0,
                bold: win.focused || is_sel,
                color: th.text,
                text: primary,
            },
        );
        draw::text_max(
            cr,
            tx,
            ry + 23.0,
            max_w,
            &Text {
                size: 11.5,
                bold: false,
                color: th.muted,
                text: &win.app_id,
            },
        );

        // Small stroked 'x' close icon on the right
        draw::set(cr, th.muted);
        cr.set_line_width(1.5);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.move_to(close_cx - 4.0, close_cy - 4.0);
        cr.line_to(close_cx + 4.0, close_cy + 4.0);
        cr.move_to(close_cx + 4.0, close_cy - 4.0);
        cr.line_to(close_cx - 4.0, close_cy + 4.0);
        let _ = cr.stroke();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_windows_handles_tsv_toplevel_dump_and_snapshot_json() {
        let tsv = "\
1\tfirefox\tMozilla Firefox — Docs\tfocused
2\tfoot\t~/edgers — foot\t
3\torg.gnome.Nautilus\tFiles\tattention
";
        let wins = parse_windows(tsv);
        assert_eq!(wins.len(), 3);
        assert_eq!(wins[0].id, 1);
        assert_eq!(wins[0].app_id, "firefox");
        assert_eq!(wins[0].title, "Mozilla Firefox — Docs");
        assert!(wins[0].focused);
        assert!(!wins[1].focused);
        assert!(wins[2].attention);

        let dump = "\
toplevel 10: app_id=\"firefox\" title=\"Rust Book\" state=activated
toplevel 11: app_id=\"foot\" title=\"shell\" state=none
";
        let wins_dump = parse_windows(dump);
        assert_eq!(wins_dump.len(), 2);
        assert_eq!(wins_dump[0].id, 10);
        assert!(wins_dump[0].focused);
        assert_eq!(wins_dump[1].id, 11);
        assert!(!wins_dump[1].focused);

        let json = r#"{"elements":[{"id":[1,42],"state":"mapped","app_id":"firefox","title":"Browser"},{"id":[1,99],"state":"mapped","app_id":"foot","title":"Terminal","attention":true}]}"#;
        let wins_json = parse_windows(json);
        assert_eq!(wins_json.len(), 2);
        assert_eq!(wins_json[0].inst, Some(1));
        assert_eq!(wins_json[0].id, 42);
        assert_eq!(wins_json[1].id, 99);
        assert!(wins_json[1].attention);
    }

    #[test]
    fn clicking_window_activates_and_clicking_x_closes() {
        let mut world = World::now();
        world.apply(
            "windows",
            "1\tfirefox\tMozilla Firefox\tfocused\n2\tfoot\tTerminal\t\n",
        );
        let w = 560.0;
        let h = 200.0;

        // Click body of second row (foot) -> activates window 2
        assert!(click(
            &mut world,
            (w, h),
            (80.0, PAD_Y + TITLE_H + ROW_H + 12.0),
            1
        ));
        assert!(!world.windows.windows[0].focused);
        assert!(world.windows.windows[1].focused);
        assert_eq!(
            world.windows.pending_ops.last(),
            Some(&WindowOp::Activate(2))
        );
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("wlr-foreign-toplevel activate 2")
        );

        // Click small 'x' on first row (firefox) -> closes window 1
        assert!(click(
            &mut world,
            (w, h),
            (w - PAD_X - 6.0, PAD_Y + TITLE_H + 12.0),
            1
        ));
        assert_eq!(world.windows.pending_ops.last(), Some(&WindowOp::Close(1)));
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("wlr-foreign-toplevel close 1")
        );
        assert_eq!(world.windows.windows.len(), 1);
        assert_eq!(world.windows.windows[0].id, 2);
    }
}
