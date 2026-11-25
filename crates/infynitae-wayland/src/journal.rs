//! The desktop journal (owner 2026-11-24: "i just need to know what happened with my pc where and
//! how"): one line per security-relevant event the compositor sees, in
//! `$XDG_STATE_HOME/infynitae/journal/journal.tsv`, never sent anywhere. `infynitae msg journal`
//! prints it; Infynitae's helpers (the lock screen, the password prompt, the automount and session
//! scripts) append their own lines to the same file.
//!
//! A line is six fields separated by tabs: the time (UTC, `2026-11-25T14:03:07Z`), the event, the
//! program as the trust code knows it (`Identity::describe`: the executable's path, or the
//! sandbox's app id), its pid, a detail and the outcome. Tab-separated so a C helper writes one
//! with a single `dprintf` and a shell script with `printf`, without a JSON encoder; a tab, newline
//! or backslash inside a field is written as `\t`, `\n` or `\\`. An empty field is unknown.
//!
//! The compositor writes from a thread of its own, fed by a bounded channel: an event costs the
//! loop a send, never a write. When the file would pass `FILE_CAP` it becomes `journal.1.tsv`
//! (the one before is deleted), so the journal holds at most twice the cap.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// The largest the current file grows before it is rotated, in bytes. A line is about 150 bytes,
/// so one file holds about 7000 events.
pub const FILE_CAP: u64 = 1 << 20;

/// Events waiting for the writer; more are counted and written as one `journal-dropped` line.
const QUEUE: usize = 256;

/// The file the journal is written to in `dir`.
pub fn current_file(dir: &Path) -> PathBuf {
    dir.join("journal.tsv")
}

/// The file the last rotation moved the journal to.
pub fn older_file(dir: &Path) -> PathBuf {
    dir.join("journal.1.tsv")
}

/// `$XDG_STATE_HOME/infynitae/journal`, else `~/.local/state/infynitae/journal`.
pub fn default_dir() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(state.join("infynitae/journal"))
}

/// One event, before its time is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub event: &'static str,
    pub program: String,
    pub pid: Option<i32>,
    pub detail: String,
    pub outcome: &'static str,
}

enum Cmd {
    Line(SystemTime, Entry, usize),
    Flush(SyncSender<()>),
}

#[derive(Default)]
struct Inner {
    sender: Option<SyncSender<Cmd>>,
    dropped: usize,
    /// (program, what): the refusals written this session (`refused_once`).
    refused: HashSet<(String, &'static str)>,
}

/// The journal, shared by every part of the compositor that writes to it. Until `open` it writes
/// nothing (a test that sets no folder, the compositor before its session starts).
#[derive(Clone, Default)]
pub struct Journal(Arc<Mutex<Inner>>);

impl Journal {
    /// Starts the writer for `dir` (made when missing). Fails when the folder or file cannot be made.
    pub fn open(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let file = open_append(&current_file(dir))?;
        let (tx, rx) = sync_channel(QUEUE);
        let dir = dir.to_path_buf();
        std::thread::Builder::new()
            .name("infynitae-journal".into())
            .spawn(move || writer_loop(dir, file, rx))?;
        if let Ok(mut inner) = self.0.lock() {
            inner.sender = Some(tx);
        }
        Ok(())
    }

    /// Queues `entry`; never waits. A full queue counts it as dropped.
    pub fn write(&self, entry: Entry) {
        let Ok(mut inner) = self.0.lock() else {
            return;
        };
        let Some(tx) = inner.sender.clone() else {
            return;
        };
        let dropped = std::mem::take(&mut inner.dropped);
        match tx.try_send(Cmd::Line(SystemTime::now(), entry, dropped)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                inner.dropped = dropped + 1;
            }
        }
    }

    /// Queues a refusal of `what` to `program` the first time this session; later ones are the
    /// same fact again (a client is asked about a global each time it lists them).
    pub fn refused_once(&self, entry: Entry, what: &'static str) {
        let first = self.0.lock().is_ok_and(|mut inner| {
            inner.sender.is_some() && inner.refused.insert((entry.program.clone(), what))
        });
        if first {
            self.write(entry);
        }
    }

