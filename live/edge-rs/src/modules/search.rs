//! The `search` module: a bar (`hidden`: shown once something is typed; `bar`: always shown) with
//! a 18 px stroked search icon and the typed query. Typing anywhere in the open panel types into
//! `world.search` and filters the `apps` and `windows` modules.

use super::{is_left_button, Rect, World};
use crate::config::Item;
use crate::draw::{self, Text, PAD_X};
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::f64::consts::PI;

pub const BAR_H: f64 = 46.0;

pub fn height(it: &Item, world: &World) -> f64 {
    let look = it.look.unwrap_or(1);
    if look == 0 && world.search.is_empty() {
        return 0.0;
    }
    if let Some((_, _, _, rh)) = it.rect {
        return rh as f64;
    }
    BAR_H
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    let (w, _h) = size;
    let (x, _y) = pos;
    let show_clear = world.panel_has_clipboard && !world.clipboard.items.is_empty();
    if show_clear && x >= w - 64.0 && is_left_button(button) {
        world.clipboard.items.clear();
        world.clipboard.scroll = 0;
        world.run("cliphist wipe 2>/dev/null".to_string());
        return true;
    }
    if (button == 3 || button == 0x111) && !world.search.is_empty() {
        world.set_search(String::new());
    }
    true
}

fn draw_search_icon(cr: &Context, cx: f64, cy: f64, color: Rgba) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.new_sub_path();
    cr.arc(cx - 1.5, cy - 1.5, 5.5, 0.0, 2.0 * PI);
    let _ = cr.stroke();
    cr.move_to(cx + 2.5, cy + 2.5);
    cr.line_to(cx + 7.0, cy + 7.0);
    let _ = cr.stroke();
}

pub fn draw(cr: &Context, th: &Theme, it: &Item, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    if it.look.unwrap_or(1) == 0 && world.search.is_empty() {
        return;
    }
    draw::card(cr, th, x, y, w, h);

    let cy = y + h / 2.0;
    let icon_col = if world.search.is_empty() {
        th.muted
    } else {
        th.accent
    };
    draw_search_icon(cr, x + PAD_X + 9.0, cy, icon_col);

    let show_clear = world.panel_has_clipboard && !world.clipboard.items.is_empty();
    let clear_w = if show_clear { 64.0 } else { 0.0 };

    if show_clear {
        draw::text_right(
            cr,
            x + w - PAD_X,
            cy - 7.0,
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: "Clear",
            },
        );
    }

    let tx = x + PAD_X + 26.0;
    let max_w = (w - 2.0 * PAD_X - 30.0 - clear_w).max(10.0);
    if !world.search.is_empty() {
        let tw = draw::text_max(
            cr,
            tx,
            cy - 8.0,
            max_w,
            &Text {
                size: 13.0,
                bold: false,
                color: th.text,
                text: &world.search,
            },
        );
        let caret_x = (tx + tw.min(max_w) + 2.0).round();
        draw::set(cr, th.accent);
        cr.rectangle(caret_x, (cy - 7.0).round(), 1.5, 14.0);
        let _ = cr.fill();
    } else {
        draw::text_max(
            cr,
            tx,
            cy - 8.0,
            max_w,
            &Text {
                size: 13.0,
                bold: false,
                color: th.muted,
                text: "Search",
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Module;

    #[test]
    fn search_hidden_look_shows_only_when_query_is_non_empty() {
        let mut world = World::now();
        let hidden = Item {
            module: Module::Search,
            look: Some(0),
            rect: None,
            half: false,
        };
        let bar = Item {
            module: Module::Search,
            look: Some(1),
            rect: None,
            half: false,
        };
        assert_eq!(height(&hidden, &world), 0.0);
        assert_eq!(height(&bar, &world), BAR_H);

        world.set_search("fire".to_string());
        assert_eq!(height(&hidden, &world), BAR_H);
        assert_eq!(height(&bar, &world), BAR_H);
    }

    #[test]
    fn search_clear_button_wipes_clipboard_when_panel_has_clipboard() {
        use crate::modules::clipboard::ClipItem;

        let mut world = World::now();
        world.panel_has_clipboard = true;
        world.clipboard.items.push(ClipItem {
            raw_line: "1\tsample".to_string(),
            preview: "sample".to_string(),
            time: "just now".to_string(),
            is_image: false,
            pinned: false,
        });

        // Click Clear button at x = 350.0 in width 380.0 (350 >= 380 - 64)
        let handled = click(&mut world, (380.0, 46.0), (350.0, 23.0), 1);
        assert!(handled);
        assert!(world.clipboard.items.is_empty());
        assert_eq!(world.clipboard.scroll, 0);
        assert_eq!(world.actions, vec!["cliphist wipe 2>/dev/null".to_string()]);

        // When panel_has_clipboard is false, clicking right side does not wipe
        world.clipboard.items.push(ClipItem {
            raw_line: "2\tanother".to_string(),
            preview: "another".to_string(),
            time: "just now".to_string(),
            is_image: false,
            pinned: false,
        });
        world.actions.clear();
        world.panel_has_clipboard = false;

        let handled2 = click(&mut world, (380.0, 46.0), (350.0, 23.0), 1);
        assert!(handled2);
        assert_eq!(world.clipboard.items.len(), 1);
        assert!(world.actions.is_empty());
    }

    #[test]
    fn search_draw_renders_clear_button_when_panel_has_clipboard() {
        use crate::modules::clipboard::ClipItem;

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 380, 46).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let mut world = World::now();
        world.panel_has_clipboard = true;
        world.clipboard.items.push(ClipItem {
            raw_line: "1\tsample".to_string(),
            preview: "sample".to_string(),
            time: "just now".to_string(),
            is_image: false,
            pinned: false,
        });

        let item = Item {
            module: Module::Search,
            look: Some(1),
            rect: None,
            half: false,
        };
        draw(&cr, &th, &item, &world, (0.0, 0.0, 380.0, 46.0));
    }
}
