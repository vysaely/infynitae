//! Drawing the Look cards' parts: list rows, the style row, the wallpaper grid and the face section.

use super::*;

impl LookCard {
    pub(super) fn draw_list(
        &self,
        cr: &Context,
        cx: &Cx,
        hits: &mut Hits,
        lr: Rect,
        entries: &[ListEntry],
    ) {
        ui::list_box(cr, cx, lr);
        if entries.is_empty() {
            if lr.3 >= ui::LIST_ROW_H + 4.0 {
                draw::text_max(
                    cr,
                    lr.0 + 10.0,
                    lr.1 + 4.0 + (ui::LIST_ROW_H - 16.0) / 2.0,
                    (lr.2 - 20.0).max(20.0),
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: cx.theme.muted,
                        text: "None found",
                    },
                );
            }
            return;
        }
        let vis = entries.len().min(MAX_LIST_ROWS);
        let max_scroll = entries.len().saturating_sub(vis);
        let start = self.list_scroll.min(max_scroll);
        let end = (start + vis).min(entries.len());
        for (offset, j) in (start..end).enumerate() {
            let rr = (
                lr.0 + 4.0,
                lr.1 + 4.0 + offset as f64 * ui::LIST_ROW_H,
                lr.2 - 8.0,
                ui::LIST_ROW_H,
            );
            if rr.1 + rr.3 > lr.1 + lr.3 + 0.5 {
                break;
            }
            ui::list_row(
                cr,
                cx,
                hits,
                LIST + j as u32,
                rr,
                &entries[j].label,
                "",
                entries[j].active,
                None,
            );
        }
        if entries.len() > vis && lr.3 >= vis as f64 * ui::LIST_ROW_H {
            ui::scrollbar(
                cr,
                cx,
                (lr.0, lr.1 + 4.0, lr.2, (lr.3 - 8.0).max(0.0)),
                entries.len() as f64 * ui::LIST_ROW_H,
                start as f64 * ui::LIST_ROW_H,
            );
        }
    }
}

pub(super) fn draw_style_row(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    first_id: u32,
    r: Rect,
    opts: &[(&str, &str)],
    chosen: Option<usize>,
) {
    if opts.is_empty() {
        return;
    }
    let gap = 5.0;
    let raw_w: Vec<f64> = opts
        .iter()
        .map(|(_, word)| {
            draw::measure(
                cr,
                &Text {
                    size: draw::SIZE_SMALL,
                    bold: true,
                    color: cx.theme.text,
                    text: word,
                },
            )
            .0 + 12.0
        })
        .collect();
    let total_raw: f64 = raw_w.iter().sum();
    let avail = (r.2 - gap * (opts.len() as f64 - 1.0)).max(20.0);
    let mut x = r.0;
    for (idx, (_, word)) in opts.iter().enumerate() {
        let w = if total_raw <= avail {
            raw_w[idx] + (avail - total_raw) / opts.len() as f64
        } else {
            raw_w[idx] * (avail / total_raw)
        };
        let b = (x, r.1, w, r.3);
        let id = first_id + idx as u32;
        ui::choice_button(cr, cx, hits, id, b, word, chosen == Some(idx));
        x += w + gap;
    }
}

pub(super) fn fmt_number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

pub(super) fn snap(v: f64, min: f64, max: f64, step: f64) -> f64 {
    let v = v.clamp(min, max);
    if step > 0.0 {
        (min + ((v - min) / step).round() * step).clamp(min, max)
    } else {
        v
    }
}

pub(super) fn value_at_right_capped(
    cr: &Context,
    cx: &Cx,
    r: Rect,
    x_right: f64,
    max_w: f64,
    text: &str,
    changed: bool,
) -> f64 {
    let t = Text {
        size: 12.0,
        bold: false,
        color: if changed {
            cx.theme.accent_text
        } else {
            cx.theme.muted
        },
        text,
    };
    let w = draw::measure(cr, &t).0.min(max_w);
    draw::text_max(cr, x_right - w, r.1 + (r.3 - 15.0) / 2.0, max_w, &t)
}

pub(super) fn thumb_cell_size(width: f64, size: Size) -> (usize, f64, f64) {
    let cols = if size == Size::M { 3 } else { 2 };
    let avail = (width - 2.0 * THUMB_PAD_X - (cols - 1) as f64 * THUMB_GAP).max(cols as f64 * 16.0);
    let cw = (avail / cols as f64).floor();
    let ch = (cw * 9.0 / 16.0).round().max(9.0);
    (cols, cw, ch)
}

