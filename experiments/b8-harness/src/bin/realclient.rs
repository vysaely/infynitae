//! X-2: real clients off-screen under a frame-callback policy, in real time.
//!
//! Isolation: private XDG_RUNTIME_DIR and socket (passed on the command line), DISPLAY and
//! HYPRLAND_INSTANCE_SIGNATURE removed from the client environment. No host compositor involved;
//! the harness never renders (one virtual 1920x1080@1.0 165Hz output).
//!
//! usage: realclient <runtime_dir> <policy: starve|throttle:<ms>> <visible_s> <offscreen_s> <return_s> -- <client cmd...>

use std::{
    os::unix::fs::MetadataExt,
    process::{Child, Command},
    sync::Arc,
    time::{Duration, Instant},
};

use b8_harness::server::*;
use smithay::{
    backend::allocator::{Format, Fourcc, Modifier},
    reexports::{
        calloop::{
            generic::Generic,
            timer::{TimeoutAction, Timer},
            EventLoop, Interest, Mode, PostAction,
        },
        wayland_server::Display,
    },
    utils::SERIAL_COUNTER,
    wayland::{dmabuf::DmabufFeedbackBuilder, socket::ListeningSocketSource},
};

struct Run {
    kick: bool,
    server: Server,
    t0: Instant,
    child: Child,
    child_pid: u32,
    phase: u8, // 0 visible, 1 offscreen, 2 returned, 3 done
    t_off: u64,
    t_ret: u64,
    t_end: u64,
    pings: Vec<u64>,
    cpu_marks: Vec<(u64, u64)>, // (time us, utime+stime ticks)
    exited_at: Option<u64>,
    next_ping: Option<u64>,
}

