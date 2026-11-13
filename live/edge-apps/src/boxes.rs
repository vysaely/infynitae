//! INFYNITAE_EDGE_BOXES (docs/EDGE_APPS.md section 9): after each drawing, a file the VM scripts read
//! to find what to click: `panel NAME x y w h locked SIDE`, `module WORD x y w h` for a card read from
//! today's module word (so today's scripts pass unchanged), `card OWNER/KIND x y w h` for every card,
//! and `hit OWNER/KIND ID x y w h` for each of its hits; `ask x y w h` while the question shows;
//! `closed` while closed.

use crate::shell::Shell;

pub fn dump(shell: &Shell, w: f64, h: f64) {
    let Some(path) = std::env::var_os("INFYNITAE_EDGE_BOXES").filter(|p| !p.is_empty()) else {
        return;
    };
    let _ = std::fs::write(path, text(shell, w, h));
}

pub fn dump_closed() {
    if let Some(path) = std::env::var_os("INFYNITAE_EDGE_BOXES").filter(|p| !p.is_empty()) {
        let _ = std::fs::write(path, "closed\n");
    }
}

pub fn text(shell: &Shell, w: f64, h: f64) -> String {
    if shell.ask.ask.on {
        let (x, y, aw, ah) = shell.ask.ask.geometry(w, h);
        return format!("ask {x:.0} {y:.0} {aw:.0} {ah:.0}\n");
    }
    let Some(idx) = shell.open else {
        return "closed\n".into();
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
    for (ci, _, hits) in shell.frame.borrow().iter() {
        let pc = &ps.cards[*ci];
        for (id, r) in &hits.list {
            out += &format!(
                "hit {}/{} {id} {:.0} {:.0} {:.0} {:.0}\n",
                pc.word.owner, pc.word.kind, r.0, r.1, r.2, r.3
            );
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
    out
}
