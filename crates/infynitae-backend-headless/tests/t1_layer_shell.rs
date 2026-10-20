//! Layer surfaces on the production path (owner decisions 2026-10-21 L1–L8): production Core,
//! adapter and headless lockstep with a real `zwlr_layer_shell_v1` client. Expected values are
//! hand-derived: one 1920 × 1080 output at scale 1; a bar anchored top, left and right, 30 px high
//! with a 30-px exclusive zone, so the usable area is (0, 30, 1920, 1050).

mod common;

use common::Lockstep;
use common::input::*;
use infynitae_core::geom::CanvasPoint;
use infynitae_core::stage::ElementRef;
use infynitae_core::{ShellCommand, ShellStream};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    Anchor, KeyboardInteractivity,
};

fn setup() -> Rig {
    let mut ls = Lockstep::new(Some(common::output_k("A", (1920, 1080), 120, 60_000)));
    ls.bind_layer_shell();
    Rig::new(ls)
}

/// Creates a layer surface, waits for its configure, draws it (asking for a frame) and lets a
/// vblank pass. Returns its index and the size it was configured with.
fn layer(
    r: &mut Rig,
    label: &'static str,
    layer: Layer,
    anchor: Anchor,
    size: (u32, u32),
    zone: i32,
    kb: KeyboardInteractivity,
) -> (usize, (u32, u32)) {
    let idx =
        r.ls.probe
            .layer_surface(&r.ls.qh, label, layer, anchor, size, zone, kb);
    r.ls.pump();
    let configured =
        r.ls.probe
            .layer_configures
            .iter()
            .rev()
            .find(|(i, _)| *i == idx)
            .map(|(_, s)| *s)
            .expect("the initial commit is answered with a configure");
    r.ls.probe.frame_and_draw(&r.ls.qh, idx);
    r.ls.pump();
    r.frame();
    (idx, configured)
}

fn bar(r: &mut Rig) -> usize {
    let (idx, size) = layer(
        r,
        "bar",
        Layer::Top,
        Anchor::Top | Anchor::Left | Anchor::Right,
        (0, 30),
        30,
        KeyboardInteractivity::OnDemand,
    );
    assert_eq!(size, (1920, 30), "full width, its own height");
    idx
}

#[test]
fn a_bar_reserves_its_strip_and_a_new_window_is_placed_below_it() {
    let mut r = setup();
    let _bar = bar(&mut r);
    let o = r.ls.output();
    assert_eq!(
        r.ls.state.core().usable_area(o),
        Some((0.0, 30.0, 1920.0, 1050.0))
    );
    assert!(
        r.ls.probe.frames.iter().any(|(_, l)| *l == "bar"),
        "the bar's frame callback completed at the vblank: {:?}",
        r.ls.probe.frames
    );
    r.map_w();
    let core = r.ls.state.core();
    let w = *core.stage().mapped_elements().last().unwrap();
    // F-PLACE-1 in the usable area: centre y 555 (15 below the view centre), camera at 0
    let size = core.stage().tree(w).unwrap().geometry.size;
    assert_eq!(
        core.stage().anchor(w),
        Some(CanvasPoint {
            x: -i64::from(size.w) / 2,
            y: 15 - i64::from(size.h) / 2,
        })
    );
}

#[test]
fn the_pointer_and_a_click_reach_the_bar_and_not_the_window_under_it() {
    let mut r = setup();
    let _bar = bar(&mut r);
    r.map_w();
    // the window under the bar's point (960, 10): window geometry from screen x 860, y 0
    // (camera at 0, zoom 1: Canvas = screen − (960, 540))
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let element = ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: w,
    };
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                ShellCommand::MoveElement {
                    element,
                    to: (-100.0, -540.0),
                    stream: ShellStream::Atomic,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();
    let o = r.ls.output();
    let under =
        r.ls.state
            .core_mut()
            .hit_test(
                o,
                infynitae_core::geom::OutputLogicalPoint { x: 960.0, y: 10.0 },
            )
            .map(|h| h.element);
    assert_eq!(
        under,
        Some(w),
        "the window lies under the bar's point (precondition)"
    );
    let focus_before = r.ls.state.core().focus().target();
    assert_ne!(focus_before, Some(w));
    r.ls.probe.ptr.clear();
    // from the view centre (960, 540) to (960, 10): on the bar
    r.motion((0.0, -530.0));
    assert!(
        r.ls.probe.ptr.iter().any(|l| l == "enter bar 960,10"),
        "the pointer enters the bar at its own coordinates: {:?}",
        r.ls.probe.ptr
    );
    r.button(BTN_LEFT, true, Default::default());
    r.button(BTN_LEFT, false, Default::default());
    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some("bar"),
        "an on-demand bar is focused"
    );
    assert_eq!(
        r.ls.state.core().focus().target(),
        focus_before,
        "the window under the bar was not focused"
    );
}

#[test]
fn an_exclusive_launcher_takes_the_keyboard_and_gives_it_back() {
    let mut r = setup();
    r.map_w();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, Some(w))
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    let w_label =
        r.ls.probe
            .kb
            .active
            .clone()
            .expect("the window has the keyboard");
    let (launcher, size) = layer(
        &mut r,
        "launcher",
        Layer::Overlay,
        Anchor::empty(),
        (600, 400),
        0,
        KeyboardInteractivity::Exclusive,
    );
    assert_eq!(size, (600, 400));
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("launcher"));
    r.ls.probe.destroy_layer(launcher);
    r.ls.pump();
    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some(w_label.as_str()),
        "the keyboard returns to the window"
    );
    assert!(
        r.ls.probe.kb.violations.is_empty(),
        "{:?}",
        r.ls.probe.kb.violations
    );
}

#[test]
fn removing_the_output_closes_its_layer_surfaces() {
    let mut r = setup();
    let bar = bar(&mut r);
    let o = r.ls.output();
    r.ls.disconnect(o);
    assert_eq!(r.ls.probe.layer_closed, [bar]);
    assert!(r.ls.state.core().layers().is_empty());
}
