//! Drawing a panel, open or closing, with its cards and their headers.

use super::*;

impl Shell {
    /// Draws the open (or closing) panel on a screen of `w x h`: each card's box, header and body,
    /// rising in as the panel opens or fading out as it closes; keeps each card's hits for the
    /// pointer while open. Returns the panel's box.
    pub fn draw(&self, cr: &cairo::Context, w: f64, h: f64) -> Option<Rect> {
        self.draw_on(screen_key(w, h), cr, w, h)
    }

    pub fn draw_on(&self, output: OutputId, cr: &cairo::Context, w: f64, h: f64) -> Option<Rect> {
        if !self.visible_on(output) {
            return None;
        }
        draw::with_font(&self.theme.font, || {
            self.draw_open_or_closing(output, cr, w, h)
        })
    }

    pub(super) fn draw_open_or_closing(
        &self,
        output: OutputId,
        cr: &cairo::Context,
        w: f64,
        h: f64,
    ) -> Option<Rect> {
        if self.ask.ask.on {
            return Some(crate::ask::draw(cr, &self.theme, &self.ask.ask, (w, h)));
        }
        if let (Some(wm), Some(r)) = (self.window_menu, self.window_menu_rect(w, h)) {
            let th = &self.theme;
            draw::card(cr, th, r.0, r.1, r.2, r.3);
            let body = (
                r.0 + PAD_X - 8.0,
                r.1 + PAD_TOP,
                r.2 - 2.0 * (PAD_X - 8.0),
                r.3 - PAD_TOP - PAD_BOTTOM,
            );
            let cx = self.cx(Size::M, None, false);
            let sel = self
                .selected
                .filter(|(c, _)| self.keyboard && *c == 0)
                .map(|(_, h)| h);
            let mut hits = Hits::new(self.pointer, sel);
            let mut y = body.1;
            for (i, (_, label, checked)) in wm.items().into_iter().enumerate() {
                let row_r = (body.0, y, body.2, crate::ui::LIST_ROW_H);
                crate::cards::tray_menu::draw_menu_row(
                    cr, &cx, &mut hits, i as u32, row_r, label, true, checked,
                );
                y += crate::ui::LIST_ROW_H;
            }
            *self.frame.borrow_mut() = vec![(0, r, hits)];
            return Some(r);
        }
        if let Some(idx) = self.open {
            return Some(self.draw_panel(
                output,
                cr,
                w,
                h,
                &self.panels[idx],
                idx,
                self.panel_scroll,
                None,
            ));
        }
        if let Some(c) = &self.closing {
            let ps = c.transient.as_ref().or_else(|| self.panels.get(c.idx))?;
            let cp = (1.0 - crate::ui::ease((c.ms / LEAVE_MS).clamp(0.0, 1.0))).clamp(0.0, 1.0);
            return Some(self.draw_panel(output, cr, w, h, ps, c.idx, c.scroll, Some(cp)));
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_panel(
        &self,
        output: OutputId,
        cr: &cairo::Context,
        w: f64,
        h: f64,
        ps: &PanelState,
        idx: usize,
        scroll_pos: f64,
        closing_p: Option<f64>,
    ) -> Rect {
        let pointer = self.pointer_for(output);
        let interactive = closing_p.is_none();
        let editing = interactive && self.editing();
        let rooms = self.panel_rooms(ps, w, h);
        let (pbox, local_rects, content_h) = self.layout_panel_with(ps, w, h, &rooms);
        let max_scroll = (content_h - pbox.3).max(0.0);
        let scroll = scroll_pos.clamp(0.0, max_scroll);
        let rects: Vec<(usize, Rect)> = local_rects
            .iter()
            .map(|&(ci, r)| (ci, (pbox.0 + r.0, pbox.1 + r.1 - scroll, r.2, r.3)))
            .collect();
        let above = ps.panel.side == Side::Bottom;
        let th = &self.theme;
        let mut frame = Vec::new();
        let bottom = rects.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        cr.save().ok();
        let clip_lift = if closing_p.is_some() {
            LEAVE_DROP
        } else if self.open_ms.is_finite() {
            ARRIVE_LIFT
        } else {
            0.0
        };
        cr.rectangle(pbox.0, pbox.1, pbox.2, pbox.3 + clip_lift);
        cr.clip();
        for (ci, r) in &rects {
            let pc = &ps.cards[*ci];
            let cx = Cx {
                room: rooms[*ci],
                ..self.cx(pc.size, pc.app_id.as_deref(), above)
            };
            let status = pc.card.status(&cx);
            let sel = if !interactive || editing {
                None
            } else {
                self.selected
                    .filter(|(c, _)| self.keyboard && c == ci)
                    .map(|(_, h)| h)
            };
            let mut hits = Hits::new(
                if !interactive || editing {
                    None
                } else {
                    pointer
                },
                sel,
            );
            // the bottom card arrives first (about one card per 90 px up)
            let k = ((bottom - (r.1 + r.3)) / 90.0).clamp(0.0, ARRIVE_CARDS);
            let p = if let Some(cp) = closing_p {
                cp
            } else if self.open_ms.is_finite() {
                crate::ui::ease((self.open_ms - k * ARRIVE_STAGGER_MS) / ARRIVE_MS)
            } else {
                1.0
            };
            if p <= 0.0 {
                if interactive {
                    frame.push((*ci, *r, hits));
                }
                continue;
            }
            if editing && status == Status::Hidden {
                let dash = [4.0, 3.0];
                draw::round_rect(
                    cr,
                    r.0 + 0.5,
                    r.1 + 0.5,
                    r.2 - 1.0,
                    r.3 - 1.0,
                    th.corners.min(7.0),
                );
                draw::set(cr, th.muted.alpha(0.35));
                cr.set_line_width(1.0);
                cr.set_dash(&dash, 0.0);
                let _ = cr.stroke();
                cr.set_dash(&[], 0.0);
                let title = pc
                    .card
                    .title(&cx)
                    .unwrap_or_else(|| pc.kind.title.to_string());
                let name = Text {
                    size: 13.0,
                    bold: true,
                    color: th.text.alpha(0.75),
                    text: &title,
                };
                let (nw, _) = draw::measure(cr, &name);
                draw::text(cr, r.0 + PAD_X, r.1 + (r.3.min(52.0) - 15.0) / 2.0, &name);
                draw::text_max(
                    cr,
                    r.0 + PAD_X + nw + 10.0,
                    r.1 + (r.3.min(52.0) - 13.0) / 2.0 + 1.0,
                    (r.2 - 2.0 * PAD_X - nw - 10.0).max(10.0),
                    &Text {
                        size: 12.0,
                        bold: false,
                        color: th.muted,
                        text: "nothing to show now",
                    },
                );
                frame.push((*ci, *r, hits));
                continue;
            }
            let lifted = p < 1.0;
            if lifted {
                let drop_y = if closing_p.is_some() {
                    LEAVE_DROP
                } else {
                    ARRIVE_LIFT
                };
                cr.push_group();
                cr.translate(0.0, (1.0 - p) * drop_y);
            }
            draw_card(cr, th, pc, &cx, status, *r, &mut hits);
            if lifted {
                let _ = cr.pop_group_to_source();
                let _ = cr.paint_with_alpha(p);
            }
            if interactive {
                frame.push((*ci, *r, hits));
            }
        }
        if max_scroll > 0.0 {
            let sb_cx = self.cx(Size::M, None, above);
            // the bar keeps the margin from the screen's edges (the panel runs edge to edge)
            let m = if pbox.3 >= h { MARGIN } else { 0.0 };
            let view = (pbox.0, pbox.1 + m, pbox.2, pbox.3 - 2.0 * m);
            crate::ui::scrollbar(cr, &sb_cx, view, content_h - 2.0 * m, scroll);
        }
        cr.restore().ok();
        if editing {
            let cards = self.edit_cards(idx);
            let (bar, add) = self.edit_rects(idx, w, h);
            let mut overlay_state = self.edit.clone();
            overlay_state.bar_rect = bar;
            overlay_state.add_rect = add;
            let overlay_cx = self.cx(Size::M, None, above);
            crate::edit::draw_overlay(
                cr,
                &overlay_cx,
                &ps.panel,
                &cards,
                &local_rects,
                (pbox.0, pbox.1),
                (pbox.2, pbox.3),
                scroll,
                pointer,
                &overlay_state,
            );
        }
        if interactive {
            if let Some(r) = self.edit_menu_rect(w, h) {
                let th = &self.theme;
                crate::draw::round_rect(cr, r.0, r.1, r.2, r.3, th.small_radius());
                crate::draw::set(cr, th.card.alpha(1.0));
                let _ = cr.fill_preserve();
                crate::draw::set(cr, th.edge);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
                let lit = pointer.is_some_and(|(px, py)| {
                    px >= r.0 && px <= r.0 + r.2 && py >= r.1 && py <= r.1 + r.3
                });
                if lit {
                    crate::draw::round_rect(
                        cr,
                        r.0 + 3.0,
                        r.1 + 3.0,
                        r.2 - 6.0,
                        r.3 - 6.0,
                        th.small_radius(),
                    );
                    crate::draw::set(cr, th.text.alpha(0.08));
                    let _ = cr.fill();
                }
                crate::draw::text_max(
                    cr,
                    r.0 + 12.0,
                    r.1 + (r.3 - 17.0) / 2.0,
                    r.2 - 24.0,
                    &crate::draw::Text {
                        size: 13.0,
                        bold: false,
                        color: th.text,
                        text: "Edit panel",
                    },
                );
            }
            self.frames.borrow_mut().insert(
                output,
                OutputFrame {
                    hits: frame.clone(),
                    scroll,
                    screen: (w, h),
                },
            );
            *self.frame.borrow_mut() = frame;
            self.frame_scroll.set(scroll);
            self.frame_screen.set((w, h));
        }
        pbox
    }
}

/// One card in its box `r`: the box, its header, and its body as its status says (a panel's card
/// or a desktop widget).
pub fn draw_card(
    cr: &cairo::Context,
    th: &Theme,
    pc: &Placed,
    cx: &Cx,
    status: Status,
    r: Rect,
    hits: &mut Hits,
) {
    // a card without its header only while it shows itself (Loading and Missing say
    // whose they are)
    let headed = pc.card.header(cx) || status != Status::Ready;
    let head = if headed { HEAD_H } else { 0.0 };
    let body = (
        r.0 + PAD_X - 8.0,
        r.1 + PAD_TOP + head,
        r.2 - 2.0 * (PAD_X - 8.0),
        r.3 - PAD_TOP - head - PAD_BOTTOM,
    );
    draw::card(cr, th, r.0, r.1, r.2, r.3);
    if headed {
        let title = pc
            .card
            .title(cx)
            .unwrap_or_else(|| pc.kind.title.to_string());
        draw_header(cr, th, r, &title, &pc.from);
    }
    match status {
        Status::Ready => {
            cr.save().ok();
            cr.rectangle(body.0 - 1.0, body.1 - 1.0, body.2 + 2.0, body.3 + 2.0);
            cr.clip();
            pc.card.draw(cr, cx, body, hits);
            cr.restore().ok();
        }
        Status::Loading => {
            draw::text(
                cr,
                body.0 + 8.0,
                body.1,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: th.muted,
                    text: "Loading",
                },
            );
        }
        Status::Missing(why) => {
            let l = missing_layout(cr, body.2 - 16.0, &why);
            draw::set(cr, th.muted);
            cr.move_to((body.0 + 8.0).round(), body.1.round());
            pangocairo::functions::show_layout(cr, &l);
        }
        Status::Hidden => {}
    }
}

pub(super) fn header_title_max_w(card_w: f64, from_w: f64) -> (f64, f64) {
    let head_w = (card_w - 2.0 * PAD_X).max(40.0);
    // Keep at least half of the header for the title when the app name is long.
    let max_from = ((head_w - 12.0) * 0.45).max(0.0);
    let capped_from = from_w.min(max_from);
    let title_w = if capped_from > 0.0 {
        (head_w - capped_from - 12.0).max(40.0)
    } else {
        head_w
    };
    (capped_from, title_w)
}

pub(super) fn draw_header(cr: &cairo::Context, th: &Theme, r: Rect, title: &str, from: &str) {
    let from_w = if from.is_empty() {
        0.0
    } else {
        let from_t = Text {
            size: 12.0,
            bold: false,
            color: th.muted,
            text: from,
        };
        let full_w = draw::measure(cr, &from_t).0;
        let (capped, _) = header_title_max_w(r.2, full_w);
        if full_w <= capped {
            draw::text_right(cr, r.0 + r.2 - PAD_X, r.1 + PAD_TOP + 3.0, &from_t)
        } else {
            draw::text_max(
                cr,
                r.0 + r.2 - PAD_X - capped,
                r.1 + PAD_TOP + 3.0,
                capped,
                &from_t,
            );
            capped
        }
    };
    let (_, title_w) = header_title_max_w(r.2, from_w);
    draw::text_max(
        cr,
        r.0 + PAD_X,
        r.1 + PAD_TOP,
        title_w,
        &Text {
            size: 15.0,
            bold: true,
            color: th.text,
            text: title,
        },
    );
}

pub(super) fn missing_layout(cr: &cairo::Context, max_w: f64, why: &str) -> pango::Layout {
    draw::wrapped_layout(cr, draw::SIZE_SMALL, false, max_w.max(20.0), Some(2), why)
}

pub(super) fn wrapped_text_height(max_w: f64, text: &str) -> f64 {
    let Ok(surf) = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1) else {
        return 20.0;
    };
    let Ok(cr) = cairo::Context::new(&surf) else {
        return 20.0;
    };
    let l = missing_layout(&cr, max_w, text);
    (l.pixel_size().1 as f64).max(20.0)
}

