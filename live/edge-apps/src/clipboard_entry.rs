//! Clipboard history (`cliphist list`) entry parsing, sanitization, and command formatting.

/// The compositor's own history holds up to 300 entries; cliphist's list is cut to 24 by the
/// command that reads it.
pub const MAX_ITEMS: usize = 300;
pub const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_SANITIZE_SCAN_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Text,
    Image,
    Link,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipItem {
    pub raw_line: String,
    pub preview: String,
    pub time: String,
    pub is_image: bool,
    pub pinned: bool,
}

impl ClipItem {
    /// The command that puts it back on the clipboard.
    pub fn copy(&self) -> String {
        copy_command(&self.raw_line)
    }
}

/// A string as one shell word.
pub fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn strip_url_scheme(t: &str) -> Option<&str> {
    for prefix in ["https://", "http://", "ftp://"] {
        if t.as_bytes()
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
        {
            return Some(&t[prefix.len()..]);
        }
    }
    None
}

pub fn is_url_alone(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return false;
    }
    strip_url_scheme(t).is_some_and(|rest| !rest.is_empty())
}

pub fn kind_of(item: &ClipItem) -> ClipKind {
    if item.is_image {
        ClipKind::Image
    } else if is_url_alone(&item.preview) {
        ClipKind::Link
    } else {
        ClipKind::Text
    }
}

fn is_bidi_override(ch: char) -> bool {
    matches!(
        ch,
        '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

pub fn sanitize_inline_text_with_home(src: &str, home: Option<&str>) -> String {
    let mut collapsed = String::with_capacity(src.len().min(255));
    let mut prev_space = true;
    for (idx, ch) in src.char_indices() {
        if idx >= MAX_SANITIZE_SCAN_BYTES {
            break;
        }
        if matches!(ch, ' ' | '\t' | '\n' | '\r') || ch.is_whitespace() {
            if !prev_space {
                if collapsed.len() >= 255 {
                    break;
                }
                collapsed.push(' ');
                prev_space = true;
            }
        } else if !ch.is_control() && !is_bidi_override(ch) {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            if collapsed.len() + s.len() > 255 {
                break;
            }
            collapsed.push(ch);
            prev_space = false;
        }
    }
    if collapsed.ends_with(' ') {
        collapsed.pop();
    }

    let Some(h) = home
        .map(|h| h.trim_end_matches('/'))
        .filter(|h| h.len() > 1)
    else {
        return collapsed;
    };
    let mut out = String::with_capacity(collapsed.len());
    let bytes = collapsed.as_bytes();
    let mut i = 0;
    while i < collapsed.len() {
        let at_start = i == 0 || bytes[i - 1] == b' ';
        if at_start && collapsed[i..].starts_with(h) {
            let after = i + h.len();
            let valid_end = after == collapsed.len() || matches!(bytes[after], b'/' | b' ');
            if valid_end {
                out.push('~');
                i = after;
                continue;
            }
        }
        let ch = collapsed[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_dims(tok: &str) -> bool {
    tok.split_once('x').is_some_and(|(w, h)| {
        !w.is_empty()
            && !h.is_empty()
            && w.bytes().all(|b| b.is_ascii_digit())
            && h.bytes().all(|b| b.is_ascii_digit())
    })
}

pub fn parse_binary_image(body: &str) -> Option<String> {
    let t = body.trim();
    if let Some(inner) = t
        .strip_prefix("[[ binary data")
        .and_then(|s| s.strip_suffix("]]"))
    {
        for tok in inner.split_whitespace() {
            if is_dims(tok) {
                return Some(format!("Image {tok}"));
            }
        }
        return Some("Image".to_string());
    }
    if let Some(dims) = t.strip_prefix("Image ") {
        let d = dims.trim();
        if is_dims(d) {
            return Some(format!("Image {d}"));
        }
    }
    None
}

pub fn parse_cliphist_with_home(out: &str, home: Option<&str>) -> Vec<ClipItem> {
    let mut items = Vec::new();
    for raw in out.lines().take(4096) {
        let line = raw.trim_end_matches('\r');
        if line.len() > MAX_LINE_BYTES || line.trim().is_empty() {
            continue;
        }
        if items.len() >= MAX_ITEMS {
            break;
        }
        let (pinned, line_rest) = if let Some(r) = line.strip_prefix("pinned:") {
            (true, r)
        } else if let Some(r) = line.strip_prefix("pin:") {
            (true, r)
        } else if let Some(r) = line.strip_prefix('*') {
            (true, r.trim_start())
        } else {
            (false, line)
        };
        let Some((id, rest)) = line_rest.split_once('\t') else {
            continue;
        };
        if id.trim().is_empty() {
            continue;
        }
        let (raw_line, body, time) = match rest.rsplit_once('\t') {
            Some((b, t)) => (format!("{id}\t{b}"), b, t.trim().to_string()),
            None => (line_rest.to_string(), rest, String::new()),
        };
        let img = parse_binary_image(body);
        let is_image = img.is_some();
        let preview = match img {
            Some(label) => label,
            None => {
                let s = sanitize_inline_text_with_home(body, home);
                if s.is_empty() {
                    "(Clipboard item)".to_string()
                } else {
                    s
                }
            }
        };
        items.push(ClipItem {
            raw_line,
            preview,
            time,
            is_image,
            pinned,
        });
    }
    items.sort_by_key(|it| !it.pinned);
    items
}

pub fn copy_command(raw_line: &str) -> String {
    format!(
        "printf '%s\\n' {} | cliphist decode 2>/dev/null | wl-copy",
        sq(raw_line)
    )
}

pub fn delete_command(raw_line: &str) -> String {
    format!(
        "printf '%s\\n' {} | cliphist delete 2>/dev/null",
        sq(raw_line)
    )
}
