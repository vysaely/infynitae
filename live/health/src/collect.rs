//! System health snapshot collectors, parsers, and threshold evaluation (`infynitae-health now`).

use crate::history::{self, CpuPrev};
use crate::model::{
    format_bytes, iso_utc, local_display_time, parse_timestamp_to_unix, round1, round2,
    BtrfsMetadataInfo, CompositorCounters, CompositorInfo, CpuInfo, DiskInfo, GpuInfo, HealthItem,
    HealthSnapshot, HistorySample, MemoryInfo, NvmeInfo, PressureInfo, PsiLine, PsiResource, State,
    SwapDeviceInfo, SystemdInfo, UpdatesInfo, SCHEMA_VERSION,
};
use crate::sys::{read_file_tail, run_cmd, statvfs_sizes, SysEnv};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * 1024 * 1024;
const SESSION_LOG_TAIL_BYTES: usize = 2 * 1024 * 1024;
/// A slow compositor turn of at least one 60 Hz frame is counted as a noticeable stall.
pub const STALL_THRESHOLD_MS: f64 = 16.0;
const UPDATE_CACHE_MAX_AGE_SECS: i64 = 6 * 3600;

/// Collects a full health snapshot using real `statvfs`.
pub fn collect_snapshot(env: &SysEnv) -> HealthSnapshot {
    let history_samples =
        history::read_samples_since(&env.health_dir(), env.now_unix - 3600, env.now_unix);
    collect_snapshot_with(env, &history_samples, statvfs_sizes)
}

/// Collects a full health snapshot with injectable history samples and `statvfs` (for tests).
pub fn collect_snapshot_with<F>(
    env: &SysEnv,
    history_samples: &[HistorySample],
    statvfs_fn: F,
) -> HealthSnapshot
where
    F: Fn(&Path) -> Option<(u64, u64)>,
{
    let cpu_start = read_proc_stat_totals(&env.proc_dir);

    let disks = collect_disks(env, &statvfs_fn);
    let nvme = collect_nvme(env);
    let memory = collect_memory(env);
    let gpu = collect_gpu(env);
    let systemd = collect_systemd(env);
    let compositor = collect_compositor(env, history_samples);
    let updates = collect_updates(env);
    let cpu = collect_cpu(env, cpu_start);

    let items = build_items(
        &disks,
        &nvme,
        &memory,
        &cpu,
        gpu.as_ref(),
        &systemd,
        &compositor,
        &updates,
    );
    let state = items
        .iter()
        .fold(State::Ok, |acc, item| acc.worst(item.state));

    HealthSnapshot {
        schema_version: SCHEMA_VERSION,
        timestamp_unix: env.now_unix,
        time: iso_utc(env.now_unix),
        state,
        items,
        disks,
        nvme,
        memory,
        cpu,
        gpu,
        systemd,
        compositor,
        updates,
    }
}

// ---------------------------------------------------------------------------------------------
// Disks and btrfs metadata

pub fn unescape_octal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let mut octal = String::new();
            for _ in 0..3 {
                if let Some(&n) = chars.peek() {
                    if n.is_digit(8) {
                        octal.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
            }
            if octal.len() == 3 {
                if let Ok(byte) = u8::from_str_radix(&octal, 8) {
                    out.push(byte as char);
                    continue;
                }
            }
            out.push('\\');
            out.push_str(&octal);
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountEntry {
    pub mount_point: String,
    pub fs_type: String,
    pub device: String,
}

fn is_real_disk_fs(fs_type: &str, device: &str, mount_point: &str) -> bool {
    if mount_point.starts_with("/run/credentials/")
        || mount_point.starts_with("/proc")
        || mount_point.starts_with("/sys")
        || mount_point.starts_with("/dev")
    {
        return false;
    }
    let pseudo = matches!(
        fs_type,
        "proc"
            | "sysfs"
            | "devtmpfs"
            | "tmpfs"
            | "devpts"
            | "cgroup"
            | "cgroup2"
            | "efivarfs"
            | "securityfs"
            | "pstore"
            | "bpf"
            | "autofs"
            | "tracefs"
            | "debugfs"
            | "configfs"
            | "fusectl"
            | "mqueue"
            | "hugetlbfs"
            | "binfmt_misc"
            | "overlay"
            | "squashfs"
            | "nsfs"
            | "rpc_pipefs"
            | "rootfs"
            | "iso9660"
            | "fuse.gvfsd-fuse"
            | "fuse.portal"
            | "ramfs"
    );
    if pseudo {
        return false;
    }
    device.starts_with("/dev/")
        || matches!(
            fs_type,
            "btrfs"
                | "ext4"
                | "ext3"
                | "ext2"
                | "xfs"
                | "vfat"
                | "exfat"
                | "ntfs"
                | "ntfs3"
                | "f2fs"
                | "zfs"
        )
}

/// Parses `/proc/self/mountinfo` into unique physical disk mounts (deduplicating btrfs subvolumes
/// mounted from the same block device, keeping `/` first or the shortest mount path).
pub fn parse_mountinfo(text: &str) -> Vec<MountEntry> {
    let mut by_device: HashMap<String, MountEntry> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 8 {
            continue;
        }
        let mount_point = unescape_octal(tokens[4]);
        let Some(sep) = tokens.iter().position(|&t| t == "-") else {
            continue;
        };
        let (Some(&fs_type), Some(&device_raw)) = (tokens.get(sep + 1), tokens.get(sep + 2)) else {
            continue;
        };
        let device = unescape_octal(device_raw);
        if !is_real_disk_fs(fs_type, &device, &mount_point) {
            continue;
        }
        let key = format!("{fs_type}\t{device}");
        let entry = MountEntry {
            mount_point,
            fs_type: fs_type.to_string(),
            device,
        };
        match by_device.get(&key) {
            None => {
                order.push(key.clone());
                by_device.insert(key, entry);
            }
            Some(existing) => {
                let replace = entry.mount_point == "/"
                    || (existing.mount_point != "/"
                        && (entry.mount_point.len() < existing.mount_point.len()
                            || (entry.mount_point.len() == existing.mount_point.len()
                                && entry.mount_point < existing.mount_point)));
                if replace {
                    by_device.insert(key, entry);
                }
            }
        }
    }
    let mut out: Vec<MountEntry> = order
        .into_iter()
        .filter_map(|k| by_device.remove(&k))
        .collect();
    out.sort_by(|a, b| {
        let a_root = a.mount_point == "/";
        let b_root = b.mount_point == "/";
        b_root
            .cmp(&a_root)
            .then_with(|| a.mount_point.cmp(&b.mount_point))
    });
    out
}

/// Reads `x-gvfs-name=` labels from `/etc/fstab` keyed by mount point.
pub fn parse_fstab_names(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let mp = unescape_octal(parts[1]);
        for opt in parts[3].split(',') {
            if let Some(name) = opt.strip_prefix("x-gvfs-name=") {
                let decoded = unescape_octal(name);
                if !decoded.is_empty() {
                    map.insert(mp.clone(), decoded);
                }
            }
        }
    }
    map
}

fn read_u64_file(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn find_btrfs_sysfs(btrfs_root: &Path, device: &str) -> Option<(String, PathBuf)> {
    let entries = std::fs::read_dir(btrfs_root).ok()?;
    let mut dev_names = Vec::new();
    if let Some(base) = Path::new(device).file_name().and_then(|s| s.to_str()) {
        dev_names.push(base.to_string());
    }
    if let Ok(canon) = std::fs::canonicalize(device) {
        if let Some(base) = canon.file_name().and_then(|s| s.to_str()) {
            if !dev_names.iter().any(|d| d == base) {
                dev_names.push(base.to_string());
            }
        }
    }
    let mut dirs: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let p = e.path();
            p.join("allocation").is_dir().then_some((name, p))
        })
        .collect();
    dirs.sort_by(|a, b| a.0.cmp(&b.0));

    for (uuid, dir) in dirs {
        for base in &dev_names {
            if dir.join("devices").join(base).exists() {
                return Some((uuid, dir));
            }
        }
    }
    None
}

