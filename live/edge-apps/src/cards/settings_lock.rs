//! Settings: Lock and sleep (docs/EDGE_SETTINGS.md, `settings/lock`): the "Lock & idle" keys
//! and BATTERY_LIMIT. Left to the file: IDLE_AUTO_LOCK, SLEEP_LOCK, SCREEN_LOCKER.

use super::settings_rows::{After, Control, Setting, TableCard};
use crate::card::{render_value, Action, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::provider::{CardKind, Size};
use std::any::Any;
use std::path::{Path, PathBuf};

pub const KIND: CardKind = CardKind {
    id: "lock",
    title: "Lock and sleep",
    about: "Screen timeout, lock on idle or sleep, battery limit",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "battery_limit"],
};

const ON_OFF: Control = Control::Switch {
    on: "true",
    off: "false",
};

const SCREENS_OFF: Setting = Setting {
    key: "IDLE_SLEEP_TIMEOUT_MIN",
    name: "Screens off after",
    says: "",
    control: Control::Pick(&[
        ("5", "5 minutes"),
        ("10", "10 minutes"),
        ("15", "15 minutes"),
        ("30", "30 minutes"),
        ("60", "60 minutes"),
        ("0", "Never"),
    ]),
    after: After::Save,
    more: false,
    needs: &[],
};

const LOCK_WHEN_AWAY: Setting = Setting {
    key: "LOCK_WHEN_AWAY",
    name: "Lock when away",
    says: "When the screens go off or before sleep",
    control: ON_OFF,
    after: After::Reload("sleep-lock"),
    more: false,
    needs: &[],
};

const BATTERY_LIMIT: Setting = Setting {
    key: "BATTERY_LIMIT",
    name: "Stop charging at",
    says: "Below full wears the battery more slowly",
    control: Control::Pick(&[("off", "Full"), ("80", "80 %"), ("90", "90 %")]),
    after: After::Save,
    more: false,
    needs: &[],
};

const START_LOCKED: Setting = Setting {
    key: "START_LOCKED",
    name: "Start at the lock screen",
    says: "",
    control: ON_OFF,
    after: After::Restart,
    more: true,
    needs: &[],
};

const SHUTDOWN_SCREEN: Setting = Setting {
    key: "SHUTDOWN_SCREEN",
    name: "Show \"Shutting down\"",
    says: "On reboot or power off",
    control: ON_OFF,
    after: After::Use,
    more: true,
    needs: &[],
};

pub const TABLE: &[Setting] = &[
    SCREENS_OFF,
    LOCK_WHEN_AWAY,
    BATTERY_LIMIT,
    START_LOCKED,
    SHUTDOWN_SCREEN,
];

// `Setting::needs` checks PATH programs only, so `LockCard` swaps between the full table and this
// slice without `BATTERY_LIMIT` when sysfs has no `BAT*/charge_control_end_threshold`.
const TABLE_NO_BATTERY: &[Setting] = &[SCREENS_OFF, LOCK_WHEN_AWAY, START_LOCKED, SHUTDOWN_SCREEN];

/// Whether any battery on this PC exposes `charge_control_end_threshold`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatteryLimitData {
    pub supported: bool,
}

/// Checks `power_supply` (`INFYNITAE_POWER_SUPPLY` or `/sys/class/power_supply`) for a battery
/// charging limit file (`live/battery-limit:12`).
pub struct BatteryLimitSource {
    pub power_supply: PathBuf,
}

impl BatteryLimitSource {
    pub fn system() -> Self {
        let power_supply = std::env::var_os("INFYNITAE_POWER_SUPPLY")
            .filter(|s| !s.is_empty())
            .map_or_else(|| PathBuf::from("/sys/class/power_supply"), PathBuf::from);
        BatteryLimitSource { power_supply }
    }
}

pub fn has_battery_limit(power_supply: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(power_supply) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_str().is_some_and(|s| s.starts_with("BAT")) {
            continue;
        }
        if entry.path().join("charge_control_end_threshold").exists() {
            return true;
        }
    }
    false
}

