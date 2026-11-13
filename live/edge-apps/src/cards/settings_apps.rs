//! Settings: Apps (docs/EDGE_SETTINGS.md, `settings/defaults`): default apps and session-app
//! toggles from config/config. Left to the file: LAUNCHER, MENU_PICKER, CLIPBOARD_PICKER,
//! CONTROL_MENU, POLKIT_AGENT, NOTIFICATIONS, ENABLE_EDGE, EDGE_IMPL, EDGE_POSITION,
//! EDGE_CONTROL_SIDE.

use super::settings_rows::{change_command, After, ConfigData, Control, Setting, TableCard};
use crate::app::App;
use crate::card::{Action, Button, Card, Cx, Event, Hits, Rect, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Size};
use crate::ui::{self, Row};
use cairo::Context;
use std::any::Any;
use std::collections::HashMap;

pub const KIND: CardKind = CardKind {
    id: "defaults",
    title: "Apps",
    about: "Default terminal, files, browser, monitor and login apps",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config"],
};

/// One "Automatic or an installed app with these desktop categories" setting.
#[derive(Clone, Copy)]
pub struct FoundSetting {
    pub key: &'static str,
    pub name: &'static str,
    pub says: &'static str,
    /// Desktop categories an app must all carry to appear in this row's list.
    pub categories: &'static [&'static str],
    /// Fallbacks checked in order on `PATH` to label `"Automatic (<cmd>)"`.
    pub auto_order: &'static [&'static str],
    pub after: After,
}

pub const FOUND: &[FoundSetting] = &[
    FoundSetting {
        key: "TERMINAL",
        name: "Terminal",
        says: "",
        categories: &["TerminalEmulator"],
        auto_order: &[
            "kitty",
            "alacritty",
            "foot",
            "wezterm",
            "ghostty",
            "konsole",
            "xfce4-terminal",
            "gnome-terminal",
            "urxvt",
            "xterm",
        ],
        after: After::Use,
    },
    FoundSetting {
        key: "FILE_MANAGER",
        name: "Files",
        says: "",
        categories: &["FileManager"],
        auto_order: &["yazi", "lf", "thunar", "nemo", "dolphin"],
        after: After::Use,
    },
    FoundSetting {
        key: "BROWSER",
        name: "Browser",
        says: "Links from every app open in it",
        categories: &["WebBrowser"],
        // launch.sh queries xdg-settings at launch; not cheap to resolve from PATH alone.
        auto_order: &[],
        after: After::Use,
    },
    FoundSetting {
        key: "SYSTEM_MONITOR",
        name: "System monitor",
        says: "",
        categories: &["System", "Monitor"],
        auto_order: &[
            "missioncenter",
            "gnome-system-monitor",
            "plasma-systemmonitor",
            "btop",
            "htop",
            "top",
        ],
        after: After::Use,
    },
];

const ON_OFF: Control = Control::Switch {
    on: "true",
    off: "false",
};

pub const TABLE: &[Setting] = &[
    Setting {
        key: "SESSION_RESTORE",
        name: "Reopen apps at login",
        says: "",
        control: ON_OFF,
        after: After::Reload("restore"),
        more: false,
        needs: &[],
    },
    Setting {
        key: "START_TERMINAL",
        name: "Open a terminal when nothing was reopened",
        says: "",
        control: ON_OFF,
        after: After::Restart,
        more: true,
        needs: &[],
    },
    Setting {
        key: "AUTOMOUNT",
        name: "Mount USB drives when plugged in",
        says: "",
        control: ON_OFF,
        after: After::Reload("automount"),
        more: true,
        needs: &["udisksctl"],
    },
    Setting {
        key: "XWAYLAND",
        name: "X11 apps and games",
        says: "",
        control: ON_OFF,
        after: After::Restart,
        more: true,
        needs: &[],
    },
];

pub const KEYS: &[&str] = &[FOUND[0].key, FOUND[1].key, FOUND[2].key, FOUND[3].key];

// Hit IDs outside TableCard's range (ROW 100.., LIST 5000.., MORE 9000).
const FOUND_LIST: u32 = 20_000;

