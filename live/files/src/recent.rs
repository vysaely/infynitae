//! Reads and atomically updates `~/.local/share/recently-used.xbel` so picked and saved files
//! appear in both the file picker's Recent place and the Super+W launcher.

use crate::mime::MimeDb;
use crate::places::file_uri_to_path;
use crate::portal::protocol::path_bytes_to_file_uri;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::collections::HashSet;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const MAX_RECENT_DISPLAY: usize = 60;
const MAX_XBEL_ENTRIES: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentItem {
    pub path: PathBuf,
    pub modified_secs: Option<i64>,
}

pub fn xbel_path() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(xdg).join("recently-used.xbel"));
    }
    std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(|h| PathBuf::from(h).join(".local/share/recently-used.xbel"))
}

pub fn load_recent_files() -> Vec<RecentItem> {
    let Some(path) = xbel_path() else {
        return Vec::new();
    };
    load_recent_from(&path)
}

pub fn load_recent_from(xbel: &Path) -> Vec<RecentItem> {
    let Ok(text) = fs::read_to_string(xbel) else {
        return Vec::new();
    };
    parse_recent_xbel(&text)
        .into_iter()
        .filter(|item| item.path.exists())
        .take(MAX_RECENT_DISPLAY)
        .collect()
}

pub fn parse_recent_xbel(text: &str) -> Vec<RecentItem> {
    let mut items: Vec<(i64, PathBuf)> = Vec::new();
    let mut pos = 0;

    while let Some(rel) = text[pos..].find("<bookmark") {
        let tag_start = pos + rel;
        let after_kw = tag_start + "<bookmark".len();
        let Some(next_ch) = text[after_kw..].chars().next() else {
            break;
        };
        if !next_ch.is_ascii_whitespace() {
            pos = after_kw;
            continue;
        }
        let Some((tag_end, self_closing)) = find_tag_end(&text[after_kw..]) else {
            break;
        };
        let attrs = &text[after_kw..after_kw + tag_end];
        let block_start = after_kw + tag_end + 1;
        let next_pos = if self_closing {
            block_start
        } else if let Some(end_rel) = text[block_start..].find("</bookmark>") {
            block_start + end_rel + "</bookmark>".len()
        } else {
            block_start
        };
        pos = next_pos;

        let Some(href) = parse_attr(attrs, "href") else {
            continue;
        };
        let Some(path) = file_uri_to_path(&href) else {
            continue;
        };
        let ts = [
            parse_attr(attrs, "visited"),
            parse_attr(attrs, "modified"),
            parse_attr(attrs, "added"),
        ]
        .into_iter()
        .flatten()
        .filter_map(|s| parse_xbel_time(&s))
        .max()
        .unwrap_or(0);
        items.push((ts, path));
    }

    items.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (ts, path) in items {
        if seen.insert(path.clone()) {
            out.push(RecentItem {
                path,
                modified_secs: (ts > 0).then_some(ts),
            });
        }
    }
    out
}

