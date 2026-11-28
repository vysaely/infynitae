//! The edge's own notifications (in place of mako): what the D-Bus server
//! (`crate::notify_server`) received, the pop-ups at the top right, the bottom-centre OSD for
//! synchronous level keys, and the history the Notifications card lists. Pure state, layout and
//! drawing; the server thread and surfaces live in `notify_server.rs` and `wayland.rs`.
//!
//! - A pop-up stays `timeout` (the sender's, else 4 s low, 6 s normal, never critical), pauses
//!   while pointed at, and goes on a click (its default action), its x, one of its buttons, or
//!   `CloseNotification`.
//! - Synchronous level notifications (`x-canonical-private-synchronous` with `int:value`, and
//!   `volume` / `brightness` tags such as `launch.sh` mute) show as one bottom-centre OSD without
//!   entering the history, gone after 1.2 s by default and replaced on each press.
//! - One with the same `x-canonical-private-synchronous` / `synchronous` tag replaces the last one.
//! - Do not disturb (`$XDG_RUNTIME_DIR/infynitae/do-not-disturb`) shows only critical ones; a game
//!   (`.../game-quiet`, launch.sh's GAME_QUIET) shows none: a pop-up over a game ends its direct
//!   path to the screen. Either way non-OSD notifications go to the history.

use crate::card::Rect;
use crate::draw::{self, Text, PAD_X};
use crate::theme::Theme;
use cairo::Context;
use std::time::{Duration, Instant};

/// Pop-ups shown at once (more wait in the history).
pub const MAX_POPUPS: usize = 4;
/// What the history keeps.
pub const MAX_HISTORY: usize = 50;
pub const POPUP_W: f64 = 360.0;
pub const POPUP_GAP: f64 = 12.0;
/// From the screen's top and right edge (and bottom margin for the OSD).
pub const MARGIN: f64 = 24.0;
pub const OSD_W: f64 = 240.0;
pub const OSD_H: f64 = 56.0;
pub const OSD_TIMEOUT: Duration = Duration::from_millis(1200);
const PAD_Y: f64 = 14.0;
const APP_H: f64 = 18.0;
const TITLE_H: f64 = 20.0;
const BODY_LINE: f64 = 17.0;
const BODY_LINES: usize = 3;
const BUTTON_H: f64 = 28.0;
const BAR_H: f64 = 22.0;
/// How long a pop-up takes to unfold from the right edge.
pub const ARRIVE_MS: f64 = 165.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    /// (key, label); "default" is the click on the pop-up itself, not a button
    pub actions: Vec<(String, String)>,
    /// 0 low, 1 normal, 2 critical
    pub urgency: u8,
    /// a level, 0 to 100 (the volume keys' "value" hint)
    pub value: Option<i32>,
    /// the synchronous tag: the next one with it replaces this one
    pub sync: Option<String>,
    /// how long it shows: None never goes by itself; Some(None) the default for its urgency
    pub expire: Option<Option<Duration>>,
    /// not kept in the history ("transient" hint, and levels)
    pub transient: bool,
    /// stays after an action ("resident" hint)
    pub resident: bool,
    /// "HH:MM" it came
    pub time: String,
}

impl Note {
    /// The action buttons (everything but "default").
    pub fn buttons(&self) -> impl Iterator<Item = &(String, String)> {
        self.actions.iter().filter(|(k, _)| k != "default")
    }
    /// How long it shows from now; None: until dismissed.
    pub fn shown_for(&self) -> Option<Duration> {
        match self.expire {
            None => None,
            Some(Some(d)) => Some(d),
            Some(None) if self.value.is_some() => Some(Duration::from_millis(2000)),
            Some(None) => match self.urgency {
                0 => Some(Duration::from_secs(4)),
                2 => None,
                _ => Some(Duration::from_secs(6)),
            },
        }
    }

    /// How long an OSD shows: the sender's timeout when positive, else 1.2 s.
    pub fn osd_shown_for(&self) -> Duration {
        match self.expire {
            Some(Some(d)) => d,
            _ => OSD_TIMEOUT,
        }
    }
}

/// Active bottom-centre on-screen display for synchronous volume/brightness keys.
#[derive(Clone, Debug, PartialEq)]
pub struct Osd {
    pub note: Note,
    pub label: String,
    pub value: i32,
    pub muted: bool,
    pub until: Instant,
}

/// Synchronous notifications with a value hint (or volume/brightness tags such as `launch.sh` mute)
/// render as the bottom-centre OSD instead of a notification pop-up or history entry.
pub fn is_osd_note(note: &Note) -> bool {
    note.sync.is_some()
        && (note.value.is_some() || matches!(note.sync.as_deref(), Some("volume" | "brightness")))
}

fn contains_word_ci(haystack: &str, word: &str) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w.eq_ignore_ascii_case(word))
}

fn first_percent(text: &str) -> Option<i32> {
    let b = text.as_bytes();
    let i = text.find('%')?;
    let mut j = i;
    while j > 0 && b[j - 1].is_ascii_digit() {
        j -= 1;
    }
    (j < i)
        .then(|| text[j..i].parse::<i32>().ok())
        .flatten()
        .map(|v| v.clamp(0, 100))
}

