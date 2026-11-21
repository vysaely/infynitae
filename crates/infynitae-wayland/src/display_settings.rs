//! The screens' settings in the config that apply while running: `OUTPUT_SCALE`,
//! `OUTPUT_<NAME>_SCALE`, `OUTPUT_ORDER`, `OUTPUT_LAYOUT`, `VRR_ENABLED` and `VIRTUAL_SCREEN`. A
//! reload that changes one leaves a [`DisplayChange`] for the backend
//! ([`WaylandState::take_display_change`]); the new scales and order of the screens connected now
//! are [`HeadChange`]s ([`WaylandState::display_head_changes`]), which the backend carries out as a
//! display settings tool's configuration (`output_management`). The first reload, at start, only
//! records what the backend started with.

use std::collections::BTreeMap;

use crate::WaylandState;
use crate::output_management::HeadChange;

/// What the config says about the screens (each key read as the backends read it at start).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplaySettings {
    /// `OUTPUT_SCALE` in 1/120 units; `None` when unset or outside 0.25 … 8 (1.0 then).
    pub scale_120: Option<u32>,
    /// `OUTPUT_<NAME>_SCALE` in 1/120 units, by connector name lowercased, `_` written as `-`.
    pub screen_scales: BTreeMap<String, u32>,
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

impl DisplaySettings {
    pub fn parse(content: &str) -> DisplaySettings {
        let mut s = DisplaySettings {
            scale_120: None,
            screen_scales: BTreeMap::new(),
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
                    if let Some(name) = key
                        .strip_prefix("OUTPUT_")
                        .and_then(|k| k.strip_suffix("_SCALE"))
                        && let Some(scale) = scale_120(val)
                    {
                        s.screen_scales
                            .insert(name.replace('_', "-").to_lowercase(), scale);
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

    /// The scales and positions `change` gives the screens connected now; empty when no scale,
    /// `OUTPUT_ORDER` or `OUTPUT_LAYOUT` changed. The screens are placed side by side from 0 in
    /// `OUTPUT_ORDER` (those it does not name after them, as they are now), else in their order
    /// now, turned round when `OUTPUT_LAYOUT` changed.
    pub fn display_head_changes(&self, change: &DisplayChange) -> Vec<HeadChange> {
        let (before, now) = (&change.before, &change.now);
        if before.scale_120 == now.scale_120
            && before.screen_scales == now.screen_scales
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
            let scale = now.scale_for(&o.key.connector);
            let position = (x.round() as i32, 0);
            x += f64::from(o.view_size().0) * 120.0 / f64::from(scale);
            let scale_changed = (o.scale * 120.0).round() as u32 != scale;
            let moved = (o.arrangement.0, o.arrangement.1)
                != (f64::from(position.0), f64::from(position.1));
            if scale_changed || moved {
                changes.push(HeadChange {
                    output: o.id,
                    mode: None,
                    position: moved.then_some(position),
                    scale_120: scale_changed.then_some(scale),
                    transform: None,
                    adaptive_sync: None,
                });
            }
        }
        changes
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
