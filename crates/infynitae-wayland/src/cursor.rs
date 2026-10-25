//! The pointer drawn by the compositor itself, for a backend that owns the display (the console
//! backend, owner decision T8 2026-10-22): nothing else draws it there. The client's cursor
//! surface when it set one (`wl_pointer.set_cursor`), a built-in arrow when it asked for a named
//! cursor or none was set, nothing when it hid the pointer. A backend inside a host window leaves
//! the pointer to the host.
//!
//! The geometry of what is drawn (the client's surface at its hotspot, the arrow's rects) is
//! `convert`'s (INV-COORD-4); this module keeps what the client asked for.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Id;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::input::pointer::CursorImageStatus;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::Transform;

use crate::WaylandState;

/// The built-in arrow's height in rows (`convert::arrow_rects`).
pub(crate) const ARROW_ROWS: i32 = 13;

/// The ids of the arrow's rects (stable across frames, so an unmoved arrow is not damage).
#[derive(Debug)]
pub(crate) struct Arrow {
    pub(crate) outline: Vec<Id>,
    pub(crate) fill: Vec<Id>,
}

impl Default for Arrow {
    fn default() -> Self {
        Arrow {
            outline: (0..ARROW_ROWS).map(|_| Id::new()).collect(),
            fill: (0..ARROW_ROWS).map(|_| Id::new()).collect(),
        }
    }
}

/// An image decoded from an Xcursor file, cached as a Smithay memory render buffer.
#[derive(Debug, Clone)]
pub(crate) struct LoadedImage {
    pub(crate) buffer: MemoryRenderBuffer,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) xhot: i32,
    pub(crate) yhot: i32,
}

/// Select the image closest to `target_size` in nominal or actual size.
///
/// Ties take the first matching frame in file order (frame 0).
pub(crate) fn best_cursor_image(
    images: &[xcursor::parser::Image],
    target_size: u32,
) -> Option<&xcursor::parser::Image> {
    images.iter().min_by_key(|img| {
        let s = if img.size > 0 { img.size } else { img.width };
        (s as i64 - target_size as i64).abs()
    })
}

/// Common alias names for X11 / CSS cursor shapes.
pub(crate) fn cursor_aliases(name: &str) -> &'static [&'static str] {
    match name {
        "default" => &["left_ptr", "arrow"],
        "pointer" => &["hand2", "hand1", "pointing_hand"],
        "text" => &["xterm", "ibeam"],
        "crosshair" => &["cross"],
        "move" => &["fleur"],
        "not-allowed" => &["crossed_circle", "forbidden", "circle"],
        "grab" => &["openhand", "hand1"],
        "grabbing" => &["closedhand"],
        "e-resize" => &["right_side"],
        "w-resize" => &["left_side"],
        "n-resize" => &["top_side"],
        "s-resize" => &["bottom_side"],
        "ne-resize" => &["top_right_corner"],
        "nw-resize" => &["top_left_corner"],
        "se-resize" => &["bottom_right_corner"],
        "sw-resize" => &["bottom_left_corner"],
        "ew-resize" => &["h_double_arrow", "sb_h_double_arrow", "split_h"],
        "ns-resize" => &["v_double_arrow", "sb_v_double_arrow", "split_v"],
        "nesw-resize" => &["fd_double_arrow", "size_bdiag"],
        "nwse-resize" => &["bd_double_arrow", "size_fdiag"],
        "col-resize" => &["split_h", "h_double_arrow", "sb_h_double_arrow"],
        "row-resize" => &["split_v", "v_double_arrow", "sb_v_double_arrow"],
        "all-scroll" => &["fleur"],
        "wait" => &["watch"],
        "progress" => &["left_ptr_watch", "half-busy", "watch"],
        "help" => &["question_arrow", "whats_this", "help"],
        _ => &[],
    }
}

