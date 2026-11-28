//! Cairo/Pango drawing primitives and 16 px / 1.6 px line icons matching `live/edge-apps/src/draw.rs`
//! and `docs/mockups/file-picker.html`. Every colour comes from `Look`.

use crate::look::{Look, Rgba};
use cairo::Context;
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI};

pub const SIZE_NOTE: f64 = 11.0;
pub const SIZE_SMALL: f64 = 12.0;
pub const SIZE_BODY: f64 = 13.0;
pub const SIZE_TITLE: f64 = 15.0;
const DEFAULT_FONT: &str = "sans-serif";

type FontDescCache = (String, HashMap<(u64, bool), pango::FontDescription>);

thread_local! {
    static CURRENT_FONT: RefCell<String> = RefCell::new(DEFAULT_FONT.to_string());
    static FONT_DESCS: RefCell<FontDescCache> =
        RefCell::new((DEFAULT_FONT.to_string(), HashMap::new()));
}

struct FontGuard(Option<String>);

impl Drop for FontGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.0.take() {
            CURRENT_FONT.with(|f| *f.borrow_mut() = prev);
        }
    }
}

/// Runs `f` with the active typeface family set from the theme (falling back to sans-serif).
pub fn with_font<R>(family: &str, f: impl FnOnce() -> R) -> R {
    let target = {
        let trimmed = family.trim();
        if trimmed.is_empty() {
            DEFAULT_FONT
        } else {
            trimmed
        }
    };
    let prev = CURRENT_FONT.with(|cell| {
        let mut cur = cell.borrow_mut();
        if cur.as_str() == target {
            None
        } else {
            Some(std::mem::replace(&mut *cur, target.to_string()))
        }
    });
    let _guard = FontGuard(prev);
    f()
}

pub fn set(cr: &Context, c: Rgba) {
    cr.set_source_rgba(c.0, c.1, c.2, c.3);
}

pub fn round_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    if r <= 0.0 {
        cr.rectangle(x, y, w, h);
        return;
    }
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

pub fn stroke_round_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64, color: Rgba) {
    round_rect(
        cr,
        x.round() + 0.5,
        y.round() + 0.5,
        (w.round() - 1.0).max(0.0),
        (h.round() - 1.0).max(0.0),
        (r - 0.5).max(0.0),
    );
    set(cr, color);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

pub fn fill_round_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64, color: Rgba) {
    round_rect(cr, x.round(), y.round(), w.round(), h.round(), r);
    set(cr, color);
    let _ = cr.fill();
}

/// 1 px horizontal divider on the half pixel.
pub fn hline(cr: &Context, x: f64, y: f64, w: f64, color: Rgba) {
    set(cr, color);
    cr.set_line_width(1.0);
    let py = y.round() + 0.5;
    cr.move_to(x.round(), py);
    cr.line_to((x + w).round(), py);
    let _ = cr.stroke();
}

/// 1 px vertical divider on the half pixel.
pub fn vline(cr: &Context, x: f64, y: f64, h: f64, color: Rgba) {
    set(cr, color);
    cr.set_line_width(1.0);
    let px = x.round() + 0.5;
    cr.move_to(px, y.round());
    cr.line_to(px, (y + h).round());
    let _ = cr.stroke();
}

/// Thin 3 px usage bar matching `live/edge-apps/src/draw.rs`.
pub fn bar(cr: &Context, look: &Look, x: f64, y: f64, w: f64, part: f64, fill: Rgba) {
    round_rect(cr, x, y, w, 3.0, 1.5);
    set(cr, look.track);
    let _ = cr.fill();
    if part > 0.0 {
        round_rect(cr, x, y, (w * part.clamp(0.0, 1.0)).max(3.0), 3.0, 1.5);
        set(cr, fill);
        let _ = cr.fill();
    }
}

pub struct Text<'a> {
    pub size: f64,
    pub bold: bool,
    pub color: Rgba,
    pub text: &'a str,
}

