//! The accent colour picker: its layout, drawing, colour conversions and the commands it writes.

use super::drawing::draw_style_row;
use super::sources::{normalize_hex, rgb_to_hex};
use super::*;

impl LookCard {
    pub(super) fn color_ask_command(&self, r: &LookRow, cur: &str) -> String {
        let old_line = self.config.as_ref().and_then(|d| d.now.get(r.key).cloned());
        let set = change_command(r.key, old_line.as_deref(), "@@V@@", r.after)
            .replace("'@@V@@'", "\"$v\"");
        format!(
            "v=$(\"$HOME/.local/bin/infynitae-edge\" ask --text --default {} {}) || exit 0; \
             bd=\"${{XDG_RUNTIME_DIR:-$HOME/.cache}}/infynitae/look-bad\"; \
             case \"$v\" in \
             \"#\"[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) \
             rm -f \"$bd\"/{k}; {set} ;; \
             *) mkdir -p \"$bd\" && : > \"$bd\"/{k} ;; \
             esac",
            sq(cur),
            sq(r.name),
            k = sq(r.key),
        )
    }

    pub(super) fn sync_accent_hsv(&mut self) {
        let cur = self.key_value("ACCENT");
        if let Some(rgb) = parse_hex(&cur) {
            self.accent_hsv = rgb_to_hsv(rgb);
        }
    }

    pub(super) fn open_accent_pick(&mut self, i: usize) {
        self.wp_mode_open = false;
        self.accent_drag = None;
        self.accent_preview = None;
        if self.pick == Some(i) {
            self.list.toggle();
            return;
        }
        self.pick = Some(i);
        self.list = ui::List::default();
        self.list.toggle();
        self.list_scroll = 0;
        self.sync_accent_hsv();
        let has_cand = self
            .look
            .as_ref()
            .is_some_and(|l| !l.accent_candidates.is_empty());
        self.accent_tab = if has_cand {
            AccentTab::Wallpaper
        } else {
            AccentTab::AnyColor
        };
    }

    pub(super) fn set_accent(&mut self, new_hex: String, out: &mut Vec<Action>) -> bool {
        if let Some(rgb) = parse_hex(&new_hex) {
            self.accent_hsv = rgb_to_hsv(rgb);
        }
        let cur = self.value(ACCENT_ROW);
        if cur.eq_ignore_ascii_case(&new_hex) {
            return false;
        }
        self.set(ACCENT_ROW, new_hex, out)
    }

    pub(super) fn recent_colors(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(cur) = normalize_hex(&self.value(ACCENT_ROW)) {
            out.push(cur);
        }
        if let Some(look) = &self.look {
            for hex in look.recent_accents.iter().take(MAX_RECENT_LOG) {
                if !out.contains(hex) {
                    out.push(hex.clone());
                }
            }
        }
        out
    }

    pub(super) fn accent_adjusted_warning(&self, cx: &Cx) -> bool {
        let cur = self.value(ACCENT_ROW);
        let Some(rgb) = normalize_hex(&cur).and_then(|h| parse_hex(&h)) else {
            return false;
        };
        let scheme = self.key_value("THEME_SCHEME");
        let bg = if scheme.is_empty() || scheme.eq_ignore_ascii_case("yours") {
            normalize_hex(&self.key_value("SURFACE_COLOR"))
                .and_then(|h| parse_hex(&h))
                .map(Rgba::rgb)
                .unwrap_or(cx.theme.card)
        } else if scheme.eq_ignore_ascii_case("monochrome") {
            // Monochrome uses a fixed accent independent of ACCENT.
            return false;
        } else {
            cx.theme.card
        };
        !theme::is_readable(Rgba::rgb(rgb), bg)
    }

    pub(super) fn accent_picker_full_h(&self, cx: &Cx, width: f64) -> f64 {
        let iw = (width - 16.0).max(40.0);
        self.accent_picker_layout(cx, 0.0, 0.0, iw).total_h
    }

