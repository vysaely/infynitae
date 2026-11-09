//! Edit mode (the panel as a small canvas): move, resize, snap, overlap, undo, reset, add, done.
//!
//! Port of the C edge's edit mode (`live/edge/infynitae-edge.c`: `edit_enter`, `edit_exit`,
//! `edit_press`, `edit_drag`, `edit_release`, `snap_move`, `snap_size`, `nearest_free`,
//! `save_panel_line`).

use crate::config::{Module, Panel, Side};
use crate::draw::{self, Text};
use crate::theme::Theme;
use cairo::Context;

pub const PAD: f64 = 8.0;
pub const GAP: f64 = 6.0;

/// Default modules for built-in panels when Reset is clicked.
pub const DEFAULT_CONTROL: &[Module] = &[
    Module::Clock,
    Module::Volume,
    Module::Media,
    Module::Wifi,
    Module::Bluetooth,
    Module::Dnd,
    Module::Night,
    Module::Keyboard,
    Module::Power,
    Module::Replay,
    Module::Tray,
    Module::Notifications,
    Module::Session,
];

pub const DEFAULT_APPS: &[Module] = &[Module::Search, Module::Favorites, Module::Apps];
pub const DEFAULT_WINDOWS: &[Module] = &[Module::Search, Module::Windows];
pub const DEFAULT_CLIPBOARD: &[Module] = &[Module::Search, Module::Clipboard];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    MoveModule(usize),
    ResizeModule(usize),
    MovePanel,
    ResizePanel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DragState {
    pub kind: DragKind,
    pub start_pos: (f64, f64),
    pub initial_rect: (i32, i32, i32, i32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditState {
    pub active: bool,
    pub changed: bool,
    pub adding: bool,
    pub selected: Option<usize>,
    pub drag: Option<DragState>,
    pub live: bool,
    pub before: String,
    pub undo: Vec<Panel>,
    pub snap: bool,
    pub overlap: bool,
    pub bar_rect: (f64, f64, f64, f64),
    pub hover_module: Option<usize>,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            active: false,
            changed: false,
            adding: false,
            selected: None,
            drag: None,
            live: false,
            before: String::new(),
            undo: Vec::new(),
            snap: true,
            overlap: false,
            bar_rect: (0.0, 0.0, 0.0, 0.0),
            hover_module: None,
        }
    }
}

/// Snap value `v` to targets in `targets` within distance 8 px.
pub fn snap_to(v: i32, targets: &[i32]) -> i32 {
    let mut best = v;
    let mut min_d = 9;
    for &t in targets {
        let d = (v - t).abs();
        if d < min_d {
            min_d = d;
            best = t;
        }
    }
    best
}

/// Snaps a moved module's edges to panel edges and other modules within 8 px.
pub fn snap_move(panel: &Panel, mi: usize, pw: f64, x: &mut i32, y: &mut i32, w: i32, h: i32) {
    let mut tx = Vec::with_capacity(4 + 4 * panel.items.len());
    let mut ty = Vec::with_capacity(2 + 4 * panel.items.len());

    tx.push(PAD as i32);
    tx.push((pw - PAD) as i32 - w);
    ty.push(PAD as i32);

    for (k, it) in panel.items.iter().enumerate() {
        if k == mi {
            continue;
        }
        if let Some((ox, oy, ow, oh)) = it.rect {
            if ow <= 0 || oh <= 0 {
                continue;
            }
            tx.push(ox);
            tx.push(ox + ow + GAP as i32);
            tx.push(ox + ow - w);
            tx.push(ox - GAP as i32 - w);

            ty.push(oy);
            ty.push(oy + oh + GAP as i32);
            ty.push(oy + oh - h);
            ty.push(oy - GAP as i32 - h);
        }
    }

    *x = snap_to(*x, &tx);
    *y = snap_to(*y, &ty);
}

