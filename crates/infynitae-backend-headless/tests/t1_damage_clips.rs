//! No element of a frame hands the damage tracker an empty rectangle: the console backend gives
//! the frame's damage to the kernel as the plane's damage clips, and the kernel refuses the whole
//! page flip for an empty one (owner, 2026-10-25: "invalid damage clip 1126 720 1126 747" at the
//! right edge of Discord's updater froze the screen at every start).

mod common;

use common::input::*;
use infynitae_wayland::convert;
use smithay::backend::renderer::element::Element;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::utils::Scale;

/// An opaque region reaching past the window's right and bottom edges (Chromium sets one) is
/// clipped to the window without leaving a 0-wide or 0-high rectangle.
#[test]
fn an_opaque_region_past_the_edge_leaves_no_empty_rectangle() {
    let mut r = Rig::x8();
    let qh = r.ls.qh.clone();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let (w, h) = r.ls.probe.surfaces[w_idx].size;
    let region =
        r.ls.probe
            .compositor
            .as_ref()
            .expect("wl_compositor")
            .create_region(&qh, ());
    region.add(0, 0, w, h - 27);
    region.add(w, h - 27, 40, 27);
    region.add(0, h, w, 30);
    let wl = r.ls.probe.surfaces[w_idx].wl.clone();
    wl.set_opaque_region(Some(&region));
    region.destroy();
    r.ls.probe.draw(&qh, w_idx);
    r.ls.pump();
    r.frame();
    let o = r.ls.output();
    let mut renderer = PixmanRenderer::new().expect("pixman");
    let (elements, k) =
        convert::output_elements(&mut r.ls.state, &mut renderer, o).expect("elements");
    let opaque: Vec<_> = elements
        .iter()
        .flat_map(|e| e.opaque_regions(Scale::from(k)).into_iter())
        .collect();
    assert!(!opaque.is_empty(), "the window has an opaque region");
    let empty: Vec<_> = opaque.iter().filter(|o| o.is_empty()).collect();
    assert!(
        empty.is_empty(),
        "empty opaque rectangles: {empty:?} of {opaque:?}"
    );
}
