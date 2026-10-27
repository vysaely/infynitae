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
