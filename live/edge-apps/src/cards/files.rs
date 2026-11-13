//! Files (docs/EDGE_APPS.md section 6): `drives` (mounted fstab `x-gvfs-show` and user
//! removable drives with free space and eject) and `folders` (XDG home folders, GTK bookmarks, and
//! recent folders from `recently-used.xbel`).

use crate::card::{
    parse_render_data, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status,
};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Claim, Listed, Provider, Size};
use crate::theme::Rgba;
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Files;

const CLAIMS: &[Claim] = &[Claim::Categories(&["FileManager"])];

const KINDS: &[CardKind] = &[
    CardKind {
        id: "drives",
        title: "Drives",
        about: "Mounted drives, free space and eject",
        sizes: &[Size::S],
        default: Size::S,
        half: false,
        sources: &["drives"],
    },
    CardKind {
        id: "folders",
        title: "Folders",
        about: "Home folders, bookmarks and recent folders",
        sizes: &[Size::S, Size::M],
        default: Size::S,
        half: false,
        sources: &["folders"],
    },
];

impl Provider for Files {
    fn id(&self) -> &'static str {
        "files"
    }
    fn name(&self) -> &'static str {
        "Files"
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
    fn card(&self, kind: &str, _argument: Option<&str>) -> Option<Box<dyn Card>> {
        match kind {
            "drives" => Some(Box::new(DrivesCard::default())),
            "folders" => Some(Box::new(FoldersCard::default())),
            _ => None,
        }
    }
    fn source(&self, key: &str) -> Option<Box<dyn Source>> {
        match key {
            "drives" => Some(Box::new(DrivesSource::system())),
            "folders" => Some(Box::new(FoldersSource::system())),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Drives

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const DRIVE_ROW_H: f64 = 54.0;

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
    (number_to_roman(total).as_deref() == Some(s)).then_some(total)
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
                gvfs_name = Some(unescape_octal(name));
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
        } else if !user.is_empty() {
            if let Some(sub) = mount_point.strip_prefix(&user_media_prefix) {
                if !sub.is_empty() {
                    mounts_by_point.insert(mount_point, (device, true));
                }
            }
        }
    }

    let mut result = Vec::new();
    for (mount_point, (device, removable)) in mounts_by_point {
        let fstab_name = fstab_map
            .get(&mount_point)
            .and_then(|e| e.gvfs_name.as_ref())
            .filter(|n| !n.is_empty())
            .cloned();
        let name = fstab_name
            .or_else(|| labels.get(&device).cloned())
            .unwrap_or_else(|| mount_last_part(&mount_point));

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

/// Maps device path -> filesystem label from `root/dev/disk/by-label`.
pub fn read_disk_by_label_from(dir: &Path, root: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return map;
    };
    let canon_root = std::fs::canonicalize(root).ok();
    for entry in entries.flatten() {
        let label = entry.file_name().to_string_lossy().into_owned();
        let Ok(target) = std::fs::canonicalize(entry.path()) else {
            continue;
        };
        let dev = if root != Path::new("/") {
            if let Some(ref cr) = canon_root {
                if let Ok(rel) = target.strip_prefix(cr) {
                    format!("/{}", rel.to_string_lossy())
                } else {
                    target.to_string_lossy().into_owned()
                }
            } else {
                target.to_string_lossy().into_owned()
            }
        } else {
            target.to_string_lossy().into_owned()
        };
        map.insert(dev, label);
    }
    map
}

pub fn statvfs_sizes(path: &Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
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
    if !s.is_empty()
        && s.chars()
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

pub struct DrivesSource {
    pub root: PathBuf,
    pub user: String,
}

impl DrivesSource {
    pub fn system() -> Self {
        let user = std::env::var("USER")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("LOGNAME").ok().filter(|s| !s.is_empty()))
            .unwrap_or_default();
        Self {
            root: PathBuf::from("/"),
            user,
        }
    }

    pub fn fetch_data(&self) -> DrivesData {
        // Allows `--render` fixtures (`INFYNITAE_EDGE_RENDER_DATA`) to supply drives lines without
        // touching host `/etc/fstab` or `/proc/self/mountinfo`.
        if self.root == Path::new("/") {
            if let Some(path) = std::env::var_os("INFYNITAE_EDGE_RENDER_DATA") {
                if let Ok(text) = std::fs::read_to_string(path) {
                    if let Some(val) = parse_render_data(&text).get("drives") {
                        return parse_drives(val);
                    }
                }
            }
        }
        let mountinfo =
            std::fs::read_to_string(self.root.join("proc/self/mountinfo")).unwrap_or_default();
        let fstab = std::fs::read_to_string(self.root.join("etc/fstab")).unwrap_or_default();
        let labels = read_disk_by_label_from(&self.root.join("dev/disk/by-label"), &self.root);
        let parsed = parse_mountinfo_and_fstab(&mountinfo, &fstab, &self.user, &labels);

        let mut drives = Vec::with_capacity(parsed.len());
        for p in parsed {
            let stat_path = if self.root == Path::new("/") {
                PathBuf::from(&p.mount_point)
            } else {
                self.root.join(p.mount_point.trim_start_matches('/'))
            };
            let (total_bytes, available_bytes) = statvfs_sizes(&stat_path).unwrap_or((0, 0));
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
        DrivesData {
            ready: true,
            drives,
        }
    }
}

impl Source for DrivesSource {
    fn fetch(&mut self) -> Data {
        Box::new(self.fetch_data())
    }
    fn every(&self) -> Option<Duration> {
        Some(Duration::from_secs(2))
    }
}

#[derive(Default)]
pub struct DrivesCard {
    pub data: Option<DrivesData>,
}

fn draw_fixed_drive_icon(cr: &Context, color: Rgba, cx: f64, cy: f64) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    draw::round_rect(cr, cx - 7.0, cy - 5.0, 14.0, 10.0, 1.5);
    let _ = cr.stroke();
    cr.move_to(cx - 4.5, cy + 1.5);
    cr.line_to(cx + 0.5, cy + 1.5);
    let _ = cr.stroke();
    cr.new_sub_path();
    cr.arc(cx + 3.5, cy + 1.5, 0.8, 0.0, 2.0 * std::f64::consts::PI);
    let _ = cr.fill();
}