/// Settings card for `settings/defaults`: app-list pickers from `cx.apps` above a `TableCard` for
/// the boolean rows.
pub struct DefaultsCard {
    data: Option<ConfigData>,
    /// Values written by this card before the config watcher catches up.
    pending: HashMap<&'static str, String>,
    pick: Option<usize>,
    list: ui::List,
    /// Set by `Card::show_list()` because `show_list` has no `Cx` to check which row is shown.
    show_first: bool,
    query: String,
    rest: TableCard,
}

impl Default for DefaultsCard {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultsCard {
    pub fn new() -> DefaultsCard {
        DefaultsCard {
            data: None,
            pending: HashMap::new(),
            pick: None,
            list: ui::List::default(),
            show_first: false,
            query: String::new(),
            rest: TableCard::new(TABLE),
        }
    }

    fn found_value(&self, i: usize) -> String {
        let key = FOUND[i].key;
        if let Some(v) = self.pending.get(key) {
            return v.clone();
        }
        self.data
            .as_ref()
            .map_or(String::new(), |d| d.value(key).to_string())
    }

    fn found_is_default(&self, i: usize) -> bool {
        let Some(d) = &self.data else { return true };
        self.found_value(i)
            == d.default
                .get(FOUND[i].key)
                .map(String::as_str)
                .unwrap_or("")
    }

    fn auto_resolved(&self, i: usize) -> Option<&'static str> {
        let d = self.data.as_ref()?;
        FOUND[i].auto_order.iter().copied().find(|p| d.has(p))
    }

    fn matching_apps<'a>(&self, i: usize, cx: &'a Cx<'_>) -> Vec<&'a App> {
        let cats = FOUND[i].categories;
        cx.apps
            .iter()
            .filter(|a| {
                !a.exec.is_empty() && cats.iter().all(|c| a.categories.iter().any(|ac| ac == c))
            })
            .collect()
    }

    fn matches_found(&self, f: &FoundSetting) -> bool {
        let hay = format!("{} {} {}", f.name, f.says, f.key).to_lowercase();
        self.query
            .split_whitespace()
            .all(|w| hay.contains(&w.to_lowercase()))
    }

    fn shown_found(&self, cx: &Cx) -> Vec<usize> {
        if self.data.is_none() {
            return Vec::new();
        }
        let searching = !self.query.trim().is_empty();
        (0..FOUND.len())
            .filter(|&i| {
                !self.matching_apps(i, cx).is_empty()
                    && (!searching || self.matches_found(&FOUND[i]))
            })
            .collect()
    }

    fn picked_row(&self, shown: &[usize]) -> Option<usize> {
        if self.show_first {
            shown.first().copied()
        } else {
            self.pick.filter(|i| shown.contains(i))
        }
    }

    fn open_row(&self, shown: &[usize]) -> Option<usize> {
        if self.list.shown() {
            self.picked_row(shown)
        } else {
            None
        }
    }

    fn row_value_label(&self, i: usize, cx: &Cx) -> String {
        let cur = self.found_value(i);
        if cur.is_empty() {
            return "Not set".to_string();
        }
        if cur.eq_ignore_ascii_case("auto") {
            return match self.auto_resolved(i) {
                Some(cmd) => format!("Automatic ({cmd})"),
                None => "Automatic".to_string(),
            };
        }
        if let Some(app) = self
            .matching_apps(i, cx)
            .into_iter()
            .find(|a| a.exec == cur)
        {
            return app.name.clone();
        }
        cur
    }

    fn set_found(
        &mut self,
        i: usize,
        new: String,
        desktop: Option<&str>,
        out: &mut Vec<Action>,
    ) -> bool {
        let f = FOUND[i];
        if self.found_value(i) == new {
            return false;
        }
        let old_line = self.data.as_ref().and_then(|d| d.now.get(f.key).cloned());
        let mut cmd = change_command(f.key, old_line.as_deref(), &new, f.after);
        // the browser is the system's default too (as on Windows), so links from every app follow
        if let (true, Some(id)) = (f.key == "BROWSER", desktop) {
            cmd += &browser_default_command(id);
        }
        out.push(Action::Run(cmd));
        self.pending.insert(f.key, new);
        true
    }
}

/// Makes the app the system's default browser: xdg-settings when there, else the mimeapps
/// entries it would write. Only after the config line was written (the command it follows).
fn browser_default_command(desktop_id: &str) -> String {
    let d = super::settings::sq(&format!("{desktop_id}.desktop"));
    format!(
        " && {{ if command -v xdg-settings >/dev/null 2>&1; then xdg-settings set default-web-browser {d}; \
else for m in x-scheme-handler/http x-scheme-handler/https text/html; do xdg-mime default {d} \"$m\"; done; fi; }} >/dev/null 2>&1"
    )
}

