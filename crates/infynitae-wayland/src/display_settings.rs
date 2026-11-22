//! The screens' settings in the config that apply while running: `OUTPUT_SCALE`,
//! `OUTPUT_<NAME>_SCALE`, `_MODE`, `_RATE`, `_TRANSFORM`, `OUTPUT_ORDER`, `OUTPUT_LAYOUT`,
//! `VRR_ENABLED` and `VIRTUAL_SCREEN`. A reload that changes one leaves a [`DisplayChange`] for
//! the backend ([`WaylandState::take_display_change`]); the new modes, turns, scales and order of
//! the screens connected now are [`HeadChange`]s ([`WaylandState::display_head_changes`]), which
//! the backend carries out as a
//! display settings tool's configuration (`output_management`). The first reload, at start, only
//! records what the backend started with.

use std::collections::BTreeMap;

use infynitae_core::output::{OutputModel, Transform};

use crate::output_management::{HeadChange, HeadMode};
use crate::{WaylandState, log_line};

/// What the config says about the screens (each key read as the backends read it at start).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplaySettings {
    /// `OUTPUT_SCALE` in 1/120 units; `None` when unset or outside 0.25 … 8 (1.0 then).
    pub scale_120: Option<u32>,
    /// `OUTPUT_<NAME>_SCALE` in 1/120 units, by connector name lowercased, `_` written as `-`.
    pub screen_scales: BTreeMap<String, u32>,
    /// `OUTPUT_<NAME>_MODE` (width, height), keyed like `screen_scales`.
    pub screen_modes: BTreeMap<String, (u16, u16)>,
    /// `OUTPUT_<NAME>_RATE` in mHz, keyed like `screen_scales`.
    pub screen_rates: BTreeMap<String, u32>,
    /// `OUTPUT_<NAME>_TRANSFORM`, keyed like `screen_scales`.
    pub screen_transforms: BTreeMap<String, Transform>,
    /// `OUTPUT_ORDER`: connector names, left to right.
    pub order: Vec<String>,
    /// `OUTPUT_LAYOUT="right-to-left"`.
    pub right_to_left: bool,
    /// `VRR_ENABLED` (or `VRR`, `ADAPTIVE_SYNC`): on, off, or `None` for auto (while fullscreen).
    pub vrr: Option<bool>,
    /// `VIRTUAL_SCREEN` is not off.
    pub virtual_screen: bool,
}

/// The settings before and after a reload that changed them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayChange {
    pub before: DisplaySettings,
    pub now: DisplaySettings,
}

/// A scale in 1/120 units, when `val` is one Core takes (0.25 … 8).
fn scale_120(val: &str) -> Option<u32> {
    let f = val.trim().parse::<f64>().ok()?;
    (0.25..=8.0)
        .contains(&f)
        .then(|| (f * 120.0).round().max(1.0) as u32)
}

/// `OUTPUT_<NAME>_TRANSFORM`: the rotation's name, as wlr-randr names them (and the degrees alone,
/// counter-clockwise like wl_output).
pub fn parse_transform(v: &str) -> Option<Transform> {
    Some(match v.trim().to_ascii_lowercase().as_str() {
        "normal" | "0" => Transform::Normal,
        "90" => Transform::R90,
        "180" => Transform::R180,
        "270" => Transform::R270,
        "flipped" => Transform::Flipped,
        "flipped-90" => Transform::Flipped90,
        "flipped-180" => Transform::Flipped180,
        "flipped-270" => Transform::Flipped270,
        _ => return None,
    })
}

/// The mode a screen takes for `OUTPUT_<NAME>_MODE` and `_RATE`: the size given (else the
/// preferred mode's, else the first's), at the rate nearest the one given (else the highest).
/// `None` when no mode has that size.
pub fn choose_mode(
    modes: &[HeadMode],
    target_size: Option<(u16, u16)>,
    target_rate: Option<u32>,
) -> Option<usize> {
    let size = target_size
        .map(|(w, h)| (i32::from(w), i32::from(h)))
        .or_else(|| {
            modes
                .iter()
                .find(|m| m.preferred)
                .or_else(|| modes.first())
                .map(|m| m.size)
        })?;
    let matching_size = modes.iter().enumerate().filter(|(_, m)| m.size == size);
    if let Some(rate) = target_rate {
        return matching_size
            .min_by_key(|(_, m)| (i64::from(m.refresh_mhz) - i64::from(rate)).abs())
            .map(|(i, _)| i);
    }
    matching_size
        .max_by_key(|(_, m)| m.refresh_mhz)
        .map(|(i, _)| i)
}