/// Global theme search paths per XDG / Xcursor conventions, respecting custom home and XCURSOR_PATH.
pub(crate) fn theme_search_paths_with(
    home: Option<&Path>,
    xcursor_path: Option<&str>,
) -> Vec<PathBuf> {
    if let Some(xc) = xcursor_path {
        return xc
            .split(':')
            .filter(|s| !s.is_empty())
            .map(|entry| {
                if let Some(stripped) = entry.strip_prefix("~/")
                    && let Some(h) = home
                {
                    return h.join(stripped);
                }
                PathBuf::from(entry)
            })
            .collect();
    }

    if let Ok(xc) = std::env::var("XCURSOR_PATH") {
        return xc
            .split(':')
            .filter(|s| !s.is_empty())
            .map(|entry| {
                if let Some(stripped) = entry.strip_prefix("~/")
                    && let Some(h) = home
                {
                    return h.join(stripped);
                }
                PathBuf::from(entry)
            })
            .collect();
    }

    let mut paths = Vec::new();

    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        if !data_home.is_empty() {
            paths.push(PathBuf::from(data_home).join("icons"));
        }
    } else if let Some(h) = home {
        paths.push(h.join(".local/share/icons"));
    }

    if let Some(h) = home {
        paths.push(h.join(".icons"));
    }

    if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in data_dirs.split(':').filter(|s| !s.is_empty()) {
            paths.push(PathBuf::from(dir).join("icons"));
        }
    } else {
        paths.push(PathBuf::from("/usr/local/share/icons"));
        paths.push(PathBuf::from("/usr/share/icons"));
    }

    paths.push(PathBuf::from("/usr/share/pixmaps"));

    if let Some(h) = home {
        paths.push(h.join(".cursors"));
    }

    paths.push(PathBuf::from("/usr/share/cursors/xorg-x11"));

    paths
}

/// Global theme search paths per XDG / Xcursor conventions.
#[allow(dead_code)]
pub fn theme_search_paths() -> Vec<PathBuf> {
    let home = std::env::var("HOME").ok().map(PathBuf::from);
    theme_search_paths_with(home.as_deref(), None)
}

