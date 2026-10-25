//! Production path tests for the off-screen frame policy (M16; INV-FRAME-1, INV-FRAME-3,
//! INV-FRAME-4, INV-IDLE-1; OB-16; TST-03 SC3 and TST-06).
//!
//! The production Core, adapter and headless backend with a recording client. Time is virtual:
//! the lockstep runs vblanks and the fallback timer in time order and ends every pump with the
//! fallback evaluation, as a backend's loop iteration does. The oracle is the client's own wire
//! log (`probe.wire`: one ordered log across objects) and the times of its callbacks
//! (`probe.frames`).
//!
//! Scene: output A 1920x1080 at 60 Hz; W 800x600 with a 20 px shadow, centred, focused. W is
//! made unobserved by a camera pan (Super + middle button) of 10 × (300, 0): the view moves
//! 3000 px, W's 840 px wide tree spans at most 1380 px from the view centre, so no pixel of it
//! is on A. Panning back by the same amount brings it back.

mod common;

use std::time::Duration;

use common::input::{BTN_MIDDLE, Rig, a_1080, logo};
use common::{Lockstep, PopupSpec};
use infynitae_backend_headless::{keyboard_key, set_focus};
use infynitae_core::input::keys::{KEY_A, KEY_LEFTMETA};
use infynitae_core::stage::ElementId;

const P: Duration = Duration::from_secs(1);
const MS: Duration = Duration::from_millis(1);

struct F {
    r: Rig,
    w: usize,
    e: ElementId,
}

impl F {
    /// W mapped and focused on A, observed.
    fn new() -> F {
        let mut r = Rig::new(Lockstep::new(Some(a_1080())));
        r.map_w();
        let w =
            r.ls.probe
                .surfaces
                .iter()
                .position(|s| s.label == "W")
                .unwrap();
        let e = r.ls.state.elements().next().unwrap();
        set_focus(&mut r.ls.state, &mut r.ls.input, Some(e)).unwrap();
        r.ls.pump();
        r.frame();
        let mut f = F { r, w, e };
        assert!(f.observed(), "W starts on A");
        f.r.frame();
        f
    }

    /// W and the scene after a pan that takes W off A.
    fn off_screen() -> F {
        let mut f = F::new();
        f.pan(300.0);
        assert!(!f.observed(), "the pan took W off A");
        f
    }

    fn pan(&mut self, dx: f64) {
        self.r.button(BTN_MIDDLE, true, logo());
        for _ in 0..10 {
            self.r.motion((dx, 0.0));
        }
        self.r.button(BTN_MIDDLE, false, logo());
    }

    fn observed(&self) -> bool {
        self.r.ls.state.core().membership().is_observed(self.e)
    }

    fn request(&mut self) {
        let qh = self.r.ls.qh.clone();
        self.r.ls.probe.frame_and_draw(&qh, self.w);
        self.r.ls.pump();
    }

    fn now(&self) -> Duration {
        self.r.ls.now
    }

    fn run(&mut self, d: Duration) {
        let t = self.r.ls.now + d;
        self.r.ls.run_until(t);
    }

    fn frames_w(&self) -> Vec<Duration> {
        self.r
            .ls
            .probe
            .frames
            .iter()
            .filter(|(_, l)| *l == "W")
            .map(|(t, _)| *t)
            .collect()
    }

    fn wire_mark(&self) -> usize {
        self.r.ls.probe.wire.len()
    }

    fn wire_since(&self, m: usize) -> Vec<&str> {
        self.r.ls.probe.wire[m..]
            .iter()
            .map(|s| s.as_str())
            .collect()
    }

    fn deadline(&self) -> Option<Duration> {
        self.r.ls.state.core().frame_fallback_deadline()
    }
}

/// INV-FRAME-3 (b), TST-03 SC3: an unobserved window's callback is withheld for one period P
/// from the moment it became pending, then completes once; nothing more completes without a new
/// request (INV-FRAME-1); a continuously requesting client gets exactly one completion per P.
#[test]
fn an_unobserved_window_gets_one_callback_per_fallback_period() {
    let mut f = F::off_screen();
    let before = f.frames_w().len();
    f.request();
    let t0 = f.now();
    assert_eq!(
        f.deadline(),
        Some(t0 + P),
        "the timer is armed for one period"
    );
    f.run(P - MS);
    assert_eq!(f.frames_w().len(), before, "withheld for P");
    f.run(MS);
    assert_eq!(f.frames_w()[before..], [t0 + P], "one completion at P");
    assert_eq!(f.deadline(), None, "nothing pending: no timer (INV-IDLE-1)");
    f.run(5 * P);
    assert_eq!(f.frames_w().len(), before + 1, "no invented callback");

    // a client that redraws on every `done`: its last completion is more than P ago, so the
    // first request completes at once, then one per P
    f.r.ls.probe.surfaces[f.w].frame_driven = true;
    f.request();
    let t1 = f.now();
    f.run(5 * P);
    let got = f.frames_w()[before + 1..].to_vec();
    assert_eq!(got, (0..=5).map(|k| t1 + k * P).collect::<Vec<_>>());
}

