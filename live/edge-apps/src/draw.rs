//! Drawing helpers: cards, text, thin bars. Every module draws through these, so they all share
//! one style (a 15 px title, text and small line icons, thin bars, the accent only on what is on).

use crate::theme::{Rgba, Theme};
use cairo::Context;

pub const PAD_X: f64 = 18.0;
pub const PAD_Y: f64 = 16.0;
pub const GAP: f64 = 12.0;
pub const FONT: &str = "sans-serif";
pub const SIZE_NOTE: f64 = 11.0;
pub const SIZE_SMALL: f64 = 12.0;
pub const SIZE_BODY: f64 = 13.0;
pub const SIZE_TITLE: f64 = 15.0;

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
    let _ = cr.fill_preserve();
    cr.new_path();
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

fn layout(cr: &Context, t: &Text) -> pango::Layout {
    let l = pangocairo::functions::create_layout(cr);
    let mut fd = pango::FontDescription::from_string(FONT);
    fd.set_absolute_size(t.size * pango::SCALE as f64);
    fd.set_weight(if t.bold {
        pango::Weight::Semibold
    } else {
        pango::Weight::Normal
    });
    l.set_font_description(Some(&fd));
    l.set_text(t.text);
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

/// Text with its right edge at x.
pub fn text_right(cr: &Context, x: f64, y: f64, t: &Text) -> f64 {
    let (w, _) = measure(cr, t);
    text(cr, x - w, y, t);
    w
}

/// Text centred on cx.
pub fn text_center(cr: &Context, cx: f64, y: f64, t: &Text) {
    let (w, _) = measure(cr, t);
    text(cr, cx - w / 2.0, y, t);
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
/// A quiet hint in 12 px muted text, right-aligned on the title line (docs/design/EDGE_DESIGN.md).
/// Dropped if it would overlap `title` on the left when the card is narrow.
pub fn title_hint(
    cr: &Context,
    th: &Theme,
    x: f64,
    y: f64,
    w: f64,
    title: &str,
    hint: &str,
) -> f64 {
    if hint.is_empty() || w <= 0.0 {
        return 0.0;
    }
    let (tw, _) = measure(
        cr,
        &Text {
            size: SIZE_TITLE,
            bold: true,
            color: th.text,
            text: title,
        },
    );
    let (hw, _) = measure(
        cr,
        &Text {
            size: SIZE_SMALL,
            bold: false,
            color: th.muted,
            text: hint,
        },
    );
    // Left of hint is (x + w - PAD_X - hw); right of title is (x + PAD_X + tw).
    // Leave at least GAP (12 px) between title and hint.
    if tw + hw + GAP > w - 2.0 * PAD_X {
        return 0.0;
    }
    text_right(
        cr,
        x + w - PAD_X,
        y + PAD_Y + 2.0,
        &Text {
            size: SIZE_SMALL,
            bold: false,
            color: th.muted,
            text: hint,
        },
    );
    hw
}

#[cfg(test)]
mod hint_tests {
    use super::*;

    #[test]
    fn a_title_hint_is_drawn_where_it_fits_and_dropped_where_it_would_touch_the_title() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 60).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        assert!(title_hint(&cr, &th, 0.0, 0.0, 380.0, "Apps", "Recent first") > 0.0);
        assert_eq!(
            title_hint(
                &cr,
                &th,
                0.0,
                0.0,
                120.0,
                "Quick settings",
                "Right click: more"
            ),
            0.0
        );
        assert_eq!(title_hint(&cr, &th, 0.0, 0.0, 380.0, "Apps", ""), 0.0);
        assert_eq!(title(&cr, &th, 0.0, 0.0, "Apps"), 20.0);
        assert_eq!(
            [SIZE_NOTE, SIZE_SMALL, SIZE_BODY, SIZE_TITLE],
            [11.0, 12.0, 13.0, 15.0]
        );
    }
}
