//! The `tray` module: StatusNotifierItem background apps over D-Bus (zbus).
//!
//! Title "Running in the background" with "Right click: menu" muted on the right.
//! Apps displayed in two columns as words:
//! - App's name: 13 px (from item's Title or Id)
//! - Short status muted on the right (from Status, tooltip, or attention; e.g. "2 new" in accent when asking for attention)
//!
//! Clicks:
//! - Left click: activates the app (`Activate(0, 0)`)
//! - Right click: opens its context menu (`com.canonical.dbusmenu`) beside the panel
//!
//! Hidden when there are no background apps.

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{is_left_button, Rect, World};
use crate::theme::Theme;
use cairo::Context;

pub const TITLE_H: f64 = 26.0;
pub const ROW_H: f64 = 26.0;
pub const COL_GAP: f64 = 12.0;
pub const MENU_W: f64 = 176.0;
pub const MENU_ROW_H: f64 = 26.0;

pub const CMDS: &[(&str, &str)] = &[("tray", "__zbus_tray__")];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub is_separator: bool,
}

impl Default for TrayMenuItem {
    fn default() -> Self {
        Self {
            id: 0,
            label: String::new(),
            enabled: true,
            is_separator: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub attention: bool,
    pub bus_name: String,
    pub obj_path: String,
    pub menu_path: String,
    pub pid: u32,
    pub menu_items: Vec<TrayMenuItem>,
}

impl Default for TrayItem {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            status: String::new(),
            attention: false,
            bus_name: String::new(),
            obj_path: "/StatusNotifierItem".to_string(),
            menu_path: String::new(),
            pid: 0,
            menu_items: Vec::new(),
        }
    }
}

impl TrayItem {
    /// What is said beside the name: nothing for the plain states every app has ("Active",
    /// "Passive"), "needs you" for "NeedsAttention", any other status (e.g. "2 new") as it is.
    pub fn note(&self) -> &str {
        let s = self.status.trim();
        if s.eq_ignore_ascii_case("active") || s.eq_ignore_ascii_case("passive") {
            ""
        } else if s.eq_ignore_ascii_case("needsattention") {
            "needs you"
        } else {
            s
        }
    }

    pub fn display_name(&self) -> &str {
        if !self.title.is_empty() {
            &self.title
        } else if !self.id.is_empty() {
            &self.id
        } else {
            "App"
        }
    }

