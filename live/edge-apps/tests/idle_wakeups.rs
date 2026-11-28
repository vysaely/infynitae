use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "infynitae-edge-apps-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn id(&self) -> u32 {
        self.0.as_ref().unwrap().id()
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.0.as_mut().unwrap().try_wait()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct WaylandServer {
    stop: Arc<AtomicBool>,
    commits: Arc<AtomicUsize>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl WaylandServer {
    fn start(sock_path: &Path) -> Self {
        let listener = UnixListener::bind(sock_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let commits = Arc::new(AtomicUsize::new(0));
        let stop_t = Arc::clone(&stop);
        let commits_t = Arc::clone(&commits);
        let join = std::thread::spawn(move || serve_wayland(listener, stop_t, commits_t));
        Self {
            stop,
            commits,
            join: Some(join),
        }
    }
}

impl Drop for WaylandServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_ne_bytes());
}

fn push_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_ne_bytes());
}

fn push_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    push_u32(buf, (bytes.len() + 1) as u32);
    buf.extend_from_slice(bytes);
    buf.push(0);
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
}

fn read_u32(b: &[u8], off: &mut usize) -> u32 {
    let v = u32::from_ne_bytes(b[*off..*off + 4].try_into().unwrap());
    *off += 4;
    v
}

fn read_str(b: &[u8], off: &mut usize) -> String {
    let len = read_u32(b, off) as usize;
    let s = String::from_utf8_lossy(&b[*off..*off + len.saturating_sub(1)]).into_owned();
    *off += (len + 3) & !3;
    s
}

fn wl_msg(obj: u32, opcode: u16, body: &[u8]) -> Vec<u8> {
    let size = (8 + body.len()) as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&obj.to_ne_bytes());
    out.extend_from_slice(&((size << 16) | u32::from(opcode)).to_ne_bytes());
    out.extend_from_slice(body);
    out
}

fn recv_chunk(sock: &UnixStream, out: &mut Vec<u8>) -> Option<bool> {
    let mut buf = [0u8; 4096];
    let mut cmsg_buf = [0u8; 256];
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr().cast();
    msg.msg_controllen = cmsg_buf.len();
    let n = unsafe { libc::recvmsg(sock.as_raw_fd(), &mut msg, 0) };
    if n < 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::WouldBlock
            || err.kind() == std::io::ErrorKind::Interrupted
        {
            return None;
        }
        return Some(false);
    }
    if n == 0 {
        return Some(false);
    }
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let data_ptr = libc::CMSG_DATA(cmsg);
                let data_len = (*cmsg).cmsg_len - libc::CMSG_LEN(0) as usize;
                for off in (0..data_len).step_by(4) {
                    let mut fd_bytes = [0u8; 4];
                    std::ptr::copy_nonoverlapping(data_ptr.add(off), fd_bytes.as_mut_ptr(), 4);
                    libc::close(i32::from_ne_bytes(fd_bytes));
                }
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }
    out.extend_from_slice(&buf[..n as usize]);
    Some(true)
}

