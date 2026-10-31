//! MAX_APP_SCALE (the scale policy's ceiling, PROV-1) from the config: a reload sets it, never
//! below a screen's own scale and at most 8; absent or invalid it is the default. That no window
//! is asked beyond the ceiling is Core's own rule (F-SCALE-2, its scale tests).

mod common;

use common::{Lockstep, output};
use infynitae_wayland::DEFAULT_MAX_APP_SCALE;
use infynitae_wayland::cursor::TestTempDir;

fn reload(ls: &mut Lockstep, home: &std::path::Path, config: &str) -> f64 {
    let dir = home.join(".config/infynitae");
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::write(dir.join("config"), config).expect("config");
    ls.state.set_config_home(home.to_path_buf());
    ls.state.reload_config();
    ls.pump();
    ls.state.core().scale_config().h_cfg
}

#[test]
fn the_config_sets_the_ceiling_within_its_bounds() {
    let temp = TestTempDir::new("t1_max_app_scale");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    assert_eq!(reload(&mut ls, temp.path(), "MAX_APP_SCALE=\"1.5\"\n"), 1.5);
    assert_eq!(
        reload(&mut ls, temp.path(), "MAX_APP_SCALE=\"20\"\n"),
        8.0,
        "at most 8"
    );
    assert_eq!(
        reload(&mut ls, temp.path(), "MAX_APP_SCALE=\"0.5\"\n"),
        1.0,
        "never below the screen's scale"
    );
    assert_eq!(
        reload(&mut ls, temp.path(), "MAX_APP_SCALE=\"x\"\n"),
        DEFAULT_MAX_APP_SCALE
    );
    assert_eq!(reload(&mut ls, temp.path(), "MAX_APP_SCALE=\"2\"\n"), 2.0);
    assert_eq!(
        reload(&mut ls, temp.path(), ""),
        DEFAULT_MAX_APP_SCALE,
        "removed: the default"
    );
}

#[test]
fn a_hidpi_screen_keeps_its_own_scale() {
    let temp = TestTempDir::new("t1_max_app_scale_hidpi");
    let mut ls = Lockstep::new(Some(output((3840, 2160), 240)));
    assert_eq!(reload(&mut ls, temp.path(), "MAX_APP_SCALE=\"1\"\n"), 2.0);
}
