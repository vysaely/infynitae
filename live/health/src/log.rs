//! Condensed recent system and Infynitae event log (`infynitae-health log`).
//!
//! Combines: app crashes (coredumps + compositor crash reports), OOM kills, failed systemd units,
//! disk-guard/btrfs warnings, GPU resets, USB/drive plug events, suspends, and updates installed.
//! Each parser takes fixture text or paths so tests run without real system logs or `journalctl`.

use crate::history;
use crate::model::{
    iso_utc, local_display_time, parse_timestamp_to_unix, HistorySample, LogEvent, LogReport,
    State, SCHEMA_VERSION,
};
use crate::sys::{read_file_tail, run_cmd, SysEnv};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

const LOG_TAIL_BYTES: usize = 512 * 1024;

pub fn collect_log(env: &SysEnv, since_unix: i64) -> LogReport {
    let mut events = Vec::new();
    let now = env.now_unix;

    // 1. Coredumps via coredumpctl (or fixture file in tests)
    let coredump_raw = if env.run_commands {
        env.program_on_path("coredumpctl").and_then(|prog| {
            let since_s = format!("@{since_unix}");
            run_cmd(
                OsStr::new(prog.as_os_str()),
                &["list", "--since", &since_s, "--no-pager", "--json=short"],
                Duration::from_secs(3),
            )
            .filter(|o| o.status == Some(0))
            .map(|o| o.stdout)
        })
    } else {
        std::fs::read_to_string(env.state_dir.join("fixture-coredumpctl.json")).ok()
    };
    if let Some(raw) = coredump_raw {
        events.extend(parse_coredumpctl_json(&raw, since_unix, now));
    }

    // 1b. Infynitae compositor crash reports (~/.local/state/infynitae/crashes/*.log)
    events.extend(parse_crash_reports_dir(
        &env.state_dir.join("crashes"),
        since_unix,
        now,
    ));

    // 2. System and user journald events (OOM, failed units, disk-guard, btrfs, GPU resets, USB, suspend)
    let journal_raw = if env.run_commands {
        query_journalctl(env, since_unix)
    } else {
        std::fs::read_to_string(env.state_dir.join("fixture-journal.jsonl")).unwrap_or_default()
    };
    if !journal_raw.is_empty() {
        events.extend(parse_journal_jsonl(&journal_raw, since_unix, now));
    }

    // 3. Infynitae automount log (drive plug/mount/unmount)
    if let Ok(raw) = read_file_tail(&env.state_dir.join("automount.log"), LOG_TAIL_BYTES) {
        events.extend(parse_automount_log(&raw, since_unix, now));
    }

    // 4. Infynitae sleep-lock log
    if let Ok(raw) = read_file_tail(&env.state_dir.join("sleep-lock.log"), LOG_TAIL_BYTES) {
        events.extend(parse_sleep_lock_log(&raw, since_unix, now));
    }

    // 5. Pacman updates installed (/var/log/pacman.log)
    if let Ok(raw) = read_file_tail(&env.var_log_dir.join("pacman.log"), LOG_TAIL_BYTES) {
        events.extend(parse_pacman_log(&raw, since_unix, now));
    }

    // 6. Infynitae install/update log
    if let Ok(raw) = read_file_tail(&env.state_dir.join("install.log"), LOG_TAIL_BYTES) {
        events.extend(parse_install_log(&raw, since_unix, now));
    }

    // 7. Health sample history disk-guard/btrfs transitions
    let samples = history::read_samples_since(&env.health_dir(), since_unix, now);
    events.extend(extract_health_disk_warnings(&samples, since_unix, now));

    let events = finalize_events(events);
    LogReport {
        schema_version: SCHEMA_VERSION,
        since_unix,
        since: iso_utc(since_unix),
        generated_unix: now,
        generated: iso_utc(now),
        events,
    }
}

