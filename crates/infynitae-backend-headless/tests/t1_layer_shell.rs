//! Production path tests for layer surfaces (owner decisions 2026-10-21, L1–L8): production Core,
//! adapter, and headless lockstep with a real `zwlr_layer_shell_v1` client. Expected values are
//! hand-derived: one 1920 × 1080 output at scale 1; a bar anchored top, left, and right, 30 px high
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

fn menu_spec(anchor_x: i32) -> common::PopupSpec {
    common::PopupSpec {
        content: (200, 300),
        shadow: 0,
        anchor_rect: (anchor_x, 29, 1, 1),
        slide: true,
    }
}

/// A panel's menu (`zwlr_layer_surface_v1.get_popup` with a grab on the click's serial): placed
/// under the bar, drawn and hit in front of the window under it, holding the pointer and the
/// keyboard while open. Window W (800 × 600) lies at screen (560, 255)–(1360, 855); the menu at
/// (900, 29)–(1100, 329) overlaps it (anchored to the bar's last row: an anchor rectangle is at
/// least 1 × 1).
fn open_menu(r: &mut Rig, bar: usize) -> usize {
    // from the view centre (960, 540) to the bar's point (960, 10), and click: the on-demand
    // bar is focused
    r.motion((0.0, -530.0));
    r.button(BTN_LEFT, true, Default::default());
    r.button(BTN_LEFT, false, Default::default());
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("bar"));
    let serial = r.ls.probe.last_serial;
    let qh = r.ls.qh.clone();
    let menu =
        r.ls.probe
            .layer_popup(&qh, "menu", bar, menu_spec(900), Some(serial));
    r.ls.pump();
    r.frame();
    assert_eq!(
        r.ls.probe.surfaces[menu].popup_configure,
        Some((900, 29, 200, 300)),
        "on the bar's last row, where the client asked"
    );
    assert!(r.ls.probe.popup_dones.is_empty(), "the grab was granted");
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("menu"));
    let o = r.ls.output();
    assert_eq!(r.ls.state.core().layer_frame(o).above.len(), 2);
    // (960, 10) → (950, 300): in the menu, over the window
    r.ls.probe.ptr.clear();
    r.motion((-10.0, 290.0));
    assert!(
        r.ls.probe.ptr.iter().any(|l| l == "enter menu 50,271"),
        "{:?}",
        r.ls.probe.ptr
    );
    menu
}

/// The client closes its menu (an item was chosen): the keyboard returns to the bar that holds
/// it, and the menu is gone from the bar's tree.
#[test]
fn a_bars_menu_holds_the_pointer_and_keyboard_and_gives_them_back() {
    let mut r = setup();
    let bar = bar(&mut r);
    r.map_w();
    let menu = open_menu(&mut r, bar);
    r.ls.probe.destroy_popup(menu);
    r.ls.pump();
    r.frame();
    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some("bar"),
        "the keyboard is back with the bar holding it"
    );
    let o = r.ls.output();
    assert_eq!(
        r.ls.state.core().layer_frame(o).above.len(),
        1,
        "the destroyed menu is gone from the bar's tree"
    );
    assert!(
        r.ls.probe.ptr.iter().any(|l| l == "enter W 410,65"),
        "the pointer is on the window again: {:?}",
        r.ls.probe.ptr
    );
    assert!(
        r.ls.probe.kb.violations.is_empty(),
        "{:?}",
        r.ls.probe.kb.violations
    );
}

/// A click outside the menu dismisses it, and the click is the window's: it is focused.
#[test]
fn a_click_outside_a_bars_menu_dismisses_it() {
    let mut r = setup();
    let bar = bar(&mut r);
    r.map_w();
    r.ls.probe.auto_destroy_popups = true;
    let _menu = open_menu(&mut r, bar);
    // (950, 300) → (1200, 600): on the window, outside the menu
    r.motion((250.0, 300.0));
    r.button(BTN_LEFT, true, Default::default());
    r.button(BTN_LEFT, false, Default::default());
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.probe.popup_dones, ["menu"]);
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("W"));
    let o = r.ls.output();
    assert_eq!(r.ls.state.core().layer_frame(o).above.len(), 1);
    assert!(
        r.ls.probe.kb.violations.is_empty(),
        "{:?}",
        r.ls.probe.kb.violations
    );
}