impl LookCard {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_wallpaper_grid(
        &self,
        cr: &Context,
        cx: &Cx,
        hits: &mut Hits,
        body: Rect,
        y: f64,
        i: usize,
        id: u32,
        changed: bool,
    ) {
        let files = self
            .wallpapers
            .as_ref()
            .map(|w| w.files.as_slice())
            .unwrap_or(&[]);
        let shown_len = files.len().min(MAX_WALLPAPERS);
        let right = body.0 + body.2 - 8.0;
        let word = self.display_word(i);

        if shown_len == 0 {
            draw::text_max(
                cr,
                body.0 + 8.0,
                y + (ui::ROW_H - 15.0) / 2.0,
                (body.2 - 16.0).max(20.0),
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "No pictures in Pictures/Wallpapers",
                },
            );
            if !self.value(i).is_empty() {
                value_at_right_capped(
                    cr,
                    cx,
                    (body.0, y, body.2, ui::ROW_H),
                    right,
                    (body.2 * 0.45).max(40.0),
                    &word,
                    changed,
                );
            }
            return;
        }

        let (cols, cw, ch) = thumb_cell_size(body.2, cx.size);
        let grid_rows = shown_len.div_ceil(cols);
        let cur = self.value(i);
        let cur_expanded = crate::paths::expand_home_with(&cur, self.home.as_deref())
            .to_string_lossy()
            .into_owned();
        let thumbs = self.wallpapers.as_ref().map(|w| {
            if cx.size == Size::M {
                &w.thumbs_m
            } else {
                &w.thumbs_s
            }
        });

        for (idx, file) in files.iter().take(shown_len).enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            let tx = body.0 + THUMB_PAD_X + col as f64 * (cw + THUMB_GAP);
            let ty = y + THUMB_TOP_PAD + row as f64 * (ch + THUMB_GAP);
            let tid = THUMB + idx as u32;
            let lit = hits.add(tid, (tx, ty, cw, ch)) || hits.selected(tid);
            let active = !cur.is_empty() && (cur == file.path || cur_expanded == file.path);

            let _ = cr.save();
            draw::round_rect(cr, tx, ty, cw, ch, THUMB_RADIUS);
            cr.clip();
            if let Some(surf) = thumbs
                .and_then(|t| t.get(idx))
                .and_then(|o| o.as_ref())
                .and_then(icons::surface_for_decoded)
            {
                let sw = surf.width() as f64;
                let sh = surf.height() as f64;
                if (sw - cw).abs() > 1.5 || (sh - ch).abs() > 1.5 {
                    cr.translate(tx, ty);
                    cr.scale(cw / sw, ch / sh);
                    let _ = cr.set_source_surface(&surf, 0.0, 0.0);
                } else {
                    let dx = ((cw - sw) / 2.0).round();
                    let dy = ((ch - sh) / 2.0).round();
                    let _ = cr.set_source_surface(&surf, tx + dx, ty + dy);
                }
                let _ = cr.paint();
            } else {
                draw::set(cr, cx.theme.track);
                let _ = cr.paint();
            }
            if lit {
                draw::set(cr, cx.theme.text.alpha(ui::LIGHT_ALPHA));
                let _ = cr.paint();
            }
            let _ = cr.restore();

            if active {
                draw::round_rect(
                    cr,
                    tx + 1.0,
                    ty + 1.0,
                    (cw - 2.0).max(1.0),
                    (ch - 2.0).max(1.0),
                    (THUMB_RADIUS - 0.5).max(0.0),
                );
                draw::set(cr, cx.theme.accent);
                cr.set_line_width(2.0);
                let _ = cr.stroke();
            } else if lit {
                draw::round_rect(
                    cr,
                    tx + 0.5,
                    ty + 0.5,
                    (cw - 1.0).max(1.0),
                    (ch - 1.0).max(1.0),
                    THUMB_RADIUS,
                );
                draw::set(cr, cx.theme.text.alpha(0.35));
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
        }

        let name_y = y
            + THUMB_TOP_PAD
            + grid_rows as f64 * ch
            + (grid_rows.saturating_sub(1)) as f64 * THUMB_GAP;
        draw::text_max(
            cr,
            body.0 + THUMB_PAD_X,
            name_y + (THUMB_NAME_H - 15.0) / 2.0,
            (body.2 - 2.0 * THUMB_PAD_X).max(20.0),
            &Text {
                size: 12.0,
                bold: false,
                color: if changed {
                    cx.theme.accent_text
                } else {
                    cx.theme.muted
                },
                text: &word,
            },
        );

