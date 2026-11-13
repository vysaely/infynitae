//! Session action buttons (settings/session): lock, sleep, log out, restart, shut down.
//! Replaces `live/edge-rs/src/modules/session.rs`.

use std::any::Any;
use std::f64::consts::PI;

use cairo::{Context, LineCap, LineJoin};

use crate::card::{Action, Button, Card, Cx, Event, Hits, Rect, Status};
use crate::draw::{self, Text};
use crate::provider::{CardKind, Size};
use crate::ui;

const SMALL_H: f64 = 52.0;
const ICON_SZ: f64 = 18.0;
const SLOT_GAP: f64 = 4.0;
/// Second press within this window runs confirm-gated actions.
const CONFIRM_MS: f64 = 3000.0;

pub const KIND: CardKind = CardKind {
    id: "session",
    title: "Session",
    about: "Lock, sleep, log out, restart, shut down",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &[],
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAction {
    Lock,
    Sleep,
    Logout,
    Restart,
    Shutdown,
}

pub const ALL: [SessionAction; 5] = [
    SessionAction::Lock,
    SessionAction::Sleep,
    SessionAction::Logout,
    SessionAction::Restart,
    SessionAction::Shutdown,
];

impl SessionAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lock => "Lock",
            Self::Sleep => "Sleep",
            Self::Logout => "Log out",
            Self::Restart => "Restart",
            Self::Shutdown => "Shut down",
        }
    }

    pub fn needs_confirm(self) -> bool {
        matches!(self, Self::Logout | Self::Restart | Self::Shutdown)
    }

    pub fn command(self) -> &'static str {
        match self {
            Self::Lock => {
                "infynitae msg lock >/dev/null 2>&1 || setsid -f ~/.local/bin/infynitae-lock >/dev/null 2>&1 || setsid -f ~/infynitae/live/lock/infynitae-lock >/dev/null 2>&1"
            }
            Self::Sleep => "systemctl suspend",
            Self::Logout => {
                "pkill -f 'infynitae restore [w]atch' 2>/dev/null || true; (~/.local/bin/infynitae restore save 2>/dev/null || infynitae restore save 2>/dev/null || true); ~/.local/bin/infynitae-session stop"
            }
            Self::Restart => {
                "[ -f ~/.config/infynitae/config ] && . ~/.config/infynitae/config 2>/dev/null || true; : > ~/.local/state/infynitae/stop; pkill -f 'infynitae restore [w]atch' 2>/dev/null || true; (~/.local/bin/infynitae restore save 2>/dev/null || infynitae restore save 2>/dev/null || true); case \"${SHUTDOWN_SCREEN:-true}\" in 0|false|no|off|FALSE|False) ;; *) rm -f ~/.local/state/infynitae/lock.log; setsid -f ~/.local/bin/infynitae-lock --status 'Rebooting…' >/dev/null 2>&1; i=0; while [ $i -lt 15 ]; do   grep -q 'ready on' ~/.local/state/infynitae/lock.log 2>/dev/null && break;   sleep 0.05; i=$((i+1)); done ;; esac; systemctl reboot"
            }
            Self::Shutdown => {
                "[ -f ~/.config/infynitae/config ] && . ~/.config/infynitae/config 2>/dev/null || true; : > ~/.local/state/infynitae/stop; pkill -f 'infynitae restore [w]atch' 2>/dev/null || true; (~/.local/bin/infynitae restore save 2>/dev/null || infynitae restore save 2>/dev/null || true); case \"${SHUTDOWN_SCREEN:-true}\" in 0|false|no|off|FALSE|False) ;; *) rm -f ~/.local/state/infynitae/lock.log; setsid -f ~/.local/bin/infynitae-lock --status 'Shutting down…' >/dev/null 2>&1; i=0; while [ $i -lt 15 ]; do   grep -q 'ready on' ~/.local/state/infynitae/lock.log 2>/dev/null && break;   sleep 0.05; i=$((i+1)); done ;; esac; systemctl poweroff"
            }
        }
    }
}

pub struct SessionCard {
    armed: Option<(SessionAction, f64)>,
}

pub fn new() -> SessionCard {
    SessionCard { armed: None }
}

impl Default for SessionCard {
    fn default() -> Self {
        new()
    }
}

impl SessionCard {
    fn is_armed(&self, action: SessionAction) -> bool {
        self.armed.is_some_and(|(armed, _)| armed == action)
    }
}

