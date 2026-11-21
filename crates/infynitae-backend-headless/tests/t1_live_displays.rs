//! OUTPUT_SCALE, OUTPUT_<NAME>_SCALE, OUTPUT_ORDER and OUTPUT_LAYOUT apply on a config reload to
//! the screens connected now: each takes its new scale, and they are placed side by side again
//! in the new order. The first reload records what the screens started with.

mod common;

use common::{Lockstep, output_k};
use infynitae_core::navigation::PersistedCameras;
use infynitae_wayland::cursor::TestTempDir;

fn reload(ls: &mut Lockstep, home: &std::path::Path, config: &str) {
    let dir = home.join(".config/infynitae");
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::write(dir.join("config"), config).expect("config");
    ls.state.set_config_home(home.to_path_buf());
    ls.state.reload_config();
    ls.pump();
}

/// The screen on `connector`: its scale and where it is placed.
fn screen(ls: &Lockstep, connector: &str) -> (f64, (f64, f64)) {
    let o = ls
        .state
        .core()
        .outputs()
        .iter()
        .find(|o| o.key.connector == connector)
        .expect("the screen is connected");
    (o.scale, o.arrangement)
}

#[test]
fn a_reload_gives_the_screens_their_new_scales_and_order() {
    let temp = TestTempDir::new("t1_live_displays");
    let mut ls = Lockstep::with(
        PersistedCameras::default(),
        &[
            output_k("A", (1920, 1080), 120, 60_000),
            output_k("B", (2560, 1440), 120, 60_000),
        ],
    );
    reload(&mut ls, temp.path(), "OUTPUT_SCALE=\"1.0\"\n");
    assert_eq!(screen(&ls, "A"), (1.0, (0.0, 0.0)));
    assert_eq!(screen(&ls, "B"), (1.0, (1920.0, 0.0)));

    reload(&mut ls, temp.path(), "OUTPUT_SCALE=\"2\"\n");
    assert_eq!(screen(&ls, "A"), (2.0, (0.0, 0.0)));
    assert_eq!(
        screen(&ls, "B"),
        (2.0, (960.0, 0.0)),
        "next to A at its new width"
    );

    reload(
        &mut ls,
        temp.path(),
        "OUTPUT_SCALE=\"2\"\nOUTPUT_B_SCALE=\"1.25\"\n",
    );
    assert_eq!(screen(&ls, "A"), (2.0, (0.0, 0.0)));
    assert_eq!(screen(&ls, "B"), (1.25, (960.0, 0.0)), "its own line wins");

    reload(
        &mut ls,
        temp.path(),
        "OUTPUT_SCALE=\"2\"\nOUTPUT_B_SCALE=\"1.25\"\nOUTPUT_ORDER=\"B,A\"\n",
    );
    assert_eq!(screen(&ls, "B"), (1.25, (0.0, 0.0)));
    assert_eq!(
        screen(&ls, "A"),
        (2.0, (2048.0, 0.0)),
        "2560 / 1.25 to the right"
    );

    // every line removed: scale 1, the order as it is now
    reload(&mut ls, temp.path(), "");
    assert_eq!(screen(&ls, "B"), (1.0, (0.0, 0.0)));
    assert_eq!(screen(&ls, "A"), (1.0, (2560.0, 0.0)));

    // the layout turned: the screens change sides
    reload(&mut ls, temp.path(), "OUTPUT_LAYOUT=\"right-to-left\"\n");
    assert_eq!(screen(&ls, "A"), (1.0, (0.0, 0.0)));
    assert_eq!(screen(&ls, "B"), (1.0, (1920.0, 0.0)));

    // a reload that changes no screen setting leaves a screen a tool moved where it is
    let b = ls
        .state
        .core()
        .outputs()
        .iter()
        .find(|o| o.key.connector == "B")
        .unwrap()
        .id;
    let fx = ls
        .state
        .core_mut()
        .output_arranged(&mut ls.token, b, (1920, 500))
        .expect("arranged");
    ls.state.apply(&fx);
    reload(
        &mut ls,
        temp.path(),
        "OUTPUT_LAYOUT=\"right-to-left\"\nBORDER_WIDTH=\"2\"\n",
    );
    assert_eq!(screen(&ls, "B"), (1.0, (1920.0, 500.0)));
}
