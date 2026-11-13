//! Music (docs/EDGE_APPS.md section 6): any MPRIS player (`music/playing`) or a tied player
//! (`melodyae/playing`, `mpv/playing` matched by `DesktopEntry`). Reads MPRIS over `zbus` blocking
//! in `Source::fetch`, including Melodyae's lyrics and queue on `app.melodyae.Extras` (with
//! `TrackList` fallback), and sends playback controls through `Action::Run` with `dbus-send`.

use crate::card::{run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::theme::Rgba;
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI};
use std::ffi::{c_char, c_int, c_void, CString};
use std::fs::File;
use std::path::Path;
use std::sync::OnceLock;

pub struct Music;

const PLAYER_CATS: &[&str] = &["AudioVideo", "Player"];
const CLAIMS: &[Claim] = &[Claim::Categories(PLAYER_CATS)];

const KINDS: &[CardKind] = &[CardKind {
    id: "playing",
    title: "Playing",
    about: "What is playing, controls and queue",
    sizes: &[Size::S, Size::M, Size::L],
    default: Size::M,
    half: true,
    sources: &["music"],
}];

impl Provider for Music {
    fn id(&self) -> &'static str {
        "music"
    }
    fn name(&self) -> &'static str {
        "Music"
    }
    fn claims(&self) -> &'static [Claim] {
        CLAIMS
    }
    fn listed(&self) -> Listed {
        Listed::Never
    }
    fn cards(&self) -> &'static [CardKind] {
        KINDS
    }
    fn card(&self, kind: &str, _argument: Option<&str>) -> Option<Box<dyn Card>> {
        match kind {
            "playing" => Some(Box::new(MusicCard::default())),
            _ => None,
        }
    }
    fn source(&self, key: &str) -> Option<Box<dyn Source>> {
        match key {
            "music" => Some(Box::new(MusicSource)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Data

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Lyrics {
    pub before: String,
    pub current: String,
    pub next: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct QueueSong {
    pub title: String,
    pub artist: String,
    pub art_url: String,
    pub length_us: i64,
    /// Present when read from `org.mpris.MediaPlayer2.TrackList`, absent for `app.melodyae.Extras`.
    pub track_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerInfo {
    pub bus_name: String,
    pub desktop_entry: String,
    pub player_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub format: String,
    pub track_id: String,
    pub playing: bool,
    pub paused: bool,
    pub position_us: i64,
    pub duration_us: i64,
    pub shuffle: bool,
    pub loop_status: String,
    pub lyrics: Lyrics,
    pub queue: Vec<QueueSong>,
}

impl Default for PlayerInfo {
    fn default() -> Self {
        Self {
            bus_name: String::new(),
            desktop_entry: String::new(),
            player_name: String::new(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            art_url: String::new(),
            format: String::new(),
            track_id: String::new(),
            playing: false,
            paused: false,
            position_us: 0,
            duration_us: 0,
            shuffle: false,
            loop_status: "None".into(),
            lyrics: Lyrics::default(),
            queue: Vec::new(),
        }
    }
}

impl PlayerInfo {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty()
            && self.bus_name.is_empty()
            && self.player_name.is_empty()
            && self.desktop_entry.is_empty()
    }

    pub fn display_player(&self) -> &str {
        if !self.player_name.is_empty() {
            &self.player_name
        } else if let Some(suffix) = self.bus_name.strip_prefix("org.mpris.MediaPlayer2.") {
            suffix
        } else if !self.desktop_entry.is_empty() {
            &self.desktop_entry
        } else {
            "Player"
        }
    }

    pub fn matches_app(&self, app_id: &str) -> bool {
        let de = self
            .desktop_entry
            .strip_suffix(".desktop")
            .unwrap_or(&self.desktop_entry);
        !de.is_empty() && de.eq_ignore_ascii_case(app_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct MusicData {
    pub players: Vec<PlayerInfo>,
}

// ---------------------------------------------------------------------------------------------
// Parsing (render fixtures and tests)

/// Formats duration in microseconds to `m:ss` or `h:mm:ss`.
pub fn format_time(us: i64) -> String {
    if us <= 0 {
        return "0:00".to_string();
    }
    let total_secs = (us / 1_000_000) as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

/// Decodes a `file://` URL (bytes first, then UTF-8 so non-ASCII paths stay valid).
pub fn decode_file_url(url: &str) -> Option<String> {
    let path_str = url
        .strip_prefix("file://localhost")
        .or_else(|| url.strip_prefix("file://"))?;
    let mut out: Vec<u8> = Vec::with_capacity(path_str.len());
    let mut bytes = path_str.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            let byte_val = u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?;
            out.push(byte_val);
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).ok()
}

/// Splits multi-player fixture text at `---` lines into one `MusicData`.
pub fn parse_music(out: &str) -> MusicData {
    let mut players = Vec::new();
    let mut block = String::new();
    for line in out.lines() {
        if line.trim() == "---" {
            if let Some(p) = parse_player(&block) {
                players.push(p);
            }
            block.clear();
        } else {
            block.push_str(line);
            block.push('\n');
        }
    }
    if let Some(p) = parse_player(&block) {
        players.push(p);
    }
    MusicData { players }
}

/// Parses one player's state from `key=value` lines, TSV, or JSON.
pub fn parse_player(out: &str) -> Option<PlayerInfo> {
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('{') {
        return parse_json_player(trimmed);
    }

    let mut p = PlayerInfo::default();
    let mut has_fields = false;
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim();
            has_fields = true;
            match key.as_str() {
                "status" => {
                    p.playing = val.eq_ignore_ascii_case("playing");
                    p.paused = val.eq_ignore_ascii_case("paused");
                }
                "title" => p.title = val.to_string(),
                "artist" => p.artist = val.to_string(),
                "album" => p.album = val.to_string(),
                "player" => p.player_name = val.to_string(),
                "desktop_entry" | "desktopentry" => p.desktop_entry = val.to_string(),
                "bus" | "bus_name" => p.bus_name = val.to_string(),
                "track_id" | "trackid" => p.track_id = val.to_string(),
                "art_url" | "arturl" | "art" => p.art_url = val.to_string(),
                "format" => p.format = val.to_string(),
                "position" | "position_us" => {
                    p.position_us = val.parse::<i64>().unwrap_or(0).max(0)
                }
                "duration" | "duration_us" | "length" | "length_us" => {
                    p.duration_us = val.parse::<i64>().unwrap_or(0).max(0);
                }
                "shuffle" => {
                    p.shuffle = val == "1"
                        || val.eq_ignore_ascii_case("true")
                        || val.eq_ignore_ascii_case("on");
                }
                "loop" | "loop_status" => p.loop_status = val.to_string(),
                "lyrics_current" | "lyrics" => p.lyrics.current = val.to_string(),
                "lyrics_next" => p.lyrics.next = val.to_string(),
                "lyrics_before" => p.lyrics.before = val.to_string(),
                "queue" => {
                    let parts: Vec<&str> = val.split('\t').collect();
                    let title = parts.first().map(|s| s.trim()).unwrap_or("");
                    if !title.is_empty() {
                        p.queue.push(QueueSong {
                            title: title.to_string(),
                            artist: parts.get(1).map(|s| s.trim()).unwrap_or("").to_string(),
                            art_url: parts.get(2).map(|s| s.trim()).unwrap_or("").to_string(),
                            length_us: parts
                                .get(3)
                                .and_then(|s| s.trim().parse::<i64>().ok())
                                .unwrap_or(0)
                                .max(0),
                            track_id: parts
                                .get(4)
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .map(str::to_string),
                        });
                    }
                }
                _ => {}
            }
        } else if line.contains('\t') {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                has_fields = true;
                p.playing = parts[0].trim().eq_ignore_ascii_case("playing");
                p.paused = parts[0].trim().eq_ignore_ascii_case("paused");
                p.title = parts[1].trim().to_string();
                if let Some(v) = parts.get(2) {
                    p.artist = v.trim().to_string();
                }
                if let Some(v) = parts.get(3) {
                    p.album = v.trim().to_string();
                }
                if let Some(v) = parts.get(4) {
                    p.player_name = v.trim().to_string();
                }
                if let Some(v) = parts.get(5) {
                    p.art_url = v.trim().to_string();
                }
                if let Some(v) = parts.get(6) {
                    p.position_us = v.trim().parse::<i64>().unwrap_or(0).max(0);
                }
                if let Some(v) = parts.get(7) {
                    p.duration_us = v.trim().parse::<i64>().unwrap_or(0).max(0);
                }
                if let Some(v) = parts.get(8) {
                    let s = v.trim();
                    p.shuffle = s == "1" || s.eq_ignore_ascii_case("true");
                }
                if let Some(v) = parts.get(9) {
                    p.loop_status = v.trim().to_string();
                }
            }
        }
    }

    (has_fields && !p.is_empty()).then_some(p)
}

fn parse_json_player(text: &str) -> Option<PlayerInfo> {
    let title = extract_json_str(text, "title")?;
    let status = extract_json_str(text, "status").unwrap_or_default();
    let playing = status.eq_ignore_ascii_case("playing")
        || text.contains("\"playing\":true")
        || text.contains("\"playing\": true");
    let paused = status.eq_ignore_ascii_case("paused");
    let shuffle = text.contains("\"shuffle\":true") || text.contains("\"shuffle\": true");

    Some(PlayerInfo {
        bus_name: extract_json_str(text, "bus").unwrap_or_default(),
        desktop_entry: extract_json_str(text, "desktop_entry").unwrap_or_default(),
        player_name: extract_json_str(text, "player").unwrap_or_default(),
        title,
        artist: extract_json_str(text, "artist").unwrap_or_default(),
        album: extract_json_str(text, "album").unwrap_or_default(),
        art_url: extract_json_str(text, "art_url").unwrap_or_default(),
        format: extract_json_str(text, "format").unwrap_or_default(),
        track_id: String::new(),
        playing,
        paused,
        position_us: extract_json_num(text, "position").unwrap_or(0).max(0),
        duration_us: extract_json_num(text, "duration").unwrap_or(0).max(0),
        shuffle,
        loop_status: extract_json_str(text, "loop").unwrap_or_else(|| "None".into()),
        lyrics: Lyrics {
            before: extract_json_str(text, "before_lyrics").unwrap_or_default(),
            current: extract_json_str(text, "current_lyrics")
                .or_else(|| extract_json_str(text, "lyrics"))
                .unwrap_or_default(),
            next: extract_json_str(text, "next_lyrics").unwrap_or_default(),
        },
        queue: Vec::new(),
    })
}

fn extract_json_str(block: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let key_idx = block.find(&needle)?;
    let after = &block[key_idx + needle.len()..];
    let colon_idx = after.find(':')?;
    let quote_idx = after[colon_idx + 1..].find('"')?;
    let start = colon_idx + 1 + quote_idx + 1;
    let mut out = String::new();
    let mut chars = after[start..].chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            return Some(out);
        }
        if c == '\\' {
            if let Some(esc) = chars.next() {
                out.push(esc);
            }
        } else {
            out.push(c);
        }
    }
    None
}

fn extract_json_num(block: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\"");
    let key_idx = block.find(&needle)?;
    let after = &block[key_idx + needle.len()..];
    let colon_idx = after.find(':')?;
    let mut num_str = String::new();
    for c in after[colon_idx + 1..].chars() {
        if c.is_ascii_digit() || c == '-' {
            num_str.push(c);
        } else if !num_str.is_empty() {
            break;
        }
    }
    num_str.parse::<i64>().ok()
}

// ---------------------------------------------------------------------------------------------
// D-Bus Source

struct MusicSource;

impl Source for MusicSource {
    fn fetch(&mut self) -> Data {
        if std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_some_and(|p| !p.is_empty()) {
            let raw = run_sh("music", "");
            let raw = if raw.is_empty() {
                run_sh("media", "")
            } else {
                raw
            };
            return Box::new(parse_music(&raw));
        }
        Box::new(fetch_dbus())
    }
}

fn meta_str(meta: &HashMap<String, zbus::zvariant::OwnedValue>, key: &str) -> String {
    meta.get(key)
        .and_then(|v| {
            let val: &zbus::zvariant::Value<'_> = v;
            match val {
                zbus::zvariant::Value::Str(s) => Some(s.as_str().to_string()),
                zbus::zvariant::Value::ObjectPath(p) => Some(p.as_str().to_string()),
                _ => None,
            }
        })
        .unwrap_or_default()
}

fn meta_artists(meta: &HashMap<String, zbus::zvariant::OwnedValue>) -> String {
    meta.get("xesam:artist")
        .and_then(|v| {
            let val: &zbus::zvariant::Value<'_> = v;
            match val {
                zbus::zvariant::Value::Array(arr) => {
                    let parts: Vec<&str> = arr
                        .iter()
                        .filter_map(|item| match item {
                            zbus::zvariant::Value::Str(s) if !s.is_empty() => Some(s.as_str()),
                            _ => None,
                        })
                        .collect();
                    (!parts.is_empty()).then(|| parts.join(", "))
                }
                zbus::zvariant::Value::Str(s) => Some(s.as_str().to_string()),
                _ => None,
            }
        })
        .unwrap_or_default()
}

fn meta_i64(meta: &HashMap<String, zbus::zvariant::OwnedValue>, key: &str) -> i64 {
    meta.get(key)
        .and_then(|v| {
            let val: &zbus::zvariant::Value<'_> = v;
            match val {
                zbus::zvariant::Value::I64(n) => Some(*n),
                zbus::zvariant::Value::U64(n) => i64::try_from(*n).ok(),
                zbus::zvariant::Value::I32(n) => Some(i64::from(*n)),
                zbus::zvariant::Value::U32(n) => Some(i64::from(*n)),
                _ => None,
            }
        })
        .unwrap_or(0)
        .max(0)
}

pub fn fetch_dbus() -> MusicData {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return MusicData::default();
    };
    let Ok(dbus_proxy) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        return MusicData::default();
    };
    let Ok(names) = dbus_proxy.list_names() else {
        return MusicData::default();
    };

    let mut buses: Vec<String> = names
        .into_iter()
        .map(|n| n.to_string())
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2."))
        .collect();
    buses.sort();

    let mut players = Vec::new();
    for bus in buses {
        let Ok(player) = zbus::blocking::Proxy::new(
            &conn,
            bus.as_str(),
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ) else {
            continue;
        };

        let status: Result<String, _> = player.get_property("PlaybackStatus");
        let meta: Result<HashMap<String, zbus::zvariant::OwnedValue>, _> =
            player.get_property("Metadata");
        if status.is_err() && meta.is_err() {
            continue;
        }
        let status = status.unwrap_or_default();
        let meta = meta.unwrap_or_default();

        let (player_name, desktop_entry) = match zbus::blocking::Proxy::new(
            &conn,
            bus.as_str(),
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2",
        ) {
            Ok(root) => (
                root.get_property::<String>("Identity").unwrap_or_default(),
                root.get_property::<String>("DesktopEntry")
                    .unwrap_or_default(),
            ),
            Err(_) => (String::new(), String::new()),
        };

        let mut p = PlayerInfo {
            bus_name: bus.clone(),
            desktop_entry,
            player_name,
            title: meta_str(&meta, "xesam:title"),
            artist: meta_artists(&meta),
            album: meta_str(&meta, "xesam:album"),
            art_url: meta_str(&meta, "mpris:artUrl"),
            format: String::new(),
            track_id: meta_str(&meta, "mpris:trackid"),
            playing: status.eq_ignore_ascii_case("playing"),
            paused: status.eq_ignore_ascii_case("paused"),
            position_us: player.get_property::<i64>("Position").unwrap_or(0).max(0),
            duration_us: meta_i64(&meta, "mpris:length"),
            shuffle: player.get_property::<bool>("Shuffle").unwrap_or(false),
            loop_status: player
                .get_property::<String>("LoopStatus")
                .unwrap_or_else(|_| "None".into()),
            lyrics: Lyrics::default(),
            queue: Vec::new(),
        };

        // Melodyae exports lyrics and queue on `app.melodyae.Extras`; fall back to standard
        // `org.mpris.MediaPlayer2.TrackList` when absent.
        let mut got_queue = false;
        if let Ok(extras) = zbus::blocking::Proxy::new(
            &conn,
            bus.as_str(),
            "/org/mpris/MediaPlayer2",
            "app.melodyae.Extras",
        ) {
            if let Ok((curr, next, before)) =
                extras.call::<&str, (), (String, String, String)>("CurrentLyrics", &())
            {
                p.lyrics = Lyrics {
                    before,
                    current: curr,
                    next,
                };
            }
            if let Ok(items) =
                extras.call::<&str, (), Vec<(String, String, String, i64)>>("Queue", &())
            {
                got_queue = true;
                for (title, artist, art_url, length_us) in items {
                    p.queue.push(QueueSong {
                        title,
                        artist,
                        art_url,
                        length_us: length_us.max(0),
                        track_id: None,
                    });
                }
            }
        }

        if !got_queue {
            if let Ok(tl) = zbus::blocking::Proxy::new(
                &conn,
                bus.as_str(),
                "/org/mpris/MediaPlayer2",
                "org.mpris.MediaPlayer2.TrackList",
            ) {
                if let Ok(tracks) =
                    tl.get_property::<Vec<zbus::zvariant::OwnedObjectPath>>("Tracks")
                {
                    if !tracks.is_empty() {
                        let refs: Vec<zbus::zvariant::ObjectPath<'_>> =
                            tracks.iter().map(|t| t.as_ref()).collect();
                        if let Ok(metas) = tl
                            .call::<&str, _, Vec<HashMap<String, zbus::zvariant::OwnedValue>>>(
                                "GetTracksMetadata",
                                &(refs,),
                            )
                        {
                            for (track_path, tm) in tracks.iter().zip(metas) {
                                p.queue.push(QueueSong {
                                    title: meta_str(&tm, "xesam:title"),
                                    artist: meta_artists(&tm),
                                    art_url: meta_str(&tm, "mpris:artUrl"),
                                    length_us: meta_i64(&tm, "mpris:length"),
                                    track_id: Some(track_path.as_str().to_string()),
                                });
                            }
                        }
                    }
                }
            }
        }

        players.push(p);
    }

    MusicData { players }
}

