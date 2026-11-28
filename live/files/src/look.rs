//! Theme tokens loaded from `~/.cache/infynitae/look` (all 21 `LOOK_*` keys written by
//! `live/settings.sh` from `live/edge-apps/src/theme.rs`) plus an `inotify` watcher that reloads
//! the theme live while a window is open.

use std::env;
use std::ffi::CString;
use std::fs;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// RGBA colour in normalized 0..1 channels for Cairo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub f64, pub f64, pub f64, pub f64);

impl Rgba {
    pub const fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(
            r as f64 / 255.0,
            g as f64 / 255.0,
            b as f64 / 255.0,
            a as f64 / 255.0,
        )
    }

    pub const fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self::rgba8(r, g, b, 255)
    }

    pub const fn alpha(self, a: f64) -> Self {
        Self(self.0, self.1, self.2, a)
    }
}

// Derived surface alphas matching live/edge-apps/src/ui.rs and docs/mockups/file-picker.html.
const LIGHT_ALPHA: f64 = 0.08;
const CHOSEN_ALPHA: f64 = 0.16;
const BOX_ALPHA: f64 = 0.055;
const ALERT_BG_ALPHA: f64 = 0.14;

/// Complete theme token set read from `~/.cache/infynitae/look`.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub card: Rgba,
    pub edge: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub accent: Rgba,
    pub on_accent: Rgba,
    pub accent_text: Rgba,
    pub alert: Rgba,
    pub track: Rgba,
    pub corners: f64,
    pub small_radius: f64,
    pub font: String,
    pub mono_font: String,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            card: Rgba::rgb8(0x11, 0x13, 0x17),
            edge: Rgba::rgba8(0x33, 0x3b, 0x54, 0xe6),
            text: Rgba::rgb8(0xee, 0xf2, 0xf8),
            muted: Rgba::rgb8(0xa7, 0xab, 0xb0),
            accent: Rgba::rgb8(0x7a, 0xa2, 0xf7),
            on_accent: Rgba::rgb8(0x11, 0x13, 0x17),
            accent_text: Rgba::rgb8(0x7a, 0xa2, 0xf7),
            alert: Rgba::rgb8(0xed, 0x6a, 0x6f),
            track: Rgba::rgba8(0xee, 0xf2, 0xf8, 0x1f),
            corners: 8.0,
            small_radius: 4.0,
            font: "sans-serif".to_string(),
            mono_font: "monospace".to_string(),
        }
    }
}

impl Look {
    /// Opaque window background so text stays legible over any app underneath.
    pub const fn window_bg(&self) -> Rgba {
        self.card.alpha(1.0)
    }

    pub const fn hover_fill(&self) -> Rgba {
        self.text.alpha(LIGHT_ALPHA)
    }

    pub const fn chosen_fill(&self) -> Rgba {
        self.accent.alpha(CHOSEN_ALPHA)
    }

    pub const fn box_fill(&self) -> Rgba {
        self.text.alpha(BOX_ALPHA)
    }

    pub const fn alert_fill(&self) -> Rgba {
        self.alert.alpha(ALERT_BG_ALPHA)
    }

    pub fn load() -> Self {
        let mut look = match look_path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        };
        if let Some(mono) = load_terminal_font() {
            look.mono_font = mono;
        }
        look
    }

    pub fn load_from(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(content) => Self::parse(&content),
            Err(_) => Self::default(),
        }
    }

    pub fn parse(content: &str) -> Self {
        let mut look = Self::default();
        // Track explicit _AA lines separately so key order in the file does not matter.
        let mut alphas: [Option<u8>; 9] = [None; 9];

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, raw_val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let val = strip_quotes(raw_val.trim());
            match key {
                "LOOK_CARD" => assign_color(&mut look.card, val),
                "LOOK_EDGE" => assign_color(&mut look.edge, val),
                "LOOK_TEXT" => assign_color(&mut look.text, val),
                "LOOK_MUTED" => assign_color(&mut look.muted, val),
                "LOOK_ACCENT" => assign_color(&mut look.accent, val),
                "LOOK_ON_ACCENT" => assign_color(&mut look.on_accent, val),
                "LOOK_ACCENT_TEXT" => assign_color(&mut look.accent_text, val),
                "LOOK_ALERT" => assign_color(&mut look.alert, val),
                "LOOK_TRACK" => assign_color(&mut look.track, val),
                "LOOK_CARD_AA" => alphas[0] = parse_hex_byte(val),
                "LOOK_EDGE_AA" => alphas[1] = parse_hex_byte(val),
                "LOOK_TEXT_AA" => alphas[2] = parse_hex_byte(val),
                "LOOK_MUTED_AA" => alphas[3] = parse_hex_byte(val),
                "LOOK_ACCENT_AA" => alphas[4] = parse_hex_byte(val),
                "LOOK_ON_ACCENT_AA" => alphas[5] = parse_hex_byte(val),
                "LOOK_ACCENT_TEXT_AA" => alphas[6] = parse_hex_byte(val),
                "LOOK_ALERT_AA" => alphas[7] = parse_hex_byte(val),
                "LOOK_TRACK_AA" => alphas[8] = parse_hex_byte(val),
                "LOOK_CORNERS" => {
                    if let Ok(n) = val.parse::<u32>() {
                        look.corners = (n as f64).min(32.0);
                    }
                }
                "LOOK_SMALL_RADIUS" => {
                    if let Ok(n) = val.parse::<u32>() {
                        look.small_radius = (n as f64).min(24.0);
                    }
                }
                "LOOK_FONT" if !val.is_empty() => look.font = val.to_string(),
                _ => {}
            }
        }

        let slots: [&mut Rgba; 9] = [
            &mut look.card,
            &mut look.edge,
            &mut look.text,
            &mut look.muted,
            &mut look.accent,
            &mut look.on_accent,
            &mut look.accent_text,
            &mut look.alert,
            &mut look.track,
        ];
        for (slot, aa) in slots.into_iter().zip(alphas) {
            if let Some(a) = aa {
                *slot = slot.alpha(a as f64 / 255.0);
            }
        }
        look
    }
}

