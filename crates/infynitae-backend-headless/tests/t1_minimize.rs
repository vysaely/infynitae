//! An app's minimize button (2026-11-07): the focused window is hidden, still open, until it is
//! picked again; a window that is not focused is never minimized (Wine asks for it when a game
//! loses the focus, the black screens of 2026-11-02).

mod common;

use common::input::*;
use infynitae_core::ShellCommand;
use infynitae_core::stage::ElementId;

fn open(r: &mut Rig, label: &'static str) -> (usize, ElementId) {
    let qh = r.ls.qh.clone();
    let i = r.ls.probe.toplevel(&qh, label, (300, 200), 0);
    r.ls.pump();
    r.frame();
    (
        i,
        *r.ls.state.core().stage().mapped_elements().last().unwrap(),
    )
}

fn minimize(r: &mut Rig, i: usize) {
    r.ls.probe.surfaces[i]
        .toplevel
        .as_ref()
        .unwrap()
        .set_minimized();
    for _ in 0..3 {
        r.ls.pump();
        r.frame();
    }
}

fn focus(r: &mut Rig, id: ElementId) {
    let core = r.ls.state.core();
    let element = infynitae_core::stage::ElementRef {
        instance: core.stage().instance(),
        element: id,
    };
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(&mut r.ls.shell, ShellCommand::Focus { element })
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();
}

#[test]
fn the_focused_window_minimizes_and_comes_back_when_picked() {
    let mut r = Rig::x8();
    let (i, b) = open(&mut r, "B");
    focus(&mut r, b);
    assert_eq!(r.ls.state.core().focus().target(), Some(b));
    minimize(&mut r, i);
    let core = r.ls.state.core();
    assert!(core.stage().is_minimized(b), "not minimized");
    assert_ne!(
        core.focus().target(),
        Some(b),
        "a minimized window kept the focus"
    );
    assert_ne!(
        core.stage().top_mapped(),
        Some(b),
        "a minimized window is still shown on top"
    );
    assert!(
        core.stage().front_to_back().all(|(e, _, _)| e != b),
        "a minimized window is still drawn"
    );
    assert!(
        core.stage().mapped_elements().contains(&b),
        "a minimized window left the windows list"
    );
    focus(&mut r, b);
    let core = r.ls.state.core();
    assert!(
        !core.stage().is_minimized(b),
        "picking it did not bring it back"
    );
    assert_eq!(core.focus().target(), Some(b));
    assert_eq!(core.stage().top_mapped(), Some(b));
}

#[test]
fn a_window_without_the_focus_is_not_minimized() {
    let mut r = Rig::x8();
    let (i, b) = open(&mut r, "B");
    let (_, c) = open(&mut r, "C");
    focus(&mut r, c);
    assert_ne!(r.ls.state.core().focus().target(), Some(b));
    minimize(&mut r, i);
    assert!(
        !r.ls.state.core().stage().is_minimized(b),
        "a window that lost the focus was minimized"
    );
}