/// Evaluates btrfs metadata health from `/sys/fs/btrfs/<uuid>`.
pub fn read_btrfs_metadata(
    btrfs_dir: &Path,
    uuid: &str,
    mount: &str,
    fs_total_bytes: Option<u64>,
) -> Option<(Option<String>, BtrfsMetadataInfo)> {
    let mut dev_bytes = 0u64;
    let mut dev_count = 0usize;
    if let Ok(devs) = std::fs::read_dir(btrfs_dir.join("devices")) {
        for entry in devs.flatten() {
            if let Some(sectors) = read_u64_file(&entry.path().join("size")) {
                dev_bytes = dev_bytes.saturating_add(sectors.saturating_mul(512));
                dev_count += 1;
            }
        }
    }
    if dev_count == 0 {
        return None;
    }
    let alloc = btrfs_dir.join("allocation");
    let d_tot = read_u64_file(&alloc.join("data/disk_total"))?;
    let m_tot = read_u64_file(&alloc.join("metadata/disk_total"))?;
    let s_tot = read_u64_file(&alloc.join("system/disk_total"))?;
    let m_bytes = read_u64_file(&alloc.join("metadata/total_bytes"))?;
    let m_used = read_u64_file(&alloc.join("metadata/bytes_used"))?;
    let rsv = read_u64_file(&alloc.join("global_rsv_size")).unwrap_or(0);
    let m_chunk = read_u64_file(&alloc.join("metadata/chunk_size"))
        .or_else(|| read_u64_file(&alloc.join("data/chunk_size")))
        .unwrap_or(GIB);
    let d_logical = read_u64_file(&alloc.join("data/total_bytes"));

    let mut effective_dev_bytes = dev_bytes;
    // Match infynitae-disk-guard: a single-device btrfs with a 4 GiB resize reserve has
    // statfs total < devices/*/size.
    if dev_count == 1 && d_logical == Some(d_tot) {
        if let Some(fs_tot) = fs_total_bytes {
            if fs_tot > 0 && fs_tot < effective_dev_bytes {
                effective_dev_bytes = fs_tot;
            }
        }
    }

    let allocated = d_tot.saturating_add(m_tot).saturating_add(s_tot);
    let unallocated_bytes = effective_dev_bytes.saturating_sub(allocated);
    let metadata_free_bytes = m_bytes.saturating_sub(m_used).saturating_sub(rsv);
    let metadata_used_pct = if m_bytes > 0 {
        round1(((m_used as f64 / m_bytes as f64) * 100.0).clamp(0.0, 100.0))
    } else {
        0.0
    };
    let needed_for_chunk = m_chunk.saturating_mul(2).max(2 * GIB);
    let can_allocate_chunk = unallocated_bytes >= needed_for_chunk;

    let (state, reason) = evaluate_btrfs_metadata(
        mount,
        metadata_used_pct,
        metadata_free_bytes,
        unallocated_bytes,
        can_allocate_chunk,
    );

    let label = std::fs::read_to_string(btrfs_dir.join("label"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Some((
        label,
        BtrfsMetadataInfo {
            uuid: uuid.to_string(),
            metadata_total_bytes: m_bytes,
            metadata_used_bytes: m_used,
            global_rsv_bytes: rsv,
            metadata_free_bytes,
            metadata_used_pct,
            unallocated_bytes,
            chunk_size_bytes: m_chunk,
            can_allocate_chunk,
            state,
            reason,
        },
    ))
}

pub fn evaluate_btrfs_metadata(
    mount: &str,
    metadata_used_pct: f64,
    metadata_free_bytes: u64,
    unallocated_bytes: u64,
    can_allocate_chunk: bool,
) -> (State, String) {
    let free_s = format_bytes(metadata_free_bytes);
    let unalloc_s = format_bytes(unallocated_bytes);
    if !can_allocate_chunk && (metadata_used_pct >= 90.0 || metadata_free_bytes < 256 * MIB) {
        (
            State::Problem,
            format!(
                "btrfs metadata on {mount} is {metadata_used_pct:.0}% full ({free_s} free, {unalloc_s} unallocated): a full one froze the PC on Nov 21"
            ),
        )
    } else if !can_allocate_chunk && (metadata_used_pct >= 75.0 || metadata_free_bytes < GIB) {
        (
            State::Watch,
            format!(
                "btrfs metadata on {mount} is {metadata_used_pct:.0}% full ({free_s} free, {unalloc_s} unallocated): watch above 75% or under 1 GB free when unallocated space is low"
            ),
        )
    } else {
        (
            State::Ok,
            format!(
                "btrfs metadata on {mount} is {metadata_used_pct:.0}% used ({free_s} free, {unalloc_s} unallocated)"
            ),
        )
    }
}

pub fn evaluate_disk_space(mount: &str, total_bytes: u64, available_bytes: u64) -> (State, String) {
    if total_bytes == 0 {
        return (
            State::Ok,
            format!("{mount} size could not be read from statvfs"),
        );
    }
    let free_pct = (available_bytes as f64 / total_bytes as f64) * 100.0;
    let free_s = format_bytes(available_bytes);
    let total_s = format_bytes(total_bytes);
    // A tiny partition (a USB stick's boot part, an EFI part) is full by design: never a warning.
    if total_bytes < 256 * MIB {
        return (State::Ok, format!("{mount} has {free_s} free of {total_s}"));
    }
    let is_small = total_bytes < 20 * GIB;
    let problem_rule = if is_small {
        "problem below 5% free"
    } else {
        "problem below 5% or 5 GB free"
    };
    let watch_rule = if is_small {
        "watch below 15% free"
    } else {
        "watch below 15% or 15 GB free"
    };
    if free_pct < 5.0 || (!is_small && available_bytes < 5 * GIB) {
        (
            State::Problem,
            format!("{mount} has {free_s} free ({free_pct:.0}% of {total_s}): {problem_rule}"),
        )
    } else if free_pct < 15.0 || (!is_small && available_bytes < 15 * GIB) {
        (
            State::Watch,
            format!("{mount} has {free_s} free ({free_pct:.0}% of {total_s}): {watch_rule}"),
        )
    } else {
        (
            State::Ok,
            format!("{mount} has {free_s} free of {total_s} ({free_pct:.0}% free)"),
        )
    }
}

pub fn collect_disks<F>(env: &SysEnv, statvfs_fn: &F) -> Vec<DiskInfo>
where
    F: Fn(&Path) -> Option<(u64, u64)>,
{
    let mountinfo = std::fs::read_to_string(env.proc_dir.join("self/mountinfo"))
        .or_else(|_| std::fs::read_to_string(env.proc_dir.join("mountinfo")))
        .unwrap_or_default();
    let fstab = std::fs::read_to_string(env.etc_dir.join("fstab")).unwrap_or_default();
    let fstab_names = parse_fstab_names(&fstab);
    let btrfs_root = env.sys_dir.join("fs/btrfs");

    let mut out = Vec::new();
    for m in parse_mountinfo(&mountinfo) {
        let Some((total_bytes, available_bytes)) = statvfs_fn(Path::new(&m.mount_point)) else {
            continue;
        };
        if total_bytes == 0 {
            continue;
        }
        let used_pct = round1(
            (((total_bytes.saturating_sub(available_bytes)) as f64 / total_bytes as f64) * 100.0)
                .clamp(0.0, 100.0),
        );
        let (state, reason) = evaluate_disk_space(&m.mount_point, total_bytes, available_bytes);
        let mut label = fstab_names.get(&m.mount_point).cloned();
        let btrfs = if m.fs_type == "btrfs" {
            find_btrfs_sysfs(&btrfs_root, &m.device).and_then(|(uuid, dir)| {
                let (btrfs_label, meta) =
                    read_btrfs_metadata(&dir, &uuid, &m.mount_point, Some(total_bytes))?;
                if label.is_none() {
                    label = btrfs_label;
                }
                Some(meta)
            })
        } else {
            None
        };
        out.push(DiskInfo {
            mount: m.mount_point,
            device: m.device,
            fs_type: m.fs_type,
            label,
            total_bytes,
            available_bytes,
            used_pct,
            state,
            reason,
            btrfs,
        });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// NVMe SMART

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NvmeSmartParsed {
    pub wear_pct: Option<u8>,
    pub data_written_bytes: Option<u64>,
    pub temp_c: Option<i32>,
    pub critical_warning: Option<u64>,
    pub media_errors: Option<u64>,
    pub permission_denied: bool,
}

/// Parses `smartctl -j -a /dev/nvmeN` JSON output.
pub fn parse_smartctl_nvme_json(text: &str) -> Option<NvmeSmartParsed> {
    let val: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut out = NvmeSmartParsed::default();
    if let Some(msgs) = val.pointer("/smartctl/messages").and_then(|v| v.as_array()) {
        for msg in msgs {
            if let Some(s) = msg.get("string").and_then(|v| v.as_str()) {
                let lower = s.to_ascii_lowercase();
                if lower.contains("permission denied") || lower.contains("operation not permitted")
                {
                    out.permission_denied = true;
                }
            }
        }
    }
    if let Some(log) = val.get("nvme_smart_health_information_log") {
        out.wear_pct = log
            .get("percentage_used")
            .and_then(|v| v.as_u64())
            .map(|n| n.min(255) as u8);
        // NVMe spec: data_units_written is in units of 1000 * 512 bytes (512,000 bytes).
        out.data_written_bytes = log
            .get("data_units_written")
            .and_then(|v| v.as_u64())
            .map(|u| u.saturating_mul(512_000));
        out.temp_c = log
            .get("temperature")
            .and_then(|v| v.as_i64())
            .map(|t| t as i32);
        out.critical_warning = log.get("critical_warning").and_then(|v| v.as_u64());
        out.media_errors = log.get("media_errors").and_then(|v| v.as_u64());
    }
    Some(out)
}

/// Parses `nvme smart-log /dev/nvmeN -o json` output.
pub fn parse_nvme_cli_json(text: &str) -> Option<NvmeSmartParsed> {
    let val: serde_json::Value = serde_json::from_str(text).ok()?;
    let wear_pct = val
        .get("percent_used")
        .and_then(|v| v.as_u64())
        .map(|n| n.min(255) as u8);
    let data_written_bytes = val
        .get("data_units_written")
        .and_then(|v| v.as_u64())
        .map(|u| u.saturating_mul(512_000));
    let temp_c = val.get("temperature").and_then(|v| v.as_i64()).map(|t| {
        if t > 200 {
            (t - 273) as i32
        } else {
            t as i32
        }
    });
    let critical_warning = val.get("critical_warning").and_then(|v| v.as_u64());
    let media_errors = val.get("media_errors").and_then(|v| v.as_u64());
    if wear_pct.is_none() && data_written_bytes.is_none() && critical_warning.is_none() {
        return None;
    }
    Some(NvmeSmartParsed {
        wear_pct,
        data_written_bytes,
        temp_c,
        critical_warning,
        media_errors,
        permission_denied: false,
    })
}

pub fn evaluate_nvme(
    device: &str,
    readable: bool,
    smart: Option<&NvmeSmartParsed>,
) -> (State, String) {
    if !readable {
        return (
            State::Ok,
            format!(
                "NVMe {device} SMART needs permission (cannot read /dev/{device} without root)"
            ),
        );
    }
    let Some(s) = smart else {
        return (
            State::Ok,
            format!("NVMe {device} SMART data was not available"),
        );
    };
    if s.critical_warning.is_some_and(|w| w > 0) {
        return (
            State::Problem,
            format!(
                "NVMe {device} reports a SMART critical warning ({}): back up the drive",
                s.critical_warning.unwrap()
            ),
        );
    }
    if s.media_errors.is_some_and(|e| e > 0) {
        return (
            State::Problem,
            format!(
                "NVMe {device} has {} media errors: problem above 0",
                s.media_errors.unwrap()
            ),
        );
    }
    if let Some(wear) = s.wear_pct {
        let written = s
            .data_written_bytes
            .map(|b| format!(", {} written", format_bytes(b)))
            .unwrap_or_default();
        if wear >= 90 {
            return (
                State::Problem,
                format!("NVMe {device} wear at {wear}%{written}: problem at 90% or above"),
            );
        }
        if wear >= 80 {
            return (
                State::Watch,
                format!("NVMe {device} wear at {wear}%{written}: watch at 80%, problem at 90%"),
            );
        }
        return (State::Ok, format!("NVMe {device} wear at {wear}%{written}"));
    }
    (State::Ok, format!("NVMe {device} SMART readable"))
}

pub fn collect_nvme(env: &SysEnv) -> Vec<NvmeInfo> {
    let nvme_class = env.sys_dir.join("class/nvme");
    let Ok(entries) = std::fs::read_dir(&nvme_class) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_str()?.to_string();
            n.starts_with("nvme").then_some(n)
        })
        .collect();
    names.sort();

    let mut out = Vec::new();
    for name in names {
        let sys_dev = nvme_class.join(&name);
        let model = std::fs::read_to_string(sys_dev.join("model"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let hwmon_temp =
            read_hwmon_temp(&sys_dev).or_else(|| read_hwmon_temp(&sys_dev.join("hwmon")));
        let dev_path = env.dev_dir.join(&name);

        // Fixture JSON next to sysfs (`smartctl.json`) lets tests exercise SMART parsing without
        // spawning external commands or touching real `/dev/nvme*`.
        let fixture_json = std::fs::read_to_string(sys_dev.join("smartctl.json")).ok();
        let (readable, smart) = if let Some(raw) = fixture_json {
            match parse_smartctl_nvme_json(&raw) {
                Some(parsed) if parsed.permission_denied => (false, None),
                Some(parsed) => (true, Some(parsed)),
                None => match parse_nvme_cli_json(&raw) {
                    Some(parsed) => (true, Some(parsed)),
                    None => (true, None),
                },
            }
        } else {
            // Check device node readability first so unprivileged 60 s samples fork nothing.
            let can_open = std::fs::File::open(&dev_path).is_ok();
            if !can_open {
                (false, None)
            } else if env.run_commands {
                let smart = query_nvme_smart(env, &dev_path);
                if smart.as_ref().is_some_and(|s| s.permission_denied) {
                    (false, None)
                } else {
                    (true, smart)
                }
            } else {
                (true, None)
            }
        };

        let (state, reason) = evaluate_nvme(&name, readable, smart.as_ref());
        let temp_c = smart.as_ref().and_then(|s| s.temp_c).or(hwmon_temp);
        out.push(NvmeInfo {
            device: name,
            model,
            readable,
            permission_note: (!readable).then(|| "needs permission".to_string()),
            wear_pct: smart.as_ref().and_then(|s| s.wear_pct),
            data_written_bytes: smart.as_ref().and_then(|s| s.data_written_bytes),
            temp_c,
            critical_warning: smart.as_ref().and_then(|s| s.critical_warning),
            media_errors: smart.as_ref().and_then(|s| s.media_errors),
            state,
            reason,
        });
    }
    out
}

fn query_nvme_smart(env: &SysEnv, dev_path: &Path) -> Option<NvmeSmartParsed> {
    let dev_s = dev_path.to_str()?;
    if let Some(smartctl) = env.program_on_path("smartctl") {
        if let Some(out) = run_cmd(
            OsStr::new(smartctl.as_os_str()),
            &["-j", "-a", dev_s],
            Duration::from_secs(3),
        ) {
            if let Some(parsed) = parse_smartctl_nvme_json(&out.stdout) {
                return Some(parsed);
            }
        }
    }
    if let Some(nvme) = env.program_on_path("nvme") {
        if let Some(out) = run_cmd(
            OsStr::new(nvme.as_os_str()),
            &["smart-log", dev_s, "-o", "json"],
            Duration::from_secs(3),
        ) {
            if let Some(parsed) = parse_nvme_cli_json(&out.stdout) {
                return Some(parsed);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Memory, swap (zswap + disk), PSI

fn parse_meminfo_kb(line: &str) -> Option<u64> {
    line.split_whitespace().nth(1)?.parse::<u64>().ok()
}

pub fn parse_meminfo(text: &str) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let Some((key, _)) = line.split_once(':') else {
            continue;
        };
        if let Some(kb) = parse_meminfo_kb(line) {
            map.insert(key.trim().to_string(), kb.saturating_mul(1024));
        }
    }
    map
}

pub fn parse_proc_swaps(text: &str) -> Vec<SwapDeviceInfo> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || (idx == 0 && trimmed.starts_with("Filename")) {
            continue;
        }
        let cols: Vec<&str> = trimmed.split_whitespace().collect();
        if cols.len() < 5 {
            continue;
        }
        let Ok(size_kb) = cols[2].parse::<u64>() else {
            continue;
        };
        let Ok(used_kb) = cols[3].parse::<u64>() else {
            continue;
        };
        let priority = cols[4].parse::<i32>().unwrap_or(0);
        out.push(SwapDeviceInfo {
            path: cols[0].to_string(),
            swap_type: cols[1].to_string(),
            size_bytes: size_kb.saturating_mul(1024),
            used_bytes: used_kb.saturating_mul(1024),
            priority,
        });
    }
    out
}

pub fn parse_psi_file(text: &str) -> Option<PsiResource> {
    let mut some = None;
    let mut full = None;
    for raw in text.lines() {
        let line = raw.trim();
        let (kind, rest) = if let Some(r) = line.strip_prefix("some ") {
            ("some", r)
        } else if let Some(r) = line.strip_prefix("full ") {
            ("full", r)
        } else {
            continue;
        };
        let mut avg10 = None;
        let mut avg60 = None;
        let mut avg300 = None;
        let mut total_us = None;
        for tok in rest.split_whitespace() {
            let Some((k, v)) = tok.split_once('=') else {
                continue;
            };
            match k {
                "avg10" => avg10 = v.parse::<f64>().ok().filter(|n| n.is_finite()),
                "avg60" => avg60 = v.parse::<f64>().ok().filter(|n| n.is_finite()),
                "avg300" => avg300 = v.parse::<f64>().ok().filter(|n| n.is_finite()),
                "total" => total_us = v.parse::<u64>().ok(),
                _ => {}
            }
        }
        if let (Some(a10), Some(a60), Some(a300), Some(tot)) = (avg10, avg60, avg300, total_us) {
            let parsed = PsiLine {
                avg10: round2(a10),
                avg60: round2(a60),
                avg300: round2(a300),
                total_us: tot,
            };
            if kind == "some" {
                some = Some(parsed);
            } else {
                full = Some(parsed);
            }
        }
    }
    if some.is_none() && full.is_none() {
        None
    } else {
        Some(PsiResource { some, full })
    }
}

pub fn collect_memory(env: &SysEnv) -> MemoryInfo {
    let meminfo_raw = std::fs::read_to_string(env.proc_dir.join("meminfo")).unwrap_or_default();
    let m = parse_meminfo(&meminfo_raw);
    let ram_total_bytes = m.get("MemTotal").copied().unwrap_or(0);
    let ram_available_bytes = m.get("MemAvailable").copied().unwrap_or_else(|| {
        m.get("MemFree")
            .copied()
            .unwrap_or(0)
            .saturating_add(m.get("Buffers").copied().unwrap_or(0))
            .saturating_add(m.get("Cached").copied().unwrap_or(0))
    });
    let ram_used_bytes = ram_total_bytes.saturating_sub(ram_available_bytes);
    let ram_used_pct = if ram_total_bytes > 0 {
        round1(((ram_used_bytes as f64 / ram_total_bytes as f64) * 100.0).clamp(0.0, 100.0))
    } else {
        0.0
    };

    let swap_total_bytes = m.get("SwapTotal").copied().unwrap_or(0);
    let swap_free_bytes = m.get("SwapFree").copied().unwrap_or(0);
    let swap_used_bytes = swap_total_bytes.saturating_sub(swap_free_bytes);
    let swap_used_pct = if swap_total_bytes > 0 {
        round1(((swap_used_bytes as f64 / swap_total_bytes as f64) * 100.0).clamp(0.0, 100.0))
    } else {
        0.0
    };
    let zswap_pool_bytes = m.get("Zswap").copied().unwrap_or(0);
    let zswapped_bytes = m.get("Zswapped").copied().unwrap_or(0);
    let swap_disk_used_bytes = swap_used_bytes.saturating_sub(zswapped_bytes);

    let zswap_dir = env.sys_dir.join("module/zswap/parameters");
    let zswap_enabled = std::fs::read_to_string(zswap_dir.join("enabled"))
        .ok()
        .is_some_and(|s| matches!(s.trim(), "Y" | "y" | "1" | "true"));
    let zswap_compressor = std::fs::read_to_string(zswap_dir.join("compressor"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let swaps_raw = std::fs::read_to_string(env.proc_dir.join("swaps")).unwrap_or_default();
    let swap_devices = parse_proc_swaps(&swaps_raw);

    let psi_dir = env.proc_dir.join("pressure");
    let mem_psi = std::fs::read_to_string(psi_dir.join("memory"))
        .ok()
        .as_deref()
        .and_then(parse_psi_file);
    let cpu_psi = std::fs::read_to_string(psi_dir.join("cpu"))
        .ok()
        .as_deref()
        .and_then(parse_psi_file);
    let io_psi = std::fs::read_to_string(psi_dir.join("io"))
        .ok()
        .as_deref()
        .and_then(parse_psi_file);
    let pressure = if mem_psi.is_some() || cpu_psi.is_some() || io_psi.is_some() {
        Some(PressureInfo {
            memory: mem_psi,
            cpu: cpu_psi,
            io: io_psi,
        })
    } else {
        None
    };

    MemoryInfo {
        ram_total_bytes,
        ram_used_bytes,
        ram_available_bytes,
        ram_used_pct,
        swap_total_bytes,
        swap_used_bytes,
        swap_disk_used_bytes,
        swap_used_pct,
        zswap_enabled,
        zswap_compressor,
        zswap_pool_bytes,
        zswapped_bytes,
        swap_devices,
        pressure,
    }
}

// ---------------------------------------------------------------------------------------------
// CPU

pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|l| {
        let t = l.trim_start();
        t == "cpu" || t.starts_with("cpu ") || t.starts_with("cpu\t")
    })?;
    let mut nums = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(|s| s.parse::<u64>().ok());
    let user = nums.next()??;
    let nice = nums.next()??;
    let system = nums.next()??;
    let idle = nums.next()??;
    let iowait = nums.next().flatten().unwrap_or(0);
    let irq = nums.next().flatten().unwrap_or(0);
    let softirq = nums.next().flatten().unwrap_or(0);
    let steal = nums.next().flatten().unwrap_or(0);
    let total = user
        .saturating_add(nice)
        .saturating_add(system)
        .saturating_add(idle)
        .saturating_add(iowait)
        .saturating_add(irq)
        .saturating_add(softirq)
        .saturating_add(steal);
    Some((idle.saturating_add(iowait), total))
}

pub fn cpu_percent(prev: (u64, u64), curr: (u64, u64)) -> Option<u8> {
    let d_total = curr.1.checked_sub(prev.1).filter(|&d| d > 0)?;
    let d_idle = curr.0.checked_sub(prev.0)?.min(d_total);
    let busy = d_total - d_idle;
    Some(
        ((busy as f64 / d_total as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8,
    )
}

pub fn parse_loadavg(text: &str) -> Option<(f64, f64, f64)> {
    let mut parts = text.split_whitespace();
    let a = parts
        .next()?
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())?;
    let b = parts
        .next()?
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())?;
    let c = parts
        .next()?
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())?;
    Some((round2(a), round2(b), round2(c)))
}

pub fn parse_hwmon_temp(raw: &str) -> Option<i32> {
    raw.trim().parse::<i64>().ok().map(|n| {
        if n.abs() >= 1000 {
            let half = if n >= 0 { 500 } else { -500 };
            ((n + half) / 1000) as i32
        } else {
            n as i32
        }
    })
}

pub fn find_hwmon_dir(hwmon_parent: &Path, expected_name: Option<&str>) -> Option<PathBuf> {
    let entries = std::fs::read_dir(hwmon_parent).ok()?;
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|s| s.starts_with("hwmon"))
        })
        .collect();
    dirs.sort();
    for dir in dirs {
        if let Some(expected) = expected_name {
            let Ok(name) = std::fs::read_to_string(dir.join("name")) else {
                continue;
            };
            if !name.trim().eq_ignore_ascii_case(expected) {
                continue;
            }
        }
        return Some(dir);
    }
    None
}