/// INV-IDLE-1: an observed animating client is clocked by A's vblank and never arms the
/// fallback timer; once it stops, nothing is armed.
#[test]
fn an_observed_client_is_clocked_by_vblank_and_arms_no_timer() {
    let mut f = F::new();
    let before = f.frames_w().len();
    f.r.ls.probe.surfaces[f.w].frame_driven = true;
    f.request();
    for _ in 0..120 {
        f.r.frame();
        assert_eq!(f.deadline(), None, "an observed element arms no timer");
    }
    let got = f.frames_w().len() - before;
    assert!(
        (119..=121).contains(&got),
        "one completion per vblank, got {got}"
    );
    f.r.ls.probe.surfaces[f.w].frame_driven = false;
    f.r.frame();
    f.r.frame();
    assert_eq!(f.deadline(), None);
}

/// OB-16, INV-FRAME-3 (a): input delivered to a focused, unobserved client with a
/// pending callback completes it first — in the same dispatch, before the key event, and once.
#[test]
fn a_key_kicks_a_focused_unobserved_client_before_the_key_arrives() {
    let mut f = F::off_screen();
    let before = f.frames_w().len();
    f.request();
    f.run(100 * MS);
    let at = f.now();
    let m = f.wire_mark();
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, true, 1).unwrap();
    f.r.ls.pump();
    assert_eq!(
        f.wire_since(m),
        ["frame W", &format!("key {KEY_A}")],
        "the withheld callback completes before the key, once"
    );
    assert_eq!(
        f.frames_w()[before..],
        [at],
        "first response within the dispatch"
    );
    let m = f.wire_mark();
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, false, 2).unwrap();
    f.r.ls.pump();
    assert_eq!(
        f.wire_since(m),
        [format!("key {KEY_A}")],
        "nothing pending: no kick"
    );
    assert_eq!(f.deadline(), None);
}

/// INV-FRAME-3 (a): a non-initial configure (here the `activated` change of a focus loss) and
/// the keyboard `leave` are preceded by the completion of the withheld callback.
#[test]
fn a_configure_kicks_before_it_is_sent() {
    let mut f = F::off_screen();
    f.request();
    let m = f.wire_mark();
    set_focus(&mut f.r.ls.state, &mut f.r.ls.input, None).unwrap();
    f.r.ls.pump();
    let wire = f.wire_since(m);
    assert_eq!(wire.first(), Some(&"frame W"), "kick first: {wire:?}");
    assert!(wire.contains(&"configure W"), "{wire:?}");
    assert_eq!(
        wire.iter().filter(|l| **l == "frame W").count(),
        1,
        "{wire:?}"
    );
}

/// The fallback period (AUD-24) is measured from the last completion, kick included, so a
/// fallback never follows a kick within P.
#[test]
fn a_kick_restarts_the_fallback_period() {
    let mut f = F::off_screen();
    f.request();
    f.run(P / 2);
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, true, 1).unwrap();
    f.r.ls.pump();
    let kick = f.now();
    assert_eq!(f.frames_w().last(), Some(&kick));
    f.request();
    f.run(P - MS);
    assert_eq!(
        f.frames_w().last(),
        Some(&kick),
        "no fallback within P of the kick"
    );
    f.run(MS);
    assert_eq!(f.frames_w().last(), Some(&(kick + P)));
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, false, 2).unwrap();
    f.r.ls.pump();
}

/// INV-FRAME-4: when W is observed again, its pending callback completes at the first vblank
/// of its clock output, without waiting for the fallback. Here re-entry is the reconnection of
/// the only output: nothing completes at the connection itself, then exactly at A's first vblank.
#[test]
fn re_entry_completes_at_the_first_vblank() {
    let mut f = F::new();
    let a = f.r.ls.output();
    f.r.ls.disconnect(a);
    f.request();
    f.run(100 * MS);
    let before = f.frames_w().len();
    let connected = f.now();
    f.r.ls.connect(&a_1080());
    assert!(f.observed(), "W is on the new output");
    assert_eq!(
        f.frames_w().len(),
        before,
        "nothing completes before the vblank"
    );
    assert!(f.r.ls.state.core().frame_pending(f.e));
    assert_eq!(f.deadline(), None, "observed: the timer is disarmed");
    f.run(Duration::from_micros(16_667));
    assert_eq!(
        f.frames_w()[before..],
        [connected + Duration::from_nanos(16_666_666)],
        "at the first vblank of the clock output"
    );
}

