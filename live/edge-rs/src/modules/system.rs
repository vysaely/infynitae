//! The `system` module: GPU name, three columns (GPU use %, VRAM used in GB, GPU temperature in C
//! with 3 px bars), and the running training lab line (`"Training <run> · ends HH:MM"` with a
//! `"Pause"` button that touches `~/aevyternae-mind/logs/GAME_PAUSE`).

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{is_left_button, Rect, World};
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const COLS_H: f64 = 80.0;
pub const LAB_ROW_H: f64 = 34.0;

pub const CMDS: &[(&str, &str)] = &[("system", "__sysfs_system__")];

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
                ((n + 500) / 1000) as i32
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
            let name_path = dir.join("name");
            if let Ok(name) = std::fs::read_to_string(name_path) {
                if !name.trim().eq_ignore_ascii_case(expected) {
                    continue;
                }
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

/// Parses the CPU total and idle ticks from a `cpu ` line in `/proc/stat`.
#[allow(dead_code)]
pub fn parse_cpu_stat_line(line: &str) -> Option<(u64, u64)> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "cpu" {
        return None;
    }
    let user: u64 = parts.next()?.parse().ok()?;
    let nice: u64 = parts.next()?.parse().ok()?;
    let system: u64 = parts.next()?.parse().ok()?;
    let idle: u64 = parts.next()?.parse().ok()?;
    let iowait: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let irq: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let softirq: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let steal: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    let idle_total = idle + iowait;
    let total = user + nice + system + idle_total + irq + softirq + steal;
    Some((total, idle_total))
}

/// Parses the CPU busy percentage between two reads of `/proc/stat`.
#[allow(dead_code)]
pub fn parse_cpu_stat(first: &str, second: &str) -> Option<f64> {
    let line1 = first.lines().find(|l| l.starts_with("cpu "))?;
    let line2 = second.lines().find(|l| l.starts_with("cpu "))?;
    let (t1, idle1) = parse_cpu_stat_line(line1)?;
    let (t2, idle2) = parse_cpu_stat_line(line2)?;
    let delta_total = t2.saturating_sub(t1);
    let delta_idle = idle2.saturating_sub(idle1);
    if delta_total == 0 {
        return Some(0.0);
    }
    let busy = delta_total.saturating_sub(delta_idle);
    Some((busy as f64 / delta_total as f64 * 100.0).clamp(0.0, 100.0))
}

/// Parses battery capacity and status from `/sys/class/power_supply/BAT*` files into
/// `(capacity_percent, is_charging)`.
#[allow(dead_code)]
pub fn parse_battery(capacity_text: &str, status_text: &str) -> (Option<u8>, bool) {
    let cap = capacity_text.trim().parse::<u8>().ok().map(|c| c.min(100));
    let st = status_text.trim();
    let charging = st.eq_ignore_ascii_case("charging") || st.eq_ignore_ascii_case("full");
    (cap, charging)
}

/// Reads the first battery (`BAT*`) found in `power_supply_dir`, returning `(capacity, is_charging)`.
#[allow(dead_code)]
pub fn read_battery_from(power_supply_dir: &Path) -> Option<(u8, bool)> {
    let Ok(entries) = std::fs::read_dir(power_supply_dir) else {
        return None;
    };
    let mut bat_entries: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let s = name.to_str()?;
            if s.starts_with("BAT") {
                Some(e.path())
            } else {
                None
            }
        })
        .collect();
    bat_entries.sort();
    for bat_path in bat_entries {
        let cap_path = bat_path.join("capacity");
        let st_path = bat_path.join("status");
        if let Ok(cap_str) = std::fs::read_to_string(cap_path) {
            let st_str = std::fs::read_to_string(st_path).unwrap_or_default();
            let (cap, charging) = parse_battery(&cap_str, &st_str);
            if let Some(c) = cap {
                return Some((c, charging));
            }
        }
    }
    None
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

