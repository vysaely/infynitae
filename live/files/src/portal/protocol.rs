//! Portal stdin/stdout JSON contract between `live/portal/file-chooser.c` and
//! `infynitae-files --portal`, raw-byte base64 + percent-encoded `file://` URIs, mnemonic
//! stripping, `current_file` splitting, and `SaveFiles` collision renaming.

use crate::text_input::normalize_lexical;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterPattern {
    /// `0` = case-sensitive glob (`*.png`), `1` = MIME type (`image/*`, `text/plain`).
    pub kind: u32,
    pub pattern: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalFilter {
    pub label: String,
    pub patterns: Vec<FilterPattern>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalChoice {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub options: Vec<ChoiceOption>,
    #[serde(default)]
    pub initial: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalOptions {
    #[serde(default)]
    pub accept_label: Option<String>,
    #[serde(default)]
    pub modal: Option<bool>,
    #[serde(default)]
    pub multiple: Option<bool>,
    #[serde(default)]
    pub directory: Option<bool>,
    #[serde(default)]
    pub filters: Vec<PortalFilter>,
    #[serde(default)]
    pub current_filter: Option<PortalFilter>,
    #[serde(default)]
    pub choices: Vec<PortalChoice>,
    #[serde(default)]
    pub current_folder_b64: Option<String>,
    #[serde(default)]
    pub current_name: Option<String>,
    #[serde(default)]
    pub current_file_b64: Option<String>,
    #[serde(default)]
    pub files_b64: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalRequest {
    pub method: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub parent_window: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub options: PortalOptions,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalResponse {
    /// `0` = user picked/saved, `1` = user cancelled.
    pub response: u32,
    #[serde(default)]
    pub uris: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub writable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<(String, String)>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_filter: Option<PortalFilter>,
}

impl PortalResponse {
    pub fn cancelled() -> Self {
        Self {
            response: 1,
            uris: Vec::new(),
            writable: None,
            choices: None,
            current_filter: None,
        }
    }
}

/// Strips GTK mnemonic underscores (`_Open` -> `Open`, `__` -> `_`).
pub fn strip_mnemonic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '_' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Removes `/` and NUL bytes from a Save filename so an app cannot inject path traversal.
pub fn sanitize_save_name(name: &str) -> String {
    name.chars()
        .filter(|&c| c != '/' && c != '\0' && !c.is_control())
        .collect()
}

/// Decodes a base64-encoded D-Bus `ay` byte array, stripping trailing NUL terminators.
pub fn decode_dbus_bytes(b64: &str) -> Option<Vec<u8>> {
    let mut bytes = base64_decode(b64)?;
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    if bytes.is_empty() {
        return None;
    }
    Some(bytes)
}

pub fn decode_dbus_path(b64: &str) -> Option<PathBuf> {
    let bytes = decode_dbus_bytes(b64)?;
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

/// Splits a raw `current_file` byte path into `(parent_dir, sanitized_filename)`.
pub fn split_current_file(raw: &[u8]) -> (Option<PathBuf>, Option<String>) {
    let mut trimmed = raw;
    while trimmed.last() == Some(&0) {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    if trimmed.is_empty() {
        return (None, None);
    }
    let path = PathBuf::from(OsString::from_vec(trimmed.to_vec()));
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);
    let name = path
        .file_name()
        .map(|n| sanitize_save_name(&n.to_string_lossy()))
        .filter(|n| !n.is_empty());
    (parent, name)
}

/// Encodes raw Linux path bytes as a canonical percent-encoded `file://` URI without losing
/// non-UTF-8 bytes.
pub fn path_bytes_to_file_uri(raw_bytes: &[u8]) -> String {
    let mut out = String::with_capacity(7 + raw_bytes.len());
    out.push_str("file://");
    if !raw_bytes.starts_with(b"/") {
        out.push('/');
    }
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &b in raw_bytes {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/');
        if keep {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

/// Normalizes a path lexically (resolving `.` and `..` without failing if the final Save target
/// does not exist yet) and encodes its raw bytes as a `file://` URI.
pub fn path_to_file_uri(path: &Path) -> String {
    let norm = normalize_lexical(path);
    path_bytes_to_file_uri(norm.as_os_str().as_bytes())
}

/// Computes a non-colliding filename inside `dir` (`name (2).ext`, `name (3).ext`, ...).
pub fn collision_rename(dir: &Path, filename: &str) -> String {
    if !path_exists_any(&dir.join(filename)) {
        return filename.to_string();
    }
    let (stem, ext) = split_stem_ext(filename);
    for n in 2u32..100_000 {
        let candidate = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        if !path_exists_any(&dir.join(&candidate)) {
            return candidate;
        }
    }
    format!("{filename}.new")
}

/// Checks existence including broken symlinks (`symlink_metadata`) so Save never silently
/// overwrites a dangling symlink either.
pub fn path_exists_any(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn split_stem_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < name.len() => (&name[..dot], &name[dot + 1..]),
        _ => (name, ""),
    }
}

/// RFC 4648 base64 decoder (zero external crates).
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let bytes: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity((bytes.len() / 4) * 3);
    for chunk in bytes.as_chunks::<4>().0 {
        let a = b64_val(chunk[0])?;
        let b = b64_val(chunk[1])?;
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            let c = b64_val(chunk[2])?;
            out.push((b << 4) | (c >> 2));
            if chunk[3] != b'=' {
                let d = b64_val(chunk[3])?;
                out.push((c << 6) | d);
            }
        } else if chunk[3] != b'=' {
            return None;
        }
    }
    Some(out)
}

#[cfg(test)]
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let v = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHA[((v >> 18) & 0x3f) as usize] as char);
        out.push(ALPHA[((v >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHA[((v >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHA[(v & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn b64_val(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_utf8_bytes_roundtrip_from_base64_to_percent_encoded_file_uri() {
        let raw = b"/tmp/caf\xff/my #1 report?.txt\0";
        let b64 = base64_encode(raw);
        let path = decode_dbus_path(&b64).expect("decoded path");
        assert_eq!(
            path.as_os_str().as_bytes(),
            b"/tmp/caf\xff/my #1 report?.txt"
        );
        let uri = path_to_file_uri(&path);
        assert_eq!(uri, "file:///tmp/caf%FF/my%20%231%20report%3F.txt");
    }

    #[test]
    fn strips_gtk_mnemonics_and_sanitizes_save_name() {
        assert_eq!(strip_mnemonic("_Open"), "Open");
        assert_eq!(strip_mnemonic("Save _As"), "Save As");
        assert_eq!(strip_mnemonic("a__b"), "a_b");
        assert_eq!(
            sanitize_save_name("../../evil/note.txt\0"),
            "....evilnote.txt"
        );
    }

    #[test]
    fn splits_current_file_and_renames_collisions() {
        let (dir, name) = split_current_file(b"/home/alice/Docs/report.pdf\0");
        assert_eq!(dir, Some(PathBuf::from("/home/alice/Docs")));
        assert_eq!(name.as_deref(), Some("report.pdf"));

        let tmp = std::env::temp_dir().join(format!(
            "infynitae-files-collide-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("report.pdf"), b"1").unwrap();
        std::fs::write(tmp.join("report (2).pdf"), b"2").unwrap();
        assert_eq!(collision_rename(&tmp, "report.pdf"), "report (3).pdf");
        assert_eq!(collision_rename(&tmp, "fresh.pdf"), "fresh.pdf");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn portal_request_and_response_json_roundtrip() {
        let req_json = r#"{
            "method": "OpenFile",
            "app_id": "org.mozilla.firefox",
            "parent_window": "wayland:abc123",
            "title": "Open File",
            "options": {
                "accept_label": "_Select",
                "multiple": true,
                "filters": [{"label": "Images", "patterns": [{"kind": 0, "pattern": "*.png"}]}]
            }
        }"#;
        let req: PortalRequest = serde_json::from_str(req_json).unwrap();
        assert_eq!(req.method, "OpenFile");
        assert_eq!(req.options.multiple, Some(true));
        assert_eq!(req.options.filters.len(), 1);

        let resp = PortalResponse {
            response: 0,
            uris: vec!["file:///tmp/a.png".into()],
            writable: Some(true),
            choices: Some(vec![("enc".into(), "utf8".into())]),
            current_filter: Some(req.options.filters[0].clone()),
        };
        let encoded = serde_json::to_string(&resp).unwrap();
        let decoded: PortalResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, resp);
    }
}