/// A layer popup is unconstrained against its whole output: a bar at the bottom whose menu would
/// open below the screen has it slid up into it (1080 − 300 = 780 on screen, −270 from the bar
/// at 1050).
#[test]
fn a_bottom_bars_menu_is_slid_into_the_output() {
    let mut r = setup();
    let (bar, _) = layer(
        &mut r,
        "bar",
        Layer::Top,
        Anchor::Bottom | Anchor::Left | Anchor::Right,
        (0, 30),
        30,
        KeyboardInteractivity::None,
    );
    let qh = r.ls.qh.clone();
    let menu =
        r.ls.probe
            .layer_popup(&qh, "menu", bar, menu_spec(900), None);
    r.ls.pump();
    r.frame();
    assert_eq!(
        r.ls.probe.surfaces[menu].popup_configure,
        Some((900, -270, 200, 300))
    );
}

/// A grab from a panel the user is not using (the pointer is on a window, the keyboard is not
/// the panel's) is refused: the menu is dismissed at once and nothing is taken from the window.
#[test]
fn a_menu_grab_from_a_panel_not_in_use_is_refused() {
    let mut r = setup();
    let bar = bar(&mut r);
    r.map_w();
    let serial = r.ls.probe.last_serial;
    let active = r.ls.probe.kb.active.clone();
    let qh = r.ls.qh.clone();
    r.ls.probe
        .layer_popup(&qh, "menu", bar, menu_spec(900), Some(serial));
    r.ls.pump();
    assert_eq!(r.ls.probe.popup_dones, ["menu"]);
    assert_eq!(r.ls.probe.kb.active, active);
}

/// A bar that unmaps (a null buffer) takes its open menu with it: the menu is dismissed, as a
/// window's popups are when it unmaps, and nothing of either is drawn. The bar never takes the
/// keyboard (interactivity none), so no keyboard change ends the menu's grab on the way.
#[test]
fn a_bar_that_unmaps_dismisses_its_menu() {
    let mut r = setup();
    let (bar, _) = layer(
        &mut r,
        "bar",
        Layer::Top,
        Anchor::Top | Anchor::Left | Anchor::Right,
        (0, 30),
        30,
        KeyboardInteractivity::None,
    );
    r.map_w();
    r.motion((0.0, -530.0));
    r.button(BTN_LEFT, true, Default::default());
    r.button(BTN_LEFT, false, Default::default());
    let serial = r.ls.probe.last_serial;
    let qh = r.ls.qh.clone();
    r.ls.probe
        .layer_popup(&qh, "menu", bar, menu_spec(900), Some(serial));
    r.ls.pump();
    r.frame();
    assert!(r.ls.probe.popup_dones.is_empty(), "the grab was granted");
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("menu"));
    r.ls.probe.unmap(bar);
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.probe.popup_dones, ["menu"]);
    let o = r.ls.output();
    assert!(r.ls.state.core().layer_frame(o).above.is_empty());
}

#[test]
#[ignore = "AUDIT_LAYERS: pre-first-buffer unmap never receives remap configure"]
fn audit_layers_unmap_before_first_buffer_never_configures_on_remap() {
    let mut r = setup();
    let idx = r.ls.probe.layer_surface(
        &r.ls.qh,
        "panel",
        Layer::Top,
        Anchor::Top | Anchor::Left | Anchor::Right,
        (0, 30),
        30,
        KeyboardInteractivity::None,
    );
    r.ls.pump();
    assert!(
        r.ls.probe.layer_configures.iter().any(|(i, _)| *i == idx),
        "initial configure arrived"
    );

    // Client unmaps (attach(None)) before drawing a first buffer, as edge-apps does under fullscreen.
    r.ls.probe.unmap(idx);
    r.ls.pump();
    r.ls.probe.layer_configures.clear();

    // Client later commits empty to remap, as edge-apps does after fullscreen ends.
    r.ls.probe.surfaces[idx].wl.commit();
    r.ls.pump();
    assert!(
        r.ls.probe.layer_configures.iter().any(|(i, _)| *i == idx),
        "layer waiting to remap after pre-first-buffer unmap must receive configure"
    );
}

