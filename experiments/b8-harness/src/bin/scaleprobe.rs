//! CP3A scale evidence: how real clients respond to preferred-scale changes (tiny 1/120 steps,
//! ordinary steps, a per-frame renegotiation storm). Real time, private socket, no host compositor.
//!
//! usage: scaleprobe <runtime_dir> -- <client cmd...>
//! Output recorded under docs/foundation/evidence/phase-c/cp3a-scaleprobe-output.txt

use std::{
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use b8_harness::server::*;
use smithay::reexports::{
    calloop::{
        generic::Generic,
        timer::{TimeoutAction, Timer},
        EventLoop, Interest, Mode, PostAction,
    },
    wayland_server::Display,
};
use smithay::wayland::socket::ListeningSocketSource;

fn cpu_ticks(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            let rest = s.rsplit_once(')')?.1;
            let f: Vec<&str> = rest.split_whitespace().collect();
            Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
        })
        .unwrap_or(0)
}

/// (seconds after map, scale) — ordinary steps, then a storm phase handled separately
const STEPS: &[(f64, f64)] = &[(1.0, 1.0), (2.5, 121.0 / 120.0), (4.0, 1.25), (5.5, 1.5), (7.0, 2.0), (8.5, 3.0), (10.0, 1.0)];
const STORM_START: f64 = 11.5;
const STORM_END: f64 = 12.5;
const END: f64 = 14.0;

struct Run {
    server: Server,
    t0: Instant,
    map_at: Option<u64>,
    step: usize,
    pid: u32,
    marks: Vec<(String, u64, u64)>, // (label, time, cpu ticks)
    storm_toggle: bool,
    storm_changes: u32,
    done: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sep = args.iter().position(|a| a == "--").expect("--");
    let runtime_dir = args[1].clone();
    let cmd = &args[sep + 1..];
    std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
    let mut event_loop: EventLoop<Run> = EventLoop::try_new().unwrap();
    let display: Display<Server> = Display::new().unwrap();
    let dh = display.handle();
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 60_000, camera: (0, 0) }];
    let mut server = Server::new(&dh, &outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    server.pending_anchors.push_back((100, 100));
    let socket = ListeningSocketSource::new_auto().unwrap();
    let name = socket.socket_name().to_string_lossy().to_string();
    event_loop.handle().insert_source(socket, |s, _, r| {
        let _ = r.server.dh.insert_client(s, Arc::new(ClientState::default()));
    }).unwrap();
    event_loop.handle().insert_source(Generic::new(display, Interest::READ, Mode::Level), |_, d, r| {
        // SAFETY: the display outlives the event loop dispatch (standard Smithay idiom)
        unsafe {
            let _ = d.get_mut().dispatch_clients(&mut r.server);
        }
        Ok(PostAction::Continue)
    }).unwrap();
    let child = Command::new(&cmd[0]).args(&cmd[1..]).env("WAYLAND_DISPLAY", &name).env("XDG_RUNTIME_DIR", &runtime_dir).env_remove("DISPLAY").env_remove("HYPRLAND_INSTANCE_SIGNATURE").spawn().expect("spawn");
    let pid = child.id();
    println!("# client {cmd:?} pid {pid}");
    let mut run = Run { server, t0: Instant::now(), map_at: None, step: 0, pid, marks: vec![], storm_toggle: false, storm_changes: 0, done: false };
    let period = Duration::from_nanos(16_666_667);
    let start = run.t0;
    let mut n = 0u32;
    event_loop.handle().insert_source(Timer::from_deadline(start + period), move |_, _, r| {
        n += 1;
        let now = r.t0.elapsed().as_micros() as u64;
        r.server.now_us = now;
        if r.map_at.is_none() && r.server.elements.first().map(|e| e.mapped).unwrap_or(false) {
            r.map_at = Some(now);
        }
        if let Some(m) = r.map_at {
            let rel = (now - m) as f64 / 1e6;
            if r.step < STEPS.len() && rel >= STEPS[r.step].0 {
                let s = STEPS[r.step].1;
                r.marks.push((format!("scale -> {:.5} ({}/120)", s, (s * 120.0).round()), now, cpu_ticks(r.pid)));
                if !r.server.elements.is_empty() {
                    r.server.set_element_scale(0, s);
                }
                r.step += 1;
            }
            if rel >= STORM_START && rel < STORM_END {
                if r.storm_changes == 0 {
                    r.marks.push(("STORM start (scale changes every frame, 1.25 <-> 1.5)".into(), now, cpu_ticks(r.pid)));
                }
                r.storm_toggle = !r.storm_toggle;
                if !r.server.elements.is_empty() {
                    r.server.set_element_scale(0, if r.storm_toggle { 1.5 } else { 1.25 });
                }
                r.storm_changes += 1;
            }
            if rel >= STORM_END && !r.marks.iter().any(|(l, _, _)| l.starts_with("STORM end")) {
                r.marks.push((format!("STORM end ({} changes)", r.storm_changes), now, cpu_ticks(r.pid)));
            }
            if rel >= END {
                r.marks.push(("END".into(), now, cpu_ticks(r.pid)));
                r.done = true;
            }
        }
        r.server.vblank(0);
        let _ = r.server.dh.flush_clients();
        TimeoutAction::ToInstant(start + period * (n + 1))
    }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    while !run.done && Instant::now() < deadline {
        event_loop.dispatch(Some(Duration::from_millis(2)), &mut run).unwrap();
        let _ = run.server.dh.flush_clients();
    }
    let mut child = child;
    let _ = child.kill();
    let _ = child.wait();

    println!("mapped at {:?} ms", run.map_at.map(|t| t as f64 / 1e3));
    for w in run.marks.windows(2) {
        let (l, t, c) = &w[0];
        let (_, t2, c2) = &w[1];
        let sizes: Vec<(i32, i32)> = {
            let mut v: Vec<(i32, i32)> = run.server.commit_sizes.iter().filter(|(ct, _, _, _)| *ct >= *t && *ct < *t2).map(|(_, _, _, s)| *s).collect();
            v.dedup();
            v
        };
        let commits = run.server.commit_sizes.iter().filter(|(ct, _, _, _)| *ct >= *t && *ct < *t2).count();
        let cpu = 100.0 * ((c2 - c) as f64 / 100.0) / ((t2 - t) as f64 / 1e6);
        let first_new = run.server.commit_sizes.iter().filter(|(ct, _, _, _)| *ct >= *t && *ct < *t2).map(|(ct, _, _, s)| (*ct - *t, *s)).next();
        println!("{:<55} for {:>5.2}s: commits={:<4} distinct buffer sizes (in order)={:?} first commit after +{:?}us cpu={:.1}%", l, (t2 - t) as f64 / 1e6, commits, sizes, first_new.map(|x| x.0), cpu);
    }
}
