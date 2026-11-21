//! XKB_DEFAULT_LAYOUT and XKB_DEFAULT_OPTIONS apply on a config reload: every app's keyboard gets
//! the new keymap, and the actions started from then on get its names. One test in its own file:
//! the names the actions get are the process's (`apply_keymap_env`), shared by the tests of a file.

mod common;

use std::time::Duration;

use common::{Lockstep, output};
use infynitae_wayland::cursor::TestTempDir;

fn reload(ls: &mut Lockstep, home: &std::path::Path, config: &str) {
    let dir = home.join(".config/infynitae");
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::write(dir.join("config"), config).expect("config");
    ls.state.set_config_home(home.to_path_buf());
    ls.state.reload_config();
    ls.pump();
}

/// The keymap text `layout` and `options` compile to, as the seat's keyboard sends it.
fn keymap(layout: &str, options: Option<&str>) -> String {
    use smithay::input::keyboard::xkb;
    let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    xkb::Keymap::new_from_names(
        &ctx,
        "evdev",
        "",
        layout,
        "",
        options.map(str::to_string),
        xkb::COMPILE_NO_FLAGS,
    )
    .expect("keymap")
    .get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1)
}

/// The keymap the probe's `wl_keyboard` was sent last.
fn sent(ls: &Lockstep) -> String {
    let bytes = ls.probe.keymap.clone().expect("a keymap was sent");
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).expect("keymap text")
}

/// The `XKB_DEFAULT_LAYOUT` and `XKB_DEFAULT_OPTIONS` the next action is started with.
fn action_names(ls: &mut Lockstep) -> String {
    let script = ls.live_dir.join("launch.sh");
    let record = ls.live_dir.join("recorded_xkb.txt");
    let _ = std::fs::remove_file(&record);
    std::fs::write(
        &script,
        "#!/bin/sh\necho \"$XKB_DEFAULT_LAYOUT/${XKB_DEFAULT_OPTIONS:-}\" > \"$(dirname \"$0\")/recorded_xkb.tmp\"\nmv \"$(dirname \"$0\")/recorded_xkb.tmp\" \"$(dirname \"$0\")/recorded_xkb.txt\"\n",
    )
    .expect("stub launch.sh");
    // the config-check action: one the compositor starts after a config change
    ls.state.config_changed();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(s) = std::fs::read_to_string(&record) {
            return s.trim().to_string();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the action was not started");
}

#[test]
fn a_reload_builds_the_keymap_from_the_new_layout_and_options() {
    let temp = TestTempDir::new("t1_live_keymap");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));

    reload(&mut ls, temp.path(), "XKB_DEFAULT_LAYOUT=\"us\"\n");
    assert_eq!(sent(&ls), keymap("us", None));
    assert_eq!(action_names(&mut ls), "us/");

    reload(
        &mut ls,
        temp.path(),
        "XKB_DEFAULT_LAYOUT=\"us,de\"\nXKB_DEFAULT_OPTIONS=\"grp:alt_shift_toggle\"\n",
    );
    assert_eq!(
        sent(&ls),
        keymap("us,de", Some("grp:alt_shift_toggle")),
        "the apps' keyboards get the new keymap"
    );
    assert_eq!(
        action_names(&mut ls),
        "us,de/grp:alt_shift_toggle",
        "actions started after the reload get the new names"
    );

    // a layout that makes no keymap: "us", as at start
    reload(
        &mut ls,
        temp.path(),
        "XKB_DEFAULT_LAYOUT=\"no-such-layout\"\n",
    );
    assert_eq!(sent(&ls), keymap("us", None));
}