    pub(super) fn accent_picker_layout(
        &self,
        cx: &Cx,
        ix: f64,
        top_y: f64,
        iw: f64,
    ) -> AccentPickerLayout {
        let mut y = top_y + 8.0;
        let tabs_r = (ix, y, iw, 30.0);
        y += 30.0 + 10.0;

        let cand_len = self.look.as_ref().map_or(0, |l| l.accent_candidates.len());
        let has_wp_bin = self.row_visible(&self.table[WP_MODE_ROW]);

        let mut no_cand_y = None;
        let mut pic_r = None;
        let mut cand_sq_y = None;
        let mut shuffle_line_y = None;
        let mut wp_mode_list_r = None;
        let mut hue_r = None;
        let mut sv_r = None;
        let mut code_line_y = None;
        let mut recent_label_y = None;
        let mut recent_sq_y = None;

        match self.accent_tab {
            AccentTab::Wallpaper => {
                if cand_len == 0 {
                    no_cand_y = Some(y);
                    y += 20.0 + 10.0;
                } else {
                    let pic_h = (iw * 10.0 / 16.0).round().max(40.0);
                    pic_r = Some((ix, y, iw, pic_h));
                    y += pic_h + 10.0;

                    cand_sq_y = Some(y);
                    let cols = ((iw + CAND_SQ_GAP) / (CAND_SQ_SIZE + CAND_SQ_GAP))
                        .floor()
                        .max(1.0) as usize;
                    let sq_rows = cand_len.div_ceil(cols);
                    let sq_h = sq_rows as f64 * CAND_SQ_SIZE
                        + sq_rows.saturating_sub(1) as f64 * CAND_SQ_GAP;
                    y += sq_h + 10.0;

                    if has_wp_bin {
                        shuffle_line_y = Some(y);
                        y += 32.0 + 6.0;
                        if self.wp_mode_open {
                            let lh = WP_MODE_OPTS.len() as f64 * ui::LIST_ROW_H + 8.0;
                            wp_mode_list_r = Some((ix, y, iw, lh));
                            y += lh + 6.0;
                        }
                    }
                }
            }
            AccentTab::AnyColor => {
                let hr = (ix, y, iw, HUE_STRIP_H);
                hue_r = Some(hr);
                y += HUE_STRIP_H + 10.0;

                let sr = (ix, y, iw, SV_SQUARE_H);
                sv_r = Some(sr);
                y += SV_SQUARE_H + 10.0;

                code_line_y = Some(y);
                y += 30.0 + 10.0;

                let recent_len = self.recent_colors().len();
                if recent_len > 0 {
                    recent_label_y = Some(y);
                    y += 20.0;
                    recent_sq_y = Some(y);
                    let cols = ((iw + RECENT_SQ_GAP) / (RECENT_SQ_SIZE + RECENT_SQ_GAP))
                        .floor()
                        .max(1.0) as usize;
                    let r_rows = recent_len.div_ceil(cols);
                    let rh = r_rows as f64 * RECENT_SQ_SIZE
                        + r_rows.saturating_sub(1) as f64 * RECENT_SQ_GAP;
                    y += rh + 10.0;
                }
            }
        }

        let style_label_y = y;
        y += 20.0;
        let style_y = y;
        let style_h = if cx.size == Size::S {
            28.0 * 2.0 + 6.0
        } else {
            28.0
        };
        y += style_h + 8.0;

        let warn_y = if self.accent_adjusted_warning(cx) {
            let wy = y;
            y += 20.0 + 4.0;
            Some(wy)
        } else {
            None
        };

        y += 8.0;
        AccentPickerLayout {
            tabs_r,
            no_cand_y,
            pic_r,
            cand_sq_y,
            shuffle_line_y,
            wp_mode_list_r,
            hue_r,
            sv_r,
            code_line_y,
            recent_label_y,
            recent_sq_y,
            style_label_y,
            style_y,
            warn_y,
            total_h: y - top_y,
        }
    }

    pub(super) fn draw_accent_picker(&self, cr: &Context, cx: &Cx, hits: &mut Hits, lr: Rect) {
        ui::list_box(cr, cx, lr);
        let _ = cr.save();
        cr.rectangle(lr.0, lr.1, lr.2, lr.3);
        cr.clip();
        let iw = (lr.2 - 16.0).max(40.0);
        let ix = lr.0 + 8.0;
        let lay = self.accent_picker_layout(cx, ix, lr.1, iw);
        *self.hue_rect.borrow_mut() = lay.hue_r;
        *self.sv_rect.borrow_mut() = lay.sv_r;

        ui::choice(
            cr,
            cx,
            hits,
            H_TAB_WP,
            lay.tabs_r,
            &["From your wallpaper", "Any colour"],
            usize::from(self.accent_tab == AccentTab::AnyColor),
        );

        let cur_hex = normalize_hex(&self.value(ACCENT_ROW));

        if let Some(ny) = lay.no_cand_y {
            draw::text_max(
                cr,
                ix,
                ny + 2.0,
                iw,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "No colours found in the wallpaper",
                },
            );
        }