fn osd_from_note(note: Note, until: Instant) -> Osd {
    let muted = contains_word_ci(&note.summary, "muted");
    let sync_tag = note.sync.as_deref().unwrap_or("");
    let label = if muted {
        "Muted".to_string()
    } else if sync_tag.eq_ignore_ascii_case("brightness")
        || contains_word_ci(&note.summary, "brightness")
        || note.app.eq_ignore_ascii_case("display")
    {
        "Brightness".to_string()
    } else if sync_tag.eq_ignore_ascii_case("volume")
        || contains_word_ci(&note.summary, "volume")
        || contains_word_ci(&note.summary, "unmuted")
        || note.app.eq_ignore_ascii_case("audio")
    {
        "Volume".to_string()
    } else {
        note.summary
            .split(':')
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| note.sync.clone())
            .unwrap_or_default()
    };
    let value = if muted {
        0
    } else {
        note.value
            .or_else(|| first_percent(&note.summary))
            .unwrap_or(0)
            .clamp(0, 100)
    };
    Osd {
        note,
        label,
        value,
        muted,
        until,
    }
}

/// What the server thread hands the edge.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Notify(Note),
    /// CloseNotification(id)
    Close(u32),
}

/// What the edge tells the senders (the server emits it as a D-Bus signal).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outgoing {
    /// ActionInvoked(id, key)
    Action(u32, String),
    /// NotificationClosed(id, reason): 1 expired, 2 dismissed, 3 closed by CloseNotification
    Closed(u32, u32),
}

#[derive(Clone, Debug)]
pub struct Popup {
    pub note: Note,
    pub born: Instant,
    /// when it goes by itself (pushed on while pointed at)
    pub until: Option<Instant>,
}

#[derive(Clone, Debug, Default)]
pub struct Notify {
    /// the edge serves org.freedesktop.Notifications (the card lists `history`, not makoctl)
    pub serving: bool,
    /// newest first
    pub popups: Vec<Popup>,
    /// newest first
    pub history: Vec<Note>,
    /// the active bottom-centre OSD, replaced in place
    pub osd: Option<Osd>,
    /// the pop-up under the pointer (its time is held)
    pub hovered: Option<u32>,
    /// signals to send
    pub out: Vec<Outgoing>,
}

/// Why pop-ups are held back now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quiet {
    No,
    /// do not disturb: critical ones still show
    Dnd,
    /// a game: none show
    Game,
}

/// Do not disturb or a game, from the files the switches and launch.sh keep.
pub fn quiet_now() -> Quiet {
    let dir = crate::paths::runtime_path("infynitae");
    if dir.join("game-quiet").exists() {
        Quiet::Game
    } else if dir.join("do-not-disturb").exists() {
        Quiet::Dnd
    } else {
        Quiet::No
    }
}

