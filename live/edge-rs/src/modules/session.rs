//! The `session` module: one card with a row of five text-and-icon buttons (`Lock`, `Sleep`,
//! `Log out`, `Restart`, `Shut down`), using 18 px stroked line icons and 11 px muted labels.

use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::modules::{is_left_button, Rect, World};
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::f64::consts::PI;

pub const CONTENT_H: f64 = 38.0;

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
            SessionAction::Lock => "Lock",
            SessionAction::Sleep => "Sleep",
            SessionAction::Logout => "Log out",
            SessionAction::Restart => "Restart",
            SessionAction::Shutdown => "Shut down",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            SessionAction::Lock => {
                "infynitae msg lock >/dev/null 2>&1 || setsid -f ~/.local/bin/infynitae-lock >/dev/null 2>&1 || setsid -f ~/infynitae/live/lock/infynitae-lock >/dev/null 2>&1"
            }
            SessionAction::Sleep => "systemctl suspend",
            SessionAction::Logout => {
                "pkill -f 'infynitae restore [w]atch' 2>/dev/null || true; (~/.local/bin/infynitae restore save 2>/dev/null || infynitae restore save 2>/dev/null || true); ~/.local/bin/infynitae-session stop"
            }
            SessionAction::Restart => {
                "[ -f ~/.config/infynitae/config ] && . ~/.config/infynitae/config 2>/dev/null || true; : > ~/.local/state/infynitae/stop; pkill -f 'infynitae restore [w]atch' 2>/dev/null || true; (~/.local/bin/infynitae restore save 2>/dev/null || infynitae restore save 2>/dev/null || true); case \"${SHUTDOWN_SCREEN:-true}\" in 0|false|no|off|FALSE|False) ;; *) rm -f ~/.local/state/infynitae/lock.log; setsid -f ~/.local/bin/infynitae-lock --status 'Rebooting…' >/dev/null 2>&1; i=0; while [ $i -lt 15 ]; do   grep -q 'ready on' ~/.local/state/infynitae/lock.log 2>/dev/null && break;   sleep 0.05; i=$((i+1)); done ;; esac; systemctl reboot"
            }
            SessionAction::Shutdown => {
                "[ -f ~/.config/infynitae/config ] && . ~/.config/infynitae/config 2>/dev/null || true; : > ~/.local/state/infynitae/stop; pkill -f 'infynitae restore [w]atch' 2>/dev/null || true; (~/.local/bin/infynitae restore save 2>/dev/null || infynitae restore save 2>/dev/null || true); case \"${SHUTDOWN_SCREEN:-true}\" in 0|false|no|off|FALSE|False) ;; *) rm -f ~/.local/state/infynitae/lock.log; setsid -f ~/.local/bin/infynitae-lock --status 'Shutting down…' >/dev/null 2>&1; i=0; while [ $i -lt 15 ]; do   grep -q 'ready on' ~/.local/state/infynitae/lock.log 2>/dev/null && break;   sleep 0.05; i=$((i+1)); done ;; esac; systemctl poweroff"
            }
        }
    }
}

pub fn height() -> f64 {
    2.0 * PAD_Y + CONTENT_H
}

/// Returns which session button is hit at card-local coordinates `pos` in a card of `size`.
pub fn action_at(size: (f64, f64), pos: (f64, f64)) -> Option<SessionAction> {
    let (w, h) = size;
    let (x, y) = pos;
    if x < PAD_X || x > w - PAD_X || y < 0.0 || y > h {
        return None;
    }
    let iw = (w - 2.0 * PAD_X).max(1.0);
    let idx = (((x - PAD_X) / iw) * (ALL.len() as f64)).floor() as usize;
    ALL.get(idx.min(ALL.len() - 1)).copied()
}

/// Returns the `(x, y, w, h)` slot rectangle of button `idx` inside a card of `size`.
#[cfg(test)]
pub fn button_rect(size: (f64, f64), idx: usize) -> Rect {
    let (w, h) = size;
    let slot_w = (w - 2.0 * PAD_X) / (ALL.len() as f64);
    (PAD_X + (idx as f64) * slot_w, 0.0, slot_w, h)
}

pub fn click(world: &mut World, size: (f64, f64), pos: (f64, f64), button: u32) -> bool {
    if !is_left_button(button) {
        return false;
    }
    let Some(act) = action_at(size, pos) else {
        return false;
    };
    world.close_requested = true;
    world.run(act.command().to_string());
    true
}

pub fn draw(cr: &Context, th: &Theme, world: &World, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    draw::card(cr, th, x, y, w, h);
    let slot_w = (w - 2.0 * PAD_X) / (ALL.len() as f64);
    let show_labels = h >= CONTENT_H + 8.0;
    let top = if show_labels {
        y + ((h - CONTENT_H) / 2.0).max(2.0)
    } else {
        y + ((h - 18.0) / 2.0).max(2.0)
    };
    for (i, &act) in ALL.iter().enumerate() {
        let cx = (x + PAD_X + (i as f64 + 0.5) * slot_w).round();
        // the whole slot presses (action_at), so the whole slot lights, inset from the card's edge
        super::hot(
            cr,
            th,
            world,
            x + PAD_X + i as f64 * slot_w + 2.0,
            y + 6.0,
            slot_w - 4.0,
            h - 12.0,
            th.small_radius(),
        );
        draw_icon(cr, act, cx - 9.0, top.round(), th.text);
        if show_labels {
            draw::text_center(
                cr,
                cx,
                (top + 24.0).round(),
                &Text {
                    size: 11.0,
                    bold: false,
                    color: th.muted,
                    text: act.label(),
                },
            );
        }
    }
}

/// Draws an 18x18 stroked line icon at `(x, y)` for `act`.
pub fn draw_icon(cr: &Context, act: SessionAction, x: f64, y: f64, color: Rgba) {
    draw::set(cr, color);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    let cx = x + 9.0;
    let cy = y + 9.0;
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
    use super::*;

    #[test]
    fn session_buttons_choose_matching_commands() {
        let mut world = World::now();
        let w = 380.0;
        let h = height();

        for (i, &expected) in ALL.iter().enumerate() {
            world.close_requested = false;
            let r = button_rect((w, h), i);
            assert!(click(&mut world, (w, h), (r.0 + r.2 / 2.0, h / 2.0), 1));
            assert!(world.close_requested);
            assert_eq!(
                world.actions.last().map(String::as_str),
                Some(expected.command())
            );
        }

        assert!(SessionAction::Lock.command().contains("infynitae-lock"));
        assert_eq!(SessionAction::Sleep.command(), "systemctl suspend");
        assert!(SessionAction::Logout
            .command()
            .contains("infynitae-session stop"));
        assert!(SessionAction::Restart
            .command()
            .contains("systemctl reboot"));
        assert!(SessionAction::Shutdown
            .command()
            .contains("systemctl poweroff"));

        // Right-click or click in padding does nothing
        let count_before = world.actions.len();
        assert!(!click(&mut world, (w, h), (w / 2.0, h / 2.0), 3));
        assert!(!click(&mut world, (w, h), (2.0, h / 2.0), 1));
        assert_eq!(world.actions.len(), count_before);
    }
}
