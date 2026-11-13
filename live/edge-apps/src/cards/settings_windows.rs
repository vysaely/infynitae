//! Settings: Windows (docs/EDGE_SETTINGS.md, `settings/windows`): the behaviour keys of "Windows
//! & canvas" in config/config. KEEP_ON_TOP is left to its key (Super+Shift+K writes it).

use super::settings_rows::{After, Control, Setting};
use crate::provider::{CardKind, Size};

pub const KIND: CardKind = CardKind {
    id: "windows",
    title: "Windows",
    about: "Focus, where new windows open, Super+Enter, dragging the canvas",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config"],
};

pub const TABLE: &[Setting] = &[
    Setting {
        key: "FOCUS_FOLLOWS_MOUSE",
        name: "Focus",
        says: "",
        control: Control::Pick(&[
            ("strict", "Under the pointer"),
            ("sloppy", "Under the pointer, kept over empty space"),
            ("click", "On a click"),
        ]),
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "FOCUS_ON_OPEN",
        name: "A new window takes the focus",
        says: "",
        control: Control::Pick(&[
            ("priority", "Unless you are busy"),
            ("user", "Only ones you opened"),
            ("1", "Always"),
            ("0", "Never"),
        ]),
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "PLACEMENT_POLICY",
        name: "A new window opens",
        says: "",
        control: Control::Pick(&[
            ("cluster", "Beside the others"),
            ("center", "In the middle"),
            ("origin", "At the origin"),
        ]),
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "SUPER_ENTER_ACTION",
        name: "Super+Enter",
        says: "",
        control: Control::Pick(&[("maximize", "Maximize"), ("fit", "Fit")]),
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "TELEPORT_ON_OPEN",
        name: "The view follows a new window",
        says: "",
        control: Control::Pick(&[
            ("user", "Only ones you opened"),
            ("1", "Always"),
            ("0", "Never"),
        ]),
        after: After::Save,
        more: true,
        needs: &[],
    },
    Setting {
        key: "FOCUS_ON_ACTIVATE",
        name: "An app asks for the focus",
        says: "",
        control: Control::Pick(&[
            ("user", "From you"),
            ("always", "Always"),
            ("never", "Never"),
        ]),
        after: After::Save,
        more: true,
        needs: &[],
    },
    Setting {
        key: "FOCUS_CYCLE_NAVIGATE",
        name: "Super+Tab moves the view",
        says: "Always on a window asking for attention",
        control: Control::Switch { on: "1", off: "0" },
        after: After::Save,
        more: true,
        needs: &[],
    },
    Setting {
        key: "PAN_MODE",
        name: "Dragging the canvas",
        says: "Super+Middle drag",
        control: Control::Pick(&[
            ("free", "Pointer moves with it"),
            ("grab", "Pointer stays"),
            ("rts", "Grip"),
        ]),
        after: After::Save,
        more: true,
        needs: &[],
    },
    Setting {
        key: "FULLSCREEN_PAN",
        name: "Pan out of fullscreen",
        says: "Super+Middle drag and Super+1…9",
        control: Control::Switch {
            on: "true",
            off: "false",
        },
        after: After::Save,
        more: true,
        needs: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Action, Button, Card, Event, Hits, Source, Status};
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_rows::{
        change_command, parse_config, ConfigData, ConfigSource, TableCard,
    };
    use crate::provider::Size;
    use crate::theme::Theme;
    use std::any::Any;

    fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-settings-windows-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn key_tag(text: &str, key: &str) -> &'static str {
        let mut section_tag = "[save]";
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("# ═══") {
                section_tag = if trimmed.contains("[reload]") {
                    "[reload]"
                } else if trimmed.contains("[use]") {
                    "[use]"
                } else if trimmed.contains("[restart]") {
                    "[restart]"
                } else {
                    "[save]"
                };
            }
            if let Some(rest) = trimmed.strip_prefix(key) {
                if rest.starts_with('=') {
                    for comment in lines.iter().skip(i + 1) {
                        let c = comment.trim_start();
                        if !c.starts_with('#') {
                            break;
                        }
                        if c.contains("[reload]") {
                            return "[reload]";
                        }
                        if c.contains("[use]") {
                            return "[use]";
                        }
                        if c.contains("[restart]") {
                            return "[restart]";
                        }
                        if c.contains("[save]") {
                            return "[save]";
                        }
                    }
                    return section_tag;
                }
            }
        }
        panic!("{key} not in config/config");
    }

    /// Every key here exists in the shipped config, with a default the controls know and a matching tag.
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
                Control::Pick(o) => {
                    assert!(o.iter().any(|(x, _)| x == v), "{} = {v}", s.key)
                }
                _ => {}
            }
            let expected_tag = match s.after {
                After::Save => "[save]",
                After::Use => "[use]",
                After::Restart => "[restart]",
                After::Reload(_) => "[reload]",
            };
            assert_eq!(key_tag(&text, s.key), expected_tag, "tag for {}", s.key);
        }
    }

    #[test]
    fn reads_usual_empty_and_broken_configs_and_handles_sizes_and_search() {
        let dir = temp_dir("configs");
        let defaults = dir.join("defaults");
        let cfg = dir.join("config");
        let shipped =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap();
        std::fs::write(&defaults, &shipped).unwrap();

        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx_s = test_cx(&th);
        let mut cx_m = test_cx(&th);
        cx_m.size = Size::M;

        // Before data arrives: Loading
        let mut card = TableCard::new(TABLE);
        assert_eq!(card.status(&cx_s), Status::Loading);

        // Usual config overrides
        std::fs::write(
            &cfg,
            "FOCUS_FOLLOWS_MOUSE='sloppy'\nSUPER_ENTER_ACTION=\"fit\"\nFOCUS_CYCLE_NAVIGATE='0'\n",
        )
        .unwrap();
        let d = ConfigSource {
            config: cfg.clone(),
            defaults: defaults.clone(),
            path: String::new(),
        }
        .fetch();
        assert!(card.apply("config", d.as_ref()));
        assert_eq!(card.status(&cx_s), Status::Ready);

        // At S, 4 visible rows + More (row 0..4 plus More)
        let mut hits = Hits::new(None, None);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 440, 900).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        card.draw(&cr, &cx_s, (0.0, 0.0, 404.0, 900.0), &mut hits);
        // Hit ids: rows 0..4 => 100, 110, 120, 130, More => 9000
        assert!(hits.list.iter().any(|(id, _)| *id == 9000));
        assert!(!hits.list.iter().any(|(id, _)| *id == 140));

        // Clicking More reveals all 9 rows at S
        assert!(card.event(&press(9000), &cx_s, &mut Vec::new()));
        let mut hits_more = Hits::new(None, None);
        card.draw(&cr, &cx_s, (0.0, 0.0, 404.0, 900.0), &mut hits_more);
        assert!(hits_more.list.iter().any(|(id, _)| *id == 180));

        // At M, all 9 rows are visible without More
        let mut hits_m = Hits::new(None, None);
        card.draw(&cr, &cx_m, (0.0, 0.0, 404.0, 900.0), &mut hits_m);
        assert!(!hits_m.list.iter().any(|(id, _)| *id == 9000));
        assert!(hits_m.list.iter().any(|(id, _)| *id == 180));

        // Typing filters across More rows and hides on no match
        assert!(card.event(&Event::Text("canvas"), &cx_s, &mut Vec::new()));
        let mut hits_q = Hits::new(None, None);
        card.draw(&cr, &cx_s, (0.0, 0.0, 404.0, 900.0), &mut hits_q);
        assert!(hits_q.list.iter().any(|(id, _)| *id == 170));
        assert!(!hits_q.list.iter().any(|(id, _)| *id == 100));
        assert!(card.event(&Event::Text("zzz-none"), &cx_s, &mut Vec::new()));
        assert_eq!(card.status(&cx_s), Status::Hidden);
        assert!(card.event(&Event::Text(""), &cx_s, &mut Vec::new()));

        // Empty user config still works via defaults
        std::fs::write(&cfg, "").unwrap();
        let d_empty = ConfigSource {
            config: cfg.clone(),
            defaults: defaults.clone(),
            path: String::new(),
        }
        .fetch();
        card.apply("config", d_empty.as_ref());
        assert_eq!(card.status(&cx_s), Status::Ready);

        // Broken config lines and unknown option values do not panic or drop rows
        std::fs::write(
            &cfg,
            "not a line\n=missing_key\nFOCUS_FOLLOWS_MOUSE='unterminated\nFOCUS_ON_OPEN=bogus\n",
        )
        .unwrap();
        let d_broken = ConfigSource {
            config: cfg,
            defaults,
            path: String::new(),
        }
        .fetch();
        card.apply("config", d_broken.as_ref());
        assert_eq!(card.status(&cx_s), Status::Ready);
        let cfg_data = d_broken.downcast_ref::<ConfigData>().unwrap();
        assert_eq!(cfg_data.value("FOCUS_ON_OPEN"), "bogus");
        assert_eq!(cfg_data.value("FOCUS_FOLLOWS_MOUSE"), "unterminated");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exact_commands_for_pick_words_and_switch() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut card = TableCard::new(TABLE);
        let d = ConfigData {
            now: [("FOCUS_FOLLOWS_MOUSE".into(), "strict".into())].into(),
            default: [
                ("FOCUS_FOLLOWS_MOUSE".into(), "strict".into()),
                ("SUPER_ENTER_ACTION".into(), "maximize".into()),
                ("FOCUS_CYCLE_NAVIGATE".into(), "1".into()),
                ("FULLSCREEN_PAN".into(), "true".into()),
            ]
            .into(),
            programs: Default::default(),
        };
        card.apply("config", &d as &dyn Any);

        // 1. Pick (FOCUS_FOLLOWS_MOUSE, row 0): open list, pick option 1 ("sloppy")
        let mut out = Vec::new();
        assert!(card.event(&press(100), &cx, &mut out));
        assert!(out.is_empty());
        assert!(card.event(&press(5001), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(change_command(
                "FOCUS_FOLLOWS_MOUSE",
                Some("strict"),
                "sloppy",
                After::Save
            ))]
        );
        assert_eq!(
            out[0],
            Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log FOCUS_FOLLOWS_MOUSE 'strict' && \
                 sh \"$HOME/infynitae/live/settings.sh\" set FOCUS_FOLLOWS_MOUSE 'sloppy' >/dev/null 2>&1"
                    .into()
            )
        );

        // 2. A short list (SUPER_ENTER_ACTION, row 3): open it (130), choose "Fit" (LIST + 1); no
        // line in now -> old is "-"
        out.clear();
        assert!(card.event(&press(130), &cx, &mut out));
        assert!(out.is_empty());
        assert!(card.event(&press(5001), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log SUPER_ENTER_ACTION - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set SUPER_ENTER_ACTION 'fit' >/dev/null 2>&1"
                    .into()
            )]
        );
        // Choosing the value already set writes nothing
        out.clear();
        card.event(&press(130), &cx, &mut out);
        card.event(&press(5001), &cx, &mut out);
        assert!(out.is_empty());

        // 3. Switch (FOCUS_CYCLE_NAVIGATE, row 6: "1"/"0"; FULLSCREEN_PAN, row 8: "true"/"false")
        out.clear();
        assert!(card.event(&press(161), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log FOCUS_CYCLE_NAVIGATE - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set FOCUS_CYCLE_NAVIGATE '0' >/dev/null 2>&1"
                    .into()
            )]
        );

        out.clear();
        assert!(card.event(&press(181), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log FULLSCREEN_PAN - && \
                 sh \"$HOME/infynitae/live/settings.sh\" set FULLSCREEN_PAN 'false' >/dev/null 2>&1"
                    .into()
            )]
        );
    }

    #[test]
    fn height_matches_drawn_hits_across_sizes_and_open_list() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx_s = test_cx(&th);
        let mut cx_m = test_cx(&th);
        cx_m.size = Size::M;

        let mut card = TableCard::new(TABLE);
        let d = ConfigData {
            now: Default::default(),
            default: [
                ("FOCUS_FOLLOWS_MOUSE".into(), "strict".into()),
                ("FOCUS_ON_OPEN".into(), "priority".into()),
                ("PLACEMENT_POLICY".into(), "cluster".into()),
                ("SUPER_ENTER_ACTION".into(), "maximize".into()),
                ("TELEPORT_ON_OPEN".into(), "user".into()),
                ("FOCUS_ON_ACTIVATE".into(), "user".into()),
                ("FOCUS_CYCLE_NAVIGATE".into(), "1".into()),
                ("PAN_MODE".into(), "free".into()),
                ("FULLSCREEN_PAN".into(), "true".into()),
            ]
            .into(),
            programs: Default::default(),
        };
        card.apply("config", &d as &dyn Any);

        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 440, 1000).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();

        // S closed
        let mut hits = Hits::new(None, None);
        card.draw(&cr, &cx_s, (0.0, 0.0, 404.0, 1000.0), &mut hits);
        let bottom = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
        assert!((bottom - card.height(&cx_s, 404.0)).abs() < 0.5, "{bottom}");

        // S with Pick list open (and settled)
        card.show_list();
        let mut hits_open = Hits::new(None, None);
        card.draw(&cr, &cx_s, (0.0, 0.0, 404.0, 1000.0), &mut hits_open);
        let bottom_open = hits_open
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0, f64::max);
        assert!(
            (bottom_open - card.height(&cx_s, 404.0)).abs() < 0.5,
            "{bottom_open}"
        );

        // S with More open
        assert!(card.event(&press(9000), &cx_s, &mut Vec::new()));
        let mut hits_more = Hits::new(None, None);
        card.draw(&cr, &cx_s, (0.0, 0.0, 404.0, 1000.0), &mut hits_more);
        let bottom_more = hits_more
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0, f64::max);
        assert!(
            (bottom_more - card.height(&cx_s, 404.0)).abs() < 0.5,
            "{bottom_more}"
        );

        // M
        let mut hits_m = Hits::new(None, None);
        card.draw(&cr, &cx_m, (0.0, 0.0, 404.0, 1000.0), &mut hits_m);
        let bottom_m = hits_m
            .list
            .iter()
            .map(|(_, r)| r.1 + r.3)
            .fold(0.0, f64::max);
        assert!(
            (bottom_m - card.height(&cx_m, 404.0)).abs() < 0.5,
            "{bottom_m}"
        );
    }

    #[test]
    fn settings_provider_exposes_windows_kind() {
        let p = crate::provider::by_id("settings").unwrap();
        let k = p.kind("windows").unwrap();
        assert_eq!(k.title, "Windows");
        assert_eq!(k.sizes, &[Size::S, Size::M]);
        assert_eq!(k.default, Size::S);
        assert!(!k.half);
        assert_eq!(k.sources, &["config"]);
        assert!(p.card("windows", None).is_some());
    }
}