impl DisplaySettings {
    pub fn parse(content: &str) -> DisplaySettings {
        let mut s = DisplaySettings {
            scale_120: None,
            screen_scales: BTreeMap::new(),
            screen_modes: BTreeMap::new(),
            screen_rates: BTreeMap::new(),
            screen_transforms: BTreeMap::new(),
            order: Vec::new(),
            right_to_left: false,
            vrr: None,
            virtual_screen: true,
        };
        let mut vrr_given = false;
        for (key, val) in content.lines().filter_map(crate::config_line) {
            let (key, val) = (key.trim(), val.trim());
            match key {
                "OUTPUT_SCALE" => s.scale_120 = scale_120(val),
                "OUTPUT_ORDER" => s.order = crate::parse_list(val),
                "OUTPUT_LAYOUT" => s.right_to_left = val.eq_ignore_ascii_case("right-to-left"),
                // the first line that says on or off, as at start (main.rs)
                "VRR_ENABLED" | "VRR" | "ADAPTIVE_SYNC" if !vrr_given => {
                    if let Some(on) = crate::parse_bool(val) {
                        s.vrr = Some(on);
                        vrr_given = true;
                    }
                }
                "VIRTUAL_SCREEN" => s.virtual_screen = crate::parse_bool(val) != Some(false),
                _ => {
                    let Some(rest) = key.strip_prefix("OUTPUT_") else {
                        continue;
                    };
                    let name = |suffix: &str| {
                        rest.strip_suffix(suffix)
                            .map(|n| n.replace('_', "-").to_lowercase())
                    };
                    if let Some(name) = name("_SCALE")
                        && let Some(scale) = scale_120(val)
                    {
                        s.screen_scales.insert(name, scale);
                    } else if let Some(name) = name("_MODE")
                        && let Some((w, h)) = val.split_once('x')
                        && let (Ok(w), Ok(h)) = (w.trim().parse(), h.trim().parse())
                    {
                        s.screen_modes.insert(name, (w, h));
                    } else if let Some(name) = name("_RATE")
                        && let Ok(r) = val.parse::<f64>()
                    {
                        s.screen_rates.insert(name, (r * 1000.0).round() as u32);
                    } else if let Some(name) = name("_TRANSFORM")
                        && let Some(t) = parse_transform(val)
                    {
                        s.screen_transforms.insert(name, t);
                    }
                }
            }
        }
        s
    }

    /// The scale of the screen on `connector`: its own line, else `OUTPUT_SCALE`, else 1.
    pub fn scale_for(&self, connector: &str) -> u32 {
        self.screen_scales
            .get(&connector.to_lowercase())
            .copied()
            .or(self.scale_120)
            .unwrap_or(120)
    }
}

impl WaylandState {
    /// Notes the config's screen settings after a reload; a change from the last ones is kept
    /// for the backend (with the oldest `before` when the backend has not taken one yet).
    pub(crate) fn note_display_settings(&mut self, now: DisplaySettings) {
        let Some(before) = self.display_settings.replace(now.clone()) else {
            return;
        };
        let before = match self.display_change.take() {
            Some(pending) => pending.before,
            None => before,
        };
        if before != now {
            self.display_change = Some(DisplayChange { before, now });
        }
    }

    /// The config's screen settings changed since the last call: what they were and are.
    pub fn take_display_change(&mut self) -> Option<DisplayChange> {
        self.display_change.take()
    }