        if let Some(pic_r) = lay.pic_r {
            let _ = cr.save();
            draw::round_rect(cr, pic_r.0, pic_r.1, pic_r.2, pic_r.3, THUMB_RADIUS);
            cr.clip();
            if let Some(surf) = self
                .look
                .as_ref()
                .and_then(|l| l.accent_thumb.as_ref())
                .and_then(icons::surface_for_decoded)
            {
                let sw = surf.width() as f64;
                let sh = surf.height() as f64;
                cr.translate(pic_r.0, pic_r.1);
                cr.scale(pic_r.2 / sw, pic_r.3 / sh);
                let _ = cr.set_source_surface(&surf, 0.0, 0.0);
                let _ = cr.paint();
            } else {
                draw::set(cr, cx.theme.track);
                let _ = cr.paint();
            }
            let _ = cr.restore();

            draw::round_rect(
                cr,
                pic_r.0 + 0.5,
                pic_r.1 + 0.5,
                (pic_r.2 - 1.0).max(1.0),
                (pic_r.3 - 1.0).max(1.0),
                THUMB_RADIUS,
            );
            draw::set(cr, cx.theme.edge);
            cr.set_line_width(1.0);
            let _ = cr.stroke();

            if let Some(cands) = self.look.as_ref().map(|l| &l.accent_candidates) {
                let margin = (ACTIVE_MARK_RADIUS + 1.0)
                    .min(pic_r.2 * 0.25)
                    .min(pic_r.3 * 0.25);
                let bounds = (
                    pic_r.0 + margin,
                    pic_r.1 + margin,
                    (pic_r.2 - 2.0 * margin).max(0.0),
                    (pic_r.3 - 2.0 * margin).max(0.0),
                );
                let mut pts: Vec<(f64, f64)> = cands
                    .iter()
                    .map(|c| {
                        (
                            (pic_r.0 + c.x * pic_r.2).clamp(bounds.0, bounds.0 + bounds.2),
                            (pic_r.1 + c.y * pic_r.3).clamp(bounds.1, bounds.1 + bounds.3),
                        )
                    })
                    .collect();
                nudge_marks(&mut pts, bounds, MARK_MIN_DIST);

                for active_pass in [false, true] {
                    for (idx, c) in cands.iter().enumerate() {
                        let active = cur_hex.as_deref() == Some(c.hex.as_str());
                        if active != active_pass {
                            continue;
                        }
                        let (mx, my) = pts[idx];
                        let rad = if active {
                            ACTIVE_MARK_RADIUS
                        } else {
                            MARK_RADIUS
                        };
                        let id = H_MARK_BASE + idx as u32;
                        let hit_r = (
                            mx - rad - 1.0,
                            my - rad - 1.0,
                            rad * 2.0 + 2.0,
                            rad * 2.0 + 2.0,
                        );
                        let _ = hits.add(id, hit_r) || hits.selected(id);
                        cr.new_sub_path();
                        cr.arc(mx, my, rad, 0.0, 2.0 * std::f64::consts::PI);
                        draw::set(cr, Rgba::rgb(c.rgb));
                        let _ = cr.fill_preserve();
                        // Thin card backing keeps the active accent ring visible on matching patches.
                        draw::set(cr, cx.theme.card.alpha(0.95));
                        cr.set_line_width(3.5);
                        let _ = cr.stroke_preserve();
                        draw::set(
                            cr,
                            if active {
                                cx.theme.accent
                            } else {
                                cx.theme.text
                            },
                        );
                        cr.set_line_width(2.0);
                        let _ = cr.stroke();
                    }
                }
            }
        }

        if let (Some(sq_y), Some(cands)) = (
            lay.cand_sq_y,
            self.look.as_ref().map(|l| &l.accent_candidates),
        ) {
            let cols = ((iw + CAND_SQ_GAP) / (CAND_SQ_SIZE + CAND_SQ_GAP))
                .floor()
                .max(1.0) as usize;
            for (idx, c) in cands.iter().enumerate() {
                let col = idx % cols;
                let row = idx / cols;
                let r = (
                    ix + col as f64 * (CAND_SQ_SIZE + CAND_SQ_GAP),
                    sq_y + row as f64 * (CAND_SQ_SIZE + CAND_SQ_GAP),
                    CAND_SQ_SIZE,
                    CAND_SQ_SIZE,
                );
                let active = cur_hex.as_deref() == Some(c.hex.as_str());
                draw_color_square(cr, cx, hits, H_CAND_SQ_BASE + idx as u32, r, c.rgb, active);
            }
        }

        if let Some(sy) = lay.shuffle_line_y {
            let btn_w = ui::button(cr, cx, hits, H_SHUFFLE, ix, sy + 3.0, "Shuffle");
            let mx = ix + btn_w + 10.0;
            let mw = (iw - btn_w - 10.0).max(40.0);
            let mode_r = (mx, sy, mw, 32.0);
            let lit = hits.add(H_WP_MODE_BOX, mode_r) || hits.selected(H_WP_MODE_BOX);
            let word = self.display_word(WP_MODE_ROW);
            let vbox_w = ui::value_box(cr, cx, mode_r, ix + iw, &word, lit, self.wp_mode_open);
            draw::text_max(
                cr,
                mx,
                sy + (32.0 - 16.0) / 2.0,
                (mw - vbox_w - 8.0).max(20.0),
                &Text {
                    size: 13.0,
                    bold: false,
                    color: cx.theme.text,
                    text: self.table[WP_MODE_ROW].name,
                },
            );
        }

        if let Some(mlr) = lay.wp_mode_list_r {
            ui::list_box(cr, cx, mlr);
            let entries = self.list_entries(WP_MODE_ROW);
            for (idx, e) in entries.iter().enumerate() {
                let rr = (
                    mlr.0 + 4.0,
                    mlr.1 + 4.0 + idx as f64 * ui::LIST_ROW_H,
                    mlr.2 - 8.0,
                    ui::LIST_ROW_H,
                );
                ui::list_row(
                    cr,
                    cx,
                    hits,
                    H_WP_MODE_OPT_BASE + idx as u32,
                    rr,
                    &e.label,
                    "",
                    e.active,
                    None,
                );
            }
        }

        if let Some(hr) = lay.hue_r {
            draw_hue_strip(cr, cx, hits, H_HUE, hr, self.accent_hsv.0);
        }

        if let Some(sr) = lay.sv_r {
            draw_sv_square(cr, cx, hits, H_SV, sr, self.accent_hsv);
        }

