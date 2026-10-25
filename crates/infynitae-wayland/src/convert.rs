//! The only conversions between Infynitae values and Smithay geometry types (C3.6, INV-COORD-4).

use infynitae_core::geom::PhysicalRect;
use infynitae_core::geom::{
    CanvasPoint, CanvasPos, LocalPoint, LocalRect, LocalSize, Q16_ONE, RenderLocation,
};
use infynitae_core::output::{OutputId, OutputModel, Transform};
use infynitae_core::pointer::PointerState;
use infynitae_core::projection::View;
use infynitae_core::render::FrameEntry;
use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::utils::{
    CommitCounter, DamageSet, OpaqueRegions, RendererSurfaceStateUserData,
};
use smithay::backend::renderer::{ImportAll, ImportMem, Renderer};
use smithay::input::pointer::{CursorImageStatus, CursorImageSurfaceData};
use smithay::reexports::wayland_server::Resource;
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
    force_transparent: bool,
}

impl<E: Element> CanonicalRect<E> {
    /// Wraps `inner` at the canonical rect `rect` (view-space physical px, i64) for a frame at `k`,
    /// cropped to [`RENDER_LIMIT`].
    pub fn new(inner: E, rect: PhysicalRect, k: f64) -> Self {
        Self::with_crop_box_transparent(inner, rect, k, RENDER_LIMIT, false)
    }

    /// Wraps `inner` with optional forced transparency blending (Wine / layered windows).
    pub fn new_opt(inner: E, rect: PhysicalRect, k: f64, force_transparent: bool) -> Self {
        Self::with_crop_box_transparent(inner, rect, k, RENDER_LIMIT, force_transparent)
    }

    /// [`CanonicalRect::new`] with the crop box `[−limit, limit]` per axis. Production uses
    /// [`RENDER_LIMIT`]; a smaller box lets tests draw a cropped surface with a software renderer.
    #[doc(hidden)]
    pub fn with_crop_box(inner: E, rect: PhysicalRect, k: f64, limit: i64) -> Self {
        Self::with_crop_box_transparent(inner, rect, k, limit, false)
    }

    /// [`CanonicalRect::new`] with crop box and forced transparency option.
    #[doc(hidden)]
    pub fn with_crop_box_transparent(
        inner: E,
        rect: PhysicalRect,
        k: f64,
        limit: i64,
        force_transparent: bool,
    ) -> Self {
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
            force_transparent,
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
        if self.remapped || self.force_transparent {
            OpaqueRegions::default()
        } else {
            self.inner.opaque_regions(scale)
        }
    }

