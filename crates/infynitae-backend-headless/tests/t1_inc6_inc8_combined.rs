//! INC-6 × INC-8 integration (2026-10-16, INTEGRATION CANDIDATE, NOT RATIFIED): the scale policy
//! and the off-screen frame policy acting on the same window, on the production path.
//!
//! X is a window on a 2x output; W, another client's window, is focused and goes fullscreen on
//! that output, which hides X: X is unobserved (INC-8 withholds its callbacks) and its hint is
//! held (C12.F13, rehearsal). A device-limit change is a correctness rule (P7) that lowers X's
//! hint while X is hidden. A scale hint is not an event that unblocks a client waiting for its
//! frame (ARCHITECTURE scenario 11: `preferred_scale` does not kick), so X's withheld callback
//! must not complete with it; it completes on the fallback timer or when X is shown again.

mod common;

use std::time::Duration;

use common::{Lockstep, output};
use infynitae_backend_headless::{DeviceLimit, HeadlessRenderer, set_focus};

const P: Duration = Duration::from_secs(1);

fn since(ls: &Lockstep, mark: usize) -> Vec<&str> {
    ls.probe.log[mark..].iter().map(|s| s.as_str()).collect()
}

#[test]
fn a_hidden_windows_lowered_hint_releases_no_withheld_callback() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 240)));
    let qh = ls.qh.clone();
    let x = ls
        .probe
        .toplevel_with_fractional_scale(&qh, "X", (300, 200), 0);
    ls.pump();
    ls.probe.draw(&qh, x);
    ls.pump();
    let w = ls.probe.toplevel(&qh, "W", (400, 300), 0);
    ls.pump();
    ls.probe.draw(&qh, w);
    ls.pump();
    let ids: Vec<_> = ls.state.elements().collect();
    let (ex, ew) = (ids[0], ids[1]);
    assert_eq!(ls.probe.preferred_scales_of("X").last(), Some(&240));
    set_focus(&mut ls.state, &mut ls.input, Some(ew)).unwrap();
    ls.pump();
    ls.probe.surfaces[w]
        .toplevel
        .clone()
        .unwrap()
        .set_fullscreen(None);
    ls.pump();
    let t = ls.now + Duration::from_millis(50);
    ls.run_until(t);
    assert!(
        !ls.state.core().membership().is_observed(ex),
        "W's presentation hides X"
    );

    // X asks for a frame while hidden: withheld
    let frames_before = ls.probe.frames.iter().filter(|(_, l)| *l == "X").count();
    ls.probe.frame_and_draw(&qh, x);
    ls.pump();
    assert!(ls.state.core().frame_pending(ex));

    // P7 while hidden: 300 px at 2 no longer fits a 400 px device
    let mark = ls.probe.log.len();
    let renderer = HeadlessRenderer::new(DeviceLimit::MaxBufferSide(400));
    assert_eq!(ls.state.report_renderer_limits(&renderer), Ok(400));
    ls.pump();
    let log = since(&ls, mark);
    assert!(
        log.contains(&"preferred_buffer_scale X 1"),
        "P7 lowered X: {log:?}"
    );
    let s120 = *ls.probe.preferred_scales_of("X").last().unwrap();
    assert!(300 * u64::from(s120) <= 400 * 120, "fits: {s120}");
    assert!(
        !log.contains(&"frame X"),
        "a hint is not a kick: the callback stays withheld: {log:?}"
    );
    assert!(ls.state.core().frame_pending(ex));

    // the fallback still serves X, once, within P of its last completion
    let t = ls.now + P;
    ls.run_until(t);
    let frames = ls.probe.frames.iter().filter(|(_, l)| *l == "X").count();
    assert_eq!(frames, frames_before + 1, "the fallback completed it once");

    // revealed: X's next callback completes at the first vblank, and X keeps a hint that fits
    ls.probe.frame_and_draw(&qh, x);
    ls.pump();
    ls.probe.surfaces[w]
        .toplevel
        .clone()
        .unwrap()
        .unset_fullscreen();
    ls.pump();
    assert!(ls.state.core().membership().is_observed(ex), "shown again");
    let t = ls.now + Duration::from_millis(34);
    ls.run_until(t);
    let frames = ls.probe.frames.iter().filter(|(_, l)| *l == "X").count();
    assert_eq!(
        frames,
        frames_before + 2,
        "completed at re-entry (INV-FRAME-4)"
    );
    let s120 = *ls.probe.preferred_scales_of("X").last().unwrap();
    assert!(300 * u64::from(s120) <= 400 * 120, "still fits: {s120}");
}
