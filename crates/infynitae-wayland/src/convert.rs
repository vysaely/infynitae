//! The only conversions between Infynitae values and Smithay geometry types (C3.6, INV-COORD-4).

use infynitae_core::geom::{LocalPoint, LocalRect, LocalSize, RenderLocation};
use infynitae_core::render::FrameEntry;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::{ImportAll, Renderer};
use smithay::utils::{Logical, Physical, Point, Rectangle, Size};
use smithay::wayland::compositor::with_states;

use crate::registry::SurfaceRegistry;

pub(crate) fn local_point(p: Point<i32, Logical>) -> LocalPoint {
    LocalPoint { x: p.x, y: p.y }
}

pub(crate) fn local_size(s: Size<i32, Logical>) -> LocalSize {
    LocalSize { w: s.w, h: s.h }
}

pub(crate) fn local_rect(r: Rectangle<i32, Logical>) -> LocalRect {
    LocalRect {
        loc: local_point(r.loc),
        size: local_size(r.size),
    }
}

/// Popup constraint target, already relative to the parent's window geometry (C3.6).
pub(crate) fn parent_geometry_rect(r: LocalRect) -> Rectangle<i32, Logical> {
    Rectangle::new((r.loc.x, r.loc.y).into(), (r.size.w, r.size.h).into())
}

/// F-RENDER-1: the f64 render location, unchanged.
pub fn projected_to_smithay_physical(l: RenderLocation) -> Point<f64, Physical> {
    Point::from((l.x, l.y))
}

/// Render elements for a frame, front to back, one per entry. An entry whose surface is no longer
/// registered or has no renderable state is skipped.
pub fn render_elements<R>(
    renderer: &mut R,
    registry: &SurfaceRegistry,
    entries: &[FrameEntry],
) -> Result<Vec<(FrameEntry, WaylandSurfaceRenderElement<R>)>, R::Error>
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + 'static,
{
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let Some(surface) = registry.surface(e.surface) else {
            continue;
        };
        let element = with_states(surface, |states| {
            WaylandSurfaceRenderElement::from_surface(
                renderer,
                surface,
                states,
                projected_to_smithay_physical(e.location),
                1.0,
                Kind::Unspecified,
            )
        })?;
        if let Some(el) = element {
            out.push((e.clone(), el));
        }
    }
    Ok(out)
}
