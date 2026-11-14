//! A tray app's own menu as a card (`apps/menu=<tray id>`): the entries the app gives over
//! `com.canonical.dbusmenu` (Steam's Library, Store, Friends...), shown with that app's own card
//! when its tray icon is right-clicked (`cards/apps.rs`, the Background card). A left click on the
//! icon stays the app's own action.

use crate::card::{run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Size};
use crate::ui;
use cairo::Context;
use std::any::Any;
use std::collections::HashMap;

pub const KIND: CardKind = CardKind {
    id: "menu",
    title: "Menu",
    about: "A tray app's own menu (opened from its icon)",
    sizes: &[Size::M],
    default: Size::M,
    half: false,
    sources: &["traymenu"],
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
}

/// One tray app's menu and where to send a click.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Menu {
    pub bus: String,
    pub path: String,
    pub entries: Vec<MenuEntry>,
}

/// Every tray app's menu, by its tray id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenusData {
    pub menus: HashMap<String, Menu>,
}

/// A tray id as it may stand in a card word (letters, digits, `_ . -`); "" when nothing is left.
pub fn word_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        .collect()
}

/// Strips the mnemonic underscores of dbusmenu labels (`_Library` -> `Library`, `__` -> `_`).
pub fn clean_label(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut it = src.chars().peekable();
    while let Some(c) = it.next() {
        if c == '_' {
            if it.peek() == Some(&'_') {
                it.next();
                out.push('_');
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn safe_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':' | b'/'))
}

/// The command that clicks entry `id` of a menu: only for a bus name and path made of safe
/// characters (they come from the app over D-Bus).
pub fn click_command(menu: &Menu, id: i32) -> Option<String> {
    (safe_token(&menu.bus) && safe_token(&menu.path)).then(|| {
        format!(
            "dbus-send --session --type=method_call --dest='{}' '{}' com.canonical.dbusmenu.Event int32:{id} string:clicked variant:int32:0 uint32:0",
            menu.bus, menu.path
        )
    })
}

/// The render fixture's `traymenu`: one line per entry, `tray id<TAB>bus<TAB>path<TAB>entry id<TAB>label`
/// (`-` as the label: a separator).
pub fn parse_fixture(text: &str) -> MenusData {
    let mut d = MenusData::default();
    for l in text.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 5 {
            continue;
        }
        let Ok(id) = f[3].trim().parse() else {
            continue;
        };
        let m = d.menus.entry(word_id(f[0])).or_insert_with(|| Menu {
            bus: f[1].into(),
            path: f[2].into(),
            entries: Vec::new(),
        });
        m.entries.push(MenuEntry {
            id,
            label: if f[4] == "-" {
                String::new()
            } else {
                clean_label(f[4])
            },
            enabled: true,
            separator: f[4] == "-",
        });
    }
    d
}

type Layout = (
    u32,
    (
        i32,
        HashMap<String, zbus::zvariant::OwnedValue>,
        Vec<zbus::zvariant::OwnedValue>,
    ),
);
type Item = (
    i32,
    HashMap<String, zbus::zvariant::OwnedValue>,
    Vec<zbus::zvariant::OwnedValue>,
);

fn fetch_menu(conn: &zbus::blocking::Connection, bus: &str, path: &str) -> Vec<MenuEntry> {
    let Ok(proxy) = zbus::blocking::Proxy::new(conn, bus, path, "com.canonical.dbusmenu") else {
        return Vec::new();
    };
    let _ = proxy.call::<&str, (i32,), bool>("AboutToShow", &(0,));
    let props = ["label", "visible", "enabled", "type"];
    let Ok((_, (_, _, children))) =
        proxy.call::<_, _, Layout>("GetLayout", &(0i32, 1i32, &props[..]))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for child in children {
        let Ok((id, p, _)) = Item::try_from(child) else {
            continue;
        };
        let text = |k: &str| {
            p.get(k)
                .and_then(|v| <&str>::try_from(v).ok())
                .unwrap_or("")
                .to_string()
        };
        let flag = |k: &str| {
            p.get(k)
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(true)
        };
        let separator = text("type") == "separator";
        let label = clean_label(&text("label"));
        if flag("visible") && (separator || !label.is_empty()) {
            out.push(MenuEntry {
                id,
                label,
                enabled: flag("enabled"),
                separator,
            });
        }
    }
    out
}

/// Every tray app's menu, read over D-Bus off the UI thread (the fixture's `traymenu` in a render).
#[derive(Default)]
pub struct MenusSource;

impl Source for MenusSource {
    fn fetch(&mut self) -> Data {
        if std::env::var_os("INFYNITAE_EDGE_RENDER_DATA").is_some_and(|p| !p.is_empty()) {
            return Box::new(parse_fixture(&run_sh("traymenu", "")));
        }
        let mut d = MenusData::default();
        let Ok(conn) = zbus::blocking::Connection::session() else {
            return Box::new(d);
        };
        let (items, _) = super::apps::tray_items();
        for it in items {
            if it.menu_path.is_empty() || !safe_token(&it.bus_name) || !safe_token(&it.menu_path) {
                continue;
            }
            let entries = fetch_menu(&conn, &it.bus_name, &it.menu_path);
            d.menus.insert(
                word_id(&it.id),
                Menu {
                    bus: it.bus_name.clone(),
                    path: it.menu_path.clone(),
                    entries,
                },
            );
        }
        Box::new(d)
    }
}

const ENTRY: u32 = 10;

pub struct MenuCard {
    pub id: String,
    menu: Option<Menu>,
    loaded: bool,
}