/// Snaps resized module edges within 8 px.
pub fn snap_size(panel: &Panel, mi: usize, pw: f64, x: i32, y: i32, w: &mut i32, h: &mut i32) {
    let mut tr = Vec::with_capacity(2 + 2 * panel.items.len());
    let mut tb = Vec::with_capacity(1 + 2 * panel.items.len());

    tr.push((pw - PAD) as i32);

    for (k, it) in panel.items.iter().enumerate() {
        if k == mi {
            continue;
        }
        if let Some((ox, oy, ow, oh)) = it.rect {
            if ow <= 0 || oh <= 0 {
                continue;
            }
            tr.push(ox + ow);
            tr.push(ox - GAP as i32);

            tb.push(oy + oh);
            tb.push(oy - GAP as i32);
        }
    }

    *w = snap_to(x + *w, &tr) - x;
    *h = snap_to(y + *h, &tb) - y;
}

#[allow(clippy::too_many_arguments)]
pub fn rects_overlap(
    ax: i32,
    ay: i32,
    aw: i32,
    ah: i32,
    bx: i32,
    by: i32,
    bw: i32,
    bh: i32,
) -> bool {
    ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
}

/// Checks if rectangle covers another placed module in the panel.
pub fn covers_other(panel: &Panel, mi: usize, x: i32, y: i32, w: i32, h: i32) -> bool {
    for (k, it) in panel.items.iter().enumerate() {
        if k == mi {
            continue;
        }
        if let Some((ox, oy, ow, oh)) = it.rect {
            if ow > 0 && oh > 0 && rects_overlap(x, y, w, h, ox, oy, ow, oh) {
                return true;
            }
        }
    }
    false
}

/// Finds the nearest free spot `(x, y)` for a `w x h` module in a panel `pw` wide without overlap.
pub fn nearest_free(panel: &Panel, mi: usize, w: i32, h: i32, pw: f64) -> (i32, i32) {
    let maxx = ((pw - PAD) as i32 - w).max(PAD as i32);
    let (mut best_x, mut best_y) = (PAD as i32, PAD as i32);
    let mut best_dist = -1i64;

    let (bx, by) = panel
        .items
        .get(mi)
        .and_then(|it| it.rect)
        .map(|(x, y, _, _)| (x, y))
        .unwrap_or((PAD as i32, PAD as i32));

    let mut yy = PAD as i32;
    while yy < 4000 {
        let dy = (yy - by) as i64;
        if best_dist >= 0 && dy * dy >= best_dist {
            break;
        }
        let mut xx = PAD as i32;
        while xx <= maxx {
            let dx = (xx - bx) as i64;
            let d = dx * dx + dy * dy;
            if (best_dist < 0 || d < best_dist) && !covers_other(panel, mi, xx, yy, w, h) {
                best_dist = d;
                best_x = xx;
                best_y = yy;
            }
            xx += 2;
        }
        yy += 2;
    }
    (best_x, best_y)
}

/// Default dimensions `(w, h)` for a newly added module in a panel `pw` wide.
pub fn module_default_size(m: Module, pw: f64) -> (i32, i32) {
    let full = (pw - 2.0 * PAD) as i32;
    let half = ((pw - 2.0 * PAD - GAP) / 2.0).floor() as i32;
    let w = if m.is_switch() { half } else { full };
    let h = match m {
        Module::Search => 40,
        Module::Clock => 34,
        Module::Volume => 32,
        Module::Media => 76,
        Module::Favorites => 88,
        Module::Apps | Module::Windows | Module::Clipboard => 32 * 8,
        Module::Tray => 84,
        Module::Notifications => 120,
        Module::Session => 32,
        _ => 46,
    };
    (w, h)
}

/// Human name of a module shown on the Add menu.
pub fn module_display_name(m: Module) -> &'static str {
    match m {
        Module::Search => "Search",
        Module::Favorites => "Favorites",
        Module::Apps => "Apps",
        Module::Windows => "Windows",
        Module::Clipboard => "Clipboard",
        Module::Clock => "Clock",
        Module::Volume => "Volume",
        Module::Media => "Media",
        Module::Wifi => "Wi-Fi",
        Module::Bluetooth => "Bluetooth",
        Module::Dnd => "Do not disturb",
        Module::Night => "Night light",
        Module::Keyboard => "Keyboard layout",
        Module::Osk => "Screen keyboard",
        Module::Power => "Power mode",
        Module::Replay => "Replay",
        Module::Tray => "Background apps",
        Module::Notifications => "Notifications",
        Module::Session => "Restart, shut down",
        Module::Calendar => "Calendar",
        Module::Memory => "Memory",
        Module::System => "System",
    }
}

