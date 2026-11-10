//! The `media` module: MPRIS media player with three sizes (small, medium, large) by card height.
//!
//! # Sizes
//!
//! - **small** (`height < 120 px`):
//!   52 px cover, title and artist, play/pause (accent fill, corners from theme) and next.
//! - **medium** (`120 px <= height < 460 px`, default unplaced ~234 px):
//!   64 px cover, title (15 px semibold), "artist · player" muted, current and next lyric lines
//!   when available, 4 px progress bar with times, controls (shuffle with accent when on, previous,
//!   play/pause, next, repeat), and under a thin line "Next: song · artist" and "Queue N" muted.
//! - **large** (`height >= 460 px`):
//!   96 px cover, title, artist · album, format line if known, three lyric lines (before, current, next),
//!   progress bar with times, controls, "Up next" header with "N songs · M min", and scrolling queue
//!   list with song cover, title, artist, length; click plays song.
//!
//! # Cover art
//!
//! Covers from `mpris:artUrl` (`file://` or cached `http(s)`), else a soft accent square with a music note.
//!
//! # Melodyae Extras D-Bus Interface (`app.melodyae.Extras`)
//!
//! Melodyae (`~/melodyae`, Rust) exports queue and lyrics through this interface:
//!
//! ```text
//! Interface: app.melodyae.Extras
//! Object Path: /org/mpris/MediaPlayer2
//!
//! Methods:
//!   Queue() -> a(sssx)
//!     Returns an array of structs: (title: String, artist: String, art_url: String, length_us: i64).
//!     length_us is the duration in microseconds (matching mpris:length).
//!
//!   CurrentLyrics() -> (s, s, s)
//!     Returns (current: String, next: String, before: String).
//! ```
//!
//! When `app.melodyae.Extras` is unavailable, fallback to standard `org.mpris.MediaPlayer2.TrackList`
//! is used for queue items.

use crate::config::Item;
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{apps, is_left_button, Rect, World};
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::f64::consts::PI;
use std::path::Path;

pub const CMDS: &[(&str, &str)] = &[("media", "__zbus_media__")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaSize {
    Small,
    Medium,
    Large,
}

pub fn size_for_height(h: f64) -> MediaSize {
    if h >= 460.0 {
        MediaSize::Large
    } else if h < 120.0 {
        MediaSize::Small
    } else {
        MediaSize::Medium
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Lyrics {
    pub current: String,
    pub next: String,
    pub before: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct QueueSong {
    pub title: String,
    pub artist: String,
    pub art_url: String,
    pub length_us: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct MediaData {
    pub ready: bool,
    pub bus_name: String,
    pub player_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub format: String,
    pub playing: bool,
    pub position_us: i64,
    pub duration_us: i64,
    pub shuffle: bool,
    pub loop_status: String, // "None", "Track", "Playlist"
    pub lyrics: Lyrics,
    pub queue: Vec<QueueSong>,
    pub queue_scroll: usize,
}

impl MediaData {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.bus_name.is_empty() && self.player_name.is_empty()
    }

    pub fn display_player(&self) -> &str {
        if !self.player_name.is_empty() {
            &self.player_name
        } else if let Some(suffix) = self.bus_name.strip_prefix("org.mpris.MediaPlayer2.") {
            suffix
        } else {
            "Player"
        }
    }
}

/// Applies raw output (from `run_sh` or `INFYNITAE_EDGE_RENDER_DATA`) to `MediaData`.
pub fn apply(d: &mut MediaData, key: &str, out: &str) -> bool {
    if key != "media" {
        return false;
    }
    // the queue's scroll is the user's, not the player's: kept across the refresh (it snapped
    // back every 2 s), within the new queue
    let scroll = d.queue_scroll;
    let mut next = parse_media_data(out).unwrap_or_default();
    next.ready = true;
    next.queue_scroll = scroll.min(next.queue.len().saturating_sub(1));
    let changed = next != *d;
    *d = next;
    changed
}

/// Formats duration in microseconds to mm:ss or hh:mm:ss.
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

/// Decodes percent-encoded URL (e.g. `file:///home/user/My%20Song.mp3` -> `/home/user/My Song.mp3`).
pub fn decode_file_url(url: &str) -> Option<String> {
    // bytes first, then UTF-8: %C3%A9 is one letter (é), not two (each byte as a char made any
    // path with letters outside ASCII wrong, and its cover missing)
    let path_str = url.strip_prefix("file://")?;
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

/// Parses media state from key=value lines, TSV, or JSON.
pub fn parse_media_data(out: &str) -> Option<MediaData> {
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.starts_with('{') {
        return parse_json_media(trimmed);
    }

    let mut data = MediaData::default();
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
                    data.playing = val.eq_ignore_ascii_case("playing");
                }
                "title" => data.title = val.to_string(),
                "artist" => data.artist = val.to_string(),
                "album" => data.album = val.to_string(),
                "player" => data.player_name = val.to_string(),
                "bus" => data.bus_name = val.to_string(),
                "art_url" | "arturl" | "art" => data.art_url = val.to_string(),
                "format" => data.format = val.to_string(),
                "position" | "position_us" => {
                    data.position_us = val.parse::<i64>().unwrap_or(0);
                }
                "duration" | "duration_us" | "length" | "length_us" => {
                    data.duration_us = val.parse::<i64>().unwrap_or(0);
                }
                "shuffle" => {
                    data.shuffle = val == "1"
                        || val.eq_ignore_ascii_case("true")
                        || val.eq_ignore_ascii_case("on");
                }
                "loop" | "loop_status" => data.loop_status = val.to_string(),
                "lyrics_current" | "lyrics" => data.lyrics.current = val.to_string(),
                "lyrics_next" => data.lyrics.next = val.to_string(),
                "lyrics_before" => data.lyrics.before = val.to_string(),
                "queue" => {
                    let parts: Vec<&str> = val.split('\t').collect();
                    if !parts.is_empty() {
                        let title = parts[0].trim().to_string();
                        let artist = if parts.len() > 1 {
                            parts[1].trim().to_string()
                        } else {
                            String::new()
                        };
                        let art_url = if parts.len() > 2 {
                            parts[2].trim().to_string()
                        } else {
                            String::new()
                        };
                        let len_us = if parts.len() > 3 {
                            parts[3].trim().parse::<i64>().unwrap_or(0)
                        } else {
                            0
                        };
                        data.queue.push(QueueSong {
                            title,
                            artist,
                            art_url,
                            length_us: len_us,
                        });
                    }
                }
                _ => {}
            }
        } else if line.contains('\t') {
            // TSV fallback: Status \t Title \t Artist \t Album \t Player \t ArtUrl \t Pos \t Dur \t Shuffle \t Loop
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                has_fields = true;
                data.playing = parts[0].trim().eq_ignore_ascii_case("playing");
                data.title = parts[1].trim().to_string();
                if parts.len() > 2 {
                    data.artist = parts[2].trim().to_string();
                }
                if parts.len() > 3 {
                    data.album = parts[3].trim().to_string();
                }
                if parts.len() > 4 {
                    data.player_name = parts[4].trim().to_string();
                }
                if parts.len() > 5 {
                    data.art_url = parts[5].trim().to_string();
                }
                if parts.len() > 6 {
                    data.position_us = parts[6].trim().parse().unwrap_or(0);
                }
                if parts.len() > 7 {
                    data.duration_us = parts[7].trim().parse().unwrap_or(0);
                }
                if parts.len() > 8 {
                    let s = parts[8].trim();
                    data.shuffle = s == "1" || s.eq_ignore_ascii_case("true");
                }
                if parts.len() > 9 {
                    data.loop_status = parts[9].trim().to_string();
                }
            }
        }
    }

    if has_fields && !data.is_empty() {
        data.ready = true;
        Some(data)
    } else {
        None
    }
}

