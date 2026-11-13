use crate::geom::Rect;
use crate::look::Look;

/// Parameters for drawing one output's full-screen selection overlay.
pub struct OverlayDraw<'a> {
    pub look: Look,
    pub frozen: Option<&'a cairo::ImageSurface>,
    /// Selection rectangle in this output's local pixel coordinates (may extend past output edges
    /// when spanning multiple outputs so seam borders are omitted naturally).
    pub selection_px: Option<Rect>,
    /// Label text (e.g. `"800 x 600"`) shown beside the dragged selection rectangle.
    pub size_label: Option<String>,
    /// Physical pixels per logical pixel (`view_w / logical_w`) for crisp text sizing.
    pub scale: f64,
}

/// Renders the selection overlay into `dst` (sized in output pixels).
pub fn render_overlay(dst: &cairo::ImageSurface, params: &OverlayDraw<'_>) -> Result<(), String> {
    let vw = dst.width() as f64;
    let vh = dst.height() as f64;
    let cr = cairo::Context::new(dst).map_err(|e| format!("cairo context failed: {e}"))?;

    if let Some(frozen) = params.frozen {
        cr.set_operator(cairo::Operator::Source);
        cr.set_source_surface(frozen, 0.0, 0.0)
            .map_err(|e| format!("cairo source failed: {e}"))?;
        cr.paint().map_err(|e| format!("cairo paint failed: {e}"))?;
    } else {
        // `--live`: keep inside of selection completely transparent and screen unblocked.
        cr.set_operator(cairo::Operator::Source);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.paint().map_err(|e| format!("cairo clear failed: {e}"))?;
    }

    // Dim everything outside the active selection (or entire output before a selection exists) by 40%.
    cr.set_operator(cairo::Operator::Over);
    params.look.card.set_source_with_alpha(&cr, 0.40);
    cr.set_fill_rule(cairo::FillRule::EvenOdd);
    cr.rectangle(0.0, 0.0, vw, vh);
    if let Some(sel) = params.selection_px {
        if sel.w > 0 && sel.h > 0 {
            cr.rectangle(sel.x as f64, sel.y as f64, sel.w as f64, sel.h as f64);
        }
    }
    cr.fill()
        .map_err(|e| format!("cairo dim fill failed: {e}"))?;

    // Draw the 1 px LOOK_ACCENT outline around the selection/hovered window.
    if let Some(sel) = params.selection_px {
        if sel.w > 0 && sel.h > 0 {
            cr.set_line_width(1.0);
            params.look.accent.set_source_with_alpha(&cr, 1.0);
            let ow = (sel.w as f64 - 1.0).max(1.0);
            let oh = (sel.h as f64 - 1.0).max(1.0);
            cr.rectangle(sel.x as f64 + 0.5, sel.y as f64 + 0.5, ow, oh);
            cr.stroke()
                .map_err(|e| format!("cairo outline stroke failed: {e}"))?;
        }
    }

    // Small "W x H" badge beside the dragged selection rectangle.
    if let (Some(sel), Some(label)) = (params.selection_px, &params.size_label) {
        if sel.w > 0 && sel.h > 0 {
            draw_size_label(&cr, params.look, sel, label, vw, vh, params.scale.max(1.0))?;
        }
    }

    drop(cr);
    dst.flush();
    Ok(())
}

