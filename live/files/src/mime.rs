//! Lightweight freedesktop `shared-mime-info` loader (`/usr/share/mime/{globs2,subclasses,aliases}`)
//! for MIME filter matching and human-readable file type labels without linking libmagic or GIO.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
struct GlobRule {
    weight: u32,
    mime: String,
    pattern: String,
    case_sensitive: bool,
}

#[derive(Clone, Debug, Default)]
pub struct MimeDb {
    /// Exact suffix map (e.g. "png" -> (weight, "image/png")) for O(1) common lookups.
    ext_map: HashMap<String, (u32, String)>,
    /// General glob rules (compound suffixes like `*.tar.gz`, wildcards, or case-sensitive globs).
    complex_globs: Vec<GlobRule>,
    /// Alias -> canonical MIME type.
    aliases: HashMap<String, String>,
    /// Child MIME -> parent MIME types (from `/usr/share/mime/subclasses`).
    subclasses: HashMap<String, Vec<String>>,
}

impl MimeDb {
    pub fn load_system() -> Self {
        Self::load_from_dir(Path::new("/usr/share/mime"))
    }

    pub fn load_from_dir(dir: &Path) -> Self {
        let globs2 = fs::read_to_string(dir.join("globs2")).unwrap_or_default();
        let subclasses = fs::read_to_string(dir.join("subclasses")).unwrap_or_default();
        let aliases = fs::read_to_string(dir.join("aliases")).unwrap_or_default();
        Self::parse(&globs2, &subclasses, &aliases)
    }

    pub fn parse(globs2: &str, subclasses_txt: &str, aliases_txt: &str) -> Self {
        let mut db = Self::default();

        for raw in aliases_txt.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split_whitespace();
            if let (Some(alias), Some(canon)) = (it.next(), it.next()) {
                db.aliases
                    .insert(alias.to_ascii_lowercase(), canon.to_ascii_lowercase());
            }
        }

