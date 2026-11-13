//! INFYNITAE_EDGE_BOXES (docs/EDGE_APPS.md section 9): after each drawing, a file the VM scripts read
//! to find what to click: `panel NAME x y w h locked SIDE`, `module WORD x y w h` for a card read from
//! today's module word (so today's scripts pass unchanged), `card OWNER/KIND x y w h` for every card,
//! and `hit OWNER/KIND ID x y w h` for each of its hits; `closed` while closed.

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
    let Some(idx) = shell.open else {
        return "closed\n".into();
    };
    let ps = &shell.panels[idx];
    let ((ox, oy, pw, ph), rects) = shell.layout(idx, w, h);
    let mut out = format!(
        "panel {} {ox:.0} {oy:.0} {pw:.0} {ph:.0} locked {}\n",
        ps.panel.name,
        ps.panel.side.word()
    );
    for (ci, r) in &rects {
        let pc = &ps.cards[*ci];
        if let Some(old) = &pc.word.old {
            out += &format!("module {old} {:.0} {:.0} {:.0} {:.0}\n", r.0, r.1, r.2, r.3);
        }
        out += &format!(
            "card {}/{} {:.0} {:.0} {:.0} {:.0}\n",
            pc.word.owner, pc.word.kind, r.0, r.1, r.2, r.3
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
    out
}
