//! Phase D 1R (PD-F2-17): Foundation 01 maximize policy. xdg-shell: after set_maximized "the compositor
//! will respond by emitting a configure event. Whether this configure actually sets the window
//! maximized is subject to compositor policies." Policy under test: decline — answer set_maximized and
//! unset_maximized with exactly one configure that does not carry `maximized`; never move a camera;
//! never change Stage geometry.
//! NEG: the record-only W-SM-13 override (no answer) — the client waits for a configure that never comes.
//! Output: docs/foundation/evidence/phase-d/xmaximize-output.txt
use b8_harness::{lockstep::Lockstep, server::*};

fn run(policy: bool) -> bool {
    println!("=== maximize policy {}", if policy { "ON (decline with a configure)" } else { "OFF (record-only override: NEG control)" });
    let outs = [
        OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) },
        OutCfg { name: "B", mode: (3840, 2160), scale: 1.5, refresh_mhz: 144_000, camera: (600, 0) },
    ];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    ls.server.maximize_policy = policy;
    let qh = ls.qh.clone();
    ls.server.pending_anchors.push_back((1000, 300));
    let w = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, false);
    ls.pump();
    ls.run_until(20_000);
    let anchor0 = ls.server.elements[0].anchor;
    let cams0: Vec<(i32, i32)> = ls.server.outputs.iter().map(|o| o.camera).collect();
    let mut ok = true;
    for (req, label) in [(true, "set_maximized"), (false, "unset_maximized")] {
        let n0 = ls.probe.surfaces[w].toplevel_configures;
        let t0 = ls.server.now_us;
        let tl = ls.probe.surfaces[w].toplevel.clone().unwrap();
        if req {
            tl.set_maximized();
        } else {
            tl.unset_maximized();
        }
        ls.pump();
        let t = ls.server.now_us + 2000;
        ls.run_until(t);
        let n = ls.probe.surfaces[w].toplevel_configures - n0;
        let lines: Vec<&String> = ls.probe.log.iter().filter(|(tt, s)| *tt >= t0 && s.contains("xdg_toplevel.configure")).map(|(_, s)| s).collect();
        let granted = lines.iter().any(|s| s.contains("maximized"));
        let anchor = ls.server.elements[0].anchor;
        let cams: Vec<(i32, i32)> = ls.server.outputs.iter().map(|o| o.camera).collect();
        let step_ok = n == 1 && !granted && anchor == anchor0 && cams == cams0;
        ok &= step_ok;
        for l in &lines {
            println!("     client: {l}");
        }
        println!(
            "  {label:<16} configures answered={n} (want 1) | `maximized` granted={granted} (want false) | anchor {anchor:?} (was {anchor0:?}) | cameras {cams:?} (were {cams0:?}) | {}",
            if step_ok { "ok" } else { "FAIL" }
        );
    }
    println!("  RESULT: {}\n", if ok { "PASS" } else { "FAIL" });
    ok
}

/// PD-F2-10: set_fullscreen / unset_fullscreen "the compositor will respond by emitting a configure
/// event". Policy under test: focused -> grant (configure with `fullscreen` and the output size);
/// not focused -> decline NOW with a configure (no deferred pending request); unset -> configure.
/// NEG: the sealed C5.2 deferral ("record it and decide when W next gains focus") = no answer.
fn run_fs(policy: bool) -> bool {
    println!("=== fullscreen policy {}", if policy { "ON (answer every request)" } else { "OFF (sealed deferral: NEG control)" });
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    ls.server.fullscreen_policy = policy;
    let qh = ls.qh.clone();
    ls.server.pending_anchors.push_back((1000, 300));
    let w = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, false);
    ls.pump();
    ls.run_until(20_000);
    let anchor0 = ls.server.elements[0].anchor;
    let cam0 = ls.server.outputs[0].camera;
    let id = ls.server.elements[0].id;
    let mut ok = true;
    for (label, focused, set, want_fs) in [("set_fullscreen while NOT focused", None, true, false), ("set_fullscreen while focused", Some(id), true, true), ("unset_fullscreen", Some(id), false, false)] {
        ls.server.focused = focused;
        let n0 = ls.probe.surfaces[w].toplevel_configures;
        let t0 = ls.server.now_us;
        let tl = ls.probe.surfaces[w].toplevel.clone().unwrap();
        if set {
            tl.set_fullscreen(None);
        } else {
            tl.unset_fullscreen();
        }
        ls.pump();
        let t = ls.server.now_us + 2000;
        ls.run_until(t);
        let n = ls.probe.surfaces[w].toplevel_configures - n0;
        let lines: Vec<&String> = ls.probe.log.iter().filter(|(tt, s)| *tt >= t0 && s.contains("xdg_toplevel.configure")).map(|(_, s)| s).collect();
        let fs = lines.iter().any(|s| s.contains("fullscreen"));
        let step_ok = n == 1 && fs == want_fs && ls.server.elements[0].anchor == anchor0 && ls.server.outputs[0].camera == cam0;
        ok &= step_ok;
        for l in &lines {
            println!("     client: {l}");
        }
        println!("  {label:<34} configures answered={n} (want 1) | `fullscreen` granted={fs} (want {want_fs}) | anchor/camera unchanged={} | {}", ls.server.elements[0].anchor == anchor0 && ls.server.outputs[0].camera == cam0, if step_ok { "ok" } else { "FAIL" });
    }
    println!("  RESULT: {}\n", if ok { "PASS" } else { "FAIL" });
    ok
}

fn main() {
    println!("xmaximize — xdg_toplevel maximize/fullscreen answer policy; smithay =0.7.0, lockstep\n");
    let on = run(true);
    let off = run(false);
    let fs_on = run_fs(true);
    let fs_off = run_fs(false);
    println!("maximize ON: {} (expected PASS) | NEG maximize OFF: {} (expected FAIL)", if on { "PASS" } else { "FAIL" }, if off { "PASS" } else { "FAIL" });
    println!("fullscreen ON: {} (expected PASS) | NEG sealed deferral: {} (expected FAIL)", if fs_on { "PASS" } else { "FAIL" }, if fs_off { "PASS" } else { "FAIL" });
    println!("ALL AS EXPECTED: {}", on && !off && fs_on && !fs_off);
}
