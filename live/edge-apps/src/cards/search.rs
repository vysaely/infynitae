//! What the launcher's search finds besides apps (`apps/results`, placed under the apps list): open
//! windows, settings rows, clipboard history, replay clips, recordings, recent files and files by
//! name. Grouped by kind, a few rows each, only while something is typed.
//!
//! Each kind is searched on the keystroke from what is already in memory: the edge's window list,
//! the settings cards' tables, and what `SearchSource` read once when the panel opened (the
//! clipboard card's history, the names in CLIPS_DIR and RECORDINGS_DIR, recently-used.xbel). Files
//! by name come from plocate on a thread of its own (`crate::locate`), killed by the next
//! keystroke. When the panel closes the card drops all of it.

use crate::card::{Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::cards::{
    apps, clipboard, settings_apps, settings_display, settings_games, settings_input,
    settings_lock, settings_look, settings_privacy, settings_recording, settings_windows,
};
use crate::locate::Located;
use crate::provider::{CardKind, Size};
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;
use std::path::{Path, PathBuf};

pub const KIND: CardKind = CardKind {
    id: "results",
    title: "Search results",
    about: "Windows, settings, clipboard, clips, recordings and files matching the search",
    sizes: &[Size::M],
    default: Size::M,
    half: false,
    // "located" is not fetched (no source answers to it): plocate's answers come under that key
    sources: &["search", "located"],
};

/// Rows shown per kind.
const PER_KIND: usize = 4;
/// Names kept per folder and from the recent-files list, newest first.
const MAX_FILES: usize = 1000;
const SUB_H: f64 = 22.0;
const HIT_BASE: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Windows,
    Settings,
    Clipboard,
    Clips,
    Recordings,
    Recent,
    Files,
}

impl Kind {
    fn heading(self) -> &'static str {
        match self {
            Kind::Windows => "Windows",
            Kind::Settings => "Settings",
            Kind::Clipboard => "Clipboard",
            Kind::Clips => "Clips",
            Kind::Recordings => "Recordings",
            Kind::Recent => "Recent files",
            Kind::Files => "Files",
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Kind::Windows => "windows",
            Kind::Settings => "settings",
            Kind::Clipboard => "clipboard",
            Kind::Clips => "clips",
            Kind::Recordings => "recordings",
            Kind::Recent => "recent",
            Kind::Files => "files",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Open {
    Window(u64),
    /// the Settings panel searching this row's name
    Setting(&'static str),
    /// the command that copies it back
    Copy(String),
    Path(String),
}

#[derive(Clone, Debug, PartialEq)]
struct Item {
    name: String,
    /// the second line: the app, the settings card, the time, the folder
    note: String,
    open: Open,
}

/// A file or clipboard entry with its text in lower case once, so a keystroke only compares.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    item: Item,
    lower: String,
}

impl Entry {
    fn new(item: Item) -> Entry {
        let lower = item.name.to_lowercase();
        Entry { item, lower }
    }
}

/// What the source read when the panel opened.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchData {
    clipboard: Vec<Entry>,
    clips: Vec<Entry>,
    recordings: Vec<Entry>,
    recent: Vec<Entry>,
}

pub struct SearchSource {
    home: Option<PathBuf>,
    config: PathBuf,
    defaults: PathBuf,
    recent_file: PathBuf,
    clipboard: clipboard::ClipboardSource,
}

impl Default for SearchSource {
    fn default() -> Self {
        let home = crate::paths::home();
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")))
            .unwrap_or_default();
        SearchSource {
            config: crate::config::config_path().unwrap_or_default(),
            defaults: home
                .as_ref()
                .map(|h| h.join("infynitae/config/config"))
                .unwrap_or_default(),
            recent_file: data_home.join("recently-used.xbel"),
            home,
            clipboard: Default::default(),
        }
    }
}

impl Source for SearchSource {
    fn fetch(&mut self) -> Data {
        Box::new(self.read())
    }

