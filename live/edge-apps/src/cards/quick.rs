//! Quick settings, a card of Settings (`settings/quick=dnd,nightlight,keyboard,osk,replay`): the
//! small switches that are not cards of their own, one row each in the order written (no argument:
//! all). The data and commands are today's edge's (live/edge-rs/src/modules/switches.rs and the
//! Replay card of child.rs); a switch this PC cannot use is not shown: the layout with one layout,
//! the screen keyboard without squeekboard.

use crate::card::{run_sh, Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::provider::{CardKind, Size};
use crate::ui::{self, Icon, Row};
use cairo::Context;
use std::any::Any;

pub const KIND: CardKind = CardKind {
    id: "quick",
    title: "Quick settings",
    about: "Do not disturb, night light, keyboard layout, screen keyboard, replay",
    sizes: &[Size::M],
    default: Size::M,
    half: false,
    sources: &["quick"],
};

/// One switch, by its word in the panel line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Dnd,
    Night,
    Keyboard,
    Osk,
    Replay,
}

impl Switch {
    pub const ALL: [Switch; 5] = [
        Switch::Dnd,
        Switch::Night,
        Switch::Keyboard,
        Switch::Osk,
        Switch::Replay,
    ];
    pub fn of_word(w: &str) -> Option<Switch> {
        match w.trim().to_ascii_lowercase().as_str() {
            "dnd" => Some(Switch::Dnd),
            "nightlight" | "night" => Some(Switch::Night),
            "keyboard" => Some(Switch::Keyboard),
            "osk" => Some(Switch::Osk),
            "replay" => Some(Switch::Replay),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuickData {
    pub dnd: bool,
    pub night: bool,
    /// the layout's full name and how many layouts there are
    pub layout: String,
    pub layouts: u32,
    /// None: no screen keyboard installed
    pub osk: Option<bool>,
    pub replay: bool,
}

/// Do not disturb: its file, or mako's mode where mako serves.
pub fn parse_dnd(out: &str) -> bool {
    out.lines().any(|l| {
        matches!(
            l.trim().to_ascii_lowercase().as_str(),
            "on" | "true" | "1" | "do-not-disturb"
        )
    })
}

pub fn parse_bool(out: &str) -> bool {
    let s = out.lines().next().unwrap_or("").trim().to_ascii_lowercase();
    matches!(s.as_str(), "on" | "true" | "1" | "yes" | "recording")
        || s.parse::<u32>().is_ok_and(|n| n > 0)
}

/// The compositor's `keyboard-layout` file: `short<TAB>full<TAB>count`.
pub fn parse_keyboard(out: &str) -> (String, u32) {
    let Some(line) = out.lines().next() else {
        return (String::new(), 0);
    };
    let mut parts = line.split('\t');
    let _short = parts.next();
    let full = parts.next().unwrap_or("").trim().to_string();
    let count = parts
        .next()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    (full, count)
}

pub fn parse_osk(out: &str) -> Option<bool> {
    match out
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "shown" | "on" | "true" | "1" => Some(true),
        "hidden" | "off" | "false" | "0" => Some(false),
        _ => None,
    }
}

const RT: &str = "${XDG_RUNTIME_DIR:-/tmp}/infynitae";

/// Each part from its own command, under today's fixture keys (dnd, nightlight, keyboard, osk,
/// replay).
pub struct QuickSource;

impl Source for QuickSource {
    fn fetch(&mut self) -> Data {
        let dnd = run_sh(
            "dnd",
            &format!("[ -e \"{RT}/do-not-disturb\" ] && echo on || makoctl mode 2>/dev/null"),
        );
        let night = run_sh(
            "nightlight",
            &format!("[ -e \"{RT}/night-light\" ] && echo on || echo off"),
        );
        let kb = run_sh(
            "keyboard",
            &format!("cat \"{RT}/keyboard-layout\" 2>/dev/null"),
        );
        let osk = run_sh(
            "osk",
            &format!(
                "command -v squeekboard >/dev/null 2>&1 && {{ [ -e \"{RT}/osk-visible\" ] && echo shown || echo hidden; }} || echo none"
            ),
        );
        let replay = run_sh(
            "replay",
            "for f in \"${XDG_RUNTIME_DIR:-/tmp}/infynitae-recorder-supervisor.pid\" \"${XDG_RUNTIME_DIR:-/tmp}/infynitae-replay.pid\"; do [ -f \"$f\" ] && kill -0 \"$(cat \"$f\" 2>/dev/null)\" 2>/dev/null && { echo on; exit 0; }; done; echo off",
        );
        let (layout, layouts) = parse_keyboard(&kb);
        Box::new(QuickData {
            dnd: parse_dnd(&dnd),
            night: parse_bool(&night),
            layout,
            layouts,
            osk: parse_osk(&osk),
            replay: parse_bool(&replay),
        })
    }
}

/// Hits: a switch's row is `ROW + i`, its switch `SWITCH + i`; the replay buttons after.
const ROW: u32 = 10;
const SWITCH: u32 = 30;
const SAVE_CLIP: u32 = 50;
const CLIPS: u32 = 51;
const BUTTONS_H: f64 = 34.0;

pub fn dnd_command(on: bool) -> String {
    if on {
        format!(
            "mkdir -p \"{RT}\" && : > \"{RT}/do-not-disturb\"; makoctl mode -a do-not-disturb >/dev/null 2>&1; true"
        )
    } else {
        format!(
            "rm -f \"{RT}/do-not-disturb\"; makoctl mode -r do-not-disturb >/dev/null 2>&1; true"
        )
    }
}

pub fn replay_command(on: bool) -> String {
    format!(
        "sh \"$HOME/infynitae/live/settings.sh\" set ENABLE_REPLAY_BUFFER {} >/dev/null 2>&1; sh \"$HOME/infynitae/live/infynitae-reload\" recorder >/dev/null 2>&1",
        if on { "true" } else { "false" }
    )
}

pub const SAVE_CLIP_COMMAND: &str =
    "sh \"$HOME/infynitae/live/launch.sh\" save-replay >/dev/null 2>&1";
pub const CLIPS_COMMAND: &str =
    "d=\"${CLIPS_DIR:-$HOME/Videos/Clips}\"; mkdir -p \"$d\"; xdg-open \"$d\" >/dev/null 2>&1";

pub struct QuickCard {
    /// the switches asked for, in order
    pub wanted: Vec<Switch>,
    pub data: Option<QuickData>,
}

impl QuickCard {
    pub fn new(argument: Option<&str>) -> QuickCard {
        let mut wanted: Vec<Switch> = Vec::new();
        if let Some(a) = argument {
            for s in a.split(',').filter_map(Switch::of_word) {
                if !wanted.contains(&s) {
                    wanted.push(s);
                }
            }
        }
        if wanted.is_empty() {
            wanted = Switch::ALL.to_vec();
        }
        QuickCard { wanted, data: None }
    }

    /// The switches this PC can use, in order.
    pub fn shown(&self) -> Vec<Switch> {
        let Some(d) = &self.data else {
            return Vec::new();
        };
        self.wanted
            .iter()
            .copied()
            .filter(|s| match s {
                Switch::Keyboard => d.layouts > 1 && !d.layout.is_empty(),
                Switch::Osk => d.osk.is_some(),
                _ => true,
            })
            .collect()
    }

    fn replay_buttons(&self) -> bool {
        self.data.as_ref().is_some_and(|d| d.replay) && self.shown().contains(&Switch::Replay)
    }
}

/// A row's look: icon, name, state, on.
fn look(s: Switch, d: &QuickData) -> (Icon, &'static str, String, bool) {
    let onoff = |b: bool| if b { "On" } else { "Off" }.to_string();
    match s {
        Switch::Dnd => (
            Icon::Bell(d.dnd),
            "Do not disturb",
            if d.dnd {
                "Only urgent ones show".into()
            } else {
                "Off".into()
            },
            d.dnd,
        ),
        Switch::Night => (Icon::Moon, "Night light", onoff(d.night), d.night),
        Switch::Keyboard => (Icon::Keyboard, "Keyboard layout", d.layout.clone(), false),
        Switch::Osk => {
            let on = d.osk == Some(true);
            (Icon::ScreenKeys, "Screen keyboard", onoff(on), on)
        }
        Switch::Replay => (
            Icon::Record,
            "Replay",
            if d.replay {
                "Keeping the last minutes".into()
            } else {
                "Off".into()
            },
            d.replay,
        ),
    }
}

impl Card for QuickCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        let (true, Some(d)) = (key == "quick", data.downcast_ref::<QuickData>()) else {
            return false;
        };
        let changed = self.data.as_ref() != Some(d);
        self.data = Some(d.clone());
        changed
    }