impl Card for SessionCard {
    fn apply(&mut self, _key: &str, _data: &dyn Any) -> bool {
        false
    }

    fn status(&self, _cx: &Cx) -> Status {
        Status::Ready
    }

    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        match cx.size {
            Size::M => ALL.len() as f64 * ui::ROW_H,
            Size::S | Size::L => SMALL_H,
        }
    }

    fn draw(&self, cr: &Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        let th = cx.theme;
        match cx.size {
            Size::M => {
                for (idx, &act) in ALL.iter().enumerate() {
                    let row_r: Rect = (body.0, body.1 + idx as f64 * ui::ROW_H, body.2, ui::ROW_H);
                    let id = idx as u32;
                    let lit = hits.add(id, row_r) || hits.selected(id);
                    let armed = self.is_armed(act);
                    draw_pill(cr, cx, row_r, armed, lit);

                    let fg = if armed { th.accent_text } else { th.text };
                    draw::set(cr, fg);
                    draw_icon(cr, act, row_r.0 + 8.0, row_r.1 + (row_r.3 - ICON_SZ) / 2.0);

                    let label = if armed { "Press again" } else { act.label() };
                    let text_x = row_r.0 + 8.0 + ICON_SZ + 8.0;
                    let max_w = (row_r.0 + row_r.2 - 8.0 - text_x).max(0.0);
                    let t = Text {
                        size: 13.0,
                        bold: armed,
                        color: fg,
                        text: label,
                    };
                    let (_, lh) = draw::measure(cr, &t);
                    draw::text_max(cr, text_x, row_r.1 + (row_r.3 - lh) / 2.0, max_w, &t);
                }
            }
            Size::S | Size::L => {
                let count = ALL.len() as f64;
                let total_gap = SLOT_GAP * (count - 1.0);
                let slot_w = (body.2 - total_gap).max(0.0) / count;
                for (idx, &act) in ALL.iter().enumerate() {
                    let x0 = body.0 + idx as f64 * (slot_w + SLOT_GAP);
                    let x1 = if idx + 1 == ALL.len() {
                        body.0 + body.2
                    } else {
                        x0 + slot_w
                    };
                    let slot_r: Rect = (x0, body.1, (x1 - x0).max(0.0), SMALL_H);
                    let id = idx as u32;
                    let lit = hits.add(id, slot_r) || hits.selected(id);
                    let armed = self.is_armed(act);
                    draw_pill(cr, cx, slot_r, armed, lit);

                    draw::set(cr, if armed { th.accent_text } else { th.text });
                    draw_icon(
                        cr,
                        act,
                        slot_r.0 + (slot_r.2 - ICON_SZ) / 2.0,
                        slot_r.1 + 6.0,
                    );

                    let label = if armed { "Press again" } else { act.label() };
                    let max_w = (slot_r.2 - 4.0).max(0.0);
                    let t = Text {
                        size: 11.0,
                        bold: false,
                        color: if armed { th.accent_text } else { th.muted },
                        text: label,
                    };
                    let (lw, _) = draw::measure(cr, &t);
                    if lw <= max_w {
                        draw::text_center(cr, slot_r.0 + slot_r.2 / 2.0, slot_r.1 + 29.0, &t);
                    } else {
                        draw::text_max(cr, slot_r.0 + 2.0, slot_r.1 + 29.0, max_w, &t);
                    }
                }
            }
        }
    }

    fn event(&mut self, ev: &Event, _cx: &Cx, out: &mut Vec<Action>) -> bool {
        let Event::Press {
            hit,
            button: Button::Left,
            ..
        } = ev
        else {
            return false;
        };
        let Some(&act) = ALL.get(*hit as usize) else {
            return false;
        };
        if !act.needs_confirm() || self.is_armed(act) {
            self.armed = None;
            out.push(Action::Run(act.command().to_string()));
            out.push(Action::ClosePanel);
        } else {
            self.armed = Some((act, CONFIRM_MS));
        }
        true
    }

    fn tick(&mut self, dt_ms: f64) -> bool {
        let Some((_, rem)) = &mut self.armed else {
            return false;
        };
        *rem -= dt_ms.max(0.0);
        if *rem <= 0.0 {
            self.armed = None;
        }
        true
    }
}

fn draw_pill(cr: &Context, cx: &Cx, r: Rect, armed: bool, lit: bool) {
    if armed {
        draw::set(cr, cx.theme.accent.alpha(if lit { 0.24 } else { 0.16 }));
        draw::round_rect(cr, r.0, r.1, r.2, r.3, cx.theme.small_radius());
        let _ = cr.fill();
    } else if lit {
        ui::light(cr, cx, r);
    }
}