/// Reads system telemetry directly from DRM sysfs, hwmon, libdrm amdgpu.ids, and lab logs without spawning shells.
pub fn fetch_system_from(
    drm_dir: &Path,
    amdgpu_ids_path: &Path,
    lab_logs_dir: Option<&Path>,
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

    if !found_gpu_name {
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
    let o = Command::new("lspci")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&o.stdout);
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

/// Returns the formatted system string for consumption by [`parse_system`].
pub fn fetch_string() -> String {
    let home = std::env::var("HOME").ok();
    let lab_dir = home.map(|h| PathBuf::from(h).join("aevyternae-mind/logs"));
    fetch_system_from(
        Path::new("/sys/class/drm"),
        Path::new("/usr/share/libdrm/amdgpu.ids"),
        lab_dir.as_deref(),
    )
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemData {
    pub ready: bool,
    pub gpu_name: String,
    pub gpu_busy: Option<u8>,
    pub vram_used_gb: Option<f64>,
    pub vram_total_gb: Option<f64>,
    pub temp_c: Option<i32>,
    pub lab: Option<(String, String)>,
}

pub fn apply(d: &mut SystemData, key: &str, out: &str) -> bool {
    if key == "system" {
        *d = parse_system(out);
        true
    } else {
        false
    }
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
            && parts[1]
                .trim()
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !parts[1].trim().is_empty())
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

/// Parses the combined `system` helper output into [`SystemData`].
pub fn parse_system(out: &str) -> SystemData {
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

    SystemData {
        ready: true,
        gpu_name: parse_gpu_name(out),
        gpu_busy,
        vram_used_gb,
        vram_total_gb,
        temp_c,
        lab: parse_lab_log(out),
    }
}

pub fn has_columns(d: &SystemData) -> bool {
    d.gpu_busy.is_some() || d.vram_used_gb.is_some() || d.temp_c.is_some()
}

pub fn visible(d: &SystemData) -> bool {
    has_columns(d) || d.lab.is_some() || !d.gpu_name.is_empty()
}

pub fn height(world: &World) -> f64 {
    if !world.system.ready {
        return 2.0 * PAD_Y + COLS_H;
    }
    if !visible(&world.system) {
        return 0.0;
    }
    let base = if has_columns(&world.system) {
        COLS_H
    } else {
        24.0
    };
    let lab = if world.system.lab.is_some() {
        LAB_ROW_H
    } else {
        0.0
    };
    2.0 * PAD_Y + base + lab
}

/// Returns the hitbox `(x, y, w, h)` of the `"Pause"` button inside the `system` card when a
/// training lab is running.
pub fn pause_rect(d: &SystemData, w: f64) -> Option<Rect> {
    d.lab.as_ref()?;
    let base = if has_columns(d) { COLS_H } else { 24.0 };
    let y = PAD_Y + base + 8.0;
    Some((w - PAD_X - 60.0, y, 60.0 + PAD_X, LAB_ROW_H))
}

pub fn pause_action() -> String {
    "touch ~/aevyternae-mind/logs/GAME_PAUSE".to_string()
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if !is_left_button(button) || !world.system.ready {
        return false;
    }
    let Some(r) = pause_rect(&world.system, size.0) else {
        return false;
    };
    if pos.0 >= r.0 && pos.0 <= r.0 + r.2 && pos.1 >= r.1 && pos.1 <= r.1 + r.3 {
        let cmd = pause_action();
        world.run(cmd);
        true
    } else {
        false
    }
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    draw::card(cr, th, x, y, w, h);
    draw::title(cr, th, x + PAD_X, y + PAD_Y, "System");
    if !world.system.ready {
        draw::text_right(
            cr,
            x + w - PAD_X,
            y + PAD_Y + 2.0,
            &Text {
                size: 13.0,
                bold: false,
                color: th.muted,
                text: "...",
            },
        );
        return;
    }

    let d = &world.system;
    if !d.gpu_name.is_empty() {
        draw::text_right(
            cr,
            x + w - PAD_X,
            y + PAD_Y + 2.0,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: &d.gpu_name,
            },
        );
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
        let inner_w = (w - 2.0 * PAD_X - (n - 1.0) * draw::GAP).max(30.0);
        let col_w = inner_w / n;
        let cy = y + PAD_Y + 30.0;
        for (i, (label, val, part, color)) in cols.iter().enumerate() {
            let cx = x + PAD_X + (i as f64) * (col_w + draw::GAP);
            draw::text(
                cr,
                cx,
                cy,
                &Text {
                    size: 11.0,
                    bold: false,
                    color: th.muted,
                    text: label,
                },
            );
            draw::text(
                cr,
                cx,
                cy + 15.0,
                &Text {
                    size: 22.0,
                    bold: true,
                    color: th.text,
                    text: val,
                },
            );
            draw::bar(cr, th, cx, cy + 46.0, col_w, *part, *color);
        }
    }

    if let Some((run, ends)) = &d.lab {
        let base = if !cols.is_empty() { COLS_H } else { 24.0 };
        let line_y = (y + PAD_Y + base + 8.0).round() + 0.5;
        cr.move_to(x + PAD_X, line_y);
        cr.line_to(x + w - PAD_X, line_y);
        draw::set(cr, th.track);
        cr.set_line_width(1.0);
        let _ = cr.stroke();

        let ty = y + PAD_Y + base + 17.0;
        let pause_w = draw::text_right(
            cr,
            x + w - PAD_X,
            ty,
            &Text {
                size: 12.0,
                bold: true,
                color: th.accent,
                text: "Pause",
            },
        );
        let label = format!("Training {run} · ends {ends}");
        draw::text_max(
            cr,
            x + PAD_X,
            ty,
            (w - 2.0 * PAD_X - pause_w - 12.0).max(20.0),
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: &label,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(d.ready);
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
    }

    #[test]
    fn pause_click_chooses_touch_game_pause_command() {
        let mut world = World::now();
        world.system = SystemData {
            ready: true,
            gpu_name: "Radeon RX 9070 XT".into(),
            gpu_busy: Some(98),
            vram_used_gb: Some(11.8),
            vram_total_gb: Some(16.0),
            temp_c: Some(64),
            lab: Some(("labv15-pushback-s1".into(), "20:55".into())),
        };
        let w = 380.0;
        let h = height(&world);
        let pr = pause_rect(&world.system, w).unwrap();

        // Click outside Pause button does nothing
        assert!(!click(&mut world, (w, h), (24.0, pr.1 + 8.0), 1));
        assert!(world.actions.is_empty());

        // Click on Pause button chooses touch ~/aevyternae-mind/logs/GAME_PAUSE
        assert!(click(&mut world, (w, h), (pr.0 + 10.0, pr.1 + 8.0), 1));
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("touch ~/aevyternae-mind/logs/GAME_PAUSE")
        );
    }

    #[test]
    fn shell_dispatches_system_and_memory_clicks_in_panel() {
        use crate::{config::Config, layout_in, panel_origin, panel_width, Shell};

        let cfg = Config::from_text("PANEL_CONTROL=\"SUPER+A top right system memory\"\n");
        let mut sh = Shell::from_config(cfg);
        sh.world.apply(
            "system",
            "gpu_busy=67\nvram_used=7807320064\nvram_total=17095983104\ntemp=57000\n7550,\tC0,\tAMD Radeon RX 9070 XT\n=== labv15-pushback-s1 20:55\n",
        );
        sh.world.apply(
            "memory",
            "MemTotal: 33554432 kB\nMemAvailable: 13002342 kB\nfirefox\t4404019\ncode\t2306867\nalacritty\t655360\n",
        );
        assert!(sh.toggle_named("control"));
        let (w, h) = (1920.0, 1080.0);
        let panel = sh.cfg.panels[sh.open.unwrap()].clone();
        let pw = panel_width(&panel, w);
        let (rects, ph) = layout_in(&panel, pw, &sh.world, &sh.theme);
        let (ox, oy) = panel_origin(&panel, w, h, pw, ph);

        let r_sys = rects[0];
        let pr = pause_rect(&sh.world.system, r_sys.2).unwrap();
        assert!(sh.click(
            w,
            h,
            ox + r_sys.0 + pr.0 + 10.0,
            oy + r_sys.1 + pr.1 + 8.0,
            1
        ));
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some("touch ~/aevyternae-mind/logs/GAME_PAUSE")
        );
        sh.pointer_release();

        let r_mem = rects[1];
        let xr = crate::modules::memory::app_close_rect(r_mem.2, 0);
        assert!(sh.click(
            w,
            h,
            ox + r_mem.0 + xr.0 + 8.0,
            oy + r_mem.1 + xr.1 + 10.0,
            1
        ));
        assert_eq!(
            sh.world.actions.last().map(String::as_str),
            Some("pkill -TERM -x firefox")
        );
    }

    #[test]
    fn parses_cpu_stat_pair_from_kernel() {
        let stat1 = "\
cpu  49933434 12661370 16812476 367504439 2035950 793615 342739 0 0 1890190
cpu0 2256435 1031265 896715 33003942 252142 57277 87602 0 0 102249
cpu1 7181386 1174642 2867389 25887343 220281 130891 144699 0 0 163908
";
        let stat2 = "\
cpu  49933846 12661381 16812697 367506844 2035954 793625 342743 0 0 1890191
cpu0 2256457 1031266 896727 33004162 252142 57278 87602 0 0 102249
cpu1 7181461 1174643 2867437 25887472 220281 130893 144700 0 0 163908
";
        let pct = parse_cpu_stat(stat1, stat2).unwrap();
        // delta_busy: 658, delta_total: 3067 -> ~21.45%
        assert!((pct - 21.45).abs() < 0.1);
    }

    #[test]
    fn parses_hwmon_tree_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("test_hwmon_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let hwmon0 = dir.join("hwmon0");
        std::fs::create_dir_all(&hwmon0).unwrap();
        std::fs::write(hwmon0.join("name"), "amdgpu\n").unwrap();
        std::fs::write(hwmon0.join("temp1_input"), "57000\n").unwrap();

        let temp = read_hwmon_temp(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(temp, Some(57));
    }

    #[test]
    fn parses_drm_sysfs_tree_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("test_drm_sysfs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
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

        let out = fetch_system_from(&dir, &amdgpu_ids, None);
        let d = parse_system(&out);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(d.gpu_busy, Some(67));
        assert_eq!(d.gpu_name, "Radeon RX 9070 XT");
        assert_eq!(d.temp_c, Some(57));
        let vram_used = d.vram_used_gb.unwrap();
        let vram_total = d.vram_total_gb.unwrap();
        assert!((vram_used - 7.271).abs() < 0.01);
        assert!((vram_total - 15.922).abs() < 0.01);
    }

    #[test]
    fn parses_battery_and_power_supply_in_temp_dir() {
        assert_eq!(parse_battery("85\n", "Discharging\n"), (Some(85), false));
        assert_eq!(parse_battery("100\n", "Full\n"), (Some(100), true));
        assert_eq!(parse_battery("42\n", "Charging\n"), (Some(42), true));

        let dir = std::env::temp_dir().join(format!("test_power_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bat0 = dir.join("BAT0");
        std::fs::create_dir_all(&bat0).unwrap();
        std::fs::write(bat0.join("capacity"), "88\n").unwrap();
        std::fs::write(bat0.join("status"), "Charging\n").unwrap();

        let res = read_battery_from(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(res, Some((88, true)));
    }
}
