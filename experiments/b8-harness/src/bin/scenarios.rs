//! Phase B checkpoint 2 protocol scenarios (lockstep, deterministic, virtual time).
//! Output: docs/archive/evidence/phase-b/b8-scenarios-output.txt

use std::time::Duration;

use b8_harness::{lockstep::Lockstep, server::*};

const MS: u64 = 1000;

fn outs_ab(cam_a: (i32, i32), cam_b: (i32, i32)) -> [OutCfg; 2] {
    [
        OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: cam_a },
        OutCfg { name: "B", mode: (3840, 2160), scale: 1.5, refresh_mhz: 144_000, camera: cam_b },
    ]
}

fn pol(scale: ScalePolicy, frames: FramePolicy, clock: ClockPolicy, suspend: SuspendPolicy) -> Policy {
    Policy { scale, frames, clock, suspend }
}

fn header(s: &str) {
    println!("\n{}\n{s}\n{}", "=".repeat(100), "=".repeat(100));
}

fn count(ls: &Lockstep, t0: u64, t1: u64, needle: &str) -> usize {
    ls.probe.log.iter().filter(|(t, s)| *t >= t0 && *t < t1 && s.contains(needle)).count()
}

fn dones(ls: &Lockstep, label: &str, t0: u64, t1: u64) -> Vec<u64> {
    ls.probe.log.iter().filter(|(t, s)| *t >= t0 && *t < t1 && s.starts_with(&format!("{label}: wl_callback.done"))).map(|(t, _)| *t).collect()
}

fn max_gap(ts: &[u64]) -> u64 {
    ts.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0)
}

/// SC1 — mandatory B8 worked example, observed at the client.
fn sc1() {
    header("SC1  B8 worked example: W on A(1920x1080@1.0,165Hz) and B(3840x2160@1.5,144Hz); A pans away; B pans away; A returns.\n     Policy: scale=Max, frames=Starve, clock=HighestRefresh, suspend=Never");
    let mut ls = Lockstep::new(&outs_ab((0, 0), (600, 0)), pol(ScalePolicy::Max, FramePolicy::Starve, ClockPolicy::HighestRefresh, SuspendPolicy::Never));
    ls.server.pending_anchors.push_back((1000, 300));
    let qh = ls.qh.clone();
    ls.probe.create_toplevel(&qh, "W", (800, 600), 20, true);
    ls.pump();
    ls.run_until(50 * MS);
    let t_a_pan = ls.server.now_us;
    ls.pan(0, (2000, 0), 40); // A pans right until W leaves A
    let t_a_gone = ls.server.now_us;
    ls.run_until(t_a_gone + 50 * MS);
    let t_b_pan = ls.server.now_us;
    ls.pan(1, (4000, 0), 40); // B pans away: W invisible everywhere
    let t_b_gone = ls.server.now_us;
    ls.run_until(t_b_gone + 200 * MS);
    let t_return = ls.server.now_us;
    ls.set_camera(0, (0, 0)); // A jumps back
    ls.run_until(t_return + 50 * MS);

    // print the full merged trace, but collapse long runs of wl_callback.done
    let mut last_done = false;
    let mut run = 0;
    for l in ls.merged_since(0) {
        let is_done = l.contains("wl_callback.done");
        if is_done && last_done {
            run += 1;
            continue;
        }
        if run > 0 {
            println!("                     … {run} more wl_callback.done collapsed");
            run = 0;
        }
        println!("{l}");
        last_done = is_done;
    }
    if run > 0 {
        println!("                     … {run} more wl_callback.done collapsed");
    }
    let end = ls.server.now_us + 1;
    println!("\n-- SC1 summary --");
    println!("phase                       interval(ms)          done/s  configures  enter  leave  pref_scale");
    for (name, a, b) in [
        ("both visible", 0, t_a_pan),
        ("A panning away", t_a_pan, t_a_gone),
        ("B only", t_a_gone, t_b_pan),
        ("B panning away", t_b_pan, t_b_gone),
        ("off-screen (starve)", t_b_gone, t_return),
        ("A returned", t_return, end),
    ] {
        let d = dones(&ls, "W", a, b).len();
        let secs = (b - a) as f64 / 1e6;
        println!(
            "{:<26} {:>8.1}-{:<8.1} {:>9.1} {:>10} {:>6} {:>6} {:>10}",
            name,
            a as f64 / 1e3,
            b as f64 / 1e3,
            d as f64 / secs,
            count(&ls, a, b, "xdg_toplevel.configure"),
            count(&ls, a, b, "wl_surface.enter"),
            count(&ls, a, b, "wl_surface.leave"),
            count(&ls, a, b, "preferred_scale")
        );
    }
    let first_after = dones(&ls, "W", t_return, end).first().copied();
    println!("re-entry: camera returned at {:.3}ms, first wl_callback.done at {:?}ms -> latency {:?}us (A period 6061us)",
        t_return as f64 / 1e3, first_after.map(|t| t as f64 / 1e3), first_after.map(|t| t - t_return));
    println!("buffer allocations (time, w, h): {:?}", ls.probe.surfaces[0].allocs);
}

