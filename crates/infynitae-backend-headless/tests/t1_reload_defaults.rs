//! A setting removed from the config goes back to its default on reload; WALLPAPER_PARALLAX applies on save.

mod common;

use std::time::Duration;

use common::{Lockstep, output};
use infynitae_wayland::DEFAULT_FRAME_GUARD;
use infynitae_wayland::cursor::TestTempDir;
use infynitae_wayland::frame_schedule::LatencyMode;

fn write_config(home: &std::path::Path, content: &str) {
    let dir = home.join(".config/infynitae");
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::write(dir.join("config"), content).expect("write config");
}

fn reload(ls: &mut Lockstep, home: &std::path::Path, content: &str) {
    write_config(home, content);
    ls.state.set_config_home(home.to_path_buf());
    ls.state.reload_config();
    ls.pump();
}

fn wait_for_reload_action(
    live_dir: &std::path::Path,
    expected_count: usize,
    timeout: Duration,
) -> Vec<String> {
    let path = live_dir.join("recorded_reload.txt");
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let lines: Vec<String> = content
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if lines.len() >= expected_count {
                return lines;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::read_to_string(&path)
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn fit_is_fullscreen_resets_to_default_when_removed() {
    let temp = TestTempDir::new("t1_reload_fit_is_fullscreen");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    reload(&mut ls, temp.path(), "FIT_IS_FULLSCREEN=\"0\"\n");
    assert!(!ls.state.fitted.enabled);
    reload(&mut ls, temp.path(), "");
    assert!(
        ls.state.fitted.enabled,
        "removed line restores default true"
    );
}

#[test]
fn latency_resets_to_default_when_removed() {
    let temp = TestTempDir::new("t1_reload_latency");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    reload(&mut ls, temp.path(), "LATENCY=\"lowest\"\n");
    assert_eq!(ls.state.latency_mode, LatencyMode::Lowest);
    reload(&mut ls, temp.path(), "");
    assert_eq!(
        ls.state.latency_mode,
        LatencyMode::Direct,
        "removed line restores default direct"
    );
}

#[test]
fn direct_scanout_resets_to_default_when_removed() {
    let temp = TestTempDir::new("t1_reload_direct_scanout");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    reload(&mut ls, temp.path(), "DIRECT_SCANOUT=\"false\"\n");
    assert_eq!(ls.state.direct_scanout, Some(false));
    reload(&mut ls, temp.path(), "");
    assert_eq!(
        ls.state.direct_scanout,
        Some(true),
        "removed line restores default true"
    );
}

#[test]
fn hardware_cursor_resets_to_default_when_removed() {
    let temp = TestTempDir::new("t1_reload_hardware_cursor");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    reload(&mut ls, temp.path(), "HARDWARE_CURSOR=\"false\"\n");
    assert!(!ls.state.hardware_cursor);
    reload(&mut ls, temp.path(), "");
    assert!(
        ls.state.hardware_cursor,
        "removed line restores default true"
    );
}

#[test]
fn overlay_planes_resets_to_default_when_removed() {
    let temp = TestTempDir::new("t1_reload_overlay_planes");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    reload(&mut ls, temp.path(), "OVERLAY_PLANES=\"true\"\n");
    assert!(ls.state.overlay_planes);
    reload(&mut ls, temp.path(), "");
    assert!(
        !ls.state.overlay_planes,
        "removed line restores default false"
    );
}

#[test]
fn wine_transparency_resets_to_default_when_removed() {
    let temp = TestTempDir::new("t1_reload_wine_transparency");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    reload(&mut ls, temp.path(), "WINE_TRANSPARENCY=\"false\"\n");
    assert!(!ls.state.wine_transparency);
    reload(&mut ls, temp.path(), "");
    assert!(
        ls.state.wine_transparency,
        "removed line restores default true"
    );
}

#[test]
fn frame_guard_resets_to_default_when_removed() {
    let temp = TestTempDir::new("t1_reload_frame_guard");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    reload(&mut ls, temp.path(), "FRAME_GUARD=\"60\"\n");
    assert_eq!(ls.state.frame_guard_percent, 60);
    reload(&mut ls, temp.path(), "");
    assert_eq!(
        ls.state.frame_guard_percent, DEFAULT_FRAME_GUARD,
        "removed line restores default 33"
    );
}

#[test]
fn wallpaper_parallax_change_triggers_respawn() {
    let temp = TestTempDir::new("t1_reload_parallax");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    reload(&mut ls, temp.path(), "WALLPAPER_PARALLAX=\"off\"\n");
    let recorded = wait_for_reload_action(&ls.live_dir, 1, Duration::from_millis(50));
    assert!(recorded.is_empty(), "first load should not spawn helper");

    reload(&mut ls, temp.path(), "WALLPAPER_PARALLAX=\"subtle\"\n");
    let recorded = wait_for_reload_action(&ls.live_dir, 1, Duration::from_millis(500));
    assert_eq!(recorded, vec!["wallpaper"]);

    reload(&mut ls, temp.path(), "WALLPAPER_PARALLAX=\"strong\"\n");
    let recorded = wait_for_reload_action(&ls.live_dir, 2, Duration::from_millis(500));
    assert_eq!(recorded, vec!["wallpaper", "wallpaper"]);

    reload(&mut ls, temp.path(), "");
    let recorded = wait_for_reload_action(&ls.live_dir, 3, Duration::from_millis(500));
    assert_eq!(recorded, vec!["wallpaper", "wallpaper", "wallpaper"]);

    reload(&mut ls, temp.path(), "");
    std::thread::sleep(Duration::from_millis(50));
    let recorded = wait_for_reload_action(&ls.live_dir, 4, Duration::from_millis(50));
    assert_eq!(recorded.len(), 3, "unchanged parallax should not respawn");
}