/// All available modules that can be added to a panel.
pub const ALL_MODULES: &[Module] = &[
    Module::Search,
    Module::Favorites,
    Module::Apps,
    Module::Windows,
    Module::Clipboard,
    Module::Clock,
    Module::Volume,
    Module::Media,
    Module::Wifi,
    Module::Bluetooth,
    Module::Dnd,
    Module::Night,
    Module::Keyboard,
    Module::Osk,
    Module::Power,
    Module::Replay,
    Module::Tray,
    Module::Notifications,
    Module::Session,
    Module::Calendar,
    Module::Memory,
    Module::System,
];

/// Returns index of module under point `(px, py)` in panel coordinates, or `None`.
pub fn module_under(panel: &Panel, px: f64, py: f64) -> Option<usize> {
    for (i, it) in panel.items.iter().enumerate().rev() {
        if let Some((x, y, w, h)) = it.rect {
            let (x, y, w, h) = (x as f64, y as f64, w as f64, h as f64);
            if w > 0.0 && h > 0.0 && px >= x && px <= x + w && py >= y && py <= y + h {
                return Some(i);
            }
        }
    }
    None
}

/// Freezes current layout coordinates into module `rect`s if panel was unplaced.
pub fn freeze_layout(panel: &mut Panel, laid_out_rects: &[(f64, f64, f64, f64)], pw: f64) {
    for (it, r) in panel.items.iter_mut().zip(laid_out_rects) {
        if r.3 > 0.0 && r.2 > 0.0 {
            it.rect = Some((
                r.0.round() as i32,
                r.1.round() as i32,
                r.2.round() as i32,
                r.3.round() as i32,
            ));
        } else {
            it.rect = Some((PAD as i32, PAD as i32, 0, 0));
        }
    }
    panel.placed = true;
    panel.height = 0;
    if panel.width <= 0 {
        panel.width = pw.round() as i32;
    }
}

/// Computes the edit bar rectangle `(bx, by, bw, bh)` in screen coordinates.
pub fn calculate_bar_rect(
    panel: &Panel,
    origin: (f64, f64),
    pw: f64,
    ph: f64,
    screen: (f64, f64),
    adding: bool,
) -> (f64, f64, f64, f64) {
    let (ox, oy) = origin;
    let (sw, sh) = screen;
    let bw = pw.max(470.0);
    let bh = 34.0;
    let na = ALL_MODULES
        .iter()
        .filter(|&&m| !panel.items.iter().any(|it| it.module == m))
        .count();
    let gh = if adding {
        8.0 + (na as f64 / 4.0).ceil() * 30.0
    } else {
        0.0
    };

    let mut bx = ox
        + if panel.align > 0 && panel.side.horizontal() {
            pw - bw
        } else {
            0.0
        };
    if bx + bw > sw - 8.0 {
        bx = sw - 8.0 - bw;
    }
    if bx < 8.0 {
        bx = 8.0;
    }

    let mut by = oy + ph + 8.0;
    if by + bh + gh > sh - 8.0 {
        by = oy - 8.0 - bh - gh;
    }
    (bx, by, bw, bh + gh)
}

/// Draws a button tile on the edit overlay.
#[allow(clippy::too_many_arguments)]
fn draw_edit_button(
    cr: &Context,
    th: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    label: &str,
    on: bool,
    dim: bool,
) {
    draw::round_rect(cr, x, y, w, h, th.small_radius());
    if on {
        draw::set(cr, th.accent.alpha(0.25));
        let _ = cr.fill_preserve();
        draw::set(cr, th.accent);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    } else {
        draw::set(cr, th.card);
        let _ = cr.fill_preserve();
        draw::set(cr, th.edge);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    }

    let color = if dim {
        th.muted.alpha(0.40)
    } else if on {
        th.accent
    } else {
        th.text.alpha(0.90)
    };
    draw::text_center(
        cr,
        x + w / 2.0,
        y + (h - 13.0) / 2.0,
        &Text {
            size: 11.5,
            bold: on,
            color,
            text: label,
        },
    );
}

