//! A window is drawn inside its window geometry only (CLIP_TO_GEOMETRY): the margins apps keep
//! around it for shadows and resize handles are not shown, so pixels the app left there are never
//! seen (owner, 2026-10-27: black blocks beside Discord's windows, gone after a resize). Its
//! border (BORDER_WIDTH) lies just outside the geometry, in the focused or unfocused colour.

mod common;

use common::input::*;
use infynitae_core::geom::{CanvasPos, Q16_ONE};
use infynitae_wayland::convert::{self, OutputElement};
use smithay::backend::renderer::element::Element;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::utils::Scale;

/// W: 800×600 content in a surface with a 20 px shadow margin on every side.
fn w_geometry(r: &Rig) -> (i32, i32, i32, i32) {
    let core = r.ls.state.core();
    let w = *core.stage().mapped_elements().last().unwrap();
    let a = core.stage().anchor(w).unwrap();
    let view = core.view(r.ls.output()).unwrap();
    let p = view.project(CanvasPos {
        x: a.x * Q16_ONE,
        y: a.y * Q16_ONE,
    });
    let (x, y) = (p.x.round() as i32, p.y.round() as i32);
    (x, y, x + 800, y + 600)
}

type Rects = Vec<smithay::utils::Rectangle<i32, smithay::utils::Physical>>;

/// The frame's surface rects and solid rects.
fn frame(r: &mut Rig) -> (Rects, Rects) {
    let o = r.ls.output();
    let mut renderer = PixmanRenderer::new().expect("pixman");
    let (elements, k) =
        convert::output_elements(&mut r.ls.state, &mut renderer, o).expect("elements");
    let mut surfaces = Vec::new();
    let mut solids = Vec::new();
    for e in &elements {
        match e {
            OutputElement::Surface(s) => surfaces.push(s.geometry(Scale::from(k))),
            OutputElement::Solid(s) => solids.push(s.geometry(Scale::from(k))),
            _ => {}
        }
    }
    (surfaces, solids)
}

#[test]
fn a_window_is_drawn_inside_its_geometry_with_its_border_just_outside() {
    let mut r = Rig::x8();
    r.ls.state.border_width = 1;
    let (x0, y0, x1, y1) = w_geometry(&r);
    let (surfaces, solids) = frame(&mut r);

    // the root surface, 840×640 with its margins, is cut to the 800×600 geometry
    assert_eq!(surfaces.len(), 1);
    let s = surfaces[0];
    assert_eq!(
        (s.loc.x, s.loc.y, s.loc.x + s.size.w, s.loc.y + s.size.h),
        (x0, y0, x1, y1),
        "drawn exactly over the window geometry, not its shadow margins"
    );

    // a 1 px border on the four sides, outside the geometry
    let border: Vec<_> = solids
        .iter()
        .map(|b| (b.loc.x, b.loc.y, b.loc.x + b.size.w, b.loc.y + b.size.h))
        .filter(|&(bx0, by0, bx1, by1)| {
            bx0 >= x0 - 1 && by0 >= y0 - 1 && bx1 <= x1 + 1 && by1 <= y1 + 1
        })
        .collect();
    assert_eq!(
        border,
        vec![
            (x0 - 1, y0 - 1, x1 + 1, y0),
            (x0 - 1, y1, x1 + 1, y1 + 1),
            (x0 - 1, y0, x0, y1),
            (x1, y0, x1 + 1, y1),
        ]
    );
}

#[test]
fn with_clip_to_geometry_off_the_whole_surface_is_drawn() {
    let mut r = Rig::x8();
    r.ls.state.clip_to_geometry = false;
    let (x0, y0, x1, y1) = w_geometry(&r);
    let (surfaces, _) = frame(&mut r);
    let s = surfaces[0];
    assert_eq!(
        (s.loc.x, s.loc.y, s.loc.x + s.size.w, s.loc.y + s.size.h),
        (x0 - 20, y0 - 20, x1 + 20, y1 + 20)
    );
}

