//! Single-line editable text field state machine (shared by Ctrl+L path edit, Ctrl+F search,
//! Save filename box, and inline New folder bar) plus Tab path completion.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    /// Byte offset of caret (always on a UTF-8 char boundary).
    cursor: usize,
    /// Optional byte offset of selection anchor.
    anchor: Option<usize>,
}

impl TextInput {
    pub fn new(initial: impl Into<String>) -> Self {
        let text = initial.into();
        let cursor = text.len();
        Self {
            text,
            cursor,
            anchor: None,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Selected byte range `(start, end)` when non-empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        if a == self.cursor {
            None
        } else if a < self.cursor {
            Some((a, self.cursor))
        } else {
            Some((self.cursor, a))
        }
    }

    pub fn selected_text(&self) -> Option<&str> {
        let (a, b) = self.selection()?;
        Some(&self.text[a..b])
    }

    pub fn set_text(&mut self, s: impl Into<String>) {
        self.text = s.into();
        self.cursor = self.text.len();
        self.anchor = None;
    }

    pub fn select_all(&mut self) {
        if self.text.is_empty() {
            self.cursor = 0;
            self.anchor = None;
        } else {
            self.anchor = Some(0);
            self.cursor = self.text.len();
        }
    }

    /// Selects the filename stem before the last extension dot (e.g. `untitled.pdf` -> `untitled`),
    /// leaving dotfiles like `.gitignore` or names without a dot fully selected.
    pub fn select_stem_before_dot(&mut self) {
        if self.text.is_empty() {
            self.cursor = 0;
            self.anchor = None;
            return;
        }
        let end = match self.text.rfind('.') {
            Some(dot) if dot > 0 => dot,
            _ => self.text.len(),
        };
        self.anchor = Some(0);
        self.cursor = end;
    }

    pub fn set_cursor(&mut self, byte_idx: usize, extend_selection: bool) {
        let idx = clamp_boundary(&self.text, byte_idx);
        if extend_selection {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = idx;
    }

    pub fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            return false;
        };
        self.text.replace_range(a..b, "");
        self.cursor = a;
        self.anchor = None;
        true
    }

    /// Inserts UTF-8 text (stripping control chars / newlines so fields stay single-line).
    pub fn insert(&mut self, s: &str) -> bool {
        let clean: String = s.chars().filter(|&c| !c.is_control()).collect();
        if clean.is_empty() {
            return false;
        }
        self.delete_selection();
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
        true
    }

    pub fn backspace(&mut self, by_word: bool) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let prev = if by_word {
            prev_word_boundary(&self.text, self.cursor)
        } else {
            prev_char_boundary(&self.text, self.cursor)
        };
        self.text.replace_range(prev..self.cursor, "");
        self.cursor = prev;
        true
    }

    pub fn delete(&mut self, by_word: bool) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        let next = if by_word {
            next_word_boundary(&self.text, self.cursor)
        } else {
            next_char_boundary(&self.text, self.cursor)
        };
        self.text.replace_range(self.cursor..next, "");
        true
    }

    pub fn move_left(&mut self, by_word: bool, extend: bool) {
        if !extend {
            if let Some((a, _)) = self.selection() {
                self.cursor = a;
                self.anchor = None;
                return;
            }
        } else if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = if by_word {
            prev_word_boundary(&self.text, self.cursor)
        } else {
            prev_char_boundary(&self.text, self.cursor)
        };
        if !extend {
            self.anchor = None;
        }
    }

    pub fn move_right(&mut self, by_word: bool, extend: bool) {
        if !extend {
            if let Some((_, b)) = self.selection() {
                self.cursor = b;
                self.anchor = None;
                return;
            }
        } else if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = if by_word {
            next_word_boundary(&self.text, self.cursor)
        } else {
            next_char_boundary(&self.text, self.cursor)
        };
        if !extend {
            self.anchor = None;
        }
    }

    pub fn move_home(&mut self, extend: bool) {
        self.set_cursor(0, extend);
    }

    pub fn move_end(&mut self, extend: bool) {
        self.set_cursor(self.text.len(), extend);
    }

    pub fn cut(&mut self) -> Option<String> {
        let sel = self.selected_text()?.to_string();
        self.delete_selection();
        Some(sel)
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
    }
}

