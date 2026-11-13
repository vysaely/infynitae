//! The card (docs/EDGE_APPS.md sections 3.2 to 3.4): sources that fetch off the UI thread, what a card
//! can read (`Cx`), its status, the `Card` trait, the hit rectangles that give hover and clicks, the
//! input it gets, and the actions it asks the core for. Cards are plain state: no Wayland, no
//! threads, no process started from them.

use crate::app::App;
use crate::provider::Size;
use crate::theme::Theme;
use std::collections::HashMap;
use std::process::Command;

pub type Rect = (f64, f64, f64, f64);

/// What a source hands back: the provider's own type, read by its cards.
pub type Data = Box<dyn std::any::Any + Send>;

/// Fetches one piece of data on a worker thread; may block (a command, D-Bus, a file).
pub trait Source: Send {
    /// A failure is part of the data (the card shows it), not an error.
    fn fetch(&mut self) -> Data;
    /// How often while a card that needs it is shown. None: once each time the panel opens.
    fn every(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs(2))
    }
}

/// An open window (from the compositor's foreign-toplevel list).
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: u64,
    pub title: String,
    pub app_id: String,
    pub focused: bool,
}

/// A notification the edge's own server kept (newest first).
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: u32,
    /// the sender's name, or its desktop entry when it gave none
    pub app: String,
    pub summary: String,
    pub body: String,
    /// (key, label); "default" is a click on it
    pub actions: Vec<(String, String)>,
    /// 0 low, 1 normal, 2 critical
    pub urgency: u8,
    /// "HH:MM" it came
    pub time: String,
}

/// What a card can read while it measures, draws or handles input. Fixed in tests.
pub struct Cx<'a> {
    pub theme: &'a Theme,
    pub now: chrono::NaiveDateTime,
    /// the size this card is placed in
    pub size: Size,
    /// the app the card is tied to; None for a provider's own card
    pub app: Option<&'a App>,
    pub apps: &'a [App],
    pub windows: &'a [Window],
    pub notes: &'a [Note],
    /// the panel is at the bottom edge: lists open over their row
    pub list_above: bool,
    pub editing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Ready,
    /// no data yet: the core draws the header and "Loading"
    Loading,
    /// nothing to show now: no card at all (a placeholder with the title while editing)
    Hidden,
    /// it cannot work, said in one sentence; the core draws the header and this line
    Missing(String),
}

pub trait Card {
    /// New data for one of its sources (`data` is the provider's type: downcast_ref it). True when
    /// what the card shows changed.
    fn apply(&mut self, key: &str, data: &dyn std::any::Any) -> bool;

    fn status(&self, cx: &Cx) -> Status;

    /// Whether the core draws its header (title and app). A card that says what it is by itself
    /// (a search field) goes without.
    fn header(&self, _cx: &Cx) -> bool {
        true
    }

    /// The header's title when it is not the kind's.
    fn title(&self, _cx: &Cx) -> Option<String> {
        None
    }

    /// The body's height at `width` (the header and padding are the core's), with a list opened in
    /// place at its present height.
    fn height(&self, cx: &Cx, width: f64) -> f64;

    /// Draws the body into `body`; every clickable part is added to `hits`.
    fn draw(&self, cr: &cairo::Context, cx: &Cx, body: Rect, hits: &mut Hits);

    /// Input on one of its hits; what should happen outside the card goes to `out`. True when the
    /// card must be drawn again.
    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool;

    /// Moves its animations on by `dt_ms`. True while one is still running.
    fn tick(&mut self, _dt_ms: f64) -> bool {
        false
    }
    /// For a still picture (`--render` with INFYNITAE_EDGE_RENDER_SUB): its list shown fully open.
    fn show_list(&mut self) {}
}

/// The clickable rectangles of one card in one frame, with the pointer and the keyboard's selection.
#[derive(Clone, Debug, Default)]
pub struct Hits {
    /// where the pointer is, in the card's body coordinates
    pub pointer: Option<(f64, f64)>,
    /// the hit the keyboard's selection is on
    pub selected: Option<u32>,
    pub list: Vec<(u32, Rect)>,
}

fn inside(r: Rect, (x, y): (f64, f64)) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

impl Hits {
    pub fn new(pointer: Option<(f64, f64)>, selected: Option<u32>) -> Hits {
        Hits {
            pointer,
            selected,
            list: Vec::new(),
        }
    }

    /// Registers a clickable rectangle under the card's own `id`. True when the pointer is over it:
    /// draw its light. A hit added later lies on top and takes the click (a More button added after
    /// its row: both are lit, the button over the row).
    pub fn add(&mut self, id: u32, rect: Rect) -> bool {
        self.list.push((id, rect));
        self.pointer.is_some_and(|p| inside(rect, p))
    }

    /// True when the keyboard's selection is on `id`.
    pub fn selected(&self, id: u32) -> bool {
        self.selected == Some(id)
    }

