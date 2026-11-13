//! Steam (docs/EDGE_APPS.md section 6): installed games from `libraryfolders.vdf` and
//! `appmanifest_<id>.acf`, last played first, with Play opening `steam://rungameid/<appid>`.
//!
//! Known gaps (kept as the doc describes):
//! - Tool filtering is by name prefix (`Proton `, `Steam Linux Runtime`,
//!   `Steamworks Common Redistributables`); a tool with another name appears as a game.
//! - Running state is not marked yet (window `app_id` matching between native and Proton games
//!   is not verified in real use).
//! - Game covers at M/L are not drawn: `app-cards.html` lays out the card with plain rows, and
//!   `ui.rs` / `app.rs` do not expose JPEG image loading yet.

use crate::card::{Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::ui::{self, Row};
use cairo::Context;
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};
use std::any::Any;
use std::path::{Path, PathBuf};

pub struct Steam;

pub const SMALL_ROWS: usize = 3;
pub const MEDIUM_ROWS: usize = 6;
pub const LARGE_ROWS: usize = 8;

const KINDS: &[CardKind] = &[CardKind {
    id: "games",
    title: "Games",
    about: "Your installed games, last played first",
    sizes: &[Size::S, Size::M, Size::L],
    default: Size::S,
    half: false,
    sources: &["games"],
}];

impl Provider for Steam {
    fn id(&self) -> &'static str {
        "steam"
    }
    fn name(&self) -> &'static str {
        "Steam"
    }
    fn claims(&self) -> &'static [Claim] {
        &[Claim::Id("steam"), Claim::Id("com.valvesoftware.Steam")]
    }
    fn listed(&self) -> Listed {
        Listed::Never
    }
    fn cards(&self) -> &'static [CardKind] {
        KINDS
    }
    fn card(&self, kind: &str, argument: Option<&str>) -> Option<Box<dyn Card>> {
        if kind == "games" && argument.is_none() {
            Some(Box::new(GamesCard::default()))
        } else {
            None
        }
    }
    fn source(&self, key: &str) -> Option<Box<dyn Source>> {
        if key == "games" {
            Some(Box::new(SteamSource::default()))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Game {
    pub appid: String,
    pub name: String,
    pub state_flags: u32,
    pub last_played: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SteamData {
    pub games: Vec<Game>,
}

/// The candidate Steam roots under `home`, in priority order.
pub fn candidate_steam_dirs(home: &Path) -> [PathBuf; 3] {
    [
        home.join(".local/share/Steam"),
        home.join(".steam/steam"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
    ]
}

/// First existing Steam root under `home`.
pub fn find_steam_dir(home: &Path) -> Option<PathBuf> {
    candidate_steam_dirs(home).into_iter().find(|p| p.is_dir())
}

/// Steam runtime / compatibility entries excluded by name prefix; other tools stay visible.
pub fn is_tool_name(name: &str) -> bool {
    name.starts_with("Proton ")
        || name.starts_with("Steam Linux Runtime")
        || name.starts_with("Steamworks Common Redistributables")
}

#[derive(Debug, PartialEq, Eq)]
enum Tok {
    Str(String),
    Open,
    Close,
}

/// Tokenizes Valve's KeyValues (VDF/ACF) text: quoted strings with backslash escapes, `{`, `}`, and
/// `//` line comments. Incomplete trailing quotes stop parsing cleanly instead of producing junk.
fn vdf_tokens(text: &str) -> Vec<Tok> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '/' {
            chars.next();
            if chars.peek() == Some(&'/') {
                for ch in chars.by_ref() {
                    if ch == '\n' {
                        break;
                    }
                }
            } else {
                let mut s = String::from('/');
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || matches!(n, '{' | '}' | '"') {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                out.push(Tok::Str(s));
            }
        } else if c == '{' {
            chars.next();
            out.push(Tok::Open);
        } else if c == '}' {
            chars.next();
            out.push(Tok::Close);
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            let mut closed = false;
            while let Some(ch) = chars.next() {
                if ch == '"' {
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    let Some(esc) = chars.next() else {
                        break;
                    };
                    match esc {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        other => s.push(other),
                    }
                } else {
                    s.push(ch);
                }
            }
            if !closed {
                break;
            }
            out.push(Tok::Str(s));
        } else {
            let mut s = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_whitespace() || matches!(n, '{' | '}' | '"') {
                    break;
                }
                s.push(n);
                chars.next();
            }
            out.push(Tok::Str(s));
        }
    }
    out
}

/// Walks KeyValues pairs `(depth, key, value)` inside a single named root object (`root_name`).
fn walk_vdf<F: FnMut(usize, &str, &str)>(text: &str, root_name: &str, mut f: F) {
    let toks = vdf_tokens(text);
    let mut depth: usize = 0;
    let mut in_root = false;
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Open => {
                depth += 1;
                i += 1;
            }
            Tok::Close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    in_root = false;
                }
                i += 1;
            }
            Tok::Str(key) => match toks.get(i + 1) {
                Some(Tok::Open) => {
                    if depth == 0 {
                        in_root = key.eq_ignore_ascii_case(root_name);
                    }
                    depth += 1;
                    i += 2;
                }
                Some(Tok::Str(val)) => {
                    if in_root && depth >= 1 {
                        f(depth, key, val);
                    }
                    i += 2;
                }
                _ => i += 1,
            },
        }
    }
}