fn serve_wayland(listener: UnixListener, stop: Arc<AtomicBool>, commits: Arc<AtomicUsize>) {
    use std::collections::{HashMap, HashSet};
    use std::io::Write;

    let mut client: Option<UnixStream> = None;
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(true).unwrap();
                client = Some(stream);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return,
        }
    }
    let Some(mut sock) = client else {
        return;
    };

    #[derive(Clone, Copy)]
    enum Obj {
        Display,
        Registry,
        Compositor,
        Shm,
        ShmPool,
        Buffer,
        Seat,
        LayerShell,
        LayerSurface,
        Surface,
        Region,
        Output,
        ToplevelMgr,
    }

    let mut objs = HashMap::from([(1u32, Obj::Display)]);
    let globals = [
        (1u32, "wl_compositor", 6u32),
        (2, "wl_shm", 1),
        (3, "wl_seat", 8),
        (4, "zwlr_layer_shell_v1", 4),
        (5, "zwlr_foreign_toplevel_manager_v1", 3),
        (10, "wl_output", 4),
    ];
    let mut surface_to_layer = HashMap::new();
    let mut layer_configured = HashSet::new();
    let mut layer_size: HashMap<u32, (u32, u32)> = HashMap::new();
    let mut layer_was_mapped: HashSet<u32> = HashSet::new();
    let mut layer_unmapped: HashSet<u32> = HashSet::new();
    let mut surface_buf: HashMap<u32, Option<u32>> = HashMap::new();
    let mut pending_frames: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut serial = 10u32;
    let mut pending = Vec::new();

    while !stop.load(Ordering::Acquire) {
        match recv_chunk(&sock, &mut pending) {
            None => {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            Some(false) => break,
            Some(true) => {}
        }
        while pending.len() >= 8 {
            let obj_id = u32::from_ne_bytes(pending[0..4].try_into().unwrap());
            let word = u32::from_ne_bytes(pending[4..8].try_into().unwrap());
            let size = (word >> 16) as usize;
            let opcode = (word & 0xffff) as u16;
            if size < 8 || pending.len() < size {
                break;
            }
            let frame: Vec<u8> = pending.drain(..size).collect();
            let body = &frame[8..];
            let mut off = 0usize;
            let mut out = Vec::new();
            let mut had_frame = false;
            match objs.get(&obj_id).copied() {
                Some(Obj::Display) => match opcode {
                    0 => {
                        let cb = read_u32(body, &mut off);
                        let mut b = Vec::new();
                        push_u32(&mut b, 0);
                        out.extend(wl_msg(cb, 0, &b));
                        let mut d = Vec::new();
                        push_u32(&mut d, cb);
                        out.extend(wl_msg(1, 1, &d));
                    }
                    1 => {
                        let reg = read_u32(body, &mut off);
                        objs.insert(reg, Obj::Registry);
                        for &(name, iface, ver) in &globals {
                            let mut b = Vec::new();
                            push_u32(&mut b, name);
                            push_str(&mut b, iface);
                            push_u32(&mut b, ver);
                            out.extend(wl_msg(reg, 0, &b));
                        }
                    }
                    _ => {}
                },
                Some(Obj::Registry) if opcode == 0 => {
                    let _name = read_u32(body, &mut off);
                    let iface = read_str(body, &mut off);
                    let _ver = read_u32(body, &mut off);
                    let new_id = read_u32(body, &mut off);
                    match iface.as_str() {
                        "wl_compositor" => {
                            objs.insert(new_id, Obj::Compositor);
                        }
                        "wl_shm" => {
                            objs.insert(new_id, Obj::Shm);
                            for fmt in [0u32, 1u32] {
                                let mut b = Vec::new();
                                push_u32(&mut b, fmt);
                                out.extend(wl_msg(new_id, 0, &b));
                            }
                        }
                        "wl_seat" => {
                            objs.insert(new_id, Obj::Seat);
                            let mut b = Vec::new();
                            push_u32(&mut b, 7);
                            out.extend(wl_msg(new_id, 0, &b));
                            let mut nb = Vec::new();
                            push_str(&mut nb, "seat0");
                            out.extend(wl_msg(new_id, 1, &nb));
                        }
                        "zwlr_layer_shell_v1" => {
                            objs.insert(new_id, Obj::LayerShell);
                        }
                        "zwlr_foreign_toplevel_manager_v1" => {
                            objs.insert(new_id, Obj::ToplevelMgr);
                        }
                        "wl_output" => {
                            objs.insert(new_id, Obj::Output);
                            let mut gb = Vec::new();
                            push_i32(&mut gb, 0);
                            push_i32(&mut gb, 0);
                            push_i32(&mut gb, 1280);
                            push_i32(&mut gb, 800);
                            push_i32(&mut gb, 0);
                            push_str(&mut gb, "Mock");
                            push_str(&mut gb, "Out");
                            push_i32(&mut gb, 0);
                            out.extend(wl_msg(new_id, 0, &gb));
                            let mut mb = Vec::new();
                            push_u32(&mut mb, 3);
                            push_i32(&mut mb, 1280);
                            push_i32(&mut mb, 800);
                            push_i32(&mut mb, 60_000);
                            out.extend(wl_msg(new_id, 1, &mb));
                            let mut sb = Vec::new();
                            push_i32(&mut sb, 1);
                            out.extend(wl_msg(new_id, 3, &sb));
                            out.extend(wl_msg(new_id, 2, &[]));
                        }
                        _ => {}
                    }
                }
                Some(Obj::Compositor) => match opcode {
                    0 => {
                        let sid = read_u32(body, &mut off);
                        objs.insert(sid, Obj::Surface);
                    }
                    1 => {
                        let rid = read_u32(body, &mut off);
                        objs.insert(rid, Obj::Region);
                    }
                    _ => {}
                },
                Some(Obj::Shm) if opcode == 0 => {
                    let pid = read_u32(body, &mut off);
                    objs.insert(pid, Obj::ShmPool);
                }
                Some(Obj::ShmPool) if opcode == 0 => {
                    let bid = read_u32(body, &mut off);
                    objs.insert(bid, Obj::Buffer);
                }
                Some(Obj::LayerShell) if opcode == 0 => {
                    let lid = read_u32(body, &mut off);
                    let sid = read_u32(body, &mut off);
                    objs.insert(lid, Obj::LayerSurface);
                    surface_to_layer.insert(sid, lid);
                }
                Some(Obj::LayerSurface) if opcode == 0 => {
                    let w = read_u32(body, &mut off);
                    let h = read_u32(body, &mut off);
                    layer_size.insert(obj_id, (w, h));
                }
                Some(Obj::Surface) => match opcode {
                    1 => {
                        let bid = read_u32(body, &mut off);
                        surface_buf.insert(obj_id, (bid != 0).then_some(bid));
                    }
                    3 => {
                        let cb = read_u32(body, &mut off);
                        pending_frames.entry(obj_id).or_default().push(cb);
                    }
                    6 => {
                        if let Some(&lid) = surface_to_layer.get(&obj_id) {
                            let first = layer_configured.insert(lid);
                            let mapped = surface_buf.get(&obj_id).copied().flatten().is_some();
                            let was_mapped = layer_was_mapped.contains(&lid);
                            let unmapped = layer_unmapped.contains(&lid);
                            let remap = !mapped && !was_mapped && unmapped;
                            if mapped {
                                layer_was_mapped.insert(lid);
                            } else {
                                layer_was_mapped.remove(&lid);
                            }
                            if !mapped && (was_mapped || remap) {
                                layer_unmapped.insert(lid);
                            } else {
                                layer_unmapped.remove(&lid);
                            }
                            if first || remap {
                                serial += 1;
                                let (rw, rh) = layer_size.get(&lid).copied().unwrap_or((0, 0));
                                let mut cb = Vec::new();
                                push_u32(&mut cb, serial);
                                push_u32(&mut cb, if rw == 0 { 1280 } else { rw });
                                push_u32(&mut cb, if rh == 0 { 800 } else { rh });
                                out.extend(wl_msg(lid, 0, &cb));
                            }
                        }
                        if let Some(Some(bid)) = surface_buf.get(&obj_id).copied() {
                            out.extend(wl_msg(bid, 0, &[]));
                            commits.fetch_add(1, Ordering::Release);
                        }
                        if let Some(cbs) = pending_frames.remove(&obj_id) {
                            had_frame = !cbs.is_empty();
                            for cb in cbs {
                                let mut b = Vec::new();
                                push_u32(&mut b, 0);
                                out.extend(wl_msg(cb, 0, &b));
                                let mut d = Vec::new();
                                push_u32(&mut d, cb);
                                out.extend(wl_msg(1, 1, &d));
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            if had_frame {
                std::thread::sleep(Duration::from_millis(16));
            }
            if !out.is_empty() {
                let _ = sock.write_all(&out);
            }
        }
    }
}

fn sample_threads(pid: u32) -> BTreeMap<u32, (String, u64)> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return out;
    };
    for entry in entries.flatten() {
        let Ok(tid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(status) = std::fs::read_to_string(entry.path().join("status")) else {
            continue;
        };
        let mut name = String::new();
        let mut vol = 0u64;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Name:") {
                name = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("voluntary_ctxt_switches:") {
                vol = rest.trim().parse().unwrap_or(0);
            }
        }
        out.insert(tid, (name, vol));
    }
    out
}

fn assert_zero_idle_wakeups(pid: u32, label: &str) {
    let s1 = sample_threads(pid);
    std::thread::sleep(Duration::from_millis(1500));
    let s2 = sample_threads(pid);

    let blocking_threads: Vec<_> = s2
        .iter()
        .filter(|(_, (name, _))| name.starts_with("blocking-"))
        .map(|(tid, (name, _))| format!("{tid}:{name}"))
        .collect();
    assert!(
        blocking_threads.is_empty(),
        "{label}: unexpected blocking pool thread(s): {blocking_threads:?}"
    );

    let mut deltas = Vec::new();
    let mut total_delta = 0u64;
    for (tid, (name, v2)) in &s2 {
        let v1 = s1.get(tid).map_or(0, |(_, v)| *v);
        let d = v2.saturating_sub(v1);
        total_delta += d;
        deltas.push(format!("{tid}:{name}={d}"));
    }
    assert_eq!(
        total_delta, 0,
        "{label}: expected 0 voluntary context switches while closed, got {total_delta}: {deltas:?}"
    );
}

struct RunningEdge {
    _tmp: TempDir,
    _dbus: ChildGuard,
    wayland: WaylandServer,
    edge: ChildGuard,
    dbus_addr: String,
    probe: zbus::Connection,
}

fn spawn_test_edge(tag: &str, config: &str, boxes_file: Option<&Path>) -> Option<RunningEdge> {
    let tmp = TempDir::new(tag);
    let dbus_dir = tmp.path().join("dbus");
    let rt_dir = tmp.path().join("runtime");
    let home_dir = tmp.path().join("home");
    let cfg_dir = home_dir.join(".config/infynitae");
    std::fs::create_dir_all(&dbus_dir).unwrap();
    std::fs::create_dir_all(&rt_dir).unwrap();
    std::fs::create_dir_all(&cfg_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&rt_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(cfg_dir.join("config"), config).unwrap();

    let spawned = Command::new("dbus-daemon")
        .arg("--session")
        .arg("--nofork")
        .arg("--print-address=1")
        .arg(format!("--address=unix:dir={}", dbus_dir.display()))
        .stdout(Stdio::piped())
        .spawn();
    let mut dbus = match spawned {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipped: no dbus-daemon to run a private bus: {e}");
            return None;
        }
    };
    let mut dbus_addr = String::new();
    BufReader::new(dbus.stdout.take().unwrap())
        .read_line(&mut dbus_addr)
        .unwrap();
    let dbus_addr = dbus_addr.trim().to_string();
    let dbus_guard = ChildGuard::new(dbus);

    let wayland = WaylandServer::start(&rt_dir.join("wayland-test"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_infynitae-edge-apps"));
    cmd.env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", &home_dir)
        .env("XDG_RUNTIME_DIR", &rt_dir)
        .env("WAYLAND_DISPLAY", "wayland-test")
        .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
        .env("LANG", "C.UTF-8")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(p) = boxes_file {
        cmd.env("INFYNITAE_EDGE_BOXES", p);
    }
    let mut edge = ChildGuard::new(cmd.spawn().unwrap());

    let probe =
        zbus::block_on(zbus::connection::Builder::address(dbus_addr.as_str()).build()).unwrap();
    let dbus_proxy = zbus::block_on(zbus::fdo::DBusProxy::new(&probe)).unwrap();
    let notify_name = zbus::names::BusName::try_from("org.freedesktop.Notifications").unwrap();
    let tray_name = zbus::names::BusName::try_from("org.kde.StatusNotifierWatcher").unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = edge.try_wait().unwrap() {
            panic!("infynitae-edge-apps exited early with {status}");
        }
        let ready = wayland.commits.load(Ordering::Acquire) > 0
            && zbus::block_on(dbus_proxy.name_has_owner(notify_name.clone())).unwrap_or(false)
            && zbus::block_on(dbus_proxy.name_has_owner(tray_name.clone())).unwrap_or(false);
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "edge did not finish Wayland and D-Bus startup within 5 s"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    Some(RunningEdge {
        _tmp: tmp,
        _dbus: dbus_guard,
        wayland,
        edge,
        dbus_addr,
        probe,
    })
}

#[test]
fn closed_edge_has_no_blocking_thread_and_zero_idle_wakeups() {
    let Some(run) = spawn_test_edge(
        "idle-wakeups",
        "PANEL_CONTROL=\"SUPER+A top right 380 apps/background music/playing\"\n",
        None,
    ) else {
        return;
    };

    // Keep `probe` connected across the idle sample so its disconnect does not emit
    // NameOwnerChanged to `tray_watcher`; wait for async-io's post-startup backoff (~20 ms).
    std::thread::sleep(Duration::from_millis(150));

    let pid = run.edge.id();
    assert_zero_idle_wakeups(pid, "initial closed state");

    // Toggle the control panel open (background + music cards fetch over the shared D-Bus
    // connection), let it render and fetch, then toggle it closed and verify it stays idle.
    let before_open = run.wayland.commits.load(Ordering::Acquire);
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGUSR2) }, 0);
    let open_deadline = Instant::now() + Duration::from_secs(3);
    while run.wayland.commits.load(Ordering::Acquire) == before_open {
        assert!(Instant::now() < open_deadline, "panel did not open/commit");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(350));

    let before_close = run.wayland.commits.load(Ordering::Acquire);
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGUSR2) }, 0);
    let close_deadline = Instant::now() + Duration::from_secs(3);
    while run.wayland.commits.load(Ordering::Acquire) == before_close {
        assert!(
            Instant::now() < close_deadline,
            "panel did not close/commit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // Wait for the leave animation (200 ms) and any final closed commit to settle.
    let mut last_commits = run.wayland.commits.load(Ordering::Acquire);
    let settle_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_millis(120));
        let now_commits = run.wayland.commits.load(Ordering::Acquire);
        if now_commits == last_commits {
            break;
        }
        last_commits = now_commits;
        assert!(Instant::now() < settle_deadline, "panel kept committing");
    }
    std::thread::sleep(Duration::from_millis(150));

    assert_zero_idle_wakeups(pid, "closed after opening D-Bus cards");

    drop(run.probe);
}

