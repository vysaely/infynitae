//! Settings: Games (docs/EDGE_SETTINGS.md, `settings/games`): the "Performance & latency" keys of
//! config/config a person changes. Left to the file: GAME_QUIET, GPU_RELIEF, LATENCY,
//! DIRECT_SCANOUT, FIT_IS_FULLSCREEN, HARDWARE_CURSOR, FRAME_GUARD, OVERLAY_PLANES, LATENCY_STATS.

use super::settings_rows::{After, Control, Setting};
use crate::provider::{CardKind, Size};

pub const KIND: CardKind = CardKind {
    id: "games",
    title: "Games",
    about: "Game mode, tearing, background priority",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config"],
};

const ON_OFF: Control = Control::Switch {
    on: "true",
    off: "false",
};

pub const TABLE: &[Setting] = &[
    Setting {
        key: "GAME_MODE",
        name: "Game mode",
        says: "Full CPU speed while a game is fullscreen",
        control: ON_OFF,
        after: After::Save,
        more: false,
        needs: &["gamemoded"],
    },
    Setting {
        key: "TEARING",
        name: "Tearing in fullscreen",
        says: "Each frame at once, with a tear line",
        control: Control::Pick(&[
            ("always", "Always"),
            ("true", "When the game asks"),
            ("false", "Never"),
        ]),
        after: After::Save,
        more: false,
        needs: &[],
    },
    Setting {
        key: "BACKGROUND_PRIORITY",
        name: "Lower priority for apps out of sight",
        says: "After 15 s with no window seen",
        control: ON_OFF,
        after: After::Save,
        more: true,
        needs: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key here exists in the shipped config, with a default the controls know.
    #[test]
    fn every_key_is_in_config_config_with_a_value_its_control_has() {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap();
        let def = super::super::settings_rows::parse_config(&text);
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
        }
        assert_eq!(TABLE.len(), 3);
        assert!(def.contains_key("LATENCY"));
        assert!(!TABLE.iter().any(|s| s.key == "LATENCY"));
    }
}
