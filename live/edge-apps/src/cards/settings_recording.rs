//! Settings card for screenshots and recording (docs/EDGE_SETTINGS.md, `settings/recording`).
//! Left to the file: `REPLAY_DIR`, `SCREENSHOT_CMD`, `RECORD_NOTIFY`.

use std::any::Any;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::card::{Action, Button, Card, Cx, Data, Event, Hits, Rect, Source, Status};
use crate::cards::settings_rows::{change_command, After, ConfigData, Control, Setting};
use crate::provider::{CardKind, Size};
use crate::ui::{self, LIST_ROW_H, ROW_H};

pub const KIND: CardKind = CardKind {
    id: "recording",
    title: "Screenshots and recording",
    about: "Screen freeze, replay length, sound, format and folders",
    sizes: &[Size::S, Size::M],
    default: Size::S,
    half: false,
    sources: &["config", "recording"],
};

const ON_OFF: Control = Control::Switch {
    on: "true",
    off: "false",
};

/// Config-backed settings for `settings/recording`.
/// The three folder-open buttons (`SCREENSHOT_DIR`, `CLIPS_DIR`, `RECORDINGS_DIR`) sit between
/// `FREEZE_SCREENSHOT` and the `More` rows (`RECORD_CODEC`, `REPLAY_BACKEND`).
pub const TABLE: &[Setting] = &[
    Setting {
        key: "REPLAY_BUFFER_MINUTES",
        name: "Replay keeps the last",
        says: "",
        control: Control::Pick(&[
            ("2", "2 minutes"),
            ("5", "5 minutes"),
            ("10", "10 minutes"),
            ("20", "20 minutes"),
            ("30", "30 minutes"),
        ]),
        after: After::Reload("recorder"),
        more: false,
        needs: &[],
    },
    Setting {
        key: "RECORD_AUDIO",
        name: "Sound recorded",
        says: "",
        control: Control::Pick(&[
            ("desktop", "The PC"),
            ("mic", "The microphone"),
            ("both", "Both"),
            ("none", "None"),
        ]),
        after: After::Reload("recorder"),
        more: false,
        needs: &[],
    },
    Setting {
        key: "RECORD_FPS",
        name: "Frames per second",
        says: "",
        control: Control::Pick(&[("30", "30"), ("60", "60"), ("120", "120")]),
        after: After::Reload("recorder"),
        more: false,
        needs: &[],
    },
    Setting {
        key: "FREEZE_SCREENSHOT",
        name: "Freeze the screen while picking a region",
        says: "",
        control: ON_OFF,
        after: After::Use,
        more: false,
        needs: &[],
    },
    Setting {
        key: "RECORD_CODEC",
        name: "Format",
        says: "",
        control: Control::Pick(&[
            ("hevc", "HEVC"),
            ("av1", "AV1"),
            ("h264", "H.264"),
            ("auto", "Automatic"),
        ]),
        after: After::Reload("recorder"),
        more: true,
        needs: &[],
    },
    Setting {
        key: "REPLAY_BACKEND",
        name: "Recorded by",
        says: "",
        control: Control::Pick(&[
            ("auto", "Automatic"),
            ("gpu-screen-recorder", "gpu-screen-recorder"),
            ("infynitae", "Infynitae"),
        ]),
        after: After::Reload("recorder"),
        more: true,
        needs: &[],
    },
];

/// Buttons that open each output folder rather than editing a key in-card.
pub const FOLDERS: &[(&str, &str)] = &[
    ("Screenshots", "SCREENSHOT_DIR"),
    ("Clips", "CLIPS_DIR"),
    ("Recordings", "RECORDINGS_DIR"),
];

pub const KEYS: &[&str] = &[FOLDERS[0].1, FOLDERS[1].1, FOLDERS[2].1];

const ROW: u32 = 100;
const FOLDER: u32 = 4000;
const LIST: u32 = 5000;
const MORE: u32 = 9000;

const BUTTONS_H: f64 = 38.0;
const BUTTON_TOP_PAD: f64 = 6.0;
const BUTTON_GAP: f64 = 8.0;

/// Home path and local recorder binary presence outside `ConfigData`'s `PATH` scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingEnv {
    pub home: PathBuf,
    pub local_recorder: bool,
}