/// Parse `Inherits=` from an `index.theme` file if present.
fn theme_inherits(theme_dir: &Path) -> Option<String> {
    let index_file = theme_dir.join("index.theme");
    let content = std::fs::read_to_string(index_file).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Inherits") {
            let rest = rest.trim_start();
            if let Some(val) = rest.strip_prefix('=') {
                let val = val.trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// Whether a cursor theme directory exists in `search_paths`.
pub(crate) fn theme_exists_in(name: &str, search_paths: &[PathBuf]) -> bool {
    search_paths.iter().any(|p| p.join(name).is_dir())
}

/// Whether a cursor theme directory exists in any search path.
#[allow(dead_code)]
pub fn theme_exists(name: &str) -> bool {
    theme_exists_in(name, &theme_search_paths())
}

/// Manages loading, caching, and fallback resolution of Xcursor themes.
#[derive(Debug)]
pub(crate) struct CursorThemeManager {
    theme_name: String,
    size: u32,
    search_paths: Vec<PathBuf>,
    cache: HashMap<(String, u32), Option<LoadedImage>>,
}

impl CursorThemeManager {
    pub(crate) fn with_search_paths(
        theme_name: &str,
        size: u32,
        search_paths: Vec<PathBuf>,
    ) -> Self {
        CursorThemeManager {
            theme_name: theme_name.to_string(),
            size,
            search_paths,
            cache: HashMap::new(),
        }
    }

    pub(crate) fn size(&self) -> u32 {
        self.size
    }

    pub(crate) fn theme_name(&self) -> &str {
        &self.theme_name
    }

    pub(crate) fn get_or_load(&mut self, name: &str, target_size: u32) -> Option<LoadedImage> {
        let key = (name.to_string(), target_size);
        if let Some(cached) = self.cache.get(&key) {
            return cached.clone();
        }

        let loaded = self.load_uncached(name, target_size);
        self.cache.insert(key, loaded.clone());
        loaded
    }

    fn load_uncached(&self, name: &str, target_size: u32) -> Option<LoadedImage> {
        // 1. Try requested name
        if let Some(img) = self.try_load_name(name, target_size) {
            return Some(img);
        }

        // 2. Try aliases for requested name
        for &alias in cursor_aliases(name) {
            if let Some(img) = self.try_load_name(alias, target_size) {
                return Some(img);
            }
        }

        // 3. Fall back to "default", then "left_ptr"
        if name != "default" && name != "left_ptr" {
            if let Some(img) = self.try_load_name("default", target_size) {
                return Some(img);
            }
            if let Some(img) = self.try_load_name("left_ptr", target_size) {
                return Some(img);
            }
        }

        None
    }

    fn try_load_name(&self, icon_name: &str, target_size: u32) -> Option<LoadedImage> {
        let mut visited = HashSet::new();

        // 1. Check in our search paths for theme_name
        if let Some(path) = self.find_icon_in_theme(&self.theme_name, icon_name, &mut visited)
            && let Some(img) = Self::load_file(&path, target_size)
        {
            return Some(img);
        }

        // 2. Try default theme fallback if primary is different
        if self.theme_name != "default"
            && let Some(path) = self.find_icon_in_theme("default", icon_name, &mut visited)
            && let Some(img) = Self::load_file(&path, target_size)
        {
            return Some(img);
        }

        None
    }

    fn find_icon_in_theme(
        &self,
        theme: &str,
        icon_name: &str,
        visited: &mut HashSet<String>,
    ) -> Option<PathBuf> {
        if !visited.insert(theme.to_string()) {
            return None;
        }

        let mut inherits = None;
        for search_dir in &self.search_paths {
            let theme_dir = search_dir.join(theme);
            if theme_dir.is_dir() {
                let icon_path = theme_dir.join("cursors").join(icon_name);
                if icon_path.is_file() {
                    return Some(icon_path);
                }
                if inherits.is_none() {
                    inherits = theme_inherits(&theme_dir);
                }
            }
        }

        if let Some(parent) = inherits {
            if let Some(path) = self.find_icon_in_theme(&parent, icon_name, visited) {
                return Some(path);
            }
        } else if theme != "default"
            && let Some(path) = self.find_icon_in_theme("default", icon_name, visited)
        {
            return Some(path);
        }

        None
    }

    fn load_file(path: &Path, target_size: u32) -> Option<LoadedImage> {
        let data = std::fs::read(path).ok()?;
        let images = xcursor::parser::parse_xcursor(&data)?;
        let best = best_cursor_image(&images, target_size)?;

        Some(LoadedImage {
            buffer: MemoryRenderBuffer::from_slice(
                &best.pixels_rgba,
                Fourcc::Argb8888,
                (best.width as i32, best.height as i32),
                1,
                Transform::Normal,
                None,
            ),
            width: best.width as i32,
            height: best.height as i32,
            xhot: best.xhot as i32,
            yhot: best.yhot as i32,
        })
    }
}

impl WaylandState {
    /// The client asked for a cursor (`SeatHandler::cursor_image`).
    pub(crate) fn set_cursor_status(&mut self, status: CursorImageStatus) {
        self.cursor = status;
    }

    /// Sets a custom HOME directory path for reading config files and themes.
    pub fn set_config_home(&mut self, home: PathBuf) {
        self.config_home = Some(home);
    }

    /// Sets a custom XCURSOR_PATH for theme lookups.
    pub fn set_xcursor_path(&mut self, path: String) {
        self.xcursor_path = Some(path);
    }

    /// Update or reload the cursor theme.
    pub fn set_cursor_theme(&mut self, theme_name: &str, cursor_size: u32) {
        let home = self
            .config_home
            .as_ref()
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::var("HOME").ok().map(PathBuf::from));
        let xcursor_path = self
            .xcursor_path
            .clone()
            .or_else(|| std::env::var("XCURSOR_PATH").ok());
        let paths = theme_search_paths_with(home.as_deref(), xcursor_path.as_deref());
        self.set_cursor_theme_with_search_paths(theme_name, cursor_size, paths);
    }

    /// Update cursor theme using custom search paths (for tests or custom loaders).
    pub fn set_cursor_theme_with_search_paths(
        &mut self,
        theme_name: &str,
        cursor_size: u32,
        search_paths: Vec<PathBuf>,
    ) {
        let mut guard = self.cursor_theme.lock().unwrap_or_else(|e| e.into_inner());
        let exists = theme_exists_in(theme_name, &search_paths);
        let default_exists = theme_exists_in("default", &search_paths);
        *guard = Some(CursorThemeManager::with_search_paths(
            theme_name,
            cursor_size,
            search_paths,
        ));
        if exists {
            eprintln!("[infynitae] Loaded cursor theme '{theme_name}' (size {cursor_size})");
        } else if theme_name != "default" && default_exists {
            eprintln!(
                "[infynitae] Cursor theme '{theme_name}' not found, falling back to 'default'"
            );
        } else {
            eprintln!(
                "[infynitae] Cursor theme '{theme_name}' not found, falling back to built-in triangle"
            );
        }
    }

    /// The name of the configured cursor theme, if loaded.
    pub fn cursor_theme_name(&self) -> Option<String> {
        self.cursor_theme
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|m| m.theme_name().to_string())
    }

    /// The nominal cursor size in logical pixels (default 24).
    pub fn cursor_size(&self) -> u32 {
        self.cursor_theme
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|m| m.size())
            .unwrap_or(24)
    }

    pub(crate) fn cursor_image(&self, name: &str, target_size: u32) -> Option<LoadedImage> {
        self.cursor_theme
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .and_then(|m| m.get_or_load(name, target_size))
    }

    /// The frame callbacks of the cursor surface complete with the output's frame (an animated
    /// cursor advances).
    pub(crate) fn complete_cursor_frames(
        &self,
        output: &smithay::output::Output,
        now: std::time::Duration,
    ) {
        if let CursorImageStatus::Surface(surface) = &self.cursor
            && surface.is_alive()
        {
            smithay::desktop::utils::send_frames_surface_tree(
                surface,
                output,
                now,
                None,
                |_, _| Some(output.clone()),
            );
        }
    }
}

