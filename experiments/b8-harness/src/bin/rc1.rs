//! RC-1 with a real toolkit: the keyboard-focused surface is destroyed by the client while keys are
//! held; focus moves to the client's other window. Records what the toolkit (GTK4) reports.
//! usage: rc1 <runtime_dir> -- <client cmd...>
use std::{process::Command, sync::Arc, time::{Duration, Instant}};
use b8_harness::{keys::KeyIntake, server::*};
use smithay::{
    reexports::{calloop::{generic::Generic, timer::{TimeoutAction, Timer}, EventLoop, Interest, Mode, PostAction}, wayland_server::Display},
    utils::SERIAL_COUNTER,
    wayland::socket::ListeningSocketSource,
};

const KEY_A: u32 = 30;
const KEY_B: u32 = 48;
const KEY_LEFTSHIFT: u32 = 42;

struct Run { server: Server, ix: KeyIntake, t0: Instant, step: usize, t_step: u64, done: bool, notes: Vec<String> }

fn focus(r: &mut Run, el: Option<usize>, why: &str) {
    let surf = el.and_then(|e| r.server.elements.get(e)).and_then(|e| e.window.toplevel().map(|t| t.wl_surface().clone()));
    let kb = r.server.seat.get_keyboard().unwrap();
    kb.set_focus(&mut r.server, surf, SERIAL_COUNTER.next_serial());
    r.notes.push(format!("t={:.3}s compositor: keyboard focus -> {:?} ({why})", r.server.now_us as f64 / 1e6, el.map(|e| r.server.elements[e].id)));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sep = args.iter().position(|a| a == "--").unwrap();
    let rt = args[1].clone();
    std::env::set_var("XDG_RUNTIME_DIR", &rt);
    let mut el: EventLoop<Run> = EventLoop::try_new().unwrap();
    let display: Display<Server> = Display::new().unwrap();
    let dh = display.handle();
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 60_000, camera: (0, 0) }];
    let mut server = Server::new(&dh, &outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    server.pending_anchors.extend([(100, 100), (700, 100)]);
    let socket = ListeningSocketSource::new_auto().unwrap();
    let name = socket.socket_name().to_string_lossy().to_string();
    el.handle().insert_source(socket, |s, _, r| { let _ = r.server.dh.insert_client(s, Arc::new(ClientState::default())); }).unwrap();
    el.handle().insert_source(Generic::new(display, Interest::READ, Mode::Level), |_, d, r| {
        // SAFETY: display outlives dispatch (standard Smithay idiom)
        unsafe { let _ = d.get_mut().dispatch_clients(&mut r.server); }
        Ok(PostAction::Continue)
    }).unwrap();
    let cmd = &args[sep + 1..];
    let mut child = Command::new(&cmd[0]).args(&cmd[1..]).env("WAYLAND_DISPLAY", &name).env("XDG_RUNTIME_DIR", &rt).env("WAYLAND_DEBUG", "client").env_remove("DISPLAY").env_remove("HYPRLAND_INSTANCE_SIGNATURE").spawn().unwrap();
    let mut run = Run { server, ix: KeyIntake::new(vec![], true), t0: Instant::now(), step: 0, t_step: 0, done: false, notes: vec![] };
    let start = run.t0;
    let period = Duration::from_millis(16);
    let mut n = 0u32;
    el.handle().insert_source(Timer::from_deadline(start + period), move |_, _, r| {
        n += 1;
        let now = r.t0.elapsed().as_micros() as u64;
        r.server.now_us = now;
        let since = now.saturating_sub(r.t_step);
        let mapped = r.server.elements.iter().filter(|e| e.mapped).count();
        let mut adv = false;
        match r.step {
            0 if mapped >= 2 && since > 1_000_000 => { focus(r, Some(1), "user focuses W2"); adv = true; }
            1 if since > 300_000 => { r.ix.key(&mut r.server, KEY_LEFTSHIFT, true); r.notes.push("physical: Shift down (client-owned)".into()); adv = true; }
            2 if since > 200_000 => { r.ix.key(&mut r.server, KEY_A, true); r.notes.push("physical: A down (client-owned)".into()); adv = true; }
            3 if since > 300_000 => {
                if let Some(t) = r.server.elements.get(1).and_then(|e| e.window.toplevel().cloned()) { t.send_close(); }
                r.notes.push(format!("t={:.3}s compositor: xdg_toplevel.close -> W2 (Shift and A still held)", now as f64 / 1e6)); adv = true;
            }
            4 if r.server.elements.len() == 1 => { r.notes.push(format!("t={:.3}s W2 destroyed by the client (toplevel_destroyed)", now as f64 / 1e6)); focus(r, Some(0), "successor after target loss"); adv = true; }
            5 if since > 300_000 => { r.ix.key(&mut r.server, KEY_A, false); r.notes.push("physical: A up".into()); adv = true; }
            6 if since > 200_000 => { r.ix.key(&mut r.server, KEY_LEFTSHIFT, false); r.notes.push("physical: Shift up".into()); adv = true; }
            7 if since > 300_000 => { r.ix.key(&mut r.server, KEY_B, true); r.notes.push("physical: b down".into()); adv = true; }
            8 if since > 100_000 => { r.ix.key(&mut r.server, KEY_B, false); r.notes.push("physical: b up".into()); adv = true; }
            9 if since > 800_000 => { r.done = true; }
            _ => {}
        }
        if adv { r.step += 1; r.t_step = now; }
        if now > 30_000_000 { r.done = true; }
        r.server.vblank(0);
        let _ = r.server.dh.flush_clients();
        TimeoutAction::ToInstant(start + period * (n + 1))
    }).unwrap();
    while !run.done {
        el.dispatch(Some(Duration::from_millis(2)), &mut run).unwrap();
        let _ = run.server.dh.flush_clients();
    }
    let _ = child.kill();
    let _ = child.wait();
    println!("== compositor/physical sequence");
    for l in &run.notes { println!("   {l}"); }
    println!("   reached step {} of 10", run.step);
}