pub(super) fn missing_height(card_width: f64, why: &str) -> f64 {
    wrapped_text_height(card_width - 2.0 * PAD_X, why)
}

pub(super) fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.0.min(b.0);
    let y0 = a.1.min(b.1);
    let x1 = (a.0 + a.2).max(b.0 + b.2);
    let y1 = (a.1 + a.3).max(b.1 + b.3);
    (x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::tests::*;

    #[test]
    fn long_app_name_on_half_card_leaves_at_least_half_header_for_title() {
        let half_w = (380.0 - GAP) / 2.0;
        let head_w = half_w - 2.0 * PAD_X;
        let long_name = "Very Long Application Name Forty Charact";
        assert_eq!(long_name.chars().count(), 40);

        let apps = vec![
            App {
                id: "longapp".into(),
                name: long_name.into(),
                exec: "true".into(),
                ..Default::default()
            },
            App {
                id: "secondapp".into(),
                name: "Second".into(),
                exec: "true".into(),
                ..Default::default()
            },
        ];
        let mut sh = Shell::new(
            Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 longapp/app@half secondapp/app@half\"\n",
            ),
            apps,
        );
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;

        let (_, rects) = sh.layout(idx, 1280.0, 800.0);
        assert_eq!(rects.len(), 2);
        let r0 = rects[0].1;
        assert!((r0.2 - half_w).abs() < 0.01);

        let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 800).unwrap();
        {
            let cr = cairo::Context::new(&surf).unwrap();
            draw::grey_text_edges(&cr);
            let full_from_w = draw::measure(
                &cr,
                &Text {
                    size: 12.0,
                    bold: false,
                    color: sh.theme.muted,
                    text: long_name,
                },
            )
            .0;
            let (capped_from, title_w) = header_title_max_w(half_w, full_from_w);
            assert!(capped_from <= head_w * 0.45);
            assert!(
                title_w >= head_w * 0.5,
                "title width {title_w} must be at least half of header {head_w}"
            );
            sh.draw(&cr, 1280.0, 800.0);
        }
        surf.flush();
        // Verify no header text spills left of the card's left edge.
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        let out_x = (r0.0 - 4.0).max(0.0) as usize;
        let y_start = (r0.1 + PAD_TOP) as usize;
        let y_end = (r0.1 + PAD_TOP + HEAD_H) as usize;
        for y in y_start..y_end {
            let px = &data[y * stride + out_x * 4..y * stride + out_x * 4 + 4];
            assert_eq!(px, &[0, 0, 0, 0], "header overflowed left of card at y={y}");
        }
    }

    #[test]
    fn missing_reason_wraps_and_fits_at_half_width() {
        let longest = "Memory information is not available";
        let half_w = (380.0 - GAP) / 2.0;
        let body_text_w = half_w - 2.0 * PAD_X;
        let mh = missing_height(half_w, longest);
        assert!(
            mh > 20.0,
            "wrapped missing text height ({mh}) should exceed single-line 20 px"
        );

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 200).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let l = missing_layout(&cr, body_text_w, longest);
        assert_eq!(l.line_count(), 2);
        assert!(
            !l.is_ellipsized(),
            "longest Missing string should fit in 2 lines without ellipsis at half width"
        );

        // Verify via Shell layout + draw with monitor/memory in Missing state at half width.
        let mut sh = shell("SUPER+A top right 380 monitor/memory@half calendar/clock@half");
        let idx = sh
            .panels
            .iter()
            .position(|p| p.panel.name == "control")
            .unwrap();
        // Send empty MemoryData so monitor/memory enters Status::Missing(longest).
        sh.apply(
            "monitor",
            "memory",
            &crate::cards::monitor::MemoryData::default() as &dyn std::any::Any,
        );
        assert!(sh.open_panel(idx, false, Instant::now()));
        sh.open_ms = f64::INFINITY;
        let (_, rects) = sh.layout(idx, 1280.0, 800.0);
        assert_eq!(
            rects[0].1 .3,
            PAD_TOP + HEAD_H + mh + PAD_BOTTOM,
            "card height grows to fit wrapped Missing sentence"
        );
        sh.draw(&cr, 1280.0, 800.0);
    }
}