// ---------------------------------------------------------------------------------------------
// Commands (`Action::Run`)

fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn valid_obj_path(p: &str) -> &str {
    let ok = p.starts_with('/')
        && (p == "/"
            || (!p.ends_with('/')
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '/')));
    if ok {
        p
    } else {
        "/org/mpris/MediaPlayer2/TrackList/NoTrack"
    }
}

fn dbus_cmd(bus: &str, args: &str) -> String {
    format!(
        "dbus-send --session --type=method_call --print-reply --reply-timeout=1500 \
         --dest={} /org/mpris/MediaPlayer2 {args} >/dev/null 2>&1",
        sq(bus)
    )
}

pub fn cmd_player(bus: &str, method: &str) -> String {
    dbus_cmd(bus, &format!("org.mpris.MediaPlayer2.Player.{method}"))
}

pub fn cmd_seek(bus: &str, track_id: &str, pos_us: i64) -> String {
    let path = valid_obj_path(track_id);
    dbus_cmd(
        bus,
        &format!(
            "org.mpris.MediaPlayer2.Player.SetPosition objpath:{} int64:{pos_us}",
            sq(path)
        ),
    )
}

pub fn cmd_shuffle(bus: &str, shuffle: bool) -> String {
    dbus_cmd(
        bus,
        &format!(
            "org.freedesktop.DBus.Properties.Set string:org.mpris.MediaPlayer2.Player \
             string:Shuffle variant:boolean:{shuffle}"
        ),
    )
}

