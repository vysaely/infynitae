//! System monitor (docs/EDGE_APPS.md section 6): CPU, GPU, memory and temperatures from `/proc`
//! and sysfs, plus the `memory` and `gpu` cards ported from `live/edge-rs/src/modules/`.

use crate::card::{
    parse_render_data, run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status,
    Window,
};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::theme::Rgba;
use crate::ui;
use cairo::Context;
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Monitor;

const CLAIMS: &[Claim] = &[
    Claim::Id("io.github.ilya_zlobintsev.LACT"),
    Claim::Categories(&["System", "Monitor"]),
];

const KINDS: &[CardKind] = &[
    CardKind {
        id: "overview",
        title: "Overview",
        about: "CPU, GPU, memory and temperatures",
        sizes: &[Size::S, Size::M],
        default: Size::S,
        half: true,
        sources: &["cpu", "memory", "gpu"],
    },
    CardKind {
        id: "memory",
        title: "Memory",
        about: "What uses memory; close what you do not need",
        sizes: &[Size::S],
        default: Size::S,
        half: true,
        sources: &["memory"],
    },
    CardKind {
        id: "gpu",
        title: "GPU",
        about: "Graphics card, video memory, heat",
        sizes: &[Size::S],
        default: Size::S,
        half: true,
        sources: &["gpu"],
    },
];

impl Provider for Monitor {
    fn id(&self) -> &'static str {
        "monitor"
    }
    fn name(&self) -> &'static str {
        "System monitor"
    }
    fn claims(&self) -> &'static [Claim] {
        CLAIMS
    }
    fn listed(&self) -> Listed {
        Listed::WithoutApp
    }
    fn cards(&self) -> &'static [CardKind] {
        KINDS
    }
    fn card(&self, kind: &str, argument: Option<&str>) -> Option<Box<dyn Card>> {
        if argument.is_some() {
            return None;
        }
        match kind {
            "overview" => Some(Box::new(OverviewCard::default())),
            "memory" => Some(Box::new(MemoryCard::default())),
            "gpu" => Some(Box::new(GpuCard::default())),
            _ => None,
        }
    }
    fn source(&self, key: &str) -> Option<Box<dyn Source>> {
        match key {
            "cpu" => Some(Box::new(CpuSource::real())),
            "memory" => Some(Box::new(MemorySource::real())),
            "gpu" => Some(Box::new(GpuSource::real())),
            _ => None,
        }
    }
}

fn render_fixture(key: &str) -> Option<String> {
    let path = std::env::var_os("INFYNITAE_EDGE_RENDER_DATA")?;
    let text = std::fs::read_to_string(path).ok()?;
    parse_render_data(&text).remove(key)
}

// ---------------------------------------------------------------------------------------------
// CPU (/proc/stat + /sys/class/hwmon)

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CpuData {
    pub busy_pct: Option<u8>,
    pub temp_c: Option<i32>,
}

/// Parses aggregate `cpu` totals from `/proc/stat` into `(idle_all, total)`.
pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|l| {
        let t = l.trim_start();
        t == "cpu" || t.starts_with("cpu ") || t.starts_with("cpu\t")
    })?;
    // Linux includes guest and guest_nice in user and nice, so sum only the first 8 columns.
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
    let idle_all = idle.saturating_add(iowait);
    Some((idle_all, total))
}

/// Computes CPU usage percentage from two `/proc/stat` snapshots `(idle, total)`.
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

/// Reads the CPU temperature from `k10temp` or `coretemp` inside `hwmon_dir`.
pub fn read_cpu_temp(hwmon_dir: &Path) -> Option<i32> {
    for driver in ["k10temp", "coretemp"] {
        if let Some(file) = find_hwmon_temp_file(hwmon_dir, Some(driver)) {
            if let Ok(content) = std::fs::read_to_string(file) {
                if let Some(temp) = parse_hwmon_temp(&content) {
                    return Some(temp);
                }
            }
        }
    }
    None
}

fn parse_cpu_fixture(text: &str) -> CpuData {
    if let Some((a, b)) = text.split_once("\n---\n") {
        let busy_pct = parse_proc_stat(a)
            .zip(parse_proc_stat(b))
            .and_then(|(p, c)| cpu_percent(p, c));
        let temp_c = b
            .lines()
            .chain(a.lines())
            .find_map(|l| l.trim().strip_prefix("temp="))
            .and_then(parse_hwmon_temp);
        return CpuData { busy_pct, temp_c };
    }
    let mut busy_pct = None;
    let mut temp_c = None;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(v) = line
            .strip_prefix("cpu_busy=")
            .or_else(|| line.strip_prefix("busy="))
        {
            busy_pct = v
                .trim()
                .trim_end_matches('%')
                .parse::<u32>()
                .ok()
                .map(|n| n.min(100) as u8);
        } else if let Some(v) = line
            .strip_prefix("cpu_temp=")
            .or_else(|| line.strip_prefix("temp="))
        {
            temp_c = parse_hwmon_temp(v);
        }
    }
    CpuData { busy_pct, temp_c }
}

pub struct CpuSource {
    pub proc_dir: PathBuf,
    pub hwmon_dir: PathBuf,
    pub prev: Option<(u64, u64)>,
}

impl CpuSource {
    pub fn real() -> Self {
        Self {
            proc_dir: PathBuf::from("/proc"),
            hwmon_dir: PathBuf::from("/sys/class/hwmon"),
            prev: None,
        }
    }
}

impl Source for CpuSource {
    fn fetch(&mut self) -> Data {
        if let Some(fixture) = render_fixture("cpu") {
            return Box::new(parse_cpu_fixture(&fixture));
        }
        let curr = std::fs::read_to_string(self.proc_dir.join("stat"))
            .ok()
            .as_deref()
            .and_then(parse_proc_stat);
        let busy_pct = match (self.prev, curr) {
            (Some(p), Some(c)) => cpu_percent(p, c),
            _ => None,
        };
        self.prev = curr;
        let temp_c = read_cpu_temp(&self.hwmon_dir);
        Box::new(CpuData { busy_pct, temp_c })
    }
}

// ---------------------------------------------------------------------------------------------
// Memory (/proc/meminfo + /proc/[pid]/status)

