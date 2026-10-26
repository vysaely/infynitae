//! Captures that wait for a change (`screencopy::CaptureDamage`, `ScreencopyState::defer`): a
//! consumer's first capture is the whole frame; an unchanged screen gives none, so the capture
//! waits; a change gives just its rectangle. Each consumer keeps its own history, and a size
//! change starts it over. Waiting frames come back at the screen's next redraw and are
//! forgotten when destroyed.

mod common;

use std::time::Instant;

use common::{Lockstep, output};
use infynitae_wayland::screencopy::CaptureDamage;
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

wayland_client::delegate_noop!(common::Probe: ignore ZwlrScreencopyManagerV1);
wayland_client::delegate_noop!(common::Probe: ignore ZwlrScreencopyFrameV1);
use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::utils::{Physical, Rectangle, Transform};

fn solid(id: &Id, rect: (i32, i32, i32, i32), commit: usize, red: f32) -> SolidColorRenderElement {
    let r: Rectangle<i32, Physical> =
        Rectangle::new((rect.0, rect.1).into(), (rect.2, rect.3).into());
    SolidColorRenderElement::new(
        id.clone(),
        r,
        commit,
        Color32F::new(red, 0.0, 0.0, 1.0),
        Kind::Unspecified,
    )
}

#[test]
fn only_a_change_is_captured_and_just_where_it_changed() {
    let (a, b) = (1, 2);
    let mut damage = CaptureDamage::<u32>::default();
    let (bg, win) = (Id::new(), Id::new());
    let now = Instant::now();
    let size = (640, 480);
    let frame = |commit: usize, x: i32| {
        vec![
            solid(&win, (x, 100, 50, 40), commit, 1.0),
            solid(&bg, (0, 0, 640, 480), 0, 0.2),
        ]
    };

    assert_eq!(
        damage.damage(&a, size, 1.0, Transform::Normal, &frame(0, 10), now),
        Some(vec![(0, 0, 640, 480)]),
        "the first capture is all of it"
    );
    assert_eq!(
        damage.damage(&a, size, 1.0, Transform::Normal, &frame(0, 10), now),
        None,
        "nothing changed: the capture waits"
    );
    let changed = damage
        .damage(&a, size, 1.0, Transform::Normal, &frame(1, 10), now)
        .expect("the window redrew");
    assert_eq!(changed, vec![(10, 100, 50, 40)], "just the window");

    // another consumer has its own history
    assert_eq!(
        damage.damage(&b, size, 1.0, Transform::Normal, &frame(1, 10), now),
        Some(vec![(0, 0, 640, 480)])
    );
    // a moved window: where it was and where it is
    let moved = damage
        .damage(&a, size, 1.0, Transform::Normal, &frame(1, 300), now)
        .expect("moved");
    let covers = |x: i32| {
        moved
            .iter()
            .any(|r| r.0 <= x && x < r.0 + r.2 && r.1 <= 100)
    };
    assert!(covers(10) && covers(300), "{moved:?}");
    // a new size starts over
    assert_eq!(
        damage.damage(&a, (800, 600), 1.0, Transform::Normal, &frame(1, 300), now),
        Some(vec![(0, 0, 800, 600)])
    );
}

#[test]
fn a_waiting_capture_comes_back_at_the_next_redraw_and_a_destroyed_one_is_forgotten() {
    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    let o = ls.output();
    ls.state.screencopy.enable_capture();
    let (name, _, _) = ls
        .probe
        .globals
        .iter()
        .find(|(_, i, _)| i == "zwlr_screencopy_manager_v1")
        .cloned()
        .expect("screencopy offered");
    let qh = ls.qh.clone();
    let m: ZwlrScreencopyManagerV1 = ls.registry().bind(name, 3, &qh, ());
    let wl_output = ls
        .probe
        .outputs
        .first()
        .map(|(_, o)| o.clone())
        .expect("wl_output");
    let frame = m.capture_output(0, &wl_output, &qh, ());
    ls.pump();
    let buffer = ls.probe.buffer(&qh, 640, 480);
    frame.copy_with_damage(&buffer);
    ls.pump();

    let taken = ls.state.screencopy.take_pending(o);
    assert_eq!(taken.len(), 1);
    assert!(taken[0].consumer.is_some(), "copy_with_damage may wait");
    ls.state.screencopy.defer(taken);
    assert!(
        !ls.state.screencopy.has_pending(o),
        "waiting is not pending"
    );
    ls.state.screencopy.wake_waiting(o);
    assert!(ls.state.screencopy.has_pending(o), "back at the redraw");
    let taken = ls.state.screencopy.take_pending(o);
    let id = taken[0].id();
    ls.state.screencopy.defer(taken);
    ls.state.screencopy.forget(&id);
    ls.state.screencopy.wake_waiting(o);
    assert!(!ls.state.screencopy.has_pending(o), "destroyed: gone");
    frame.destroy();
}
