//! INFYNITAE_EDGE_BOXES (docs/EDGE_APPS.md section 9): after each drawing, a file the VM scripts read
//! to find what to click: `panel NAME x y w h locked SIDE`, `query TEXT` when typed, `module WORD x y w h`
//! for a card read from today's module word (so today's scripts pass unchanged), `card OWNER/KIND x y w h`
//! for every card, `hit OWNER/KIND ID x y w h` for each of its hits, and `label OWNER/KIND ID TEXT` /
//! `note OWNER/KIND TEXT` where cards annotate them; `ask x y w h` while the question shows; `osd x y w h
//! VALUE LABEL` while the OSD shows; `closed` while closed.

use crate::shell::Shell;

fn clean_text(s: &str) -> String {
    s.replace(['\r', '\n', '\t'], " ")
}

fn boxes_path() -> Option<std::ffi::OsString> {
    std::env::var_os("INFYNITAE_EDGE_BOXES").filter(|p| !p.is_empty())
}

fn write_boxes_to(path: &std::path::Path, body: &str) -> bool {
    match std::fs::write(path, body) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("infynitae-edge-apps: boxes: write {}: {e}", path.display());
            false
        }
    }
}

pub fn dump(shell: &Shell, w: f64, h: f64) {
    if let Some(path) = boxes_path() {
        write_boxes_to(std::path::Path::new(&path), &text(shell, w, h));
    }
}

pub fn dump_on(shell: &Shell, output: crate::shell::OutputId, w: f64, h: f64) {
    if let Some(path) = boxes_path() {
        write_boxes_to(
            std::path::Path::new(&path),
            &text_on(shell, Some(output), w, h),
        );
    }
}

pub fn dump_closed() {
    if let Some(path) = boxes_path() {
        write_boxes_to(std::path::Path::new(&path), "closed\n");
    }
}

pub fn text(shell: &Shell, w: f64, h: f64) -> String {
    text_on(shell, shell.visible_output(), w, h)
}

fn osd_line(shell: &Shell, w: f64, h: f64) -> Option<String> {
    let osd = shell.notify.osd.as_ref()?;
    let (x, y, ow, oh) = crate::notify::osd_rect(w, h);
    let val = if osd.muted { 0 } else { osd.value };
    Some(format!(
        "osd {x:.0} {y:.0} {ow:.0} {oh:.0} {val} {}\n",
        osd.label
    ))
}