    pub fn effective_menu_items(&self) -> Vec<TrayMenuItem> {
        if !self.menu_items.is_empty() {
            self.menu_items.clone()
        } else {
            // Default fallback actions matching the C edge
            vec![
                TrayMenuItem {
                    id: -1,
                    label: "Open".to_string(),
                    enabled: true,
                    is_separator: false,
                },
                TrayMenuItem {
                    id: -2,
                    label: "Quit".to_string(),
                    enabled: true,
                    is_separator: false,
                },
                TrayMenuItem {
                    id: -3,
                    label: "Force Kill".to_string(),
                    enabled: true,
                    is_separator: false,
                },
            ]
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TrayData {
    pub ready: bool,
    pub items: Vec<TrayItem>,
    pub open_menu: Option<usize>,
}

/// `$XDG_RUNTIME_DIR/infynitae/capturing`: the programs capturing the screen, one a line (the compositor
/// writes it while any captures, `CAPTURE_INDICATOR="edge"`).
fn read_capturing() -> String {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|d| {
            std::path::PathBuf::from(d)
                .join("infynitae")
                .join("capturing")
        })
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

/// The row that says the screen is being captured, and by what: "Screen capture" with the programs as its
/// status in the accent. None while nothing captures.
pub fn capture_item(capturing: &str) -> Option<TrayItem> {
    let names: Vec<&str> = capturing
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if names.is_empty() {
        return None;
    }
    Some(TrayItem {
        id: "screen-capture".into(),
        title: "Screen capture".into(),
        status: names.join(", "),
        attention: true,
        ..TrayItem::default()
    })
}

/// Applies raw output (from `run_sh` or `INFYNITAE_EDGE_RENDER_DATA`) to `TrayData`.
pub fn apply(d: &mut TrayData, key: &str, out: &str) -> bool {
    if key != "tray" {
        return false;
    }
    let mut items = parse_tray_data(out);
    // a screen capture is said here, first (the compositor draws no dot by default: CAPTURE_INDICATOR)
    if let Some(it) = capture_item(&read_capturing()) {
        items.insert(0, it);
    }
    // a menu fetched on a right click stays (the refresh every 2 s has none: it turned an app's
    // open menu into Open / Quit / Force Kill under the pointer); the open menu stays on its app
    for it in &mut items {
        if it.menu_items.is_empty() {
            if let Some(old) = d.items.iter().find(|o| o.bus_name == it.bus_name) {
                it.menu_items = old.menu_items.clone();
            }
        }
    }
    let open_bus = d
        .open_menu
        .and_then(|i| d.items.get(i))
        .map(|it| it.bus_name.clone());
    let changed = !d.ready || items != d.items;
    d.items = items;
    d.ready = true;
    d.open_menu = open_bus.and_then(|b| d.items.iter().position(|it| it.bus_name == b));
    changed
}

/// Strips mnemonic underscores from dbusmenu labels (e.g. `_Mute` -> `Mute`, `__` -> `_`).
pub fn clean_menu_label(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                out.push('_');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses tray lines (TSV, JSON, or key-value format).
pub fn parse_tray_data(out: &str) -> Vec<TrayItem> {
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // Try parsing as JSON array
    if trimmed.starts_with('[') {
        if let Some(items) = parse_json_tray(trimmed) {
            return items;
        }
    }

    let mut items: Vec<TrayItem> = Vec::new();
    for raw in out.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("SYS\t") {
            continue;
        }

        // Sub-item for menu of the last tray item: "menu:id\tlabel"
        if let Some(rest) = line.strip_prefix("menu:") {
            if let Some(last) = items.last_mut() {
                let parts: Vec<&str> = rest.split('\t').collect();
                let id = parts[0].trim().parse::<i32>().unwrap_or(0);
                let label = if parts.len() > 1 { parts[1] } else { "" };
                let is_sep = label.eq_ignore_ascii_case("separator") || label == "---";
                last.menu_items.push(TrayMenuItem {
                    id,
                    label: clean_menu_label(label),
                    enabled: true,
                    is_separator: is_sep,
                });
            }
            continue;
        }

        // TSV format: could be:
        // 1. "Name\tStatus"
        // 2. "Name\tStatus\tAttention"
        // 3. "Id\tTitle\tStatus\tAttention\tBus\tPath\tMenuPath"
        // 4. C-edge format: "anything\tproc_name\tmem\tpid"
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.is_empty() {
            continue;
        }

        if parts.len() >= 7 {
            let id = parts[0].trim().to_string();
            let title = parts[1].trim().to_string();
            let status = parts[2].trim().to_string();
            let att_str = parts[3].trim();
            let attention = att_str == "1"
                || att_str.eq_ignore_ascii_case("true")
                || att_str.eq_ignore_ascii_case("attention")
                || att_str.eq_ignore_ascii_case("yes")
                || status.to_ascii_lowercase().contains("new")
                || status.eq_ignore_ascii_case("needsattention");
            let bus_name = parts[4].trim().to_string();
            let obj_path = parts[5].trim().to_string();
            let menu_path = parts[6].trim().to_string();

            items.push(TrayItem {
                id: if id.is_empty() { title.clone() } else { id },
                title,
                status,
                attention,
                bus_name,
                obj_path: if obj_path.is_empty() {
                    "/StatusNotifierItem".to_string()
                } else {
                    obj_path
                },
                menu_path,
                pid: 0,
                menu_items: Vec::new(),
            });
        } else if parts.len() >= 4 && parts[3].trim().parse::<u32>().is_ok() {
            // C-edge format: field \t proc_name \t mem \t pid
            let proc_name = parts[1].trim();
            let mem = parts[2].trim();
            let pid = parts[3].trim().parse::<u32>().unwrap_or(0);
            let att = mem.to_ascii_lowercase().contains("new");
            items.push(TrayItem {
                id: proc_name.to_lowercase(),
                title: proc_name.to_string(),
                status: mem.to_string(),
                attention: att,
                bus_name: String::new(),
                obj_path: "/StatusNotifierItem".to_string(),
                menu_path: String::new(),
                pid,
                menu_items: Vec::new(),
            });
        } else {
            let name = parts[0].trim();
            let status = if parts.len() > 1 {
                parts[1].trim().to_string()
            } else {
                "Active".to_string()
            };
            let att = if parts.len() > 2 {
                let a = parts[2].trim();
                a == "1"
                    || a.eq_ignore_ascii_case("true")
                    || a.eq_ignore_ascii_case("attention")
                    || a.eq_ignore_ascii_case("yes")
            } else {
                status.to_ascii_lowercase().contains("new")
                    || status.eq_ignore_ascii_case("needsattention")
            };
            items.push(TrayItem {
                id: name.to_lowercase(),
                title: name.to_string(),
                status,
                attention: att,
                bus_name: String::new(),
                obj_path: "/StatusNotifierItem".to_string(),
                menu_path: String::new(),
                pid: 0,
                menu_items: Vec::new(),
            });
        }
    }
    items
}

fn parse_json_tray(text: &str) -> Option<Vec<TrayItem>> {
    let mut items = Vec::new();
    let mut in_obj = false;
    let mut obj_str = String::new();

    for c in text.chars() {
        if c == '{' {
            in_obj = true;
            obj_str.clear();
        } else if c == '}' && in_obj {
            in_obj = false;
            let id = extract_json_str(&obj_str, "id").unwrap_or_default();
            let title = extract_json_str(&obj_str, "title").unwrap_or_else(|| id.clone());
            let status = extract_json_str(&obj_str, "status").unwrap_or_else(|| "Active".into());
            let att = obj_str.contains("\"attention\":true")
                || obj_str.contains("\"attention\": true")
                || status.to_ascii_lowercase().contains("new")
                || status.eq_ignore_ascii_case("needsattention");
            let bus = extract_json_str(&obj_str, "bus").unwrap_or_default();
            let path =
                extract_json_str(&obj_str, "path").unwrap_or_else(|| "/StatusNotifierItem".into());
            let menu = extract_json_str(&obj_str, "menu_path").unwrap_or_default();

            if !title.is_empty() || !id.is_empty() {
                items.push(TrayItem {
                    id,
                    title,
                    status,
                    attention: att,
                    bus_name: bus,
                    obj_path: path,
                    menu_path: menu,
                    pid: 0,
                    menu_items: Vec::new(),
                });
            }
        } else if in_obj {
            obj_str.push(c);
        }
    }
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn extract_json_str(block: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let key_idx = block.find(&needle)?;
    let after = &block[key_idx + needle.len()..];
    let colon_idx = after.find(':')?;
    let quote_idx = after[colon_idx + 1..].find('"')?;
    let start = colon_idx + 1 + quote_idx + 1;
    let mut out = String::new();
    let mut chars = after[start..].chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            return Some(out);
        }
        if c == '\\' {
            if let Some(esc) = chars.next() {
                out.push(esc);
            }
        } else {
            out.push(c);
        }
    }
    None
}

/// The module's height given `world`: hidden (0.0) when there are no items.
pub fn height(world: &World) -> f64 {
    if !world.tray.ready || world.tray.items.is_empty() {
        return 0.0;
    }
    let rows = world.tray.items.len().div_ceil(2);
    2.0 * PAD_Y + TITLE_H + (rows as f64) * ROW_H
}

/// Draws the `tray` card and background apps list in two columns.
pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if world.tray.items.is_empty() {
        return;
    }

    draw::card(cr, th, x, y, w, h);
    draw::title(cr, th, x + PAD_X, y + PAD_Y, "Running in the background");
    draw::text_right(
        cr,
        x + w - PAD_X,
        y + PAD_Y + 2.0,
        &Text {
            size: 11.5,
            bold: false,
            color: th.muted,
            text: "Right click: menu",
        },
    );

    let inner_w = (w - 2.0 * PAD_X).max(0.0);
    let col_w = ((inner_w - COL_GAP) / 2.0).max(40.0);
    let start_y = y + PAD_Y + TITLE_H;

    for (i, item) in world.tray.items.iter().enumerate() {
        let row = i / 2;
        let col = i % 2;
        let ix = x + PAD_X + (col as f64) * (col_w + COL_GAP);
        let iy = start_y + (row as f64) * ROW_H;

        if iy + ROW_H > y + h {
            break;
        }

        let is_selected = world.tray.open_menu == Some(i);
        super::hot(cr, th, world, ix - 4.0, iy, col_w + 8.0, ROW_H - 2.0, th.small_radius());
        if is_selected {
            draw::round_rect(
                cr,
                ix - 4.0,
                iy,
                col_w + 8.0,
                ROW_H - 2.0,
                th.small_radius(),
            );
            draw::set(cr, th.track);
            let _ = cr.fill();
        }

        // Measure status width to allocate space
        let status_color = if item.attention { th.accent } else { th.muted };
        let status_t = Text {
            size: 11.5,
            bold: item.attention,
            color: status_color,
            text: item.note(),
        };
        let (sw, _) = if status_t.text.is_empty() {
            (0.0, 0.0)
        } else {
            draw::measure(cr, &status_t)
        };
        let name_max_w = (col_w - sw - 8.0).max(20.0);

        // App name: 13 px from item's Title or Id
        let name_t = Text {
            size: 13.0,
            bold: false,
            color: th.text,
            text: item.display_name(),
        };
        draw::text_max(cr, ix, iy + 4.0, name_max_w, &name_t);

        // Status: muted on the right, or accent when asking for attention
        if !status_t.text.is_empty() {
            draw::text_right(cr, ix + col_w, iy + 4.0, &status_t);
        }
    }
}

/// Computes the screen hitbox of item `i` inside card of `size` at local origin `(0, 0)`.
pub fn item_slot(size: (f64, f64), i: usize) -> Option<Rect> {
    let (w, h) = size;
    let inner_w = (w - 2.0 * PAD_X).max(0.0);
    let col_w = ((inner_w - COL_GAP) / 2.0).max(40.0);
    let start_y = PAD_Y + TITLE_H;
    let row = i / 2;
    let col = i % 2;
    let ix = PAD_X + (col as f64) * (col_w + COL_GAP);
    let iy = start_y + (row as f64) * ROW_H;
    if iy + ROW_H > h {
        return None;
    }
    Some((ix, iy, col_w, ROW_H))
}

/// Dispatches clicks on the tray card:
/// - Left click: activates app (`tray:activate:NAME`)
/// - Right click: opens its menu (`tray:menu-open:NAME`)
pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if world.tray.items.is_empty() {
        return false;
    }
    let (px, py) = pos;

