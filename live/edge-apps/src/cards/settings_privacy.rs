//! Settings: Privacy (docs/EDGE_SETTINGS.md, `settings/privacy`): the "Privacy & screen capture"
//! and clipboard keys of config/config a person changes, plus the `screenshare-allowed` list from
//! portal consent (`shares`). Left to the file: every `ALLOW_*` list and `PRIVACY_TITLES` (a wrong
//! word there breaks screenshots or clipboard managers; the file has the explanation beside each).

use super::settings::sq;
use super::settings_rows::{change_command, After, ConfigData, Control, Setting, TableCard};
use crate::card::{Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Size};
use crate::ui;
use cairo::Context;
use std::any::Any;
use std::collections::HashSet;
use std::path::PathBuf;

pub const KIND: CardKind = CardKind {
    id: "privacy",
    title: "Privacy",
    about: "Screen sharing, capture indicator, clipboard",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "shares"],
};

const ON_OFF: Control = Control::Switch {
    on: "true",
    off: "false",
};

pub const TABLE: &[Setting] = &[
    Setting {
        key: "SCREENSHARE_ASK",
        name: "When an app shares the screen",
        says: "",
        control: Control::Pick(&[
            ("app", "Ask once per app"),
            ("true", "Always pick what to share"),
            ("false", "Never ask"),
        ]),
        after: After::Reload("screenshare"),
        more: false,
        needs: &[],
    },
    Setting {
        key: "CAPTURE_INDICATOR",
        name: "Show that the screen is captured",
        says: "",
        control: Control::Pick(&[("edge", "In the edge"), ("dot", "A dot"), ("false", "No")]),
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "CLIPBOARD_HISTORY",
        name: "Clipboard history",
        says: "Kept on disk",
        control: ON_OFF,
        after: After::Reload("clipboard"),
        more: false,
        needs: &["cliphist", "wl-paste"],
    },
    Setting {
        key: "CLIPBOARD_PERSIST",
        name: "Keep the clipboard after an app closes",
        says: "",
        control: ON_OFF,
        after: After::Reload("clipboard"),
        more: true,
        needs: &["wl-clip-persist"],
    },
    Setting {
        key: "SCREENCOPY_CURSOR",
        name: "Cursor in captures",
        says: "",
        control: ON_OFF,
        after: After::Save,
        more: true,
        needs: &[],
    },
    Setting {
        key: "REMOTE_DESKTOP",
        name: "Remote desktop",
        says: "",
        control: ON_OFF,
        after: After::Restart,
        more: true,
        needs: &[],
    },
];

pub const KEYS: &[&str] = &["PRIVACY_APPS"];

/// App IDs allowed to share the screen without asking again (lines of `screenshare-allowed`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SharesData {
    pub apps: Vec<String>,
}

/// Parses trimmed non-empty lines from `screenshare-allowed`, dropping duplicates and control bytes.
pub fn parse_shares(text: &str) -> SharesData {
    let mut apps = Vec::new();
    for line in text.lines() {
        let app = line.trim();
        if app.is_empty()
            || app.chars().any(char::is_control)
            || apps.iter().any(|seen| seen == app)
        {
            continue;
        }
        apps.push(app.to_string());
    }
    SharesData { apps }
}

/// Reads `$XDG_STATE_HOME/infynitae/screenshare-allowed` off the UI thread.
pub struct SharesSource {
    pub path: PathBuf,
    pub use_fixture: bool,
}

impl SharesSource {
    pub fn system() -> SharesSource {
        let state = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").unwrap_or_default();
                PathBuf::from(home).join(".local/state")
            });
        SharesSource {
            path: state.join("infynitae/screenshare-allowed"),
            use_fixture: true,
        }
    }

    #[cfg(test)]
    pub fn from_path(path: PathBuf) -> SharesSource {
        SharesSource {
            path,
            use_fixture: false,
        }
    }
}

impl Source for SharesSource {
    fn fetch(&mut self) -> Data {
        if let Some(text) = self
            .use_fixture
            .then(|| crate::card::render_value("shares"))
            .flatten()
        {
            return Box::new(parse_shares(&text));
        }
        let Ok(bytes) = std::fs::read(&self.path) else {
            return Box::new(SharesData::default());
        };
        let valid_lines: String = bytes
            .split(|&b| b == b'\n')
            .filter_map(|line| std::str::from_utf8(line).ok())
            .collect::<Vec<_>>()
            .join("\n");
        Box::new(parse_shares(&valid_lines))
    }
}