/// SC2 — preferred-scale policy comparison under pans that change membership and overlap areas.
fn sc2() {
    header("SC2  Preferred-scale policy comparison (W content 800x600 + 20px shadow; anchor (1000,300))\n     choreography: (1) B sweeps across W while A shows W fully; (2) A shows W partially, B oscillates around the area crossover;\n     (3) B oscillates across its viewport edge (membership flapping)");
    println!("{:<12} {:>9} {:>9} {:>9} {:>11} {:>14} {:>16} {:>16}", "policy", "phase1", "phase2", "phase3", "allocs", "alloc MB", "ms on B @<1.5", "ms on A @>1.0");
    for policy in [ScalePolicy::Max, ScalePolicy::MaxGesture, ScalePolicy::Min, ScalePolicy::Primary, ScalePolicy::LargestArea] {
        let mut ls = Lockstep::new(&outs_ab((0, 0), (-3000, 0)), pol(policy, FramePolicy::Starve, ClockPolicy::HighestRefresh, SuspendPolicy::Never));
        ls.probe.record_frames = false;
        ls.server.quiet = true;
        ls.server.pending_anchors.push_back((1000, 300));
        let qh = ls.qh.clone();
        ls.probe.create_toplevel(&qh, "W", (800, 600), 20, true);
        ls.pump();
        ls.run_until(20 * MS);
        // exposure accounting: sample every vblank of either output
        let mut blur_b_us = 0u64; // W visible on B while its buffer scale < 1.5 (upsampled on B)
        let mut down_a_us = 0u64; // W visible on A while buffer scale > 1.0 (downsampled on A)
        let mut last_t = ls.server.now_us;
        let account = |ls: &Lockstep, blur_b: &mut u64, down_a: &mut u64, last: &mut u64| {
            let now = ls.server.now_us;
            let dt = now - *last;
            *last = now;
            let e = &ls.server.elements[0];
            let bufscale = ls.probe.surfaces[0].scale120 as f64 / 120.0;
            if e.overlap[1].is_some() && bufscale < 1.5 {
                *blur_b += dt;
            }
            if e.overlap[0].is_some() && bufscale > 1.0 {
                *down_a += dt;
            }
        };
        let pref = |ls: &Lockstep| count(ls, 0, u64::MAX, "preferred_scale");
        let p0 = pref(&ls);
        // phase 1: B sweeps from far left to far right across W (W fully on A all along)
        ls.gesture(true);
        for x in (-3000..=3000).step_by(30) {
            ls.set_camera(1, (x, 0));
            let t = ls.server.now_us + 6944;
            ls.run_until(t);
            account(&ls, &mut blur_b_us, &mut down_a_us, &mut last_t);
        }
        ls.gesture(false);
        let p1 = pref(&ls);
        // phase 2: A shows 220 px of W's 840; B oscillates between showing 300 px and 120 px:
        // the visible-area ordering of A and B flips on every swing (area crossover)
        ls.set_camera(0, (1200 - 1920, 0)); // A viewport right edge at x=1200 -> shows 980..1200
        ls.gesture(true);
        for i in 0..20 {
            let x = if i % 2 == 0 { 1280 - 2560 } else { 1100 - 2560 };
            for _ in 0..5 {
                ls.set_camera(1, (x, 0));
                let t = ls.server.now_us + 6944;
                ls.run_until(t);
                account(&ls, &mut blur_b_us, &mut down_a_us, &mut last_t);
            }
        }
        ls.gesture(false);
        let p2 = pref(&ls);
        // phase 3: B oscillates across its right viewport edge at W's left edge (enter/leave B flapping)
        ls.set_camera(0, (0, 0));
        ls.gesture(true);
        for i in 0..20 {
            let x = if i % 2 == 0 { 980 - 2560 + 15 } else { 980 - 2560 - 15 };
            ls.set_camera(1, (x, 0));
            let t = ls.server.now_us + 6944 * 3;
            ls.run_until(t);
            account(&ls, &mut blur_b_us, &mut down_a_us, &mut last_t);
        }
        ls.gesture(false);
        let p3 = pref(&ls);
        let allocs = &ls.probe.surfaces[0].allocs;
        let mb: f64 = allocs.iter().map(|(_, w, h)| (*w as f64) * (*h as f64) * 4.0 / 1e6).sum();
        println!(
            "{:<12} {:>9} {:>9} {:>9} {:>11} {:>14.1} {:>16.1} {:>16.1}",
            format!("{policy:?}"),
            p1 - p0,
            p2 - p1,
            p3 - p2,
            allocs.len(),
            mb,
            blur_b_us as f64 / 1e3,
            down_a_us as f64 / 1e3
        );
    }
    println!("(columns phase1..3 = wp_fractional_scale_v1.preferred_scale events received by the client in that phase, including the gesture-end evaluation;\n each phase is one camera gesture. Exposure columns are summed over all three phases.)");
}