/// The edge's panel opened by its key, as the edge does it: one layer surface over the whole
/// output, its input region the panel's box. It asks for the keyboard exclusively when it opens
/// and on demand once the keyboard has come. A click on a window then gives the window the
/// keyboard, and the first key typed reaches the window. In the test VM (2026-11-22) a terminal
/// clicked after `infynitae-edge control` got "ouch" for "touch"; this sequence passes, so the
/// compositor routes the key right whenever the edge has asked on demand.
fn key_opened_panel_then_a_click_on_the_window(click_inside: bool) {
    let mut r = setup();
    r.ls.state.core_mut().set_focus_follows_mouse(true);
    r.ls.state.core_mut().set_focus_follows_mouse_strict(true);
    r.map_w();
    // W at screen (560, 255)–(1360, 855): the pointer goes onto it and clicks
    r.motion((-200.0, 0.0));
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("W"));
    let (edge, size) = layer(
        &mut r,
        "edge",
        Layer::Top,
        Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        (0, 0),
        -1,
        KeyboardInteractivity::None,
    );
    assert_eq!(size, (1920, 1080));
    let qh = r.ls.qh.clone();
    r.ls.probe.input_region(&qh, edge, &[(true, (0, 0, 0, 0))]);
    r.ls.pump();
    // to empty canvas at (300, 300): strict focus leaves W
    r.motion((-460.0, -240.0));
    // the panel opens at (1540, 680)–(1920, 1080)
    r.ls.probe
        .layer_of(edge)
        .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    r.ls.probe
        .input_region(&qh, edge, &[(true, (1540, 680, 380, 400))]);
    r.ls.probe.frame_and_draw(&qh, edge);
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("edge"));
    // the keyboard came: from now on on demand
    r.ls.probe
        .layer_of(edge)
        .set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
    r.ls.probe.frame_and_draw(&qh, edge);
    r.ls.pump();
    r.frame();
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("edge"));
    if click_inside {
        // (300, 300) → (1730, 700)
        r.motion((1430.0, 400.0));
        r.button(BTN_LEFT, true, none());
        r.button(BTN_LEFT, false, none());
        assert_eq!(r.ls.probe.kb.active.as_deref(), Some("edge"));
        r.motion((-1430.0, -400.0));
    }
    // (300, 300) → (760, 540) on W, and a click on it
    r.motion((460.0, 240.0));
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    let before = r.ls.probe.kb_log.len();
    r.t += 1000;
    infynitae_backend_headless::keyboard_key(&mut r.ls.state, &mut r.ls.input, 20, true, r.t)
        .expect("key");
    r.ls.pump();
    assert_eq!(
        r.ls.probe.kb_log[before..],
        ["wl_keyboard.key 20 pressed"],
        "{:?}",
        r.ls.probe.kb_log
    );
    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some("W"),
        "the first key after the click is the window's: {:?}",
        r.ls.probe.kb_log
    );
}

#[test]
fn a_click_on_a_window_after_a_key_opened_panel_gets_the_first_key() {
    key_opened_panel_then_a_click_on_the_window(false);
}

#[test]
fn a_click_on_a_window_after_a_click_in_a_key_opened_panel_gets_the_first_key() {
    key_opened_panel_then_a_click_on_the_window(true);
}

