//! Read-only keyboard shortcut help overlay (`Super+/` or `Super+F1`), showing bound actions
//! grouped by area with live `BIND_*` / `EXEC_*` overrides from the config.

use crate::card::{Cx, Hits, Rect};
use crate::cards::settings_keys::{
    self, chord_cap_labels, draw_key_caps, measure_caps_width, row_matches, KeyRow, ShortcutGroup,
    CAP_GAP,
};
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::shell::OutputId;
use crate::ui::{self, TextField};
use cairo::Context;
use std::collections::HashMap;

pub const HIT_CHANGE: u32 = 1;
pub const HIT_QUERY: u32 = 2;

const OVERLAY_W: f64 = 580.0;
const SCREEN_MARGIN: f64 = 24.0;
const MIN_HEIGHT: f64 = 210.0;
const HEADER_H: f64 = 26.0;
const SEARCH_H: f64 = 34.0;
const SECTION_GAP: f64 = 12.0;
const GROUP_HDR_H: f64 = 24.0;
const GROUP_GAP: f64 = 10.0;
const ROW_H: f64 = ui::LIST_ROW_H;
const CAPS_RIGHT_PAD: f64 = 8.0;
const OR_PAD_X: f64 = 6.0;

#[derive(Clone, Debug, Default)]
pub struct ShortcutsOverlay {
    pub open: bool,
    pub query: String,
    pub scroll: f64,
    pub output: Option<OutputId>,
}

impl ShortcutsOverlay {
    pub fn open_on(&mut self, output: Option<OutputId>) {
        self.open = true;
        self.query.clear();
        self.scroll = 0.0;
        self.output = output;
    }