    /// Once each time the panel opens.
    fn every(&self) -> Option<std::time::Duration> {
        None
    }
}

impl SearchSource {
    fn read(&mut self) -> SearchData {
        let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
        let user = crate::cards::settings_rows::parse_config(&read(&self.config));
        let defaults = crate::cards::settings_rows::parse_config(&read(&self.defaults));
        let home = self.home.as_deref();
        let folder = |key: &str| {
            user.get(key)
                .or_else(|| defaults.get(key))
                .filter(|v| !v.trim().is_empty())
                .map(|v| crate::paths::expand_home_with(v, home))
        };
        let in_folder = |key: &str| folder(key).map_or_else(Vec::new, |d| files_newest_first(&d));
        SearchData {
            clipboard: self
                .clipboard
                .read()
                .items
                .into_iter()
                .map(|c| {
                    Entry::new(Item {
                        open: Open::Copy(c.copy()),
                        name: c.preview,
                        note: c.time,
                    })
                })
                .collect(),
            clips: in_folder("CLIPS_DIR"),
            recordings: in_folder("RECORDINGS_DIR"),
            recent: parse_xbel(&read(&self.recent_file), home),
        }
    }
}

/// `/home/a/Videos` as `~/Videos`.
fn short(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

fn path_item(path: &Path, note: String) -> Option<Entry> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(Entry::new(Item {
        name,
        note,
        open: Open::Path(path.to_string_lossy().into_owned()),
    }))
}

/// The files (not folders) in `dir`, newest first, with their day and time as the note.
fn files_newest_first(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok().filter(|m| m.is_file())?;
            Some((m.modified().ok()?, e.path()))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files
        .into_iter()
        .take(MAX_FILES)
        .filter_map(|(t, p)| {
            let when = chrono::DateTime::<chrono::Local>::from(t)
                .format("%Y-%m-%d %H:%M")
                .to_string();
            path_item(&p, when)
        })
        .collect()
}

/// The value of `name="..."` in an element's opening tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let mut from = 0;
    while let Some(i) = tag[from..].find(&pat) {
        let at = from + i;
        let start = at + pat.len();
        // at 0: the split on "<bookmark " took the space before the first attribute
        if at == 0 || tag[..at].ends_with(char::is_whitespace) {
            let len = tag[start..].find('"')?;
            return Some(&tag[start..start + len]);
        }
        from = start;
    }
    None
}

fn unescape_xml(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// `file:///a%20b` as `/a b`; None for other schemes or a broken escape.
fn file_uri_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = rest.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok().filter(|p| p.starts_with('/'))
}

/// The local files in a recently-used.xbel, newest first by when they were last changed or
/// opened, with their folder as the note.
fn parse_xbel(text: &str, home: Option<&Path>) -> Vec<Entry> {
    let mut found: Vec<(&str, String)> = Vec::new();
    for chunk in text.split("<bookmark ").skip(1) {
        let tag = &chunk[..chunk.find('>').unwrap_or(chunk.len())];
        let Some(href) = attr(tag, "href") else {
            continue;
        };
        let Some(path) = file_uri_path(&unescape_xml(href)) else {
            continue;
        };
        // ISO 8601 in UTC: newer sorts later as text
        let when = [attr(tag, "visited"), attr(tag, "modified")]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or("");
        found.push((when, path));
    }
    found.sort_by(|a, b| b.0.cmp(a.0));
    found
        .into_iter()
        .take(MAX_FILES)
        .filter_map(|(_, p)| {
            let path = Path::new(&p);
            let note = path.parent().map(|d| short(d, home)).unwrap_or_default();
            path_item(path, note)
        })
        .collect()
}

/// Every settings row the Settings panel can show, with its card's title.
fn settings_rows() -> impl Iterator<Item = (&'static str, &'static str)> {
    let look = [
        (
            settings_look::TABLE_APPEARANCE,
            settings_look::KIND_APPEARANCE.title,
        ),
        (
            settings_look::TABLE_WALLPAPER,
            settings_look::KIND_WALLPAPER.title,
        ),
        (settings_look::TABLE_FONTS, settings_look::KIND_FONTS.title),
    ]
    .into_iter()
    .flat_map(|(t, title)| t.iter().map(move |r| (r.name, title)));
    let tables = [
        (settings_display::TABLE, settings_display::KIND.title),
        (settings_input::TABLE, settings_input::KIND.title),
        (settings_windows::TABLE, settings_windows::KIND.title),
        (settings_apps::TABLE, settings_apps::KIND.title),
        (settings_games::TABLE, settings_games::KIND.title),
        (settings_lock::TABLE, settings_lock::KIND.title),
        (settings_privacy::TABLE, settings_privacy::KIND.title),
        (settings_recording::TABLE, settings_recording::KIND.title),
    ]
    .into_iter()
    .flat_map(|(t, title)| t.iter().map(move |r| (r.name, title)));
    let found = settings_apps::FOUND
        .iter()
        .map(|f| (f.name, settings_apps::KIND.title));
    look.chain(tables).chain(found)
}