        if let Some(cy) = lay.code_line_y {
            let box_w = 104.0_f64.min(iw * 0.55);
            let box_r = (ix + iw - box_w, cy + 2.0, box_w, 26.0);
            let lit = hits.add(H_CODE_ASK, box_r) || hits.selected(H_CODE_ASK);
            draw::round_rect(
                cr,
                box_r.0 + 0.5,
                box_r.1 + 0.5,
                box_r.2 - 1.0,
                box_r.3 - 1.0,
                cx.theme.small_radius(),
            );
            draw::set(
                cr,
                if lit {
                    cx.theme.text.alpha(ui::LIGHT_ALPHA)
                } else {
                    cx.theme.text.alpha(ui::BOX_ALPHA)
                },
            );
            let _ = cr.fill_preserve();
            draw::set(
                cr,
                if lit {
                    cx.theme.text.alpha(0.28)
                } else {
                    cx.theme.edge
                },
            );
            cr.set_line_width(1.0);
            let _ = cr.stroke();

            let code_label = self.value(ACCENT_ROW);
            let code_text = if code_label.is_empty() {
                "Not set"
            } else {
                &code_label
            };
            draw::text_center(
                cr,
                box_r.0 + box_r.2 / 2.0,
                box_r.1 + 5.0,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: if code_label.is_empty() {
                        cx.theme.muted
                    } else {
                        cx.theme.text
                    },
                    text: code_text,
                },
            );

            let bad = self.is_bad_color(&self.table[ACCENT_ROW]);
            let left_w = (iw - box_w - 8.0).max(20.0);
            if bad {
                let bw = draw::text_right(
                    cr,
                    box_r.0 - 8.0,
                    cy + 7.0,
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: cx.theme.alert,
                        text: "Not a colour",
                    },
                );
                draw::text_max(
                    cr,
                    ix,
                    cy + 7.0,
                    (left_w - bw - 8.0).max(20.0),
                    &Text {
                        size: 13.0,
                        bold: false,
                        color: cx.theme.text,
                        text: "Code",
                    },
                );
            } else {
                draw::text_max(
                    cr,
                    ix,
                    cy + 7.0,
                    left_w,
                    &Text {
                        size: 13.0,
                        bold: false,
                        color: cx.theme.text,
                        text: "Code",
                    },
                );
            }
        }

        if let (Some(ry), Some(sq_y)) = (lay.recent_label_y, lay.recent_sq_y) {
            draw::text(
                cr,
                ix,
                ry,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "Recent",
                },
            );
            let recent = self.recent_colors();
            let cols = ((iw + RECENT_SQ_GAP) / (RECENT_SQ_SIZE + RECENT_SQ_GAP))
                .floor()
                .max(1.0) as usize;
            for (idx, hex) in recent.iter().enumerate() {
                if let Some(rgb) = parse_hex(hex) {
                    let col = idx % cols;
                    let row = idx / cols;
                    let r = (
                        ix + col as f64 * (RECENT_SQ_SIZE + RECENT_SQ_GAP),
                        sq_y + row as f64 * (RECENT_SQ_SIZE + RECENT_SQ_GAP),
                        RECENT_SQ_SIZE,
                        RECENT_SQ_SIZE,
                    );
                    let active = cur_hex.as_deref() == Some(hex.as_str());
                    draw_color_square(cr, cx, hits, H_RECENT_BASE + idx as u32, r, rgb, active);
                }
            }
        }

        draw::text(
            cr,
            ix,
            lay.style_label_y,
            &Text {
                size: 12.0,
                bold: false,
                color: cx.theme.muted,
                text: "Style",
            },
        );
        let scheme = self.value(SCHEME_ROW);
        let scheme_idx = SCHEME_OPTS
            .iter()
            .position(|(v, _)| v.eq_ignore_ascii_case(&scheme));
        if cx.size == Size::S {
            draw_style_row(
                cr,
                cx,
                hits,
                H_SCHEME_BASE,
                (ix, lay.style_y, iw, 28.0),
                &SCHEME_OPTS[..3],
                scheme_idx,
            );
            draw_style_row(
                cr,
                cx,
                hits,
                H_SCHEME_BASE + 3,
                (ix, lay.style_y + 34.0, iw, 28.0),
                &SCHEME_OPTS[3..],
                scheme_idx.and_then(|s| s.checked_sub(3)),
            );
        } else {
            draw_style_row(
                cr,
                cx,
                hits,
                H_SCHEME_BASE,
                (ix, lay.style_y, iw, 28.0),
                SCHEME_OPTS,
                scheme_idx,
            );
        }

        if let Some(wy) = lay.warn_y {
            draw::text_max(
                cr,
                ix,
                wy + 2.0,
                iw,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "Text on the accent is adjusted to stay readable",
                },
            );
        }

        let _ = cr.restore();
    }
}

pub(super) struct AccentPickerLayout {
    tabs_r: Rect,
    no_cand_y: Option<f64>,
    pic_r: Option<Rect>,
    cand_sq_y: Option<f64>,
    shuffle_line_y: Option<f64>,
    wp_mode_list_r: Option<Rect>,
    hue_r: Option<Rect>,
    sv_r: Option<Rect>,
    code_line_y: Option<f64>,
    recent_label_y: Option<f64>,
    recent_sq_y: Option<f64>,
    style_label_y: f64,
    style_y: f64,
    warn_y: Option<f64>,
    total_h: f64,
}

