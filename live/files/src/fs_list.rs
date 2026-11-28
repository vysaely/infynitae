//! Asynchronous directory reader (keyed by generation number so changing folders cancels stale
//! work), `.hidden` handling, natural filename sorting, and plain date/size formatting.

use crate::mime::MimeDb;
use chrono::{Local, TimeZone};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomOrd};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub name: OsString,
    pub display_name: String,
    pub path: PathBuf,
    /// Optional subtitle shown beneath the filename in Recent view.
    pub subtitle: Option<String>,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified_secs: Option<i64>,
    pub is_hidden: bool,
    pub mime: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Modified,
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn flip(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

pub fn sort_entries(entries: &mut [FileEntry], col: SortColumn, order: SortOrder) {
    entries.sort_by(|a, b| {
        // Folders always stay above files regardless of sort column or direction.
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            let ord = match col {
                SortColumn::Name => natural_cmp(&a.display_name, &b.display_name),
                SortColumn::Modified => a
                    .modified_secs
                    .unwrap_or(0)
                    .cmp(&b.modified_secs.unwrap_or(0))
                    .then_with(|| natural_cmp(&a.display_name, &b.display_name)),
                SortColumn::Size => a
                    .size
                    .cmp(&b.size)
                    .then_with(|| natural_cmp(&a.display_name, &b.display_name)),
            };
            match order {
                SortOrder::Asc => ord,
                SortOrder::Desc => ord.reverse(),
            }
        })
    });
}

/// Natural case-insensitive filename comparison (`track 2.flac` before `track 10.flac`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ia = a.as_bytes();
    let mut ib = b.as_bytes();
    while !ia.is_empty() && !ib.is_empty() {
        if ia[0].is_ascii_digit() && ib[0].is_ascii_digit() {
            let na_end = ia
                .iter()
                .position(|c| !c.is_ascii_digit())
                .unwrap_or(ia.len());
            let nb_end = ib
                .iter()
                .position(|c| !c.is_ascii_digit())
                .unwrap_or(ib.len());
            let na_trim = &ia[..na_end];
            let nb_trim = &ib[..nb_end];
            let sa = strip_leading_zeros(na_trim);
            let sb = strip_leading_zeros(nb_trim);
            let ord = sa.len().cmp(&sb.len()).then_with(|| sa.cmp(sb));
            if ord != Ordering::Equal {
                return ord;
            }
            // Same numeric value: fewer leading zeros first for determinism.
            let ord_len = na_trim.len().cmp(&nb_trim.len());
            if ord_len != Ordering::Equal {
                return ord_len;
            }
            ia = &ia[na_end..];
            ib = &ib[nb_end..];
        } else {
            let ca = ia[0].to_ascii_lowercase();
            let cb = ib[0].to_ascii_lowercase();
            if ca != cb {
                return ca.cmp(&cb);
            }
            ia = &ia[1..];
            ib = &ib[1..];
        }
    }
    ia.len().cmp(&ib.len()).then_with(|| a.cmp(b))
}

fn strip_leading_zeros(digits: &[u8]) -> &[u8] {
    let first_nz = digits
        .iter()
        .position(|&b| b != b'0')
        .unwrap_or(digits.len().saturating_sub(1));
    &digits[first_nz..]
}

