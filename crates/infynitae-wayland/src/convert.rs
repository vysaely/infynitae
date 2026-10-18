//! The only conversions between Infynitae values and Smithay geometry types (C3.6, INV-COORD-4).

use infynitae_core::geom::PhysicalRect;
use infynitae_core::geom::{
    CanvasPoint, CanvasPos, LocalPoint, LocalRect, LocalSize, Q16_ONE, RenderLocation,
};
use infynitae_core::output::{OutputId, OutputModel, Transform};
use infynitae_core::projection::View;
use infynitae_core::render::FrameEntry;
use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::backend::renderer::{ImportAll, Renderer};
use smithay::utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size};
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

/// Where a client asks for its window menu (`xdg_toplevel.show_window_menu`), in the toplevel's
/// window geometry. Named here so that no handler names a Smithay geometry type (INV-COORD-4);
/// the request is ignored and the value never read.
pub type WindowMenuLocation = Point<i32, Logical>;

/// Core's toplevel capabilities as the `xdg_toplevel.wm_capabilities` set Smithay announces.
pub fn wm_capabilities(
    c: infynitae_core::protocol::ToplevelCapabilities,
) -> Vec<smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities> {
    use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities as W;
    [
        (c.window_menu, W::WindowMenu),
        (c.maximize, W::Maximize),
        (c.fullscreen, W::Fullscreen),
        (c.minimize, W::Minimize),
    ]
    .into_iter()
    .filter_map(|(on, cap)| on.then_some(cap))
    .collect()
}

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

/// The crop box of C12.F15.5: a canonical rect is cropped in i64 to `[−2³⁰, 2³⁰]` per axis before
/// Smithay's i32 geometry. Every view lies inside it (`view_O` ≤ 2¹⁵), so no visible pixel is cut.
pub const RENDER_LIMIT: i64 = 1 << 30;

/// INV-RENDER-1, F-RENDER-1 as amended by C12.F15 (option B): a surface's render element whose
/// geometry is the surface's canonical rect — Core's F-LATTICE-1 rect, the one membership and hit
/// testing use — and whose `draw` hands the destination `OutputDamageTracker::render_output` gives
/// it (that rect) to the wrapped element unchanged, so the surface is placed **and** drawn there.
///
/// When the rect differs from the wrapped element's own geometry at the frame's k, or was cropped,
/// the element is *remapped*: its damage since any older commit is the whole rect, it claims no
/// opaque region and offers no underlying storage (no direct scanout); otherwise all three pass
/// through. Stored by value; nothing here allocates (INV-ALLOC-1).
#[derive(Debug)]
pub struct CanonicalRect<E> {
    inner: E,
    rect: Rectangle<i32, Physical>,
    src: Rectangle<f64, Buffer>,
    remapped: bool,
}

impl<E: Element> CanonicalRect<E> {
    /// Wraps `inner` at the canonical rect `rect` (view-space physical px, i64) for a frame at `k`,
    /// cropped to [`RENDER_LIMIT`].
    pub fn new(inner: E, rect: PhysicalRect, k: f64) -> Self {
        Self::with_crop_box(inner, rect, k, RENDER_LIMIT)
    }

    /// [`CanonicalRect::new`] with the crop box `[−limit, limit]` per axis. Production uses
    /// [`RENDER_LIMIT`]; a smaller box lets tests draw a cropped surface with a software renderer.
    #[doc(hidden)]
    pub fn with_crop_box(inner: E, rect: PhysicalRect, k: f64, limit: i64) -> Self {
        let r = rect;
        let c = (
            r.x0.clamp(-limit, limit),
            r.y0.clamp(-limit, limit),
            r.x1.clamp(-limit, limit),
            r.y1.clamp(-limit, limit),
        );
        // a rect over the whole box is 2·limit wide; Smithay's i32 size holds at most 2³¹ − 1, so
        // its far edge stays one pixel inside the box (at ±2³⁰, outside every view)
        let c = (
            c.0,
            c.1,
            c.2.min(c.0 + i32::MAX as i64),
            c.3.min(c.1 + i32::MAX as i64),
        );
        let cropped = c != (r.x0, r.y0, r.x1, r.y1);
        let (w, h) = ((c.2 - c.0).max(0), (c.3 - c.1).max(0));
        let src = if cropped && w > 0 && h > 0 {
            crop_src(
                inner.src(),
                inner.transform(),
                ((r.x1 - r.x0) as f64, (r.y1 - r.y0) as f64),
                ((c.0 - r.x0) as f64, (c.1 - r.y0) as f64, w as f64, h as f64),
            )
        } else {
            inner.src()
        };
        // every coordinate is within ±limit ≤ 2³⁰ here: the conversions are exact
        let rect = Rectangle::new((c.0 as i32, c.1 as i32).into(), (w as i32, h as i32).into());
        let remapped = cropped || inner.geometry(Scale::from(k)) != rect;
        CanonicalRect {
            inner,
            rect,
            src,
            remapped,
        }
    }