fn parse_json_media(text: &str) -> Option<MediaData> {
    let title = extract_json_str(text, "title")?;
    let artist = extract_json_str(text, "artist").unwrap_or_default();
    let album = extract_json_str(text, "album").unwrap_or_default();
    let player_name = extract_json_str(text, "player").unwrap_or_default();
    let art_url = extract_json_str(text, "art_url").unwrap_or_default();
    let format = extract_json_str(text, "format").unwrap_or_default();
    let loop_status = extract_json_str(text, "loop").unwrap_or_else(|| "None".into());

    let status = extract_json_str(text, "status").unwrap_or_default();
    let playing = status.eq_ignore_ascii_case("playing")
        || text.contains("\"playing\":true")
        || text.contains("\"playing\": true");
    let shuffle = text.contains("\"shuffle\":true") || text.contains("\"shuffle\": true");

    let position_us = extract_json_num(text, "position").unwrap_or(0);
    let duration_us = extract_json_num(text, "duration").unwrap_or(0);

    let current = extract_json_str(text, "current_lyrics")
        .or_else(|| extract_json_str(text, "lyrics"))
        .unwrap_or_default();
    let next = extract_json_str(text, "next_lyrics").unwrap_or_default();
    let before = extract_json_str(text, "before_lyrics").unwrap_or_default();

    Some(MediaData {
        ready: true,
        bus_name: String::new(),
        player_name,
        title,
        artist,
        album,
        art_url,
        format,
        playing,
        position_us,
        duration_us,
        shuffle,
        loop_status,
        lyrics: Lyrics {
            current,
            next,
            before,
        },
        queue: Vec::new(),
        queue_scroll: 0,
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

/// Returns the height for the media module.
/// Hidden (0.0) when there is no player or track.
/// The line and the "Next: ... / Queue N" row under the medium look's controls.
const QUEUE_ROW_H: f64 = 33.0;

pub fn height(it: &Item, world: &World) -> f64 {
    if !world.media.ready || world.media.is_empty() {
        return 0.0;
    }
    if let Some((_, _, _, ph)) = it.rect {
        if ph > 0 {
            return ph as f64;
        }
    }
    // the medium look: 234 px with two lines of lyrics, less what has no lyrics to show
    let ly = &world.media.lyrics;
    234.0
        - if world.media.queue.is_empty() {
            QUEUE_ROW_H
        } else {
            0.0
        }
        - if ly.current.is_empty() {
            40.0
        } else if ly.next.is_empty() {
            16.0
        } else {
            0.0
        }
}

/// The width text starting at `text_x` (on the screen) may take in a card at `x` of width `w`,
/// keeping the right padding and `reserved` px for what is drawn right of it (at least 20 px).
fn room(x: f64, w: f64, text_x: f64, reserved: f64) -> f64 {
    (x + w - PAD_X - reserved - text_x).max(20.0)
}

/// Draws the `media` module in small, medium, or large size according to `h`.
pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if world.media.is_empty() {
        return;
    }

    draw::card(cr, th, x, y, w, h);

    match size_for_height(h) {
        MediaSize::Small => draw_small(cr, th, world, rect),
        MediaSize::Medium => draw_medium(cr, th, world, rect),
        MediaSize::Large => draw_large(cr, th, world, rect),
    }
}

// =============================================================================
// Small Size: 52 px cover, title, artist, play/pause (accent fill), next
// =============================================================================

fn draw_small(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    let cover_size = 52.0;
    let cy = y + ((h - cover_size) / 2.0).max(4.0);
    let cx = x + PAD_X;

    draw_cover(cr, th, cx, cy, cover_size, &world.media.art_url);

    // Text between cover and buttons
    let btn_total_w = 36.0 + 8.0 + 32.0;
    let text_x = cx + cover_size + 12.0;
    let max_text_w = room(x, w, text_x, btn_total_w + 12.0);

    let ty = y + (h / 2.0) - 16.0;
    draw::text_max(
        cr,
        text_x,
        ty,
        max_text_w,
        &Text {
            size: 13.5,
            bold: true,
            color: th.text,
            text: &world.media.title,
        },
    );

    let artist_str = if !world.media.artist.is_empty() {
        &world.media.artist
    } else {
        world.media.display_player()
    };
    draw::text_max(
        cr,
        text_x,
        ty + 18.0,
        max_text_w,
        &Text {
            size: 11.5,
            bold: false,
            color: th.muted,
            text: artist_str,
        },
    );

    // Buttons on right: play/pause (accent fill) + next
    let btn_y = y + ((h - 32.0) / 2.0).max(2.0);
    let play_x = x + w - PAD_X - btn_total_w;
    draw::round_rect(cr, play_x, btn_y, 36.0, 32.0, th.small_radius());
    draw::set(cr, th.accent);
    let _ = cr.fill();

    // Play/Pause icon in white / contrasting color on accent fill
    let icon_col = Rgba(1.0, 1.0, 1.0, 0.98);
    if world.media.playing {
        draw_pause_icon(
            cr,
            play_x + (36.0 - 18.0) / 2.0,
            btn_y + (32.0 - 18.0) / 2.0,
            icon_col,
        );
    } else {
        draw_play_icon(
            cr,
            play_x + (36.0 - 18.0) / 2.0 + 1.0,
            btn_y + (32.0 - 18.0) / 2.0,
            icon_col,
        );
    }

    let next_x = play_x + 36.0 + 8.0;
    draw_next_icon(
        cr,
        next_x + (32.0 - 18.0) / 2.0,
        btn_y + (32.0 - 18.0) / 2.0,
        th.text,
    );
}

// =============================================================================
// Medium Size: 64 px cover, title (15 px semibold), artist · player, lyrics,
// progress bar, shuffle, prev, play/pause, next, repeat, next line & queue
// =============================================================================

fn draw_medium(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, _) = rect;
    let cover_size = 64.0;
    let cx = x + PAD_X;
    let cy = y + PAD_Y;

    draw_cover(cr, th, cx, cy, cover_size, &world.media.art_url);

    // Title (15 px semibold) & "artist · player" muted
    let text_x = cx + cover_size + 14.0;
    let max_text_w = room(x, w, text_x, 0.0);

    draw::text_max(
        cr,
        text_x,
        cy + 12.0,
        max_text_w,
        &Text {
            size: 15.0,
            bold: true,
            color: th.text,
            text: &world.media.title,
        },
    );

    let sub_text = format!("{} · {}", world.media.artist, world.media.display_player());
    draw::text_max(
        cr,
        text_x,
        cy + 34.0,
        max_text_w,
        &Text {
            size: 12.0,
            bold: false,
            color: th.muted,
            text: &sub_text,
        },
    );

    let mut cur_y = cy + cover_size + 10.0;

    // Lyrics lines (current and next)
    if !world.media.lyrics.current.is_empty() {
        draw::text_max(
            cr,
            x + PAD_X,
            cur_y,
            w - 2.0 * PAD_X,
            &Text {
                size: 13.0,
                bold: true,
                color: th.text,
                text: &world.media.lyrics.current,
            },
        );
        cur_y += 18.0;
        if !world.media.lyrics.next.is_empty() {
            draw::text_max(
                cr,
                x + PAD_X,
                cur_y,
                w - 2.0 * PAD_X,
                &Text {
                    size: 11.5,
                    bold: false,
                    color: th.muted,
                    text: &world.media.lyrics.next,
                },
            );
            cur_y += 16.0;
        }
        cur_y += 6.0;
    }

    // Progress bar (4 px) with elapsed and duration times
    let time_left = format_time(world.media.position_us);
    let time_right = format_time(world.media.duration_us);
    let time_t_left = Text {
        size: 11.0,
        bold: false,
        color: th.muted,
        text: &time_left,
    };
    let time_t_right = Text {
        size: 11.0,
        bold: false,
        color: th.muted,
        text: &time_right,
    };
    let (tw_l, _) = draw::measure(cr, &time_t_left);
    let (tw_r, _) = draw::measure(cr, &time_t_right);

    draw::text(cr, x + PAD_X, cur_y + 1.0, &time_t_left);
    draw::text_right(cr, x + w - PAD_X, cur_y + 1.0, &time_t_right);

    let bar_x = x + PAD_X + tw_l + 8.0;
    let bar_w = (w - 2.0 * PAD_X - (tw_l + tw_r + 16.0)).max(20.0);
    let frac = if world.media.duration_us > 0 {
        (world.media.position_us as f64 / world.media.duration_us as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    draw::round_rect(cr, bar_x, cur_y + 5.0, bar_w, 4.0, 2.0);
    draw::set(cr, th.track);
    let _ = cr.fill();
    if frac > 0.0 {
        draw::round_rect(cr, bar_x, cur_y + 5.0, (bar_w * frac).max(4.0), 4.0, 2.0);
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }
    cur_y += 22.0;

    // Controls row: shuffle, prev, play/pause, next, repeat
    let ctrl_w = 36.0;
    let ctrl_h = 32.0;
    let total_ctrls_w = 5.0 * ctrl_w + 4.0 * 12.0;
    let start_ctrl_x = x + (w - total_ctrls_w) / 2.0;

    // Shuffle
    let shuf_col = if world.media.shuffle {
        th.accent
    } else {
        th.muted
    };
    draw_shuffle_icon(
        cr,
        start_ctrl_x + (ctrl_w - 18.0) / 2.0,
        cur_y + (ctrl_h - 18.0) / 2.0,
        shuf_col,
    );

    // Prev
    let prev_x = start_ctrl_x + ctrl_w + 12.0;
    draw_prev_icon(
        cr,
        prev_x + (ctrl_w - 18.0) / 2.0,
        cur_y + (ctrl_h - 18.0) / 2.0,
        th.text,
    );

    // Play/Pause (accent fill)
    let play_x = prev_x + ctrl_w + 12.0;
    draw::round_rect(cr, play_x, cur_y, ctrl_w, ctrl_h, th.small_radius());
    draw::set(cr, th.accent);
    let _ = cr.fill();
    let play_col = Rgba(1.0, 1.0, 1.0, 0.98);
    if world.media.playing {
        draw_pause_icon(
            cr,
            play_x + (ctrl_w - 18.0) / 2.0,
            cur_y + (ctrl_h - 18.0) / 2.0,
            play_col,
        );
    } else {
        draw_play_icon(
            cr,
            play_x + (ctrl_w - 18.0) / 2.0 + 1.0,
            cur_y + (ctrl_h - 18.0) / 2.0,
            play_col,
        );
    }

    // Next
    let next_x = play_x + ctrl_w + 12.0;
    draw_next_icon(
        cr,
        next_x + (ctrl_w - 18.0) / 2.0,
        cur_y + (ctrl_h - 18.0) / 2.0,
        th.text,
    );

    // Repeat
    let rep_x = next_x + ctrl_w + 12.0;
    let rep_col = if world.media.loop_status != "None" && !world.media.loop_status.is_empty() {
        th.accent
    } else {
        th.muted
    };
    draw_repeat_icon(
        cr,
        rep_x + (ctrl_w - 18.0) / 2.0,
        cur_y + (ctrl_h - 18.0) / 2.0,
        rep_col,
    );

    cur_y += ctrl_h + 10.0;

    // Nothing queued: no line and no row under the controls (the card is shorter by QUEUE_ROW_H)
    if world.media.queue.is_empty() {
        return;
    }

    // Thin line under controls
    draw::round_rect(cr, x + PAD_X, cur_y, w - 2.0 * PAD_X, 1.0, 0.5);
    draw::set(cr, th.track);
    let _ = cr.fill();
    cur_y += 6.0;

    // Under line: "Next: song · artist" and "Queue N" muted
    let next_song_str = if let Some(first_up) = world.media.queue.first() {
        format!("Next: {} · {}", first_up.title, first_up.artist)
    } else {
        String::new()
    };
    if !next_song_str.is_empty() {
        draw::text_max(
            cr,
            x + PAD_X,
            cur_y,
            w - 2.0 * PAD_X - 70.0,
            &Text {
                size: 11.5,
                bold: false,
                color: th.muted,
                text: &next_song_str,
            },
        );
    }

    if !world.media.queue.is_empty() {
        let q_str = format!("Queue {}", world.media.queue.len());
        draw::text_right(
            cr,
            x + w - PAD_X,
            cur_y,
            &Text {
                size: 11.5,
                bold: false,
                color: th.muted,
                text: &q_str,
            },
        );
    }
}

// =============================================================================
// Large Size: 96 px cover, title, artist · album, format line, three lyrics,
// progress, controls, "Up next" with N songs · M min, scrolling queue
// =============================================================================

fn draw_large(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    let cover_size = 96.0;
    let cx = x + PAD_X;
    let cy = y + PAD_Y;

    draw_cover(cr, th, cx, cy, cover_size, &world.media.art_url);

    // Track metadata beside 96 px cover
    let text_x = cx + cover_size + 16.0;
    let max_text_w = room(x, w, text_x, 0.0);

    draw::text_max(
        cr,
        text_x,
        cy + 10.0,
        max_text_w,
        &Text {
            size: 17.0,
            bold: true,
            color: th.text,
            text: &world.media.title,
        },
    );

    let artist_album = if !world.media.album.is_empty() {
        format!("{} · {}", world.media.artist, world.media.album)
    } else {
        world.media.artist.clone()
    };
    draw::text_max(
        cr,
        text_x,
        cy + 36.0,
        max_text_w,
        &Text {
            size: 13.0,
            bold: false,
            color: th.muted,
            text: &artist_album,
        },
    );

    let format_str = if !world.media.format.is_empty() {
        world.media.format.clone()
    } else {
        world.media.display_player().to_string()
    };
    draw::text_max(
        cr,
        text_x,
        cy + 58.0,
        max_text_w,
        &Text {
            size: 11.0,
            bold: false,
            color: th.muted,
            text: &format_str,
        },
    );

    let mut cur_y = cy + cover_size + 14.0;

    // Three lyric lines (before, current, next)
    if !world.media.lyrics.before.is_empty() {
        draw::text_max(
            cr,
            x + PAD_X,
            cur_y,
            w - 2.0 * PAD_X,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: &world.media.lyrics.before,
            },
        );
        cur_y += 18.0;
    }
    if !world.media.lyrics.current.is_empty() {
        draw::text_max(
            cr,
            x + PAD_X,
            cur_y,
            w - 2.0 * PAD_X,
            &Text {
                size: 14.0,
                bold: true,
                color: th.accent,
                text: &world.media.lyrics.current,
            },
        );
        cur_y += 20.0;
    }
    if !world.media.lyrics.next.is_empty() {
        draw::text_max(
            cr,
            x + PAD_X,
            cur_y,
            w - 2.0 * PAD_X,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: &world.media.lyrics.next,
            },
        );
        cur_y += 18.0;
    }
    cur_y += 6.0;

    // Progress bar (4 px) with times
    let time_left = format_time(world.media.position_us);
    let time_right = format_time(world.media.duration_us);
    let time_t_left = Text {
        size: 11.0,
        bold: false,
        color: th.muted,
        text: &time_left,
    };
    let time_t_right = Text {
        size: 11.0,
        bold: false,
        color: th.muted,
        text: &time_right,
    };
    let (tw_l, _) = draw::measure(cr, &time_t_left);
    let (tw_r, _) = draw::measure(cr, &time_t_right);

    draw::text(cr, x + PAD_X, cur_y + 1.0, &time_t_left);
    draw::text_right(cr, x + w - PAD_X, cur_y + 1.0, &time_t_right);

    let bar_x = x + PAD_X + tw_l + 8.0;
    let bar_w = (w - 2.0 * PAD_X - (tw_l + tw_r + 16.0)).max(20.0);
    let frac = if world.media.duration_us > 0 {
        (world.media.position_us as f64 / world.media.duration_us as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    draw::round_rect(cr, bar_x, cur_y + 5.0, bar_w, 4.0, 2.0);
    draw::set(cr, th.track);
    let _ = cr.fill();
    if frac > 0.0 {
        draw::round_rect(cr, bar_x, cur_y + 5.0, (bar_w * frac).max(4.0), 4.0, 2.0);
        draw::set(cr, th.accent);
        let _ = cr.fill();
    }
    cur_y += 24.0;

    // Controls
    let ctrl_w = 40.0;
    let ctrl_h = 34.0;
    let total_ctrls_w = 5.0 * ctrl_w + 4.0 * 14.0;
    let start_ctrl_x = x + (w - total_ctrls_w) / 2.0;

    let shuf_col = if world.media.shuffle {
        th.accent
    } else {
        th.muted
    };
    draw_shuffle_icon(
        cr,
        start_ctrl_x + (ctrl_w - 18.0) / 2.0,
        cur_y + (ctrl_h - 18.0) / 2.0,
        shuf_col,
    );

    let prev_x = start_ctrl_x + ctrl_w + 14.0;
    draw_prev_icon(
        cr,
        prev_x + (ctrl_w - 18.0) / 2.0,
        cur_y + (ctrl_h - 18.0) / 2.0,
        th.text,
    );

    let play_x = prev_x + ctrl_w + 14.0;
    draw::round_rect(cr, play_x, cur_y, ctrl_w, ctrl_h, th.small_radius());
    draw::set(cr, th.accent);
    let _ = cr.fill();
    let play_col = Rgba(1.0, 1.0, 1.0, 0.98);
    if world.media.playing {
        draw_pause_icon(
            cr,
            play_x + (ctrl_w - 18.0) / 2.0,
            cur_y + (ctrl_h - 18.0) / 2.0,
            play_col,
        );
    } else {
        draw_play_icon(
            cr,
            play_x + (ctrl_w - 18.0) / 2.0 + 1.0,
            cur_y + (ctrl_h - 18.0) / 2.0,
            play_col,
        );
    }

    let next_x = play_x + ctrl_w + 14.0;
    draw_next_icon(
        cr,
        next_x + (ctrl_w - 18.0) / 2.0,
        cur_y + (ctrl_h - 18.0) / 2.0,
        th.text,
    );

    let rep_x = next_x + ctrl_w + 14.0;
    let rep_col = if world.media.loop_status != "None" && !world.media.loop_status.is_empty() {
        th.accent
    } else {
        th.muted
    };
    draw_repeat_icon(
        cr,
        rep_x + (ctrl_w - 18.0) / 2.0,
        cur_y + (ctrl_h - 18.0) / 2.0,
        rep_col,
    );

    cur_y += ctrl_h + 16.0;

    // "Up next" with "N songs · M min"
    let total_mins = world
        .media
        .queue
        .iter()
        .map(|s| s.length_us / 60_000_000)
        .sum::<i64>();
    draw::text(
        cr,
        x + PAD_X,
        cur_y,
        &Text {
            size: 13.5,
            bold: true,
            color: th.text,
            text: "Up next",
        },
    );

    let queue_summary = format!("{} songs · {} min", world.media.queue.len(), total_mins);
    draw::text_right(
        cr,
        x + w - PAD_X,
        cur_y + 1.0,
        &Text {
            size: 11.5,
            bold: false,
            color: th.muted,
            text: &queue_summary,
        },
    );
    cur_y += 24.0;

    // Scrolling queue list (row height 36 px)
    let q_row_h = 36.0;
    let start_idx = world.media.queue_scroll;
    for (idx, song) in world.media.queue.iter().enumerate().skip(start_idx) {
        let row_y = cur_y + ((idx - start_idx) as f64) * q_row_h;
        if row_y + q_row_h > y + h - PAD_Y {
            break;
        }

        // Small thumbnail cover 28x28
        draw_cover(cr, th, x + PAD_X, row_y + 4.0, 28.0, &song.art_url);

        // Song title & artist
        let s_text_x = x + PAD_X + 36.0;
        let s_dur_str = format_time(song.length_us);
        let s_dur_t = Text {
            size: 11.0,
            bold: false,
            color: th.muted,
            text: &s_dur_str,
        };
        let (dw, _) = draw::measure(cr, &s_dur_t);
        let s_max_w = room(x, w, s_text_x, dw + 12.0);

        draw::text_max(
            cr,
            s_text_x,
            row_y + 4.0,
            s_max_w,
            &Text {
                size: 12.5,
                bold: false,
                color: th.text,
                text: &song.title,
            },
        );
        draw::text_max(
            cr,
            s_text_x,
            row_y + 20.0,
            s_max_w,
            &Text {
                size: 11.0,
                bold: false,
                color: th.muted,
                text: &song.artist,
            },
        );
        draw::text_right(cr, x + w - PAD_X, row_y + 10.0, &s_dur_t);
    }
}

// =============================================================================
// Drawing Helpers: Cover art & stroked line icons
// =============================================================================

thread_local! {
    /// The last covers drawn, by path and size: a cover was read from disk and scaled on every
    /// redraw (a few per second while music plays).
    static COVERS: std::cell::RefCell<Vec<(String, i32, cairo::ImageSurface)>> =
        const { std::cell::RefCell::new(Vec::new()) };
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
    let surf = apps::load_icon_surface(p, size)?;
    COVERS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 4 {
            c.remove(0);
        }
        c.push((key, size, surf.clone()));
    });
    Some(surf)
}

fn draw_cover(cr: &Context, th: &Theme, x: f64, y: f64, size: f64, art_url: &str) {
    let mut loaded = false;

    if !art_url.is_empty() {
        let local_path = decode_file_url(art_url).unwrap_or_else(|| art_url.to_string());
        let p = Path::new(&local_path);
        if p.exists() {
            if let Some(surf) = cover_surface(p, size as i32) {
                let _ = cr.save();
                draw::round_rect(cr, x, y, size, size, th.small_radius());
                cr.clip();
                let _ = cr.set_source_surface(&surf, x, y);
                let _ = cr.paint();
                let _ = cr.restore();
                loaded = true;
            }
        }
    }

    if !loaded {
        // Soft accent square with music note
        draw::round_rect(cr, x, y, size, size, th.small_radius());
        draw::set(cr, Rgba(th.accent.0, th.accent.1, th.accent.2, 0.20));
        let _ = cr.fill();

        let icon_size = (size * 0.40).clamp(16.0, 32.0);
        let ix = x + (size - icon_size) / 2.0;
        let iy = y + (size - icon_size) / 2.0;
        draw_music_note_icon(cr, ix, iy, icon_size, th.accent);
    }
}

fn draw_music_note_icon(cr: &Context, x: f64, y: f64, size: f64, col: Rgba) {
    draw::set(cr, col);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    // Beamed double eighth note: two noteheads, stems, beam across top
    let s = size / 18.0;
    // Notehead 1
    cr.new_sub_path();
    cr.arc(x + 4.5 * s, y + 13.5 * s, 2.5 * s, 0.0, 2.0 * PI);
    let _ = cr.fill();

    // Notehead 2
    cr.new_sub_path();
    cr.arc(x + 13.5 * s, y + 11.0 * s, 2.5 * s, 0.0, 2.0 * PI);
    let _ = cr.fill();

    // Stem 1
    cr.move_to(x + 7.0 * s, y + 13.5 * s);
    cr.line_to(x + 7.0 * s, y + 4.0 * s);

    // Stem 2
    cr.move_to(x + 16.0 * s, y + 11.0 * s);
    cr.line_to(x + 16.0 * s, y + 1.5 * s);

    // Beam
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

    // Right-pointing triangle
    cr.move_to(x + 4.0, y + 4.0);
    cr.line_to(x + 11.5, y + 9.0);
    cr.line_to(x + 4.0, y + 14.0);
    cr.close_path();
    let _ = cr.fill();

    // Right bar
    cr.move_to(x + 13.5, y + 4.0);
    cr.line_to(x + 13.5, y + 14.0);
    let _ = cr.stroke();
}

fn draw_prev_icon(cr: &Context, x: f64, y: f64, col: Rgba) {
    draw::set(cr, col);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    // Left bar
    cr.move_to(x + 4.5, y + 4.0);
    cr.line_to(x + 4.5, y + 14.0);
    let _ = cr.stroke();

    // Left-pointing triangle
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

    // Crossing arrows
    cr.move_to(x + 3.0, y + 5.0);
    cr.line_to(x + 8.0, y + 5.0);
    cr.line_to(x + 13.0, y + 13.0);
    cr.line_to(x + 15.0, y + 13.0);

    cr.move_to(x + 3.0, y + 13.0);
    cr.line_to(x + 8.0, y + 13.0);
    cr.line_to(x + 13.0, y + 5.0);
    cr.line_to(x + 15.0, y + 5.0);
    let _ = cr.stroke();

    // Arrowhead top right
    cr.move_to(x + 12.5, y + 3.0);
    cr.line_to(x + 15.5, y + 5.0);
    cr.line_to(x + 12.5, y + 7.0);

    // Arrowhead bottom right
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

    // Looping rectangle
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

    // Arrowhead
    cr.move_to(x + 14.0, y + 3.0);
    cr.line_to(x + 16.5, y + 5.0);
    cr.line_to(x + 14.0, y + 7.0);
    let _ = cr.stroke();
}

use std::f64::consts::FRAC_PI_2;

// =============================================================================
// Clicks, Drag & Scroll
// =============================================================================

/// Dispatches click events inside the `media` card.
pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if !is_left_button(button) || world.media.is_empty() {
        return false;
    }
    let (w, h) = size;
    let (px, py) = pos;

    match size_for_height(h) {
        MediaSize::Small => {
            let btn_total_w = 36.0 + 8.0 + 32.0;
            let btn_y = ((h - 32.0) / 2.0).max(2.0);
            let play_x = w - PAD_X - btn_total_w;
            let next_x = play_x + 36.0 + 8.0;

            if px >= play_x && px <= play_x + 36.0 && py >= btn_y && py <= btn_y + 32.0 {
                world.media.playing = !world.media.playing;
                let bus = world.media.bus_name.clone();
                world.note("media:play-pause".to_string());
                if world.live && !bus.is_empty() {
                    execute_media_control(&bus, "PlayPause");
                }
                return true;
            }
            if px >= next_x && px <= next_x + 32.0 && py >= btn_y && py <= btn_y + 32.0 {
                let bus = world.media.bus_name.clone();
                world.note("media:next".to_string());
                if world.live && !bus.is_empty() {
                    execute_media_control(&bus, "Next");
                }
                return true;
            }
        }
        MediaSize::Medium => {
            let cover_size = 64.0;
            let mut cur_y = PAD_Y + cover_size + 10.0;
            if !world.media.lyrics.current.is_empty() {
                cur_y += 18.0;
                if !world.media.lyrics.next.is_empty() {
                    cur_y += 16.0;
                }
                cur_y += 6.0;
            }

            // Progress bar click / seek
            let bar_y = cur_y;
            let bar_x = PAD_X + 44.0;
            let bar_w = (w - 2.0 * PAD_X - 88.0).max(20.0);
            if py >= bar_y - 8.0 && py <= bar_y + 16.0 && px >= bar_x && px <= bar_x + bar_w {
                let frac = ((px - bar_x) / bar_w).clamp(0.0, 1.0);
                let seek_us = (frac * world.media.duration_us as f64) as i64;
                world.media.position_us = seek_us;
                let bus = world.media.bus_name.clone();
                world.note(format!("media:seek:{seek_us}"));
                if world.live && !bus.is_empty() {
                    execute_media_seek(&bus, seek_us);
                }
                return true;
            }
            cur_y += 22.0;

            // Controls
            let ctrl_w = 36.0;
            let ctrl_h = 32.0;
            let total_ctrls_w = 5.0 * ctrl_w + 4.0 * 12.0;
            let start_ctrl_x = (w - total_ctrls_w) / 2.0;

            if py >= cur_y && py <= cur_y + ctrl_h {
                let bus = world.media.bus_name.clone();
                // Shuffle
                if px >= start_ctrl_x && px <= start_ctrl_x + ctrl_w {
                    world.media.shuffle = !world.media.shuffle;
                    world.note("media:shuffle".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_shuffle(&bus, world.media.shuffle);
                    }
                    return true;
                }
                // Prev
                let prev_x = start_ctrl_x + ctrl_w + 12.0;
                if px >= prev_x && px <= prev_x + ctrl_w {
                    world.note("media:previous".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_control(&bus, "Previous");
                    }
                    return true;
                }
                // Play/Pause
                let play_x = prev_x + ctrl_w + 12.0;
                if px >= play_x && px <= play_x + ctrl_w {
                    world.media.playing = !world.media.playing;
                    world.note("media:play-pause".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_control(&bus, "PlayPause");
                    }
                    return true;
                }
                // Next
                let next_x = play_x + ctrl_w + 12.0;
                if px >= next_x && px <= next_x + ctrl_w {
                    world.note("media:next".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_control(&bus, "Next");
                    }
                    return true;
                }
                // Repeat
                let rep_x = next_x + ctrl_w + 12.0;
                if px >= rep_x && px <= rep_x + ctrl_w {
                    world.media.loop_status = match world.media.loop_status.as_str() {
                        "None" => "Playlist".into(),
                        "Playlist" => "Track".into(),
                        _ => "None".into(),
                    };
                    world.note("media:repeat".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_loop(&bus, &world.media.loop_status);
                    }
                    return true;
                }
            }
        }
        MediaSize::Large => {
            let cover_size = 96.0;
            let mut cur_y = PAD_Y + cover_size + 14.0;
            if !world.media.lyrics.before.is_empty() {
                cur_y += 18.0;
            }
            if !world.media.lyrics.current.is_empty() {
                cur_y += 20.0;
            }
            if !world.media.lyrics.next.is_empty() {
                cur_y += 18.0;
            }
            cur_y += 6.0;

            // Progress bar
            let bar_y = cur_y;
            let bar_x = PAD_X + 44.0;
            let bar_w = (w - 2.0 * PAD_X - 88.0).max(20.0);
            if py >= bar_y - 8.0 && py <= bar_y + 16.0 && px >= bar_x && px <= bar_x + bar_w {
                let frac = ((px - bar_x) / bar_w).clamp(0.0, 1.0);
                let seek_us = (frac * world.media.duration_us as f64) as i64;
                world.media.position_us = seek_us;
                let bus = world.media.bus_name.clone();
                world.note(format!("media:seek:{seek_us}"));
                if world.live && !bus.is_empty() {
                    execute_media_seek(&bus, seek_us);
                }
                return true;
            }
            cur_y += 24.0;

            // Controls
            let ctrl_w = 40.0;
            let ctrl_h = 34.0;
            let total_ctrls_w = 5.0 * ctrl_w + 4.0 * 14.0;
            let start_ctrl_x = (w - total_ctrls_w) / 2.0;

            if py >= cur_y && py <= cur_y + ctrl_h {
                let bus = world.media.bus_name.clone();
                if px >= start_ctrl_x && px <= start_ctrl_x + ctrl_w {
                    world.media.shuffle = !world.media.shuffle;
                    world.note("media:shuffle".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_shuffle(&bus, world.media.shuffle);
                    }
                    return true;
                }
                let prev_x = start_ctrl_x + ctrl_w + 14.0;
                if px >= prev_x && px <= prev_x + ctrl_w {
                    world.note("media:previous".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_control(&bus, "Previous");
                    }
                    return true;
                }
                let play_x = prev_x + ctrl_w + 14.0;
                if px >= play_x && px <= play_x + ctrl_w {
                    world.media.playing = !world.media.playing;
                    world.note("media:play-pause".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_control(&bus, "PlayPause");
                    }
                    return true;
                }
                let next_x = play_x + ctrl_w + 14.0;
                if px >= next_x && px <= next_x + ctrl_w {
                    world.note("media:next".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_control(&bus, "Next");
                    }
                    return true;
                }
                let rep_x = next_x + ctrl_w + 14.0;
                if px >= rep_x && px <= rep_x + ctrl_w {
                    world.media.loop_status = match world.media.loop_status.as_str() {
                        "None" => "Playlist".into(),
                        "Playlist" => "Track".into(),
                        _ => "None".into(),
                    };
                    world.note("media:repeat".to_string());
                    if world.live && !bus.is_empty() {
                        execute_media_loop(&bus, &world.media.loop_status);
                    }
                    return true;
                }
            }
            cur_y += ctrl_h + 16.0 + 24.0;

            // Queue song click
            let q_row_h = 36.0;
            if py >= cur_y {
                let row_idx = ((py - cur_y) / q_row_h) as usize;
                let song_idx = world.media.queue_scroll + row_idx;
                if song_idx < world.media.queue.len() {
                    let song = &world.media.queue[song_idx];
                    let bus = world.media.bus_name.clone();
                    world.note(format!("media:play-queue:{song_idx}:{}", song.title));
                    if world.live && !bus.is_empty() {
                        execute_media_play_queue(&bus, song_idx);
                    }
                    return true;
                }
            }
        }
    }
    false
}