        if files.len() > MAX_WALLPAPERS {
            let more_y = name_y + THUMB_NAME_H;
            let extra = format!("{} more", files.len() - MAX_WALLPAPERS);
            let entries = if self.pick == Some(i) && self.list.shown() {
                self.list_entries(i)
            } else {
                Vec::new()
            };
            let vis = entries.len().clamp(1, MAX_LIST_ROWS);
            let (row_r, list_r) = if self.pick == Some(i) && self.list.shown() {
                let lh = self.list.height(vis);
                self.list
                    .split((body.0, more_y, body.2, ui::ROW_H + lh), vis, cx.list_above)
            } else {
                ((body.0, more_y, body.2, ui::ROW_H), (0.0, 0.0, 0.0, 0.0))
            };
            let ew = draw::measure(
                cr,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: &extra,
                },
            )
            .0 + 14.0;
            ui::row(
                cr,
                cx,
                hits,
                id,
                row_r,
                &Row {
                    icon: None,
                    name: "More",
                    state: "",
                    on: false,
                    right: ew,
                },
            );
            value_at_right_capped(cr, cx, row_r, right, 96.0, &extra, false);
            if self.pick == Some(i) && self.list.shown() {
                self.draw_list(cr, cx, hits, list_r, &entries);
            }
        }
    }
}