fn query_journalctl(env: &SysEnv, since_unix: i64) -> String {
    let Some(jctl) = env.program_on_path("journalctl") else {
        return String::new();
    };
    let since_s = format!("@{since_unix}");
    let mut combined = String::new();
    // Filtered queries keep journalctl fast even on long-running systems with huge journals.
    let queries: &[&[&str]] = &[
        &[
            "--since",
            &since_s,
            "-o",
            "json",
            "--no-pager",
            "-n",
            "200",
            "_SYSTEMD_UNIT=systemd-oomd.service",
            "+",
            "_SYSTEMD_UNIT=systemd-sleep.service",
            "+",
            "_SYSTEMD_UNIT=systemd-suspend.service",
            "+",
            "SYSLOG_IDENTIFIER=systemd",
            "PRIORITY=3",
        ],
        &[
            "--user",
            "--since",
            &since_s,
            "-o",
            "json",
            "--no-pager",
            "-n",
            "100",
            "_SYSTEMD_USER_UNIT=infynitae-disk-guard.service",
            "+",
            "SYSLOG_IDENTIFIER=systemd",
            "PRIORITY=3",
        ],
        &[
            "-k",
            "--since",
            &since_s,
            "-o",
            "json",
            "--no-pager",
            "-n",
            "200",
            "-g",
            "Out of memory|oom-kill|BTRFS (warning|error)|No space left|GPU reset|ring .* timeout|NVRM: Xid|PM: suspend|USB disconnect|usb .*: Product:|Attached .* disk",
        ],
    ];
    for args in queries {
        if let Some(out) = run_cmd(OsStr::new(jctl.as_os_str()), args, Duration::from_secs(3)) {
            if !out.stdout.is_empty() {
                combined.push_str(&out.stdout);
                if !combined.ends_with('\n') {
                    combined.push('\n');
                }
            }
        }
    }
    combined
}

fn make_event(
    ts: i64,
    now: i64,
    kind: &str,
    severity: &str,
    source: &str,
    summary: String,
) -> LogEvent {
    LogEvent {
        timestamp_unix: ts,
        time: iso_utc(ts),
        local_time: local_display_time(ts, now),
        kind: kind.to_string(),
        severity: severity.to_string(),
        source: source.to_string(),
        summary,
    }
}

fn signal_name(sig: i64) -> String {
    match sig {
        4 => "SIGILL".into(),
        5 => "SIGTRAP".into(),
        6 => "SIGABRT".into(),
        7 => "SIGBUS".into(),
        8 => "SIGFPE".into(),
        9 => "SIGKILL".into(),
        11 => "SIGSEGV".into(),
        _ => format!("signal {sig}"),
    }
}

/// Parses `coredumpctl list --json=short` output.
pub fn parse_coredumpctl_json(text: &str, since_unix: i64, now_unix: i64) -> Vec<LogEvent> {
    let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in arr {
        let Some(us) = entry.get("time").and_then(|v| v.as_i64()) else {
            continue;
        };
        let ts = us / 1_000_000;
        if ts < since_unix {
            continue;
        }
        let exe = entry
            .get("exe")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let name = Path::new(exe)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(exe);
        let sig = entry
            .get("sig")
            .and_then(|v| v.as_i64())
            .map(signal_name)
            .unwrap_or_else(|| "crash".into());
        out.push(make_event(
            ts,
            now_unix,
            "crash",
            "problem",
            "coredumpctl",
            format!("{name} crashed ({sig})"),
        ));
    }
    out
}

/// Parses Infynitae compositor crash reports (`~/.local/state/infynitae/crashes/*.log`).
pub fn parse_crash_reports_dir(dir: &Path, since_unix: i64, now_unix: i64) -> Vec<LogEvent> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("log") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut headline = None;
        let mut date_ts = None;
        let mut panic_line = None;
        for line in text.lines().take(12) {
            if headline.is_none() && line.starts_with("Infynitae crashed") {
                headline = Some(line.trim().to_string());
            } else if let Some(d) = line.strip_prefix("Date:") {
                date_ts = parse_timestamp_to_unix(d.trim());
            } else if let Some(p) = line.strip_prefix("Panic:") {
                panic_line = Some(p.trim().to_string());
            }
        }
        let Some(ts) = date_ts else { continue };
        if ts < since_unix {
            continue;
        }
        let summary = match (headline, panic_line) {
            (Some(h), Some(p)) if !p.is_empty() => format!("{h}: {p}"),
            (Some(h), _) => h,
            _ => "Infynitae compositor crashed".to_string(),
        };
        out.push(make_event(
            ts,
            now_unix,
            "crash",
            "problem",
            "infynitae-session",
            summary,
        ));
    }
    out
}