    // Check which item was clicked
    for i in 0..world.tray.items.len() {
        if let Some((sx, sy, sw, sh)) = item_slot(size, i) {
            if px >= sx && px <= sx + sw && py >= sy && py <= sy + sh {
                // the capture row says something; it is no app to activate, and its fallback menu
                // (Quit, Force Kill) would have no process to act on
                if world.tray.items[i].id == "screen-capture" {
                    return true;
                }
                if is_left_button(button) {
                    let id = world.tray.items[i].id.clone();
                    let bus = world.tray.items[i].bus_name.clone();
                    let path = world.tray.items[i].obj_path.clone();
                    world.note(format!("tray:activate:{id}"));
                    if world.live && !bus.is_empty() {
                        activate_dbus_item(&bus, &path);
                    }
                    world.tray.open_menu = None;
                    return true;
                } else if button == 3 || button == 0x111 {
                    // Right click opens menu
                    if world.tray.open_menu == Some(i) {
                        world.tray.open_menu = None;
                    } else {
                        world.tray.open_menu = Some(i);
                        let id = world.tray.items[i].id.clone();
                        let bus = world.tray.items[i].bus_name.clone();
                        let menu_path = world.tray.items[i].menu_path.clone();
                        world.note(format!("tray:menu-open:{id}"));
                        if world.live && !bus.is_empty() && !menu_path.is_empty() {
                            let items = fetch_dbusmenu_items(&bus, &menu_path);
                            if !items.is_empty() {
                                world.tray.items[i].menu_items = items;
                            }
                        }
                    }
                    return true;
                }
            }
        }
    }