/// Dispatches drag events on the progress bar.
pub fn drag(world: &mut World, size: (f64, f64), pos: (f64, f64)) -> bool {
    let (w, h) = size;
    let (px, py) = pos;
    if world.media.is_empty() || world.media.duration_us <= 0 {
        return false;
    }

    let (bar_y, bar_x, bar_w) = match size_for_height(h) {
        MediaSize::Small => return false,
        MediaSize::Medium => {
            let mut cy = PAD_Y + 64.0 + 10.0;
            if !world.media.lyrics.current.is_empty() {
                cy += 18.0;
                if !world.media.lyrics.next.is_empty() {
                    cy += 16.0;
                }
                cy += 6.0;
            }
            (cy, PAD_X + 44.0, (w - 2.0 * PAD_X - 88.0).max(20.0))
        }
        MediaSize::Large => {
            let mut cy = PAD_Y + 96.0 + 14.0;
            if !world.media.lyrics.before.is_empty() {
                cy += 18.0;
            }
            if !world.media.lyrics.current.is_empty() {
                cy += 20.0;
            }
            if !world.media.lyrics.next.is_empty() {
                cy += 18.0;
            }
            cy += 6.0;
            (cy, PAD_X + 44.0, (w - 2.0 * PAD_X - 88.0).max(20.0))
        }
    };

    if py >= bar_y - 12.0 && py <= bar_y + 20.0 {
        let frac = ((px - bar_x) / bar_w).clamp(0.0, 1.0);
        let seek_us = (frac * world.media.duration_us as f64) as i64;
        world.media.position_us = seek_us;
        return true;
    }
    false
}

