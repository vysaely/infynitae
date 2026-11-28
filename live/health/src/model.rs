//! Stable data model and formatting helpers for `infynitae-health` (documented in `docs/HEALTH.md`).

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Ok,
    Watch,
    Problem,
}

impl State {
    pub fn severity_rank(self) -> u8 {
        match self {
            State::Problem => 2,
            State::Watch => 1,
            State::Ok => 0,
        }
    }

    pub fn worst(self, other: State) -> State {
        if other.severity_rank() > self.severity_rank() {
            other
        } else {
            self
        }
    }
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        self.severity_rank().cmp(&other.severity_rank())
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthItem {
    pub id: String,
    pub category: String,
    pub label: String,
    pub state: State,
    pub summary: String,
    pub reason: String,
    pub threshold: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub schema_version: u32,
    pub timestamp_unix: i64,
    pub time: String,
    pub state: State,
    pub items: Vec<HealthItem>,
    pub disks: Vec<DiskInfo>,
    pub nvme: Vec<NvmeInfo>,
    pub memory: MemoryInfo,
    pub cpu: CpuInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuInfo>,
    pub systemd: SystemdInfo,
    pub compositor: CompositorInfo,
    pub updates: UpdatesInfo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiskInfo {
    pub mount: String,
    pub device: String,
    pub fs_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_pct: f64,
    pub state: State,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub btrfs: Option<BtrfsMetadataInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BtrfsMetadataInfo {
    pub uuid: String,
    pub metadata_total_bytes: u64,
    pub metadata_used_bytes: u64,
    pub global_rsv_bytes: u64,
    pub metadata_free_bytes: u64,
    pub metadata_used_pct: f64,
    pub unallocated_bytes: u64,
    pub chunk_size_bytes: u64,
    pub can_allocate_chunk: bool,
    pub state: State,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NvmeInfo {
    pub device: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub readable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wear_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_written_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical_warning: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_errors: Option<u64>,
    pub state: State,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub ram_total_bytes: u64,
    pub ram_used_bytes: u64,
    pub ram_available_bytes: u64,
    pub ram_used_pct: f64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_disk_used_bytes: u64,
    pub swap_used_pct: f64,
    pub zswap_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zswap_compressor: Option<String>,
    pub zswap_pool_bytes: u64,
    pub zswapped_bytes: u64,
    pub swap_devices: Vec<SwapDeviceInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<PressureInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapDeviceInfo {
    pub path: String,
    pub swap_type: String,
    pub size_bytes: u64,
    pub used_bytes: u64,
    pub priority: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PressureInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<PsiResource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<PsiResource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io: Option<PsiResource>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PsiResource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub some: Option<PsiLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<PsiLine>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PsiLine {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    pub total_us: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CpuInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_1m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_5m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_15m: Option<f64>,
    pub state: State,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GpuInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_used_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub junction_temp_c: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_temp_c: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_watts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_cap_watts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_pct_of_cap: Option<f64>,
    pub state: State,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemdInfo {
    pub system_available: bool,
    pub system_failed: Vec<String>,
    pub user_available: bool,
    pub user_failed: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompositorCounters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_start: Option<String>,
    pub missed_frames_total: u64,
    pub frames_total: u64,
    pub stalls_total: u64,
    pub slow_turns_total: u64,
    pub gpu_resets_total: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompositorInfo {
    pub window: String,
    pub latency_stats_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missed_frames_1h: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_frames_1h: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missed_pct_1h: Option<f64>,
    pub stalls_1h: u64,
    pub slow_turns_1h: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worst_stall_ms: Option<f64>,
    pub gpu_resets_1h: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_responsive: Option<bool>,
    pub counters: CompositorCounters,
    pub state: State,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatesInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infynitae_behind: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pacman_updates: Option<u32>,
    pub checkupdates_present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at_unix: Option<i64>,
    pub state: State,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEvent {
    pub timestamp_unix: i64,
    pub time: String,
    pub local_time: String,
    pub kind: String,
    pub severity: String,
    pub source: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogReport {
    pub schema_version: u32,
    pub since_unix: i64,
    pub since: String,
    pub generated_unix: i64,
    pub generated: String,
    pub events: Vec<LogEvent>,
}

/// Compact non-ok item recorded in the 60 s JSONL history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryAlert {
    pub id: String,
    pub label: String,
    pub state: State,
    pub summary: String,
    pub reason: String,
}

/// Compact per-mount disk metrics recorded in the 60 s JSONL history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryDisk {
    pub mount: String,
    pub available_bytes: u64,
    pub total_bytes: u64,
    pub used_pct: f64,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub btrfs_used_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub btrfs_free_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub btrfs_unallocated_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub btrfs_state: Option<State>,
}

/// Compact 60 s JSONL history line written to `~/.local/state/infynitae/health/YYYY-MM-DD.jsonl`.
/// Keeps every numeric metric, non-ok alert, and compositor counter while dropping per-item
/// boilerplate prose so 7 days of 60 s samples stay small on disk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistorySample {
    pub schema_version: u32,
    pub timestamp_unix: i64,
    pub time: String,
    pub state: State,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<HistoryAlert>,
    pub disks: Vec<HistoryDisk>,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub swap_disk_used_bytes: u64,
    pub zswap_pool_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psi_mem_full_60: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psi_mem_some_60: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psi_io_full_60: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_busy_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_temp_c: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_1m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_busy_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_temp_c: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_junction_temp_c: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_power_watts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_power_cap_watts: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_system_units: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_user_units: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missed_frames_1h: Option<u64>,
    pub stalls_1h: u64,
    pub gpu_resets_1h: u64,
    pub compositor_counters: CompositorCounters,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infynitae_behind: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pacman_updates: Option<u32>,
}

impl HistorySample {
    pub fn from_snapshot(snap: &HealthSnapshot) -> Self {
        let alerts = snap
            .items
            .iter()
            .filter(|i| i.state != State::Ok)
            .map(|i| HistoryAlert {
                id: i.id.clone(),
                label: i.label.clone(),
                state: i.state,
                summary: i.summary.clone(),
                reason: i.reason.clone(),
            })
            .collect();
        let disks = snap
            .disks
            .iter()
            .map(|d| HistoryDisk {
                mount: d.mount.clone(),
                available_bytes: d.available_bytes,
                total_bytes: d.total_bytes,
                used_pct: d.used_pct,
                state: d.state,
                btrfs_used_pct: d.btrfs.as_ref().map(|b| b.metadata_used_pct),
                btrfs_free_bytes: d.btrfs.as_ref().map(|b| b.metadata_free_bytes),
                btrfs_unallocated_bytes: d.btrfs.as_ref().map(|b| b.unallocated_bytes),
                btrfs_state: d.btrfs.as_ref().map(|b| b.state),
            })
            .collect();
        let psi = snap.memory.pressure.as_ref();
        Self {
            schema_version: snap.schema_version,
            timestamp_unix: snap.timestamp_unix,
            time: snap.time.clone(),
            state: snap.state,
            alerts,
            disks,
            ram_used_bytes: snap.memory.ram_used_bytes,
            ram_total_bytes: snap.memory.ram_total_bytes,
            swap_disk_used_bytes: snap.memory.swap_disk_used_bytes,
            zswap_pool_bytes: snap.memory.zswap_pool_bytes,
            psi_mem_full_60: psi
                .and_then(|p| p.memory.as_ref())
                .and_then(|m| m.full.as_ref())
                .map(|l| l.avg60),
            psi_mem_some_60: psi
                .and_then(|p| p.memory.as_ref())
                .and_then(|m| m.some.as_ref())
                .map(|l| l.avg60),
            psi_io_full_60: psi
                .and_then(|p| p.io.as_ref())
                .and_then(|m| m.full.as_ref())
                .map(|l| l.avg60),
            cpu_busy_pct: snap.cpu.busy_pct,
            cpu_temp_c: snap.cpu.temp_c,
            load_1m: snap.cpu.load_1m,
            gpu_busy_pct: snap.gpu.as_ref().and_then(|g| g.busy_pct),
            gpu_temp_c: snap.gpu.as_ref().and_then(|g| g.temp_c),
            gpu_junction_temp_c: snap.gpu.as_ref().and_then(|g| g.junction_temp_c),
            gpu_power_watts: snap.gpu.as_ref().and_then(|g| g.power_watts),
            gpu_power_cap_watts: snap.gpu.as_ref().and_then(|g| g.power_cap_watts),
            failed_system_units: snap.systemd.system_failed.clone(),
            failed_user_units: snap.systemd.user_failed.clone(),
            missed_frames_1h: snap.compositor.missed_frames_1h,
            stalls_1h: snap.compositor.stalls_1h,
            gpu_resets_1h: snap.compositor.gpu_resets_1h,
            compositor_counters: snap.compositor.counters.clone(),
            infynitae_behind: snap.updates.infynitae_behind,
            pacman_updates: snap.updates.pacman_updates,
        }
    }
}

/// Formats bytes using binary GiB/MiB/KiB magnitudes with plain GB/MB/KB/TB labels,
/// matching the rest of Infynitae's monitor and drives cards.
pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const TIB: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= TIB {
        let v = b / TIB;
        if v >= 100.0 {
            format!("{:.0} TB", v.round())
        } else {
            format!("{v:.1} TB")
        }
    } else if b >= GIB {
        let v = b / GIB;
        if v >= 100.0 {
            format!("{:.0} GB", v.round())
        } else {
            format!("{v:.1} GB")
        }
    } else if b >= MIB {
        let v = b / MIB;
        if v >= 10.0 {
            format!("{:.0} MB", v.round())
        } else {
            format!("{v:.1} MB")
        }
    } else if b >= KIB {
        format!("{:.0} KB", (b / KIB).round())
    } else {
        format!("{bytes} B")
    }
}

pub fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Converts Unix seconds (UTC) into `(year, month, day, hour, minute, second)` via Howard
/// Hinnant's civil-from-days algorithm (same formula as `crates/infynitae-wayland/src/journal.rs`).
pub fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, rem / 3600, (rem / 60) % 60, rem % 60)
}

/// Inverse of `civil_from_unix`: UTC `(year, month, day, hour, min, sec)` to Unix seconds.
pub fn unix_from_civil(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + i64::from(hour) * 3600 + i64::from(min) * 60 + i64::from(sec))
}

pub fn iso_utc(secs: i64) -> String {
    let (y, m, d, hh, mm, ss) = civil_from_unix(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

pub fn date_utc(secs: i64) -> String {
    let (y, m, d, _, _, _) = civil_from_unix(secs);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Formats a timestamp in local time (`HH:MM` when on the same local date as `now_unix`,
/// otherwise `YYYY-MM-DD HH:MM`). Uses `libc::localtime_r` with UTC fallback.
pub fn local_display_time(ts_unix: i64, now_unix: i64) -> String {
    let to_tm = |t: i64| -> Option<(i32, i32, i32, i32, i32)> {
        let mut tm = std::mem::MaybeUninit::<libc::tm>::zeroed();
        let tt = t as libc::time_t;
        let ptr = unsafe { libc::localtime_r(&tt, tm.as_mut_ptr()) };
        if ptr.is_null() {
            return None;
        }
        let tm = unsafe { tm.assume_init() };
        Some((
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
        ))
    };
    if let (Some((ty, tm_m, td, th, tmin)), Some((ny, nm, nd, _, _))) =
        (to_tm(ts_unix), to_tm(now_unix))
    {
        if (ty, tm_m, td) == (ny, nm, nd) {
            return format!("{th:02}:{tmin:02}");
        }
        return format!("{ty:04}-{tm_m:02}-{td:02} {th:02}:{tmin:02}");
    }
    let (ty, tm_m, td, th, tmin, _) = civil_from_unix(ts_unix);
    let (ny, nm, nd, _, _, _) = civil_from_unix(now_unix);
    if (ty, tm_m, td) == (ny, nm, nd) {
        format!("{th:02}:{tmin:02}")
    } else {
        format!("{ty:04}-{tm_m:02}-{td:02} {th:02}:{tmin:02}")
    }
}

/// Converts a local civil datetime (from logs like `session.log`, `automount.log`) to Unix seconds.
pub fn local_civil_to_unix(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year - 1900;
    tm.tm_mon = month as i32 - 1;
    tm.tm_mday = day as i32;
    tm.tm_hour = hour as i32;
    tm.tm_min = min as i32;
    tm.tm_sec = sec as i32;
    tm.tm_isdst = -1;
    let res = unsafe { libc::mktime(&mut tm) };
    if res == -1 {
        unix_from_civil(i64::from(year), month, day, hour, min, sec)
    } else {
        Some(res as i64)
    }
}

/// Parses ISO-8601 timestamps such as `2026-11-28T07:41:58Z`, `2026-11-16T16:00:45+0200`,
/// `2026-11-16T16:00:45+02:00`, `2026-11-28 07:41:58.337`, or `2026-11-28`.
pub fn parse_timestamp_to_unix(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.len() < 10 {
        return None;
    }
    let b = t.as_bytes();
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let year: i64 = t[0..4].parse().ok()?;
    let month: u32 = t[5..7].parse().ok()?;
    let day: u32 = t[8..10].parse().ok()?;
    if t.len() == 10 {
        return unix_from_civil(year, month, day, 0, 0, 0);
    }
    if !matches!(b[10], b'T' | b' ') || t.len() < 19 || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let hour: u32 = t[11..13].parse().ok()?;
    let min: u32 = t[14..16].parse().ok()?;
    let sec: u32 = t[17..19].parse().ok()?;
    let mut rest = &t[19..];
    if let Some(after_dot) = rest.strip_prefix('.') {
        let digits = after_dot.bytes().take_while(|c| c.is_ascii_digit()).count();
        rest = &after_dot[digits..];
    }
    if rest.is_empty() {
        // Local wall time without timezone suffix (e.g. session.log / automount.log)
        return local_civil_to_unix(year as i32, month, day, hour, min, sec);
    }
    let base = unix_from_civil(year, month, day, hour, min, sec)?;
    if rest == "Z" || rest == "z" {
        return Some(base);
    }
    let (sign, tz) = if let Some(r) = rest.strip_prefix('+') {
        (1i64, r)
    } else {
        let r = rest.strip_prefix('-')?;
        (-1i64, r)
    };
    let (tz_h, tz_m): (i64, i64) = if tz.len() == 4 {
        (tz[0..2].parse().ok()?, tz[2..4].parse().ok()?)
    } else if tz.len() == 5 && tz.as_bytes()[2] == b':' {
        (tz[0..2].parse().ok()?, tz[3..5].parse().ok()?)
    } else {
        return None;
    };
    Some(base - sign * (tz_h * 3600 + tz_m * 60))
}

/// Parses `--since` arguments: relative durations (`30m`, `1h`, `24h`, `7d`, `90s`) or timestamps
/// (`2026-11-28`, `2026-11-28T07:00:00Z`).
pub fn parse_since_arg(arg: &str, now_unix: i64) -> Option<i64> {
    let t = arg.trim();
    if t.is_empty() {
        return None;
    }
    let stripped = t.strip_prefix('-').unwrap_or(t);
    if let Some(last) = stripped.chars().last() {
        let mult = match last {
            's' | 'S' => Some(1i64),
            'm' | 'M' => Some(60),
            'h' | 'H' => Some(3600),
            'd' | 'D' => Some(86_400),
            _ => None,
        };
        if let Some(m) = mult {
            let num_str = &stripped[..stripped.len() - 1];
            if !num_str.is_empty() && num_str.chars().all(|c| c.is_ascii_digit()) {
                let n = num_str.parse::<i64>().ok()?;
                return Some(now_unix.saturating_sub(n.saturating_mul(m)));
            }
        }
    }
    parse_timestamp_to_unix(t)
}