    // Click inside the card but not on an item closes open menu
    if world.tray.open_menu.is_some() {
        world.tray.open_menu = None;
        return true;
    }
    false
}

/// Calculates the rectangle for the tray context menu beside the panel.
pub fn menu_rect(
    panel_origin: (f64, f64),
    panel_size: (f64, f64),
    card_rect: Rect,
    num_items: usize,
    screen_size: (f64, f64),
) -> Rect {
    let (ox, oy) = panel_origin;
    let (pw, _) = panel_size;
    let (sw, sh) = screen_size;

    let items_count = num_items.max(1);
    let menu_h = 16.0 + (items_count as f64) * MENU_ROW_H;
    let menu_w = MENU_W;

    // Prefer placing to the right of panel, or to the left if overflowing
    let menu_x = if ox + pw + menu_w + 8.0 <= sw {
        ox + pw + 8.0
    } else {
        (ox - menu_w - 8.0).max(8.0)
    };

    let card_top = oy + card_rect.1;
    let menu_y = card_top.clamp(8.0, (sh - menu_h - 8.0).max(8.0));

    (menu_x, menu_y, menu_w, menu_h)
}

/// Draws the context menu as a simple list beside the panel.
pub fn draw_menu(cr: &Context, th: &Theme, world: &World, m_rect: Rect) {
    let Some(open_idx) = world.tray.open_menu else {
        return;
    };
    let Some(item) = world.tray.items.get(open_idx) else {
        return;
    };

    let (mx, my, mw, mh) = m_rect;
    draw::card(cr, th, mx, my, mw, mh);

    let menu_items = item.effective_menu_items();
    let start_y = my + 8.0;

    for (k, mi) in menu_items.iter().enumerate() {
        let row_y = start_y + (k as f64) * MENU_ROW_H;
        if row_y + MENU_ROW_H > my + mh {
            break;
        }

        if mi.is_separator {
            draw::round_rect(cr, mx + 12.0, row_y + 12.0, mw - 24.0, 1.0, 0.5);
            draw::set(cr, th.track);
            let _ = cr.fill();
        } else {
            if mi.enabled {
                super::hot(
                    cr,
                    th,
                    world,
                    mx + 6.0,
                    row_y,
                    mw - 12.0,
                    MENU_ROW_H,
                    th.small_radius(),
                );
            }
            let color = if mi.enabled { th.text } else { th.muted };
            draw::text(
                cr,
                mx + 14.0,
                row_y + 4.0,
                &Text {
                    size: 13.0,
                    bold: false,
                    color,
                    text: &mi.label,
                },
            );
        }
    }
}

