//! 60 s JSONL history storage, 7-day rotation, CPU delta persistence, and one-shot problem
//! transition notifications.

use crate::model::{date_utc, unix_from_civil, HealthItem, HealthSnapshot, HistorySample, State};
use crate::sys::{read_file_tail, run_cmd, SysEnv};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const KEEP_DAYS: i64 = 7;

/// Daily JSONL file name for `ts_unix` (UTC): `YYYY-MM-DD.jsonl`.
pub fn jsonl_path(dir: &Path, ts_unix: i64) -> PathBuf {
    dir.join(format!("{}.jsonl", date_utc(ts_unix)))
}

/// Parses a `YYYY-MM-DD.jsonl` file name into UTC midnight Unix seconds.
pub fn parse_jsonl_date(name: &str) -> Option<i64> {
    let stem = name.strip_suffix(".jsonl")?;
    if stem.len() != 10 {
        return None;
    }
    let b = stem.as_bytes();
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = stem[0..4].parse().ok()?;
    let m: u32 = stem[5..7].parse().ok()?;
    let d: u32 = stem[8..10].parse().ok()?;
    unix_from_civil(y, m, d, 0, 0, 0)
}

/// Appends one compact `HistorySample` JSON line for `snapshot` and deletes daily files older than
/// `keep_days`.
pub fn append_and_rotate(
    dir: &Path,
    snapshot: &HealthSnapshot,
    keep_days: i64,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = jsonl_path(dir, snapshot.timestamp_unix);
    let compact = HistorySample::from_snapshot(snapshot);
    let mut line = serde_json::to_string(&compact)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    rotate_old_files(dir, snapshot.timestamp_unix, keep_days);
    Ok(path)
}

/// Deletes `YYYY-MM-DD.jsonl` files strictly older than `keep_days` full UTC days before `now_unix`.
pub fn rotate_old_files(dir: &Path, now_unix: i64, keep_days: i64) -> usize {
    let today_midnight = now_unix - now_unix.rem_euclid(86_400);
    let cutoff = today_midnight - (keep_days.max(1) - 1) * 86_400;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let Some(file_midnight) = parse_jsonl_date(name_str) else {
            continue;
        };
        if file_midnight < cutoff && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Reads stored history samples from `dir` with `timestamp_unix >= since_unix`, oldest first.
/// For short windows (<= 2 h, used by the 60 s sampler), reads only the last 256 KiB of each file
/// so per-sample cost stays O(1) even near the end of the day.
pub fn read_samples_since(dir: &Path, since_unix: i64, now_unix: i64) -> Vec<HistorySample> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let start_day = since_unix - since_unix.rem_euclid(86_400);
    let end_day = now_unix - now_unix.rem_euclid(86_400);
    let mut files: Vec<(i64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let day = parse_jsonl_date(name.to_str()?)?;
            (day >= start_day && day <= end_day + 86_400).then_some((day, e.path()))
        })
        .collect();
    files.sort_by_key(|(d, _)| *d);

    let short_window = now_unix.saturating_sub(since_unix) <= 7200;
    let mut out = Vec::new();
    for (_, path) in files {
        let text = if short_window {
            read_file_tail(&path, 256 * 1024)
        } else {
            std::fs::read_to_string(&path)
        };
        let Ok(text) = text else {
            continue;
        };
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(sample) = serde_json::from_str::<HistorySample>(trimmed) {
                if sample.timestamp_unix >= since_unix && sample.timestamp_unix <= now_unix + 60 {
                    out.push(sample);
                }
            }
        }
    }
    out.sort_by_key(|s| s.timestamp_unix);
    out
}

