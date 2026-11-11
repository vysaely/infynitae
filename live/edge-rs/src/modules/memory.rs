//! The `memory` module: used and total RAM (`"19.6 of 32 GB"` with a 3 px bar), the three apps
//! using the most memory from `/proc` (summed by process name, skipping kernel threads and the
//! edge itself) with a small `x` to close each (`SIGTERM`), and the optional
//! `"Close background apps · frees N GB"` button when window `app_id`s are known.

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{is_left_button, Rect, World};
use crate::theme::Theme;
use cairo::Context;
use std::collections::HashMap;
use std::path::Path;

pub const HEADER_H: f64 = 42.0;
pub const APP_ROW_H: f64 = 28.0;
pub const BG_BTN_H: f64 = 26.0;

pub const CMDS: &[(&str, &str)] = &[("memory", "__proc_memory__")];

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

/// Returns the formatted memory string from `/proc` for consumption by [`parse_memory`].
pub fn fetch_string() -> String {
    fetch_proc_memory_from(Path::new("/proc"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppMem {
    pub name: String,
    pub rss_kb: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryData {
    pub ready: bool,
    pub used_kb: u64,
    pub total_kb: u64,
    pub apps: Vec<AppMem>,
    /// Window `app_id`s from foreign-toplevel (empty until `windows` is populated in Rust;
    /// when empty, the `"Close background apps"` button hides).
    pub window_app_ids: Vec<String>,
}

pub fn apply(d: &mut MemoryData, key: &str, out: &str) -> bool {
    if key == "memory" {
        let prev_wins = d.window_app_ids.clone();
        let mut parsed = parse_memory(out);
        if parsed.window_app_ids.is_empty() && !prev_wins.is_empty() {
            parsed.window_app_ids = prev_wins;
        }
        *d = parsed;
        true
    } else {
        false
    }
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
        ready: true,
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
        id_l == lower
            || id_l.ends_with(&format!(".{lower}"))
            || id_l.starts_with(&lower)
            || lower.starts_with(&id_l)
    })
}

pub fn background_apps(d: &MemoryData) -> Vec<&AppMem> {
    if d.window_app_ids.is_empty() {
        return Vec::new();
    }
    d.apps
        .iter()
        .filter(|a| !has_window(&a.name, &d.window_app_ids))
        .collect()
}

pub fn close_bg_label(d: &MemoryData) -> Option<String> {
    let bg = background_apps(d);
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

pub fn height(world: &World) -> f64 {
    if !world.memory.ready {
        return 2.0 * PAD_Y + 40.0;
    }
    let rows = world.memory.apps.len().min(3) as f64;
    let bg_h = if close_bg_label(&world.memory).is_some() {
        BG_BTN_H
    } else {
        0.0
    };
    2.0 * PAD_Y + HEADER_H + rows * APP_ROW_H + bg_h
}

/// Returns the `(x, y, w, h)` hitbox of the `x` close button for top app row `idx` (`0..3`).
pub fn app_close_rect(w: f64, idx: usize) -> Rect {
    let ry = PAD_Y + HEADER_H + (idx as f64) * APP_ROW_H;
    (w - PAD_X - 20.0, ry, 20.0 + PAD_X, APP_ROW_H)
}

/// Returns the `(x, y, w, h)` hitbox of the `"Close background apps · frees N GB"` button when
/// visible.
pub fn close_bg_rect(d: &MemoryData, w: f64) -> Option<Rect> {
    close_bg_label(d)?;
    let rows = d.apps.len().min(3) as f64;
    let y = PAD_Y + HEADER_H + rows * APP_ROW_H;
    Some((PAD_X, y, (w - 2.0 * PAD_X).max(20.0), BG_BTN_H + PAD_Y))
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

/// Closes all background apps (apps with no window in `window_app_ids`) and returns the shell
/// command string.
pub fn close_bg_action(d: &mut MemoryData) -> Option<String> {
    if d.window_app_ids.is_empty() {
        return None;
    }
    let wins = d.window_app_ids.clone();
    let mut cmds = Vec::new();
    let mut freed_kb = 0u64;
    d.apps.retain(|a| {
        // what keeps the session running is never a background app to close, window or not
        // (the compositor itself has none: it was in reach of this)
        if has_window(&a.name, &wins) || protected(&a.name) {
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

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if !is_left_button(button) || !world.memory.ready {
        return false;
    }
    let (w, _) = size;
    let shown = world.memory.apps.len().min(3);
    for idx in 0..shown {
        let r = app_close_rect(w, idx);
        if pos.0 >= r.0 && pos.0 <= r.0 + r.2 && pos.1 >= r.1 && pos.1 < r.1 + r.3 {
            if let Some(cmd) = close_app_action(&mut world.memory, idx) {
                world.run(cmd);
                return true;
            }
        }
    }
    if let Some(r) = close_bg_rect(&world.memory, w) {
        if pos.0 >= r.0 && pos.0 <= r.0 + r.2 && pos.1 >= r.1 && pos.1 <= r.1 + r.3 {
            if let Some(cmd) = close_bg_action(&mut world.memory) {
                world.run(cmd);
                return true;
            }
        }
    }
    false
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    draw::card(cr, th, x, y, w, h);
    draw::title(cr, th, x + PAD_X, y + PAD_Y, "Memory");
    if !world.memory.ready {
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

    let d = &world.memory;
    let used_gb = d.used_kb as f64 / 1048576.0;
    let total_gb = ((d.total_kb as f64 / 1048576.0).round() as u64).max(1);
    let of_total = format!(" of {total_gb} GB");
    let used_str = format!("{used_gb:.1}");
    let of_w = draw::text_right(
        cr,
        x + w - PAD_X,
        y + PAD_Y + 2.0,
        &Text {
            size: 13.0,
            bold: false,
            color: th.muted,
            text: &of_total,
        },
    );
    draw::text_right(
        cr,
        x + w - PAD_X - of_w,
        y + PAD_Y + 2.0,
        &Text {
            size: 13.0,
            bold: true,
            color: th.text,
            text: &used_str,
        },
    );

    let part = if d.total_kb > 0 {
        (d.used_kb as f64 / d.total_kb as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    draw::bar(
        cr,
        th,
        x + PAD_X,
        y + PAD_Y + 28.0,
        w - 2.0 * PAD_X,
        part,
        th.accent,
    );

    for (i, app) in d.apps.iter().take(3).enumerate() {
        let ry = y + PAD_Y + HEADER_H + (i as f64) * APP_ROW_H;
        let size_str = format_app_size(app.rss_kb);
        let size_w = draw::text_right(
            cr,
            x + w - PAD_X - 20.0,
            ry + 5.0,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: &size_str,
            },
        );
        draw::text_max(
            cr,
            x + PAD_X,
            ry + 4.0,
            (w - 2.0 * PAD_X - size_w - 28.0).max(20.0),
            &Text {
                size: 13.0,
                bold: false,
                color: th.text,
                text: &app.name,
            },
        );

        let cx = (x + w - PAD_X - 5.0).round();
        let cy = (ry + 12.0).round();
        draw::set(cr, th.muted);
        cr.set_line_width(1.5);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.move_to(cx - 3.5, cy - 3.5);
        cr.line_to(cx + 3.5, cy + 3.5);
        cr.move_to(cx + 3.5, cy - 3.5);
        cr.line_to(cx - 3.5, cy + 3.5);
        let _ = cr.stroke();
    }

    if let Some(lbl) = close_bg_label(d) {
        let rows = d.apps.len().min(3) as f64;
        let by = y + PAD_Y + HEADER_H + rows * APP_ROW_H + 6.0;
        draw::text_max(
            cr,
            x + PAD_X,
            by,
            (w - 2.0 * PAD_X).max(20.0),
            &Text {
                size: 12.0,
                bold: true,
                color: th.accent,
                text: &lbl,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(d.ready);
        assert_eq!(d.total_kb, 33554432);
        assert_eq!(d.used_kb, 33554432 - 13002342);
        let used_gb = d.used_kb as f64 / 1048576.0;
        assert!((used_gb - 19.6).abs() < 0.01);

        // Kernel threads and infynitae-edge / infynitae-edge- are skipped; firefox PIDs summed.
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

        // By default window_app_ids is empty, so Close background apps button is hidden.
        assert!(d.window_app_ids.is_empty());
        assert_eq!(close_bg_label(&d), None);
        assert_eq!(close_bg_rect(&d, 380.0), None);
    }

    #[test]
    fn app_close_x_and_close_background_apps_choose_sigterm_commands() {
        let mut world = World::now();
        world.memory = parse_memory(
            "MemTotal: 33554432 kB\nMemAvailable: 13002342 kB\n\
             firefox\t4404019\ncode\t2097152\ndiscord\t1048576\nalacritty\t524288\n",
        );
        let w = 380.0;
        let h = height(&world);

        // Click the small x on row 1 ("code")
        let rx = app_close_rect(w, 1);
        assert!(click(&mut world, (w, h), (rx.0 + 8.0, rx.1 + 10.0), 1));
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("pkill -TERM -x code")
        );
        assert_eq!(world.memory.apps.len(), 3);
        assert_eq!(world.memory.apps[1].name, "discord");

        // Now populate window_app_ids with ["firefox"]: discord (1 GB) and alacritty (0.5 GB)
        // have no window -> "Close background apps · frees 1.5 GB" appears.
        world.memory.window_app_ids = vec!["firefox".to_string()];
        assert_eq!(
            close_bg_label(&world.memory).as_deref(),
            Some("Close background apps · frees 1.5 GB")
        );
        let bg_r = close_bg_rect(&world.memory, w).unwrap();
        let h2 = height(&world);
        assert!(click(&mut world, (w, h2), (bg_r.0 + 20.0, bg_r.1 + 8.0), 1));
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("pkill -TERM -x discord; pkill -TERM -x alacritty")
        );
        assert_eq!(world.memory.apps.len(), 1);
        assert_eq!(world.memory.apps[0].name, "firefox");
    }

    #[test]
    fn the_session_is_never_a_background_app() {
        let mut d = MemoryData {
            window_app_ids: vec!["firefox".into()],
            ..Default::default()
        };
        for n in ["infynitae", "pipewire", "wireplumber", "steam", "firefox"] {
            d.apps.push(AppMem {
                name: n.into(),
                rss_kb: 1000,
            });
        }
        let cmd = close_bg_action(&mut d).unwrap();
        assert_eq!(cmd, "pkill -TERM -x steam");
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
        assert!(
            !d.apps
                .iter()
                .any(|a| a.name == "infynitae" || a.name.starts_with("infynitae-session")),
            "compositor and session must be excluded from apps"
        );
        assert_eq!(d.apps.len(), 1);
        assert_eq!(d.apps[0].name, "firefox");

        // Even if a protected process were somehow in apps, close_app_action must refuse to kill it
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
    fn parses_full_kernel_meminfo_layout() {
        let meminfo = "\
MemTotal:       32453284 kB
MemFree:         2125112 kB
MemAvailable:   20688720 kB
Buffers:             428 kB
Cached:         20143660 kB
SwapCached:          308 kB
Active:          7240876 kB
Inactive:       19639648 kB
Active(anon):    5312432 kB
Inactive(anon):  1744244 kB
Active(file):    1928444 kB
Inactive(file): 17895404 kB
Unevictable:         392 kB
Mlocked:             392 kB
SwapTotal:      83335160 kB
SwapFree:       79339996 kB
Zswap:                 0 kB
Zswapped:              0 kB
Dirty:              6524 kB
Writeback:             0 kB
AnonPages:       6733464 kB
Mapped:          3076412 kB
Shmem:            320300 kB
KReclaimable:     450436 kB
Slab:             825720 kB
";
        let (used_kb, total_kb) = parse_meminfo(meminfo);
        assert_eq!(total_kb, 32453284);
        assert_eq!(used_kb, 32453284 - 20688720);
    }

    #[test]
    fn reads_proc_meminfo_and_process_status_from_directory() {
        let dir = std::env::temp_dir().join(format!("test_proc_mem_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("meminfo"),
            "MemTotal:       32453284 kB\nMemAvailable:   20688720 kB\n",
        )
        .unwrap();

        // PID 101: firefox
        let p101 = dir.join("101");
        std::fs::create_dir_all(&p101).unwrap();
        std::fs::write(
            p101.join("status"),
            "Name:\tfirefox\nState:\tS\nVmRSS:\t 2800000 kB\n",
        )
        .unwrap();

        // PID 102: firefox child
        let p102 = dir.join("102");
        std::fs::create_dir_all(&p102).unwrap();
        std::fs::write(
            p102.join("status"),
            "Name:\tfirefox\nState:\tS\nVmRSS:\t 1604019 kB\n",
        )
        .unwrap();

        // PID 103: kernel worker (should be skipped)
        let p103 = dir.join("103");
        std::fs::create_dir_all(&p103).unwrap();
        std::fs::write(p103.join("status"), "Name:\tkworker/u64:1\nState:\tI\n").unwrap();

        let out = fetch_proc_memory_from(&dir);
        let d = parse_memory(&out);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(d.total_kb, 32453284);
        assert_eq!(d.used_kb, 32453284 - 20688720);
        assert_eq!(d.apps.len(), 1);
        assert_eq!(d.apps[0].name, "firefox");
        assert_eq!(d.apps[0].rss_kb, 2800000 + 1604019);
    }
}