/// The edge's closed surface under a fullscreen window, hidden one of two ways and shown again
/// when the window leaves fullscreen. Returns whether the surface still had its texture when it
/// was shown again, before the next frame was made.
fn edge_hidden_under_fullscreen_keeps_its_texture(unmap: bool) -> bool {
    use infynitae_wayland::convert;
    use smithay::backend::renderer::Renderer;
    use smithay::backend::renderer::pixman::{PixmanRenderer, PixmanTexture};
    use smithay::backend::renderer::utils::RendererSurfaceStateUserData;

    let mut r = setup();
    r.map_w();
    // W is focused: it may go fullscreen
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    let (edge, size) = layer(
        &mut r,
        "edge",
        Layer::Top,
        Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        (0, 0),
        -1,
        KeyboardInteractivity::None,
    );
    assert_eq!(size, (1920, 1080));
    let o = r.ls.output();
    let key = r.ls.state.core().layer_frame(o).above[0].surface;
    let mut renderer = PixmanRenderer::new().expect("pixman");
    let ctx = renderer.context_id();
    let has_texture = |r: &Rig| {
        let surface =
            r.ls.state
                .registry()
                .surface(key)
                .expect("the edge's surface");
        smithay::wayland::compositor::with_states(surface, |states| {
            states
                .data_map
                .get::<RendererSurfaceStateUserData>()
                .is_some_and(|d| {
                    d.lock()
                        .unwrap()
                        .texture::<PixmanTexture>(ctx.clone())
                        .is_some()
                })
        })
    };
    convert::output_elements(&mut r.ls.state, &mut renderer, o).expect("elements");
    assert!(has_texture(&r), "the first frame made its texture");

    let toplevel = r.ls.probe.by_label("W").toplevel.clone().unwrap();
    toplevel.set_fullscreen(None);
    r.ls.pump();
    r.frame();
    assert!(
        r.ls.state.core().presentation(o).is_some(),
        "W is fullscreen"
    );
    if unmap {
        r.ls.probe.unmap(edge);
    } else {
        r.ls.probe.layer_of(edge).set_layer(Layer::Bottom);
        r.ls.probe.surfaces[edge].wl.commit();
    }
    r.ls.pump();
    r.frame();
    let frame = r.ls.state.core().layer_frame(o);
    assert!(
        frame
            .above
            .iter()
            .chain(&frame.below)
            .all(|e| e.surface != key),
        "nothing of the edge is drawn over the fullscreen window"
    );
    convert::output_elements(&mut r.ls.state, &mut renderer, o).expect("elements");

    toplevel.unset_fullscreen();
    r.ls.pump();
    r.frame();
    assert!(r.ls.state.core().presentation(o).is_none());
    if unmap {
        // as the edge did until 2026-11-23: a commit to map again, the configure, a clear buffer
        r.ls.probe.layer_configures.clear();
        r.ls.probe.surfaces[edge].wl.commit();
        r.ls.pump();
        assert!(
            r.ls.probe.layer_configures.iter().any(|(i, _)| *i == edge),
            "the commit after the null buffer is answered with a configure"
        );
        r.ls.probe.draw(&r.ls.qh.clone(), edge);
    } else {
        r.ls.probe.layer_of(edge).set_layer(Layer::Top);
        r.ls.probe.surfaces[edge].wl.commit();
    }
    r.ls.pump();
    r.frame();
    assert!(
        r.ls.state
            .core()
            .layer_frame(o)
            .above
            .iter()
            .any(|e| e.surface == key),
        "the edge is over the canvas again"
    );
    has_texture(&r)
}

/// Moved to the Bottom layer under a fullscreen window, the edge's surface keeps its buffer and
/// its texture: shown again, nothing is uploaded. Unmapped (a null buffer), as the edge did until
/// 2026-11-23, it lost both, and its whole 1920x1080 buffer was uploaded again, 7-10 ms in a slow
/// "layers above" turn each time a fullscreen window left (session log, 2026-11-22).
#[test]
fn the_edge_moved_below_a_fullscreen_window_keeps_its_texture() {
    assert!(edge_hidden_under_fullscreen_keeps_its_texture(false));
    assert!(!edge_hidden_under_fullscreen_keeps_its_texture(true));
}