pub fn cmd_loop(bus: &str, loop_status: &str) -> String {
    dbus_cmd(
        bus,
        &format!(
            "org.freedesktop.DBus.Properties.Set string:org.mpris.MediaPlayer2.Player \
             string:LoopStatus variant:string:{}",
            sq(loop_status)
        ),
    )
}

pub fn cmd_play_queue(bus: &str, idx: usize, track_id: Option<&str>) -> String {
    if let Some(tid) = track_id {
        dbus_cmd(
            bus,
            &format!(
                "org.mpris.MediaPlayer2.TrackList.GoTo objpath:{}",
                sq(valid_obj_path(tid))
            ),
        )
    } else {
        dbus_cmd(
            bus,
            &format!("app.melodyae.Extras.PlayTrack int32:{}", idx as i32),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Card

const MAX_QUEUE_VISIBLE: usize = 6;
const QUEUE_ROW_H: f64 = 36.0;

const H_OPEN_ROW: u32 = 0;
const H_OPEN: u32 = 1;
const H_SEEK: u32 = 2;
const H_SHUFFLE: u32 = 3;
const H_PREV: u32 = 4;
const H_PLAY: u32 = 5;
const H_NEXT: u32 = 6;
const H_REPEAT: u32 = 7;
const H_QUEUE: u32 = 10;

#[derive(Default)]
pub struct MusicCard {
    pub data: Option<MusicData>,
    /// Remembers the most recently playing bus for `music/playing` when all players pause.
    pub last_playing_bus: Option<String>,
    pub queue_scroll: usize,
    pub pending_seek_us: Option<i64>,
    dragging_seek: bool,
    seek_rect: Cell<Option<Rect>>,
}

impl MusicCard {
    fn player<'a>(&'a self, cx: &Cx) -> Option<&'a PlayerInfo> {
        let d = self.data.as_ref()?;
        if let Some(app) = cx.app {
            return d.players.iter().find(|p| p.matches_app(&app.id));
        }
        if let Some(last) = self.last_playing_bus.as_deref() {
            if let Some(p) = d.players.iter().find(|p| p.playing && p.bus_name == last) {
                return Some(p);
            }
        }
        if let Some(p) = d.players.iter().find(|p| p.playing) {
            return Some(p);
        }
        if let Some(last) = self.last_playing_bus.as_deref() {
            if let Some(p) = d.players.iter().find(|p| p.bus_name == last) {
                return Some(p);
            }
        }
        d.players
            .iter()
            .find(|p| p.paused)
            .or_else(|| d.players.iter().find(|p| !p.is_empty()))
    }

    fn player_mut<'a>(&'a mut self, cx: &Cx) -> Option<&'a mut PlayerInfo> {
        let bus = self.player(cx)?.bus_name.clone();
        let d = self.data.as_mut()?;
        if bus.is_empty() {
            d.players.first_mut()
        } else {
            d.players.iter_mut().find(|p| p.bus_name == bus)
        }
    }
}

fn cover_size_for(size: Size, width: f64) -> f64 {
    let cw = (width - 16.0).max(20.0);
    match size {
        Size::S => {
            if cw < 200.0 {
                44.0
            } else {
                48.0
            }
        }
        Size::M => {
            if cw < 200.0 {
                48.0
            } else {
                60.0
            }
        }
        Size::L => {
            if cw < 220.0 {
                56.0
            } else {
                92.0
            }
        }
    }
}

fn player_height(p: &PlayerInfo, size: Size, width: f64) -> f64 {
    let cw = (width - 16.0).max(20.0);
    let cov = cover_size_for(size, width);
    match size {
        Size::S => {
            if cw < 200.0 {
                cov + 6.0 + 32.0
            } else {
                cov
            }
        }
        Size::M => {
            let mut h = cov + 10.0;
            if !p.lyrics.current.is_empty() {
                h += 18.0;
                if !p.lyrics.next.is_empty() {
                    h += 16.0;
                }
                h += 6.0;
            }
            // Slider (18) + times (16) + gap (6) + controls (34)
            h += 18.0 + 16.0 + 6.0 + 34.0;
            if !p.queue.is_empty() {
                // Gap (10) + rule (1) + gap (7) + next/count line (16)
                h += 10.0 + 1.0 + 7.0 + 16.0;
            }
            h
        }
        Size::L => {
            let mut h = cov + 14.0;
            if !p.lyrics.before.is_empty()
                || !p.lyrics.current.is_empty()
                || !p.lyrics.next.is_empty()
            {
                if !p.lyrics.before.is_empty() {
                    h += 18.0;
                }
                if !p.lyrics.current.is_empty() {
                    h += 20.0;
                }
                if !p.lyrics.next.is_empty() {
                    h += 18.0;
                }
                h += 6.0;
            }
            // Slider (18) + times (16) + gap (8) + controls (34)
            h += 18.0 + 16.0 + 8.0 + 34.0;
            if !p.queue.is_empty() {
                let vis = p.queue.len().min(MAX_QUEUE_VISIBLE);
                h += 16.0 + 24.0 + vis as f64 * QUEUE_ROW_H;
            }
            h
        }
    }
}