/// Reads every `"path"` value from `steamapps/libraryfolders.vdf`, deduplicated in file order.
pub fn parse_library_folders(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    walk_vdf(text, "libraryfolders", |depth, key, val| {
        if depth == 2 && key.eq_ignore_ascii_case("path") && !val.trim().is_empty() {
            let p = PathBuf::from(val);
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
    });
    paths
}

/// Reads one `appmanifest_<id>.acf` into a `Game`; returns `None` when `appid` or `name` is
/// missing or `appid` is not numeric.
pub fn parse_manifest(text: &str) -> Option<Game> {
    let mut appid: Option<String> = None;
    let mut name: Option<String> = None;
    let mut state_flags: u32 = 0;
    let mut last_played: u64 = 0;
    walk_vdf(text, "AppState", |depth, key, val| {
        if depth != 1 {
            return;
        }
        if key.eq_ignore_ascii_case("appid") {
            appid = Some(val.to_string());
        } else if key.eq_ignore_ascii_case("name") {
            name = Some(val.to_string());
        } else if key.eq_ignore_ascii_case("StateFlags") {
            if let Ok(n) = val.parse::<u32>() {
                state_flags = n;
            }
        } else if key.eq_ignore_ascii_case("LastPlayed") {
            if let Ok(n) = val.parse::<u64>() {
                last_played = n;
            }
        }
    });
    let appid = appid.filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))?;
    let name = name.filter(|n| !n.trim().is_empty())?;
    Some(Game {
        appid,
        name,
        state_flags,
        last_played,
    })
}

/// Loads games from a Steam root folder: reads `steamapps/libraryfolders.vdf`, scans
/// `appmanifest_<id>.acf` in each library, filters tools, and sorts by `LastPlayed` descending,
/// then never-played alphabetically by name.
pub fn load_steam_from(root: &Path) -> SteamData {
    let Ok(vdf) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) else {
        return SteamData::default();
    };
    let mut games: Vec<Game> = Vec::new();
    let mut seen_libs: Vec<PathBuf> = Vec::new();
    for lib in parse_library_folders(&vdf) {
        let key = std::fs::canonicalize(&lib).unwrap_or_else(|_| lib.clone());
        if seen_libs.contains(&key) {
            continue;
        }
        seen_libs.push(key);
        let Ok(rd) = std::fs::read_dir(lib.join("steamapps")) else {
            continue;
        };
        for entry in rd.flatten() {
            let fname = entry.file_name();
            let Some(file) = fname.to_str() else {
                continue;
            };
            if !(file.starts_with("appmanifest_") && file.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let Some(game) = parse_manifest(&text) else {
                continue;
            };
            if is_tool_name(&game.name) {
                continue;
            }
            if let Some(existing) = games.iter_mut().find(|g| g.appid == game.appid) {
                if game.last_played > existing.last_played {
                    *existing = game;
                }
            } else {
                games.push(game);
            }
        }
    }
    games.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.appid.cmp(&b.appid))
    });
    SteamData { games }
}