#[doc(hidden)]
pub type TestCursorImageDesc<'a> = (u32, u32, u32, (u32, u32), &'a [u8]);

#[doc(hidden)]
pub fn make_xcursor_file(images: &[TestCursorImageDesc<'_>]) -> Vec<u8> {
    let ntoc = images.len() as u32;
    let header_size: u32 = 16;
    let toc_size = ntoc * 12;
    let mut out = Vec::new();

    out.extend_from_slice(b"Xcur");
    out.extend_from_slice(&header_size.to_le_bytes());
    out.extend_from_slice(&0x0001_0000u32.to_le_bytes());
    out.extend_from_slice(&ntoc.to_le_bytes());

    let mut chunk_pos = header_size + toc_size;
    let mut chunk_positions = Vec::new();
    for img in images {
        chunk_positions.push(chunk_pos);
        let chunk_size = 36 + img.1 * img.2 * 4;
        chunk_pos += chunk_size;
    }

    for (i, img) in images.iter().enumerate() {
        out.extend_from_slice(&0xfffd_0002u32.to_le_bytes());
        out.extend_from_slice(&img.0.to_le_bytes());
        out.extend_from_slice(&chunk_positions[i].to_le_bytes());
    }

    for img in images {
        let chunk_header_size: u32 = 36;
        out.extend_from_slice(&chunk_header_size.to_le_bytes());
        out.extend_from_slice(&0xfffd_0002u32.to_le_bytes());
        out.extend_from_slice(&img.0.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&img.1.to_le_bytes());
        out.extend_from_slice(&img.2.to_le_bytes());
        out.extend_from_slice(&img.3.0.to_le_bytes());
        out.extend_from_slice(&img.3.1.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(img.4);
    }

    out
}

#[doc(hidden)]
pub struct TestTempDir(pub PathBuf);

impl TestTempDir {
    pub fn new(prefix: &str) -> Self {
        static CNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = CNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "infynitae_test_{prefix}_{}_{}",
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test temp dir");
        TestTempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_choice_picks_nearest_image_and_keeps_hotspot() {
        let img24_pix = vec![0x11; 24 * 24 * 4];
        let img48_pix = vec![0x22; 48 * 48 * 4];
        let data = make_xcursor_file(&[
            (24, 24, 24, (2, 3), &img24_pix),
            (48, 48, 48, (4, 6), &img48_pix),
        ]);
        let parsed = xcursor::parser::parse_xcursor(&data).expect("valid xcursor");
        assert_eq!(parsed.len(), 2);

        // Target 24 picks 24x24
        let pick24 = best_cursor_image(&parsed, 24).expect("image");
        assert_eq!(pick24.size, 24);
        assert_eq!(pick24.width, 24);
        assert_eq!(pick24.xhot, 2);
        assert_eq!(pick24.yhot, 3);

        // Target 48 (e.g. 24 on a 2x scale) picks 48x48
        let pick48 = best_cursor_image(&parsed, 48).expect("image");
        assert_eq!(pick48.size, 48);
        assert_eq!(pick48.width, 48);
        assert_eq!(pick48.xhot, 4);
        assert_eq!(pick48.yhot, 6);

        // Target 40 picks 48 (distance 8 vs 16)
        let pick40 = best_cursor_image(&parsed, 40).expect("image");
        assert_eq!(pick40.size, 48);
    }

    #[test]
    fn theme_lookup_and_aliases_with_temp_dir() {
        let tmp = TestTempDir::new("lookup");
        let theme_dir = tmp.path().join("my_test_theme");
        let cursors_dir = theme_dir.join("cursors");
        std::fs::create_dir_all(&cursors_dir).expect("mkdir");

        std::fs::write(
            theme_dir.join("index.theme"),
            "[Icon Theme]\nName=my_test_theme\n",
        )
        .expect("write index.theme");

        // Provide "left_ptr" in the theme
        let pix = vec![0xAA; 16 * 16 * 4];
        let cur_bytes = make_xcursor_file(&[(16, 16, 16, (1, 1), &pix)]);
        std::fs::write(cursors_dir.join("left_ptr"), cur_bytes).expect("write left_ptr");

        let search_paths = vec![tmp.path().to_path_buf()];
        assert!(theme_exists_in("my_test_theme", &search_paths));
        assert!(!theme_exists_in("nonexistent_theme", &search_paths));

        let mut mgr = CursorThemeManager::with_search_paths("my_test_theme", 16, search_paths);

        // "default" is not present directly, but its alias "left_ptr" is!
        let loaded = mgr.get_or_load("default", 16).expect("loaded via alias");
        assert_eq!(loaded.width, 16);
        assert_eq!(loaded.height, 16);
        assert_eq!(loaded.xhot, 1);
        assert_eq!(loaded.yhot, 1);

        // Cache must have stored it
        let cached = mgr.get_or_load("default", 16).expect("cached");
        assert_eq!(cached.width, 16);

        // An unknown icon with no aliases falls back to "default"/"left_ptr"
        let unknown = mgr
            .get_or_load("some_unknown_icon", 16)
            .expect("fallback to left_ptr");
        assert_eq!(unknown.width, 16);
    }

    #[test]
    fn theme_inheritance_traversal() {
        let tmp = TestTempDir::new("inherits");

        // Parent theme has "pointer" (hand2)
        let parent_dir = tmp.path().join("parent_theme");
        let parent_cursors = parent_dir.join("cursors");
        std::fs::create_dir_all(&parent_cursors).expect("mkdir");
        std::fs::write(
            parent_dir.join("index.theme"),
            "[Icon Theme]\nName=parent_theme\n",
        )
        .expect("write");
        let pix = vec![0xBB; 24 * 24 * 4];
        std::fs::write(
            parent_cursors.join("hand2"),
            make_xcursor_file(&[(24, 24, 24, (3, 4), &pix)]),
        )
        .expect("write hand2");

        // Child theme inherits parent_theme
        let child_dir = tmp.path().join("child_theme");
        let child_cursors = child_dir.join("cursors");
        std::fs::create_dir_all(&child_cursors).expect("mkdir");
        std::fs::write(
            child_dir.join("index.theme"),
            "[Icon Theme]\nName=child_theme\nInherits=parent_theme\n",
        )
        .expect("write");

        let search_paths = vec![tmp.path().to_path_buf()];
        let mut mgr = CursorThemeManager::with_search_paths("child_theme", 24, search_paths);
        let loaded = mgr.get_or_load("pointer", 24).expect("inherited");
        assert_eq!(loaded.width, 24);
        assert_eq!(loaded.xhot, 3);
        assert_eq!(loaded.yhot, 4);
    }

    #[test]
    fn fallback_to_none_when_theme_does_not_exist() {
        let tmp = TestTempDir::new("no_theme");
        let search_paths = vec![tmp.path().to_path_buf()];

        assert!(!theme_exists_in("nonexistent", &search_paths));
        let mut mgr = CursorThemeManager::with_search_paths("nonexistent", 24, search_paths);
        assert!(mgr.get_or_load("default", 24).is_none());
    }
}
