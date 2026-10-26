//! Capturing a rotated screen (a convertible in tablet mode, a portrait monitor): the capture is
//! in the panel's own orientation, drawn through the screen's rotation as the screen itself is
//! (as wlroots gives it; grim and xdg-desktop-portal-wlr turn it with `wl_output.transform`).
//! A region of what the screen shows is cut from that buffer where `capture_region_in_buffer`
//! says, and rendering the region directly (the GPU path) gives the same pixels at the size
//! `capture_buffer_size` gives. This drives the console backend's two capture paths the way it
//! runs them, with the pixman renderer.

mod common;

use common::Lockstep;
use common::input::*;
use infynitae_core::output::Transform;
use infynitae_wayland::convert;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::utils::{Relocate, RelocateRenderElement};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, Color32F, ExportMem, Offscreen};
use smithay::reexports::pixman::Image;
use smithay::utils::{Point, Rectangle, Scale};
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1;

wayland_client::delegate_noop!(common::Probe: ignore WpSinglePixelBufferManagerV1);

const SQUARE: u32 = 0xFF12_AB34;

/// A 1920×1080 panel turned by `transform`, with W mapped and a 50×50 square subsurface in it.
fn rig(transform: Transform) -> Rig {
    let mut out = common::output((1920, 1080), 120);
    out.transform = transform;
    let mut r = Rig::new(Lockstep::new(Some(out)));
    r.map_w();
    let qh = r.ls.qh.clone();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let s_idx =
        r.ls.probe
            .subsurface_uncommitted(&qh, "S", w_idx, (30, 40), (50, 50), false);
    let (name, _, _) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "wp_single_pixel_buffer_manager_v1")
            .cloned()
            .expect("single pixel buffers");
    let spb: WpSinglePixelBufferManagerV1 = r.ls.registry().bind(name, 1, &qh, ());
    let ch = |v: u32| v * 0x0101_0101;
    let buffer = spb.create_u32_rgba_buffer(ch(0x12), ch(0xAB), ch(0x34), u32::MAX, &qh, ());
    let s = r.ls.probe.surfaces[s_idx].wl.clone();
    let viewport =
        r.ls.probe
            .viewporter
            .as_ref()
            .expect("wp_viewporter")
            .get_viewport(&s, &qh, ());
    viewport.set_destination(50, 50);
    s.attach(Some(&buffer), 0, 0);
    s.damage_buffer(0, 0, 1, 1);
    s.commit();
    r.ls.probe.surfaces[w_idx].wl.commit();
    r.ls.pump();
    r.frame();
    r
}

/// Renders `elements` into a `size` buffer through `transform`, the pixels row by row.
fn render<E>(
    renderer: &mut PixmanRenderer,
    elements: &[E],
    size: (i32, i32),
    k: f64,
    transform: Transform,
) -> Vec<u32>
where
    E: smithay::backend::renderer::element::RenderElement<PixmanRenderer>,
{
    let mut image: Image<'static, 'static> = renderer
        .create_buffer(Fourcc::Argb8888, size.into())
        .expect("target");
    let mut target = renderer.bind(&mut image).expect("bind");
    let mut tracker =
        OutputDamageTracker::new(size, Scale::from(k), convert::output_transform(transform));
    tracker
        .render_output(renderer, &mut target, 0, elements, Color32F::TRANSPARENT)
        .expect("render");
    let m = renderer
        .copy_framebuffer(&target, Rectangle::from_size(size.into()), Fourcc::Argb8888)
        .expect("read back");
    renderer
        .map_texture(&m)
        .expect("map")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect()
}

