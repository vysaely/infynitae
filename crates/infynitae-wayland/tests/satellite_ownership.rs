//! Session process ownership: the compositor reaps the orphans it adopted (`ownership::reap_adopted`,
//! run by the session's SIGCHLD handler). The satellite is an owned child, so that reaper leaves its
//! exit status to the supervisor, which notices the exit and restarts it.
//!
//! Its own test binary: `reap_adopted` reaps every unregistered zombie child of the process, so
//! in a binary shared with other tests it could take theirs.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use infynitae_wayland::ownership;
use infynitae_wayland::xwayland::{RestartPolicy, SatelliteState, XDisplay, XwaylandSupervisor};

/// The process state letter from `/proc/<pid>/stat` (`Z` for a zombie).
fn state(pid: u32) -> Option<char> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit(')').next()?.trim_start().chars().next()
}

#[test]
fn the_orphan_reaper_leaves_the_satellite_to_its_supervisor() {
    // a private root, never `/tmp` itself, so no real display is touched
    let root: PathBuf =
        std::env::temp_dir().join(format!("infynitae-satellite-owned-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let display = XDisplay::reserve(&root, 0..=3).unwrap();
    let policy = RestartPolicy {
        backoff: Duration::from_millis(50),
        max_exits: 3,
        window: Duration::from_secs(60),
    };
    let mut s = XwaylandSupervisor::new(
        display,
        vec!["sh".into(), "-c".into(), "exit 7".into(), "sh".into()],
        "wl-test",
        policy,
    );
    s.start().unwrap();
    let pid = s.pid().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while state(pid) != Some('Z') && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        state(pid),
        Some('Z'),
        "the satellite exited and awaits its reaper"
    );

    // what the session's SIGCHLD handler does while it runs
    ownership::reap_adopted();
    assert_eq!(
        state(pid),
        Some('Z'),
        "the orphan reaper left the owned satellite alone"
    );

    let t0 = Instant::now();
    s.poll(t0);
    assert_eq!(
        s.state(),
        SatelliteState::Crashed,
        "the supervisor noticed the exit"
    );
    assert_eq!(
        s.last_exit().and_then(|e| e.code()),
        Some(7),
        "with the satellite's own exit status"
    );
    assert_eq!(state(pid), None, "reaped by the supervisor");
    assert_eq!(s.poll(t0 + policy.backoff), None);
    assert_eq!(s.state(), SatelliteState::Running, "and restarted it");
    drop(s);
    fs::remove_dir_all(&root).unwrap();
}