fn draw_removable_icon(cr: &Context, color: Rgba, cx: f64, cy: f64) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    draw::round_rect(cr, cx - 6.5, cy - 3.5, 9.0, 7.0, 1.0);
    let _ = cr.stroke();
    cr.rectangle(cx + 2.5, cy - 2.5, 4.0, 5.0);
    let _ = cr.stroke();
}

fn draw_eject_icon(cr: &Context, color: Rgba, cx: f64, cy: f64) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
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

impl Card for DrivesCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "drives", data.downcast_ref::<DrivesData>()) else {
            return false;
        };
        let mut next = d.clone();
        if let Some(prev) = &self.data {
            let prev_statuses: HashMap<&str, DriveStatus> = prev
                .drives
                .iter()
                .map(|item| (item.device.as_str(), item.status))
                .collect();
            for item in &mut next.drives {
                if item.status == DriveStatus::Normal {
                    if let Some(&status) = prev_statuses.get(item.device.as_str()) {
                        if matches!(status, DriveStatus::Busy | DriveStatus::Ejecting) {
                            item.status = status;
                        }
                    }
                }
            }
        }
        let changed = self.data.as_ref() != Some(&next);
        self.data = Some(next);
        changed
    }

    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if !d.ready => Status::Loading,
            Some(d) if d.drives.is_empty() => Status::Missing("No drives".into()),
            Some(_) => Status::Ready,
        }
    }

    fn height(&self, _: &Cx, _: f64) -> f64 {
        match &self.data {
            Some(d) if d.ready && !d.drives.is_empty() => d.drives.len() as f64 * DRIVE_ROW_H - 4.0,
            _ => 0.0,
        }
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else { return };
        let th = cx.theme;
        for (i, drive) in d.drives.iter().enumerate() {
            let ry = body.1 + i as f64 * DRIVE_ROW_H;
            let row_r = (body.0, ry, body.2, DRIVE_ROW_H - 4.0);
            let row_id = (i as u32) * 2;
            let eject_id = row_id + 1;

            if hits.add(row_id, row_r) || hits.selected(row_id) {
                ui::light(cr, cx, row_r);
            }
            let eject_r = (row_r.0 + row_r.2 - 30.0, ry + 2.0, 26.0, 24.0);
            let eject_lit =
                drive.removable && (hits.add(eject_id, eject_r) || hits.selected(eject_id));
            if eject_lit {
                ui::light(cr, cx, eject_r);
            }

            let line1_y = ry + 5.0;
            let icon_cx = row_r.0 + 16.0;
            let icon_cy = line1_y + 8.0;
            if drive.removable {
                draw_removable_icon(cr, th.muted, icon_cx, icon_cy);
            } else {
                draw_fixed_drive_icon(cr, th.muted, icon_cx, icon_cy);
            }

            let content_x = row_r.0 + 34.0;
            let content_end = if drive.removable {
                let eject_cx = (eject_r.0 + eject_r.2 / 2.0).round();
                let eject_cy = (eject_r.1 + eject_r.3 / 2.0).round();
                draw_eject_icon(
                    cr,
                    if eject_lit { th.text } else { th.muted },
                    eject_cx,
                    eject_cy,
                );
                row_r.0 + row_r.2 - 34.0
            } else {
                row_r.0 + row_r.2 - 8.0
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

            let bar_y = line1_y + 22.0;
            let bar_w = (content_end - content_x).max(10.0);
            let used_bytes = drive.total_bytes.saturating_sub(drive.available_bytes);
            let part = if drive.total_bytes > 0 {
                (used_bytes as f64 / drive.total_bytes as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            draw::bar(cr, th, content_x, bar_y, bar_w, part, th.accent);

            let line2_y = bar_y + 7.0;
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

    fn event(&mut self, ev: &Event, _: &Cx, out: &mut Vec<Action>) -> bool {
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
        let idx = (*hit / 2) as usize;
        let Some(drive) = d.drives.get_mut(idx) else {
            return false;
        };
        if *hit % 2 == 1 {
            if !drive.removable || drive.status == DriveStatus::Ejecting {
                return false;
            }
            drive.status = DriveStatus::Ejecting;
            out.push(Action::Run(eject_command(&drive.device)));
            true
        } else {
            out.push(Action::OpenUri(drive.mount_point.clone()));
            true
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Folders

const MAX_FOLDERS_S: usize = 5;
const MAX_FOLDERS_M: usize = 10;
const FOLDER_ROW_H: f64 = 36.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderItem {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FoldersData {
    pub folders: Vec<FolderItem>,
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn parent_label(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(folder_name)
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn unescape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            let entity = &after[..semi];
            let ch = match entity {
                "amp" => Some('&'),
                "apos" => Some('\''),
                "quot" => Some('"'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                }
                _ if entity.starts_with('#') => {
                    entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(c) = ch {
                out.push(c);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn file_uri_to_path(uri: &str) -> Option<(PathBuf, bool)> {
    let rest = uri.trim().strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let decoded = percent_decode(rest);
    let trailing_slash = decoded.len() > 1 && decoded.ends_with('/');
    let trimmed = if decoded.len() > 1 {
        decoded.trim_end_matches('/')
    } else {
        &decoded
    };
    if trimmed.is_empty() {
        return None;
    }
    Some((PathBuf::from(trimmed), trailing_slash))
}

fn unquote_shell_dir(raw: &str) -> String {
    let s = raw.trim();
    if let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else {
                out.push(c);
            }
        }
        out
    } else if let Some(inner) = s.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        inner.to_string()
    } else {
        s.to_string()
    }
}

fn expand_home(val: &str, home: &Path) -> Option<PathBuf> {
    if val == "$HOME" || val == "$HOME/" || val == "${HOME}" || val == "${HOME}/" {
        return Some(home.to_path_buf());
    }
    if let Some(rest) = val
        .strip_prefix("$HOME/")
        .or_else(|| val.strip_prefix("${HOME}/"))
    {
        let trimmed = rest.trim_end_matches('/');
        return Some(if trimmed.is_empty() {
            home.to_path_buf()
        } else {
            home.join(trimmed)
        });
    }
    if val.starts_with('/') {
        let trimmed = val.trim_end_matches('/');
        return Some(PathBuf::from(if trimmed.is_empty() {
            "/"
        } else {
            trimmed
        }));
    }
    None
}

/// Parses `~/.config/user-dirs.dirs` into `(key, path)` pairs, ordered so primary folders
/// (`Documents`, `Downloads`, `Pictures`, `Videos`, `Music`, `Desktop`) appear before extra XDG
/// entries, `Public`, and `Templates`.
pub fn parse_user_dirs(text: &str, home: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !key.starts_with("XDG_") || !key.ends_with("_DIR") {
            continue;
        }
        let unquoted = unquote_shell_dir(val);
        let Some(path) = expand_home(&unquoted, home) else {
            continue;
        };
        if path == home {
            continue;
        }
        entries.push((key.to_string(), path));
    }

    let rank = |k: &str| match k {
        "XDG_DOCUMENTS_DIR" => 0,
        "XDG_DOWNLOAD_DIR" => 1,
        "XDG_PICTURES_DIR" => 2,
        "XDG_VIDEOS_DIR" => 3,
        "XDG_MUSIC_DIR" => 4,
        "XDG_DESKTOP_DIR" => 5,
        "XDG_PUBLICSHARE_DIR" => 7,
        "XDG_TEMPLATES_DIR" => 8,
        _ => 6,
    };
    entries.sort_by_key(|(k, _)| rank(k));
    entries.into_iter().map(|(_, p)| p).collect()
}

/// Parses `~/.config/gtk-3.0/bookmarks` (`file://` URI and optional label per line).
pub fn parse_gtk_bookmarks(text: &str) -> Vec<(PathBuf, Option<String>)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (uri, label) = match line.split_once([' ', '\t']) {
            Some((u, l)) => {
                let l = l.trim();
                (u, (!l.is_empty()).then(|| l.to_string()))
            }
            None => (line, None),
        };
        if let Some((path, _)) = file_uri_to_path(uri) {
            out.push((path, label));
        }
    }
    out
}

fn find_tag_end(s: &str) -> Option<(usize, bool)> {
    let mut quote: Option<char> = None;
    let mut prev_slash = false;
    for (idx, ch) in s.char_indices() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '>' => return Some((idx, prev_slash)),
            None => {}
        }
        prev_slash = ch == '/' && quote.is_none();
    }
    None
}

fn parse_attr(attrs: &str, target: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let name = &attrs[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let val = if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
            let q = bytes[i];
            i += 1;
            let v_start = i;
            while i < bytes.len() && bytes[i] != q {
                i += 1;
            }
            let v = &attrs[v_start..i];
            if i < bytes.len() {
                i += 1;
            }
            v
        } else {
            let v_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' {
                i += 1;
            }
            &attrs[v_start..i]
        };
        if name == target {
            return Some(unescape_xml(val));
        }
    }
    None
}

fn parse_xbel_time(s: &str) -> Option<(i64, u32)> {
    let t = s.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t) {
        return Some((dt.timestamp(), dt.timestamp_subsec_nanos()));
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S") {
        return Some((ndt.and_utc().timestamp(), 0));
    }
    None
}

/// Extracts candidate folder paths from `~/.local/share/recently-used.xbel`, newest `modified`
/// first.
pub fn parse_recent_xbel(text: &str) -> Vec<PathBuf> {
    let mut entries: Vec<(Option<(i64, u32)>, PathBuf)> = Vec::new();
    let mut pos = 0;
    while let Some(rel) = text[pos..].find("<bookmark") {
        let tag_start = pos + rel;
        let after_kw = tag_start + "<bookmark".len();
        let Some(next_ch) = text[after_kw..].chars().next() else {
            break;
        };
        if !next_ch.is_ascii_whitespace() {
            pos = after_kw;
            continue;
        }
        let Some((tag_end, self_closing)) = find_tag_end(&text[after_kw..]) else {
            break;
        };
        let attrs = &text[after_kw..after_kw + tag_end];
        let block_start = after_kw + tag_end + 1;
        let (is_dir_mime, next_pos) = if self_closing {
            (false, block_start)
        } else if let Some(end_rel) = text[block_start..].find("</bookmark>") {
            let block = &text[block_start..block_start + end_rel];
            let is_dir = block.contains("type=\"inode/directory\"")
                || block.contains("type='inode/directory'");
            (is_dir, block_start + end_rel + "</bookmark>".len())
        } else {
            (false, block_start)
        };
        pos = next_pos;

        let Some(href) = parse_attr(attrs, "href") else {
            continue;
        };
        let Some((path, trailing_slash)) = file_uri_to_path(&href) else {
            continue;
        };
        let folder = if is_dir_mime || trailing_slash {
            path
        } else {
            let Some(parent) = path.parent() else {
                continue;
            };
            parent.to_path_buf()
        };
        let time = parse_attr(attrs, "modified")
            .or_else(|| parse_attr(attrs, "visited"))
            .or_else(|| parse_attr(attrs, "added"))
            .and_then(|s| parse_xbel_time(&s));
        entries.push((time, folder));
    }

    entries.sort_by_key(|a| std::cmp::Reverse(a.0));
    entries.into_iter().map(|(_, p)| p).collect()
}

pub struct FoldersSource {
    pub home: PathBuf,
}

impl FoldersSource {
    pub fn system() -> Self {
        let home = std::env::var_os("HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_default();
        Self { home }
    }

    pub fn fetch_data(&self) -> FoldersData {
        if self.home.as_os_str().is_empty() {
            return FoldersData::default();
        }
        let mut folders = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut push_dir = |path: PathBuf, custom_name: Option<String>| {
            if !path.is_dir() {
                return;
            }
            let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !seen.insert(key) {
                return;
            }
            let name = custom_name.unwrap_or_else(|| folder_name(&path));
            folders.push(FolderItem {
                name,
                path: path.to_string_lossy().into_owned(),
            });
        };

        let user_dirs_path = self.home.join(".config/user-dirs.dirs");
        let xdg_candidates = if let Ok(text) = std::fs::read_to_string(&user_dirs_path) {
            parse_user_dirs(&text, &self.home)
        } else {
            [
                "Documents",
                "Downloads",
                "Pictures",
                "Videos",
                "Music",
                "Desktop",
                "Public",
                "Templates",
            ]
            .iter()
            .map(|d| self.home.join(d))
            .collect()
        };
        for p in xdg_candidates {
            push_dir(p, None);
        }

        if let Ok(text) = std::fs::read_to_string(self.home.join(".config/gtk-3.0/bookmarks")) {
            for (p, label) in parse_gtk_bookmarks(&text) {
                push_dir(p, label);
            }
        }

        if let Ok(text) = std::fs::read_to_string(self.home.join(".local/share/recently-used.xbel"))
        {
            for p in parse_recent_xbel(&text) {
                push_dir(p, None);
            }
        }

        FoldersData { folders }
    }
}

impl Source for FoldersSource {
    fn fetch(&mut self) -> Data {
        Box::new(self.fetch_data())
    }
    fn every(&self) -> Option<Duration> {
        None
    }
}

#[derive(Default)]
pub struct FoldersCard {
    pub data: Option<FoldersData>,
}

impl FoldersCard {
    fn shown(&self, cx: &Cx) -> &[FolderItem] {
        let Some(d) = &self.data else { return &[] };
        let limit = match cx.size {
            Size::S => MAX_FOLDERS_S,
            Size::M | Size::L => MAX_FOLDERS_M,
        };
        &d.folders[..d.folders.len().min(limit)]
    }
}

impl Card for FoldersCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "folders", data.downcast_ref::<FoldersData>()) else {
            return false;
        };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
    }

    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(d) if d.folders.is_empty() => Status::Missing("No folders".into()),
            Some(_) => Status::Ready,
        }
    }

    fn height(&self, cx: &Cx, _: f64) -> f64 {
        self.shown(cx).len() as f64 * FOLDER_ROW_H
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let shown = self.shown(cx);
        let th = cx.theme;
        for (i, f) in shown.iter().enumerate() {
            let dup = shown.iter().filter(|o| o.name == f.name).count() > 1;
            let note = if dup {
                parent_label(&f.path)
            } else {
                String::new()
            };
            let right = if note.is_empty() {
                44.0
            } else {
                let nw = draw::measure(
                    cr,
                    &Text {
                        size: draw::SIZE_NOTE,
                        bold: false,
                        color: th.muted,
                        text: &note,
                    },
                )
                .0;
                (nw + 16.0).max(44.0)
            };
            let row_r = (
                body.0,
                body.1 + i as f64 * FOLDER_ROW_H,
                body.2,
                FOLDER_ROW_H,
            );
            let lit = ui::row(
                cr,
                cx,
                hits,
                i as u32,
                row_r,
                &Row {
                    icon: None,
                    name: &f.name,
                    state: "",
                    on: false,
                    right,
                },
            );
            let right_text = if lit { "Open" } else { note.as_str() };
            if !right_text.is_empty() {
                draw::text_right(
                    cr,
                    row_r.0 + row_r.2 - 8.0,
                    row_r.1 + (row_r.3 - 15.0) / 2.0,
                    &Text {
                        size: if lit {
                            draw::SIZE_SMALL
                        } else {
                            draw::SIZE_NOTE
                        },
                        bold: false,
                        color: th.muted,
                        text: right_text,
                    },
                );
            }
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Event::Press {
            hit,
            button: Button::Left,
            ..
        } = ev
        else {
            return false;
        };
        let Some(f) = self.shown(cx).get(*hit as usize) else {
            return false;
        };
        out.push(Action::OpenUri(f.path.clone()));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::provider::{owner_of, resolve};
    use crate::theme::Theme;

    fn test_cx(th: &Theme, size: Size) -> Cx<'_> {
        Cx {
            theme: th,
            now: chrono::NaiveDate::from_ymd_opt(2026, 11, 13)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            size,
            app: None,
            apps: &[],
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "edge-apps-files-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // -----------------------------------------------------------------------------------------
    // Drives tests (ported from live/edge-rs/src/modules/drives.rs + card/source tests)

    #[test]
    fn drives_not_ready_shows_loading() {
        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th, Size::S);
        let mut c = DrivesCard::default();
        assert_eq!(c.status(&cx), Status::Loading);

        c.apply("drives", &DrivesData::default() as &dyn Any);
        assert_eq!(c.status(&cx), Status::Loading);

        c.apply(
            "drives",
            &DrivesData {
                ready: true,
                drives: Vec::new(),
            } as &dyn Any,
        );
        assert_eq!(c.status(&cx), Status::Missing("No drives".into()));
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
        assert_eq!(find("/mnt/one").name, "CustomOne");
        assert_eq!(find("/mnt/two").name, "LabelTwo");
        assert_eq!(find("/mnt/three").name, "three");
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
        assert_eq!(names, vec!["I", "II", "IV", "X", "Archive", "Backup"]);
    }

    #[test]
    fn size_text_formatting() {
        assert_eq!(format_free(300 * 1024 * 1024 * 1024), "300 GB free");
        assert_eq!(
            format_used_of_total(623 * 1024 * 1024 * 1024, 932 * 1024 * 1024 * 1024),
            "623 of 932 GB"
        );

        let free_bytes = (45.2 * 1024.0 * 1024.0 * 1024.0) as u64;
        assert_eq!(format_free(free_bytes), "45.2 GB free");

        let used_bytes = (18.5 * 1024.0 * 1024.0 * 1024.0) as u64;
        let total_bytes = (64.0 * 1024.0 * 1024.0 * 1024.0) as u64;
        assert_eq!(
            format_used_of_total(used_bytes, total_bytes),
            "18.5 of 64.0 GB"
        );

        let total_big = (500.0 * 1024.0 * 1024.0 * 1024.0) as u64;
        assert_eq!(
            format_used_of_total(used_bytes, total_big),
            "18.5 of 500 GB"
        );
    }

    #[test]
    fn clicking_row_opens_in_file_manager() {
        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th, Size::S);
        let mut c = DrivesCard::default();
        c.apply(
            "drives",
            &parse_drives("I\t/\t/dev/nvme0n1p1\t0\t1000000000000\t300000000000\tnormal\n")
                as &dyn Any,
        );
        assert_eq!(c.status(&cx), Status::Ready);

        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: 0,
                button: Button::Left,
                at: (50.0, 20.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(out, vec![Action::OpenUri("/".into())]);

        let mut out_right = Vec::new();
        assert!(!c.event(
            &Event::Press {
                hit: 0,
                button: Button::Right,
                at: (50.0, 20.0),
            },
            &cx,
            &mut out_right,
        ));
        assert!(out_right.is_empty());
    }

    #[test]
    fn removable_row_eject_button_and_busy_state() {
        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th, Size::S);
        let mut c = DrivesCard::default();
        c.apply(
            "drives",
            &parse_drives(
                "I\t/\t/dev/nvme0n1p1\t0\t1000000000000\t300000000000\tnormal\n\
                 Flash\t/run/media/vysaely/Flash\t/dev/sdc1\t1\t64000000000\t20000000000\tnormal\n",
            ) as &dyn Any,
        );

        let mut out = Vec::new();
        // Fixed drive has no eject hit (hit 1 belongs to drive 0)
        assert!(!c.event(
            &Event::Press {
                hit: 1,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        ));
        assert!(out.is_empty());

        // Drive 1 eject hit is 3
        assert!(c.event(
            &Event::Press {
                hit: 3,
                button: Button::Left,
                at: (340.0, 64.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(
            c.data.as_ref().unwrap().drives[1].status,
            DriveStatus::Ejecting
        );
        assert_eq!(
            out,
            vec![Action::Run(
                "udisksctl unmount -b /dev/sdc1 && udisksctl power-off -b /dev/sdc1".into()
            )]
        );

        // Clicking eject while already ejecting is ignored
        let mut out2 = Vec::new();
        assert!(!c.event(
            &Event::Press {
                hit: 3,
                button: Button::Left,
                at: (340.0, 64.0),
            },
            &cx,
            &mut out2,
        ));
        assert!(out2.is_empty());
    }

    #[test]
    fn apply_preserves_busy_and_ejecting_statuses() {
        let mut c = DrivesCard {
            data: Some(parse_drives(
                "Flash\t/run/media/vysaely/Flash\t/dev/sdc1\t1\t64000000000\t20000000000\tnormal\n\
                 Card\t/run/media/vysaely/Card\t/dev/sdd1\t1\t64000000000\t20000000000\tnormal\n",
            )),
        };
        c.data.as_mut().unwrap().drives[0].status = DriveStatus::Ejecting;
        c.data.as_mut().unwrap().drives[1].status = DriveStatus::Busy;

        let refresh = parse_drives(
            "Flash\t/run/media/vysaely/Flash\t/dev/sdc1\t1\t64000000000\t20000000000\tnormal\n\
             Card\t/run/media/vysaely/Card\t/dev/sdd1\t1\t64000000000\t20000000000\tnormal\n",
        );
        assert!(!c.apply("drives", &refresh as &dyn Any));
        let drives = &c.data.as_ref().unwrap().drives;
        assert_eq!(drives[0].status, DriveStatus::Ejecting);
        assert_eq!(drives[1].status, DriveStatus::Busy);
    }

    #[test]
    fn render_surface_draws_drives_without_panic_and_height_matches_hits() {
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 380, 300).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let th = Theme::from_config(&Config::from_text(""));
        let cx = test_cx(&th, Size::S);
        let mut c = DrivesCard::default();

        assert_eq!(c.height(&cx, 360.0), 0.0);

        c.apply(
            "drives",
            &parse_drives(
                "I\t/\t/dev/nvme0n1p1\t0\t1000755998720\t322122547200\tnormal\n\
                 USB\t/run/media/vysaely/USB\t/dev/sdc1\t1\t64000000000\t25000000000\tbusy\n",
            ) as &dyn Any,
        );
        let h = c.height(&cx, 360.0);
        assert_eq!(h, 2.0 * DRIVE_ROW_H - 4.0);
        let body = (10.0, 40.0, 360.0, h);
        let mut hits = Hits::new(Some((350.0, 40.0 + DRIVE_ROW_H + 10.0)), None);
        c.draw(&cr, &cx, body, &mut hits);

        // Drive 0 (fixed): hit 0 only; Drive 1 (removable): hit 2 (row) and hit 3 (eject, on top)
        assert_eq!(
            hits.list.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![0, 2, 3]
        );
        assert_eq!(hits.top_at((350.0, 40.0 + DRIVE_ROW_H + 10.0)), Some(3));
        assert_eq!(hits.top_at((50.0, 40.0 + DRIVE_ROW_H + 10.0)), Some(2));
        for (_, r) in &hits.list {
            assert!(
                r.0 >= body.0
                    && r.1 >= body.1
                    && r.0 + r.2 <= body.0 + body.2
                    && r.1 + r.3 <= body.1 + body.3,
                "hit {r:?} outside body {body:?}"
            );
        }
    }

    #[test]
    fn drives_parsing_empty_and_broken_inputs() {
        assert!(parse_mountinfo_and_fstab("", "", "vysaely", &HashMap::new()).is_empty());
        assert!(parse_drives("").drives.is_empty());

        let broken_fstab = "not enough fields\n# comment\nUUID=1 /mnt/bad ext4 x-gvfs-show\n";
        let broken_mountinfo = "short line\n1 2 3 4 /mnt/ok rw shared:1 no_dash /dev/sda1 rw\n";
        assert!(parse_mountinfo_and_fstab(
            broken_mountinfo,
            broken_fstab,
            "vysaely",
            &HashMap::new()
        )
        .is_empty());

        let octal = parse_mountinfo_and_fstab(
            "10 1 8:1 / /mnt/My\\040Drive rw - ext4 /dev/sda1 rw\n",
            "UUID=1 /mnt/My\\040Drive ext4 rw,x-gvfs-show 0 0\n",
            "vysaely",
            &HashMap::new(),
        );
        assert_eq!(octal.len(), 1);
        assert_eq!(octal[0].mount_point, "/mnt/My Drive");
        assert_eq!(octal[0].name, "My Drive");
    }

    #[test]
    fn drives_source_reads_temporary_root_with_real_layout() {
        let root = temp_dir("drives-root");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(root.join("proc/self")).unwrap();
        std::fs::create_dir_all(root.join("dev/disk/by-label")).unwrap();
        std::fs::create_dir_all(root.join("mnt/II")).unwrap();
        std::fs::create_dir_all(root.join("mnt/III")).unwrap();
        std::fs::create_dir_all(root.join("run/media/vysaely/Thumb")).unwrap();
        std::fs::write(root.join("dev/sda1"), b"").unwrap();
        std::os::unix::fs::symlink("../../sda1", root.join("dev/disk/by-label/III")).unwrap();

        std::fs::write(
            root.join("etc/fstab"),
            "\
UUID=1 / btrfs rw,x-gvfs-show,x-gvfs-name=I 0 0
UUID=2 /mnt/II btrfs noatime,x-gvfs-show,x-gvfs-name=II 0 0
UUID=3 /mnt/III btrfs noatime,x-gvfs-show 0 0
UUID=4 /mnt/IV btrfs noatime,x-gvfs-show,x-gvfs-name=IV 0 0
",
        )
        .unwrap();
        std::fs::write(
            root.join("proc/self/mountinfo"),
            "\
32 2 0:29 /@ / rw - btrfs /dev/nvme1n1p3 rw
62 32 0:47 / /mnt/II rw - btrfs /dev/nvme0n1p3 rw
246 32 0:49 / /mnt/III rw - btrfs /dev/sda1 rw
254 29 8:35 / /run/media/vysaely/Thumb rw - vfat /dev/sdc1 rw
",
        )
        .unwrap();

        let mut src = DrivesSource {
            root: root.clone(),
            user: "vysaely".into(),
        };
        let boxed = src.fetch();
        let data = boxed.downcast_ref::<DrivesData>().unwrap();
        assert!(data.ready);
        assert_eq!(data.drives.len(), 4);
        assert_eq!(
            data.drives
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            vec!["I", "II", "III", "Thumb"]
        );
        assert!(data.drives[0].total_bytes > 0);
        assert!(!data.drives[0].removable);
        assert!(data.drives[3].removable);

        let _ = std::fs::remove_dir_all(&root);
    }

    // -----------------------------------------------------------------------------------------
    // Folders tests

    #[test]
    fn folders_source_reads_temporary_home_usual_empty_and_broken() {
        let home = temp_dir("folders-home");
        // Empty home: no folders
        let mut empty_src = FoldersSource { home: home.clone() };
        let empty = empty_src.fetch().downcast::<FoldersData>().unwrap();
        assert!(empty.folders.is_empty());

        // Broken files inside home: no panic, returns empty
        std::fs::create_dir_all(home.join(".config/gtk-3.0")).unwrap();
        std::fs::create_dir_all(home.join(".local/share")).unwrap();
        std::fs::write(
            home.join(".config/user-dirs.dirs"),
            "broken line without equals\nXDG_BAD=\"\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".config/gtk-3.0/bookmarks"),
            "not-a-uri\nhttp://example.com Web\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".local/share/recently-used.xbel"),
            "<bookmark href=\"unterminated",
        )
        .unwrap();
        let broken = empty_src.fetch().downcast::<FoldersData>().unwrap();
        assert!(broken.folders.is_empty());

        // Usual layout: XDG folders, bookmarks, recent folders
        for d in [
            "Documents",
            "Downloads",
            "Projects",
            "Games",
            "Alpha",
            "Beta & Gamma",
        ] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        let user_dirs = "# xdg dirs\n\
             XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n\
             XDG_TEMPLATES_DIR=\"$HOME/\"\n\
             XDG_DOWNLOAD_DIR=\"$HOME/Downloads\"\n\
             XDG_DOCUMENTS_DIR=\"$HOME/Documents\"\n\
             XDG_PROJECTS_DIR=\"$HOME/Projects\"\n";
        std::fs::write(home.join(".config/user-dirs.dirs"), user_dirs).unwrap();

        let bookmarks = format!(
            "file://{}/Games My Games\n\
             file://{}/Documents\n\
             file://{}/MissingFolder\n\
             smb://server/share Network Share\n",
            home.display(),
            home.display(),
            home.display(),
        );
        std::fs::write(home.join(".config/gtk-3.0/bookmarks"), bookmarks).unwrap();

        let xbel = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <xbel version=\"1.0\">\n\
               <bookmark href=\"file://{0}/Alpha/old.txt\" modified=\"2026-11-10T10:00:00Z\"/>\n\
               <bookmark href=\"file://{0}/Beta%20&amp;%20Gamma/new.txt\" modified=\"2026-11-12T08:30:00.500Z\"/>\n\
               <bookmark href=\"file://{0}/DeletedDir/file.txt\" modified=\"2026-11-13T01:00:00Z\"/>\n\
               <bookmark href=\"file://{0}/Downloads/pic.png\" modified=\"2026-11-12T20:00:00Z\"/>\n\
               <bookmark href=\"file://{0}/Games\" modified=\"2026-11-12T12:00:00Z\">\n\
                 <info><metadata><mime:mime-type type=\"inode/directory\"/></metadata></info>\n\
               </bookmark>\n\
             </xbel>\n",
            home.display()
        );
        std::fs::write(home.join(".local/share/recently-used.xbel"), xbel).unwrap();

        let usual = empty_src.fetch().downcast::<FoldersData>().unwrap();
        assert_eq!(
            usual
                .folders
                .iter()
                .map(|f| (f.name.as_str(), f.path.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("Documents", home.join("Documents").to_str().unwrap()),
                ("Downloads", home.join("Downloads").to_str().unwrap()),
                ("Projects", home.join("Projects").to_str().unwrap()),
                ("My Games", home.join("Games").to_str().unwrap()),
                ("Beta & Gamma", home.join("Beta & Gamma").to_str().unwrap()),
                ("Alpha", home.join("Alpha").to_str().unwrap()),
            ]
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn folders_fallback_when_user_dirs_file_is_absent() {
        let home = temp_dir("folders-fallback");
        std::fs::create_dir_all(home.join("Documents")).unwrap();
        std::fs::create_dir_all(home.join("Downloads")).unwrap();
        let mut src = FoldersSource { home: home.clone() };
        let data = src.fetch().downcast::<FoldersData>().unwrap();
        assert_eq!(
            data.folders
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Documents", "Downloads"]
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn folders_card_states_sizes_and_clicks() {
        let th = Theme::from_config(&Config::from_text(""));
        let cx_s = test_cx(&th, Size::S);
        let cx_m = test_cx(&th, Size::M);

        let mut c = FoldersCard::default();
        assert_eq!(c.status(&cx_s), Status::Loading);

        c.apply("folders", &FoldersData::default() as &dyn Any);
        assert_eq!(c.status(&cx_s), Status::Missing("No folders".into()));

        let folders: Vec<FolderItem> = (0..12)
            .map(|i| FolderItem {
                name: format!("Folder {i}"),
                path: format!("/tmp/f{i}"),
            })
            .collect();
        assert!(c.apply("folders", &FoldersData { folders } as &dyn Any));
        assert_eq!(c.status(&cx_s), Status::Ready);
        assert_eq!(c.height(&cx_s, 360.0), 5.0 * FOLDER_ROW_H);
        assert_eq!(c.height(&cx_m, 360.0), 10.0 * FOLDER_ROW_H);

        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: 2,
                button: Button::Left,
                at: (20.0, 80.0),
            },
            &cx_s,
            &mut out,
        ));
        assert_eq!(out, vec![Action::OpenUri("/tmp/f2".into())]);

        // Hit 7 is beyond S's 5 rows, valid only in M
        let mut out_oob = Vec::new();
        assert!(!c.event(
            &Event::Press {
                hit: 7,
                button: Button::Left,
                at: (20.0, 260.0),
            },
            &cx_s,
            &mut out_oob,
        ));
        assert!(c.event(
            &Event::Press {
                hit: 7,
                button: Button::Left,
                at: (20.0, 260.0),
            },
            &cx_m,
            &mut out_oob,
        ));
        assert_eq!(out_oob, vec![Action::OpenUri("/tmp/f7".into())]);
    }

    #[test]
    fn folders_draw_matches_height_and_registers_hits() {
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 380, 500).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let th = Theme::from_config(&Config::from_text(""));

        let mut c = FoldersCard::default();
        c.apply(
            "folders",
            &FoldersData {
                folders: vec![
                    FolderItem {
                        name: "Music".into(),
                        path: "/home/vysaely/Music".into(),
                    },
                    FolderItem {
                        name: "Downloads".into(),
                        path: "/home/vysaely/Downloads".into(),
                    },
                    FolderItem {
                        name: "Music".into(),
                        path: "/mnt/III/Save/Music".into(),
                    },
                    FolderItem {
                        name: "Projects".into(),
                        path: "/home/vysaely/Projects".into(),
                    },
                    FolderItem {
                        name: "Videos".into(),
                        path: "/home/vysaely/Videos".into(),
                    },
                    FolderItem {
                        name: "Desktop".into(),
                        path: "/home/vysaely/Desktop".into(),
                    },
                ],
            } as &dyn Any,
        );

        for (size, expected_rows) in [(Size::S, 5), (Size::M, 6)] {
            let cx = test_cx(&th, size);
            let h = c.height(&cx, 360.0);
            assert_eq!(h, expected_rows as f64 * FOLDER_ROW_H);
            let body = (10.0, 40.0, 360.0, h);
            let mut hits = Hits::new(Some((50.0, 50.0)), None);
            c.draw(&cr, &cx, body, &mut hits);
            assert_eq!(hits.list.len(), expected_rows);
            for (_, r) in &hits.list {
                assert!(
                    r.0 >= body.0
                        && r.1 >= body.1
                        && r.0 + r.2 <= body.0 + body.2
                        && r.1 + r.3 <= body.1 + body.3
                );
            }
        }
    }

    #[test]
    fn files_provider_claims_file_managers_and_creates_cards_and_sources() {
        let fm = App {
            id: "pcmanfm-qt".into(),
            name: "PCManFM-Qt File Manager".into(),
            categories: vec![
                "FileManager".into(),
                "Utility".into(),
                "Core".into(),
                "Qt".into(),
            ],
            ..Default::default()
        };
        let apps = [fm.clone()];
        assert_eq!(owner_of(&fm).map(|p| p.id()), Some("files"));

        let (prov, app) = resolve("pcmanfm-qt", &apps).unwrap();
        assert_eq!(prov.id(), "files");
        assert_eq!(app.unwrap().id, "pcmanfm-qt");

        let (prov2, app2) = resolve("files", &apps).unwrap();
        assert_eq!(prov2.id(), "files");
        assert!(app2.is_none());
        assert_eq!(prov2.listed(), Listed::WithoutApp);

        assert!(prov2.card("drives", None).is_some());
        assert!(prov2.card("folders", None).is_some());
        assert!(prov2.card("unknown", None).is_none());

        assert_eq!(
            prov2.source("drives").unwrap().every(),
            Some(Duration::from_secs(2))
        );
        assert_eq!(prov2.source("folders").unwrap().every(), None);
        assert!(prov2.source("unknown").is_none());
    }
}
