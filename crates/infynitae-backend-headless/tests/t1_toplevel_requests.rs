//! Production path tests for `xdg_toplevel` requests and compositor capabilities (M2).
//!
//! TST-22 (xmaximize) ported: `set_maximized`, `unset_maximized`, `set_fullscreen` and
//! `unset_fullscreen` are each answered by exactly one configure; `set_minimized`, `move`,
//! `resize` and `show_window_menu` are answered by none; a request before the initial configure,
//! or while unmapped, is not answered. No request moves a window, a camera or the focus. The
//! oracle is the client's own record of the events it received (`probe.toplevel_configures`,
//! `probe.wm_capabilities`).
//!
//! What a granted fullscreen looks like on the output is in `t1_fullscreen`.

mod common;

use std::time::Duration;

use common::{Lockstep, output};
use infynitae_backend_headless::{pointer_button, pointer_motion, set_focus};
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::stage::{ElementId, Lifecycle};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{ResizeEdge, XdgToplevel};

const MAXIMIZED: u32 = 1;
const FULLSCREEN: u32 = 2;
const ACTIVATED: u32 = 4;
const BTN_LEFT: u32 = 0x110;

struct T {
    ls: Lockstep,
    t: u64,
}

impl T {
    fn new() -> T {
        T {
            ls: Lockstep::new(Some(output((1920, 1080), 120))),
            t: 10_000,
        }
    }

    fn adv(&mut self) {
        self.ls.pump();
        let t = self.ls.now + Duration::from_micros(16_667);
        self.ls.run_until(t);
    }

    /// A mapped 400x300 toplevel.
    fn map(&mut self, label: &'static str) -> (usize, ElementId) {
        let qh = self.ls.qh.clone();
        let w = self.ls.probe.toplevel(&qh, label, (400, 300), 0);
        self.adv();
        let id = self.ls.state.elements().last().expect("element");
        assert_eq!(
            self.ls.state.core().stage().state(id),
            Some(Lifecycle::Mapped)
        );
        (w, id)
    }

    fn toplevel(&self, idx: usize) -> XdgToplevel {
        self.ls.probe.surfaces[idx]
            .toplevel
            .clone()
            .expect("xdg_toplevel")
    }

    /// The toplevel configures `idx` has received so far.
    fn configures(&self, idx: usize) -> Vec<((i32, i32), Vec<u32>)> {
        self.ls
            .probe
            .toplevel_configures
            .iter()
            .filter(|(i, _, _)| *i == idx)
            .map(|(_, size, states)| {
                // the states array is a set: its order on the wire carries no meaning
                let mut states = states.clone();
                states.sort_unstable();
                (*size, states)
            })
            .collect()
    }

    /// Sends one request and returns the configures it caused.
    fn ask(
        &mut self,
        idx: usize,
        request: impl FnOnce(&XdgToplevel),
    ) -> Vec<((i32, i32), Vec<u32>)> {
        let before = self.configures(idx).len();
        request(&self.toplevel(idx));
        self.adv();
        self.configures(idx)[before..].to_vec()
    }

    /// A click on the window under the pointer; returns the press serial.
    fn click(&mut self) -> u32 {
        for pressed in [true, false] {
            self.t += 1000;
            pointer_button(
                &mut self.ls.state,
                &mut self.ls.input,
                BTN_LEFT,
                pressed,
                Modifiers::default(),
                self.t,
            )
            .expect("button");
            self.adv();
        }
        self.ls.probe.last_serial
    }

    /// Everything a client's request must not change, as Core holds it.
    fn authority(&self) -> String {
        let core = self.ls.state.core();
        let anchors: Vec<_> = core
            .stage()
            .z_order()
            .iter()
            .map(|id| (*id, core.stage().anchor(*id), core.stage().state(*id)))
            .collect();
        format!(
            "{:?}",
            (
                anchors,
                core.cameras().center(0),
                core.cameras().zoom(0),
                core.focus().target(),
                core.pointer()
            )
        )
    }
}

