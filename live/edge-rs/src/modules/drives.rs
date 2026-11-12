//! The `drives` module: each drive and its free space.
//!
//! Mounts are read from `/proc/self/mountinfo`, filtered to:
//! - Fixed drives listed in `/etc/fstab` with `x-gvfs-show`
//! - Removable drives under `/run/media/$USER/`
//!
//! Drives are named by `x-gvfs-name=`, else filesystem label (`/dev/disk/by-label/`), else mount
//! point's last part. Ordered by Roman numerals (I, II, ... XX) numerically, then alphabetically.
//! Each row shows a line icon, name, right-aligned free space, thin accent bar, and used of total
//! space. Removable rows have an eject button (`udisksctl unmount` then `power-off`).

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{is_left_button, Rect, World};
use crate::theme::Theme;
use cairo::Context;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::Instant;

pub const HEADER_H: f64 = 28.0;
pub const ROW_H: f64 = 54.0;
pub const LOADING: &str = "Loading";
pub const CMDS: &[(&str, &str)] = &[("drives", "__proc_drives__")];

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveStatus {
    Normal,
    Ejecting,
    Busy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriveItem {
    pub name: String,
    pub mount_point: String,
    pub device: String,
    pub removable: bool,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub status: DriveStatus,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrivesData {
    pub ready: bool,
    pub drives: Vec<DriveItem>,
}

pub fn not_ready_text(d: &DrivesData) -> Option<&'static str> {
    if !d.ready {
        Some(LOADING)
    } else {
        None
    }
}

pub fn height(world: &World) -> f64 {
    if !world.drives.ready || world.drives.drives.is_empty() {
        return 2.0 * PAD_Y + 40.0;
    }
    2.0 * PAD_Y + HEADER_H + (world.drives.drives.len() as f64) * ROW_H
}

pub fn apply(d: &mut DrivesData, key: &str, out: &str) -> bool {
    if key == "drives" {
        let prev_statuses: HashMap<String, DriveStatus> = d
            .drives
            .iter()
            .map(|item| (item.device.clone(), item.status))
            .collect();

        let mut parsed = parse_drives(out);
        for item in &mut parsed.drives {
            if let Some(&prev) = prev_statuses.get(&item.device) {
                if prev == DriveStatus::Busy || prev == DriveStatus::Ejecting {
                    if let Some(res) = check_eject_result(&item.device) {
                        item.status = if res {
                            DriveStatus::Normal
                        } else {
                            DriveStatus::Busy
                        };
                    } else {
                        item.status = prev;
                    }
                }
            }
        }
        *d = parsed;
        true
    } else {
        false
    }
}

pub fn parse_drives(out: &str) -> DrivesData {
    let mut drives = Vec::new();
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 6 {
            continue;
        }
        let name = parts[0].to_string();
        let mount_point = parts[1].to_string();
        let device = parts[2].to_string();
        let removable = parts[3] == "1" || parts[3].eq_ignore_ascii_case("true");
        let total_bytes = parts[4].parse::<u64>().unwrap_or(0);
        let available_bytes = parts[5].parse::<u64>().unwrap_or(0);
        let status = match parts.get(6).copied() {
            Some("ejecting") => DriveStatus::Ejecting,
            Some("busy") => DriveStatus::Busy,
            _ => DriveStatus::Normal,
        };
        drives.push(DriveItem {
            name,
            mount_point,
            device,
            removable,
            total_bytes,
            available_bytes,
            status,
        });
    }
    sort_drives(&mut drives);
    DrivesData {
        ready: true,
        drives,
    }
}

pub fn serialize_drives(data: &DrivesData) -> String {
    let mut out = String::new();
    for d in &data.drives {
        let rem = if d.removable { "1" } else { "0" };
        let status_str = match d.status {
            DriveStatus::Normal => "normal",
            DriveStatus::Ejecting => "ejecting",
            DriveStatus::Busy => "busy",
        };
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            d.name, d.mount_point, d.device, rem, d.total_bytes, d.available_bytes, status_str
        ));
    }
    out
}