fn json_str_field(v: &serde_json::Value, key: &str) -> Option<String> {
    let field = v.get(key)?;
    if let Some(s) = field.as_str() {
        return Some(s.to_string());
    }
    // journald emits non-UTF8 MESSAGE as a JSON byte array
    if let Some(arr) = field.as_array() {
        let bytes: Vec<u8> = arr
            .iter()
            .filter_map(|x| x.as_u64().and_then(|n| u8::try_from(n).ok()))
            .collect();
        return Some(String::from_utf8_lossy(&bytes).into_owned());
    }
    None
}

fn oomd_app_name(raw: &str) -> String {
    let mut name = raw.rsplit('/').next().unwrap_or(raw);
    for suf in [".scope", ".service", ".slice"] {
        name = name.strip_suffix(suf).unwrap_or(name);
    }
    for pre in ["app-flatpak-", "app-gnome-", "app-kde-", "app-"] {
        if let Some(r) = name.strip_prefix(pre) {
            name = r;
            break;
        }
    }
    if let Some((before, _)) = name.split_once('@') {
        name = before;
    }
    if name.is_empty() {
        "an app".into()
    } else {
        name.to_string()
    }
}

/// Parses `journalctl -o json` JSON lines into condensed `LogEvent`s.
pub fn parse_journal_jsonl(text: &str, since_unix: i64, now_unix: i64) -> Vec<LogEvent> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let ts = v
            .get("__REALTIME_TIMESTAMP")
            .and_then(|t| {
                t.as_str()
                    .and_then(|s| s.parse::<i64>().ok())
                    .or_else(|| t.as_i64())
            })
            .map(|us| us / 1_000_000);
        let Some(ts) = ts else { continue };
        if ts < since_unix {
            continue;
        }
        let Some(msg) = json_str_field(&v, "MESSAGE") else {
            continue;
        };
        let msg = msg.trim();
        if msg.is_empty() {
            continue;
        }
        let unit = json_str_field(&v, "_SYSTEMD_UNIT")
            .or_else(|| json_str_field(&v, "_SYSTEMD_USER_UNIT"))
            .unwrap_or_default();
        let transport = json_str_field(&v, "_TRANSPORT").unwrap_or_default();
        let ident = json_str_field(&v, "SYSLOG_IDENTIFIER").unwrap_or_default();

        // 1. OOM kills (systemd-oomd and kernel OOM killer)
        if unit == "systemd-oomd.service" {
            if let Some(rest) = msg.strip_prefix("Killed ") {
                let target = rest.split(" due to ").next().unwrap_or(rest);
                out.push(make_event(
                    ts,
                    now_unix,
                    "oom",
                    "problem",
                    "systemd-oomd",
                    format!("OOM killed {}", oomd_app_name(target)),
                ));
                continue;
            }
        }
        if msg.contains("Out of memory: Killed process ") {
            if let Some((_, rest)) = msg.split_once("Out of memory: Killed process ") {
                let proc_name = rest
                    .split_once('(')
                    .and_then(|(_, r)| r.split_once(')'))
                    .map(|(n, _)| n)
                    .unwrap_or("process");
                out.push(make_event(
                    ts,
                    now_unix,
                    "oom",
                    "problem",
                    "kernel",
                    format!("Kernel OOM killed {proc_name}"),
                ));
                continue;
            }
        }

        // 2. Disk guard / btrfs warnings
        if unit == "infynitae-disk-guard.service" {
            let lower = msg.to_ascii_lowercase();
            if lower.contains("reserve")
                || lower.contains("full")
                || lower.contains("low")
                || lower.contains("failed")
                || lower.contains("warning")
                || lower.contains("error")
            {
                out.push(make_event(
                    ts,
                    now_unix,
                    "disk_guard",
                    "problem",
                    "disk-guard",
                    format!("Disk guard: {msg}"),
                ));
            }
            continue;
        }
        if transport == "kernel"
            && (msg.contains("BTRFS error")
                || msg.contains("BTRFS warning")
                || msg.contains("No space left on device"))
        {
            out.push(make_event(
                ts,
                now_unix,
                "disk_guard",
                "problem",
                "kernel",
                shorten(msg, 120),
            ));
            continue;
        }

        // 3. GPU resets
        let lower = msg.to_ascii_lowercase();
        if transport == "kernel"
            && (lower.contains("gpu reset")
                || (lower.contains("amdgpu")
                    && lower.contains("ring")
                    && lower.contains("timeout"))
                || msg.contains("NVRM: Xid"))
        {
            out.push(make_event(
                ts,
                now_unix,
                "gpu_reset",
                "problem",
                "kernel",
                format!("GPU reset: {}", shorten(msg, 100)),
            ));
            continue;
        }

        // 4. Failed systemd units
        if ident == "systemd"
            && (msg.contains("Failed with result") || msg.starts_with("Failed to start "))
        {
            let failed_unit = json_str_field(&v, "UNIT")
                .or_else(|| json_str_field(&v, "USER_UNIT"))
                .unwrap_or_default();
            let summary = if !failed_unit.is_empty() && !msg.contains(&failed_unit) {
                format!("{failed_unit}: {}", shorten(msg, 90))
            } else {
                shorten(msg, 110)
            };
            out.push(make_event(
                ts,
                now_unix,
                "unit_failed",
                "problem",
                "systemd",
                summary,
            ));
            continue;
        }

        // 5. Suspends and resumes
        if msg.contains("PM: suspend entry") {
            let mode = msg
                .split_once('(')
                .and_then(|(_, r)| r.split_once(')'))
                .map(|(m, _)| format!(" ({m})"))
                .unwrap_or_default();
            out.push(make_event(
                ts,
                now_unix,
                "suspend",
                "info",
                "kernel",
                format!("System suspended{mode}"),
            ));
            continue;
        }
        if msg.contains("PM: suspend exit") {
            out.push(make_event(
                ts,
                now_unix,
                "suspend",
                "info",
                "kernel",
                "System woke from suspend".into(),
            ));
            continue;
        }
        if (unit == "systemd-suspend.service" || unit == "systemd-sleep.service")
            && (msg.contains("Entering sleep state") || msg.contains("System returned from sleep"))
        {
            let text = if msg.contains("Entering") {
                "System suspended"
            } else {
                "System woke from suspend"
            };
            out.push(make_event(
                ts,
                now_unix,
                "suspend",
                "info",
                "systemd-sleep",
                text.into(),
            ));
            continue;
        }

        // 6. USB / drive plug events
        if transport == "kernel" {
            if let Some((_, prod)) = msg.split_once(": Product: ") {
                out.push(make_event(
                    ts,
                    now_unix,
                    "usb_drive",
                    "info",
                    "kernel",
                    format!("USB plugged: {}", prod.trim()),
                ));
                continue;
            }
            if msg.contains("USB disconnect") {
                out.push(make_event(
                    ts,
                    now_unix,
                    "usb_drive",
                    "info",
                    "kernel",
                    shorten(msg, 100),
                ));
                continue;
            }
            if msg.contains("Attached ") && msg.contains(" disk") {
                out.push(make_event(
                    ts,
                    now_unix,
                    "usb_drive",
                    "info",
                    "kernel",
                    format!("Drive attached: {}", shorten(msg, 90)),
                ));
                continue;
            }
        }
    }
    out
}