/// Draws edit overlay: dashed panel outline, thin module outlines, selected module with
/// 10 px corner handle and Remove button, placement bar for dragged module, and the edit bar.
#[allow(clippy::too_many_arguments)]
pub fn draw_edit_overlay(
    cr: &Context,
    th: &Theme,
    panel: &Panel,
    origin: (f64, f64),
    pw: f64,
    ph: f64,
    _screen: (f64, f64),
    state: &EditState,
) {
    let (ox, oy) = origin;
    let dash = [4.0, 3.0];

    // 1. Panel outline: dashed in accent
    draw::round_rect(cr, ox + 0.5, oy + 0.5, pw - 1.0, ph - 1.0, th.corners);
    draw::set(cr, th.accent);
    cr.set_line_width(1.5);
    cr.set_dash(&dash, 0.0);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);

    // 2. Each module's outline
    for (i, it) in panel.items.iter().enumerate() {
        let Some((mx, my, mw, mh)) = it.rect else {
            continue;
        };
        if mw <= 0 || mh <= 0 {
            continue;
        }
        let (x, y, w, h) = (ox + mx as f64, oy + my as f64, mw as f64, mh as f64);

        let is_selected = state.selected == Some(i);
        let is_hovered = state.hover_module == Some(i) && state.drag.is_none();

        if is_selected {
            // Selected module: light accent fill + solid 2px outline + 10px corner handle + Remove
            draw::round_rect(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, th.corners.min(7.0));
            draw::set(cr, th.accent.alpha(0.12));
            let _ = cr.fill_preserve();
            draw::set(cr, th.accent);
            cr.set_line_width(2.0);
            let _ = cr.stroke();

            // 10 px corner handle at bottom-right
            draw::set(cr, th.accent);
            for k in 0..3 {
                let off = 3.0 + 3.5 * k as f64;
                cr.move_to(x + w - 3.0 - off, y + h - 3.0);
                cr.line_to(x + w - 3.0, y + h - 3.0 - off);
            }
            cr.set_line_width(1.5);
            let _ = cr.stroke();

            // Remove button
            let rw = 58.0;
            let rh = 18.0;
            let rx = x + w - rw - 4.0;
            let ry = if w < rw + 10.0 || h < rh + 10.0 {
                y - rh - 3.0
            } else {
                y + 4.0
            };
            draw_edit_button(cr, th, rx, ry, rw, rh, "Remove", false, false);
        } else {
            // Normal / Hovered outline
            draw::round_rect(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, th.corners.min(7.0));
            if is_hovered {
                draw::set(cr, th.accent.alpha(0.95));
                cr.set_line_width(1.5);
                let _ = cr.stroke();
            } else {
                draw::set(cr, th.accent.alpha(0.45));
                cr.set_line_width(1.0);
                cr.set_dash(&dash, 0.0);
                let _ = cr.stroke();
                cr.set_dash(&[], 0.0);
            }
        }
    }

    // 3. Placement bar where a dragged module would land
    if state.live {
        if let Some(ref drag) = state.drag {
            if let DragKind::MoveModule(mi) = drag.kind {
                if let Some(it) = panel.items.get(mi) {
                    if let Some((nx, ny, nw, nh)) = it.rect {
                        let px = ox + nx as f64;
                        let py = oy + ny as f64;
                        let pw_m = nw as f64;
                        let ph_m = nh as f64;

                        // Landing placement bar / rectangle
                        draw::round_rect(
                            cr,
                            px + 0.5,
                            py + 0.5,
                            pw_m - 1.0,
                            ph_m - 1.0,
                            th.corners.min(7.0),
                        );
                        draw::set(cr, th.accent.alpha(0.18));
                        let _ = cr.fill_preserve();
                        draw::set(cr, th.accent);
                        cr.set_line_width(2.0);
                        cr.set_dash(&dash, 0.0);
                        let _ = cr.stroke();
                        cr.set_dash(&[], 0.0);
                    }
                }
            }
        }
    }

    // 4. Panel corner resize handle
    let cx = if panel.align > 0 && panel.side.horizontal() {
        ox + 4.0
    } else {
        ox + pw - 4.0
    };
    let cy = if panel.side == Side::Bottom {
        oy + 4.0
    } else {
        oy + ph - 4.0
    };
    draw::set(cr, th.muted);
    cr.rectangle(cx - 3.0, cy - 3.0, 6.0, 6.0);
    let _ = cr.fill();

    // 5. The Edit Bar
    let (bx, by, bw, bh) = state.bar_rect;
    draw::card(cr, th, bx, by, bw, bh);

    let labels = [
        "Undo",
        "Reset",
        if state.snap { "Snap on" } else { "Snap off" },
        if state.overlap {
            "Overlap on"
        } else {
            "Overlap off"
        },
        "Add",
        "Done",
    ];

    let w6 = (bw - 10.0 - 5.0 * 4.0) / 6.0;
    for (i, label) in labels.iter().enumerate() {
        let is_on = (i == 2 && state.snap)
            || (i == 3 && state.overlap)
            || (i == 4 && state.adding)
            || i == 5;
        let is_dim = i == 0 && state.undo.is_empty();
        let btn_x = bx + 5.0 + i as f64 * (w6 + 4.0);
        let btn_y = by + 5.0;
        draw_edit_button(cr, th, btn_x, btn_y, w6, 24.0, label, is_on, is_dim);
    }

    // If Add is active: grid of modules not in the panel
    if state.adding {
        let available: Vec<Module> = ALL_MODULES
            .iter()
            .copied()
            .filter(|&m| !panel.items.iter().any(|it| it.module == m))
            .collect();
        let w4 = (bw - 10.0 - 3.0 * 4.0) / 4.0;
        for (k, &m) in available.iter().enumerate() {
            let col = k % 4;
            let row = k / 4;
            let ax = bx + 5.0 + col as f64 * (w4 + 4.0);
            let ay = by + 34.0 + 4.0 + row as f64 * 30.0;
            draw_edit_button(
                cr,
                th,
                ax,
                ay,
                w4,
                26.0,
                module_display_name(m),
                false,
                false,
            );
        }
    }
}