impl Card for MusicCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "music", data.downcast_ref::<MusicData>()) else {
            return false;
        };
        let mut next = d.clone();

        let still_playing = self
            .last_playing_bus
            .as_deref()
            .is_some_and(|bus| next.players.iter().any(|p| p.playing && p.bus_name == bus));
        if !still_playing {
            if let Some(p) = next.players.iter().find(|p| p.playing) {
                self.last_playing_bus = Some(p.bus_name.clone());
            }
        }

        // Keep the dragged slider position across background refreshes until pointer release.
        if let Some(pending) = self.pending_seek_us {
            let idx = self
                .last_playing_bus
                .as_deref()
                .and_then(|bus| next.players.iter().position(|p| p.bus_name == bus))
                .or_else(|| (!next.players.is_empty()).then_some(0));
            if let Some(i) = idx {
                next.players[i].position_us = pending;
            }
        }

        let max_q = next
            .players
            .iter()
            .map(|p| p.queue.len())
            .max()
            .unwrap_or(0);
        self.queue_scroll = self
            .queue_scroll
            .min(max_q.saturating_sub(MAX_QUEUE_VISIBLE));

        let changed = self.data.as_ref() != Some(&next);
        self.data = Some(next);
        changed
    }

    fn status(&self, cx: &Cx) -> Status {
        if self.data.is_none() {
            return Status::Loading;
        }
        if cx.app.is_some() {
            return Status::Ready;
        }
        if self.player(cx).is_some() {
            Status::Ready
        } else {
            Status::Hidden
        }
    }

    fn height(&self, cx: &Cx, width: f64) -> f64 {
        let Some(p) = self.player(cx) else {
            return if cx.app.is_some() { ui::ROW_H } else { 0.0 };
        };
        player_height(p, cx.size, width)
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(p) = self.player(cx) else {
            if let Some(app) = cx.app {
                draw_not_running(cr, cx, body, hits, &app.name);
            }
            return;
        };

        match cx.size {
            Size::S => {
                self.seek_rect.set(None);
                draw_small(cr, cx, body, hits, p);
            }
            Size::M => draw_medium(cr, cx, body, hits, p, &self.seek_rect),
            Size::L => draw_large(cr, cx, body, hits, p, self.queue_scroll, &self.seek_rect),
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let has_player = self.player(cx).is_some();
        if !has_player {
            let (
                Some(app),
                Event::Press {
                    hit,
                    button: Button::Left,
                    ..
                },
            ) = (cx.app, ev)
            else {
                return false;
            };
            if *hit == H_OPEN || *hit == H_OPEN_ROW {
                out.push(Action::Launch(app.id.clone()));
                return true;
            }
            return false;
        }

        match ev {
            Event::Scroll { dy, .. } if cx.size == Size::L => {
                let len = self.player(cx).map_or(0, |p| p.queue.len());
                let max_scroll = len.saturating_sub(MAX_QUEUE_VISIBLE);
                if max_scroll == 0 {
                    return false;
                }
                if *dy > 0.0 && self.queue_scroll < max_scroll {
                    self.queue_scroll += 1;
                    return true;
                }
                if *dy < 0.0 && self.queue_scroll > 0 {
                    self.queue_scroll -= 1;
                    return true;
                }
                false
            }
            Event::Drag { hit, at } if *hit == H_SEEK => {
                let r = self.seek_rect.get().unwrap_or((18.0, 0.0, 344.0, 18.0));
                let part = ui::slider_part(r, *at);
                let Some(p) = self.player_mut(cx) else {
                    return false;
                };
                if p.duration_us <= 0 {
                    return false;
                }
                let seek_us = (part * p.duration_us as f64).round() as i64;
                p.position_us = seek_us;
                self.pending_seek_us = Some(seek_us);
                self.dragging_seek = true;
                true
            }
            Event::Release { hit } if *hit == H_SEEK => {
                let seek_us = self.pending_seek_us.take();
                let was_dragging = std::mem::take(&mut self.dragging_seek);
                if was_dragging {
                    if let (Some(pos), Some(p)) = (seek_us, self.player(cx)) {
                        if !p.bus_name.is_empty() {
                            out.push(Action::Run(cmd_seek(&p.bus_name, &p.track_id, pos)));
                        }
                    }
                    return true;
                }
                false
            }
            Event::Press {
                hit,
                button: Button::Left,
                at,
            } => {
                if *hit == H_SEEK {
                    let r = self.seek_rect.get().unwrap_or((18.0, 0.0, 344.0, 18.0));
                    let part = ui::slider_part(r, *at);
                    let Some(p) = self.player_mut(cx) else {
                        return false;
                    };
                    if p.duration_us <= 0 {
                        return false;
                    }
                    let seek_us = (part * p.duration_us as f64).round() as i64;
                    p.position_us = seek_us;
                    if !p.bus_name.is_empty() {
                        out.push(Action::Run(cmd_seek(&p.bus_name, &p.track_id, seek_us)));
                    }
                    self.pending_seek_us = Some(seek_us);
                    self.dragging_seek = false;
                    return true;
                }

                let scroll = self.queue_scroll;
                let Some(p) = self.player_mut(cx) else {
                    return false;
                };
                let bus = p.bus_name.clone();
                match *hit {
                    H_PLAY => {
                        p.playing = !p.playing;
                        p.paused = !p.playing;
                        if !bus.is_empty() {
                            out.push(Action::Run(cmd_player(&bus, "PlayPause")));
                        }
                        true
                    }
                    H_NEXT => {
                        if !bus.is_empty() {
                            out.push(Action::Run(cmd_player(&bus, "Next")));
                        }
                        true
                    }
                    H_PREV => {
                        if !bus.is_empty() {
                            out.push(Action::Run(cmd_player(&bus, "Previous")));
                        }
                        true
                    }
                    H_SHUFFLE => {
                        p.shuffle = !p.shuffle;
                        if !bus.is_empty() {
                            out.push(Action::Run(cmd_shuffle(&bus, p.shuffle)));
                        }
                        true
                    }
                    H_REPEAT => {
                        p.loop_status = match p.loop_status.as_str() {
                            "None" | "" => "Playlist".into(),
                            "Playlist" => "Track".into(),
                            _ => "None".into(),
                        };
                        if !bus.is_empty() {
                            out.push(Action::Run(cmd_loop(&bus, &p.loop_status)));
                        }
                        true
                    }
                    h if h >= H_QUEUE && cx.size == Size::L => {
                        let song_idx = scroll + (h - H_QUEUE) as usize;
                        let Some(song) = p.queue.get(song_idx) else {
                            return false;
                        };
                        if !bus.is_empty() {
                            out.push(Action::Run(cmd_play_queue(
                                &bus,
                                song_idx,
                                song.track_id.as_deref(),
                            )));
                        }
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

fn draw_not_running(cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits, app_name: &str) {
    let row_r = (body.0, body.1, body.2, ui::ROW_H);
    let btn_t = Text {
        size: 12.0,
        bold: false,
        color: cx.theme.text,
        text: "Open",
    };
    let btn_w = draw::measure(cr, &btn_t).0 + 18.0;
    ui::row(
        cr,
        cx,
        hits,
        H_OPEN_ROW,
        row_r,
        &Row {
            icon: None,
            name: app_name,
            state: "",
            on: false,
            right: btn_w + 8.0,
        },
    );
    let btn_x = row_r.0 + row_r.2 - 8.0 - btn_w;
    let btn_y = row_r.1 + (ui::ROW_H - 26.0) / 2.0;
    ui::button(cr, cx, hits, H_OPEN, btn_x, btn_y, "Open");
}

fn draw_small(cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits, p: &PlayerInfo) {
    let th = cx.theme;
    let (cx_left, cx_right) = (body.0 + 8.0, body.0 + body.2 - 8.0);
    let cw = (cx_right - cx_left).max(20.0);
    let cov = cover_size_for(Size::S, body.2);
    let narrow = cw < 200.0;

    draw_cover(cr, cx, cx_left, body.1, cov, &p.art_url);

    let title_str = if p.title.is_empty() {
        p.display_player()
    } else {
        &p.title
    };
    let sub_str = if !p.artist.is_empty() {
        p.artist.as_str()
    } else if cx.app.is_none() {
        p.display_player()
    } else {
        ""
    };

    if narrow {
        let text_x = cx_left + cov + 10.0;
        let max_w = (cx_right - text_x).max(20.0);
        let ty = body.1 + 4.0;
        draw::text_max(
            cr,
            text_x,
            ty,
            max_w,
            &Text {
                size: 13.0,
                bold: true,
                color: th.text,
                text: title_str,
            },
        );
        if !sub_str.is_empty() {
            draw::text_max(
                cr,
                text_x,
                ty + 18.0,
                max_w,
                &Text {
                    size: 11.0,
                    bold: false,
                    color: th.muted,
                    text: sub_str,
                },
            );
        }
        let btn_y = body.1 + cov + 6.0;
        let total_w = 34.0 + 8.0 + 32.0;
        let play_x = cx_left + (cw - total_w) / 2.0;
        draw_play_button(cr, cx, hits, (play_x, btn_y, 34.0, 32.0), p.playing);
        draw_icon_button(
            cr,
            cx,
            hits,
            H_NEXT,
            (play_x + 42.0, btn_y, 32.0, 32.0),
            th.text,
            draw_next_icon,
        );
    } else {
        let btn_total_w = 36.0 + 8.0 + 32.0;
        let play_x = cx_right - btn_total_w;
        let btn_y = body.1 + (cov - 32.0) / 2.0;
        let text_x = cx_left + cov + 12.0;
        let max_w = (play_x - 10.0 - text_x).max(20.0);
        let ty = if sub_str.is_empty() {
            body.1 + (cov - 18.0) / 2.0
        } else {
            body.1 + 6.0
        };
        draw::text_max(
            cr,
            text_x,
            ty,
            max_w,
            &Text {
                size: 13.5,
                bold: true,
                color: th.text,
                text: title_str,
            },
        );
        if !sub_str.is_empty() {
            draw::text_max(
                cr,
                text_x,
                ty + 19.0,
                max_w,
                &Text {
                    size: 11.5,
                    bold: false,
                    color: th.muted,
                    text: sub_str,
                },
            );
        }
        draw_play_button(cr, cx, hits, (play_x, btn_y, 36.0, 32.0), p.playing);
        draw_icon_button(
            cr,
            cx,
            hits,
            H_NEXT,
            (play_x + 44.0, btn_y, 32.0, 32.0),
            th.text,
            draw_next_icon,
        );
    }
}

fn draw_medium(
    cr: &Context,
    cx: &Cx,
    body: Rect,
    hits: &mut Hits,
    p: &PlayerInfo,
    seek_rect: &Cell<Option<Rect>>,
) {
    let th = cx.theme;
    let (cx_left, cx_right) = (body.0 + 8.0, body.0 + body.2 - 8.0);
    let cw = (cx_right - cx_left).max(20.0);
    let cov = cover_size_for(Size::M, body.2);
    let mut y = body.1;

    draw_cover(cr, cx, cx_left, y, cov, &p.art_url);

    let text_x = cx_left + cov + 12.0;
    let max_w = (cx_right - text_x).max(20.0);
    let title_str = if p.title.is_empty() {
        p.display_player()
    } else {
        &p.title
    };
    let sub = if cx.app.is_some() {
        if !p.artist.is_empty() {
            p.artist.clone()
        } else {
            p.album.clone()
        }
    } else if !p.artist.is_empty() {
        format!("{} · {}", p.artist, p.display_player())
    } else {
        p.display_player().to_string()
    };

    draw::text_max(
        cr,
        text_x,
        y + (cov - 38.0).max(0.0) / 2.0,
        max_w,
        &Text {
            size: 15.0,
            bold: true,
            color: th.text,
            text: title_str,
        },
    );
    if !sub.is_empty() {
        draw::text_max(
            cr,
            text_x,
            y + (cov - 38.0).max(0.0) / 2.0 + 22.0,
            max_w,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: &sub,
            },
        );
    }
    y += cov + 10.0;

    if !p.lyrics.current.is_empty() {
        draw::text_max(
            cr,
            cx_left,
            y,
            cw,
            &Text {
                size: 13.0,
                bold: true,
                color: th.text,
                text: &p.lyrics.current,
            },
        );
        y += 18.0;
        if !p.lyrics.next.is_empty() {
            draw::text_max(
                cr,
                cx_left,
                y,
                cw,
                &Text {
                    size: 11.5,
                    bold: false,
                    color: th.muted,
                    text: &p.lyrics.next,
                },
            );
            y += 16.0;
        }
        y += 6.0;
    }

    y = draw_progress(cr, cx, hits, cx_left, y, cw, p, seek_rect);
    y += 6.0;
    y = draw_controls(cr, cx, hits, cx_left, y, cw, p);

    if !p.queue.is_empty() {
        y += 10.0;
        draw::round_rect(cr, cx_left, y, cw, 1.0, 0.5);
        draw::set(cr, th.track);
        let _ = cr.fill();
        y += 8.0;

        let q_str = format!("Queue {}", p.queue.len());
        let q_w = draw::text_right(
            cr,
            cx_right,
            y,
            &Text {
                size: 11.5,
                bold: false,
                color: th.muted,
                text: &q_str,
            },
        );
        if let Some(first) = p.queue.first() {
            let next_str = if first.artist.is_empty() {
                format!("Next: {}", first.title)
            } else {
                format!("Next: {} · {}", first.title, first.artist)
            };
            draw::text_max(
                cr,
                cx_left,
                y,
                (cw - q_w - 10.0).max(20.0),
                &Text {
                    size: 11.5,
                    bold: false,
                    color: th.muted,
                    text: &next_str,
                },
            );
        }
    }
}

fn draw_large(
    cr: &Context,
    cx: &Cx,
    body: Rect,
    hits: &mut Hits,
    p: &PlayerInfo,
    queue_scroll: usize,
    seek_rect: &Cell<Option<Rect>>,
) {
    let th = cx.theme;
    let (cx_left, cx_right) = (body.0 + 8.0, body.0 + body.2 - 8.0);
    let cw = (cx_right - cx_left).max(20.0);
    let cov = cover_size_for(Size::L, body.2);
    let mut y = body.1;

    draw_cover(cr, cx, cx_left, y, cov, &p.art_url);

    let text_x = cx_left + cov + 14.0;
    let max_w = (cx_right - text_x).max(20.0);
    let title_str = if p.title.is_empty() {
        p.display_player()
    } else {
        &p.title
    };
    let compact = cw < 220.0;
    let title_sz = if compact { 15.0 } else { 20.0 };
    let (title_dy, sub_dy, sub_sz, detail_dy) = if compact {
        (0.0, 21.0, 12.0, 38.0)
    } else {
        (4.0, 34.0, 13.0, 56.0)
    };
    draw::text_max(
        cr,
        text_x,
        y + title_dy,
        max_w,
        &Text {
            size: title_sz,
            bold: true,
            color: th.text,
            text: title_str,
        },
    );

    let artist_album = match (p.artist.is_empty(), p.album.is_empty()) {
        (false, false) => format!("{} · {}", p.artist, p.album),
        (false, true) => p.artist.clone(),
        (true, false) => p.album.clone(),
        (true, true) => String::new(),
    };
    if !artist_album.is_empty() {
        draw::text_max(
            cr,
            text_x,
            y + sub_dy,
            max_w,
            &Text {
                size: sub_sz,
                bold: false,
                color: th.muted,
                text: &artist_album,
            },
        );
    }

    let detail = if !p.format.is_empty() {
        p.format.clone()
    } else if cx.app.is_none() {
        p.display_player().to_string()
    } else {
        String::new()
    };
    if !detail.is_empty() {
        draw::text_max(
            cr,
            text_x,
            y + detail_dy,
            max_w,
            &Text {
                size: 11.0,
                bold: false,
                color: th.muted,
                text: &detail,
            },
        );
    }
    y += cov + 14.0;

    if !p.lyrics.before.is_empty() || !p.lyrics.current.is_empty() || !p.lyrics.next.is_empty() {
        if !p.lyrics.before.is_empty() {
            draw::text_max(
                cr,
                cx_left,
                y,
                cw,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: &p.lyrics.before,
                },
            );
            y += 18.0;
        }
        if !p.lyrics.current.is_empty() {
            draw::text_max(
                cr,
                cx_left,
                y,
                cw,
                &Text {
                    size: 14.0,
                    bold: true,
                    color: th.accent,
                    text: &p.lyrics.current,
                },
            );
            y += 20.0;
        }
        if !p.lyrics.next.is_empty() {
            draw::text_max(
                cr,
                cx_left,
                y,
                cw,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: &p.lyrics.next,
                },
            );
            y += 18.0;
        }
        y += 6.0;
    }

    y = draw_progress(cr, cx, hits, cx_left, y, cw, p, seek_rect);
    y += 8.0;
    y = draw_controls(cr, cx, hits, cx_left, y, cw, p);

    if !p.queue.is_empty() {
        y += 16.0;
        let total_mins = p
            .queue
            .iter()
            .map(|s| s.length_us / 60_000_000)
            .sum::<i64>();
        let word = if p.queue.len() == 1 { "song" } else { "songs" };
        let summary = format!("{} {word} · {total_mins} min", p.queue.len());
        draw::text(
            cr,
            cx_left,
            y,
            &Text {
                size: 13.5,
                bold: true,
                color: th.text,
                text: "Up next",
            },
        );
        draw::text_right(
            cr,
            cx_right,
            y + 1.0,
            &Text {
                size: 11.5,
                bold: false,
                color: th.muted,
                text: &summary,
            },
        );
        y += 24.0;

        let start = queue_scroll.min(p.queue.len().saturating_sub(MAX_QUEUE_VISIBLE));
        for (i, song) in p
            .queue
            .iter()
            .skip(start)
            .take(MAX_QUEUE_VISIBLE)
            .enumerate()
        {
            let row_r = (body.0, y, body.2, QUEUE_ROW_H);
            let id = H_QUEUE + i as u32;
            if hits.add(id, row_r) || hits.selected(id) {
                ui::light(cr, cx, row_r);
            }
            draw_cover(cr, cx, cx_left, y + 4.0, 28.0, &song.art_url);

            let dur_str = format_time(song.length_us);
            let dur_w = if song.length_us > 0 {
                draw::text_right(
                    cr,
                    cx_right,
                    y + 10.0,
                    &Text {
                        size: 11.0,
                        bold: false,
                        color: th.muted,
                        text: &dur_str,
                    },
                ) + 10.0
            } else {
                0.0
            };
            let s_text_x = cx_left + 36.0;
            let s_max_w = (cx_right - dur_w - s_text_x).max(20.0);
            draw::text_max(
                cr,
                s_text_x,
                y + 4.0,
                s_max_w,
                &Text {
                    size: 12.5,
                    bold: false,
                    color: th.text,
                    text: &song.title,
                },
            );
            if !song.artist.is_empty() {
                draw::text_max(
                    cr,
                    s_text_x,
                    y + 20.0,
                    s_max_w,
                    &Text {
                        size: 11.0,
                        bold: false,
                        color: th.muted,
                        text: &song.artist,
                    },
                );
            }
            y += QUEUE_ROW_H;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_progress(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    x: f64,
    y: f64,
    w: f64,
    p: &PlayerInfo,
    seek_rect: &Cell<Option<Rect>>,
) -> f64 {
    let th = cx.theme;
    let slider_r = (x, y, w, 18.0);
    if p.duration_us > 0 {
        let part = (p.position_us as f64 / p.duration_us as f64).clamp(0.0, 1.0);
        ui::slider(cr, cx, hits, H_SEEK, slider_r, part);
        seek_rect.set(Some(slider_r));
    } else {
        draw::bar(cr, th, x, y + 7.5, w, 0.0, th.accent);
        seek_rect.set(None);
    }
    let ty = y + 18.0;
    let left_t = format_time(p.position_us);
    let right_t = format_time(p.duration_us);
    draw::text(
        cr,
        x,
        ty,
        &Text {
            size: 11.0,
            bold: false,
            color: th.muted,
            text: &left_t,
        },
    );
    draw::text_right(
        cr,
        x + w,
        ty,
        &Text {
            size: 11.0,
            bold: false,
            color: th.muted,
            text: &right_t,
        },
    );
    ty + 16.0
}

fn draw_controls(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    x: f64,
    y: f64,
    w: f64,
    p: &PlayerInfo,
) -> f64 {
    let th = cx.theme;
    let gap = ((w - 5.0 * 36.0) / 4.0).clamp(4.0, 12.0);
    let btn_w = ((w - 4.0 * gap) / 5.0).min(36.0);
    let total_w = 5.0 * btn_w + 4.0 * gap;
    let start_x = x + (w - total_w) / 2.0;

    let shuf_col = if p.shuffle { th.accent } else { th.muted };
    let rep_col = if p.loop_status != "None" && !p.loop_status.is_empty() {
        th.accent
    } else {
        th.muted
    };

    let bx = |i: usize| start_x + i as f64 * (btn_w + gap);
    draw_icon_button(
        cr,
        cx,
        hits,
        H_SHUFFLE,
        (bx(0), y, btn_w, 34.0),
        shuf_col,
        draw_shuffle_icon,
    );
    draw_icon_button(
        cr,
        cx,
        hits,
        H_PREV,
        (bx(1), y, btn_w, 34.0),
        th.text,
        draw_prev_icon,
    );
    draw_play_button(cr, cx, hits, (bx(2), y, btn_w, 34.0), p.playing);
    draw_icon_button(
        cr,
        cx,
        hits,
        H_NEXT,
        (bx(3), y, btn_w, 34.0),
        th.text,
        draw_next_icon,
    );
    draw_icon_button(
        cr,
        cx,
        hits,
        H_REPEAT,
        (bx(4), y, btn_w, 34.0),
        rep_col,
        draw_repeat_icon,
    );
    y + 34.0
}

fn draw_icon_button(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    id: u32,
    r: Rect,
    color: Rgba,
    icon_fn: fn(&Context, f64, f64, Rgba),
) {
    if hits.add(id, r) || hits.selected(id) {
        ui::light(cr, cx, r);
    }
    icon_fn(
        cr,
        r.0 + (r.2 - 18.0) / 2.0,
        r.1 + (r.3 - 18.0) / 2.0,
        color,
    );
}

fn draw_play_button(cr: &Context, cx: &Cx, hits: &mut Hits, r: Rect, playing: bool) {
    let th = cx.theme;
    let lit = hits.add(H_PLAY, r) || hits.selected(H_PLAY);
    draw::round_rect(
        cr,
        r.0 + 0.5,
        r.1 + 0.5,
        r.2 - 1.0,
        r.3 - 1.0,
        th.small_radius(),
    );
    if playing {
        draw::set(cr, th.accent.alpha(if lit { 0.26 } else { 0.16 }));
        let _ = cr.fill_preserve();
    } else if lit {
        draw::set(cr, th.text.alpha(0.08));
        let _ = cr.fill_preserve();
    }
    draw::set(cr, th.accent);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
    let ix = r.0 + (r.2 - 18.0) / 2.0;
    let iy = r.1 + (r.3 - 18.0) / 2.0;
    if playing {
        draw_pause_icon(cr, ix, iy, th.accent);
    } else {
        draw_play_icon(cr, ix + 1.0, iy, th.accent);
    }
}

// ---------------------------------------------------------------------------------------------
// Cover art and line icons

thread_local! {
    static COVERS: RefCell<Vec<(String, i32, cairo::ImageSurface)>> =
        const { RefCell::new(Vec::new()) };
}

struct PixbufFns {
    new_at_scale:
        unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, *mut *mut c_void) -> *mut c_void,
    pixels: unsafe extern "C" fn(*mut c_void) -> *mut u8,
    width: unsafe extern "C" fn(*mut c_void) -> c_int,
    height: unsafe extern "C" fn(*mut c_void) -> c_int,
    rowstride: unsafe extern "C" fn(*mut c_void) -> c_int,
    channels: unsafe extern "C" fn(*mut c_void) -> c_int,
    unref: unsafe extern "C" fn(*mut c_void),
    g_error_free: Option<unsafe extern "C" fn(*mut c_void)>,
}

fn pixbuf_fns() -> Option<&'static PixbufFns> {
    static PB: OnceLock<Option<PixbufFns>> = OnceLock::new();
    PB.get_or_init(|| unsafe {
        let lib = libc::dlopen(
            CString::new("libgdk_pixbuf-2.0.so.0").ok()?.as_ptr(),
            libc::RTLD_LAZY,
        );
        let gobj = libc::dlopen(
            CString::new("libgobject-2.0.so.0").ok()?.as_ptr(),
            libc::RTLD_LAZY,
        );
        if lib.is_null() || gobj.is_null() {
            return None;
        }
        let glib = libc::dlopen(
            CString::new("libglib-2.0.so.0").ok()?.as_ptr(),
            libc::RTLD_LAZY,
        );
        let sym = |h: *mut c_void, n: &str| -> Option<*mut c_void> {
            let c = CString::new(n).ok()?;
            let p = libc::dlsym(h, c.as_ptr());
            (!p.is_null()).then_some(p)
        };
        let err = if glib.is_null() {
            None
        } else {
            sym(glib, "g_error_free")
                .map(|p| std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(p))
        };
        Some(PixbufFns {
            new_at_scale: std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, *mut *mut c_void) -> *mut c_void,
            >(sym(lib, "gdk_pixbuf_new_from_file_at_scale")?),
            pixels: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> *mut u8>(
                sym(lib, "gdk_pixbuf_get_pixels")?,
            ),
            width: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_width")?,
            ),
            height: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_height")?,
            ),
            rowstride: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_rowstride")?,
            ),
            channels: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_n_channels")?,
            ),
            unref: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(
                sym(gobj, "g_object_unref")?,
            ),
            g_error_free: err,
        })
    })
    .as_ref()
}