/// SQUARE_CORNERS: a window is told it is tiled on every side, so an app that rounds its corners
/// draws them square, flush with the border; switched off, it is told nothing of the kind.
#[test]
fn windows_are_told_they_are_tiled_unless_square_corners_is_off() {
    const TILED: [u32; 4] = [5, 6, 7, 8];
    let mut r = Rig::x8();
    let states = |r: &Rig, label: &str| {
        let idx =
            r.ls.probe
                .surfaces
                .iter()
                .position(|s| s.label == label)
                .unwrap();
        r.ls.probe
            .toplevel_states
            .iter()
            .rev()
            .find(|(i, _)| *i == idx)
            .map(|(_, s)| s.clone())
            .unwrap()
    };
    assert!(TILED.iter().all(|t| states(&r, "W").contains(t)));

    r.ls.state.square_corners = false;
    let qh = r.ls.qh.clone();
    r.ls.probe.toplevel(&qh, "X", (300, 200), 0);
    r.ls.pump();
    r.frame();
    assert!(TILED.iter().all(|t| !states(&r, "X").contains(t)));
}

/// The border follows the focus: when the window loses it, the border's colour changes and so
/// does its commit, which is what makes the damage tracker repaint it (2026-10-27: the border
/// kept its focused colour after the focus moved).
#[test]
fn the_border_is_repainted_when_the_focus_moves() {
    use smithay::backend::renderer::element::solid::SolidColorRenderElement;
    let mut r = Rig::x8();
    r.ls.state.border_width = 1;
    let border = |r: &mut Rig| {
        let o = r.ls.output();
        let mut renderer = PixmanRenderer::new().expect("pixman");
        let (elements, _) =
            convert::output_elements(&mut r.ls.state, &mut renderer, o).expect("elements");
        let solid: Vec<&SolidColorRenderElement> = elements
            .iter()
            .filter_map(|e| match e {
                OutputElement::Solid(s) => Some(s),
                _ => None,
            })
            .collect();
        let first = solid.first().copied().expect("a border");
        (first.color(), first.current_commit())
    };
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    infynitae_backend_headless::set_focus(&mut r.ls.state, &mut r.ls.input, Some(w)).unwrap();
    let (on, c_on) = border(&mut r);
    assert_eq!(on, r.ls.state.border_active);
    infynitae_backend_headless::set_focus(&mut r.ls.state, &mut r.ls.input, None).unwrap();
    let (off, c_off) = border(&mut r);
    assert_eq!(off, r.ls.state.border_inactive);
    assert_ne!(c_on, c_off, "a new colour is a new commit");
    // unchanged: the same commit, nothing to repaint
    assert_eq!(border(&mut r).1, c_off);
}

/// A window's menu that opens over its edge is drawn over the window's border, not under it
/// (2026-10-27: the border line crossed Thunar's context menu). Front to back, the border comes
/// after every surface of its window, popups included.
#[test]
fn a_menu_over_the_windows_edge_covers_its_border() {
    use common::PopupSpec;
    let mut r = Rig::x8();
    r.ls.state.border_width = 1;
    let qh = r.ls.qh.clone();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    // a 200 x 300 menu anchored at W's bottom edge (W: 800 x 600 inside a 20 px margin)
    r.ls.probe.popup(
        &qh,
        "M",
        w_idx,
        PopupSpec {
            content: (200, 300),
            shadow: 0,
            anchor_rect: (100, 590, 1, 1),
            slide: false,
        },
    );
    r.ls.pump();
    r.frame();
    let o = r.ls.output();
    let mut renderer = PixmanRenderer::new().expect("pixman");
    let (elements, _) =
        convert::output_elements(&mut r.ls.state, &mut renderer, o).expect("elements");
    let kinds: Vec<&str> = elements
        .iter()
        .filter_map(|e| match e {
            OutputElement::Surface(_) => Some("surface"),
            OutputElement::Solid(_) => Some("border"),
            _ => None,
        })
        .collect();
    let surfaces = kinds.iter().filter(|k| **k == "surface").count();
    assert_eq!(surfaces, 2, "the window and its menu: {kinds:?}");
    let first_border = kinds.iter().position(|k| *k == "border").expect("a border");
    let last_surface = kinds.iter().rposition(|k| *k == "surface").unwrap();
    assert!(
        first_border > last_surface,
        "front to back the border is behind the window and its menu: {kinds:?}"
    );
}