    /// The modes, turns, scales and positions `change` gives the screens connected now; empty
    /// when none of them, `OUTPUT_ORDER` or `OUTPUT_LAYOUT` changed. A screen whose `_MODE` or
    /// `_RATE` line changed takes the mode they pick as at start (a removed line: the preferred
    /// mode); one whose `_TRANSFORM` line changed takes the new turn (a removed line leaves it
    /// as it is: the turn it started with may be the panel's own). The screens are placed side
    /// by side from 0 in `OUTPUT_ORDER` (those it does not name after them, as they are now),
    /// else in their order now, turned round when `OUTPUT_LAYOUT` changed.
    pub fn display_head_changes(&self, change: &DisplayChange) -> Vec<HeadChange> {
        let (before, now) = (&change.before, &change.now);
        if before.scale_120 == now.scale_120
            && before.screen_scales == now.screen_scales
            && before.screen_modes == now.screen_modes
            && before.screen_rates == now.screen_rates
            && before.screen_transforms == now.screen_transforms
            && before.order == now.order
            && before.right_to_left == now.right_to_left
        {
            return Vec::new();
        }
        let mut outputs: Vec<_> = self.core.outputs().iter().collect();
        outputs.sort_by(|a, b| a.arrangement.0.total_cmp(&b.arrangement.0));
        if !now.order.is_empty() {
            outputs.sort_by_key(|o| {
                now.order
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(&o.key.connector))
                    .unwrap_or(usize::MAX)
            });
        } else if now.right_to_left != before.right_to_left {
            outputs.reverse();
        }
        let mut x = 0.0f64;
        let mut changes = Vec::new();
        for o in outputs {
            let name = o.key.connector.to_lowercase();
            let scale = now.scale_for(&o.key.connector);
            let mode = self.config_mode(o, &name, change);
            let transform =
                now.screen_transforms.get(&name).copied().filter(|t| {
                    before.screen_transforms.get(&name) != Some(t) && *t != o.transform
                });
            let (w, h) = mode.map_or(o.phys, |m| (m.size.0 as u32, m.size.1 as u32));
            let view_w = if transform.unwrap_or(o.transform).swaps() {
                h
            } else {
                w
            };
            let position = (x.round() as i32, 0);
            x += f64::from(view_w) * 120.0 / f64::from(scale);
            let scale_changed = (o.scale * 120.0).round() as u32 != scale;
            let moved = (o.arrangement.0, o.arrangement.1)
                != (f64::from(position.0), f64::from(position.1));
            if scale_changed || moved || mode.is_some() || transform.is_some() {
                changes.push(HeadChange {
                    output: o.id,
                    mode,
                    position: moved.then_some(position),
                    scale_120: scale_changed.then_some(scale),
                    transform,
                    adaptive_sync: None,
                });
            }
        }
        changes
    }

    /// The mode screen `o` (connector `name`, lowercased) takes when its `_MODE` or `_RATE` line
    /// changed and that mode is not the one it has. A size it has no mode for gives its preferred
    /// mode, as at start.
    fn config_mode(&self, o: &OutputModel, name: &str, change: &DisplayChange) -> Option<HeadMode> {
        let (before, now) = (&change.before, &change.now);
        let size = now.screen_modes.get(name).copied();
        let rate = now.screen_rates.get(name).copied();
        if (
            before.screen_modes.get(name).copied(),
            before.screen_rates.get(name).copied(),
        ) == (size, rate)
        {
            return None;
        }
        let current = HeadMode {
            size: (o.phys.0 as i32, o.phys.1 as i32),
            refresh_mhz: o.refresh_mhz as i32,
            preferred: true,
        };
        let modes = match self.output_management.info.get(&o.id) {
            Some(info) if !info.modes.is_empty() => info.modes.as_slice(),
            _ => std::slice::from_ref(&current),
        };
        let i = choose_mode(modes, size, rate).or_else(|| {
            log_line!(
                "[infynitae] Screen {}: no mode {size:?} @ {rate:?} mHz; it takes its preferred one",
                o.key.connector
            );
            choose_mode(modes, None, None)
        })?;
        let m = modes[i];
        (m.size != current.size || m.refresh_mhz != current.refresh_mhz).then_some(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_key_as_the_backends_do_at_start() {
        let s = DisplaySettings::parse(
            "OUTPUT_SCALE=\"1.5\"\nOUTPUT_DP_1_SCALE=\"2\"\nOUTPUT_HDMI_A_1_SCALE=\"9\"\n\
             OUTPUT_ORDER=\"DP-1, HDMI-A-1\"\nOUTPUT_LAYOUT=\"right-to-left\"\n\
             VRR_ENABLED=\"auto\"\nVRR=\"off\"\nADAPTIVE_SYNC=\"on\"\nVIRTUAL_SCREEN=\"off\"\n",
        );
        assert_eq!(s.scale_120, Some(180));
        assert_eq!(s.screen_scales.get("dp-1"), Some(&240));
        assert_eq!(s.screen_scales.get("hdmi-a-1"), None, "outside 0.25 … 8");
        assert_eq!(s.order, vec!["dp-1", "hdmi-a-1"]);
        assert!(s.right_to_left);
        assert_eq!(s.vrr, Some(false), "the first line that says on or off");
        assert!(!s.virtual_screen);
        assert_eq!(s.scale_for("DP-1"), 240);
        assert_eq!(s.scale_for("eDP-1"), 180);

        let empty = DisplaySettings::parse("");
        assert_eq!(empty.scale_120, None);
        assert_eq!(empty.scale_for("DP-1"), 120);
        assert_eq!(empty.vrr, None);
        assert!(empty.virtual_screen);
    }

    #[test]
    fn rotations_are_read_by_their_names() {
        assert_eq!(parse_transform("90"), Some(Transform::R90));
        assert_eq!(
            parse_transform(" Flipped-270 "),
            Some(Transform::Flipped270)
        );
        assert_eq!(parse_transform("normal"), Some(Transform::Normal));
        assert_eq!(parse_transform("45"), None);
        assert_eq!(parse_transform(""), None);
    }

    #[test]
    fn modes_rates_and_turns_are_read_per_screen() {
        let s = DisplaySettings::parse(
            "OUTPUT_DP_1_MODE=\"2560x1440\"\nOUTPUT_DP_1_RATE=\"143.9\"\n\
             OUTPUT_EDP_1_TRANSFORM=\"90\"\nOUTPUT_HDMI_A_1_MODE=\"big\"\n\
             OUTPUT_HDMI_A_1_TRANSFORM=\"45\"\n",
        );
        assert_eq!(s.screen_modes.get("dp-1"), Some(&(2560, 1440)));
        assert_eq!(s.screen_rates.get("dp-1"), Some(&143_900));
        assert_eq!(s.screen_transforms.get("edp-1"), Some(&Transform::R90));
        assert_eq!(s.screen_modes.get("hdmi-a-1"), None, "not a size");
        assert_eq!(s.screen_transforms.get("hdmi-a-1"), None, "not a turn");
    }

    #[test]
    fn a_mode_is_chosen_as_at_start() {
        let m = |w, h, refresh_mhz, preferred| HeadMode {
            size: (w, h),
            refresh_mhz,
            preferred,
        };
        let modes = [
            m(2560, 1440, 59_951, true),
            m(2560, 1440, 143_912, false),
            m(1920, 1080, 60_000, false),
        ];
        assert_eq!(
            choose_mode(&modes, None, None),
            Some(1),
            "preferred size, highest rate"
        );
        assert_eq!(
            choose_mode(&modes, None, Some(60_000)),
            Some(0),
            "nearest rate"
        );
        assert_eq!(choose_mode(&modes, Some((1920, 1080)), None), Some(2));
        assert_eq!(
            choose_mode(&modes, Some((800, 600)), None),
            None,
            "no such size"
        );
    }

    #[test]
    fn a_change_is_kept_from_the_second_reload_on() {
        use smithay::reexports::wayland_server::Display;
        let (core, _, _, _) =
            infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
        let display: Display<WaylandState> = Display::new().unwrap();
        let mut state = WaylandState::new(&display.handle(), core);

        // the first: what the backend started with
        state.note_display_settings(DisplaySettings::parse("VRR_ENABLED=\"auto\"\n"));
        assert_eq!(state.take_display_change(), None);

        state.note_display_settings(DisplaySettings::parse("VRR_ENABLED=\"auto\"\n"));
        assert_eq!(state.take_display_change(), None, "nothing changed");

        state.note_display_settings(DisplaySettings::parse("VRR_ENABLED=\"on\"\n"));
        state.note_display_settings(DisplaySettings::parse(
            "VRR_ENABLED=\"on\"\nVIRTUAL_SCREEN=\"off\"\n",
        ));
        let change = state.take_display_change().expect("a change");
        assert_eq!(change.before.vrr, None, "the oldest before");
        assert_eq!(change.now.vrr, Some(true));
        assert!(change.before.virtual_screen && !change.now.virtual_screen);
        assert_eq!(state.take_display_change(), None, "taken once");

        // changed and changed back before the backend looked: nothing to do
        state.note_display_settings(DisplaySettings::parse("VRR_ENABLED=\"off\"\n"));
        state.note_display_settings(DisplaySettings::parse(
            "VRR_ENABLED=\"on\"\nVIRTUAL_SCREEN=\"off\"\n",
        ));
        assert_eq!(state.take_display_change(), None);
    }
}