/// Text from a sender on one line: whitespace runs as one space, control characters gone, at most
/// 255 bytes, the home folder as `~` (copied from the old edge's clipboard module).
pub fn sanitize_inline_text(src: &str) -> String {
    let home = crate::paths::home();
    sanitize_inline_text_with_home(src, home.as_deref().and_then(|p| p.to_str()))
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

/// One line of text from a sender: control characters as spaces, trimmed (`sanitize_inline_text`).
pub fn clean_line(s: &str) -> String {
    sanitize_inline_text(s)
}

/// A body: its lines cleaned, empty ones dropped, at most 1000 characters.
pub fn clean_body(s: &str) -> String {
    let lines: Vec<String> = s
        .lines()
        .map(sanitize_inline_text)
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n").chars().take(1000).collect()
}

impl Notify {
    /// A notification came: synchronous level notifications go to the bottom-centre OSD without
    /// entering the history; others go into the history (unless transient or a level) and a pop-up
    /// unless held back. One with the same id or synchronous tag takes the old one's place. Says
    /// whether anything shows differently.
    pub fn receive(&mut self, note: Note, now: Instant, quiet: Quiet) -> bool {
        let same = |n: &Note| n.id == note.id || (note.sync.is_some() && n.sync == note.sync);
        let osd = is_osd_note(&note);
        // a notification that leaves without the sender being told keeps the sender waiting for
        // NotificationClosed forever (2026-11-24: a drive's "mounted" notification held three
        // processes for hours): one replaced under its synchronous tag or in the OSD by a new id,
        // or pushed out of the history, is told it closed
        let mut replaced: Vec<u32> = self
            .history
            .iter()
            .chain(self.popups.iter().map(|p| &p.note))
            .filter(|n| same(n) && n.id != note.id)
            .map(|n| n.id)
            .collect();
        if let Some(old_osd) = self.osd.as_ref().filter(|o| osd || same(&o.note)) {
            if old_osd.note.id != note.id {
                replaced.push(old_osd.note.id);
            }
        }
        self.history.retain(|n| !same(n));
        if !note.transient && note.value.is_none() && !osd {
            self.history.insert(0, note.clone());
            for old in self.history.drain(MAX_HISTORY.min(self.history.len())..) {
                if !self.popups.iter().any(|p| p.note.id == old.id) {
                    self.out.push(Outgoing::Closed(old.id, 1));
                }
            }
        }
        if osd || self.osd.as_ref().is_some_and(|o| same(&o.note)) {
            self.osd = None;
        }
        let mut told = Vec::new();
        for id in replaced {
            if !told.contains(&id) {
                told.push(id);
                self.out.push(Outgoing::Closed(id, 4));
            }
        }
        let shows = match quiet {
            Quiet::No => true,
            Quiet::Dnd => note.urgency >= 2,
            Quiet::Game => false,
        };
        let old = self.popups.iter().position(|p| same(&p.note));
        if !shows {
            // an old pop-up of it goes too (a level held back leaves no stale bar)
            if let Some(i) = old {
                self.popups.remove(i);
            }
            if quiet == Quiet::Game {
                if let Some(stale) = self.osd.take() {
                    self.out.push(Outgoing::Closed(stale.note.id, 1));
                }
            }
            // held back and kept nowhere: it is over for the sender too
            if !self.history.iter().any(|n| n.id == note.id) {
                self.out.push(Outgoing::Closed(note.id, 1));
            }
            return true;
        }
        if osd {
            if let Some(i) = old {
                self.popups.remove(i);
            }
            let until = now + note.osd_shown_for();
            self.osd = Some(osd_from_note(note, until));
            return true;
        }
        let until = note.shown_for().map(|d| now + d);
        match old {
            Some(i) => {
                // in its place, without arriving again
                let born = self.popups[i].born;
                self.popups[i] = Popup { note, born, until };
            }
            None => {
                self.popups.insert(
                    0,
                    Popup {
                        note,
                        born: now,
                        until,
                    },
                );
                for old in self.popups.drain(MAX_POPUPS.min(self.popups.len())..) {
                    if !self.history.iter().any(|n| n.id == old.note.id) {
                        self.out.push(Outgoing::Closed(old.note.id, 1));
                    }
                }
            }
        }
        true
    }

    /// CloseNotification: gone from the screen and the history, NotificationClosed(id, 3).
    pub fn close(&mut self, id: u32) -> bool {
        let before = self.popups.len() + self.history.len() + usize::from(self.osd.is_some());
        self.popups.retain(|p| p.note.id != id);
        self.history.retain(|n| n.id != id);
        if self.osd.as_ref().is_some_and(|o| o.note.id == id) {
            self.osd = None;
        }
        let after = self.popups.len() + self.history.len() + usize::from(self.osd.is_some());
        let changed = before != after;
        if changed {
            self.out.push(Outgoing::Closed(id, 3));
        }
        changed
    }

    /// Pop-ups and the OSD whose time is up go (pop-ups stay in the history),
    /// NotificationClosed(id, 1) unless still listed there. The pop-up pointed at waits. Says
    /// whether anything went.
    pub fn expire(&mut self, now: Instant) -> bool {
        let hovered = self.hovered;
        let mut gone = Vec::new();
        self.popups.retain(|p| {
            let up = p.until.is_some_and(|t| now >= t) && Some(p.note.id) != hovered;
            if up {
                gone.push(p.note.id);
            }
            !up
        });
        for id in &gone {
            if !self.history.iter().any(|n| n.id == *id) {
                self.out.push(Outgoing::Closed(*id, 1));
            }
        }
        let mut osd_gone = false;
        if let Some(o) = &self.osd {
            if now >= o.until {
                let id = o.note.id;
                self.osd = None;
                self.out.push(Outgoing::Closed(id, 1));
                osd_gone = true;
            }
        }
        !gone.is_empty() || osd_gone
    }

    /// The pointer is over pop-up `id` (or none): its time stops while it is, and it gets at
    /// least 2 s more when the pointer leaves.
    pub fn hover(&mut self, id: Option<u32>, now: Instant) -> bool {
        if self.hovered == id {
            return false;
        }
        if let Some(prev) = self.hovered {
            if let Some(p) = self.popups.iter_mut().find(|p| p.note.id == prev) {
                if let Some(t) = p.until {
                    p.until = Some(t.max(now + Duration::from_secs(2)));
                }
            }
        }
        self.hovered = id;
        true
    }

    /// The next moment something changes by itself: a pop-up's or the OSD's time up.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.popups
            .iter()
            .filter(|p| Some(p.note.id) != self.hovered)
            .filter_map(|p| p.until)
            .chain(self.osd.as_ref().map(|o| o.until))
            .min()
    }

    /// The user dismissed `id` (its x, or the card's): gone everywhere, NotificationClosed(id, 2).
    pub fn dismiss(&mut self, id: u32) -> bool {
        let before = self.popups.len() + self.history.len() + usize::from(self.osd.is_some());
        self.popups.retain(|p| p.note.id != id);
        self.history.retain(|n| n.id != id);
        if self.osd.as_ref().is_some_and(|o| o.note.id == id) {
            self.osd = None;
        }
        if self.hovered == Some(id) {
            self.hovered = None;
        }
        let after = self.popups.len() + self.history.len() + usize::from(self.osd.is_some());
        let changed = before != after;
        if changed {
            self.out.push(Outgoing::Closed(id, 2));
        }
        changed
    }

    /// The user chose `key` ("default" for a click on it): ActionInvoked(id, key), then it goes
    /// (NotificationClosed(id, 2)) unless resident.
    pub fn invoke(&mut self, id: u32, key: &str) -> bool {
        let note = self
            .popups
            .iter()
            .map(|p| &p.note)
            .chain(self.history.iter())
            .find(|n| n.id == id)
            .cloned();
        let Some(note) = note else {
            return false;
        };
        if note.actions.iter().any(|(k, _)| k == key) {
            self.out.push(Outgoing::Action(id, key.to_string()));
        }
        if !note.resident {
            self.dismiss(id);
        }
        true
    }

    #[cfg(test)]
    /// Clear all (the card): every one dismissed.
    pub fn clear_all(&mut self) -> bool {
        let ids: Vec<u32> = self
            .history
            .iter()
            .map(|n| n.id)
            .chain(self.popups.iter().map(|p| p.note.id))
            .chain(self.osd.iter().map(|o| o.note.id))
            .collect();
        let mut any = false;
        for id in ids {
            any |= self.dismiss(id);
        }
        any
    }
}