pub fn read_hwmon_temp(hwmon_parent: &Path) -> Option<i32> {
    let dir = find_hwmon_dir(hwmon_parent, None)?;
    let t1 = dir.join("temp1_input");
    if let Ok(content) = std::fs::read_to_string(&t1) {
        if let Some(temp) = parse_hwmon_temp(&content) {
            return Some(temp);
        }
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|s| s.starts_with("temp") && s.ends_with("_input"))
        })
        .collect();
    files.sort();
    for f in files {
        if let Ok(content) = std::fs::read_to_string(f) {
            if let Some(temp) = parse_hwmon_temp(&content) {
                return Some(temp);
            }
        }
    }
    None
}

pub fn read_cpu_temp(hwmon_dir: &Path) -> Option<i32> {
    for driver in ["k10temp", "coretemp", "zenpower", "acpitz"] {
        if let Some(dir) = find_hwmon_dir(hwmon_dir, Some(driver)) {
            if let Ok(content) = std::fs::read_to_string(dir.join("temp1_input")) {
                if let Some(temp) = parse_hwmon_temp(&content) {
                    return Some(temp);
                }
            }
        }
    }
    None
}

fn read_proc_stat_totals(proc_dir: &Path) -> Option<(u64, u64)> {
    let raw = std::fs::read_to_string(proc_dir.join("stat")).ok()?;
    parse_proc_stat(&raw)
}