fn all_in(lower: &str, terms: &[String]) -> bool {
    terms.iter().all(|t| lower.contains(t.as_str()))
}

/// The first `PER_KIND` of `entries` holding every word, those starting with the first word ahead
/// (the order kept otherwise: newest first).
fn first_matches<'a>(entries: impl Iterator<Item = &'a Entry>, terms: &[String]) -> Vec<Item> {
    let mut starts = Vec::new();
    let mut rest = Vec::new();
    for e in entries {
        if !all_in(&e.lower, terms) {
            continue;
        }
        if e.lower.starts_with(terms[0].as_str()) {
            starts.push(&e.item);
            if starts.len() == PER_KIND {
                break;
            }
        } else if rest.len() < PER_KIND {
            rest.push(&e.item);
        }
    }
    starts
        .into_iter()
        .chain(rest)
        .take(PER_KIND)
        .cloned()
        .collect()
}

#[derive(Default)]
pub struct ResultsCard {
    query: String,
    /// the query's words in lower case
    terms: Vec<String>,
    /// between a panel opening (its first Text) and its closing: data is kept only then
    shown: bool,
    data: Option<SearchData>,
    /// plocate's answer for `query`
    located: Vec<Entry>,
    found: Vec<(Kind, Vec<Item>)>,
}

impl ResultsCard {
    fn find(&mut self, cx: &Cx) {
        self.found.clear();
        if self.terms.is_empty() {
            return;
        }
        let terms = &self.terms;
        let mut groups: Vec<(Kind, Vec<Item>)> = Vec::new();

        let windows: Vec<Item> = cx
            .windows
            .iter()
            .filter_map(|w| {
                let app = crate::cards::windows::resolve_app_name(cx.apps, w);
                let hay = format!("{} {} {}", w.title, app, w.app_id).to_lowercase();
                all_in(&hay, terms).then(|| Item {
                    name: if w.title.is_empty() {
                        app.to_string()
                    } else {
                        w.title.clone()
                    },
                    note: app.to_string(),
                    open: Open::Window(w.id),
                })
            })
            .take(PER_KIND)
            .collect();
        groups.push((Kind::Windows, windows));

        let mut settings: Vec<Item> = Vec::new();
        for (name, card) in settings_rows() {
            if settings.iter().any(|s| s.open == Open::Setting(name)) {
                continue;
            }
            if all_in(&name.to_lowercase(), terms) {
                settings.push(Item {
                    name: name.to_string(),
                    note: card.to_string(),
                    open: Open::Setting(name),
                });
                if settings.len() == PER_KIND {
                    break;
                }
            }
        }
        groups.push((Kind::Settings, settings));

        groups.extend(self.read_kinds());
        groups.retain(|(_, items)| !items.is_empty());
        self.found = groups;
    }

    /// The kinds read from disk or plocate, matched against `terms` (not empty).
    fn read_kinds(&self) -> Vec<(Kind, Vec<Item>)> {
        let terms = &self.terms;
        let mut groups = Vec::new();
        if let Some(d) = &self.data {
            groups.push((Kind::Clipboard, first_matches(d.clipboard.iter(), terms)));
            groups.push((Kind::Clips, first_matches(d.clips.iter(), terms)));
            groups.push((Kind::Recordings, first_matches(d.recordings.iter(), terms)));
            groups.push((Kind::Recent, first_matches(d.recent.iter(), terms)));
        }
        groups.push((Kind::Files, first_matches(self.located.iter(), terms)));
        groups
    }