fn load_pixbuf_surface(path: &Path, size: i32) -> Option<cairo::ImageSurface> {
    let fns = pixbuf_fns()?;
    let c_path = CString::new(path.to_str()?).ok()?;
    unsafe {
        let mut err: *mut c_void = std::ptr::null_mut();
        let pb = (fns.new_at_scale)(c_path.as_ptr(), size, size, 0, &mut err);
        if !err.is_null() {
            if let Some(free_fn) = fns.g_error_free {
                free_fn(err);
            }
        }
        if pb.is_null() {
            return None;
        }
        let (w, h) = ((fns.width)(pb), (fns.height)(pb));
        let (stride, ch) = ((fns.rowstride)(pb) as usize, (fns.channels)(pb) as usize);
        let px = (fns.pixels)(pb);
        let out = (|| {
            if w <= 0 || h <= 0 || px.is_null() || !(ch == 3 || ch == 4) {
                return None;
            }
            let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).ok()?;
            let dst_stride = surf.stride() as usize;
            {
                let mut data = surf.data().ok()?;
                for y in 0..h as usize {
                    let row = std::slice::from_raw_parts(px.add(y * stride), w as usize * ch);
                    for x in 0..w as usize {
                        let s = &row[x * ch..x * ch + ch];
                        let a = if ch == 4 { s[3] as u32 } else { 255 };
                        let pm = |c: u8| (c as u32 * a + 127) / 255;
                        let v = (a << 24) | (pm(s[0]) << 16) | (pm(s[1]) << 8) | pm(s[2]);
                        data[y * dst_stride + x * 4..y * dst_stride + x * 4 + 4]
                            .copy_from_slice(&v.to_ne_bytes());
                    }
                }
            }
            surf.mark_dirty();
            Some(surf)
        })();
        (fns.unref)(pb);
        out
    }
}