/// SC3 — frame-callback liveness matrix.
fn sc3() {
    header("SC3  Frame-callback liveness: pending callbacks across invisibility, re-entry, multi-refresh visibility, output handover");
    // (a)/(b)/(c): off-screen with Starve and Throttle; then re-entry on A
    for (name, frames) in [
        ("Starve", FramePolicy::Starve),
        ("Throttle(100ms)", FramePolicy::Throttle(Duration::from_millis(100))),
        ("Throttle(1000ms)", FramePolicy::Throttle(Duration::from_millis(1000))),
    ] {
        for frame_driven in [true, false] {
            let mut ls = Lockstep::new(&outs_ab((0, 0), (-5000, 0)), pol(ScalePolicy::Max, frames, ClockPolicy::HighestRefresh, SuspendPolicy::Never));
            ls.probe.record_frames = true;
            ls.server.pending_anchors.push_back((1000, 300));
            let qh = ls.qh.clone();
            let idx = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, frame_driven);
            ls.pump();
            ls.run_until(30 * MS);
            let t_off = ls.server.now_us;
            ls.set_camera(0, (5000, 0)); // off-screen everywhere
            if !frame_driven {
                // an idle client that wakes up WHILE off-screen and requests exactly one callback
                ls.run_until(t_off + 500 * MS);
                ls.probe.surfaces[idx].frame_driven = true;
                ls.probe.draw(&qh, idx);
                ls.probe.surfaces[idx].frame_driven = false;
                ls.pump();
            }
            ls.run_until(t_off + 3000 * MS);
            let t_on = ls.server.now_us;
            let pending = ls.probe.surfaces[idx].frame_pending;
            ls.set_camera(0, (0, 0));
            ls.run_until(t_on + 100 * MS);
            let off = dones(&ls, "W", t_off + 1, t_on);
            let on = dones(&ls, "W", t_on, t_on + 100 * MS + 1);
            println!(
                "{:<17} client={:<13} off-screen 3000ms: dones={:<3} (max gap {:>7.1}ms) pending-at-return={:<5} | re-entry first done after {:>6}us; dones in next 100ms={}",
                name,
                if frame_driven { "frame-driven" } else { "one-shot@off" },
                off.len(),
                max_gap(&off) as f64 / 1e3,
                pending,
                on.first().map(|t| (t - t_on).to_string()).unwrap_or("none".into()),
                on.len()
            );
        }
    }
    // (d) simultaneous visibility on A(165) and B(144): done rate under each clock policy
    for clock in [ClockPolicy::HighestRefresh, ClockPolicy::Primary] {
        let mut ls = Lockstep::new(&outs_ab((0, 0), (600, 0)), pol(ScalePolicy::Max, FramePolicy::Starve, clock, SuspendPolicy::Never));
        ls.server.pending_anchors.push_back((1000, 300));
        let qh = ls.qh.clone();
        ls.probe.create_toplevel(&qh, "W", (800, 600), 20, true);
        ls.pump();
        ls.run_until(1000 * MS);
        let d = dones(&ls, "W", 0, 1000 * MS + 1);
        println!("(d) visible on A and B, clock={:<15?}: {} dones in 1s (max gap {:.3}ms)", clock, d.len(), max_gap(&d[1..]) as f64 / 1e3);
    }
    // (d2) A shows a sliver of W, B shows all of it: Primary (2x area rule) picks B, HighestRefresh picks A
    for clock in [ClockPolicy::HighestRefresh, ClockPolicy::Primary] {
        let mut ls = Lockstep::new(&outs_ab((1000 - 1920, 0), (600, 0)), pol(ScalePolicy::Max, FramePolicy::Starve, clock, SuspendPolicy::Never));
        ls.server.pending_anchors.push_back((1000, 300));
        let qh = ls.qh.clone();
        ls.probe.create_toplevel(&qh, "W", (800, 600), 20, true);
        ls.pump();
        ls.run_until(1000 * MS);
        let d = dones(&ls, "W", 0, 1000 * MS + 1);
        println!("(d2) A shows 20px sliver, B shows all, clock={:<15?}: {} dones in 1s", clock, d.len());
    }
    // (e) handover A -> B with a callback pending at the moment A stops showing W
    let mut ls = Lockstep::new(&outs_ab((0, 0), (600, 0)), pol(ScalePolicy::Max, FramePolicy::Starve, ClockPolicy::HighestRefresh, SuspendPolicy::Never));
    ls.server.pending_anchors.push_back((1000, 300));
    let qh = ls.qh.clone();
    ls.probe.create_toplevel(&qh, "W", (800, 600), 20, true);
    ls.pump();
    ls.run_until(100 * MS);
    let t0 = ls.server.now_us;
    ls.set_camera(0, (5000, 0)); // A jumps away; W remains on B
    ls.run_until(t0 + 200 * MS);
    let d = dones(&ls, "W", t0 - 50 * MS, t0 + 200 * MS);
    let first_after = d.iter().find(|t| **t > t0).copied();
    println!(
        "(e) handover A->B at {:.3}ms: first done after handover at +{:?}us (B period 6944us); max gap across handover {:.3}ms; dones/s after = {:.1}",
        t0 as f64 / 1e3,
        first_after.map(|t| t - t0),
        max_gap(&d) as f64 / 1e3,
        dones(&ls, "W", t0 + 1, t0 + 200 * MS).len() as f64 / 0.2
    );
}

