//! Settings: Advanced (docs/EDGE_SETTINGS.md, `settings/advanced`): the "Advanced" section of
//! config/config, the last group of `settings/all` and closed until opened like every group.

use super::settings_rows::{After, Control, Setting};
use crate::provider::{CardKind, Size};

pub const KIND: CardKind = CardKind {
    id: "advanced",
    title: "Advanced",
    about: "Workarounds for apps and drivers, commands for built-in parts",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config"],
};

const ON_OFF: Control = Control::Switch {
    on: "true",
    off: "false",
};

const fn row(
    key: &'static str,
    name: &'static str,
    says: &'static str,
    control: Control,
) -> Setting {
    Setting {
        key,
        name,
        says,
        control,
        after: After::Save,
        more: false,
        needs: &[],
    }
}

const fn used(key: &'static str, name: &'static str, says: &'static str) -> Setting {
    Setting {
        after: After::Use,
        ..row(key, name, says, Control::Text)
    }
}

pub const TABLE: &[Setting] = &[
    row(
        "CLIP_TO_GEOMETRY",
        "Clip windows to their edges",
        "Off: the margins apps keep for shadows can show as black blocks",
        ON_OFF,
    ),
    row(
        "WINE_TRANSPARENCY",
        "Transparent Wine and X11 windows",
        "No black boxes behind Wine, Proton and X11 layered windows",
        ON_OFF,
    ),
    row(
        "FORCE_TRANSPARENT_APPS",
        "More apps blended that way",
        "App ids or titles, with commas",
        Control::Text,
    ),
    row(
        "DIRECT_SCANOUT",
        "Games straight to the screen",
        "A fullscreen game's frames go to the screen directly",
        ON_OFF,
    ),
    row(
        "FIT_IS_FULLSCREEN",
        "A window filling the screen is fullscreen",
        "Shown directly when possible, and tearing applies",
        ON_OFF,
    ),
    row(
        "FRAME_GUARD",
        "Frame guard",
        "With Latency off: how late in a refresh a frame waits for the next",
        Control::Slide {
            min: 0.0,
            max: 100.0,
            step: 1.0,
            unit: " %",
        },
    ),
    row(
        "HARDWARE_CURSOR",
        "Cursor on the GPU's cursor layer",
        "Off: drawn into each frame",
        ON_OFF,
    ),
    row(
        "OVERLAY_PLANES",
        "Windows on overlay planes",
        "Experimental: on AMD it dropped frames while Electron apps drew",
        ON_OFF,
    ),
    row(
        "MAX_APP_SCALE",
        "Sharper when zoomed in",
        "",
        Control::Pick(&[("1", "Never"), ("2", "2x"), ("3", "3x")]),
    ),
    row(
        "ZOOM_SHARPEN",
        "When apps redraw sharper",
        "",
        Control::Pick(&[("after", "After the zoom stops"), ("live", "While zooming")]),
    ),
    used(
        "SCREENSHOT_CMD",
        "Screenshot command",
        "Super+Shift+S; empty: the built-in one",
    ),
    used(
        "MONITOR_LOG_DIR",
        "Lab log folder",
        "lab*.log files for the System monitor's GPU card",
    ),
    used(
        "SCREEN_LOCKER",
        "Lock screen command",
        "Super+Shift+L; auto: infynitae-lock",
    ),
    used(
        "UPDATE_SIGNING_KEY",
        "Update signing key",
        "infynitae-update takes only versions signed by it",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings_rows::parse_config;

    /// The table is the config's Advanced section, key for key and in its order, each with a
    /// default its control has and the section's tag (or the key's own).
    #[test]
    fn the_table_is_the_advanced_section_of_config_config() {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config"))
                .unwrap();
        let section = text
            .split("# ═══ Advanced ")
            .nth(1)
            .expect("an Advanced section in config/config");
        let keys: Vec<&str> = section
            .lines()
            .filter_map(|l| l.split_once('=').map(|(k, _)| k))
            .filter(|k| k.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
            .collect();
        assert_eq!(keys, TABLE.iter().map(|s| s.key).collect::<Vec<_>>());

        let def = parse_config(&text);
        for s in TABLE {
            let v = &def[s.key];
            match s.control {
                Control::Switch { on, off } => assert!(v == on || v == off, "{} = {v}", s.key),
                Control::Pick(o) => assert!(o.iter().any(|(x, _)| x == v), "{} = {v}", s.key),
                Control::Slide { min, max, .. } => {
                    let n: f64 = v.parse().unwrap();
                    assert!((min..=max).contains(&n), "{} = {v}", s.key);
                }
                Control::Text => {}
            }
            let block: Vec<&str> = section
                .lines()
                .skip_while(|l| !l.starts_with(&format!("{}=", s.key)))
                .skip(1)
                .take_while(|l| l.trim_start().starts_with('#'))
                .collect();
            let use_tag = block.iter().any(|l| l.contains("[use]"));
            assert_eq!(s.after == After::Use, use_tag, "tag for {}", s.key);
        }
    }
}