fn load_cover_surface(path: &Path, size: i32) -> Option<cairo::ImageSurface> {
    if size <= 0 || !path.is_file() {
        return None;
    }
    let mut f = File::open(path).ok()?;
    let mut magic = [0u8; 8];
    let is_png =
        std::io::Read::read_exact(&mut f, &mut magic).is_ok() && magic == *b"\x89PNG\r\n\x1a\n";
    if is_png {
        drop(f);
        let mut f = File::open(path).ok()?;
        let src = cairo::ImageSurface::create_from_png(&mut f).ok()?;
        let (sw, sh) = (src.width(), src.height());
        if sw <= 0 || sh <= 0 {
            return None;
        }
        if sw == size && sh == size {
            return Some(src);
        }
        let dst = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size).ok()?;
        let cr = cairo::Context::new(&dst).ok()?;
        let scale = (size as f64 / sw as f64).max(size as f64 / sh as f64);
        let dx = (size as f64 - sw as f64 * scale) / 2.0;
        let dy = (size as f64 - sh as f64 * scale) / 2.0;
        cr.translate(dx, dy);
        cr.scale(scale, scale);
        let _ = cr.set_source_surface(&src, 0.0, 0.0);
        let _ = cr.paint();
        drop(cr);
        dst.flush();
        return Some(dst);
    }
    load_pixbuf_surface(path, size)
}

fn cover_surface(p: &Path, size: i32) -> Option<cairo::ImageSurface> {
    let key = p.to_string_lossy().to_string();
    if let Some(hit) = COVERS.with(|c| {
        c.borrow()
            .iter()
            .find(|(k, s, _)| *k == key && *s == size)
            .map(|(_, _, surf)| surf.clone())
    }) {
        return Some(hit);
    }
    let surf = load_cover_surface(p, size)?;
    COVERS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 16 {
            c.remove(0);
        }
        c.push((key, size, surf.clone()));
    });
    Some(surf)
}

fn draw_cover(cr: &Context, cx: &Cx, x: f64, y: f64, size: f64, art_url: &str) {
    let th = cx.theme;
    if !art_url.is_empty() {
        let local_path = decode_file_url(art_url).unwrap_or_else(|| art_url.to_string());
        let p = Path::new(&local_path);
        if p.is_absolute() && p.is_file() {
            if let Some(surf) = cover_surface(p, size.round() as i32) {
                let _ = cr.save();
                draw::round_rect(cr, x, y, size, size, th.small_radius());
                cr.clip();
                let _ = cr.set_source_surface(&surf, x, y);
                let _ = cr.paint();
                let _ = cr.restore();
                return;
            }
        }
    }

    draw::round_rect(cr, x, y, size, size, th.small_radius());
    draw::set(cr, th.track);
    let _ = cr.fill();

    let icon_size = (size * 0.40).clamp(14.0, 32.0);
    let ix = x + (size - icon_size) / 2.0;
    let iy = y + (size - icon_size) / 2.0;
    draw_music_note_icon(cr, ix, iy, icon_size, th.muted);
}

fn draw_music_note_icon(cr: &Context, x: f64, y: f64, size: f64, col: Rgba) {
    draw::set(cr, col);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    let s = size / 18.0;
    cr.new_sub_path();
    cr.arc(x + 4.5 * s, y + 13.5 * s, 2.5 * s, 0.0, 2.0 * PI);
    let _ = cr.fill();

    cr.new_sub_path();
    cr.arc(x + 13.5 * s, y + 11.0 * s, 2.5 * s, 0.0, 2.0 * PI);
    let _ = cr.fill();

    cr.move_to(x + 7.0 * s, y + 13.5 * s);
    cr.line_to(x + 7.0 * s, y + 4.0 * s);
    cr.move_to(x + 16.0 * s, y + 11.0 * s);
    cr.line_to(x + 16.0 * s, y + 1.5 * s);
    cr.line_to(x + 7.0 * s, y + 4.0 * s);
    let _ = cr.stroke();
}

fn draw_play_icon(cr: &Context, x: f64, y: f64, col: Rgba) {
    draw::set(cr, col);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(x + 5.0, y + 3.0);
    cr.line_to(x + 14.5, y + 9.0);
    cr.line_to(x + 5.0, y + 15.0);
    cr.close_path();
    let _ = cr.fill();
}

fn draw_pause_icon(cr: &Context, x: f64, y: f64, col: Rgba) {
    draw::set(cr, col);
    draw::round_rect(cr, x + 4.0, y + 3.5, 3.2, 11.0, 1.2);
    let _ = cr.fill();
    draw::round_rect(cr, x + 10.8, y + 3.5, 3.2, 11.0, 1.2);
    let _ = cr.fill();
}

fn draw_next_icon(cr: &Context, x: f64, y: f64, col: Rgba) {
    draw::set(cr, col);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    cr.move_to(x + 4.0, y + 4.0);
    cr.line_to(x + 11.5, y + 9.0);
    cr.line_to(x + 4.0, y + 14.0);
    cr.close_path();
    let _ = cr.fill();

    cr.move_to(x + 13.5, y + 4.0);
    cr.line_to(x + 13.5, y + 14.0);
    let _ = cr.stroke();
}

fn draw_prev_icon(cr: &Context, x: f64, y: f64, col: Rgba) {
    draw::set(cr, col);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    cr.move_to(x + 4.5, y + 4.0);
    cr.line_to(x + 4.5, y + 14.0);
    let _ = cr.stroke();

    cr.move_to(x + 14.0, y + 4.0);
    cr.line_to(x + 6.5, y + 9.0);
    cr.line_to(x + 14.0, y + 14.0);
    cr.close_path();
    let _ = cr.fill();
}

fn draw_shuffle_icon(cr: &Context, x: f64, y: f64, col: Rgba) {
    draw::set(cr, col);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    cr.move_to(x + 3.0, y + 5.0);
    cr.line_to(x + 8.0, y + 5.0);
    cr.line_to(x + 13.0, y + 13.0);
    cr.line_to(x + 15.0, y + 13.0);

    cr.move_to(x + 3.0, y + 13.0);
    cr.line_to(x + 8.0, y + 13.0);
    cr.line_to(x + 13.0, y + 5.0);
    cr.line_to(x + 15.0, y + 5.0);
    let _ = cr.stroke();

    cr.move_to(x + 12.5, y + 3.0);
    cr.line_to(x + 15.5, y + 5.0);
    cr.line_to(x + 12.5, y + 7.0);

    cr.move_to(x + 12.5, y + 11.0);
    cr.line_to(x + 15.5, y + 13.0);
    cr.line_to(x + 12.5, y + 15.0);
    let _ = cr.stroke();
}

