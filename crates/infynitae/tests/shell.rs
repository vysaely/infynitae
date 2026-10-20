//! INC-11 end to end: the real binary, headless, in a private XDG_RUNTIME_DIR, serves the trusted
//! shell at `$XDG_RUNTIME_DIR/infynitae/shell.sock` (C7.1: directory 0700, socket 0600, the same
//! user admitted by SO_PEERCRED); commands reach Core through the backend's loop; a second
//! compositor never takes over a live socket; the socket goes with an orderly shutdown. Only the
//! processes these tests started are signalled.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn wait_for(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A compositor this test started: killed and reaped when the handle goes, however the test ends.
struct Run(Child);

impl Drop for Run {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn start(dir: &Path, socket_name: &str) -> Run {
    Run(Command::new(env!("CARGO_BIN_EXE_infynitae"))
        .args(["--headless", "--socket", socket_name])
        .env("XDG_RUNTIME_DIR", dir)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the compositor"))
}

fn private_dir(name: &str) -> PathBuf {
    // short: the Wayland socket path must fit sun_path (see tests/shutdown.rs)
    let dir = std::env::temp_dir().join(format!("ifs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The `"instance"` a snapshot reply names.
fn instance_of(reply: &str) -> String {
    reply
        .split(r#""instance":"#)
        .nth(1)
        .and_then(|t| t.split([',', '}']).next())
        .expect("an instance")
        .to_owned()
}

fn request(stream: &mut UnixStream, reader: &mut BufReader<UnixStream>, line: &str) -> String {
    stream.write_all(line.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    loop {
        let mut got = String::new();
        assert!(reader.read_line(&mut got).unwrap() > 0, "the server closed");
        // events may come first; the reply is the line with "ok"
        if got.contains("\"ok\"") {
            return got;
        }
    }
}

#[test]
fn the_compositor_serves_the_shell_and_removes_its_socket() {
    let dir = private_dir("serve");
    let mut run = start(&dir, "shell-a");
    let shell = dir.join("infynitae/shell.sock");
    wait_for("the shell socket", Duration::from_secs(10), || {
        shell.exists()
    });
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir.join("infynitae")), 0o700);
    assert_eq!(mode(&shell), 0o600);

    let mut s = UnixStream::connect(&shell).expect("the same user is admitted");
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap());
    let snap = request(&mut s, &mut r, r#"{"id":1,"cmd":"snapshot"}"#);
    assert!(
        snap.contains(r#""ok":true"#) && snap.contains(r#""outputs":[{"id":"#),
        "one output: {snap}"
    );
    let output = snap
        .split(r#""outputs":[{"id":"#)
        .nth(1)
        .and_then(|t| t.split(',').next())
        .expect("an output id")
        .to_owned();
    let nav = request(
        &mut s,
        &mut r,
        &format!(r#"{{"id":2,"cmd":"navigate","output":{output},"to":{{"canvas":[123,456]}}}}"#),
    );
    assert!(nav.contains(r#""ok":true"#), "{nav}");
    let after = request(&mut s, &mut r, r#"{"id":3,"cmd":"snapshot"}"#);
    assert!(after.contains(r#""center":[123,456]"#), "{after}");

    // a second compositor finds the socket live and leaves it alone
    let mut second = start(&dir, "shell-b");
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        matches!(second.0.try_wait(), Ok(None)),
        "it runs on without a shell"
    );
    drop(second);
    assert!(shell.exists(), "the first compositor's socket is untouched");
    let again = request(&mut s, &mut r, r#"{"id":4,"cmd":"snapshot"}"#);
    assert!(
        again.contains(r#""center":[123,456]"#),
        "the same compositor answers"
    );

    let pid = run.0.id().to_string();
    Command::new("kill").args(["-TERM", &pid]).status().unwrap();
    let t0 = Instant::now();
    let status = loop {
        if let Some(st) = run.0.try_wait().unwrap() {
            break st;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "no exit after SIGTERM"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(status.success(), "{status}");
    assert!(!shell.exists(), "the shell socket was left behind");
    drop(run);

    // INV-STAGE-3 across restarts: element ids start at 1 again in a new compositor, so its
    // instance must differ, or a shell's old references would name the new run's elements
    let run2 = start(&dir, "shell-c");
    wait_for("the shell socket again", Duration::from_secs(10), || {
        shell.exists()
    });
    let mut s2 = UnixStream::connect(&shell).unwrap();
    s2.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut r2 = BufReader::new(s2.try_clone().unwrap());
    let snap2 = request(&mut s2, &mut r2, r#"{"id":1,"cmd":"snapshot"}"#);
    assert_ne!(
        instance_of(&snap2),
        instance_of(&snap),
        "a new run is a new instance"
    );
    drop(run2);
    let _ = std::fs::remove_dir_all(&dir);
}