/// SC4 — suspension: (a) protocol consequence for one window; (b) configure amplification during rapid pans over many windows.
fn sc4() {
    header("SC4  xdg_toplevel `suspended`: protocol consequence and configure amplification under rapid pans");
    // (a) one window, Immediate policy, camera crosses its edge 6 times
    let mut ls = Lockstep::new(&outs_ab((0, 0), (-9000, 0)), pol(ScalePolicy::Max, FramePolicy::Starve, ClockPolicy::HighestRefresh, SuspendPolicy::Immediate));
    ls.probe.record_frames = false;
    ls.server.pending_anchors.push_back((1000, 300));
    let qh = ls.qh.clone();
    ls.probe.create_toplevel(&qh, "W", (800, 600), 20, true);
    ls.pump();
    ls.run_until(20 * MS);
    let t0 = ls.server.now_us;
    for i in 0..6 {
        ls.set_camera(0, (if i % 2 == 0 { 1830 } else { 1810 }, 0)); // W's bbox right edge is x=1820
        let t = ls.server.now_us + 10 * MS;
        ls.run_until(t);
    }
    println!("(a) Immediate suspension, 6 edge crossings (pan of 20px): client trace:");
    for l in ls.merged_since(t0) {
        if l.contains("configure") || l.contains("suspended") || l.contains("enter") || l.contains("leave") {
            println!("    {l}");
        }
    }

    // (b) amplification: 48 windows on a 8x6 grid (800x600 + 20 shadow, spacing 1000x800), one output 2560x1440@144;
    //     camera pans horizontally across the whole grid at several speeds.
    println!("\n(b) 48 windows, 8x6 grid, output 2560x1440@1.5 144Hz, horizontal pan across 8000px, client-observed traffic:");
    println!("{:<20} {:>10} {:>10} {:>12} {:>12} {:>12} {:>12} {:>16}", "policy", "speed px/s", "pan ms", "conf in pan", "conf/s", "enter+leave", "e+l /s", "conf incl +3s");
    for suspend in [SuspendPolicy::Never, SuspendPolicy::Immediate, SuspendPolicy::Debounce(Duration::from_millis(250)), SuspendPolicy::Debounce(Duration::from_millis(1000))] {
        for speed in [2_000i32, 8_000, 30_000] {
            let outs = [OutCfg { name: "B", mode: (3840, 2160), scale: 1.5, refresh_mhz: 144_000, camera: (-3000, 0) }];
            let mut ls = Lockstep::new(&outs, pol(ScalePolicy::Max, FramePolicy::Starve, ClockPolicy::HighestRefresh, suspend));
            ls.probe.record_frames = false;
            ls.server.quiet = true;
            let qh = ls.qh.clone();
            for r in 0..6 {
                for c in 0..8 {
                    ls.server.pending_anchors.push_back((c * 1000, r * 800 - 1000));
                    ls.probe.create_toplevel(&qh, &format!("W{r}{c}"), (800, 600), 20, false);
                }
            }
            ls.pump();
            ls.run_until(2000 * MS); // settle; debounce policies suspend the off-screen ones here
            let t0 = ls.server.now_us;
            let step = (speed as f64 / 144.0).round() as i32;
            ls.pan(0, (8000, 0), step);
            let t1 = ls.server.now_us;
            ls.run_until(t1 + 3000 * MS); // settle: delayed (debounced) suspensions land here
            let t2 = ls.server.now_us;
            let conf_total = count(&ls, t0, t2 + 1, "xdg_toplevel.configure");
            let secs = (t1 - t0) as f64 / 1e6;
            let conf = count(&ls, t0, t1 + 1, "xdg_toplevel.configure");
            let el = count(&ls, t0, t1 + 1, "wl_surface.enter") + count(&ls, t0, t1 + 1, "wl_surface.leave");
            println!(
                "{:<20} {:>10} {:>10.0} {:>12} {:>12.1} {:>12} {:>12.1} {:>16}",
                format!("{suspend:?}"),
                speed,
                secs * 1e3,
                conf,
                conf as f64 / secs,
                el,
                el as f64 / secs,
                conf_total
            );
        }
    }
}