/// Parses `~/.local/state/infynitae/automount.log` for mount/unmount events (skipping `ignoring`).
pub fn parse_automount_log(text: &str, since_unix: i64, now_unix: i64) -> Vec<LogEvent> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.len() < 20 {
            continue;
        }
        let Some((ts_part, msg)) = split_leading_timestamp(line) else {
            continue;
        };
        let Some(ts) = parse_timestamp_to_unix(ts_part) else {
            continue;
        };
        if ts < since_unix {
            continue;
        }
        let lower = msg.to_ascii_lowercase();
        if lower.starts_with("ignoring ")
            || lower.starts_with("scanning")
            || lower.starts_with("started")
        {
            continue;
        }
        if lower.starts_with("mounted ")
            || lower.starts_with("unmounted ")
            || lower.starts_with("removed ")
            || lower.starts_with("failed ")
        {
            let sev = if lower.starts_with("failed ") {
                "watch"
            } else {
                "info"
            };
            out.push(make_event(
                ts,
                now_unix,
                "usb_drive",
                sev,
                "automount",
                capitalize_first(msg),
            ));
        }
    }
    out
}

/// Parses `~/.local/state/infynitae/sleep-lock.log` for suspend/wake lock events.
pub fn parse_sleep_lock_log(text: &str, since_unix: i64, now_unix: i64) -> Vec<LogEvent> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.len() < 20 {
            continue;
        }
        let Some((ts_part, msg)) = split_leading_timestamp(line) else {
            continue;
        };
        let Some(ts) = parse_timestamp_to_unix(ts_part) else {
            continue;
        };
        if ts < since_unix {
            continue;
        }
        let lower = msg.to_ascii_lowercase();
        if lower.starts_with("started ") {
            continue;
        }
        if lower.contains("sleep") || lower.contains("suspend") || lower.contains("lock") {
            out.push(make_event(
                ts,
                now_unix,
                "suspend",
                "info",
                "sleep-lock",
                capitalize_first(msg),
            ));
        }
    }
    out
}