#[test]
fn osd_via_notify_send_replaces_skips_history_expires_and_stays_idle() {
    let boxes_dir = TempDir::new("osd-boxes");
    let boxes_path = boxes_dir.path().join("boxes");
    let Some(run) = spawn_test_edge(
        "osd-dbus",
        "PANEL_CONTROL=\"SUPER+A top right 380 notifications/list settings/session\"\n",
        Some(&boxes_path),
    ) else {
        return;
    };
    let pid = run.edge.id();

    let wait_boxes = |want: &dyn Fn(&str) -> bool, timeout: Duration, label: &str| {
        let start = Instant::now();
        let deadline = start + timeout;
        loop {
            if let Ok(text) = std::fs::read_to_string(&boxes_path) {
                if want(&text) {
                    return (start.elapsed(), text);
                }
            }
            assert!(
                Instant::now() < deadline,
                "{label}: timed out; last boxes = {:?}",
                std::fs::read_to_string(&boxes_path)
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    };

    let notify = |args: &[&str]| {
        let status = Command::new("notify-send")
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("DBUS_SESSION_BUS_ADDRESS", &run.dbus_addr)
            .env("LANG", "C.UTF-8")
            .args(args)
            .status();
        match status {
            Ok(s) => {
                assert!(s.success(), "notify-send {args:?} exited {s}");
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("skipped: notify-send not installed: {e}");
                false
            }
            Err(e) => panic!("notify-send {args:?}: {e}"),
        }
    };

    let t0 = Instant::now();
    if !notify(&[
        "-a",
        "Volume",
        "-h",
        "string:x-canonical-private-synchronous:volume",
        "-h",
        "int:value:55",
        "-t",
        "1200",
        "Volume: 55%",
    ]) {
        return;
    }
    let (_, first) = wait_boxes(
        &|s| s == "osd 520 720 240 56 55 Volume\n",
        Duration::from_secs(2),
        "first volume OSD",
    );
    let first_ms = t0.elapsed().as_millis();
    assert_eq!(first.lines().count(), 1);
    eprintln!("TIMING osd notify-send->first-frame ms {first_ms}");

    // Same mute format as live/launch.sh (sync tag without int:value) replaces without stacking.
    assert!(notify(&[
        "-a",
        "Audio",
        "-h",
        "string:x-canonical-private-synchronous:volume",
        "-t",
        "1200",
        "Muted",
    ]));
    let (_, muted) = wait_boxes(
        &|s| s == "osd 520 720 240 56 0 Muted\n",
        Duration::from_secs(2),
        "mute OSD replacement",
    );
    assert_eq!(muted.lines().count(), 1);

    // After ~1.2 s it disappears and returns boxes to closed.
    wait_boxes(
        &|s| s == "closed\n",
        Duration::from_secs(3),
        "OSD expiry after mute",
    );

    // Brightness arrives after expiry -> remaps cleanly and expires again.
    assert!(notify(&[
        "-a",
        "Brightness",
        "-h",
        "string:x-canonical-private-synchronous:brightness",
        "-h",
        "int:value:80",
        "-t",
        "1200",
        "Brightness: 80%",
    ]));
    wait_boxes(
        &|s| s == "osd 520 720 240 56 80 Brightness\n",
        Duration::from_secs(2),
        "brightness OSD remap",
    );
    wait_boxes(
        &|s| s == "closed\n",
        Duration::from_secs(3),
        "brightness OSD expiry",
    );

    // No leftover polling/timers after OSD hides.
    std::thread::sleep(Duration::from_millis(150));
    assert_zero_idle_wakeups(pid, "closed after OSD expired");

    // Notification history never received the OSD entries.
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGUSR2) }, 0);
    let (_, panel) = wait_boxes(
        &|s| s.contains("panel control") && s.contains("settings/session"),
        Duration::from_secs(3),
        "control panel open after OSD",
    );
    assert!(
        !panel.contains("notifications/list"),
        "OSD must not enter notification history: {panel}"
    );
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGUSR2) }, 0);
    drop(run.probe);
}
