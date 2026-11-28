//! What the Look cards read: the look file, wallpapers, faces, fonts, icon and cursor themes, and the parsers and folder scans behind them.

use super::*;

/// A candidate accent extracted from the wallpaper (`#rrggbb x y`, `x`/`y` in `0..=1`).
#[derive(Clone, Debug, PartialEq)]
pub struct AccentCandidate {
    pub hex: String,
    pub rgb: [u8; 3],
    pub x: f64,
    pub y: f64,
}

/// Extra state for `settings/appearance`: wallpaper tool presence, invalid `#rrggbb` attempts,
/// extracted accent candidates with cached 16:10 wallpaper preview, and recent logged accents.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LookData {
    pub wallpaper_bin: bool,
    pub bad_colors: HashSet<String>,
    pub accent_candidates: Vec<AccentCandidate>,
    pub accent_thumb: Option<Arc<DecodedImage>>,
    pub recent_accents: Vec<String>,
}

/// Discovered wallpaper files, tool presence, and decoded thumbnails for `settings/wallpaper`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WallpaperFile {
    pub name: String,
    pub path: String,
}

pub type ThumbList = Vec<Option<Arc<DecodedImage>>>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WallpapersData {
    pub has_bin: bool,
    pub files: Vec<WallpaperFile>,
    pub thumbs_s: ThumbList,
    pub thumbs_m: ThumbList,
}

/// Names discovered for `fonts`, `fonts-mono`, `icons`, or `cursors`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FoundNames(pub Vec<String>);

/// Reads `~/.local/bin/infynitae-wallpaper` and any refused `#rrggbb` markers under the look runtime
/// directory.
pub struct LookSource {
    pub home: PathBuf,
    pub bad_dir: PathBuf,
}

impl LookSource {
    pub fn system() -> LookSource {
        let home = crate::paths::home().unwrap_or_default();
        let bad_dir = crate::paths::runtime_or_cache_dir().join("infynitae/look-bad");
        LookSource { home, bad_dir }
    }
}

