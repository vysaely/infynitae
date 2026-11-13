//! The `notifications` module: title `"Notifications"` and a `"Clear all"` text button (`th.muted`)
//! on the right, notifications from `makoctl list` / `makoctl history` newest first (app and time
//! in 11 px muted on one line, title in 13 px medium, body in 12 px muted up to two lines), click
//! to run the default action and dismiss, and hidden when there are none.

use crate::draw::{self, Text, FONT, PAD_X, PAD_Y};
use crate::modules::{clipboard::sanitize_inline_text, is_left_button, Rect, World};
use crate::theme::Theme;
use cairo::Context;

pub const MAX_NOTIFS: usize = 4;
pub const HEAD_H: f64 = 28.0;
pub const ITEM_GAP: f64 = 10.0;

pub const CMDS: &[(&str, &str)] = &[(
    "notifications",
    "makoctl list -j 2>/dev/null; makoctl history -j 2>/dev/null",
)];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifItem {
    pub id: u32,
    pub app: String,
    pub time: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Default)]
pub struct NotificationsData {
    pub ready: bool,
    pub items: Vec<NotifItem>,
    pub cleared_id: u32,
    pub max_id: u32,
}

pub fn apply(d: &mut NotificationsData, key: &str, out: &str) -> bool {
    if key != "notifications" {
        return false;
    }
    let (items, max_id) = parse_mako(out, d.cleared_id);
    if max_id > d.max_id {
        d.max_id = max_id;
    }
    d.items = items;
    d.ready = true;
    true
}

fn clean_body_text(src: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for raw_line in src.lines() {
        let cleaned = sanitize_inline_text(raw_line);
        if !cleaned.is_empty() {
            lines.push(cleaned);
        }
    }
    lines.join("\n")
}

#[derive(Default)]
struct RawNotif {
    id: u32,
    app: String,
    time: String,
    summary: String,
    body: String,
}

fn push_raw_notif(raw: RawNotif, cleared_id: u32, max_id: &mut u32, out: &mut Vec<NotifItem>) {
    if raw.id > *max_id {
        *max_id = raw.id;
    }
    if raw.id != 0 && cleared_id > 0 && raw.id <= cleared_id {
        return;
    }
    if raw.id != 0 && out.iter().any(|n| n.id == raw.id) {
        return;
    }
    let s_clean = sanitize_inline_text(&raw.summary);
    let b_inline = sanitize_inline_text(&raw.body);
    let b_multiline = clean_body_text(&raw.body);
    if s_clean.is_empty() && b_inline.is_empty() {
        return;
    }
    let app_clean = sanitize_inline_text(&raw.app);
    let app = if app_clean.is_empty() {
        "Notify".to_string()
    } else {
        app_clean
    };
    let (title, body) = if s_clean.is_empty() {
        (b_inline, String::new())
    } else if !b_inline.is_empty() && !s_clean.eq_ignore_ascii_case(&b_inline) {
        (s_clean, b_multiline)
    } else {
        (s_clean, String::new())
    };
    if out.iter().any(|n| n.app == app && n.title == title) {
        return;
    }
    out.push(NotifItem {
        id: raw.id,
        app,
        time: sanitize_inline_text(&raw.time),
        title,
        body,
    });
}

fn parse_json_string(s: &str) -> Option<(String, &str)> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if first != '"' {
        return None;
    }
    let mut out = String::new();
    while let Some((idx, c)) = chars.next() {
        match c {
            '"' => return Some((out, &s[idx + 1..])),
            '\\' => {
                let (_, esc) = chars.next()?;
                match esc {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{0008}'),
                    'f' => out.push('\u{000c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let mut hex = String::with_capacity(4);
                        for _ in 0..4 {
                            if let Some((_, hc)) = chars.next() {
                                hex.push(hc);
                            }
                        }
                        if let Ok(code) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                            }
                        }
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
    }
    None
}

fn skip_balanced(s: &str, open: char, close: char) -> Option<(&str, &str)> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if first != open {
        return None;
    }
    let mut depth = 1usize;
    let mut in_str = false;
    let mut escape = false;
    for (idx, c) in chars {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some((&s[..idx + close.len_utf8()], &s[idx + close.len_utf8()..]));
            }
        }
    }
    None
}

