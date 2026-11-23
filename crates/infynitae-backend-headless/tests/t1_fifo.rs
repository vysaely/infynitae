//! FIFO presentation (`wp_fifo_v1`, `fifo`) with a real client, as Mesa's Vulkan FIFO mode uses
//! it: every commit sets a barrier and waits for the one the commit before set. A commit that waits
//! applies at the next refresh of the window's screen, one per refresh; for a window no screen
//! shows, at the off-screen fallback (INV-FRAME-3), also when the client asks for no frame callback.
//!
//! Scene: output A 1920x1080 at 60 Hz; W 800x600 with a 20 px shadow (an 840x640 buffer). Each
//! commit's buffer is one pixel taller than the last, so the height the compositor holds says which
//! commit applied.

mod common;

use std::time::Duration;

use common::Lockstep;
use common::input::{BTN_MIDDLE, Rig, a_1080, logo};
use infynitae_core::stage::ElementId;
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use wayland_protocols::wp::fifo::v1::client::{
    wp_fifo_manager_v1::WpFifoManagerV1, wp_fifo_v1::WpFifoV1,
};

wayland_client::delegate_noop!(common::Probe: ignore WpFifoManagerV1);
wayland_client::delegate_noop!(common::Probe: ignore WpFifoV1);

const MS: Duration = Duration::from_millis(1);

struct F {
    r: Rig,
    w: usize,
    e: ElementId,
    fifo: WpFifoV1,
    height: i32,
}

impl F {
    /// W mapped on A with a FIFO object, its first frame shown.
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
                .find(|(_, i, _)| i == "wp_fifo_manager_v1")
                .cloned()
                .expect("wp_fifo_manager_v1 is offered");
        let qh = r.ls.qh.clone();
        let manager: WpFifoManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
        let fifo = manager.get_fifo(&r.ls.probe.surfaces[w].wl, &qh, ());
        r.ls.pump();
        r.frame();
        F {
            r,
            w,
            e,
            fifo,
            height: 640,
        }
    }

    /// A new frame that sets a barrier and, with `wait`, waits for the previous one (no frame
    /// callback); the height of its buffer.
    fn commit(&mut self, wait: bool) -> i32 {
        self.height += 1;
        let qh = self.r.ls.qh.clone();
        let buffer = self.r.ls.probe.buffer(&qh, 840, self.height);
        self.fifo.set_barrier();
        if wait {
            self.fifo.wait_barrier();
        }
        let wl = &self.r.ls.probe.surfaces[self.w].wl;
        wl.attach(Some(&buffer), 0, 0);
        wl.damage_buffer(0, 0, 840, self.height);
        wl.commit();
        self.r.ls.pump();
        self.height
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

    fn observed(&self) -> bool {
        self.r.ls.state.core().membership().is_observed(self.e)
    }

    fn run(&mut self, d: Duration) {
        let t = self.r.ls.now + d;
        self.r.ls.run_until(t);
    }
}

#[test]
fn a_waiting_commit_applies_at_the_next_refresh_one_per_refresh() {
    let mut f = F::new();
    let first = f.commit(false);
    assert_eq!(
        f.applied(),
        first,
        "a commit that does not wait applies at once"
    );

    let second = f.commit(true);
    assert_eq!(f.applied(), first, "held until the first frame was shown");
    f.r.frame();
    assert_eq!(f.applied(), second, "the refresh lifted the barrier");

    // a client that commits twice within one refresh gets the second in only at the next one
    let third = f.commit(true);
    let fourth = f.commit(true);
    assert_eq!(f.applied(), second);
    f.r.frame();
    assert_eq!(f.applied(), third, "one commit per refresh");
    f.r.frame();
    assert_eq!(f.applied(), fourth);
}

#[test]
fn a_hidden_window_is_paced_by_the_fallback_not_blocked() {
    let mut f = F::new();
    f.commit(false);
    f.r.frame();
    // a pan of 3000 px takes W off A (as in t1_frames)
    f.r.button(BTN_MIDDLE, true, logo());
    for _ in 0..10 {
        f.r.motion((300.0, 0.0));
    }
    f.r.button(BTN_MIDDLE, false, logo());
    assert!(!f.observed(), "the pan took W off A");
    let period = f.r.ls.state.core().frame_fallback_period();

    // the barrier before was lifted while W was shown: this one applies at once
    let first = f.commit(true);
    assert_eq!(f.applied(), first);
    let second = f.commit(true);
    assert_eq!(f.applied(), first, "held while no screen shows W");
    // the barrier counts as a requested frame, though the client asked for no callback
    let due =
        f.r.ls
            .state
            .core()
            .frame_fallback_deadline()
            .expect("the fallback timer is armed for the waiting barrier");
    f.r.ls.run_until(due - MS);
    assert_eq!(f.applied(), first, "withheld until the fallback");
    f.r.ls.run_until(due);
    assert_eq!(f.applied(), second, "the fallback lifted the barrier");

    let third = f.commit(true);
    assert_eq!(f.applied(), second);
    assert_eq!(
        f.r.ls.state.core().frame_fallback_deadline(),
        Some(due + period),
        "one commit per fallback period"
    );
    f.run(period);
    assert_eq!(f.applied(), third);
}