/// How a pop-up's body is laid out: its lines at most `BODY_LINES` (cut by Pango with an ellipsis).
fn body_lines(note: &Note) -> usize {
    if note.body.is_empty() {
        0
    } else {
        // an estimate for the height (the drawing cuts at BODY_LINES); 46 characters a line at 12 px
        let est: usize = note
            .body
            .lines()
            .map(|l| l.chars().count().div_ceil(46).max(1))
            .sum();
        est.clamp(1, BODY_LINES)
    }
}

/// A pop-up's height.
pub fn popup_h(note: &Note) -> f64 {
    let mut h = 2.0 * PAD_Y + APP_H + TITLE_H;
    h += body_lines(note) as f64 * BODY_LINE;
    if note.value.is_some() {
        h += BAR_H;
    }
    if note.buttons().next().is_some() {
        h += 8.0 + BUTTON_H;
    }
    h
}

/// Each pop-up's rectangle on a surface `w` wide (anchored to the screen's top right: the cards
/// sit `MARGIN` in from its right and top), newest at the top.
pub fn layout(n: &Notify, w: f64) -> Vec<(u32, Rect)> {
    let x = w - MARGIN - POPUP_W;
    let mut y = MARGIN;
    n.popups
        .iter()
        .map(|p| {
            let h = popup_h(&p.note);
            let r = (x, y, POPUP_W, h);
            y += h + POPUP_GAP;
            (p.note.id, r)
        })
        .collect()
}

/// The x in a pop-up `r` (shown while pointed at).
pub fn x_rect(r: Rect) -> Rect {
    (r.0 + r.2 - PAD_X - 18.0, r.1 + PAD_Y - 3.0, 22.0, 22.0)
}

/// The action buttons of a pop-up `r`, left to right: (key, rect).
pub fn button_rects(cr: Option<&Context>, note: &Note, r: Rect) -> Vec<(String, Rect)> {
    let y = r.1 + r.3 - PAD_Y - BUTTON_H;
    let mut x = r.0 + PAD_X;
    let mut out = Vec::new();
    for (k, label) in note.buttons() {
        let t = Text {
            size: 12.0,
            bold: false,
            // measured only, never drawn: the colour does not change the size
            color: crate::theme::Rgba(0.0, 0.0, 0.0, 0.0),
            text: label,
        };
        let tw = cr.map_or(label.chars().count() as f64 * 7.0, |c| {
            draw::measure(c, &t).0
        });
        let bw = (tw + 24.0).clamp(56.0, r.2 - 2.0 * PAD_X);
        if x + bw > r.0 + r.2 - PAD_X + 0.5 {
            break;
        }
        out.push((k.clone(), (x, y, bw, BUTTON_H)));
        x += bw + 8.0;
    }
    out
}

/// What a click at `p` on the pop-ups' surface (`w` wide) does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Dismiss(u32),
    Action(u32, String),
    Open(u32),
}

pub fn hit(n: &Notify, w: f64, p: (f64, f64)) -> Option<Hit> {
    let inside = |r: Rect| p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3;
    for ((id, r), pop) in layout(n, w).into_iter().zip(&n.popups) {
        if !inside(r) {
            continue;
        }
        if inside(x_rect(r)) {
            return Some(Hit::Dismiss(id));
        }
        for (k, br) in button_rects(None, &pop.note, r) {
            if inside(br) {
                return Some(Hit::Action(id, k));
            }
        }
        return Some(Hit::Open(id));
    }
    None
}

/// The pop-up under `p`.
pub fn popup_at(n: &Notify, w: f64, p: (f64, f64)) -> Option<u32> {
    layout(n, w)
        .into_iter()
        .find(|(_, r)| p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3)
        .map(|(id, _)| id)
}

/// Draws every pop-up on a surface `w` wide. `pointer` lights the x and buttons under it; each
/// pop-up slides in from the right as it arrives.
pub fn draw(
    cr: &Context,
    th: &Theme,
    n: &Notify,
    w: f64,
    pointer: Option<(f64, f64)>,
    now: Instant,
) {
    draw::with_font(&th.font, || {
        let inside = |r: Rect| {
            pointer.is_some_and(|p| p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3)
        };
        for ((_, r), pop) in layout(n, w).into_iter().zip(&n.popups) {
            let age = now.saturating_duration_since(pop.born).as_secs_f64() * 1000.0;
            let t = crate::ui::ease(age / ARRIVE_MS);
            if t <= 0.0 {
                continue;
            }
            let lifted = t < 1.0;
            if lifted {
                let scale = 0.955 + 0.045 * t;
                let ax = r.0 + r.2;
                let ay = r.1 + r.3 * 0.5;
                cr.push_group();
                cr.translate(ax + (1.0 - t) * 12.0, ay);
                cr.scale(scale, scale);
                cr.translate(-ax, -ay);
            }
            draw_one(cr, th, &pop.note, r, inside(r), &inside);
            if lifted {
                let _ = cr.pop_group_to_source();
                let _ = cr.paint_with_alpha(t.powf(0.65));
            }
        }
    });
}