fn extract_nested_data_str(obj: &str) -> Option<String> {
    let mut rest = obj.strip_prefix('{')?.strip_suffix('}')?.trim();
    while !rest.is_empty() {
        let Some((key, after_key)) = parse_json_string(rest) else {
            break;
        };
        let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
        if key == "data" {
            if let Some((val, _)) = parse_json_string(after_colon) {
                return Some(val);
            }
            let num: String = after_colon
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !num.is_empty() {
                return Some(num);
            }
            return None;
        }
        rest = skip_json_value(after_colon)?.trim_start();
        if let Some(r) = rest.strip_prefix(',') {
            rest = r.trim_start();
        }
    }
    None
}

fn skip_json_value(s: &str) -> Option<&str> {
    let s = s.trim_start();
    if s.starts_with('"') {
        Some(parse_json_string(s)?.1)
    } else if s.starts_with('{') {
        Some(skip_balanced(s, '{', '}')?.1)
    } else if s.starts_with('[') {
        Some(skip_balanced(s, '[', ']')?.1)
    } else {
        let end = s
            .find([',', '}', ']', '\n', '\r', ' ', '\t'])
            .unwrap_or(s.len());
        (end > 0).then_some(&s[end..])
    }
}

fn parse_json_notif_object(obj: &str) -> Option<RawNotif> {
    let mut rest = obj.strip_prefix('{')?.strip_suffix('}')?.trim();
    let mut raw = RawNotif::default();
    let mut found_field = false;
    while !rest.is_empty() {
        let Some((key, after_key)) = parse_json_string(rest) else {
            break;
        };
        let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
        let val_str = if after_colon.starts_with('"') {
            parse_json_string(after_colon).map(|(v, _)| v)
        } else if after_colon.starts_with('{') {
            skip_balanced(after_colon, '{', '}')
                .and_then(|(nested, _)| extract_nested_data_str(nested))
        } else if after_colon.starts_with("null") {
            Some(String::new())
        } else {
            let digits: String = after_colon
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            (!digits.is_empty()).then_some(digits)
        };
        match key.as_str() {
            "id" => {
                if let Some(v) = val_str.and_then(|s| s.parse::<u32>().ok()) {
                    raw.id = v;
                    found_field = true;
                }
            }
            "app_name" | "app-name" => {
                if let Some(v) = val_str {
                    raw.app = v;
                    found_field = true;
                }
            }
            "summary" => {
                if let Some(v) = val_str {
                    raw.summary = v;
                    found_field = true;
                }
            }
            "body" => {
                if let Some(v) = val_str {
                    raw.body = v;
                    found_field = true;
                }
            }
            "time" | "timestamp" => {
                if let Some(v) = val_str {
                    raw.time = v;
                }
            }
            _ => {}
        }
        rest = skip_json_value(after_colon)?.trim_start();
        if let Some(r) = rest.strip_prefix(',') {
            rest = r.trim_start();
        }
    }
    (found_field && (!raw.summary.is_empty() || !raw.body.is_empty())).then_some(raw)
}

fn collect_json_objects(s: &str, out: &mut Vec<RawNotif>) {
    let mut idx = 0;
    let bytes = s.as_bytes();
    let mut in_str = false;
    let mut escape = false;
    while idx < bytes.len() {
        let c = bytes[idx] as char;
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            idx += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
            idx += 1;
            continue;
        }
        if c == '{' {
            if let Some((obj, _)) = skip_balanced(&s[idx..], '{', '}') {
                if let Some(notif) = parse_json_notif_object(obj) {
                    out.push(notif);
                    idx += obj.len();
                    continue;
                }
            }
        }
        idx += 1;
    }
}

