//! The desktop's small switches the compositor itself does (owner, 2026-11-07): night light (the
//! screens' colours warmer, through the gamma tables the compositor owns, so no night light app
//! is needed) and the keyboard layout (the next one of `XKB_DEFAULT_LAYOUT`'s).
//!
//! Each writes what it is now to a file in `$XDG_RUNTIME_DIR/infynitae/` for the edge's panels to
//! show: `night-light` exists while it is on; `keyboard-layout` holds the layout's short name,
//! a tab, its full name, a tab and how many layouts there are.

use std::path::PathBuf;

use infynitae_core::output::OutputId;
use smithay::input::keyboard::Layout;

use crate::WaylandState;

/// What a shell asked of the compositor besides Core's commands (`infynitae msg …`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeskAsk {
    /// Night light on (`Some(true)`), off, or the other way round (`None`).
    NightLight(Option<bool>),
    /// The next keyboard layout.
    NextKeyboardLayout,
    /// The on-screen keyboard shown by hand (`Some(true)`), hidden, or the other way round.
    Osk(Option<bool>),
}

/// Night light's colour temperature unless `NIGHT_LIGHT_TEMP` says otherwise, in kelvin.
pub const DEFAULT_NIGHT_TEMP: u32 = 4000;

fn state_file(name: &str) -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("infynitae").join(name))
}

/// The red, green and blue factors of white light at `kelvin` (Tanner Helland's fit of the
/// black body colours, 1000 K to 40000 K; 6500 K is about white).
pub fn white_at(kelvin: u32) -> (f64, f64, f64) {
    let t = f64::from(kelvin.clamp(1000, 40000)) / 100.0;
    let red = if t <= 66.0 {
        255.0
    } else {
        329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
    };
    let green = if t <= 66.0 {
        99.470_802_586_1 * t.ln() - 161.119_568_166_1
    } else {
        288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2)
    };
    let blue = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7
    };
    let f = |v: f64| (v / 255.0).clamp(0.0, 1.0);
    (f(red), f(green), f(blue))
}

/// A gamma table of `size` entries per colour, each straight from black to `factor` of full.
pub fn ramps(size: usize, kelvin: u32) -> (Vec<u16>, Vec<u16>, Vec<u16>) {
    let (r, g, b) = white_at(kelvin);
    let ramp = |k: f64| -> Vec<u16> {
        (0..size)
            .map(|i| {
                let x = if size > 1 {
                    i as f64 / (size - 1) as f64
                } else {
                    1.0
                };
                (x * k * 65535.0).round().clamp(0.0, 65535.0) as u16
            })
            .collect()
    };
    (ramp(r), ramp(g), ramp(b))
}

/// The short name of the `index`th of the config's layouts (`"us,de"` → `US`, `DE`), else the
/// first letters of the keymap's name for it.
pub fn short_layout_name(configured: &str, index: usize, full: &str) -> String {
    if let Some(n) = configured
        .split(',')
        .nth(index)
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        return n.split('(').next().unwrap_or(n).to_uppercase();
    }
    full.chars()
        .filter(|c| c.is_alphabetic())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

impl WaylandState {
    /// Does what a shell asked (`infynitae msg night-light`, `keyboard-layout`).
    pub fn desk_ask(&mut self, ask: DeskAsk) {
        match ask {
            DeskAsk::NightLight(on) => {
                let on = on.unwrap_or(!self.night_light);
                self.set_night_light(on);
            }
            DeskAsk::Osk(on) => self.osk_by_hand(on),
            DeskAsk::NextKeyboardLayout => {
                let kb = self.keyboard_handle();
                kb.with_xkb_state(self, |mut ctx| ctx.cycle_next_layout());
                self.note_keyboard_layout();
            }
        }
    }

    /// Night light on or off on every screen whose colours no app holds (an app's own night
    /// light, wlsunset or gammastep, keeps its screen).
    pub fn set_night_light(&mut self, on: bool) {
        self.night_light = on;
        let outputs: Vec<OutputId> = self.core.outputs().iter().map(|o| o.id).collect();
        for id in outputs {
            self.night_light_on(id);
        }
        if let Some(path) = state_file("night-light") {
            if on {
                let _ = std::fs::write(path, format!("{}\n", self.night_light_temp));
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// Night light as it is now on one screen (a screen that has just come is given it).
    pub(crate) fn night_light_on(&mut self, id: OutputId) {
        if self.gamma.active_controls.contains_key(&id) {
            return;
        }
        let Some(handler) = self.gamma.handler.clone() else {
            return;
        };
        let mut h = handler.lock().unwrap();
        if !self.night_light {
            h.reset_gamma(id);
            return;
        }
        let Some(size) = h.gamma_size(id) else {
            return;
        };
        let (r, g, b) = ramps(size as usize, self.night_light_temp);
        let _ = h.set_gamma(id, &r, &g, &b);
    }

    /// Writes the keyboard's layout to `keyboard-layout` when it is not what was written last
    /// (after a key that may have changed it, e.g. Alt+Shift with `grp:alt_shift_toggle`).
    pub(crate) fn note_keyboard_layout(&mut self) {
        let kb = self.keyboard_handle();
        let (index, full, count) = kb.with_xkb_state(self, |ctx| {
            let xkb = ctx.xkb().lock().unwrap();
            let active = xkb.active_layout();
            let Layout(i) = active;
            (
                i as usize,
                xkb.layout_name(active).to_string(),
                xkb.layouts().count(),
            )
        });
        if self.keyboard_layout_written == Some(index) {
            return;
        }
        self.keyboard_layout_written = Some(index);
        let configured = crate::virtual_input::seat_layout_names();
        let short = short_layout_name(&configured, index, &full);
        if let Some(path) = state_file("keyboard-layout") {
            let _ = std::fs::write(path, format!("{short}\t{full}\t{count}\n"));
        }
    }

    /// At start: night light off (the screens start with their own colours).
    pub(crate) fn desk_start(&mut self) {
        if let Some(path) = state_file("night-light") {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daylight_is_about_white_and_night_is_warm() {
        let (r, g, b) = white_at(6500);
        assert!(r > 0.99 && g > 0.95 && b > 0.95, "{r} {g} {b}");
        let (r, g, b) = white_at(4000);
        assert!(r > 0.99 && g < 0.85 && b < 0.7 && g > b, "{r} {g} {b}");
    }

    #[test]
    fn a_ramp_goes_from_black_to_the_factor() {
        let (r, _, b) = ramps(256, 4000);
        assert_eq!(r.len(), 256);
        assert_eq!(r[0], 0);
        assert_eq!(r[255], 65535);
        assert!(b[255] < 50000);
        assert!(r.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn short_names_come_from_the_config() {
        assert_eq!(short_layout_name("us,de", 1, "German"), "DE");
        assert_eq!(short_layout_name("us(intl)", 0, "English"), "US");
        assert_eq!(short_layout_name("", 0, "English (US)"), "EN");
    }
}
