//! Places sidebar discovery: Home + XDG user folders (`~/.config/user-dirs.dirs`), saved GTK
//! bookmarks (`~/.config/gtk-3.0/bookmarks`), and Roman-numeral / USB drives (`/etc/fstab`,
//! `/proc/self/mountinfo`, `/dev/disk/by-label`, `/run/media/<user>/`) matching
//! `live/edge-apps/src/cards/files.rs` and `docs/DRIVES.md`.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs;
use std::mem::MaybeUninit;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderPlace {
    pub name: String,
    pub path: PathBuf,
    pub is_home: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrivePlace {
    pub name: String,
    pub mount_point: PathBuf,
    pub device: String,
    pub removable: bool,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

impl DrivePlace {
    pub fn used_fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        let used = self.total_bytes.saturating_sub(self.available_bytes);
        (used as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
    }

    pub fn free_short(&self) -> String {
        if self.total_bytes == 0 {
            return String::new();
        }
        let gb = self.available_bytes as f64 / GIB;
        if gb >= 100.0 {
            format!("{:.0} GB", gb.round())
        } else {
            format!("{:.1} GB", gb)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Places {
    pub folders: Vec<FolderPlace>,
    pub drives: Vec<DrivePlace>,
}

impl Places {
    pub fn load_system() -> Self {
        let home = home_dir();
        let user = std::env::var("USER")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("LOGNAME").ok().filter(|s| !s.is_empty()))
            .unwrap_or_default();
        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".config")));
        Self::load_from(
            Path::new("/"),
            home.as_deref(),
            config_dir.as_deref(),
            &user,
        )
    }

    pub fn load_from(
        root: &Path,
        home: Option<&Path>,
        config_dir: Option<&Path>,
        user: &str,
    ) -> Self {
        let mut folders = Vec::new();
        let mut seen = HashSet::new();

        if let Some(h) = home {
            if h.is_dir() && seen.insert(h.to_path_buf()) {
                folders.push(FolderPlace {
                    name: "Home".to_string(),
                    path: h.to_path_buf(),
                    is_home: true,
                });
            }
            if let Some(cfg) = config_dir {
                if let Ok(text) = fs::read_to_string(cfg.join("user-dirs.dirs")) {
                    for p in parse_user_dirs(&text, h) {
                        if p.is_dir() && seen.insert(p.clone()) {
                            let name = p
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| p.to_string_lossy().into_owned());
                            folders.push(FolderPlace {
                                name,
                                path: p,
                                is_home: false,
                            });
                        }
                    }
                }
                if let Ok(text) = fs::read_to_string(cfg.join("gtk-3.0/bookmarks")) {
                    for (p, label) in parse_gtk_bookmarks(&text) {
                        if p.is_dir() && seen.insert(p.clone()) {
                            let name = label.unwrap_or_else(|| {
                                p.file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| p.to_string_lossy().into_owned())
                            });
                            folders.push(FolderPlace {
                                name,
                                path: p,
                                is_home: false,
                            });
                        }
                    }
                }
            }
        }

        let mountinfo = fs::read_to_string(root.join("proc/self/mountinfo")).unwrap_or_default();
        let fstab = fs::read_to_string(root.join("etc/fstab")).unwrap_or_default();
        let labels = read_disk_by_label_from(&root.join("dev/disk/by-label"), root);
        let parsed = parse_mountinfo_and_fstab(&mountinfo, &fstab, user, &labels);

        let mut drives = Vec::with_capacity(parsed.len());
        for p in parsed {
            let stat_path = if root == Path::new("/") {
                PathBuf::from(&p.mount_point)
            } else {
                root.join(p.mount_point.trim_start_matches('/'))
            };
            let (total_bytes, available_bytes) = statvfs_sizes(&stat_path).unwrap_or((0, 0));
            drives.push(DrivePlace {
                name: p.name,
                mount_point: PathBuf::from(p.mount_point),
                device: p.device,
                removable: p.removable,
                total_bytes,
                available_bytes,
            });
        }

        Self { folders, drives }
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
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

fn mount_last_part(mount_point: &str) -> String {
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

pub fn read_disk_by_label_from(dir: &Path, root: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return map;
    };
    let canon_root = fs::canonicalize(root).ok();
    for entry in entries.flatten() {
        let label = entry.file_name().to_string_lossy().into_owned();
        let Ok(target) = fs::canonicalize(entry.path()) else {
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
    let total_bytes = stat.f_blocks.saturating_mul(frsize);
    let avail_bytes = stat.f_bavail.saturating_mul(frsize);
    Some((total_bytes, avail_bytes))
}

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
        let path = expand_home_dir(&unquoted, home);
        if !path.is_absolute() || path == home {
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

fn unquote_shell_dir(raw: &str) -> String {
    let t = raw.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        let inner = &t[1..t.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else {
                out.push(c);
            }
        }
        out
    } else if t.len() >= 2 && t.starts_with('\'') && t.ends_with('\'') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn expand_home_dir(s: &str, home: &Path) -> PathBuf {
    if s == "$HOME" || s == "${HOME}" || s == "~" {
        return home.to_path_buf();
    }
    for prefix in ["$HOME/", "${HOME}/", "~/"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return home.join(rest);
        }
    }
    PathBuf::from(s)
}

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
        if let Some(path) = file_uri_to_path(uri) {
            out.push((path, label));
        }
    }
    out
}

pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri
        .strip_prefix("file://localhost/")
        .map(|r| format!("/{r}"))
        .or_else(|| uri.strip_prefix("file://").map(str::to_string))?;
    if !rest.starts_with('/') {
        return None;
    }
    let decoded = percent_decode_bytes(rest.as_bytes());
    if decoded.is_empty() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(decoded)))
}

