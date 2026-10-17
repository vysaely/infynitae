//! NB-1 regression (2026-10-17): a nested (winit) compositor whose host stops sending frame
//! callbacks must keep its loop running — its run limit ends it on time.
//!
//! No desktop is involved. The test re-runs itself inside private user, mount, network and pid
//! namespaces (`/tmp` and `/dev/dri` are private tmpfs mounts, so rendering is software and no GPU
//! device is opened). Inside, the host is this crate's own binary, headless; the compositor under
//! test is the same binary, nested as the host's client, with an animating client of its own so it
//! draws every frame. After 3 s a second host client goes fullscreen and hides the nested window;
//! the headless host then sends it no frame callbacks (INC-7 residual R-10 on `main`), as a desktop
//! compositor does for a minimised window. The nested compositor must exit at its 8 s run limit.
//! Before the fix its next buffer swap blocked until the window was shown again — forever here.
//!
//! Ignored by default: needs `unshare`, `mpv` (Wayland shm output) and Mesa's software EGL. Run:
//! `cargo test -p infynitae --test nested_hidden_window -- --ignored`.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const INNER: &str = "INFYNITAE_NB1_INNER";
const MPV: &[&str] = &[
    "--no-config",
    "--really-quiet",
    "--ao=null",
    "--vo=wlshm",
    "--loop=inf",
];

/// Kills and reaps a child when dropped, however the test ends.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn available(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
#[ignore = "runtime check: needs unshare, mpv and Mesa software EGL"]
fn a_hidden_nested_window_does_not_block_the_loop() {
    if std::env::var_os(INNER).is_none() {
        for p in ["unshare", "mpv"] {
            if !available(p) {
                eprintln!("skipped: `{p}` is not installed");
                return;
            }
        }
        let exe = std::env::current_exe().expect("test binary");
        let script = "mount -t tmpfs tmpfs /tmp && \
            { [ ! -d /dev/dri ] || mount -t tmpfs tmpfs /dev/dri; } && \
            { [ ! -d /run/systemd/journal ] || mount -t tmpfs tmpfs /run/systemd/journal; } && \
            mkdir -m 700 /tmp/rt && exec \"$0\" \"$@\"";
        // --kill-child: if this outer test is killed, the whole sandbox goes with it
        let mut outer = Reaped(
            Command::new("unshare")
                .args([
                    "-Urnmpf",
                    "--mount-proc",
                    "--kill-child",
                    "--",
                    "sh",
                    "-c",
                    script,
                ])
                .arg(exe)
                .args([
                    "--exact",
                    "a_hidden_nested_window_does_not_block_the_loop",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(INNER, "1")
                .env("XDG_RUNTIME_DIR", "/tmp/rt")
                .env("LIBGL_ALWAYS_SOFTWARE", "1")
                .env("DBUS_SESSION_BUS_ADDRESS", "disabled:")
                .env_remove("DISPLAY")
                .env_remove("WAYLAND_DISPLAY")
                .env_remove("WAYLAND_SOCKET")
                .env_remove("NOTIFY_SOCKET")
                .spawn()
                .expect("unshare"),
        );
        // the outer deadline: far beyond a passing run (~9 s), bounded whatever happens inside
        let end = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(s) = outer.0.try_wait().expect("wait") {
                break s;
            }
            assert!(
                Instant::now() < end,
                "the isolated run did not finish within 60 s"
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(status.success(), "the isolated run failed: {status}");
        return;
    }

    let bin = env!("CARGO_BIN_EXE_infynitae");
    let rt = Path::new("/tmp/rt");
    let host = Reaped(
        Command::new(bin)
            .args(["--headless", "--socket", "wl-host", "--duration", "60"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("host"),
    );
    let socket_end = Instant::now() + Duration::from_secs(10);
    while !rt.join("wl-host").exists() {
        assert!(Instant::now() < socket_end, "the host did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let animation = format!(
        "mpv {} av://lavfi:testsrc=size=320x240:rate=60",
        MPV.join(" ")
    );
    let start = Instant::now();
    let mut nested = Reaped(
        Command::new(bin)
            .args([
                "--socket",
                "wl-nested",
                "--duration",
                "8",
                "--width",
                "640",
                "--height",
                "400",
            ])
            .args(["--exec", &animation])
            .env("WAYLAND_DISPLAY", "wl-host")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("nested compositor"),
    );
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        matches!(nested.0.try_wait(), Ok(None)),
        "the nested compositor ended before its window was hidden"
    );
    // a fullscreen client of the host hides the nested window: the host withholds its frames
    let _hider = Reaped(
        Command::new("mpv")
            .args(MPV)
            .args(["--fs", "av://lavfi:testsrc=size=320x240:rate=30"])
            .env("WAYLAND_DISPLAY", "wl-host")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("mpv"),
    );
    let deadline = Duration::from_secs(20);
    let status = loop {
        if let Some(s) = nested.0.try_wait().expect("wait") {
            break s;
        }
        assert!(
            start.elapsed() < deadline,
            "NB-1: the nested compositor, hidden since 3 s, was still running at 20 s (run limit 8 s)"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let took = start.elapsed();
    eprintln!("nested compositor exited {status} after {took:?} (run limit 8 s, hidden at 3 s)");
    assert!(status.success(), "{status}");
    assert!(
        took < Duration::from_secs(11),
        "the run limit was overrun: {took:?}"
    );
    drop(host);
}