/// Rewrites `PANEL_<NAME>=...` in `config_text` and appends backup line to `panels.backup` format.
#[allow(dead_code)]
pub fn format_config_update(config_text: &str, panel: &Panel) -> String {
    let key = format!("PANEL_{}=", panel.name.to_ascii_uppercase());
    let new_line = format!(
        "PANEL_{}=\"{}\"",
        panel.name.to_ascii_uppercase(),
        panel.value()
    );
    let mut out = String::with_capacity(config_text.len() + new_line.len() + 16);
    let mut found = false;

    for line in config_text.lines() {
        if line.starts_with(&key) {
            out.push_str(&new_line);
            out.push('\n');
            found = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !found {
        out.push_str(&new_line);
        out.push('\n');
    }
    out
}

/// Commands to save updated panel line and panels.backup as C edge does.
pub fn save_panel_commands(panel: &Panel, before_val: &str) -> (String, String) {
    let name_up = panel.name.to_ascii_uppercase();
    let val = panel.value();

    let backup_cmd = format!(
        "d=\"$HOME/.config/infynitae\"; [ -d \"$d\" ] || exit 0; \
         printf '%s PANEL_{}=\"{}\"\\n' \"$(date '+%F %T')\" >> \"$d/panels.backup\"",
        name_up, before_val
    );

    let save_cmd = format!(
        "f=\"$HOME/.config/infynitae/config\"; [ -f \"$f\" ] || exit 0; \
         case \"{name_up}{val}\" in *[!A-Za-z0-9+:_@,\\ ]*) exit 0;; esac; \
         awk -v k=\"PANEL_{name_up}\" -v v=\"{val}\" 'BEGIN{{d=0}} index($0, k \"=\")==1{{print k \"=\\\"\" v \"\\\"\"; d=1; next}} {{print}} \
         END{{if(!d) print k \"=\\\"\" v \"\\\"\"}}' \"$f\" > \"$f.tmp.$$\" && mv -f \"$f.tmp.$$\" \"$f\""
    );

    (backup_cmd, save_cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Item;

    #[test]
    fn test_snap_to_threshold() {
        let targets = [10, 50, 100];
        assert_eq!(snap_to(14, &targets), 10);
        assert_eq!(snap_to(18, &targets), 10);
        assert_eq!(snap_to(19, &targets), 19); // distance 9 is outside 8 px threshold
        assert_eq!(snap_to(45, &targets), 50);
        assert_eq!(snap_to(106, &targets), 100);
    }

    #[test]
    fn test_snap_move_and_size() {
        let p = Panel {
            name: "control".into(),
            side: Side::Top,
            align: 1,
            width: 360,
            height: 400,
            hover: false,
            key: None,
            items: vec![
                Item {
                    module: Module::Clock,
                    look: None,
                    rect: Some((8, 8, 344, 34)),
                },
                Item {
                    module: Module::Volume,
                    look: None,
                    rect: Some((8, 100, 344, 32)),
                },
            ],
            placed: true,
        };

        // Moving Volume near y=48 (which is Clock bottom at 8 + 34 + GAP 6 = 48)
        let mut x = 12;
        let mut y = 50;
        snap_move(&p, 1, 360.0, &mut x, &mut y, 344, 32);
        assert_eq!(x, 8); // snapped to PAD (8)
        assert_eq!(y, 8 + 34 + 6); // snapped to clock bottom + GAP = 48!

        let mut w = 340;
        let mut h = 30;
        snap_size(&p, 0, 360.0, 8, 8, &mut w, &mut h);
        assert_eq!(w, 360 - 8 - 8); // snapped to pw - PAD = 344!
    }

    #[test]
    fn test_covers_other_and_nearest_free() {
        let p = Panel {
            name: "test".into(),
            side: Side::Top,
            align: 0,
            width: 360,
            height: 400,
            hover: false,
            key: None,
            items: vec![
                Item {
                    module: Module::Clock,
                    look: None,
                    rect: Some((8, 8, 344, 34)),
                },
                Item {
                    module: Module::Volume,
                    look: None,
                    rect: Some((8, 48, 170, 46)),
                },
            ],
            placed: true,
        };

        // (8, 20) overlaps Clock (8, 8, 344, 34)
        assert!(covers_other(&p, 2, 8, 20, 100, 30));
        // (184, 48) does not overlap Clock (8, 8, 344, 34) nor Volume (8, 48, 170, 46)
        assert!(!covers_other(&p, 2, 184, 48, 100, 30));

        let free = nearest_free(&p, 2, 160, 40, 360.0);
        assert!(!covers_other(&p, 2, free.0, free.1, 160, 40));
    }

    #[test]
    fn test_format_config_update() {
        let p = Panel {
            name: "control".into(),
            side: Side::Top,
            align: 1,
            width: 360,
            height: 400,
            hover: true,
            key: Some('a'),
            items: vec![
                Item {
                    module: Module::Clock,
                    look: None,
                    rect: Some((8, 8, 344, 34)),
                },
                Item {
                    module: Module::Volume,
                    look: None,
                    rect: Some((8, 48, 344, 32)),
                },
            ],
            placed: true,
        };

        let config = "EDGE_POSITION=\"top\"\nPANEL_CONTROL=\"old\"\nFAVORITES=\"app\"\n";
        let updated = format_config_update(config, &p);
        assert!(updated.contains(
            "PANEL_CONTROL=\"SUPER+A top right 360x400 hover clock@8,8,344x34 volume@8,48,344x32\""
        ));
        assert!(updated.contains("EDGE_POSITION=\"top\""));
        assert!(updated.contains("FAVORITES=\"app\""));
    }
}