pub fn percent_decode_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roman_numeral_drives_sort_numerically_before_usb() {
        let fstab = "\
UUID=1 /         btrfs defaults,x-gvfs-show,x-gvfs-name=I   0 0
UUID=2 /mnt/IV   ext4  defaults,x-gvfs-show,x-gvfs-name=IV  0 0
UUID=3 /mnt/II   ext4  defaults,x-gvfs-show,x-gvfs-name=II  0 0
UUID=4 /mnt/VI   ntfs  defaults,x-gvfs-show,x-gvfs-name=VI  0 0
UUID=5 /mnt/III  ext4  defaults,x-gvfs-show,x-gvfs-name=III 0 0
UUID=6 /mnt/V    ext4  defaults,x-gvfs-show,x-gvfs-name=V   0 0
";
        let mountinfo = "\
10 1 259:1 / / rw - btrfs /dev/nvme0n1p1 rw
11 1 8:1 / /mnt/IV rw - ext4 /dev/sda1 rw
12 1 8:2 / /mnt/II rw - ext4 /dev/sdb1 rw
13 1 8:3 / /mnt/III rw - ext4 /dev/sdc1 rw
14 1 8:4 / /mnt/V rw - ext4 /dev/sdd1 rw
15 1 8:5 / /mnt/VI rw - ntfs3 /dev/sde1 rw
16 1 8:17 / /run/media/alice/USB\\040Stick rw - exfat /dev/sdf1 rw
";
        let labels = HashMap::new();
        let mounts = parse_mountinfo_and_fstab(mountinfo, fstab, "alice", &labels);
        let names: Vec<&str> = mounts.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["I", "II", "III", "IV", "V", "VI", "USB Stick"]);
        assert!(mounts.last().unwrap().removable);
        assert!(!mounts[0].removable);
    }

    #[test]
    fn parses_xdg_dirs_and_bookmarks_in_order() {
        let home = Path::new("/home/alice");
        let xdg = "\
XDG_DESKTOP_DIR=\"$HOME/Desktop\"
XDG_DOWNLOAD_DIR=\"$HOME/Downloads\"
XDG_DOCUMENTS_DIR=\"$HOME/Documents\"
XDG_PICTURES_DIR=\"$HOME/Pictures\"
";
        let dirs = parse_user_dirs(xdg, home);
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/home/alice/Documents"),
                PathBuf::from("/home/alice/Downloads"),
                PathBuf::from("/home/alice/Pictures"),
                PathBuf::from("/home/alice/Desktop"),
            ]
        );

        let bm = "file:///home/alice/Projects My%20Work\nfile:///mnt/III/Music\n";
        let parsed = parse_gtk_bookmarks(bm);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, PathBuf::from("/home/alice/Projects"));
        assert_eq!(parsed[0].1.as_deref(), Some("My%20Work"));
        assert_eq!(parsed[1].0, PathBuf::from("/mnt/III/Music"));
        assert_eq!(parsed[1].1, None);
    }
}