/// SC4(c) — scrubbing: camera oscillates back and forth across the grid.
fn sc4c() {
    println!("\n(c) same grid; camera scrubs back and forth 10x between x=0 and x=3000 at 8000 px/s, then 3s settle:");
    println!("{:<20} {:>12} {:>12} {:>16}", "policy", "conf total", "enter+leave", "conf per e+l");
    for suspend in [SuspendPolicy::Never, SuspendPolicy::Immediate, SuspendPolicy::Debounce(Duration::from_millis(250)), SuspendPolicy::Debounce(Duration::from_millis(1000))] {
        let outs = [OutCfg { name: "B", mode: (3840, 2160), scale: 1.5, refresh_mhz: 144_000, camera: (0, 0) }];
        let mut ls = Lockstep::new(&outs, pol(ScalePolicy::Max, FramePolicy::Starve, ClockPolicy::HighestRefresh, suspend));
        ls.probe.record_frames = false;
        ls.server.quiet = true;
        let qh = ls.qh.clone();
        for r in 0..6 {
            for c in 0..8 {
                ls.server.pending_anchors.push_back((c * 1000, r * 800 - 1000));
                ls.probe.create_toplevel(&qh, &format!("W{r}{c}"), (800, 600), 20, false);
            }
        }
        ls.pump();
        ls.run_until(2000 * MS);
        let t0 = ls.server.now_us;
        let step = (8000.0f64 / 144.0).round() as i32;
        for i in 0..10 {
            ls.pan(0, (if i % 2 == 0 { 3000 } else { 0 }, 0), step);
        }
        let t1 = ls.server.now_us;
        ls.run_until(t1 + 3000 * MS);
        let t2 = ls.server.now_us;
        let conf = count(&ls, t0, t2 + 1, "xdg_toplevel.configure");
        let el = count(&ls, t0, t2 + 1, "wl_surface.enter") + count(&ls, t0, t2 + 1, "wl_surface.leave");
        println!("{:<20} {:>12} {:>12} {:>16.2}", format!("{suspend:?}"), conf, el, conf as f64 / el.max(1) as f64);
    }
}

