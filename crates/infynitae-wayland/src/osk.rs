//! The on-screen keyboard (squeekboard) opens when a text field is pressed, not whenever one is
//! active (owner, 2026-11-07, on the Yoga: squeekboard by itself shows for every focused field,
//! one an app focuses on its own included). squeekboard runs hidden from the session's start
//! with its own showing off: it is the input method, without which no text field is active
//! here (smithay drops text-input requests while there is none). The compositor, which sees
//! the press, asks it to show (`live/launch.sh osk show`, its `sm.puri.OSK0.SetVisible`).
//!
//! A press (a finger's tap; with `OSK="always"` a click as well) that leaves a text field active
//! within [`PRESS_WINDOW`] shows it; the field going away for longer than [`HIDE_GRACE`] hides it.
//! The panel's keyboard switch (`infynitae msg osk`) shows or hides it by hand; one shown by
//! hand also goes when a field that was active goes (squeekboard hides itself then). `$XDG_RUNTIME_DIR/infynitae/osk-visible` exists while shown.

use std::time::{Duration, Instant};

use smithay::wayland::text_input::TextInputSeat;

use crate::WaylandState;
use crate::log_line;

/// How long after a press a text field becoming active counts as pressed.
const PRESS_WINDOW: Duration = Duration::from_millis(700);
/// How long a field may be gone before the keyboard hides (an app refocusing its own field
/// disables and enables it again).
const HIDE_GRACE: Duration = Duration::from_millis(350);

/// Which presses open the keyboard (`OSK`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OskMode {
    /// `auto`: a finger's tap on a field.
    #[default]
    Taps,
    /// `always`: a tap or a click.
    Clicks,
    /// `off`: only by hand (the panel's switch).
    Off,
}

impl OskMode {
    pub fn parse(v: &str) -> OskMode {
        match v.trim().to_lowercase().as_str() {
            "always" | "on" | "true" | "click" | "clicks" => OskMode::Clicks,
            "off" | "false" | "no" | "never" => OskMode::Off,
            _ => OskMode::Taps,
        }
    }
}

#[derive(Debug, Default)]
pub struct OskState {
    pub mode: OskMode,
    pressed_at: Option<Instant>,
    shown: bool,
    /// A text field was active while it was shown (one shown by hand with none yet stays).
    had_field: bool,
    gone_since: Option<Instant>,
}

impl WaylandState {
    /// A press reached a window (`touch`: a finger, else a button).
    pub(crate) fn osk_pressed(&mut self, touch: bool) {
        let counts = match self.osk.mode {
            OskMode::Taps => touch,
            OskMode::Clicks => true,
            OskMode::Off => false,
        };
        if counts {
            self.osk.pressed_at = Some(Instant::now());
        }
        if std::env::var_os("INFYNITAE_OSK_DEBUG").is_some() {
            log_line!(
                "[infynitae] On-screen keyboard: press (touch {touch}, counts {counts}, field active {})",
                self.text_field_active()
            );
        }
    }

    fn text_field_active(&self) -> bool {
        let mut active = false;
        self.seat
            .text_input()
            .with_active_text_input(|_, _| active = true);
        active
    }

    /// Once a loop turn: a pressed field shows the keyboard, a field gone hides it.
    pub(crate) fn osk_check(&mut self) {
        if self.osk.pressed_at.is_none() && !self.osk.shown {
            return;
        }
        let now = Instant::now();
        let active = !self.is_locked() && self.text_field_active();
        if let Some(t) = self.osk.pressed_at {
            if now.duration_since(t) > PRESS_WINDOW {
                self.osk.pressed_at = None;
            } else if active {
                // every press on a field asks again: the keyboard's own hide key may have hidden it
                self.osk.pressed_at = None;
                self.osk_show(true, false);
            }
        }
        if self.osk.shown {
            if active {
                self.osk.gone_since = None;
                self.osk.had_field = true;
            } else if self.osk.had_field {
                let since = *self.osk.gone_since.get_or_insert(now);
                if now.duration_since(since) > HIDE_GRACE {
                    self.osk_show(false, false);
                }
            }
        }
    }

    /// By hand (`infynitae msg osk [on|off]`): on, off, or the other way round.
    pub fn osk_by_hand(&mut self, on: Option<bool>) {
        let on = on.unwrap_or(!self.osk.shown);
        self.osk_show(on, true);
    }

    fn osk_show(&mut self, on: bool, by_hand: bool) {
        log_line!(
            "[infynitae] On-screen keyboard {}{}",
            if on { "shown" } else { "hidden" },
            if by_hand { " by hand" } else { "" }
        );
        self.osk.shown = on;
        self.osk.had_field = on && !by_hand;
        self.osk.gone_since = None;
        self.spawn_action_with("osk", if on { "show" } else { "hide" });
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            let path = std::path::PathBuf::from(dir).join("infynitae/osk-visible");
            if on {
                let _ = std::fs::write(path, "");
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_words() {
        assert_eq!(OskMode::parse("auto"), OskMode::Taps);
        assert_eq!(OskMode::parse(""), OskMode::Taps);
        assert_eq!(OskMode::parse("always"), OskMode::Clicks);
        assert_eq!(OskMode::parse("off"), OskMode::Off);
    }
}
