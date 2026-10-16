//! Orderly shutdown on SIGTERM and SIGINT: a signal ends the loop and the compositor shuts down
//! as for a run limit — the session's command is terminated (SIGTERM, a bounded wait, SIGKILL)
//! and reaped, the socket and its lock file are removed, and the exit status is success. The real
//! binary runs headless in a private XDG_RUNTIME_DIR; nothing outside it is touched, and only
//! processes these tests started are signalled.
//!
//! Times are measured (signal sent → compositor exited) and printed; the bound asserted is
//! deliberately loose (CI load), the printed values are the evidence.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

fn wait_for(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct Run {
    dir: PathBuf,
    compositor: Child,
    socket: PathBuf,
    lock: PathBuf,
}

/// Starts the compositor with a session command written as `body` (a shell script; `$PIDF` is
/// where it writes its pid).
fn start(name: &str, body: &str) -> Run {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("shutdown-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("private runtime dir");
    let script = dir.join("child.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nPIDF={}\n{body}\n",
            dir.join("child.pid").display()
        ),
    )
    .expect("script");
    let socket_name = format!("shutdown-{name}");
    let compositor = Command::new(env!("CARGO_BIN_EXE_infynitae"))
        .args(["--headless", "--socket", &socket_name, "--exec"])
        .arg(format!("sh {}", script.display()))
        .env("XDG_RUNTIME_DIR", &dir)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the compositor");
    Run {
        socket: dir.join(&socket_name),
        lock: dir.join(format!("{socket_name}.lock")),
        dir,
        compositor,
    }
}

impl Run {
    fn child_pid(&self) -> u32 {
        let f = self.dir.join("child.pid");
        wait_for("the session command", Duration::from_secs(5), || {
            std::fs::read_to_string(&f).is_ok_and(|s| s.ends_with('\n'))
        });
        std::fs::read_to_string(&f).unwrap().trim().parse().unwrap()
    }

    fn signal(&self, sig: &str) {
        let ok = Command::new("kill")
            .args([&format!("-{sig}"), &self.compositor.id().to_string()])
            .status()
            .expect("kill");
        assert!(ok.success());
    }

    /// Waits for the compositor to exit: its status and how long it took from `since`.
    fn exit(&mut self, since: Instant) -> (ExitStatus, Duration) {
        loop {
            if let Some(status) = self.compositor.try_wait().expect("wait") {
                return (status, since.elapsed());
            }
            if since.elapsed() > Duration::from_secs(10) {
                let _ = self.compositor.kill();
                panic!("the compositor did not exit within 10 s");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn assert_clean(&self, child: Option<u32>) {
        if let Some(child) = child {
            let left = Path::new(&format!("/proc/{child}")).exists()
                && !std::fs::read_to_string(format!("/proc/{child}/status"))
                    .unwrap_or_default()
                    .contains("State:\tZ");
            if left {
                let _ = Command::new("kill")
                    .args(["-KILL", &child.to_string()])
                    .status();
            }
            assert!(!left, "the session command {child} outlived the compositor");
        }
        assert!(!self.socket.exists(), "the socket was left behind");
        assert!(!self.lock.exists(), "the lock file was left behind");
    }
}

const SLEEPER: &str = "echo $$ > $PIDF\nexec sleep 30 </dev/null >/dev/null 2>&1";

fn clean_shutdown(signal: &str) {
    let mut r = start(&signal.to_lowercase(), SLEEPER);
    let child = r.child_pid();
    wait_for("the socket", Duration::from_secs(5), || r.socket.exists());
    let t = Instant::now();
    r.signal(signal);
    let (status, took) = r.exit(t);
    eprintln!("SIG{signal}: exited {status} after {took:?}");
    assert!(
        status.success(),
        "SIG{signal} ended the compositor with {status}"
    );
    r.assert_clean(Some(child));
    // the command exits on its SIGTERM: no wait for the grace period, no SIGKILL (a command
    // that inherited a blocked SIGTERM would take the whole grace period)
    assert!(took < Duration::from_millis(400), "{took:?}");
}

#[test]
fn sigterm_shuts_down_cleanly() {
    clean_shutdown("TERM");
}

#[test]
fn sigint_shuts_down_cleanly() {
    clean_shutdown("INT");
}

/// A second and a third signal while the first is being handled change nothing: one orderly
/// shutdown, success, nothing left behind.
#[test]
fn repeated_signals_shut_down_once() {
    let mut r = start("repeat", SLEEPER);
    let child = r.child_pid();
    wait_for("the socket", Duration::from_secs(5), || r.socket.exists());
    let t = Instant::now();
    r.signal("TERM");
    r.signal("INT");
    r.signal("TERM");
    let (status, took) = r.exit(t);
    eprintln!("three signals: exited {status} after {took:?}");
    assert!(status.success(), "{status}");
    r.assert_clean(Some(child));
}

/// The session's command does not inherit the compositor's blocked signals (the signal source
/// blocks SIGTERM and SIGINT in the compositor's thread; std resets the mask of every child).
/// The mask is read from the command itself (`sleep`, exec'd by the script, keeps the pid and
/// the mask it inherited).
#[test]
fn session_children_do_not_inherit_blocked_signals() {
    let mut r = start("mask", SLEEPER);
    let child = r.child_pid();
    let status = std::fs::read_to_string(format!("/proc/{child}/status")).unwrap();
    let mask = status.lines().find(|l| l.starts_with("SigBlk:")).unwrap();
    assert_eq!(mask, "SigBlk:\t0000000000000000");
    r.signal("TERM");
    let (status, _) = r.exit(Instant::now());
    assert!(status.success());
    r.assert_clean(Some(child));
}

/// A session command that ignores SIGTERM is killed after the bounded grace period: the
/// compositor still exits with success, the command does not survive, and the wait is bounded.
#[test]
fn a_child_ignoring_sigterm_is_killed_after_the_grace_period() {
    let mut r = start(
        "stubborn",
        "trap '' TERM\necho $$ > $PIDF\nwhile :; do sleep 1; done </dev/null >/dev/null 2>&1",
    );
    let child = r.child_pid();
    wait_for("the socket", Duration::from_secs(5), || r.socket.exists());
    let t = Instant::now();
    r.signal("TERM");
    let (status, took) = r.exit(t);
    eprintln!("stubborn child: exited {status} after {took:?}");
    assert!(status.success(), "{status}");
    r.assert_clean(Some(child));
    assert!(
        took >= Duration::from_millis(450),
        "the grace period was given: {took:?}"
    );
    assert!(took < Duration::from_secs(3), "{took:?}");
}

/// Partial initialization: a SIGTERM at any moment of start-up — before the signal source exists
/// (the default action ends the process, before any socket was created) or after it (an
/// orderly shutdown) — leaves no socket and no lock file behind.
#[test]
fn a_signal_during_start_up_leaves_nothing_behind() {
    for delay_ms in [0u64, 1, 2, 3, 5, 8, 12, 20, 35, 60] {
        let mut r = start(&format!("early-{delay_ms}"), SLEEPER);
        std::thread::sleep(Duration::from_millis(delay_ms));
        let t = Instant::now();
        r.signal("TERM");
        let (status, took) = r.exit(t);
        eprintln!("SIGTERM {delay_ms} ms after start: {status} after {took:?}");
        let child = std::fs::read_to_string(r.dir.join("child.pid"))
            .ok()
            .and_then(|s| s.trim().parse().ok());
        // a child that started reports its pid; it must be gone too
        r.assert_clean(child);
    }
}