    fn items(&self) -> impl Iterator<Item = &Item> {
        self.found.iter().flat_map(|(_, items)| items.iter())
    }

    fn open(item: &Item, out: &mut Vec<Action>) {
        match &item.open {
            Open::Window(id) => out.push(Action::Focus(*id)),
            Open::Setting(name) => {
                out.push(Action::OpenPanel("settings".into(), (*name).to_string()))
            }
            Open::Copy(cmd) => {
                out.push(Action::RunBackground(cmd.clone()));
                out.push(Action::ClosePanel);
            }
            Open::Path(p) => out.push(Action::OpenUri(p.clone())),
        }
    }
}

impl Card for ResultsCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        if !self.shown {
            return false;
        }
        if let (true, Some(d)) = (key == "search", data.downcast_ref::<SearchData>()) {
            self.data = Some(d.clone());
        } else if let (true, Some(l)) = (key == "located", data.downcast_ref::<Located>()) {
            if l.query != self.query.trim() {
                return false;
            }
            let home = crate::paths::home();
            self.located = l
                .paths
                .iter()
                .filter_map(|p| {
                    let path = Path::new(p);
                    let note = path
                        .parent()
                        .map(|d| short(d, home.as_deref()))
                        .unwrap_or_default();
                    path_item(path, note)
                })
                .collect();
        } else {
            return false;
        }
        // no Cx here: the windows and settings found at the last keystroke stay as they were
        if self.terms.is_empty() {
            return false;
        }
        self.found
            .retain(|(k, _)| matches!(k, Kind::Windows | Kind::Settings));
        let mut read = self.read_kinds();
        read.retain(|(_, items)| !items.is_empty());
        self.found.extend(read);
        true
    }

    fn status(&self, _cx: &Cx) -> Status {
        if self.found.is_empty() {
            Status::Hidden
        } else {
            Status::Ready
        }
    }

    fn header(&self, _cx: &Cx) -> bool {
        false
    }

    fn height(&self, _cx: &Cx, _width: f64) -> f64 {
        self.found
            .iter()
            .map(|(_, items)| SUB_H + items.len() as f64 * ui::ROW_H)
            .sum()
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let mut y = body.1;
        let mut hit = HIT_BASE;
        for (kind, items) in &self.found {
            apps::draw_subhead(cr, cx.theme, body, y, kind.heading());
            y += SUB_H;
            for item in items {
                ui::row(
                    cr,
                    cx,
                    hits,
                    hit,
                    (body.0, y, body.2, ui::ROW_H),
                    &Row {
                        icon: None,
                        name: &item.name,
                        state: &item.note,
                        on: false,
                        right: 0.0,
                    },
                );
                hits.label(hit, format!("{} {}", kind.tag(), item.name));
                hit += 1;
                y += ui::ROW_H;
            }
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Text(q) => {
                self.shown = true;
                if self.query == *q {
                    return false;
                }
                let same_words = self.query.trim() == q.trim();
                self.query = (*q).to_string();
                self.terms = q.split_whitespace().map(str::to_lowercase).collect();
                // a space typed after a word: plocate's answer stands
                if !same_words {
                    self.located.clear();
                    // the run for the query before is killed; an empty one only cancels
                    out.push(Action::Locate(self.query.trim().to_string()));
                }
                let had = !self.found.is_empty();
                self.find(cx);
                had || !self.found.is_empty()
            }
            Event::Enter => {
                let Some(first) = self.items().next() else {
                    return false;
                };
                Self::open(first, out);
                true
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } if *hit >= HIT_BASE => {
                let Some(item) = self.items().nth((*hit - HIT_BASE) as usize) else {
                    return false;
                };
                Self::open(item, out);
                true
            }
            _ => false,
        }
    }

    fn panel_closed(&mut self) {
        *self = ResultsCard::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::card::Window;
    use crate::cards::settings::tests::test_cx;
    use crate::theme::Theme;
    use std::time::{Duration, Instant};

    fn theme() -> Theme {
        Theme::from_config(&crate::config::Config::from_text(""))
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("edge-apps-search-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(name: &str, note: &str, open: Open) -> Entry {
        Entry::new(Item {
            name: name.into(),
            note: note.into(),
            open,
        })
    }

    fn file(name: &str, note: &str) -> Entry {
        entry(name, note, Open::Path(format!("/home/a/{name}")))
    }

    /// The card with a panel open (its first Text) and this data.
    fn open_card(data: SearchData, cx: &Cx) -> ResultsCard {
        let mut c = ResultsCard::default();
        c.event(&Event::Text(""), cx, &mut Vec::new());
        assert!(!c.apply("search", &data as &dyn Any), "nothing typed yet");
        c
    }

    fn type_q(c: &mut ResultsCard, q: &str, cx: &Cx) -> Vec<Action> {
        let mut out = Vec::new();
        c.event(&Event::Text(q), cx, &mut out);
        out
    }

    fn shown(c: &ResultsCard) -> Vec<(&'static str, Vec<&str>)> {
        c.found
            .iter()
            .map(|(k, items)| (k.heading(), items.iter().map(|i| i.name.as_str()).collect()))
            .collect()
    }

    fn enter(c: &mut ResultsCard, cx: &Cx) -> Vec<Action> {
        let mut out = Vec::new();
        c.event(&Event::Enter, cx, &mut out);
        out
    }

    #[test]
    fn windows_match_by_title_and_app_and_enter_focuses_the_first() {
        let th = theme();
        let apps = [App {
            id: "org.mozilla.firefox".into(),
            name: "Firefox".into(),
            icon: String::new(),
            exec: "firefox".into(),
            categories: vec![],
            wm_class: None,
            terminal: false,
        }];
        let windows = [
            Window {
                id: 7,
                title: "Bank statement".into(),
                app_id: "org.mozilla.firefox".into(),
                focused: false,
            },
            Window {
                id: 8,
                title: "notes.txt".into(),
                app_id: "foot".into(),
                focused: true,
            },
        ];
        let mut cx = test_cx(&th);
        cx.apps = &apps;
        cx.windows = &windows;
        let mut c = open_card(SearchData::default(), &cx);
        type_q(&mut c, "fire bank", &cx);
        assert_eq!(shown(&c), [("Windows", vec!["Bank statement"])]);
        assert_eq!(c.found[0].1[0].note, "Firefox");
        assert_eq!(enter(&mut c, &cx), [Action::Focus(7)]);
        type_q(&mut c, "NOTES", &cx);
        assert_eq!(shown(&c), [("Windows", vec!["notes.txt"])]);
    }

    #[test]
    fn a_settings_row_is_found_by_its_label_and_opens_the_settings_panel_searching_it() {
        let th = theme();
        let cx = test_cx(&th);
        let mut c = open_card(SearchData::default(), &cx);
        type_q(&mut c, "reopen", &cx);
        assert_eq!(
            shown(&c),
            [(
                "Settings",
                vec!["Reopen apps", "Open a terminal when nothing was reopened"]
            )]
        );
        assert_eq!(c.found[0].1[0].note, "Apps");
        assert_eq!(
            enter(&mut c, &cx),
            [Action::OpenPanel("settings".into(), "Reopen apps".into())]
        );
        // every table is in the index: one row of each card
        for (q, card) in [
            ("x11 apps", "Apps"),
            ("terminal", "Apps"),
            (settings_display::TABLE[0].name, "Display"),
            (
                settings_recording::TABLE[0].name,
                "Screenshots and recording",
            ),
            (settings_look::TABLE_FONTS[0].name, "Fonts, icons, cursor"),
        ] {
            type_q(&mut c, q, &cx);
            assert!(
                c.found[0].1.iter().any(|i| i.note == card),
                "{q}: {:?}",
                shown(&c)
            );
        }
    }

    #[test]
    fn clipboard_clips_recordings_and_recent_files_are_grouped_in_that_order() {
        let th = theme();
        let cx = test_cx(&th);
        let data = SearchData {
            clipboard: vec![entry(
                "git push origin main",
                "10:02",
                Open::Copy("copy 1".into()),
            )],
            clips: vec![file("Clip 2026-11-20 push.mkv", "")],
            recordings: vec![file("push-ups.mkv", "")],
            recent: vec![file("push.md", "~/Documents")],
        };
        let mut c = open_card(data, &cx);
        type_q(&mut c, "push", &cx);
        assert_eq!(
            shown(&c),
            [
                ("Clipboard", vec!["git push origin main"]),
                ("Clips", vec!["Clip 2026-11-20 push.mkv"]),
                ("Recordings", vec!["push-ups.mkv"]),
                ("Recent files", vec!["push.md"]),
            ]
        );
        // a clipboard item is copied back in the background and the panel closes
        assert_eq!(
            enter(&mut c, &cx),
            [Action::RunBackground("copy 1".into()), Action::ClosePanel]
        );
        // a press on the clip (the second row) opens it
        let mut out = Vec::new();
        c.event(
            &Event::Press {
                hit: HIT_BASE + 1,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        );
        assert_eq!(
            out,
            [Action::OpenUri("/home/a/Clip 2026-11-20 push.mkv".into())]
        );
        // a name starting with the word comes first; at most PER_KIND a kind
        let many = SearchData {
            recent: (0..9)
                .map(|i| file(&format!("old push {i}.txt"), ""))
                .chain([file("push list.txt", "")])
                .collect(),
            ..Default::default()
        };
        let mut c = open_card(many, &cx);
        type_q(&mut c, "push", &cx);
        assert_eq!(
            shown(&c),
            [(
                "Recent files",
                vec![
                    "push list.txt",
                    "old push 0.txt",
                    "old push 1.txt",
                    "old push 2.txt"
                ]
            )]
        );
    }

    #[test]
    fn plocate_answers_only_for_the_query_typed_now() {
        let th = theme();
        let cx = test_cx(&th);
        let mut c = open_card(SearchData::default(), &cx);
        assert_eq!(type_q(&mut c, "tax", &cx), [Action::Locate("tax".into())]);
        assert_eq!(
            type_q(&mut c, "taxes", &cx),
            [Action::Locate("taxes".into())]
        );
        // the answer for "tax" came late: dropped
        let late = Located {
            query: "tax".into(),
            paths: vec!["/home/a/tax.pdf".into()],
        };
        assert!(!c.apply("located", &late as &dyn Any));
        assert!(c.found.is_empty());
        let now = Located {
            query: "taxes".into(),
            paths: vec![
                "/home/a/Documents/taxes 2026.pdf".into(),
                // plocate matched the folder, not the name: not shown
                "/home/a/taxes/notes.txt".into(),
            ],
        };
        assert!(c.apply("located", &now as &dyn Any));
        assert_eq!(shown(&c), [("Files", vec!["taxes 2026.pdf"])]);
        // a space after the word asks nothing again and keeps the answer
        assert!(type_q(&mut c, "taxes ", &cx).is_empty());
        assert_eq!(shown(&c), [("Files", vec!["taxes 2026.pdf"])]);
        assert_eq!(
            enter(&mut c, &cx),
            [Action::OpenUri("/home/a/Documents/taxes 2026.pdf".into())]
        );
        // emptied: plocate is only cancelled and nothing shows
        assert_eq!(type_q(&mut c, "", &cx), [Action::Locate(String::new())]);
        assert_eq!(c.status(&cx), Status::Hidden);
        assert!(enter(&mut c, &cx).is_empty());
    }

    #[test]
    fn the_card_keeps_nothing_while_its_panel_is_closed() {
        let th = theme();
        let cx = test_cx(&th);
        let data = SearchData {
            recent: vec![file("a.txt", "")],
            ..Default::default()
        };
        let mut c = open_card(data.clone(), &cx);
        type_q(&mut c, "a.txt", &cx);
        assert_eq!(shown(&c), [("Recent files", vec!["a.txt"])]);
        c.panel_closed();
        assert!(c.data.is_none() && c.found.is_empty() && c.query.is_empty());
        // data that arrives after the closing is not kept
        assert!(!c.apply("search", &data as &dyn Any));
        assert!(c.data.is_none());
    }

    #[test]
    fn height_is_what_is_drawn() {
        let th = theme();
        let cx = test_cx(&th);
        let data = SearchData {
            clipboard: vec![entry("note one", "", Open::Copy("c".into()))],
            recent: (0..6).map(|i| file(&format!("note {i}"), "~")).collect(),
            ..Default::default()
        };
        let mut c = open_card(data, &cx);
        type_q(&mut c, "note", &cx);
        let h = c.height(&cx, 400.0);
        assert_eq!(h, 2.0 * SUB_H + 5.0 * ui::ROW_H);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 600).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 400.0, h), &mut hits);
        let ids: Vec<u32> = hits.list.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, (HIT_BASE..HIT_BASE + 5).collect::<Vec<_>>());
        let bottom = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        assert_eq!(bottom, h);
    }

    #[test]
    fn draw_labels_each_hit_with_its_kind_and_name() {
        let th = theme();
        let mut cx = test_cx(&th);
        let win = [Window {
            id: 1,
            title: "find window".into(),
            app_id: "app".into(),
            focused: false,
        }];
        cx.windows = &win;
        let data = SearchData {
            clipboard: vec![entry("find clip", "", Open::Copy("find clip".into()))],
            clips: vec![file("find-clip.mkv", "")],
            recordings: vec![file("find-rec.mp4", "")],
            recent: vec![file("find-recent.txt", "")],
        };
        let mut c = open_card(data, &cx);
        type_q(&mut c, "find", &cx);
        c.apply(
            "located",
            &crate::locate::Located {
                query: "find".into(),
                paths: vec!["/tmp/find-file.pdf".into()],
            } as &dyn Any,
        );
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 600).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 400.0, c.height(&cx, 400.0)), &mut hits);
        let labels: Vec<&str> = hits
            .texts
            .iter()
            .filter_map(|(id, text)| id.is_some().then_some(text.as_str()))
            .collect();
        assert_eq!(
            labels,
            vec![
                "windows find window",
                "clipboard find clip",
                "clips find-clip.mkv",
                "recordings find-rec.mp4",
                "recent find-recent.txt",
                "files find-file.pdf",
            ]
        );
        assert_eq!(labels.len(), hits.list.len());

        type_q(&mut c, "night", &cx);
        let mut setting_hits = Hits::new(None, None);
        c.draw(
            &cr,
            &cx,
            (0.0, 0.0, 400.0, c.height(&cx, 400.0)),
            &mut setting_hits,
        );
        assert!(
            setting_hits
                .texts
                .iter()
                .any(|(id, text)| *id == Some(HIT_BASE) && text == "settings Night light warmth"),
            "{:?}",
            setting_hits.texts
        );
    }

    #[test]
    fn the_source_reads_the_folders_from_the_config_and_the_recent_files_newest_first() {
        let home = temp_dir("source");
        let clips = home.join("Videos/Clips");
        let recs = home.join("Rec");
        std::fs::create_dir_all(clips.join("a folder")).unwrap();
        std::fs::create_dir_all(&recs).unwrap();
        std::fs::write(clips.join("older.mkv"), "").unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(clips.join("older.mkv"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        std::fs::write(clips.join("newer.mkv"), "").unwrap();
        std::fs::write(recs.join("talk.mp4"), "").unwrap();
        // the owner's line wins over the default; a key only in the defaults is read from there
        std::fs::write(
            home.join("config"),
            format!("RECORDINGS_DIR=\"{}\"\n", recs.display()),
        )
        .unwrap();
        std::fs::write(
            home.join("defaults"),
            "CLIPS_DIR=\"$HOME/Videos/Clips\"\nRECORDINGS_DIR=\"$HOME/nowhere\"\n",
        )
        .unwrap();
        std::fs::write(
            home.join("recent.xbel"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0">
  <bookmark href="file://{h}/Docs/old%20plan.odt" added="2026-01-01T10:00:00Z" modified="2026-01-02T10:00:00Z" visited="2026-01-03T10:00:00Z">
  </bookmark>
  <bookmark href="https://example.org/page" added="2026-11-01T10:00:00Z" modified="2026-11-01T10:00:00Z" visited="2026-11-01T10:00:00Z"/>
  <bookmark href="file://{h}/Docs/R&amp;D.txt" added="2026-11-20T10:00:00Z" modified="2026-11-20T10:00:00Z" visited="2026-11-20T10:00:00Z"/>
</xbel>
"#,
                h = home.display()
            ),
        )
        .unwrap();
        let mut src = SearchSource {
            home: Some(home.clone()),
            config: home.join("config"),
            defaults: home.join("defaults"),
            recent_file: home.join("recent.xbel"),
            clipboard: clipboard::ClipboardSource {
                path: Some(home.join("no-bin").into_os_string()),
            },
        };
        let d = src.read();
        let names = |v: &[Entry]| v.iter().map(|e| e.item.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&d.clips), ["newer.mkv", "older.mkv"]);
        assert_eq!(names(&d.recordings), ["talk.mp4"]);
        assert_eq!(names(&d.recent), ["R&D.txt", "old plan.odt"]);
        assert_eq!(d.recent[0].item.note, "~/Docs");
        assert_eq!(
            d.recent[1].item.open,
            Open::Path(format!("{}/Docs/old plan.odt", home.display()))
        );
        assert!(d.clipboard.is_empty(), "cliphist is not on that PATH");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn enter_on_a_settings_row_opens_the_settings_panel_with_its_label_typed() {
        let mut sh = crate::shell::Shell::new(crate::config::Config::from_text(""), Vec::new());
        let at = |sh: &crate::shell::Shell, name: &str| {
            sh.panels.iter().position(|p| p.panel.name == name).unwrap()
        };
        let apps = at(&sh, "apps");
        assert!(sh.open_panel(apps, false, Instant::now()));
        sh.type_text("x11 apps and");
        assert!(sh.key(crate::shell::Key::Enter));
        assert_eq!(sh.open, Some(at(&sh, "settings")));
        assert_eq!(sh.query, "X11 apps and games");
    }

    /// The budget: a keystroke's results in under 16 ms with 500 windows, 500 apps and 500 entries
    /// of each kind read from disk, through the whole launcher (its search, favorites, apps list and
    /// these results), typing and the frame drawn.
    #[test]
    fn a_keystroke_with_500_windows_apps_and_entries_is_typed_and_drawn_in_16_ms() {
        let apps: Vec<App> = (0..500)
            .map(|i| App {
                id: format!("app{i}"),
                name: format!("App number {i}"),
                icon: String::new(),
                exec: format!("app{i}"),
                categories: vec!["Utility".into()],
                wm_class: None,
                terminal: false,
            })
            .collect();
        let mut sh = crate::shell::Shell::new(crate::config::Config::from_text(""), apps);
        sh.windows = (0..500)
            .map(|i| Window {
                id: i,
                title: format!("Document {i} - Editor"),
                app_id: format!("app{i}"),
                focused: false,
            })
            .collect();
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "apps")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        let many = |what: &str| -> Vec<Entry> {
            (0..500)
                .map(|i| file(&format!("{what} {i} document.txt"), "~/Documents"))
                .collect()
        };
        let data = SearchData {
            clipboard: (0..500)
                .map(|i| entry(&format!("copied text {i}"), "", Open::Copy("c".into())))
                .collect(),
            clips: many("clip"),
            recordings: many("recording"),
            recent: many("recent"),
        };
        assert!(!sh.apply("apps", "search", &data as &dyn Any));
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        let cr = Context::new(&surf).unwrap();
        sh.draw(&cr, 1280.0, 800.0);
        let mut worst = Duration::ZERO;
        let mut query = String::new();
        // "document 1": every kind matches as the words grow
        for ch in "document 1".chars() {
            query.push(ch);
            let t = Instant::now();
            sh.type_text(&query);
            sh.draw(&cr, 1280.0, 800.0);
            worst = worst.max(t.elapsed());
        }
        let shown = sh.panels[idx].cards.iter().any(|pc| {
            pc.word.kind == "results" && pc.card.status(&test_cx(&theme())) == Status::Ready
        });
        assert!(shown, "the results card shows");
        eprintln!("slowest keystroke with 500 of each: {worst:?}");
        assert!(
            worst < Duration::from_millis(16),
            "slowest keystroke {worst:?}"
        );
    }
}