fn assign_color(dst: &mut Rgba, val: &str) {
    if let Some((c, has_alpha)) = parse_hex_color(val) {
        *dst = if has_alpha { c } else { c.alpha(dst.3) };
    }
}

pub fn look_path() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CACHE_HOME").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(xdg).join("infynitae/look"));
    }
    env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(|h| PathBuf::from(h).join(".cache/infynitae/look"))
}

fn config_path() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(xdg).join("infynitae/config"));
    }
    env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(|h| PathBuf::from(h).join(".config/infynitae/config"))
}

fn load_terminal_font() -> Option<String> {
    let path = config_path()?;
    let text = fs::read_to_string(path).ok()?;
    parse_terminal_font(&text)
}

pub fn parse_terminal_font(text: &str) -> Option<String> {
    let mut found = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("TERMINAL_FONT=") {
            let v = strip_quotes(rest.trim());
            if !v.is_empty() {
                found = Some(v.to_string());
            }
        }
    }
    found
}

fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn parse_hex_byte(s: &str) -> Option<u8> {
    if s.len() != 2 {
        return None;
    }
    u8::from_str_radix(s, 16).ok()
}

/// Parses `#RRGGBB` or `#RRGGBBAA` hex colours, returning `(colour, has_explicit_alpha)`.
pub fn parse_hex_color(s: &str) -> Option<(Rgba, bool)> {
    let hex = s.strip_prefix('#')?;
    let r = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
    let g = u8::from_str_radix(hex.get(2..4)?, 16).ok()?;
    let b = u8::from_str_radix(hex.get(4..6)?, 16).ok()?;
    match hex.len() {
        6 => Some((Rgba::rgb8(r, g, b), false)),
        8 => {
            let a = u8::from_str_radix(hex.get(6..8)?, 16).ok()?;
            Some((Rgba::rgba8(r, g, b, a), true))
        }
        _ => None,
    }
}

/// Watches the parent directory of `~/.cache/infynitae/look` with Linux `inotify` so both
/// in-place writes and atomic `rename()` replacements reload the theme live.
pub struct LookWatcher {
    fd: OwnedFd,
    path: PathBuf,
    file_name: Vec<u8>,
}

impl LookWatcher {
    pub fn watch(path: PathBuf) -> Option<Self> {
        let parent = path.parent()?;
        let file_name = path.file_name()?.as_bytes().to_vec();
        if !parent.is_dir() {
            return None;
        }
        let raw_fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if raw_fd < 0 {
            return None;
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        let c_parent = CString::new(parent.as_os_str().as_bytes()).ok()?;
        let mask = libc::IN_CLOSE_WRITE
            | libc::IN_MOVED_TO
            | libc::IN_CREATE
            | libc::IN_DELETE
            | libc::IN_ATTRIB;
        let wd = unsafe { libc::inotify_add_watch(fd.as_raw_fd(), c_parent.as_ptr(), mask) };
        if wd < 0 {
            return None;
        }
        Some(Self {
            fd,
            path,
            file_name,
        })
    }

    pub fn poll_changed(&self) -> Option<Look> {
        let mut buf = [0u8; 4096];
        let mut touched = false;
        let hdr_size = std::mem::size_of::<libc::inotify_event>();
        loop {
            let n = unsafe {
                libc::read(
                    self.fd.as_raw_fd(),
                    buf.as_mut_ptr().cast::<libc::c_void>(),
                    buf.len(),
                )
            };
            if n <= 0 {
                break;
            }
            let total = n as usize;
            let mut off = 0;
            while off + hdr_size <= total {
                let mut ev = std::mem::MaybeUninit::<libc::inotify_event>::uninit();
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        buf.as_ptr().add(off),
                        ev.as_mut_ptr().cast::<u8>(),
                        hdr_size,
                    );
                }
                let ev = unsafe { ev.assume_init() };
                let name_len = ev.len as usize;
                let next = off.saturating_add(hdr_size).saturating_add(name_len);
                if next > total {
                    break;
                }
                if name_len > 0 {
                    let raw_name = &buf[off + hdr_size..off + hdr_size + name_len];
                    let end = raw_name
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(raw_name.len());
                    if &raw_name[..end] == self.file_name.as_slice() {
                        touched = true;
                    }
                }
                off = next;
            }
        }
        if touched {
            let mut look = Look::load_from(&self.path);
            if let Some(mono) = load_terminal_font() {
                look.mono_font = mono;
            }
            Some(look)
        } else {
            None
        }
    }
}