/// Converts sRGB bytes `[r, g, b]` to HSV `(h in 0..360, s in 0..=1, v in 0..=1)`.
pub fn rgb_to_hsv(rgb: [u8; 3]) -> (f64, f64, f64) {
    let r = rgb[0] as f64 / 255.0;
    let g = rgb[1] as f64 / 255.0;
    let b = rgb[2] as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    let s = if max == 0.0 { 0.0 } else { d / max };
    let h = if d == 0.0 {
        0.0
    } else if (max - r).abs() < 1e-12 {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if (max - g).abs() < 1e-12 {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, v)
}

/// Converts HSV `(h in degrees, s in 0..=1, v in 0..=1)` to sRGB bytes `[r, g, b]`.
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> [u8; 3] {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0).floor() as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((g + m).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((b + m).clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

pub(super) fn hsv_to_hex(h: f64, s: f64, v: f64) -> String {
    rgb_to_hex(hsv_to_rgb(h, s, v))
}

/// Nudges any pair of candidate marks closer than `min_dist` pixels apart within `bounds`.
pub fn nudge_marks(pts: &mut [(f64, f64)], bounds: Rect, min_dist: f64) {
    if pts.len() < 2 || min_dist <= 0.0 {
        return;
    }
    let min_x = bounds.0;
    let min_y = bounds.1;
    let max_x = bounds.0 + bounds.2.max(0.0);
    let max_y = bounds.1 + bounds.3.max(0.0);
    for _pass in 0..48 {
        let mut moved = false;
        for i in 0..pts.len() {
            for j in (i + 1)..pts.len() {
                let dx = pts[j].0 - pts[i].0;
                let dy = pts[j].1 - pts[i].1;
                let d = dx.hypot(dy);
                if d + 1e-6 >= min_dist {
                    continue;
                }
                let (ux, uy) = if d > 1e-6 {
                    (dx / d, dy / d)
                } else {
                    let angle = (i + j) as f64 * 2.399_963;
                    (angle.cos(), angle.sin())
                };
                let push = 0.5 * (min_dist - d) + 0.05;
                pts[i].0 -= ux * push;
                pts[i].1 -= uy * push;
                pts[j].0 += ux * push;
                pts[j].1 += uy * push;
                moved = true;
            }
        }
        for p in pts.iter_mut() {
            p.0 = p.0.clamp(min_x, max_x);
            p.1 = p.1.clamp(min_y, max_y);
        }
        if !moved {
            break;
        }
    }
}

pub(super) fn draw_color_square(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    id: u32,
    r: Rect,
    rgb: [u8; 3],
    active: bool,
) {
    let lit = hits.add(id, r) || hits.selected(id);
    draw::round_rect(cr, r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, THUMB_RADIUS);
    draw::set(cr, Rgba::rgb(rgb));
    let _ = cr.fill_preserve();
    if active {
        draw::set(cr, cx.theme.text);
        cr.set_line_width(2.0);
    } else if lit {
        draw::set(cr, cx.theme.text.alpha(0.55));
        cr.set_line_width(1.5);
    } else {
        draw::set(cr, cx.theme.edge);
        cr.set_line_width(1.0);
    }
    let _ = cr.stroke();
}

pub(super) fn draw_hue_strip(cr: &Context, cx: &Cx, hits: &mut Hits, id: u32, r: Rect, hue: f64) {
    let _ = hits.add(id, r) || hits.selected(id);
    hits.mark_draggable(id);
    let pat = cairo::LinearGradient::new(r.0, r.1, r.0 + r.2, r.1);
    let stops = [
        (0.0 / 6.0, (1.0, 0.0, 0.0)),
        (1.0 / 6.0, (1.0, 1.0, 0.0)),
        (2.0 / 6.0, (0.0, 1.0, 0.0)),
        (3.0 / 6.0, (0.0, 1.0, 1.0)),
        (4.0 / 6.0, (0.0, 0.0, 1.0)),
        (5.0 / 6.0, (1.0, 0.0, 1.0)),
        (1.0, (1.0, 0.0, 0.0)),
    ];
    for &(off, (rc, gc, bc)) in &stops {
        pat.add_color_stop_rgb(off, rc, gc, bc);
    }
    let _ = cr.save();
    draw::round_rect(cr, r.0, r.1, r.2, r.3, r.3 / 2.0);
    cr.clip();
    let _ = cr.set_source(&pat);
    let _ = cr.paint();
    let _ = cr.restore();

    draw::round_rect(cr, r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, r.3 / 2.0);
    draw::set(cr, cx.theme.edge);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    let rx = r.0 + (hue.clamp(0.0, 360.0) / 360.0) * r.2;
    let ry = r.1 + r.3 / 2.0;
    let pure = hsv_to_rgb(hue, 1.0, 1.0);
    draw_picker_ring(cr, cx, rx, ry, 5.5, Rgba::rgb(pure));
}

pub(super) fn draw_sv_square(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    id: u32,
    r: Rect,
    hsv: (f64, f64, f64),
) {
    let _ = hits.add(id, r) || hits.selected(id);
    hits.mark_draggable(id);
    let pure = Rgba::rgb(hsv_to_rgb(hsv.0, 1.0, 1.0));
    let _ = cr.save();
    draw::round_rect(cr, r.0, r.1, r.2, r.3, THUMB_RADIUS);
    cr.clip();
    let horiz = cairo::LinearGradient::new(r.0, r.1, r.0 + r.2, r.1);
    horiz.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0);
    horiz.add_color_stop_rgb(1.0, pure.0, pure.1, pure.2);
    let _ = cr.set_source(&horiz);
    let _ = cr.paint();

    let vert = cairo::LinearGradient::new(r.0, r.1, r.0, r.1 + r.3);
    vert.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
    vert.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 1.0);
    let _ = cr.set_source(&vert);
    let _ = cr.paint();
    let _ = cr.restore();

    draw::round_rect(cr, r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, THUMB_RADIUS);
    draw::set(cr, cx.theme.edge);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    let rx = r.0 + hsv.1.clamp(0.0, 1.0) * r.2;
    let ry = r.1 + (1.0 - hsv.2.clamp(0.0, 1.0)) * r.3;
    let cur_rgb = hsv_to_rgb(hsv.0, hsv.1, hsv.2);
    draw_picker_ring(cr, cx, rx, ry, 6.0, Rgba::rgb(cur_rgb));
}

pub(super) fn draw_picker_ring(cr: &Context, cx: &Cx, x: f64, y: f64, radius: f64, fill: Rgba) {
    cr.new_sub_path();
    cr.arc(x, y, radius, 0.0, 2.0 * std::f64::consts::PI);
    draw::set(cr, fill);
    let _ = cr.fill_preserve();
    draw::set(cr, cx.theme.card.alpha(1.0));
    cr.set_line_width(3.0);
    let _ = cr.stroke_preserve();
    draw::set(cr, cx.theme.text);
    cr.set_line_width(1.5);
    let _ = cr.stroke();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_look::tests::*;
    use crate::theme::Theme;
    use std::fs;

    #[test]
    fn color_shell_command_validates_hex_and_refuses_injection() {
        let root = TempDir::new("color-sh");
        let home = root.0.join("home");
        let xdg = root.0.join("xdg");
        let bin = home.join(".local/bin");
        let live = home.join("infynitae/live");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&xdg).unwrap();

        let calls = root.0.join("calls");
        let answer = root.0.join("answer");
        let pwn = root.0.join("pwn");
        fs::write(
            bin.join("infynitae-edge"),
            format!("#!/bin/sh\ncat {}\n", sq(&answer.to_string_lossy())),
        )
        .unwrap();
        fs::write(
            live.join("settings.sh"),
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n",
                sq(&calls.to_string_lossy())
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            bin.join("infynitae-edge"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fs::set_permissions(live.join("settings.sh"), fs::Permissions::from_mode(0o755)).unwrap();

        let mut c = init_appearance(&[], true, &[]);
        let th = test_theme();
        let cx_m = test_cx(&th);
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        assert!(c.event(&press(H_TAB_ANY), &cx_m, &mut out));
        assert!(c.event(&press(H_CODE_ASK), &cx_m, &mut out));
        let Action::Run(cmd) = out.pop().unwrap() else {
            panic!("expected Run action");
        };

        let run_with_answer = |ans: &str| {
            fs::write(&answer, ans).unwrap();
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .env("HOME", &home)
                .env("XDG_RUNTIME_DIR", &xdg)
                .status()
                .unwrap()
        };

        // Invalid input containing shell metacharacters is refused without setting or executing
        let evil = format!("$(touch {}); notacolour", pwn.display());
        assert!(run_with_answer(&evil).success());
        assert!(!pwn.exists());
        assert!(!calls.exists());
        let bad_dir = xdg.join("infynitae/look-bad");
        assert!(bad_dir.join("ACCENT").is_file());

        let mut src = LookSource {
            home: home.clone(),
            bad_dir: bad_dir.clone(),
        };
        let d1 = src.fetch().downcast::<LookData>().unwrap();
        assert!(d1.bad_colors.contains("ACCENT"));

        // Valid #rrggbb clears the marker and runs log + set
        assert!(run_with_answer("#22aa44").success());
        assert!(!bad_dir.join("ACCENT").exists());
        let d2 = src.fetch().downcast::<LookData>().unwrap();
        assert!(!d2.bad_colors.contains("ACCENT"));
        let logged = fs::read_to_string(&calls).unwrap();
        assert_eq!(logged, "log ACCENT -\nset ACCENT #22aa44\n");
    }

    #[test]
    fn nudges_close_candidate_marks_without_leaving_bounds() {
        let bounds = (10.0, 20.0, 300.0, 180.0);
        let mut pts = vec![(50.0, 50.0), (50.0, 50.0), (54.0, 52.0), (250.0, 140.0)];
        nudge_marks(&mut pts, bounds, MARK_MIN_DIST);
        for i in 0..pts.len() {
            let (px, py) = pts[i];
            assert!(px >= bounds.0 && px <= bounds.0 + bounds.2);
            assert!(py >= bounds.1 && py <= bounds.1 + bounds.3);
            for (j, &(qx, qy)) in pts.iter().enumerate().skip(i + 1) {
                let d = (qx - px).hypot(qy - py);
                assert!(d + 1e-3 >= MARK_MIN_DIST, "pair ({i},{j}) d={d}");
            }
        }
        // Far-apart marks stay where they were.
        let mut far = vec![(40.0, 40.0), (200.0, 150.0)];
        nudge_marks(&mut far, bounds, MARK_MIN_DIST);
        assert_eq!(far, vec![(40.0, 40.0), (200.0, 150.0)]);
    }

    #[test]
    fn hsv_rgb_hex_round_trip_and_drag_preview_writes_once_on_release() {
        for sample in [
            "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#00ffff", "#ff00ff",
            "#7aa2f7", "#3b82f6", "#d97706", "#a855f7", "#1e293b", "#808080",
        ] {
            let rgb = parse_hex(sample).unwrap();
            let (h, s, v) = rgb_to_hsv(rgb);
            let back = hsv_to_rgb(h, s, v);
            assert_eq!(back, rgb, "round trip failed for {sample} -> {h},{s},{v}");
            assert_eq!(hsv_to_hex(h, s, v), sample);
        }

        let th = test_theme();
        let cx_m = Cx {
            size: Size::M,
            ..test_cx(&th)
        };
        let mut c = init_appearance(&[("ACCENT", "#7aa2f7")], true, &[]);
        c.look.as_mut().unwrap().accent_candidates =
            parse_accent_lines("#e05d44 0.2 0.2\n#44cc88 0.8 0.8\n");

        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        assert!(c.event(&press(H_TAB_ANY), &cx_m, &mut out));
        assert!(c.tick(500.0));
        draw_card(&c, &cx_m, 404.0);
        let hr = c.hue_rect.borrow().unwrap();
        let sr = c.sv_rect.borrow().unwrap();

        // Press and Drag on Hue strip and SV square change live value without emitting any command.
        out.clear();
        assert!(c.event(
            &Event::Press {
                hit: H_HUE,
                button: Button::Left,
                at: (hr.0 + hr.2 * 0.25, hr.1 + hr.3 / 2.0),
            },
            &cx_m,
            &mut out,
        ));
        assert!(out.is_empty());
        assert!(c.event(
            &Event::Drag {
                hit: H_HUE,
                at: (hr.0 + hr.2 / 3.0, hr.1 + hr.3 / 2.0),
            },
            &cx_m,
            &mut out,
        ));
        assert!(out.is_empty());
        assert!(c.event(&Event::Release { hit: H_HUE }, &cx_m, &mut out,));
        assert_eq!(out.len(), 1);
        let (_, orig_s, orig_v) = rgb_to_hsv(parse_hex("#7aa2f7").unwrap());
        assert_eq!(c.value(ACCENT_ROW), hsv_to_hex(120.0, orig_s, orig_v));
        assert!(c.accent_preview.is_none());

        out.clear();
        assert!(c.event(
            &Event::Press {
                hit: H_SV,
                button: Button::Left,
                at: (sr.0 + sr.2 * 0.5, sr.1 + sr.3 * 0.5),
            },
            &cx_m,
            &mut out,
        ));
        assert!(c.event(
            &Event::Drag {
                hit: H_SV,
                at: (sr.0 + sr.2, sr.1 + sr.3), // v = 0 -> black
            },
            &cx_m,
            &mut out,
        ));
        assert_eq!(c.value(ACCENT_ROW), "#000000");
        assert!(out.is_empty());
        assert!(c.event(&Event::Release { hit: H_SV }, &cx_m, &mut out));
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0],
            Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log ACCENT '#7aa2f7' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT '#000000' >/dev/null 2>&1"
                    .to_string()
            )
        );
        // Pressing + dragging and releasing on the current value emits no command.
        out.clear();
        assert!(c.event(
            &Event::Press {
                hit: H_SV,
                button: Button::Left,
                at: (sr.0 + sr.2 * 0.5, sr.1 + sr.3),
            },
            &cx_m,
            &mut out,
        ));
        assert!(c.event(&Event::Release { hit: H_SV }, &cx_m, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn accent_picker_commands_quote_values_and_height_matches_drawn_bounds() {
        let th = test_theme();
        let cx_s = test_cx(&th);
        let cx_m = Cx {
            size: Size::M,
            ..test_cx(&th)
        };

        let mut c = init_appearance(
            &[("ACCENT", "#11111b"), ("SURFACE_COLOR", "#11111b")],
            true,
            &[],
        );
        {
            let look = c.look.as_mut().unwrap();
            look.accent_candidates = parse_accent_lines(
                "#e05d44 0.150 0.250\n#44cc88 0.850 0.750\n#3b82f6 0.500 0.500\n",
            );
            look.recent_accents = vec!["#e05d44".to_string(), "#44cc88".to_string()];
        }

        // Open accent picker (defaults to "From your wallpaper" when candidates exist).
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10), &cx_m, &mut out));
        assert!(c.accent_picker_open());
        assert_eq!(c.accent_tab, AccentTab::Wallpaper);
        assert!(c.tick(500.0));

        // While Accent picker is open without search, outer THEME_FROM_WALLPAPER and THEME_SCHEME are hidden.
        let shown_keys: Vec<&str> = c.shown(&cx_m).0.iter().map(|&i| c.table[i].key).collect();
        assert!(!shown_keys.contains(&"THEME_FROM_WALLPAPER"));
        assert!(!shown_keys.contains(&"THEME_SCHEME"));

        // Verify height == drawn bounds on Wallpaper tab (S and M, with and without WP dropdown open).
        for (label, cx_cur) in [("wp-s", &cx_s), ("wp-m", &cx_m)] {
            let (hits, h) = draw_card(&c, cx_cur, 404.0);
            let max_bottom = hits
                .list
                .iter()
                .map(|(_, r)| r.1 + r.3)
                .fold(0.0_f64, f64::max);
            assert!(
                (h - max_bottom).abs() < 1.0,
                "{label}: h={h} max_bottom={max_bottom}"
            );
            assert!(hits.list.iter().any(|(id, _)| *id == H_MARK_BASE));
            assert!(hits.list.iter().any(|(id, _)| *id == H_CAND_SQ_BASE));
            assert!(hits.list.iter().any(|(id, _)| *id == H_SHUFFLE));
            assert!(hits.list.iter().any(|(id, _)| *id == H_WP_MODE_BOX));
        }

        // Open Colours-from-wallpaper inline dropdown and choose "shuffle" ("A new accent each time").
        out.clear();
        assert!(c.event(&press(H_WP_MODE_BOX), &cx_m, &mut out));
        let (hits_dd, h_dd) = draw_card(&c, &cx_m, 404.0);
        let max_bottom_dd = hits_dd
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0_f64, f64::max);
        assert!((h_dd - max_bottom_dd).abs() < 1.0);
        assert!(hits_dd
            .list
            .iter()
            .any(|(id, _)| *id == H_WP_MODE_OPT_BASE + 3));
        assert!(c.event(&press(H_WP_MODE_OPT_BASE + 3), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log THEME_FROM_WALLPAPER - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set THEME_FROM_WALLPAPER 'shuffle' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Shuffle button runs wallpaper-colors shuffle.
        out.clear();
        assert!(c.event(&press(H_SHUFFLE), &cx_m, &mut out));
        assert_eq!(out, vec![Action::Run(shuffle_command().to_string())]);

        // Click candidate mark and candidate square -> change_command with sq quoting.
        out.clear();
        assert!(c.event(&press(H_MARK_BASE + 1), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log ACCENT '#11111b' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT '#44cc88' >/dev/null 2>&1"
                    .to_string()
            )]
        );
        out.clear();
        assert!(c.event(&press(H_CAND_SQ_BASE + 2), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log ACCENT '#11111b' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT '#3b82f6' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Style scheme words set THEME_SCHEME.
        out.clear();
        assert!(c.event(&press(H_SCHEME_BASE + 5), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log THEME_SCHEME - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set THEME_SCHEME 'expressive' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Switch to Any colour tab with Recent + unreadably dark accent (#11111b on #11111b).
        out.clear();
        assert!(c.event(&press(H_TAB_ANY), &cx_m, &mut out));
        assert!(c.event(&press(H_RECENT_BASE + 1), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log ACCENT '#11111b' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set ACCENT '#e05d44' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Force unreadable current accent to verify warning line height at S and M.
        c.pending.insert("ACCENT", "#11111b".to_string());
        assert!(c.accent_adjusted_warning(&cx_m));
        for (label, cx_cur) in [("any-s", &cx_s), ("any-m", &cx_m)] {
            let (hits_any, h_any) = draw_card(&c, cx_cur, 404.0);
            let max_bottom_any = hits_any
                .list
                .iter()
                .map(|(_, r)| r.1 + r.3)
                .fold(0.0_f64, f64::max);
            assert!(
                (h_any - max_bottom_any).abs() < 1.0,
                "{label}: h={h_any} max_bottom={max_bottom_any}"
            );
            assert!(hits_any.list.iter().any(|(id, _)| *id == H_SCHEME_BASE + 5));
        }

        // With 0 candidates, Accent opens directly on "Any colour", and Wallpaper tab shows exact bounds.
        let mut c_empty = init_appearance(&[("ACCENT", "#7aa2f7")], true, &[]);
        assert!(c_empty.event(&press(ROW + 10), &cx_m, &mut out));
        assert_eq!(c_empty.accent_tab, AccentTab::AnyColor);
        assert!(c_empty.event(&press(H_TAB_WP), &cx_m, &mut out));
        c_empty.tick(500.0);
        let (hits_e, h_e) = draw_card(&c_empty, &cx_m, 404.0);
        let max_bottom_e = hits_e
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0_f64, f64::max);
        assert!((h_e - max_bottom_e).abs() < 1.0);
    }

    #[test]
    fn picker_error_uses_theme_alert_not_accent() {
        let mut th1 = test_theme();
        th1.alert = Rgba::rgb([0xff, 0x22, 0x22]);
        let mut th2 = th1.clone();
        th2.alert = Rgba::rgb([0x22, 0xff, 0x22]);

        let render = |th: &Theme| -> Vec<u8> {
            let cx = Cx {
                size: Size::M,
                ..test_cx(th)
            };
            let mut c = init_appearance(&[], true, &["ACCENT"]);
            let mut out = Vec::new();
            assert!(c.event(&press(ROW + 10), &cx, &mut out));
            c.tick(500.0);
            let h = c.height(&cx, 404.0).ceil() as i32 + 10;
            let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 420, h).unwrap();
            let mut hits = Hits::new(None, None);
            {
                let cr = Context::new(&surf).unwrap();
                c.draw(&cr, &cx, (0.0, 0.0, 404.0, h as f64), &mut hits);
            }
            surf.flush();
            let bytes = surf.data().unwrap().to_vec();
            bytes
        };

        assert_ne!(render(&th1), render(&th2));
    }
}