/// Draw one of the five 18x18 stroked line icons at `(x, y)`.
fn draw_icon(cr: &Context, act: SessionAction, x: f64, y: f64) {
    // Prior text calls leave a current point; start clean so stroke doesn't join from it.
    cr.new_path();
    cr.set_line_width(1.6);
    cr.set_line_cap(LineCap::Round);
    cr.set_line_join(LineJoin::Round);
    let cx = x + ICON_SZ / 2.0;
    let cy = y + ICON_SZ / 2.0;
    match act {
        SessionAction::Lock => {
            draw::round_rect(cr, cx - 5.5, cy - 0.5, 11.0, 7.5, 2.0);
            let _ = cr.stroke();
            cr.new_sub_path();
            cr.arc(cx, cy - 2.5, 3.5, PI, 2.0 * PI);
            cr.line_to(cx + 3.5, cy - 0.5);
            cr.move_to(cx - 3.5, cy - 2.5);
            cr.line_to(cx - 3.5, cy - 0.5);
            let _ = cr.stroke();
        }
        SessionAction::Sleep => {
            cr.new_sub_path();
            cr.arc(cx, cy, 6.5, 0.25 * PI, 1.45 * PI);
            cr.arc_negative(cx + 3.0, cy - 2.0, 5.5, 1.35 * PI, 0.35 * PI);
            cr.close_path();
            let _ = cr.stroke();
        }
        SessionAction::Logout => {
            cr.move_to(cx - 1.5, cy - 6.5);
            cr.line_to(cx - 6.5, cy - 6.5);
            cr.line_to(cx - 6.5, cy + 6.5);
            cr.line_to(cx - 1.5, cy + 6.5);
            let _ = cr.stroke();
            cr.move_to(cx - 2.0, cy);
            cr.line_to(cx + 6.5, cy);
            cr.move_to(cx + 3.5, cy - 3.2);
            cr.line_to(cx + 6.5, cy);
            cr.line_to(cx + 3.5, cy + 3.2);
            let _ = cr.stroke();
        }
        SessionAction::Restart => {
            cr.new_sub_path();
            cr.arc(cx, cy, 6.5, -0.35 * PI, 1.3 * PI);
            let _ = cr.stroke();
            cr.move_to(cx - 2.5, cy - 6.8);
            cr.line_to(cx - 5.5, cy - 4.2);
            cr.line_to(cx - 1.8, cy - 3.2);
            let _ = cr.stroke();
        }
        SessionAction::Shutdown => {
            cr.new_sub_path();
            cr.arc(cx, cy + 0.5, 6.5, -0.3 * PI, 1.3 * PI);
            let _ = cr.stroke();
            cr.move_to(cx, cy - 7.0);
            cr.line_to(cx, cy);
            let _ = cr.stroke();
        }
    }
}

#[cfg(test)]
mod tests {
    use cairo::{Format, ImageSurface};

    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::config::Config;
    use crate::theme::Theme;

    fn surface_cr(w: i32, h: i32) -> (ImageSurface, Context) {
        let surf = ImageSurface::create(Format::ARgb32, w, h).expect("image surface");
        let cr = Context::new(&surf).expect("cairo context");
        (surf, cr)
    }