/// Parses `makoctl list` / `makoctl history` output (JSON `-j`, plain text `Notification N: ...`,
/// or tab-separated lines) into newest-first notifications (up to [`MAX_NOTIFS`]) and `max_id`.
pub fn parse_mako(out: &str, cleared_id: u32) -> (Vec<NotifItem>, u32) {
    let mut raw_list: Vec<RawNotif> = Vec::new();
    if out.contains('{') {
        collect_json_objects(out, &mut raw_list);
    }

    if raw_list.is_empty() {
        let mut current: Option<RawNotif> = None;
        for line in out.lines() {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.trim().is_empty() {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("Notification ") {
                if let Some(prev) = current.take() {
                    raw_list.push(prev);
                }
                let (id_part, sum_part) = rest.split_once(':').unwrap_or((rest, ""));
                let id = id_part.trim().parse::<u32>().unwrap_or(0);
                current = Some(RawNotif {
                    id,
                    summary: sum_part.trim().to_string(),
                    ..RawNotif::default()
                });
            } else if let Some(cur) = current.as_mut() {
                let t = trimmed.trim();
                if let Some(app) = t.strip_prefix("App name:") {
                    cur.app = app.trim().to_string();
                } else if let Some(body) = t.strip_prefix("Body:") {
                    cur.body = body.trim().to_string();
                } else if let Some(time) = t.strip_prefix("Time:") {
                    cur.time = time.trim().to_string();
                } else if cur.app.is_empty() {
                    if let Some(entry) = t.strip_prefix("Desktop entry:") {
                        cur.app = entry.trim().to_string();
                    }
                }
            } else if trimmed.contains('\t') {
                let parts: Vec<&str> = trimmed.split('\t').collect();
                match parts.as_slice() {
                    [id_s, app, summary] => raw_list.push(RawNotif {
                        id: id_s.trim().parse().unwrap_or(0),
                        app: (*app).to_string(),
                        time: String::new(),
                        summary: (*summary).to_string(),
                        body: String::new(),
                    }),
                    [id_s, app, summary, body] => raw_list.push(RawNotif {
                        id: id_s.trim().parse().unwrap_or(0),
                        app: (*app).to_string(),
                        time: String::new(),
                        summary: (*summary).to_string(),
                        body: (*body).to_string(),
                    }),
                    [id_s, app, time, summary, body, ..] => raw_list.push(RawNotif {
                        id: id_s.trim().parse().unwrap_or(0),
                        app: (*app).to_string(),
                        time: (*time).to_string(),
                        summary: (*summary).to_string(),
                        body: (*body).to_string(),
                    }),
                    _ => {}
                }
            }
        }
        if let Some(last) = current.take() {
            raw_list.push(last);
        }
    }

    // Sort newest first when IDs are present before deduplicating
    raw_list.sort_by_key(|a| std::cmp::Reverse(a.id));

    let mut max_id = 0u32;
    let mut items = Vec::new();
    for raw in raw_list {
        push_raw_notif(raw, cleared_id, &mut max_id, &mut items);
        if items.len() >= MAX_NOTIFS {
            break;
        }
    }
    (items, max_id)
}

pub fn body_line_count(body: &str) -> usize {
    if body.is_empty() {
        0
    } else if body.contains('\n') || body.chars().count() > 48 {
        2
    } else {
        1
    }
}

pub fn item_height(item: &NotifItem) -> f64 {
    match body_line_count(&item.body) {
        0 => 36.0,
        1 => 54.0,
        _ => 70.0,
    }
}

/// Notifications from one app stack under one name: the app's line shows on the first of a run
/// (with how many there are), and the others go without it.
pub fn shows_app(items: &[NotifItem], idx: usize) -> bool {
    idx == 0 || items.get(idx - 1).map(|p| &p.app) != items.get(idx).map(|i| &i.app)
}

/// How many in a row come from the app of `idx` (the run it starts).
pub fn run_len(items: &[NotifItem], idx: usize) -> usize {
    items[idx..]
        .iter()
        .take_while(|i| i.app == items[idx].app)
        .count()
}

/// The height of notification `idx` among `items`: without its app's line inside a run.
pub fn item_h(items: &[NotifItem], idx: usize) -> f64 {
    item_height(&items[idx]) - if shows_app(items, idx) { 0.0 } else { APP_LINE }
}

/// The app's line above a notification's title.
pub const APP_LINE: f64 = 16.0;

/// The dismiss x of a notification at `ry` in a card at `x` of width `w` (shown on hover).
pub fn dismiss_rect(x: f64, w: f64, ry: f64) -> Rect {
    (x + w - PAD_X - 16.0, ry - 3.0, 20.0, 20.0)
}

/// Returns `0.0` when there are no notifications so the module hides itself.
pub fn height(world: &World) -> f64 {
    let items = &world.notifications.items;
    if items.is_empty() {
        return 0.0;
    }
    let content_h: f64 = (0..items.len()).map(|i| item_h(items, i)).sum::<f64>()
        + (items.len().saturating_sub(1) as f64) * ITEM_GAP;
    2.0 * PAD_Y + HEAD_H + content_h
}

pub fn clear_all_action(d: &mut NotificationsData) -> String {
    if d.max_id > d.cleared_id {
        d.cleared_id = d.max_id;
    }
    for it in &d.items {
        if it.id > d.cleared_id {
            d.cleared_id = it.id;
        }
    }
    d.items.clear();
    "makoctl dismiss -a 2>/dev/null".to_string()
}

pub fn invoke_item_action(d: &mut NotificationsData, idx: usize) -> Option<String> {
    if idx >= d.items.len() {
        return None;
    }
    let item = d.items.remove(idx);
    if item.id > d.max_id {
        d.max_id = item.id;
    }
    Some(format!(
        "makoctl invoke -n {} default 2>/dev/null; makoctl dismiss -n {} 2>/dev/null",
        item.id, item.id
    ))
}

/// Hitbox of the `"Clear all"` button inside a card of width `w`.
pub fn clear_all_rect(w: f64) -> Rect {
    (w - PAD_X - 72.0, 0.0, 72.0 + PAD_X, PAD_Y + 24.0)
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if !is_left_button(button) || world.notifications.items.is_empty() {
        return false;
    }
    let (w, _) = size;
    let (x, y) = pos;
    let clear = clear_all_rect(w);
    if x >= clear.0 && x <= clear.0 + clear.2 && y >= clear.1 && y <= clear.1 + clear.3 {
        let cmd = clear_all_action(&mut world.notifications);
        world.run(cmd);
        return true;
    }

    let mut ry = PAD_Y + HEAD_H;
    let mut hit_idx = None;
    let items = &world.notifications.items;
    for idx in 0..items.len() {
        let ih = item_h(items, idx);
        if y >= ry && y <= ry + ih && x >= 0.0 && x <= w {
            hit_idx = Some((idx, ry));
            break;
        }
        ry += ih + ITEM_GAP;
    }
    if let Some((idx, ry)) = hit_idx {
        // its x (shown on hover): gone without opening it
        let d = dismiss_rect(0.0, w, ry);
        if x >= d.0 && x <= d.0 + d.2 && y >= d.1 && y <= d.1 + d.3 {
            let item = world.notifications.items.remove(idx);
            world.run(format!("makoctl dismiss -n {} 2>/dev/null", item.id));
            return true;
        }
        if let Some(cmd) = invoke_item_action(&mut world.notifications, idx) {
            world.run(cmd);
            return true;
        }
    }
    false
}

fn draw_body_two_lines(cr: &Context, x: f64, y: f64, max_w: f64, t: &Text) {
    let l = pangocairo::functions::create_layout(cr);
    let mut fd = pango::FontDescription::from_string(FONT);
    fd.set_absolute_size(t.size * pango::SCALE as f64);
    fd.set_weight(pango::Weight::Normal);
    l.set_font_description(Some(&fd));
    if max_w > 0.0 {
        l.set_width((max_w * pango::SCALE as f64) as i32);
        l.set_wrap(pango::WrapMode::WordChar);
        l.set_ellipsize(pango::EllipsizeMode::End);
        l.set_height(-2);
    }
    l.set_text(t.text);
    draw::set(cr, t.color);
    cr.move_to(x.round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 || world.notifications.items.is_empty() {
        return;
    }
    draw::card(cr, th, x, y, w, h);
    draw::title(cr, th, x + PAD_X, y + PAD_Y, "Notifications");
    let clear_t = Text {
        size: 12.0,
        bold: false,
        color: th.muted,
        text: "Clear all",
    };
    let (cw, _) = draw::measure(cr, &clear_t);
    super::hot(
        cr,
        th,
        world,
        x + w - PAD_X - cw - 8.0,
        y + PAD_Y - 3.0,
        cw + 16.0,
        22.0,
        th.small_radius(),
    );
    draw::text_right(cr, x + w - PAD_X, y + PAD_Y + 2.0, &clear_t);

    let iw = (w - 2.0 * PAD_X).max(20.0);
    let mut ry = y + PAD_Y + HEAD_H;
    let items = &world.notifications.items;
    for (idx, item) in items.iter().enumerate() {
        let ih = item_h(items, idx);
        let head = shows_app(items, idx);
        // whole notifications only (the first always, however small the card)
        if idx > 0 && ry + ih > y + h - PAD_Y / 2.0 {
            break;
        }
        // its x first (under the pointer it is lit, not the notification); a click elsewhere
        // opens the notification (and dismisses it)
        let d = dismiss_rect(x, w, ry);
        let x_lit = super::hot(cr, th, world, d.0, d.1, d.2, d.3, th.small_radius());
        let pointed = x_lit
            || world.pointer.is_some_and(|(px, py)| {
                px >= x + PAD_X - 8.0
                    && px < x + w - PAD_X + 8.0
                    && py >= ry - 5.0
                    && py < ry - 5.0 + ih + ITEM_GAP
            });
        super::hot(
            cr,
            th,
            world,
            x + PAD_X - 8.0,
            ry - 5.0,
            iw + 16.0,
            ih + ITEM_GAP,
            th.small_radius(),
        );
        let time_w = if pointed {
            draw_x(cr, th, d, x_lit);
            24.0
        } else if !item.time.is_empty() {
            draw::text_right(
                cr,
                x + w - PAD_X,
                ry,
                &Text {
                    size: 11.0,
                    bold: false,
                    color: th.muted,
                    text: &item.time,
                },
            ) + 8.0
        } else {
            0.0
        };
        let run = if head { run_len(items, idx) } else { 1 };
        let app_line = if run > 1 {
            format!("{} \u{b7} {run}", item.app)
        } else {
            item.app.clone()
        };
        if head {
            draw::text_max(
                cr,
                x + PAD_X,
                ry,
                (iw - time_w).max(20.0),
                &Text {
                    size: 11.0,
                    bold: false,
                    color: th.muted,
                    text: &app_line,
                },
            );
        }
        // inside a run the title takes the app line's place
        let ty = if head { ry } else { ry - APP_LINE };
        let title_w = if head { iw } else { (iw - time_w).max(20.0) };
        draw::text_max(
            cr,
            x + PAD_X,
            ty + 16.0,
            title_w,
            &Text {
                size: 13.0,
                bold: true,
                color: th.text,
                text: &item.title,
            },
        );
        if !item.body.is_empty() {
            draw_body_two_lines(
                cr,
                x + PAD_X,
                ty + 36.0,
                iw,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: &item.body,
                },
            );
        }
        ry += ih + ITEM_GAP;
    }
}

/// The dismiss x in `r`, brighter while pointed at.
fn draw_x(cr: &Context, th: &Theme, r: Rect, lit: bool) {
    let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    draw::set(cr, if lit { th.text } else { th.muted });
    cr.set_line_width(1.5);
    cr.move_to(cx - 4.0, cy - 4.0);
    cr.line_to(cx + 4.0, cy + 4.0);
    cr.move_to(cx + 4.0, cy - 4.0);
    cr.line_to(cx - 4.0, cy + 4.0);
    let _ = cr.stroke();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notif(id: u32, app: &str, body: &str) -> NotifItem {
        NotifItem {
            id,
            app: app.into(),
            title: format!("t{id}"),
            body: body.into(),
            time: String::new(),
        }
    }

    /// A run from one app shows the app once; the others go without its line. The x dismisses
    /// without opening.
    #[test]
    fn one_apps_notifications_stack_and_the_x_dismisses() {
        let items = vec![
            notif(3, "Discord", "hi"),
            notif(2, "Discord", ""),
            notif(1, "Steam", ""),
        ];
        assert!(shows_app(&items, 0) && !shows_app(&items, 1) && shows_app(&items, 2));
        assert_eq!(run_len(&items, 0), 2);
        assert_eq!(item_h(&items, 1), item_height(&items[1]) - APP_LINE);
        let mut world = World::now();
        world.notifications.items = items;
        let w = 380.0;
        let first = PAD_Y + HEAD_H;
        let d = dismiss_rect(0.0, w, first);
        assert!(click(
            &mut world,
            (w, 400.0),
            (d.0 + 10.0, d.1 + 10.0),
            0x110
        ));
        assert_eq!(world.notifications.items.len(), 2);
        assert_eq!(world.notifications.items[0].id, 2, "the first one went");
        assert!(world
            .actions
            .iter()
            .any(|a| a.contains("makoctl dismiss -n 3")));
        assert!(
            !world.actions.iter().any(|a| a.contains("invoke")),
            "not opened"
        );
    }

    #[test]
    fn parses_real_makoctl_json_and_text_outputs_newest_first() {
        let mako_json = r#"[
  {
    "id": 11,
    "app_name": "Steam",
    "app_icon": null,
    "category": null,
    "desktop_entry": "steam",
    "summary": "Download complete",
    "body": "Download complete",
    "urgency": "normal",
    "actions": {}
  },
  {
    "id": 14,
    "app_name": "Discord",
    "app_icon": "discord",
    "category": "im.received",
    "desktop_entry": "discord",
    "summary": "New message from a friend",
    "body": "Hey, are we still on for tonight? Let me know when you are ready to join.",
    "urgency": "normal",
    "actions": {
      "default": "Open"
    }
  }
]
[
  {
    "id": 9,
    "app_name": "",
    "summary": "Infynitae",
    "body": "Screen is being captured",
    "urgency": "low",
    "actions": {}
  },
  {
    "id": 14,
    "app_name": "Discord",
    "summary": "New message from a friend",
    "body": "Duplicate in history",
    "urgency": "normal",
    "actions": {}
  }
]"#;
        let (items, max_id) = parse_mako(mako_json, 0);
        assert_eq!(max_id, 14);
        assert_eq!(items.len(), 3);

        // Newest first (id 14, then 11, then 9)
        assert_eq!(items[0].id, 14);
        assert_eq!(items[0].app, "Discord");
        assert_eq!(items[0].title, "New message from a friend");
        assert_eq!(
            items[0].body,
            "Hey, are we still on for tonight? Let me know when you are ready to join."
        );
        assert_eq!(body_line_count(&items[0].body), 2);

        // Identical summary and body collapses body to empty
        assert_eq!(items[1].id, 11);
        assert_eq!(items[1].app, "Steam");
        assert_eq!(items[1].title, "Download complete");
        assert_eq!(items[1].body, "");

        // Empty app_name defaults to "Notify"
        assert_eq!(items[2].id, 9);
        assert_eq!(items[2].app, "Notify");
        assert_eq!(items[2].title, "Infynitae");
        assert_eq!(items[2].body, "Screen is being captured");

        // Plain-text makoctl list output also parses
        let mako_text = "\
Notification 21: Build finished
  App name: Cargo
  Urgency: normal
  Actions:
    default: Open
Notification 19: Low battery
  App name: Power
  Body: 15% remaining
  Urgency: critical
";
        let (text_items, max_id2) = parse_mako(mako_text, 0);
        assert_eq!(max_id2, 21);
        assert_eq!(text_items.len(), 2);
        assert_eq!(text_items[0].id, 21);
        assert_eq!(text_items[0].app, "Cargo");
        assert_eq!(text_items[0].title, "Build finished");
        assert_eq!(text_items[1].id, 19);
        assert_eq!(text_items[1].body, "15% remaining");
    }

    #[test]
    fn notifications_click_invokes_default_and_clear_all_hides_module() {
        let mut world = World::now();
        assert_eq!(height(&world), 0.0);

        apply(
            &mut world.notifications,
            "notifications",
            "15\tDiscord\tNow\tNew message\tHello there\n12\tSteam\t5m\tDownload complete\t\n",
        );
        let w = 380.0;
        let h = height(&world);
        assert!(h > 100.0);

        // Click first notification item (id 15)
        let item0_y = PAD_Y + HEAD_H + 10.0;
        assert!(click(&mut world, (w, h), (w / 2.0, item0_y), 1));
        assert_eq!(world.notifications.items.len(), 1);
        assert_eq!(world.notifications.items[0].id, 12);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("makoctl invoke -n 15 default 2>/dev/null; makoctl dismiss -n 15 2>/dev/null")
        );

        // Click "Clear all" on the right of the header
        let clear = clear_all_rect(w);
        assert!(click(&mut world, (w, h), (clear.0 + 10.0, PAD_Y + 8.0), 1));
        assert!(world.notifications.items.is_empty());
        assert_eq!(height(&world), 0.0);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("makoctl dismiss -a 2>/dev/null")
        );
    }

    #[test]
    fn invoke_single_notification_action_preserves_older_unread_notifications() {
        let mut d = NotificationsData::default();
        let raw = "\
Notification 12: Newer
  App name: App
Notification 11: Middle
  App name: App
Notification 10: Older
  App name: App
";
        let (items, max_id) = parse_mako(raw, d.cleared_id);
        d.items = items;
        d.max_id = max_id;
        assert_eq!(d.items.len(), 3);
        assert_eq!(d.cleared_id, 0);

        // Invoke action on newest notification (id 12, index 0)
        let act = invoke_item_action(&mut d, 0);
        assert!(act.is_some());
        assert_eq!(
            d.cleared_id, 0,
            "cleared_id should only update on Clear all"
        );

        // Next poll returns remaining active notifications 11 and 10
        let remaining_raw = "\
Notification 11: Middle
  App name: App
Notification 10: Older
  App name: App
";
        let (new_items, _) = parse_mako(remaining_raw, d.cleared_id);
        assert_eq!(
            new_items.len(),
            2,
            "older notifications must not be dropped"
        );
        assert_eq!(new_items[0].id, 11);
        assert_eq!(new_items[1].id, 10);
    }
}