/// TST-22: each of the four state requests is answered by exactly one configure that grants
/// nothing and proposes no size; the four others are answered by none. The window is focused, so
/// the configures carry `activated` and nothing else. Stage, camera, focus and pointer are as
/// before, and the client saw no pointer event.
#[test]
fn tst22_each_request_gets_its_policy_answer_and_nothing_moves() {
    let mut t = T::new();
    let (w, id) = t.map("W");
    set_focus(&mut t.ls.state, &mut t.ls.input, Some(id)).expect("focus");
    t.adv();
    // the pointer is at the view centre, over the centred window: a click gives a serial
    t.t += 1000;
    pointer_motion(
        &mut t.ls.state,
        &mut t.ls.input,
        (1.0, 1.0),
        (1.0, 1.0),
        t.t,
    )
    .expect("motion");
    t.adv();
    let serial = t.click();
    let seat = t.ls.probe.seat.clone().expect("seat");
    let before = t.authority();
    let ptr = t.ls.probe.ptr.len();

    let declined = vec![((0, 0), vec![ACTIVATED])];
    assert_eq!(t.ask(w, |tl| tl.set_maximized()), declined, "set_maximized");
    assert_eq!(
        t.ask(w, |tl| tl.unset_maximized()),
        declined,
        "unset_maximized"
    );
    // not fullscreen: an unset is answered once and changes nothing
    assert_eq!(
        t.ask(w, |tl| tl.unset_fullscreen()),
        declined,
        "unset_fullscreen"
    );

    assert_eq!(t.ask(w, |tl| tl.set_minimized()), vec![], "set_minimized");
    assert_eq!(t.ask(w, |tl| tl._move(&seat, serial)), vec![], "move");
    assert_eq!(
        t.ask(w, |tl| tl.resize(&seat, serial, ResizeEdge::BottomRight)),
        vec![],
        "resize"
    );
    assert_eq!(
        t.ask(w, |tl| tl.show_window_menu(&seat, serial, 10, 10)),
        vec![],
        "show_window_menu"
    );

    assert_eq!(
        t.authority(),
        before,
        "a request changed authoritative state"
    );
    assert_eq!(
        t.ls.probe.ptr.len(),
        ptr,
        "a request produced a pointer event"
    );
    assert_eq!(t.ls.state.core().stage().state(id), Some(Lifecycle::Mapped));
}

/// The same answers for a window that does not hold the focus: declined, without `activated`.
/// The focused window gets nothing.
#[test]
fn an_unfocused_window_is_answered_the_same_way() {
    let mut t = T::new();
    let (w1, _) = t.map("W1");
    let (w2, id2) = t.map("W2");
    set_focus(&mut t.ls.state, &mut t.ls.input, Some(id2)).expect("focus");
    t.adv();
    let before = t.authority();
    let other = t.configures(w2).len();
    assert_eq!(
        t.ask(w1, |tl| tl.set_fullscreen(None)),
        vec![((0, 0), vec![])]
    );
    assert_eq!(t.ask(w1, |tl| tl.set_maximized()), vec![((0, 0), vec![])]);
    assert_eq!(t.ask(w1, |tl| tl.set_minimized()), vec![]);
    assert_eq!(
        t.configures(w2).len(),
        other,
        "the focused window was configured"
    );
    assert_eq!(t.authority(), before);
}

/// INV-PROTO-1 "a pre-configure request is recorded and ignored" (W-SM-13): requests made before
/// the initial commit cause no configure of their own, and the initial configure that follows is
/// the only one and grants neither state. Smithay's default handlers answer such a request at
/// once.
#[test]
fn requests_before_the_initial_configure_are_not_answered() {
    let mut t = T::new();
    let qh = t.ls.qh.clone();
    let x = t.ls.probe.toplevel_uncommitted(&qh, "X", (300, 200));
    let tl = t.toplevel(x);
    tl.set_maximized();
    tl.set_fullscreen(None);
    tl.set_minimized();
    tl.unset_fullscreen();
    t.adv();
    assert_eq!(
        t.configures(x),
        vec![],
        "answered before the initial commit"
    );
    t.ls.probe.surfaces[x].wl.commit();
    t.adv();
    let got = t.configures(x);
    assert_eq!(got.len(), 1, "exactly the initial configure: {got:?}");
    assert!(
        !got[0].1.contains(&MAXIMIZED) && !got[0].1.contains(&FULLSCREEN),
        "the initial configure granted a state: {got:?}"
    );
    assert_eq!(got[0].0, (0, 0));
}