/// Dispatches mouse scroll on the queue list in Large view.
pub fn scroll(world: &mut World, size: (f64, f64), pos: (f64, f64), dy: f64) -> bool {
    if size_for_height(size.1) != MediaSize::Large || world.media.queue.is_empty() {
        return false;
    }
    let cur_scroll = world.media.queue_scroll;
    if dy > 0.0 {
        if cur_scroll + 1 < world.media.queue.len() {
            world.media.queue_scroll += 1;
            return true;
        }
    } else if dy < 0.0 && cur_scroll > 0 {
        world.media.queue_scroll -= 1;
        return true;
    }
    let _ = pos;
    false
}

// =============================================================================
// Live D-Bus (zbus) interactions
// =============================================================================

/// [`execute_media_control_blocking`] on a helper thread: an app that does not answer never freezes the
/// panel for the D-Bus timeout (it ran on the Wayland thread).
pub fn execute_media_control(bus: &str, method: &str) {
    let bus = bus.to_string();
    let method = method.to_string();
    std::thread::spawn(move || execute_media_control_blocking(&bus, &method));
}

fn execute_media_control_blocking(bus: &str, method: &str) {
    if let Ok(conn) = zbus::blocking::Connection::session() {
        if let Ok(proxy) = zbus::blocking::Proxy::new(
            &conn,
            bus,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ) {
            let _ = proxy.call::<&str, (), ()>(method, &());
        }
    }
}

