//! Clipboard history (`clipboard/history`): `cliphist list` items with click-to-copy and
//! right/middle-click delete.

use std::any::Any;
use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::os::unix::fs::PermissionsExt;

use crate::card::{
    parse_render_data, run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status,
};
use crate::cards::settings::sq;
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::theme::Rgba;
use crate::ui;

const MAX_ITEMS: usize = 24;
const MEDIUM_ROWS: usize = 8;
const LARGE_ROWS: usize = 16;
const ROW_H: f64 = ui::LIST_ROW_H;
const CMD: &str = "cliphist list 2>/dev/null | head -n 24";
const MISSING_MSG: &str = "cliphist is not installed";

pub struct Clipboard;

const KINDS: &[CardKind] = &[CardKind {
    id: "history",
    title: "History",
    about: "Recent items copied to the clipboard",
    sizes: &[Size::M, Size::L],
    default: Size::M,
    half: false,
    sources: &["clipboard"],
}];

impl Provider for Clipboard {
    fn id(&self) -> &'static str {
        "clipboard"
    }

    fn name(&self) -> &'static str {
        "Clipboard"
    }

    fn claims(&self) -> &'static [Claim] {
        &[]
    }

    fn listed(&self) -> Listed {
        Listed::Always
    }

    fn cards(&self) -> &'static [CardKind] {
        KINDS
    }

    fn card(&self, kind: &str, _argument: Option<&str>) -> Option<Box<dyn Card>> {
        (kind == "history").then(|| Box::new(HistoryCard::default()) as Box<dyn Card>)
    }

    fn source(&self, key: &str) -> Option<Box<dyn Source>> {
        (key == "clipboard").then(|| Box::new(ClipboardSource::default()) as Box<dyn Source>)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipKind {
    Text,
    Image,
    Link,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ClipItem {
    raw_line: String,
    preview: String,
    time: String,
    is_image: bool,
    pinned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ClipboardData {
    installed: bool,
    items: Vec<ClipItem>,
}

#[derive(Default)]
struct ClipboardSource {
    /// Optional `PATH` override so tests can check binary detection without env races.
    path: Option<OsString>,
}

impl Source for ClipboardSource {
    fn fetch(&mut self) -> Data {
        if let (None, Some(render_file)) =
            (&self.path, std::env::var_os("INFYNITAE_EDGE_RENDER_DATA"))
        {
            let has_key = std::fs::read_to_string(&render_file)
                .ok()
                .is_some_and(|s| parse_render_data(&s).contains_key("clipboard"));
            if !has_key {
                return Box::new(ClipboardData {
                    installed: false,
                    items: Vec::new(),
                });
            }
            let out = run_sh("clipboard", CMD);
            return Box::new(ClipboardData {
                installed: true,
                items: parse_cliphist(&out),
            });
        }

        let installed = match &self.path {
            Some(p) => has_executable("cliphist", p),
            None => std::env::var_os("PATH")
                .as_deref()
                .is_some_and(|p| has_executable("cliphist", p)),
        };
        if !installed {
            return Box::new(ClipboardData {
                installed: false,
                items: Vec::new(),
            });
        }

        let out = run_sh("clipboard", CMD);
        Box::new(ClipboardData {
            installed: true,
            items: parse_cliphist(&out),
        })
    }
}

fn has_executable(bin: &str, path: &OsStr) -> bool {
    std::env::split_paths(path).any(|dir| {
        std::fs::metadata(dir.join(bin))
            .is_ok_and(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
    })
}

fn is_url_alone(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
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

fn kind_of(item: &ClipItem) -> ClipKind {
    if item.is_image {
        ClipKind::Image
    } else if is_url_alone(&item.preview) {
        ClipKind::Link
    } else {
        ClipKind::Text
    }
}

fn sanitize_inline_text_with_home(src: &str, home: Option<&str>) -> String {
    let mut collapsed = String::with_capacity(src.len().min(255));
    let mut prev_space = true;
    for ch in src.chars() {
        if matches!(ch, ' ' | '\t' | '\n' | '\r') || ch.is_whitespace() {
            if !prev_space {
                if collapsed.len() >= 255 {
                    break;
                }
                collapsed.push(' ');
                prev_space = true;
            }
        } else if !ch.is_control() {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            if collapsed.len() + s.len() > 255 {
                break;
            }
            collapsed.push(ch);
            prev_space = false;
        }
    }
    if collapsed.ends_with(' ') {
        collapsed.pop();
    }

    let Some(h) = home
        .map(|h| h.trim_end_matches('/'))
        .filter(|h| h.len() > 1)
    else {
        return collapsed;
    };
    let mut out = String::with_capacity(collapsed.len());
    let bytes = collapsed.as_bytes();
    let mut i = 0;
    while i < collapsed.len() {
        let at_start = i == 0 || bytes[i - 1] == b' ';
        if at_start && collapsed[i..].starts_with(h) {
            let after = i + h.len();
            let valid_end = after == collapsed.len() || matches!(bytes[after], b'/' | b' ');
            if valid_end {
                out.push('~');
                i = after;
                continue;
            }
        }
        let ch = collapsed[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_dims(tok: &str) -> bool {
    tok.split_once('x').is_some_and(|(w, h)| {
        !w.is_empty()
            && !h.is_empty()
            && w.bytes().all(|b| b.is_ascii_digit())
            && h.bytes().all(|b| b.is_ascii_digit())
    })
}

fn parse_binary_image(body: &str) -> Option<String> {
    let t = body.trim();
    if let Some(inner) = t
        .strip_prefix("[[ binary data")
        .and_then(|s| s.strip_suffix("]]"))
    {
        for tok in inner.split_whitespace() {
            if is_dims(tok) {
                return Some(format!("Image {tok}"));
            }
        }
        return Some("Image".to_string());
    }
    if let Some(dims) = t.strip_prefix("Image ") {
        let d = dims.trim();
        if is_dims(d) {
            return Some(format!("Image {d}"));
        }
    }
    None
}

fn parse_cliphist(out: &str) -> Vec<ClipItem> {
    let home = std::env::var("HOME").ok().filter(|h| h.len() > 1);
    parse_cliphist_with_home(out, home.as_deref())
}

fn parse_cliphist_with_home(out: &str, home: Option<&str>) -> Vec<ClipItem> {
    let mut items = Vec::new();
    for raw in out.lines() {
        let line = raw.trim_end_matches('\r');
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
            (true, r.trim_start())
        } else {
            (false, line)
        };
        let Some((id, rest)) = line_rest.split_once('\t') else {
            continue;
        };
        if id.trim().is_empty() {
            continue;
        }
        let (raw_line, body, time) = match rest.rsplit_once('\t') {
            Some((b, t)) => (format!("{id}\t{b}"), b, t.trim().to_string()),
            None => (line_rest.to_string(), rest, String::new()),
        };
        let img = parse_binary_image(body);
        let is_image = img.is_some();
        let preview = match img {
            Some(label) => label,
            None => {
                let s = sanitize_inline_text_with_home(body, home);
                if s.is_empty() {
                    "(Clipboard item)".to_string()
                } else {
                    s
                }
            }
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

fn copy_command(raw_line: &str) -> String {
    format!(
        "printf '%s\\n' {} | cliphist decode 2>/dev/null | wl-copy",
        sq(raw_line)
    )
}

fn delete_command(raw_line: &str) -> String {
    format!(
        "printf '%s\\n' {} | cliphist delete 2>/dev/null",
        sq(raw_line)
    )
}

#[derive(Default)]
struct HistoryCard {
    data: Option<ClipboardData>,
    scroll: usize,
}

impl HistoryCard {
    fn visible_limit(size: Size) -> usize {
        match size {
            Size::L => LARGE_ROWS,
            _ => MEDIUM_ROWS,
        }
    }

    fn visible_range_for(d: &ClipboardData, size: Size, scroll: usize) -> Range<usize> {
        let n = d.items.len();
        let limit = Self::visible_limit(size);
        let max_scroll = n.saturating_sub(limit);
        let start = scroll.min(max_scroll);
        let end = (start + limit).min(n);
        start..end
    }
}

impl Card for HistoryCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        if key != "clipboard" {
            return false;
        }
        let Some(next) = data.downcast_ref::<ClipboardData>() else {
            return false;
        };
        if self.data.as_ref() == Some(next) {
            return false;
        }
        self.scroll = self
            .scroll
            .min(next.items.len().saturating_sub(MEDIUM_ROWS));
        self.data = Some(next.clone());
        true
    }

    fn status(&self, _cx: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if !d.installed => Status::Missing(MISSING_MSG.into()),
            Some(_) => Status::Ready,
        }
    }

    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        match &self.data {
            None => ROW_H,
            Some(d) if !d.installed || d.items.is_empty() => ROW_H,
            Some(d) => Self::visible_range_for(d, cx.size, self.scroll).len() as f64 * ROW_H,
        }
    }

    fn draw(&self, cr: &cairo::Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else {
            return;
        };
        if !d.installed {
            return;
        }
        if d.items.is_empty() {
            let t = Text {
                size: 13.0,
                bold: false,
                color: cx.theme.muted,
                text: "Nothing copied yet",
            };
            let (_, th) = draw::measure(cr, &t);
            draw::text(cr, body.0 + 8.0, body.1 + (ROW_H - th) / 2.0, &t);
            return;
        }

        let range = Self::visible_range_for(d, cx.size, self.scroll);
        for (row_i, idx) in range.enumerate() {
            let item = &d.items[idx];
            let ry = body.1 + row_i as f64 * ROW_H;
            let row_r = (body.0, ry, body.2, ROW_H);
            let id = (idx + 1) as u32;
            if hits.add(id, row_r) || hits.selected(id) {
                ui::light(cr, cx, row_r);
            }

            let ic_color = if item.pinned {
                cx.theme.accent
            } else {
                cx.theme.muted
            };
            draw_item_icon(
                cr,
                kind_of(item),
                item.pinned,
                body.0 + 8.0,
                ry + (ROW_H - 16.0) / 2.0,
                ic_color,
            );

            let right_pad = if item.time.is_empty() {
                8.0
            } else {
                let time_t = Text {
                    size: 11.5,
                    bold: false,
                    color: cx.theme.muted,
                    text: &item.time,
                };
                let (tw, th) = draw::measure(cr, &time_t);
                draw::text_right(cr, body.0 + body.2 - 8.0, ry + (ROW_H - th) / 2.0, &time_t);
                tw + 16.0
            };

            let text_x = body.0 + 8.0 + 16.0 + 10.0;
            let max_w = (body.0 + body.2 - right_pad - text_x).max(0.0);
            if max_w > 0.0 {
                let preview_t = Text {
                    size: 13.0,
                    bold: false,
                    color: cx.theme.text,
                    text: &item.preview,
                };
                let (_, th) = draw::measure(cr, &preview_t);
                draw::text_max(cr, text_x, ry + (ROW_H - th) / 2.0, max_w, &preview_t);
            }
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Some(d) = &mut self.data else {
            return false;
        };
        if !d.installed {
            return false;
        }
        match ev {
            Event::Press { hit, button, .. } => {
                let Some(idx) = hit.checked_sub(1).map(|i| i as usize) else {
                    return false;
                };
                let Some(item) = d.items.get(idx) else {
                    return false;
                };
                match button {
                    Button::Left => {
                        // Shell breaks out of action processing once ClosePanel runs, so Run must come first.
                        out.push(Action::Run(copy_command(&item.raw_line)));
                        out.push(Action::ClosePanel);
                        true
                    }
                    Button::Right | Button::Middle => {
                        let removed = d.items.remove(idx);
                        let max_scroll = d.items.len().saturating_sub(Self::visible_limit(cx.size));
                        self.scroll = self.scroll.min(max_scroll);
                        out.push(Action::Run(delete_command(&removed.raw_line)));
                        true
                    }
                }
            }
            Event::Scroll { dy, .. } => {
                let limit = Self::visible_limit(cx.size);
                let max_scroll = d.items.len().saturating_sub(limit);
                if max_scroll == 0 {
                    return false;
                }
                let cur = self.scroll.min(max_scroll);
                let next = if *dy > 0.0 {
                    (cur + 1).min(max_scroll)
                } else if *dy < 0.0 {
                    cur.saturating_sub(1)
                } else {
                    cur
                };
                if next != self.scroll {
                    self.scroll = next;
                    true
                } else {
                    false
                }
            }
            Event::Text(_) => false,
            _ => false,
        }
    }
}

fn draw_item_icon(cr: &cairo::Context, kind: ClipKind, pinned: bool, x: f64, y: f64, color: Rgba) {
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    draw::set(cr, color);

    if pinned {
        cr.move_to(x + 5.0, y + 2.5);
        cr.line_to(x + 11.0, y + 2.5);
        cr.move_to(x + 6.0, y + 2.5);
        cr.line_to(x + 6.0, y + 7.0);
        cr.line_to(x + 4.0, y + 9.5);
        cr.line_to(x + 12.0, y + 9.5);
        cr.line_to(x + 10.0, y + 7.0);
        cr.line_to(x + 10.0, y + 2.5);
        cr.move_to(x + 8.0, y + 9.5);
        cr.line_to(x + 8.0, y + 14.0);
        let _ = cr.stroke();
        return;
    }

    match kind {
        ClipKind::Link => {
            let p = std::f64::consts::FRAC_PI_2;
            cr.new_sub_path();
            cr.arc(x + 4.5, y + 8.0, 2.5, p, 3.0 * p);
            cr.move_to(x + 4.5, y + 5.5);
            cr.line_to(x + 7.0, y + 5.5);
            cr.move_to(x + 4.5, y + 10.5);
            cr.line_to(x + 7.0, y + 10.5);
            cr.new_sub_path();
            cr.arc(x + 11.5, y + 8.0, 2.5, -p, p);
            cr.move_to(x + 9.0, y + 5.5);
            cr.line_to(x + 11.5, y + 5.5);
            cr.move_to(x + 9.0, y + 10.5);
            cr.line_to(x + 11.5, y + 10.5);
            cr.move_to(x + 6.0, y + 8.0);
            cr.line_to(x + 10.0, y + 8.0);
            let _ = cr.stroke();
        }
        ClipKind::Image => {
            draw::round_rect(cr, x + 2.0, y + 2.5, 12.0, 11.0, 2.0);
            let _ = cr.stroke();
            cr.move_to(x + 3.0, y + 11.5);
            cr.line_to(x + 6.2, y + 7.8);
            cr.line_to(x + 8.8, y + 10.5);
            cr.line_to(x + 11.0, y + 8.5);
            cr.line_to(x + 13.0, y + 11.5);
            let _ = cr.stroke();
            cr.arc(x + 10.6, y + 5.6, 1.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
        ClipKind::Text => {
            cr.move_to(x + 2.5, y + 4.0);
            cr.line_to(x + 13.5, y + 4.0);
            cr.move_to(x + 2.5, y + 8.0);
            cr.line_to(x + 13.5, y + 8.0);
            cr.move_to(x + 2.5, y + 12.0);
            cr.line_to(x + 9.5, y + 12.0);
            let _ = cr.stroke();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::config::Config;
    use crate::theme::Theme;

    fn test_theme() -> Theme {
        Theme::from_config(&Config::from_text(""))
    }

    fn make_cx<'a>(th: &'a Theme, size: Size) -> Cx<'a> {
        let mut cx = test_cx(th);
        cx.size = size;
        cx
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "edge-apps-clipboard-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn provider_metadata_and_lookup() {
        let p = Clipboard;
        assert_eq!(p.id(), "clipboard");
        assert_eq!(p.name(), "Clipboard");
        assert!(p.claims().is_empty());
        assert_eq!(p.listed(), Listed::Always);
        assert_eq!(p.cards().len(), 1);
        let k = &p.cards()[0];
        assert_eq!(k.id, "history");
        assert_eq!(k.sizes, &[Size::M, Size::L]);
        assert_eq!(k.default, Size::M);
        assert_eq!(k.sources, &["clipboard"]);
        assert!(p.card("history", None).is_some());
        assert!(p.card("unknown", None).is_none());
        assert!(p.source("clipboard").is_some());
        assert!(p.source("other").is_none());
    }

    #[test]
    fn parses_real_cliphist_output_images_links_home_and_pinned() {
        let sample = "\
108\tgit push origin main
107\t[[ binary data 24 KiB png 1920x1080 ]]
pin:106\t/home/alice/projects/infynitae/live/launch.sh\t2m
105\t[[ binary data 972 B jpeg 640x480 ]]\t5m
104\t   \t  
103\thttps://github.com/vysaely/infynitae\t12m
*102\t/home/alice2/keep-full-name
pinned:101\tmulti\rline\x07 bell\t1h
";
        let items = parse_cliphist_with_home(sample, Some("/home/alice"));
        assert_eq!(items.len(), 8);

        // Pinned items move to the top in their relative order.
        assert!(items[0].pinned);
        assert_eq!(
            items[0].raw_line,
            "106\t/home/alice/projects/infynitae/live/launch.sh"
        );
        assert_eq!(items[0].preview, "~/projects/infynitae/live/launch.sh");
        assert_eq!(items[0].time, "2m");
        assert_eq!(kind_of(&items[0]), ClipKind::Text);

        assert!(items[1].pinned);
        assert_eq!(items[1].raw_line, "102\t/home/alice2/keep-full-name");
        assert_eq!(items[1].preview, "/home/alice2/keep-full-name");

        assert!(items[2].pinned);
        assert_eq!(items[2].preview, "multi line bell");
        assert_eq!(items[2].time, "1h");

        // Unpinned items keep their original order afterwards.
        assert!(!items[3].pinned);
        assert_eq!(items[3].raw_line, "108\tgit push origin main");
        assert_eq!(items[3].preview, "git push origin main");

        assert_eq!(items[4].preview, "Image 1920x1080");
        assert_eq!(kind_of(&items[4]), ClipKind::Image);

        assert_eq!(items[5].preview, "Image 640x480");
        assert_eq!(items[5].time, "5m");

        assert_eq!(items[6].preview, "(Clipboard item)");
        assert_eq!(items[6].time, "");

        assert_eq!(items[7].preview, "https://github.com/vysaely/infynitae");
        assert_eq!(items[7].time, "12m");
        assert_eq!(kind_of(&items[7]), ClipKind::Link);
    }

    #[test]
    fn parses_empty_broken_and_caps_at_max_items() {
        assert!(parse_cliphist_with_home("", None).is_empty());
        assert!(parse_cliphist_with_home("   \n\t \n", None).is_empty());

        let broken = "no tab here\n\tmissing-id\n109\tvalid item\t1m\n[[ binary data foo ]]\n";
        let items = parse_cliphist_with_home(broken, None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].raw_line, "109\tvalid item");

        let mut many = String::new();
        for i in 1..=30 {
            many.push_str(&format!("{i}\tentry {i}\n"));
        }
        assert_eq!(parse_cliphist_with_home(&many, None).len(), MAX_ITEMS);
    }

    #[test]
    fn source_reports_missing_when_cliphist_absent_and_present_with_executable() {
        let dir = temp_dir("path");
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        let mut src = ClipboardSource {
            path: Some(empty.clone().into_os_string()),
        };
        let d = src.fetch();
        let d = d.downcast_ref::<ClipboardData>().unwrap();
        assert!(!d.installed);
        assert!(d.items.is_empty());

        let with_nonexec = dir.join("nonexec");
        std::fs::create_dir_all(&with_nonexec).unwrap();
        std::fs::write(with_nonexec.join("cliphist"), b"#!/bin/sh\n").unwrap();
        assert!(!has_executable("cliphist", with_nonexec.as_os_str()));

        let with_exec = dir.join("exec");
        std::fs::create_dir_all(&with_exec).unwrap();
        let bin = with_exec.join("cliphist");
        std::fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        assert!(has_executable("cliphist", with_exec.as_os_str()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn card_status_apply_and_actions_cover_all_states() {
        let th = test_theme();
        let cx_m = make_cx(&th, Size::M);
        let mut card = HistoryCard::default();
        assert_eq!(card.status(&cx_m), Status::Loading);
        assert!(!card.apply(
            "wrong",
            &ClipboardData {
                installed: true,
                items: vec![]
            }
        ));
        assert!(!card.apply("clipboard", &"not clipboard data"));

        assert!(card.apply(
            "clipboard",
            &ClipboardData {
                installed: false,
                items: vec![],
            }
        ));
        assert_eq!(card.status(&cx_m), Status::Missing(MISSING_MSG.into()));
        let mut out = Vec::new();
        assert!(!card.event(
            &Event::Press {
                hit: 1,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx_m,
            &mut out
        ));

        let empty_data = ClipboardData {
            installed: true,
            items: vec![],
        };
        assert!(card.apply("clipboard", &empty_data));
        assert!(!card.apply("clipboard", &empty_data));
        assert_eq!(card.status(&cx_m), Status::Ready);

        let items = parse_cliphist_with_home(
            "108\tgit push origin main\n107\t[[ binary data 24 KiB png 1920x1080 ]]\t3m\n",
            None,
        );
        assert!(card.apply(
            "clipboard",
            &ClipboardData {
                installed: true,
                items,
            }
        ));
        assert_eq!(card.status(&cx_m), Status::Ready);

        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: 1,
                button: Button::Left,
                at: (20.0, 10.0),
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(
            out,
            vec![
                Action::Run(
                    "printf '%s\\n' '108\tgit push origin main' | cliphist decode 2>/dev/null | wl-copy"
                        .into()
                ),
                Action::ClosePanel,
            ]
        );

        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: 2,
                button: Button::Right,
                at: (20.0, 42.0),
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Run(
                "printf '%s\\n' '107\t[[ binary data 24 KiB png 1920x1080 ]]' | cliphist delete 2>/dev/null"
                    .into()
            )]
        );
        assert_eq!(card.data.as_ref().unwrap().items.len(), 1);

        // Middle click deletes as well; Text events do not filter clipboard.
        out.clear();
        assert!(!card.event(&Event::Text("git"), &cx_m, &mut out));
        assert!(out.is_empty());
        assert!(card.event(
            &Event::Press {
                hit: 1,
                button: Button::Middle,
                at: (20.0, 10.0),
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Run(
                "printf '%s\\n' '108\tgit push origin main' | cliphist delete 2>/dev/null".into()
            )]
        );
        assert!(card.data.as_ref().unwrap().items.is_empty());
    }

    #[test]
    fn untrusted_history_line_cannot_execute_shell_syntax() {
        let dir = temp_dir("untrusted");
        let marker_semi = dir.join("pwn_semi");
        let marker_sub = dir.join("pwn_sub");
        let marker_nl = dir.join("pwn_nl");
        let captured = dir.join("decoded.txt");

        let raw_line = format!(
            "108\talpha'beta; touch {}; $(touch {})\nnext\ttouch {}\ttail",
            marker_semi.display(),
            marker_sub.display(),
            marker_nl.display(),
        );
        let mut card = HistoryCard::default();
        card.apply(
            "clipboard",
            &ClipboardData {
                installed: true,
                items: vec![ClipItem {
                    raw_line: raw_line.clone(),
                    preview: "alpha'beta".into(),
                    time: "1m".into(),
                    is_image: false,
                    pinned: false,
                }],
            },
        );
        let th = test_theme();
        let cx = make_cx(&th, Size::M);
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: 1,
                button: Button::Left,
                at: (10.0, 10.0),
            },
            &cx,
            &mut out
        ));
        let Action::Run(ref copy_cmd) = out[0] else {
            panic!("expected Action::Run first, got {out:?}");
        };
        assert_eq!(out.get(1), Some(&Action::ClosePanel));

        // Stub cliphist + wl-copy into a private bin dir so execution proves stdin integrity.
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let cliphist_stub = bin.join("cliphist");
        std::fs::write(&cliphist_stub, "#!/bin/sh\ncat\n").unwrap();
        let wlcopy_stub = bin.join("wl-copy");
        std::fs::write(
            &wlcopy_stub,
            format!("#!/bin/sh\ncat > {}\n", sq(&captured.display().to_string())),
        )
        .unwrap();
        for p in [&cliphist_stub, &wlcopy_stub] {
            let mut perms = std::fs::metadata(p).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(p, perms).unwrap();
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let new_path = std::env::join_paths(
            std::iter::once(bin.clone()).chain(std::env::split_paths(&old_path)),
        )
        .unwrap();

        let status = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(copy_cmd)
            .env("PATH", &new_path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!marker_semi.exists(), "semicolon executed");
        assert!(!marker_sub.exists(), "$(...) executed");
        assert!(!marker_nl.exists(), "newline executed");
        let got = std::fs::read_to_string(&captured).unwrap();
        assert_eq!(got, format!("{raw_line}\n"));

        // Also check delete_command on the same untrusted line does not trigger injections.
        let del_cmd = delete_command(&raw_line);
        let status = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&del_cmd)
            .env("PATH", &new_path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!marker_semi.exists());
        assert!(!marker_sub.exists());
        assert!(!marker_nl.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn height_matches_draw_and_scroll_works_in_m_and_l() {
        let mut raw = String::new();
        for i in 1..=20 {
            raw.push_str(&format!("{i}\titem {i}\t{i}m\n"));
        }
        let items = parse_cliphist_with_home(&raw, None);

        let th = test_theme();
        let mut card = HistoryCard::default();
        let cx_m = make_cx(&th, Size::M);
        let cx_l = make_cx(&th, Size::L);

        // Loading / missing / empty all use one row height and emit no row hits.
        for (data, expected_hits) in [
            (None, 0usize),
            (
                Some(ClipboardData {
                    installed: false,
                    items: vec![],
                }),
                0,
            ),
            (
                Some(ClipboardData {
                    installed: true,
                    items: vec![],
                }),
                0,
            ),
        ] {
            card.data = data;
            let h = card.height(&cx_m, 360.0);
            assert_eq!(h, ROW_H);
            let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 360, h as i32).unwrap();
            let cr = cairo::Context::new(&surf).unwrap();
            let mut hits = Hits::new(None, None);
            card.draw(&cr, &cx_m, (10.0, 20.0, 340.0, h), &mut hits);
            assert_eq!(hits.list.len(), expected_hits);
        }

        card.apply(
            "clipboard",
            &ClipboardData {
                installed: true,
                items,
            },
        );
        let mut out = Vec::new();
        assert_eq!(card.height(&cx_m, 360.0), MEDIUM_ROWS as f64 * ROW_H);
        assert_eq!(card.height(&cx_l, 360.0), LARGE_ROWS as f64 * ROW_H);

        // Scroll in Size::M: down shifts visible ids, up clamps back to 0.
        assert!(!card.event(
            &Event::Scroll {
                hit: None,
                dy: -1.0
            },
            &cx_m,
            &mut out
        ));
        assert!(card.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_m, &mut out));
        assert_eq!(card.scroll, 1);
        assert!(card.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out));
        assert_eq!(card.scroll, 2);

        for (cx, expected_rows) in [(&cx_m, MEDIUM_ROWS), (&cx_l, LARGE_ROWS)] {
            let h = card.height(cx, 360.0);
            assert_eq!(h, expected_rows as f64 * ROW_H);
            let surf =
                cairo::ImageSurface::create(cairo::Format::ARgb32, 400, h as i32 + 40).unwrap();
            let cr = cairo::Context::new(&surf).unwrap();
            let body = (10.0, 20.0, 360.0, h);
            let mut hits = Hits::new(Some((20.0, 25.0)), Some(4));
            card.draw(&cr, cx, body, &mut hits);
            assert_eq!(hits.list.len(), expected_rows);
            for &(_, r) in &hits.list {
                assert!(r.0 >= body.0);
                assert!(r.1 >= body.1);
                assert!(r.0 + r.2 <= body.0 + body.2);
                assert!(r.1 + r.3 <= body.1 + h + 0.001);
            }
        }
    }
}