/// Handles clicks on the open tray context menu beside the panel.
pub fn click_menu(world: &mut World, m_rect: Rect, pos: (f64, f64)) -> bool {
    let Some(open_idx) = world.tray.open_menu else {
        return false;
    };
    let (mx, my, mw, mh) = m_rect;
    let (px, py) = pos;

    if px < mx || px > mx + mw || py < my || py > my + mh {
        world.tray.open_menu = None;
        return false;
    }

    let Some(item) = world.tray.items.get(open_idx) else {
        world.tray.open_menu = None;
        return false;
    };

    let menu_items = item.effective_menu_items();
    let row = ((py - (my + 8.0)) / MENU_ROW_H) as usize;

    if row < menu_items.len() {
        let mi = &menu_items[row];
        if !mi.is_separator && mi.enabled {
            let app_id = item.id.clone();
            let bus = item.bus_name.clone();
            let menu_path = item.menu_path.clone();
            let pid = item.pid;
            let action_id = mi.id;

            world.note(format!("tray:menu-action:{app_id}:{action_id}"));
            if world.live {
                execute_dbusmenu_action(&bus, &menu_path, action_id, pid, &app_id);
            }
            world.tray.open_menu = None;
            return true;
        }
    }

    world.tray.open_menu = None;
    true
}

// =============================================================================
// Live D-Bus (zbus) interactions
// =============================================================================

/// Live activation of a StatusNotifierItem via D-Bus.
/// [`activate_dbus_item_blocking`] on a helper thread: an app that does not answer never freezes the
/// panel for the D-Bus timeout (it ran on the Wayland thread).
pub fn activate_dbus_item(bus: &str, path: &str) {
    let bus = bus.to_string();
    let path = path.to_string();
    std::thread::spawn(move || activate_dbus_item_blocking(&bus, &path));
}

