//! The `clipboard` module: title `"Clipboard"` with `"Super+V"` muted on the right, history from
//! `cliphist list` (text cut with an ellipsis, images as `"Image WxH"`, time muted on the right,
//! pinned items first with a pin icon in the accent), and click to copy back and close the panel.

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{is_left_button, Rect, World};
use crate::theme::{Rgba, Theme};
use cairo::Context;

pub const MAX_ITEMS: usize = 24;
pub const VISIBLE_ROWS: usize = 8;
pub const HEAD_H: f64 = 28.0;
pub const ROW_H: f64 = 28.0;

pub const CMDS: &[(&str, &str)] = &[("clipboard", "cliphist list 2>/dev/null | head -n 24")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Link,
    Image,
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipItem {
    pub raw_line: String,
    pub preview: String,
    pub time: String,
    pub is_image: bool,
    pub pinned: bool,
}

/// Returns true when `s` is an http, https or ftp URL alone on the row.
pub fn is_url_alone(s: &str) -> bool {
    let t = s.trim();
    if t.contains(char::is_whitespace) {
        return false;
    }
    let rest = if t.len() >= 8 && t[..8].eq_ignore_ascii_case("https://") {
        &t[8..]
    } else if t.len() >= 7 && t[..7].eq_ignore_ascii_case("http://") {
        &t[7..]
    } else if t.len() >= 6 && t[..6].eq_ignore_ascii_case("ftp://") {
        &t[6..]
    } else {
        return false;
    };
    !rest.is_empty()
}

/// Returns the clipboard entry kind: link (URL alone on the row), image (`is_image`), or text.
pub fn kind_of(entry: &ClipItem) -> ClipKind {
    if entry.is_image {
        ClipKind::Image
    } else if is_url_alone(&entry.preview) {
        ClipKind::Link
    } else {
        ClipKind::Text
    }
}

#[derive(Clone, Debug, Default)]
pub struct ClipboardData {
    pub ready: bool,
    pub items: Vec<ClipItem>,
    pub scroll: usize,
}

pub fn apply(d: &mut ClipboardData, key: &str, out: &str) -> bool {
    if key != "clipboard" {
        return false;
    }
    d.items = parse_cliphist(out);
    if d.scroll >= d.items.len() {
        d.scroll = 0;
    }
    d.ready = true;
    true
}

/// Normalises whitespace and shortens `$HOME` to `~` (matching the C edge's `sanitize_inline_text`).
pub fn sanitize_inline_text(src: &str) -> String {
    let home = std::env::var("HOME").ok().filter(|h| h.len() > 1);
    sanitize_inline_text_with_home(src, home.as_deref())
}

pub fn sanitize_inline_text_with_home(src: &str, home: Option<&str>) -> String {
    let mut collapsed = String::with_capacity(src.len().min(256));
    let mut prev_space = true;
    for c in src.chars() {
        if collapsed.len() >= 255 {
            break;
        }
        if matches!(c, '\n' | '\r' | '\t' | ' ') {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else if !c.is_control() {
            collapsed.push(c);
            prev_space = false;
        }
    }
    let trimmed = collapsed.trim_end_matches(' ');
    let Some(h) = home.filter(|h| h.len() > 1) else {
        return trimmed.to_string();
    };
    let mut out = String::with_capacity(trimmed.len());
    let mut rest = trimmed;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(h) {
            if after.is_empty() || after.starts_with('/') || after.starts_with(' ') {
                out.push('~');
                rest = after;
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

fn is_dimensions_token(tok: &str) -> bool {
    let Some((w, h)) = tok.split_once('x') else {
        return false;
    };
    !w.is_empty()
        && !h.is_empty()
        && w.chars().all(|c| c.is_ascii_digit())
        && h.chars().all(|c| c.is_ascii_digit())
}

/// Formats `[[ binary data ... WxH ]]` from `cliphist list` as `Some("Image WxH")`.
pub fn parse_binary_image(body: &str) -> Option<String> {
    let t = body.trim();
    if let Some(inner) = t
        .strip_prefix("[[ binary data")
        .and_then(|s| s.strip_suffix("]]"))
    {
        for tok in inner.split_whitespace().rev() {
            if is_dimensions_token(tok) {
                return Some(format!("Image {tok}"));
            }
        }
        return Some("Image".to_string());
    }
    if let Some(dims) = t.strip_prefix("Image ") {
        if is_dimensions_token(dims.trim()) {
            return Some(format!("Image {}", dims.trim()));
        }
    }
    None
}

/// Parses `cliphist list` output into up to [`MAX_ITEMS`] items, placing pinned items first.
pub fn parse_cliphist(out: &str) -> Vec<ClipItem> {
    let home = std::env::var("HOME").ok().filter(|h| h.len() > 1);
    parse_cliphist_with_home(out, home.as_deref())
}

pub fn parse_cliphist_with_home(out: &str, home: Option<&str>) -> Vec<ClipItem> {
    let mut items = Vec::new();
    for raw in out.lines() {
        let line = raw.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        if items.len() >= MAX_ITEMS {
            break;
        }
        let (pinned, line_rest) = if let Some(r) = line.strip_prefix("pinned:") {
            (true, r)
        } else if let Some(r) = line.strip_prefix("pin:") {
            (true, r)
        } else if let Some(r) = line.strip_prefix('*') {
            (true, r)
        } else {
            (false, line)
        };

        let (raw_line, body, time) = match line_rest.split_once('\t') {
            Some((id, after_id)) => match after_id.split_once('\t') {
                Some((b, t)) => (format!("{id}\t{b}"), b, t.trim().to_string()),
                None => (line_rest.to_string(), after_id, String::new()),
            },
            None => (line_rest.to_string(), line_rest, String::new()),
        };

        let (preview, is_image) = if let Some(img) = parse_binary_image(body) {
            (img, true)
        } else {
            let cleaned = sanitize_inline_text_with_home(body, home);
            let p = if cleaned.is_empty() {
                "(Clipboard item)".to_string()
            } else {
                cleaned
            };
            (p, false)
        };

        items.push(ClipItem {
            raw_line,
            preview,
            time,
            is_image,
            pinned,
        });
    }
    items.sort_by_key(|it| !it.pinned);
    items
}

fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Builds the command that decodes `raw_line` via `cliphist decode` and copies it to `wl-copy`.
pub fn copy_command(raw_line: &str) -> String {
    format!(
        "printf '%s\\n' {} | cliphist decode 2>/dev/null | wl-copy",
        shell_single_quote(raw_line)
    )
}

/// Builds the command that deletes `raw_line` from `cliphist`.
pub fn delete_command(raw_line: &str) -> String {
    format!(
        "printf '%s\\n' {} | cliphist delete 2>/dev/null",
        shell_single_quote(raw_line)
    )
}

pub fn visible_count(d: &ClipboardData) -> usize {
    d.items.len().min(VISIBLE_ROWS)
}

pub fn height(world: &World) -> f64 {
    let rows = visible_count(&world.clipboard).max(1);
    2.0 * PAD_Y + HEAD_H + (rows as f64) * ROW_H
}

fn item_index_at(d: &ClipboardData, size: (f64, f64), pos: (f64, f64)) -> Option<usize> {
    let (w, h) = size;
    let (x, y) = pos;
    if x < 0.0 || x > w || y < PAD_Y + HEAD_H || y > h - PAD_Y + 4.0 {
        return None;
    }
    let row = ((y - (PAD_Y + HEAD_H)) / ROW_H).floor() as usize;
    let shown = visible_count(d);
    if row >= shown {
        return None;
    }
    let idx = d.scroll.min(d.items.len().saturating_sub(shown)) + row;
    (idx < d.items.len()).then_some(idx)
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    let Some(idx) = item_index_at(&world.clipboard, size, pos) else {
        return false;
    };
    if is_left_button(button) {
        let raw_line = world.clipboard.items[idx].raw_line.clone();
        let cmd = copy_command(&raw_line);
        world.close_requested = true;
        world.run(cmd);
        true
    } else if button == 2 || button == 3 || button == 0x111 || button == 0x112 {
        let removed = world.clipboard.items.remove(idx);
        if world.clipboard.scroll > 0
            && world.clipboard.scroll + VISIBLE_ROWS > world.clipboard.items.len()
        {
            world.clipboard.scroll = world.clipboard.items.len().saturating_sub(VISIBLE_ROWS);
        }
        let cmd = delete_command(&removed.raw_line);
        world.run(cmd);
        true
    } else {
        false
    }
}

pub fn scroll(world: &mut World, _size: (f64, f64), _pos: (f64, f64), dy: f64) -> bool {
    let len = world.clipboard.items.len();
    if len <= VISIBLE_ROWS || dy.abs() < 0.01 {
        return false;
    }
    let max_scroll = len - VISIBLE_ROWS;
    let next = if dy > 0.0 {
        (world.clipboard.scroll + 1).min(max_scroll)
    } else {
        world.clipboard.scroll.saturating_sub(1)
    };
    if next != world.clipboard.scroll {
        world.clipboard.scroll = next;
        true
    } else {
        false
    }
}

/// The hint for the clipboard card title: its shortcut key from config (default `"Super+V"`),
/// or `None` when no key is configured.
pub fn hint(world: &World) -> Option<String> {
    crate::config::format_key(world.clipboard_key.or(world.panel_key))
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    draw::card(cr, th, x, y, w, h);
    draw::title(cr, th, x + PAD_X, y + PAD_Y, "Clipboard");
    if let Some(h_text) = hint(world) {
        draw::title_hint(cr, th, x, y, w, "Clipboard", &h_text);
    }

    let d = &world.clipboard;
    let top = y + PAD_Y + HEAD_H;
    if d.items.is_empty() {
        draw::text(
            cr,
            x + PAD_X,
            top + 5.0,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: "Clipboard is empty",
            },
        );
        return;
    }

    let max_rows = ((h - 2.0 * PAD_Y - HEAD_H) / ROW_H).floor().max(1.0) as usize;
    let shown = d.items.len().min(VISIBLE_ROWS).min(max_rows);
    let start = d.scroll.min(d.items.len().saturating_sub(shown));
    for (row, item) in d.items[start..start + shown].iter().enumerate() {
        let ry = (top + (row as f64) * ROW_H).round();
        super::hot(
            cr,
            th,
            world,
            x + PAD_X - 8.0,
            ry,
            w - 2.0 * PAD_X + 16.0,
            ROW_H - 2.0,
            th.small_radius(),
        );
        let mut tx = x + PAD_X;
        if item.pinned {
            draw_pin_icon(cr, tx, ry + 5.0, th.accent);
        } else {
            draw_icon(cr, kind_of(item), tx, ry + 5.0, th.muted);
        }
        tx += 24.0;
        let time_w = if !item.time.is_empty() {
            draw::text_right(
                cr,
                x + w - PAD_X,
                ry + 7.0,
                &Text {
                    size: 11.0,
                    bold: false,
                    color: th.muted,
                    text: &item.time,
                },
            ) + 10.0
        } else {
            0.0
        };
        let max_w = (x + w - PAD_X - time_w - tx).max(20.0);
        draw::text_max(
            cr,
            tx,
            ry + 5.0,
            max_w,
            &Text {
                size: 13.0,
                bold: false,
                color: th.text,
                text: &item.preview,
            },
        );
    }
}

/// Draws an 18x18 stroked pin icon at `(x, y)` in `color`.
pub fn draw_pin_icon(cr: &Context, x: f64, y: f64, color: Rgba) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    let cx = x + 9.0;
    let cy = y + 9.0;
    // Pin head and body
    cr.move_to(cx - 3.5, cy - 6.0);
    cr.line_to(cx + 3.5, cy - 6.0);
    cr.move_to(cx - 2.5, cy - 6.0);
    cr.line_to(cx - 2.5, cy - 1.5);
    cr.line_to(cx - 5.0, cy + 1.5);
    cr.line_to(cx + 5.0, cy + 1.5);
    cr.line_to(cx + 2.5, cy - 1.5);
    cr.line_to(cx + 2.5, cy - 6.0);
    // Pin needle
    cr.move_to(cx, cy + 1.5);
    cr.line_to(cx, cy + 7.0);
    let _ = cr.stroke();
}

/// Draws a 14 px stroked line icon at `(x, y)` in `color` for clipboard kind `kind`.
pub fn draw_icon(cr: &Context, kind: ClipKind, x: f64, y: f64, color: Rgba) {
    draw::set(cr, color);
    cr.set_line_width(1.5);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    let cx = x + 9.0;
    let cy = y + 9.0;
    match kind {
        ClipKind::Link => {
            let _ = cr.save();
            cr.translate(cx, cy);
            cr.rotate(-std::f64::consts::FRAC_PI_4);
            // Left loop
            cr.new_sub_path();
            cr.move_to(-1.0, 2.2);
            cr.line_to(-2.5, 2.2);
            cr.arc(
                -2.5,
                0.0,
                2.2,
                0.5 * std::f64::consts::PI,
                1.5 * std::f64::consts::PI,
            );
            cr.line_to(-1.0, -2.2);
            // Right loop
            cr.new_sub_path();
            cr.move_to(1.0, -2.2);
            cr.line_to(2.5, -2.2);
            cr.arc(
                2.5,
                0.0,
                2.2,
                -0.5 * std::f64::consts::PI,
                0.5 * std::f64::consts::PI,
            );
            cr.line_to(1.0, 2.2);
            // Center connecting bar
            cr.move_to(-1.5, 0.0);
            cr.line_to(1.5, 0.0);
            let _ = cr.stroke();
            let _ = cr.restore();
        }
        ClipKind::Image => {
            draw::round_rect(cr, cx - 6.0, cy - 4.5, 12.0, 9.0, 1.5);
            let _ = cr.stroke();
            // Sun dot
            cr.new_sub_path();
            cr.arc(cx + 2.5, cy - 1.8, 0.9, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
            // Mountain peaks
            cr.move_to(cx - 4.5, cy + 2.5);
            cr.line_to(cx - 1.5, cy - 0.8);
            cr.line_to(cx + 1.2, cy + 2.0);
            cr.line_to(cx + 3.0, cy + 0.6);
            cr.line_to(cx + 4.5, cy + 2.0);
            let _ = cr.stroke();
        }
        ClipKind::Text => {
            cr.move_to(cx - 5.0, cy - 3.5);
            cr.line_to(cx + 5.0, cy - 3.5);
            cr.move_to(cx - 5.0, cy);
            cr.line_to(cx + 5.0, cy);
            cr.move_to(cx - 5.0, cy + 3.5);
            cr.line_to(cx + 1.0, cy + 3.5);
            let _ = cr.stroke();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_cliphist_output_with_images_home_and_pinned() {
        let sample = "\
108\tgit push origin main
107\t[[ binary data 24 KiB png 1920x1080 ]]
pin:106\t/home/alice/projects/infynitae/live/launch.sh\t2m
105\t[[ binary data 972 B jpeg 640x480 ]]\t5m
104\t   \t  
";
        let items = parse_cliphist_with_home(sample, Some("/home/alice"));
        assert_eq!(items.len(), 5);

        // Pinned item comes first
        assert!(items[0].pinned);
        assert_eq!(
            items[0].raw_line,
            "106\t/home/alice/projects/infynitae/live/launch.sh"
        );
        assert_eq!(items[0].preview, "~/projects/infynitae/live/launch.sh");
        assert_eq!(items[0].time, "2m");
        assert!(!items[0].is_image);

        // Regular text item
        assert!(!items[1].pinned);
        assert_eq!(items[1].raw_line, "108\tgit push origin main");
        assert_eq!(items[1].preview, "git push origin main");

        // Binary PNG image formatted as "Image WxH"
        assert!(items[2].is_image);
        assert_eq!(items[2].preview, "Image 1920x1080");
        assert_eq!(
            items[2].raw_line,
            "107\t[[ binary data 24 KiB png 1920x1080 ]]"
        );

        // Binary JPEG image with time
        assert!(items[3].is_image);
        assert_eq!(items[3].preview, "Image 640x480");
        assert_eq!(items[3].time, "5m");

        // Whitespace-only body falls back to "(Clipboard item)"
        assert_eq!(items[4].preview, "(Clipboard item)");
    }

    #[test]
    fn clipboard_click_copies_and_closes_or_deletes_on_right_click() {
        let mut world = World::now();
        apply(
            &mut world.clipboard,
            "clipboard",
            "108\tgit push origin main\n107\t[[ binary data 24 KiB png 1920x1080 ]]\n",
        );
        let w = 560.0;
        let h = height(&world);

        // Left click on first row copies item 0 and requests closing the panel
        let row0_y = PAD_Y + HEAD_H + ROW_H * 0.5;
        assert!(click(&mut world, (w, h), (w / 2.0, row0_y), 1));
        assert!(world.close_requested);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("printf '%s\\n' '108\tgit push origin main' | cliphist decode 2>/dev/null | wl-copy")
        );

        // Right click on second row deletes item 1 from cliphist and removes it from list
        world.close_requested = false;
        let row1_y = PAD_Y + HEAD_H + ROW_H * 1.5;
        assert!(click(&mut world, (w, h), (w / 2.0, row1_y), 3));
        assert!(!world.close_requested);
        assert_eq!(world.clipboard.items.len(), 1);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("printf '%s\\n' '107\t[[ binary data 24 KiB png 1920x1080 ]]' | cliphist delete 2>/dev/null")
        );
    }

    #[test]
    fn clipboard_hint_reads_key_from_config_and_omits_when_none() {
        let mut world = World::now();
        world.clipboard_key = Some('v');
        assert_eq!(hint(&world), Some("Super+V".to_string()));

        world.clipboard_key = Some('c');
        assert_eq!(hint(&world), Some("Super+C".to_string()));

        world.clipboard_key = None;
        world.panel_key = None;
        assert_eq!(hint(&world), None);

        // Falls back to panel_key when clipboard_key is None
        world.panel_key = Some('x');
        assert_eq!(hint(&world), Some("Super+X".to_string()));
    }

    #[test]
    fn clipboard_draw_drops_hint_when_card_too_narrow() {
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 100, 100).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let mut world = World::now();
        world.clipboard_key = Some('v');

        // Narrow width 100.0 drops hint without overlapping title
        draw(&cr, &th, &world, (0.0, 0.0, 100.0, 100.0));

        // Normal width 380.0 draws hint
        let surf_wide = cairo::ImageSurface::create(cairo::Format::ARgb32, 380, 100).unwrap();
        let cr_wide = cairo::Context::new(&surf_wide).unwrap();
        draw(&cr_wide, &th, &world, (0.0, 0.0, 380.0, 100.0));
    }

    #[test]
    fn kind_of_distinguishes_links_images_and_text() {
        let item = |preview: &str, is_image: bool| ClipItem {
            raw_line: format!("1\t{preview}"),
            preview: preview.to_string(),
            time: "1m".to_string(),
            is_image,
            pinned: false,
        };

        // A link: http, https, ftp URL alone on the row
        assert_eq!(
            kind_of(&item("https://github.com/vysaely/infynitae", false)),
            ClipKind::Link
        );
        assert_eq!(
            kind_of(&item("http://localhost:8080/index.html", false)),
            ClipKind::Link
        );
        assert_eq!(
            kind_of(&item("ftp://ftp.example.org/pub/file.tar.gz", false)),
            ClipKind::Link
        );
        assert_eq!(
            kind_of(&item("HTTPS://EXAMPLE.COM/DOCS", false)),
            ClipKind::Link
        );

        // A URL inside a sentence is text
        assert_eq!(
            kind_of(&item("see https://example.com for details", false)),
            ClipKind::Text
        );
        assert_eq!(
            kind_of(&item("https://example.com is our site", false)),
            ClipKind::Text
        );
        assert_eq!(
            kind_of(&item("download from ftp://ftp.gnu.org now", false)),
            ClipKind::Text
        );

        // An image
        assert_eq!(kind_of(&item("Image 1920x1080", true)), ClipKind::Image);
        assert_eq!(kind_of(&item("Image 640x480", true)), ClipKind::Image);
        assert_eq!(kind_of(&item("Image", true)), ClipKind::Image);

        // Plain text
        assert_eq!(
            kind_of(&item("git push origin main", false)),
            ClipKind::Text
        );
        assert_eq!(kind_of(&item("(Clipboard item)", false)), ClipKind::Text);
        assert_eq!(kind_of(&item("https://", false)), ClipKind::Text);
        assert_eq!(kind_of(&item("http://", false)), ClipKind::Text);
        assert_eq!(kind_of(&item("ftp://", false)), ClipKind::Text);
        assert_eq!(
            kind_of(&item("file:///home/user/test.png", false)),
            ClipKind::Text
        );
    }

    #[test]
    fn clipboard_draw_renders_kind_icons_and_keeps_pin_for_pinned_rows() {
        let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 380, 200).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let mut world = World::now();
        world.clipboard.items = vec![
            ClipItem {
                raw_line: "1\thttps://example.com".into(),
                preview: "https://example.com".into(),
                time: "1m".into(),
                is_image: false,
                pinned: true, // Pinned row keeps pin icon even though it's a link
            },
            ClipItem {
                raw_line: "2\thttps://github.com/vysaely/infynitae".into(),
                preview: "https://github.com/vysaely/infynitae".into(),
                time: "2m".into(),
                is_image: false,
                pinned: false, // Unpinned link shows link icon
            },
            ClipItem {
                raw_line: "3\t[[ binary data png 1920x1080 ]]".into(),
                preview: "Image 1920x1080".into(),
                time: "3m".into(),
                is_image: true,
                pinned: false, // Unpinned image shows image icon
            },
            ClipItem {
                raw_line: "4\tgit push origin main".into(),
                preview: "git push origin main".into(),
                time: "4m".into(),
                is_image: false,
                pinned: false, // Unpinned text shows text icon
            },
        ];

        draw(&cr, &th, &world, (0.0, 0.0, 380.0, 200.0));
        drop(cr);

        // Surface has drawn contents (non-zero alpha pixels)
        let data = surf.data().unwrap();
        assert!(data.iter().any(|&b| b > 0));
    }

    #[test]
    fn draw_icon_renders_pixels_for_all_kinds() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        for kind in [ClipKind::Link, ClipKind::Image, ClipKind::Text] {
            let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 24, 28).unwrap();
            let cr = cairo::Context::new(&surf).unwrap();
            draw_icon(&cr, kind, 0.0, 5.0, th.muted);
            drop(cr);
            let data = surf.data().unwrap();
            let drawn_pixels = data.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count();
            assert!(
                drawn_pixels > 10,
                "kind {kind:?} should draw visible line strokes, got {drawn_pixels} pixels"
            );
        }

        // Pin icon also draws visible pixels
        let mut surf_pin = cairo::ImageSurface::create(cairo::Format::ARgb32, 24, 28).unwrap();
        let cr_pin = cairo::Context::new(&surf_pin).unwrap();
        draw_pin_icon(&cr_pin, 0.0, 5.0, th.accent);
        drop(cr_pin);
        let data_pin = surf_pin.data().unwrap();
        let pin_pixels = data_pin
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0)
            .count();
        assert!(pin_pixels > 10, "pin should draw visible strokes");
    }
}