    /// The topmost hit under `p` (the last added that holds it).
    pub fn top_at(&self, p: (f64, f64)) -> Option<u32> {
        self.list
            .iter()
            .rev()
            .find(|(_, r)| inside(*r, p))
            .map(|(id, _)| *id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event<'a> {
    Press {
        hit: u32,
        button: Button,
        at: (f64, f64),
    },
    /// the pointer moved with the button held on `hit`
    Drag {
        hit: u32,
        at: (f64, f64),
    },
    Release {
        hit: u32,
    },
    /// None: over the card but on no hit
    Scroll {
        hit: Option<u32>,
        dy: f64,
    },
    /// what was typed, for a card with a search field (the whole query so far)
    Text(&'a str),
    /// Enter with nothing selected: the card's own default (a search's first result), if it has one
    Enter,
}

/// What a card asks the core to do. Cards never start a process or touch a window themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// sh -c, off the UI thread, one at a time per card
    Run(String),
    /// an app by id: its Exec line
    Launch(String),
    /// "steam://rungameid/730", a folder
    OpenUri(String),
    Focus(u64),
    CloseWindow(u64),
    /// a notification
    Dismiss(u32),
    /// a notification's action
    Invoke(u32, String),
    ClosePanel,
    /// the panel's typed query becomes this (a search field's Clear: "")
    SetQuery(String),
}

/// The fixture's values (`INFYNITAE_EDGE_RENDER_DATA`: `key=value` lines, `\n` and `\t` escaped),
/// read once.
fn render_data() -> Option<&'static HashMap<String, String>> {
    static DATA: std::sync::OnceLock<Option<HashMap<String, String>>> = std::sync::OnceLock::new();
    DATA.get_or_init(|| {
        let path = std::env::var_os("INFYNITAE_EDGE_RENDER_DATA")?;
        let text = std::fs::read_to_string(path).ok()?;
        Some(parse_render_data(&text))
    })
    .as_ref()
}

pub fn parse_render_data(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| {
            let mut out = String::with_capacity(v.len());
            let mut it = v.chars();
            while let Some(c) = it.next() {
                if c == '\\' {
                    match it.next() {
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('\\') => out.push('\\'),
                        Some(o) => {
                            out.push('\\');
                            out.push(o)
                        }
                        None => out.push('\\'),
                    }
                } else {
                    out.push(c)
                }
            }
            (k.trim().to_string(), out)
        })
        .collect()
}

/// A render fixture's value for `key` (None outside renders, or not in the fixture).
pub fn render_value(key: &str) -> Option<String> {
    render_data()?.get(key).cloned()
}

/// The render fixture's `windows` (one per line: `id<TAB>app id<TAB>title<TAB>focused 0/1`) and
/// `notes` (`id<TAB>app<TAB>summary<TAB>body<TAB>time`): what the Wayland side and the notification
/// server give outside renders.
pub fn render_windows() -> Vec<Window> {
    render_value("windows")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Some(Window {
                id: f.first()?.trim().parse().ok()?,
                app_id: f.get(1)?.to_string(),
                title: f.get(2).unwrap_or(&"").to_string(),
                focused: f.get(3).is_some_and(|v| v.trim() == "1"),
            })
        })
        .collect()
}

pub fn render_notes() -> Vec<Note> {
    render_value("notes")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Some(Note {
                id: f.first()?.trim().parse().ok()?,
                app: f.get(1)?.to_string(),
                summary: f.get(2).unwrap_or(&"").to_string(),
                body: f.get(3).unwrap_or(&"").to_string(),
                actions: vec![],
                urgency: 1,
                time: f.get(4).unwrap_or(&"").to_string(),
            })
        })
        .collect()
}

/// Runs `cmd` with sh and gives its output; with a render fixture set, the fixture's value for `key`
/// instead (nothing runs).
pub fn run_sh(key: &str, cmd: &str) -> String {
    if let Some(d) = render_data() {
        return d.get(key).cloned().unwrap_or_default();
    }
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_topmost_hit_takes_the_pointer() {
        let mut h = Hits::new(Some((50.0, 10.0)), None);
        assert!(h.add(1, (0.0, 0.0, 100.0, 20.0)));
        assert!(h.add(2, (40.0, 0.0, 20.0, 20.0)));
        assert_eq!(h.top_at((50.0, 10.0)), Some(2), "added later, on top");
        assert_eq!(h.top_at((10.0, 10.0)), Some(1));
        assert_eq!(h.top_at((10.0, 30.0)), None);
    }

    #[test]
    fn fixture_values_unescape() {
        let d = parse_render_data("wifi=enabled\\nyes:Home\\\\:Net:72\nempty=\n");
        assert_eq!(d["wifi"], "enabled\nyes:Home\\:Net:72");
        assert_eq!(d["empty"], "");
    }
}