/// [`execute_media_seek_blocking`] on a helper thread: an app that does not answer never freezes the
/// panel for the D-Bus timeout (it ran on the Wayland thread).
pub fn execute_media_seek(bus: &str, pos_us: i64) {
    let bus = bus.to_string();
    std::thread::spawn(move || execute_media_seek_blocking(&bus, pos_us));
}

fn execute_media_seek_blocking(bus: &str, pos_us: i64) {
    if let Ok(conn) = zbus::blocking::Connection::session() {
        if let Ok(proxy) = zbus::blocking::Proxy::new(
            &conn,
            bus,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ) {
            // SetPosition(track_id: ObjectPath, position: i64)
            let path =
                zbus::zvariant::ObjectPath::try_from("/org/mpris/MediaPlayer2/TrackList/NoTrack")
                    .unwrap_or_else(|_| zbus::zvariant::ObjectPath::from_static_str_unchecked("/"));
            let _ = proxy.call::<&str, (zbus::zvariant::ObjectPath<'_>, i64), ()>(
                "SetPosition",
                &(path, pos_us),
            );
        }
    }
}

/// [`execute_media_shuffle_blocking`] on a helper thread: an app that does not answer never freezes the
/// panel for the D-Bus timeout (it ran on the Wayland thread).
pub fn execute_media_shuffle(bus: &str, shuffle: bool) {
    let bus = bus.to_string();
    std::thread::spawn(move || execute_media_shuffle_blocking(&bus, shuffle));
}