    pub fn close(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.query.clear();
        self.scroll = 0.0;
        self.output = None;
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GroupSection {
    pub group: ShortcutGroup,
    pub rows: Vec<KeyRow>,
}

fn is_modifier_word(w: &str) -> bool {
    matches!(
        w,
        "super" | "logo" | "win" | "ctrl" | "control" | "alt" | "shift"
    )
}

pub(crate) fn visible_sections(keys: &HashMap<String, String>, query: &str) -> Vec<GroupSection> {
    let all = settings_keys::shortcut_rows(Some(keys), None);
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut out = Vec::new();
    for group in ShortcutGroup::ALL {
        let group_title = group.title().to_lowercase();
        let rows: Vec<KeyRow> = all
            .iter()
            .filter(|r| r.group == group && !r.chords.is_empty())
            .filter(|r| {
                if words.is_empty() {
                    return true;
                }
                if words.len() == 1 {
                    return row_matches(r, &words) || group_title.contains(&words[0]);
                }
                if row_matches(r, &words) {
                    return true;
                }
                !is_modifier_word(&words[0])
                    && words
                        .iter()
                        .all(|w| row_matches(r, std::slice::from_ref(w)) || group_title.contains(w))
            })
            .cloned()
            .collect();
        if !rows.is_empty() {
            out.push(GroupSection { group, rows });
        }
    }
    out
}

pub(crate) fn content_height(sections: &[GroupSection]) -> f64 {
    if sections.is_empty() {
        return 48.0;
    }
    let body: f64 = sections
        .iter()
        .map(|s| GROUP_HDR_H + (s.rows.len() as f64) * ROW_H)
        .sum();
    body + GROUP_GAP * (sections.len().saturating_sub(1) as f64)
}

pub fn rect(w: f64, h: f64, keys: &HashMap<String, String>, query: &str) -> Rect {
    let rw = OVERLAY_W
        .min((w - 2.0 * SCREEN_MARGIN).max(300.0))
        .min(w.max(0.0));
    let sections = visible_sections(keys, query);
    let ch = content_height(&sections);
    let chrome = PAD_Y + HEADER_H + SECTION_GAP + SEARCH_H + SECTION_GAP + PAD_Y;
    let max_h = (h - 2.0 * SCREEN_MARGIN)
        .max(MIN_HEIGHT.min(h.max(0.0)))
        .min(h.max(0.0));
    let min_h = MIN_HEIGHT.min(max_h);
    let rh = (chrome + ch).clamp(min_h, max_h);
    let rx = ((w - rw) / 2.0).round();
    let ry = ((h - rh) / 2.0).round();
    (rx, ry, rw, rh)
}

pub fn viewport(r: Rect) -> Rect {
    let x = r.0 + PAD_X;
    let y = r.1 + PAD_Y + HEADER_H + SECTION_GAP + SEARCH_H + SECTION_GAP;
    let vw = (r.2 - 2.0 * PAD_X).max(20.0);
    let vh = (r.1 + r.3 - PAD_Y - y).max(20.0);
    (x, y, vw, vh)
}

pub fn max_scroll(w: f64, h: f64, keys: &HashMap<String, String>, query: &str) -> f64 {
    let sections = visible_sections(keys, query);
    let view = viewport(rect(w, h, keys, query));
    (content_height(&sections) - view.3).max(0.0)
}

pub fn inside(r: Rect, x: f64, y: f64) -> bool {
    x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3
}

pub fn draw(
    cr: &Context,
    cx: &Cx,
    state: &ShortcutsOverlay,
    keys: &HashMap<String, String>,
    w: f64,
    h: f64,
    hits: &mut Hits,
) -> Rect {
    let th = cx.theme;
    let sections = visible_sections(keys, &state.query);
    let ch = content_height(&sections);
    let r = rect(w, h, keys, &state.query);
    draw::card(cr, th, r.0, r.1, r.2, r.3);

    let x_in = r.0 + PAD_X;
    let inner_w = (r.2 - 2.0 * PAD_X).max(20.0);
    let y_hdr = r.1 + PAD_Y;

    let btn_word = "Change shortcuts";
    let btn_w = (draw::measure(
        cr,
        &Text {
            size: draw::SIZE_SMALL,
            bold: false,
            color: th.text,
            text: btn_word,
        },
    )
    .0 + 18.0)
        .min(ui::BUTTON_MAX_W);
    let btn_x = x_in + inner_w - btn_w;
    ui::button(cr, cx, hits, HIT_CHANGE, btn_x, y_hdr, btn_word);

    draw::text_max(
        cr,
        x_in,
        y_hdr + 3.0,
        (btn_x - x_in - 12.0).max(40.0),
        &Text {
            size: draw::SIZE_TITLE,
            bold: true,
            color: th.text,
            text: "Keyboard shortcuts",
        },
    );

    let y_search = y_hdr + HEADER_H + SECTION_GAP;
    let q_rect = (x_in, y_search, inner_w, SEARCH_H);
    hits.add(HIT_QUERY, q_rect);
    ui::text_field(
        cr,
        th,
        q_rect,
        &TextField {
            value: &state.query,
            placeholder: "Type to filter",
            focused: true,
            show_caret: true,
            left_pad: 12.0,
            right_pad: 12.0,
        },
    );

    let view = viewport(r);
    let max_sc = (ch - view.3).max(0.0);
    let scroll = state.scroll.clamp(0.0, max_sc);

    cr.save().ok();
    cr.rectangle(view.0, view.1, view.2, view.3);
    cr.clip();

    if sections.is_empty() {
        draw::text_max(
            cr,
            view.0,
            view.1 + 12.0,
            view.2,
            &Text {
                size: draw::SIZE_BODY,
                bold: false,
                color: th.muted,
                text: "No matching shortcuts",
            },
        );
    } else {
        let mut cur_y = view.1 - scroll;
        let view_bot = view.1 + view.3;
        for (si, section) in sections.iter().enumerate() {
            if si > 0 {
                cur_y += GROUP_GAP;
            }
            if cur_y + GROUP_HDR_H >= view.1 && cur_y <= view_bot {
                draw::text_max(
                    cr,
                    view.0,
                    cur_y + 4.0,
                    view.2 - CAPS_RIGHT_PAD,
                    &Text {
                        size: draw::SIZE_SMALL,
                        bold: true,
                        color: th.muted,
                        text: section.group.title(),
                    },
                );
            }
            cur_y += GROUP_HDR_H;

            for row in &section.rows {
                if cur_y + ROW_H >= view.1 && cur_y <= view_bot {
                    let row_r = (view.0, cur_y, view.2, ROW_H);
                    let mut x_right = view.0 + view.2 - CAPS_RIGHT_PAD;
                    for (ci, chord) in row.chords.iter().rev().enumerate() {
                        if ci > 0 {
                            let or_t = Text {
                                size: draw::SIZE_NOTE,
                                bold: false,
                                color: th.muted,
                                text: "or",
                            };
                            let (or_w, _) = draw::measure(cr, &or_t);
                            x_right -= OR_PAD_X + or_w;
                            draw::text(cr, x_right, cur_y + (ROW_H - 14.0) / 2.0, &or_t);
                            x_right -= OR_PAD_X;
                        }
                        let caps = chord_cap_labels(chord);
                        let caps_w = measure_caps_width(cr, cx, &caps);
                        draw_key_caps(cr, cx, row_r, x_right, &caps, false);
                        x_right -= caps_w;
                    }
                    let label_max_w = (x_right - CAP_GAP - view.0 - 8.0).max(60.0);
                    draw::text_max(
                        cr,
                        view.0,
                        cur_y + (ROW_H - 16.0) / 2.0,
                        label_max_w,
                        &Text {
                            size: draw::SIZE_BODY,
                            bold: false,
                            color: th.text,
                            text: &row.name,
                        },
                    );
                }
                cur_y += ROW_H;
            }
        }
    }

    cr.restore().ok();
    ui::scrollbar(cr, cx, view, ch, scroll);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::config::Config;
    use crate::theme::Theme;

    fn theme() -> Theme {
        Theme::from_config(&Config::from_text(""))
    }

    #[test]
    fn groups_all_five_categories_and_includes_shortcuts_key() {
        let keys = HashMap::new();
        let sections = visible_sections(&keys, "");
        let titles: Vec<&str> = sections.iter().map(|s| s.group.title()).collect();
        assert_eq!(
            titles,
            [
                "Windows",
                "Canvas",
                "Apps",
                "Screenshots and recording",
                "System",
            ]
        );
        let sys = sections
            .iter()
            .find(|s| s.group == ShortcutGroup::System)
            .unwrap();
        let self_row = sys.rows.iter().find(|r| r.key == "BIND_SHORTCUTS").unwrap();
        assert_eq!(
            self_row.chords,
            vec!["SUPER+SLASH".to_string(), "SUPER+F1".to_string()]
        );
    }

    #[test]
    fn user_bind_and_exec_overrides_replace_defaults_and_drop_unbound() {
        let cfg = Config::from_text(
            "BIND_TERMINAL=\"SUPER+T\"\nBIND_RELOAD=\"none\"\nEXEC_BROWSER=\"SUPER+B:firefox\"\n",
        );
        let sections = visible_sections(&cfg.keys, "");
        let apps = sections
            .iter()
            .find(|s| s.group == ShortcutGroup::Apps)
            .unwrap();
        let term = apps.rows.iter().find(|r| r.key == "BIND_TERMINAL").unwrap();
        assert_eq!(term.chords, vec!["SUPER+T".to_string()]);
        assert!(!visible_sections(&cfg.keys, "super+q")
            .iter()
            .any(|s| s.rows.iter().any(|r| r.key == "BIND_TERMINAL")));
        let browser = apps.rows.iter().find(|r| r.key == "EXEC_BROWSER").unwrap();
        assert_eq!(browser.name, "firefox");
        assert_eq!(browser.chords, vec!["SUPER+B".to_string()]);

        let sys = sections
            .iter()
            .find(|s| s.group == ShortcutGroup::System)
            .unwrap();
        assert!(!sys.rows.iter().any(|r| r.key == "BIND_RELOAD"));
    }

    #[test]
    fn typing_query_filters_rows_and_omits_empty_groups() {
        let keys = HashMap::new();
        let sections = visible_sections(&keys, "screenshot");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].group, ShortcutGroup::Screenshots);

        assert!(visible_sections(&keys, "zzzznotashortcut").is_empty());
    }

    #[test]
    fn overlay_draws_centered_with_change_button_and_scrollbar_bounds() {
        let th = theme();
        let cx = test_cx(&th);
        let state = ShortcutsOverlay {
            open: true,
            ..Default::default()
        };
        let keys = HashMap::new();
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1920, 1080).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        let r = draw(&cr, &cx, &state, &keys, 1920.0, 1080.0, &mut hits);
        assert!(r.2 > 400.0 && r.3 > 400.0);
        assert!((r.0 + r.2 / 2.0 - 960.0).abs() <= 1.0);
        assert!((r.1 + r.3 / 2.0 - 540.0).abs() <= 1.0);
        assert!(hits.list.iter().any(|(id, _)| *id == HIT_CHANGE));
        assert!(hits.list.iter().any(|(id, _)| *id == HIT_QUERY));
        assert!(max_scroll(1920.0, 720.0, &keys, "") > 0.0);
    }
}