    fn alpha(&self) -> f32 {
        if self.force_transparent && self.inner.alpha() >= 1.0 {
            0.9999
        } else {
            self.inner.alpha()
        }
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
        if self.remapped || self.force_transparent {
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
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    render_elements_opt(renderer, registry, entries, k, |_| false)
}

/// Render elements with optional forced transparency predicate per element.
pub fn render_elements_opt<R, F>(
    renderer: &mut R,
    registry: &SurfaceRegistry,
    entries: &[FrameEntry],
    k: f64,
    force_transparent: F,
) -> Result<Vec<(FrameEntry, SurfaceElement<R>)>, R::Error>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
    F: Fn(infynitae_core::stage::ElementId) -> bool,
{
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        if let Some(el) = surface_element(renderer, registry, e)? {
            let ft = force_transparent(e.element);
            out.push((e.clone(), CanonicalRect::new_opt(el, e.rect, k, ft)));
        }
    }
    Ok(out)
}

/// Render elements for layer-surface entries (L2), front to back, each at its physical rect in the
/// output (a layer surface is never zoomed: its rect is its logical rect at the output scale). `k`
/// is the frame's, as for the Canvas entries: the damage tracker draws the whole frame at it.
pub fn layer_render_elements<R>(
    renderer: &mut R,
    registry: &SurfaceRegistry,
    entries: &[infynitae_core::layer::LayerEntry],
    k: f64,
) -> Result<Vec<SurfaceElement<R>>, R::Error>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let Some(surface) = registry.surface(e.surface) else {
            continue;
        };
        let el = with_states(surface, |states| {
            WaylandSurfaceRenderElement::from_surface(
                renderer,
                surface,
                states,
                projected_to_smithay_physical(e.location),
                1.0,
                Kind::Unspecified,
            )
        })?;
        if let Some(el) = el {
            out.push(CanonicalRect::new(el, e.rect, k));
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
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
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
/// The opaque colour of the empty Canvas in a host window (a dark grey).
pub const CANVAS_CLEAR: Color32F = Color32F::new(0.11, 0.11, 0.13, 1.0);

/// What a backend's framebuffer is: whether its rows run bottom-up (GL) and what the empty
/// Canvas is cleared to.
#[derive(Clone, Copy, Debug)]
pub struct RenderTarget {
    pub flip_y: bool,
    pub clear: Color32F,
    /// The compositor draws the pointer (`cursor`): a backend that owns the display.
    pub draw_cursor: bool,
}

impl Default for RenderTarget {
    fn default() -> Self {
        RenderTarget {
            flip_y: false,
            clear: Color32F::TRANSPARENT,
            draw_cursor: false,
        }
    }
}

pub fn damage_tracker(
    model: &OutputModel,
    view: &View,
    target: RenderTarget,
) -> OutputDamageTracker {
    let transform = match (target.flip_y, output_transform(model.transform)) {
        // a vertical flip of the framebuffer; the nested output is never rotated, so only the
        // normal transform needs composing
        (true, smithay::utils::Transform::Normal) => smithay::utils::Transform::Flipped180,
        (_, t) => t,
    };
    OutputDamageTracker::new(
        (model.phys.0 as i32, model.phys.1 as i32),
        Scale::from(view.k),
        transform,
    )
}

smithay::backend::renderer::element::render_elements! {
    /// What an output's frame is made of: surfaces at their canonical rects, and the solid rects
    /// of the built-in arrow (`cursor`).
    pub OutputElement<R> where R: ImportAll + ImportMem, R::TextureId: Send;
    Surface=SurfaceElement<R>,
    Solid=SolidColorRenderElement,
    NamedCursor=CanonicalRect<MemoryRenderBufferRenderElement<R>>,
}

/// An output's frame elements and the frame's `k`.
pub type OutputFrame<R> = (Vec<OutputElement<R>>, f64);

/// What an output's frame is made of, optionally including cursor elements.
pub fn output_elements_opt<R>(
    state: &mut WaylandState,
    renderer: &mut R,
    output: OutputId,
    include_cursor: bool,
) -> Result<OutputFrame<R>, RenderOutputError<R::Error>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let frame = state
        .core
        .render_list(output)
        .map_err(|_| RenderOutputError::NoFrame)?;
    let view = frame.view;
    let entries = frame.entries.clone();
    let layers = state.core.layer_frame(output);
    let mut elements = Vec::new();
    if include_cursor {
        cursor_elements(state, renderer, output, view.k, &mut elements)
            .map_err(RenderOutputError::Renderer)?;
    }
    elements.extend(
        layer_render_elements(renderer, &state.registry, &layers.above, view.k)
            .map_err(RenderOutputError::Renderer)?
            .into_iter()
            .map(OutputElement::Surface),
    );
    let force_transparent = |elem| state.is_element_transparent(elem);
    elements.extend(
        render_elements_opt(
            renderer,
            &state.registry,
            &entries,
            view.k,
            force_transparent,
        )
        .map_err(RenderOutputError::Renderer)?
        .into_iter()
        .map(|(_, e)| OutputElement::Surface(e)),
    );
    elements.extend(
        layer_render_elements(renderer, &state.registry, &layers.below, view.k)
            .map_err(RenderOutputError::Renderer)?
            .into_iter()
            .map(OutputElement::Surface),
    );
    Ok((elements, view.k))
}

/// One frame's elements for screencopy, excluding any private window (hidden from screenshots/screenshares).
pub fn output_elements_screencopy<R>(
    state: &mut WaylandState,
    renderer: &mut R,
    output: OutputId,
    include_cursor: bool,
) -> Result<OutputFrame<R>, RenderOutputError<R::Error>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let (view, raw_entries) = {
        let frame = state
            .core
            .render_list(output)
            .map_err(|_| RenderOutputError::NoFrame)?;
        (frame.view, frame.entries.clone())
    };
    let entries: Vec<FrameEntry> = raw_entries
        .into_iter()
        .filter(|e| !state.is_element_private(e.element))
        .collect();
    let layers = state.core.layer_frame(output);
    let mut elements = Vec::new();
    if include_cursor {
        cursor_elements(state, renderer, output, view.k, &mut elements)
            .map_err(RenderOutputError::Renderer)?;
    }
    elements.extend(
        layer_render_elements(renderer, &state.registry, &layers.above, view.k)
            .map_err(RenderOutputError::Renderer)?
            .into_iter()
            .map(OutputElement::Surface),
    );
    let force_transparent = |elem| state.is_element_transparent(elem);
    elements.extend(
        render_elements_opt(
            renderer,
            &state.registry,
            &entries,
            view.k,
            force_transparent,
        )
        .map_err(RenderOutputError::Renderer)?
        .into_iter()
        .map(|(_, e)| OutputElement::Surface(e)),
    );
    elements.extend(
        layer_render_elements(renderer, &state.registry, &layers.below, view.k)
            .map_err(RenderOutputError::Renderer)?
            .into_iter()
            .map(OutputElement::Surface),
    );
    Ok((elements, view.k))
}