fn cpu_ticks(pid: u32) -> u64 {
    // utime + stime of the process and all its threads (fields 14 and 15 of /proc/pid/stat)
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            let rest = s.rsplit_once(')')?.1;
            let f: Vec<&str> = rest.split_whitespace().collect();
            Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
        })
        .unwrap_or(0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sep = args.iter().position(|a| a == "--").expect("missing --");
    let runtime_dir = &args[1];
    let policy = &args[2];
    let vis_s: f64 = args[3].parse().unwrap();
    let off_s: f64 = args[4].parse().unwrap();
    let ret_s: f64 = args[5].parse().unwrap();
    let cmd = &args[sep + 1..];
    // "+kick": when the compositor sends a protocol request that needs the client's main loop
    // (here: xdg_wm_base.ping), complete that client's withheld frame callbacks once.
    let kick = policy.ends_with("+kick");
    let base = policy.trim_end_matches("+kick");
    let frames = if base == "starve" {
        FramePolicy::Starve
    } else {
        let ms: u64 = base.strip_prefix("throttle:").expect("policy").parse().unwrap();
        FramePolicy::Throttle(Duration::from_millis(ms))
    };

    std::env::set_var("XDG_RUNTIME_DIR", runtime_dir);
    let mut event_loop: EventLoop<Run> = EventLoop::try_new().unwrap();
    let display: Display<Server> = Display::new().unwrap();
    let dh = display.handle();
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) }];
    let mut server = Server::new(&dh, &outs, Policy { scale: ScalePolicy::Max, frames, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    server.pending_anchors.push_back((100, 100));

    // linux-dmabuf v4 with the render node as main device; buffers are accepted, never imported.
    if let Ok(meta) = std::fs::metadata("/dev/dri/renderD128") {
        let formats: Vec<Format> = [Fourcc::Argb8888, Fourcc::Xrgb8888]
            .into_iter()
            .flat_map(|code| [Modifier::Linear, Modifier::Invalid].into_iter().map(move |modifier| Format { code, modifier }))
            .collect();
        let fb = DmabufFeedbackBuilder::new(meta.rdev() as libc::dev_t, formats).build().unwrap();
        let g = server.dmabuf_state.create_global_with_default_feedback::<Server>(&dh, &fb);
        server.dmabuf_global = Some(g);
    }

    let socket = ListeningSocketSource::new_auto().unwrap();
    let socket_name = socket.socket_name().to_string_lossy().to_string();
    event_loop
        .handle()
        .insert_source(socket, |stream, _, run| {
            let _ = run.server.dh.insert_client(stream, Arc::new(ClientState::default()));
        })
        .unwrap();
    event_loop
        .handle()
        .insert_source(Generic::new(display, Interest::READ, Mode::Level), |_, d, run| {
            // SAFETY: the display is not dropped while the event loop runs (standard Smithay idiom).
            unsafe {
                let _ = d.get_mut().dispatch_clients(&mut run.server);
            }
            Ok(PostAction::Continue)
        })
        .unwrap();

    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..])
        .env("WAYLAND_DISPLAY", &socket_name)
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env_remove("DISPLAY")
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .env_remove("HYPRLAND_CMD");
    let child = c.spawn().expect("spawn client");
    let child_pid = child.id();
    println!("# socket {runtime_dir}/{socket_name}; client pid {child_pid}: {cmd:?}; policy {policy}");

    let t_off = (vis_s * 1e6) as u64;
    let t_ret = t_off + (off_s * 1e6) as u64;
    let t_end = t_ret + (ret_s * 1e6) as u64;
    let mut run = Run { kick, server, t0: Instant::now(), child, child_pid, phase: 0, t_off, t_ret, t_end, pings: vec![], cpu_marks: vec![], exited_at: None, next_ping: Some(t_off + 1_000_000) };
    run.cpu_marks.push((0, cpu_ticks(child_pid)));

    let period = Duration::from_nanos(1_000_000_000_000 / 165_000);
    let start = run.t0;
    let mut vb = 0u64;
    event_loop
        .handle()
        .insert_source(Timer::from_deadline(start + period), move |_, _, run| {
            vb += 1;
            let now = run.t0.elapsed().as_micros() as u64;
            run.server.now_us = now;
            if run.phase == 0 && now >= run.t_off {
                run.phase = 1;
                run.cpu_marks.push((now, cpu_ticks(run.child_pid)));
                run.server.outputs[0].camera = (100_000, 0);
                run.server.refresh_visibility();
            }
            if run.phase == 1 {
                if let Some(tp) = run.next_ping {
                    if now >= tp {
                        if let Some(t) = run.server.elements.first().and_then(|e| e.window.toplevel().cloned()) {
                            let _ = t.client().send_ping(SERIAL_COUNTER.next_serial());
                            run.pings.push(now);
                            if run.kick {
                                let out = run.server.outputs[0].output.clone();
                                for ei in 0..run.server.elements.len() {
                                    if run.server.clock_output(ei).is_none() {
                                        let w = run.server.elements[ei].window.clone();
                                        w.send_frame(&out, Duration::from_micros(now), Some(Duration::ZERO), |_, _| None);
                                    }
                                }
                            }
                        }
                        run.next_ping = if run.pings.len() < 3 { Some(tp + 1_000_000) } else { None };
                    }
                }
                if now >= run.t_ret {
                    run.phase = 2;
                    run.cpu_marks.push((now, cpu_ticks(run.child_pid)));
                    run.server.outputs[0].camera = (0, 0);
                    run.server.refresh_visibility();
                }
            }
            if run.phase == 2 && now >= run.t_end {
                run.phase = 3;
                run.cpu_marks.push((now, cpu_ticks(run.child_pid)));
            }
            if run.exited_at.is_none() {
                if let Ok(Some(_)) = run.child.try_wait() {
                    run.exited_at = Some(now);
                }
            }
            run.server.vblank(0);
            let _ = run.server.dh.flush_clients();
            TimeoutAction::ToInstant(start + period * (vb as u32 + 1))
        })
        .unwrap();

    while run.phase != 3 {
        event_loop.dispatch(Some(Duration::from_millis(2)), &mut run).unwrap();
        let _ = run.server.dh.flush_clients();
    }
    let _ = run.child.kill();
    let _ = run.child.wait();

    // ---- report ----
    let tick = 100.0; // CLK_TCK
    let commits = &run.server.commits;
    let rate = |a: u64, b: u64| commits.iter().filter(|(t, _)| *t >= a && *t < b).count() as f64 / ((b - a) as f64 / 1e6);
    let cpu = |i: usize| {
        let (ta, ca) = run.cpu_marks[i];
        let (tb, cb) = run.cpu_marks[i + 1];
        100.0 * ((cb - ca) as f64 / tick) / ((tb - ta) as f64 / 1e6)
    };
    let mapped = !commits.is_empty();
    println!("mapped: {mapped}; first commit at {:?} ms", commits.first().map(|c| c.0 as f64 / 1e3));
    if run.cpu_marks.len() >= 4 {
        println!("phase          commits/s   client CPU %");
        println!("visible        {:>9.1}   {:>11.1}", rate(1_000_000.min(run.t_off), run.t_off), cpu(0));
        println!("off-screen     {:>9.1}   {:>11.1}", rate(run.t_off, run.t_ret), cpu(1));
        println!("returned       {:>9.1}   {:>11.1}", rate(run.t_ret, run.t_end), cpu(2));
    }
    let first_after = commits.iter().find(|(t, _)| *t >= run.t_ret).map(|c| c.0 - run.t_ret);
    println!("re-entry: first commit {:?} us after the camera returned", first_after);
    for (i, p) in run.pings.iter().enumerate() {
        let pong = run.server.pongs.iter().find(|t| **t >= *p).map(|t| t - p);
        println!("ping {} sent while off-screen at +{:.1}s: pong after {:?} us", i + 1, (*p - run.t_off) as f64 / 1e6, pong);
    }
    let pongs_after_return: Vec<u64> = run.server.pongs.iter().filter(|t| **t >= run.t_ret).map(|t| t - run.t_ret).collect();
    println!("pongs received after camera return (us after return): {:?}", pongs_after_return);
    println!("client exited during run: {:?}", run.exited_at.map(|t| t as f64 / 1e6));
}