fn draw_one(
    cr: &Context,
    th: &Theme,
    note: &Note,
    r: Rect,
    pointed: bool,
    inside: &dyn Fn(Rect) -> bool,
) {
    let (x, y, w, h) = r;
    draw::card(cr, th, x, y, w, h);
    if note.urgency >= 2 {
        // critical: a thin accent line along its left edge inside the card
        draw::round_rect(cr, x + 6.0, y + 12.0, 3.0, h - 24.0, 1.5);
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }
    let tx = x + PAD_X;
    let tw = w - 2.0 * PAD_X;
    let mut ty = y + PAD_Y;
    // the app, and the x while pointed at
    let x_room = if pointed { 26.0 } else { 0.0 };
    draw::text_max(
        cr,
        tx,
        ty,
        tw - x_room,
        &Text {
            size: 11.0,
            bold: false,
            color: th.muted,
            text: &note.app,
        },
    );
    if pointed {
        let xr = x_rect(r);
        let lit = inside(xr);
        if lit {
            draw::round_rect(cr, xr.0, xr.1, xr.2, xr.3, th.small_radius());
            draw::set(cr, th.text.alpha(0.08));
            let _ = cr.fill();
        }
        let (cx, cy) = (xr.0 + xr.2 / 2.0, xr.1 + xr.3 / 2.0);
        draw::set(cr, if lit { th.text } else { th.muted });
        cr.set_line_width(1.5);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.move_to(cx - 4.0, cy - 4.0);
        cr.line_to(cx + 4.0, cy + 4.0);
        cr.move_to(cx + 4.0, cy - 4.0);
        cr.line_to(cx - 4.0, cy + 4.0);
        let _ = cr.stroke();
    }
    ty += APP_H;
    draw::text_max(
        cr,
        tx,
        ty,
        tw,
        &Text {
            size: 13.0,
            bold: true,
            color: th.text,
            text: &note.summary,
        },
    );
    ty += TITLE_H;
    let lines = body_lines(note);
    if lines > 0 {
        draw_body(cr, tx, ty, tw, lines, &note.body, th);
        ty += lines as f64 * BODY_LINE;
    }
    if let Some(v) = note.value {
        draw_value_bar(cr, th, tx, ty + 9.0, tw, v);
    }
    for (k, br) in button_rects(Some(cr), note, r) {
        let label = note
            .buttons()
            .find(|(kk, _)| *kk == k)
            .map(|(_, l)| l.as_str())
            .unwrap_or("");
        let lit = inside(br);
        draw::round_rect(cr, br.0, br.1, br.2, br.3, th.small_radius());
        draw::set(
            cr,
            th.text.alpha(if lit {
                crate::ui::CHOSEN_ALPHA
            } else {
                crate::ui::LIGHT_ALPHA
            }),
        );
        let _ = cr.fill();
        let lbl_t = Text {
            size: 12.0,
            bold: false,
            color: th.text,
            text: label,
        };
        let (_, lh) = draw::measure(cr, &lbl_t);
        draw::text_center(cr, br.0 + br.2 / 2.0, br.1 + (br.3 - lh) / 2.0, &lbl_t);
    }
}