impl Card for DefaultsCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let rest_changed = self.rest.apply(key, data);
        let (true, Some(d)) = (key == "config", data.downcast_ref::<ConfigData>()) else {
            return rest_changed;
        };
        self.pending.retain(|k, v| d.value(k) != v.as_str());
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed || rest_changed
    }

    fn status(&self, cx: &Cx) -> Status {
        if self.data.is_none() {
            return Status::Loading;
        }
        if self.shown_found(cx).is_empty() && matches!(self.rest.status(cx), Status::Hidden) {
            return Status::Hidden;
        }
        Status::Ready
    }

    fn height(&self, cx: &Cx, width: f64) -> f64 {
        let shown = self.shown_found(cx);
        let open = self.open_row(&shown);
        let found_h: f64 = shown
            .iter()
            .map(|&i| {
                ui::ROW_H
                    + if open == Some(i) {
                        self.list.height(1 + self.matching_apps(i, cx).len())
                    } else {
                        0.0
                    }
            })
            .sum();
        let rest_h = if matches!(self.rest.status(cx), Status::Hidden) {
            0.0
        } else {
            self.rest.height(cx, width)
        };
        found_h + rest_h
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let shown = self.shown_found(cx);
        let open = self.open_row(&shown);
        let mut y = body.1;
        let right = body.0 + body.2 - 8.0;
        for i in shown {
            let f = &FOUND[i];
            let r = (body.0, y, body.2, ui::ROW_H);
            let label = self.row_value_label(i, cx);
            let t = Text {
                size: 12.0,
                bold: false,
                color: if !self.found_is_default(i) {
                    cx.theme.accent_text
                } else {
                    cx.theme.muted
                },
                text: &label,
            };
            let max_w = (body.2 / 2.0).max(20.0);
            let tw = draw::measure(cr, &t).0.min(max_w);
            let lit = ui::row(
                cr,
                cx,
                hits,
                i as u32,
                r,
                &Row {
                    icon: None,
                    name: f.name,
                    state: f.says,
                    on: false,
                    right: tw + 40.0,
                },
            );
            ui::value_box(cr, cx, r, right, &label, lit, open == Some(i));

            if open == Some(i) {
                let apps = self.matching_apps(i, cx);
                let lh = self.list.height(1 + apps.len());
                let lr = (body.0, y + ui::ROW_H, body.2, lh);
                ui::list_box(cr, cx, lr);
                let cur = self.found_value(i);
                let auto_note = self.auto_resolved(i).unwrap_or("");
                let row0 = (body.0 + 4.0, lr.1 + 4.0, body.2 - 8.0, ui::LIST_ROW_H);
                if row0.1 + row0.3 <= lr.1 + lr.3 + 0.5 {
                    ui::list_row(
                        cr,
                        cx,
                        hits,
                        FOUND_LIST,
                        row0,
                        "Automatic",
                        auto_note,
                        cur.eq_ignore_ascii_case("auto"),
                        None,
                    );
                }
                for (idx, app) in apps.iter().enumerate() {
                    let j = idx + 1;
                    let rr = (
                        body.0 + 4.0,
                        lr.1 + 4.0 + j as f64 * ui::LIST_ROW_H,
                        body.2 - 8.0,
                        ui::LIST_ROW_H,
                    );
                    if rr.1 + rr.3 > lr.1 + lr.3 + 0.5 {
                        break;
                    }
                    ui::list_row(
                        cr,
                        cx,
                        hits,
                        FOUND_LIST + j as u32,
                        rr,
                        &app.name,
                        "",
                        cur == app.exec,
                        None,
                    );
                }
                y += ui::ROW_H + lh;
            } else {
                y += ui::ROW_H;
            }
        }
        if !matches!(self.rest.status(cx), Status::Hidden) {
            let rest_h = (body.1 + body.3 - y).max(0.0);
            self.rest.draw(cr, cx, (body.0, y, body.2, rest_h), hits);
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Text(q) => {
                let rest_changed = self.rest.event(ev, cx, out);
                let q_changed = self.query != *q;
                if q_changed {
                    self.query = (*q).to_string();
                }
                q_changed || rest_changed
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } if (*hit as usize) < FOUND.len() => {
                let i = *hit as usize;
                let shown = self.shown_found(cx);
                if !shown.contains(&i) {
                    return false;
                }
                let active = self.list.open && self.picked_row(&shown) == Some(i);
                let same_pick = !self.show_first && self.pick == Some(i);
                self.show_first = false;
                self.pick = Some(i);
                if active || same_pick {
                    self.list.toggle();
                } else {
                    self.list = ui::List::default();
                    self.list.toggle();
                }
                true
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } if *hit >= FOUND_LIST => {
                if !self.list.open {
                    return false;
                }
                let j = (*hit - FOUND_LIST) as usize;
                let shown = self.shown_found(cx);
                let Some(i) = self.picked_row(&shown) else {
                    return false;
                };
                let (new, desktop) = if j == 0 {
                    ("auto".to_string(), None)
                } else {
                    let apps = self.matching_apps(i, cx);
                    let Some(app) = apps.get(j - 1) else {
                        return false;
                    };
                    (app.exec.clone(), Some(app.id.clone()))
                };
                self.list.open = false;
                self.show_first = false;
                let _ = self.set_found(i, new, desktop.as_deref(), out);
                true
            }
            _ => self.rest.event(ev, cx, out),
        }
    }

    fn tick(&mut self, dt_ms: f64) -> bool {
        let a = self.list.tick(dt_ms);
        let b = self.rest.tick(dt_ms);
        a || b
    }

    fn show_list(&mut self) {
        self.show_first = true;
        self.list.open = true;
        self.list.settle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_rows::tests::data;
    use crate::theme::Theme;

    fn app(name: &str, exec: &str, categories: &[&str]) -> App {
        App {
            id: exec.to_string(),
            name: name.to_string(),
            icon: String::new(),
            exec: exec.to_string(),
            categories: categories.iter().map(|s| s.to_string()).collect(),
            wm_class: None,
            terminal: false,
        }
    }

    fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    /// Every key here exists in the shipped config, with a default the controls know.
    #[test]
    fn every_key_is_in_config_config_with_a_value_its_control_has() {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap();
        let def = super::super::settings_rows::parse_config(&text);
        for f in FOUND {
            let v = def
                .get(f.key)
                .unwrap_or_else(|| panic!("{} not in config/config", f.key));
            assert_eq!(v, "auto", "{} = {v}", f.key);
        }
        for s in TABLE {
            let v = def
                .get(s.key)
                .unwrap_or_else(|| panic!("{} not in config/config", s.key));
            match s.control {
                Control::Switch { on, off } => assert!(v == on || v == off, "{} = {v}", s.key),
                Control::Pick(o) => {
                    assert!(o.iter().any(|(x, _)| x == v), "{} = {v}", s.key)
                }
                _ => {}
            }
        }
    }

    #[test]
    fn rows_hide_when_category_or_udisksctl_missing_and_more_folds_at_s() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let apps = [
            app("Kitty", "kitty", &["System", "TerminalEmulator"]),
            app("Lstopo", "lstopo", &["System"]),
            app("Firefox", "firefox", &["Network", "WebBrowser"]),
        ];
        let mut cx = test_cx(&th);
        cx.apps = &apps;

        let mut c = DefaultsCard::new();
        assert_eq!(c.status(&cx), Status::Loading);

        c.apply(
            "config",
            &data(
                &[],
                &[("TERMINAL", "auto"), ("BROWSER", "auto")],
                &["kitty"],
            ) as &dyn Any,
        );
        // Terminal (0) and Browser (2) have matching apps; Files (1) has none;
        // System monitor (3) requires BOTH System + Monitor, so "System" alone stays hidden.
        assert_eq!(c.shown_found(&cx), vec![0, 2]);
        // At Size::S: 2 Found rows + SESSION_RESTORE + More row (START_TERMINAL & XWAYLAND).
        assert_eq!(c.height(&cx, 400.0), 4.0 * ui::ROW_H);

        // Clicking More (9000) reveals START_TERMINAL and XWAYLAND, still hiding AUTOMOUNT without udisksctl.
        let mut out = Vec::new();
        assert!(c.event(&press(9000), &cx, &mut out));
        assert_eq!(c.height(&cx, 400.0), 6.0 * ui::ROW_H);

        // Adding udisksctl reveals AUTOMOUNT too; Size::M shows everything without a More row.
        c.apply("config", &data(&[], &[], &["udisksctl"]) as &dyn Any);
        assert_eq!(c.height(&cx, 400.0), 7.0 * ui::ROW_H);
        cx.size = Size::M;
        assert_eq!(c.height(&cx, 400.0), 6.0 * ui::ROW_H);

        // Searching filters both Found and Switch rows, ignoring More, and hides when nothing matches.
        c.event(&Event::Text("usb"), &cx, &mut out);
        assert_eq!(c.status(&cx), Status::Ready);
        assert!(c.shown_found(&cx).is_empty());
        assert_eq!(c.height(&cx, 400.0), ui::ROW_H);

        c.event(&Event::Text("nothing matches this"), &cx, &mut out);
        assert_eq!(c.status(&cx), Status::Hidden);
    }

    #[test]
    fn found_lists_apps_resolves_auto_cheaply_and_keeps_custom_values() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let apps = [
            app("Foot", "foot", &["TerminalEmulator"]),
            app("Kitty", "kitty", &["TerminalEmulator"]),
            app("Firefox", "firefox", &["WebBrowser"]),
            app("Btop", "btop", &["System", "Monitor"]),
        ];
        let mut cx = test_cx(&th);
        cx.apps = &apps;

        let mut c = DefaultsCard::new();
        c.apply(
            "config",
            &data(
                &[("SYSTEM_MONITOR", "custom-mon -v")],
                &[
                    ("TERMINAL", "auto"),
                    ("BROWSER", "auto"),
                    ("SYSTEM_MONITOR", "auto"),
                ],
                &["alacritty", "foot", "kitty"],
            ) as &dyn Any,
        );

        // Terminal uses launch.sh priority ("kitty" beats "alacritty" and "foot").
        assert_eq!(c.row_value_label(0, &cx), "Automatic (kitty)");
        assert!(c.found_is_default(0));
        // Browser auto is not resolved from PATH alone.
        assert_eq!(c.row_value_label(2, &cx), "Automatic");
        // Custom value outside cx.apps is shown verbatim and stays marked changed.
        assert_eq!(c.row_value_label(3, &cx), "custom-mon -v");
        assert!(!c.found_is_default(3));

        // Open Terminal list: "Automatic (kitty)" + Foot + Kitty -> pick Foot (j=1).
        let mut out = Vec::new();
        assert!(c.event(&press(0), &cx, &mut out));
        assert!(c.list.open);
        assert!(c.event(&press(FOUND_LIST + 1), &cx, &mut out));
        assert!(!c.list.open);
        assert_eq!(c.found_value(0), "foot");
        assert_eq!(c.row_value_label(0, &cx), "Foot");
        assert_eq!(out.len(), 1);

        // Pick "Automatic" (j=0) when already pending "foot" restores "auto".
        out.clear();
        assert!(c.event(&press(0), &cx, &mut out));
        assert!(c.event(&press(FOUND_LIST), &cx, &mut out));
        assert_eq!(c.found_value(0), "auto");
        assert_eq!(out.len(), 1);

        // Clicking the already-active option closes the list without emitting a command.
        out.clear();
        assert!(c.event(&press(0), &cx, &mut out));
        assert!(c.event(&press(FOUND_LIST), &cx, &mut out));
        assert!(out.is_empty());
        assert!(!c.list.open);
    }

    #[test]
    fn exact_change_commands_for_found_and_switch_controls() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let apps = [app("Kitty's Term", "kitty 'a'", &["TerminalEmulator"])];
        let mut cx = test_cx(&th);
        cx.apps = &apps;

        let mut c = DefaultsCard::new();
        c.apply(
            "config",
            &data(
                &[
                    ("TERMINAL", "old'term"),
                    ("SESSION_RESTORE", "true"),
                    ("AUTOMOUNT", "true"),
                ],
                &[],
                &["udisksctl"],
            ) as &dyn Any,
        );

        let mut out = Vec::new();
        // Found change: TERMINAL (After::Use) quotes old & new with single-quote escaping.
        assert!(c.event(&press(0), &cx, &mut out));
        assert!(c.event(&press(FOUND_LIST + 1), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log TERMINAL 'old'\\''term' && sh \"$HOME/infynitae/live/settings.sh\" set TERMINAL 'kitty '\\''a'\\''' >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Switch change: SESSION_RESTORE (index 0 in TABLE -> hit ROW + 1 = 101, After::Reload("restore")).
        out.clear();
        assert!(c.event(&press(101), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log SESSION_RESTORE 'true' && sh \"$HOME/infynitae/live/settings.sh\" set SESSION_RESTORE 'false' >/dev/null 2>&1 && sh \"$HOME/infynitae/live/infynitae-reload\" restore >/dev/null 2>&1"
                    .to_string()
            )]
        );

        // Switch under More: reveal More (9000), click AUTOMOUNT (index 2 -> 121, After::Reload("automount")),
        // and START_TERMINAL (index 1 -> 111, After::Restart).
        out.clear();
        assert!(c.event(&press(9000), &cx, &mut out));
        assert!(c.event(&press(121), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log AUTOMOUNT 'true' && sh \"$HOME/infynitae/live/settings.sh\" set AUTOMOUNT 'false' >/dev/null 2>&1 && sh \"$HOME/infynitae/live/infynitae-reload\" automount >/dev/null 2>&1"
                    .to_string()
            )]
        );
        out.clear();
        assert!(c.event(&press(111), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log START_TERMINAL - && sh \"$HOME/infynitae/live/settings.sh\" set START_TERMINAL 'true' >/dev/null 2>&1"
                    .to_string()
            )]
        );
    }

    #[test]
    fn provider_registers_defaults_card() {
        use crate::provider::Provider;
        let s = super::super::settings::Settings;
        assert!(s
            .cards()
            .iter()
            .any(|k| k.id == "defaults" && k.title == "Apps"));
        assert!(s.card("defaults", None).is_some());
    }

    #[test]
    fn height_matches_what_is_drawn() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let apps = [
            app("Kitty", "kitty", &["TerminalEmulator"]),
            app("Files", "thunar", &["FileManager"]),
            app("Firefox", "firefox", &["WebBrowser"]),
            app("Btop", "btop", &["System", "Monitor"]),
        ];
        let mut cx = test_cx(&th);
        cx.apps = &apps;

        let mut c = DefaultsCard::new();
        c.apply(
            "config",
            &data(
                &[],
                &[
                    ("TERMINAL", "auto"),
                    ("FILE_MANAGER", "auto"),
                    ("BROWSER", "auto"),
                    ("SYSTEM_MONITOR", "auto"),
                ],
                &["kitty", "udisksctl"],
            ) as &dyn Any,
        );

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 420, 600).unwrap();
        let cr = Context::new(&surf).unwrap();

        let assert_bottom_matches_height = |card: &DefaultsCard, cx: &Cx, label: &str| {
            let mut hits = Hits::new(None, None);
            let h = card.height(cx, 380.0);
            card.draw(&cr, cx, (0.0, 0.0, 380.0, h), &mut hits);
            let bottom = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
            assert!(
                bottom <= h + 0.5 && (h - bottom) <= 10.0,
                "{label}: bottom={bottom}, height={h}"
            );
        };

        // Closed at S.
        assert_bottom_matches_height(&c, &cx, "closed S");
        // Half-open animation.
        c.event(&press(0), &cx, &mut Vec::new());
        c.tick(90.0);
        assert_bottom_matches_height(&c, &cx, "animating open");
        while c.tick(16.0) {}
        assert_bottom_matches_height(&c, &cx, "open S");

        // show_list (starting with Terminal hidden, Files opens instead) at M.
        cx.size = Size::M;
        let apps_no_term = &apps[1..];
        cx.apps = apps_no_term;
        let mut c2 = DefaultsCard::new();
        c2.apply("config", &data(&[], &[], &["udisksctl"]) as &dyn Any);
        c2.show_list();
        assert_bottom_matches_height(&c2, &cx, "show_list M");
    }

    #[test]
    fn a_chosen_browser_becomes_the_system_default_with_its_id_quoted() {
        let c = browser_default_command("org.mozilla.firefox");
        assert!(c.starts_with(" && "));
        assert!(c.contains("xdg-settings set default-web-browser 'org.mozilla.firefox.desktop'"));
        assert!(c.contains("xdg-mime default 'org.mozilla.firefox.desktop' \"$m\""));
        let evil = browser_default_command("a'b; rm -rf ~");
        assert!(evil.contains("'a'\\''b; rm -rf ~.desktop'"));
    }
}
