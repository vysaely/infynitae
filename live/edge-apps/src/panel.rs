//! A panel's cards as `owner/kind[=argument][:size][@half|@x,y,WxH]` words (docs/EDGE_APPS.md
//! section 1): read from the `PANEL_` lines (`config.rs` keeps each panel's words as written), with
//! today's module words mapped to the app they belong to, and printed back for `--print-panels`.

use crate::config::{Config, Panel};
use crate::provider::Size;

/// One card placed in a panel.
#[derive(Clone, Debug, PartialEq)]
pub struct CardRef {
    /// a provider id ("settings", "music") or a desktop entry id ("steam")
    pub owner: String,
    pub kind: String,
    /// what a kind needs to know ("settings/quick=dnd,nightlight": which switches, in order)
    pub argument: Option<String>,
    /// None: the kind's default size
    pub size: Option<Size>,
    pub half: bool,
    /// placed by hand (x, y, w, h inside the panel), as the older editor wrote it
    pub place: Option<(i32, i32, i32, i32)>,
    /// the module word it was read from, kept for the VM scripts' box dump (`module <word>`)
    pub old: Option<String>,
}

impl CardRef {
    pub fn new(owner: &str, kind: &str) -> CardRef {
        CardRef {
            owner: owner.into(),
            kind: kind.into(),
            argument: None,
            size: None,
            half: false,
            place: None,
            old: None,
        }
    }

    /// The word as written in a PANEL_ line.
    pub fn word(&self) -> String {
        let mut w = format!("{}/{}", self.owner, self.kind);
        if let Some(a) = &self.argument {
            w += "=";
            w += a;
        }
        if let Some(s) = self.size {
            w += ":";
            w += s.letter();
        }
        if let Some((x, y, pw, ph)) = self.place {
            w += &format!("@{x},{y},{pw}x{ph}");
        } else if self.half {
            w += "@half";
        }
        w
    }
}

/// The switches today's lines name one by one; they share one Quick settings card.
const QUICK: &[&str] = &["dnd", "nightlight", "night", "keyboard", "osk", "replay"];

/// Today's module word (and its look) as the card it becomes; None for a word it does not know.
/// The table of docs/EDGE_APPS.md section 1.
pub fn map_old(word: &str, look: Option<&str>) -> Option<CardRef> {
    let w = word.to_ascii_lowercase();
    let look = look.map(|l| l.to_ascii_lowercase());
    let small_or_wide = |l: Option<&str>| match l {
        Some("small") | Some("tile") => Some(Size::S),
        Some("wide") => Some(Size::M),
        _ => None,
    };
    let (owner, kind, size) = match w.as_str() {
        "wifi" | "network" => ("settings", "network", small_or_wide(look.as_deref())),
        "bluetooth" => ("settings", "bluetooth", small_or_wide(look.as_deref())),
        "power" => ("settings", "power", small_or_wide(look.as_deref())),
        "volume" => ("sound", "volume", None),
        "clock" => match look.as_deref() {
            Some("calendar") => ("calendar", "month", None),
            Some("big") => ("calendar", "clock", Some(Size::M)),
            _ => ("calendar", "clock", Some(Size::S)),
        },
        "calendar" => ("calendar", "month", None),
        "notifications" => ("notifications", "list", None),
        "session" => ("settings", "session", None),
        "media" => ("music", "playing", None),
        "memory" => ("monitor", "memory", None),
        "system" => ("monitor", "gpu", None),
        "drives" => ("files", "drives", None),
        "search" => (
            "apps",
            "search",
            match look.as_deref() {
                Some("hidden") => Some(Size::S),
                Some("bar") => Some(Size::M),
                _ => None,
            },
        ),
        "favorites" => ("apps", "favorites", None),
        "apps" => ("apps", "list", None),
        "windows" => ("windows", "list", None),
        "clipboard" => ("clipboard", "history", None),
        "tray" => ("apps", "background", None),
        _ => return None,
    };
    let mut c = CardRef::new(owner, kind);
    c.size = size;
    c.old = Some(w);
    Some(c)
}

/// Reads one word of a panel line: a new `owner/kind...` word, or an old module word mapped.
fn read_word(raw: &str) -> Option<CardRef> {
    let (body, at) = match raw.split_once('@') {
        Some((a, b)) => (a, Some(b)),
        None => (raw, None),
    };
    let mut card = if let Some((owner, rest)) = body.split_once('/') {
        let (rest, size) = match rest.rsplit_once(':') {
            Some((r, s)) => (r, Size::of_letter(s)),
            None => (rest, None),
        };
        let (kind, argument) = match rest.split_once('=') {
            Some((k, a)) => (k, Some(a.to_string()).filter(|a| !a.is_empty())),
            None => (rest, None),
        };
        let ok = |s: &str| {
            !s.is_empty()
                && s.len() <= 120
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        };
        if !ok(owner) || !ok(kind) {
            return None;
        }
        let mut c = CardRef::new(owner, kind);
        c.argument = argument;
        c.size = size;
        c
    } else {
        let (w, look) = match body.split_once(':') {
            Some((a, b)) => (a, Some(b)),
            None => (body, None),
        };
        map_old(w, look)?
    };
    if let Some(a) = at {
        if a.eq_ignore_ascii_case("half") {
            card.half = true;
        } else if let Some(r) = parse_place(a) {
            card.place = Some(r);
        }
    }
    Some(card)
}

