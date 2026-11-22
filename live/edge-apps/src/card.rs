//! The card (docs/EDGE_APPS.md sections 3.2 to 3.4): sources that fetch off the UI thread, what a card
//! can read (`Cx`), its status, the `Card` trait, the hit rectangles that give hover and clicks, the
//! input it gets, and the actions it asks the core for. Cards are plain state: no Wayland, no
//! threads, no process started from them.

use crate::app::App;
use crate::provider::Size;
use crate::theme::Theme;
use std::collections::HashMap;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
    /// the apps started lately, for the order of search results
    pub recent: &'a crate::recent::Recent,
    pub windows: &'a [Window],
    pub notes: &'a [Note],
    /// the panel is at the bottom edge: lists open over their row
    pub list_above: bool,
    pub editing: bool,
    /// the body height this card may take: infinite unless its panel would be taller than the
    /// screen, then a flexible card (`Card::flexible`) gets less and shows fewer items
    pub room: f64,
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

    /// It can show less when its panel is taller than the screen (a list that shows fewer items
    /// and counts the rest in its title): the panel gives it less `Cx::room` instead of cutting a
    /// card in half at the screen's edge.
    fn flexible(&self) -> bool {
        false
    }

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
    /// Called when the panel containing this card closes.
    fn panel_closed(&mut self) {}
    /// True when the card reads `cx.now` and should redraw on minute boundaries while open.
    fn wants_clock(&self) -> bool {
        false
    }
    /// Called when an `Action::Run` or `Action::RunBackground` from this card ends.
    /// True when what the card shows changed.
    fn command_done(&mut self, _res: &CommandResult, _now: Instant) -> bool {
        false
    }
    /// Next time transient command feedback on this card expires.
    fn next_deadline(&self) -> Option<Instant> {
        None
    }
    /// Clears expired transient feedback at `now`. True when what the card shows changed.
    fn expire(&mut self, _now: Instant) -> bool {
        false
    }
}

pub const MAX_STDERR_BYTES: usize = 200;

