//! The only conversions between Infynitae values and Smithay geometry types (C3.6, INV-COORD-4).

use infynitae_core::geom::{
    CanvasPoint, CanvasPos, LocalPoint, LocalRect, LocalSize, Q16_ONE, RenderLocation,
};
use infynitae_core::output::{OutputId, OutputModel, Transform};
use infynitae_core::projection::View;
use infynitae_core::render::FrameEntry;
use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::{ImportAll, Renderer};
use smithay::utils::{Logical, Physical, Point, Rectangle, Scale, Size};
use smithay::wayland::compositor::with_states;

use crate::WaylandState;
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
/// A point in Smithay's global pointer space, which is Canvas (D-B2-PTR-SPACE, INV-COORD-5).
pub type GlobalPoint = Point<f64, Logical>;

/// Canvas Q16 → Smithay global (B2.7 `canvas_to_smithay_global`): exact, |x| ≤ 2⁵² units.
pub fn canvas_to_global(q: CanvasPos) -> GlobalPoint {
    let px = |v: i64| v as f64 / Q16_ONE as f64;
    Point::from((px(q.x), px(q.y)))
}

/// An integral Canvas point (a surface origin) → Smithay global.
pub fn canvas_point_to_global(p: CanvasPoint) -> GlobalPoint {
    canvas_to_global(CanvasPos::from_point(p))
}

/// A physical device delta, unchanged (R6: no rounding), as Smithay's relative-motion type.
pub(crate) fn global_delta(d: (f64, f64)) -> GlobalPoint {
    Point::from(d)
}

pub fn modifiers_from_smithay(
    mods: &smithay::input::keyboard::ModifiersState,
) -> infynitae_core::input::bindings::Modifiers {
    infynitae_core::input::bindings::Modifiers {
        logo: mods.logo,
        ctrl: mods.ctrl,
        alt: mods.alt,
        shift: mods.shift,
    }
}

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

// ------------------------------------------------------------------ outputs (INC-2)

/// `wl_output.geometry` physical size: not a hardware fact Core carries; reported as unknown.
pub(crate) fn unknown_physical_size() -> Size<i32, smithay::utils::Raw> {
    (0, 0).into()
}

/// `wl_output.mode` from the physical mode `phys_O` (before the transform) and refresh (mHz).
pub(crate) fn output_mode(model: &OutputModel) -> smithay::output::Mode {
    smithay::output::Mode {
        size: (model.phys.0 as i32, model.phys.1 as i32).into(),
        refresh: model.refresh_mhz as i32,
    }
}

pub(crate) fn output_transform(t: Transform) -> smithay::utils::Transform {
    use smithay::utils::Transform as S;
    match t {
        Transform::Normal => S::Normal,
        Transform::R90 => S::_90,
        Transform::R180 => S::_180,
        Transform::R270 => S::_270,
        Transform::Flipped => S::Flipped,
        Transform::Flipped90 => S::Flipped90,
        Transform::Flipped180 => S::Flipped180,
        Transform::Flipped270 => S::Flipped270,
    }
}

/// The output scale (fractional; `wl_output.scale` is its ceiling, sent by Smithay).
pub(crate) fn output_scale(model: &OutputModel) -> smithay::output::Scale {
    smithay::output::Scale::Fractional(model.scale)
}

/// xdg_output logical position: the output's Arrangement origin.
pub(crate) fn output_location(model: &OutputModel) -> Point<i32, Logical> {
    (
        model.arrangement.0.round() as i32,
        model.arrangement.1.round() as i32,
    )
        .into()
}

/// F-RENDER-1: `OutputDamageTracker::new(phys_O, Scale(k_O), transform_O)`, the **untransformed**
/// physical mode; Smithay derives the element space `transform_size(phys_O) = view_O` itself.
/// `OutputDamageTracker::from_output` is not used: it ignores zoom.
pub fn damage_tracker(model: &OutputModel, view: &View) -> OutputDamageTracker {
    OutputDamageTracker::new(
        (model.phys.0 as i32, model.phys.1 as i32),
        Scale::from(view.k),
        output_transform(model.transform),
    )
}

/// One frame of an output through its damage tracker: Core's render list (recorded as the
/// output's ProjectionFrame) → render elements → `OutputDamageTracker::render_output`. Returns
/// the damage, or `None` when nothing changed.
pub fn render_output<R>(
    state: &mut WaylandState,
    renderer: &mut R,
    framebuffer: &mut R::Framebuffer<'_>,
    output: OutputId,
    age: usize,
) -> Result<Option<Damage>, RenderOutputError<R::Error>>
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + 'static,
{
    let entries = state
        .core
        .render_list(output)
        .map_err(|_| RenderOutputError::NoFrame)?
        .entries
        .clone();
    let elements: Vec<_> = render_elements(renderer, &state.registry, &entries)
        .map_err(RenderOutputError::Renderer)?
        .into_iter()
        .map(|(_, e)| e)
        .collect();
    let handle = state
        .outputs
        .iter_mut()
        .find(|h| h.id == output)
        .ok_or(RenderOutputError::NoFrame)?;
    let result = handle
        .tracker
        .render_output(renderer, framebuffer, age, &elements, Color32F::TRANSPARENT)
        .map_err(|e| match e {
            smithay::backend::renderer::damage::Error::Rendering(e) => {
                RenderOutputError::Renderer(e)
            }
            smithay::backend::renderer::damage::Error::OutputNoMode(_) => {
                RenderOutputError::NoFrame
            }
        })?;
    Ok(result.damage.cloned())
}

/// Damage of one frame, in the output's element (view) space.
pub type Damage = Vec<Rectangle<i32, Physical>>;

#[derive(Debug)]
pub enum RenderOutputError<E> {
    /// The output is not connected or has no view.
    NoFrame,
    Renderer(E),
}