    pub fn inner(&self) -> &E {
        &self.inner
    }

    /// Whether the canonical rect differs from the wrapped element's own geometry (or was cropped):
    /// damage and opaque regions are then remapped conservatively.
    pub fn is_remapped(&self) -> bool {
        self.remapped
    }
}

/// The `src` of the visible part `visible` (x, y, w, h, relative to the uncropped rect's origin) of
/// an element of `size` (the uncropped canonical rect's size) whose whole `src` is `src`, drawn with
/// the buffer transform `transform`: the part of `src` the renderer samples for those destination
/// pixels, in proportion (C12.F15.5), evaluated in f64 on the uncropped i64 rect.
///
/// Not Smithay's `CropRenderElement::from_element` formula, which C12.F15.5 names: that maps the
/// visible part through `Rectangle::to_buffer(.., transform, ..)`, the inverse of the rotation the
/// renderers apply, and so selects the wrong part of a buffer under the four rotating transforms
/// (90°, 270°, flipped 90°, flipped 270°; shown in Pixman pixels by `t1_canonical_rect`, reported to
/// change control in the INC-10 evidence).
fn crop_src(
    src: Rectangle<f64, Buffer>,
    transform: smithay::utils::Transform,
    size: (f64, f64),
    visible: (f64, f64, f64, f64),
) -> Rectangle<f64, Buffer> {
    // the visible interval on each destination axis, as distances from its start
    let dx = (visible.0, visible.0 + visible.2, size.0);
    let dy = (visible.1, visible.1 + visible.3, size.1);
    let (bx, by) = sampled_axes(transform);
    // a buffer axis takes one destination axis, read forwards or backwards; each edge is
    // `distance · extent / size`, one rounding, so a cut on a whole buffer pixel stays whole
    let axis = |(d, forward): (bool, bool), loc: f64, extent: f64| {
        let (lo, hi, n) = if d { dy } else { dx };
        let (a, b) = if forward { (lo, hi) } else { (n - hi, n - lo) };
        (loc + a * extent / n, (b - a) * extent / n)
    };
    let (x, w) = axis(bx, src.loc.x, src.size.w);
    let (y, h) = axis(by, src.loc.y, src.size.h);
    Rectangle::new((x, y).into(), (w, h).into())
}

/// Which destination axis a renderer samples along each buffer axis under the buffer transform
/// `t`, and in which direction: `((x reads y?, forwards?), (y reads y?, forwards?))`. The mapping
/// of Smithay 0.7.0's GLES texture matrix (`gles/mod.rs` `build_texture_mat`: `Transform::matrix()`
/// then its translation) and of its Pixman renderer (`render_texture_from_to`, which inverts the
/// transform): under 90° the buffer's x follows the destination's y forwards and its y the
/// destination's x backwards.
fn sampled_axes(t: smithay::utils::Transform) -> ((bool, bool), (bool, bool)) {
    use smithay::utils::Transform as T;
    match t {
        T::Normal => ((false, true), (true, true)),
        T::_90 => ((true, true), (false, false)),
        T::_180 => ((false, false), (true, false)),
        T::_270 => ((true, false), (false, true)),
        T::Flipped => ((false, false), (true, true)),
        T::Flipped90 => ((true, true), (false, true)),
        T::Flipped180 => ((false, true), (true, false)),
        T::Flipped270 => ((true, false), (false, false)),
    }
}

impl<E: Element> Element for CanonicalRect<E> {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.src
    }

    fn transform(&self) -> smithay::utils::Transform {
        self.inner.transform()
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.rect
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        let damage = self.inner.damage_since(scale, commit);
        if !self.remapped || damage.is_empty() {
            damage
        } else {
            DamageSet::from_slice(&[Rectangle::from_size(self.rect.size)])
        }
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        if self.remapped {
            OpaqueRegions::default()
        } else {
            self.inner.opaque_regions(scale)
        }
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<R: Renderer, E: RenderElement<R>> RenderElement<R> for CanonicalRect<E> {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        self.inner.draw(frame, src, dst, damage, opaque_regions)
    }

    fn underlying_storage(&self, renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        if self.remapped {
            None
        } else {
            self.inner.underlying_storage(renderer)
        }
    }
}

/// A surface's render element as presented: [`CanonicalRect`] around Smithay's surface element.
pub type SurfaceElement<R> = CanonicalRect<WaylandSurfaceRenderElement<R>>;