fn execute_media_shuffle_blocking(bus: &str, shuffle: bool) {
    if let Ok(conn) = zbus::blocking::Connection::session() {
        if let Ok(proxy) = zbus::blocking::Proxy::new(
            &conn,
            bus,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ) {
            let _ = proxy.set_property("Shuffle", shuffle);
        }
    }
}

/// [`execute_media_loop_blocking`] on a helper thread: an app that does not answer never freezes the
/// panel for the D-Bus timeout (it ran on the Wayland thread).
pub fn execute_media_loop(bus: &str, loop_status: &str) {
    let bus = bus.to_string();
    let loop_status = loop_status.to_string();
    std::thread::spawn(move || execute_media_loop_blocking(&bus, &loop_status));
}

fn execute_media_loop_blocking(bus: &str, loop_status: &str) {
    if let Ok(conn) = zbus::blocking::Connection::session() {
        if let Ok(proxy) = zbus::blocking::Proxy::new(
            &conn,
            bus,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ) {
            let _ = proxy.set_property("LoopStatus", loop_status);
        }
    }
}

/// [`execute_media_play_queue_blocking`] on a helper thread: an app that does not answer never freezes the
/// panel for the D-Bus timeout (it ran on the Wayland thread).
pub fn execute_media_play_queue(bus: &str, idx: usize) {
    let bus = bus.to_string();
    std::thread::spawn(move || execute_media_play_queue_blocking(&bus, idx));
}