    fn press(id: u32, button: Button) -> Event<'static> {
        Event::Press {
            hit: id,
            button,
            at: (10.0, 10.0),
        }
    }

    fn default_theme() -> Theme {
        Theme::from_config(&Config::from_text(""))
    }

    #[test]
    fn kind_matches_spec() {
        assert_eq!(KIND.id, "session");
        assert_eq!(KIND.title, "Session");
        assert_eq!(KIND.sizes, &[Size::S, Size::M]);
        assert_eq!(KIND.default, Size::S);
        const {
            assert!(!KIND.half);
        }
        assert!(KIND.sources.is_empty());
    }

    #[test]
    fn apply_and_status_are_plain_and_stable() {
        let th = default_theme();
        let cx = test_cx(&th);
        let mut card = new();
        assert!(!card.apply("anything", &42u32));
        assert_eq!(card.status(&cx), Status::Ready);
    }

    #[test]
    fn immediate_actions_run_on_first_left_press_and_close_panel() {
        let th = default_theme();
        let cx = test_cx(&th);

        for (hit, act) in [(0u32, SessionAction::Lock), (1u32, SessionAction::Sleep)] {
            let mut card = new();
            let mut out = Vec::new();
            assert!(card.event(&press(hit, Button::Left), &cx, &mut out));
            assert_eq!(
                out,
                vec![Action::Run(act.command().to_string()), Action::ClosePanel]
            );
            assert!(!card.is_armed(act));
        }
    }

    #[test]
    fn confirm_actions_require_second_press_on_same_button_within_3s() {
        let th = default_theme();
        let cx = test_cx(&th);

        for (hit, act) in [
            (2u32, SessionAction::Logout),
            (3u32, SessionAction::Restart),
            (4u32, SessionAction::Shutdown),
        ] {
            let mut card = new();
            let mut out = Vec::new();

            // First press arms confirmation, emits no actions.
            assert!(card.event(&press(hit, Button::Left), &cx, &mut out));
            assert!(out.is_empty());
            assert!(card.is_armed(act));

            // Still armed before 3 seconds elapse.
            assert!(card.tick(2999.0));
            assert!(card.is_armed(act));

            // Second press on the same button runs the exact command and closes panel.
            assert!(card.event(&press(hit, Button::Left), &cx, &mut out));
            assert_eq!(
                out,
                vec![Action::Run(act.command().to_string()), Action::ClosePanel]
            );
            assert!(!card.is_armed(act));
            assert!(!card.tick(16.0));
        }
    }

    #[test]
    fn confirm_expires_after_3s_via_tick() {
        let th = default_theme();
        let cx = test_cx(&th);
        let mut card = new();
        let mut out = Vec::new();

        assert!(!card.tick(16.0));
        assert!(card.event(&press(3, Button::Left), &cx, &mut out));
        assert!(out.is_empty());
        assert!(card.is_armed(SessionAction::Restart));

        // 1.5s elapsed -> still armed.
        assert!(card.tick(1500.0));
        assert!(card.is_armed(SessionAction::Restart));

        // Remaining 1.5s elapsed -> step ending it returns true to trigger repaint, then settles.
        assert!(card.tick(1500.0));
        assert!(!card.is_armed(SessionAction::Restart));
        assert!(!card.tick(16.0));

        // Next press must re-arm rather than run.
        assert!(card.event(&press(3, Button::Left), &cx, &mut out));
        assert!(out.is_empty());
        assert!(card.is_armed(SessionAction::Restart));
    }

    #[test]
    fn switching_between_confirm_buttons_moves_arm_without_running() {
        let th = default_theme();
        let cx = test_cx(&th);
        let mut card = new();
        let mut out = Vec::new();

        // Arm Log out, then press Restart after 2s -> switches arm to Restart with fresh 3s.
        assert!(card.event(&press(2, Button::Left), &cx, &mut out));
        assert!(card.tick(2000.0));
        assert!(card.event(&press(3, Button::Left), &cx, &mut out));
        assert!(out.is_empty());
        assert!(!card.is_armed(SessionAction::Logout));
        assert!(card.is_armed(SessionAction::Restart));

        // Restart still has a fresh 3s window (survives another 2s).
        assert!(card.tick(2000.0));
        assert!(card.is_armed(SessionAction::Restart));

        // Pressing Lock clears any armed confirm action and runs Lock.
        assert!(card.event(&press(0, Button::Left), &cx, &mut out));
        assert!(!card.is_armed(SessionAction::Restart));
        assert_eq!(
            out,
            vec![
                Action::Run(SessionAction::Lock.command().to_string()),
                Action::ClosePanel,
            ]
        );
    }

    #[test]
    fn ignores_non_left_clicks_and_unknown_hits() {
        let th = default_theme();
        let cx = test_cx(&th);
        let mut card = new();
        let mut out = Vec::new();

        assert!(!card.event(&press(0, Button::Right), &cx, &mut out));
        assert!(!card.event(&press(0, Button::Middle), &cx, &mut out));
        assert!(!card.event(&press(5, Button::Left), &cx, &mut out));
        assert!(out.is_empty());
        assert!(card.armed.is_none());
    }

    #[test]
    fn exact_commands_match_session_module() {
        assert_eq!(SessionAction::Lock.label(), "Lock");
        assert_eq!(SessionAction::Sleep.label(), "Sleep");
        assert_eq!(SessionAction::Logout.label(), "Log out");
        assert_eq!(SessionAction::Restart.label(), "Restart");
        assert_eq!(SessionAction::Shutdown.label(), "Shut down");

        assert!(SessionAction::Lock.command().contains("infynitae-lock"));
        assert_eq!(SessionAction::Sleep.command(), "systemctl suspend");
        assert!(SessionAction::Logout
            .command()
            .contains("infynitae-session stop"));
        assert!(SessionAction::Restart
            .command()
            .ends_with("systemctl reboot"));
        assert!(SessionAction::Restart
            .command()
            .contains("--status 'Rebooting…'"));
        assert!(SessionAction::Shutdown
            .command()
            .ends_with("systemctl poweroff"));
        assert!(SessionAction::Shutdown
            .command()
            .contains("--status 'Shutting down…'"));
    }

    #[test]
    fn draw_bounds_match_height_and_register_all_hits_in_s_and_m() {
        let th = default_theme();
        let mut cx = test_cx(&th);
        let card = new();
        let (_, cr) = surface_cr(420, 260);
        let width = 360.0;

        // Size::S: one horizontal row of five buttons.
        cx.size = Size::S;
        let h_s = card.height(&cx, width);
        assert_eq!(h_s, SMALL_H);
        let body_s: Rect = (10.0, 40.0, width, h_s);
        let mut hits_s = Hits::new(None, Some(2));
        card.draw(&cr, &cx, body_s, &mut hits_s);
        assert_eq!(hits_s.list.len(), ALL.len());

        for (i, &(id, r)) in hits_s.list.iter().enumerate() {
            assert_eq!(id, i as u32);
            assert!(r.0 >= body_s.0 && r.0 + r.2 <= body_s.0 + body_s.2);
            assert!(r.1 >= body_s.1 && r.1 + r.3 <= body_s.1 + h_s);
            assert!(r.2 > 0.0 && r.3 == SMALL_H);
            if i > 0 {
                assert!(hits_s.list[i - 1].1 .0 + hits_s.list[i - 1].1 .2 <= r.0 + 1e-9);
            }
            assert_eq!(hits_s.top_at((r.0 + r.2 / 2.0, r.1 + r.3 / 2.0)), Some(id));
        }

        // Size::M: five stacked rows of ui::ROW_H.
        cx.size = Size::M;
        let h_m = card.height(&cx, width);
        assert_eq!(h_m, 5.0 * ui::ROW_H);
        let body_m: Rect = (10.0, 40.0, width, h_m);
        let mut hits_m = Hits::new(None, Some(3));
        card.draw(&cr, &cx, body_m, &mut hits_m);
        assert_eq!(hits_m.list.len(), ALL.len());

        for (i, &(id, r)) in hits_m.list.iter().enumerate() {
            assert_eq!(id, i as u32);
            assert_eq!(
                r,
                (body_m.0, body_m.1 + i as f64 * ui::ROW_H, width, ui::ROW_H)
            );
            assert_eq!(hits_m.top_at((r.0 + r.2 / 2.0, r.1 + r.3 / 2.0)), Some(id));
        }
    }

    fn render_bytes(card: &SessionCard, cx: &Cx, selected: Option<u32>) -> Vec<u8> {
        let (mut surf, cr) = surface_cr(420, 280);
        let h = card.height(cx, 360.0);
        let mut hits = Hits::new(None, selected);
        card.draw(&cr, cx, (10.0, 10.0, 360.0, h), &mut hits);
        assert_eq!(hits.list.len(), ALL.len());
        drop(cr);
        surf.flush();
        let px = surf.data().expect("surface data").to_vec();
        px
    }

    #[test]
    fn armed_and_selected_states_render_distinctly_in_s_and_m() {
        let th = default_theme();
        let mut cx = test_cx(&th);
        let idle = new();
        let mut armed = new();
        let mut out = Vec::new();
        assert!(armed.event(&press(4, Button::Left), &cx, &mut out));

        for size in [Size::S, Size::M] {
            cx.size = size;
            let px_idle = render_bytes(&idle, &cx, None);
            let px_selected = render_bytes(&idle, &cx, Some(2));
            let px_armed = render_bytes(&armed, &cx, None);
            let px_armed_selected = render_bytes(&armed, &cx, Some(4));

            assert!(px_idle.iter().any(|&b| b != 0));
            assert_ne!(px_idle, px_selected);
            assert_ne!(px_idle, px_armed);
            assert_ne!(px_armed, px_armed_selected);
        }
    }
}