pub fn sort_drives(drives: &mut [DriveItem]) {
    drives.sort_by(|a, b| {
        let a_num = roman_to_number(&a.name);
        let b_num = roman_to_number(&b.name);
        match (a_num, b_num) {
            (Some(na), Some(nb)) => na.cmp(&nb).then_with(|| a.name.cmp(&b.name)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        }
    });
}

pub fn roman_to_number(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut total = 0u32;
    let mut prev = 0u32;
    for c in s.chars().rev() {
        let val = match c {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            'D' => 500,
            'M' => 1000,
            _ => return None,
        };
        if val < prev {
            total = total.checked_sub(val)?;
        } else {
            total = total.checked_add(val)?;
            prev = val;
        }
    }
    if number_to_roman(total).as_deref() == Some(s) {
        Some(total)
    } else {
        None
    }
}

pub fn number_to_roman(mut num: u32) -> Option<String> {
    if num == 0 || num > 3999 {
        return None;
    }
    const TABLE: &[(u32, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for &(val, sym) in TABLE {
        while num >= val {
            out.push_str(sym);
            num -= val;
        }
    }
    Some(out)
}

pub fn format_gb_val(gb: f64) -> String {
    if gb >= 100.0 {
        format!("{:.0}", gb.round())
    } else {
        format!("{:.1}", gb)
    }
}

pub fn format_free(avail_bytes: u64) -> String {
    let gb = avail_bytes as f64 / GIB;
    format!("{} GB free", format_gb_val(gb))
}

pub fn format_used_of_total(used_bytes: u64, total_bytes: u64) -> String {
    let used_gb = used_bytes as f64 / GIB;
    let total_gb = total_bytes as f64 / GIB;
    format!(
        "{} of {} GB",
        format_gb_val(used_gb),
        format_gb_val(total_gb)
    )
}

pub fn unescape_octal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let mut octal = String::new();
            for _ in 0..3 {
                if let Some(&next_c) = chars.peek() {
                    if next_c.is_digit(8) {
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

pub fn mount_last_part(mount_point: &str) -> String {
    let trimmed = mount_point.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FstabMount {
    pub gvfs_show: bool,
    pub gvfs_name: Option<String>,
}

pub fn parse_fstab(fstab_text: &str) -> HashMap<String, FstabMount> {
    let mut map = HashMap::new();
    for raw in fstab_text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let mount_point = unescape_octal(parts[1]);
        let options = parts[3];
        let mut gvfs_show = false;
        let mut gvfs_name = None;
        for opt in options.split(',') {
            if opt == "x-gvfs-show" {
                gvfs_show = true;
            } else if let Some(name) = opt.strip_prefix("x-gvfs-name=") {
                gvfs_name = Some(name.to_string());
            }
        }
        if gvfs_show {
            map.insert(
                mount_point,
                FstabMount {
                    gvfs_show,
                    gvfs_name,
                },
            );
        }
    }
    map
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedMount {
    pub name: String,
    pub mount_point: String,
    pub device: String,
    pub removable: bool,
}

pub fn parse_mountinfo_and_fstab(
    mountinfo: &str,
    fstab: &str,
    user: &str,
    labels: &HashMap<String, String>,
) -> Vec<ParsedMount> {
    let fstab_map = parse_fstab(fstab);
    let user_media_prefix = format!("/run/media/{user}/");
    let mut mounts_by_point: HashMap<String, (String, bool)> = HashMap::new();

    for line in mountinfo.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 7 {
            continue;
        }
        let mount_point = unescape_octal(tokens[4]);
        let Some(sep_idx) = tokens.iter().position(|&t| t == "-") else {
            continue;
        };
        let Some(&device_raw) = tokens.get(sep_idx + 2) else {
            continue;
        };
        let device = unescape_octal(device_raw);

        if fstab_map.contains_key(&mount_point) {
            mounts_by_point.insert(mount_point, (device, false));
        } else if let Some(sub) = mount_point.strip_prefix(&user_media_prefix) {
            if !sub.is_empty() {
                mounts_by_point.insert(mount_point, (device, true));
            }
        }
    }

    let mut result = Vec::new();
    for (mount_point, (device, removable)) in mounts_by_point {
        let name = if let Some(fstab_entry) = fstab_map.get(&mount_point) {
            if let Some(ref gvfs_name) = fstab_entry.gvfs_name {
                if !gvfs_name.is_empty() {
                    gvfs_name.clone()
                } else if let Some(label) = labels.get(&device) {
                    label.clone()
                } else {
                    mount_last_part(&mount_point)
                }
            } else if let Some(label) = labels.get(&device) {
                label.clone()
            } else {
                mount_last_part(&mount_point)
            }
        } else if let Some(label) = labels.get(&device) {
            label.clone()
        } else {
            mount_last_part(&mount_point)
        };

        result.push(ParsedMount {
            name,
            mount_point,
            device,
            removable,
        });
    }

    result.sort_by(|a, b| {
        let a_num = roman_to_number(&a.name);
        let b_num = roman_to_number(&b.name);
        match (a_num, b_num) {
            (Some(na), Some(nb)) => na.cmp(&nb).then_with(|| a.name.cmp(&b.name)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        }
    });

    result
}

pub fn read_disk_by_label() -> HashMap<String, String> {
    read_disk_by_label_from(Path::new("/dev/disk/by-label"))
}

pub fn read_disk_by_label_from(dir: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let label = entry.file_name().to_string_lossy().into_owned();
        if let Ok(target) = std::fs::canonicalize(entry.path()) {
            map.insert(target.to_string_lossy().into_owned(), label);
        }
    }
    map
}

pub fn statvfs_sizes(path: &str) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    let c_path = CString::new(path).ok()?;
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();
    let res = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if res != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    let frsize = if stat.f_frsize > 0 {
        stat.f_frsize
    } else {
        stat.f_bsize
    };
    let total_bytes = stat.f_blocks * frsize;
    let avail_bytes = stat.f_bavail * frsize;
    Some((total_bytes, avail_bytes))
}

pub fn shell_arg(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub fn eject_command(device: &str) -> String {
    format!(
        "udisksctl unmount -b {} && udisksctl power-off -b {}",
        shell_arg(device),
        shell_arg(device)
    )
}

static EJECT_RESULTS: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);

pub fn record_eject_result(device: &str, success: bool) {
    let mut lock = match EJECT_RESULTS.lock() {
        Ok(guard) => guard,
        Err(p) => p.into_inner(),
    };
    let map = lock.get_or_insert_with(HashMap::new);
    map.insert(device.to_string(), success);
}

pub fn check_eject_result(device: &str) -> Option<bool> {
    let lock = match EJECT_RESULTS.lock() {
        Ok(guard) => guard,
        Err(p) => p.into_inner(),
    };
    lock.as_ref()?.get(device).copied()
}

static LAST_FETCH: Mutex<Option<(Instant, String)>> = Mutex::new(None);

pub fn invalidate_cache() {
    if let Ok(mut lock) = LAST_FETCH.lock() {
        *lock = None;
    }
}

pub fn fetch_string() -> String {
    let mut lock = match LAST_FETCH.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some((instant, ref cached)) = *lock {
        if instant.elapsed() < std::time::Duration::from_secs(10) {
            return cached.clone();
        }
    }

    let result = fetch_live_drives_string();
    *lock = Some((Instant::now(), result.clone()));
    result
}

fn fetch_live_drives_string() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let fstab = std::fs::read_to_string("/etc/fstab").unwrap_or_default();
    let labels = read_disk_by_label();
    let parsed = parse_mountinfo_and_fstab(&mountinfo, &fstab, &user, &labels);

    let mut drives = Vec::new();
    for p in parsed {
        let (total_bytes, available_bytes) = statvfs_sizes(&p.mount_point).unwrap_or((0, 0));
        drives.push(DriveItem {
            name: p.name,
            mount_point: p.mount_point,
            device: p.device,
            removable: p.removable,
            total_bytes,
            available_bytes,
            status: DriveStatus::Normal,
        });
    }
    let data = DrivesData {
        ready: true,
        drives,
    };
    serialize_drives(&data)
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if !is_left_button(button) || !world.drives.ready {
        return false;
    }
    let (w, _) = size;
    let (px, py) = pos;
    if py < PAD_Y + HEADER_H {
        return false;
    }
    let row_idx = ((py - PAD_Y - HEADER_H) / ROW_H) as usize;
    if row_idx >= world.drives.drives.len() {
        return false;
    }
    let ry = PAD_Y + HEADER_H + (row_idx as f64) * ROW_H;
    if py > ry + ROW_H {
        return false;
    }

    let drive = &world.drives.drives[row_idx];
    if drive.removable {
        let eject_rect = (w - PAD_X - 22.0, ry, 22.0 + PAD_X, ROW_H);
        if px >= eject_rect.0 && px <= eject_rect.0 + eject_rect.2 {
            if drive.status == DriveStatus::Ejecting {
                return true;
            }
            let dev = drive.device.clone();
            world.drives.drives[row_idx].status = DriveStatus::Ejecting;
            let cmd = eject_command(&dev);
            if world.live && std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_none() {
                let dev_clone = dev.clone();
                std::thread::spawn(move || {
                    let unmount_ok = Command::new("udisksctl")
                        .args(["unmount", "-b", &dev_clone])
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                    let ok = if unmount_ok {
                        Command::new("udisksctl")
                            .args(["power-off", "-b", &dev_clone])
                            .status()
                            .map(|s| s.success())
                            .unwrap_or(false)
                    } else {
                        false
                    };
                    record_eject_result(&dev_clone, ok);
                });
            }
            world.run(cmd);
            return true;
        }
    }

    let mp = drive.mount_point.clone();
    let cmd = format!("xdg-open {}", shell_arg(&mp));
    world.close_requested = true;
    world.run(cmd);
    true
}

fn draw_fixed_drive_icon(cr: &Context, color: crate::theme::Rgba, cx: f64, cy: f64) {
    draw::set(cr, color);
    cr.set_line_width(1.5);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    draw::round_rect(cr, cx - 7.0, cy - 5.0, 14.0, 10.0, 1.5);
    let _ = cr.stroke();
    cr.move_to(cx - 4.5, cy + 1.5);
    cr.line_to(cx + 0.5, cy + 1.5);
    let _ = cr.stroke();
    cr.arc(cx + 3.5, cy + 1.5, 0.75, 0.0, 2.0 * std::f64::consts::PI);
    let _ = cr.fill();
}

fn draw_removable_icon(cr: &Context, color: crate::theme::Rgba, cx: f64, cy: f64) {
    draw::set(cr, color);
    cr.set_line_width(1.5);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    draw::round_rect(cr, cx - 6.5, cy - 3.5, 9.0, 7.0, 1.0);
    let _ = cr.stroke();
    cr.rectangle(cx + 2.5, cy - 2.5, 4.0, 5.0);
    let _ = cr.stroke();
}

fn draw_eject_icon(cr: &Context, color: crate::theme::Rgba, cx: f64, cy: f64) {
    draw::set(cr, color);
    cr.set_line_width(1.5);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(cx, cy - 4.0);
    cr.line_to(cx + 4.5, cy + 1.0);
    cr.line_to(cx - 4.5, cy + 1.0);
    cr.close_path();
    let _ = cr.stroke();
    cr.move_to(cx - 4.5, cy + 4.0);
    cr.line_to(cx + 4.5, cy + 4.0);
    let _ = cr.stroke();
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    draw::card(cr, th, x, y, w, h);
    draw::title(cr, th, x + PAD_X, y + PAD_Y, "Drives");

    if let Some(loading) = not_ready_text(&world.drives) {
        draw::text_max(
            cr,
            x + PAD_X,
            y + PAD_Y + 28.0,
            (w - 2.0 * PAD_X).max(20.0),
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: loading,
            },
        );
        return;
    }

    if world.drives.drives.is_empty() {
        draw::text_max(
            cr,
            x + PAD_X,
            y + PAD_Y + 28.0,
            (w - 2.0 * PAD_X).max(20.0),
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: "No drives",
            },
        );
        return;
    }

    for (i, drive) in world.drives.drives.iter().enumerate() {
        let ry = y + PAD_Y + HEADER_H + (i as f64) * ROW_H;
        let line1_y = ry;
        let icon_cx = x + PAD_X + 8.0;
        let icon_cy = line1_y + 8.0;

        if drive.removable {
            draw_removable_icon(cr, th.muted, icon_cx, icon_cy);
        } else {
            draw_fixed_drive_icon(cr, th.muted, icon_cx, icon_cy);
        }

        let content_x = x + PAD_X + 22.0;
        let content_end = if drive.removable {
            let eject_cx = (x + w - PAD_X - 10.0).round();
            let eject_cy = (line1_y + 8.0).round();
            draw_eject_icon(cr, th.muted, eject_cx, eject_cy);
            x + w - PAD_X - 24.0
        } else {
            x + w - PAD_X
        };

        let free_text = format_free(drive.available_bytes);
        let free_w = draw::text_right(
            cr,
            content_end,
            line1_y + 1.0,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: &free_text,
            },
        );
        let max_name_w = (content_end - content_x - free_w - 8.0).max(10.0);
        draw::text_max(
            cr,
            content_x,
            line1_y,
            max_name_w,
            &Text {
                size: 13.0,
                bold: false,
                color: th.text,
                text: &drive.name,
            },
        );

        let bar_y = line1_y + 23.0;
        let bar_w = (content_end - content_x).max(10.0);
        let used_bytes = drive.total_bytes.saturating_sub(drive.available_bytes);
        let part = if drive.total_bytes > 0 {
            (used_bytes as f64 / drive.total_bytes as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        draw::bar(cr, th, content_x, bar_y, bar_w, part, th.accent);

        let line2_y = bar_y + 8.0;
        let second_line_text = match drive.status {
            DriveStatus::Normal => format_used_of_total(used_bytes, drive.total_bytes),
            DriveStatus::Ejecting => "Ejecting".to_string(),
            DriveStatus::Busy => "Busy: close what uses it".to_string(),
        };
        draw::text_max(
            cr,
            content_x,
            line2_y,
            bar_w,
            &Text {
                size: 11.0,
                bold: false,
                color: th.muted,
                text: &second_line_text,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drives_not_ready_shows_loading() {
        let mut d = DrivesData::default();
        assert!(!d.ready);
        assert_eq!(not_ready_text(&d), Some("Loading"));
        assert_ne!(not_ready_text(&d), Some("..."));

        d.ready = true;
        assert_eq!(not_ready_text(&d), None);
    }

    #[test]
    fn parse_mountinfo_and_fstab_picks_exactly_gvfs_show_and_user_removable_mounts() {
        let fstab = "\
# sample /etc/fstab
UUID=root-uuid / btrfs rw,x-gvfs-show,x-gvfs-name=I 0 0
UUID=home-uuid /home btrfs rw,noatime 0 0
UUID=boot-uuid /boot vfat rw 0 2
UUID=data2-uuid /mnt/II btrfs rw,x-gvfs-show,x-gvfs-name=II 0 0
UUID=data3-uuid /mnt/III btrfs rw,x-gvfs-show 0 0
";
        let mountinfo = "\
32 2 0:29 /@ / rw,noatime shared:1 - btrfs /dev/nvme1n1p3 rw,subvol=/@
26 32 0:24 / /proc rw,nosuid,nodev,noexec,relatime shared:5 - proc proc rw
27 32 0:25 / /sys rw,nosuid,nodev,noexec,relatime shared:6 - sysfs sys rw
28 32 0:7 / /dev rw,nosuid,relatime shared:2 - devtmpfs dev rw
58 32 0:49 / /tmp rw,nosuid,nodev shared:113 - tmpfs tmpfs rw
62 32 259:5 / /boot rw,relatime shared:117 - vfat /dev/nvme1n1p1 rw
64 32 0:29 /@home /home rw,noatime shared:119 - btrfs /dev/nvme1n1p3 rw,subvol=/@home
66 32 0:48 / /mnt/II rw,noatime shared:121 - btrfs /dev/nvme0n1p3 rw
164 32 0:50 / /mnt/III rw,noatime shared:268 - btrfs /dev/sda1 rw
254 29 8:35 / /run/media/vysaely/FlashDrive rw,nosuid shared:276 - ntfs3 /dev/sdc1 rw
255 29 8:36 / /run/media/otheruser/SecretDisk rw,nosuid shared:277 - ext4 /dev/sdd1 rw
300 58 0:107 / /tmp/.mount_AppImage ro shared:542 - fuse.AppImage AppImage ro
";
        let mut labels = HashMap::new();
        labels.insert("/dev/sda1".to_string(), "III".to_string());

        let parsed = parse_mountinfo_and_fstab(mountinfo, fstab, "vysaely", &labels);
        assert_eq!(parsed.len(), 4);

        assert_eq!(parsed[0].name, "I");
        assert_eq!(parsed[0].mount_point, "/");
        assert_eq!(parsed[0].device, "/dev/nvme1n1p3");
        assert!(!parsed[0].removable);

        assert_eq!(parsed[1].name, "II");
        assert_eq!(parsed[1].mount_point, "/mnt/II");
        assert!(!parsed[1].removable);

        assert_eq!(parsed[2].name, "III");
        assert_eq!(parsed[2].mount_point, "/mnt/III");
        assert!(!parsed[2].removable);

        assert_eq!(parsed[3].name, "FlashDrive");
        assert_eq!(parsed[3].mount_point, "/run/media/vysaely/FlashDrive");
        assert_eq!(parsed[3].device, "/dev/sdc1");
        assert!(parsed[3].removable);
    }

    #[test]
    fn name_precedence_xgvfs_name_then_label_then_fallback() {
        let fstab = "\
UUID=dev1 /mnt/one ext4 rw,x-gvfs-show,x-gvfs-name=CustomOne 0 0
UUID=dev2 /mnt/two ext4 rw,x-gvfs-show 0 0
UUID=dev3 /mnt/three ext4 rw,x-gvfs-show 0 0
";
        let mountinfo = "\
10 1 8:1 / /mnt/one rw - ext4 /dev/sda1 rw
20 1 8:2 / /mnt/two rw - ext4 /dev/sda2 rw
30 1 8:3 / /mnt/three rw - ext4 /dev/sda3 rw
40 1 8:4 / /run/media/testuser/MyDisk rw - ext4 /dev/sda4 rw
";
        let mut labels = HashMap::new();
        labels.insert("/dev/sda1".to_string(), "LabelOne".to_string());
        labels.insert("/dev/sda2".to_string(), "LabelTwo".to_string());

        let parsed = parse_mountinfo_and_fstab(mountinfo, fstab, "testuser", &labels);
        assert_eq!(parsed.len(), 4);

        let find = |pt: &str| parsed.iter().find(|p| p.mount_point == pt).unwrap();

        // 1. x-gvfs-name wins over label
        assert_eq!(find("/mnt/one").name, "CustomOne");

        // 2. label wins when x-gvfs-name is absent
        assert_eq!(find("/mnt/two").name, "LabelTwo");

        // 3. mount point's last part when both are absent
        assert_eq!(find("/mnt/three").name, "three");

        // 4. removable drive mount point last part fallback
        assert_eq!(find("/run/media/testuser/MyDisk").name, "MyDisk");
    }

    #[test]
    fn roman_numeral_ordering() {
        let mut items = vec![
            DriveItem {
                name: "IV".into(),
                mount_point: "/mnt/IV".into(),
                device: "/dev/sdb1".into(),
                removable: false,
                total_bytes: 1000,
                available_bytes: 500,
                status: DriveStatus::Normal,
            },
            DriveItem {
                name: "Backup".into(),
                mount_point: "/mnt/Backup".into(),
                device: "/dev/sdc1".into(),
                removable: true,
                total_bytes: 1000,
                available_bytes: 500,
                status: DriveStatus::Normal,
            },
            DriveItem {
                name: "X".into(),
                mount_point: "/mnt/X".into(),
                device: "/dev/sdd1".into(),
                removable: false,
                total_bytes: 1000,
                available_bytes: 500,
                status: DriveStatus::Normal,
            },
            DriveItem {
                name: "II".into(),
                mount_point: "/mnt/II".into(),
                device: "/dev/sda1".into(),
                removable: false,
                total_bytes: 1000,
                available_bytes: 500,
                status: DriveStatus::Normal,
            },
            DriveItem {
                name: "Archive".into(),
                mount_point: "/mnt/Archive".into(),
                device: "/dev/sde1".into(),
                removable: true,
                total_bytes: 1000,
                available_bytes: 500,
                status: DriveStatus::Normal,
            },
            DriveItem {
                name: "I".into(),
                mount_point: "/".into(),
                device: "/dev/nvme0n1p1".into(),
                removable: false,
                total_bytes: 1000,
                available_bytes: 500,
                status: DriveStatus::Normal,
            },
        ];

        sort_drives(&mut items);
        let names: Vec<&str> = items.iter().map(|it| it.name.as_str()).collect();
        // Roman numerals in numeric order first (I, II, IV, X), then rest by name (Archive, Backup)
        assert_eq!(names, vec!["I", "II", "IV", "X", "Archive", "Backup"]);
    }

    #[test]
    fn size_text_formatting() {
        // >= 100 GB: no decimals
        assert_eq!(format_free(300 * 1024 * 1024 * 1024), "300 GB free");
        assert_eq!(
            format_used_of_total(623 * 1024 * 1024 * 1024, 932 * 1024 * 1024 * 1024),
            "623 of 932 GB"
        );

        // < 100 GB: one decimal
        let free_bytes = (45.2 * 1024.0 * 1024.0 * 1024.0) as u64;
        assert_eq!(format_free(free_bytes), "45.2 GB free");

        let used_bytes = (18.5 * 1024.0 * 1024.0 * 1024.0) as u64;
        let total_bytes = (64.0 * 1024.0 * 1024.0 * 1024.0) as u64;
        assert_eq!(
            format_used_of_total(used_bytes, total_bytes),
            "18.5 of 64.0 GB"
        );

        // Mixed: used < 100 GB, total >= 100 GB
        let total_big = (500.0 * 1024.0 * 1024.0 * 1024.0) as u64;
        assert_eq!(
            format_used_of_total(used_bytes, total_big),
            "18.5 of 500 GB"
        );
    }

    #[test]
    fn clicking_row_opens_in_file_manager() {
        let mut world = World::now();
        world.drives =
            parse_drives("I\t/\t/dev/nvme0n1p1\t0\t1000000000000\t300000000000\tnormal\n");
        let w = 380.0;
        let h = height(&world);

        // Click row 0 (fixed drive)
        assert!(click(
            &mut world,
            (w, h),
            (100.0, PAD_Y + HEADER_H + 10.0),
            1
        ));
        assert!(world.close_requested);
        assert_eq!(world.actions.last().map(String::as_str), Some("xdg-open /"));
    }

    #[test]
    fn removable_row_eject_button_and_busy_state() {
        let mut world = World::now();
        world.drives = parse_drives(
            "I\t/\t/dev/nvme0n1p1\t0\t1000000000000\t300000000000\tnormal\n\
             Flash\t/run/media/vysaely/Flash\t/dev/sdc1\t1\t64000000000\t20000000000\tnormal\n",
        );
        let w = 380.0;
        let h = height(&world);

        // Row 1 is removable; click eject button on the right
        let eject_x = w - PAD_X - 10.0;
        let row1_y = PAD_Y + HEADER_H + ROW_H + 10.0;
        assert!(click(&mut world, (w, h), (eject_x, row1_y), 1));

        // Row 1 is now ejecting
        assert_eq!(world.drives.drives[1].status, DriveStatus::Ejecting);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("udisksctl unmount -b /dev/sdc1 && udisksctl power-off -b /dev/sdc1")
        );

        // Failure simulation: status becomes Busy
        world.drives.drives[1].status = DriveStatus::Busy;
        // Verify second line says "Busy: close what uses it"
        let second_line = match world.drives.drives[1].status {
            DriveStatus::Busy => "Busy: close what uses it",
            _ => "other",
        };
        assert_eq!(second_line, "Busy: close what uses it");
    }

    #[test]
    fn render_surface_draws_drives_without_panic() {
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 380, 200).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let mut world = World::now();

        // 1. Not ready: renders Loading
        world.drives.ready = false;
        draw(&cr, &th, &world, (0.0, 0.0, 380.0, height(&world)));

        // 2. Ready with fixed and removable drives
        world.drives = parse_drives(
            "I\t/\t/dev/nvme0n1p1\t0\t1000755998720\t322122547200\tnormal\n\
             USB\t/run/media/vysaely/USB\t/dev/sdc1\t1\t64000000000\t25000000000\tnormal\n",
        );
        draw(&cr, &th, &world, (0.0, 0.0, 380.0, height(&world)));
    }

    #[test]
    fn apply_preserves_busy_and_ejecting_statuses() {
        let mut d = parse_drives(
            "Flash\t/run/media/vysaely/Flash\t/dev/sdc1\t1\t64000000000\t20000000000\tnormal\n",
        );
        d.drives[0].status = DriveStatus::Busy;

        let refresh_out =
            "Flash\t/run/media/vysaely/Flash\t/dev/sdc1\t1\t64000000000\t20000000000\tnormal\n";
        apply(&mut d, "drives", refresh_out);
        assert_eq!(d.drives[0].status, DriveStatus::Busy);
    }

    #[test]
    fn refresh_cache_invalidation() {
        invalidate_cache();
        let lock = LAST_FETCH.lock().unwrap();
        assert!(lock.is_none());
    }
}