/// Condenses `/var/log/pacman.log` ALPM package transactions into one event per transaction.
pub fn parse_pacman_log(text: &str, since_unix: i64, now_unix: i64) -> Vec<LogEvent> {
    let mut out = Vec::new();
    let mut installed: Vec<String> = Vec::new();
    let mut upgraded: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut last_ts: Option<i64> = None;

    let flush = |out: &mut Vec<LogEvent>,
                 ts: Option<i64>,
                 installed: &mut Vec<String>,
                 upgraded: &mut Vec<String>,
                 removed: &mut Vec<String>| {
        if installed.is_empty() && upgraded.is_empty() && removed.is_empty() {
            return;
        }
        let Some(ts) = ts else {
            installed.clear();
            upgraded.clear();
            removed.clear();
            return;
        };
        if ts >= since_unix {
            let mut parts = Vec::new();
            if !upgraded.is_empty() {
                parts.push(format_pkg_group("Upgraded", upgraded));
            }
            if !installed.is_empty() {
                parts.push(format_pkg_group("Installed", installed));
            }
            if !removed.is_empty() {
                parts.push(format_pkg_group("Removed", removed));
            }
            out.push(make_event(
                ts,
                now_unix,
                "update",
                "info",
                "pacman",
                parts.join("; "),
            ));
        }
        installed.clear();
        upgraded.clear();
        removed.clear();
    };

    for raw in text.lines() {
        let line = raw.trim();
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((ts_str, after_ts)) = rest.split_once("] ") else {
            continue;
        };
        let Some(alpm) = after_ts.strip_prefix("[ALPM] ") else {
            continue;
        };
        let Some(ts) = parse_timestamp_to_unix(ts_str) else {
            continue;
        };
        if alpm == "transaction started" {
            flush(
                &mut out,
                last_ts,
                &mut installed,
                &mut upgraded,
                &mut removed,
            );
            last_ts = Some(ts);
            continue;
        }
        if alpm == "transaction completed" {
            last_ts = Some(ts);
            flush(
                &mut out,
                last_ts,
                &mut installed,
                &mut upgraded,
                &mut removed,
            );
            continue;
        }
        if let Some(pkg) = alpm
            .strip_prefix("upgraded ")
            .and_then(|s| s.split_whitespace().next())
        {
            upgraded.push(pkg.to_string());
            last_ts = Some(ts);
        } else if let Some(pkg) = alpm
            .strip_prefix("installed ")
            .and_then(|s| s.split_whitespace().next())
        {
            installed.push(pkg.to_string());
            last_ts = Some(ts);
        } else if let Some(pkg) = alpm
            .strip_prefix("removed ")
            .and_then(|s| s.split_whitespace().next())
        {
            removed.push(pkg.to_string());
            last_ts = Some(ts);
        }
    }
    flush(
        &mut out,
        last_ts,
        &mut installed,
        &mut upgraded,
        &mut removed,
    );
    out
}