/// Fetches `$HOME` and checks whether `~/.local/bin/infynitae-recorder` is executable.
pub struct RecordingSource {
    pub home: PathBuf,
}

impl RecordingSource {
    pub fn system() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        Self { home }
    }
}

impl Source for RecordingSource {
    fn fetch(&mut self) -> Data {
        Box::new(RecordingEnv {
            home: self.home.clone(),
            local_recorder: has_local_recorder(&self.home),
        })
    }
}

fn has_local_recorder(home: &Path) -> bool {
    if home.as_os_str().is_empty() {
        return false;
    }
    let path = home.join(".local/bin/infynitae-recorder");
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
}

#[derive(Default)]
pub struct RecordingCard {
    config: Option<ConfigData>,
    env: Option<RecordingEnv>,
    pending: HashMap<&'static str, String>,
    pick: Option<usize>,
    list: ui::List,
    more: bool,
    query: String,
}

impl RecordingCard {
    pub fn new() -> Self {
        Self::default()
    }

    fn recorder_present(&self) -> bool {
        let (Some(cfg), Some(env)) = (self.config.as_ref(), self.env.as_ref()) else {
            return false;
        };
        cfg.has("infynitae-recorder") || env.local_recorder
    }

    fn value(&self, idx: usize) -> &str {
        let key = TABLE[idx].key;
        if let Some(v) = self.pending.get(key) {
            return v.as_str();
        }
        self.config.as_ref().map_or("", |cfg| cfg.value(key))
    }

    fn visible_rows(&self, cx: &Cx) -> (Vec<usize>, bool) {
        let searching = !self.query.trim().is_empty();
        let all = cx.size != Size::S || self.more || searching;
        let mut shown = Vec::new();
        let mut hidden_more = false;
        for (i, s) in TABLE.iter().enumerate() {
            if searching && !matches_query(s.name, s.key, &self.query) {
                continue;
            }
            if s.more && !all {
                hidden_more = true;
                continue;
            }
            shown.push(i);
        }
        let more_row = !searching && cx.size == Size::S && (hidden_more || self.more);
        (shown, more_row)
    }

    fn visible_folders(&self) -> Vec<usize> {
        let searching = !self.query.trim().is_empty();
        FOLDERS
            .iter()
            .enumerate()
            .filter_map(|(i, &(label, key))| {
                (!searching || matches_query(label, key, &self.query)).then_some(i)
            })
            .collect()
    }

    fn pick_options(&self, idx: usize) -> Vec<(&'static str, &'static str)> {
        let Control::Pick(opts) = TABLE[idx].control else {
            return Vec::new();
        };
        let has_gsr = self
            .config
            .as_ref()
            .is_some_and(|c| c.has("gpu-screen-recorder"));
        opts.iter()
            .copied()
            .filter(|&(val, _)| {
                TABLE[idx].key != "REPLAY_BACKEND" || val != "gpu-screen-recorder" || has_gsr
            })
            .collect()
    }

    fn row_h(&self, idx: usize) -> f64 {
        let pick_h = if self.pick == Some(idx) {
            self.list.height(self.pick_options(idx).len())
        } else {
            0.0
        };
        ROW_H + pick_h
    }

    fn commit(&mut self, idx: usize, val: String, out: &mut Vec<Action>) -> bool {
        let Some(cfg) = self.config.as_ref() else {
            return false;
        };
        if self.value(idx) == val {
            return false;
        }
        let s = &TABLE[idx];
        let old_line = cfg.now.get(s.key).map(String::as_str);
        // The recorder is reloaded only while ENABLE_REPLAY_BUFFER is true.
        let after = match s.after {
            After::Reload("recorder") if !replay_enabled(cfg) => After::Use,
            other => other,
        };
        out.push(Action::Run(change_command(s.key, old_line, &val, after)));
        self.pending.insert(s.key, val);
        true
    }
}

impl Card for RecordingCard {
    fn apply(&mut self, key: &str, data: &dyn Any) -> bool {
        match key {
            "config" => {
                let Some(cfg) = data.downcast_ref::<ConfigData>() else {
                    return false;
                };
                self.pending.retain(|k, v| cfg.value(k) != v.as_str());
                let changed = self.config.as_ref() != Some(cfg);
                self.config = Some(cfg.clone());
                changed
            }
            "recording" => {
                let Some(env) = data.downcast_ref::<RecordingEnv>() else {
                    return false;
                };
                let changed = self.env.as_ref() != Some(env);
                self.env = Some(env.clone());
                changed
            }
            _ => false,
        }
    }