fn activate_dbus_item_blocking(bus: &str, path: &str) {
    if let Ok(conn) = zbus::blocking::Connection::session() {
        let path = if path.is_empty() {
            "/StatusNotifierItem"
        } else {
            path
        };
        if let Ok(proxy) =
            zbus::blocking::Proxy::new(&conn, bus, path, "org.kde.StatusNotifierItem")
        {
            let _ = proxy.call::<&str, (i32, i32), ()>("Activate", &(0, 0));
        }
    }
}

type DbusMenuItem = (
    i32,
    std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    Vec<zbus::zvariant::OwnedValue>,
);
type DbusMenuLayout = (u32, DbusMenuItem);

/// Queries menu items over `com.canonical.dbusmenu`.
pub fn fetch_dbusmenu_items(bus: &str, path: &str) -> Vec<TrayMenuItem> {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return Vec::new();
    };
    let Ok(proxy) = zbus::blocking::Proxy::new(&conn, bus, path, "com.canonical.dbusmenu") else {
        return Vec::new();
    };

    let _ = proxy.call::<&str, (i32,), ()>("AboutToShow", &(0,));
    let props = ["label", "visible", "enabled", "type"];
    // GetLayout(parentId: i32, recursionDepth: i32, propertyNames: &[&str])
    // returns (revision: u32, layout: (id: i32, properties: a{sv}, children: av))
    let layout_res: zbus::Result<DbusMenuLayout> =
        proxy.call("GetLayout", &(0i32, 1i32, &props[..]));

    let Ok((_, (_, _, children))) = layout_res else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for child in children {
        if let Ok((cid, cprops, _)) = DbusMenuItem::try_from(child) {
            let label = cprops
                .get("label")
                .and_then(|v| <&str>::try_from(v).ok())
                .unwrap_or("")
                .to_string();
            let is_sep = cprops
                .get("type")
                .and_then(|v| <&str>::try_from(v).ok())
                .map(|t| t == "separator")
                .unwrap_or(false);
            let visible = cprops
                .get("visible")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(true);
            let enabled = cprops
                .get("enabled")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(true);

            if visible && (!label.is_empty() || is_sep) {
                out.push(TrayMenuItem {
                    id: cid,
                    label: clean_menu_label(&label),
                    enabled,
                    is_separator: is_sep,
                });
            }
        }
    }
    out
}

/// Executes a click or action on a dbusmenu item.
/// [`execute_dbusmenu_action_blocking`] on a helper thread: an app that does not answer never freezes the
/// panel for the D-Bus timeout (it ran on the Wayland thread).
pub fn execute_dbusmenu_action(
    bus: &str,
    menu_path: &str,
    item_id: i32,
    pid: u32,
    proc_name: &str,
) {
    let bus = bus.to_string();
    let menu_path = menu_path.to_string();
    let proc_name = proc_name.to_string();
    std::thread::spawn(move || {
        execute_dbusmenu_action_blocking(&bus, &menu_path, item_id, pid, &proc_name)
    });
}

fn execute_dbusmenu_action_blocking(
    bus: &str,
    menu_path: &str,
    item_id: i32,
    pid: u32,
    proc_name: &str,
) {
    if item_id >= 0 {
        if let Ok(conn) = zbus::blocking::Connection::session() {
            if let Ok(proxy) =
                zbus::blocking::Proxy::new(&conn, bus, menu_path, "com.canonical.dbusmenu")
            {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as u32)
                    .unwrap_or(0);
                let _ = proxy.call::<&str, (i32, &str, zbus::zvariant::Value<'_>, u32), ()>(
                    "Event",
                    &(item_id, "clicked", zbus::zvariant::Value::I32(0), now),
                );
            }
        }
    } else if item_id == -1 {
        // Open app
        // the app's program by its name, started directly (no shell: the name comes from the
        // app); without one, nothing
        let _ = bus;
        if !proc_name.is_empty() && !proc_name.contains('/') {
            let _ = std::process::Command::new(proc_name)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    } else if item_id == -2 {
        // Quit (SIGTERM)
        if pid > 0 {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        } else if !proc_name.is_empty() {
            let _ = std::process::Command::new("pkill")
                .arg("-TERM")
                .arg(proc_name)
                .spawn();
        }
    } else if item_id == -3 {
        // Force Kill (SIGKILL)
        if pid > 0 {
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        } else if !proc_name.is_empty() {
            let _ = std::process::Command::new("pkill")
                .arg("-KILL")
                .arg(proc_name)
                .spawn();
        }
    }
}