/// One frame's elements for `output`, front to back: the cursor (when the target draws it),
/// overlay and top layers, the Canvas from Core's render list (recorded as the output's
/// ProjectionFrame), bottom and background layers (L2); and the frame's `k`.
pub fn output_elements<R>(
    state: &mut WaylandState,
    renderer: &mut R,
    output: OutputId,
) -> Result<OutputFrame<R>, RenderOutputError<R::Error>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    output_elements_opt(state, renderer, output, state.target.draw_cursor)
}

/// While the session is locked (`lock`): the output's frame is only its lock surface, over the
/// whole output, or nothing (drawn over [`LOCKED_CLEAR`]). `None` when not locked.
pub fn lock_elements<R>(
    state: &WaylandState,
    renderer: &mut R,
    output: OutputId,
) -> Result<Option<OutputFrame<R>>, R::Error>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    if !state.is_locked() {
        return Ok(None);
    }
    let k = state.core.view(output).map_or(1.0, |v| v.k);
    let mut elements = Vec::new();
    if let (Some(surface), Some((w, h))) =
        (state.lock_surface(output), state.output_mode_size(output))
    {
        let el = with_states(&surface, |states| {
            WaylandSurfaceRenderElement::from_surface(
                renderer,
                &surface,
                states,
                smithay::utils::Point::<f64, Physical>::from((0.0, 0.0)),
                1.0,
                Kind::Unspecified,
            )
        })?;
        if let Some(el) = el {
            let rect = PhysicalRect {
                x0: 0,
                y0: 0,
                x1: i64::from(w),
                y1: i64::from(h),
            };
            elements.push(OutputElement::Surface(CanonicalRect::new(el, rect, k)));
        }
    }
    Ok(Some((elements, k)))
}

/// What a locked output is cleared to: no window may show through.
pub const LOCKED_CLEAR: smithay::backend::renderer::Color32F =
    smithay::backend::renderer::Color32F::new(0.0, 0.0, 0.0, 1.0);

/// One frame of an output through its damage tracker: [`output_elements`] →
/// `OutputDamageTracker::render_output`. Returns the damage, or `None` when nothing changed.
pub fn render_output<R>(
    state: &mut WaylandState,
    renderer: &mut R,
    framebuffer: &mut R::Framebuffer<'_>,
    output: OutputId,
    age: usize,
) -> Result<Option<Damage>, RenderOutputError<R::Error>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let (elements, k) = output_elements(state, renderer, output)?;
    let view = state.core.view(output).ok_or(RenderOutputError::NoFrame)?;
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
    if handle.tracker_k != k {
        handle.tracker = damage_tracker(model, &view, state.target);
        handle.tracker_k = k;
    }
    let result = handle
        .tracker
        .render_output(renderer, framebuffer, age, &elements, state.target.clear)
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

// ------------------------------------------------------------------ cursor (T8, 2026-10-22)