impl AsFd for LookWatcher {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_look_uses_defaults() {
        let parsed = Look::parse("");
        assert_eq!(parsed, Look::default());
    }

    #[test]
    fn parses_all_21_tokens_from_edge_theme_format() {
        let input = r##"
LOOK_CARD="#171013"
LOOK_CARD_AA="cc"
LOOK_EDGE="#452e3c"
LOOK_EDGE_AA="e6"
LOOK_TEXT="#ffffff"
LOOK_TEXT_AA="ff"
LOOK_MUTED="#b8b0b4"
LOOK_MUTED_AA="ff"
LOOK_ACCENT="#cc70a9"
LOOK_ACCENT_AA="ff"
LOOK_ON_ACCENT="#171013"
LOOK_ON_ACCENT_AA="ff"
LOOK_ACCENT_TEXT="#cc70a9"
LOOK_ACCENT_TEXT_AA="ff"
LOOK_ALERT="#f0686c"
LOOK_ALERT_AA="ff"
LOOK_TRACK="#ffffff"
LOOK_TRACK_AA="1f"
LOOK_CORNERS="12"
LOOK_SMALL_RADIUS="7"
LOOK_FONT="Inter"
"##;
        let parsed = Look::parse(input);
        assert_eq!(parsed.card, Rgba::rgba8(0x17, 0x10, 0x13, 0xcc));
        assert_eq!(parsed.window_bg(), Rgba::rgb8(0x17, 0x10, 0x13));
        assert_eq!(parsed.edge, Rgba::rgba8(0x45, 0x2e, 0x3c, 0xe6));
        assert_eq!(parsed.text, Rgba::rgb8(0xff, 0xff, 0xff));
        assert_eq!(parsed.muted, Rgba::rgb8(0xb8, 0xb0, 0xb4));
        assert_eq!(parsed.accent, Rgba::rgb8(0xcc, 0x70, 0xa9));
        assert_eq!(parsed.on_accent, Rgba::rgb8(0x17, 0x10, 0x13));
        assert_eq!(parsed.accent_text, Rgba::rgb8(0xcc, 0x70, 0xa9));
        assert_eq!(parsed.alert, Rgba::rgb8(0xf0, 0x68, 0x6c));
        assert_eq!(parsed.track, Rgba::rgba8(0xff, 0xff, 0xff, 0x1f));
        assert_eq!(parsed.corners, 12.0);
        assert_eq!(parsed.small_radius, 7.0);
        assert_eq!(parsed.font, "Inter");
    }

    #[test]
    fn invalid_values_preserve_defaults() {
        let input = r##"
LOOK_CARD="#12345"
LOOK_EDGE="#gggggg"
LOOK_ACCENT_AA="zz"
LOOK_CORNERS="-4"
LOOK_SMALL_RADIUS="nope"
LOOK_FONT=""
"##;
        assert_eq!(Look::parse(input), Look::default());
    }

    #[test]
    fn watcher_reloads_on_atomic_rename() {
        let dir = env::temp_dir().join(format!("infynitae-files-look-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let look_file = dir.join("look");
        fs::write(&look_file, "LOOK_ACCENT=\"#112233\"\n").unwrap();

        let watcher = LookWatcher::watch(look_file.clone()).expect("inotify watcher");
        assert!(watcher.poll_changed().is_none());

        let tmp = dir.join("look.tmp");
        fs::write(&tmp, "LOOK_ACCENT=\"#ff8800\"\nLOOK_CORNERS=\"14\"\n").unwrap();
        fs::rename(&tmp, &look_file).unwrap();

        let updated = watcher.poll_changed().expect("watcher sees atomic rename");
        assert_eq!(updated.accent, Rgba::rgb8(0xff, 0x88, 0x00));
        assert_eq!(updated.corners, 14.0);

        let _ = fs::remove_dir_all(&dir);
    }
}