pub fn collect_cpu(env: &SysEnv, start_totals: Option<(u64, u64)>) -> CpuInfo {
    let health_dir = env.health_dir();
    let prev = history::read_cpu_prev(&health_dir);
    let busy_pct = if env.sample_mode {
        if let Some(curr) = start_totals {
            let pct = prev.and_then(|p| cpu_percent((p.idle, p.total), curr));
            let _ = history::write_cpu_prev(
                &health_dir,
                CpuPrev {
                    timestamp_unix: env.now_unix,
                    idle: curr.0,
                    total: curr.1,
                },
            );
            pct
        } else {
            None
        }
    } else if let Some(first) = start_totals {
        // Prefer a second live reading; wait briefly only if other collection finished too fast.
        let mut second = read_proc_stat_totals(&env.proc_dir);
        if env.run_commands && second == Some(first) {
            std::thread::sleep(Duration::from_millis(60));
            second = read_proc_stat_totals(&env.proc_dir);
        }
        second.and_then(|s| cpu_percent(first, s)).or_else(|| {
            prev.filter(|p| (env.now_unix - p.timestamp_unix).abs() <= 180)
                .and_then(|p| cpu_percent((p.idle, p.total), first))
        })
    } else {
        None
    };

    let loadavg = std::fs::read_to_string(env.proc_dir.join("loadavg"))
        .ok()
        .as_deref()
        .and_then(parse_loadavg);
    let temp_c = read_cpu_temp(&env.sys_dir.join("class/hwmon"));

    let (state, reason) = evaluate_cpu(busy_pct, temp_c, loadavg.map(|l| l.0));
    CpuInfo {
        busy_pct,
        temp_c,
        load_1m: loadavg.map(|l| l.0),
        load_5m: loadavg.map(|l| l.1),
        load_15m: loadavg.map(|l| l.2),
        state,
        reason,
    }
}

pub fn evaluate_cpu(
    busy_pct: Option<u8>,
    temp_c: Option<i32>,
    load_1m: Option<f64>,
) -> (State, String) {
    if let Some(t) = temp_c {
        if t >= 90 {
            return (
                State::Problem,
                format!("CPU temperature is {t}°C: problem at 90°C or above"),
            );
        }
        if t >= 80 {
            return (
                State::Watch,
                format!("CPU temperature is {t}°C: watch at 80°C, problem at 90°C"),
            );
        }
    }
    if let Some(busy) = busy_pct {
        if busy >= 95 {
            return (
                State::Watch,
                format!("CPU load is {busy}%: watch at 95% or above"),
            );
        }
    }
    let mut parts = Vec::new();
    if let Some(busy) = busy_pct {
        parts.push(format!("{busy}% load"));
    }
    if let Some(t) = temp_c {
        parts.push(format!("{t}°C"));
    }
    if parts.is_empty() {
        if let Some(l1) = load_1m {
            parts.push(format!("load {l1:.2}"));
        }
    }
    let msg = if parts.is_empty() {
        "CPU metrics unavailable".to_string()
    } else {
        format!("CPU {}", parts.join(", "))
    };
    (State::Ok, msg)
}

// ---------------------------------------------------------------------------------------------
// GPU (DRM sysfs + hwmon power & temperatures)

fn lookup_amdgpu_ids(ids_text: &str, dev: &str, rev: &str) -> Option<String> {
    let dev_clean = dev
        .trim()
        .strip_prefix("0x")
        .unwrap_or(dev.trim())
        .to_ascii_uppercase();
    let rev_clean = rev
        .trim()
        .strip_prefix("0x")
        .unwrap_or(rev.trim())
        .to_ascii_uppercase();
    if dev_clean.is_empty() {
        return None;
    }
    for line in ids_text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = t.splitn(3, ',').collect();
        if parts.len() == 3
            && parts[0].trim().eq_ignore_ascii_case(&dev_clean)
            && parts[1].trim().eq_ignore_ascii_case(&rev_clean)
        {
            let name = parts[2].trim();
            return Some(name.strip_prefix("AMD ").unwrap_or(name).trim().to_string());
        }
    }
    None
}

fn find_drm_card_device(drm_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(drm_dir).ok()?;
    let mut cards: Vec<(u32, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(s) = name.to_str() else { continue };
        if let Some(num_str) = s.strip_prefix("card") {
            if !num_str.is_empty() && num_str.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(num) = num_str.parse::<u32>() {
                    cards.push((num, entry.path()));
                }
            }
        }
    }
    cards.sort_by_key(|(n, _)| *n);
    for (_, card) in cards {
        let d = card.join("device");
        if !d.is_dir() {
            continue;
        }
        if d.join("gpu_busy_percent").is_file()
            || d.join("mem_info_vram_used").is_file()
            || find_hwmon_dir(&d.join("hwmon"), None).is_some()
        {
            return Some(d);
        }
    }
    None
}

fn parse_microwatts(raw: &str) -> Option<f64> {
    let uw = raw.trim().parse::<u64>().ok()?;
    Some(round1(uw as f64 / 1_000_000.0))
}

