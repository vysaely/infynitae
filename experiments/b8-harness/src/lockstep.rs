//! Deterministic lockstep: server and probe client in one thread over a socketpair, driven by a
//! virtual vblank clock. No real time, no host compositor, no sockets on disk.

use std::{os::fd::AsRawFd, os::unix::net::UnixStream, sync::{atomic::{AtomicU64, Ordering}, Arc}};

use smithay::reexports::wayland_server::Display;
use wayland_client::{globals::registry_queue_init, Connection, EventQueue, QueueHandle};

use crate::{
    client::Probe,
    server::{ClientState, OutCfg, Policy, Server},
};

pub struct Lockstep {
    pub globals: wayland_client::globals::GlobalList,
    pub display: Display<Server>,
    pub server: Server,
    pub conn: Connection,
    pub queue: EventQueue<Probe>,
    pub probe: Probe,
    pub qh: QueueHandle<Probe>,
    pub now: Arc<AtomicU64>,
}

impl Lockstep {
    pub fn new(outs: &[OutCfg], policy: Policy) -> Self {
        let mut display: Display<Server> = Display::new().expect("display");
        let mut dh = display.handle();
        let mut server = Server::new(&dh, outs, policy);
        let (s, c) = UnixStream::pair().expect("socketpair");
        dh.insert_client(s, Arc::new(ClientState::default())).expect("insert client");
        let conn = Connection::from_socket(c).expect("client connection");

        // Registry roundtrip needs the server to run while the client blocks, so do it by hand.
        let display_ptr: *mut Display<Server> = &mut display;
        let server_ptr: *mut Server = &mut server;
        let pump_server = move || {
            // SAFETY: single-threaded; these pointers are only used during this constructor while
            // `display` and `server` are alive and not otherwise borrowed.
            unsafe {
                let _ = (*display_ptr).dispatch_clients(&mut *server_ptr);
                let _ = (*display_ptr).flush_clients();
            }
        };
        let conn2 = conn.clone();
        let handle = std::thread::spawn(move || registry_queue_init::<Probe>(&conn2));
        // Serve the registry roundtrip from this thread until the client thread finishes.
        while !handle.is_finished() {
            pump_server();
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        let (globals, queue) = handle.join().expect("join").expect("registry init");
        let qh = queue.handle();
        let now = Arc::new(AtomicU64::new(0));
        let probe = Probe::new(&globals, &qh, now.clone());
        let mut ls = Lockstep { globals, display, server, conn, queue, probe, qh, now };
        ls.pump();
        ls
    }

    /// Exchange messages until both sides are quiescent.
    pub fn pump(&mut self) {
        let mut idle_rounds = 0;
        for _ in 0..10_000 {
            let n_srv = self.display.dispatch_clients(&mut self.server).unwrap_or(0);
            let _ = self.display.flush_clients();
            let _ = self.conn.flush();
            let mut n_cli = 0;
            if let Some(guard) = self.conn.prepare_read() {
                let mut pfd = libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
                // SAFETY: valid pollfd array of length 1, zero timeout.
                let r = unsafe { libc::poll(&mut pfd, 1, 0) };
                if r > 0 {
                    let _ = guard.read();
                }
            }
            n_cli += self.queue.dispatch_pending(&mut self.probe).unwrap_or(0);
            let _ = self.conn.flush();
            if n_srv == 0 && n_cli == 0 {
                idle_rounds += 1;
                if idle_rounds >= 3 {
                    return;
                }
            } else {
                idle_rounds = 0;
            }
        }
        panic!("pump did not converge");
    }

    fn set_now(&mut self, t: u64) {
        self.server.now_us = t;
        self.now.store(t, Ordering::Relaxed);
    }

    /// Advance virtual time to `t_us`, processing every vblank in order.
    pub fn run_until(&mut self, t_us: u64) {
        loop {
            let (oi, t) = self.server.next_vblank();
            if t > t_us as f64 {
                break;
            }
            self.set_now(t.round() as u64);
            self.server.consume_vblank(oi);
            self.server.vblank(oi);
            self.pump();
        }
        self.set_now(t_us);
        self.pump();
    }

    pub fn set_camera(&mut self, oi: usize, cam: (i32, i32)) {
        self.server.outputs[oi].camera = cam;
        self.server.refresh_visibility();
        self.pump();
    }

    /// Pan output `oi`'s camera to `to`, moving `step` px per vblank of that output (a pan whose
    /// rate is tied to the output's refresh, as a pointer-driven pan would be).
    pub fn gesture(&mut self, active: bool) {
        self.server.gesture_active = active;
        if !active {
            self.server.refresh_visibility();
            self.pump();
        }
    }

    pub fn pan(&mut self, oi: usize, to: (i32, i32), step: i32) {
        self.server.gesture_active = true;
        self.pan_inner(oi, to, step);
        self.gesture(false);
    }

    fn pan_inner(&mut self, oi: usize, to: (i32, i32), step: i32) {
        loop {
            let cam = self.server.outputs[oi].camera;
            if cam == to {
                break;
            }
            let mv = |c: i32, t: i32| if (t - c).abs() <= step { t } else { c + step * (t - c).signum() };
            let next = (mv(cam.0, to.0), mv(cam.1, to.1));
            // wait for the next vblank of this output, processing others in between
            loop {
                let (o, t) = self.server.next_vblank();
                self.set_now(t.round() as u64);
                self.server.consume_vblank(o);
                self.server.vblank(o);
                self.pump();
                if o == oi {
                    break;
                }
            }
            self.set_camera(oi, next);
        }
    }

    pub fn client_log_since(&self, t0: u64) -> Vec<String> {
        self.probe.log.iter().filter(|(t, _)| *t >= t0).map(|(t, s)| format!("t={:>8.3}ms  client  {s}", *t as f64 / 1000.0)).collect()
    }
    pub fn server_log_since(&self, t0: u64) -> Vec<String> {
        self.server.log.iter().filter(|(t, _)| *t >= t0).map(|(t, s)| format!("t={:>8.3}ms  server  {s}", *t as f64 / 1000.0)).collect()
    }
    /// Merged, time-ordered log since t0 (server lines first at equal time).
    pub fn merged_since(&self, t0: u64) -> Vec<String> {
        let mut v: Vec<(u64, u8, String)> = Vec::new();
        for (t, s) in &self.server.log {
            if *t >= t0 {
                v.push((*t, 0, format!("t={:>8.3}ms  server  {s}", *t as f64 / 1000.0)));
            }
        }
        for (t, s) in &self.probe.log {
            if *t >= t0 {
                v.push((*t, 1, format!("t={:>8.3}ms  client  {s}", *t as f64 / 1000.0)));
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        v.into_iter().map(|x| x.2).collect()
    }
}