fn format_pkg_group(verb: &str, pkgs: &[String]) -> String {
    let n = pkgs.len();
    let word = if n == 1 { "package" } else { "packages" };
    let sample = if n <= 3 {
        pkgs.join(", ")
    } else {
        format!("{}, +{} more", pkgs[..3].join(", "), n - 3)
    };
    format!("{verb} {n} {word} ({sample})")
}

/// Parses `~/.local/state/infynitae/install.log` for Infynitae update runs.
pub fn parse_install_log(text: &str, since_unix: i64, now_unix: i64) -> Vec<LogEvent> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let Some(rest) = raw.trim().strip_prefix("==> ") else {
            continue;
        };
        let Some((ts_part, msg)) = split_leading_timestamp(rest) else {
            continue;
        };
        let Some(ts) = parse_timestamp_to_unix(ts_part) else {
            continue;
        };
        if ts < since_unix {
            continue;
        }
        if msg.starts_with("Updating Infynitae") {
            out.push(make_event(
                ts,
                now_unix,
                "update",
                "info",
                "infynitae-update",
                "Infynitae updated".into(),
            ));
        } else if msg.starts_with("Infynitae install") {
            out.push(make_event(
                ts,
                now_unix,
                "update",
                "info",
                "infynitae-install",
                "Infynitae installed".into(),
            ));
        }
    }
    out
}

/// Extracts disk/btrfs warning/problem transitions recorded in the 60 s health history.
pub fn extract_health_disk_warnings(
    samples: &[HistorySample],
    since_unix: i64,
    now_unix: i64,
) -> Vec<LogEvent> {
    let mut out = Vec::new();
    let mut active: HashSet<String> = HashSet::new();
    for sample in samples {
        let mut next_active = HashSet::new();
        for alert in &sample.alerts {
            if (!alert.id.starts_with("disk:") && !alert.id.starts_with("btrfs:"))
                || alert.state == State::Ok
            {
                continue;
            }
            next_active.insert(alert.id.clone());
            if sample.timestamp_unix >= since_unix && !active.contains(&alert.id) {
                let sev = if alert.state == State::Problem {
                    "problem"
                } else {
                    "watch"
                };
                out.push(make_event(
                    sample.timestamp_unix,
                    now_unix,
                    "disk_guard",
                    sev,
                    "health",
                    alert.reason.clone(),
                ));
            }
        }
        active = next_active;
    }
    out
}

fn split_leading_timestamp(line: &str) -> Option<(&str, &str)> {
    // Expects "YYYY-MM-DD HH:MM:SS[.fff] message"
    if line.len() < 20 {
        return None;
    }
    let b = line.as_bytes();
    if b[4] != b'-' || b[7] != b'-' || b[10] != b' ' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let mut end = 19;
    if line.as_bytes().get(end) == Some(&b'.') {
        end += 1;
        while line.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
    }
    let rest = line[end..].trim();
    (!rest.is_empty()).then_some((&line[..end], rest))
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn shorten(s: &str, max_chars: usize) -> String {
    let single = s.replace(['\r', '\n', '\t'], " ");
    let trimmed = single.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let cut: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Deduplicates identical events within 2 seconds and sorts strictly newest-first.
pub fn finalize_events(mut events: Vec<LogEvent>) -> Vec<LogEvent> {
    events.sort_by(|a, b| {
        b.timestamp_unix
            .cmp(&a.timestamp_unix)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.summary.cmp(&b.summary))
    });
    let mut out: Vec<LogEvent> = Vec::with_capacity(events.len());
    for ev in events {
        let dup = out.iter().rev().take(8).any(|prev| {
            prev.kind == ev.kind
                && prev.summary == ev.summary
                && (prev.timestamp_unix - ev.timestamp_unix).abs() <= 2
        });
        if !dup {
            out.push(ev);
        }
    }
    out
}

pub fn render_log_text(report: &LogReport) -> String {
    if report.events.is_empty() {
        return format!("No events since {}\n", report.since);
    }
    let mut out = String::new();
    for ev in &report.events {
        out.push_str(&format!(
            "{}  {:<11}  {}\n",
            ev.local_time, ev.kind, ev.summary
        ));
    }
    out
}