        for raw in subclasses_txt.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split_whitespace();
            if let (Some(child), Some(parent)) = (it.next(), it.next()) {
                db.subclasses
                    .entry(child.to_ascii_lowercase())
                    .or_default()
                    .push(parent.to_ascii_lowercase());
            }
        }

        for raw in globs2.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(4, ':');
            let Some(w_str) = parts.next() else { continue };
            let Some(mime) = parts.next() else { continue };
            let Some(pat) = parts.next() else { continue };
            let flags = parts.next().unwrap_or("");
            let Ok(weight) = w_str.parse::<u32>() else {
                continue;
            };
            let case_sensitive = flags.split(',').any(|f| f == "cs");
            let mime_lower = mime.to_ascii_lowercase();

            if !case_sensitive {
                if let Some(ext) = pat.strip_prefix("*.") {
                    if !ext.is_empty() && !ext.contains(['*', '?', '[', ']', '.']) {
                        let key = ext.to_ascii_lowercase();
                        let replace = db
                            .ext_map
                            .get(&key)
                            .is_none_or(|(old_w, _)| weight > *old_w);
                        if replace {
                            db.ext_map.insert(key, (weight, mime_lower));
                        }
                        continue;
                    }
                }
            }

            db.complex_globs.push(GlobRule {
                weight,
                mime: mime_lower,
                pattern: pat.to_string(),
                case_sensitive,
            });
        }

        // Highest weight first, then longer pattern first per freedesktop shared-mime-info spec.
        db.complex_globs.sort_by(|a, b| {
            b.weight
                .cmp(&a.weight)
                .then_with(|| b.pattern.len().cmp(&a.pattern.len()))
        });
        db
    }

    pub fn canonical<'a>(&'a self, mime: &'a str) -> &'a str {
        self.aliases.get(mime).map(String::as_str).unwrap_or(mime)
    }

    /// Guesses a file's MIME type from its filename using `globs2`, with built-in common
    /// fallbacks when `/usr/share/mime/globs2` is absent in minimal test environments.
    pub fn guess_mime(&self, filename: &str) -> String {
        let lower = filename.to_ascii_lowercase();
        let mut best: Option<(u32, usize, &str)> = None;

        for rule in &self.complex_globs {
            let matched = if rule.case_sensitive {
                glob_match(&rule.pattern, filename)
            } else {
                glob_match(&rule.pattern.to_ascii_lowercase(), &lower)
            };
            if matched {
                let score = (rule.weight, rule.pattern.len(), rule.mime.as_str());
                if best.is_none_or(|b| (score.0, score.1) > (b.0, b.1)) {
                    best = Some(score);
                }
            }
        }

        if let Some((_, ext)) = lower.rsplit_once('.') {
            if let Some((w, mime)) = self.ext_map.get(ext) {
                let pat_len = ext.len() + 2;
                if best.is_none_or(|b| (*w, pat_len) > (b.0, b.1)) {
                    best = Some((*w, pat_len, mime.as_str()));
                }
            }
        }

        if let Some((_, _, mime)) = best {
            return self.canonical(mime).to_string();
        }

        builtin_mime_by_ext(&lower).to_string()
    }

    /// Returns `true` when `file_mime` matches `filter_mime` directly, via wildcard (`image/*`,
    /// `*/*`), via alias resolution, or via transitive `/usr/share/mime/subclasses` inheritance.
    pub fn mime_matches(&self, file_mime: &str, filter_mime: &str) -> bool {
        let f_lower = filter_mime.trim().to_ascii_lowercase();
        if f_lower == "*/*" || f_lower == "*" {
            return true;
        }
        let target = self.canonical(&f_lower);
        let start = self
            .canonical(&file_mime.trim().to_ascii_lowercase())
            .to_string();

        if matches_direct(&start, target) {
            return true;
        }

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        visited.insert(start.clone());
        queue.push_back(start);

        while let Some(cur) = queue.pop_front() {
            if let Some(parents) = self.subclasses.get(&cur) {
                for p in parents {
                    let cp = self.canonical(p).to_string();
                    if matches_direct(&cp, target) {
                        return true;
                    }
                    if visited.insert(cp.clone()) {
                        queue.push_back(cp);
                    }
                }
            }
        }
        false
    }

    /// Whether a file should be previewed as an image (PNG, JPEG, WebP, GIF; SVG intentionally excluded).
    pub fn is_previewable_image(&self, mime: &str) -> bool {
        let c = self.canonical(mime);
        matches!(c, "image/png" | "image/jpeg" | "image/webp" | "image/gif")
    }

    /// Whether a file should be previewed as plain text/code (including SVG as source text).
    pub fn is_previewable_text(&self, mime: &str) -> bool {
        let c = self.canonical(mime);
        if c.starts_with("text/") || self.mime_matches(c, "text/plain") {
            return true;
        }
        matches!(
            c,
            "application/json"
                | "application/xml"
                | "application/toml"
                | "application/yaml"
                | "application/x-yaml"
                | "application/javascript"
                | "application/x-shellscript"
                | "image/svg+xml"
        )
    }
}

fn matches_direct(mime: &str, pattern: &str) -> bool {
    if mime == pattern {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("/*") {
        if let Some((maj, _)) = mime.split_once('/') {
            return maj == prefix;
        }
    }
    false
}

/// Case-sensitive glob matcher supporting `*`, `?`, and `[abc]` / `[a-z]` / `[!a]` character classes.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_slice(&p, &t)
}