pub fn collect_gpu(env: &SysEnv) -> Option<GpuInfo> {
    let d = find_drm_card_device(&env.sys_dir.join("class/drm"))?;
    let busy_pct = std::fs::read_to_string(d.join("gpu_busy_percent"))
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|n| n.min(100) as u8);
    let vram_used_bytes = read_u64_file(&d.join("mem_info_vram_used"));
    let vram_total_bytes = read_u64_file(&d.join("mem_info_vram_total"));

    let mut name = std::fs::read_to_string(d.join("product_name"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if name.is_none() {
        let dev = std::fs::read_to_string(d.join("device")).unwrap_or_default();
        let rev = std::fs::read_to_string(d.join("revision")).unwrap_or_default();
        if let Ok(ids) = std::fs::read_to_string(&env.amdgpu_ids) {
            name = lookup_amdgpu_ids(&ids, &dev, &rev);
        }
    }

    let mut temp_c = None;
    let mut junction_temp_c = None;
    let mut mem_temp_c = None;
    let mut power_watts = None;
    let mut power_cap_watts = None;

    if let Some(hw) = find_hwmon_dir(&d.join("hwmon"), None) {
        temp_c = std::fs::read_to_string(hw.join("temp1_input"))
            .ok()
            .as_deref()
            .and_then(parse_hwmon_temp);
        for idx in 1..=4 {
            let label = std::fs::read_to_string(hw.join(format!("temp{idx}_label")))
                .ok()
                .map(|s| s.trim().to_ascii_lowercase());
            let val = std::fs::read_to_string(hw.join(format!("temp{idx}_input")))
                .ok()
                .as_deref()
                .and_then(parse_hwmon_temp);
            if let (Some(lbl), Some(v)) = (label, val) {
                if lbl == "edge" && temp_c.is_none() {
                    temp_c = Some(v);
                } else if lbl == "junction" || lbl == "hotspot" {
                    junction_temp_c = Some(v);
                } else if lbl == "mem" {
                    mem_temp_c = Some(v);
                }
            }
        }
        power_watts = std::fs::read_to_string(hw.join("power1_average"))
            .or_else(|_| std::fs::read_to_string(hw.join("power1_input")))
            .ok()
            .as_deref()
            .and_then(parse_microwatts);
        power_cap_watts = std::fs::read_to_string(hw.join("power1_cap"))
            .ok()
            .as_deref()
            .and_then(parse_microwatts);
    }

    let power_pct_of_cap = match (power_watts, power_cap_watts) {
        (Some(w), Some(cap)) if cap > 0.0 => Some(round1((w / cap) * 100.0)),
        _ => None,
    };

    let (state, reason) = evaluate_gpu(
        busy_pct,
        temp_c,
        junction_temp_c,
        power_watts,
        power_cap_watts,
        power_pct_of_cap,
    );

    Some(GpuInfo {
        name,
        busy_pct,
        vram_used_bytes,
        vram_total_bytes,
        temp_c,
        junction_temp_c,
        mem_temp_c,
        power_watts,
        power_cap_watts,
        power_pct_of_cap,
        state,
        reason,
    })
}

pub fn evaluate_gpu(
    busy_pct: Option<u8>,
    temp_c: Option<i32>,
    junction_temp_c: Option<i32>,
    power_watts: Option<f64>,
    power_cap_watts: Option<f64>,
    power_pct_of_cap: Option<f64>,
) -> (State, String) {
    if temp_c.is_some_and(|t| t >= 90) || junction_temp_c.is_some_and(|j| j >= 105) {
        let t_s = temp_c.map(|t| format!("{t}°C")).unwrap_or_default();
        let j_s = junction_temp_c
            .map(|j| format!(" (hotspot {j}°C)"))
            .unwrap_or_default();
        return (
            State::Problem,
            format!("GPU temperature is {t_s}{j_s}: problem at 90°C edge or 105°C hotspot"),
        );
    }
    if temp_c.is_some_and(|t| t >= 80) || junction_temp_c.is_some_and(|j| j >= 95) {
        let t_s = temp_c.map(|t| format!("{t}°C")).unwrap_or_default();
        let j_s = junction_temp_c
            .map(|j| format!(" (hotspot {j}°C)"))
            .unwrap_or_default();
        return (
            State::Watch,
            format!("GPU temperature is {t_s}{j_s}: watch at 80°C edge or 95°C hotspot"),
        );
    }
    if power_pct_of_cap.is_some_and(|p| p >= 99.0) && temp_c.is_some_and(|t| t >= 75) {
        return (
            State::Watch,
            format!(
                "GPU power at {:.0} of {:.0} W ({:.0}% of cap) and {}°C",
                power_watts.unwrap_or(0.0),
                power_cap_watts.unwrap_or(0.0),
                power_pct_of_cap.unwrap_or(0.0),
                temp_c.unwrap_or(0)
            ),
        );
    }
    let mut parts = Vec::new();
    if let Some(b) = busy_pct {
        parts.push(format!("{b}%"));
    }
    if let Some(t) = temp_c {
        parts.push(format!("{t}°C"));
    }
    if let (Some(w), Some(cap)) = (power_watts, power_cap_watts) {
        parts.push(format!("{w:.0} / {cap:.0} W"));
    } else if let Some(w) = power_watts {
        parts.push(format!("{w:.0} W"));
    }
    let msg = if parts.is_empty() {
        "GPU detected".to_string()
    } else {
        format!("GPU {}", parts.join(", "))
    };
    (State::Ok, msg)
}

// ---------------------------------------------------------------------------------------------
// Failed systemd units

/// Parses `systemctl list-units --failed --plain --no-legend --no-pager` output.
pub fn parse_systemctl_failed(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    for raw in text.lines() {
        let line = raw
            .trim_start()
            .trim_start_matches(['●', '*', ' '])
            .trim_start();
        if line.is_empty() {
            continue;
        }
        let Some(unit) = line.split_whitespace().next() else {
            continue;
        };
        if unit.contains('.') && !units.iter().any(|u| u == unit) {
            units.push(unit.to_string());
        }
    }
    units
}

pub fn collect_systemd(env: &SysEnv) -> SystemdInfo {
    if !env.run_commands {
        let sys_fixture = env.state_dir.join("systemd-failed-system.txt");
        let usr_fixture = env.state_dir.join("systemd-failed-user.txt");
        let (system_available, system_failed) = match std::fs::read_to_string(sys_fixture) {
            Ok(t) => (true, parse_systemctl_failed(&t)),
            Err(_) => (false, Vec::new()),
        };
        let (user_available, user_failed) = match std::fs::read_to_string(usr_fixture) {
            Ok(t) => (true, parse_systemctl_failed(&t)),
            Err(_) => (false, Vec::new()),
        };
        return SystemdInfo {
            system_available,
            system_failed,
            user_available,
            user_failed,
        };
    }

    let Some(systemctl) = env.program_on_path("systemctl") else {
        return SystemdInfo::default();
    };
    let (system_available, system_failed) = if Path::new("/run/systemd/system").is_dir() {
        match run_cmd(
            OsStr::new(systemctl.as_os_str()),
            &[
                "list-units",
                "--failed",
                "--plain",
                "--no-legend",
                "--no-pager",
            ],
            Duration::from_secs(2),
        ) {
            Some(out) if out.status == Some(0) => (true, parse_systemctl_failed(&out.stdout)),
            _ => (false, Vec::new()),
        }
    } else {
        (false, Vec::new())
    };
    let (user_available, user_failed) = match run_cmd(
        OsStr::new(systemctl.as_os_str()),
        &[
            "--user",
            "list-units",
            "--failed",
            "--plain",
            "--no-legend",
            "--no-pager",
        ],
        Duration::from_secs(2),
    ) {
        Some(out) if out.status == Some(0) => (true, parse_systemctl_failed(&out.stdout)),
        _ => (false, Vec::new()),
    };
    SystemdInfo {
        system_available,
        system_failed,
        user_available,
        user_failed,
    }
}

// ---------------------------------------------------------------------------------------------
// Compositor (session.log missed frames & stalls over the last hour + shell.sock liveness)

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionLogParsed {
    pub session_start: Option<String>,
    pub session_start_unix: Option<i64>,
    pub latency_stats_enabled: bool,
    pub missed_frames_total: u64,
    pub frames_total: u64,
    pub stalls_total: u64,
    pub slow_turns_total: u64,
    pub worst_stall_ms: Option<f64>,
    pub gpu_resets_total: u64,
}

fn parse_latency_line(line: &str) -> Option<(String, u64, u64)> {
    let rest = line.strip_prefix("[infynitae] Latency ")?;
    let (out_id, body) = rest.split_once(':')?;
    // "... , 86 of 22596 frames in a run missed their refresh ..."
    let idx = body.find(" frames in a run missed their refresh")?;
    let before = &body[..idx];
    let comma = before.rfind(", ")?;
    let pair = &before[comma + 2..];
    let (missed_s, total_s) = pair.split_once(" of ")?;
    let missed = missed_s.trim().parse::<u64>().ok()?;
    let total = total_s.trim().parse::<u64>().ok()?;
    Some((out_id.trim().to_string(), missed, total))
}

fn parse_slow_turn_ms(line: &str) -> Option<f64> {
    let rest = line.strip_prefix("[infynitae] Slow turn ")?;
    let (ms_s, _) = rest.split_once(" ms:")?;
    ms_s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Parses the latest compositor session inside `session.log`.
pub fn parse_session_log(text: &str) -> SessionLogParsed {
    let mut out = SessionLogParsed::default();
    let mut per_output: HashMap<String, (u64, u64)> = HashMap::new();

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("[infynitae-session] ") {
            if rest.ends_with("Starting Infynitae compositor...") && rest.len() >= 19 {
                let ts = &rest[..19];
                out = SessionLogParsed {
                    session_start: Some(ts.to_string()),
                    session_start_unix: parse_timestamp_to_unix(ts),
                    ..SessionLogParsed::default()
                };
                per_output.clear();
            }
            continue;
        }
        if let Some((out_id, missed, total)) = parse_latency_line(line) {
            out.latency_stats_enabled = true;
            per_output.insert(out_id, (missed, total));
            continue;
        }
        if let Some(ms) = parse_slow_turn_ms(line) {
            out.slow_turns_total += 1;
            if ms >= STALL_THRESHOLD_MS {
                out.stalls_total += 1;
            }
            out.worst_stall_ms = Some(out.worst_stall_ms.map_or(ms, |prev| prev.max(ms)));
            continue;
        }
        if line.starts_with("[infynitae] The GPU reset or ran out of memory:") {
            out.gpu_resets_total += 1;
        }
    }

    for (missed, total) in per_output.values() {
        out.missed_frames_total = out.missed_frames_total.saturating_add(*missed);
        out.frames_total = out.frames_total.saturating_add(*total);
    }
    out
}

/// Sums positive deltas across a sequence of cumulative counters from the last hour, handling
/// compositor restarts when `session_start` changes or a counter resets downward.
pub fn compute_window_deltas(
    history_counters: &[&CompositorCounters],
    current: &CompositorCounters,
) -> (u64, u64, u64, u64, u64) {
    let mut missed = 0u64;
    let mut frames = 0u64;
    let mut stalls = 0u64;
    let mut slow_turns = 0u64;
    let mut gpu_resets = 0u64;

    let step =
        |prev: &CompositorCounters, next: &CompositorCounters| -> (u64, u64, u64, u64, u64) {
            let same_session = prev.session_start == next.session_start
                && next.frames_total >= prev.frames_total
                && next.stalls_total >= prev.stalls_total;
            if same_session {
                (
                    next.missed_frames_total
                        .saturating_sub(prev.missed_frames_total),
                    next.frames_total.saturating_sub(prev.frames_total),
                    next.stalls_total.saturating_sub(prev.stalls_total),
                    next.slow_turns_total.saturating_sub(prev.slow_turns_total),
                    next.gpu_resets_total.saturating_sub(prev.gpu_resets_total),
                )
            } else {
                (
                    next.missed_frames_total,
                    next.frames_total,
                    next.stalls_total,
                    next.slow_turns_total,
                    next.gpu_resets_total,
                )
            }
        };

    if history_counters.is_empty() {
        return (
            current.missed_frames_total,
            current.frames_total,
            current.stalls_total,
            current.slow_turns_total,
            current.gpu_resets_total,
        );
    }

    let mut prev = history_counters[0];
    for &next in history_counters.iter().skip(1) {
        let (dm, df, ds, dt, dr) = step(prev, next);
        missed = missed.saturating_add(dm);
        frames = frames.saturating_add(df);
        stalls = stalls.saturating_add(ds);
        slow_turns = slow_turns.saturating_add(dt);
        gpu_resets = gpu_resets.saturating_add(dr);
        prev = next;
    }
    let (dm, df, ds, dt, dr) = step(prev, current);
    (
        missed.saturating_add(dm),
        frames.saturating_add(df),
        stalls.saturating_add(ds),
        slow_turns.saturating_add(dt),
        gpu_resets.saturating_add(dr),
    )
}

fn check_shell_socket(socket: &Path) -> Option<bool> {
    if !socket.exists() {
        return None;
    }
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return Some(false);
    };
    let timeout = Duration::from_millis(200);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    if stream
        .write_all(b"{\"id\":1,\"cmd\":\"snapshot\"}\n")
        .is_err()
    {
        return Some(false);
    }
    let mut line = String::new();
    match BufReader::new(stream).read_line(&mut line) {
        Ok(n) if n > 0 && line.contains("\"ok\":true") => Some(true),
        _ => Some(false),
    }
}