/// The body in up to `lines` lines, wrapped, the last cut with an ellipsis.
fn draw_body(cr: &Context, x: f64, y: f64, w: f64, lines: usize, body: &str, th: &Theme) {
    let l = draw::wrapped_layout(cr, 12.0, false, w, Some(lines), body);
    draw::set(cr, th.muted);
    cr.move_to(x.round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
}

fn draw_value_bar(cr: &Context, th: &Theme, x: f64, y: f64, w: f64, value: i32) {
    let part = (value.clamp(0, 100) as f64) / 100.0;
    draw::round_rect(cr, x, y, w, 4.0, 2.0);
    draw::set(cr, th.track);
    let _ = cr.fill();
    if part > 0.0 {
        draw::round_rect(cr, x, y, (w * part).max(4.0), 4.0, 2.0);
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }
}

/// The OSD card's screen rectangle at the bottom centre of a `w x h` output.
pub fn osd_rect(w: f64, h: f64) -> Rect {
    let ow = OSD_W.min((w - 2.0 * MARGIN).max(120.0));
    let x = ((w - ow) / 2.0).round().max(0.0);
    let y = (h - MARGIN - OSD_H).round().max(0.0);
    (x, y, ow, OSD_H)
}

/// Draws the bottom-centre OSD (no icon, the word, a bar for the value, theme colours & corners).
pub fn draw_osd(cr: &Context, th: &Theme, osd: &Osd, w: f64, h: f64) {
    let r = if w <= OSD_W + 1.0 && h <= OSD_H + 1.0 {
        (0.0, 0.0, w, h)
    } else {
        osd_rect(w, h)
    };
    draw::with_font(&th.font, || {
        draw::card(cr, th, r.0, r.1, r.2, r.3);
        let tx = r.0 + PAD_X;
        let tw = (r.2 - 2.0 * PAD_X).max(0.0);
        let label_t = Text {
            size: draw::SIZE_BODY,
            bold: true,
            color: th.text,
            text: &osd.label,
        };
        let (_, lh) = draw::measure(cr, &label_t);
        let pct_w = if osd.muted {
            0.0
        } else {
            let pct = format!("{}%", osd.value);
            let pct_t = Text {
                size: draw::SIZE_SMALL,
                bold: false,
                color: th.muted,
                text: &pct,
            };
            let (pw, ph) = draw::measure(cr, &pct_t);
            draw::text_right(
                cr,
                tx + tw,
                r.1 + 13.0 + ((lh - ph) * 0.65).max(0.0),
                &pct_t,
            );
            pw + 8.0
        };
        draw::text_max(cr, tx, r.1 + 13.0, (tw - pct_w).max(20.0), &label_t);
        draw_value_bar(
            cr,
            th,
            tx,
            r.1 + r.3 - 14.0 - 4.0,
            tw,
            if osd.muted { 0 } else { osd.value },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u32) -> Note {
        Note {
            id,
            app: "Discord".into(),
            summary: format!("s{id}"),
            body: "hello".into(),
            actions: vec![],
            urgency: 1,
            value: None,
            sync: None,
            expire: Some(None),
            transient: false,
            resident: false,
            time: "12:00".into(),
        }
    }

    #[test]
    fn a_notification_pops_up_and_goes_to_the_history_and_expires() {
        let mut n = Notify::default();
        let t0 = Instant::now();
        assert!(n.receive(note(1), t0, Quiet::No));
        assert_eq!(n.popups.len(), 1);
        assert_eq!(n.history.len(), 1);
        assert_eq!(n.next_deadline(), Some(t0 + Duration::from_secs(6)));
        assert!(!n.expire(t0 + Duration::from_secs(5)));
        assert!(n.expire(t0 + Duration::from_secs(6)));
        assert!(n.popups.is_empty());
        assert_eq!(n.history.len(), 1, "still in the history");
        assert!(
            n.out.is_empty(),
            "no NotificationClosed while listed in the history"
        );
    }

    #[test]
    fn urgency_sets_how_long_and_critical_stays() {
        let mut low = note(1);
        low.urgency = 0;
        assert_eq!(low.shown_for(), Some(Duration::from_secs(4)));
        let mut crit = note(2);
        crit.urgency = 2;
        assert_eq!(crit.shown_for(), None);
        let mut given = note(3);
        given.expire = Some(Some(Duration::from_millis(1500)));
        assert_eq!(given.shown_for(), Some(Duration::from_millis(1500)));
        let mut never = note(4);
        never.expire = None;
        assert_eq!(never.shown_for(), None);
    }

    #[test]
    fn a_level_with_a_sync_tag_replaces_the_last_one_in_place() {
        let mut n = Notify::default();
        let t0 = Instant::now();
        let mut v = note(10);
        v.app = "Audio".into();
        v.summary = "Volume: 40%".into();
        v.value = Some(40);
        v.sync = Some("volume".into());
        n.receive(v.clone(), t0, Quiet::No);
        n.receive(note(11), t0, Quiet::No);
        let mut v2 = v.clone();
        v2.id = 12;
        v2.summary = "Volume: 45%".into();
        v2.value = Some(45);
        n.receive(v2, t0 + Duration::from_millis(500), Quiet::No);
        assert_eq!(n.popups.len(), 1, "level uses OSD, not popups");
        let osd = n.osd.as_ref().expect("active OSD");
        assert_eq!(osd.label, "Volume");
        assert_eq!(osd.value, 45);
        assert_eq!(osd.until, t0 + Duration::from_millis(500) + OSD_TIMEOUT);
        assert_eq!(n.out, vec![Outgoing::Closed(10, 4)]);
        assert!(
            n.history.iter().all(|h| h.app != "Audio"),
            "levels stay out of the history"
        );
    }

    #[test]
    fn osd_handles_volume_brightness_mute_replacement_and_expiry() {
        let mut n = Notify::default();
        let t0 = Instant::now();

        let mut vol = note(1);
        vol.app = "Audio".into();
        vol.summary = "Volume: 60%".into();
        vol.value = Some(60);
        vol.sync = Some("volume".into());
        vol.expire = Some(Some(Duration::from_millis(1200)));
        assert!(n.receive(vol, t0, Quiet::No));
        assert!(n.popups.is_empty());
        assert!(n.history.is_empty());
        let osd = n.osd.as_ref().unwrap();
        assert_eq!(osd.label, "Volume");
        assert_eq!(osd.value, 60);
        assert!(!osd.muted);
        assert_eq!(n.next_deadline(), Some(t0 + Duration::from_millis(1200)));

        // Brightness replaces volume (not stacked) and closes the superseded id.
        let mut bright = note(2);
        bright.app = "Display".into();
        bright.summary = "Brightness: 80%".into();
        bright.value = Some(80);
        bright.sync = Some("brightness".into());
        assert!(n.receive(bright, t0 + Duration::from_millis(200), Quiet::No));
        let osd = n.osd.as_ref().unwrap();
        assert_eq!(osd.label, "Brightness");
        assert_eq!(osd.value, 80);
        assert!(n.history.is_empty());
        assert_eq!(n.out, vec![Outgoing::Closed(1, 4)]);
        n.out.clear();

        // Mute from launch.sh (sync="volume", no int:value hint) still makes an OSD showing "Muted".
        let mut muted = note(3);
        muted.app = "Audio".into();
        muted.summary = "Muted".into();
        muted.value = None;
        muted.sync = Some("volume".into());
        muted.expire = Some(Some(Duration::from_millis(1200)));
        assert!(n.receive(muted, t0 + Duration::from_millis(400), Quiet::No));
        let osd = n.osd.as_ref().unwrap();
        assert_eq!(osd.label, "Muted");
        assert_eq!(osd.value, 0);
        assert!(osd.muted);
        assert!(n.history.is_empty());
        assert_eq!(n.out, vec![Outgoing::Closed(2, 4)]);
        n.out.clear();

        // Unmute from launch.sh ("Unmuted (60%)" without int:value hint) restores "Volume" with 60%.
        let mut unmuted = note(3);
        unmuted.app = "Audio".into();
        unmuted.summary = "Unmuted (60%)".into();
        unmuted.value = None;
        unmuted.sync = Some("volume".into());
        assert!(n.receive(unmuted, t0 + Duration::from_millis(600), Quiet::No));
        let osd = n.osd.as_ref().unwrap();
        assert_eq!(osd.label, "Volume");
        assert_eq!(osd.value, 60);
        assert!(!osd.muted);
        assert!(n.out.is_empty(), "same id updated in place");

        // Expires after 1.2 s and tells the sender.
        assert!(!n.expire(t0 + Duration::from_millis(1700)));
        assert!(n.expire(t0 + Duration::from_millis(1800)));
        assert!(n.osd.is_none());
        assert_eq!(n.next_deadline(), None);
        assert_eq!(n.out, vec![Outgoing::Closed(3, 1)]);
    }

    #[test]
    fn non_level_sync_notifications_stay_regular_popups() {
        let mut n = Notify::default();
        let t0 = Instant::now();
        let mut privacy = note(1);
        privacy.app = "Infynitae".into();
        privacy.summary = "This window is hidden from captures".into();
        privacy.sync = Some("privacy".into());
        privacy.value = None;
        n.receive(privacy, t0, Quiet::No);
        assert!(n.osd.is_none());
        assert_eq!(n.popups.len(), 1);
        assert_eq!(n.history.len(), 1);
    }

    #[test]
    fn osd_follows_quiet_rules_and_draws_with_theme_only() {
        let mut n = Notify::default();
        let t0 = Instant::now();
        let mut vol = note(5);
        vol.app = "Audio".into();
        vol.summary = "Volume: 50%".into();
        vol.sync = Some("volume".into());
        vol.value = Some(50);

        // Held during a game, not added to history, sender told it closed.
        n.receive(vol.clone(), t0, Quiet::Game);
        assert!(n.osd.is_none());
        assert!(n.history.is_empty());
        assert_eq!(n.out, vec![Outgoing::Closed(5, 1)]);
        n.out.clear();

        // Also held under Do not disturb for normal urgency, and active OSD clears if a game starts.
        n.receive(vol.clone(), t0, Quiet::Dnd);
        assert!(n.osd.is_none());
        n.out.clear();
        n.receive(vol.clone(), t0, Quiet::No);
        assert!(n.osd.is_some());
        let mut other = note(6);
        other.sync = Some("brightness".into());
        other.value = Some(40);
        n.receive(other, t0, Quiet::Game);
        assert!(n.osd.is_none());
        assert_eq!(n.out, vec![Outgoing::Closed(5, 4), Outgoing::Closed(6, 1)]);

        // Drawing checks: card background, bar accent for positive volume, no accent when muted.
        let cfg = crate::config::Config::from_text(
            "SURFACE_COLOR=\"#101216\"\nTEXT_COLOR=\"#eef2f8\"\nACCENT=\"#e05090\"\nCORNERS=\"10\"\n",
        );
        let th = Theme::from_config(&cfg);
        let draw_bytes = |osd: &Osd| {
            let mut surf =
                cairo::ImageSurface::create(cairo::Format::ARgb32, OSD_W as i32, OSD_H as i32)
                    .unwrap();
            {
                let cr = cairo::Context::new(&surf).unwrap();
                draw::grey_text_edges(&cr);
                draw_osd(&cr, &th, osd, OSD_W, OSD_H);
            }
            surf.flush();
            let bytes = surf.data().unwrap().to_vec();
            bytes
        };
        let osd_on = Osd {
            note: vol.clone(),
            label: "Volume".into(),
            value: 50,
            muted: false,
            until: t0 + OSD_TIMEOUT,
        };
        let mut osd_muted = osd_on.clone();
        osd_muted.label = "Muted".into();
        osd_muted.value = 0;
        osd_muted.muted = true;

        let px_on = draw_bytes(&osd_on);
        let px_muted = draw_bytes(&osd_muted);
        let at = |buf: &[u8], x: usize, y: usize| -> [u8; 4] {
            let i = (y * OSD_W as usize + x) * 4;
            [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
        };
        // Corner pixel outside rounded radius (10 px) is transparent.
        assert_eq!(at(&px_on, 0, 0), [0, 0, 0, 0]);
        // At 50% volume, left quarter of bar (x=40, y=39) is accent; right quarter (x=180, y=39) is track.
        assert_ne!(at(&px_on, 40, 39), at(&px_on, 180, 39));
        // When muted, both sides of the bar match the track colour.
        assert_eq!(at(&px_muted, 40, 39), at(&px_muted, 180, 39));
        assert_ne!(at(&px_on, 40, 39), at(&px_muted, 40, 39));

        // Unmuted OSD renders right-aligned percentage text; muted OSD leaves that area clear.
        let count_bright = |buf: &[u8]| -> usize {
            let mut n = 0;
            for y in 12..30 {
                for x in 175..(OSD_W as usize - 16) {
                    let px = at(buf, x, y);
                    if px[2] > 80 || px[1] > 80 || px[0] > 80 {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(
            count_bright(&px_on) > 10,
            "unmuted OSD should draw percentage text on the right"
        );
        assert_eq!(
            count_bright(&px_muted),
            0,
            "muted OSD should not draw a percentage readout"
        );
    }

    #[test]
    fn do_not_disturb_shows_only_critical_and_a_game_shows_none() {
        let mut n = Notify::default();
        let t0 = Instant::now();
        n.receive(note(1), t0, Quiet::Dnd);
        assert!(n.popups.is_empty());
        let mut c = note(2);
        c.urgency = 2;
        n.receive(c.clone(), t0, Quiet::Dnd);
        assert_eq!(n.popups.len(), 1);
        let mut c3 = c.clone();
        c3.id = 3;
        n.receive(c3, t0, Quiet::Game);
        assert_eq!(n.popups.len(), 1, "a game holds critical ones back too");
        assert_eq!(n.history.len(), 3, "all listed");
    }

    #[test]
    fn pointing_holds_the_time_and_gives_two_seconds_after() {
        let mut n = Notify::default();
        let t0 = Instant::now();
        n.receive(note(1), t0, Quiet::No);
        n.hover(Some(1), t0 + Duration::from_secs(5));
        assert!(
            !n.expire(t0 + Duration::from_secs(9)),
            "held while pointed at"
        );
        n.hover(None, t0 + Duration::from_secs(9));
        assert!(!n.expire(t0 + Duration::from_secs(10)));
        assert!(n.expire(t0 + Duration::from_secs(11)));
    }

    #[test]
    fn actions_dismiss_and_close_send_the_signals() {
        let mut n = Notify::default();
        let t0 = Instant::now();
        let mut a = note(1);
        a.actions = vec![
            ("default".into(), "Open".into()),
            ("reply".into(), "Reply".into()),
        ];
        n.receive(a, t0, Quiet::No);
        n.invoke(1, "reply");
        assert_eq!(
            n.out,
            vec![Outgoing::Action(1, "reply".into()), Outgoing::Closed(1, 2)]
        );
        assert!(n.popups.is_empty() && n.history.is_empty());
        n.out.clear();
        n.receive(note(2), t0, Quiet::No);
        n.close(2);
        assert_eq!(n.out, vec![Outgoing::Closed(2, 3)]);
        n.out.clear();
        n.receive(note(3), t0, Quiet::No);
        n.receive(note(4), t0, Quiet::No);
        n.clear_all();
        assert_eq!(n.out.len(), 2);
        assert!(n.history.is_empty() && n.popups.is_empty());
    }

    #[test]
    fn a_resident_notification_stays_after_its_action() {
        let mut n = Notify::default();
        let mut a = note(1);
        a.resident = true;
        a.actions = vec![("pause".into(), "Pause".into())];
        n.receive(a, Instant::now(), Quiet::No);
        n.invoke(1, "pause");
        assert_eq!(n.out, vec![Outgoing::Action(1, "pause".into())]);
        assert_eq!(n.popups.len(), 1);
    }

    /// Every notification that leaves tells its sender (NotificationClosed): pushed out of the
    /// history, replaced under its synchronous tag by a new id, a transient one pushed off the
    /// screen or held back. A sender waiting on it (notify-send with actions) never waits forever.
    #[test]
    fn a_notification_that_leaves_tells_its_sender() {
        let t0 = Instant::now();
        // pushed out of the history: the oldest ten
        let mut n = Notify::default();
        for i in 1..=60 {
            n.receive(note(i), t0, Quiet::No);
        }
        let closed: Vec<u32> = n
            .out
            .iter()
            .filter_map(|o| match o {
                Outgoing::Closed(id, 1) => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(closed, (1..=10).collect::<Vec<_>>());
        // replaced under its tag by a new id
        let mut n = Notify::default();
        let mut a = note(1);
        a.sync = Some("volume".into());
        n.receive(a, t0, Quiet::No);
        let mut b = note(2);
        b.sync = Some("volume".into());
        n.receive(b, t0, Quiet::No);
        assert_eq!(n.out, vec![Outgoing::Closed(1, 4)]);
        // the same id again is an update, not a close
        let mut n = Notify::default();
        n.receive(note(5), t0, Quiet::No);
        n.receive(note(5), t0, Quiet::No);
        assert!(n.out.is_empty(), "{:?}", n.out);
        // a transient one pushed off the screen, and one held back during a game
        let mut n = Notify::default();
        let mut t = note(1);
        t.transient = true;
        n.receive(t, t0, Quiet::No);
        for i in 2..=(MAX_POPUPS as u32 + 1) {
            n.receive(note(i), t0, Quiet::No);
        }
        assert_eq!(n.out, vec![Outgoing::Closed(1, 1)]);
        let mut n = Notify::default();
        let mut g = note(9);
        g.transient = true;
        n.receive(g, t0, Quiet::Game);
        assert_eq!(n.out, vec![Outgoing::Closed(9, 1)]);
    }

    #[test]
    fn at_most_four_pop_ups_and_fifty_in_the_history() {
        let mut n = Notify::default();
        let t0 = Instant::now();
        for i in 1..=60 {
            n.receive(note(i), t0, Quiet::No);
        }
        assert_eq!(n.popups.len(), MAX_POPUPS);
        assert_eq!(n.popups[0].note.id, 60, "newest first");
        assert_eq!(n.history.len(), MAX_HISTORY);
    }

    #[test]
    fn clicks_find_the_x_the_buttons_and_the_card() {
        let mut n = Notify::default();
        let mut a = note(7);
        a.actions = vec![
            ("reply".into(), "Reply".into()),
            ("mute".into(), "Mute".into()),
        ];
        n.receive(a, Instant::now(), Quiet::No);
        let w = 420.0;
        let (_, r) = layout(&n, w)[0];
        assert_eq!(r.0, w - MARGIN - POPUP_W);
        let xr = x_rect(r);
        assert_eq!(hit(&n, w, (xr.0 + 5.0, xr.1 + 5.0)), Some(Hit::Dismiss(7)));
        let b = button_rects(None, &n.popups[0].note, r);
        assert_eq!(b.len(), 2);
        assert_eq!(
            hit(&n, w, (b[1].1 .0 + 3.0, b[1].1 .1 + 3.0)),
            Some(Hit::Action(7, "mute".into()))
        );
        assert_eq!(hit(&n, w, (r.0 + 20.0, r.1 + 40.0)), Some(Hit::Open(7)));
        assert_eq!(hit(&n, w, (5.0, 5.0)), None);
    }

    #[test]
    fn bodies_are_cleaned() {
        // control characters dropped (sanitize_inline_text), empty lines gone
        assert_eq!(clean_body("a\u{7}b\n\n  c  \n"), "ab\nc");
        assert!(clean_body(&"x".repeat(2000)).chars().count() <= 1000);
    }
}