fn glob_match_slice(p: &[char], t: &[char]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_pi: Option<usize> = None;
    let mut star_ti: usize = 0;

    while ti < t.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star_pi = Some(pi);
                    star_ti = ti;
                    pi += 1;
                    continue;
                }
                '?' => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                '[' => {
                    if let Some((matched, next_pi)) = match_char_class(&p[pi..], t[ti]) {
                        if matched {
                            pi += next_pi;
                            ti += 1;
                            continue;
                        }
                    } else if p[pi] == t[ti] {
                        pi += 1;
                        ti += 1;
                        continue;
                    }
                }
                c if c == t[ti] => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                _ => {}
            }
        }
        if let Some(sp) = star_pi {
            star_ti += 1;
            ti = star_ti;
            pi = sp + 1;
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn match_char_class(p: &[char], ch: char) -> Option<(bool, usize)> {
    debug_assert_eq!(p[0], '[');
    let mut i = 1;
    if i >= p.len() {
        return None;
    }
    let negate = p[i] == '!' || p[i] == '^';
    if negate {
        i += 1;
    }
    let mut matched = false;
    let start = i;
    while i < p.len() {
        if p[i] == ']' && i > start {
            return Some((if negate { !matched } else { matched }, i + 1));
        }
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if ch >= p[i] && ch <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if ch == p[i] {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

fn builtin_mime_by_ext(lower_name: &str) -> &'static str {
    let Some((_, ext)) = lower_name.rsplit_once('.') else {
        return "application/octet-stream";
    };
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "txt" | "log" | "cfg" | "conf" | "ini" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "rs" => "text/rust",
        "c" | "h" => "text/x-csrc",
        "py" => "text/x-python",
        "sh" | "bash" => "application/x-shellscript",
        "json" => "application/json",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/x-yaml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg" | "opus" => "audio/ogg",
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        _ => "application/octet-stream",
    }
}

/// Plain human-readable type label for the Preview pane.
pub fn plain_type_name(mime: &str, is_dir: bool) -> String {
    if is_dir {
        return "Folder".to_string();
    }
    match mime {
        "image/png" => "PNG image".to_string(),
        "image/jpeg" => "JPEG image".to_string(),
        "image/webp" => "WebP image".to_string(),
        "image/gif" => "GIF image".to_string(),
        "image/svg+xml" => "SVG vector image".to_string(),
        "text/plain" => "Plain text".to_string(),
        "text/markdown" => "Markdown text".to_string(),
        "text/html" => "HTML document".to_string(),
        "text/css" => "CSS stylesheet".to_string(),
        "text/rust" => "Rust source".to_string(),
        "text/x-csrc" | "text/x-chdr" => "C source".to_string(),
        "text/x-python" => "Python script".to_string(),
        "application/x-shellscript" => "Shell script".to_string(),
        "application/json" => "JSON file".to_string(),
        "application/toml" => "TOML config".to_string(),
        "application/pdf" => "PDF document".to_string(),
        "application/zip" => "ZIP archive".to_string(),
        "application/gzip" | "application/x-compressed-tar" => "Compressed archive".to_string(),
        "application/octet-stream" | "" => "File".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_globs_subclasses_and_aliases() {
        let globs2 = "\
50:image/png:*.png
50:application/x-shellscript:*.sh
50:text/markdown:*.md
60:application/x-compressed-tar:*.tar.gz
50:application/gzip:*.gz
50:text/x-c++src:*.C:cs
";
        let subclasses = "\
text/markdown text/plain
application/x-shellscript text/plain
text/x-c++src text/plain
";
        let aliases = "text/xml application/xml\n";
        let db = MimeDb::parse(globs2, subclasses, aliases);

        assert_eq!(db.guess_mime("photo.PNG"), "image/png");
        assert_eq!(
            db.guess_mime("archive.tar.gz"),
            "application/x-compressed-tar"
        );
        assert_eq!(db.guess_mime("main.C"), "text/x-c++src");
        assert_eq!(db.guess_mime("main.c"), "text/x-csrc"); // built-in fallback, cs rule didn't match lowercase

        assert!(db.mime_matches("text/markdown", "text/plain"));
        assert!(db.mime_matches("application/x-shellscript", "text/plain"));
        assert!(db.mime_matches("image/png", "image/*"));
        assert!(db.mime_matches("text/xml", "application/xml"));
        assert!(!db.mime_matches("image/png", "text/plain"));
    }

    #[test]
    fn glob_match_handles_wildcards_and_char_classes() {
        assert!(glob_match("*.png", "shot.png"));
        assert!(!glob_match("*.png", "shot.PNG"));
        assert!(glob_match("*.[jJ][pP][gG]", "photo.JpG"));
        assert!(glob_match("img_??.png", "img_01.png"));
        assert!(!glob_match("img_??.png", "img_1.png"));
        assert!(glob_match("[!0-9]*.txt", "readme.txt"));
        assert!(!glob_match("[!0-9]*.txt", "1readme.txt"));
    }
}