    fn status(&self, cx: &Cx) -> Status {
        if self.config.is_none() || self.env.is_none() {
            return Status::Loading;
        }
        if !self.recorder_present() {
            return Status::Hidden;
        }
        let (rows, _) = self.visible_rows(cx);
        let folders = self.visible_folders();
        if rows.is_empty() && folders.is_empty() {
            Status::Hidden
        } else {
            Status::Ready
        }
    }

    fn height(&self, cx: &Cx, _width: f64) -> f64 {
        if !matches!(self.status(cx), Status::Ready) {
            return 0.0;
        }
        let (rows, has_more) = self.visible_rows(cx);
        let folders = self.visible_folders();
        let mut h: f64 = rows.iter().map(|&i| self.row_h(i)).sum();
        if !folders.is_empty() {
            h += BUTTONS_H;
        }
        if has_more {
            h += ROW_H;
        }
        h
    }

    fn draw(&self, cr: &cairo::Context, cx: &Cx, body: Rect, hits: &mut Hits) {
        if !matches!(self.status(cx), Status::Ready) {
            return;
        }
        let (rows, has_more) = self.visible_rows(cx);
        let folders = self.visible_folders();
        let right = body.0 + body.2 - 8.0;
        let mut y = body.1;
        let mut buttons_drawn = false;

        for &idx in &rows {
            // Folder buttons sit right after the basic rows (`more == false`).
            if !buttons_drawn && TABLE[idx].more && !folders.is_empty() {
                draw_folder_buttons(cr, cx, hits, (body.0, y, body.2, BUTTONS_H), &folders);
                y += BUTTONS_H;
                buttons_drawn = true;
            }

            let s = &TABLE[idx];
            let id = ROW + 10 * idx as u32;
            let r = (body.0, y, body.2, ROW_H);
            let cur = self.value(idx);

            match s.control {
                Control::Switch { on, .. } => {
                    ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &ui::Row {
                            icon: None,
                            name: s.name,
                            state: s.says,
                            on: false,
                            right: 46.0,
                        },
                    );
                    ui::switch(cr, cx, hits, id + 1, r, right, cur.eq_ignore_ascii_case(on));
                }
                Control::Pick(all_opts) => {
                    let word = pick_label(all_opts, cur);
                    let vw = 40.0 + word.chars().count() as f64 * 7.0;
                    let lit = ui::row(
                        cr,
                        cx,
                        hits,
                        id,
                        r,
                        &ui::Row {
                            icon: None,
                            name: s.name,
                            state: s.says,
                            on: false,
                            right: vw.min(body.2 * 0.6),
                        },
                    );
                    let open = self.pick == Some(idx) && self.list.shown();
                    ui::value_box(cr, cx, r, right, word, lit, open);
                    if self.pick == Some(idx) && self.list.shown() {
                        let opts = self.pick_options(idx);
                        let lh = self.list.height(opts.len());
                        let lr = (body.0, y + ROW_H, body.2, lh);
                        ui::list_box(cr, cx, lr);
                        for (opt_i, &(val, label)) in opts.iter().enumerate() {
                            let rr = (
                                body.0 + 4.0,
                                lr.1 + 4.0 + opt_i as f64 * LIST_ROW_H,
                                body.2 - 8.0,
                                LIST_ROW_H,
                            );
                            if rr.1 + rr.3 > lr.1 + lr.3 + 0.5 {
                                break;
                            }
                            ui::list_row(
                                cr,
                                cx,
                                hits,
                                LIST + opt_i as u32,
                                rr,
                                label,
                                "",
                                val.eq_ignore_ascii_case(cur),
                                None,
                            );
                        }
                    }
                }
                _ => {}
            }
            y += self.row_h(idx);
        }

        if !buttons_drawn && !folders.is_empty() {
            draw_folder_buttons(cr, cx, hits, (body.0, y, body.2, BUTTONS_H), &folders);
            y += BUTTONS_H;
        }

