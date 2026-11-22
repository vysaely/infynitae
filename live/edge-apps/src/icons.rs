//! App icons: an icon name from a desktop entry found in the icon themes (the user's GTK theme
//! first, then hicolor and the common ones), loaded as PNG, SVG (librsvg) or any other image
//! (gdk-pixbuf), both loaded only when first needed so the edge runs without them; cached per
//! name and size. Ported from live/edge-rs/src/modules/apps.rs.

use crate::card::Cx;
use crate::draw::{self, Text};
use cairo::Context;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{c_char, c_double, c_int, c_void, CString};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// The icon theme the user chose for GTK apps (`gtk-icon-theme-name` in settings.ini).
fn user_icon_theme() -> Option<String> {
    let cfg = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::paths::home().map(|h| h.join(".config")))?;
    for f in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
        let Ok(text) = std::fs::read_to_string(cfg.join(f)) else {
            continue;
        };
        for l in text.lines() {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim() == "gtk-icon-theme-name" {
                    let v = v.trim().trim_matches('"').trim();
                    if !v.is_empty() && !v.contains('/') && v != ".." {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}

fn icon_base_dirs() -> Vec<PathBuf> {
    let mut base_dirs: Vec<PathBuf> = Vec::new();
    if let Some(dh) = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
    {
        base_dirs.push(PathBuf::from(dh).join("icons"));
    } else if let Some(hp) = crate::paths::home() {
        base_dirs.push(hp.join(".local/share/icons"));
        base_dirs.push(hp.join(".icons"));
        base_dirs.push(hp.join(".local/share/flatpak/exports/share/icons"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    for part in data_dirs.split(':').filter(|p| !p.is_empty()) {
        base_dirs.push(PathBuf::from(part).join("icons"));
    }
    base_dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
    base_dirs
}

/// Reads `Inherits=` from `<base>/<theme>/index.theme` in `[Icon Theme]`.
fn theme_inherits(base_dirs: &[PathBuf], theme: &str) -> Vec<String> {
    for base in base_dirs {
        let index = base.join(theme).join("index.theme");
        let Ok(text) = std::fs::read_to_string(&index) else {
            continue;
        };
        let mut in_section = false;
        for raw in text.lines() {
            let line = raw.trim();
            if line.starts_with('[') {
                in_section = line == "[Icon Theme]";
                continue;
            }
            if !in_section {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == "Inherits" {
                    return v
                        .split(',')
                        .map(str::trim)
                        .filter(|p| !p.is_empty() && !p.contains('/') && *p != "..")
                        .map(str::to_string)
                        .collect();
                }
            }
        }
        return Vec::new();
    }
    Vec::new()
}

/// Expands `user_theme` via `index.theme` `Inherits=` (BFS) before falling back to hicolor and
/// common desktop themes.
fn theme_chain(base_dirs: &[PathBuf], user_theme: Option<&str>) -> Vec<String> {
    let mut themes = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    if let Some(t) = user_theme.map(str::trim).filter(|s| !s.is_empty()) {
        queue.push_back(t.to_string());
    }
    while let Some(theme) = queue.pop_front() {
        if !seen.insert(theme.clone()) {
            continue;
        }
        for parent in theme_inherits(base_dirs, &theme) {
            if !seen.contains(&parent) {
                queue.push_back(parent);
            }
        }
        themes.push(theme);
    }
    for fallback in ["hicolor", "Adwaita", "Papirus", "breeze", "gnome"] {
        if seen.insert(fallback.to_string()) {
            themes.push(fallback.to_string());
        }
    }
    themes
}

const ICON_SUBDIRS: &[&str] = &[
    "scalable/apps",
    "48x48/apps",
    "64x64/apps",
    "32x32/apps",
    "128x128/apps",
    "256x256/apps",
    "24x24/apps",
    "apps/48",
    "apps/64",
    "apps/scalable",
    "symbolic/apps",
    "scalable/devices",
    "scalable/status",
    "scalable/places",
    "scalable/categories",
];
const ICON_EXTS: &[&str] = &["svg", "png"];

/// Pre-resolved icon base directories and theme inheritance order.
#[derive(Clone, Debug)]
pub struct IconSearch {
    base_dirs: Vec<PathBuf>,
    themes: Vec<String>,
    pixmaps: Option<PathBuf>,
}

impl IconSearch {
    pub fn system() -> Self {
        let base_dirs = icon_base_dirs();
        let user_theme = user_icon_theme();
        let themes = theme_chain(&base_dirs, user_theme.as_deref());
        Self {
            base_dirs,
            themes,
            pixmaps: Some(PathBuf::from("/usr/share/pixmaps")),
        }
    }

    #[cfg(test)]
    pub fn with_dirs(base_dirs: Vec<PathBuf>, user_theme: Option<&str>) -> Self {
        let themes = theme_chain(&base_dirs, user_theme);
        Self {
            base_dirs,
            themes,
            pixmaps: None,
        }
    }

    pub fn resolve(&self, icon: &str) -> Option<PathBuf> {
        let icon = icon.trim();
        if icon.is_empty() {
            return None;
        }
        let direct = Path::new(icon);
        if direct.is_absolute() {
            return direct.is_file().then(|| direct.to_path_buf());
        }
        let stem = icon
            .strip_suffix(".png")
            .or_else(|| icon.strip_suffix(".svg"))
            .unwrap_or(icon);

        for theme in &self.themes {
            for base in &self.base_dirs {
                let theme_dir = base.join(theme);
                if !theme_dir.is_dir() {
                    continue;
                }
                for sub in ICON_SUBDIRS {
                    for ext in ICON_EXTS {
                        let candidate = theme_dir.join(sub).join(format!("{stem}.{ext}"));
                        if candidate.is_file() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }

        if let Some(pixmaps) = &self.pixmaps {
            for ext in ICON_EXTS {
                let candidate = pixmaps.join(format!("{stem}.{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

/// Resolves an icon name or path from a `.desktop` file to a PNG or SVG file on disk.
#[cfg(test)]
pub fn resolve_icon_path(icon: &str) -> Option<PathBuf> {
    IconSearch::system().resolve(icon)
}

#[repr(C)]
struct RsvgRectangle {
    x: c_double,
    y: c_double,
    width: c_double,
    height: c_double,
}

struct RsvgFns {
    new_from_file: unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> *mut c_void,
    render_document: unsafe extern "C" fn(
        *mut c_void,
        *mut cairo::ffi::cairo_t,
        *const RsvgRectangle,
        *mut *mut c_void,
    ) -> c_int,
    g_object_unref: unsafe extern "C" fn(*mut c_void),
    g_error_free: Option<unsafe extern "C" fn(*mut c_void)>,
}

fn rsvg_fns() -> Option<&'static RsvgFns> {
    static RSVG: OnceLock<Option<RsvgFns>> = OnceLock::new();
    RSVG.get_or_init(|| unsafe {
        let rsvg_name = CString::new("librsvg-2.so.2").ok()?;
        let gobj_name = CString::new("libgobject-2.0.so.0").ok()?;
        let glib_name = CString::new("libglib-2.0.so.0").ok()?;
        let rsvg = libc::dlopen(rsvg_name.as_ptr(), libc::RTLD_LAZY);
        if rsvg.is_null() {
            return None;
        }
        let gobj = libc::dlopen(gobj_name.as_ptr(), libc::RTLD_LAZY);
        if gobj.is_null() {
            return None;
        }
        let glib = libc::dlopen(glib_name.as_ptr(), libc::RTLD_LAZY);

        let s_new = CString::new("rsvg_handle_new_from_file").ok()?;
        let s_render = CString::new("rsvg_handle_render_document").ok()?;
        let s_unref = CString::new("g_object_unref").ok()?;
        let s_err = CString::new("g_error_free").ok()?;

        let p_new = libc::dlsym(rsvg, s_new.as_ptr());
        let p_render = libc::dlsym(rsvg, s_render.as_ptr());
        let p_unref = libc::dlsym(gobj, s_unref.as_ptr());
        if p_new.is_null() || p_render.is_null() || p_unref.is_null() {
            return None;
        }
        let p_err = if !glib.is_null() {
            let p = libc::dlsym(glib, s_err.as_ptr());
            if p.is_null() {
                None
            } else {
                Some(std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(*mut c_void),
                >(p))
            }
        } else {
            None
        };

        Some(RsvgFns {
            new_from_file: std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> *mut c_void,
            >(p_new),
            render_document: std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(
                    *mut c_void,
                    *mut cairo::ffi::cairo_t,
                    *const RsvgRectangle,
                    *mut *mut c_void,
                ) -> c_int,
            >(p_render),
            g_object_unref: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(
                p_unref,
            ),
            g_error_free: p_err,
        })
    })
    .as_ref()
}

/// Function pointers loaded from `gdk-pixbuf` (when first needed, like librsvg) to decode JPEG
/// covers and any other image format it has a loader for.
struct PixbufFns {
    get_file_info:
        Option<unsafe extern "C" fn(*const c_char, *mut c_int, *mut c_int) -> *mut c_void>,
    new_at_scale:
        unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, *mut *mut c_void) -> *mut c_void,
    pixels: unsafe extern "C" fn(*mut c_void) -> *mut u8,
    width: unsafe extern "C" fn(*mut c_void) -> c_int,
    height: unsafe extern "C" fn(*mut c_void) -> c_int,
    rowstride: unsafe extern "C" fn(*mut c_void) -> c_int,
    channels: unsafe extern "C" fn(*mut c_void) -> c_int,
    unref: unsafe extern "C" fn(*mut c_void),
    g_error_free: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// Pre-scaled premultiplied Cairo ARGB32 bitmap decoded off the draw thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ThumbKey {
    path: PathBuf,
    width: i32,
    height: i32,
    stamp: Option<(u64, i64, u32)>,
}

/// Thread-safe thumbnail cache keyed by file path, metadata stamp, and target pixel size.
#[derive(Default)]
pub struct ThumbCache {
    entries: Mutex<HashMap<ThumbKey, Option<Arc<DecodedImage>>>>,
    decodes: AtomicUsize,
}

impl ThumbCache {
    pub fn get_or_decode_with<F>(
        &self,
        path: &Path,
        width: i32,
        height: i32,
        decode: F,
    ) -> Option<Arc<DecodedImage>>
    where
        F: FnOnce(&Path, i32, i32) -> Option<DecodedImage>,
    {
        if width <= 0 || height <= 0 {
            return None;
        }
        let stamp = file_stamp(path)?;
        let key = ThumbKey {
            path: path.to_path_buf(),
            width,
            height,
            stamp: Some(stamp),
        };
        if let Ok(guard) = self.entries.lock() {
            if let Some(cached) = guard.get(&key) {
                return cached.clone();
            }
        }
        self.decodes.fetch_add(1, Ordering::Relaxed);
        let decoded = decode(path, width, height).map(Arc::new);
        if let Ok(mut guard) = self.entries.lock() {
            // Double-check if another thread raced to decode the exact same key first.
            if let Some(cached) = guard.get(&key) {
                return cached.clone();
            }
            guard.retain(|k, _| !(k.path == path && k.width == width && k.height == height));
            if guard.len() >= 256 {
                guard.clear();
            }
            guard.insert(key, decoded.clone());
        }
        decoded
    }

    pub fn get_or_decode(&self, path: &Path, width: i32, height: i32) -> Option<Arc<DecodedImage>> {
        self.get_or_decode_with(path, width, height, decode_cover_image)
    }

    #[cfg(test)]
    pub fn decode_count(&self) -> usize {
        self.decodes.load(Ordering::Relaxed)
    }
}

fn file_stamp(path: &Path) -> Option<(u64, i64, u32)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let (secs, nanos) = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| (d.as_secs() as i64, d.subsec_nanos()))
        .unwrap_or((0, 0));
    Some((meta.len(), secs, nanos))
}

/// Shared thumbnail cache used by `WallpapersSource` so each `(file, size)` decodes and scales once.
pub fn cached_wallpaper_thumb(path: &Path, width: i32, height: i32) -> Option<Arc<DecodedImage>> {
    static THUMB_CACHE: OnceLock<ThumbCache> = OnceLock::new();
    THUMB_CACHE
        .get_or_init(ThumbCache::default)
        .get_or_decode(path, width, height)
}

fn pixbuf_fns() -> Option<&'static PixbufFns> {
    static PB: OnceLock<Option<PixbufFns>> = OnceLock::new();
    PB.get_or_init(|| unsafe {
        let lib = libc::dlopen(
            CString::new("libgdk_pixbuf-2.0.so.0").ok()?.as_ptr(),
            libc::RTLD_LAZY,
        );
        let gobj = libc::dlopen(
            CString::new("libgobject-2.0.so.0").ok()?.as_ptr(),
            libc::RTLD_LAZY,
        );
        if lib.is_null() || gobj.is_null() {
            return None;
        }
        let glib = libc::dlopen(
            CString::new("libglib-2.0.so.0").ok()?.as_ptr(),
            libc::RTLD_LAZY,
        );
        let sym = |h: *mut c_void, n: &str| -> Option<*mut c_void> {
            let c = CString::new(n).ok()?;
            let p = libc::dlsym(h, c.as_ptr());
            (!p.is_null()).then_some(p)
        };
        let err = if glib.is_null() {
            None
        } else {
            sym(glib, "g_error_free")
                .map(|p| std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(p))
        };
        let get_file_info = sym(lib, "gdk_pixbuf_get_file_info").map(|p| {
            std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(*const c_char, *mut c_int, *mut c_int) -> *mut c_void,
            >(p)
        });
        Some(PixbufFns {
            get_file_info,
            new_at_scale: std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, *mut *mut c_void) -> *mut c_void,
            >(sym(lib, "gdk_pixbuf_new_from_file_at_scale")?),
            pixels: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> *mut u8>(
                sym(lib, "gdk_pixbuf_get_pixels")?,
            ),
            width: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_width")?,
            ),
            height: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_height")?,
            ),
            rowstride: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_rowstride")?,
            ),
            channels: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> c_int>(
                sym(lib, "gdk_pixbuf_get_n_channels")?,
            ),
            unref: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(
                sym(gobj, "g_object_unref")?,
            ),
            g_error_free: err,
        })
    })
    .as_ref()
}

fn free_g_error(fns: &PixbufFns, err: *mut c_void) {
    if !err.is_null() {
        if let Some(free_fn) = fns.g_error_free {
            unsafe { free_fn(err) };
        }
    }
}

/// Decodes and scales an image through gdk-pixbuf into a premultiplied ARGB32 buffer. When `cover`
/// is true, scales to cover `width x height` preserving aspect ratio and center-crops.
fn decode_pixbuf_image(
    fns: &PixbufFns,
    path: &Path,
    width: i32,
    height: i32,
    cover: bool,
) -> Option<DecodedImage> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let c_path = CString::new(path.to_str()?).ok()?;
    unsafe {
        let (scale_w, scale_h) = if cover {
            let mut sw: c_int = 0;
            let mut sh: c_int = 0;
            let fmt = fns.get_file_info.map_or(std::ptr::null_mut(), |info| {
                info(c_path.as_ptr(), &mut sw, &mut sh)
            });
            if !fmt.is_null() && sw > 0 && sh > 0 {
                let scale = (width as f64 / sw as f64).max(height as f64 / sh as f64);
                (
                    ((sw as f64 * scale).round() as i32).max(width),
                    ((sh as f64 * scale).round() as i32).max(height),
                )
            } else {
                (width, height)
            }
        } else {
            (width, height)
        };
        let mut err: *mut c_void = std::ptr::null_mut();
        let pb = (fns.new_at_scale)(c_path.as_ptr(), scale_w, scale_h, 0, &mut err);
        free_g_error(fns, err);
        if pb.is_null() {
            return None;
        }
        let (pw, ph) = ((fns.width)(pb), (fns.height)(pb));
        let (stride, ch) = ((fns.rowstride)(pb) as usize, (fns.channels)(pb) as usize);
        let px = (fns.pixels)(pb);
        let out = (|| {
            if pw <= 0 || ph <= 0 || px.is_null() || !(ch == 3 || ch == 4) {
                return None;
            }
            if stride < pw as usize * ch {
                return None;
            }
            let out_w = if cover { width } else { pw };
            let out_h = if cover { height } else { ph };
            let ox = ((pw - out_w).max(0) / 2) as usize;
            let oy = ((ph - out_h).max(0) / 2) as usize;
            let dx = ((out_w - pw).max(0) / 2) as usize;
            let dy = ((out_h - ph).max(0) / 2) as usize;
            let copy_w = pw.min(out_w) as usize;
            let copy_h = ph.min(out_h) as usize;
            let dst_stride = out_w as usize * 4;
            let mut data = vec![0u8; dst_stride * out_h as usize];
            for y in 0..copy_h {
                let row = std::slice::from_raw_parts(px.add((oy + y) * stride), pw as usize * ch);
                for x in 0..copy_w {
                    let s = &row[(ox + x) * ch..(ox + x) * ch + ch];
                    let a = if ch == 4 { s[3] as u32 } else { 255 };
                    let pm = |c: u8| (c as u32 * a + 127) / 255;
                    let v = (a << 24) | (pm(s[0]) << 16) | (pm(s[1]) << 8) | pm(s[2]);
                    let off = (dy + y) * dst_stride + (dx + x) * 4;
                    data[off..off + 4].copy_from_slice(&v.to_ne_bytes());
                }
            }
            Some(DecodedImage {
                width: out_w,
                height: out_h,
                stride: dst_stride as i32,
                data,
            })
        })();
        (fns.unref)(pb);
        out
    }
}

/// Decodes an image file via gdk-pixbuf scaled to cover `width x height`, returning `None` when
/// `libgdk_pixbuf-2.0` is absent or the file is unreadable/invalid.
pub fn decode_cover_image(path: &Path, width: i32, height: i32) -> Option<DecodedImage> {
    decode_pixbuf_image(pixbuf_fns()?, path, width, height, true)
}

pub const FACE_SIZE: i32 = 256;

/// Decodes `src` via `gdk-pixbuf` and scales/centre-crops it into a 256x256 PNG written
/// atomically to `dst`.
pub fn write_face_png(src: &Path, dst: &Path) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let img = decode_cover_image(src, FACE_SIZE, FACE_SIZE)
        .ok_or_else(|| format!("cannot decode image {}", src.display()))?;
    let surf = image_surface_from_decoded(&img)
        .ok_or_else(|| format!("cannot prepare image surface for {}", src.display()))?;
    if let Some(parent) = dst.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dst.with_file_name(format!(".face.tmp.{}.{seq}", std::process::id()));
    surf.flush();
    let res = (|| -> Result<(), String> {
        let mut f = File::create(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
        surf.write_to_png(&mut f)
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        drop(f);
        std::fs::rename(&tmp, dst).map_err(|e| format!("rename to {}: {e}", dst.display()))
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Converts a pre-decoded ARGB32 bitmap into a Cairo `ImageSurface` without any image decoding or scaling.
pub fn image_surface_from_decoded(img: &DecodedImage) -> Option<cairo::ImageSurface> {
    if img.width <= 0 || img.height <= 0 || img.stride < img.width * 4 {
        return None;
    }
    let mut surf =
        cairo::ImageSurface::create(cairo::Format::ARgb32, img.width, img.height).ok()?;
    let dst_stride = surf.stride() as usize;
    let src_stride = img.stride as usize;
    let row_bytes = img.width as usize * 4;
    if img.data.len() < img.height as usize * src_stride {
        return None;
    }
    {
        let mut data = surf.data().ok()?;
        for y in 0..img.height as usize {
            let s0 = y * src_stride;
            let d0 = y * dst_stride;
            data[d0..d0 + row_bytes].copy_from_slice(&img.data[s0..s0 + row_bytes]);
        }
    }
    surf.mark_dirty();
    Some(surf)
}

thread_local! {
    static DECODED_SURFACES: RefCell<HashMap<usize, (Arc<DecodedImage>, cairo::ImageSurface)>> =
        RefCell::new(HashMap::new());
}

/// Converts and caches a `DecodedImage` as a Cairo `ImageSurface` on the draw thread, keyed by
/// the `Arc` allocation pointer so identical decoded bitmaps are copied at most once.
pub fn surface_for_decoded(img: &Arc<DecodedImage>) -> Option<cairo::ImageSurface> {
    let key = Arc::as_ptr(img) as usize;
    DECODED_SURFACES.with(|cache| {
        let mut map = cache.borrow_mut();
        if let Some((held, surf)) = map.get(&key) {
            if Arc::ptr_eq(held, img) {
                return Some(surf.clone());
            }
        }
        let surf = image_surface_from_decoded(img)?;
        if map.len() >= 256 {
            map.clear();
        }
        map.insert(key, (Arc::clone(img), surf.clone()));
        Some(surf)
    })
}

fn decoded_from_surface(surf: &mut cairo::ImageSurface) -> Option<DecodedImage> {
    let width = surf.width();
    let height = surf.height();
    let stride = surf.stride();
    if width <= 0 || height <= 0 || stride < width * 4 {
        return None;
    }
    surf.flush();
    let data = surf.data().ok()?.to_vec();
    Some(DecodedImage {
        width,
        height,
        stride,
        data,
    })
}

/// Decodes a PNG, SVG, or `gdk-pixbuf`-supported image file into a `DecodedImage`.
pub fn load_icon_decoded(path: &Path, size: i32) -> Option<DecodedImage> {
    if size <= 0 {
        return None;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "png" {
        let mut f = File::open(path).ok()?;
        let mut src = cairo::ImageSurface::create_from_png(&mut f).ok()?;
        let sw = src.width();
        let sh = src.height();
        if sw <= 0 || sh <= 0 {
            return None;
        }
        if sw == size && sh == size {
            return decoded_from_surface(&mut src);
        }
        let mut dst = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size).ok()?;
        let cr = cairo::Context::new(&dst).ok()?;
        cr.scale(size as f64 / sw as f64, size as f64 / sh as f64);
        let _ = cr.set_source_surface(&src, 0.0, 0.0);
        let _ = cr.paint();
        drop(cr);
        return decoded_from_surface(&mut dst);
    }
    if ext == "svg" {
        let fns = rsvg_fns()?;
        let c_path = CString::new(path.to_str()?).ok()?;
        unsafe {
            let mut err: *mut c_void = std::ptr::null_mut();
            let handle = (fns.new_from_file)(c_path.as_ptr(), &mut err);
            if !err.is_null() {
                if let Some(free_fn) = fns.g_error_free {
                    free_fn(err);
                }
            }
            if handle.is_null() {
                return None;
            }
            let Some(mut dst) = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size).ok()
            else {
                (fns.g_object_unref)(handle);
                return None;
            };
            let Some(cr) = cairo::Context::new(&dst).ok() else {
                (fns.g_object_unref)(handle);
                return None;
            };
            let viewport = RsvgRectangle {
                x: 0.0,
                y: 0.0,
                width: size as f64,
                height: size as f64,
            };
            let mut render_err: *mut c_void = std::ptr::null_mut();
            let ok = (fns.render_document)(handle, cr.to_raw_none(), &viewport, &mut render_err);
            if !render_err.is_null() {
                if let Some(free_fn) = fns.g_error_free {
                    free_fn(render_err);
                }
            }
            (fns.g_object_unref)(handle);
            drop(cr);
            if ok == 0 {
                return None;
            }
            return decoded_from_surface(&mut dst);
        }
    }
    // anything else (JPEG covers, an embedded cover written without an extension): gdk-pixbuf
    decode_pixbuf_image(pixbuf_fns()?, path, size, size, false)
}

/// Loads a PNG, SVG, or `gdk-pixbuf`-supported image file into a `size x size` `cairo::ImageSurface`.
#[cfg(test)]
pub fn load_icon_surface(path: &Path, size: i32) -> Option<cairo::ImageSurface> {
    load_icon_decoded(path, size).and_then(|d| image_surface_from_decoded(&d))
}

enum CacheSlot {
    Pending,
    Ready(Option<Arc<DecodedImage>>),
}

#[derive(Default)]
struct IconState {
    generation: u64,
    entries: HashMap<String, Vec<(i32, CacheSlot)>>,
    queue: VecDeque<(u64, String, i32)>,
    active: usize,
    search: Option<IconSearch>,
}

type SearchFn = Box<dyn Fn() -> IconSearch + Send + Sync>;
type WakeFn = Box<dyn Fn() + Send + Sync>;

struct IconCacheInner {
    state: Mutex<IconState>,
    work_cv: Condvar,
    idle_cv: Condvar,
    make_search: SearchFn,
    waker: Mutex<Option<WakeFn>>,
    lookups: AtomicUsize,
}

/// Asynchronous app-icon cache: `draw_app_icon` returns immediately (drawing the letter tile on
/// misses) while a single background worker resolves theme paths and decodes bitmaps.
pub struct IconCache {
    inner: Arc<IconCacheInner>,
    worker_started: AtomicBool,
}

impl IconCache {
    pub fn new(make_search: impl Fn() -> IconSearch + Send + Sync + 'static) -> Self {
        Self {
            inner: Arc::new(IconCacheInner {
                state: Mutex::new(IconState::default()),
                work_cv: Condvar::new(),
                idle_cv: Condvar::new(),
                make_search: Box::new(make_search),
                waker: Mutex::new(None),
                lookups: AtomicUsize::new(0),
            }),
            worker_started: AtomicBool::new(false),
        }
    }

    pub fn system() -> Self {
        Self::new(IconSearch::system)
    }

    pub fn set_wakeup(&self, waker: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut guard) = self.inner.waker.lock() {
            *guard = Some(Box::new(waker));
        }
    }

    fn ensure_worker(&self) {
        if self.worker_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let spawn = std::thread::Builder::new()
            .name("edge-icons".into())
            .spawn(move || loop {
                let (gen, icon, size, search_opt) = {
                    let mut st = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                    while st.queue.is_empty() {
                        st = inner.work_cv.wait(st).unwrap_or_else(|p| p.into_inner());
                    }
                    let (gen, icon, size) = st.queue.pop_front().unwrap();
                    st.active += 1;
                    let search_opt = st.search.clone();
                    (gen, icon, size, search_opt)
                };
                let search = match search_opt {
                    Some(s) => s,
                    None => {
                        let built = (inner.make_search)();
                        let mut st = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                        if st.generation == gen && st.search.is_none() {
                            st.search = Some(built.clone());
                        }
                        built
                    }
                };
                inner.lookups.fetch_add(1, Ordering::Relaxed);
                let decoded = search
                    .resolve(&icon)
                    .and_then(|p| load_icon_decoded(&p, size))
                    .map(Arc::new);
                let should_wake = {
                    let mut st = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                    st.active -= 1;
                    let keep = st.generation == gen;
                    if keep {
                        let list = st.entries.entry(icon).or_default();
                        if let Some((_, slot)) = list.iter_mut().find(|(s, _)| *s == size) {
                            *slot = CacheSlot::Ready(decoded.clone());
                        } else {
                            list.push((size, CacheSlot::Ready(decoded.clone())));
                        }
                    }
                    if st.queue.is_empty() && st.active == 0 {
                        inner.idle_cv.notify_all();
                    }
                    keep && decoded.is_some()
                };
                if should_wake {
                    if let Ok(guard) = inner.waker.lock() {
                        if let Some(w) = guard.as_ref() {
                            w();
                        }
                    }
                }
            });
        if spawn.is_err() {
            self.worker_started.store(false, Ordering::Release);
        }
    }

    pub fn get_or_queue(&self, icon: &str, size: i32) -> Option<Arc<DecodedImage>> {
        if icon.is_empty() || size <= 0 {
            return None;
        }
        {
            let mut st = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(list) = st.entries.get(icon) {
                if let Some((_, slot)) = list.iter().find(|(s, _)| *s == size) {
                    return match slot {
                        CacheSlot::Ready(img) => img.clone(),
                        CacheSlot::Pending => None,
                    };
                }
            }
            let gen = st.generation;
            st.entries
                .entry(icon.to_string())
                .or_default()
                .push((size, CacheSlot::Pending));
            st.queue.push_back((gen, icon.to_string(), size));
        }
        self.ensure_worker();
        self.inner.work_cv.notify_one();
        None
    }

    pub fn clear(&self) {
        let mut st = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        st.generation = st.generation.wrapping_add(1);
        st.entries.clear();
        st.queue.clear();
        st.search = None;
        if st.active == 0 {
            self.inner.idle_cv.notify_all();
        }
    }

    pub fn wait_idle(&self) {
        let mut st = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        while !st.queue.is_empty() || st.active > 0 {
            st = self
                .inner
                .idle_cv
                .wait(st)
                .unwrap_or_else(|p| p.into_inner());
        }
    }

    #[cfg(test)]
    pub fn lookup_count(&self) -> usize {
        self.inner.lookups.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub fn generation(&self) -> u64 {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .generation
    }
}

static GLOBAL_ICON_CACHE: OnceLock<IconCache> = OnceLock::new();
static DISPLAY_SCALE_BITS: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

fn global_icon_cache() -> &'static IconCache {
    GLOBAL_ICON_CACHE.get_or_init(IconCache::system)
}

/// Records the active output scale used when off-UI sources decode covers and thumbnails.
pub fn set_display_scale(scale: f64) {
    let s = if scale.is_finite() && scale > 0.0 {
        scale as f32
    } else {
        1.0
    };
    DISPLAY_SCALE_BITS.store(s.to_bits(), Ordering::Relaxed);
}

/// Active output scale for cover and thumbnail decoding (1.0 until set).
pub fn display_scale() -> f64 {
    let s = f64::from(f32::from_bits(DISPLAY_SCALE_BITS.load(Ordering::Relaxed)));
    if s.is_finite() && s > 0.0 {
        s
    } else {
        1.0
    }
}

/// Logical size converted to rounded device pixels at `scale` (at least 1).
pub fn scaled_px(logical: i32, scale: f64) -> i32 {
    let s = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    ((f64::from(logical) * s).round().max(1.0)) as i32
}

/// Clears the shared app-icon cache so theme, desktop-entry, or icon file changes are picked up.
pub fn clear_icon_cache() {
    global_icon_cache().clear();
}

#[cfg(test)]
pub fn global_icon_generation() -> u64 {
    global_icon_cache().generation()
}

/// Registers a callback invoked on the worker thread whenever an icon finishes decoding.
pub fn set_icon_wakeup(waker: impl Fn() + Send + Sync + 'static) {
    global_icon_cache().set_wakeup(waker);
}

/// Waits for any queued background icon lookups/decodes to finish (used by `--render`).
pub fn wait_icon_idle() {
    if let Some(cache) = GLOBAL_ICON_CACHE.get() {
        cache.wait_idle();
    }
}

/// An app's icon at `(x, y)`, `size` px square; without one (or while loading), a tile in the
/// theme's track colour with the name's first letter.
#[allow(clippy::too_many_arguments)]
pub fn draw_app_icon_with(
    cache: &IconCache,
    cr: &Context,
    cx: &Cx,
    icon: &str,
    name: &str,
    x: f64,
    y: f64,
    size: f64,
) {
    let (dx, dy) = cr
        .user_to_device_distance(size, size)
        .unwrap_or((size, size));
    let px_size = dx.abs().max(dy.abs()).round().max(1.0) as i32;
    if let Some(img) = cache.get_or_queue(icon, px_size) {
        if let Some(surface) = surface_for_decoded(&img) {
            let _ = cr.save();
            let sw = surface.width() as f64;
            let sh = surface.height() as f64;
            if (sw - size).abs() > 0.25 || (sh - size).abs() > 0.25 {
                let (dev_x, dev_y) = cr.user_to_device(x, y);
                let (ux, uy) = cr
                    .device_to_user(dev_x.round(), dev_y.round())
                    .unwrap_or((x, y));
                cr.translate(ux, uy);
                cr.scale(size / sw, size / sh);
                let _ = cr.set_source_surface(&surface, 0.0, 0.0);
            } else {
                let _ = cr.set_source_surface(&surface, x.round(), y.round());
            }
            let _ = cr.paint();
            let _ = cr.restore();
            return;
        }
    }
    let th = cx.theme;
    draw::round_rect(cr, x, y, size, size, th.small_radius().min(size * 0.25));
    draw::set(cr, th.track);
    let _ = cr.fill();
    let letter: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .or_else(|| name.chars().next())
        .unwrap_or('?')
        .to_uppercase()
        .collect();
    let t = Text {
        size: (size * 0.5).round().max(10.0),
        bold: true,
        color: th.text,
        text: &letter,
    };
    let (_, lh) = draw::measure(cr, &t);
    draw::text_center(cr, x + size / 2.0, y + (size - lh) / 2.0, &t);
}

/// An app's icon at `(x, y)`, `size` px square; without one, a tile in the theme's track colour
/// with the name's first letter.
#[allow(clippy::too_many_arguments)]
pub fn draw_app_icon(cr: &Context, cx: &Cx, icon: &str, name: &str, x: f64, y: f64, size: f64) {
    draw_app_icon_with(global_icon_cache(), cr, cx, icon, name, x, y, size);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_face_png_centre_crops_to_256_square_and_rejects_invalid_source() {
        let dir = std::env::temp_dir().join(format!("edge-apps-face-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = dir.join("wide.png");
        let (w, h) = (400, 200);
        let mut s = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).unwrap();
        let stride = s.stride() as usize;
        {
            let mut d = s.data().unwrap();
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let off = y * stride + x * 4;
                    let bgra = if x < 100 {
                        [0x00, 0x00, 0xff, 0xff] // left strip red
                    } else if x >= 300 {
                        [0xff, 0x00, 0x00, 0xff] // right strip blue
                    } else {
                        [0x00, 0xff, 0x00, 0xff] // centre 200x200 square green
                    };
                    d[off..off + 4].copy_from_slice(&bgra);
                }
            }
        }
        s.mark_dirty();
        s.write_to_png(&mut File::create(&src).unwrap()).unwrap();

        let dst = dir.join(".face");
        write_face_png(&src, &dst).unwrap();
        let mut out = cairo::ImageSurface::create_from_png(&mut File::open(&dst).unwrap()).unwrap();
        assert_eq!((out.width(), out.height()), (FACE_SIZE, FACE_SIZE));
        let ostr = out.stride() as usize;
        let odata = out.data().unwrap();
        for (px, py) in [(4, 4), (128, 128), (251, 251)] {
            let off = py * ostr + px * 4;
            assert!(
                odata[off + 1] > 200 && odata[off] < 30 && odata[off + 2] < 30,
                "expected centre green crop at ({px},{py}), got {:?}",
                &odata[off..off + 4]
            );
        }

        // Invalid input fails cleanly and does not overwrite existing dst.
        let bad = dir.join("bad.png");
        std::fs::write(&bad, b"not an image").unwrap();
        assert!(write_face_png(&bad, &dst).is_err());
        assert!(dst.is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_and_loads_a_png_and_an_svg_by_name_and_path() {
        let dir = std::env::temp_dir().join(format!("edge-apps-icons-{}", std::process::id()));
        let apps = dir.join("icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&apps).unwrap();
        let png = apps.join("edge-test-icon.png");
        let s = cairo::ImageSurface::create(cairo::Format::ARgb32, 48, 48).unwrap();
        s.write_to_png(&mut File::create(&png).unwrap()).unwrap();
        // found by its absolute path, and loaded at another size
        assert_eq!(resolve_icon_path(png.to_str().unwrap()), Some(png.clone()));
        let loaded = load_icon_surface(&png, 32).unwrap();
        assert_eq!((loaded.width(), loaded.height()), (32, 32));
        // a missing name and an empty one find nothing
        assert_eq!(resolve_icon_path(""), None);
        assert_eq!(resolve_icon_path("/nonexistent/x.png"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_solid_png(path: &Path, w: i32, h: i32, r: u8, g: u8, b: u8) {
        let mut s = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).unwrap();
        {
            let stride = s.stride() as usize;
            let mut data = s.data().unwrap();
            let px = (255u32 << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            let bytes = px.to_ne_bytes();
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let off = y * stride + x * 4;
                    data[off..off + 4].copy_from_slice(&bytes);
                }
            }
        }
        s.mark_dirty();
        s.write_to_png(&mut File::create(path).unwrap()).unwrap();
    }

    #[test]
    fn thumb_cache_scales_cover_once_per_file_and_size_and_handles_broken() {
        let dir = std::env::temp_dir().join(format!("edge-apps-thumbs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let p1 = dir.join("wide.png");
        let p2 = dir.join("tall.png");
        let broken = dir.join("broken.png");
        write_solid_png(&p1, 320, 180, 200, 40, 80);
        write_solid_png(&p2, 100, 200, 30, 120, 220);
        std::fs::write(&broken, b"not a valid png file").unwrap();

        let cache = ThumbCache::default();
        // Missing file decodes nothing and increments no decoder calls.
        assert!(cache
            .get_or_decode(&dir.join("missing.png"), 192, 108)
            .is_none());
        assert_eq!(cache.decode_count(), 0);

        let t1a = cache.get_or_decode(&p1, 192, 108).unwrap();
        assert_eq!((t1a.width, t1a.height), (192, 108));
        assert_eq!(cache.decode_count(), 1);
        // Repeat at same size hits cache.
        let t1b = cache.get_or_decode(&p1, 192, 108).unwrap();
        assert!(Arc::ptr_eq(&t1a, &t1b));
        assert_eq!(cache.decode_count(), 1);

        // Different size scales and caches independently.
        let t1m = cache.get_or_decode(&p1, 129, 73).unwrap();
        assert_eq!((t1m.width, t1m.height), (129, 73));
        assert!(!Arc::ptr_eq(&t1a, &t1m));
        assert_eq!(cache.decode_count(), 2);

        // Tall 1:2 image still fills 16:9 target via cover crop without aspect stretch.
        let t2 = cache.get_or_decode(&p2, 192, 108).unwrap();
        assert_eq!((t2.width, t2.height), (192, 108));
        let surf2 = image_surface_from_decoded(&t2).unwrap();
        assert_eq!((surf2.width(), surf2.height()), (192, 108));
        assert_eq!(cache.decode_count(), 3);

        // Broken file returns None and is cached until replaced.
        assert!(cache.get_or_decode(&broken, 192, 108).is_none());
        assert_eq!(cache.decode_count(), 4);
        assert!(cache.get_or_decode(&broken, 192, 108).is_none());
        assert_eq!(cache.decode_count(), 4);

        // Edge still works when pixbuf decoder is absent (simulated via None).
        let no_pb = ThumbCache::default();
        assert!(no_pb
            .get_or_decode_with(&p1, 192, 108, |_, _, _| None)
            .is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn icon_search_follows_index_theme_inherits_and_handles_cycles() {
        let dir = std::env::temp_dir().join(format!("edge-apps-inherits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let child = dir.join("child");
        let parent = dir.join("parent");
        let grand = dir.join("grand");
        std::fs::create_dir_all(child.join("48x48/apps")).unwrap();
        std::fs::create_dir_all(parent.join("48x48/apps")).unwrap();
        std::fs::create_dir_all(grand.join("48x48/apps")).unwrap();

        std::fs::write(
            child.join("index.theme"),
            "[Icon Theme]\nName=Child\nInherits=parent,grand\n",
        )
        .unwrap();
        // Cycle back to child should terminate cleanly.
        std::fs::write(
            parent.join("index.theme"),
            "[Icon Theme]\nName=Parent\nInherits=child,grand\n",
        )
        .unwrap();
        std::fs::write(grand.join("index.theme"), "[Icon Theme]\nName=Grand\n").unwrap();

        let p_parent = parent.join("48x48/apps/inherited-app.png");
        let p_grand = grand.join("48x48/apps/grand-app.png");
        write_solid_png(&p_parent, 48, 48, 10, 200, 30);
        write_solid_png(&p_grand, 48, 48, 30, 40, 220);

        let search = IconSearch::with_dirs(vec![dir.clone()], Some("child"));
        assert_eq!(search.resolve("inherited-app"), Some(p_parent));
        assert_eq!(search.resolve("grand-app"), Some(p_grand));
        assert_eq!(search.resolve("nonexistent-app"), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn icon_cache_decodes_on_worker_reloads_changed_files_and_retries_failed_lookups() {
        let dir =
            std::env::temp_dir().join(format!("edge-apps-icon-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let apps_dir = dir.join("custom/48x48/apps");
        std::fs::create_dir_all(&apps_dir).unwrap();
        let icon_path = apps_dir.join("reload-app.png");
        write_solid_png(&icon_path, 48, 48, 220, 20, 20);

        let base = dir.clone();
        let cache =
            IconCache::new(move || IconSearch::with_dirs(vec![base.clone()], Some("custom")));
        let woke = Arc::new(AtomicUsize::new(0));
        let woke_cb = Arc::clone(&woke);
        cache.set_wakeup(move || {
            woke_cb.fetch_add(1, Ordering::Relaxed);
        });

        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = Cx {
            theme: &th,
            now: chrono::NaiveDateTime::default(),
            size: crate::provider::Size::M,
            app: None,
            apps: &[],
            recent: &crate::recent::NONE,
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
            room: f64::INFINITY,
        };

        let sample_center = |icon: &str| -> [u8; 4] {
            let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 48, 48).unwrap();
            {
                let cr = Context::new(&surf).unwrap();
                draw_app_icon_with(&cache, &cr, &cx, icon, "App", 8.0, 8.0, 28.0);
            }
            surf.flush();
            let stride = surf.stride() as usize;
            let data = surf.data().unwrap();
            let off = 22 * stride + 22 * 4;
            [data[off], data[off + 1], data[off + 2], data[off + 3]]
        };

        // First draw queues worker and draws fallback immediately.
        let _ = sample_center("reload-app");
        let _ = sample_center("later-app");
        cache.wait_idle();
        assert_eq!(cache.lookup_count(), 2);
        assert_eq!(woke.load(Ordering::Relaxed), 1);

        // Second draw paints the red icon pixels; missing icon stays remembered without re-lookup.
        let red_px = sample_center("reload-app");
        assert!(
            red_px[2] > 180 && red_px[1] < 40,
            "expected red: {red_px:?}"
        );
        let _ = sample_center("later-app");
        cache.wait_idle();
        assert_eq!(cache.lookup_count(), 2);

        // Update icon file on disk, add the previously missing icon, and signal a reload.
        write_solid_png(&icon_path, 48, 48, 20, 220, 40);
        write_solid_png(&apps_dir.join("later-app.png"), 48, 48, 30, 60, 230);
        cache.clear();

        let _ = sample_center("reload-app");
        let _ = sample_center("later-app");
        cache.wait_idle();
        assert_eq!(cache.lookup_count(), 4);

        let green_px = sample_center("reload-app");
        assert!(
            green_px[1] > 180 && green_px[2] < 40,
            "expected green after reload: {green_px:?}"
        );
        let blue_px = sample_center("later-app");
        assert!(
            blue_px[0] > 180 && blue_px[2] < 60,
            "expected blue for retried icon after reload: {blue_px:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn app_icon_caches_and_draws_at_device_pixel_size_when_scaled() {
        let dir = std::env::temp_dir().join(format!("edge-apps-icon-scale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let apps_dir = dir.join("custom/256x256/apps");
        std::fs::create_dir_all(&apps_dir).unwrap();
        let icon_path = apps_dir.join("scaled-app.png");
        // Left half red, right half blue at 128x128 so scaling down to 64px keeps crisp halves.
        let mut src = cairo::ImageSurface::create(cairo::Format::ARgb32, 128, 128).unwrap();
        {
            let stride = src.stride() as usize;
            let mut data = src.data().unwrap();
            for y in 0..128usize {
                for x in 0..128usize {
                    let off = y * stride + x * 4;
                    let bgra = if x < 64 {
                        [0, 0, 240, 255]
                    } else {
                        [240, 0, 0, 255]
                    };
                    data[off..off + 4].copy_from_slice(&bgra);
                }
            }
        }
        src.mark_dirty();
        src.write_to_png(&mut File::create(&icon_path).unwrap())
            .unwrap();

        let base = dir.clone();
        let cache =
            IconCache::new(move || IconSearch::with_dirs(vec![base.clone()], Some("custom")));
        let th = crate::theme::Theme::from_config(&crate::config::Config::from_text(""));
        let cx = Cx {
            theme: &th,
            now: chrono::NaiveDateTime::default(),
            size: crate::provider::Size::M,
            app: None,
            apps: &[],
            recent: &crate::recent::NONE,
            windows: &[],
            notes: &[],
            list_above: false,
            editing: false,
            room: f64::INFINITY,
        };

        // 1x draw queues and caches 32x32.
        let surf_1x = cairo::ImageSurface::create(cairo::Format::ARgb32, 48, 48).unwrap();
        {
            let cr = Context::new(&surf_1x).unwrap();
            draw_app_icon_with(&cache, &cr, &cx, "scaled-app", "App", 8.0, 8.0, 32.0);
        }
        cache.wait_idle();
        assert_eq!(cache.lookup_count(), 1);
        let img_1x = cache.get_or_queue("scaled-app", 32).unwrap();
        assert_eq!((img_1x.width, img_1x.height), (32, 32));

        // 2x scaled draw queues and caches 64x64 independently and paints the full 64x64 device box.
        let mut surf_2x = cairo::ImageSurface::create(cairo::Format::ARgb32, 96, 96).unwrap();
        {
            let cr = Context::new(&surf_2x).unwrap();
            cr.scale(2.0, 2.0);
            draw_app_icon_with(&cache, &cr, &cx, "scaled-app", "App", 8.0, 8.0, 32.0);
        }
        cache.wait_idle();
        assert_eq!(cache.lookup_count(), 2);
        let img_2x = cache.get_or_queue("scaled-app", 64).unwrap();
        assert_eq!((img_2x.width, img_2x.height), (64, 64));

        {
            let cr = Context::new(&surf_2x).unwrap();
            cr.scale(2.0, 2.0);
            draw_app_icon_with(&cache, &cr, &cx, "scaled-app", "App", 8.0, 8.0, 32.0);
        }
        surf_2x.flush();
        let stride = surf_2x.stride() as usize;
        let data = surf_2x.data().unwrap();
        // Device pixels (16..48) are red (left half of 64x64), (48..80) are blue (right half).
        let left_off = 48 * stride + 30 * 4;
        let right_off = 48 * stride + 66 * 4;
        assert!(data[left_off + 2] > 200 && data[left_off] < 30);
        assert!(data[right_off] > 200 && data[right_off + 2] < 30);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