/// Render elements for a frame at `k` (the frame's `View::k`), front to back, one per entry, each
/// at its canonical rect ([`CanonicalRect`]). An entry whose surface is no longer registered or
/// has no renderable state is skipped.
pub fn render_elements<R>(
    renderer: &mut R,
    registry: &SurfaceRegistry,
    entries: &[FrameEntry],
    k: f64,
) -> Result<Vec<(FrameEntry, SurfaceElement<R>)>, R::Error>
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + 'static,
{
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        if let Some(el) = surface_element(renderer, registry, e)? {
            out.push((e.clone(), CanonicalRect::new(el, e.rect, k)));
        }
    }
    Ok(out)
}

/// Smithay's element for one entry's surface at the F-RENDER-1 location, before it is wrapped;
/// `None` when the surface is no longer registered or has no renderable state.
pub fn surface_element<R>(
    renderer: &mut R,
    registry: &SurfaceRegistry,
    e: &FrameEntry,
) -> Result<Option<WaylandSurfaceRenderElement<R>>, R::Error>
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + 'static,
{
    let Some(surface) = registry.surface(e.surface) else {
        return Ok(None);
    };
    with_states(surface, |states| {
        WaylandSurfaceRenderElement::from_surface(
            renderer,
            surface,
            states,
            projected_to_smithay_physical(e.location),
            1.0,
            Kind::Unspecified,
        )
    })
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
    let frame = state
        .core
        .render_list(output)
        .map_err(|_| RenderOutputError::NoFrame)?;
    let view = frame.view;
    let entries = frame.entries.clone();
    let elements: Vec<_> = render_elements(renderer, &state.registry, &entries, view.k)
        .map_err(RenderOutputError::Renderer)?
        .into_iter()
        .map(|(_, e)| e)
        .collect();
    // borrowed, not cloned: the model's key owns strings, and it is read only for a rebuild
    let model = state
        .core
        .output(output)
        .ok_or(RenderOutputError::NoFrame)?;
    let handle = state
        .outputs
        .iter_mut()
        .find(|h| h.id == output)
        .ok_or(RenderOutputError::NoFrame)?;
    // C12.F15.4: the tracker computes damage and opaque regions at its Scale(k); a frame at
    // another k (a fit, a restore, a presentation) rebuilds it before rendering
    if handle.tracker_k != view.k {
        handle.tracker = damage_tracker(model, &view);
        handle.tracker_k = view.k;
    }
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

pub fn hint_to_local_point(location: GlobalPoint) -> LocalPoint {
    LocalPoint {
        x: location.x as i32,
        y: location.y as i32,
    }
}

pub(crate) fn convert_region(
    attrs: &smithay::wayland::compositor::RegionAttributes,
) -> Option<LocalRect> {
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    let mut any = false;
    for (kind, r) in &attrs.rects {
        if matches!(kind, smithay::wayland::compositor::RectangleKind::Add) {
            any = true;
            min_x = min_x.min(r.loc.x);
            min_y = min_y.min(r.loc.y);
            max_x = max_x.max(r.loc.x + r.size.w);
            max_y = max_y.max(r.loc.y + r.size.h);
        }
    }
    if any && max_x > min_x && max_y > min_y {
        Some(LocalRect {
            loc: LocalPoint { x: min_x, y: min_y },
            size: LocalSize {
                w: max_x - min_x,
                h: max_y - min_y,
            },
        })
    } else {
        None
    }
}

/// Sends an element's hint to every surface of its tree: the root tree and each popup tree, the
/// set the snapshot reports to Core and `s_max(E)` ranges over (F-SCALE-2).
pub fn send_preferred_scale_to_window(
    window: &smithay::desktop::Window,
    scale_120: u32,
    integer_scale: u32,
) {
    window.with_surfaces(|surface, states| {
        send_preferred_scale(surface, states, scale_120, integer_scale);
    });
}

/// Sends a hint to `root` and the subsurfaces below it: a subtree that joined an element.
pub(crate) fn send_preferred_scale_to_tree(
    root: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    scale_120: u32,
    integer_scale: u32,
) {
    smithay::desktop::utils::with_surfaces_surface_tree(root, |surface, states| {
        send_preferred_scale(surface, states, scale_120, integer_scale);
    });
}

/// `wp_fractional_scale_v1.preferred_scale` and `wl_surface.preferred_buffer_scale` for one
/// surface; Smithay sends each only when it differs from what the surface was last told.
///
/// Dispatch safety: `states` is the `SurfaceData` the traversal hands out while it holds the
/// surface's lock. A `with_states` call for the same surface here would deadlock.
fn send_preferred_scale(
    surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    states: &smithay::wayland::compositor::SurfaceData,
    scale_120: u32,
    integer_scale: u32,
) {
    smithay::wayland::fractional_scale::with_fractional_scale(states, |fs| {
        fs.set_preferred_scale(scale_120 as f64 / 120.0);
    });
    smithay::wayland::compositor::send_surface_state(
        surface,
        states,
        integer_scale as i32,
        smithay::utils::Transform::Normal,
    );
}