pub fn parse_battery_limit_fixture(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

impl Source for BatteryLimitSource {
    fn fetch(&mut self) -> Data {
        let supported = match render_value("battery_limit") {
            Some(v) => parse_battery_limit_fixture(&v),
            None => has_battery_limit(&self.power_supply),
        };
        Box::new(BatteryLimitData { supported })
    }

    fn every(&self) -> Option<std::time::Duration> {
        None
    }
}

/// Wraps `TableCard` to hide `BATTERY_LIMIT` without firmware support and run `live/battery-limit`
/// after saving it (`settings_rows::After` has no variant that also passes the chosen value).
pub struct LockCard {
    inner: TableCard,
    battery: Option<BatteryLimitData>,
}

impl Default for LockCard {
    fn default() -> Self {
        LockCard {
            inner: TableCard::new(TABLE_NO_BATTERY),
            battery: None,
        }
    }
}

fn battery_limit_set_value(cmd: &str) -> Option<&str> {
    let rest = cmd.strip_prefix("sh \"$HOME/infynitae/live/settings.sh\" log BATTERY_LIMIT ")?;
    let (_, after_set) =
        rest.split_once(" && sh \"$HOME/infynitae/live/settings.sh\" set BATTERY_LIMIT ")?;
    after_set.strip_suffix(" >/dev/null 2>&1")
}

impl Card for LockCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        if key == "battery_limit" {
            let Some(d) = data.downcast_ref::<BatteryLimitData>() else {
                return false;
            };
            let changed = self.battery != Some(*d);
            self.battery = Some(*d);
            self.inner.table = if d.supported { TABLE } else { TABLE_NO_BATTERY };
            return changed;
        }
        if key == "config" {
            if let Some(d) = data.downcast_ref::<super::settings_rows::ConfigData>() {
                // Configs from before LOCK_WHEN_AWAY only have IDLE_AUTO_LOCK / SLEEP_LOCK: derive
                // the shown state without pretending LOCK_WHEN_AWAY was already in the file.
                if !d.now.get("LOCK_WHEN_AWAY").is_some_and(|v| !v.is_empty()) {
                    let legacy: Vec<&str> = ["IDLE_AUTO_LOCK", "SLEEP_LOCK"]
                        .into_iter()
                        .filter_map(|k| d.now.get(k).map(String::as_str).filter(|v| !v.is_empty()))
                        .collect();
                    if !legacy.is_empty() {
                        let shown = if legacy.iter().any(|v| v.eq_ignore_ascii_case("true")) {
                            "true"
                        } else {
                            "false"
                        };
                        let mut migrated = d.clone();
                        migrated
                            .default
                            .insert("LOCK_WHEN_AWAY".into(), shown.into());
                        return self.inner.apply(key, &migrated as &dyn Any);
                    }
                }
            }
        }
        self.inner.apply(key, data)
    }

    fn status(&self, cx: &Cx) -> Status {
        if self.battery.is_none() {
            return Status::Loading;
        }
        self.inner.status(cx)
    }

    fn height(&self, cx: &Cx, width: f64) -> f64 {
        self.inner.height(cx, width)
    }

    fn draw(&self, cr: &cairo::Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        self.inner.draw(cr, cx, body, hits)
    }

    fn event(&mut self, ev: &Event, cx: &Cx, out: &mut Vec<Action>) -> bool {
        let start = out.len();
        let changed = self.inner.event(ev, cx, out);
        for action in &mut out[start..] {
            if let Action::Run(cmd) = action {
                if let Some(val) = battery_limit_set_value(cmd) {
                    cmd.push_str(&format!(
                        " && sh \"$HOME/infynitae/live/battery-limit\" {val} >/dev/null 2>&1"
                    ));
                }
            }
        }
        changed
    }

    fn tick(&mut self, dt_ms: f64) -> bool {
        self.inner.tick(dt_ms)
    }

    fn show_list(&mut self) {
        self.inner.show_list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Button;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings::Settings;
    use crate::cards::settings_rows::parse_config;
    use crate::cards::settings_rows::tests::data;
    use crate::provider::Provider;
    use crate::theme::Theme;
    use cairo::{Context, Format, ImageSurface};
    use std::sync::atomic::{AtomicU64, Ordering};

    const ROW: u32 = 100;
    const LIST: u32 = 5000;
    const MORE: u32 = 9000;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "infynitae-edge-apps-lock-{label}-{}-{n}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn press(hit: u32) -> Event<'static> {
        Event::Press {
            hit,
            button: Button::Left,
            at: (0.0, 0.0),
        }
    }

    fn supported(v: bool) -> BatteryLimitData {
        BatteryLimitData { supported: v }
    }

    /// Every key here exists in the shipped config, with a default the controls know.
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
        }
        assert!(def.contains_key("SCREEN_LOCKER"));
        assert!(def.contains_key("IDLE_AUTO_LOCK"));
        assert!(def.contains_key("SLEEP_LOCK"));
        assert!(!TABLE
            .iter()
            .any(|s| matches!(s.key, "SCREEN_LOCKER" | "IDLE_AUTO_LOCK" | "SLEEP_LOCK")));
        let keys_full: Vec<&str> = TABLE
            .iter()
            .filter(|s| s.key != "BATTERY_LIMIT")
            .map(|s| s.key)
            .collect();
        let keys_no_bat: Vec<&str> = TABLE_NO_BATTERY.iter().map(|s| s.key).collect();
        assert_eq!(keys_full, keys_no_bat);
    }

    #[test]
    fn provider_registers_lock_card_and_battery_limit_source() {
        let s = Settings;
        assert!(s.cards().iter().any(|k| k.id == "lock"));
        assert!(s.card("lock", None).is_some());
        assert!(s.source("battery_limit").is_some());
    }

    #[test]
    fn battery_limit_source_checks_power_supply_dir_and_fixtures() {
        // empty and missing folders
        let empty = TempDir::new("empty");
        let mut src = BatteryLimitSource {
            power_supply: empty.0.clone(),
        };
        assert_eq!(src.every(), None);
        assert_eq!(
            src.fetch().downcast_ref::<BatteryLimitData>(),
            Some(&supported(false))
        );
        let missing = BatteryLimitSource {
            power_supply: empty.0.join("missing"),
        };
        assert!(!has_battery_limit(&missing.power_supply));

        // broken entries: non-BAT dir with threshold, BAT dir without threshold, BAT file, dangling symlink
        let broken = TempDir::new("broken");
        std::fs::create_dir_all(broken.0.join("ACAD")).unwrap();
        std::fs::write(broken.0.join("ACAD/charge_control_end_threshold"), "100\n").unwrap();
        std::fs::create_dir_all(broken.0.join("BAT0")).unwrap();
        std::fs::write(broken.0.join("BAT1"), "not a dir").unwrap();
        std::fs::create_dir_all(broken.0.join("BAT2")).unwrap();
        std::os::unix::fs::symlink(
            broken.0.join("nowhere"),
            broken.0.join("BAT2/charge_control_end_threshold"),
        )
        .unwrap();
        let mut broken_src = BatteryLimitSource {
            power_supply: broken.0.clone(),
        };
        assert_eq!(
            broken_src.fetch().downcast_ref::<BatteryLimitData>(),
            Some(&supported(false))
        );

        // usual: BAT0 threshold file exists alongside non-battery supplies
        let usual = TempDir::new("usual");
        std::fs::create_dir_all(usual.0.join("AC")).unwrap();
        std::fs::create_dir_all(usual.0.join("BAT0")).unwrap();
        std::fs::write(usual.0.join("BAT0/charge_control_end_threshold"), "80\n").unwrap();
        let mut usual_src = BatteryLimitSource {
            power_supply: usual.0.clone(),
        };
        assert_eq!(
            usual_src.fetch().downcast_ref::<BatteryLimitData>(),
            Some(&supported(true))
        );

        // Linux sysfs style: BAT0 is a symlink to the device directory
        let symlinked = TempDir::new("symlinked");
        let target = symlinked.0.join("device_bat");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("charge_control_end_threshold"), "100\n").unwrap();
        std::os::unix::fs::symlink(&target, symlinked.0.join("BAT0")).unwrap();
        assert!(has_battery_limit(&symlinked.0));

        assert!(parse_battery_limit_fixture("1"));
        assert!(parse_battery_limit_fixture(" true\n"));
        assert!(parse_battery_limit_fixture("ON"));
        assert!(!parse_battery_limit_fixture("0"));
        assert!(!parse_battery_limit_fixture("false"));
        assert!(!parse_battery_limit_fixture(""));
        assert!(!parse_battery_limit_fixture("corrupt"));
    }

    #[test]
    fn hiding_rule_and_loading_status() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx_s = test_cx(&th);
        let mut cx_m = test_cx(&th);
        cx_m.size = Size::M;

        // waits for both config and battery_limit in either order
        let mut c = LockCard::default();
        assert_eq!(c.status(&cx_s), Status::Loading);
        assert!(c.apply("config", &data(&[], &[], &[]) as &dyn Any));
        assert_eq!(c.status(&cx_s), Status::Loading);
        assert!(c.apply("battery_limit", &supported(false) as &dyn Any));
        assert_eq!(c.status(&cx_s), Status::Ready);
        assert!(!c.apply("battery_limit", &supported(false) as &dyn Any));

        let mut c2 = LockCard::default();
        assert!(c2.apply("battery_limit", &supported(false) as &dyn Any));
        assert_eq!(c2.status(&cx_s), Status::Loading);
        assert!(c2.apply("config", &data(&[], &[], &[]) as &dyn Any));
        assert_eq!(c2.status(&cx_s), Status::Ready);

        // without battery limit: 2 visible + More at S (3 rows), 4 at M; search for battery finds nothing
        let h_s_no_bat = c.height(&cx_s, 440.0);
        let h_m_no_bat = c.height(&cx_m, 440.0);
        assert!(c.event(&Event::Text("charging"), &cx_s, &mut Vec::new()));
        assert_eq!(c.status(&cx_s), Status::Hidden);
        assert!(c.event(&Event::Text("sleep"), &cx_s, &mut Vec::new()));
        assert_eq!(c.status(&cx_s), Status::Ready);
        assert!(c.event(&Event::Text(""), &cx_s, &mut Vec::new()));

        // with battery limit: 1 extra visible row at S and M; search for charging shows the row
        assert!(c.apply("battery_limit", &supported(true) as &dyn Any));
        assert_eq!(c.status(&cx_s), Status::Ready);
        assert!(c.height(&cx_s, 440.0) > h_s_no_bat);
        assert!(c.height(&cx_m, 440.0) > h_m_no_bat);
        assert!(c.event(&Event::Text("charging"), &cx_s, &mut Vec::new()));
        assert_eq!(c.status(&cx_s), Status::Ready);
        assert!(c.event(&Event::Text(""), &cx_s, &mut Vec::new()));

        // clicking More reveals Start at the lock screen and Show "Shutting down"
        let before_more = c.height(&cx_s, 440.0);
        assert!(c.event(&press(MORE), &cx_s, &mut Vec::new()));
        assert!(c.height(&cx_s, 440.0) > before_more);
    }

    #[test]
    fn commands_for_switch_pick_reload_restart_and_battery_limit() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx = test_cx(&th);
        let mut cx_m = test_cx(&th);
        cx_m.size = Size::M;

        let mut c = LockCard::default();
        c.apply("battery_limit", &supported(true) as &dyn Any);
        c.apply(
            "config",
            &data(
                &[
                    ("IDLE_SLEEP_TIMEOUT_MIN", "15"),
                    ("LOCK_WHEN_AWAY", "true"),
                    ("BATTERY_LIMIT", "off"),
                ],
                &[
                    ("LOCK_WHEN_AWAY", "true"),
                    ("START_LOCKED", "false"),
                    ("SHUTDOWN_SCREEN", "true"),
                ],
                &[],
            ) as &dyn Any,
        );

        // Pick (row 0: IDLE_SLEEP_TIMEOUT_MIN -> "5")
        let mut out = Vec::new();
        assert!(c.event(&press(ROW), &cx, &mut out));
        assert!(out.is_empty());
        assert!(c.event(&press(LIST), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log IDLE_SLEEP_TIMEOUT_MIN '15' && sh \"$HOME/infynitae/live/settings.sh\" set IDLE_SLEEP_TIMEOUT_MIN '5' >/dev/null 2>&1".into()
            )]
        );

        // Switch [reload] (row 1: LOCK_WHEN_AWAY -> "false", infynitae-reload sleep-lock)
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 10 + 1), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log LOCK_WHEN_AWAY 'true' && sh \"$HOME/infynitae/live/settings.sh\" set LOCK_WHEN_AWAY 'false' >/dev/null 2>&1 && sh \"$HOME/infynitae/live/infynitae-reload\" sleep-lock >/dev/null 2>&1".into()
            )]
        );

        // Pick BATTERY_LIMIT (row 2: "off" -> "80", appends battery-limit '80')
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 20), &cx, &mut out));
        assert!(c.event(&press(LIST + 1), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log BATTERY_LIMIT 'off' && sh \"$HOME/infynitae/live/settings.sh\" set BATTERY_LIMIT '80' >/dev/null 2>&1 && sh \"$HOME/infynitae/live/battery-limit\" '80' >/dev/null 2>&1".into()
            )]
        );

        // Switch [restart] (row 3: START_LOCKED unset -> "true") and Switch [use] (row 4)
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 30 + 1), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log START_LOCKED - && sh \"$HOME/infynitae/live/settings.sh\" set START_LOCKED 'true' >/dev/null 2>&1".into()
            )]
        );

        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 40 + 1), &cx_m, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log SHUTDOWN_SCREEN - && sh \"$HOME/infynitae/live/settings.sh\" set SHUTDOWN_SCREEN 'false' >/dev/null 2>&1".into()
            )]
        );

        // Quote-safe check: old value containing a single quote still gets battery-limit appended
        c.apply(
            "config",
            &data(&[("BATTERY_LIMIT", "a'b")], &[], &[]) as &dyn Any,
        );
        let mut out = Vec::new();
        assert!(c.event(&press(ROW + 20), &cx, &mut out));
        assert!(c.event(&press(LIST + 2), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log BATTERY_LIMIT 'a'\\''b' && sh \"$HOME/infynitae/live/settings.sh\" set BATTERY_LIMIT '90' >/dev/null 2>&1 && sh \"$HOME/infynitae/live/battery-limit\" '90' >/dev/null 2>&1".into()
            )]
        );

        // Legacy migration without LOCK_WHEN_AWAY: both false -> switch off (click sets true)
        let mut legacy = LockCard::default();
        legacy.apply("battery_limit", &supported(false) as &dyn Any);
        legacy.apply(
            "config",
            &data(
                &[("IDLE_AUTO_LOCK", "false"), ("SLEEP_LOCK", "false")],
                &[("LOCK_WHEN_AWAY", "true")],
                &[],
            ) as &dyn Any,
        );
        let mut out = Vec::new();
        assert!(legacy.event(&press(ROW + 10 + 1), &cx, &mut out));
        assert_eq!(
            out,
            vec![Action::Run(
                "sh \"$HOME/infynitae/live/settings.sh\" log LOCK_WHEN_AWAY - && sh \"$HOME/infynitae/live/settings.sh\" set LOCK_WHEN_AWAY 'true' >/dev/null 2>&1 && sh \"$HOME/infynitae/live/infynitae-reload\" sleep-lock >/dev/null 2>&1".into()
            )]
        );
    }

    #[test]
    fn height_matches_what_is_drawn() {
        let th = Theme::from_config(&crate::config::Config::from_text(""));
        let cx_s = test_cx(&th);
        let mut cx_m = test_cx(&th);
        cx_m.size = Size::M;

        let surf = ImageSurface::create(Format::ARgb32, 500, 900).unwrap();
        let cr = Context::new(&surf).unwrap();

        for &bat in &[false, true] {
            let mut c = LockCard::default();
            c.apply("battery_limit", &supported(bat) as &dyn Any);
            c.apply(
                "config",
                &data(&[("IDLE_SLEEP_TIMEOUT_MIN", "20")], &[], &[]) as &dyn Any,
            );

            for cx in [&cx_s, &cx_m] {
                let mut hits = Hits::new(None, None);
                c.draw(&cr, cx, (0.0, 0.0, 440.0, 900.0), &mut hits);
                let bottom = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
                assert!(
                    (bottom - c.height(cx, 440.0)).abs() < 0.5,
                    "bat={bat} {bottom}"
                );
            }

            // Pick list open in place
            c.show_list();
            let mut hits = Hits::new(None, None);
            c.draw(&cr, &cx_s, (0.0, 0.0, 440.0, 900.0), &mut hits);
            let bottom = hits.list.iter().map(|(_, r)| r.1 + r.3).fold(0.0, f64::max);
            assert!(
                (bottom - c.height(&cx_s, 440.0)).abs() < 0.5,
                "open bat={bat} {bottom}"
            );
        }
    }
}