impl Source for LookSource {
    fn fetch(&mut self) -> Data {
        let opt_home = (!self.home.as_os_str().is_empty()).then_some(self.home.as_path());
        if let Some(raw) = render_value("look") {
            let mut out = LookData::default();
            let mut acc_lines = String::new();
            let mut thumb_path: Option<PathBuf> = None;
            for line in raw.lines() {
                let l = line.trim();
                if let Some(v) = l.strip_prefix("wallpaper_bin=") {
                    out.wallpaper_bin = matches!(v, "1" | "true" | "yes" | "on");
                } else if let Some(k) = l.strip_prefix("bad=") {
                    if !k.is_empty() {
                        out.bad_colors.insert(k.to_string());
                    }
                } else if let Some(cand) = l.strip_prefix("accent=") {
                    acc_lines.push_str(cand);
                    acc_lines.push('\n');
                } else if let Some(p) = l.strip_prefix("wp_thumb=") {
                    thumb_path = Some(crate::paths::expand_home_with(p.trim(), opt_home));
                } else if let Some(r) = l.strip_prefix("recent=") {
                    if let Some(hex) = normalize_hex(r.trim()) {
                        if !out.recent_accents.contains(&hex)
                            && out.recent_accents.len() < MAX_RECENT_LOG
                        {
                            out.recent_accents.push(hex);
                        }
                    }
                } else if matches!(l, "1" | "true") {
                    out.wallpaper_bin = true;
                }
            }
            if !acc_lines.is_empty() {
                out.accent_candidates = parse_accent_lines(&acc_lines);
            }
            if let Some(p) = thumb_path {
                let scale = icons::display_scale();
                out.accent_thumb = icons::cached_wallpaper_thumb(
                    &p,
                    icons::scaled_px(PICKER_PREVIEW_SIZE.0, scale),
                    icons::scaled_px(PICKER_PREVIEW_SIZE.1, scale),
                );
            }
            return Box::new(out);
        }
        let mut bad_colors = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(&self.bad_dir) {
            for e in entries.flatten() {
                if e.path().is_file() {
                    if let Some(name) = e.file_name().to_str() {
                        bad_colors.insert(name.to_string());
                    }
                }
            }
        }
        let wp_bin = opt_home.map(|h| h.join(".local/bin/infynitae-wallpaper"));
        let wallpaper_bin = wp_bin.as_ref().is_some_and(|p| p.is_file());
        let active_wp = opt_home.and_then(resolve_active_wallpaper);
        let mut accent_candidates = Vec::new();
        let mut accent_thumb = None;
        if let (Some(ref wp), Some(ref bin)) = (active_wp, wp_bin) {
            if wallpaper_bin {
                let scale = icons::display_scale();
                accent_thumb = icons::cached_wallpaper_thumb(
                    wp,
                    icons::scaled_px(PICKER_PREVIEW_SIZE.0, scale),
                    icons::scaled_px(PICKER_PREVIEW_SIZE.1, scale),
                );
                if let Ok(out) = Command::new(bin).arg("--accents").arg(wp).output() {
                    if out.status.success() {
                        accent_candidates =
                            parse_accent_lines(&String::from_utf8_lossy(&out.stdout));
                    }
                }
            }
        }
        let recent_accents = opt_home
            .and_then(|h| {
                std::fs::read_to_string(h.join(".local/state/infynitae/settings-changes")).ok()
            })
            .map(|raw| parse_recent_accents(&raw))
            .unwrap_or_default();
        Box::new(LookData {
            wallpaper_bin,
            bad_colors,
            accent_candidates,
            accent_thumb,
            recent_accents,
        })
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

pub(super) fn normalize_hex(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() != 7 || !t.starts_with('#') {
        return None;
    }
    let rgb = parse_hex(t)?;
    Some(rgb_to_hex(rgb))
}

pub(super) fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// Parses `#rrggbb x y` lines from `infynitae-wallpaper --accents` / `settings.sh wallpaper-colors accents`.
/// Malformed lines (missing coordinates, non-hex colours, out-of-range coordinates) are ignored.
pub fn parse_accent_lines(raw: &str) -> Vec<AccentCandidate> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let mut parts = line.split_whitespace();
        let Some(hex_raw) = parts.next() else {
            continue;
        };
        let (Some(xs), Some(ys), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Some(hex) = normalize_hex(hex_raw) else {
            continue;
        };
        let Some(rgb) = parse_hex(&hex) else {
            continue;
        };
        let (Ok(x), Ok(y)) = (xs.parse::<f64>(), ys.parse::<f64>()) else {
            continue;
        };
        if !x.is_finite()
            || !y.is_finite()
            || !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
        {
            continue;
        }
        out.push(AccentCandidate { hex, rgb, x, y });
        if out.len() >= MAX_ACCENT_CANDIDATES {
            break;
        }
    }
    out
}

/// Reads the last 8 distinct valid `#rrggbb` values logged for `ACCENT` (or `BORDER_ACTIVE` from
/// before `ACCENT` existed) in `settings-changes`, most recent first. Malformed lines and `-` markers
/// are skipped.
pub fn parse_recent_accents(log_text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in log_text.lines().rev() {
        let mut cols = line.split('\t');
        let (Some(_ts), Some(key), Some(old), None) =
            (cols.next(), cols.next(), cols.next(), cols.next())
        else {
            continue;
        };
        if !matches!(key, "ACCENT" | "BORDER_ACTIVE") {
            continue;
        }
        let Some(hex) = normalize_hex(old) else {
            continue;
        };
        if !out.contains(&hex) {
            out.push(hex);
            if out.len() >= MAX_RECENT_LOG {
                break;
            }
        }
    }
    out
}

pub(super) fn resolve_active_wallpaper(home: &Path) -> Option<PathBuf> {
    if home.as_os_str().is_empty() {
        return None;
    }
    let cfg_path = home.join(".config/infynitae/config");
    let raw = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|t| crate::cards::settings_rows::parse_config(&t).remove("WALLPAPER"))
        .unwrap_or_default();
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        let p = crate::paths::expand_home_with(trimmed, Some(home));
        return p.is_file().then_some(p);
    }
    let dirs = [
        home.join("Pictures/Wallpapers"),
        home.join(".local/share/wallpapers"),
    ];
    scan_wallpapers(&dirs)
        .into_iter()
        .next()
        .map(|f| PathBuf::from(f.path))
        .filter(|p| p.is_file())
}

/// Scans `~/Pictures/Wallpapers` and `~/.local/share/wallpapers` for images and checks whether
/// `infynitae-wallpaper` is available on the system.
pub struct WallpapersSource {
    pub home: PathBuf,
    pub path: String,
}

impl WallpapersSource {
    pub fn system() -> WallpapersSource {
        WallpapersSource {
            home: crate::paths::home().unwrap_or_default(),
            path: std::env::var("PATH").unwrap_or_default(),
        }
    }
}

