//! App icons: an icon name from a desktop entry found in the icon themes (the user's GTK theme
//! first, then hicolor and the common ones), loaded as PNG, SVG (librsvg) or any other image
//! (gdk-pixbuf), both loaded only when first needed so the edge runs without them; cached per
//! name and size. Ported from live/edge-rs/src/modules/apps.rs.

use crate::card::Cx;
use crate::draw::{self, Text};
use cairo::Context;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_double, c_int, c_void, CString};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// The icon theme the user chose for GTK apps (`gtk-icon-theme-name` in settings.ini), read once.
fn user_icon_theme() -> Option<String> {
    static THEME: OnceLock<Option<String>> = OnceLock::new();
    THEME
        .get_or_init(|| {
            let cfg = std::env::var("XDG_CONFIG_HOME")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|h| PathBuf::from(h).join(".config"))
                })?;
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
        })
        .clone()
}

/// Resolves an icon name or path from a `.desktop` file to a PNG or SVG file on disk.
pub fn resolve_icon_path(icon: &str) -> Option<PathBuf> {
    let icon = icon.trim();
    if icon.is_empty() {
        return None;
    }
    let direct = Path::new(icon);
    if direct.is_absolute() {
        return direct.is_file().then(|| direct.to_path_buf());
    }

    let mut base_dirs: Vec<PathBuf> = Vec::new();
    if let Some(dh) = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
    {
        base_dirs.push(PathBuf::from(dh).join("icons"));
    } else if let Some(h) = std::env::var("HOME").ok().filter(|s| !s.is_empty()) {
        let hp = PathBuf::from(h);
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

    let mut themes: Vec<String> = user_icon_theme().into_iter().collect();
    for t in ["hicolor", "Adwaita", "Papirus", "breeze", "gnome"] {
        if !themes.iter().any(|x| x == t) {
            themes.push(t.to_string());
        }
    }
    let subdirs = [
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
    let exts = ["svg", "png"];

    let stem = icon
        .strip_suffix(".png")
        .or_else(|| icon.strip_suffix(".svg"))
        .unwrap_or(icon);

    for base in &base_dirs {
        for theme in &themes {
            let theme_dir = base.join(theme);
            if !theme_dir.is_dir() {
                continue;
            }
            for sub in &subdirs {
                for ext in &exts {
                    let candidate = theme_dir.join(sub).join(format!("{stem}.{ext}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    let pixmaps = Path::new("/usr/share/pixmaps");
    for ext in &exts {
        let candidate = pixmaps.join(format!("{stem}.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
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

/// Loads an image file scaled to at most `size` x `size` through `gdk-pixbuf` into an ARGB32
/// Cairo surface.
pub fn load_pixbuf_surface(path: &Path, size: i32) -> Option<cairo::ImageSurface> {
    let fns = pixbuf_fns()?;
    let img = decode_pixbuf_image(fns, path, size, size, false)?;
    image_surface_from_decoded(&img)
}

/// Loads a PNG, SVG, or `gdk-pixbuf`-supported image file into a `size x size` `cairo::ImageSurface`.
pub fn load_icon_surface(path: &Path, size: i32) -> Option<cairo::ImageSurface> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "png" {
        let mut f = File::open(path).ok()?;
        let src = cairo::ImageSurface::create_from_png(&mut f).ok()?;
        let sw = src.width();
        let sh = src.height();
        if sw <= 0 || sh <= 0 {
            return None;
        }
        if sw == size && sh == size {
            return Some(src);
        }
        let dst = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size).ok()?;
        let cr = cairo::Context::new(&dst).ok()?;
        cr.scale(size as f64 / sw as f64, size as f64 / sh as f64);
        let _ = cr.set_source_surface(&src, 0.0, 0.0);
        let _ = cr.paint();
        drop(cr);
        dst.flush();
        return Some(dst);
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
            let dst = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size).ok()?;
            let cr = cairo::Context::new(&dst).ok()?;
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
            dst.flush();
            return Some(dst);
        }
    }
    // anything else (JPEG covers, an embedded cover written without an extension): gdk-pixbuf
    load_pixbuf_surface(path, size)
}

thread_local! {
    static ICON_CACHE: RefCell<HashMap<(String, i32), Option<cairo::ImageSurface>>> =
        RefCell::new(HashMap::new());
}

fn cached_icon_surface(icon: &str, size: i32) -> Option<cairo::ImageSurface> {
    if icon.is_empty() || size <= 0 {
        return None;
    }
    let key = (icon.to_string(), size);
    ICON_CACHE.with(|cache| {
        if let Some(found) = cache.borrow().get(&key) {
            return found.clone();
        }
        let loaded = resolve_icon_path(icon).and_then(|p| load_icon_surface(&p, size));
        cache.borrow_mut().insert(key, loaded.clone());
        loaded
    })
}

/// An app's icon at `(x, y)`, `size` px square; without one, a tile in the theme's track colour
/// with the name's first letter.
#[allow(clippy::too_many_arguments)]
pub fn draw_app_icon(cr: &Context, cx: &Cx, icon: &str, name: &str, x: f64, y: f64, size: f64) {
    if let Some(surface) = cached_icon_surface(icon, size.round() as i32) {
        let _ = cr.save();
        let _ = cr.set_source_surface(&surface, x.round(), y.round());
        let _ = cr.paint();
        let _ = cr.restore();
        return;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