/// Reads Steam games from `root`, or from the first existing Steam directory under `$HOME`.
#[derive(Clone, Debug, Default)]
pub struct SteamSource {
    pub root: Option<PathBuf>,
}

impl Source for SteamSource {
    fn fetch(&mut self) -> Data {
        let root = self.root.clone().or_else(|| {
            let home = std::env::var_os("HOME").filter(|s| !s.is_empty())?;
            find_steam_dir(Path::new(&home))
        });
        let data = root
            .filter(|p| p.is_dir())
            .map(|p| load_steam_from(&p))
            .unwrap_or_default();
        Box::new(data)
    }

    fn every(&self) -> Option<std::time::Duration> {
        None
    }
}

pub fn format_played_day(played: NaiveDate, today: NaiveDate) -> String {
    if played == today {
        "Last played today".into()
    } else if Some(played) == today.pred_opt() {
        "Last played yesterday".into()
    } else if played.year() == today.year() {
        format!("Last played {}", played.format("%a %-d %b"))
    } else {
        format!("Last played {}", played.format("%a %-d %b %Y"))
    }
}

pub fn format_last_played(last_played: u64, now: NaiveDateTime) -> String {
    if last_played == 0 {
        return "Never played".into();
    }
    let Ok(ts) = i64::try_from(last_played) else {
        return "Never played".into();
    };
    let Some(utc) = chrono::DateTime::from_timestamp(ts, 0) else {
        return "Never played".into();
    };
    let played = utc.with_timezone(&Local).date_naive();
    format_played_day(played, now.date())
}

#[derive(Default)]
pub struct GamesCard {
    pub data: Option<SteamData>,
    pub scroll: usize,
}

impl GamesCard {
    fn visible_range(&self, cx: &Cx) -> std::ops::Range<usize> {
        let n = self.data.as_ref().map_or(0, |d| d.games.len());
        let limit = match cx.size {
            Size::S => SMALL_ROWS,
            Size::M => MEDIUM_ROWS,
            Size::L => LARGE_ROWS,
        };
        let vis = n.min(limit);
        let start = if cx.size == Size::L {
            self.scroll.min(n.saturating_sub(vis))
        } else {
            0
        };
        start..start + vis
    }
}