pub(super) fn draw_swatch(cr: &Context, cx: &Cx, x: f64, r: Rect, rgb: [u8; 3]) {
    let sy = r.1 + (r.3 - SWATCH) / 2.0;
    draw::round_rect(
        cr,
        x + 0.5,
        sy + 0.5,
        SWATCH - 1.0,
        SWATCH - 1.0,
        cx.theme.small_radius(),
    );
    draw::set(cr, Rgba::rgb(rgb));
    let _ = cr.fill_preserve();
    draw::set(cr, cx.theme.edge);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

impl LookCard {
    pub(super) fn show_face_row(&self, cx: &Cx) -> bool {
        let _ = cx;
        if self.kind != LookKind::Appearance || self.config.is_none() {
            return false;
        }
        if self.query.trim().is_empty() {
            return true;
        }
        let hay = "your picture face avatar photo";
        self.query
            .split_whitespace()
            .all(|w| hay.contains(&w.to_lowercase()))
    }

    pub(super) fn face_grid_geom(cx: &Cx, width: f64) -> (usize, f64) {
        let cols = if cx.size == Size::S { 3 } else { 4 };
        let avail =
            (width - 2.0 * THUMB_PAD_X - (cols - 1) as f64 * THUMB_GAP).max(cols as f64 * 32.0);
        let cell = (avail / cols as f64).floor();
        (cols, cell)
    }

    pub(super) fn face_h(&self, cx: &Cx, width: f64) -> f64 {
        if !self.show_face_row(cx) {
            return 0.0;
        }
        if !self.face_open {
            return ui::ROW_H;
        }
        let n = self
            .face
            .as_ref()
            .map_or(0, |f| f.files.len().min(FACE_MAX_THUMBS));
        if n == 0 {
            return ui::ROW_H * 2.0;
        }
        let (cols, cell) = Self::face_grid_geom(cx, width);
        let rows = n.div_ceil(cols);
        ui::ROW_H + THUMB_TOP_PAD + rows as f64 * cell + rows.saturating_sub(1) as f64 * THUMB_GAP
    }

    pub(super) fn draw_face_section(
        &self,
        cr: &Context,
        cx: &Cx,
        hits: &mut Hits,
        body: Rect,
        y: f64,
    ) -> f64 {
        let r = (body.0, y, body.2, ui::ROW_H);
        let right = body.0 + body.2 - 8.0;
        let current = self
            .face_preview
            .as_ref()
            .or_else(|| self.face.as_ref().and_then(|f| f.current.as_ref()));
        let right_pad = if current.is_some() { 32.0 } else { 44.0 };
        ui::row(
            cr,
            cx,
            hits,
            FACE_ROW,
            r,
            &Row {
                icon: None,
                name: "Your picture",
                state: "",
                on: self.face_open,
                right: right_pad,
            },
        );
        if let Some(img) = current {
            let rad = 12.0;
            let cx_c = right - rad;
            let cy_c = y + ui::ROW_H / 2.0;
            let _ = cr.save();
            cr.arc(cx_c, cy_c, rad, 0.0, std::f64::consts::TAU);
            cr.clip();
            if let Some(surf) = icons::surface_for_decoded(img) {
                let sw = surf.width() as f64;
                let sh = surf.height() as f64;
                let d = 2.0 * rad;
                cr.translate(cx_c - rad, cy_c - rad);
                cr.scale(d / sw, d / sh);
                let _ = cr.set_source_surface(&surf, 0.0, 0.0);
                let _ = cr.paint();
            } else {
                draw::set(cr, cx.theme.track.alpha(0.35));
                let _ = cr.paint();
            }
            let _ = cr.restore();
        } else {
            value_at_right_capped(cr, cx, r, right, body.2 * 0.4, "None", false);
        }

        if !self.face_open {
            return ui::ROW_H;
        }
        let grid_y = y + ui::ROW_H;
        let files = self
            .face
            .as_ref()
            .map(|f| &f.files[..f.files.len().min(FACE_MAX_THUMBS)])
            .unwrap_or(&[]);
        if files.is_empty() {
            draw::text_max(
                cr,
                body.0 + 8.0,
                grid_y + (ui::ROW_H - 15.0) / 2.0,
                (body.2 - 16.0).max(20.0),
                &Text {
                    size: 12.0,
                    bold: false,
                    color: cx.theme.muted,
                    text: "No pictures in ~/Pictures",
                },
            );
            return ui::ROW_H * 2.0;
        }
        let (cols, cell) = Self::face_grid_geom(cx, body.2);
        let thumbs = self.face.as_ref().map(|f| f.thumbs.as_slice());
        for idx in 0..files.len() {
            let row = idx / cols;
            let col = idx % cols;
            let tx = body.0 + THUMB_PAD_X + col as f64 * (cell + THUMB_GAP);
            let ty = grid_y + THUMB_TOP_PAD + row as f64 * (cell + THUMB_GAP);
            let cell_r = (tx, ty, cell, cell);
            let id = FACE_THUMB + idx as u32;
            let hover = hits.add(id, cell_r);
            let sel = hits.selected(id);

            let _ = cr.save();
            draw::round_rect(cr, tx, ty, cell, cell, THUMB_RADIUS);
            cr.clip();
            if let Some(surf) = thumbs
                .and_then(|t| t.get(idx))
                .and_then(|o| o.as_ref())
                .and_then(icons::surface_for_decoded)
            {
                let sw = surf.width() as f64;
                let sh = surf.height() as f64;
                if (sw - cell).abs() > 1.0 || (sh - cell).abs() > 1.0 {
                    cr.translate(tx, ty);
                    cr.scale(cell / sw, cell / sh);
                    let _ = cr.set_source_surface(&surf, 0.0, 0.0);
                } else {
                    let _ = cr.set_source_surface(&surf, tx, ty);
                }
                let _ = cr.paint();
            } else {
                draw::set(cr, cx.theme.track.alpha(0.28));
                let _ = cr.paint();
            }
            if hover {
                draw::set(cr, cx.theme.text.alpha(ui::LIGHT_ALPHA));
                let _ = cr.paint();
            }
            let _ = cr.restore();

            if sel {
                cr.set_line_width(2.0);
                draw::set(cr, cx.theme.accent);
                draw::round_rect(cr, tx, ty, cell, cell, THUMB_RADIUS);
                let _ = cr.stroke();
            }
        }
        self.face_h(cx, body.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_look::tests::*;
    use crate::theme::Theme;

    #[test]
    fn style_row_uses_borderless_soft_fills_via_choice_button() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let opts = [("balanced", "Balanced"), ("dark", "Dark")];
        let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 320, 60).unwrap();
        let mut hits = Hits::new(None, None);
        {
            let cr = Context::new(&surf).unwrap();
            draw_style_row(
                &cr,
                &cx,
                &mut hits,
                H_SCHEME_BASE,
                (10.0, 10.0, 280.0, 28.0),
                &opts,
                Some(0),
            );
        }
        surf.flush();
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        let px = |x: usize, y: usize| -> [u8; 4] {
            let off = y * stride + x * 4;
            [data[off], data[off + 1], data[off + 2], data[off + 3]]
        };
        let b0 = hits.list[0].1;
        let b1 = hits.list[1].1;
        // Chosen pill top edge matches interior (no outline stroke) and differs from unchosen pill.
        let chosen_edge = px((b0.0 + 10.0) as usize, b0.1 as usize);
        let chosen_in = px((b0.0 + 10.0) as usize, (b0.1 + 2.0) as usize);
        let other_in = px((b1.0 + 10.0) as usize, (b1.1 + 2.0) as usize);
        assert!(chosen_in[3] > 0);
        assert!(other_in[3] > 0);
        assert_eq!(chosen_edge, chosen_in);
        assert_ne!(chosen_in, other_in);
    }

    #[test]
    fn face_row_visibility_query_filter_and_panel_close() {
        let th = test_theme();
        let cx = test_cx(&th);

        // Wallpaper card never draws Your picture.
        let wall = init_wallpaper(&[], true, &[]);
        let (wall_hits, _) = draw_card(&wall, &cx, 404.0);
        assert!(!has_hit(&wall_hits, FACE_ROW));

        // Appearance shows Your picture at bottom; None when empty.
        let mut card = init_appearance(&[], true, &[]);
        assert!(!card.face_open);
        let (hits, h_closed) = draw_card(&card, &cx, 404.0);
        assert!(has_hit(&hits, FACE_ROW));

        // Clicking opens the empty grid with an honest empty state, and panel_closed closes it.
        let mut out = Vec::new();
        assert!(card.event(&press(FACE_ROW), &cx, &mut out));
        assert!(card.face_open);
        assert!(out.is_empty());
        let h_empty_open = card.height(&cx, 404.0);
        assert!((h_empty_open - (h_closed + ui::ROW_H)).abs() < 0.01);

        card.panel_closed();
        assert!(!card.face_open);

        // Query matching "picture" keeps the card visible with Your picture shown.
        assert!(card.event(&Event::Text("picture"), &cx, &mut out));
        assert_eq!(card.status(&cx), Status::Ready);
        let (q_hits, _) = draw_card(&card, &cx, 404.0);
        assert!(has_hit(&q_hits, FACE_ROW));

        // Non-matching query hides Appearance completely.
        assert!(card.event(&Event::Text("zzz-missing-zzz"), &cx, &mut out));
        assert_eq!(card.status(&cx), Status::Hidden);
    }

    #[test]
    fn face_source_scans_and_grid_picks_image_with_quoted_command() {
        let dir = TempDir::new("face-source");
        let home = &dir.0;
        write_png_rgba(&home.join(".face"), 32, 32, [30, 80, 200, 255]);
        let pic1 = home.join("Pictures/avatar 'one'.png");
        let pic2 = home.join("Pictures/Wallpapers/wide.png");
        write_png_rgba(&pic1, 64, 32, [220, 40, 40, 255]);
        write_png_rgba(&pic2, 32, 80, [40, 200, 60, 255]);

        let mut src = FaceSource { home: home.clone() };
        let data = src.fetch();
        let face = data.downcast_ref::<FaceData>().unwrap();
        assert!(face.current.is_some());
        assert_eq!(face.files.len(), 2);
        assert_eq!(face.thumbs.len(), 2);
        assert!(face.thumbs.iter().all(|t| t.is_some()));

        let th = test_theme();
        let cx = test_cx(&th);
        let mut card = init_appearance(&[], true, &[]);
        let h_no_face = card.height(&cx, 404.0);
        card.apply("face", &*data);
        let h_with_face = card.height(&cx, 404.0);
        assert!((h_no_face - h_with_face).abs() < 0.01);

        let mut out = Vec::new();
        assert!(card.event(&press(FACE_ROW), &cx, &mut out));
        assert!(card.face_open);

        let (hits, h_open) = draw_card(&card, &cx, 404.0);
        assert!(h_open > h_with_face + ui::ROW_H);
        assert!(has_hit(&hits, FACE_THUMB));
        assert!(has_hit(&hits, FACE_THUMB + 1));

        let pic1_idx = face
            .files
            .iter()
            .position(|f| f.path.contains("avatar 'one'.png"))
            .unwrap() as u32;
        assert!(card.event(&press(FACE_THUMB + pic1_idx), &cx, &mut out));
        assert!(!card.face_open);
        assert!(card.face_preview.is_some());
        assert_eq!(out.len(), 1);
        match &out[0] {
            Action::Run(cmd) => {
                assert!(cmd.contains("--crop-face"));
                assert!(cmd.contains(&sq(&pic1.to_string_lossy())));
                assert!(cmd.contains("log FACE"));
            }
            other => panic!("expected Action::Run, got {other:?}"),
        }
    }
}
