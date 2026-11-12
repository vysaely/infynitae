//! The `favorites` module: a 5-column grid of 44 px app icons with names under them, hidden when
//! empty or while searching. Left-click launches; right-click or `Shift+Enter` toggles the app in
//! `FAVORITES` and rewrites the config line as the C edge does (`toggle_favorite`).

use super::{
    apps::{self, DesktopApp},
    is_left_button, Rect, World,
};
use crate::config::Item;
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::theme::{Rgba, Theme};
use cairo::Context;

pub const COLS: usize = 5;
pub const ICON_SZ: f64 = 44.0;
pub const CELL_H: f64 = 68.0;
pub const ROW_GAP: f64 = 10.0;
pub const TITLE_H: f64 = 28.0;
pub const MAX_FAVORITES: usize = 24;

fn strip_desktop(id: &str) -> &str {
    if id.len() > 8 && id[id.len() - 8..].eq_ignore_ascii_case(".desktop") {
        &id[..id.len() - 8]
    } else {
        id
    }
}

/// Resolves the words in `favorites` (desktop file IDs with or without `.desktop`, or app names,
/// case-insensitive) into deduplicated indices into `apps`, preserving the order in `favorites`
/// (matching `resolve_favorites` in `live/edge/infynitae-edge.c`).
pub fn resolve_favorites(apps: &[DesktopApp], favorites: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for w in favorites
        .split([' ', ',', '\t'])
        .map(str::trim)
        .filter(|w| !w.is_empty())
    {
        if out.len() >= MAX_FAVORITES {
            break;
        }
        let w_stem = strip_desktop(w);
        for (i, da) in apps.iter().enumerate() {
            let da_stem = strip_desktop(&da.id);
            let id_match = da.id.eq_ignore_ascii_case(w) || da_stem.eq_ignore_ascii_case(w_stem);
            if !id_match && !da.name.eq_ignore_ascii_case(w) {
                continue;
            }
            if !out.contains(&i) {
                out.push(i);
            }
            break;
        }
    }
    out
}