impl Source for WallpapersSource {
    fn fetch(&mut self) -> Data {
        if let Some(raw) = render_value("wallpapers") {
            let mut has_bin = true;
            let mut files = Vec::new();
            for line in raw.lines() {
                let l = line.trim();
                if l.is_empty() {
                    continue;
                }
                if let Some(v) = l.strip_prefix("has_bin=") {
                    has_bin = matches!(v, "1" | "true" | "yes" | "on");
                    continue;
                }
                let name = Path::new(l)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(l)
                    .to_string();
                files.push(WallpaperFile {
                    name,
                    path: l.to_string(),
                });
            }
            let (thumbs_s, thumbs_m) = decode_wallpaper_thumbs(&files);
            return Box::new(WallpapersData {
                has_bin,
                files,
                thumbs_s,
                thumbs_m,
            });
        }
        let opt_home = (!self.home.as_os_str().is_empty()).then_some(self.home.as_path());
        let has_bin = has_wallpaper_bin(opt_home, &self.path);
        let files = if let Some(home) = opt_home {
            let dirs = [
                home.join("Pictures/Wallpapers"),
                home.join(".local/share/wallpapers"),
            ];
            scan_wallpapers(&dirs)
        } else {
            Vec::new()
        };
        let (thumbs_s, thumbs_m) = decode_wallpaper_thumbs(&files);
        Box::new(WallpapersData {
            has_bin,
            files,
            thumbs_s,
            thumbs_m,
        })
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

pub(super) fn decode_wallpaper_thumbs(files: &[WallpaperFile]) -> (ThumbList, ThumbList) {
    let scale = icons::display_scale();
    let sw = icons::scaled_px(THUMB_S.0, scale);
    let sh = icons::scaled_px(THUMB_S.1, scale);
    let mw = icons::scaled_px(THUMB_M.0, scale);
    let mh = icons::scaled_px(THUMB_M.1, scale);
    let shown = &files[..files.len().min(MAX_WALLPAPERS)];
    let thumbs_s = shown
        .iter()
        .map(|f| icons::cached_wallpaper_thumb(Path::new(&f.path), sw, sh))
        .collect();
    let thumbs_m = shown
        .iter()
        .map(|f| icons::cached_wallpaper_thumb(Path::new(&f.path), mw, mh))
        .collect();
    (thumbs_s, thumbs_m)
}

/// Current user picture (`~/.face`) and candidate pictures from `~/Pictures` and
/// `~/Pictures/Wallpapers`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceData {
    pub current: Option<Arc<DecodedImage>>,
    pub files: Vec<WallpaperFile>,
    pub thumbs: ThumbList,
}

/// Scans `~/.face` and `~/Pictures` / `~/Pictures/Wallpapers` off the UI thread, reusing
/// `icons::ThumbCache` via `icons::cached_wallpaper_thumb`.
pub struct FaceSource {
    pub home: PathBuf,
}

impl FaceSource {
    pub fn system() -> FaceSource {
        FaceSource {
            home: crate::paths::home().unwrap_or_default(),
        }
    }
}

impl Source for FaceSource {
    fn fetch(&mut self) -> Data {
        let opt_home = (!self.home.as_os_str().is_empty()).then_some(self.home.as_path());
        if let Some(raw) = render_value("face") {
            let mut current_path: Option<PathBuf> = None;
            let mut files = Vec::new();
            for line in raw.lines() {
                let l = line.trim();
                if l.is_empty() {
                    continue;
                }
                if let Some(p) = l.strip_prefix("current=") {
                    if !p.is_empty() {
                        current_path = Some(crate::paths::expand_home_with(p, opt_home));
                    }
                    continue;
                }
                let expanded = crate::paths::expand_home_with(l, opt_home)
                    .to_string_lossy()
                    .into_owned();
                let name = Path::new(&expanded)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(l)
                    .to_string();
                files.push(WallpaperFile {
                    name,
                    path: expanded,
                });
            }
            let row_px = icons::scaled_px(FACE_ROW_THUMB, icons::display_scale());
            let current = current_path
                .filter(|p| p.is_file())
                .and_then(|p| icons::cached_wallpaper_thumb(&p, row_px, row_px));
            let thumbs = decode_face_thumbs(&files);
            return Box::new(FaceData {
                current,
                files,
                thumbs,
            });
        }
        let Some(home) = opt_home else {
            return Box::new(FaceData::default());
        };
        let face_path = home.join(".face");
        let row_px = icons::scaled_px(FACE_ROW_THUMB, icons::display_scale());
        let current = face_path
            .is_file()
            .then(|| icons::cached_wallpaper_thumb(&face_path, row_px, row_px))
            .flatten();
        let dirs = [home.join("Pictures"), home.join("Pictures/Wallpapers")];
        let files = scan_wallpapers(&dirs);
        let thumbs = decode_face_thumbs(&files);
        Box::new(FaceData {
            current,
            files,
            thumbs,
        })
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

pub(super) fn decode_face_thumbs(files: &[WallpaperFile]) -> ThumbList {
    let px = icons::scaled_px(FACE_GRID_THUMB, icons::display_scale());
    files[..files.len().min(FACE_MAX_THUMBS)]
        .iter()
        .map(|f| icons::cached_wallpaper_thumb(Path::new(&f.path), px, px))
        .collect()
}

/// Shell command to crop `src` into `~/.face` (256x256 PNG), keep `~/.face.bak`, and log `FACE`.
pub fn face_command(src: &str) -> String {
    let q = sq(src);
    format!(
        "E=\"${{INFYNITAE_EDGE_APPS:-$HOME/.local/bin/infynitae-edge-apps}}\"; \
         [ -x \"$E\" ] || E=infynitae-edge-apps; \
         f=\"$HOME/.face\"; b=\"$HOME/.face.bak\"; \
         if [ -e \"$f\" ]; then old=.face.bak; else old=-; fi; \
         if \"$E\" --crop-face {q} \"$f.new\"; then \
         if [ -e \"$f\" ]; then cp -f \"$f\" \"$b\"; fi; \
         mv -f \"$f.new\" \"$f\" && \
         sh \"${{INFYNITAE_DIR:-$HOME/infynitae}}/live/settings.sh\" log FACE \"$old\" >/dev/null 2>&1; \
         else rm -f \"$f.new\"; false; fi"
    )
}

/// Lists installed font families from `fc-list`.
pub struct FontsSource {
    pub mono: bool,
}

impl FontsSource {
    pub fn all() -> FontsSource {
        FontsSource { mono: false }
    }

    pub fn mono() -> FontsSource {
        FontsSource { mono: true }
    }
}

impl Source for FontsSource {
    fn fetch(&mut self) -> Data {
        let (key, cmd, first): (&str, &str, &[&str]) = if self.mono {
            ("fonts-mono", "fc-list :spacing=100 family 2>/dev/null", &[])
        } else {
            (
                "fonts",
                "fc-list : family 2>/dev/null",
                &["sans-serif", "monospace"],
            )
        };
        let out = run_sh(key, cmd);
        Box::new(FoundNames(parse_fonts(&out, first)))
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

/// Lists installed icon themes, excluding `Infynitae-*` and cursor-only themes.
pub struct IconsSource {
    pub dirs: Vec<PathBuf>,
}

impl IconsSource {
    pub fn system() -> IconsSource {
        IconsSource {
            dirs: icon_roots(crate::paths::home().as_deref()),
        }
    }
}

impl Source for IconsSource {
    fn fetch(&mut self) -> Data {
        if let Some(raw) = render_value("icons") {
            return Box::new(FoundNames(parse_lines(&raw)));
        }
        Box::new(FoundNames(scan_icons(&self.dirs)))
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

/// Lists installed cursor themes (folders containing a `cursors/` subdirectory).
pub struct CursorsSource {
    pub dirs: Vec<PathBuf>,
}

impl CursorsSource {
    pub fn system() -> CursorsSource {
        CursorsSource {
            dirs: icon_roots(crate::paths::home().as_deref()),
        }
    }
}

impl Source for CursorsSource {
    fn fetch(&mut self) -> Data {
        if let Some(raw) = render_value("cursors") {
            return Box::new(FoundNames(parse_lines(&raw)));
        }
        Box::new(FoundNames(scan_cursors(&self.dirs)))
    }

    fn every(&self) -> Option<Duration> {
        None
    }
}

pub(super) fn icon_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/share/icons")];
    if let Some(h) = home.filter(|p| !p.as_os_str().is_empty()) {
        dirs.push(h.join(".local/share/icons"));
        dirs.push(h.join(".icons"));
    }
    dirs
}

pub(super) fn has_wallpaper_bin(home: Option<&Path>, path: &str) -> bool {
    if let Some(h) = home.filter(|p| !p.as_os_str().is_empty()) {
        if h.join(".local/bin/infynitae-wallpaper").is_file()
            || crate::paths::checkout_dir(h)
                .join("live/infynitae-wallpaper")
                .is_file()
        {
            return true;
        }
    }
    path.split(':')
        .filter(|d| !d.is_empty())
        .any(|d| Path::new(d).join("infynitae-wallpaper").is_file())
}

pub(super) fn parse_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parses `fc-list` family output: takes the first comma-separated name per line, deduplicates,
/// sorts, and puts `prefer_first` entries at the front when non-empty.
pub fn parse_fonts(out: &str, prefer_first: &[&str]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for line in out.lines() {
        let first = line.split(',').next().unwrap_or("").trim();
        if !first.is_empty() {
            set.insert(first.to_string());
        }
    }
    if set.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<String> = set.into_iter().collect();
    sorted.sort_by(|a, b| {
        a.to_ascii_lowercase()
            .cmp(&b.to_ascii_lowercase())
            .then_with(|| a.cmp(b))
    });
    let mut result = Vec::with_capacity(sorted.len() + prefer_first.len());
    for &p in prefer_first {
        result.push(p.to_string());
    }
    for name in sorted {
        if !prefer_first.contains(&name.as_str()) {
            result.push(name);
        }
    }
    result
}

pub(super) fn is_wallpaper_ext(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp"
    )
}

/// Scans wallpaper folders for `.png`, `.jpg`, `.jpeg`, `.webp` files, sorted by filename.
pub fn scan_wallpapers(dirs: &[PathBuf]) -> Vec<WallpaperFile> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for item in rd.flatten() {
            let path = item.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = item.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.starts_with('.') || !is_wallpaper_ext(&name) {
                continue;
            }
            let full = path.to_string_lossy().into_owned();
            if seen.insert(full.clone()) {
                entries.push(WallpaperFile { name, path: full });
            }
        }
    }
    entries.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    });
    entries
}

pub(super) fn index_has_icon_directories(index_text: &str) -> bool {
    let mut in_icon_theme = false;
    for raw in index_text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_icon_theme = line == "[Icon Theme]";
            continue;
        }
        if !in_icon_theme {
            continue;
        }
        let Some(rest) = line.strip_prefix("Directories=") else {
            continue;
        };
        // Require at least one non-empty directory entry other than "cursors".
        return rest
            .split(',')
            .map(str::trim)
            .any(|d| !d.is_empty() && !d.eq_ignore_ascii_case("cursors"));
    }
    false
}