/// "8,48,344x32"
fn parse_place(a: &str) -> Option<(i32, i32, i32, i32)> {
    let (x, rest) = a.split_once(',')?;
    let (y, size) = rest.split_once(',')?;
    let (w, h) = size.split_once(['x', 'X'])?;
    let (x, y, w, h) = (
        x.parse().ok()?,
        y.parse().ok()?,
        w.parse().ok()?,
        h.parse().ok()?,
    );
    (w >= 24 && h >= 20).then_some((x, y, w, h))
}

/// A panel's cards from its words: old switch words become one `settings/quick` card at the place
/// of the first of them, the switches in the order written; each card once.
pub fn cards(p: &Panel) -> Vec<CardRef> {
    let mut out: Vec<CardRef> = Vec::new();
    let mut quick: Vec<String> = Vec::new();
    let mut quick_at: Option<usize> = None;
    for raw in &p.words {
        let base = raw
            .split(['@', ':'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if QUICK.contains(&base.as_str()) {
            let name = if base == "night" {
                "nightlight".to_string()
            } else {
                base
            };
            if !quick.contains(&name) {
                quick.push(name);
            }
            quick_at.get_or_insert(out.len());
            continue;
        }
        if let Some(c) = read_word(raw) {
            if !out.iter().any(|o| o.owner == c.owner && o.kind == c.kind) {
                out.push(c);
            }
        }
    }
    if let Some(i) = quick_at {
        let mut q = CardRef::new("settings", "quick");
        q.argument = Some(quick.join(","));
        q.old = Some("switches".into());
        out.insert(i.min(out.len()), q);
    }
    out
}

/// A panel's line in the new words (what `--print-panels` prints after the panel's name).
pub fn line(p: &Panel) -> String {
    let mut out = p.value();
    // the header part of today's value, then the cards in the new words
    let head: Vec<&str> = out
        .split_whitespace()
        .take_while(|w| {
            let base = w.split(['@', ':']).next().unwrap_or("");
            crate::config::Module::of_word(base).is_none()
        })
        .collect();
    let mut s = head.join(" ");
    for c in cards(p) {
        s += " ";
        s += &c.word();
    }
    out.clear();
    out.push_str(s.trim());
    out
}

/// Every panel of the config (the desktop's widgets too, named "widgets"), as `--print-panels`.
pub fn print(cfg: &Config) -> String {
    let mut s = String::new();
    for p in &cfg.panels {
        s += &format!("{} {}\n", p.name, line(p));
    }
    if let Some(w) = &cfg.widgets {
        s += &format!("widgets {}\n", line(w));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(line: &str) -> Vec<String> {
        let cfg = Config::from_text(&format!("PANEL_CONTROL=\"{line}\"\n"));
        cards(cfg.panels.iter().find(|p| p.name == "control").unwrap())
            .iter()
            .map(|c| c.word())
            .collect()
    }

    #[test]
    fn an_old_line_is_read_as_cards_of_their_apps() {
        assert_eq!(
            panel("SUPER+A bottom right 380 hover clock volume wifi bluetooth dnd power replay nightlight tray notifications session"),
            vec![
                "calendar/clock:s",
                "sound/volume",
                "settings/network",
                "settings/bluetooth",
                "settings/quick=dnd,replay,nightlight",
                "settings/power",
                "apps/background",
                "notifications/list",
                "settings/session",
            ]
        );
    }

    #[test]
    fn looks_halves_and_places_carry_over() {
        assert_eq!(
            panel("clock:big wifi:small media@half search:bar"),
            vec![
                "calendar/clock:m",
                "settings/network:s",
                "music/playing@half",
                "apps/search:m"
            ]
        );
        assert_eq!(panel("clock:calendar"), vec!["calendar/month"]);
        assert_eq!(
            panel("volume@8,48,344x32"),
            vec!["sound/volume@8,48,344x32"]
        );
    }

    #[test]
    fn new_words_are_read_with_their_argument_size_and_half() {
        assert_eq!(
            panel("SUPER+A top right settings/network steam/games:m settings/quick=dnd,nightlight music/playing@half org.telegram.desktop._8cd7/messages:s"),
            vec![
                "settings/network",
                "steam/games:m",
                "settings/quick=dnd,nightlight",
                "music/playing@half",
                "org.telegram.desktop._8cd7/messages:s",
            ]
        );
    }

    #[test]
    fn bad_words_are_left_out_and_a_card_comes_once() {
        assert_eq!(
            panel("settings/ /x bad$/kind steam/games steam/games:l unknownword"),
            vec!["steam/games"]
        );
    }

    #[test]
    fn the_printed_line_keeps_the_panel_s_place_and_key() {
        let cfg =
            Config::from_text("PANEL_CONTROL=\"SUPER+A bottom right 380 hover clock wifi\"\n");
        let p = cfg.panels.iter().find(|p| p.name == "control").unwrap();
        let l = line(p);
        assert!(l.starts_with("SUPER+A bottom right 380 hover"), "{l}");
        assert!(l.ends_with("calendar/clock:s settings/network"), "{l}");
    }
}