/// X5 — EB-H1: popup enter/leave offset for CSD toplevel + CSD popup.
fn x5() {
    header("X5  EB-H1: popup wl_surface.enter/leave vs true popup geometry (toplevel shadow 20, popup shadow 10)");
    println!("toplevel anchor (1000,300) -> root surface origin (980,280). Popup content 200x100, shadow 10, anchor rect (100,50) in parent geometry.");
    println!("True popup surface rect in Canvas: x 1090..1310 (geometry origin 1100 minus popup shadow 10).");
    for (case, cam_x, truth) in [
        ("viewport right edge at x=1085 (popup truly INVISIBLE)", 1085 - 1920, "no enter expected"),
        ("viewport left edge at x=1305 (popup truly VISIBLE, 5px)", 1305, "enter expected"),
        ("viewport right edge at x=1095 (popup truly VISIBLE, 5px)", 1095 - 1920, "enter expected"),
    ] {
        let mut ls = Lockstep::new(&[OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) }], pol(ScalePolicy::Max, FramePolicy::Starve, ClockPolicy::HighestRefresh, SuspendPolicy::Never));
        ls.probe.record_frames = false;
        ls.server.pending_anchors.push_back((1000, 300));
        let qh = ls.qh.clone();
        let w = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, false);
        ls.pump();
        ls.run_until(10 * MS);
        ls.probe.create_popup(&qh, "P", w, (200, 100), 10, (100, 50, 1, 1));
        ls.pump();
        ls.run_until(20 * MS);
        let t0 = ls.server.now_us;
        ls.set_camera(0, (cam_x, 0));
        ls.run_until(t0 + 10 * MS);
        let popup_cfg: Vec<_> = ls.probe.log.iter().filter(|(_, s)| s.starts_with("P: xdg_popup.configure")).map(|(_, s)| s.clone()).collect();
        let entered = ls.probe.log.iter().filter(|(t, s)| *t >= t0 && s.starts_with("P: wl_surface.")).map(|(_, s)| s.clone()).collect::<Vec<_>>();
        // net membership after the move
        let mut on = ls.probe.log.iter().filter(|(_, s)| s.starts_with("P: wl_surface.enter")).count() as i32
            - ls.probe.log.iter().filter(|(_, s)| s.starts_with("P: wl_surface.leave")).count() as i32;
        on = on.max(0);
        println!("- {case}: {truth}; {:?}; popup events after move: {:?}; popup currently entered on A: {}", popup_cfg, entered, on > 0);
    }
}

fn main() {
    println!("b8-harness scenarios — smithay 0.7.0, wl_compositor v6 (CompositorState::new_v6), virtual time");
    sc1();
    sc2();
    sc3();
    sc4();
    sc4c();
    x5();
}