/// INV-FRAME-4 after a camera pan: the callback withheld while W was off A completes once W is
/// back, long before the fallback would have run.
#[test]
fn re_entry_after_a_pan_completes_before_the_fallback() {
    let mut f = F::off_screen();
    f.request();
    let requested = f.now();
    f.run(100 * MS);
    let before = f.frames_w().len();
    f.pan(-300.0);
    assert!(f.observed(), "the pan brought W back");
    let got = f.frames_w()[before..].to_vec();
    assert_eq!(got.len(), 1, "one completion: {got:?}");
    assert!(got[0] < requested + P, "{got:?}");
    assert_eq!(f.deadline(), None);
}

/// M16 with zero outputs: every element is unobserved; a pending callback still completes once
/// per P, so a client does not freeze while no output is connected.
#[test]
fn zero_outputs_still_complete_on_the_fallback() {
    let mut f = F::new();
    let a = f.r.ls.output();
    f.r.ls.disconnect(a);
    assert!(!f.observed());
    let before = f.frames_w().len();
    f.request();
    let t0 = f.now();
    f.run(3 * P);
    assert_eq!(f.frames_w()[before..], [t0 + P]);
}

/// INV-FRAME-2, INV-FRAME-3: an observed client is never kicked; input reaches it at once and its
/// callback still completes at the next vblank of its clock output (no double rate).
#[test]
fn an_observed_client_is_not_kicked() {
    let mut f = F::new();
    f.request();
    let m = f.wire_mark();
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, true, 1).unwrap();
    f.r.ls.pump();
    assert_eq!(f.wire_since(m), [format!("key {KEY_A}")]);
    f.r.frame();
    assert_eq!(f.wire_since(m), [format!("key {KEY_A}"), "frame W".into()]);
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, false, 2).unwrap();
    f.r.ls.pump();
}

/// OBJ-17, INV-IDLE-1: callbacks completed by vblanks are recorded as completed, so a window
/// that drew while observed and then stopped arms no timer when it leaves the view.
#[test]
fn a_window_that_stopped_drawing_arms_no_timer_when_it_leaves() {
    let mut f = F::new();
    f.request();
    f.r.frame();
    f.r.frame();
    assert!(
        !f.r.ls.state.core().frame_pending(f.e),
        "completed at the vblank"
    );
    f.pan(300.0);
    assert!(!f.observed());
    assert_eq!(f.deadline(), None, "no pending callback: no timer");
}

/// INV-FRAME-5, TST-06: the presentation feedback of an unobserved commit stays
/// pending through the kick and the fallback; a newer commit discards it; the latest is
/// `presented` once W is shown again, synchronised to its clock output.
#[test]
fn unobserved_feedback_stays_pending_until_shown_or_superseded() {
    let mut f = F::new();
    let a = f.r.ls.output();
    f.r.ls.disconnect(a);
    let qh = f.r.ls.qh.clone();
    let first = f.r.ls.probe.frame_and_draw(&qh, f.w);
    f.r.ls.pump();
    let m = f.r.ls.probe.log.len();
    f.run(2 * P);
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, true, 1).unwrap();
    f.r.ls.pump();
    let log = |f: &F, m: usize| f.r.ls.probe.log[m..].to_vec();
    assert!(
        !log(&f, m).iter().any(|l| l.contains(&format!("#{first}"))),
        "feedback answered while unobserved: {:?}",
        log(&f, m)
    );
    let second = f.r.ls.probe.frame_and_draw(&qh, f.w);
    f.r.ls.pump();
    assert!(
        log(&f, m).contains(&format!("discarded #{first}")),
        "superseded: {:?}",
        log(&f, m)
    );
    let a = f.r.ls.connect(&a_1080());
    f.run(Duration::from_micros(16_667));
    let l = log(&f, m);
    assert!(l.contains(&format!("presented #{second}")), "{l:?}");
    let name = l
        .iter()
        .find_map(|x| x.strip_prefix(&format!("sync #{second} ")))
        .expect("sync_output");
    assert!(
        l.contains(&format!("global wl_output {name}")),
        "synchronised to the reconnected output {a:?}: {l:?}"
    );
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, false, 2).unwrap();
    f.r.ls.pump();
}