impl MenuCard {
    pub fn new(id: &str) -> MenuCard {
        MenuCard {
            id: word_id(id),
            menu: None,
            loaded: false,
        }
    }
    fn rows(&self) -> Vec<(usize, &MenuEntry)> {
        self.menu
            .as_ref()
            .map(|m| m.entries.iter().enumerate().collect())
            .unwrap_or_default()
    }
}

const SEP_H: f64 = 9.0;

impl Card for MenuCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "traymenu", data.downcast_ref::<MenusData>()) else {
            return false;
        };
        let m = d.menus.get(&self.id).cloned();
        let changed = !self.loaded || m != self.menu;
        self.menu = m;
        self.loaded = true;
        changed
    }
    /// The app's name ("Steam"), not "Menu".
    fn title(&self, cx: &Cx) -> Option<String> {
        cx.apps
            .iter()
            .find(|a| {
                a.id.eq_ignore_ascii_case(&self.id)
                    || a.wm_class
                        .as_deref()
                        .is_some_and(|w| w.eq_ignore_ascii_case(&self.id))
                    || a.name.eq_ignore_ascii_case(&self.id)
            })
            .map(|a| a.name.clone())
            .or_else(|| Some(self.id.clone()))
    }
    fn status(&self, _: &Cx) -> Status {
        if !self.loaded {
            return Status::Loading;
        }
        match &self.menu {
            Some(m) if m.entries.iter().any(|e| !e.separator) => Status::Ready,
            _ => Status::Missing("This app gives no menu".into()),
        }
    }
    fn height(&self, _: &Cx, _: f64) -> f64 {
        self.rows()
            .iter()
            .map(|(_, e)| if e.separator { SEP_H } else { ui::LIST_ROW_H })
            .sum()
    }
    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let mut y = body.1;
        for (i, e) in self.rows() {
            if e.separator {
                draw::set(cr, cx.theme.edge);
                cr.rectangle(body.0 + 8.0, y + SEP_H / 2.0, body.2 - 16.0, 1.0);
                let _ = cr.fill();
                y += SEP_H;
                continue;
            }
            let r = (body.0, y, body.2, ui::LIST_ROW_H);
            if e.enabled {
                ui::list_row(cr, cx, hits, ENTRY + i as u32, r, &e.label, "", false, None);
            } else {
                draw::text_max(
                    cr,
                    r.0 + 8.0,
                    r.1 + (r.3 - 15.0) / 2.0,
                    r.2 - 16.0,
                    &Text {
                        size: 13.0,
                        bold: false,
                        color: cx.theme.muted,
                        text: &e.label,
                    },
                );
            }
            y += ui::LIST_ROW_H;
        }
    }
    fn event(&mut self, ev: &Event, _: &Cx, out: &mut Vec<Action>) -> bool {
        let Event::Press {
            hit,
            button: Button::Left,
            ..
        } = ev
        else {
            return false;
        };
        let Some(m) = &self.menu else { return false };
        let Some(e) = hit
            .checked_sub(ENTRY)
            .and_then(|i| m.entries.get(i as usize))
            .filter(|e| e.enabled && !e.separator)
        else {
            return false;
        };
        let Some(cmd) = click_command(m, e.id) else {
            return false;
        };
        out.push(Action::Run(cmd));
        out.push(Action::ClosePanel);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::theme::Theme;

    #[test]
    fn labels_lose_their_mnemonics_and_ids_their_odd_characters() {
        assert_eq!(clean_label("_Library"), "Library");
        assert_eq!(clean_label("a__b"), "a_b");
        assert_eq!(word_id("chrome_status/icon 1;x"), "chrome_statusicon1x");
    }

    #[test]
    fn a_click_sends_the_entry_and_unsafe_names_send_nothing() {
        let m = Menu {
            bus: ":1.42".into(),
            path: "/MenuBar".into(),
            entries: vec![],
        };
        assert_eq!(
            click_command(&m, 7).unwrap(),
            "dbus-send --session --type=method_call --dest=':1.42' '/MenuBar' com.canonical.dbusmenu.Event int32:7 string:clicked variant:int32:0 uint32:0"
        );
        let bad = Menu {
            bus: ":1.42'; rm -rf ~; '".into(),
            ..m
        };
        assert!(click_command(&bad, 7).is_none());
    }

    #[test]
    fn the_card_lists_its_apps_entries_and_a_click_runs_one() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let d = parse_fixture(
            "steam\t:1.9\t/MenuBar\t1\t_Library\nsteam\t:1.9\t/MenuBar\t2\t-\nsteam\t:1.9\t/MenuBar\t3\tStore\nother\t:1.5\t/M\t1\tQuit\n",
        );
        let mut c = MenuCard::new("steam");
        assert_eq!(c.status(&cx), Status::Loading);
        c.apply("traymenu", &d as &dyn Any);
        assert_eq!(c.status(&cx), Status::Ready);
        assert_eq!(c.height(&cx, 300.0), 2.0 * ui::LIST_ROW_H + SEP_H);
        let mut out = Vec::new();
        assert!(c.event(
            &Event::Press {
                hit: ENTRY,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx,
            &mut out
        ));
        assert!(matches!(&out[0], Action::Run(s) if s.contains("int32:1 ")));
        assert_eq!(out[1], Action::ClosePanel);
        // the separator is no entry
        out.clear();
        assert!(!c.event(
            &Event::Press {
                hit: ENTRY + 1,
                button: Button::Left,
                at: (0.0, 0.0)
            },
            &cx,
            &mut out
        ));
        let mut none = MenuCard::new("nothing");
        none.apply("traymenu", &d as &dyn Any);
        assert!(matches!(none.status(&cx), Status::Missing(_)));
    }
}
