//! Desktop isolation test: desktop actions triggered during tests run through the harness's
//! stub launcher and never reach the checkout's `live/launch.sh` (the one recorded in
//! `~/.config/infynitae/checkout`, or `$HOME/infynitae`) or the live desktop session.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_L, KEY_LEFTMETA, KEY_LEFTSHIFT};

mod common;
use common::Lockstep;

const INNER: &str = "INFYNITAE_ISOLATION_TEST_INNER";
static TEST_RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn action_runs_from_harness_stub_and_never_from_home() {
    if std::env::var_os(INNER).is_none() {
        let id = TEST_RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let fake_home =
            std::env::temp_dir().join(format!("fake-home-{}-{}", std::process::id(), id));
        let fake_live = fake_home.join("infynitae/live");
        fs::create_dir_all(&fake_live).expect("create fake live dir");

        let marker = fake_home.join("MARKER_HOST_LAUNCH_WAS_RUN");
        // the checkout an install elsewhere records
        let recorded_live = fake_home.join("src/infynitae/live");
        fs::create_dir_all(&recorded_live).expect("create recorded live dir");
        fs::create_dir_all(fake_home.join(".config/infynitae")).expect("create config dir");
        fs::write(
            fake_home.join(".config/infynitae/checkout"),
            format!("{}\n", fake_home.join("src/infynitae").display()),
        )
        .expect("write checkout record");
        for live in [&fake_live, &recorded_live] {
            let fake_launch = live.join("launch.sh");
            let script = format!("#!/bin/sh\ntouch \"{}\"\n", marker.display());
            fs::write(&fake_launch, script).expect("write fake launch.sh");
            fs::set_permissions(&fake_launch, fs::Permissions::from_mode(0o755))
                .expect("chmod fake launch.sh");
        }

        let status = Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "action_runs_from_harness_stub_and_never_from_home",
                "--nocapture",
            ])
            .env(INNER, "1")
            .env("HOME", &fake_home)
            .status()
            .expect("run inner test");

        let marker_ran = marker.exists();
        let _ = fs::remove_dir_all(&fake_home);

        assert!(status.success(), "inner test failed: {status}");
        assert!(
            !marker_ran,
            "action ran from the checkout's live/launch.sh instead of harness stub!"
        );
        return;
    }

    // Inner test: HOME points at fake_home, which has launch.sh that touches marker.
    // The Lockstep harness gives the compositor its own stub launcher.
    let mut ls = Lockstep::new(None);

    // Press Super+Shift+L to trigger lock-screen action
    let mut t = 1000u64;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, t).expect("press super");
    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTSHIFT, true, t).expect("press shift");
    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_L, true, t).expect("press L");
    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_L, false, t).expect("release L");
    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTSHIFT, false, t).expect("release shift");
    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, t).expect("release super");

    ls.pump();

    // Verify the harness stub recorded "lock-screen"
    assert!(
        ls.wait_for_launcher_action("lock-screen", Duration::from_secs(2)),
        "harness stub did not record lock-screen action; recorded: {:?}",
        ls.recorded_launcher_actions()
    );
}