pub fn list_directory_sync(dir: &Path, mime_db: &MimeDb) -> Result<Vec<FileEntry>, String> {
    let hidden_names = read_dot_hidden(dir);
    let read_dir = fs::read_dir(dir).map_err(|e| format!("Cannot open {}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for item in read_dir {
        let Ok(entry) = item else { continue };
        let name = entry.file_name();
        let display_name = name.to_string_lossy().into_owned();
        let path = entry.path();
        let sym_meta = fs::symlink_metadata(&path).ok();
        let is_symlink = sym_meta
            .as_ref()
            .is_some_and(|m| m.file_type().is_symlink());
        let meta = if is_symlink {
            fs::metadata(&path).ok().or(sym_meta)
        } else {
            sym_meta
        };
        let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
        let size = meta
            .as_ref()
            .map_or(0, |m| if is_dir { 0 } else { m.len() });
        let modified_secs = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        let is_hidden = display_name.starts_with('.') || hidden_names.contains(&display_name);
        let mime = if is_dir {
            "inode/directory".to_string()
        } else {
            mime_db.guess_mime(&display_name)
        };
        out.push(FileEntry {
            name,
            display_name,
            path,
            subtitle: None,
            is_dir,
            is_symlink,
            size,
            modified_secs,
            is_hidden,
            mime,
        });
    }
    Ok(out)
}

pub fn entry_from_recent(
    path: PathBuf,
    home: Option<&Path>,
    mime_db: &MimeDb,
    fallback_secs: Option<i64>,
) -> Option<FileEntry> {
    let meta = fs::metadata(&path).ok()?;
    let is_dir = meta.is_dir();
    let name = path.file_name()?.to_os_string();
    let display_name = name.to_string_lossy().into_owned();
    let subtitle = path.parent().map(|p| shorten_home(p, home));
    let modified_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .or(fallback_secs);
    let mime = if is_dir {
        "inode/directory".to_string()
    } else {
        mime_db.guess_mime(&display_name)
    };
    Some(FileEntry {
        name,
        display_name,
        path,
        subtitle,
        is_dir,
        is_symlink: false,
        size: if is_dir { 0 } else { meta.len() },
        modified_secs,
        is_hidden: false,
        mime,
    })
}

pub fn shorten_home(path: &Path, home: Option<&Path>) -> String {
    if let Some(h) = home {
        if path == h {
            return "~".to_string();
        }
        if let Ok(rel) = path.strip_prefix(h) {
            return format!("~/{}", rel.to_string_lossy());
        }
    }
    path.to_string_lossy().into_owned()
}

fn read_dot_hidden(dir: &Path) -> HashSet<String> {
    let Ok(text) = fs::read_to_string(dir.join(".hidden")) else {
        return HashSet::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if b < MB {
        let v = b / KB;
        if v >= 100.0 {
            format!("{:.0} KB", v)
        } else {
            format!("{:.1} KB", v)
        }
    } else if b < GB {
        let v = b / MB;
        if v >= 100.0 {
            format!("{:.0} MB", v)
        } else {
            format!("{:.1} MB", v)
        }
    } else if b < TB {
        let v = b / GB;
        if v >= 100.0 {
            format!("{:.0} GB", v)
        } else {
            format!("{:.1} GB", v)
        }
    } else {
        format!("{:.1} TB", b / TB)
    }
}

pub fn format_modified(secs: Option<i64>) -> String {
    let Some(ts) = secs else {
        return String::new();
    };
    let Some(dt) = Local.timestamp_opt(ts, 0).single() else {
        return String::new();
    };
    let now = Local::now();
    if dt.date_naive() == now.date_naive() {
        dt.format("Today %H:%M").to_string()
    } else if dt.date_naive().succ_opt() == Some(now.date_naive()) {
        dt.format("Yesterday %H:%M").to_string()
    } else if dt.format("%Y").to_string() == now.format("%Y").to_string() {
        dt.format("%b %d %H:%M").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

pub struct DirLoaded {
    pub generation: u64,
    pub dir: PathBuf,
    pub result: Result<Vec<FileEntry>, String>,
}

/// Background worker thread reading directories off the UI thread, cancelling stale requests
/// via a shared monotonic generation counter.
pub struct DirWorker {
    tx: mpsc::Sender<(u64, PathBuf)>,
    gen: Arc<AtomicU64>,
}

impl DirWorker {
    pub fn spawn(mime_db: Arc<MimeDb>, notify: impl Fn(DirLoaded) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<(u64, PathBuf)>();
        let gen = Arc::new(AtomicU64::new(0));
        let worker_gen = Arc::clone(&gen);

        thread::spawn(move || {
            while let Ok((mut req_gen, mut dir)) = rx.recv() {
                // Drain queued directory switches so only the latest request runs.
                while let Ok((g, d)) = rx.try_recv() {
                    req_gen = g;
                    dir = d;
                }
                if worker_gen.load(AtomOrd::Relaxed) != req_gen {
                    continue;
                }
                let result = list_directory_sync(&dir, &mime_db);
                if worker_gen.load(AtomOrd::Relaxed) == req_gen {
                    notify(DirLoaded {
                        generation: req_gen,
                        dir,
                        result,
                    });
                }
            }
        });

        Self { tx, gen }
    }

    pub fn request(&self, dir: PathBuf) -> u64 {
        let g = self.gen.fetch_add(1, AtomOrd::SeqCst) + 1;
        let _ = self.tx.send((g, dir));
        g
    }

    pub fn cancel(&self) {
        self.gen.fetch_add(1, AtomOrd::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort_orders_numbers_and_keeps_folders_first() {
        let mut names = vec![
            "track 10.flac",
            "track 2.flac",
            "track 1.flac",
            "Track 3.flac",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            vec![
                "track 1.flac",
                "track 2.flac",
                "Track 3.flac",
                "track 10.flac"
            ]
        );

        let mut entries = vec![
            FileEntry {
                name: "a.txt".into(),
                display_name: "a.txt".into(),
                path: PathBuf::from("/tmp/a.txt"),
                subtitle: None,
                is_dir: false,
                is_symlink: false,
                size: 100,
                modified_secs: Some(10),
                is_hidden: false,
                mime: "text/plain".into(),
            },
            FileEntry {
                name: "z_dir".into(),
                display_name: "z_dir".into(),
                path: PathBuf::from("/tmp/z_dir"),
                subtitle: None,
                is_dir: true,
                is_symlink: false,
                size: 0,
                modified_secs: Some(5),
                is_hidden: false,
                mime: "inode/directory".into(),
            },
        ];
        sort_entries(&mut entries, SortColumn::Name, SortOrder::Asc);
        assert!(entries[0].is_dir);
        sort_entries(&mut entries, SortColumn::Name, SortOrder::Desc);
        assert!(entries[0].is_dir);
    }
}