/// The built-in arrow, one row per entry: the outline covers `[0, row + 1)`, the fill
/// `[1, row)` — a right triangle, its tip the hotspot.
/// (each row's fill in front of its outline). Pure geometry.
pub fn arrow_rects(at: (i32, i32), unit: i32) -> Vec<(bool, Rectangle<i32, Physical>)> {
    let mut out = Vec::with_capacity(2 * crate::cursor::ARROW_ROWS as usize);
    for row in 0..crate::cursor::ARROW_ROWS {
        let y = at.1 + row * unit;
        if row >= 2 {
            out.push((
                true,
                Rectangle::new((at.0 + unit, y).into(), ((row - 1) * unit, unit).into()),
            ));
        }
        out.push((
            false,
            Rectangle::new((at.0, y).into(), ((row + 1) * unit, unit).into()),
        ));
    }
    out
}

/// The cursor's elements on `output` (front-most first), when the pointer is on it.
pub(crate) fn cursor_elements<R>(
    state: &WaylandState,
    renderer: &mut R,
    output: OutputId,
    k: f64,
    out: &mut Vec<OutputElement<R>>,
) -> Result<(), R::Error>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let PointerState::Located { output: o, p } = state.core.pointer() else {
        return Ok(());
    };
    if o != output {
        return Ok(());
    }
    let Some(scale) = state.core.output(output).map(|m| m.scale) else {
        return Ok(());
    };
    let px = |v: f64| (v * scale).round() as i32;
    match &state.cursor {
        CursorImageStatus::Hidden => {}
        CursorImageStatus::Named(icon) if icon.name() == "none" => {}
        CursorImageStatus::Surface(surface) if surface.is_alive() => {
            let (hotspot, size) = with_states(surface, |states| {
                let hotspot = states
                    .data_map
                    .get::<CursorImageSurfaceData>()
                    .map(|d| d.lock().unwrap_or_else(|e| e.into_inner()).hotspot)
                    .unwrap_or_default();
                let size = states
                    .data_map
                    .get::<RendererSurfaceStateUserData>()
                    .and_then(|d| d.lock().unwrap_or_else(|e| e.into_inner()).view())
                    .map(|v| v.dst)
                    .unwrap_or_default();
                (hotspot, size)
            });
            let x0 = px(p.0 - f64::from(hotspot.x));
            let y0 = px(p.1 - f64::from(hotspot.y));
            let rect = PhysicalRect {
                x0: i64::from(x0),
                y0: i64::from(y0),
                x1: i64::from(x0 + px(f64::from(size.w))),
                y1: i64::from(y0 + px(f64::from(size.h))),
            };
            let el = with_states(surface, |states| {
                WaylandSurfaceRenderElement::from_surface(
                    renderer,
                    surface,
                    states,
                    Point::from((f64::from(x0), f64::from(y0))),
                    1.0,
                    Kind::Cursor,
                )
            })?;
            if let Some(el) = el {
                out.push(OutputElement::Surface(CanonicalRect::new(el, rect, k)));
            }
        }
        // a named cursor, or a cursor surface that is gone: the theme's image, or fallback arrow
        _ => {
            let icon_name = match &state.cursor {
                CursorImageStatus::Named(icon) => icon.name(),
                _ => "default",
            };
            let target_size = (f64::from(state.cursor_size()) * scale).round().max(1.0) as u32;
            let loaded = state.cursor_image(icon_name, target_size);
            if let Some(img) = loaded {
                let x0 = px(p.0) - img.xhot;
                let y0 = px(p.1) - img.yhot;
                let rect = PhysicalRect {
                    x0: i64::from(x0),
                    y0: i64::from(y0),
                    x1: i64::from(x0 + img.width),
                    y1: i64::from(y0 + img.height),
                };
                let el = MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    Point::from((f64::from(x0), f64::from(y0))),
                    &img.buffer,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                )?;
                out.push(OutputElement::NamedCursor(CanonicalRect::new(el, rect, k)));
            } else {
                let unit = scale.round().max(1.0) as i32;
                let (mut fill, mut outline) = (state.arrow.fill.iter(), state.arrow.outline.iter());
                for (is_fill, rect) in arrow_rects((px(p.0), px(p.1)), unit) {
                    let (id, color) = if is_fill {
                        (fill.next(), Color32F::new(1.0, 1.0, 1.0, 1.0))
                    } else {
                        (outline.next(), Color32F::new(0.0, 0.0, 0.0, 1.0))
                    };
                    if let Some(id) = id {
                        out.push(OutputElement::Solid(SolidColorRenderElement::new(
                            id.clone(),
                            rect,
                            CommitCounter::default(),
                            color,
                            Kind::Unspecified,
                        )));
                    }
                }
            }
        }
    }
    Ok(())
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
    let mut min_x = i64::MAX;
    let mut min_y = i64::MAX;
    let mut max_x = i64::MIN;
    let mut max_y = i64::MIN;
    let mut any = false;
    for (kind, r) in &attrs.rects {
        if matches!(kind, smithay::wayland::compositor::RectangleKind::Add) {
            any = true;
            let rx = r.loc.x as i64;
            let ry = r.loc.y as i64;
            let rw = r.size.w as i64;
            let rh = r.size.h as i64;
            min_x = min_x.min(rx);
            min_y = min_y.min(ry);
            max_x = max_x.max(rx.saturating_add(rw));
            max_y = max_y.max(ry.saturating_add(rh));
        }
    }
    if any && max_x > min_x && max_y > min_y {
        let x = min_x.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let y = min_y.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let w = (max_x - min_x).clamp(0, i32::MAX as i64) as i32;
        let h = (max_y - min_y).clamp(0, i32::MAX as i64) as i32;
        if w > 0 && h > 0 {
            Some(LocalRect {
                loc: LocalPoint { x, y },
                size: LocalSize { w, h },
            })
        } else {
            None
        }
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

#[cfg(test)]
mod cursor_tests {
    use super::*;

    /// The arrow's tip is the hotspot; its fill never covers the outline's left column or the
    /// diagonal; 13 outline rows and 11 fill rows; scaled by whole units.
    #[test]
    fn the_arrow_has_its_tip_at_the_hotspot() {
        let r = arrow_rects((100, 50), 1);
        assert_eq!(r.iter().filter(|(f, _)| !f).count(), 13);
        assert_eq!(r.iter().filter(|(f, _)| *f).count(), 11);
        let tip = r.iter().find(|(f, _)| !f).unwrap().1;
        assert_eq!(tip, Rectangle::new((100, 50).into(), (1, 1).into()));
        for (fill, rect) in &r {
            if *fill {
                assert!(rect.loc.x > 100);
                let row = rect.loc.y - 50;
                assert!(rect.loc.x + rect.size.w < 100 + row + 1);
            }
        }
        let big = arrow_rects((0, 0), 2);
        assert_eq!(
            big.last().unwrap().1,
            Rectangle::new((0, 24).into(), (26, 2).into())
        );
    }

    #[test]
    fn convert_region_handles_overflow_and_clamps() {
        use smithay::wayland::compositor::{RectangleKind, RegionAttributes};
        let mut attrs = RegionAttributes::default();
        attrs.rects.push((
            RectangleKind::Add,
            Rectangle::new((i32::MAX - 10, 10).into(), (100, 50).into()),
        ));
        let rect = convert_region(&attrs).expect("region should be valid");
        assert_eq!(rect.loc.x, i32::MAX - 10);
        assert_eq!(rect.loc.y, 10);
        assert!(rect.size.w > 0);
        assert_eq!(rect.size.h, 50);

        let mut attrs2 = RegionAttributes::default();
        attrs2.rects.push((
            RectangleKind::Add,
            Rectangle::new((10, i32::MAX - 20).into(), (50, 100).into()),
        ));
        let rect2 = convert_region(&attrs2).expect("region should be valid");
        assert_eq!(rect2.loc.y, i32::MAX - 20);
        assert!(rect2.size.h > 0);

        let mut attrs3 = RegionAttributes::default();
        attrs3.rects.push((
            RectangleKind::Add,
            Rectangle::new((i32::MIN, i32::MIN).into(), (i32::MAX, i32::MAX).into()),
        ));
        let rect3 = convert_region(&attrs3).expect("region should be valid");
        assert_eq!(rect3.loc.x, i32::MIN);
        assert_eq!(rect3.loc.y, i32::MIN);
        assert_eq!(rect3.size.w, i32::MAX);
        assert_eq!(rect3.size.h, i32::MAX);
    }
}