pub(super) fn scan_theme_dirs<F>(dirs: &[PathBuf], keep: F) -> Vec<String>
where
    F: Fn(&Path) -> bool,
{
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in dirs {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        for item in rd.flatten() {
            let path = item.path();
            if !path.is_dir() {
                continue;
            }
            let file_name = item.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if name.starts_with('.') || name.starts_with("Infynitae-") {
                continue;
            }
            if keep(&path) && seen.insert(name.to_string()) {
                out.push(name.to_string());
            }
        }
    }
    sort_names(&mut out);
    out
}

/// Scans icon directories for themes with an `index.theme` holding `Directories=`, excluding
/// `Infynitae-*` generated folder themes and cursor-only themes.
pub fn scan_icons(dirs: &[PathBuf]) -> Vec<String> {
    scan_theme_dirs(dirs, |path| {
        std::fs::read_to_string(path.join("index.theme"))
            .is_ok_and(|index| index_has_icon_directories(&index))
    })
}

/// Scans icon directories for cursor themes (folders holding a `cursors/` subdirectory).
pub fn scan_cursors(dirs: &[PathBuf]) -> Vec<String> {
    scan_theme_dirs(dirs, |path| path.join("cursors").is_dir())
}

pub(super) fn sort_names(v: &mut [String]) {
    v.sort_by(|a, b| {
        a.to_ascii_lowercase()
            .cmp(&b.to_ascii_lowercase())
            .then_with(|| a.cmp(b))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings_look::tests::*;
    use cairo::{Format, ImageSurface};
    use std::fs;

    #[test]
    fn parses_fonts_from_usual_empty_and_broken_fc_list_output() {
        let usual = "Noto Sans Malayalam,Noto Sans Malayalam Light\n\
                     DejaVu Sans\n\
                     Noto Sans Malayalam\n\
                     monospace\n\
                     Adwaita Sans,Adwaita Sans Bold\n";
        assert_eq!(
            parse_fonts(usual, &["sans-serif", "monospace"]),
            vec![
                "sans-serif",
                "monospace",
                "Adwaita Sans",
                "DejaVu Sans",
                "Noto Sans Malayalam",
            ]
        );
        assert_eq!(
            parse_fonts("Liberation Mono,Bold\nDejaVu Sans Mono\n", &[]),
            vec!["DejaVu Sans Mono", "Liberation Mono"]
        );
        assert!(parse_fonts("", &["sans-serif", "monospace"]).is_empty());

        let broken = "\n   \n,AltOnly\n  ,  \nInter , Inter Bold\r\n";
        assert_eq!(parse_fonts(broken, &[]), vec!["Inter"]);
    }

    #[test]
    fn scans_icons_and_cursors_from_usual_empty_and_broken_folders() {
        let root = TempDir::new("icons");
        let sys = root.0.join("sys");
        let user = root.0.join("user");
        let empty = root.0.join("empty");
        fs::create_dir_all(&empty).unwrap();

        // Usual icon+cursor theme, Papirus-Dark, and user override duplicate
        fs::create_dir_all(sys.join("Adwaita/cursors")).unwrap();
        fs::write(
            sys.join("Adwaita/index.theme"),
            "[Icon Theme]\nName=Adwaita\nHidden=true\nDirectories=16x16/apps,scalable/places\n",
        )
        .unwrap();
        fs::create_dir_all(sys.join("Papirus-Dark")).unwrap();
        fs::write(
            sys.join("Papirus-Dark/index.theme"),
            "[Icon Theme]\r\nDirectories=16x16/places,24x24/apps\r\n",
        )
        .unwrap();
        fs::create_dir_all(user.join("Adwaita/cursors")).unwrap();
        fs::write(
            user.join("Adwaita/index.theme"),
            "[Icon Theme]\nDirectories=16x16/apps\n",
        )
        .unwrap();

        // Infynitae-* generated theme must not appear in Icons
        fs::create_dir_all(user.join("Infynitae-Papirus-Dark")).unwrap();
        fs::write(
            user.join("Infynitae-Papirus-Dark/index.theme"),
            "[Icon Theme]\nDirectories=16x16/places\n",
        )
        .unwrap();

        // Cursor-only theme: no icon Directories= (or only Directories=cursors)
        fs::create_dir_all(sys.join("Bibata-Cursor/cursors")).unwrap();
        fs::write(
            sys.join("Bibata-Cursor/index.theme"),
            "[Icon Theme]\nName=Bibata\nInherits=hicolor\n",
        )
        .unwrap();
        fs::create_dir_all(sys.join("CursorsOnlyDir/cursors")).unwrap();
        fs::write(
            sys.join("CursorsOnlyDir/index.theme"),
            "[Icon Theme]\nDirectories=cursors\n",
        )
        .unwrap();

        // Broken entries: invalid UTF-8 index.theme, missing Directories, "cursors" as a regular file
        fs::create_dir_all(sys.join("BrokenTheme")).unwrap();
        fs::write(sys.join("BrokenTheme/index.theme"), [0xff, 0xfe, 0x00]).unwrap();
        fs::create_dir_all(sys.join("EmptyDirs")).unwrap();
        fs::write(
            sys.join("EmptyDirs/index.theme"),
            "[Icon Theme]\nDirectories= , \n",
        )
        .unwrap();
        fs::create_dir_all(sys.join("NotACursorDir")).unwrap();
        fs::write(sys.join("NotACursorDir/cursors"), b"file").unwrap();
        fs::create_dir_all(sys.join(".hidden/cursors")).unwrap();

        let roots = vec![
            sys.clone(),
            user.clone(),
            empty.clone(),
            root.0.join("missing"),
        ];
        assert_eq!(scan_icons(&roots), vec!["Adwaita", "Papirus-Dark"]);
        assert_eq!(
            scan_cursors(&roots),
            vec!["Adwaita", "Bibata-Cursor", "CursorsOnlyDir"]
        );
        assert!(scan_icons(std::slice::from_ref(&empty)).is_empty());
        assert!(scan_cursors(&[empty]).is_empty());
    }

    #[test]
    fn scans_wallpapers_sorted_decodes_up_to_24_and_handles_empty_and_broken() {
        let root = TempDir::new("wallpapers");
        let w1 = root.0.join("Pictures/Wallpapers");
        let w2 = root.0.join(".local/share/wallpapers");
        fs::create_dir_all(&w1).unwrap();
        fs::create_dir_all(&w2).unwrap();

        // Empty folder
        let mut empty_src = WallpapersSource {
            home: root.0.clone(),
            path: String::new(),
        };
        let empty_data = empty_src.fetch().downcast::<WallpapersData>().unwrap();
        assert!(empty_data.files.is_empty());
        assert!(empty_data.thumbs_s.is_empty());
        assert!(scan_wallpapers(&[w1.clone(), root.0.join("missing")]).is_empty());

        // Write 26 valid PNGs + 1 broken image + non-image files
        let make_png = |path: &Path| {
            let s = ImageSurface::create(Format::ARgb32, 32, 18).unwrap();
            s.write_to_png(&mut fs::File::create(path).unwrap())
                .unwrap();
        };
        for n in 1..=25 {
            make_png(&w1.join(format!("img_{n:02}.png")));
        }
        make_png(&w2.join("a_top.png"));
        fs::write(w2.join("broken.webp"), b"not an image").unwrap();
        fs::write(w2.join("notes.txt"), b"").unwrap();
        fs::write(w2.join(".hidden.png"), b"").unwrap();
        fs::create_dir_all(w2.join("dir.png")).unwrap();

        let found = scan_wallpapers(&[w1, w2]);
        assert_eq!(found.len(), 27);
        assert_eq!(found[0].name, "a_top.png");
        assert_eq!(found[1].name, "broken.webp");
        assert_eq!(found[26].name, "img_25.png");

        let data = empty_src.fetch().downcast::<WallpapersData>().unwrap();
        assert_eq!(data.files.len(), 27);
        // Only the first 24 shown items are decoded into S/M thumbnails.
        assert_eq!(data.thumbs_s.len(), 24);
        assert_eq!(data.thumbs_m.len(), 24);
        assert!(data.thumbs_s[0].is_some());
        // Broken file stays None without failing valid neighbors.
        assert!(data.thumbs_s[1].is_none());
        assert!(data.thumbs_m[1].is_none());
        assert!(data.thumbs_s[2].is_some());
    }

    #[test]
    fn look_source_reads_wallpaper_bin_and_bad_color_markers() {
        let root = TempDir::new("look-src");
        let home = root.0.join("home");
        let bad = root.0.join("bad");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&bad).unwrap();

        let mut src = LookSource {
            home: home.clone(),
            bad_dir: bad.clone(),
        };
        let empty = src.fetch().downcast::<LookData>().unwrap();
        assert!(!empty.wallpaper_bin);
        assert!(empty.bad_colors.is_empty());

        fs::create_dir_all(home.join(".local/bin")).unwrap();
        fs::write(home.join(".local/bin/infynitae-wallpaper"), b"").unwrap();
        fs::write(bad.join("ACCENT"), b"").unwrap();
        // Subdirectories inside bad_dir must be ignored
        fs::create_dir_all(bad.join("dir_ignored")).unwrap();

        let full = src.fetch().downcast::<LookData>().unwrap();
        assert!(full.wallpaper_bin);
        assert_eq!(full.bad_colors, HashSet::from(["ACCENT".to_string()]));
    }

    #[test]
    fn parses_accent_candidates_and_ignores_malformed() {
        let raw = "\n\
                   #7AA2F7 0.125 0.875\n\
                   malformed\n\
                   #ff00aa 0 1\n\
                   #gggggg 0.5 0.5\n\
                   #fff 0.5 0.5\n\
                   #112233 -0.01 0.5\n\
                   #112233 0.5 1.01\n\
                   #112233 NaN 0.5\n\
                   #112233 0.5 0.5 extra\n\
                   #223344 0.25\n\
                   #00aabb 0.500 0.250\n\
                   #334455 0.750 0.750\n\
                   #445566 0.100 0.200\n\
                   #556677 0.900 0.900\n\
                   #667788 0.400 0.400\n";
        let cands = parse_accent_lines(raw);
        assert_eq!(cands.len(), MAX_ACCENT_CANDIDATES);
        assert_eq!(cands[0].hex, "#7aa2f7");
        assert_eq!(cands[0].rgb, [0x7a, 0xa2, 0xf7]);
        assert!((cands[0].x - 0.125).abs() < 1e-6);
        assert!((cands[0].y - 0.875).abs() < 1e-6);
        assert_eq!(cands[1].hex, "#ff00aa");
        assert_eq!(cands[2].hex, "#00aabb");
        assert_eq!(cands[5].hex, "#556677");

        assert!(parse_accent_lines("").is_empty());
        assert!(parse_accent_lines("  \n#nothex 0.5 0.5\n").is_empty());
    }

    #[test]
    fn reads_recent_accents_from_temporary_log() {
        let log = "\
100\tFONT_NAME\tSans\n\
101\tACCENT\t-\n\
102\tBORDER_ACTIVE\t#111111\n\
103\tACCENT\tnotacolor\n\
104\tBORDER_ACTIVE\t#222222\n\
105\tBORDER_ACTIVE\t#333333\n\
106\tACCENT\t#444444\n\
107\tACCENT\t#555555\n\
108\tBORDER_ACTIVE\t#666666\n\
109\tACCENT\t#777777\n\
110\tACCENT\t#222222\tEXTRA\n\
111\tBORDER_ACTIVE\t#888888\n\
112\tACCENT\t#999999\n\
113\tACCENT\t#888888\n";
        let parsed = parse_recent_accents(log);
        assert_eq!(
            parsed,
            vec![
                "#888888", "#999999", "#777777", "#666666", "#555555", "#444444", "#333333",
                "#222222"
            ]
        );

        let root = TempDir::new("recent-log");
        let state = root.0.join(".local/state/infynitae");
        fs::create_dir_all(&state).unwrap();
        fs::write(state.join("settings-changes"), log).unwrap();

        let mut src = LookSource {
            home: root.0.clone(),
            bad_dir: root.0.join("bad"),
        };
        let look = src.fetch().downcast::<LookData>().unwrap();
        assert_eq!(look.recent_accents, parsed);

        let mut c = init_appearance(&[("ACCENT", "#ABCDEF")], true, &[]);
        c.look = Some(*look);
        let colors = c.recent_colors();
        assert_eq!(colors.len(), 9);
        assert_eq!(colors[0], "#abcdef");
        assert_eq!(colors[1], "#888888");
        assert_eq!(colors[8], "#222222");
    }

    #[test]
    fn face_command_backs_up_existing_face_and_logs_undo_entry() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new("face-cmd");
        let home = &dir.0;
        let tricky = home.join("Pictures/it's \"me\" (1).png");
        write_png_rgba(&tricky, 100, 60, [200, 30, 50, 255]);

        let bin = dir.0.join("fake-edge");
        let log_file = dir.0.join("log.txt");
        fs::write(
            &bin,
            format!(
                "#!/bin/sh\nif [ \"$1\" != \"--crop-face\" ]; then exit 2; fi\nprintf 'crop:%s\\n' \"$2\" >> '{}'\ncp \"$2\" \"$3\"\n",
                log_file.display()
            ),
        )
        .unwrap();
        let mut perm = fs::metadata(&bin).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&bin, perm).unwrap();

        let sh_path = home.join("infynitae/live/settings.sh");
        fs::create_dir_all(sh_path.parent().unwrap()).unwrap();
        fs::write(
            &sh_path,
            format!(
                "#!/bin/sh\nprintf 'sh:%s %s %s\\n' \"$1\" \"$2\" \"$3\" >> '{}'\n",
                log_file.display()
            ),
        )
        .unwrap();

        let run_face = |src: &Path| {
            let cmd = face_command(&src.to_string_lossy());
            std::process::Command::new("sh")
                .args(["-c", &cmd])
                .env("HOME", home)
                .env_remove("INFYNITAE_DIR")
                .env("INFYNITAE_EDGE_APPS", &bin)
                .status()
                .unwrap()
        };

        // First set when ~/.face does not exist -> old is "-".
        assert!(run_face(&tricky).success());
        assert!(home.join(".face").is_file());
        assert!(!home.join(".face.bak").exists());
        let first_bytes = fs::read(home.join(".face")).unwrap();
        assert_eq!(first_bytes, fs::read(&tricky).unwrap());

        // Second set when ~/.face exists -> old backed up to ~/.face.bak and old is ".face.bak".
        let second = home.join("Pictures/second.png");
        write_png_rgba(&second, 40, 40, [20, 200, 40, 255]);
        assert!(run_face(&second).success());
        assert_eq!(fs::read(home.join(".face.bak")).unwrap(), first_bytes);
        assert_eq!(
            fs::read(home.join(".face")).unwrap(),
            fs::read(&second).unwrap()
        );

        let logged = fs::read_to_string(&log_file).unwrap();
        assert_eq!(
            logged,
            format!(
                "crop:{}\nsh:log FACE -\ncrop:{}\nsh:log FACE .face.bak\n",
                tricky.display(),
                second.display()
            )
        );
    }
}