    fn status(&self, _: &Cx) -> Status {
        match &self.data {
            None => Status::Loading,
            Some(_) if self.shown().is_empty() => Status::Hidden,
            Some(_) => Status::Ready,
        }
    }

    fn height(&self, _: &Cx, _: f64) -> f64 {
        self.shown().len() as f64 * ui::ROW_H
            + if self.replay_buttons() {
                BUTTONS_H
            } else {
                0.0
            }
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let Some(d) = &self.data else { return };
        let mut y = body.1;
        for (i, s) in self.shown().into_iter().enumerate() {
            let (icon, name, state, on) = look(s, d);
            let r = (body.0, y, body.2, ui::ROW_H);
            let toggles = s != Switch::Keyboard;
            ui::row(
                cr,
                cx,
                hits,
                ROW + i as u32,
                r,
                &Row {
                    icon: Some(icon),
                    name,
                    state: &state,
                    on,
                    right: if toggles { 46.0 } else { 0.0 },
                },
            );
            if toggles {
                ui::switch(
                    cr,
                    cx,
                    hits,
                    SWITCH + i as u32,
                    r,
                    body.0 + body.2 - 8.0,
                    on,
                );
            }
            y += ui::ROW_H;
            if s == Switch::Replay && d.replay {
                let bx = body.0 + 8.0 + 26.0;
                let w = ui::button(cr, cx, hits, SAVE_CLIP, bx, y + 2.0, "Save a clip");
                ui::button(cr, cx, hits, CLIPS, bx + w + 8.0, y + 2.0, "Clips folder");
                y += BUTTONS_H;
            }
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
        match *hit {
            SAVE_CLIP => {
                out.push(Action::Run(SAVE_CLIP_COMMAND.into()));
                return true;
            }
            CLIPS => {
                out.push(Action::Run(CLIPS_COMMAND.into()));
                out.push(Action::ClosePanel);
                return true;
            }
            _ => {}
        }
        // a row and its switch do the same
        let i = if (SWITCH..SAVE_CLIP).contains(hit) {
            hit - SWITCH
        } else if (ROW..SWITCH).contains(hit) {
            hit - ROW
        } else {
            return false;
        };
        let shown = self.shown();
        let (Some(s), Some(d)) = (shown.get(i as usize).copied(), self.data.as_mut()) else {
            return false;
        };
        // shown at once; the fetch after the command confirms or corrects it
        let cmd = match s {
            Switch::Dnd => {
                d.dnd = !d.dnd;
                dnd_command(d.dnd)
            }
            Switch::Night => {
                d.night = !d.night;
                format!(
                    "infynitae msg night-light {} >/dev/null 2>&1",
                    if d.night { "on" } else { "off" }
                )
            }
            Switch::Keyboard => "infynitae msg keyboard-layout >/dev/null 2>&1".into(),
            Switch::Osk => {
                let on = d.osk != Some(true);
                d.osk = Some(on);
                format!(
                    "infynitae msg osk {} >/dev/null 2>&1",
                    if on { "on" } else { "off" }
                )
            }
            Switch::Replay => {
                d.replay = !d.replay;
                replay_command(d.replay)
            }
        };
        out.push(Action::Run(cmd));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::theme::Theme;

    fn data() -> QuickData {
        QuickData {
            dnd: false,
            night: true,
            layout: "English (US)".into(),
            layouts: 2,
            osk: None,
            replay: false,
        }
    }

    fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    #[test]
    fn parses_todays_outputs() {
        assert!(parse_dnd("on\n"));
        assert!(parse_dnd("default\ndo-not-disturb\n"));
        assert!(!parse_dnd("default\n"));
        assert!(!parse_dnd(""));
        assert!(parse_bool("on"));
        assert!(!parse_bool("off"));
        assert_eq!(
            parse_keyboard("us\tEnglish (US)\t2\n"),
            ("English (US)".into(), 2)
        );
        assert_eq!(parse_keyboard(""), (String::new(), 0));
        assert_eq!(parse_osk("hidden"), Some(false));
        assert_eq!(parse_osk("none"), None);
    }

    #[test]
    fn the_argument_picks_and_orders_switches() {
        let c = QuickCard::new(Some("replay,dnd,bogus,dnd"));
        assert_eq!(c.wanted, vec![Switch::Replay, Switch::Dnd]);
        assert_eq!(QuickCard::new(None).wanted, Switch::ALL.to_vec());
        assert_eq!(QuickCard::new(Some("")).wanted, Switch::ALL.to_vec());
    }

    #[test]
    fn what_this_pc_cannot_use_is_not_shown() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = QuickCard::new(None);
        assert_eq!(c.status(&cx), Status::Loading);
        c.apply("quick", &data() as &dyn Any);
        // no screen keyboard installed
        assert_eq!(
            c.shown(),
            vec![Switch::Dnd, Switch::Night, Switch::Keyboard, Switch::Replay]
        );
        let mut one = data();
        one.layouts = 1;
        c.apply("quick", &one as &dyn Any);
        assert!(!c.shown().contains(&Switch::Keyboard));
        let mut c = QuickCard::new(Some("osk"));
        c.apply("quick", &data() as &dyn Any);
        assert_eq!(c.status(&cx), Status::Hidden);
    }

    #[test]
    fn clicks_switch_at_once_and_run_todays_commands() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = QuickCard::new(Some("dnd,nightlight,keyboard,replay"));
        c.apply("quick", &data() as &dyn Any);
        let mut out = Vec::new();
        assert!(c.event(&press(SWITCH), &cx, &mut out));
        assert!(c.data.as_ref().unwrap().dnd);
        assert_eq!(out, vec![Action::Run(dnd_command(true))]);
        assert!(dnd_command(true).contains("do-not-disturb"));
        out.clear();
        // the row does what its switch does
        assert!(c.event(&press(ROW + 1), &cx, &mut out));
        assert!(!c.data.as_ref().unwrap().night);
        assert_eq!(
            out,
            vec![Action::Run(
                "infynitae msg night-light off >/dev/null 2>&1".into()
            )]
        );
        out.clear();
        assert!(c.event(&press(ROW + 2), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "infynitae msg keyboard-layout >/dev/null 2>&1".into()
            )]
        );
        out.clear();
        assert!(c.event(&press(SWITCH + 3), &cx, &mut out));
        assert_eq!(out, vec![Action::Run(replay_command(true))]);
        out.clear();
        // with replay on, its buttons
        assert!(c.replay_buttons());
        assert!(c.event(&press(SAVE_CLIP), &cx, &mut out));
        assert_eq!(out, vec![Action::Run(SAVE_CLIP_COMMAND.into())]);
        out.clear();
        assert!(c.event(&press(CLIPS), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(CLIPS_COMMAND.into()), Action::ClosePanel]
        );
        out.clear();
        // a row past the shown ones does nothing
        assert!(!c.event(&press(ROW + 9), &cx, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn height_counts_rows_and_the_replay_buttons() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut c = QuickCard::new(Some("dnd,replay"));
        c.apply("quick", &data() as &dyn Any);
        assert_eq!(c.height(&cx, 300.0), 2.0 * ui::ROW_H);
        let mut on = data();
        on.replay = true;
        c.apply("quick", &on as &dyn Any);
        assert_eq!(c.height(&cx, 300.0), 2.0 * ui::ROW_H + BUTTONS_H);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 300).unwrap();
        let cr = Context::new(&surf).unwrap();
        let mut hits = Hits::new(None, None);
        c.draw(&cr, &cx, (0.0, 0.0, 340.0, 200.0), &mut hits);
        let ids: Vec<u32> = hits.list.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            vec![ROW, SWITCH, ROW + 1, SWITCH + 1, SAVE_CLIP, CLIPS]
        );
    }
}