/// Reads `/proc/meminfo` and `/proc/[pid]/status` directly from `proc_dir` without spawning shells.
pub fn fetch_proc_memory_from(proc_dir: &Path) -> String {
    let mut out = std::fs::read_to_string(proc_dir.join("meminfo")).unwrap_or_default();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }

    let Ok(entries) = std::fs::read_dir(proc_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };
        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let status_path = entry.path().join("status");
        let Ok(status) = std::fs::read_to_string(status_path) else {
            continue;
        };
        let mut proc_name: Option<String> = None;
        let mut vm_rss: Option<u64> = None;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Name:") {
                let name = rest.trim();
                if is_skipped_process(name) {
                    break;
                }
                proc_name = Some(name.to_string());
            } else if let Some(rest) = line.strip_prefix("VmRSS:") {
                vm_rss = rest
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u64>().ok());
                break;
            }
        }
        if let (Some(name), Some(rss)) = (proc_name, vm_rss) {
            if rss > 0 && !is_skipped_process(&name) {
                out.push_str(&name);
                out.push('\t');
                out.push_str(&rss.to_string());
                out.push('\n');
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppMem {
    pub name: String,
    pub rss_kb: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryData {
    pub used_kb: u64,
    pub total_kb: u64,
    pub apps: Vec<AppMem>,
    /// Window `app_id`s passed via fixture (`window_apps=...`) when `cx.windows` is empty.
    pub window_app_ids: Vec<String>,
}

fn parse_kb_field(line: &str) -> Option<u64> {
    line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
}

fn is_skipped_process(name: &str) -> bool {
    let n = name.trim();
    n.is_empty()
        || n.starts_with('[')
        || n == "kthreadd"
        || n.starts_with("kworker/")
        || n.starts_with("ksoftirqd/")
        || n.starts_with("migration/")
        || n == "infynitae"
        || n.starts_with("infynitae-session")
        || n == "infynitae-edge"
        || n.starts_with("infynitae-edge-")
}

/// Parses `/proc/meminfo` lines into `(used_kb, total_kb)`.
pub fn parse_meminfo(out: &str) -> (u64, u64) {
    let mut total_kb: u64 = 0;
    let mut avail_kb: Option<u64> = None;
    let mut free_plus_cache_kb: u64 = 0;

    for raw in out.lines() {
        let line = raw.trim();
        if line.starts_with("MemTotal:") {
            total_kb = parse_kb_field(line).unwrap_or(total_kb);
        } else if line.starts_with("MemAvailable:") {
            avail_kb = parse_kb_field(line);
        } else if line.starts_with("MemFree:")
            || line.starts_with("Buffers:")
            || line.starts_with("Cached:")
        {
            free_plus_cache_kb += parse_kb_field(line).unwrap_or(0);
        }
    }
    let avail = avail_kb.unwrap_or(free_plus_cache_kb);
    let used_kb = total_kb.saturating_sub(avail);
    (used_kb, total_kb)
}

/// Parses process memory from `/proc/[pid]/status` (`Name:` + `VmRSS:`) or `<name>\t<rss_kb>`
/// lines, skipping kernel threads and the edge itself, summing RSS per process name and sorting
/// descending.
pub fn parse_proc_apps(out: &str) -> Vec<AppMem> {
    let mut sums: HashMap<String, u64> = HashMap::new();
    let mut cur_name: Option<String> = None;

    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Name:") {
            cur_name = Some(rest.trim().to_string());
            continue;
        }
        if line.starts_with("VmRSS:") {
            if let (Some(name), Some(kb)) = (cur_name.take(), parse_kb_field(line)) {
                if kb > 0 && !is_skipped_process(&name) {
                    *sums.entry(name).or_insert(0) += kb;
                }
            }
            continue;
        }
        if let Some((name, rhs)) = line.split_once('\t') {
            let name = name.trim();
            let kb = rhs
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .unwrap_or(0);
            if kb > 0 && !is_skipped_process(name) {
                *sums.entry(name.to_string()).or_insert(0) += kb;
            }
        }
    }

    let mut apps: Vec<AppMem> = sums
        .into_iter()
        .map(|(name, rss_kb)| AppMem { name, rss_kb })
        .collect();
    apps.sort_by(|a, b| b.rss_kb.cmp(&a.rss_kb).then_with(|| a.name.cmp(&b.name)));
    apps
}

/// Parses combined `/proc/meminfo` and process status output into [`MemoryData`].
pub fn parse_memory(out: &str) -> MemoryData {
    let (used_kb, total_kb) = parse_meminfo(out);
    let apps = parse_proc_apps(out);
    let mut window_app_ids = Vec::new();
    for raw in out.lines() {
        if let Some(rest) = raw.trim().strip_prefix("window_apps=") {
            window_app_ids = rest
                .split([',', ' ', '\t'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
        }
    }
    MemoryData {
        used_kb,
        total_kb,
        apps,
        window_app_ids,
    }
}

pub fn format_app_size(rss_kb: u64) -> String {
    if rss_kb >= 1048576 {
        format!("{:.1} GB", rss_kb as f64 / 1048576.0)
    } else {
        format!("{} MB", ((rss_kb + 512) / 1024).max(1))
    }
}

pub fn has_window(app_name: &str, window_app_ids: &[String]) -> bool {
    let lower = app_name.to_ascii_lowercase();
    window_app_ids.iter().any(|id| {
        let id_l = id.to_ascii_lowercase();
        !id_l.is_empty()
            && (id_l == lower
                || id_l.ends_with(&format!(".{lower}"))
                || id_l.starts_with(&lower)
                || lower.starts_with(&id_l))
    })
}

fn effective_window_ids(d: &MemoryData, windows: &[Window]) -> Vec<String> {
    let mut ids: Vec<String> = windows
        .iter()
        .map(|w| w.app_id.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    for id in &d.window_app_ids {
        let id = id.trim();
        if !id.is_empty() && !ids.iter().any(|x| x == id) {
            ids.push(id.to_string());
        }
    }
    ids
}

/// The process name an installed app starts as: its Exec's program, its id's last part, its
/// window class. A process that is none of these (the lab's python, a server, a build, this
/// terminal's tools) is never a background app to close.
pub fn is_installed_app(name: &str, apps: &[crate::app::App]) -> bool {
    let n = name.to_ascii_lowercase();
    apps.iter().any(|a| {
        let program = a
            .exec
            .split_whitespace()
            .find(|w| !w.contains('=') && *w != "env")
            .and_then(|w| w.rsplit('/').next())
            .map(|w| w.to_ascii_lowercase());
        // a program run through a launcher (flatpak run, sh -c) says nothing about its process
        let program = program
            .filter(|w| !matches!(w.as_str(), "flatpak" | "sh" | "bash" | "python" | "python3"));
        let id_last = a.id.rsplit('.').next().map(|w| w.to_ascii_lowercase());
        let class = a.wm_class.as_ref().map(|w| w.to_ascii_lowercase());
        // the kernel keeps 15 characters of a process name
        let same = |w: &String| *w == n || (n.len() == 15 && w.starts_with(&n));
        program.as_ref().is_some_and(same)
            || id_last.as_ref().is_some_and(same)
            || class.as_ref().is_some_and(same)
    })
}

pub fn background_apps<'a>(
    d: &'a MemoryData,
    windows: &[Window],
    apps: &[crate::app::App],
) -> Vec<&'a AppMem> {
    let ids = effective_window_ids(d, windows);
    if ids.is_empty() {
        return Vec::new();
    }
    d.apps
        .iter()
        .filter(|a| {
            !has_window(&a.name, &ids) && !protected(&a.name) && is_installed_app(&a.name, apps)
        })
        .collect()
}

pub fn close_bg_label(
    d: &MemoryData,
    windows: &[Window],
    apps: &[crate::app::App],
) -> Option<String> {
    let bg = background_apps(d, windows, apps);
    if bg.is_empty() {
        return None;
    }
    let freed_kb: u64 = bg.iter().map(|a| a.rss_kb).sum();
    let freed_gb = freed_kb as f64 / 1048576.0;
    let gb_str = if (freed_gb - freed_gb.round()).abs() < 0.05 || freed_gb >= 10.0 {
        format!("{}", (freed_gb.round() as u64).max(1))
    } else {
        format!("{freed_gb:.1}")
    };
    Some(format!("Close background apps · frees {gb_str} GB"))
}

fn shell_arg(name: &str) -> String {
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "'\\''"))
    }
}

/// Closes the app at `idx` (`0..3`) and returns the `pkill -TERM -x` command string.
pub fn close_app_action(d: &mut MemoryData, idx: usize) -> Option<String> {
    if idx >= d.apps.len().min(3) {
        return None;
    }
    if protected(&d.apps[idx].name) {
        return None;
    }
    let removed = d.apps.remove(idx);
    d.used_kb = d.used_kb.saturating_sub(removed.rss_kb);
    Some(format!("pkill -TERM -x {}", shell_arg(&removed.name)))
}

/// The session's own processes: the compositor and its helpers, sound, D-Bus, the keyring,
/// shells and system services. "Close background apps" never ends them.
pub fn protected(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("infynitae")
        || n.starts_with("xdg-")
        || n.starts_with("systemd")
        || n.starts_with("dbus")
        || n.starts_with("gnome-keyring")
        || n.starts_with("pipewire")
        || n.starts_with("gvfs")
        || n.starts_with("at-spi")
        || matches!(
            n.as_str(),
            "wireplumber"
                | "xwayland"
                | "xwayland-satellite"
                | "mako"
                | "squeekboard"
                | "sh"
                | "bash"
                | "zsh"
                | "fish"
                | "dash"
                | "agetty"
                | "login"
                | "sudo"
                | "polkitd"
                | "ssh-agent"
                | "gpg-agent"
                | "fcitx5"
                | "ibus-daemon"
        )
}

/// Closes all background apps (apps with no open window) and returns the shell command string.
pub fn close_bg_action(
    d: &mut MemoryData,
    windows: &[Window],
    apps: &[crate::app::App],
) -> Option<String> {
    let wins = effective_window_ids(d, windows);
    if wins.is_empty() {
        return None;
    }
    let mut cmds = Vec::new();
    let mut freed_kb = 0u64;
    d.apps.retain(|a| {
        // what keeps the session running is never a background app to close, window or not
        if has_window(&a.name, &wins) || protected(&a.name) || !is_installed_app(&a.name, apps) {
            true
        } else {
            freed_kb += a.rss_kb;
            cmds.push(format!("pkill -TERM -x {}", shell_arg(&a.name)));
            false
        }
    });
    if cmds.is_empty() {
        None
    } else {
        d.used_kb = d.used_kb.saturating_sub(freed_kb);
        Some(cmds.join("; "))
    }
}

pub struct MemorySource {
    pub proc_dir: PathBuf,
}

impl MemorySource {
    pub fn real() -> Self {
        Self {
            proc_dir: PathBuf::from("/proc"),
        }
    }
}

impl Source for MemorySource {
    fn fetch(&mut self) -> Data {
        let out =
            render_fixture("memory").unwrap_or_else(|| fetch_proc_memory_from(&self.proc_dir));
        Box::new(parse_memory(&out))
    }
}

// ---------------------------------------------------------------------------------------------
// GPU (/sys/class/drm + hwmon + amdgpu.ids + lab logs)

/// Parses temperature input (millidegrees C or raw degrees C) from a hwmon `temp*_input` file into integer degrees C.
pub fn parse_hwmon_temp(temp_input: &str) -> Option<i32> {
    temp_input
        .trim()
        .trim_end_matches("°C")
        .trim_end_matches('C')
        .trim()
        .parse::<i64>()
        .ok()
        .map(|n| {
            if n.abs() >= 1000 {
                let half = if n >= 0 { 500 } else { -500 };
                ((n + half) / 1000) as i32
            } else {
                n as i32
            }
        })
}

/// Discovers a hwmon directory and temperature file inside `hwmon_parent`, optionally matching `expected_name`.
pub fn find_hwmon_temp_file(hwmon_parent: &Path, expected_name: Option<&str>) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(hwmon_parent) else {
        return None;
    };
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
        let t1 = dir.join("temp1_input");
        if t1.is_file() {
            return Some(t1);
        }
        if let Ok(files) = std::fs::read_dir(&dir) {
            let mut temps: Vec<PathBuf> = files
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|s| s.starts_with("temp") && s.ends_with("_input"))
                })
                .collect();
            temps.sort();
            if let Some(first_temp) = temps.into_iter().next() {
                return Some(first_temp);
            }
        }
    }
    None
}

/// Reads temperature from a hwmon directory tree under `hwmon_parent`.
pub fn read_hwmon_temp(hwmon_parent: &Path) -> Option<i32> {
    let file = find_hwmon_temp_file(hwmon_parent, None)?;
    let content = std::fs::read_to_string(file).ok()?;
    parse_hwmon_temp(&content)
}

/// Looks up `dev` and `rev` (hex strings) in `/usr/share/libdrm/amdgpu.ids` text format.
pub fn lookup_amdgpu_ids(ids_text: &str, dev: &str, rev: &str) -> Option<String> {
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
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.splitn(3, ',').collect();
        if parts.len() == 3
            && parts[0].trim().eq_ignore_ascii_case(&dev_clean)
            && parts[1].trim().eq_ignore_ascii_case(&rev_clean)
        {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Discovers the first DRM card device path with telemetry in `drm_dir` (e.g. `/sys/class/drm`).
pub fn find_drm_card_device(drm_dir: &Path) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(drm_dir) else {
        return None;
    };
    let mut card_paths: Vec<(u32, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(s) = name.to_str() else {
            continue;
        };
        if let Some(num_str) = s.strip_prefix("card") {
            if !num_str.is_empty() && num_str.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(num) = num_str.parse::<u32>() {
                    card_paths.push((num, entry.path()));
                }
            }
        }
    }
    card_paths.sort_by_key(|(num, _)| *num);

    for (_, card_path) in card_paths {
        let d = card_path.join("device");
        if !d.is_dir() {
            continue;
        }
        let has_busy = d.join("gpu_busy_percent").is_file();
        let has_vram = d.join("mem_info_vram_used").is_file();
        let has_temp = find_hwmon_temp_file(&d.join("hwmon"), None).is_some();
        if has_busy || has_vram || has_temp {
            return Some(d);
        }
    }
    None
}