/// INV-FRAME-3 (a): the pointer focus is followed by surface, whatever the element's state when
/// the pointer entered it. W is under the pointer and visible, then another window's
/// presentation hides it: the pointer `leave` that W receives is preceded by its withheld
/// callback.
#[test]
fn a_pointer_leave_kicks_a_window_hidden_after_the_pointer_entered_it() {
    let mut f = F::new();
    let qh = f.r.ls.qh.clone();
    let x = f.r.ls.probe.toplevel(&qh, "X", (200, 100), 0);
    f.r.ls.pump();
    f.r.frame();
    let ex = f.r.ls.state.elements().last().unwrap();
    // the pointer enters W (X is centred on top: move off it, still on W)
    f.r.motion((-300.0, -200.0));
    assert!(
        f.r.ls.probe.ptr.iter().any(|l| l.starts_with("enter W")),
        "{:?}",
        f.r.ls.probe.ptr
    );
    set_focus(&mut f.r.ls.state, &mut f.r.ls.input, Some(ex)).unwrap();
    f.r.ls.pump();
    f.request();
    assert!(f.observed() && f.r.ls.state.core().frame_pending(f.e));
    let m = f.wire_mark();
    let tl = f.r.ls.probe.surfaces[x].toplevel.clone().unwrap();
    tl.set_fullscreen(None);
    f.r.ls.pump();
    f.r.frame();
    assert!(!f.observed(), "hidden by X's presentation");
    let wire = f.wire_since(m);
    let leave = wire
        .iter()
        .position(|l| *l == "pointer leave W")
        .unwrap_or_else(|| panic!("no pointer leave for W: {wire:?}"));
    assert_eq!(
        wire.iter().position(|l| *l == "frame W"),
        Some(leave - 1),
        "kick right before the leave: {wire:?}"
    );
}

/// A camera-pan press dismisses popup grabs (OB-4); with no popup grab no
/// `popup_done` is sent, so nothing reaches W and nothing may complete its withheld callback
/// before the fallback (INV-FRAME-1: a kick only precedes an event that reaches the client).
/// Super is pressed first (it reaches W, K3, and kicks it), then W asks for a frame, then the
/// middle button: that press reaches no client.
#[test]
fn a_pan_press_without_a_popup_grab_kicks_nothing() {
    let mut f = F::off_screen();
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_LEFTMETA, true, 1).unwrap();
    f.r.ls.pump();
    f.r.meta_held = true;
    f.request();
    f.run(100 * MS);
    let m = f.wire_mark();
    f.r.button(BTN_MIDDLE, true, logo());
    let wire = f.wire_since(m);
    assert!(!wire.contains(&"frame W"), "spurious kick: {wire:?}");
    assert!(f.r.ls.state.core().frame_pending(f.e), "still withheld");
    f.r.button(BTN_MIDDLE, false, logo());
}

/// INV-FRAME-3 (a): `popup_done` is preceded by the withheld callback of the client
/// that receives it. W, off A and focused, takes a popup grab (with the serial of a key it was
/// sent); with Super already held, a pan press dismisses the grab: W's callback completes, then
/// `popup_done` arrives.
#[test]
fn a_popup_done_kicks_before_it_is_sent() {
    let mut f = F::off_screen();
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, true, 1).unwrap();
    f.r.ls.pump();
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_A, false, 2).unwrap();
    f.r.ls.pump();
    let qh = f.r.ls.qh.clone();
    f.r.ls.probe.popup_with_grab(
        &qh,
        "P",
        f.w,
        PopupSpec {
            content: (120, 80),
            shadow: 0,
            anchor_rect: (10, 10, 1, 1),
            slide: false,
        },
    );
    f.r.ls.pump();
    f.run(20 * MS);
    assert!(f.r.ls.probe.popup_dones.is_empty(), "the grab was accepted");
    // Super first: it reaches W's popup and kicks W; the request after it is what the
    // dismissal must complete
    keyboard_key(&mut f.r.ls.state, &mut f.r.ls.input, KEY_LEFTMETA, true, 3).unwrap();
    f.r.ls.pump();
    f.r.meta_held = true;
    f.request();
    f.run(MS);
    assert!(f.r.ls.state.core().frame_pending(f.e));
    let m = f.wire_mark();
    f.r.button(BTN_MIDDLE, true, logo());
    let wire = f.wire_since(m);
    let done = wire
        .iter()
        .position(|l| *l == "popup_done P")
        .unwrap_or_else(|| panic!("no popup_done: {wire:?}"));
    let frame = wire
        .iter()
        .position(|l| *l == "frame W")
        .unwrap_or_else(|| panic!("no kick: {wire:?}"));
    assert!(frame < done, "{wire:?}");
    f.r.button(BTN_MIDDLE, false, logo());
}