/// An unmapped toplevel has no configure sequence until its next initial commit: a request made
/// in between is not answered, and the re-initial commit gets exactly one configure.
#[test]
fn requests_while_unmapped_are_not_answered() {
    let mut t = T::new();
    let (w, id) = t.map("W");
    t.ls.probe.unmap(w);
    t.adv();
    assert_eq!(
        t.ls.state.core().stage().state(id),
        Some(Lifecycle::Unmapped)
    );
    assert_eq!(t.ask(w, |tl| tl.set_maximized()), vec![]);
    assert_eq!(t.ask(w, |tl| tl.set_fullscreen(None)), vec![]);
    let before = t.configures(w).len();
    t.ls.probe.surfaces[w].wl.commit();
    t.adv();
    assert_eq!(
        t.configures(w).len() - before,
        1,
        "one configure for the re-initial commit"
    );
}

/// What the compositor says it can do is what its policy grants. The client is told once, with
/// its first configure, and the set is fullscreen alone: no maximize, minimize or window menu.
/// (A client that reads the set shows no control for those.)
///
/// Also recorded: the first configure carries the room the window has, `configure_bounds` of the
/// screen's usable area less the 12 px tiling gap on each side (INV-PROTO-1 as amended 2026-10-27),
/// and nothing later repeats it while that room is unchanged.
#[test]
fn the_announced_capabilities_are_what_the_policy_grants() {
    let mut t = T::new();
    let (w, id) = t.map("W");
    let caps = t.ls.state.core().toplevel_capabilities();
    let want: Vec<u32> = [
        (caps.window_menu, 1u32),
        (caps.maximize, 2),
        (caps.fullscreen, 3),
        (caps.minimize, 4),
    ]
    .into_iter()
    .filter_map(|(on, v)| on.then_some(v))
    .collect();
    assert_eq!(want, vec![3], "fullscreen, and nothing else");
    assert_eq!(t.ls.probe.wm_capabilities, vec![(w, want.clone())]);
    // later configures do not repeat or change it
    set_focus(&mut t.ls.state, &mut t.ls.input, Some(id)).expect("focus");
    t.adv();
    t.ask(w, |tl| tl.set_maximized());
    assert_eq!(t.ls.probe.wm_capabilities, vec![(w, want)]);
    assert_eq!(
        t.ls.probe.configure_bounds,
        vec![(w, (1920 - 24, 1080 - 24))]
    );
}

/// 300 requests in a fixed pseudo-random order on a focused window that never resizes: the
/// configures received are exactly one per state request, in order, and each is what a model of
/// the policy predicts — `[fullscreen, activated]` with the output's size while fullscreen, the
/// saved 400x300 once when it ends, `[activated]` with no size otherwise. No anchor, camera or
/// focus changes.
#[test]
fn a_stream_of_requests_is_answered_one_for_one() {
    let mut t = T::new();
    let (w, id) = t.map("W");
    set_focus(&mut t.ls.state, &mut t.ls.input, Some(id)).expect("focus");
    t.adv();
    let serial = t.click();
    let seat = t.ls.probe.seat.clone().expect("seat");
    let before = t.authority();
    let start = t.configures(w).len();
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut expected = Vec::new();
    let mut fullscreen = false;
    let windowed = ((0, 0), vec![ACTIVATED]);
    let full = ((1920, 1080), vec![FULLSCREEN, ACTIVATED]);
    for _ in 0..300 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let tl = t.toplevel(w);
        match x % 8 {
            0 => tl.set_maximized(),
            1 => tl.unset_maximized(),
            2 => tl.set_fullscreen(None),
            3 => tl.unset_fullscreen(),
            4 => tl.set_minimized(),
            5 => tl._move(&seat, serial),
            6 => tl.resize(&seat, serial, ResizeEdge::Top),
            _ => tl.show_window_menu(&seat, serial, 1, 1),
        }
        match x % 8 {
            0 | 1 if fullscreen => expected.push(full.clone()),
            0 | 1 => expected.push(windowed.clone()),
            2 => {
                fullscreen = true;
                expected.push(full.clone());
            }
            3 if fullscreen => {
                fullscreen = false;
                expected.push(((400, 300), vec![ACTIVATED]));
            }
            3 => expected.push(windowed.clone()),
            _ => {}
        }
        if x.is_multiple_of(5) {
            t.adv();
        }
    }
    t.adv();
    let got = &t.configures(w)[start..];
    assert_eq!(got.len(), expected.len(), "one configure per state request");
    assert_eq!(got, expected.as_slice());
    assert!(expected.contains(&full) && expected.contains(&((400, 300), vec![ACTIVATED])));
    assert_eq!(t.authority(), before);
}
