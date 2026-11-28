//! Portal `filters` and `current_filter` matching: glob (`kind == 0`, case-sensitive) and MIME
//! (`kind == 1`, wildcards + `/usr/share/mime/subclasses` inheritance). Directories always stay
//! visible so the user can navigate.

use crate::mime::{glob_match, MimeDb};
use crate::portal::protocol::{FilterPattern, PortalFilter};

#[derive(Clone, Debug, Default)]
pub struct FilterSet {
    pub filters: Vec<PortalFilter>,
    pub active: usize,
}

impl FilterSet {
    /// Builds the picker's filter list from `options.filters` and `options.current_filter`.
    /// When at least one filter is supplied, always appends an `All files` (`*`) fallback unless
    /// a wildcard filter is already present.
    pub fn from_request(
        mut filters: Vec<PortalFilter>,
        current_filter: Option<PortalFilter>,
    ) -> Self {
        let mut active = 0usize;
        if let Some(cf) = current_filter {
            if let Some(pos) = filters.iter().position(|f| f == &cf) {
                active = pos;
            } else {
                filters.insert(0, cf);
                active = 0;
            }
        }
        if !filters.is_empty() && !filters.iter().any(is_catch_all) {
            filters.push(PortalFilter {
                label: "All files".to_string(),
                patterns: vec![FilterPattern {
                    kind: 0,
                    pattern: "*".to_string(),
                }],
            });
        }
        Self { filters, active }
    }

    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    pub fn active_filter(&self) -> Option<&PortalFilter> {
        self.filters.get(self.active)
    }

    pub fn set_active(&mut self, index: usize) -> bool {
        if index < self.filters.len() && index != self.active {
            self.active = index;
            true
        } else {
            false
        }
    }

    pub fn matches_entry(
        &self,
        filename: &str,
        file_mime: &str,
        is_dir: bool,
        mime_db: &MimeDb,
    ) -> bool {
        if is_dir {
            return true;
        }
        let Some(flt) = self.active_filter() else {
            return true;
        };
        if flt.patterns.is_empty() {
            return true;
        }
        flt.patterns.iter().any(|p| match p.kind {
            0 => glob_match(&p.pattern, filename),
            1 => mime_db.mime_matches(file_mime, &p.pattern),
            _ => false,
        })
    }
}

fn is_catch_all(f: &PortalFilter) -> bool {
    f.patterns.iter().any(|p| {
        (p.kind == 0 && (p.pattern == "*" || p.pattern == "*.*"))
            || (p.kind == 1 && (p.pattern == "*/*" || p.pattern == "application/octet-stream"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_all_files_and_matches_globs_and_mimes() {
        let db = MimeDb::parse(
            "50:image/png:*.png\n50:text/markdown:*.md\n",
            "text/markdown text/plain\n",
            "",
        );
        let images = PortalFilter {
            label: "Images".into(),
            patterns: vec![
                FilterPattern {
                    kind: 0,
                    pattern: "*.png".into(),
                },
                FilterPattern {
                    kind: 0,
                    pattern: "*.[jJ][pP][gG]".into(),
                },
            ],
        };
        let text = PortalFilter {
            label: "Text files".into(),
            patterns: vec![FilterPattern {
                kind: 1,
                pattern: "text/plain".into(),
            }],
        };
        let mut set = FilterSet::from_request(vec![images.clone(), text.clone()], Some(text));
        assert_eq!(set.filters.len(), 3);
        assert_eq!(set.active, 1);
        assert_eq!(set.filters[2].label, "All files");

        // Active filter is text/plain: matches markdown via subclass, always matches dirs.
        assert!(set.matches_entry("notes.md", "text/markdown", false, &db));
        assert!(!set.matches_entry("shot.png", "image/png", false, &db));
        assert!(set.matches_entry("Photos", "inode/directory", true, &db));

        // Switch to Images (case-sensitive glob).
        assert!(set.set_active(0));
        assert!(set.matches_entry("shot.png", "image/png", false, &db));
        assert!(!set.matches_entry("shot.PNG", "image/png", false, &db));
        assert!(set.matches_entry("photo.JpG", "image/jpeg", false, &db));

        // Switch to All files.
        assert!(set.set_active(2));
        assert!(set.matches_entry("anything.bin", "application/octet-stream", false, &db));
    }

    #[test]
    fn empty_filters_stays_empty_and_accepts_everything() {
        let db = MimeDb::default();
        let set = FilterSet::from_request(Vec::new(), None);
        assert!(set.is_empty());
        assert!(set.matches_entry("foo.bin", "application/octet-stream", false, &db));
    }
}