    /// Waits until every queued line is in the file.
    pub fn flush(&self) {
        let tx = self.0.lock().ok().and_then(|inner| inner.sender.clone());
        if let Some(tx) = tx {
            let (ack_tx, ack_rx) = sync_channel(0);
            if tx.send(Cmd::Flush(ack_tx)).is_ok() {
                let _ = ack_rx.recv();
            }
        }
    }
}

fn open_append(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

fn writer_loop(dir: PathBuf, mut file: std::fs::File, rx: Receiver<Cmd>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Line(time, entry, dropped) => {
                let mut text = String::new();
                if dropped > 0 {
                    text.push_str(&line(
                        time,
                        &Entry {
                            event: "journal-dropped",
                            program: String::new(),
                            pid: None,
                            detail: format!("{dropped} events not written: the queue was full"),
                            outcome: "lost",
                        },
                    ));
                }
                text.push_str(&line(time, &entry));
                // the helpers append to the same file: its length on disk, not a count of ours
                let len = file.metadata().map_or(0, |m| m.len());
                if len > 0 && len + text.len() as u64 > FILE_CAP {
                    match rotate(&dir) {
                        Ok(f) => file = f,
                        Err(e) => {
                            crate::log_line!("[infynitae] Journal: rotation failed: {e}")
                        }
                    }
                }
                if let Err(e) = file.write_all(text.as_bytes()) {
                    crate::log_line!("[infynitae] Journal: write failed: {e}");
                }
            }
            Cmd::Flush(ack) => {
                let _ = ack.send(());
            }
        }
    }
}

fn rotate(dir: &Path) -> std::io::Result<std::fs::File> {
    std::fs::rename(current_file(dir), older_file(dir))?;
    open_append(&current_file(dir))
}

/// The line for `entry` at `time`, with its newline.
pub fn line(time: SystemTime, entry: &Entry) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\n",
        iso_utc(time),
        entry.event,
        escape(&entry.program),
        entry.pid.map(|p| p.to_string()).unwrap_or_default(),
        escape(&entry.detail),
        entry.outcome
    )
}

