//! Clipboard history (`clipboard/history`): the compositor's own history (`infynitae msg
//! clipboard`, 2026-11-25), else `cliphist list` with an older compositor. Click copies,
//! right/middle-click deletes, the pin at a row's end pins it (the compositor's history only), and
//! the panel's typed query filters the list.

use std::any::Any;
use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::os::unix::fs::PermissionsExt;

use crate::card::{
    parse_render_data, run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status,
};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::theme::Rgba;
use crate::ui;
use infynitae_edge_apps::clipboard_entry::{
    copy_command, delete_command, kind_of, parse_cliphist_with_home, ClipItem, ClipKind,
};
#[cfg(test)]
use infynitae_edge_apps::clipboard_entry::{sq, MAX_ITEMS};

const MEDIUM_ROWS: usize = 8;
const LARGE_ROWS: usize = 16;
const ROW_H: f64 = ui::LIST_ROW_H;
/// The compositor's history; with a compositor from before it, a `cliphist` line and cliphist's.
const CMD: &str =
    "infynitae msg clipboard 2>/dev/null || { echo cliphist; cliphist list 2>/dev/null | head -n 24; }";
/// The first line of `CMD`'s output when cliphist answered.
const CLIPHIST_MARK: &str = "cliphist";
/// The pin's hit at the end of row `i` (rows are `i + 1`).
const PIN_HIT: u32 = 10_000;
const MISSING_MSG: &str = "cliphist is not installed";

pub struct Clipboard;

