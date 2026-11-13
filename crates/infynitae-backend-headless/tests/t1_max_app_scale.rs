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

#[test]
fn zooming_in_with_fsr_keeps_app_scale_untouched() {
    use infynitae_backend_headless::{keyboard_key, pointer_axis};
    use infynitae_core::input::keys::{KEY_LEFTMETA, KEY_LEFTSHIFT};
    use infynitae_core::input::{Axis, AxisSource};
    use std::time::Duration;

    let temp = TestTempDir::new("t1_fsr_zoom_no_app_scale");
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));

    let dir = temp.path().join(".config/infynitae");
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::write(dir.join("config"), "SCALING_FILTER=\"fsr\"\n").expect("config");
    ls.state.set_config_home(temp.path().to_path_buf());
    ls.state.reload_config();
    ls.pump();
    assert_eq!(ls.state.core().scale_config().h_cfg, DEFAULT_MAX_APP_SCALE);

    let w = ls
        .probe
        .toplevel_with_fractional_scale(&ls.qh, "W", (800, 600), 0);
    ls.pump();
    ls.probe.draw(&ls.qh, w);
    ls.pump();
    ls.run_until(ls.now + Duration::from_millis(20));

    let initial_scales: Vec<u32> = ls
        .probe
        .preferred_scales
        .iter()
        .filter(|(label, _)| label == "W")
        .map(|(_, s)| *s)
        .collect();
    let initial_bufs: Vec<i32> = ls
        .probe
        .preferred_buffer_scales
        .iter()
        .filter(|(label, _)| label == "W")
        .map(|(_, b)| *b)
        .collect();
    assert_eq!(initial_scales, vec![120]);
    assert_eq!(initial_bufs, Vec::<i32>::new());

    // Zoom camera in with Super+Shift+wheel and settle past 300ms.
    let mut t = 10_000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, t).unwrap();
    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTSHIFT, true, t).unwrap();
    for step in [-120, -120, -120, -120] {
        t += 16_000;
        pointer_axis(
            &mut ls.state,
            &mut ls.input,
            Axis {
                source: AxisSource::Wheel,
                amount: (0.0, f64::from(step) / 8.0),
                v120: (0, step),
            },
            t,
        )
        .unwrap();
        ls.pump();
        ls.run_until(ls.now + Duration::from_millis(17));
    }
    t += 16_000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTSHIFT, false, t).unwrap();
    t += 1000;
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, t).unwrap();
    ls.pump();
    ls.run_until(ls.now + Duration::from_millis(600));

    assert!(
        ls.state.core().cameras().zoom(0).unwrap() > 1.2,
        "camera zoomed in, got {}",
        ls.state.core().cameras().zoom(0).unwrap()
    );

    let after_scales: Vec<u32> = ls
        .probe
        .preferred_scales
        .iter()
        .filter(|(label, _)| label == "W")
        .map(|(_, s)| *s)
        .collect();
    let after_bufs: Vec<i32> = ls
        .probe
        .preferred_buffer_scales
        .iter()
        .filter(|(label, _)| label == "W")
        .map(|(_, b)| *b)
        .collect();
    assert_eq!(
        after_scales, initial_scales,
        "apps must never be asked to redraw at a higher scale when zooming in"
    );
    assert_eq!(after_bufs, initial_bufs);
}