/// Reads the latest running training lab lines from `lab_dir` (`~/aevyternae-mind/logs/lab*.log`).
pub fn read_recent_lab_log(lab_dir: &Path) -> Option<String> {
    let Ok(entries) = std::fs::read_dir(lab_dir) else {
        return None;
    };
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(s) = name.to_str() else {
            continue;
        };
        if s.starts_with("lab") && s.ends_with(".log") {
            let path = entry.path();
            if let Ok(meta) = path.metadata() {
                if let Ok(mtime) = meta.modified() {
                    logs.push((mtime, path));
                }
            }
        }
    }
    logs.sort_by_key(|a| std::cmp::Reverse(a.0));

    for (_, path) in logs {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        if content.contains("=== ") {
            let mut lines = Vec::new();
            for raw in content.lines() {
                let line = raw.trim();
                if line.starts_with("=== ")
                    || (line.starts_with("LAB ") && line.ends_with("DONE"))
                    || line.starts_with("LAB DONE")
                    || line.starts_with("FAILED ")
                    || line.contains("stopped by hand")
                {
                    lines.push(line.to_string());
                }
            }
            if !lines.is_empty() {
                return Some(lines.join("\n"));
            }
            break;
        }
    }
    None
}

/// Reads GPU telemetry directly from DRM sysfs, hwmon, libdrm amdgpu.ids, and lab logs.
pub fn fetch_system_from(
    drm_dir: &Path,
    amdgpu_ids_path: &Path,
    lab_logs_dir: Option<&Path>,
    use_lspci: bool,
) -> String {
    let mut out = String::new();
    let mut found_gpu_name = false;

    if let Some(d) = find_drm_card_device(drm_dir) {
        if let Ok(busy) = std::fs::read_to_string(d.join("gpu_busy_percent")) {
            out.push_str("gpu_busy=");
            out.push_str(busy.trim());
            out.push('\n');
        } else {
            out.push_str("gpu_busy=\n");
        }
        if let Ok(vram_used) = std::fs::read_to_string(d.join("mem_info_vram_used")) {
            out.push_str("vram_used=");
            out.push_str(vram_used.trim());
            out.push('\n');
        } else {
            out.push_str("vram_used=\n");
        }
        if let Ok(vram_total) = std::fs::read_to_string(d.join("mem_info_vram_total")) {
            out.push_str("vram_total=");
            out.push_str(vram_total.trim());
            out.push('\n');
        } else {
            out.push_str("vram_total=\n");
        }
        if let Some(temp) = read_hwmon_temp(&d.join("hwmon")) {
            out.push_str("temp=");
            out.push_str(&temp.to_string());
            out.push('\n');
        } else {
            out.push_str("temp=\n");
        }
        if let Ok(prod) = std::fs::read_to_string(d.join("product_name")) {
            let trimmed = prod.trim();
            out.push_str("product_name=");
            out.push_str(trimmed);
            out.push('\n');
            if !trimmed.is_empty() {
                found_gpu_name = true;
            }
        } else {
            out.push_str("product_name=\n");
        }

        let dev_raw = std::fs::read_to_string(d.join("device")).unwrap_or_default();
        let dev = dev_raw
            .trim()
            .strip_prefix("0x")
            .unwrap_or(dev_raw.trim())
            .to_ascii_uppercase();
        let rev_raw = std::fs::read_to_string(d.join("revision")).unwrap_or_default();
        let rev = rev_raw
            .trim()
            .strip_prefix("0x")
            .unwrap_or(rev_raw.trim())
            .to_ascii_uppercase();

        if !found_gpu_name && !dev.is_empty() && amdgpu_ids_path.is_file() {
            if let Ok(ids_text) = std::fs::read_to_string(amdgpu_ids_path) {
                if let Some(match_line) = lookup_amdgpu_ids(&ids_text, &dev, &rev) {
                    out.push_str(&match_line);
                    out.push('\n');
                    found_gpu_name = true;
                }
            }
        }
    }

    if !found_gpu_name && use_lspci {
        if let Some(lspci_line) = fetch_lspci_fallback() {
            out.push_str(&lspci_line);
            out.push('\n');
        }
    }

    if let Some(lab_dir) = lab_logs_dir {
        if let Some(lab_text) = read_recent_lab_log(lab_dir) {
            out.push_str(&lab_text);
            out.push('\n');
        }
    }

    out
}

fn fetch_lspci_fallback() -> Option<String> {
    let text = run_sh("gpu_pci", "lspci 2>/dev/null");
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if (lower.contains("vga") || lower.contains("3d") || lower.contains("display"))
            && line.contains(':')
        {
            return Some(line.to_string());
        }
    }
    None
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuData {
    pub gpu_name: String,
    pub gpu_busy: Option<u8>,
    pub vram_used_gb: Option<f64>,
    pub vram_total_gb: Option<f64>,
    pub temp_c: Option<i32>,
    pub lab: Option<(String, String)>,
}

/// Parses a GPU display name from `product_name`, `/usr/share/libdrm/amdgpu.ids`, or `lspci`.
pub fn parse_gpu_name(out: &str) -> String {
    let mut from_lspci: Option<String> = None;
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("name=")
            .or_else(|| line.strip_prefix("product_name="))
        {
            let name = rest.trim();
            if !name.is_empty() {
                return clean_gpu_name(name);
            }
            continue;
        }
        // /usr/share/libdrm/amdgpu.ids format: "7550,\tC0,\tAMD Radeon RX 9070 XT"
        let parts: Vec<&str> = line.splitn(3, ',').collect();
        if parts.len() == 3
            && parts[0].trim().len() == 4
            && parts[0].trim().chars().all(|c| c.is_ascii_hexdigit())
            && !parts[1].trim().is_empty()
            && parts[1].trim().chars().all(|c| c.is_ascii_hexdigit())
        {
            let name = parts[2].trim();
            if !name.is_empty() {
                return clean_gpu_name(name);
            }
        }
        // lspci format: "03:00.0 VGA compatible controller: Advanced Micro Devices, Inc. [AMD/ATI] Navi 48 [Radeon RX 9070/9070 XT/9070 GRE] (rev c0)"
        let lower = line.to_ascii_lowercase();
        if (lower.contains("vga") || lower.contains("3d") || lower.contains("display"))
            && line.contains(':')
        {
            if let Some((_, after_type)) = line.split_once(": ") {
                from_lspci = Some(clean_lspci_name(after_type));
            }
        }
    }
    from_lspci.unwrap_or_default()
}

fn clean_gpu_name(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("AMD ").unwrap_or(s);
    s.trim().to_string()
}

fn clean_lspci_name(s: &str) -> String {
    let without_rev = match s.rfind(" (rev ") {
        Some(idx) => &s[..idx],
        None => s,
    };
    if let (Some(l), Some(r)) = (without_rev.rfind('['), without_rev.rfind(']')) {
        if r > l + 1 {
            let bracketed = without_rev[l + 1..r].trim();
            if !bracketed.eq_ignore_ascii_case("AMD/ATI") && !bracketed.is_empty() {
                return bracketed.to_string();
            }
        }
    }
    without_rev
        .replace("Advanced Micro Devices, Inc. [AMD/ATI] ", "AMD ")
        .replace("Intel Corporation ", "Intel ")
        .replace("NVIDIA Corporation ", "NVIDIA ")
        .trim()
        .to_string()
}

/// Parses `/sys/class/drm/card*/device` values (`gpu_busy_percent`, `mem_info_vram_used`,
/// `mem_info_vram_total`, `hwmon/hwmon*/temp1_input`) into `(gpu_busy_pct, vram_used_and_total_gb, temp_c)`.
pub fn parse_drm_sysfs(
    gpu_busy: &str,
    vram_used: &str,
    vram_total: &str,
    temp_input: &str,
) -> (Option<u8>, Option<(f64, f64)>, Option<i32>) {
    let busy = gpu_busy
        .trim()
        .trim_end_matches('%')
        .parse::<u32>()
        .ok()
        .map(|n| n.min(100) as u8);

    let parse_vram_val = |s: &str| -> Option<f64> {
        let t = s.trim().trim_end_matches("GB").trim();
        if t.is_empty() {
            return None;
        }
        if let Ok(bytes) = t.parse::<u64>() {
            if bytes > 1_000_000 {
                return Some(bytes as f64 / (1024.0 * 1024.0 * 1024.0));
            }
            return Some(bytes as f64);
        }
        t.parse::<f64>().ok()
    };

    let vram = parse_vram_val(vram_used).map(|used| {
        let total = parse_vram_val(vram_total).unwrap_or(0.0);
        (used, total)
    });

    let temp = parse_hwmon_temp(temp_input);

    (busy, vram, temp)
}

fn is_hh_mm(s: &str) -> bool {
    let Some((h, m)) = s.split_once(':') else {
        return false;
    };
    (1..=2).contains(&h.len())
        && m.len() == 2
        && h.chars().all(|c| c.is_ascii_digit())
        && m.chars().all(|c| c.is_ascii_digit())
}