/// Outcome of a finished `Action::Run` or `Action::RunBackground` shell command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResult {
    pub command: String,
    /// Exit code, or `None` when killed by a signal or failed to start.
    pub status: Option<i32>,
    /// Last non-empty stderr line (or spawn error), stripped of CR/ANSI escapes and bounded to 200 bytes.
    pub stderr_line: String,
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            if let Some(next) = it.next() {
                if next == '[' {
                    for b in it.by_ref() {
                        if ('@'..='~').contains(&b) {
                            break;
                        }
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Returns the last non-empty stderr line, cleaned of `\r` progress overwrites and ANSI escapes
/// and capped at `MAX_STDERR_BYTES` on a UTF-8 boundary.
pub fn last_stderr_line(bytes: &[u8]) -> String {
    let raw = String::from_utf8_lossy(bytes);
    for line in raw.split(['\n', '\r']).rev() {
        let clean = strip_ansi(line);
        let trimmed = clean.trim();
        if !trimmed.is_empty() {
            return truncate_utf8(trimmed, MAX_STDERR_BYTES)
                .trim_end()
                .to_string();
        }
    }
    String::new()
}

/// Pixels moved per discrete mouse wheel notch (120 units in Wayland).
pub const SCROLL_NOTCH_PX: f64 = 36.0;

/// The clickable rectangles of one card in one frame, with the pointer and the keyboard's selection.
#[derive(Clone, Debug, Default)]
pub struct Hits {
    /// where the pointer is, in the card's body coordinates
    pub pointer: Option<(f64, f64)>,
    /// the hit the keyboard's selection is on
    pub selected: Option<u32>,
    pub list: Vec<(u32, Rect)>,
    /// hits that respond to a pointer/touch drag (sliders) instead of panel scrolling
    pub draggable: Vec<u32>,
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
            draggable: Vec::new(),
        }
    }

    /// Registers a clickable rectangle under the card's own `id`. True when the pointer is over it:
    /// draw its light. A hit added later lies on top and takes the click (a More button added after
    /// its row: both are lit, the button over the row).
    pub fn add(&mut self, id: u32, rect: Rect) -> bool {
        self.list.push((id, rect));
        self.pointer.is_some_and(|p| inside(rect, p))
    }

    /// Marks `id` as a draggable control (slider) so touch drags move it instead of scrolling.
    pub fn mark_draggable(&mut self, id: u32) {
        if !self.draggable.contains(&id) {
            self.draggable.push(id);
        }
    }

    /// True when `id` is a draggable control.
    pub fn is_draggable(&self, id: u32) -> bool {
        self.draggable.contains(&id)
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
    /// an active touch sequence on `hit` was cancelled by the compositor
    Cancel {
        hit: u32,
    },
    /// None: over the card but on no hit
    Scroll {
        hit: Option<u32>,
        /// scroll distance in pixels (positive = down)
        dy: f64,
        /// discrete wheel notches when present, 0.0 for continuous touchpad scrolling
        notches: f64,
    },
    /// Left/Right (-1 / +1) on the keyboard-selected hit (sliders, choice strips)
    Step {
        hit: u32,
        dir: i8,
    },
    /// Delete on the keyboard-selected hit, for cards with an item remove action
    Delete {
        hit: u32,
    },
    /// what was typed, for a card with a search field (the whole query so far)
    Text(&'a str),
    /// Enter with nothing selected: the card's own default (a search's first result), if it has one
    Enter,
}

/// Converts discrete wheel notches or continuous pixel deltas into integer step counts.
pub fn scroll_steps(accum: &mut f64, dy: f64, notches: f64, step_px: f64) -> isize {
    let delta = if notches != 0.0 {
        notches * step_px
    } else {
        dy
    };
    if delta == 0.0 || step_px <= 0.0 {
        return 0;
    }
    if *accum != 0.0 && delta.signum() != accum.signum() {
        *accum = 0.0;
    }
    *accum += delta;
    let steps = (*accum / step_px).trunc() as isize;
    *accum -= steps as f64 * step_px;
    steps
}

/// Steps a row-indexed scroll offset by wheel notches or accumulated touchpad pixels.
/// Returns `true` when the card consumed the event (changed rows or absorbed a sub-step swipe
/// while it still has room to scroll in that direction), `false` at a boundary to hand over.
pub fn scroll_rows(
    scroll: &mut usize,
    accum: &mut f64,
    max_scroll: usize,
    dy: f64,
    notches: f64,
) -> bool {
    let cur = (*scroll).min(max_scroll);
    let dir = if notches != 0.0 { notches } else { dy };
    if max_scroll == 0 || dir == 0.0 {
        *accum = 0.0;
        return false;
    }
    let can_scroll = (dir > 0.0 && cur < max_scroll) || (dir < 0.0 && cur > 0);
    if !can_scroll {
        *accum = 0.0;
        return false;
    }
    let steps = scroll_steps(accum, dy, notches, SCROLL_NOTCH_PX);
    if steps == 0 {
        return true;
    }
    let next = if steps > 0 {
        cur.saturating_add(steps as usize).min(max_scroll)
    } else {
        cur.saturating_sub((-steps) as usize)
    };
    if next == cur {
        *accum = 0.0;
        return false;
    }
    *scroll = next;
    true
}

/// What a card asks the core to do. Cards never start a process or touch a window themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// sh -c, off the UI thread, one at a time per card
    Run(String),
    /// sh -c, off the UI thread, in its own process outside the card's queue
    RunBackground(String),
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
    /// these cards (words, as in a panel line) in the open panel's place until it closes: a tray
    /// app's menu and card
    ShowCards(Vec<String>),
    /// remove this card from the panel line and save
    RemoveCard,
    /// a JSON request line sent to `$XDG_RUNTIME_DIR/infynitae/shell.sock`
    ShellCommand(String),
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

/// Whether a render fixture (`INFYNITAE_EDGE_RENDER_DATA`) is active.
pub fn has_render_data() -> bool {
    render_data().is_some()
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

const RUN_TIMEOUT: Duration = Duration::from_secs(5);

fn kill_process_group(child: &mut Child) {
    #[cfg(unix)]
    {
        let pgid = child.id() as libc::pid_t;
        if pgid > 0 {
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn run_command_with_timeout(mut cmd: Command, limit: Duration) -> String {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let Some(mut stdout) = child.stdout.take() else {
        kill_process_group(&mut child);
        return String::new();
    };

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    let deadline = Instant::now() + limit;
    match rx.recv_timeout(limit) {
        Ok(buf) => loop {
            match child.try_wait() {
                Ok(Some(_)) => return String::from_utf8_lossy(&buf).into_owned(),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                _ => {
                    kill_process_group(&mut child);
                    return String::new();
                }
            }
        },
        Err(_) => {
            kill_process_group(&mut child);
            String::new()
        }
    }
}

/// Runs `cmd` with sh within `RUN_TIMEOUT` (5 s) and gives its output; with a render fixture set,
/// returns the fixture's value for `key` instead (nothing runs). On timeout the process group is
/// killed and an empty string is returned so the card shows its Missing status.
pub fn run_sh(key: &str, cmd: &str) -> String {
    if let Some(d) = render_data() {
        return d.get(key).cloned().unwrap_or_default();
    }
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    run_command_with_timeout(c, RUN_TIMEOUT)
}

/// Runs `prog` directly with `args` (no shell, `LC_ALL=C`) within `RUN_TIMEOUT` (5 s) and gives
/// its stdout; with a render fixture set, returns the fixture's value for `key` instead.
pub fn run_cmd(key: &str, prog: &str, args: &[&str]) -> String {
    if let Some(d) = render_data() {
        return d.get(key).cloned().unwrap_or_default();
    }
    let mut c = Command::new(prog);
    c.args(args).env("LC_ALL", "C");
    run_command_with_timeout(c, RUN_TIMEOUT)
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

    #[test]
    fn run_sh_returns_stdout_of_fast_command() {
        assert_eq!(run_sh("k", "printf 'ok\\n'"), "ok\n");
    }

    #[test]
    fn run_sh_times_out_and_kills_stuck_command() {
        let started = Instant::now();
        let out = run_sh("k", "sleep 30");
        let elapsed = started.elapsed();
        assert_eq!(out, "");
        assert!(
            elapsed < Duration::from_secs(7),
            "run_sh took {elapsed:?}, expected ~5 s timeout"
        );
    }

    #[test]
    fn scroll_rows_accumulates_touchpad_pixels_and_hands_over_at_bounds() {
        let mut scroll = 0usize;
        let mut accum = 0.0;
        // At top, scrolling up cannot scroll the list and hands over immediately.
        assert!(!scroll_rows(&mut scroll, &mut accum, 3, -2.5, 0.0));
        assert_eq!(scroll, 0);

        // Sub-step touchpad swipe down is absorbed without leaking until it reaches one notch.
        for _ in 0..14 {
            assert!(scroll_rows(&mut scroll, &mut accum, 3, 2.5, 0.0));
        }
        assert_eq!(scroll, 0);
        // 15th * 2.5 = 37.5 >= 36.0 advances 1 row.
        assert!(scroll_rows(&mut scroll, &mut accum, 3, 2.5, 0.0));
        assert_eq!(scroll, 1);

        // Two discrete wheel notches hit the bottom and the next notch hands over.
        assert!(scroll_rows(&mut scroll, &mut accum, 3, 72.0, 2.0));
        assert_eq!(scroll, 3);
        assert!(!scroll_rows(&mut scroll, &mut accum, 3, 36.0, 1.0));
    }

    #[test]
    fn last_stderr_line_picks_last_non_empty_strips_ansi_and_caps_200_bytes() {
        assert_eq!(last_stderr_line(b""), "");
        assert_eq!(last_stderr_line(b"\n  \r\n\t\n"), "");
        assert_eq!(
            last_stderr_line(b"first line\n\x1b[31m  Error: failed\x1b[0m\r\n\n  \n"),
            "Error: failed"
        );
        assert_eq!(
            last_stderr_line(b"scanning...\rFailed to pair"),
            "Failed to pair"
        );
        let long = "é".repeat(120); // 240 bytes
        let cut = last_stderr_line(long.as_bytes());
        assert!(cut.len() <= MAX_STDERR_BYTES);
        assert_eq!(cut.len(), 200);
        assert!(std::str::from_utf8(cut.as_bytes()).is_ok());
    }
}
