//! Commit timing (`wp_commit_timing_v1`, `commit_timing`) with a real client: a commit given a
//! time is held until the first refresh at or after that time, and its presentation feedback is
//! presented at that refresh, never at one before. A hidden window's timed commit applies when its
//! time comes.
//!
//! Scene: output A 1920x1080 at 60 Hz (vblanks every 16.667 ms of virtual time); W 800x600 with a
//! 20 px shadow (an 840x640 buffer). Each commit's buffer is one pixel taller than the last, so the
//! height the compositor holds says which commit applied.

mod common;

use std::time::Duration;

use common::Lockstep;
use common::input::{BTN_MIDDLE, Rig, a_1080, logo};
use infynitae_backend_headless::vblank_period;
use infynitae_core::stage::ElementId;
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use wayland_protocols::wp::commit_timing::v1::client::{
    wp_commit_timer_v1::WpCommitTimerV1, wp_commit_timing_manager_v1::WpCommitTimingManagerV1,
};

wayland_client::delegate_noop!(common::Probe: ignore WpCommitTimingManagerV1);
wayland_client::delegate_noop!(common::Probe: ignore WpCommitTimerV1);

const NS: Duration = Duration::from_nanos(1);

struct F {
    r: Rig,
    w: usize,
    e: ElementId,
    timer: WpCommitTimerV1,
    height: i32,
}

impl F {
    /// W mapped on A with a commit timer, its first frame shown.
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
        let (name, _, _) =
            r.ls.probe
                .globals
                .iter()
                .find(|(_, i, _)| i == "wp_commit_timing_manager_v1")
                .cloned()
                .expect("wp_commit_timing_manager_v1 is offered");
        let qh = r.ls.qh.clone();
        let manager: WpCommitTimingManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
        let timer = manager.get_timer(&r.ls.probe.surfaces[w].wl, &qh, ());
        r.ls.pump();
        r.frame();
        F {
            r,
            w,
            e,
            timer,
            height: 640,
        }
    }

    /// A new frame to be shown at `at` (the presentation clock, here virtual), with a frame
    /// callback and a presentation feedback: its height and feedback number.
    fn commit_at(&mut self, at: Duration) -> (i32, u32) {
        self.height += 1;
        self.r.ls.probe.surfaces[self.w].size = (840, self.height);
        self.timer.set_timestamp(
            (at.as_secs() >> 32) as u32,
            at.as_secs() as u32,
            at.subsec_nanos(),
        );
        let qh = self.r.ls.qh.clone();
        let seq = self.r.ls.probe.frame_and_draw(&qh, self.w);
        self.r.ls.pump();
        (self.height, seq)
    }

    /// The height of the buffer the compositor applied last.
    fn applied(&self) -> i32 {
        let surface = self.r.ls.state.window_surface(self.e).expect("W's surface");
        smithay::wayland::compositor::with_states(&surface, |states| {
            states
                .data_map
                .get::<RendererSurfaceStateUserData>()
                .and_then(|d| d.lock().unwrap().buffer_size())
                .expect("W has a buffer")
                .h
        })
    }

    /// The last vblank: the lockstep connected A at 0, so they come every period from 0.
    fn last_vblank(&self) -> Duration {
        let p = vblank_period(60_000);
        let k = self.r.ls.now.as_nanos() / p.as_nanos();
        Duration::from_nanos((p.as_nanos() * k) as u64)
    }

    fn presented(&self, seq: u32) -> bool {
        self.r.ls.probe.log.contains(&format!("presented #{seq}"))
    }
}

#[test]
fn a_timed_commit_is_shown_at_the_first_refresh_at_or_after_its_time() {
    let mut f = F::new();
    let p = vblank_period(60_000);
    let v0 = f.last_vblank();
    let before = f.applied();
    let (height, seq) = f.commit_at(v0 + p * 2 + p / 2);
    assert_eq!(f.applied(), before, "held until its time");

    f.r.ls.run_until(v0 + p);
    f.r.ls.run_until(v0 + p * 2);
    assert_eq!(f.applied(), before, "not at the refreshes before its time");
    assert!(!f.presented(seq));

    f.r.ls.run_until(v0 + p * 3);
    assert_eq!(
        f.applied(),
        height,
        "in the frame of the first refresh after its time"
    );
    assert!(f.presented(seq), "presented at that refresh");
}

#[test]
fn a_commit_timed_for_a_refresh_is_shown_at_that_refresh() {
    let mut f = F::new();
    let p = vblank_period(60_000);
    let v0 = f.last_vblank();
    let before = f.applied();
    let (height, seq) = f.commit_at(v0 + p * 2);

    f.r.ls.run_until(v0 + p * 2 - NS);
    assert_eq!(f.applied(), before);
    f.r.ls.run_until(v0 + p * 2);
    assert_eq!(
        f.applied(),
        height,
        "a time on a refresh is met by that refresh"
    );
    assert!(f.presented(seq));

    // a time already past applies at once
    let now = f.r.ls.now;
    let (late, _) = f.commit_at(now - p);
    assert_eq!(f.applied(), late);
}

#[test]
fn a_hidden_windows_timed_commit_applies_when_its_time_comes() {
    let mut f = F::new();
    f.r.button(BTN_MIDDLE, true, logo());
    for _ in 0..10 {
        f.r.motion((300.0, 0.0));
    }
    f.r.button(BTN_MIDDLE, false, logo());
    assert!(
        !f.r.ls.state.core().membership().is_observed(f.e),
        "the pan took W off A"
    );
    let p = vblank_period(60_000);
    let at = f.r.ls.now + Duration::from_millis(40);
    let before = f.applied();
    let (height, _) = f.commit_at(at);

    f.r.ls.run_until(at - NS);
    assert_eq!(f.applied(), before, "not before its time");
    // the loop sees its time at its next turn: here the next vblank's
    f.r.ls.run_until(at + p);
    assert_eq!(f.applied(), height, "applied though no screen shows W");
}