fn draw_repeat_icon(cr: &Context, x: f64, y: f64, col: Rgba) {
    draw::set(cr, col);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    cr.move_to(x + 6.0, y + 5.0);
    cr.line_to(x + 13.0, y + 5.0);
    cr.arc(x + 13.0, y + 8.0, 3.0, -FRAC_PI_2, 0.0);
    cr.line_to(x + 16.0, y + 10.0);
    cr.arc(x + 13.0, y + 10.0, 3.0, 0.0, FRAC_PI_2);
    cr.line_to(x + 5.0, y + 13.0);
    cr.arc(x + 5.0, y + 10.0, 3.0, FRAC_PI_2, PI);
    cr.line_to(x + 2.0, y + 8.0);
    cr.arc(x + 5.0, y + 8.0, 3.0, PI, 3.0 * FRAC_PI_2);
    let _ = cr.stroke();

    cr.move_to(x + 14.0, y + 3.0);
    cr.line_to(x + 16.5, y + 5.0);
    cr.line_to(x + 14.0, y + 7.0);
    let _ = cr.stroke();
}

// ---------------------------------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::theme::Theme;

    fn test_cx<'a>(th: &'a Theme, size: Size, app: Option<&'a App>) -> Cx<'a> {
        Cx {
            theme: th,
            now: chrono::NaiveDate::from_ymd_opt(2026, 11, 13)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            size,
            app,
            apps: &[],
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
        }
    }

    fn melodyae_app() -> App {
        App {
            id: "melodyae".into(),
            name: "Melodyae".into(),
            icon: "melodyae".into(),
            exec: "melodyae".into(),
            categories: vec!["AudioVideo".into(), "Audio".into(), "Player".into()],
            wm_class: None,
            terminal: false,
        }
    }

    #[test]
    fn provider_claims_audio_players_only() {
        let m = melodyae_app();
        let mpv = App {
            id: "mpv".into(),
            name: "mpv".into(),
            categories: vec![
                "AudioVideo".into(),
                "Audio".into(),
                "Video".into(),
                "Player".into(),
            ],
            ..Default::default()
        };
        let firefox = App {
            id: "firefox".into(),
            name: "Firefox".into(),
            categories: vec!["Network".into(), "WebBrowser".into()],
            ..Default::default()
        };
        assert_eq!(crate::provider::owner_of(&m).map(|p| p.id()), Some("music"));
        assert_eq!(
            crate::provider::owner_of(&mpv).map(|p| p.id()),
            Some("music")
        );
        assert_eq!(crate::provider::owner_of(&firefox).unwrap().id(), "simple");
        let (p_untied, a_untied) = crate::provider::resolve("music", &[]).unwrap();
        assert_eq!(p_untied.id(), "music");
        assert!(a_untied.is_none());
        let apps = vec![m.clone()];
        let (p_tied, a_tied) = crate::provider::resolve("melodyae", &apps).unwrap();
        assert_eq!(p_tied.id(), "music");
        assert_eq!(a_tied.map(|a| a.id.as_str()), Some("melodyae"));
        assert_eq!(Music.listed(), Listed::Never);
        let k = Music.kind("playing").unwrap();
        assert_eq!(k.sizes, &[Size::S, Size::M, Size::L]);
        assert_eq!(k.default, Size::M);
        assert!(k.half);
    }

    #[test]
    fn parses_key_value_media_data() {
        let input = "status=Playing\n\
                     title=Starless\n\
                     artist=King Crimson\n\
                     album=Red\n\
                     player=Melodyae\n\
                     desktop_entry=melodyae\n\
                     bus=org.mpris.MediaPlayer2.melodyae\n\
                     position=234000000\n\
                     duration=742000000\n\
                     shuffle=true\n\
                     loop=Playlist\n\
                     lyrics_current=Starless and bible black\n\
                     lyrics_next=Ice and frozen darkness\n\
                     lyrics_before=Sundown dazzling day\n\
                     queue=Starless\tKing Crimson\tfile:///covers/red.jpg\t742000000\n\
                     queue=Fallen Angel\tKing Crimson\tfile:///covers/red.jpg\t363000000\n";

        let d = parse_music(input);
        assert_eq!(d.players.len(), 1);
        let data = &d.players[0];
        assert!(data.playing);
        assert_eq!(data.title, "Starless");
        assert_eq!(data.artist, "King Crimson");
        assert_eq!(data.album, "Red");
        assert_eq!(data.player_name, "Melodyae");
        assert_eq!(data.desktop_entry, "melodyae");
        assert_eq!(data.position_us, 234_000_000);
        assert_eq!(data.duration_us, 742_000_000);
        assert!(data.shuffle);
        assert_eq!(data.loop_status, "Playlist");
        assert_eq!(data.lyrics.current, "Starless and bible black");
        assert_eq!(data.lyrics.next, "Ice and frozen darkness");
        assert_eq!(data.lyrics.before, "Sundown dazzling day");
        assert_eq!(data.queue.len(), 2);
        assert_eq!(data.queue[1].title, "Fallen Angel");
        assert_eq!(data.queue[1].length_us, 363_000_000);
    }

    #[test]
    fn parses_json_empty_and_broken_inputs() {
        let json = r#"{
            "status": "Playing",
            "title": "Echoes",
            "artist": "Pink Floyd",
            "album": "Meddle",
            "player": "Spotify",
            "position": 300000000,
            "duration": 1415000000,
            "shuffle": false,
            "lyrics": "Overhead the albatross hangs motionless upon the air"
        }"#;
        let d = parse_music(json);
        assert_eq!(d.players.len(), 1);
        let p = &d.players[0];
        assert!(p.playing);
        assert_eq!(p.title, "Echoes");
        assert_eq!(p.artist, "Pink Floyd");
        assert_eq!(
            p.lyrics.current,
            "Overhead the albatross hangs motionless upon the air"
        );

        assert!(parse_music("").players.is_empty());
        assert!(parse_music("   \n# comment\n---\n").players.is_empty());
        assert!(parse_music("not valid key value or tsv").players.is_empty());
        let broken = parse_music("title=Song\nposition=notnum\nduration=-50\nqueue=\t\n");
        assert_eq!(broken.players.len(), 1);
        assert_eq!(broken.players[0].position_us, 0);
        assert_eq!(broken.players[0].duration_us, 0);
        assert!(broken.players[0].queue.is_empty());
    }

    #[test]
    fn format_time_handles_zero_mins_and_hours() {
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(-5_000_000), "0:00");
        assert_eq!(format_time(65_000_000), "1:05");
        assert_eq!(format_time(234_000_000), "3:54");
        assert_eq!(format_time(3_665_000_000), "1:01:05");
    }

    #[test]
    fn decode_file_url_keeps_letters_outside_ascii() {
        assert_eq!(
            decode_file_url("file:///music/Caf%C3%A9/%D0%A0%D0%B0%D0%B9.jpg").as_deref(),
            Some("/music/Café/Рай.jpg")
        );
        assert_eq!(
            decode_file_url("file:///home/user/Music/My%20Song.png"),
            Some("/home/user/Music/My Song.png".to_string())
        );
        assert_eq!(
            decode_file_url("file:///tmp/covers/track%231.png"),
            Some("/tmp/covers/track#1.png".to_string())
        );
        assert_eq!(decode_file_url("https://example.com/art.png"), None);
        assert_eq!(decode_file_url("file:///bad%ZZ"), None);
    }

    #[test]
    fn statuses_and_player_selection_for_untied_and_tied_cards() {
        let th = Theme::from_config(&Config::from_text(""));
        let app = melodyae_app();
        let cx_untied = test_cx(&th, Size::M, None);
        let cx_tied = test_cx(&th, Size::M, Some(&app));

        let mut card = MusicCard::default();
        assert_eq!(card.status(&cx_untied), Status::Loading);
        assert_eq!(card.status(&cx_tied), Status::Loading);

        // No players: untied hides, tied shows Open button.
        card.apply("music", &MusicData::default() as &dyn Any);
        assert_eq!(card.status(&cx_untied), Status::Hidden);
        assert_eq!(card.status(&cx_tied), Status::Ready);
        assert_eq!(card.height(&cx_tied, 360.0), ui::ROW_H);

        // Two players: Firefox playing, Melodyae paused.
        let two = parse_music(
            "status=Paused\ntitle=Silent Track\nplayer=Melodyae\ndesktop_entry=Melodyae\n\
             bus=org.mpris.MediaPlayer2.melodyae\n---\n\
             status=Playing\ntitle=Video\nplayer=Firefox\ndesktop_entry=firefox\n\
             bus=org.mpris.MediaPlayer2.firefox\n",
        );
        assert!(card.apply("music", &two as &dyn Any));
        assert_eq!(card.status(&cx_untied), Status::Ready);
        assert_eq!(
            card.player(&cx_untied).unwrap().title,
            "Video",
            "music/playing picks the playing player"
        );
        assert_eq!(
            card.player(&cx_tied).unwrap().title,
            "Silent Track",
            "melodyae/playing picks by DesktopEntry without case"
        );

        // Firefox pauses: music/playing keeps the last player that played.
        let both_paused = parse_music(
            "status=Paused\ntitle=Silent Track\nplayer=Melodyae\ndesktop_entry=Melodyae\n\
             bus=org.mpris.MediaPlayer2.melodyae\n---\n\
             status=Paused\ntitle=Video\nplayer=Firefox\ndesktop_entry=firefox\n\
             bus=org.mpris.MediaPlayer2.firefox\n",
        );
        card.apply("music", &both_paused as &dyn Any);
        assert_eq!(card.player(&cx_untied).unwrap().title, "Video");
    }

    #[test]
    fn open_button_launches_tied_app_when_not_on_bus() {
        let th = Theme::from_config(&Config::from_text(""));
        let app = melodyae_app();
        let cx = test_cx(&th, Size::M, Some(&app));
        let mut card = MusicCard::default();
        card.apply("music", &MusicData::default() as &dyn Any);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 120).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::default();
        let h = card.height(&cx, 360.0);
        card.draw(&cr, &cx, (10.0, 40.0, 360.0, h), &mut hits);
        assert!(hits.list.iter().any(|(id, _)| *id == H_OPEN));

        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_OPEN,
                button: Button::Left,
                at: (300.0, 55.0)
            },
            &cx,
            &mut out
        ));
        assert_eq!(out, vec![Action::Launch("melodyae".into())]);
    }

    #[test]
    fn small_medium_large_controls_seek_and_queue_emit_actions() {
        let th = Theme::from_config(&Config::from_text(""));
        let mut card = MusicCard::default();
        let d = parse_music(
            "status=Paused\ntitle=Starless\nartist=King Crimson\nalbum=Red\n\
             player=Melodyae\nbus=org.mpris.MediaPlayer2.melodyae\n\
             track_id=/org/mpris/MediaPlayer2/Track/7\n\
             position=100000000\nduration=500000000\nshuffle=false\nloop=None\n\
             lyrics_before=Line 1\nlyrics_current=Line 2\nlyrics_next=Line 3\n\
             queue=Starless\tKing Crimson\t\t742000000\n\
             queue=Fallen Angel\tKing Crimson\t\t363000000\n",
        );
        card.apply("music", &d as &dyn Any);

        // Size::S: play/pause and next.
        let cx_s = test_cx(&th, Size::S, None);
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_PLAY,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_s,
            &mut out
        ));
        assert!(card.player(&cx_s).unwrap().playing);
        assert!(out[0] == Action::Run(cmd_player("org.mpris.MediaPlayer2.melodyae", "PlayPause")));

        out.clear();
        assert!(card.event(
            &Event::Press {
                hit: H_NEXT,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_s,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Run(cmd_player(
                "org.mpris.MediaPlayer2.melodyae",
                "Next"
            ))]
        );

        // Size::M: draw to populate seek_rect, then test shuffle, prev, repeat, seek click & drag.
        let cx_m = test_cx(&th, Size::M, None);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 600).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::default();
        let hm = card.height(&cx_m, 360.0);
        card.draw(&cr, &cx_m, (10.0, 40.0, 360.0, hm), &mut hits);
        let seek_r = hits.list.iter().find(|(id, _)| *id == H_SEEK).unwrap().1;

        out.clear();
        card.event(
            &Event::Press {
                hit: H_SHUFFLE,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_m,
            &mut out,
        );
        assert!(card.player(&cx_m).unwrap().shuffle);
        assert_eq!(
            out,
            vec![Action::Run(cmd_shuffle(
                "org.mpris.MediaPlayer2.melodyae",
                true
            ))]
        );

        out.clear();
        card.event(
            &Event::Press {
                hit: H_PREV,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_m,
            &mut out,
        );
        assert_eq!(
            out,
            vec![Action::Run(cmd_player(
                "org.mpris.MediaPlayer2.melodyae",
                "Previous"
            ))]
        );

        out.clear();
        card.event(
            &Event::Press {
                hit: H_REPEAT,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_m,
            &mut out,
        );
        assert_eq!(card.player(&cx_m).unwrap().loop_status, "Playlist");
        assert_eq!(
            out,
            vec![Action::Run(cmd_loop(
                "org.mpris.MediaPlayer2.melodyae",
                "Playlist"
            ))]
        );

        // Dragging keeps position across background apply until release.
        out.clear();
        assert!(card.event(
            &Event::Drag {
                hit: H_SEEK,
                at: (seek_r.0 + seek_r.2 * 0.5, seek_r.1 + 5.0),
            },
            &cx_m,
            &mut out,
        ));
        assert_eq!(card.player(&cx_m).unwrap().position_us, 250_000_000);
        assert!(out.is_empty(), "drag previews without spawning commands");

        card.apply("music", &d as &dyn Any);
        assert_eq!(
            card.player(&cx_m).unwrap().position_us,
            250_000_000,
            "background refresh does not snap slider back mid-drag"
        );

        assert!(card.event(&Event::Release { hit: H_SEEK }, &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(cmd_seek(
                "org.mpris.MediaPlayer2.melodyae",
                "/org/mpris/MediaPlayer2/Track/7",
                250_000_000
            ))]
        );

        // Size::L: queue row click and TrackList GoTo variant.
        let cx_l = test_cx(&th, Size::L, None);
        out.clear();
        card.event(
            &Event::Press {
                hit: H_QUEUE + 1,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx_l,
            &mut out,
        );
        assert_eq!(
            out,
            vec![Action::Run(cmd_play_queue(
                "org.mpris.MediaPlayer2.melodyae",
                1,
                None
            ))]
        );
        assert!(cmd_play_queue(
            "org.mpris.MediaPlayer2.vlc",
            0,
            Some("/org/mpris/MediaPlayer2/Track/9")
        )
        .contains("TrackList.GoTo objpath:'/org/mpris/MediaPlayer2/Track/9'"));
    }

    #[test]
    fn heights_match_drawn_bounds_across_sizes_and_widths() {
        let th = Theme::from_config(&Config::from_text(""));
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 500, 900).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();

        let mut full_q = String::from(
            "status=Playing\ntitle=Starless\nartist=King Crimson\nalbum=Red\n\
             player=Melodyae\nbus=org.mpris.MediaPlayer2.melodyae\n\
             position=100000000\nduration=500000000\n\
             lyrics_before=B\nlyrics_current=C\nlyrics_next=N\n",
        );
        for i in 0..8 {
            full_q.push_str(&format!("queue=Song {i}\tArtist\t\t180000000\n"));
        }
        let minimal = parse_music("status=Playing\ntitle=Track\nbus=org.mpris.MediaPlayer2.mpv\n");
        let rich = parse_music(&full_q);

        for (label, data) in [("minimal", minimal), ("rich", rich)] {
            let mut card = MusicCard::default();
            card.apply("music", &data as &dyn Any);
            for size in [Size::S, Size::M, Size::L] {
                for width in [360.0, 164.0] {
                    let cx = test_cx(&th, size, None);
                    let h = card.height(&cx, width);
                    let body = (10.0, 40.0, width, h);
                    let mut hits = Hits::default();
                    card.draw(&cr, &cx, body, &mut hits);
                    assert!(h > 0.0, "{label} {size:?} w={width}");
                    for (id, r) in &hits.list {
                        assert!(
                            r.0 >= body.0 - 0.1
                                && r.1 >= body.1 - 0.1
                                && r.0 + r.2 <= body.0 + body.2 + 0.1
                                && r.1 + r.3 <= body.1 + body.3 + 0.1,
                            "{label} {size:?} w={width} hit {id} ({r:?}) outside body ({body:?})"
                        );
                    }
                }
            }
        }

        // Tied-app card when no player is on the bus renders an Open button whose hit stays in bounds.
        let mut tied_empty = MusicCard::default();
        tied_empty.apply("music", &MusicData::default() as &dyn Any);
        let app = melodyae_app();
        for width in [360.0, 164.0] {
            let cx = test_cx(&th, Size::M, Some(&app));
            assert_eq!(tied_empty.status(&cx), Status::Ready);
            let h = tied_empty.height(&cx, width);
            let body = (10.0, 40.0, width, h);
            let mut hits = Hits::default();
            tied_empty.draw(&cr, &cx, body, &mut hits);
            assert!(h > 0.0, "tied empty w={width}");
            assert!(!hits.list.is_empty(), "expected Open hit for tied empty");
            for (id, r) in &hits.list {
                assert!(
                    r.0 >= body.0 - 0.1
                        && r.1 >= body.1 - 0.1
                        && r.0 + r.2 <= body.0 + body.2 + 0.1
                        && r.1 + r.3 <= body.1 + body.3 + 0.1,
                    "tied empty w={width} hit {id} ({r:?}) outside body ({body:?})"
                );
            }
        }

        // Scroll in Large changes visible rows when > MAX_QUEUE_VISIBLE without changing height.
        let mut card = MusicCard::default();
        card.apply("music", &parse_music(&full_q) as &dyn Any);
        let cx_l = test_cx(&th, Size::L, None);
        let h_before = card.height(&cx_l, 360.0);
        let mut out = Vec::new();
        assert!(card.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out));
        assert_eq!(card.queue_scroll, 1);
        assert_eq!(card.height(&cx_l, 360.0), h_before);
        assert!(card.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out));
        assert_eq!(card.queue_scroll, 2);
        assert!(!card.event(&Event::Scroll { hit: None, dy: 1.0 }, &cx_l, &mut out));
        assert!(card.event(
            &Event::Scroll {
                hit: None,
                dy: -1.0
            },
            &cx_l,
            &mut out
        ));
        assert_eq!(card.queue_scroll, 1);
    }

    #[test]
    fn loads_png_cover_from_disk_and_caches_it() {
        let dir =
            std::env::temp_dir().join(format!("edge-apps-music-cover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cover.png");
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 64, 48).unwrap();
        {
            let cr = cairo::Context::new(&surf).unwrap();
            cr.set_source_rgb(0.8, 0.4, 0.6);
            let _ = cr.paint();
        }
        surf.flush();
        let mut f = File::create(&path).unwrap();
        surf.write_to_png(&mut f).unwrap();

        let loaded = cover_surface(&path, 60).expect("loaded png cover");
        assert_eq!((loaded.width(), loaded.height()), (60, 60));
        let cached = cover_surface(&path, 60).expect("cached png cover");
        assert_eq!((cached.width(), cached.height()), (60, 60));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