/// Surface destruction (INV-IDLE-1): a destroyed window with a withheld callback disarms the
/// fallback timer and nothing is completed for it afterwards.
#[test]
fn a_destroyed_window_disarms_the_fallback_timer() {
    let mut f = F::off_screen();
    // unfocused first, so that the destruction sends W no focus change (which would kick it)
    set_focus(&mut f.r.ls.state, &mut f.r.ls.input, None).unwrap();
    f.r.ls.pump();
    f.request();
    assert!(f.deadline().is_some(), "armed for W");
    f.r.ls.probe.destroy_toplevel(f.w);
    f.r.ls.pump();
    assert_eq!(f.deadline(), None);
    let n = f.frames_w().len();
    f.run(3 * P);
    assert_eq!(f.frames_w().len(), n);
}

/// Repeated visibility transitions: W leaves and re-enters A again and again while
/// it keeps asking for frames. Every request completes exactly once (INV-FRAME-1) and none
/// waits longer than P plus one vblank (no starvation, INV-FRAME-3/4).
#[test]
fn repeated_visibility_transitions_neither_starve_nor_duplicate() {
    let mut f = F::new();
    let vblank = Duration::from_micros(16_667);
    for i in 0..12u32 {
        f.request();
        let asked = f.now();
        let before = f.frames_w().len();
        f.pan(if i % 2 == 0 { 300.0 } else { -300.0 });
        // a dwell of a few ms to most of a period, so transitions fall at every phase
        f.run(Duration::from_millis(u64::from(7 + 83 * (i % 6))));
        let deadline = asked + P + 2 * vblank;
        while f.frames_w().len() == before && f.now() < deadline {
            f.run(MS);
        }
        let new = f.frames_w()[before..].to_vec();
        assert_eq!(
            new.len(),
            1,
            "transition {i}: exactly one completion: {new:?}"
        );
        assert!(
            new[0] - asked <= P + 2 * vblank,
            "transition {i}: waited {:?}",
            new[0] - asked
        );
        f.run(5 * MS);
        assert_eq!(
            f.frames_w().len(),
            before + 1,
            "transition {i}: no second completion"
        );
    }
}

/// Multiple outputs: another output's vblanks never complete a
/// window that output does not observe. W is observed by A only (B, 144 Hz, looks far away);
/// A is disconnected: W is unobserved while B keeps ticking; W's callback is withheld through
/// B's vblanks and completes once, by the fallback, P after its last completion.
#[test]
fn another_outputs_vblanks_do_not_clock_an_unobserved_window() {
    use common::output_k;
    use infynitae_core::geom::CanvasPos;
    use infynitae_core::navigation::PersistedCameras;
    let b_far = PersistedCameras::from_entries(vec![(
        infynitae_backend_headless::VirtualOutput::key("B"),
        CanvasPos {
            x: 100_000 << 16,
            y: 0,
        },
        1.0,
    )])
    .unwrap();
    let mut r = Rig::new(Lockstep::with(
        b_far,
        &[a_1080(), output_k("B", (1920, 1080), 120, 144_000)],
    ));
    r.map_w();
    let w =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let e = r.ls.state.elements().next().unwrap();
    let a = r.ls.state.core().outputs()[0].id;
    let b = r.ls.state.core().outputs()[1].id;
    assert_eq!(
        r.ls.state.core().membership().observers(e),
        vec![a],
        "A only"
    );
    let mut f = F { r, w, e };
    f.r.frame();
    f.r.ls.disconnect(a);
    assert!(!f.observed(), "no output observes W; B still runs");
    assert!(f.r.ls.state.core().output(b).is_some());
    let before = f.frames_w().len();
    let last = f.frames_w().last().copied().unwrap_or_default();
    f.request();
    let t0 = f.now();
    f.run(P / 2);
    assert_eq!(
        f.frames_w().len(),
        before,
        "B's vblanks complete nothing of W"
    );
    f.run(2 * P);
    let new = f.frames_w()[before..].to_vec();
    assert_eq!(new.len(), 1, "once: {new:?}");
    assert!(
        new[0] >= t0 && new[0] <= last.max(t0) + P,
        "{new:?} t0 {t0:?}"
    );
}
