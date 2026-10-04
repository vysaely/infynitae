//! X-9: presentation feedback for commits made while unobserved (B9.4), observed at the client.
//! Output: docs/foundation/evidence/phase-b/x9-presentation-output.txt
use b8_harness::{lockstep::Lockstep, server::*};

fn main() {
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    let qh = ls.qh.clone();
    let p = ls.globals.bind(&qh, 1..=1, ()).expect("wp_presentation");
    ls.probe.presentation = Some(p);
    ls.probe.feedback_seq = Some(0);
    ls.server.pending_anchors.push_back((100, 100));
    let w = ls.probe.create_toplevel(&qh, "W", (400, 300), 0, false); // commit #1 (first buffer) requests feedback
    ls.pump();
    ls.run_until(20_000);
    let t_off = ls.server.now_us;
    ls.set_camera(0, (10_000, 0)); // W unobserved
    ls.run_until(t_off + 10_000);
    ls.probe.draw(&qh, w); // commit #2 while unobserved
    ls.pump();
    ls.run_until(t_off + 20_000);
    ls.probe.draw(&qh, w); // commit #3 while unobserved: supersedes #2
    ls.pump();
    ls.run_until(t_off + 100_000);
    let t_on = ls.server.now_us;
    ls.set_camera(0, (0, 0)); // W observed again
    ls.run_until(t_on + 20_000);
    for (t, s) in &ls.probe.log {
        if s.contains("presentation") || s.contains("enter") || s.contains("leave") {
            println!("t={:>8.3}ms client {s}", *t as f64 / 1e3);
        }
    }
    let ev = |n: u32, what: &str| ls.probe.log.iter().find(|(_, s)| s == &format!("presentation_feedback#{n}.{what}")).map(|(t, _)| *t);
    let ok1 = ev(1, "presented").map_or(false, |t| t < t_off);
    let ok2 = ev(2, "discarded").map_or(false, |t| t > t_off && t < t_on) && ev(2, "presented").is_none();
    let ok3 = ev(3, "presented").map_or(false, |t| t >= t_on) && ev(3, "discarded").is_none();
    println!("[{}] commit #1 (visible) presented while visible", if ok1 { "PASS" } else { "FAIL" });
    println!("[{}] commit #2 (unobserved, superseded by #3) discarded at supersede time, never presented", if ok2 { "PASS" } else { "FAIL" });
    println!("[{}] commit #3 (latest while unobserved) stays pending, then presented on re-entry: latency {:?} us", if ok3 { "PASS" } else { "FAIL" }, ev(3, "presented").map(|t| t - t_on));
}