/// Atomically adds or refreshes `paths` in `xbel` using a sibling temp file + `rename()`.
pub fn record_recent_files(
    xbel: &Path,
    paths: &[PathBuf],
    mime_db: &MimeDb,
    app_name: &str,
) -> std::io::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    if let Some(parent) = xbel.parent() {
        fs::create_dir_all(parent)?;
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let app = if app_name.is_empty() {
        "infynitae-files"
    } else {
        app_name
    };

    let existing_text = fs::read_to_string(xbel).unwrap_or_default();
    let existing_blocks = extract_bookmark_blocks(&existing_text);

    let new_hrefs: HashSet<String> = paths
        .iter()
        .map(|p| path_bytes_to_file_uri(p.as_os_str().as_bytes()))
        .collect();

    let mut out = String::with_capacity(existing_text.len() + paths.len() * 320 + 256);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<xbel version=\"1.0\"\n");
    out.push_str(
        "      xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\"\n",
    );
    out.push_str("      xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\"\n");
    out.push_str(">\n");

    let mut kept = 0usize;
    let max_old = MAX_XBEL_ENTRIES.saturating_sub(paths.len());
    let skip_old = existing_blocks
        .iter()
        .filter(|(href, _)| !new_hrefs.contains(href))
        .count()
        .saturating_sub(max_old);

    let mut skipped = 0usize;
    for (href, block) in existing_blocks {
        if new_hrefs.contains(&href) {
            continue;
        }
        if skipped < skip_old {
            skipped += 1;
            continue;
        }
        out.push_str("  ");
        out.push_str(&block);
        out.push('\n');
        kept += 1;
        if kept >= max_old {
            break;
        }
    }

    for p in paths {
        let href = path_bytes_to_file_uri(p.as_os_str().as_bytes());
        let mime = if p.is_dir() {
            "inode/directory".to_string()
        } else {
            let fname = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            mime_db.guess_mime(&fname)
        };
        out.push_str(&format!(
            "  <bookmark href=\"{}\" added=\"{now}\" modified=\"{now}\" visited=\"{now}\">\n\
             \x20   <info>\n\
             \x20     <metadata owner=\"http://freedesktop.org\">\n\
             \x20       <mime:mime-type type=\"{}\"/>\n\
             \x20       <bookmark:applications>\n\
             \x20         <bookmark:application name=\"{}\" exec=\"&apos;{} %u&apos;\" modified=\"{now}\" count=\"1\"/>\n\
             \x20       </bookmark:applications>\n\
             \x20     </metadata>\n\
             \x20   </info>\n\
             \x20 </bookmark>\n",
            escape_xml(&href),
            escape_xml(&mime),
            escape_xml(app),
            escape_xml(app),
        ));
    }

    out.push_str("</xbel>\n");

    let parent = xbel.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(".recently-used.xbel.{}.tmp", std::process::id()));
    fs::write(&tmp, out.as_bytes())?;
    if let Err(e) = fs::rename(&tmp, xbel) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn extract_bookmark_blocks(text: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut pos = 0;
    while let Some(rel) = text[pos..].find("<bookmark") {
        let tag_start = pos + rel;
        let after_kw = tag_start + "<bookmark".len();
        let Some(next_ch) = text[after_kw..].chars().next() else {
            break;
        };
        if !next_ch.is_ascii_whitespace() {
            pos = after_kw;
            continue;
        }
        let Some((tag_end, self_closing)) = find_tag_end(&text[after_kw..]) else {
            break;
        };
        let attrs = &text[after_kw..after_kw + tag_end];
        let block_start = after_kw + tag_end + 1;
        let block_end = if self_closing {
            block_start
        } else if let Some(end_rel) = text[block_start..].find("</bookmark>") {
            block_start + end_rel + "</bookmark>".len()
        } else {
            break;
        };
        pos = block_end;
        if let Some(href) = parse_attr(attrs, "href") {
            blocks.push((href, text[tag_start..block_end].trim().to_string()));
        }
    }
    blocks
}

fn find_tag_end(s: &str) -> Option<(usize, bool)> {
    let mut quote: Option<char> = None;
    let mut prev_slash = false;
    for (idx, ch) in s.char_indices() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '>' => return Some((idx, prev_slash)),
            None => {}
        }
        prev_slash = ch == '/' && quote.is_none();
    }
    None
}

fn parse_attr(attrs: &str, target: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let name = &attrs[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let val = if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
            let q = bytes[i];
            i += 1;
            let v_start = i;
            while i < bytes.len() && bytes[i] != q {
                i += 1;
            }
            let v = &attrs[v_start..i];
            if i < bytes.len() {
                i += 1;
            }
            v
        } else {
            let v_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' {
                i += 1;
            }
            &attrs[v_start..i]
        };
        if name == target {
            return Some(unescape_xml(val));
        }
    }
    None
}

fn parse_xbel_time(s: &str) -> Option<i64> {
    let t = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Some(dt.timestamp());
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S") {
        return Some(ndt.and_utc().timestamp());
    }
    None
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn unescape_xml(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xbel_newest_first_and_atomic_write_roundtrips() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-files-recent-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f1 = dir.join("old note.txt");
        let f2 = dir.join("new & pic.png");
        fs::write(&f1, b"a").unwrap();
        fs::write(&f2, b"b").unwrap();

        let xbel = dir.join("recently-used.xbel");
        let initial = format!(
            "<?xml version=\"1.0\"?>\n<xbel version=\"1.0\">\n\
             <bookmark href=\"{}\" modified=\"2026-01-01T10:00:00Z\"/>\n\
             </xbel>\n",
            escape_xml(&path_bytes_to_file_uri(f1.as_os_str().as_bytes()))
        );
        fs::write(&xbel, initial).unwrap();

        let db = MimeDb::default();
        record_recent_files(&xbel, std::slice::from_ref(&f2), &db, "firefox").unwrap();

        let loaded = load_recent_from(&xbel);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].path, f2);
        assert_eq!(loaded[1].path, f1);

        let _ = fs::remove_dir_all(&dir);
    }
}
