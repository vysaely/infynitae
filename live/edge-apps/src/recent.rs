//! The apps started from the edge, in `$XDG_STATE_HOME/infynitae/recent-apps`: one
//! "<unix seconds>\t<desktop file>" line per start, newest first. The frozen edges wrote the same
//! file with one line per app; both read the newest line of an app first.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Lines kept: the newest 200 starts.
pub const MAX_LINES: usize = 200;
/// A start's weight halves every 20 newer starts (see `scores`).
const HALF_LIFE: usize = 20;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recent {
    /// (unix seconds, desktop file name such as "firefox.desktop"), newest first
    lines: Vec<(i64, String)>,
}

#[cfg(test)]
pub static NONE: Recent = Recent { lines: Vec::new() };

pub fn path() -> Option<PathBuf> {
    crate::paths::state_dir().map(|d| d.join("infynitae/recent-apps"))
}

impl Recent {
    /// Lines that are not "<seconds>\t<name>" are skipped; past `MAX_LINES` the rest is dropped.
    pub fn parse(text: &str) -> Recent {
        let lines = text
            .lines()
            .filter_map(|l| {
                let (when, name) = l.trim_end_matches('\r').split_once('\t')?;
                let when = when.trim().parse::<i64>().ok().filter(|w| *w > 0)?;
                (!name.is_empty()).then(|| (when, name.to_string()))
            })
            .take(MAX_LINES)
            .collect();
        Recent { lines }
    }

    pub fn format(&self) -> String {
        self.lines
            .iter()
            .map(|(w, n)| format!("{w}\t{n}\n"))
            .collect()
    }

    /// The file's starts; none when it does not exist yet.
    pub fn load(path: &Path) -> std::io::Result<Recent> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Recent::parse(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Recent::default()),
            Err(e) => Err(e),
        }
    }

    /// Writes the whole file through a temporary one beside it, so a reader never sees half of it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        let res = std::fs::write(&tmp, self.format()).and_then(|()| std::fs::rename(&tmp, path));
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res
    }

    /// A start of app `id` (its desktop file name without ".desktop") at `now`, as the newest line.
    pub fn note(&mut self, id: &str, now: i64) {
        self.lines.insert(0, (now, format!("{id}.desktop")));
        self.lines.truncate(MAX_LINES);
    }

    /// How much each app was started lately, by app id: the newest start counts 1, a start's weight
    /// halves every `HALF_LIFE` newer starts, and an app's starts add up. So of two apps
    /// started once the later one leads, and an app started often gains on one started once just
    /// now: two starts 10 and 11 starts ago (0.71 + 0.68) beat it.
    pub fn scores(&self) -> HashMap<&str, f64> {
        let mut out: HashMap<&str, f64> = HashMap::new();
        for (i, (_, name)) in self.lines.iter().enumerate() {
            let id = name.strip_suffix(".desktop").unwrap_or(name);
            *out.entry(id).or_default() += 0.5f64.powf(i as f64 / HALF_LIFE as f64);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_frozen_edges_file_and_skips_bad_lines() {
        let r = Recent::parse(
            "300\tdiscord.desktop\nnot a line\n0\tzero.desktop\n200\t\n100\tfirefox.desktop\n",
        );
        assert_eq!(r.format(), "300\tdiscord.desktop\n100\tfirefox.desktop\n");
        let s = r.scores();
        assert_eq!(s.get("discord"), Some(&1.0));
        assert!(s["firefox"] < 1.0 && s["firefox"] > 0.9, "{s:?}");
    }

    #[test]
    fn a_start_goes_first_and_the_file_keeps_the_newest_200() {
        let mut r = Recent::default();
        for i in 0..MAX_LINES as i64 + 5 {
            r.note(&format!("app{i}"), 1000 + i);
        }
        r.note("foot", 5000);
        let text = r.format();
        assert_eq!(text.lines().count(), MAX_LINES);
        assert_eq!(text.lines().next(), Some("5000\tfoot.desktop"));
        assert_eq!(text.lines().nth(1), Some("1204\tapp204.desktop"));
        // the oldest went: the 6 first starts
        assert!(!text.contains("\tapp5.desktop"));
        assert!(text.contains("\tapp6.desktop"));
        assert_eq!(Recent::parse(&text), r);
    }

    #[test]
    fn more_often_and_more_lately_count_more() {
        let mut r = Recent::default();
        r.note("old", 1);
        for i in 0..25 {
            r.note(&format!("filler{i}"), 2 + i);
        }
        r.note("often", 30);
        r.note("often", 31);
        r.note("once", 32);
        let s = r.scores();
        assert!(s["often"] > s["once"], "{s:?}");
        assert!(s["once"] > s["old"], "{s:?}");
    }

    #[test]
    fn save_writes_the_whole_file_and_load_reads_it_back() {
        let dir = std::env::temp_dir().join(format!("edge-apps-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("infynitae/recent-apps");
        assert_eq!(Recent::load(&path).unwrap(), Recent::default());
        let mut r = Recent::parse("100\tfirefox.desktop\n");
        r.note("foot", 200);
        r.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "200\tfoot.desktop\n100\tfirefox.desktop\n"
        );
        assert_eq!(Recent::load(&path).unwrap(), r);
        // no temporary file left beside it
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