fn with_font_desc<R>(size: f64, bold: bool, f: impl FnOnce(&pango::FontDescription) -> R) -> R {
    CURRENT_FONT.with(|family_cell| {
        let family = family_cell.borrow();
        FONT_DESCS.with(|cache_cell| {
            let mut cache = cache_cell.borrow_mut();
            if cache.0 != *family {
                cache.0.clone_from(&family);
                cache.1.clear();
            }
            let key = (size.to_bits(), bold);
            let fd = cache.1.entry(key).or_insert_with(|| {
                let mut fd = pango::FontDescription::from_string(&family);
                fd.set_absolute_size(size * pango::SCALE as f64);
                fd.set_weight(if bold {
                    pango::Weight::Semibold
                } else {
                    pango::Weight::Normal
                });
                fd
            });
            f(fd)
        })
    })
}

fn layout(cr: &Context, t: &Text) -> pango::Layout {
    let l = pangocairo::functions::create_layout(cr);
    with_font_desc(t.size, t.bold, |fd| l.set_font_description(Some(fd)));
    l.set_text(t.text);
    l
}

pub fn measure(cr: &Context, t: &Text) -> (f64, f64) {
    let (w, h) = layout(cr, t).pixel_size();
    (w as f64, h as f64)
}

pub fn text(cr: &Context, x: f64, y: f64, t: &Text) -> f64 {
    let l = layout(cr, t);
    set(cr, t.color);
    cr.move_to(x.round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
    l.pixel_size().0 as f64
}

pub fn text_max(cr: &Context, x: f64, y: f64, max_w: f64, t: &Text) -> f64 {
    let l = layout(cr, t);
    if max_w > 0.0 {
        l.set_width((max_w * pango::SCALE as f64) as i32);
        l.set_ellipsize(pango::EllipsizeMode::End);
    }
    set(cr, t.color);
    cr.move_to(x.round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
    l.pixel_size().0 as f64
}

pub fn text_right(cr: &Context, x: f64, y: f64, t: &Text) -> f64 {
    let l = layout(cr, t);
    let w = l.pixel_size().0 as f64;
    set(cr, t.color);
    cr.move_to((x - w).round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
    w
}

pub fn text_center(cr: &Context, cx: f64, y: f64, t: &Text) {
    let l = layout(cr, t);
    let w = l.pixel_size().0 as f64;
    set(cr, t.color);
    cr.move_to((cx - w * 0.5).round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
}

/// Draws multi-line wrapped text clamped to `(x, y, w, h)` in the current font.
pub fn text_wrapped_clipped(
    cr: &Context,
    bounds: (f64, f64, f64, f64),
    size: f64,
    color: Rgba,
    s: &str,
) {
    let (x, y, w, max_h) = bounds;
    let l = pangocairo::functions::create_layout(cr);
    with_font_desc(size, false, |fd| l.set_font_description(Some(fd)));
    l.set_width((w.max(1.0) * pango::SCALE as f64) as i32);
    l.set_wrap(pango::WrapMode::WordChar);
    l.set_text(s);
    let _ = cr.save();
    cr.rectangle(x.round(), y.round(), w.max(0.0), max_h.max(0.0));
    cr.clip();
    set(cr, color);
    cr.move_to(x.round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
    let _ = cr.restore();
}

/// Measures the x offset of byte index `byte_idx` inside a single-line string in the current font.
pub fn text_index_x(cr: &Context, size: f64, s: &str, byte_idx: usize) -> f64 {
    let clamped = byte_idx.min(s.len());
    let l = pangocairo::functions::create_layout(cr);
    with_font_desc(size, false, |fd| l.set_font_description(Some(fd)));
    l.set_text(s);
    let (strong, _) = l.cursor_pos(clamped as i32);
    strong.x() as f64 / pango::SCALE as f64
}

/// Maps a local x coordinate inside a single-line string back to the nearest UTF-8 byte boundary.
pub fn text_x_to_index(cr: &Context, size: f64, s: &str, local_x: f64) -> usize {
    let l = pangocairo::functions::create_layout(cr);
    with_font_desc(size, false, |fd| l.set_font_description(Some(fd)));
    l.set_text(s);
    let px = (local_x.max(0.0) * pango::SCALE as f64) as i32;
    let (_, idx, trailing) = l.xy_to_index(px, 0);
    let mut byte_idx = (idx.max(0) as usize).min(s.len());
    if trailing > 0 {
        if let Some(ch) = s[byte_idx..].chars().next() {
            byte_idx = (byte_idx + ch.len_utf8()).min(s.len());
        }
    }
    while byte_idx > 0 && !s.is_char_boundary(byte_idx) {
        byte_idx -= 1;
    }
    byte_idx
}

/// 16 px / 1.6 px stroke line icons matching `docs/mockups/file-picker.html`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Back,
    Forward,
    Up,
    Search,
    Preview,
    Hidden,
    NewFolder,
    Close,
    Clock,
    Home,
    Folder,
    Drive,
    Usb,
    File,
    Image,
    TextFile,
    ChevronDown,
    Check,
}

pub fn icon(cr: &Context, ic: Icon, x: f64, y: f64, color: Rgba) {
    cr.new_path();
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    set(cr, color);
    let (ox, oy) = (x.round(), y.round());
    match ic {
        Icon::Back => {
            cr.move_to(ox + 10.0, oy + 3.5);
            cr.line_to(ox + 5.5, oy + 8.0);
            cr.line_to(ox + 10.0, oy + 12.5);
            let _ = cr.stroke();
        }
        Icon::Forward => {
            cr.move_to(ox + 6.0, oy + 3.5);
            cr.line_to(ox + 10.5, oy + 8.0);
            cr.line_to(ox + 6.0, oy + 12.5);
            let _ = cr.stroke();
        }
        Icon::Up => {
            cr.move_to(ox + 8.0, oy + 12.5);
            cr.line_to(ox + 8.0, oy + 4.0);
            let _ = cr.stroke();
            cr.move_to(ox + 4.5, oy + 7.5);
            cr.line_to(ox + 8.0, oy + 4.0);
            cr.line_to(ox + 11.5, oy + 7.5);
            let _ = cr.stroke();
        }
        Icon::Search => {
            cr.new_sub_path();
            cr.arc(ox + 7.0, oy + 7.0, 4.5, 0.0, 2.0 * PI);
            let _ = cr.stroke();
            cr.move_to(ox + 10.5, oy + 10.5);
            cr.line_to(ox + 14.0, oy + 14.0);
            let _ = cr.stroke();
        }
        Icon::Preview => {
            round_rect(cr, ox + 2.0, oy + 3.0, 12.0, 10.0, 1.5);
            let _ = cr.stroke();
            cr.move_to(ox + 10.0, oy + 3.0);
            cr.line_to(ox + 10.0, oy + 13.0);
            let _ = cr.stroke();
        }
        Icon::Hidden => {
            cr.move_to(ox + 2.0, oy + 8.0);
            cr.curve_to(ox + 4.4, oy + 4.0, ox + 11.6, oy + 4.0, ox + 14.0, oy + 8.0);
            cr.curve_to(
                ox + 11.6,
                oy + 12.0,
                ox + 4.4,
                oy + 12.0,
                ox + 2.0,
                oy + 8.0,
            );
            let _ = cr.stroke();
            cr.new_sub_path();
            cr.arc(ox + 8.0, oy + 8.0, 1.8, 0.0, 2.0 * PI);
            let _ = cr.stroke();
        }
        Icon::NewFolder => {
            draw_folder_outline(cr, ox, oy);
            cr.move_to(ox + 8.0, oy + 7.6);
            cr.line_to(ox + 8.0, oy + 11.4);
            cr.move_to(ox + 6.1, oy + 9.5);
            cr.line_to(ox + 9.9, oy + 9.5);
            let _ = cr.stroke();
        }
        Icon::Close => {
            cr.move_to(ox + 4.0, oy + 4.0);
            cr.line_to(ox + 12.0, oy + 12.0);
            cr.move_to(ox + 12.0, oy + 4.0);
            cr.line_to(ox + 4.0, oy + 12.0);
            let _ = cr.stroke();
        }
        Icon::Clock => {
            cr.new_sub_path();
            cr.arc(ox + 8.0, oy + 8.0, 5.5, 0.0, 2.0 * PI);
            let _ = cr.stroke();
            cr.move_to(ox + 8.0, oy + 5.0);
            cr.line_to(ox + 8.0, oy + 8.2);
            cr.line_to(ox + 10.2, oy + 9.5);
            let _ = cr.stroke();
        }
        Icon::Home => {
            cr.move_to(ox + 2.5, oy + 7.5);
            cr.line_to(ox + 8.0, oy + 3.0);
            cr.line_to(ox + 13.5, oy + 7.5);
            let _ = cr.stroke();
            cr.move_to(ox + 4.2, oy + 6.8);
            cr.line_to(ox + 4.2, oy + 13.0);
            cr.line_to(ox + 11.8, oy + 13.0);
            cr.line_to(ox + 11.8, oy + 6.8);
            let _ = cr.stroke();
        }
        Icon::Folder => {
            draw_folder_outline(cr, ox, oy);
        }
        Icon::Drive => {
            round_rect(cr, ox + 1.5, oy + 3.5, 13.0, 9.0, 1.5);
            let _ = cr.stroke();
            cr.move_to(ox + 4.0, oy + 9.5);
            cr.line_to(ox + 8.5, oy + 9.5);
            let _ = cr.stroke();
            cr.new_sub_path();
            cr.arc(ox + 11.5, oy + 9.5, 0.8, 0.0, 2.0 * PI);
            let _ = cr.fill();
        }
        Icon::Usb => {
            round_rect(cr, ox + 1.5, oy + 4.5, 9.0, 7.0, 1.0);
            let _ = cr.stroke();
            cr.rectangle(ox + 10.5, oy + 5.5, 4.0, 5.0);
            let _ = cr.stroke();
        }
        Icon::File => {
            draw_doc_outline(cr, ox, oy);
        }
        Icon::Image => {
            round_rect(cr, ox + 2.0, oy + 3.0, 12.0, 10.0, 1.5);
            let _ = cr.stroke();
            cr.new_sub_path();
            cr.arc(ox + 5.5, oy + 6.2, 1.0, 0.0, 2.0 * PI);
            let _ = cr.fill();
            cr.move_to(ox + 3.0, oy + 11.5);
            cr.line_to(ox + 6.5, oy + 8.5);
            cr.line_to(ox + 9.0, oy + 10.5);
            cr.line_to(ox + 11.0, oy + 8.8);
            cr.line_to(ox + 13.0, oy + 11.5);
            let _ = cr.stroke();
        }
        Icon::TextFile => {
            draw_doc_outline(cr, ox, oy);
            cr.move_to(ox + 5.5, oy + 8.5);
            cr.line_to(ox + 10.5, oy + 8.5);
            cr.move_to(ox + 5.5, oy + 11.0);
            cr.line_to(ox + 9.0, oy + 11.0);
            let _ = cr.stroke();
        }
        Icon::ChevronDown => {
            cr.move_to(ox + 4.5, oy + 6.5);
            cr.line_to(ox + 8.0, oy + 10.0);
            cr.line_to(ox + 11.5, oy + 6.5);
            let _ = cr.stroke();
        }
        Icon::Check => {
            cr.move_to(ox + 3.5, oy + 8.5);
            cr.line_to(ox + 6.5, oy + 11.5);
            cr.line_to(ox + 12.5, oy + 5.0);
            let _ = cr.stroke();
        }
    }
}

fn draw_folder_outline(cr: &Context, ox: f64, oy: f64) {
    cr.move_to(ox + 2.0, oy + 4.5);
    cr.line_to(ox + 6.0, oy + 4.5);
    cr.line_to(ox + 7.4, oy + 6.0);
    cr.line_to(ox + 14.0, oy + 6.0);
    cr.line_to(ox + 14.0, oy + 12.5);
    cr.line_to(ox + 2.0, oy + 12.5);
    cr.close_path();
    let _ = cr.stroke();
}

fn draw_doc_outline(cr: &Context, ox: f64, oy: f64) {
    cr.move_to(ox + 3.5, oy + 2.5);
    cr.line_to(ox + 9.5, oy + 2.5);
    cr.line_to(ox + 12.5, oy + 5.5);
    cr.line_to(ox + 12.5, oy + 13.5);
    cr.line_to(ox + 3.5, oy + 13.5);
    cr.close_path();
    let _ = cr.stroke();
    cr.move_to(ox + 9.5, oy + 2.5);
    cr.line_to(ox + 9.5, oy + 5.5);
    cr.line_to(ox + 12.5, oy + 5.5);
    let _ = cr.stroke();
}
