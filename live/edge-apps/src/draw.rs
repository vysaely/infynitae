//! Drawing helpers: cards, text, thin bars. Every module draws through these, so they all share
//! one style (a 15 px title, text and small line icons, thin bars, the accent only on what is on).

use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::cell::RefCell;
use std::collections::HashMap;

pub const PAD_X: f64 = 18.0;
pub const PAD_Y: f64 = 16.0;
pub const GAP: f64 = 12.0;
const DEFAULT_FONT: &str = "sans-serif";
pub const SIZE_NOTE: f64 = 11.0;
pub const SIZE_SMALL: f64 = 12.0;
pub const SIZE_BODY: f64 = 13.0;
pub const SIZE_TITLE: f64 = 15.0;

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
            let old = std::mem::replace(&mut *cur, target.to_string());
            Some(old)
        }
    });
    let _guard = FontGuard(prev);
    f()
}

pub fn set(cr: &Context, c: Rgba) {
    cr.set_source_rgba(c.0, c.1, c.2, c.3);
}

/// A rectangle with rounded corners, its edges on whole pixels (a 1 px line on the half pixel).
pub fn round_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    if r <= 0.0 {
        cr.rectangle(x, y, w, h);
        return;
    }
    use std::f64::consts::{FRAC_PI_2, PI};
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

/// A module's card: its fill and its 1 px edge.
pub fn card(cr: &Context, t: &Theme, x: f64, y: f64, w: f64, h: f64) {
    round_rect(cr, x.round(), y.round(), w.round(), h.round(), t.corners);
    set(cr, t.card);
    let _ = cr.fill();
    round_rect(
        cr,
        x.round() + 0.5,
        y.round() + 0.5,
        w.round() - 1.0,
        h.round() - 1.0,
        (t.corners - 0.5).max(0.0),
    );
    set(cr, t.edge);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

pub struct Text<'a> {
    pub size: f64,
    pub bold: bool,
    pub color: Rgba,
    pub text: &'a str,
}

/// Tests that sort pixels by colour: text edges in grey, whatever the machine's font smoothing.
#[cfg(test)]
pub fn grey_text_edges(cr: &Context) {
    let mut fo = cairo::FontOptions::new().unwrap();
    fo.set_antialias(cairo::Antialias::Gray);
    cr.set_font_options(&fo);
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

/// A wrapped Pango layout in the current theme font, clamped to `max_lines` with an ellipsis when given.
pub fn wrapped_layout(
    cr: &Context,
    size: f64,
    bold: bool,
    max_w: f64,
    max_lines: Option<usize>,
    text: &str,
) -> pango::Layout {
    let l = pangocairo::functions::create_layout(cr);
    with_font_desc(size, bold, |fd| l.set_font_description(Some(fd)));
    l.set_width((max_w * pango::SCALE as f64) as i32);
    l.set_wrap(pango::WrapMode::WordChar);
    if let Some(lines) = max_lines {
        l.set_ellipsize(pango::EllipsizeMode::End);
        l.set_height(-(lines as i32));
    }
    l.set_text(text);
    l
}

/// Text's width and height in px.
pub fn measure(cr: &Context, t: &Text) -> (f64, f64) {
    let (w, h) = layout(cr, t).pixel_size();
    (w as f64, h as f64)
}

/// Text with its top left at (x, y), on whole pixels; returns its width.
pub fn text(cr: &Context, x: f64, y: f64, t: &Text) -> f64 {
    let l = layout(cr, t);
    set(cr, t.color);
    cr.move_to(x.round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
    l.pixel_size().0 as f64
}

/// Text with its top left at (x, y), cut with an ellipsis past `max_w` when `max_w > 0`.
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

fn show_aligned(cr: &Context, x: f64, y: f64, t: &Text, align_factor: f64) -> f64 {
    let l = layout(cr, t);
    let w = l.pixel_size().0 as f64;
    set(cr, t.color);
    cr.move_to((x - w * align_factor).round(), y.round());
    pangocairo::functions::show_layout(cr, &l);
    w
}

/// Text with its right edge at x.
pub fn text_right(cr: &Context, x: f64, y: f64, t: &Text) -> f64 {
    show_aligned(cr, x, y, t, 1.0)
}

/// Text centred on cx.
pub fn text_center(cr: &Context, cx: f64, y: f64, t: &Text) {
    show_aligned(cr, cx, y, t, 0.5);
}

/// A thin bar (3 px): its track and the filled part, `part` 0..1.
pub fn bar(cr: &Context, th: &Theme, x: f64, y: f64, w: f64, part: f64, fill: Rgba) {
    round_rect(cr, x, y, w, 3.0, 1.5);
    set(cr, th.track);
    let _ = cr.fill();
    if part > 0.0 {
        round_rect(cr, x, y, (w * part.clamp(0.0, 1.0)).max(3.0), 3.0, 1.5);
        set(cr, fill);
        let _ = cr.fill();
    }
}

/// A module's title in 15 px semibold, at the top left of its content.
pub fn title(cr: &Context, th: &Theme, x: f64, y: f64, s: &str) -> f64 {
    text(
        cr,
        x,
        y,
        &Text {
            size: SIZE_TITLE,
            bold: true,
            color: th.text,
            text: s,
        },
    );
    20.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_font_changes_measured_and_wrapped_text_width() {
        let t_sans = Theme::from_config(&crate::config::Config::from_text(
            "FONT_NAME=\"sans-serif\"\n",
        ));
        let t_mono = Theme::from_config(&crate::config::Config::from_text(
            "FONT_NAME=\"monospace\"\n",
        ));
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 80).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let sample = Text {
            size: SIZE_BODY,
            bold: false,
            color: t_sans.text,
            text: "iiiiiiiiiiiiiiiiiiii",
        };
        let (w_sans, _) = with_font(&t_sans.font, || measure(&cr, &sample));
        let (w_mono, _) = with_font(&t_mono.font, || measure(&cr, &sample));
        assert_ne!(
            w_sans, w_mono,
            "different installed font families should measure differently"
        );
        let wrap_sans = with_font(&t_sans.font, || {
            wrapped_layout(&cr, SIZE_SMALL, false, 400.0, None, "iiiiiiiiiiiiiiiiiiii")
                .pixel_size()
                .0
        });
        let wrap_mono = with_font(&t_mono.font, || {
            wrapped_layout(&cr, SIZE_SMALL, false, 400.0, None, "iiiiiiiiiiiiiiiiiiii")
                .pixel_size()
                .0
        });
        assert_ne!(wrap_sans, wrap_mono);
    }
}