/// The square's bounding box in `px` (`width` wide).
fn square_box(px: &[u32], width: usize) -> Option<(usize, usize, usize, usize)> {
    let at: Vec<(usize, usize)> = px
        .iter()
        .enumerate()
        .filter(|(_, p)| **p == SQUARE)
        .map(|(i, _)| (i % width, i / width))
        .collect();
    let (x0, y0) = at.iter().fold((usize::MAX, usize::MAX), |a, p| {
        (a.0.min(p.0), a.1.min(p.1))
    });
    let (x1, y1) = at.iter().fold((0, 0), |a, p| (a.0.max(p.0), a.1.max(p.1)));
    (!at.is_empty()).then_some((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

#[test]
fn a_rotated_screen_is_captured_and_its_regions_cut_where_they_are() {
    for transform in [
        Transform::R90,
        Transform::R180,
        Transform::R270,
        Transform::Flipped90,
    ] {
        let mut r = rig(transform);
        let output = r.ls.output();
        let mut renderer = PixmanRenderer::new().expect("pixman");
        let (elements, k) =
            convert::output_elements_screencopy(&mut r.ls.state, &mut renderer, output, false)
                .unwrap_or_else(|_| panic!("a frame"));
        let phys = (1920, 1080);
        let view = convert::capture_buffer_size(phys, transform);
        assert_eq!(
            r.ls.state.core().view(output).unwrap().size,
            (view.0 as u32, view.1 as u32),
            "{transform:?}: Core's view is what the screen shows"
        );

        // where the square is in what the screen shows
        let upright = render(&mut renderer, &elements, view, k, Transform::Normal);
        let (sx, sy, sw, sh) =
            square_box(&upright, view.0 as usize).expect("the square is in view");
        assert_eq!((sw, sh), (50, 50), "{transform:?}");

        // the whole capture: the panel's orientation, the whole square in it
        let full = render(&mut renderer, &elements, phys, k, transform);
        let (fx, fy, fw, fh) = square_box(&full, phys.0 as usize).expect("the square is captured");
        assert_eq!((fw, fh), (50, 50), "{transform:?}: the whole square");

        // the capture is what the panel shows: a touch on the panel where the square is (the
        // touchscreen reports the panel's own orientation) lands on the square in the view
        for (dx, dy) in [(5.0, 5.0), (45.0, 10.0), (10.0, 40.0)] {
            let at = (fx as f64 + dx, fy as f64 + dy);
            let (vx, vy) = convert::panel_point_to_view(at, phys, transform);
            assert!(
                vx >= sx as f64
                    && vx < (sx + sw) as f64
                    && vy >= sy as f64
                    && vy < (sy + sh) as f64,
                "{transform:?}: a touch at {at:?} on the panel lands at ({vx}, {vy}), the square is at ({sx}, {sy})"
            );
        }

        // a region over part of the square and the background beside it, asymmetric so that a
        // wrong turn would cut the wrong place
        let region = (sx as i32 - 30, sy as i32 - 10, 70, 45);
        let (bx, by, bw, bh) = convert::capture_region_in_buffer(region, view, transform);
        assert_eq!(
            (bw, bh),
            convert::capture_buffer_size((70, 45), transform),
            "{transform:?}: the region's buffer is turned"
        );
        let cut: Vec<u32> = (0..bh)
            .flat_map(|y| {
                let row = ((by + y) * phys.0 + bx) as usize;
                full[row..row + bw as usize].to_vec()
            })
            .collect();

        // the GPU path renders the region alone: the elements moved by the region's corner in
        // what the screen shows, drawn into the turned buffer
        let moved: Vec<_> = elements
            .iter()
            .map(|e| {
                RelocateRenderElement::from_element(
                    e,
                    Point::from((-region.0, -region.1)),
                    Relocate::Relative,
                )
            })
            .collect();
        let direct = render(&mut renderer, &moved, (bw, bh), k, transform);
        assert!(
            cut.contains(&SQUARE) && cut.iter().any(|p| *p != SQUARE),
            "{transform:?}: the region holds part of the square and the background"
        );
        assert_eq!(
            cut, direct,
            "{transform:?}: cut from the whole capture = rendered alone"
        );
    }
}