/// A field with its tabs, newlines and backslashes written as escapes: one event stays one line.
pub fn escape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    for c in field.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// `time` as `YYYY-MM-DDTHH:MM:SSZ`. UTC: the C helpers and scripts write it the same way
/// (`gmtime_r`, `date -u`), and in UTC the lines sort and compare as text (`--since`).
pub fn iso_utc(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // days since 1970-01-01 to a civil date (Howard Hinnant's `civil_from_days`)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// Whether a journal line is kept by `infynitae msg journal`: written at or after `since` (a
/// prefix of the time, `2026-11-25` or `2026-11-25T14`) and with `app` in its program field.
pub fn matches(line: &str, since: Option<&str>, app: Option<&str>) -> bool {
    let mut fields = line.split('\t');
    let time = fields.next().unwrap_or("");
    let program = fields.nth(1).unwrap_or("");
    since.is_none_or(|s| time >= s) && app.is_none_or(|a| program.contains(a))
}

/// A client as the journal names it: its program (`Identity::describe`) and pid. A connection with
/// no identity is the compositor's own (an internal socket pair).
pub fn who(client: &smithay::reexports::wayland_server::Client) -> (String, Option<i32>) {
    let state = client.get_data::<crate::ClientState>();
    match state.and_then(|s| s.identity()) {
        Some(identity) => (identity.describe(), state.and_then(|s| s.pid())),
        None => (
            std::env::current_exe()
                .map(|p| format!("{} (own connection)", p.display()))
                .unwrap_or_else(|_| "the compositor (own connection)".into()),
            Some(std::process::id() as i32),
        ),
    }
}

impl crate::WaylandState {
    /// Starts writing the journal to `dir` (a real session: `default_dir`; a test: its own folder).
    pub fn open_journal(&mut self, dir: &Path) -> std::io::Result<()> {
        self.journal.open(dir)
    }

    /// The journal (a test flushes it before reading the file).
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// Writes an event done by `client`.
    pub(crate) fn journal_client(
        &self,
        event: &'static str,
        client: &smithay::reexports::wayland_server::Client,
        detail: String,
        outcome: &'static str,
    ) {
        let (program, pid) = who(client);
        self.journal.write(Entry {
            event,
            program,
            pid,
            detail,
            outcome,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn entry(detail: &str) -> Entry {
        Entry {
            event: "capture-start",
            program: "/usr/bin/grim".into(),
            pid: Some(42),
            detail: detail.into(),
            outcome: "allowed",
        }
    }

    #[test]
    fn times_are_utc_in_iso_form() {
        let at = |s| iso_utc(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        // 2026-11-25 14:03:07 UTC
        assert_eq!(at(1_795_615_387), "2026-11-25T14:03:07Z");
        // a leap day
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn a_line_is_six_fields_and_a_field_cannot_break_it() {
        let l = line(SystemTime::UNIX_EPOCH, &entry("output\tDP-1\nnext \\"));
        assert_eq!(
            l,
            "1970-01-01T00:00:00Z\tcapture-start\t/usr/bin/grim\t42\toutput\\tDP-1\\nnext \\\\\tallowed\n"
        );
        assert_eq!(l.trim_end().split('\t').count(), 6);
        let unknown = Entry {
            pid: None,
            ..entry("")
        };
        assert_eq!(
            line(SystemTime::UNIX_EPOCH, &unknown),
            "1970-01-01T00:00:00Z\tcapture-start\t/usr/bin/grim\t\t\tallowed\n"
        );
    }

    #[test]
    fn the_reader_filters_by_time_and_program() {
        let l = "2026-11-25T14:03:07Z\tcapture-start\t/usr/bin/grim\t42\toutput DP-1\tallowed";
        assert!(matches(l, None, None));
        assert!(matches(l, Some("2026-11-25"), Some("grim")));
        assert!(matches(l, Some("2026-11-25T14:03:07Z"), None));
        assert!(!matches(l, Some("2026-11-25T15"), None));
        assert!(!matches(l, None, Some("obs")));
        // the detail is not the program
        assert!(!matches(l, None, Some("DP-1")));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("infynitae-journal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn nothing_is_written_before_it_is_opened_and_each_event_once_after() {
        let dir = temp_dir("open");
        let j = Journal::default();
        j.write(entry("before"));
        j.open(&dir).unwrap();
        j.write(entry("after"));
        j.flush();
        let text = std::fs::read_to_string(current_file(&dir)).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.contains("\tafter\t"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_refusal_is_written_once_per_program_and_global() {
        let dir = temp_dir("refused");
        let j = Journal::default();
        j.open(&dir).unwrap();
        for _ in 0..3 {
            j.refused_once(entry("lock"), "ALLOW_LOCK");
        }
        j.refused_once(entry("capture"), "ALLOW_CAPTURE");
        j.flush();
        let text = std::fs::read_to_string(current_file(&dir)).unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_full_file_is_rotated_and_the_journal_stays_under_twice_the_cap() {
        let dir = temp_dir("rotate");
        let j = Journal::default();
        j.open(&dir).unwrap();
        let big = "x".repeat(10_000);
        // about 2.5 caps of lines, flushed in steps so the queue never overflows
        for i in 0..260 {
            j.write(entry(&big));
            if i % 50 == 0 {
                j.flush();
            }
        }
        j.flush();
        let cur = std::fs::metadata(current_file(&dir)).unwrap().len();
        let old = std::fs::metadata(older_file(&dir)).unwrap().len();
        assert!(cur <= FILE_CAP && old <= FILE_CAP, "{cur} {old}");
        assert!(old > FILE_CAP - 11_000, "the older file was full: {old}");
        assert!(
            !std::fs::read_to_string(current_file(&dir))
                .unwrap()
                .contains("journal-dropped")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