pub fn collect_compositor(env: &SysEnv, history_samples: &[HistorySample]) -> CompositorInfo {
    let session_log_path = env.state_dir.join("session.log");
    let raw = read_file_tail(&session_log_path, SESSION_LOG_TAIL_BYTES).unwrap_or_default();
    let parsed = parse_session_log(&raw);
    let counters = CompositorCounters {
        session_start: parsed.session_start.clone(),
        missed_frames_total: parsed.missed_frames_total,
        frames_total: parsed.frames_total,
        stalls_total: parsed.stalls_total,
        slow_turns_total: parsed.slow_turns_total,
        gpu_resets_total: parsed.gpu_resets_total,
    };

    let hist_refs: Vec<&CompositorCounters> = history_samples
        .iter()
        .map(|s| &s.compositor_counters)
        .collect();
    let session_within_hour = parsed
        .session_start_unix
        .is_some_and(|start| env.now_unix.saturating_sub(start) <= 3600);
    let window = if !hist_refs.is_empty() || session_within_hour {
        "1h".to_string()
    } else {
        "session".to_string()
    };

    let (missed, frames, stalls, slow_turns, gpu_resets) = if !hist_refs.is_empty() {
        compute_window_deltas(&hist_refs, &counters)
    } else {
        (
            counters.missed_frames_total,
            counters.frames_total,
            counters.stalls_total,
            counters.slow_turns_total,
            counters.gpu_resets_total,
        )
    };

    let missed_frames_1h = parsed.latency_stats_enabled.then_some(missed);
    let total_frames_1h = parsed.latency_stats_enabled.then_some(frames);
    let missed_pct_1h = if parsed.latency_stats_enabled && frames > 0 {
        Some(round2((missed as f64 / frames as f64) * 100.0))
    } else if parsed.latency_stats_enabled {
        Some(0.0)
    } else {
        None
    };

    let shell_responsive = if env.run_commands {
        check_shell_socket(&env.runtime_dir.join("shell.sock"))
    } else {
        None
    };

    let (state, reason) = evaluate_compositor(
        &window,
        parsed.latency_stats_enabled,
        missed_frames_1h,
        missed_pct_1h,
        stalls,
        gpu_resets,
        shell_responsive,
    );

    CompositorInfo {
        window,
        latency_stats_enabled: parsed.latency_stats_enabled,
        missed_frames_1h,
        total_frames_1h,
        missed_pct_1h,
        stalls_1h: stalls,
        slow_turns_1h: slow_turns,
        worst_stall_ms: parsed.worst_stall_ms.map(round1),
        gpu_resets_1h: gpu_resets,
        shell_responsive,
        counters,
        state,
        reason,
    }
}

pub fn evaluate_compositor(
    window: &str,
    latency_stats_enabled: bool,
    missed_frames: Option<u64>,
    missed_pct: Option<f64>,
    stalls: u64,
    gpu_resets: u64,
    shell_responsive: Option<bool>,
) -> (State, String) {
    let win_text = if window == "1h" {
        "in the last hour"
    } else {
        "this session"
    };
    if shell_responsive == Some(false) {
        return (
            State::Problem,
            "Compositor shell socket did not answer: compositor may be hung".to_string(),
        );
    }
    if gpu_resets > 0 {
        return (
            State::Problem,
            format!("Compositor hit {gpu_resets} GPU reset(s) {win_text}: problem above 0"),
        );
    }
    let m = missed_frames.unwrap_or(0);
    let pct = missed_pct.unwrap_or(0.0);
    if stalls >= 20 || (m >= 500 && pct >= 5.0) {
        return (
            State::Problem,
            format!(
                "Compositor had {stalls} stalls and {m} missed frames ({pct:.1}%) {win_text}: problem at 20 stalls or 5% missed"
            ),
        );
    }
    if stalls >= 5 || (m >= 100 && pct >= 1.0) {
        return (
            State::Watch,
            format!(
                "Compositor had {stalls} stalls and {m} missed frames ({pct:.1}%) {win_text}: watch at 5 stalls or 1% missed"
            ),
        );
    }
    if latency_stats_enabled {
        (
            State::Ok,
            format!("Compositor {m} missed frames ({pct:.2}%), {stalls} stalls {win_text}"),
        )
    } else {
        (
            State::Ok,
            format!("Compositor {stalls} stalls {win_text} (LATENCY_STATS is off)"),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Updates (Infynitae git origin/main + pacman checkupdates / update-status)

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateStatusFile {
    pub system: Option<u32>,
    pub infynitae: Option<u32>,
    pub checkupdates: Option<bool>,
    pub checked_at: Option<i64>,
}

pub fn parse_update_status(text: &str) -> UpdateStatusFile {
    let mut out = UpdateStatusFile::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "system" => out.system = v.parse::<u32>().ok(),
            "infynitae" => out.infynitae = v.parse::<u32>().ok(),
            "checkupdates" => {
                out.checkupdates = match v {
                    "present" => Some(true),
                    "missing" => Some(false),
                    _ => None,
                }
            }
            "checked_at" => out.checked_at = v.parse::<i64>().ok(),
            _ => {}
        }
    }
    out
}

fn read_git_ref(git_dir: &Path, ref_name: &str) -> Option<String> {
    let loose = git_dir.join(ref_name);
    if let Ok(text) = std::fs::read_to_string(loose) {
        let trimmed = text.trim();
        if !trimmed.is_empty() && !trimmed.starts_with("ref:") {
            return Some(trimmed.to_string());
        }
    }
    let packed = std::fs::read_to_string(git_dir.join("packed-refs")).ok()?;
    for line in packed.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with('^') {
            continue;
        }
        let mut parts = t.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        if name == ref_name {
            return Some(hash.to_string());
        }
    }
    None
}

/// Fast zero-fork check: returns `Some(0)` when `.git/HEAD` resolves to the exact same commit hash
/// as `refs/remotes/origin/main`.
fn fast_git_matches_origin_main(repo: &Path) -> Option<u32> {
    let git_entry = repo.join(".git");
    let git_dir = if git_entry.is_dir() {
        git_entry
    } else if git_entry.is_file() {
        let txt = std::fs::read_to_string(&git_entry).ok()?;
        let rel = txt.trim().strip_prefix("gitdir:")?.trim();
        let p = Path::new(rel);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            repo.join(p)
        }
    } else {
        return None;
    };
    let head_txt = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head_hash = if let Some(ref_name) = head_txt.trim().strip_prefix("ref:") {
        read_git_ref(&git_dir, ref_name.trim())?
    } else {
        head_txt.trim().to_string()
    };
    let common_dir = std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|s| git_dir.join(s.trim()))
        .unwrap_or_else(|| git_dir.clone());
    let origin_hash = read_git_ref(&common_dir, "refs/remotes/origin/main")?;
    (head_hash == origin_hash).then_some(0)
}