fn text_on(shell: &Shell, output: Option<crate::shell::OutputId>, w: f64, h: f64) -> String {
    let osd = osd_line(shell, w, h);
    if shell.ask.ask.on {
        let (x, y, aw, ah) = shell.ask.ask.geometry(w, h);
        let mut out = format!("ask {x:.0} {y:.0} {aw:.0} {ah:.0}\n");
        if let Some(line) = osd {
            out += &line;
        }
        return out;
    }
    if let Some((x, y, mw, mh)) = shell.window_menu_rect(w, h) {
        let mut out = format!("windowmenu {x:.0} {y:.0} {mw:.0} {mh:.0}\n");
        for (_, _, hits) in shell.frame.borrow().iter() {
            for (id, r) in &hits.list {
                out += &format!(
                    "hit window/menu {id} {:.0} {:.0} {:.0} {:.0}\n",
                    r.0, r.1, r.2, r.3
                );
            }
        }
        if let Some(line) = osd {
            out += &line;
        }
        return out;
    }
    if shell.shortcuts.open {
        let r = crate::shortcuts::rect(w, h, &shell.cfg.keys, &shell.shortcuts.query);
        let mut out = format!("shortcuts {:.0} {:.0} {:.0} {:.0}\n", r.0, r.1, r.2, r.3);
        for (_, _, hits) in shell.frame_hits(output) {
            for (id, hr) in hits.list {
                let tag = if id == crate::shortcuts::HIT_CHANGE {
                    "change"
                } else if id == crate::shortcuts::HIT_QUERY {
                    "query"
                } else {
                    continue;
                };
                out += &format!(
                    "hit shortcuts/{tag} {id} {:.0} {:.0} {:.0} {:.0}\n",
                    hr.0, hr.1, hr.2, hr.3
                );
            }
        }
        return out;
    }
    let Some(idx) = shell.open else {
        return osd.unwrap_or_else(|| "closed\n".into());
    };
    let ps = &shell.panels[idx];
    let editing = shell.editing();
    let (pbox, local_rects, content_h) = shell.layout_local(idx, w, h);
    let (ox, oy, pw, ph) = pbox;
    let scroll = shell.panel_scroll.clamp(0.0, (content_h - ph).max(0.0));
    let mut out = format!(
        "panel {} {ox:.0} {oy:.0} {pw:.0} {ph:.0} {} {}\n",
        ps.panel.name,
        if editing { "editing" } else { "locked" },
        ps.panel.side.word()
    );
    if !shell.query.is_empty() {
        out += &format!("query {}\n", clean_text(&shell.query));
    }
    for &(ci, r) in &local_rects {
        let pc = &ps.cards[ci];
        let sx = ox + r.0;
        let sy = oy + r.1 - scroll;
        if let Some(old) = &pc.word.old {
            out += &format!("module {old} {sx:.0} {sy:.0} {:.0} {:.0}\n", r.2, r.3);
        }
        out += &format!(
            "card {}/{} {sx:.0} {sy:.0} {:.0} {:.0}\n",
            pc.word.owner, pc.word.kind, r.2, r.3
        );
    }
    for (ci, _, hits) in shell.frame_hits(output) {
        let pc = &ps.cards[ci];
        for (id, r) in hits.list {
            out += &format!(
                "hit {}/{} {id} {:.0} {:.0} {:.0} {:.0}\n",
                pc.word.owner, pc.word.kind, r.0, r.1, r.2, r.3
            );
        }
        for (id, text) in hits.texts {
            let text = clean_text(&text);
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            match id {
                Some(id) => {
                    out += &format!("label {}/{} {id} {text}\n", pc.word.owner, pc.word.kind);
                }
                None => {
                    out += &format!("note {}/{} {text}\n", pc.word.owner, pc.word.kind);
                }
            }
        }
    }
    if let Some(r) = shell.edit_menu_rect(w, h) {
        out += &format!("editmenu {:.0} {:.0} {:.0} {:.0}\n", r.0, r.1, r.2, r.3);
    }
    if editing {
        let (bar, add) = shell.edit_rects(idx, w, h);
        out += &format!("bar {:.0} {:.0} {:.0} {:.0}\n", bar.0, bar.1, bar.2, bar.3);
        for (k, r) in crate::edit::bar_buttons(bar) {
            out += &format!(
                "barbutton {k:?} {:.0} {:.0} {:.0} {:.0}\n",
                r.0, r.1, r.2, r.3
            );
        }
        let cards = shell.edit_cards(idx);
        for &(ci, r) in &local_rects {
            let Some(pc) = ps.cards.get(ci) else {
                continue;
            };
            for (k, br) in crate::edit::card_buttons(&cards, &shell.apps, ci, r, pw).1 {
                out += &format!(
                    "cardbutton {}/{} {k:?} {:.0} {:.0} {:.0} {:.0}\n",
                    pc.word.owner,
                    pc.word.kind,
                    ox + br.0,
                    oy + br.1 - scroll,
                    br.2,
                    br.3
                );
            }
        }
        if shell.edit.adding {
            out += &format!("add {:.0} {:.0} {:.0} {:.0}\n", add.0, add.1, add.2, add.3);
            let entries = crate::edit::add_entries(&cards, &shell.apps);
            let view = crate::edit::add_viewport(add);
            let (items, _, _) = crate::edit::build_add_items(
                add,
                &entries,
                shell.edit.expanded_add.as_deref(),
                shell.edit.add_scroll,
            );
            for item in items {
                match item {
                    crate::edit::AddItem::Entry { owner, rect, .. } => {
                        let cy = rect.1 + rect.3 * 0.5;
                        if cy >= view.1 && cy <= view.1 + view.3 {
                            out += &format!(
                                "addrow {owner} {:.0} {:.0} {:.0} {:.0}\n",
                                rect.0, rect.1, rect.2, rect.3
                            );
                        }
                    }
                    crate::edit::AddItem::Kind {
                        owner,
                        kind,
                        button,
                        ..
                    } => {
                        let cy = button.1 + button.3 * 0.5;
                        if cy >= view.1 && cy <= view.1 + view.3 {
                            out += &format!(
                                "addrow {owner}/{} {:.0} {:.0} {:.0} {:.0}\n",
                                kind.id, button.0, button.1, button.2, button.3
                            );
                        }
                    }
                    crate::edit::AddItem::Group { .. } => {}
                }
            }
        }
    }
    if let Some(line) = osd {
        out += &line;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_boxes_to_writes_file_and_reports_missing_parent() {
        let dir =
            std::env::temp_dir().join(format!("infynitae-edge-apps-boxes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let good = dir.join("boxes.txt");
        assert!(write_boxes_to(&good, "closed\n"));
        assert_eq!(std::fs::read_to_string(&good).unwrap(), "closed\n");

        let bad = dir.join("missing").join("boxes.txt");
        assert!(!write_boxes_to(&bad, "closed\n"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_reports_osd_when_closed_and_when_panel_is_open() {
        let mut sh = Shell::new(
            crate::config::Config::from_text(
                "PANEL_CONTROL=\"SUPER+A top right 380 settings/session\"\n",
            ),
            Vec::new(),
        );
        assert_eq!(text(&sh, 1920.0, 1080.0), "closed\n");
        let note = crate::notify::Note {
            id: 1,
            app: "Audio".into(),
            summary: "Volume: 65%".into(),
            body: String::new(),
            actions: vec![],
            urgency: 1,
            value: Some(65),
            sync: Some("volume".into()),
            expire: Some(Some(std::time::Duration::from_millis(1200))),
            transient: false,
            resident: false,
            time: "12:00".into(),
            desktop_entry: String::new(),
            program: String::new(),
            private: false,
        };
        sh.notify
            .receive(note, std::time::Instant::now(), crate::notify::Quiet::No);
        let closed_osd = text(&sh, 1920.0, 1080.0);
        assert_eq!(closed_osd, "osd 840 1000 240 56 65 Volume\n");

        assert!(sh.toggle_named_on(None, "control"));
        let open_osd = text(&sh, 1920.0, 1080.0);
        assert!(open_osd.starts_with("panel control "), "{open_osd}");
        assert!(
            open_osd.ends_with("osd 840 1000 240 56 65 Volume\n"),
            "{open_osd}"
        );
    }

    fn hit_center(dump: &str, prefix: &str) -> (f64, f64) {
        let line = dump
            .lines()
            .find(|l| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing line starting with {prefix:?} in:\n{dump}"));
        let nums: Vec<f64> = line
            .split_whitespace()
            .rev()
            .take(4)
            .filter_map(|s| s.parse().ok())
            .collect();
        assert_eq!(nums.len(), 4, "bad box coordinates on line: {line}");
        (nums[3] + nums[1] * 0.5, nums[2] + nums[0] * 0.5)
    }

    fn draw_dump(sh: &mut Shell) -> String {
        sh.open_ms = f64::INFINITY;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1920, 1080).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        sh.draw(&cr, 1920.0, 1080.0);
        text(sh, 1920.0, 1080.0)
    }

    #[test]
    fn text_dumps_query_labels_and_plocate_note() {
        let apps = vec![crate::app::App {
            id: "org.alacritty.Alacritty".into(),
            name: "Alacritty".into(),
            icon: String::new(),
            exec: "alacritty".into(),
            categories: vec![],
            wm_class: None,
            terminal: false,
        }];
        let mut sh = Shell::new(crate::config::Config::from_text(""), apps);
        sh.windows = vec![crate::card::Window {
            id: 7,
            title: "alpha\twin".into(),
            app_id: "org.example.Alpha".into(),
            focused: false,
        }];

        assert!(sh.toggle_named_on(None, "apps"));
        sh.type_text("alacr");
        let app_dump = draw_dump(&mut sh);
        assert!(app_dump.contains("query alacr\n"), "{app_dump}");
        assert!(
            app_dump.contains("label apps/list 0 org.alacritty.Alacritty Alacritty\n"),
            "{app_dump}"
        );

        assert!(sh.toggle_named_on(None, "apps"));
        assert!(sh.toggle_named_on(None, "apps"));
        sh.type_text("alpha");
        sh.apply(
            "apps",
            "located",
            &crate::locate::Located {
                query: "alpha".into(),
                paths: vec!["/tmp/alpha-doc.txt".into()],
            } as &dyn std::any::Any,
        );
        let res_dump = draw_dump(&mut sh);
        assert!(res_dump.contains("query alpha\n"), "{res_dump}");
        assert!(
            res_dump
                .lines()
                .any(|l| l.starts_with("label apps/results ") && l.ends_with(" windows alpha win")),
            "{res_dump}"
        );
        assert!(
            res_dump.lines().any(
                |l| l.starts_with("label apps/results ") && l.ends_with(" files alpha-doc.txt")
            ),
            "{res_dump}"
        );

        let mut cfg = crate::cards::settings_rows::ConfigData {
            now: std::collections::HashMap::new(),
            default: std::collections::HashMap::new(),
            programs: std::collections::HashSet::new(),
        };
        let screens = crate::cards::settings_display::ScreensData::default();
        assert!(sh.toggle_named_on(None, "settings"));
        sh.apply("settings", "config", &cfg as &dyn std::any::Any);
        sh.apply("settings", "screens", &screens as &dyn std::any::Any);
        sh.type_text("night light");
        let disp_dump = draw_dump(&mut sh);
        assert!(disp_dump.contains("query night light\n"), "{disp_dump}");
        assert!(
            disp_dump.contains("label settings/all 400100 Night light warmth\n"),
            "{disp_dump}"
        );

        assert!(sh.toggle_named_on(None, "settings"));
        assert!(sh.toggle_named_on(None, "settings"));
        sh.apply("settings", "config", &cfg as &dyn std::any::Any);
        sh.apply("settings", "screens", &screens as &dyn std::any::Any);
        let groups_dump = draw_dump(&mut sh);
        assert!(
            groups_dump.contains("label settings/all 6 Apps\n"),
            "{groups_dump}"
        );
        let (ax, ay) = hit_center(&groups_dump, "hit settings/all 6 ");
        assert!(sh.click(1920.0, 1080.0, ax, ay, crate::card::Button::Left));
        let missing_dump = draw_dump(&mut sh);
        assert!(
            missing_dump.contains(
                "note settings/all Searching files by name needs plocate, which is not installed\n"
            ),
            "{missing_dump}"
        );

        cfg.programs.insert("plocate".into());
        sh.apply("settings", "config", &cfg as &dyn std::any::Any);
        let present_dump = draw_dump(&mut sh);
        assert!(
            !present_dump.contains("Searching files by name needs plocate"),
            "{present_dump}"
        );
    }
}