fn draw_size_label(
    cr: &cairo::Context,
    look: Look,
    sel: Rect,
    text: &str,
    vw: f64,
    vh: f64,
    scale: f64,
) -> Result<(), String> {
    let _ = cr.save();
    cr.scale(scale, scale);

    let layout = pangocairo::functions::create_layout(cr);
    let mut font = pango::FontDescription::from_string("sans-serif");
    font.set_size(10 * pango::SCALE);
    layout.set_font_description(Some(&font));
    layout.set_text(text);

    let (tw, th) = layout.pixel_size();
    let pad_x = 6.0;
    let pad_y = 3.0;
    let gap = 6.0;
    let box_w = tw as f64 + pad_x * 2.0;
    let box_h = th as f64 + pad_y * 2.0;

    let lw = vw / scale;
    let lh = vh / scale;
    let sx = sel.x as f64 / scale;
    let sy = sel.y as f64 / scale;
    let sr = sel.right() as f64 / scale;
    let sb = sel.bottom() as f64 / scale;

    // Place to the right or below the selection, flipping inside if near an output edge.
    let bx = if sr + gap + box_w <= lw - 2.0 {
        sr + gap
    } else if sx >= box_w + gap + 2.0 {
        sx - gap - box_w
    } else {
        (sr - box_w).clamp(2.0, (lw - box_w - 2.0).max(2.0))
    };

    let by = if sb + gap + box_h <= lh - 2.0 && bx <= sr {
        sb + gap
    } else if bx > sr {
        (sb - box_h).clamp(2.0, (lh - box_h - 2.0).max(2.0))
    } else if sy >= box_h + gap + 2.0 {
        sy - gap - box_h
    } else {
        2.0
    };

    look.card.set_source_with_alpha(cr, 0.90);
    rounded_rect(cr, bx, by, box_w, box_h, (look.corners as f64).min(4.0));
    cr.fill()
        .map_err(|e| format!("cairo label fill failed: {e}"))?;

    cr.set_line_width(1.0);
    look.edge.set_source_with_alpha(cr, 1.0);
    rounded_rect(
        cr,
        bx + 0.5,
        by + 0.5,
        box_w - 1.0,
        box_h - 1.0,
        (look.corners as f64).min(4.0),
    );
    cr.stroke()
        .map_err(|e| format!("cairo label border failed: {e}"))?;

    look.text.set_source_with_alpha(cr, 1.0);
    cr.move_to(bx + pad_x, by + pad_y);
    pangocairo::functions::show_layout(cr, &layout);

    let _ = cr.restore();
    Ok(())
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    let r = radius.min(w * 0.5).min(h * 0.5).max(0.0);
    if r <= 0.0 {
        cr.rectangle(x, y, w, h);
        return;
    }
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 1.5 * pi);
    cr.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::Color;

    #[test]
    fn overlay_dims_outside_preserves_inside_and_strokes_accent() {
        let look = Look {
            card: Color::rgb8(0, 0, 0),
            accent: Color::rgb8(0, 255, 0),
            ..Default::default()
        };

        // Frozen capture: solid white 20x20.
        let frozen = cairo::ImageSurface::create(cairo::Format::ARgb32, 20, 20).unwrap();
        {
            let cr = cairo::Context::new(&frozen).unwrap();
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().unwrap();
        }

        let mut dst = cairo::ImageSurface::create(cairo::Format::ARgb32, 20, 20).unwrap();
        render_overlay(
            &dst,
            &OverlayDraw {
                look,
                frozen: Some(&frozen),
                selection_px: Some(Rect::new(5, 5, 10, 10)),
                size_label: None,
                scale: 1.0,
            },
        )
        .unwrap();

        let stride = dst.stride() as usize;
        let data = dst.data().unwrap();
        let px = |x: usize, y: usize| -> [u8; 4] {
            let o = y * stride + x * 4;
            [data[o], data[o + 1], data[o + 2], data[o + 3]]
        };

        // Outside selection: white dimmed by 40% black => ~153 per channel.
        let out = px(1, 1);
        assert!((out[0] as i32 - 153).abs() <= 1, "outside={out:?}");
        assert_eq!(out[3], 255);

        // Inside selection (strictly interior): preserved white.
        assert_eq!(px(10, 10), [255, 255, 255, 255]);

        // Selection perimeter pixel (5, 5): 1 px LOOK_ACCENT (green -> BGRA [0, 255, 0, 255]).
        assert_eq!(px(5, 5), [0, 255, 0, 255]);
        assert_eq!(px(14, 14), [0, 255, 0, 255]);
    }

    #[test]
    fn live_overlay_is_transparent_inside_selection_and_dimmed_outside() {
        let look = Look {
            card: Color::rgb8(0x10, 0x10, 0x10),
            ..Default::default()
        };

        let mut dst = cairo::ImageSurface::create(cairo::Format::ARgb32, 20, 20).unwrap();
        render_overlay(
            &dst,
            &OverlayDraw {
                look,
                frozen: None,
                selection_px: Some(Rect::new(4, 4, 10, 10)),
                size_label: None,
                scale: 1.0,
            },
        )
        .unwrap();

        let stride = dst.stride() as usize;
        let data = dst.data().unwrap();
        let alpha_at = |x: usize, y: usize| -> u8 { data[y * stride + x * 4 + 3] };

        assert_eq!(alpha_at(8, 8), 0);
        let outside_a = alpha_at(1, 1);
        assert!((outside_a as i32 - 102).abs() <= 1, "alpha={outside_a}");
    }
}