/// Parses `logs/lab*.log` lines for a running lab (`"=== <run> HH:MM"` not followed by `LAB DONE`
/// or `LAB ... DONE`).
pub fn parse_lab_log(out: &str) -> Option<(String, String)> {
    let mut current: Option<(String, String)> = None;
    for raw in out.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("=== ") {
            let rest = rest.trim();
            if let Some((before_time, time)) = rest.rsplit_once(char::is_whitespace) {
                let time = time.trim();
                if is_hh_mm(time) {
                    let run = before_time
                        .split_once(" (")
                        .map(|(r, _)| r)
                        .unwrap_or(before_time)
                        .trim();
                    if !run.is_empty() {
                        current = Some((run.to_string(), time.to_string()));
                    }
                }
            }
        } else if line.starts_with("LAB DONE")
            || (line.starts_with("LAB ") && line.ends_with("DONE"))
            || line.starts_with("FAILED ")
            || line.contains("stopped by hand")
        {
            current = None;
        }
    }
    current
}

/// Parses the combined `system` / `gpu` output into [`GpuData`].
pub fn parse_system(out: &str) -> GpuData {
    let mut gpu_busy_str = String::new();
    let mut vram_used_str = String::new();
    let mut vram_total_str = String::new();
    let mut temp_str = String::new();

    for raw in out.lines() {
        let line = raw.trim();
        if let Some(v) = line.strip_prefix("gpu_busy=") {
            gpu_busy_str = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("vram_used=") {
            vram_used_str = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("vram_total=") {
            vram_total_str = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("temp=") {
            temp_str = v.trim().to_string();
        }
    }

    let (gpu_busy, vram, temp_c) =
        parse_drm_sysfs(&gpu_busy_str, &vram_used_str, &vram_total_str, &temp_str);
    let (vram_used_gb, vram_total_gb) = match vram {
        Some((u, t)) => (Some(u), (t > 0.0).then_some(t)),
        None => (None, None),
    };

    GpuData {
        gpu_name: parse_gpu_name(out),
        gpu_busy,
        vram_used_gb,
        vram_total_gb,
        temp_c,
        lab: parse_lab_log(out),
    }
}

pub fn has_columns(d: &GpuData) -> bool {
    d.gpu_busy.is_some() || d.vram_used_gb.is_some() || d.temp_c.is_some()
}

pub fn gpu_visible(d: &GpuData) -> bool {
    has_columns(d) || d.lab.is_some() || !d.gpu_name.is_empty()
}

pub struct GpuSource {
    pub drm_dir: PathBuf,
    pub amdgpu_ids: PathBuf,
    pub lab_dir: Option<PathBuf>,
    pub use_lspci: bool,
}

impl GpuSource {
    pub fn real() -> Self {
        let lab_dir = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join("aevyternae-mind/logs"));
        Self {
            drm_dir: PathBuf::from("/sys/class/drm"),
            amdgpu_ids: PathBuf::from("/usr/share/libdrm/amdgpu.ids"),
            lab_dir,
            use_lspci: true,
        }
    }
}

impl Source for GpuSource {
    fn fetch(&mut self) -> Data {
        let out = render_fixture("gpu")
            .or_else(|| render_fixture("system"))
            .unwrap_or_else(|| {
                fetch_system_from(
                    &self.drm_dir,
                    &self.amdgpu_ids,
                    self.lab_dir.as_deref(),
                    self.use_lspci,
                )
            });
        Box::new(parse_system(&out))
    }
}

// ---------------------------------------------------------------------------------------------
// Cards: Overview, Memory, GPU

const METER_H: f64 = 34.0;
const APP_ROW_H: f64 = 28.0;
const BG_ROW_H: f64 = 30.0;
const GPU_NAME_H: f64 = 20.0;
const GPU_COLS_H: f64 = 56.0;
const LAB_ROW_H: f64 = 34.0;

enum MeterValue {
    Plain(String),
    Split { primary: String, rest: String },
}

struct Meter {
    label: &'static str,
    value: MeterValue,
    temp: Option<String>,
    part: f64,
    fill: Rgba,
}

fn format_used_total_gb(used_kb: u64, total_kb: u64) -> (String, String, f64) {
    let used_gb = used_kb as f64 / 1048576.0;
    let total_gb = ((total_kb as f64 / 1048576.0).round() as u64).max(1);
    let part = (used_kb as f64 / total_kb as f64).clamp(0.0, 1.0);
    (format!("{used_gb:.1}"), format!(" of {total_gb} GB"), part)
}

fn draw_meter(cr: &Context, cx: &Cx, r: Rect, m: &Meter) {
    let th = cx.theme;
    let x = r.0 + 8.0;
    let w = (r.2 - 16.0).max(20.0);
    let ty = r.1 + 4.0;

    let mut right_w = 0.0;
    if let Some(t) = &m.temp {
        right_w += draw::text_right(
            cr,
            x + w,
            ty + 1.0,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: t,
            },
        ) + 6.0;
    }

    match &m.value {
        MeterValue::Plain(v) => {
            right_w += draw::text_right(
                cr,
                x + w - right_w,
                ty,
                &Text {
                    size: 13.0,
                    bold: true,
                    color: th.text,
                    text: v,
                },
            );
        }
        MeterValue::Split { primary, rest } => {
            let of_w = draw::text_right(
                cr,
                x + w - right_w,
                ty,
                &Text {
                    size: 13.0,
                    bold: false,
                    color: th.muted,
                    text: rest,
                },
            );
            let pri_w = draw::text_right(
                cr,
                x + w - right_w - of_w,
                ty,
                &Text {
                    size: 13.0,
                    bold: true,
                    color: th.text,
                    text: primary,
                },
            );
            right_w += of_w + pri_w;
        }
    }

    draw::text_max(
        cr,
        x,
        ty + 1.0,
        (w - right_w - 8.0).max(20.0),
        &Text {
            size: 12.0,
            bold: false,
            color: th.muted,
            text: m.label,
        },
    );

    draw::bar(cr, th, x, r.1 + 24.0, w, m.part, m.fill);
}

// ---------------------------------------------------------------------------------------------
// Overview card

#[derive(Default)]
pub struct OverviewCard {
    pub cpu: Option<CpuData>,
    pub memory: Option<MemoryData>,
    pub gpu: Option<GpuData>,
}

impl OverviewCard {
    fn meters(&self, cx: &Cx) -> Vec<Meter> {
        let th = cx.theme;
        let show_temp = cx.size == Size::M;
        let mut out = Vec::with_capacity(3);

        if let Some(c) = &self.cpu {
            let temp = show_temp.then_some(c.temp_c).flatten();
            match (c.busy_pct, temp) {
                (Some(pct), t) => out.push(Meter {
                    label: "CPU",
                    value: MeterValue::Plain(format!("{pct}%")),
                    temp: t.map(|v| format!("{v}°C")),
                    part: pct as f64 / 100.0,
                    fill: th.accent,
                }),
                // First /proc/stat fetch has no CPU % yet, so show CPU temp alone in M if present.
                (None, Some(t)) => out.push(Meter {
                    label: "CPU",
                    value: MeterValue::Plain(format!("{t}°C")),
                    temp: None,
                    part: (t as f64 / 100.0).clamp(0.0, 1.0),
                    fill: th.muted,
                }),
                (None, None) => {}
            }
        }

        if let Some(g) = &self.gpu {
            let temp = show_temp.then_some(g.temp_c).flatten();
            match (g.gpu_busy, temp) {
                (Some(busy), t) => out.push(Meter {
                    label: "GPU",
                    value: MeterValue::Plain(format!("{busy}%")),
                    temp: t.map(|v| format!("{v}°C")),
                    part: busy as f64 / 100.0,
                    fill: th.accent,
                }),
                (None, Some(t)) => out.push(Meter {
                    label: "GPU",
                    value: MeterValue::Plain(format!("{t}°C")),
                    temp: None,
                    part: (t as f64 / 100.0).clamp(0.0, 1.0),
                    fill: th.muted,
                }),
                (None, None) => {}
            }
        }

        if let Some(m) = &self.memory {
            if m.total_kb > 0 {
                let (used, rest, part) = format_used_total_gb(m.used_kb, m.total_kb);
                out.push(Meter {
                    label: "Memory",
                    value: MeterValue::Split {
                        primary: used,
                        rest,
                    },
                    temp: None,
                    part,
                    fill: th.accent,
                });
            }
        }

        out
    }
}

impl Card for OverviewCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        match key {
            "cpu" => {
                let Some(d) = data.downcast_ref::<CpuData>() else {
                    return false;
                };
                let changed = self.cpu.as_ref() != Some(d);
                self.cpu = Some(d.clone());
                changed
            }
            "memory" => {
                let Some(d) = data.downcast_ref::<MemoryData>() else {
                    return false;
                };
                let changed = self.memory.as_ref().map(|m| (m.used_kb, m.total_kb))
                    != Some((d.used_kb, d.total_kb));
                self.memory = Some(d.clone());
                changed
            }
            "gpu" => {
                let Some(d) = data.downcast_ref::<GpuData>() else {
                    return false;
                };
                let changed = self.gpu.as_ref().map(|g| (g.gpu_busy, g.temp_c))
                    != Some((d.gpu_busy, d.temp_c));
                self.gpu = Some(d.clone());
                changed
            }
            _ => false,
        }
    }

    fn status(&self, cx: &Cx) -> Status {
        if !self.meters(cx).is_empty() {
            return Status::Ready;
        }
        if self.cpu.is_some() && self.memory.is_some() && self.gpu.is_some() {
            Status::Missing("No system data available".into())
        } else {
            Status::Loading
        }
    }

    fn title(&self, cx: &Cx) -> Option<String> {
        cx.app.map(|_| "System monitor".to_string())
    }

    fn height(&self, cx: &Cx, _: f64) -> f64 {
        self.meters(cx).len() as f64 * METER_H
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let meters = self.meters(cx);
        for (i, m) in meters.iter().enumerate() {
            let r = (body.0, body.1 + i as f64 * METER_H, body.2, METER_H);
            if cx.app.is_some() && (hits.add(i as u32, r) || hits.selected(i as u32)) {
                ui::light(cr, cx, r);
            }
            draw_meter(cr, cx, r, m);
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let (
            Event::Press {
                hit,
                button: Button::Left,
                ..
            },
            Some(app),
        ) = (ev, cx.app)
        else {
            return false;
        };
        if (*hit as usize) < self.meters(cx).len() {
            out.push(Action::Launch(app.id.clone()));
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Memory card

const H_MEM_LAUNCH: u32 = 0;
const H_MEM_BG: u32 = 1;
const H_MEM_ROW: u32 = 10;
const H_MEM_CLOSE: u32 = 20;

#[derive(Default)]
pub struct MemoryCard {
    pub data: Option<MemoryData>,
}

impl Card for MemoryCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "memory", data.downcast_ref::<MemoryData>()) else {
            return false;
        };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
    }

    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if d.total_kb == 0 && d.apps.is_empty() => {
                Status::Missing("Memory information is not available".into())
            }
            Some(_) => Status::Ready,
        }
    }

    fn height(&self, cx: &Cx, _: f64) -> f64 {
        let Some(d) = &self.data else { return 0.0 };
        let summary_h = if d.total_kb > 0 { METER_H + 4.0 } else { 0.0 };
        let rows_h = d.apps.len().min(3) as f64 * APP_ROW_H;
        let bg_h = if close_bg_label(d, cx.windows, cx.apps).is_some() {
            4.0 + BG_ROW_H
        } else {
            0.0
        };
        summary_h + rows_h + bg_h
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else { return };
        let th = cx.theme;
        let mut y = body.1;

        if d.total_kb > 0 {
            let summary_r = (body.0, y, body.2, METER_H);
            if cx.app.is_some()
                && (hits.add(H_MEM_LAUNCH, summary_r) || hits.selected(H_MEM_LAUNCH))
            {
                ui::light(cr, cx, summary_r);
            }
            let (used, rest, part) = format_used_total_gb(d.used_kb, d.total_kb);
            draw_meter(
                cr,
                cx,
                summary_r,
                &Meter {
                    label: "Used",
                    value: MeterValue::Split {
                        primary: used,
                        rest,
                    },
                    temp: None,
                    part,
                    fill: th.accent,
                },
            );
            y += METER_H + 4.0;
        }

        for (i, app) in d.apps.iter().take(3).enumerate() {
            let row_r = (body.0, y, body.2, APP_ROW_H);
            if cx.app.is_some()
                && (hits.add(H_MEM_ROW + i as u32, row_r) || hits.selected(H_MEM_ROW + i as u32))
            {
                ui::light(cr, cx, row_r);
            }

            let can_close = !protected(&app.name);
            let close_w = if can_close { 24.0 } else { 0.0 };
            let size_str = format_app_size(app.rss_kb);
            let size_w = draw::text_right(
                cr,
                body.0 + body.2 - 8.0 - close_w,
                y + 6.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: &size_str,
                },
            );
            draw::text_max(
                cr,
                body.0 + 8.0,
                y + 5.0,
                (body.2 - 16.0 - size_w - close_w - 8.0).max(20.0),
                &Text {
                    size: 13.0,
                    bold: false,
                    color: th.text,
                    text: &app.name,
                },
            );

            if can_close {
                let x_r = (body.0 + body.2 - 28.0, y + 2.0, 24.0, APP_ROW_H - 4.0);
                let x_lit =
                    hits.add(H_MEM_CLOSE + i as u32, x_r) || hits.selected(H_MEM_CLOSE + i as u32);
                if x_lit {
                    ui::light(cr, cx, x_r);
                }
                let ic_x = (x_r.0 + x_r.2 / 2.0).round();
                let ic_y = (x_r.1 + x_r.3 / 2.0).round();
                draw::set(cr, if x_lit { th.text } else { th.muted });
                cr.set_line_width(1.5);
                cr.set_line_cap(cairo::LineCap::Round);
                cr.move_to(ic_x - 3.5, ic_y - 3.5);
                cr.line_to(ic_x + 3.5, ic_y + 3.5);
                cr.move_to(ic_x + 3.5, ic_y - 3.5);
                cr.line_to(ic_x - 3.5, ic_y + 3.5);
                let _ = cr.stroke();
            }

            y += APP_ROW_H;
        }

        if let Some(lbl) = close_bg_label(d, cx.windows, cx.apps) {
            y += 4.0;
            let btn_r = (body.0, y, body.2, BG_ROW_H);
            if hits.add(H_MEM_BG, btn_r) || hits.selected(H_MEM_BG) {
                ui::light(cr, cx, btn_r);
            }
            draw::text_max(
                cr,
                body.0 + 8.0,
                y + 7.0,
                (body.2 - 16.0).max(20.0),
                &Text {
                    size: 12.0,
                    bold: true,
                    color: th.accent,
                    text: &lbl,
                },
            );
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let (
            Event::Press {
                hit,
                button: Button::Left,
                ..
            },
            Some(d),
        ) = (ev, &mut self.data)
        else {
            return false;
        };

        if *hit == H_MEM_BG {
            if let Some(cmd) = close_bg_action(d, cx.windows, cx.apps) {
                out.push(Action::Run(cmd));
                return true;
            }
            return false;
        }
        if *hit >= H_MEM_CLOSE {
            let idx = (*hit - H_MEM_CLOSE) as usize;
            if let Some(cmd) = close_app_action(d, idx) {
                out.push(Action::Run(cmd));
                return true;
            }
            return false;
        }
        if (*hit == H_MEM_LAUNCH || (*hit >= H_MEM_ROW && *hit < H_MEM_CLOSE)) && cx.app.is_some() {
            if let Some(app) = cx.app {
                out.push(Action::Launch(app.id.clone()));
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------------------------
// GPU card

const H_GPU_LAUNCH: u32 = 0;
const H_GPU_PAUSE: u32 = 1;

#[derive(Default)]
pub struct GpuCard {
    pub data: Option<GpuData>,
}

pub fn pause_action() -> String {
    "touch ~/aevyternae-mind/logs/GAME_PAUSE".to_string()
}

impl Card for GpuCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "gpu", data.downcast_ref::<GpuData>()) else {
            return false;
        };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
    }

    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if !gpu_visible(d) => Status::Hidden,
            Some(_) => Status::Ready,
        }
    }

    fn height(&self, _: &Cx, _: f64) -> f64 {
        let Some(d) = &self.data else { return 0.0 };
        if !gpu_visible(d) {
            return 0.0;
        }
        let name_h = if d.gpu_name.is_empty() {
            0.0
        } else {
            GPU_NAME_H
        };
        let cols_h = if has_columns(d) { GPU_COLS_H } else { 0.0 };
        let lab_h = if d.lab.is_some() { LAB_ROW_H } else { 0.0 };
        name_h + cols_h + lab_h
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else { return };
        let th = cx.theme;
        let mut y = body.1;

        let name_h = if d.gpu_name.is_empty() {
            0.0
        } else {
            GPU_NAME_H
        };
        let cols_h = if has_columns(d) { GPU_COLS_H } else { 0.0 };
        if cx.app.is_some() && (name_h + cols_h > 0.0) {
            let launch_r = (body.0, body.1, body.2, name_h + cols_h);
            if hits.add(H_GPU_LAUNCH, launch_r) || hits.selected(H_GPU_LAUNCH) {
                ui::light(cr, cx, launch_r);
            }
        }

        if !d.gpu_name.is_empty() {
            draw::text_max(
                cr,
                body.0 + 8.0,
                y,
                (body.2 - 16.0).max(20.0),
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: &d.gpu_name,
                },
            );
            y += GPU_NAME_H;
        }

        let mut cols: Vec<(&str, String, f64, Rgba)> = Vec::with_capacity(3);
        if let Some(busy) = d.gpu_busy {
            cols.push(("GPU", format!("{busy}%"), (busy as f64) / 100.0, th.accent));
        }
        if let Some(used) = d.vram_used_gb {
            let total = d.vram_total_gb.unwrap_or(0.0);
            let part = if total > 0.0 {
                (used / total).clamp(0.0, 1.0)
            } else {
                0.0
            };
            cols.push(("VRAM", format!("{used:.1} GB"), part, th.accent));
        }
        if let Some(temp) = d.temp_c {
            let part = (temp as f64 / 100.0).clamp(0.0, 1.0);
            cols.push(("Temp", format!("{temp}°C"), part, th.muted));
        }

        if !cols.is_empty() {
            let n = cols.len() as f64;
            let inner_w = (body.2 - 16.0 - (n - 1.0) * draw::GAP).max(30.0);
            let col_w = inner_w / n;
            let val_size = if col_w < 75.0 { 15.0 } else { 22.0 };
            let cy = y + 2.0;
            for (i, (label, val, part, color)) in cols.iter().enumerate() {
                let col_x = body.0 + 8.0 + (i as f64) * (col_w + draw::GAP);
                draw::text_max(
                    cr,
                    col_x,
                    cy,
                    col_w,
                    &Text {
                        size: 11.0,
                        bold: false,
                        color: th.muted,
                        text: label,
                    },
                );
                draw::text_max(
                    cr,
                    col_x,
                    cy + 15.0,
                    col_w,
                    &Text {
                        size: val_size,
                        bold: true,
                        color: th.text,
                        text: val,
                    },
                );
                draw::bar(cr, th, col_x, cy + 45.0, col_w, *part, *color);
            }
            y += GPU_COLS_H;
        }

        if let Some((run, ends)) = &d.lab {
            let line_y = (y + 4.0).round() + 0.5;
            cr.move_to(body.0 + 8.0, line_y);
            cr.line_to(body.0 + body.2 - 8.0, line_y);
            draw::set(cr, th.track);
            cr.set_line_width(1.0);
            let _ = cr.stroke();

            let pause_t = Text {
                size: 12.0,
                bold: true,
                color: th.accent,
                text: "Pause",
            };
            let (pw, _) = draw::measure(cr, &pause_t);
            let pause_r = (body.0 + body.2 - 14.0 - pw, y + 7.0, pw + 12.0, 24.0);
            if hits.add(H_GPU_PAUSE, pause_r) || hits.selected(H_GPU_PAUSE) {
                ui::light(cr, cx, pause_r);
            }
            let ty = y + 11.0;
            draw::text_right(cr, body.0 + body.2 - 8.0, ty, &pause_t);

            let label = format!("Training {run} · ends {ends}");
            draw::text_max(
                cr,
                body.0 + 8.0,
                ty,
                (body.2 - 16.0 - pw - 16.0).max(20.0),
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: &label,
                },
            );
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let (
            Event::Press {
                hit,
                button: Button::Left,
                ..
            },
            Some(d),
        ) = (ev, &self.data)
        else {
            return false;
        };
        match *hit {
            H_GPU_PAUSE if d.lab.is_some() => {
                out.push(Action::Run(pause_action()));
                true
            }
            H_GPU_LAUNCH if cx.app.is_some() => {
                if let Some(app) = cx.app {
                    out.push(Action::Launch(app.id.clone()));
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::panel;
    use crate::provider;
    use crate::theme::Theme;

    fn test_cx<'a>(
        th: &'a Theme,
        size: Size,
        app: Option<&'a App>,
        windows: &'a [Window],
    ) -> Cx<'a> {
        Cx {
            theme: th,
            now: chrono::NaiveDate::from_ymd_opt(2026, 11, 13)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            size,
            app,
            apps: test_apps(),
            windows,
            notes: &[],
            list_above: false,
            editing: false,
        }
    }

    /// Installed apps for the tests: what "Close background apps" may end.
    fn test_apps() -> &'static [App] {
        static APPS: std::sync::OnceLock<Vec<App>> = std::sync::OnceLock::new();
        APPS.get_or_init(|| {
            [
                ("discord", "/usr/bin/discord"),
                ("Alacritty", "alacritty"),
                ("steam", "/usr/bin/steam %U"),
                ("firefox", "/usr/lib/firefox/firefox"),
                ("code", "code --unity-launch"),
                ("btop", "btop"),
            ]
            .iter()
            .map(|(id, exec)| App {
                id: id.to_string(),
                name: id.to_string(),
                exec: exec.to_string(),
                ..Default::default()
            })
            .collect()
        })
    }

    #[test]
    fn only_installed_apps_are_closed_in_the_background() {
        let mut d = MemoryData::default();
        for n in ["python", "llama-server", "node", "cargo", "steam"] {
            d.apps.push(AppMem {
                name: n.into(),
                rss_kb: 1000,
            });
        }
        let wins = [Window {
            id: 1,
            title: "Firefox".into(),
            app_id: "firefox".into(),
            focused: true,
        }];
        assert_eq!(
            close_bg_action(&mut d, &wins, test_apps()).as_deref(),
            Some("pkill -TERM -x steam")
        );
        assert_eq!(d.apps.len(), 4);
        assert!(close_bg_action(&mut d, &wins, test_apps()).is_none());
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "edge_apps_monitor_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn provider_claims_btop_and_lact_and_maps_old_words() {
        let p = provider::by_id("monitor").unwrap();
        assert_eq!(p.name(), "System monitor");
        assert_eq!(p.listed(), Listed::WithoutApp);
        assert!(p.card("overview", None).is_some());
        assert!(p.card("memory", None).is_some());
        assert!(p.card("gpu", None).is_some());
        assert!(p.card("overview", Some("x")).is_none());
        assert!(p.source("cpu").is_some());
        assert!(p.source("memory").is_some());
        assert!(p.source("gpu").is_some());
        assert!(p.source("unknown").is_none());

        let btop = App {
            id: "btop".into(),
            name: "btop++".into(),
            categories: vec!["System".into(), "Monitor".into(), "ConsoleOnly".into()],
            terminal: true,
            ..Default::default()
        };
        let lact = App {
            id: "io.github.ilya_zlobintsev.LACT".into(),
            name: "LACT".into(),
            categories: vec!["System".into()],
            ..Default::default()
        };
        let other_sys = App {
            id: "other".into(),
            name: "Other".into(),
            categories: vec!["System".into()],
            ..Default::default()
        };
        assert_eq!(provider::owner_of(&btop).unwrap().id(), "monitor");
        assert_eq!(provider::owner_of(&lact).unwrap().id(), "monitor");
        assert_eq!(provider::owner_of(&other_sys).unwrap().id(), "simple");

        let cfg = Config::from_text("PANEL_CONTROL=\"memory system\"\n");
        let control = cfg.panels.iter().find(|p| p.name == "control").unwrap();
        let cards = panel::cards(control);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].word(), "monitor/memory");
        assert_eq!(cards[1].word(), "monitor/gpu");
        for c in cards {
            let (prov, app) = provider::resolve(&c.owner, &[]).unwrap();
            assert!(app.is_none());
            assert!(prov.card(&c.kind, c.argument.as_deref()).is_some());
        }
    }

    #[test]
    fn parses_real_proc_stat_and_computes_two_read_delta() {
        let s1 = "\
cpu  1056813 150222 519124 9675566 33200 14459 6853 0 0 9751
cpu0 18198 11488 15647 908306 3989 1191 2821 0 0 1200
";
        let s2 = "\
cpu  1056913 150222 519174 9676416 33200 14459 6853 0 0 9999
cpu0 18298 11488 15697 909156 3989 1191 2821 0 0 1200
";
        let t1 = parse_proc_stat(s1).unwrap();
        let t2 = parse_proc_stat(s2).unwrap();
        assert_eq!(
            t1,
            (
                9675566 + 33200,
                1056813 + 150222 + 519124 + 9675566 + 33200 + 14459 + 6853
            )
        );
        // delta: +100 user, +50 sys, +850 idle => 150 busy / 1000 total = 15% (guest ignored)
        assert_eq!(cpu_percent(t1, t2), Some(15));
        assert_eq!(cpu_percent(t1, t1), None);
        assert_eq!(cpu_percent(t2, t1), None);
        assert_eq!(parse_proc_stat(""), None);
        assert_eq!(parse_proc_stat("cpu broken data here"), None);
        assert_eq!(parse_proc_stat("cpu0 100 0 50 850"), None);
    }

    #[test]
    fn cpu_source_reads_temp_dir_and_needs_two_fetches() {
        let dir = temp_dir("cpu_src");
        let proc_dir = dir.join("proc");
        let hwmon_dir = dir.join("hwmon");
        std::fs::create_dir_all(&proc_dir).unwrap();
        std::fs::create_dir_all(hwmon_dir.join("hwmon0")).unwrap();
        std::fs::create_dir_all(hwmon_dir.join("hwmon1")).unwrap();
        std::fs::create_dir_all(hwmon_dir.join("hwmon4")).unwrap();

        // Missing name file in hwmon0 must not match k10temp.
        std::fs::write(hwmon_dir.join("hwmon0/temp1_input"), "99000\n").unwrap();
        std::fs::write(hwmon_dir.join("hwmon1/name"), "nvme\n").unwrap();
        std::fs::write(hwmon_dir.join("hwmon1/temp1_input"), "38000\n").unwrap();
        std::fs::write(hwmon_dir.join("hwmon4/name"), "k10temp\n").unwrap();
        std::fs::write(hwmon_dir.join("hwmon4/temp1_input"), "50500\n").unwrap();
        std::fs::write(proc_dir.join("stat"), "cpu  100 0 50 850 0 0 0 0\n").unwrap();

        let mut src = CpuSource {
            proc_dir: proc_dir.clone(),
            hwmon_dir: hwmon_dir.clone(),
            prev: None,
        };
        let d1 = src.fetch().downcast::<CpuData>().unwrap();
        assert_eq!(
            *d1,
            CpuData {
                busy_pct: None,
                temp_c: Some(51)
            }
        );

        std::fs::write(proc_dir.join("stat"), "cpu  180 0 70 1150 0 0 0 0\n").unwrap();
        let d2 = src.fetch().downcast::<CpuData>().unwrap();
        // +100 busy / +400 total = 25%
        assert_eq!(
            *d2,
            CpuData {
                busy_pct: Some(25),
                temp_c: Some(51)
            }
        );

        // coretemp also works; empty dir yields None.
        std::fs::remove_dir_all(hwmon_dir.join("hwmon4")).unwrap();
        std::fs::create_dir_all(hwmon_dir.join("hwmon2")).unwrap();
        std::fs::write(hwmon_dir.join("hwmon2/name"), "coretemp\n").unwrap();
        std::fs::write(hwmon_dir.join("hwmon2/temp1_input"), "62000\n").unwrap();
        assert_eq!(read_cpu_temp(&hwmon_dir), Some(62));

        let mut empty_src = CpuSource {
            proc_dir: dir.join("missing_proc"),
            hwmon_dir: dir.join("missing_hwmon"),
            prev: None,
        };
        let d_empty = empty_src.fetch().downcast::<CpuData>().unwrap();
        assert_eq!(
            *d_empty,
            CpuData {
                busy_pct: None,
                temp_c: None
            }
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_real_meminfo_and_proc_status_summing_by_app_and_skipping_kernel_and_edge() {
        let sample = "\
MemTotal:       33554432 kB
MemFree:         4181216 kB
MemAvailable:   13002342 kB
Buffers:             228 kB
Cached:         15386976 kB
Name:\tkthreadd
State:\tS (sleeping)
Name:\tkworker/u64:1-events_unbound
State:\tI (idle)
Name:\tinfynitae-edge
VmRSS:\t   37264 kB
Name:\tinfynitae-edge-
VmRSS:\t   28400 kB
Name:\tfirefox
VmRSS:\t 2800000 kB
Name:\tfirefox
VmRSS:\t 1604019 kB
Name:\tcode
VmRSS:\t 2306867 kB
Name:\talacritty
VmRSS:\t  655360 kB
Name:\tpipewire
VmRSS:\t   26188 kB
";
        let d = parse_memory(sample);
        assert_eq!(d.total_kb, 33554432);
        assert_eq!(d.used_kb, 33554432 - 13002342);
        let used_gb = d.used_kb as f64 / 1048576.0;
        assert!((used_gb - 19.6).abs() < 0.01);

        assert_eq!(d.apps.len(), 4);
        assert_eq!(
            d.apps[0],
            AppMem {
                name: "firefox".into(),
                rss_kb: 4404019
            }
        );
        assert_eq!(
            d.apps[1],
            AppMem {
                name: "code".into(),
                rss_kb: 2306867
            }
        );
        assert_eq!(
            d.apps[2],
            AppMem {
                name: "alacritty".into(),
                rss_kb: 655360
            }
        );
        assert_eq!(format_app_size(d.apps[0].rss_kb), "4.2 GB");
        assert_eq!(format_app_size(d.apps[1].rss_kb), "2.2 GB");
        assert_eq!(format_app_size(d.apps[2].rss_kb), "640 MB");

        assert!(d.window_app_ids.is_empty());
        assert_eq!(close_bg_label(&d, &[], test_apps()), None);
    }

    #[test]
    fn parses_full_kernel_meminfo_layout_and_broken_inputs() {
        let meminfo = "\
MemTotal:       32453284 kB
MemFree:         2125112 kB
MemAvailable:   20688720 kB
Buffers:             428 kB
Cached:         20143660 kB
SwapCached:          308 kB
Active:          7240876 kB
Inactive:       19639648 kB
";
        let (used_kb, total_kb) = parse_meminfo(meminfo);
        assert_eq!(total_kb, 32453284);
        assert_eq!(used_kb, 32453284 - 20688720);

        assert_eq!(parse_memory(""), MemoryData::default());
        assert_eq!(
            parse_memory("MemTotal: invalid kB\nName:\tfoo\nVmRSS:\tabc kB\n"),
            MemoryData::default()
        );
    }

    #[test]
    fn reads_proc_meminfo_and_process_status_from_directory() {
        let dir = temp_dir("proc_mem");
        std::fs::write(
            dir.join("meminfo"),
            "MemTotal:       32453284 kB\nMemAvailable:   20688720 kB\n",
        )
        .unwrap();

        let p101 = dir.join("101");
        std::fs::create_dir_all(&p101).unwrap();
        std::fs::write(
            p101.join("status"),
            "Name:\tfirefox\nState:\tS\nVmRSS:\t 2800000 kB\n",
        )
        .unwrap();

        let p102 = dir.join("102");
        std::fs::create_dir_all(&p102).unwrap();
        std::fs::write(
            p102.join("status"),
            "Name:\tfirefox\nState:\tS\nVmRSS:\t 1604019 kB\n",
        )
        .unwrap();

        let p103 = dir.join("103");
        std::fs::create_dir_all(&p103).unwrap();
        std::fs::write(p103.join("status"), "Name:\tkworker/u64:1\nState:\tI\n").unwrap();

        let mut src = MemorySource {
            proc_dir: dir.clone(),
        };
        let d = src.fetch().downcast::<MemoryData>().unwrap();
        assert_eq!(d.total_kb, 32453284);
        assert_eq!(d.used_kb, 32453284 - 20688720);
        assert_eq!(d.apps.len(), 1);
        assert_eq!(d.apps[0].name, "firefox");
        assert_eq!(d.apps[0].rss_kb, 2800000 + 1604019);

        let mut empty_src = MemorySource {
            proc_dir: dir.join("missing"),
        };
        assert_eq!(
            *empty_src.fetch().downcast::<MemoryData>().unwrap(),
            MemoryData::default()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compositor_and_session_processes_are_never_in_top_apps_or_killed() {
        let sample = "\
MemTotal: 33554432 kB
MemAvailable: 13002342 kB
infynitae\t8000000
infynitae-session\t500000
firefox\t4404019
";
        let mut d = parse_memory(sample);
        assert!(!d
            .apps
            .iter()
            .any(|a| a.name == "infynitae" || a.name.starts_with("infynitae-session")));
        assert_eq!(d.apps.len(), 1);
        assert_eq!(d.apps[0].name, "firefox");

        d.apps.insert(
            0,
            AppMem {
                name: "infynitae".into(),
                rss_kb: 8000000,
            },
        );
        assert_eq!(close_app_action(&mut d, 0), None);
    }

    #[test]
    fn the_session_is_never_a_background_app() {
        let mut d = MemoryData::default();
        for n in ["infynitae", "pipewire", "wireplumber", "steam", "firefox"] {
            d.apps.push(AppMem {
                name: n.into(),
                rss_kb: 1000,
            });
        }
        let wins = [Window {
            id: 1,
            title: "Firefox".into(),
            app_id: "firefox".into(),
            focused: true,
        }];
        let cmd = close_bg_action(&mut d, &wins, test_apps()).unwrap();
        assert_eq!(cmd, "pkill -TERM -x steam");
    }

    #[test]
    fn parses_real_amd_sysfs_and_lab_outputs() {
        let sample = "\
gpu_busy=67
vram_used=7807320064
vram_total=17095983104
temp=57000
product_name=
7550,\tC0,\tAMD Radeon RX 9070 XT
03:00.0 VGA compatible controller: Advanced Micro Devices, Inc. [AMD/ATI] Navi 48 [Radeon RX 9070/9070 XT/9070 GRE] (rev c0)
=== labv15-pushback-s0 20:36
LAB DONE labv15-pushback-s0: {\"wiki\": 4.108}
=== labv15-pushback-s1 20:55
";
        let d = parse_system(sample);
        assert_eq!(d.gpu_name, "Radeon RX 9070 XT");
        assert_eq!(d.gpu_busy, Some(67));
        let vram_used = d.vram_used_gb.unwrap();
        let vram_total = d.vram_total_gb.unwrap();
        assert!((vram_used - 7.271).abs() < 0.01);
        assert!((vram_total - 15.922).abs() < 0.01);
        assert_eq!(d.temp_c, Some(57));
        assert_eq!(
            d.lab,
            Some(("labv15-pushback-s1".to_string(), "20:55".to_string()))
        );
    }

    #[test]
    fn hides_missing_metrics_on_other_gpus_and_completed_labs() {
        let intel_sample = "\
gpu_busy=
vram_used=
vram_total=
temp=48000
product_name=
00:02.0 VGA compatible controller: Intel Corporation Alder Lake-P Integrated Graphics Controller (rev 0c)
=== laba5b-stop-s2 19:07
LAB DONE laba5b-stop-s2: {\"wiki\": 4.097}
LAB A5b DONE
";
        let d = parse_system(intel_sample);
        assert_eq!(
            d.gpu_name,
            "Intel Alder Lake-P Integrated Graphics Controller"
        );
        assert_eq!(d.gpu_busy, None);
        assert_eq!(d.vram_used_gb, None);
        assert_eq!(d.vram_total_gb, None);
        assert_eq!(d.temp_c, Some(48));
        assert_eq!(d.lab, None);

        assert_eq!(parse_lab_log("=== labx 20:00\nstopped by hand\n"), None);
        assert_eq!(parse_system(""), GpuData::default());
        assert_eq!(
            parse_system("gpu_busy=bad\ntemp=nope\n"),
            GpuData::default()
        );
    }

    #[test]
    fn parses_hwmon_and_drm_sysfs_trees_in_temp_dir() {
        let dir = temp_dir("drm_sysfs");
        let card1_dev = dir.join("card1/device");
        let hwmon2 = card1_dev.join("hwmon/hwmon2");
        std::fs::create_dir_all(&hwmon2).unwrap();

        std::fs::write(card1_dev.join("gpu_busy_percent"), "67\n").unwrap();
        std::fs::write(card1_dev.join("mem_info_vram_used"), "7807320064\n").unwrap();
        std::fs::write(card1_dev.join("mem_info_vram_total"), "17095983104\n").unwrap();
        std::fs::write(card1_dev.join("device"), "0x7550\n").unwrap();
        std::fs::write(card1_dev.join("revision"), "0xc0\n").unwrap();
        std::fs::write(hwmon2.join("name"), "amdgpu\n").unwrap();
        std::fs::write(hwmon2.join("temp1_input"), "57000\n").unwrap();

        let amdgpu_ids = dir.join("amdgpu.ids");
        std::fs::write(&amdgpu_ids, "7550,\tC0,\tAMD Radeon RX 9070 XT\n").unwrap();

        assert_eq!(read_hwmon_temp(&card1_dev.join("hwmon")), Some(57));

        let mut src = GpuSource {
            drm_dir: dir.clone(),
            amdgpu_ids: amdgpu_ids.clone(),
            lab_dir: None,
            use_lspci: false,
        };
        let d = src.fetch().downcast::<GpuData>().unwrap();
        assert_eq!(d.gpu_busy, Some(67));
        assert_eq!(d.gpu_name, "Radeon RX 9070 XT");
        assert_eq!(d.temp_c, Some(57));
        assert!((d.vram_used_gb.unwrap() - 7.271).abs() < 0.01);
        assert!((d.vram_total_gb.unwrap() - 15.922).abs() < 0.01);

        let mut empty_src = GpuSource {
            drm_dir: dir.join("missing"),
            amdgpu_ids,
            lab_dir: None,
            use_lspci: false,
        };
        assert_eq!(
            *empty_src.fetch().downcast::<GpuData>().unwrap(),
            GpuData::default()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overview_card_status_sizes_and_launch_events() {
        let th = Theme::from_config(&Config::from_text(""));
        let btop = App {
            id: "btop".into(),
            name: "btop++".into(),
            ..Default::default()
        };
        let cx_s = test_cx(&th, Size::S, Some(&btop), &[]);
        let cx_m = test_cx(&th, Size::M, Some(&btop), &[]);
        let cx_no_app = test_cx(&th, Size::M, None, &[]);

        let mut card = OverviewCard::default();
        assert_eq!(card.status(&cx_s), Status::Loading);
        assert_eq!(card.title(&cx_m).as_deref(), Some("System monitor"));
        assert_eq!(card.title(&cx_no_app), None);

        // First fetch: no CPU %, only temp + mem + gpu.
        assert!(card.apply(
            "cpu",
            &CpuData {
                busy_pct: None,
                temp_c: Some(48)
            } as &dyn Any
        ));
        assert!(card.apply(
            "memory",
            &parse_memory("MemTotal: 33554432 kB\nMemAvailable: 13002342 kB\n") as &dyn Any
        ));
        assert!(card.apply(
            "gpu",
            &parse_system("gpu_busy=97\ntemp=71000\n") as &dyn Any
        ));
        assert_eq!(card.status(&cx_s), Status::Ready);
        assert_eq!(
            card.meters(&cx_s).len(),
            2,
            "S hides CPU when CPU % is None"
        );
        assert_eq!(
            card.meters(&cx_m).len(),
            3,
            "M shows CPU temp row even before 2nd CPU fetch"
        );

        // Second CPU fetch adds CPU %.
        assert!(card.apply(
            "cpu",
            &CpuData {
                busy_pct: Some(23),
                temp_c: Some(48)
            } as &dyn Any
        ));
        assert!(!card.apply(
            "cpu",
            &CpuData {
                busy_pct: Some(23),
                temp_c: Some(48)
            } as &dyn Any
        ));
        assert_eq!(card.meters(&cx_s).len(), 3);
        assert_eq!(card.meters(&cx_m).len(), 3);
        assert_eq!(card.meters(&cx_m)[0].temp.as_deref(), Some("48°C"));
        assert_eq!(card.meters(&cx_m)[1].temp.as_deref(), Some("71°C"));

        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: 0,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_m,
            &mut out
        ));
        assert_eq!(out, vec![Action::Launch("btop".into())]);

        let mut out_no_app = Vec::new();
        assert!(!card.event(
            &Event::Press {
                hit: 0,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_no_app,
            &mut out_no_app
        ));
        assert!(out_no_app.is_empty());

        // Missing when all 3 sources report nothing readable.
        let mut empty = OverviewCard::default();
        empty.apply("cpu", &CpuData::default() as &dyn Any);
        empty.apply("memory", &MemoryData::default() as &dyn Any);
        empty.apply("gpu", &GpuData::default() as &dyn Any);
        assert_eq!(
            empty.status(&cx_s),
            Status::Missing("No system data available".into())
        );
    }

    #[test]
    fn memory_card_status_and_events_with_cx_windows() {
        let th = Theme::from_config(&Config::from_text(""));
        let btop = App {
            id: "btop".into(),
            name: "btop++".into(),
            ..Default::default()
        };
        let wins = [Window {
            id: 1,
            title: "Firefox".into(),
            app_id: "firefox".into(),
            focused: true,
        }];
        let cx_no_win = test_cx(&th, Size::S, Some(&btop), &[]);
        let cx_win = test_cx(&th, Size::S, Some(&btop), &wins);

        let mut card = MemoryCard::default();
        assert_eq!(card.status(&cx_no_win), Status::Loading);

        card.apply(
            "memory",
            &parse_memory(
                "MemTotal: 33554432 kB\nMemAvailable: 13002342 kB\n\
                 firefox\t4404019\ncode\t2097152\ndiscord\t1048576\nalacritty\t524288\n",
            ) as &dyn Any,
        );
        assert_eq!(card.status(&cx_no_win), Status::Ready);

        // Close row 1 ("code") via x hit.
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_MEM_CLOSE + 1,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_no_win,
            &mut out
        ));
        assert_eq!(out, vec![Action::Run("pkill -TERM -x code".into())]);
        assert_eq!(card.data.as_ref().unwrap().apps.len(), 3);

        // Click summary row launches tied app.
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_MEM_LAUNCH,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_no_win,
            &mut out
        ));
        assert_eq!(out, vec![Action::Launch("btop".into())]);

        // With cx.windows populated, Close background apps closes discord + alacritty.
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_MEM_BG,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_win,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Run(
                "pkill -TERM -x discord; pkill -TERM -x alacritty".into()
            )]
        );
        assert_eq!(card.data.as_ref().unwrap().apps.len(), 1);

        // Empty data gives Missing.
        let mut empty = MemoryCard::default();
        empty.apply("memory", &MemoryData::default() as &dyn Any);
        assert_eq!(
            empty.status(&cx_no_win),
            Status::Missing("Memory information is not available".into())
        );
    }

    #[test]
    fn gpu_card_status_and_events() {
        let th = Theme::from_config(&Config::from_text(""));
        let lact = App {
            id: "io.github.ilya_zlobintsev.LACT".into(),
            name: "LACT".into(),
            ..Default::default()
        };
        let cx_no_app = test_cx(&th, Size::S, None, &[]);
        let cx_app = test_cx(&th, Size::S, Some(&lact), &[]);

        let mut card = GpuCard::default();
        assert_eq!(card.status(&cx_no_app), Status::Loading);

        card.apply("gpu", &GpuData::default() as &dyn Any);
        assert_eq!(card.status(&cx_no_app), Status::Hidden);

        card.apply(
            "gpu",
            &parse_system(
                "gpu_busy=67\nvram_used=7807320064\nvram_total=17095983104\ntemp=57000\n\
                 7550,\tC0,\tAMD Radeon RX 9070 XT\n=== labv15-pushback-s1 20:55\n",
            ) as &dyn Any,
        );
        assert_eq!(card.status(&cx_no_app), Status::Ready);

        // Clicking Pause runs touch GAME_PAUSE.
        let mut out = Vec::new();
        assert!(card.event(
            &Event::Press {
                hit: H_GPU_PAUSE,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_no_app,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Run(
                "touch ~/aevyternae-mind/logs/GAME_PAUSE".into()
            )]
        );

        // Clicking metrics with no tied app does nothing; with LACT tied it launches LACT.
        let mut out = Vec::new();
        assert!(!card.event(
            &Event::Press {
                hit: H_GPU_LAUNCH,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_no_app,
            &mut out
        ));
        assert!(card.event(
            &Event::Press {
                hit: H_GPU_LAUNCH,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx_app,
            &mut out
        ));
        assert_eq!(
            out,
            vec![Action::Launch("io.github.ilya_zlobintsev.LACT".into())]
        );
    }

    #[test]
    fn heights_match_drawn_bounds_across_sizes() {
        let th = Theme::from_config(&Config::from_text(""));
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 400).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let btop = App {
            id: "btop".into(),
            name: "btop++".into(),
            ..Default::default()
        };
        let wins = [Window {
            id: 1,
            title: "Firefox".into(),
            app_id: "firefox".into(),
            focused: true,
        }];

        // Overview S and M
        let mut ov = OverviewCard::default();
        ov.apply(
            "cpu",
            &CpuData {
                busy_pct: Some(23),
                temp_c: Some(48),
            } as &dyn Any,
        );
        ov.apply(
            "memory",
            &parse_memory("MemTotal: 33554432 kB\nMemAvailable: 13002342 kB\n") as &dyn Any,
        );
        ov.apply(
            "gpu",
            &parse_system("gpu_busy=97\ntemp=71000\n") as &dyn Any,
        );
        for size in [Size::S, Size::M] {
            let cx = test_cx(&th, size, Some(&btop), &[]);
            let h = ov.height(&cx, 360.0);
            assert_eq!(h, 3.0 * METER_H);
            let mut hits = Hits::default();
            ov.draw(&cr, &cx, (10.0, 40.0, 360.0, h), &mut hits);
            assert_eq!(hits.list.len(), 3);
            let max_y = hits
                .list
                .iter()
                .map(|(_, r)| r.1 + r.3)
                .fold(0.0f64, f64::max);
            assert!((max_y - (40.0 + h)).abs() < 1e-6);
        }

        // Memory with and without background apps button
        let mut mem = MemoryCard::default();
        mem.apply(
            "memory",
            &parse_memory(
                "MemTotal: 33554432 kB\nMemAvailable: 13002342 kB\n\
                 firefox\t4404019\ndiscord\t1048576\n",
            ) as &dyn Any,
        );
        let cx_no_win = test_cx(&th, Size::S, Some(&btop), &[]);
        let h_no_bg = mem.height(&cx_no_win, 360.0);
        assert_eq!(h_no_bg, METER_H + 4.0 + 2.0 * APP_ROW_H);

        let cx_win = test_cx(&th, Size::S, Some(&btop), &wins);
        let h_bg = mem.height(&cx_win, 360.0);
        assert_eq!(h_bg, METER_H + 4.0 + 2.0 * APP_ROW_H + 4.0 + BG_ROW_H);
        let mut hits = Hits::default();
        mem.draw(&cr, &cx_win, (10.0, 40.0, 360.0, h_bg), &mut hits);
        let max_y = hits
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0f64, f64::max);
        assert!((max_y - (40.0 + h_bg)).abs() < 1e-6);

        // GPU with name + cols + lab
        let mut gpu = GpuCard::default();
        gpu.apply(
            "gpu",
            &parse_system(
                "gpu_busy=67\nvram_used=7807320064\nvram_total=17095983104\ntemp=57000\n\
                 7550,\tC0,\tAMD Radeon RX 9070 XT\n=== labv15-pushback-s1 20:55\n",
            ) as &dyn Any,
        );
        let h_gpu = gpu.height(&cx_no_win, 360.0);
        assert_eq!(h_gpu, GPU_NAME_H + GPU_COLS_H + LAB_ROW_H);
        let mut hits = Hits::default();
        gpu.draw(&cr, &cx_no_win, (10.0, 40.0, 360.0, h_gpu), &mut hits);
        assert_eq!(hits.list.len(), 2);
        assert!(hits
            .list
            .iter()
            .all(|(_, r)| r.1 >= 40.0 && r.1 + r.3 <= 40.0 + h_gpu));
    }
}