impl Card for GamesCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "games", data.downcast_ref::<SteamData>()) else {
            return false;
        };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        self.scroll = self.scroll.min(d.games.len().saturating_sub(1));
        changed
    }

    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if d.games.is_empty() => Status::Missing("Steam has no games installed".into()),
            Some(_) => Status::Ready,
        }
    }

    fn height(&self, cx: &Cx, _: f64) -> f64 {
        self.visible_range(cx).len() as f64 * ui::ROW_H
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else { return };
        let range = self.visible_range(cx);
        if range.is_empty() {
            return;
        }
        cr.save().ok();
        cr.rectangle(body.0, body.1, body.2, self.height(cx, body.2));
        cr.clip();
        let play_w = draw::measure(
            cr,
            &Text {
                size: 12.0,
                bold: false,
                color: cx.theme.text,
                text: "Play",
            },
        )
        .0 + 18.0;
        for (row_i, idx) in range.enumerate() {
            let g = &d.games[idx];
            let r = (body.0, body.1 + row_i as f64 * ui::ROW_H, body.2, ui::ROW_H);
            let row_id = (idx * 2) as u32;
            let play_id = row_id + 1;
            let hovered = hits
                .pointer
                .is_some_and(|(px, py)| px >= r.0 && px < r.0 + r.2 && py >= r.1 && py < r.1 + r.3);
            let show_play = hovered || hits.selected(row_id) || hits.selected(play_id);
            let state = format_last_played(g.last_played, cx.now);
            let lit = ui::row(
                cr,
                cx,
                hits,
                row_id,
                r,
                &Row {
                    icon: None,
                    name: &g.name,
                    state: &state,
                    on: false,
                    right: if show_play { play_w + 8.0 } else { 0.0 },
                },
            );
            if lit || hits.selected(play_id) {
                let bx = r.0 + r.2 - 8.0 - play_w;
                let by = r.1 + (r.3 - 26.0) / 2.0;
                ui::button(cr, cx, hits, play_id, bx, by, "Play");
            }
        }
        cr.restore().ok();
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Some(d) = &self.data else {
            return false;
        };
        let range = self.visible_range(cx);
        match ev {
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } => {
                let idx = (*hit / 2) as usize;
                if !range.contains(&idx) {
                    return false;
                }
                let Some(g) = d.games.get(idx) else {
                    return false;
                };
                out.push(Action::OpenUri(format!("steam://rungameid/{}", g.appid)));
                true
            }
            Event::Scroll { dy, .. } if cx.size == Size::L && d.games.len() > LARGE_ROWS => {
                let max_scroll = d.games.len() - LARGE_ROWS;
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
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::provider;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_cx(th: &crate::theme::Theme, size: Size) -> Cx<'_> {
        Cx {
            theme: th,
            now: chrono::NaiveDate::from_ymd_opt(2026, 11, 13)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            size,
            app: None,
            apps: &[],
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("edge-apps-steam-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_acf(dir: &Path, appid: &str, name: &str, state_flags: u32, last_played: u64) {
        let body = format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"Universe\"\t\t\"1\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"{state_flags}\"\n\t\"LastPlayed\"\t\t\"{last_played}\"\n\t\"UserConfig\"\n\t{{\n\t\t\"language\"\t\t\"english\"\n\t}}\n}}\n"
        );
        std::fs::write(dir.join(format!("appmanifest_{appid}.acf")), body).unwrap();
    }

    #[test]
    fn provider_claims_and_source_schedule_match_brief() {
        let steam_app = App {
            id: "steam".into(),
            name: "Steam".into(),
            categories: vec!["Network".into(), "FileTransfer".into(), "Game".into()],
            ..Default::default()
        };
        let flatpak_steam = App {
            id: "com.valvesoftware.Steam".into(),
            name: "Steam".into(),
            categories: vec!["Game".into()],
            ..Default::default()
        };
        let game_shortcut = App {
            id: "Counter-Strike 2".into(),
            name: "Counter-Strike 2".into(),
            categories: vec!["Game".into()],
            ..Default::default()
        };
        assert_eq!(
            provider::owner_of(&steam_app).map(|p| p.id()),
            Some("steam")
        );
        assert_eq!(
            provider::owner_of(&flatpak_steam).map(|p| p.id()),
            Some("steam")
        );
        assert_eq!(provider::owner_of(&game_shortcut).unwrap().id(), "simple");
        assert_eq!(Steam.listed(), Listed::Never);
        assert!(Steam.card("games", None).is_some());
        assert!(Steam.card("games", Some("arg")).is_none());
        assert!(Steam.card("unknown", None).is_none());
        let src = Steam.source("games").unwrap();
        assert_eq!(src.every(), None);
        assert!(Steam.source("other").is_none());
    }

    #[test]
    fn parses_real_steam_layout_across_libraries_and_filters_tools() {
        let tmp = temp_dir("usual");
        let steam = tmp.join("primary");
        let lib2 = tmp.join("mnt/II/Steam");
        std::fs::create_dir_all(steam.join("steamapps")).unwrap();
        std::fs::create_dir_all(lib2.join("steamapps")).unwrap();

        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"apps\"\n\t\t{{\n\t\t\t\"730\"\t\t\"1\"\n\t\t}}\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"apps\"\n\t\t{{\n\t\t}}\n\t}}\n}}\n",
            steam.display(),
            lib2.display()
        );
        std::fs::write(steam.join("steamapps/libraryfolders.vdf"), vdf).unwrap();

        // Played games across both libraries, plus a Steam tool entry each, plus never-played games.
        write_acf(
            &steam.join("steamapps"),
            "730",
            "Counter-Strike 2",
            4,
            1794484800,
        );
        write_acf(
            &lib2.join("steamapps"),
            "1422450",
            "Deadlock",
            4,
            1794500000,
        );
        write_acf(
            &lib2.join("steamapps"),
            "440",
            "Team Fortress 2",
            4,
            1794400000,
        );
        write_acf(
            &lib2.join("steamapps"),
            "3372060",
            "Hell Maiden",
            550,
            1793373285,
        );
        write_acf(&lib2.join("steamapps"), "1091500", "Cyberpunk 2077", 4, 0);
        write_acf(&lib2.join("steamapps"), "999", "Apex Legends", 4, 0);
        write_acf(
            &lib2.join("steamapps"),
            "1493710",
            "Proton Experimental",
            4,
            0,
        );
        write_acf(
            &lib2.join("steamapps"),
            "1628350",
            "Steam Linux Runtime 3.0 (sniper)",
            4,
            0,
        );
        write_acf(
            &lib2.join("steamapps"),
            "228980",
            "Steamworks Common Redistributables",
            4,
            0,
        );

        let mut src = SteamSource {
            root: Some(steam.clone()),
        };
        let data = src.fetch();
        let d = data.downcast_ref::<SteamData>().unwrap();
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(
            d.games
                .iter()
                .map(|g| (g.appid.as_str(), g.name.as_str(), g.state_flags))
                .collect::<Vec<_>>(),
            vec![
                ("1422450", "Deadlock", 4),
                ("730", "Counter-Strike 2", 4),
                ("440", "Team Fortress 2", 4),
                ("3372060", "Hell Maiden", 550),
                ("999", "Apex Legends", 4),
                ("1091500", "Cyberpunk 2077", 4),
            ],
            "LastPlayed descending, never-played last alphabetically, tools removed"
        );
    }

    #[test]
    fn handles_empty_tools_only_and_broken_steam_folders() {
        // 1. Missing folder
        let missing = temp_dir("missing").join("nope");
        let data = SteamSource {
            root: Some(missing),
        }
        .fetch();
        assert!(data.downcast_ref::<SteamData>().unwrap().games.is_empty());

        // 2. Empty folder (no libraryfolders.vdf)
        let empty = temp_dir("empty");
        std::fs::create_dir_all(empty.join("steamapps")).unwrap();
        let data = SteamSource {
            root: Some(empty.clone()),
        }
        .fetch();
        let _ = std::fs::remove_dir_all(&empty);
        assert!(data.downcast_ref::<SteamData>().unwrap().games.is_empty());

        // 3. Only tools installed -> empty after filter
        let tools_only = temp_dir("tools");
        std::fs::create_dir_all(tools_only.join("steamapps")).unwrap();
        std::fs::write(
            tools_only.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} }}",
                tools_only.display()
            ),
        )
        .unwrap();
        write_acf(
            &tools_only.join("steamapps"),
            "1493710",
            "Proton Experimental",
            4,
            0,
        );
        write_acf(
            &tools_only.join("steamapps"),
            "228980",
            "Steamworks Common Redistributables",
            4,
            0,
        );
        let data = SteamSource {
            root: Some(tools_only.clone()),
        }
        .fetch();
        let _ = std::fs::remove_dir_all(&tools_only);
        assert!(data.downcast_ref::<SteamData>().unwrap().games.is_empty());

        // 4. Broken VDF/ACF alongside one valid manifest and a missing second library
        let broken = temp_dir("broken");
        std::fs::create_dir_all(broken.join("steamapps")).unwrap();
        std::fs::write(
            broken.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"/nonexistent/steam/lib\"\n\t}}\n\t\"2\"\n\t{{\n\t\t\"path\"\t\t\"unclosed\n",
                broken.display()
            ),
        )
        .unwrap();
        std::fs::write(
            broken.join("steamapps/appmanifest_bad1.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"abc\"\n\t\"name\"\t\t\"Bad ID\"\n}\n",
        )
        .unwrap();
        std::fs::write(
            broken.join("steamapps/appmanifest_bad2.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"123\"\n\t\"name\"\t\t\"unterminated",
        )
        .unwrap();
        std::fs::write(
            broken.join("steamapps/appmanifest_440.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"440\"\n\t\"name\"\t\t\"Team \\\"Fortress\\\" 2\"\n\t\"LastPlayed\"\t\t\"not_a_num\"\n}\n",
        )
        .unwrap();
        let data = SteamSource {
            root: Some(broken.clone()),
        }
        .fetch();
        let _ = std::fs::remove_dir_all(&broken);
        let d = data.downcast_ref::<SteamData>().unwrap();
        assert_eq!(d.games.len(), 1);
        assert_eq!(d.games[0].appid, "440");
        assert_eq!(d.games[0].name, "Team \"Fortress\" 2");
        assert_eq!(d.games[0].last_played, 0);
    }

    #[test]
    fn finds_first_existing_steam_folder_in_priority_order() {
        let home = temp_dir("home");
        assert_eq!(find_steam_dir(&home), None);

        let flatpak = home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
        std::fs::create_dir_all(&flatpak).unwrap();
        assert_eq!(find_steam_dir(&home), Some(flatpak));

        let dot_steam = home.join(".steam/steam");
        std::fs::create_dir_all(&dot_steam).unwrap();
        assert_eq!(find_steam_dir(&home), Some(dot_steam));

        let local_steam = home.join(".local/share/Steam");
        std::fs::create_dir_all(&local_steam).unwrap();
        assert_eq!(find_steam_dir(&home), Some(local_steam));

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn formats_last_played_days_plainly() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        assert_eq!(format_played_day(today, today), "Last played today");
        assert_eq!(
            format_played_day(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(), today),
            "Last played yesterday"
        );
        assert_eq!(
            format_played_day(NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(), today),
            "Last played Sun 8 Nov"
        );
        assert_eq!(
            format_played_day(NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(), today),
            "Last played Mon 2 Nov"
        );
        assert_eq!(
            format_played_day(NaiveDate::from_ymd_opt(2025, 12, 15).unwrap(), today),
            "Last played Mon 15 Dec 2025"
        );
        let now = today.and_hms_opt(12, 0, 0).unwrap();
        assert_eq!(format_last_played(0, now), "Never played");
        // Noon UTC stays on 2026-11-12 in every timezone from UTC-11 to UTC+11.
        let noon_yesterday = NaiveDate::from_ymd_opt(2026, 11, 12)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp() as u64;
        assert_eq!(
            format_last_played(noon_yesterday, now),
            "Last played yesterday"
        );
    }

    fn sample_games(n: usize) -> SteamData {
        SteamData {
            games: (0..n)
                .map(|i| Game {
                    appid: format!("{}", 100 + i),
                    name: format!("Game {}", i + 1),
                    state_flags: 4,
                    last_played: if i == n - 1 {
                        0
                    } else {
                        1_794_500_000 - i as u64 * 86_400
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn status_transitions_and_events_emit_expected_actions() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx_s = test_cx(&th, Size::S);
        let cx_l = test_cx(&th, Size::L);
        let mut c = GamesCard::default();
        assert_eq!(c.status(&cx_s), Status::Loading);

        assert!(c.apply("games", &SteamData::default() as &dyn Any));
        assert_eq!(
            c.status(&cx_s),
            Status::Missing("Steam has no games installed".into())
        );

        let d = sample_games(10);
        assert!(c.apply("games", &d as &dyn Any));
        assert!(!c.apply("games", &d as &dyn Any), "same data: no redraw");
        assert!(
            !c.apply("other", &d as &dyn Any),
            "unrelated source key ignored"
        );
        assert_eq!(c.status(&cx_s), Status::Ready);

        // Clicking row 1 (hit 2) or Play button of row 2 (hit 5) opens its steam:// URI.
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: 2,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_s,
            &mut out
        ));
        assert_eq!(out, vec![Action::OpenUri("steam://rungameid/101".into())]);

        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: 5,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_s,
            &mut out
        ));
        assert_eq!(out, vec![Action::OpenUri("steam://rungameid/102".into())]);

        // Row 3 (hit 6) is hidden in Size::S (only 0..3 shown).
        let mut out = Vec::new();
        assert!(!c.event(
            &Event::Press {
                hit: 6,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_s,
            &mut out
        ));
        assert!(out.is_empty());

        // Right-click does nothing.
        let mut out = Vec::new();
        assert!(!c.event(
            &Event::Press {
                hit: 0,
                button: Button::Right,
                at: (0.0, 0.0),
            },
            &cx_s,
            &mut out
        ));
        assert!(out.is_empty());

        // Scroll is ignored in S, works in L and clamps to [0, n - LARGE_ROWS].
        let mut out = Vec::new();
        assert!(!c.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_s, &mut out));
        assert!(c.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out));
        assert_eq!(c.scroll, 1);
        assert!(c.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out));
        assert_eq!(c.scroll, 2);
        assert!(
            !c.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out),
            "clamped at bottom"
        );
        // After scrolling 2 down on 10 games (visible indices 2..10), the last game (idx 9, hit 18) is clickable.
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: 18,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_l,
            &mut out
        ));
        assert_eq!(out, vec![Action::OpenUri("steam://rungameid/109".into())]);
    }

    #[test]
    fn height_matches_drawn_rows_across_sizes_and_hover_exposes_play_hit() {
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 500).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut c = GamesCard::default();
        c.apply("games", &sample_games(10) as &dyn Any);

        for (size, expected_rows) in [
            (Size::S, SMALL_ROWS),
            (Size::M, MEDIUM_ROWS),
            (Size::L, LARGE_ROWS),
        ] {
            let cx = test_cx(&th, size);
            let h = c.height(&cx, 360.0);
            assert_eq!(h, expected_rows as f64 * ui::ROW_H);
            let body = (10.0, 20.0, 360.0, h);
            let mut hits = Hits::new(None, None);
            c.draw(&cr, &cx, body, &mut hits);
            assert_eq!(hits.list.len(), expected_rows);
            let last = hits.list.last().unwrap().1;
            assert_eq!(last.1 + last.3, body.1 + h);
            for (_, r) in &hits.list {
                assert!(r.1 >= body.1 && r.1 + r.3 <= body.1 + h);
            }
        }

        // Hovering row 0 registers Play button on top of the row hit.
        let cx = test_cx(&th, Size::M);
        let h = c.height(&cx, 360.0);
        let body = (10.0, 20.0, 360.0, h);
        let mut hits = Hits::new(Some((340.0, 40.0)), None);
        c.draw(&cr, &cx, body, &mut hits);
        assert_eq!(hits.list.len(), MEDIUM_ROWS + 1);
        assert_eq!(
            hits.top_at((340.0, 40.0)),
            Some(1),
            "Play hit lies over row 0"
        );
        assert_eq!(hits.top_at((30.0, 40.0)), Some(0), "rest of row 0 hits row");
    }
}