pub fn collect_updates(env: &SysEnv) -> UpdatesInfo {
    let status_raw = std::fs::read_to_string(env.state_dir.join("update-status")).ok();
    let cached = status_raw
        .as_deref()
        .map(parse_update_status)
        .unwrap_or_default();
    let path_has_cu = env.program_on_path("checkupdates").is_some();
    let checkupdates_present = if env.run_commands {
        path_has_cu
    } else {
        cached.checkupdates.unwrap_or(path_has_cu)
    };

    let mut infynitae_behind = cached.infynitae;
    if let Some(repo) = env.resolve_checkout() {
        if repo.exists() {
            if let Some(zero) = fast_git_matches_origin_main(&repo) {
                infynitae_behind = Some(zero);
            } else if env.run_commands {
                if let Some(git) = env.program_on_path("git") {
                    if let Some(repo_s) = repo.to_str() {
                        if let Some(out) = run_cmd(
                            OsStr::new(git.as_os_str()),
                            &["-C", repo_s, "rev-list", "--count", "HEAD..origin/main"],
                            Duration::from_secs(2),
                        ) {
                            if out.status == Some(0) {
                                if let Ok(n) = out.stdout.trim().parse::<u32>() {
                                    infynitae_behind = Some(n);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let cache_fresh = cached
        .checked_at
        .is_some_and(|ts| env.now_unix.saturating_sub(ts) <= UPDATE_CACHE_MAX_AGE_SECS);
    let mut pacman_updates = cached.system;
    if !env.sample_mode
        && env.run_commands
        && checkupdates_present
        && (pacman_updates.is_none() || !cache_fresh)
    {
        if let Some(cu) = env.program_on_path("checkupdates") {
            if let Some(out) = run_cmd(OsStr::new(cu.as_os_str()), &[], Duration::from_secs(5)) {
                match out.status {
                    Some(0) => {
                        let count =
                            out.stdout.lines().filter(|l| !l.trim().is_empty()).count() as u32;
                        pacman_updates = Some(count);
                    }
                    Some(2) => pacman_updates = Some(0),
                    _ => {}
                }
            }
        }
    }

    let (state, reason) = evaluate_updates(infynitae_behind, pacman_updates, checkupdates_present);
    UpdatesInfo {
        infynitae_behind,
        pacman_updates,
        checkupdates_present,
        checked_at_unix: cached.checked_at,
        state,
        reason,
    }
}

pub fn evaluate_updates(
    infynitae_behind: Option<u32>,
    pacman_updates: Option<u32>,
    checkupdates_present: bool,
) -> (State, String) {
    let infy = infynitae_behind.unwrap_or(0);
    let pac = pacman_updates.unwrap_or(0);
    if infy > 0 || pac >= 25 {
        let mut parts = Vec::new();
        if infy > 0 {
            let word = if infy == 1 { "commit" } else { "commits" };
            parts.push(format!("Infynitae {infy} {word} behind origin/main"));
        }
        if pac > 0 {
            let word = if pac == 1 { "package" } else { "packages" };
            parts.push(format!("{pac} pacman {word} waiting"));
        }
        return (
            State::Watch,
            format!(
                "{}: watch when behind or 25+ packages waiting",
                parts.join(", ")
            ),
        );
    }
    let mut parts = Vec::new();
    match infynitae_behind {
        Some(0) => parts.push("Infynitae up to date".to_string()),
        Some(n) => parts.push(format!("Infynitae {n} behind")),
        None => parts.push("Infynitae status unknown".to_string()),
    }
    if !checkupdates_present {
        parts.push("checkupdates not installed".to_string());
    } else if let Some(n) = pacman_updates {
        if n == 0 {
            parts.push("pacman up to date".to_string());
        } else {
            let word = if n == 1 { "package" } else { "packages" };
            parts.push(format!("{n} pacman {word} waiting"));
        }
    }
    (State::Ok, parts.join("; "))
}

// ---------------------------------------------------------------------------------------------
// Assemble sorted HealthItems

#[allow(clippy::too_many_arguments)]
fn build_items(
    disks: &[DiskInfo],
    nvme: &[NvmeInfo],
    memory: &MemoryInfo,
    cpu: &CpuInfo,
    gpu: Option<&GpuInfo>,
    systemd: &SystemdInfo,
    compositor: &CompositorInfo,
    updates: &UpdatesInfo,
) -> Vec<HealthItem> {
    let mut items = Vec::new();

    for d in disks {
        let name = match &d.label {
            Some(l) if l != &d.mount => format!("Disk {} ({l})", d.mount),
            _ => format!("Disk {}", d.mount),
        };
        let free_pct = (100.0 - d.used_pct).clamp(0.0, 100.0);
        items.push(HealthItem {
            id: format!("disk:{}", d.mount),
            category: "disk".into(),
            label: name,
            state: d.state,
            summary: format!("{} free ({free_pct:.0}%)", format_bytes(d.available_bytes)),
            reason: d.reason.clone(),
            threshold: "watch below 15% or 15 GB free, problem below 5% or 5 GB free".into(),
            action: (d.mount == "/" && d.btrfs.is_some() && d.state != State::Ok)
                .then(|| "disk-fix".to_string()),
        });
        if let Some(b) = &d.btrfs {
            let btrfs_summary = if b.can_allocate_chunk && b.metadata_free_bytes < 256 * MIB {
                format!(
                    "{:.0}% used, {} unallocated",
                    b.metadata_used_pct,
                    format_bytes(b.unallocated_bytes)
                )
            } else {
                format!(
                    "{:.0}% used ({} free)",
                    b.metadata_used_pct,
                    format_bytes(b.metadata_free_bytes)
                )
            };
            items.push(HealthItem {
                id: format!("btrfs:{}", d.mount),
                category: "disk".into(),
                label: format!("Btrfs metadata {}", d.mount),
                state: b.state,
                summary: btrfs_summary,
                reason: b.reason.clone(),
                threshold: "watch at 75% or <1 GB free, problem at 90% or <256 MB free when unallocated space is low".into(),
                action: (d.mount == "/" && b.state != State::Ok).then(|| "disk-fix".to_string()),
            });
        }
    }

    for n in nvme {
        let summary = if !n.readable {
            match n.temp_c {
                Some(t) => format!("Needs permission, {t}°C"),
                None => "Needs permission".to_string(),
            }
        } else {
            let mut parts = Vec::new();
            if let Some(w) = n.wear_pct {
                parts.push(format!("{w}% wear"));
            }
            if let Some(b) = n.data_written_bytes {
                parts.push(format!("{} written", format_bytes(b)));
            }
            if let Some(t) = n.temp_c {
                parts.push(format!("{t}°C"));
            }
            if parts.is_empty() {
                "SMART ok".to_string()
            } else {
                parts.join(", ")
            }
        };
        items.push(HealthItem {
            id: format!("nvme:{}", n.device),
            category: "nvme".into(),
            label: format!("NVMe {}", n.device),
            state: n.state,
            summary,
            reason: n.reason.clone(),
            threshold: "watch at 80% wear, problem at 90% wear or SMART warning".into(),
            action: None,
        });
    }

    // Memory: RAM
    let (ram_state, ram_reason) = evaluate_ram(memory);
    items.push(HealthItem {
        id: "memory:ram".into(),
        category: "memory".into(),
        label: "Memory".into(),
        state: ram_state,
        summary: if memory.ram_total_bytes > 0 {
            format!(
                "{} of {} ({:.0}%)",
                format_bytes(memory.ram_used_bytes),
                format_bytes(memory.ram_total_bytes),
                memory.ram_used_pct
            )
        } else {
            "Unavailable".into()
        },
        reason: ram_reason,
        threshold: "watch at 80% used, problem at 92% used or under 1 GB free".into(),
        action: None,
    });

    // Memory: Swap (zswap + disk)
    let (swap_state, swap_reason, swap_summary) = evaluate_swap(memory);
    items.push(HealthItem {
        id: "memory:swap".into(),
        category: "memory".into(),
        label: "Swap".into(),
        state: swap_state,
        summary: swap_summary,
        reason: swap_reason,
        threshold: "watch at 60% used, problem at 90% used".into(),
        action: None,
    });

    // Memory pressure (PSI)
    if let Some(psi) = &memory.pressure {
        let (psi_state, psi_reason, psi_summary) = evaluate_pressure(psi);
        items.push(HealthItem {
            id: "memory:psi".into(),
            category: "memory".into(),
            label: "Memory pressure".into(),
            state: psi_state,
            summary: psi_summary,
            reason: psi_reason,
            threshold: "watch at 2% full or 10% some (1 min), problem at 10% full or 40% some"
                .into(),
            action: None,
        });
    }

    // CPU
    let mut cpu_sum = Vec::new();
    if let Some(b) = cpu.busy_pct {
        cpu_sum.push(format!("{b}%"));
    }
    if let Some(t) = cpu.temp_c {
        cpu_sum.push(format!("{t}°C"));
    }
    if cpu_sum.is_empty() {
        if let Some(l1) = cpu.load_1m {
            cpu_sum.push(format!("load {l1:.2}"));
        } else {
            cpu_sum.push("Unavailable".into());
        }
    }
    items.push(HealthItem {
        id: "cpu".into(),
        category: "cpu".into(),
        label: "CPU".into(),
        state: cpu.state,
        summary: cpu_sum.join(", "),
        reason: cpu.reason.clone(),
        threshold: "watch at 80°C or 95% load, problem at 90°C".into(),
        action: None,
    });

    // GPU
    if let Some(g) = gpu {
        let mut gpu_sum = Vec::new();
        if let Some(b) = g.busy_pct {
            gpu_sum.push(format!("{b}%"));
        }
        if let Some(t) = g.temp_c {
            gpu_sum.push(format!("{t}°C"));
        }
        if let (Some(w), Some(cap)) = (g.power_watts, g.power_cap_watts) {
            gpu_sum.push(format!("{w:.0} / {cap:.0} W"));
        } else if let Some(w) = g.power_watts {
            gpu_sum.push(format!("{w:.0} W"));
        }
        if gpu_sum.is_empty() {
            gpu_sum.push("Ready".into());
        }
        items.push(HealthItem {
            id: "gpu".into(),
            category: "gpu".into(),
            label: "GPU".into(),
            state: g.state,
            summary: gpu_sum.join(", "),
            reason: g.reason.clone(),
            threshold: "watch at 80°C edge or 95°C hotspot, problem at 90°C edge or 105°C hotspot"
                .into(),
            action: None,
        });
    }

    // Systemd system units
    let (sys_state, sys_sum, sys_reason) = evaluate_systemd_scope(
        "System services",
        systemd.system_available,
        &systemd.system_failed,
    );
    items.push(HealthItem {
        id: "systemd:system".into(),
        category: "systemd".into(),
        label: "System services".into(),
        state: sys_state,
        summary: sys_sum,
        reason: sys_reason,
        threshold: "problem when any systemd service fails".into(),
        action: None,
    });

    // Systemd user units
    let (usr_state, usr_sum, usr_reason) = evaluate_systemd_scope(
        "User services",
        systemd.user_available,
        &systemd.user_failed,
    );
    items.push(HealthItem {
        id: "systemd:user".into(),
        category: "systemd".into(),
        label: "User services".into(),
        state: usr_state,
        summary: usr_sum,
        reason: usr_reason,
        threshold: "problem when any user systemd service fails".into(),
        action: None,
    });

    // Compositor
    let comp_sum = if compositor.gpu_resets_1h > 0 {
        format!("{} GPU reset(s)", compositor.gpu_resets_1h)
    } else if let Some(m) = compositor.missed_frames_1h {
        format!("{m} missed, {} stalls", compositor.stalls_1h)
    } else {
        format!("{} stalls", compositor.stalls_1h)
    };
    items.push(HealthItem {
        id: "compositor".into(),
        category: "compositor".into(),
        label: "Compositor".into(),
        state: compositor.state,
        summary: comp_sum,
        reason: compositor.reason.clone(),
        threshold: "watch at 5 stalls or 1% missed, problem at 20 stalls, 5% missed, or GPU reset"
            .into(),
        action: (compositor.state != State::Ok).then(|| "settings:advanced".to_string()),
    });

    // Updates
    let upd_sum = match (
        updates.infynitae_behind,
        updates.pacman_updates,
        updates.checkupdates_present,
    ) {
        (Some(0), Some(0), true) => "Up to date".to_string(),
        (Some(i), Some(p), true) if i > 0 && p > 0 => {
            format!("{i} Infynitae, {p} pacman waiting")
        }
        (Some(i), _, _) if i > 0 => format!("{i} Infynitae behind"),
        (_, Some(p), true) if p > 0 => format!("{p} pacman waiting"),
        (Some(0), None, false) => "Infynitae current, no checkupdates".to_string(),
        _ => "Checked".to_string(),
    };
    items.push(HealthItem {
        id: "updates".into(),
        category: "updates".into(),
        label: "Updates".into(),
        state: updates.state,
        summary: upd_sum,
        reason: updates.reason.clone(),
        threshold: "watch when Infynitae is behind origin/main or 25+ pacman packages wait".into(),
        action: (updates.state != State::Ok).then(|| "updates".to_string()),
    });

    // Stable sort: problem first, then watch, then ok.
    items.sort_by_key(|a| std::cmp::Reverse(a.state.severity_rank()));
    items
}

pub fn evaluate_ram(memory: &MemoryInfo) -> (State, String) {
    if memory.ram_total_bytes == 0 {
        return (State::Ok, "RAM size could not be read".into());
    }
    let avail = format_bytes(memory.ram_available_bytes);
    let used = format_bytes(memory.ram_used_bytes);
    let total = format_bytes(memory.ram_total_bytes);
    if memory.ram_used_pct >= 92.0
        || (memory.ram_total_bytes >= 4 * GIB && memory.ram_available_bytes < GIB)
    {
        (
            State::Problem,
            format!(
                "RAM is {:.0}% full ({avail} free): problem at 92% or under 1 GB free",
                memory.ram_used_pct
            ),
        )
    } else if memory.ram_used_pct >= 80.0 {
        (
            State::Watch,
            format!(
                "RAM is {:.0}% used ({avail} free): watch at 80%, problem at 92%",
                memory.ram_used_pct
            ),
        )
    } else {
        (
            State::Ok,
            format!(
                "RAM {used} of {total} ({:.0}% used, {avail} free)",
                memory.ram_used_pct
            ),
        )
    }
}

pub fn evaluate_swap(memory: &MemoryInfo) -> (State, String, String) {
    if memory.swap_total_bytes == 0 {
        return (
            State::Ok,
            "No swap partition or file configured".into(),
            "None".into(),
        );
    }
    let used = format_bytes(memory.swap_used_bytes);
    let total = format_bytes(memory.swap_total_bytes);
    let disk_used = format_bytes(memory.swap_disk_used_bytes);
    let summary = if memory.zswap_enabled {
        format!(
            "{disk_used} disk, {} zswap",
            format_bytes(memory.zswap_pool_bytes)
        )
    } else {
        format!("{used} of {total}")
    };
    let zswap_note = if memory.zswap_enabled {
        format!(
            " ({disk_used} on disk, {} compressed in zswap)",
            format_bytes(memory.zswap_pool_bytes)
        )
    } else {
        String::new()
    };
    if memory.swap_used_pct >= 90.0 {
        (
            State::Problem,
            format!(
                "Swap is {:.0}% full ({used} of {total}){zswap_note}: problem at 90%",
                memory.swap_used_pct
            ),
            summary,
        )
    } else if memory.swap_used_pct >= 60.0 {
        (
            State::Watch,
            format!(
                "Swap is {:.0}% used ({used} of {total}){zswap_note}: watch at 60%, problem at 90%",
                memory.swap_used_pct
            ),
            summary,
        )
    } else {
        (
            State::Ok,
            format!(
                "Swap {used} of {total} ({:.0}%){zswap_note}",
                memory.swap_used_pct
            ),
            summary,
        )
    }
}

pub fn evaluate_pressure(psi: &PressureInfo) -> (State, String, String) {
    let mem_full = psi
        .memory
        .as_ref()
        .and_then(|m| m.full.as_ref())
        .map_or(0.0, |l| l.avg60);
    let mem_some = psi
        .memory
        .as_ref()
        .and_then(|m| m.some.as_ref())
        .map_or(0.0, |l| l.avg60);
    let io_full = psi
        .io
        .as_ref()
        .and_then(|m| m.full.as_ref())
        .map_or(0.0, |l| l.avg60);
    let summary = format!("{mem_full:.2}% full, {mem_some:.2}% some");
    if mem_full >= 10.0 || mem_some >= 40.0 {
        (
            State::Problem,
            format!(
                "Memory pressure high ({mem_full:.1}% full, {mem_some:.1}% some over 1 min): problem at 10% full or 40% some"
            ),
            summary,
        )
    } else if mem_full >= 2.0 || mem_some >= 10.0 {
        (
            State::Watch,
            format!(
                "Memory pressure rising ({mem_full:.1}% full, {mem_some:.1}% some over 1 min): watch at 2% full or 10% some"
            ),
            summary,
        )
    } else if io_full >= 50.0 {
        (
            State::Problem,
            format!("Disk I/O pressure high ({io_full:.1}% full over 1 min): problem at 50%"),
            format!("I/O {io_full:.1}% full"),
        )
    } else if io_full >= 25.0 {
        (
            State::Watch,
            format!("Disk I/O pressure rising ({io_full:.1}% full over 1 min): watch at 25%"),
            format!("I/O {io_full:.1}% full"),
        )
    } else {
        (
            State::Ok,
            format!("Memory pressure {mem_full:.2}% full, {mem_some:.2}% some (1 min)"),
            summary,
        )
    }
}

fn evaluate_systemd_scope(
    title: &str,
    available: bool,
    failed: &[String],
) -> (State, String, String) {
    if !available {
        return (
            State::Ok,
            "Unavailable".into(),
            format!("{title} could not be queried"),
        );
    }
    if failed.is_empty() {
        return (
            State::Ok,
            "0 failed".into(),
            format!("{title}: none failed"),
        );
    }
    let list = if failed.len() <= 3 {
        failed.join(", ")
    } else {
        format!("{}, +{} more", failed[..3].join(", "), failed.len() - 3)
    };
    (
        State::Problem,
        format!("{} failed", failed.len()),
        format!("{title}: {} failed ({list}): problem above 0", failed.len()),
    )
}

/// Formats `HealthSnapshot` as plain human-readable text (problems and watch first, then ok).
pub fn render_now_text(snap: &HealthSnapshot) -> String {
    let mut out = String::new();
    let problems = snap
        .items
        .iter()
        .filter(|i| i.state == State::Problem)
        .count();
    let watches = snap
        .items
        .iter()
        .filter(|i| i.state == State::Watch)
        .count();
    let header = match (problems, watches) {
        (0, 0) => "Health: all ok".to_string(),
        (p, 0) => format!("Health: {p} problem{}", if p == 1 { "" } else { "s" }),
        (0, w) => format!("Health: {w} to watch"),
        (p, w) => format!(
            "Health: {p} problem{}, {w} to watch",
            if p == 1 { "" } else { "s" }
        ),
    };
    let shown = parse_timestamp_to_unix(&snap.time)
        .map(|t| local_display_time(t, t))
        .unwrap_or_else(|| snap.time.clone());
    out.push_str(&format!("{header} ({shown})\n"));
    for item in &snap.items {
        let tag = match item.state {
            State::Problem => "PROBLEM",
            State::Watch => "WATCH  ",
            State::Ok => "ok     ",
        };
        if item.state == State::Ok {
            out.push_str(&format!("  {tag}  {:<22} {}\n", item.label, item.summary));
        } else {
            out.push_str(&format!("  {tag}  {:<22} {}\n", item.label, item.reason));
        }
    }
    out
}