fn clamp_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn prev_char_boundary(s: &str, idx: usize) -> usize {
    s[..idx].char_indices().next_back().map_or(0, |(i, _)| i)
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    s[idx..]
        .chars()
        .next()
        .map_or(s.len(), |c| idx + c.len_utf8())
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn prev_word_boundary(s: &str, idx: usize) -> usize {
    let mut chars: Vec<(usize, char)> = s[..idx].char_indices().collect();
    while let Some(&(_, c)) = chars.last() {
        if is_word_char(c) {
            break;
        }
        chars.pop();
    }
    while let Some(&(_, c)) = chars.last() {
        if !is_word_char(c) {
            break;
        }
        chars.pop();
    }
    chars.last().map_or(0, |&(i, c)| i + c.len_utf8())
}

fn next_word_boundary(s: &str, idx: usize) -> usize {
    let mut it = s[idx..].char_indices().peekable();
    while let Some(&(_, c)) = it.peek() {
        if is_word_char(c) {
            break;
        }
        it.next();
    }
    while let Some(&(_, c)) = it.peek() {
        if !is_word_char(c) {
            break;
        }
        it.next();
    }
    it.peek().map_or(s.len(), |&(rel, _)| idx + rel)
}

/// Expands leading `~` or `~/...` against `home`, and resolves relative paths against `current_dir`.
pub fn resolve_typed_path(raw: &str, current_dir: &Path, home: Option<&Path>) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return current_dir.to_path_buf();
    }
    let expanded = if trimmed == "~" {
        home.map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(trimmed))
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(h) = home {
            h.join(rest)
        } else {
            PathBuf::from(trimmed)
        }
    } else {
        let p = PathBuf::from(trimmed);
        if p.is_absolute() {
            p
        } else {
            current_dir.join(p)
        }
    };
    normalize_lexical(&expanded)
}

/// Lexically resolves `.` and `..` without touching the filesystem so nonexistent Save targets work.
pub fn normalize_lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

/// Completes a typed path for `Ctrl+L` `Tab`: matches subdirectories (or all entries if no
/// directory matches) in the parent folder and returns the completed string when a longer prefix
/// or unique match exists.
pub fn complete_path(input: &str, current_dir: &Path, home: Option<&Path>) -> Option<String> {
    let had_tilde = input == "~" || input.starts_with("~/");
    if input == "~" {
        return Some("~/".to_string());
    }
    let expanded = if let Some(rest) = input.strip_prefix("~/") {
        let h = home?;
        format!("{}/{}", h.to_string_lossy().trim_end_matches('/'), rest)
    } else if input.starts_with('/') {
        input.to_string()
    } else {
        format!(
            "{}/{}",
            current_dir.to_string_lossy().trim_end_matches('/'),
            input
        )
    };

    let (dir_str, prefix) = match expanded.rfind('/') {
        Some(0) => ("/", &expanded[1..]),
        Some(pos) => (&expanded[..pos], &expanded[pos + 1..]),
        None => return None,
    };
    let dir_path = Path::new(dir_str);
    let entries = fs::read_dir(dir_path).ok()?;
    let show_hidden = prefix.starts_with('.');

    let mut dir_matches: Vec<String> = Vec::new();
    let mut file_matches: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if !name.starts_with(prefix) {
            continue;
        }
        let is_dir = entry
            .file_type()
            .map(|t| t.is_dir() || (t.is_symlink() && entry.path().is_dir()))
            .unwrap_or(false);
        if is_dir {
            dir_matches.push(name);
        } else {
            file_matches.push(name);
        }
    }

    let (candidates, are_dirs) = if !dir_matches.is_empty() {
        (dir_matches, true)
    } else if !file_matches.is_empty() {
        (file_matches, false)
    } else {
        return None;
    };

    let lcp = longest_common_prefix(&candidates);
    let suffix = if candidates.len() == 1 && are_dirs {
        "/"
    } else {
        ""
    };

    let mut full = if dir_str == "/" {
        format!("/{lcp}{suffix}")
    } else {
        format!("{dir_str}/{lcp}{suffix}")
    };

    if had_tilde {
        if let Some(h) = home {
            let hs = h.to_string_lossy();
            let hs_trim = hs.trim_end_matches('/');
            if let Some(rest) = full.strip_prefix(hs_trim) {
                full = format!("~{rest}");
            }
        }
    }

    if full == input {
        None
    } else {
        Some(full)
    }
}