fn execute_media_play_queue_blocking(bus: &str, idx: usize) {
    if let Ok(conn) = zbus::blocking::Connection::session() {
        // Try app.melodyae.Extras PlayTrack
        if let Ok(extras) =
            zbus::blocking::Proxy::new(&conn, bus, "/org/mpris/MediaPlayer2", "app.melodyae.Extras")
        {
            if extras
                .call::<&str, (i32,), ()>("PlayTrack", &(idx as i32,))
                .is_ok()
            {
                return;
            }
        }
        // Fallback to standard MPRIS TrackList
        if let Ok(tl) = zbus::blocking::Proxy::new(
            &conn,
            bus,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.TrackList",
        ) {
            if let Ok(tracks) = tl.get_property::<Vec<zbus::zvariant::OwnedObjectPath>>("Tracks") {
                if let Some(track) = tracks.get(idx) {
                    let _ =
                        tl.call::<&str, (&zbus::zvariant::OwnedObjectPath,), ()>("GoTo", &(track,));
                }
            }
        }
    }
}

/// Queries running MPRIS players and Melodyae Extras over D-Bus as key=value lines.
pub fn fetch_dbus_string() -> String {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return String::new();
    };

    let Ok(dbus_proxy) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        return String::new();
    };

    let Ok(names) = dbus_proxy.list_names() else {
        return String::new();
    };

    let mut players = Vec::new();
    for name in names {
        if name.starts_with("org.mpris.MediaPlayer2.") {
            players.push(name);
        }
    }
    if players.is_empty() {
        return String::new();
    }

    // Pick playing player, else first paused player
    let mut chosen_bus = String::new();
    let mut chosen_playing = false;

    for bus in &players {
        if let Ok(proxy) = zbus::blocking::Proxy::new(
            &conn,
            bus,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ) {
            let status: String = proxy.get_property("PlaybackStatus").unwrap_or_default();
            if status.eq_ignore_ascii_case("playing") {
                chosen_bus = bus.to_string();
                chosen_playing = true;
                break;
            } else if chosen_bus.is_empty() && status.eq_ignore_ascii_case("paused") {
                chosen_bus = bus.to_string();
            }
        }
    }

    if chosen_bus.is_empty() {
        chosen_bus = players[0].to_string();
    }

    let Ok(proxy) = zbus::blocking::Proxy::new(
        &conn,
        chosen_bus.as_str(),
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    ) else {
        return String::new();
    };

    let meta: std::collections::HashMap<String, zbus::zvariant::OwnedValue> =
        proxy.get_property("Metadata").unwrap_or_default();
    let pos_us: i64 = proxy.get_property("Position").unwrap_or(0);
    let shuffle: bool = proxy.get_property("Shuffle").unwrap_or(false);
    let loop_stat: String = proxy
        .get_property("LoopStatus")
        .unwrap_or_else(|_| "None".into());

    let title = meta
        .get("xesam:title")
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or("")
        .to_string();
    let artist = meta
        .get("xesam:artist")
        .and_then(|v| {
            let val: &zbus::zvariant::Value<'_> = v;
            if let zbus::zvariant::Value::Array(arr) = val {
                arr.get::<&str>(0).ok().flatten().map(|s| s.to_string())
            } else if let Ok(s) = <&str>::try_from(v) {
                Some(s.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let album = meta
        .get("xesam:album")
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or("")
        .to_string();
    let art_url = meta
        .get("mpris:artUrl")
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or("")
        .to_string();
    let dur_us: i64 = meta
        .get("mpris:length")
        .and_then(|v| i64::try_from(v).ok())
        .unwrap_or(0);

    let mut out = format!(
        "status={}\ntitle={}\nartist={}\nalbum={}\nbus={}\nart_url={}\nposition={}\nduration={}\nshuffle={}\nloop={}\n",
        if chosen_playing { "Playing" } else { "Paused" },
        title,
        artist,
        album,
        chosen_bus,
        art_url,
        pos_us,
        dur_us,
        if shuffle { "true" } else { "false" },
        loop_stat
    );

    // Query Melodyae Extras: CurrentLyrics & Queue
    let bus_str = chosen_bus.as_str();
    if let Ok(extras) = zbus::blocking::Proxy::new(
        &conn,
        bus_str,
        "/org/mpris/MediaPlayer2",
        "app.melodyae.Extras",
    ) {
        if let Ok((curr, next, before)) =
            extras.call::<&str, (), (String, String, String)>("CurrentLyrics", &())
        {
            if !curr.is_empty() {
                out.push_str(&format!("lyrics_current={curr}\n"));
            }
            if !next.is_empty() {
                out.push_str(&format!("lyrics_next={next}\n"));
            }
            if !before.is_empty() {
                out.push_str(&format!("lyrics_before={before}\n"));
            }
        }
        if let Ok(q_items) =
            extras.call::<&str, (), Vec<(String, String, String, i64)>>("Queue", &())
        {
            for (qt, qa, qu, qlen) in q_items {
                out.push_str(&format!("queue={qt}\t{qa}\t{qu}\t{qlen}\n"));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_value_media_data() {
        let input = "status=Playing\n\
                     title=Starless\n\
                     artist=King Crimson\n\
                     album=Red\n\
                     player=Melodyae\n\
                     position=234000000\n\
                     duration=742000000\n\
                     shuffle=true\n\
                     loop=Playlist\n\
                     lyrics_current=Starless and bible black\n\
                     lyrics_next=Ice and frozen darkness\n\
                     lyrics_before=Sundown dazzling day\n\
                     queue=Starless\tKing Crimson\tfile:///covers/red.jpg\t742000000\n\
                     queue=Fallen Angel\tKing Crimson\tfile:///covers/red.jpg\t363000000\n";

        let data = parse_media_data(input).expect("parsed media data");
        assert!(data.playing);
        assert_eq!(data.title, "Starless");
        assert_eq!(data.artist, "King Crimson");
        assert_eq!(data.album, "Red");
        assert_eq!(data.player_name, "Melodyae");
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
    fn parses_json_media_data() {
        let input = r#"{
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

        let data = parse_media_data(input).expect("parsed json media data");
        assert!(data.playing);
        assert_eq!(data.title, "Echoes");
        assert_eq!(data.artist, "Pink Floyd");
        assert_eq!(data.album, "Meddle");
        assert_eq!(data.player_name, "Spotify");
        assert_eq!(
            data.lyrics.current,
            "Overhead the albatross hangs motionless upon the air"
        );
    }

    #[test]
    fn format_time_handles_zero_mins_and_hours() {
        assert_eq!(format_time(0), "0:00");
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
    }

    #[test]
    fn decode_file_url_handles_spaces_and_special_chars() {
        assert_eq!(
            decode_file_url("file:///home/user/Music/My%20Song.png"),
            Some("/home/user/Music/My Song.png".to_string())
        );
        assert_eq!(
            decode_file_url("file:///tmp/covers/track%231.png"),
            Some("/tmp/covers/track#1.png".to_string())
        );
        assert_eq!(decode_file_url("https://example.com/art.png"), None);
    }

    #[test]
    fn media_click_toggles_playback_and_controls() {
        let mut world = World::now();
        apply(
            &mut world.media,
            "media",
            "status=Paused\ntitle=Red\nartist=King Crimson\nposition=100000000\nduration=376000000\n",
        );

        // In Medium view (height 234 px)
        let size = (345.0, 234.0);
        // Controls row is at y = 112..144 px, center button is play/pause
        let play_pos = (345.0 / 2.0, 128.0);
        assert!(click(&mut world, size, play_pos, 1));
        assert!(world.media.playing);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("media:play-pause")
        );

        // Click again pauses
        assert!(click(&mut world, size, play_pos, 1));
        assert!(!world.media.playing);

        // Progress bar seek is at y = 90..106 px
        let bar_pos = (345.0 / 2.0, 95.0);
        assert!(click(&mut world, size, bar_pos, 1));
        assert!(world.actions.iter().any(|a| a.starts_with("media:seek:")));
    }

    #[test]
    fn media_small_size_click_play_and_next() {
        let mut world = World::now();
        apply(
            &mut world.media,
            "media",
            "status=Playing\ntitle=Frame by Frame\nartist=King Crimson\n",
        );

        // Small view (height 76 px)
        let size = (344.0, 76.0);
        // Play button is at w - PAD_X - 76.0 = 344 - 18 - 76 = 250..286
        let play_pos = (260.0, 38.0);
        assert!(click(&mut world, size, play_pos, 1));
        assert!(!world.media.playing);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("media:play-pause")
        );

        // Next button is at 250 + 36 + 8 = 294..326
        let next_pos = (305.0, 38.0);
        assert!(click(&mut world, size, next_pos, 1));
        assert_eq!(world.actions.last().map(String::as_str), Some("media:next"));
    }

    #[test]
    fn media_hides_when_empty() {
        let it = Item {
            module: crate::config::Module::Media,
            look: None,
            rect: None,
        };
        let world = World::now();
        assert_eq!(height(&it, &world), 0.0);
    }

    #[test]
    fn text_room_is_measured_inside_the_card_wherever_the_card_is() {
        // a medium card 344 px wide at the screen's left and at its right: the same room
        let left = room(8.0, 344.0, 8.0 + PAD_X + 64.0 + 14.0, 0.0);
        let right = room(1560.0, 344.0, 1560.0 + PAD_X + 64.0 + 14.0, 0.0);
        assert_eq!(left, right);
        assert!(left > 200.0, "{left}");
    }
}