/// Splits `PRIVACY_APPS` on spaces, commas or tabs (as the compositor does).
pub fn parse_privacy_apps(value: &str) -> Vec<String> {
    value
        .split([' ', ',', '\t'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn remove_privacy_app_value(current: &str, remove_index: usize) -> Option<String> {
    let mut apps = parse_privacy_apps(current);
    if remove_index >= apps.len() {
        return None;
    }
    apps.remove(remove_index);
    Some(apps.join(", "))
}

/// Deletes every exact trimmed line matching `app` from `screenshare-allowed` without treating `app`
/// as shell code or a regex.
pub fn remove_share_command(app: &str) -> String {
    format!(
        "f=\"${{XDG_STATE_HOME:-$HOME/.local/state}}/infynitae/screenshare-allowed\"; [ -f \"$f\" ] || exit 0; t=$(mktemp \"$f.tmp.XXXXXX\") && RM_APP={} awk '{{ s = $0; gsub(/^[ \\t\\r]+|[ \\t\\r]+$/, \"\", s); if (s != ENVIRON[\"RM_APP\"]) print }}' \"$f\" > \"$t\" && mv -f \"$t\" \"$f\" || {{ rm -f \"$t\"; exit 1; }}",
        sq(app)
    )
}

const SHARES: u32 = 1_000;
const HIDDEN: u32 = 2_000;
const TOP: u32 = 10_000;
const BOTTOM: u32 = 20_000;
const LIST_LABEL_H: f64 = 24.0;

pub struct PrivacyCard {
    top: TableCard,
    bottom: TableCard,
    config: Option<ConfigData>,
    pending_privacy_apps: Option<String>,
    shares: SharesData,
    removed_shares: HashSet<String>,
    query: String,
}

impl Default for PrivacyCard {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivacyCard {
    pub fn new() -> PrivacyCard {
        PrivacyCard {
            top: TableCard::new(&TABLE[..1]),
            bottom: TableCard::new(&TABLE[1..]),
            config: None,
            pending_privacy_apps: None,
            shares: SharesData::default(),
            removed_shares: HashSet::new(),
            query: String::new(),
        }
    }

    fn privacy_apps_value(&self) -> String {
        if let Some(pending) = &self.pending_privacy_apps {
            return pending.clone();
        }
        self.config
            .as_ref()
            .map_or_else(String::new, |d| d.value("PRIVACY_APPS").to_string())
    }

    fn matches_section_or_item(&self, section_hay: &str, item: &str) -> bool {
        let q = self.query.trim();
        if q.is_empty() {
            return true;
        }
        let hay = format!("{section_hay} {item}").to_ascii_lowercase();
        q.split_whitespace()
            .all(|w| hay.contains(&w.to_ascii_lowercase()))
    }

    fn visible_shares(&self) -> Vec<(usize, &str)> {
        self.shares
            .apps
            .iter()
            .enumerate()
            .filter(|(_, app)| !self.removed_shares.contains(app.as_str()))
            .filter(|(_, app)| {
                self.matches_section_or_item(
                    "Apps always allowed to share screenshare-allowed shares",
                    app,
                )
            })
            .map(|(i, app)| (i, app.as_str()))
            .collect()
    }

    fn visible_privacy_apps(&self) -> Vec<(usize, String)> {
        parse_privacy_apps(&self.privacy_apps_value())
            .into_iter()
            .enumerate()
            .filter(|(_, app)| {
                self.matches_section_or_item("Hidden from captures PRIVACY_APPS", app)
            })
            .collect()
    }
}

fn list_section_height(count: usize) -> f64 {
    if count == 0 {
        0.0
    } else {
        LIST_LABEL_H + count as f64 * ui::LIST_ROW_H + 8.0
    }
}

fn draw_sub(card: &TableCard, offset: u32, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
    let selected = hits
        .selected
        .and_then(|id| id.checked_sub(offset))
        .filter(|&id| id < TOP);
    let mut sub_hits = Hits::new(hits.pointer, selected);
    card.draw(cr, cx, body, &mut sub_hits);
    hits.list
        .extend(sub_hits.list.into_iter().map(|(id, r)| (id + offset, r)));
}

fn draw_remove_list<S: AsRef<str>>(
    cr: &Context,
    cx: &Cx,
    hits: &mut Hits,
    hit_base: u32,
    title: &str,
    items: &[(usize, S)],
    pos: (f64, f64, f64),
) -> f64 {
    let (x, y, w) = pos;
    let th = cx.theme;
    draw::text_max(
        cr,
        x + 8.0,
        y + 6.0,
        (w - 16.0).max(20.0),
        &Text {
            size: 12.0,
            bold: false,
            color: th.muted,
            text: title,
        },
    );
    let box_h = items.len() as f64 * ui::LIST_ROW_H + 8.0;
    let lr = (x, y + LIST_LABEL_H, w, box_h);
    ui::list_box(cr, cx, lr);
    let remove_w = draw::measure(
        cr,
        &Text {
            size: 12.0,
            bold: false,
            color: th.text,
            text: "Remove",
        },
    )
    .0 + 18.0;
    for (j, (idx, name)) in items.iter().enumerate() {
        let rr = (
            x + 4.0,
            lr.1 + 4.0 + j as f64 * ui::LIST_ROW_H,
            w - 8.0,
            ui::LIST_ROW_H,
        );
        let bx = rr.0 + rr.2 - 4.0 - remove_w;
        let by = rr.1 + (rr.3 - 26.0) / 2.0;
        let text_x = rr.0 + 6.0;
        draw::text_max(
            cr,
            text_x,
            rr.1 + (rr.3 - 17.0) / 2.0,
            (bx - 8.0 - text_x).max(20.0),
            &Text {
                size: 13.0,
                bold: false,
                color: th.text,
                text: name.as_ref(),
            },
        );
        ui::button(cr, cx, hits, hit_base + *idx as u32, bx, by, "Remove");
    }
    LIST_LABEL_H + box_h
}

impl Card for PrivacyCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        if key == "config" {
            let a = self.top.apply(key, data);
            let b = self.bottom.apply(key, data);
            let mut changed = a || b;
            if let Some(d) = data.downcast_ref::<ConfigData>() {
                if self.pending_privacy_apps.as_ref().is_some_and(|pending| {
                    parse_privacy_apps(d.value("PRIVACY_APPS")) == parse_privacy_apps(pending)
                }) {
                    self.pending_privacy_apps = None;
                }
                if self.config.as_ref() != Some(d) {
                    changed = true;
                }
                self.config = Some(d.clone());
            }
            return changed;
        }
        if key == "shares" {
            let Some(d) = data.downcast_ref::<SharesData>() else {
                return false;
            };
            let changed = self.shares != *d;
            self.shares = d.clone();
            self.removed_shares.retain(|app| d.apps.contains(app));
            return changed;
        }
        false
    }

    fn status(&self, cx: &Cx) -> Status {
        if self.config.is_none() {
            return Status::Loading;
        }
        if self.top.status(cx) == Status::Ready
            || self.bottom.status(cx) == Status::Ready
            || !self.visible_shares().is_empty()
            || !self.visible_privacy_apps().is_empty()
        {
            Status::Ready
        } else {
            Status::Hidden
        }
    }

    fn height(&self, cx: &Cx, width: f64) -> f64 {
        let mut h = 0.0;
        if self.top.status(cx) == Status::Ready {
            h += self.top.height(cx, width);
        }
        h += list_section_height(self.visible_shares().len());
        h += list_section_height(self.visible_privacy_apps().len());
        if self.bottom.status(cx) == Status::Ready {
            h += self.bottom.height(cx, width);
        }
        h
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let mut y = body.1;
        if self.top.status(cx) == Status::Ready {
            let h = self.top.height(cx, body.2);
            draw_sub(&self.top, TOP, cr, cx, (body.0, y, body.2, h), hits);
            y += h;
        }
        let shares = self.visible_shares();
        if !shares.is_empty() {
            y += draw_remove_list(
                cr,
                cx,
                hits,
                SHARES,
                "Apps always allowed to share",
                &shares,
                (body.0, y, body.2),
            );
        }
        let hidden = self.visible_privacy_apps();
        if !hidden.is_empty() {
            y += draw_remove_list(
                cr,
                cx,
                hits,
                HIDDEN,
                "Hidden from captures",
                &hidden,
                (body.0, y, body.2),
            );
        }
        if self.bottom.status(cx) == Status::Ready {
            let h = self.bottom.height(cx, body.2);
            draw_sub(&self.bottom, BOTTOM, cr, cx, (body.0, y, body.2, h), hits);
        }
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Text(q) => {
                let mut changed = self.query != *q;
                self.query = (*q).to_string();
                let a = self.top.event(ev, cx, out);
                let b = self.bottom.event(ev, cx, out);
                changed |= a || b;
                changed
            }
            Event::Press {
                hit,
                button: Button::Left,
                at,
            } => {
                let hit = *hit;
                if (SHARES..HIDDEN).contains(&hit) {
                    let idx = (hit - SHARES) as usize;
                    let Some(app) = self.shares.apps.get(idx).cloned() else {
                        return false;
                    };
                    if !self.removed_shares.insert(app.clone()) {
                        return false;
                    }
                    out.push(Action::Run(remove_share_command(&app)));
                    return true;
                }
                if (HIDDEN..TOP).contains(&hit) {
                    let idx = (hit - HIDDEN) as usize;
                    let current = self.privacy_apps_value();
                    let Some(new_val) = remove_privacy_app_value(&current, idx) else {
                        return false;
                    };
                    let old_line = self.pending_privacy_apps.clone().or_else(|| {
                        self.config
                            .as_ref()
                            .and_then(|d| d.now.get("PRIVACY_APPS").cloned())
                    });
                    out.push(Action::Run(change_command(
                        "PRIVACY_APPS",
                        old_line.as_deref(),
                        &new_val,
                        After::Save,
                    )));
                    self.pending_privacy_apps = Some(new_val);
                    return true;
                }
                if (TOP..BOTTOM).contains(&hit) {
                    return self.top.event(
                        &Event::Press {
                            hit: hit - TOP,
                            button: Button::Left,
                            at: *at,
                        },
                        cx,
                        out,
                    );
                }
                if hit >= BOTTOM {
                    return self.bottom.event(
                        &Event::Press {
                            hit: hit - BOTTOM,
                            button: Button::Left,
                            at: *at,
                        },
                        cx,
                        out,
                    );
                }
                false
            }
            Event::Drag { hit, at } => {
                let hit = *hit;
                if (TOP..BOTTOM).contains(&hit) {
                    self.top.event(
                        &Event::Drag {
                            hit: hit - TOP,
                            at: *at,
                        },
                        cx,
                        out,
                    )
                } else if hit >= BOTTOM {
                    self.bottom.event(
                        &Event::Drag {
                            hit: hit - BOTTOM,
                            at: *at,
                        },
                        cx,
                        out,
                    )
                } else {
                    false
                }
            }
            Event::Release { hit } => {
                let sub_hit = if *hit >= BOTTOM {
                    hit - BOTTOM
                } else if *hit >= TOP {
                    hit - TOP
                } else {
                    *hit
                };
                let a = self.top.event(&Event::Release { hit: sub_hit }, cx, out);
                let b = self.bottom.event(&Event::Release { hit: sub_hit }, cx, out);
                a || b
            }
            _ => false,
        }
    }

    fn tick(&mut self, dt_ms: f64) -> bool {
        let a = self.top.tick(dt_ms);
        let b = self.bottom.tick(dt_ms);
        a || b
    }

    fn show_list(&mut self) {
        self.top.show_list();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_rows::{parse_config, tests::data};
    use crate::theme::Theme;
    use std::process::Command;

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
        let def = parse_config(&text);
        for s in TABLE {
            let v = def
                .get(s.key)
                .unwrap_or_else(|| panic!("{} not in config/config", s.key));
            match s.control {
                Control::Switch { on, off } => assert!(v == on || v == off, "{} = {v}", s.key),
                Control::Pick(o) | Control::Words(o) => {
                    assert!(o.iter().any(|(x, _)| x == v), "{} = {v}", s.key)
                }
                _ => {}
            }
        }
        let privacy_apps = def
            .get("PRIVACY_APPS")
            .expect("PRIVACY_APPS not in config/config");
        assert!(parse_privacy_apps(privacy_apps).is_empty());
    }

    #[test]
    fn shares_source_parses_usual_empty_and_broken_inputs() {
        assert_eq!(
            parse_shares("com.obsproject.Studio\norg.mozilla.firefox\n").apps,
            vec!["com.obsproject.Studio", "org.mozilla.firefox"]
        );
        assert!(parse_shares(" \n\t\r\n").apps.is_empty());
        assert_eq!(
            parse_shares("  org.mozilla.firefox  \r\norg.mozilla.firefox\nbad\x07line\nkeep.App\n")
                .apps,
            vec!["org.mozilla.firefox", "keep.App"]
        );

        let dir = std::env::temp_dir().join(format!(
            "infynitae-privacy-shares-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Missing file -> empty
        let missing = dir.join("missing");
        let got: Box<SharesData> = SharesSource::from_path(missing).fetch().downcast().unwrap();
        assert!(got.apps.is_empty());

        // Directory instead of file (broken) -> empty
        let got: Box<SharesData> = SharesSource::from_path(dir.clone())
            .fetch()
            .downcast()
            .unwrap();
        assert!(got.apps.is_empty());

        // Empty file -> empty
        let file = dir.join("screenshare-allowed");
        std::fs::write(&file, b"").unwrap();
        let got: Box<SharesData> = SharesSource::from_path(file.clone())
            .fetch()
            .downcast()
            .unwrap();
        assert!(got.apps.is_empty());

        // Usual file
        std::fs::write(&file, b"org.mozilla.firefox\ncom.obsproject.Studio\n").unwrap();
        let got: Box<SharesData> = SharesSource::from_path(file.clone())
            .fetch()
            .downcast()
            .unwrap();
        assert_eq!(
            got.apps,
            vec!["org.mozilla.firefox", "com.obsproject.Studio"]
        );

        // Broken UTF-8 mixed with valid line preserves valid lines; pure invalid UTF-8 -> empty
        std::fs::write(&file, b"keep.App\n\xff\xfe\xfd\n").unwrap();
        let got: Box<SharesData> = SharesSource::from_path(file.clone())
            .fetch()
            .downcast()
            .unwrap();
        assert_eq!(got.apps, vec!["keep.App"]);

        std::fs::write(&file, b"\xff\xfe\x00\x01").unwrap();
        let got: Box<SharesData> = SharesSource::from_path(file).fetch().downcast().unwrap();
        assert!(got.apps.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rows_hide_when_empty_or_missing_tools() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx_s = test_cx(&th);
        let mut cx_m = test_cx(&th);
        cx_m.size = Size::M;

        let mut c = PrivacyCard::new();
        assert_eq!(c.status(&cx_s), Status::Loading);

        // No clipboard tools, both lists empty: at S, only SCREENSHARE_ASK, CAPTURE_INDICATOR, More
        c.apply(
            "config",
            &data(
                &[],
                &[
                    ("SCREENSHARE_ASK", "app"),
                    ("CAPTURE_INDICATOR", "edge"),
                    ("PRIVACY_APPS", ""),
                ],
                &[],
            ) as &dyn Any,
        );
        assert_eq!(c.status(&cx_s), Status::Ready);
        assert_eq!(c.height(&cx_s, 360.0), 3.0 * ui::ROW_H);
        // At M without wl-clip-persist: SCREENSHARE_ASK, CAPTURE_INDICATOR, SCREENCOPY_CURSOR, REMOTE_DESKTOP
        assert_eq!(c.height(&cx_m, 360.0), 4.0 * ui::ROW_H);

        // Having only cliphist (without wl-paste) still hides Clipboard history
        c.apply("config", &data(&[], &[], &["cliphist"]) as &dyn Any);
        assert_eq!(c.height(&cx_s, 360.0), 3.0 * ui::ROW_H);

        // Having both cliphist and wl-paste shows Clipboard history at S
        c.apply(
            "config",
            &data(&[], &[], &["cliphist", "wl-paste"]) as &dyn Any,
        );
        assert_eq!(c.height(&cx_s, 360.0), 4.0 * ui::ROW_H);

        // Adding wl-clip-persist shows Keep the clipboard after an app closes at M
        c.apply(
            "config",
            &data(&[], &[], &["cliphist", "wl-paste", "wl-clip-persist"]) as &dyn Any,
        );
        assert_eq!(c.height(&cx_m, 360.0), 6.0 * ui::ROW_H);

        // Non-empty shares and PRIVACY_APPS grow the card; filtering to an unmatched query hides it
        c.apply(
            "shares",
            &parse_shares("com.obsproject.Studio\n") as &dyn Any,
        );
        c.apply(
            "config",
            &data(&[("PRIVACY_APPS", "firefox, obsidian")], &[], &[]) as &dyn Any,
        );
        assert_eq!(
            c.height(&cx_s, 360.0),
            3.0 * ui::ROW_H
                + (LIST_LABEL_H + ui::LIST_ROW_H + 8.0)
                + (LIST_LABEL_H + 2.0 * ui::LIST_ROW_H + 8.0)
        );

        let mut out = Vec::new();
        assert!(c.event(&Event::Text("obsidian"), &cx_s, &mut out));
        assert_eq!(c.status(&cx_s), Status::Ready);
        assert_eq!(c.height(&cx_s, 360.0), LIST_LABEL_H + ui::LIST_ROW_H + 8.0);

        assert!(c.event(&Event::Text("no-such-setting"), &cx_s, &mut out));
        assert_eq!(c.status(&cx_s), Status::Hidden);
    }

    #[test]
    fn each_control_kind_emits_expected_command_with_quoted_values() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = PrivacyCard::new();
        c.apply(
            "config",
            &data(
                &[
                    ("SCREENSHARE_ASK", "app"),
                    ("CLIPBOARD_HISTORY", "true"),
                    ("PRIVACY_APPS", "firefox, obsidian"),
                ],
                &[("CAPTURE_INDICATOR", "edge")],
                &["cliphist", "wl-paste"],
            ) as &dyn Any,
        );
        c.apply(
            "shares",
            &parse_shares("a'b.App; rm -rf /\nkeep.App\n") as &dyn Any,
        );

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 500).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 360.0, 500.0), &mut hits);
        let max_bottom = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        assert!(
            (max_bottom - c.height(&cx, 360.0)).abs() < 0.5,
            "{max_bottom}"
        );

        // 1. Pick (`SCREENSHARE_ASK` in top at ROW=100 -> TOP + 100 = 10100; item 2 LIST=5002 -> TOP + 5002)
        let mut out = Vec::new();
        assert!(c.event(&press(TOP + 100), &cx, &mut out));
        assert!(out.is_empty());
        assert!(c.event(&press(TOP + 5002), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(change_command(
                "SCREENSHARE_ASK",
                Some("app"),
                "false",
                After::Reload("screenshare"),
            ))]
        );

        // 2. Switch (`CLIPBOARD_HISTORY` is index 1 of bottom -> BOTTOM + ROW + 10*1 + 1 = 20111)
        out.clear();
        assert!(c.event(&press(BOTTOM + 111), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(change_command(
                "CLIPBOARD_HISTORY",
                Some("true"),
                "false",
                After::Reload("clipboard"),
            ))]
        );

        // 3. `PRIVACY_APPS` Remove (first entry "firefox" -> leaves "obsidian", then last -> "")
        out.clear();
        assert!(c.event(&press(HIDDEN), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(change_command(
                "PRIVACY_APPS",
                Some("firefox, obsidian"),
                "obsidian",
                After::Save,
            ))]
        );
        out.clear();
        assert!(c.event(&press(HIDDEN), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(change_command(
                "PRIVACY_APPS",
                Some("obsidian"),
                "",
                After::Save,
            ))]
        );
        assert!(c.visible_privacy_apps().is_empty());

        // 4. `shares` Remove: quotes metacharacters safely and deletes only the exact line
        out.clear();
        assert!(c.event(&press(SHARES), &cx, &mut out));
        let Action::Run(cmd) = &out[0] else {
            panic!("expected Action::Run");
        };
        assert_eq!(cmd, &remove_share_command("a'b.App; rm -rf /"));
        assert!(cmd.contains("RM_APP='a'\\''b.App; rm -rf /'"));

        // Verify the command deletes only the targeted line against a temporary state dir
        let state_dir =
            std::env::temp_dir().join(format!("infynitae-privacy-cmd-test-{}", std::process::id()));
        let app_dir = state_dir.join("infynitae");
        let _ = std::fs::remove_dir_all(&state_dir);
        std::fs::create_dir_all(&app_dir).unwrap();
        let allowed = app_dir.join("screenshare-allowed");
        std::fs::write(&allowed, "a'b.App; rm -rf /\nkeep.App\n").unwrap();
        let st = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .env("XDG_STATE_HOME", &state_dir)
            .status()
            .unwrap();
        assert!(st.success());
        assert_eq!(std::fs::read_to_string(&allowed).unwrap(), "keep.App\n");
        let _ = std::fs::remove_dir_all(&state_dir);
    }
}