/// Fetches background apps list from StatusNotifierWatcher over D-Bus as TSV lines.
/// The session bus connection the tray fetch keeps between fetches (it ran every 2 s on a fresh
/// connection and registered again each time: review 22), and how many fetches since it last
/// registered as a host.
static TRAY_CONN: std::sync::Mutex<Option<(zbus::blocking::Connection, u32)>> =
    std::sync::Mutex::new(None);

/// Register as a host again after this many fetches (about a minute), in case the watcher restarted.
const REREGISTER_EVERY: u32 = 30;

pub fn fetch_dbus_string() -> String {
    let out = fetch_with_kept_connection();
    if out.is_none() {
        // the bus or the watcher went away: start over next time
        if let Ok(mut g) = TRAY_CONN.lock() {
            *g = None;
        }
    }
    out.unwrap_or_default()
}

fn fetch_with_kept_connection() -> Option<String> {
    let (conn, register) = {
        let mut g = TRAY_CONN.lock().ok()?;
        if g.is_none() {
            *g = Some((zbus::blocking::Connection::session().ok()?, 0));
        }
        let (conn, n) = g.as_mut()?;
        let register = *n == 0;
        *n = (*n + 1) % REREGISTER_EVERY;
        (conn.clone(), register)
    };
    fetch_on(&conn, register)
}