fn longest_common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let mut end = first.len();
    for s in &items[1..] {
        let mut matched = 0;
        for (a, b) in first[..end].chars().zip(s.chars()) {
            if a == b {
                matched += a.len_utf8();
            } else {
                break;
            }
        }
        end = matched;
        if end == 0 {
            break;
        }
    }
    first[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_selection_preserves_extension() {
        let mut ti = TextInput::new("untitled.pdf");
        ti.select_stem_before_dot();
        assert_eq!(ti.selected_text(), Some("untitled"));
        ti.insert("budget");
        assert_eq!(ti.as_str(), "budget.pdf");
    }

    #[test]
    fn stem_selection_selects_all_for_dotfile_or_no_extension() {
        let mut dot = TextInput::new(".gitignore");
        dot.select_stem_before_dot();
        assert_eq!(dot.selected_text(), Some(".gitignore"));

        let mut plain = TextInput::new("README");
        plain.select_stem_before_dot();
        assert_eq!(plain.selected_text(), Some("README"));
    }

    #[test]
    fn editing_and_word_navigation_with_unicode() {
        let mut ti = TextInput::new("hello café world");
        ti.move_left(true, false);
        ti.backspace(false);
        ti.insert("-");
        assert_eq!(ti.as_str(), "hello café-world");
        ti.select_all();
        assert_eq!(ti.cut().as_deref(), Some("hello café-world"));
        assert!(ti.is_empty());
    }

    #[test]
    fn path_resolution_expands_tilde_and_dotdot() {
        let home = Path::new("/home/alice");
        let cur = Path::new("/home/alice/Documents/Notes");
        assert_eq!(
            resolve_typed_path("~/Pictures/../Music", cur, Some(home)),
            PathBuf::from("/home/alice/Music")
        );
        assert_eq!(
            resolve_typed_path("sub/file.txt", cur, Some(home)),
            PathBuf::from("/home/alice/Documents/Notes/sub/file.txt")
        );
        assert_eq!(
            resolve_typed_path("~", cur, Some(home)),
            PathBuf::from("/home/alice")
        );
    }

    #[test]
    fn tab_completion_completes_unique_and_shared_prefixes() {
        let base =
            std::env::temp_dir().join(format!("infynitae-files-tab-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("Documents")).unwrap();
        fs::create_dir_all(base.join("Downloads")).unwrap();
        fs::create_dir_all(base.join("Pictures")).unwrap();
        fs::write(base.join("notes.txt"), b"hi").unwrap();

        // "Do" is already the full common prefix of Documents and Downloads -> None
        assert_eq!(complete_path("~/Do", &base, Some(&base)), None);
        let base_prefix = format!("{}/D", base.display());
        assert_eq!(
            complete_path(&base_prefix, &base, Some(&base)).as_deref(),
            Some(format!("{}/Do", base.display()).as_str())
        );

        let p_unique = complete_path("~/Pic", &base, Some(&base));
        assert_eq!(p_unique.as_deref(), Some("~/Pictures/"));

        let p_file = complete_path("~/not", &base, Some(&base));
        assert_eq!(p_file.as_deref(), Some("~/notes.txt"));

        let _ = fs::remove_dir_all(&base);
    }
}
