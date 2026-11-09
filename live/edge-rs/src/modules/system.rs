//! The `system` module: GPU name, three columns (GPU use %, VRAM used in GB, GPU temperature in C
//! with 3 px bars), and the running training lab line (`"Training <run> · ends HH:MM"` with a
//! `"Pause"` button that touches `~/aevyternae-mind/logs/GAME_PAUSE`).

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{is_left_button, Rect, World};
use crate::theme::{Rgba, Theme};
use cairo::Context;

pub const COLS_H: f64 = 80.0;
pub const LAB_ROW_H: f64 = 34.0;

pub const CMDS: &[(&str, &str)] = &[(
    "system",
    "for d in /sys/class/drm/card[0-9]*/device; do [ -d \"$d\" ] || continue; \
     [ -f \"$d/gpu_busy_percent\" ] || [ -f \"$d/mem_info_vram_used\" ] || ls \"$d\"/hwmon/hwmon*/temp1_input >/dev/null 2>&1 || continue; \
     echo \"gpu_busy=$(cat \"$d/gpu_busy_percent\" 2>/dev/null)\"; \
     echo \"vram_used=$(cat \"$d/mem_info_vram_used\" 2>/dev/null)\"; \
     echo \"vram_total=$(cat \"$d/mem_info_vram_total\" 2>/dev/null)\"; \
     echo \"temp=$(cat \"$d\"/hwmon/hwmon*/temp1_input 2>/dev/null | head -n 1)\"; \
     echo \"product_name=$(cat \"$d/product_name\" 2>/dev/null)\"; \
     dev=$(sed 's/^0x//' \"$d/device\" 2>/dev/null | tr 'a-f' 'A-F'); \
     rev=$(sed 's/^0x//' \"$d/revision\" 2>/dev/null | tr 'a-f' 'A-F'); \
     [ -n \"$dev\" ] && [ -f /usr/share/libdrm/amdgpu.ids ] && grep -i \"^$dev,[[:space:]]*$rev,\" /usr/share/libdrm/amdgpu.ids 2>/dev/null | head -n 1; \
     break; done; \
     lspci 2>/dev/null | grep -iE 'vga|3d|display' | head -n 1; \
     for f in $(ls -t \"$HOME\"/aevyternae-mind/logs/lab*.log 2>/dev/null); do \
     if grep -q '^=== ' \"$f\" 2>/dev/null; then grep -E '^=== |^LAB .*DONE|^LAB DONE|^FAILED ' \"$f\" 2>/dev/null; break; fi; done",
)];

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

    let temp = temp_input
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
        });

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
}