const KINDS: &[CardKind] = &[CardKind {
    id: "history",
    title: "Clipboard",
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

/// before the tab, which the compositor wrote). None for a line without one.
fn own_request(cmd: &str, raw_line: &str, on: Option<bool>) -> Option<String> {
    let id: u64 = raw_line.split('\t').next()?.parse().ok()?;
    let on = on.map_or(String::new(), |on| format!(",\"on\":{on}"));
    Some(format!(
        "{{\"id\":0,\"cmd\":\"{cmd}\",\"entry\":{id}{on}}}\n"
    ))
}

/// An entry put back through the compositor's own history (`infynitae msg clipboard use`); `None`
/// with a compositor that keeps none (cliphist's then, through [`ClipItem::copy`]).
pub(crate) trait CopyOwn {
    fn copy_own(&self) -> Option<String>;
}

impl CopyOwn for ClipItem {
    fn copy_own(&self) -> Option<String> {
        own_request("clipboard_use", &self.raw_line, None)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardData {
    installed: bool,
    /// The compositor's history (else cliphist's): entries are used, pinned and deleted through
    /// the shell socket.
    pub(crate) own: bool,
    pub(crate) items: Vec<ClipItem>,
}

#[derive(Default)]
pub(crate) struct ClipboardSource {
    /// Optional `PATH` override so tests can check binary detection without env races.
    pub(crate) path: Option<OsString>,
}

impl Source for ClipboardSource {
    fn fetch(&mut self) -> Data {
        Box::new(self.read())
    }
}

impl ClipboardSource {
    /// The history as the card shows it (also the launcher's search, `cards/search.rs`).
    pub(crate) fn read(&mut self) -> ClipboardData {
        if let (None, Some(render_file)) =
            (&self.path, std::env::var_os("INFYNITAE_EDGE_RENDER_DATA"))
        {
            let has_key = std::fs::read_to_string(&render_file)
                .ok()
                .is_some_and(|s| parse_render_data(&s).contains_key("clipboard"));
            if !has_key {
                return ClipboardData {
                    installed: false,
                    own: false,
                    items: Vec::new(),
                };
            }
        }
        self.data_of(&run_sh("clipboard", CMD))
    }

    /// `CMD`'s output as the card's data: the compositor's history, or cliphist's when cliphist
    /// answered (and missing when it is not installed).
    fn data_of(&self, out: &str) -> ClipboardData {
        let Some(rest) = out
            .strip_prefix(CLIPHIST_MARK)
            .and_then(|r| r.strip_prefix('\n'))
        else {
            return ClipboardData {
                installed: true,
                own: true,
                items: parse_cliphist(out),
            };
        };
        let installed = match &self.path {
            Some(p) => has_executable("cliphist", p),
            None => std::env::var_os("PATH")
                .as_deref()
                .is_some_and(|p| has_executable("cliphist", p)),
        };
        ClipboardData {
            installed,
            own: false,
            items: if installed {
                parse_cliphist(rest)
            } else {
                Vec::new()
            },
        }
    }
}

fn has_executable(bin: &str, path: &OsStr) -> bool {
    std::env::split_paths(path).any(|dir| {
        std::fs::metadata(dir.join(bin))
            .is_ok_and(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
    })
}

fn parse_cliphist(out: &str) -> Vec<ClipItem> {
    let home = crate::paths::home();
    parse_cliphist_with_home(out, home.as_deref().and_then(|p| p.to_str()))
}

#[derive(Default)]
struct HistoryCard {
    data: Option<ClipboardData>,
    /// The panel's typed query, in lowercase: only items whose text holds it are shown.
    query: String,
    scroll: usize,
    scroll_accum: f64,
}

impl HistoryCard {
    fn visible_limit(size: Size) -> usize {
        match size {
            Size::L => LARGE_ROWS,
            _ => MEDIUM_ROWS,
        }
    }

    /// The items the query lets through, as indices into `items`.
    fn shown(&self, d: &ClipboardData) -> Vec<usize> {
        (0..d.items.len())
            .filter(|&i| {
                self.query.is_empty() || d.items[i].preview.to_lowercase().contains(&self.query)
            })
            .collect()
    }

    fn visible_range_for(n: usize, size: Size, scroll: usize) -> Range<usize> {
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
            Some(d) if !d.installed => ROW_H,
            Some(d) => {
                let n = self.shown(d).len();
                (Self::visible_range_for(n, cx.size, self.scroll).len() as f64 * ROW_H).max(ROW_H)
            }
        }
    }

    fn draw(&self, cr: &cairo::Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else {
            return;
        };
        if !d.installed {
            return;
        }
        let shown = self.shown(d);
        if shown.is_empty() {
            let t = Text {
                size: 13.0,
                bold: false,
                color: cx.theme.muted,
                text: if d.items.is_empty() {
                    "Nothing copied yet"
                } else {
                    "No copied item holds this text"
                },
            };
            let (_, th) = draw::measure(cr, &t);
            draw::text(cr, body.0 + 8.0, body.1 + (ROW_H - th) / 2.0, &t);
            return;
        }

        let range = Self::visible_range_for(shown.len(), cx.size, self.scroll);
        let start = range.start;
        for (row_i, pos) in range.enumerate() {
            let item = &d.items[shown[pos]];
            let ry = body.1 + row_i as f64 * ROW_H;
            let row_r = (body.0, ry, body.2, ROW_H);
            let id = (pos + 1) as u32;
            let pointed = hits.add(id, row_r);
            if pointed || hits.selected(id) {
                ui::light(cr, cx, row_r);
            }
            // the pin: shown on a pinned row, and on the row pointed at to pin it
            let pin_w = if d.own && (pointed || item.pinned) {
                let pin_r = (body.0 + body.2 - 32.0, ry, 32.0, ROW_H);
                let over = hits.add(PIN_HIT + id, pin_r);
                let color = if item.pinned || over {
                    cx.theme.accent
                } else {
                    cx.theme.muted
                };
                draw_item_icon(
                    cr,
                    ClipKind::Text,
                    true,
                    pin_r.0 + 8.0,
                    ry + (ROW_H - 16.0) / 2.0,
                    color,
                );
                32.0
            } else {
                0.0
            };

            // the compositor's history shows a pin at the row's end; cliphist's in place of the icon
            let pin_left = item.pinned && !d.own;
            let ic_color = if pin_left {
                cx.theme.accent
            } else {
                cx.theme.muted
            };
            draw_item_icon(
                cr,
                kind_of(item),
                pin_left,
                body.0 + 8.0,
                ry + (ROW_H - 16.0) / 2.0,
                ic_color,
            );

            let right_pad = pin_w
                + if item.time.is_empty() {
                    8.0
                } else {
                    let time_t = Text {
                        size: draw::SIZE_NOTE,
                        bold: false,
                        color: cx.theme.muted,
                        text: &item.time,
                    };
                    let (tw, th) = draw::measure(cr, &time_t);
                    draw::text_right(
                        cr,
                        body.0 + body.2 - pin_w - 8.0,
                        ry + (ROW_H - th) / 2.0,
                        &time_t,
                    );
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
        ui::scrollbar(
            cr,
            cx,
            body,
            shown.len() as f64 * ROW_H,
            start as f64 * ROW_H,
        );
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        if let Event::Text(q) = ev {
            let q = q.trim().to_lowercase();
            if q == self.query {
                return false;
            }
            self.query = q;
            self.scroll = 0;
            return true;
        }
        let Some(d) = &self.data else {
            return false;
        };
        if !d.installed {
            return false;
        }
        let shown = self.shown(d);
        // a row's hit (or its pin's) as the item's index
        let item_of = |hit: u32| {
            let pos = (hit % PIN_HIT).checked_sub(1)? as usize;
            shown.get(pos).copied()
        };
        let Some(d) = &mut self.data else {
            return false;
        };
        match ev {
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } if *hit > PIN_HIT && d.own => {
                let Some(idx) = item_of(*hit) else {
                    return false;
                };
                let item = &mut d.items[idx];
                item.pinned = !item.pinned;
                if let Some(line) = own_request("clipboard_pin", &item.raw_line, Some(item.pinned))
                {
                    out.push(Action::ShellCommand(line));
                }
                d.items.sort_by_key(|it| !it.pinned);
                true
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } => {
                let Some(item) = item_of(*hit).map(|i| &d.items[i]) else {
                    return false;
                };
                // Shell breaks out of action processing once ClosePanel runs, so the copy comes first.
                if d.own {
                    match own_request("clipboard_use", &item.raw_line, None) {
                        Some(line) => out.push(Action::ShellCommand(line)),
                        None => return false,
                    }
                } else {
                    out.push(Action::Run(copy_command(&item.raw_line)));
                }
                out.push(Action::ClosePanel);
                true
            }
            Event::Press {
                hit,
                button: Button::Right | Button::Middle,
                ..
            }
            | Event::Delete { hit } => {
                let Some(idx) = item_of(*hit) else {
                    return false;
                };
                let removed = d.items.remove(idx);
                let max_scroll = (shown.len() - 1).saturating_sub(Self::visible_limit(cx.size));
                self.scroll = self.scroll.min(max_scroll);
                if d.own {
                    if let Some(line) = own_request("clipboard_delete", &removed.raw_line, None) {
                        out.push(Action::ShellCommand(line));
                    }
                } else {
                    out.push(Action::Run(delete_command(&removed.raw_line)));
                }
                true
            }
            Event::Scroll { dy, notches, .. } => {
                let limit = Self::visible_limit(cx.size);
                let max_scroll = shown.len().saturating_sub(limit);
                crate::card::scroll_rows(
                    &mut self.scroll,
                    &mut self.scroll_accum,
                    max_scroll,
                    *dy,
                    *notches,
                )
            }
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
        for i in 1..=MAX_ITEMS + 6 {
            many.push_str(&format!("{i}\tentry {i}\n"));
        }
        assert_eq!(parse_cliphist_with_home(&many, None).len(), MAX_ITEMS);
    }

    #[test]
    fn multibyte_utf8_preview_does_not_panic_url_check() {
        let items = parse_cliphist_with_home("1\t🦀🦀\n2\t€€€\n3\tHTTPS://example.com/ok\n", None);
        assert_eq!(items.len(), 3);
        assert_eq!(kind_of(&items[0]), ClipKind::Text);
        assert_eq!(kind_of(&items[1]), ClipKind::Text);
        assert_eq!(kind_of(&items[2]), ClipKind::Link);
    }

    #[test]
    fn clipboard_entry_crash_and_seed_corpus() {
        let huge_ctrl = format!("1\t{}hello\n", "\0".repeat(16_384));
        let huge_line = format!("2\t{}\n", "a".repeat(70_000));
        let bidi = "3\thello\u{202E}gpj.exe\u{202C}\n";
        let utf8_home = "4\t/home/🦀/file.txt\n";
        for raw in [
            "",
            "\0\r\n",
            "no-tab",
            "\tmissing-id",
            "5\t[[ binary data ]]\n",
            "6\t[[ binary data 0x10 🦀 ]]\n",
            "7\tftp://\n",
            &huge_ctrl,
            &huge_line,
            bidi,
            utf8_home,
        ] {
            let parsed = parse_cliphist_with_home(raw, Some("/home/🦀"));
            for it in &parsed {
                let _ = kind_of(it);
                let _ = copy_command(&it.raw_line);
                let _ = delete_command(&it.raw_line);
            }
        }
        assert!(parse_cliphist_with_home(&huge_line, None).is_empty());
        let bidi_items = parse_cliphist_with_home(bidi, None);
        assert_eq!(bidi_items[0].preview, "hellogpj.exe");
        let home_items = parse_cliphist_with_home(utf8_home, Some("/home/🦀"));
        assert_eq!(home_items[0].preview, "~/file.txt");
    }

    #[test]
    fn source_reports_missing_when_cliphist_absent_and_present_with_executable() {
        let dir = temp_dir("path");
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        // what `CMD` prints with a compositor from before its own history (not run here: it
        // would ask the desktop the tests run on)
        let src = ClipboardSource {
            path: Some(empty.clone().into_os_string()),
        };
        let d = src.data_of("cliphist\n");
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
                own: false,
                items: vec![]
            }
        ));
        assert!(!card.apply("clipboard", &"not clipboard data"));

        assert!(card.apply(
            "clipboard",
            &ClipboardData {
                installed: false,
                own: false,
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
            own: false,
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
                own: false,
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

        // Middle click deletes as well; the typed query filters the list (here to the one left).
        out.clear();
        assert!(card.event(&Event::Text("git"), &cx_m, &mut out));
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
                own: false,
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
                    own: false,
                    items: vec![],
                }),
                0,
            ),
            (
                Some(ClipboardData {
                    installed: true,
                    own: false,
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
                own: false,
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
                dy: -36.0,
                notches: -1.0,
            },
            &cx_m,
            &mut out
        ));
        assert!(card.event(
            &Event::Scroll {
                hit: None,
                dy: 36.0,
                notches: 1.0,
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(card.scroll, 1);
        assert!(card.event(
            &Event::Scroll {
                hit: None,
                dy: 36.0,
                notches: 1.0,
            },
            &cx_l,
            &mut out
        ));
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

    #[test]
    fn the_compositors_history_is_used_pinned_and_deleted_through_the_shell_socket() {
        let src = ClipboardSource::default();
        let d = src.data_of(
            "pinned:7\tkept note\n5\t[[ binary data 24 KiB png 1920x1080 ]]\n3\tgit push\n",
        );
        assert!(d.installed && d.own);
        assert_eq!(d.items.len(), 3);
        assert!(d.items[0].pinned);
        assert_eq!(d.items[1].preview, "Image 1920x1080");
        let th = test_theme();
        let cx = make_cx(&th, Size::M);
        let mut card = HistoryCard::default();
        assert!(card.apply("clipboard", &d));

        // the row pointed at shows its pin, and only it (and the pinned row) has one
        let h = card.height(&cx, 360.0);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, h as i32).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(Some((20.0, ROW_H * 1.5)), None);
        card.draw(&cr, &cx, (0.0, 0.0, 360.0, h), &mut hits);
        let ids: Vec<u32> = hits.list.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [1, PIN_HIT + 1, 2, PIN_HIT + 2, 3]);

        let press = |hit: u32, button: Button| Event::Press {
            hit,
            button,
            at: (0.0, 0.0),
        };
        let mut out = Vec::new();
        assert!(card.event(&press(2, Button::Left), &cx, &mut out));
        assert_eq!(
            out,
            [
                Action::ShellCommand("{\"id\":0,\"cmd\":\"clipboard_use\",\"entry\":5}\n".into()),
                Action::ClosePanel
            ]
        );
        out.clear();
        assert!(card.event(&press(PIN_HIT + 1, Button::Left), &cx, &mut out));
        assert_eq!(
            out,
            [Action::ShellCommand(
                "{\"id\":0,\"cmd\":\"clipboard_pin\",\"entry\":7,\"on\":false}\n".into()
            )]
        );
        assert!(!card.data.as_ref().unwrap().items[0].pinned);
        out.clear();
        assert!(card.event(&press(3, Button::Right), &cx, &mut out));
        assert_eq!(
            out,
            [Action::ShellCommand(
                "{\"id\":0,\"cmd\":\"clipboard_delete\",\"entry\":3}\n".into()
            )]
        );
        assert_eq!(card.data.as_ref().unwrap().items.len(), 2);
    }

    #[test]
    fn the_typed_query_filters_the_list_and_says_when_nothing_holds_it() {
        let src = ClipboardSource::default();
        let d = src.data_of("9\tGit push origin\n8\tcargo test\n7\tgit status\n");
        let th = test_theme();
        let cx = make_cx(&th, Size::M);
        let mut card = HistoryCard::default();
        card.apply("clipboard", &d);
        let mut out = Vec::new();
        assert!(card.event(&Event::Text(" GIT "), &cx, &mut out));
        assert!(
            !card.event(&Event::Text("git"), &cx, &mut out),
            "the same query"
        );
        assert_eq!(card.height(&cx, 360.0), 2.0 * ROW_H);
        // the second row shown is the third item
        assert!(card.event(
            &Event::Press {
                hit: 2,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out
        ));
        assert_eq!(
            out[0],
            Action::ShellCommand("{\"id\":0,\"cmd\":\"clipboard_use\",\"entry\":7}\n".into())
        );
        assert!(card.event(&Event::Text("nothing like it"), &cx, &mut out));
        assert_eq!(card.height(&cx, 360.0), ROW_H);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 360, ROW_H as i32).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        card.draw(&cr, &cx, (0.0, 0.0, 360.0, ROW_H), &mut hits);
        assert!(hits.list.is_empty());
        assert!(card.event(&Event::Text(""), &cx, &mut out));
        assert_eq!(card.height(&cx, 360.0), 3.0 * ROW_H);
    }
}