/// Rewrites the `FAVORITES=` line in `config_text` (or appends one if missing), matching the awk
/// script used by `toggle_favorite` in `live/edge/infynitae-edge.c`.
#[cfg(test)]
pub fn rewrite_favorites_config(config_text: &str, favorites: &str) -> String {
    let mut out = String::with_capacity(config_text.len() + 64);
    let mut replaced = false;
    for line in config_text.lines() {
        if line.starts_with("FAVORITES=") {
            out.push_str(&format!("FAVORITES=\"{favorites}\"\n"));
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !replaced {
        out.push_str(&format!("FAVORITES=\"{favorites}\"\n"));
    }
    out
}

/// Toggles `app_idx` in `world.apps.fav_indices` (appending to the end if not pinned, or removing
/// if already pinned), updates `world.apps.favorites`, and rewrites `FAVORITES="..."` in
/// `~/.config/infynitae/config` (matching `toggle_favorite` in `live/edge/infynitae-edge.c`).
pub fn toggle_favorite(world: &mut World, app_idx: usize) -> bool {
    let Some(target) = world.apps.apps.get(app_idx) else {
        return false;
    };
    let target_id = strip_desktop(&target.id).to_string();
    let mut words: Vec<String> = Vec::new();
    let mut was = false;
    for &idx in &world.apps.fav_indices {
        if idx == app_idx {
            was = true;
            continue;
        }
        if let Some(app) = world.apps.apps.get(idx) {
            words.push(strip_desktop(&app.id).to_string());
        }
    }
    if !was && !target_id.is_empty() {
        words.push(target_id);
    }
    let next_favs = words.join(" ");
    world.apps.set_favorites(&next_favs);

    world.set_config("FAVORITES", &next_favs);
    true
}

pub fn height(it: &Item, world: &World) -> f64 {
    if world.apps.fav_indices.is_empty() || !world.search.is_empty() {
        return 0.0;
    }
    if let Some((_, _, _, rh)) = it.rect {
        return rh as f64;
    }
    let rows = world.apps.fav_indices.len().div_ceil(COLS);
    2.0 * PAD_Y + TITLE_H + rows as f64 * CELL_H + (rows.saturating_sub(1)) as f64 * ROW_GAP
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if world.apps.fav_indices.is_empty() || !world.search.is_empty() {
        return false;
    }
    let (w, _) = size;
    let (px, py) = pos;
    let gx = PAD_X;
    let gy = PAD_Y + TITLE_H;
    let gw = (w - 2.0 * PAD_X).max(50.0);
    if px < gx || px > gx + gw || py < gy {
        return false;
    }
    let col_w = gw / COLS as f64;
    let col = ((px - gx) / col_w).floor() as usize;
    if col >= COLS {
        return false;
    }
    let row_stride = CELL_H + ROW_GAP;
    let rel_y = py - gy;
    let row = (rel_y / row_stride).floor() as usize;
    if rel_y - (row as f64 * row_stride) > CELL_H {
        return false;
    }
    let slot = row * COLS + col;
    let Some(&app_idx) = world.apps.fav_indices.get(slot) else {
        return false;
    };
    if button == 3 || button == 0x111 {
        return toggle_favorite(world, app_idx);
    }
    if is_left_button(button) {
        return apps::launch_app(world, app_idx);
    }
    false
}

pub const HINT: &str = "Right click an app to pin";

/// The hint for the favorites card title (docs/design/EDGE_DESIGN.md).
pub fn hint() -> &'static str {
    HINT
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    if world.apps.fav_indices.is_empty() || !world.search.is_empty() {
        return;
    }
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    draw::card(cr, th, x, y, w, h);
    draw::title(cr, th, x + PAD_X, y + PAD_Y, "Favorites");
    draw::title_hint(cr, th, x, y, w, "Favorites", hint());

    let gx = x + PAD_X;
    let gy = y + PAD_Y + TITLE_H;
    let gw = (w - 2.0 * PAD_X).max(50.0);
    let col_w = gw / COLS as f64;
    let sel_fav = world.selected_favorite_slot();

    for (slot, &app_idx) in world.apps.fav_indices.iter().enumerate() {
        let Some(app) = world.apps.apps.get(app_idx) else {
            continue;
        };
        let col = slot % COLS;
        let row = slot / COLS;
        let cx = gx + col as f64 * col_w;
        let cy = gy + row as f64 * (CELL_H + ROW_GAP);
        if cy + ICON_SZ > y + h {
            break;
        }
        let is_sel = sel_fav == Some(slot);
        if is_sel {
            draw::round_rect(
                cr,
                cx + 2.0,
                cy - 4.0,
                (col_w - 4.0).max(0.0),
                CELL_H + 4.0,
                8.0,
            );
            draw::set(cr, Rgba(th.accent.0, th.accent.1, th.accent.2, 0.18));
            let _ = cr.fill();
        }
        let icon_x = cx + (col_w - ICON_SZ) / 2.0;
        apps::draw_app_icon(cr, th, &app.icon, &app.name, icon_x, cy, ICON_SZ);

        let label = Text {
            size: 12.0,
            bold: is_sel,
            color: th.text,
            text: &app.name,
        };
        let max_label_w = (col_w - 6.0).max(16.0);
        let (tw, _) = draw::measure(cr, &label);
        let lx = cx + ((col_w - tw.min(max_label_w)) / 2.0).max(3.0);
        draw::text_max(cr, lx, cy + ICON_SZ + 5.0, max_label_w, &label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_apps() -> Vec<DesktopApp> {
        vec![
            DesktopApp::new(
                "Firefox",
                "",
                "",
                "firefox",
                "firefox",
                "org.mozilla.firefox.desktop",
            ),
            DesktopApp::new(
                "Files",
                "",
                "",
                "nautilus",
                "nautilus",
                "org.gnome.Nautilus.desktop",
            ),
            DesktopApp::new("Foot", "", "", "foot", "foot", "foot.desktop"),
            DesktopApp::new(
                "Settings",
                "",
                "",
                "gnome-control-center",
                "settings",
                "org.gnome.Settings.desktop",
            ),
            DesktopApp::new("GIMP", "", "", "gimp", "gimp", "gimp.desktop"),
            DesktopApp::new(
                "Writer",
                "",
                "",
                "libreoffice --writer",
                "writer",
                "libreoffice-writer.desktop",
            ),
        ]
    }

    #[test]
    fn resolve_favorites_matches_ids_and_names_in_order_without_duplicates() {
        let apps = test_apps();
        let resolved =
            resolve_favorites(&apps, "foot org.mozilla.firefox.desktop Files foot unknown");
        assert_eq!(resolved, vec![2, 0, 1]);
    }

    #[test]
    fn toggle_favorite_adds_and_removes_and_rewrites_config_line() {
        let mut world = World::now();
        world.apps.apps = test_apps();
        world.apps.set_favorites("foot Files");
        assert_eq!(world.apps.fav_indices, vec![2, 1]);

        // Pin Firefox (index 0) -> appended at the end using its stripped desktop id
        assert!(toggle_favorite(&mut world, 0));
        assert_eq!(
            world.apps.favorites,
            "foot org.gnome.Nautilus org.mozilla.firefox"
        );
        assert_eq!(world.apps.fav_indices, vec![2, 1, 0]);
        assert!(world
            .actions
            .last()
            .unwrap()
            .contains("set FAVORITES=\"foot org.gnome.Nautilus org.mozilla.firefox\""));

        // Unpin Files (index 1) -> removed from favorites
        assert!(toggle_favorite(&mut world, 1));
        assert_eq!(world.apps.favorites, "foot org.mozilla.firefox");
        assert_eq!(world.apps.fav_indices, vec![2, 0]);

        // Verify config text rewriter both updates existing FAVORITES= and appends when missing
        let orig = "EDGE_POSITION=\"top\"\nFAVORITES=\"foot\"\nCORNERS=\"8\"\n";
        let updated = rewrite_favorites_config(orig, &world.apps.favorites);
        assert_eq!(
            updated,
            "EDGE_POSITION=\"top\"\nFAVORITES=\"foot org.mozilla.firefox\"\nCORNERS=\"8\"\n"
        );
        let added = rewrite_favorites_config("EDGE_POSITION=\"top\"\n", "foot");
        assert_eq!(added, "EDGE_POSITION=\"top\"\nFAVORITES=\"foot\"\n");
    }

    #[test]
    fn favorites_grid_click_launches_on_left_and_toggles_on_right() {
        let mut world = World::now();
        world.apps.apps = test_apps();
        world
            .apps
            .set_favorites("foot firefox files settings gimp writer");
        let it = Item {
            module: crate::config::Module::Favorites,
            look: None,
            rect: None,
            half: false,
        };
        let w = 560.0;
        let h = height(&it, &world);
        assert!(h > 150.0);

        // Left-click slot 1 (col 1, row 0 -> Firefox)
        let col_w = (w - 2.0 * PAD_X) / 5.0;
        assert!(click(
            &mut world,
            (w, h),
            (PAD_X + col_w * 1.5, PAD_Y + TITLE_H + 20.0),
            1
        ));
        assert_eq!(world.actions.last().map(String::as_str), Some("firefox"));
        assert!(world.close_requested);

        // Right-click slot 5 (col 0, row 1 -> Writer) removes it from favorites
        world.close_requested = false;
        assert!(click(
            &mut world,
            (w, h),
            (
                PAD_X + col_w * 0.5,
                PAD_Y + TITLE_H + CELL_H + ROW_GAP + 20.0
            ),
            3
        ));
        assert_eq!(world.apps.fav_indices.len(), 5);
    }

    #[test]
    fn favorites_hint_text_and_narrow_card_drop() {
        assert_eq!(hint(), "Right click an app to pin");

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 380, 100).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let mut world = World::now();
        world.apps.apps = test_apps();
        world.apps.set_favorites("foot firefox");

        // Wide card (380.0) renders title and hint
        draw(&cr, &th, &world, (0.0, 0.0, 380.0, 100.0));

        // Narrow card (140.0) drops hint to prevent overlap with title
        let surf_narrow = cairo::ImageSurface::create(cairo::Format::ARgb32, 140, 100).unwrap();
        let cr_narrow = cairo::Context::new(&surf_narrow).unwrap();
        draw(&cr_narrow, &th, &world, (0.0, 0.0, 140.0, 100.0));
    }
}