/// Reads the most recent valid sample from the latest `.jsonl` file in `dir`.
#[allow(dead_code)]
pub fn read_latest_sample(dir: &Path) -> Option<HistorySample> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut files: Vec<(i64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let day = parse_jsonl_date(name.to_str()?)?;
            Some((day, e.path()))
        })
        .collect();
    files.sort_by_key(|(d, _)| std::cmp::Reverse(*d));
    for (_, path) in files {
        let Ok(tail) = read_file_tail(&path, 32 * 1024) else {
            continue;
        };
        for line in tail.lines().rev() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(sample) = serde_json::from_str::<HistorySample>(trimmed) {
                return Some(sample);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Persisted previous /proc/stat totals for zero-sleep 60 s CPU %

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuPrev {
    pub timestamp_unix: i64,
    pub idle: u64,
    pub total: u64,
}

pub fn read_cpu_prev(dir: &Path) -> Option<CpuPrev> {
    let text = std::fs::read_to_string(dir.join("cpu-prev")).ok()?;
    let mut parts = text.split_whitespace();
    let timestamp_unix = parts.next()?.parse::<i64>().ok()?;
    let idle = parts.next()?.parse::<u64>().ok()?;
    let total = parts.next()?.parse::<u64>().ok()?;
    Some(CpuPrev {
        timestamp_unix,
        idle,
        total,
    })
}

pub fn write_cpu_prev(dir: &Path, prev: CpuPrev) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("cpu-prev.tmp.{}", std::process::id()));
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    writeln!(f, "{} {} {}", prev.timestamp_unix, prev.idle, prev.total)?;
    drop(f);
    std::fs::rename(&tmp, dir.join("cpu-prev"))
}

// ---------------------------------------------------------------------------------------------
// One-shot notifications on transition into State::Problem

pub fn read_notified_problems(dir: &Path) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(dir.join("notified.json")) else {
        return BTreeSet::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn write_notified_problems(dir: &Path, ids: &BTreeSet<String>) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("notified.json.tmp.{}", std::process::id()));
    let body = serde_json::to_string(ids)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(body.as_bytes())?;
    f.write_all(b"\n")?;
    drop(f);
    std::fs::rename(&tmp, dir.join("notified.json"))
}

/// Computes newly-entering `Problem` items and updates `notified` to the current problem set.
/// An item staying in `Problem` is not returned again; leaving `Problem` clears it so a later
/// occurrence notifies once.
pub fn transition_problems<'a>(
    notified: &mut BTreeSet<String>,
    items: &'a [HealthItem],
) -> Vec<&'a HealthItem> {
    let mut current = BTreeSet::new();
    let mut newly = Vec::new();
    for item in items {
        if item.state == State::Problem {
            current.insert(item.id.clone());
            if !notified.contains(&item.id) {
                newly.push(item);
            }
        }
    }
    *notified = current;
    newly
}

/// Persists transition state and sends one desktop notification per item that just turned Problem.
/// If notification delivery fails (e.g. D-Bus unavailable), the failed item is omitted from the
/// persisted set so the next tick retries once the session is ready.
pub fn notify_new_problems(env: &SysEnv, items: &[HealthItem]) -> Vec<String> {
    let dir = env.health_dir();
    let mut notified = read_notified_problems(&dir);
    let newly: Vec<HealthItem> = transition_problems(&mut notified, items)
        .into_iter()
        .cloned()
        .collect();
    if newly.is_empty() {
        let _ = write_notified_problems(&dir, &notified);
        return Vec::new();
    }

    let notify_prog = std::env::var_os("INFYNITAE_HEALTH_NOTIFY_SEND")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env.program_on_path("notify-send"));

    let mut sent = Vec::new();
    for item in &newly {
        let Some(prog) = notify_prog.as_deref() else {
            notified.remove(&item.id);
            continue;
        };
        let title = format!("Health: {}", item.label);
        let ok = run_cmd(
            OsStr::new(prog.as_os_str()),
            &["-a", "Infynitae", "-u", "critical", &title, &item.reason],
            Duration::from_secs(3),
        )
        .is_some_and(|out| out.status == Some(0));
        if ok {
            sent.push(item.id.clone());
        } else {
            notified.remove(&item.id);
        }
    }
    let _ = write_notified_problems(&dir, &notified);
    sent
}