        if has_more {
            ui::row(
                cr,
                cx,
                hits,
                MORE,
                (body.0, y, body.2, ROW_H),
                &ui::Row {
                    icon: None,
                    name: if self.more { "Less" } else { "More" },
                    state: "",
                    on: false,
                    right: 0.0,
                },
            );
        }
    }

    fn event(&mut self, ev: &Event, _cx: &Cx, out: &mut Vec<Action>) -> bool {
        match ev {
            Event::Text(q) => {
                if self.query == *q {
                    return false;
                }
                self.query = (*q).to_string();
                self.pick = None;
                self.list = ui::List::default();
                true
            }
            Event::Press {
                hit,
                button: Button::Left,
                ..
            } => {
                let hit = *hit;
                if hit == MORE {
                    self.more = !self.more;
                    if !self.more {
                        if let Some(pick_idx) = self.pick {
                            if TABLE[pick_idx].more {
                                self.pick = None;
                                self.list = ui::List::default();
                            }
                        }
                    }
                    return true;
                }

                if (LIST..MORE).contains(&hit) {
                    if !self.list.open {
                        return false;
                    }
                    let Some(pick_idx) = self.pick else {
                        return false;
                    };
                    let opts = self.pick_options(pick_idx);
                    let Some(&(val, _)) = opts.get((hit - LIST) as usize) else {
                        return false;
                    };
                    self.list.open = false;
                    self.commit(pick_idx, val.to_string(), out);
                    return true;
                }

                if (FOLDER..LIST).contains(&hit) {
                    let Some(&(_, key)) = FOLDERS.get((hit - FOLDER) as usize) else {
                        return false;
                    };
                    let (Some(cfg), Some(env)) = (self.config.as_ref(), self.env.as_ref()) else {
                        return false;
                    };
                    self.pick = None;
                    self.list = ui::List::default();
                    let raw = cfg.value(key);
                    if raw.is_empty() {
                        return false;
                    }
                    out.push(Action::OpenUri(expand_home(raw, &env.home)));
                    return true;
                }

                if hit < ROW {
                    return false;
                }
                let idx = ((hit - ROW) / 10) as usize;
                let Some(s) = TABLE.get(idx).copied() else {
                    return false;
                };
                match s.control {
                    Control::Switch { on, off } => {
                        let is_on = self.value(idx).eq_ignore_ascii_case(on);
                        let next = if is_on { off } else { on };
                        self.commit(idx, next.to_string(), out);
                        true
                    }
                    Control::Pick(_) => {
                        if self.pick == Some(idx) {
                            self.list.toggle();
                        } else {
                            self.pick = Some(idx);
                            self.list = ui::List::default();
                            self.list.toggle();
                        }
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn tick(&mut self, dt_ms: f64) -> bool {
        self.list.tick(dt_ms)
    }

    fn show_list(&mut self) {
        if let Some(i) = TABLE
            .iter()
            .position(|s| matches!(s.control, Control::Pick(_)))
        {
            self.pick = Some(i);
            self.list.open = true;
            self.list.settle();
        }
    }
}

fn draw_folder_buttons(cr: &cairo::Context, cx: &Cx, hits: &mut Hits, r: Rect, indices: &[usize]) {
    let mut x = r.0 + 8.0;
    let y = r.1 + BUTTON_TOP_PAD;
    for &i in indices {
        let w = ui::button(cr, cx, hits, FOLDER + i as u32, x, y, FOLDERS[i].0);
        x += w + BUTTON_GAP;
    }
}

fn pick_label<'a>(opts: &[(&'static str, &'static str)], value: &'a str) -> &'a str {
    opts.iter()
        .find(|&&(val, _)| val.eq_ignore_ascii_case(value))
        .map_or(value, |&(_, word)| word)
}

fn replay_enabled(cfg: &ConfigData) -> bool {
    matches!(
        cfg.value("ENABLE_REPLAY_BUFFER")
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "true" | "1" | "yes" | "on"
    )
}

fn matches_query(name: &str, key: &str, query: &str) -> bool {
    let hay = format!("{name} {key}").to_ascii_lowercase();
    query
        .split_whitespace()
        .all(|w| hay.contains(&w.to_ascii_lowercase()))
}

fn expand_home(raw: &str, home: &Path) -> String {
    let home = home.to_string_lossy();
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return raw.to_string();
    }
    for prefix in ["$HOME", "${HOME}", "~"] {
        if raw == prefix {
            return home.to_string();
        }
        if let Some(rest) = raw.strip_prefix(prefix).and_then(|s| s.strip_prefix('/')) {
            return format!("{home}/{rest}");
        }
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::settings::tests::test_cx;
    use crate::cards::settings_rows::parse_config;
    use crate::theme::Theme;
    use std::collections::BTreeSet;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn theme() -> Theme {
        Theme::from_config(&crate::config::Config::from_text(""))
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "infynitae-recording-test-{}-{tag}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cfg_with(entries: &[(&str, &str)], programs: &[&str]) -> ConfigData {
        let default = parse_config(include_str!("../../../../config/config"));
        let mut now = HashMap::new();
        for &(k, v) in entries {
            now.insert(k.to_string(), v.to_string());
        }
        ConfigData {
            now,
            default,
            programs: programs.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn card_with(
        entries: &[(&str, &str)],
        programs: &[&str],
        local_recorder: bool,
    ) -> RecordingCard {
        let mut card = RecordingCard::new();
        let cfg = cfg_with(entries, programs);
        let env = RecordingEnv {
            home: PathBuf::from("/home/tester"),
            local_recorder,
        };
        assert!(card.apply("config", &cfg));
        assert!(card.apply("recording", &env));
        card
    }

    #[test]
    fn every_key_is_in_config_config_with_a_value_its_control_has() {
        let def = parse_config(include_str!("../../../../config/config"));
        for s in TABLE {
            let v = def
                .get(s.key)
                .unwrap_or_else(|| panic!("{} not in config/config", s.key));
            match s.control {
                Control::Switch { on, off } => assert!(v == on || v == off, "{} = {v}", s.key),
                Control::Pick(o) | Control::Words(o) => {
                    assert!(o.iter().any(|(x, _)| x == v), "{} = {v}", s.key)
                }
                _ => panic!("unexpected control for {}", s.key),
            }
        }
        for &(_, key) in FOLDERS {
            let v = def
                .get(key)
                .unwrap_or_else(|| panic!("{key} not in config/config"));
            assert!(!v.is_empty(), "{key} is empty in config/config");
        }
        assert!(
            def.contains_key("ENABLE_REPLAY_BUFFER"),
            "ENABLE_REPLAY_BUFFER not in config/config"
        );
        let mut keys: BTreeSet<&str> = TABLE.iter().map(|s| s.key).collect();
        for &(_, k) in FOLDERS {
            assert!(keys.insert(k), "duplicate key {k}");
        }
    }

    #[test]
    fn recording_source_checks_local_bin_executable_empty_and_broken() {
        let dir_ok = tmp_dir("ok");
        let bin_ok = dir_ok.join(".local/bin/infynitae-recorder");
        fs::create_dir_all(bin_ok.parent().unwrap()).unwrap();
        fs::write(&bin_ok, "#!/bin/sh\n").unwrap();
        let mut perms = fs::metadata(&bin_ok).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin_ok, perms).unwrap();
        let mut src_ok = RecordingSource {
            home: dir_ok.clone(),
        };
        let env_ok = src_ok.fetch().downcast::<RecordingEnv>().unwrap();
        assert!(env_ok.local_recorder);
        assert_eq!(env_ok.home, dir_ok);

        let dir_empty = tmp_dir("empty");
        let mut src_empty = RecordingSource {
            home: dir_empty.clone(),
        };
        assert!(
            !src_empty
                .fetch()
                .downcast::<RecordingEnv>()
                .unwrap()
                .local_recorder
        );

        let dir_noexec = tmp_dir("noexec");
        let bin_noexec = dir_noexec.join(".local/bin/infynitae-recorder");
        fs::create_dir_all(bin_noexec.parent().unwrap()).unwrap();
        fs::write(&bin_noexec, "not executable").unwrap();
        let mut perms = fs::metadata(&bin_noexec).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&bin_noexec, perms).unwrap();
        let mut src_noexec = RecordingSource {
            home: dir_noexec.clone(),
        };
        assert!(
            !src_noexec
                .fetch()
                .downcast::<RecordingEnv>()
                .unwrap()
                .local_recorder
        );

        let dir_isdir = tmp_dir("isdir");
        fs::create_dir_all(dir_isdir.join(".local/bin/infynitae-recorder")).unwrap();
        let mut src_isdir = RecordingSource {
            home: dir_isdir.clone(),
        };
        assert!(
            !src_isdir
                .fetch()
                .downcast::<RecordingEnv>()
                .unwrap()
                .local_recorder
        );

        let mut src_blank = RecordingSource {
            home: PathBuf::new(),
        };
        assert!(
            !src_blank
                .fetch()
                .downcast::<RecordingEnv>()
                .unwrap()
                .local_recorder
        );

        let _ = fs::remove_dir_all(dir_ok);
        let _ = fs::remove_dir_all(dir_empty);
        let _ = fs::remove_dir_all(dir_noexec);
        let _ = fs::remove_dir_all(dir_isdir);
    }

    #[test]
    fn hides_whole_card_without_recorder_and_hides_gpu_screen_recorder_choice() {
        let th = theme();
        let cx_s = test_cx(&th);
        let mut cx_m = test_cx(&th);
        cx_m.size = Size::M;

        let mut loading = RecordingCard::new();
        assert!(matches!(loading.status(&cx_s), Status::Loading));
        assert!(loading.apply("config", &cfg_with(&[], &["infynitae-recorder"])));
        assert!(matches!(loading.status(&cx_s), Status::Loading));

        let hidden = card_with(&[], &[], false);
        assert!(matches!(hidden.status(&cx_s), Status::Hidden));
        assert_eq!(hidden.height(&cx_s, 424.0), 0.0);

        let local_only = card_with(&[], &[], true);
        assert!(matches!(local_only.status(&cx_s), Status::Ready));
        let backend_idx = TABLE
            .iter()
            .position(|s| s.key == "REPLAY_BACKEND")
            .unwrap();
        let vals_no_gsr: Vec<&str> = local_only
            .pick_options(backend_idx)
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        assert_eq!(vals_no_gsr, vec!["auto", "infynitae"]);

        let with_gsr = card_with(&[], &["infynitae-recorder", "gpu-screen-recorder"], false);
        let vals_gsr: Vec<&str> = with_gsr
            .pick_options(backend_idx)
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        assert_eq!(vals_gsr, vec!["auto", "gpu-screen-recorder", "infynitae"]);

        assert_eq!(with_gsr.visible_rows(&cx_s), (vec![0, 1, 2, 3], true));
        assert_eq!(
            with_gsr.visible_rows(&cx_m),
            (vec![0, 1, 2, 3, 4, 5], false)
        );
    }

    #[test]
    fn exact_commands_for_switch_pick_and_folder_buttons() {
        let th = theme();
        let mut cx = test_cx(&th);
        cx.size = Size::M;
        let mut out = Vec::new();

        // 1. Switch (`FREEZE_SCREENSHOT`): logs existing line, sets false with After::Use.
        let mut card = card_with(
            &[
                ("FREEZE_SCREENSHOT", "true"),
                ("RECORD_AUDIO", "desktop"),
                ("ENABLE_REPLAY_BUFFER", "true"),
            ],
            &["infynitae-recorder"],
            false,
        );
        let freeze_idx = TABLE
            .iter()
            .position(|s| s.key == "FREEZE_SCREENSHOT")
            .unwrap();
        assert!(card.event(
            &Event::Press {
                hit: ROW + 10 * freeze_idx as u32 + 1,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(
            out,
            vec![Action::Run(change_command(
                "FREEZE_SCREENSHOT",
                Some("true"),
                "false",
                After::Use,
            ))]
        );

        // 2. Pick (`RECORD_AUDIO`) while ENABLE_REPLAY_BUFFER=true -> After::Reload("recorder").
        out.clear();
        let audio_idx = TABLE.iter().position(|s| s.key == "RECORD_AUDIO").unwrap();
        assert!(card.event(
            &Event::Press {
                hit: ROW + 10 * audio_idx as u32,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        ));
        assert!(out.is_empty());
        assert!(card.event(
            &Event::Press {
                hit: LIST + 2, // ("both", "Both")
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(
            out,
            vec![Action::Run(change_command(
                "RECORD_AUDIO",
                Some("desktop"),
                "both",
                After::Reload("recorder"),
            ))]
        );

        // 3. Pick while ENABLE_REPLAY_BUFFER=false (and key not in user file) -> old=None, After::Use.
        let mut off_card = card_with(
            &[("ENABLE_REPLAY_BUFFER", "false")],
            &["infynitae-recorder"],
            false,
        );
        out.clear();
        let fps_idx = TABLE.iter().position(|s| s.key == "RECORD_FPS").unwrap();
        assert!(off_card.event(
            &Event::Press {
                hit: ROW + 10 * fps_idx as u32,
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        ));
        assert!(off_card.event(
            &Event::Press {
                hit: LIST + 2, // ("120", "120")
                button: Button::Left,
                at: (0.0, 0.0),
            },
            &cx,
            &mut out,
        ));
        assert_eq!(
            out,
            vec![Action::Run(change_command(
                "RECORD_FPS",
                None,
                "120",
                After::Use,
            ))]
        );

        // 4. Folder buttons expand `$HOME` and `~` and pass literal paths unchanged.
        let mut dir_card = card_with(
            &[
                ("SCREENSHOT_DIR", "$HOME/Pictures/Shots"),
                ("CLIPS_DIR", "~/Videos/My Clips"),
                ("RECORDINGS_DIR", "/mnt/media/Recordings"),
            ],
            &["infynitae-recorder"],
            false,
        );
        for (i, expected) in [
            "/home/tester/Pictures/Shots",
            "/home/tester/Videos/My Clips",
            "/mnt/media/Recordings",
        ]
        .iter()
        .enumerate()
        {
            out.clear();
            assert!(dir_card.event(
                &Event::Press {
                    hit: FOLDER + i as u32,
                    button: Button::Left,
                    at: (0.0, 0.0),
                },
                &cx,
                &mut out,
            ));
            assert_eq!(out, vec![Action::OpenUri((*expected).to_string())]);
        }
    }

    #[test]
    fn search_filters_rows_and_folder_buttons() {
        let th = theme();
        let cx = test_cx(&th);
        let mut card = card_with(&[], &["infynitae-recorder"], false);
        let mut out = Vec::new();

        assert!(card.event(&Event::Text("clips"), &cx, &mut out));
        assert!(matches!(card.status(&cx), Status::Ready));
        assert!(card.visible_rows(&cx).0.is_empty());
        assert_eq!(card.visible_folders(), vec![1]);

        assert!(card.event(&Event::Text("format"), &cx, &mut out));
        assert!(matches!(card.status(&cx), Status::Ready));
        assert_eq!(card.visible_rows(&cx), (vec![4], false));
        assert!(card.visible_folders().is_empty());

        assert!(card.event(&Event::Text("zzznomatch"), &cx, &mut out));
        assert!(matches!(card.status(&cx), Status::Hidden));
    }

    #[test]
    fn drawn_hits_fit_inside_reported_height() {
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 424, 900).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();

        let th = theme();
        let cx_s = test_cx(&th);
        let mut cx_m = test_cx(&th);
        cx_m.size = Size::M;
        let mut card = card_with(&[], &["infynitae-recorder", "gpu-screen-recorder"], false);

        for (label, setup, cx) in [
            ("s_closed", 0, &cx_s),
            ("s_pick", 1, &cx_s),
            ("s_more", 2, &cx_s),
            ("m", 3, &cx_m),
        ] {
            card.pick = None;
            card.list = ui::List::default();
            card.more = false;
            let mut out = Vec::new();
            match setup {
                1 => card.show_list(),
                2 => {
                    card.event(
                        &Event::Press {
                            hit: MORE,
                            button: Button::Left,
                            at: (0.0, 0.0),
                        },
                        cx,
                        &mut out,
                    );
                }
                _ => {}
            }
            let h = card.height(cx, 424.0);
            let mut hits = Hits::default();
            card.draw(&cr, cx, (8.0, 10.0, 408.0, h), &mut hits);
            let max_y = hits
                .list
                .iter()
                .map(|(_, hr)| hr.1 + hr.3)
                .fold(10.0_f64, f64::max);
            assert!(
                max_y <= 10.0 + h + 0.5,
                "{label}: hit bottom {max_y} exceeded height {h}"
            );
            assert!(!hits.list.is_empty(), "{label}: expected non-empty hits");
        }
    }
}