fn fetch_on(conn: &zbus::blocking::Connection, register: bool) -> Option<String> {
    let conn = conn.clone();

    // Register as host with StatusNotifierWatcher
    let watcher_proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    );

    let Ok(watcher) = watcher_proxy else {
        return None;
    };

    if register {
        let _ = watcher
            .call::<&str, (&str,), ()>("RegisterStatusNotifierHost", &("infynitae-edge-rs",));
    }
    // a fresh proxy each fetch reads the property anew (zbus caches it per proxy)
    let Ok(item_names) = watcher.get_property::<Vec<String>>("RegisteredStatusNotifierItems")
    else {
        return None;
    };

    let mut lines = Vec::new();
    for full in item_names {
        let (bus_name, obj_path) = if let Some(slash_idx) = full.find('/') {
            (&full[..slash_idx], &full[slash_idx..])
        } else {
            (full.as_str(), "/StatusNotifierItem")
        };

        if let Ok(proxy) =
            zbus::blocking::Proxy::new(&conn, bus_name, obj_path, "org.kde.StatusNotifierItem")
        {
            let id: String = proxy.get_property("Id").unwrap_or_default();
            let title: String = proxy.get_property("Title").unwrap_or_default();
            let status: String = proxy
                .get_property("Status")
                .unwrap_or_else(|_| "Active".into());
            let menu: String = proxy
                .get_property::<zbus::zvariant::OwnedObjectPath>("Menu")
                .map(|p| p.to_string())
                .unwrap_or_default();

            let att = status.eq_ignore_ascii_case("needsattention")
                || status.to_ascii_lowercase().contains("new");
            lines.push(format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                id,
                title,
                status,
                if att { "1" } else { "0" },
                bus_name,
                obj_path,
                menu
            ));
        }
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_capture_is_a_row_naming_its_programs_and_nothing_else() {
        assert!(super::capture_item("").is_none());
        assert!(super::capture_item("\n  \n").is_none());
        let it = super::capture_item("obs\nfirefox\n").unwrap();
        assert_eq!(it.display_name(), "Screen capture");
        assert_eq!(it.note(), "obs, firefox");
        assert!(it.attention, "in the accent");
        let mut world = crate::modules::World::default();
        world.tray.items = vec![it];
        world.tray.ready = true;
        let size = (380.0, super::height(&world));
        let (x, y, w, h) = super::item_slot(size, 0).unwrap();
        let at = (x + w / 2.0, y + h / 2.0);
        assert!(super::click(&mut world, size, at, 0x111));
        assert_eq!(world.tray.open_menu, None, "no menu for it");
    }

    use super::*;

    #[test]
    fn notes_leave_out_the_plain_states() {
        let mut it = parse_tray_data("Steam\tActive\n").remove(0);
        assert_eq!(it.note(), "");
        it.status = "Passive".into();
        assert_eq!(it.note(), "");
        it.status = "NeedsAttention".into();
        assert_eq!(it.note(), "needs you");
        it.status = "2 new".into();
        assert_eq!(it.note(), "2 new");
    }

    #[test]
    fn parses_tsv_and_c_edge_format_tray_data() {
        let input = "Steam\t2159 MB\nDiscord\t2 new\tattention\nRustDesk\tIdle\n";
        let items = parse_tray_data(input);
        assert_eq!(items.len(), 3);

        assert_eq!(items[0].display_name(), "Steam");
        assert_eq!(items[0].status, "2159 MB");
        assert!(!items[0].attention);

        assert_eq!(items[1].display_name(), "Discord");
        assert_eq!(items[1].status, "2 new");
        assert!(items[1].attention);

        assert_eq!(items[2].display_name(), "RustDesk");
        assert_eq!(items[2].status, "Idle");
        assert!(!items[2].attention);
    }

    #[test]
    fn parses_full_sni_tsv_with_menu_items() {
        let input = "steam\tSteam\tRunning\t0\t:1.50\t/StatusNotifierItem\t/MenuBar\n\
                     menu:10\tCommunity\n\
                     menu:11\tFriends\n\
                     menu:0\tseparator\n\
                     discord\tDiscord\t3 new\t1\t:1.60\t/StatusNotifierItem\t/MenuBar\n";
        let items = parse_tray_data(input);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "steam");
        assert_eq!(items[0].title, "Steam");
        assert_eq!(items[0].menu_items.len(), 3);
        assert_eq!(items[0].menu_items[0].label, "Community");
        assert!(items[0].menu_items[2].is_separator);

        assert_eq!(items[1].id, "discord");
        assert!(items[1].attention);
    }

    #[test]
    fn parses_json_tray_data() {
        let input = r#"[
            {"id":"steam","title":"Steam","status":"Running","attention":false},
            {"id":"slack","title":"Slack","status":"1 new","attention":true}
        ]"#;
        let items = parse_tray_data(input);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Steam");
        assert!(!items[0].attention);
        assert_eq!(items[1].title, "Slack");
        assert!(items[1].attention);
    }

    #[test]
    fn clean_menu_label_strips_single_underscore() {
        assert_eq!(clean_menu_label("_Mute"), "Mute");
        assert_eq!(clean_menu_label("Save _As..."), "Save As...");
        assert_eq!(clean_menu_label("Under__score"), "Under_score");
    }

    #[test]
    fn tray_click_chooses_activate_or_menu_open() {
        let mut world = World::now();
        apply(
            &mut world.tray,
            "tray",
            "Steam\t2159 MB\nDiscord\t2 new\tattention\n",
        );
        let size = (345.0, 143.0);

        // Left-click Discord (item 1: row 0, col 1)
        let slot1 = item_slot(size, 1).expect("slot 1 exists");
        let click_pos = (slot1.0 + 10.0, slot1.1 + 10.0);
        assert!(click(&mut world, size, click_pos, 1));
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("tray:activate:discord")
        );
        assert_eq!(world.tray.open_menu, None);

        // Right-click Discord opens menu
        assert!(click(&mut world, size, click_pos, 3));
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("tray:menu-open:discord")
        );
        assert_eq!(world.tray.open_menu, Some(1));

        // Right-click again toggles menu closed
        assert!(click(&mut world, size, click_pos, 3));
        assert_eq!(world.tray.open_menu, None);
    }

    #[test]
    fn tray_context_menu_click_chooses_action_and_closes() {
        let mut world = World::now();
        apply(
            &mut world.tray,
            "tray",
            "Steam\t2159 MB\nDiscord\t2 new\tattention\nmenu:10\tSettings\nmenu:20\tQuit\n",
        );
        world.tray.open_menu = Some(1);

        let m_rect = (400.0, 100.0, 176.0, 68.0);
        // Click second item (Quit: id 20)
        let click_pos = (420.0, 100.0 + 8.0 + 26.0 + 5.0);
        assert!(click_menu(&mut world, m_rect, click_pos));
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("tray:menu-action:discord:20")
        );
        assert_eq!(world.tray.open_menu, None);
    }

    #[test]
    fn tray_hides_when_empty() {
        let world = World::now();
        assert_eq!(height(&world), 0.0);
    }
}
