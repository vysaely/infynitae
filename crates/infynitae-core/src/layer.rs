//! Layer surfaces (panels, bars, launchers, wallpapers; `zwlr_layer_shell_v1`): owner decisions
//! 2026-10-21 L1–L8 (`evidence/phase-f/LAYER/DECISION_RECORD_2026-10-21.md`; layer C12.F19
//! prepared, not adopted).
//!
//! A layer surface is not an element (L1): it has no anchor, no Stage entry and no camera. It
//! belongs to one output and lives in that output's logical coordinates; no camera ever moves it
//! and it is never in membership's camera views. `background` and `bottom` are drawn below the
//! Canvas, `top` and `overlay` above it (L2), and hit-tested in the same order (L3). Exclusive
//! zones shrink the output's usable area, which placement (F-PLACE-1) and fit (F-FIT-1) use; the
//! camera's view is unchanged (L4).
//!
//! Arrangement follows the protocol, in creation order per output: a surface whose exclusive zone
//! is −1 is laid out in the whole output, every other one in the area left by the exclusive zones
//! before it; a positive zone reserves its edge's strip (plus the margin on that edge) when the
//! surface is anchored to exactly that edge, or to it and both perpendicular edges.

use crate::geom::{LocalPos, LocalRect, LocalSize, PhysicalRect, Q16_ONE, RenderLocation};
use crate::output::OutputId;
use crate::snapshot::{InputRegion, SurfaceKey, SurfaceTree};

/// Layer-surface identity: never reused within a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(pub(crate) u64);

impl LayerId {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// `zwlr_layer_shell_v1.layer`, bottom to top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Background,
    Bottom,
    Top,
    Overlay,
}

impl Layer {
    /// Drawn above the Canvas (L2).
    pub fn above_canvas(self) -> bool {
        matches!(self, Layer::Top | Layer::Overlay)
    }
}

/// Which edges a surface is anchored to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Anchors {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

/// `zwlr_layer_surface_v1.set_keyboard_interactivity` (L5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyboardInteractivity {
    /// Never focused.
    #[default]
    None,
    /// Takes the keyboard while mapped in `top` or `overlay`.
    Exclusive,
    /// Focused by a click on it, like a window.
    OnDemand,
}

/// The double-buffered state a client committed (its last commit).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerState {
    pub layer: Option<Layer>,
    pub anchors: Anchors,
    /// The size asked for; 0 on an axis: as large as the anchors allow.
    pub size: (u32, u32),
    /// top, right, bottom, left (protocol order).
    pub margins: (i32, i32, i32, i32),
    /// > 0 reserves a strip; 0 avoids other zones; −1 ignores them.
    pub exclusive_zone: i32,
    pub keyboard: KeyboardInteractivity,
}

/// A layer surface as Core holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRecord {
    pub id: LayerId,
    pub output: OutputId,
    pub state: LayerState,
    /// The surface tree of its last commit (root, subsurfaces); `None` before the first commit.
    pub tree: Option<SurfaceTree>,
    /// Its rect in the output's logical coordinates, from the last arrangement.
    pub geometry: LocalRect,
    /// The size last sent in a configure (`None`: no configure yet).
    pub configured: Option<LocalSize>,
    /// The surfaces told `wl_surface.enter` for the output.
    pub(crate) entered: Vec<SurfaceKey>,
}

impl LayerRecord {
    pub fn layer(&self) -> Layer {
        self.state.layer.unwrap_or(Layer::Top)
    }

    /// Mapped: committed with a mapped root after a configure.
    pub fn mapped(&self) -> bool {
        self.configured.is_some()
            && self
                .tree
                .as_ref()
                .and_then(|t| t.root())
                .is_some_and(|r| r.mapped)
    }

    pub fn root(&self) -> Option<SurfaceKey> {
        self.tree.as_ref().and_then(|t| t.root()).map(|r| r.key)
    }
}

/// An output's logical area, `(x, y, w, h)` in output-logical px.
pub type Area = (f64, f64, f64, f64);

/// Lays out the layer surfaces of one output (`extent`: its logical size), in creation order.
/// Writes each record's `geometry` and returns the usable area left by the exclusive zones.
pub(crate) fn arrange<'a>(
    extent: (f64, f64),
    records: impl Iterator<Item = &'a mut LayerRecord>,
) -> Area {
    let full = (0.0, 0.0, extent.0, extent.1);
    let mut usable = full;
    for r in records {
        let s = r.state;
        let bounds = if s.exclusive_zone < 0 { full } else { usable };
        let (mt, mr, mb, ml) = (
            f64::from(s.margins.0),
            f64::from(s.margins.1),
            f64::from(s.margins.2),
            f64::from(s.margins.3),
        );
        let a = s.anchors;
        let axis =
            |requested: u32, lo: bool, hi: bool, start: f64, len: f64, m_lo: f64, m_hi: f64| {
                let size = if requested == 0 || (lo && hi && requested == 0) {
                    (len - m_lo - m_hi).max(0.0)
                } else {
                    f64::from(requested)
                };
                let pos = match (lo, hi) {
                    (true, false) => start + m_lo,
                    (false, true) => start + len - m_hi - size,
                    // both or neither: centred in the bounds (margins only apply when anchored)
                    (true, true) => start + m_lo + ((len - m_lo - m_hi) - size) / 2.0,
                    (false, false) => start + (len - size) / 2.0,
                };
                (pos, size)
            };
        let (x, w) = axis(s.size.0, a.left, a.right, bounds.0, bounds.2, ml, mr);
        let (y, h) = axis(s.size.1, a.top, a.bottom, bounds.1, bounds.3, mt, mb);
        r.geometry = LocalRect {
            loc: crate::geom::LocalPoint {
                x: x.round() as i32,
                y: y.round() as i32,
            },
            size: LocalSize {
                w: w.round() as i32,
                h: h.round() as i32,
            },
        };
        if s.exclusive_zone > 0 {
            let z = f64::from(s.exclusive_zone);
            // the edge a zone reserves: anchored to it, and not to its opposite unless it is
            // anchored to both perpendicular edges too (a full-width bar anchored on three sides)
            let edge = match (a.top, a.bottom, a.left, a.right) {
                (true, false, l, r) if l == r => Some('t'),
                (false, true, l, r) if l == r => Some('b'),
                (t, b, true, false) if t == b => Some('l'),
                (t, b, false, true) if t == b => Some('r'),
                _ => None,
            };
            match edge {
                Some('t') => {
                    let d = z + mt;
                    usable.1 += d;
                    usable.3 = (usable.3 - d).max(0.0);
                }
                Some('b') => usable.3 = (usable.3 - (z + mb)).max(0.0),
                Some('l') => {
                    let d = z + ml;
                    usable.0 += d;
                    usable.2 = (usable.2 - d).max(0.0);
                }
                Some('r') => usable.2 = (usable.2 - (z + mr)).max(0.0),
                _ => {}
            }
        }
    }
    usable
}

/// One shown surface of a layer surface, as rendered and hit-tested.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerEntry {
    pub layer: LayerId,
    pub surface: SurfaceKey,
    /// Physical edges in the output's framebuffer.
    pub rect: PhysicalRect,
    pub location: RenderLocation,
    /// The surface's logical origin in the output.
    pub origin: (i32, i32),
    pub size: LocalSize,
    pub input: InputRegion,
}

/// The layer entries of one output that are drawn above the Canvas and those drawn below it, each
/// front to back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerFrame {
    pub above: Vec<LayerEntry>,
    pub below: Vec<LayerEntry>,
}

/// What a layer hit selected: the surface and the position in it (Q16, clamped into it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerHit {
    pub layer: LayerId,
    pub surface: SurfaceKey,
    pub local: LocalPos,
}

/// The entries of one mapped record at output scale `scale`, front to back.
pub(crate) fn entries_of(r: &LayerRecord, scale: f64, out: &mut Vec<LayerEntry>) {
    let Some(tree) = r.tree.as_ref() else { return };
    // F-ORIGIN-1 with the window-geometry origin at the arranged position, in output-logical px
    let origin = crate::geom::CanvasPoint {
        x: i64::from(r.geometry.loc.x),
        y: i64::from(r.geometry.loc.y),
    };
    let Ok(origins) = tree.origins(origin) else {
        return;
    };
    let shown = tree.shown();
    // snapshot order is the render order, front to back (as `render::entries_for`)
    for ((s, o), shown) in tree.surfaces.iter().zip(origins).zip(shown) {
        if !shown || s.size.w <= 0 || s.size.h <= 0 {
            continue;
        }
        let px = |v: i64| (v as f64 * scale).round() as i64;
        let rect = PhysicalRect {
            x0: px(o.x),
            y0: px(o.y),
            x1: px(o.x + i64::from(s.size.w)),
            y1: px(o.y + i64::from(s.size.h)),
        };
        out.push(LayerEntry {
            layer: r.id,
            surface: s.key,
            rect,
            location: RenderLocation {
                x: rect.x0 as f64,
                y: rect.y0 as f64,
            },
            origin: (o.x as i32, o.y as i32),
            size: s.size,
            input: s.input.clone(),
        });
    }
}

/// Whether two trees of a layer surface hold the same surfaces, where they are, how large and
/// what they accept input on (a new frame request is not a change of shape).
fn same_shape(a: &SurfaceTree, b: &SurfaceTree) -> bool {
    a.geometry == b.geometry
        && a.surfaces.len() == b.surfaces.len()
        && a.surfaces.iter().zip(&b.surfaces).all(|(x, y)| {
            (x.key, x.role, x.size, x.mapped, &x.input)
                == (y.key, y.role, y.size, y.mapped, &y.input)
        })
}

/// The front-most entry of `entries` with an input region containing the logical point `p`.
pub(crate) fn select(entries: &[LayerEntry], p: (f64, f64)) -> Option<LayerHit> {
    for e in entries {
        let lx = p.0 - f64::from(e.origin.0);
        let ly = p.1 - f64::from(e.origin.1);
        if lx < 0.0 || ly < 0.0 || lx >= f64::from(e.size.w) || ly >= f64::from(e.size.h) {
            continue;
        }
        let q = |v: f64, extent: i32| {
            ((v * Q16_ONE as f64).round_ties_even() as i64)
                .clamp(0, i64::from(extent) * Q16_ONE - 1)
        };
        let local = LocalPos {
            x: q(lx, e.size.w),
            y: q(ly, e.size.h),
        };
        if e.input
            .contains(local.x.div_euclid(Q16_ONE), local.y.div_euclid(Q16_ONE))
        {
            return Some(LayerHit {
                layer: e.layer,
                surface: e.surface,
                local,
            });
        }
    }
    None
}

use crate::protocol::{Effect, ProtocolError};

impl crate::Core {
    /// The layer surfaces, in creation order.
    pub fn layers(&self) -> &[LayerRecord] {
        &self.layers
    }

    pub fn layer(&self, id: LayerId) -> Option<&LayerRecord> {
        self.layers.iter().find(|r| r.id == id)
    }

    /// The output's usable area (L4): its logical extent less the exclusive zones of its layer
    /// surfaces, in its logical coordinates. `None` for an output that is not connected.
    pub fn usable_area(&self, output: OutputId) -> Option<Area> {
        let model = self.output(output)?;
        let full = model.logical_extent();
        Some(
            self.usable
                .iter()
                .find(|(o, _)| *o == output)
                .map_or((0.0, 0.0, full.0, full.1), |(_, a)| *a),
        )
    }

    /// The layer entries of `output` to draw and hit-test (L2, L3, L6): above the Canvas
    /// `overlay` then `top`, below it `bottom` then `background`, each front to back (a later
    /// surface of the same layer in front). While the output presents a protocol-fullscreen
    /// element only `overlay` surfaces are shown.
    pub fn layer_frame(&self, output: OutputId) -> LayerFrame {
        let mut frame = LayerFrame::default();
        let Some(model) = self.output(output) else {
            return frame;
        };
        let presenting =
            self.presentation(output).is_some() && !self.is_presentation_suppressed(output);
        for layer in [Layer::Overlay, Layer::Top, Layer::Bottom, Layer::Background] {
            if presenting && layer != Layer::Overlay {
                continue;
            }
            let into = if layer.above_canvas() {
                &mut frame.above
            } else {
                &mut frame.below
            };
            for r in self.layers.iter().rev() {
                if r.output == output && r.layer() == layer && r.mapped() {
                    entries_of(r, model.scale, into);
                }
            }
        }
        frame
    }

    /// The layer surface under the output-logical point `p`, among those drawn above the Canvas
    /// (`above`) or below it (L3).
    pub(crate) fn layer_hit(
        &self,
        output: OutputId,
        p: (f64, f64),
        above: bool,
    ) -> Option<LayerHit> {
        if !self.layers.iter().any(|r| r.output == output) {
            return None;
        }
        let frame = self.layer_frame(output);
        select(if above { &frame.above } else { &frame.below }, p)
    }

    /// The constraint target for a popup of the layer surface `id` (xdg-positioner
    /// `get_popup`): its whole output, in the surface's own coordinates — a layer surface does not
    /// move with any camera, so the output is where its popups can be seen. Rounded inward at a
    /// fractional scale (R8). `None` for a surface that is not Core's or not yet arranged.
    pub fn layer_popup_constraint_target(&self, id: LayerId) -> Option<LocalRect> {
        let r = self.layer(id)?;
        r.configured?;
        let (w, h) = self.output(r.output)?.logical_extent();
        Some(LocalRect {
            loc: crate::geom::LocalPoint {
                x: -r.geometry.loc.x,
                y: -r.geometry.loc.y,
            },
            size: LocalSize {
                w: w.floor() as i32,
                h: h.floor() as i32,
            },
        })
    }

    /// The preferred scale of the layer surface `id`'s trees (`(scale_120, integer_scale)`): its
    /// output's. A surface that joins the tree later (a popup) is told it before its first
    /// configure.
    pub fn layer_preferred_scale(&self, id: LayerId) -> Option<(u32, u32)> {
        let scale = self.output(self.layer(id)?.output)?.scale;
        Some(((scale * 120.0).round() as u32, scale.ceil() as u32))
    }

    /// `zwlr_layer_shell_v1.get_layer_surface` (L1, L8: any client): a new layer surface on
    /// `output`, or, when the client named none, on the output that holds the pointer (or the
    /// first one). `None` when no output is connected.
    pub fn layer_created(&mut self, output: Option<OutputId>) -> Option<LayerId> {
        let output = output
            .filter(|o| self.output(*o).is_some())
            .or_else(|| self.pointer_output())
            .or_else(|| self.outputs.first().map(|o| o.id))?;
        self.next_layer += 1;
        let id = LayerId(self.next_layer);
        self.layers.push(LayerRecord {
            id,
            output,
            state: LayerState::default(),
            tree: None,
            geometry: LocalRect::default(),
            configured: None,
            entered: Vec::new(),
        });
        Some(id)
    }

    /// A commit of the layer surface's root: its double-buffered state and its surface tree. The
    /// output is arranged again; surfaces whose size changed are configured (the first commit is
    /// answered with the initial configure); entering and leaving the output follow its mapping;
    /// the keyboard follows its interactivity (L5); and, when something under the pointer moved,
    /// the pointer gets one true-position event (M10).
    pub fn layer_committed(
        &mut self,
        id: LayerId,
        state: LayerState,
        tree: SurfaceTree,
    ) -> Result<Vec<Effect>, ProtocolError> {
        tree.validate().map_err(ProtocolError::Snapshot)?;
        let i = self
            .layers
            .iter()
            .position(|r| r.id == id)
            .ok_or(ProtocolError::StaleElement)?;
        let r = &mut self.layers[i];
        let was_mapped = r.mapped();
        let before = (r.geometry, r.state);
        // what is under the pointer can change without the root changing: a popup opens or closes
        let reshaped = r.tree.as_ref().is_none_or(|old| !same_shape(old, &tree));
        if r.configured.is_none() && tree.root().is_some_and(|s| s.mapped) {
            // W-SM-6 for layer surfaces: a buffer before the initial configure
            return Err(ProtocolError::BufferBeforeConfigure);
        }
        r.state = state;
        r.tree = Some(tree);
        let output = r.output;
        let mut fx = self.relayout(output);
        let r = &self.layers[i];
        let mapped = r.mapped();
        let changed = mapped != was_mapped || (r.geometry, r.state) != before || reshaped;
        fx.extend(self.layer_membership(i));
        if mapped && !was_mapped {
            fx.extend(self.layer_scale(i));
        }
        fx.extend(self.layer_keyboard(i));
        if changed {
            self.invalidate_frame(output);
            self.layer_pointer(output, &mut fx);
        }
        Ok(fx)
    }

    /// The layer surface is destroyed: it leaves its output, lets go of the keyboard, and the
    /// output is arranged without it.
    pub fn layer_destroyed(&mut self, id: LayerId) -> Vec<Effect> {
        let Some(i) = self.layers.iter().position(|r| r.id == id) else {
            return Vec::new();
        };
        let r = self.layers.remove(i);
        let mut fx: Vec<Effect> = r
            .entered
            .iter()
            .map(|&surface| Effect::Leave {
                output: r.output,
                surface,
            })
            .collect();
        fx.extend(self.focus.layer_release(id));
        fx.extend(self.relayout(r.output));
        self.invalidate_frame(r.output);
        self.layer_pointer(r.output, &mut fx);
        fx
    }

    /// Arranges `output`'s layer surfaces; configures those that need a new size.
    pub(crate) fn relayout(&mut self, output: OutputId) -> Vec<Effect> {
        let Some(extent) = self.output(output).map(|m| m.logical_extent()) else {
            return Vec::new();
        };
        let usable = arrange(
            extent,
            self.layers
                .iter_mut()
                .filter(|r| r.output == output && r.tree.is_some()),
        );
        match self.usable.iter_mut().find(|(o, _)| *o == output) {
            Some((_, a)) => *a = usable,
            None => self.usable.push((output, usable)),
        }
        let mut fx = Vec::new();
        for r in self.layers.iter_mut().filter(|r| r.output == output) {
            if r.tree.is_some() && r.configured != Some(r.geometry.size) {
                r.configured = Some(r.geometry.size);
                fx.push(Effect::LayerConfigure {
                    layer: r.id,
                    size: r.geometry.size,
                });
            }
        }
        fx
    }

    /// The output is gone: its layer surfaces are closed (they leave first, INV-OUTPUT-1) and are
    /// no longer Core's.
    pub(crate) fn layers_output_removed(&mut self, output: OutputId) -> Vec<Effect> {
        let mut fx = Vec::new();
        let gone: Vec<LayerId> = self
            .layers
            .iter()
            .filter(|r| r.output == output)
            .map(|r| r.id)
            .collect();
        for id in gone {
            if let Some(i) = self.layers.iter().position(|r| r.id == id) {
                let r = self.layers.remove(i);
                fx.extend(
                    r.entered
                        .iter()
                        .map(|&surface| Effect::Leave { output, surface }),
                );
                fx.extend(self.focus.layer_release(id));
                fx.push(Effect::LayerClosed(id));
            }
        }
        self.usable.retain(|(o, _)| *o != output);
        fx
    }

    /// After an output reconfiguration: arranged again, and told the output's new scale.
    pub(crate) fn layers_output_reconfigured(&mut self, output: OutputId) -> Vec<Effect> {
        let mut fx = self.relayout(output);
        for i in 0..self.layers.len() {
            if self.layers[i].output == output && self.layers[i].mapped() {
                fx.extend(self.layer_scale(i));
            }
        }
        fx
    }

    /// `wl_surface.enter`/`leave` of the output for the record's surfaces, as its mapping says.
    fn layer_membership(&mut self, i: usize) -> Vec<Effect> {
        let r = &mut self.layers[i];
        let now: Vec<SurfaceKey> = match (&r.tree, r.mapped()) {
            (Some(t), true) => t.surfaces.iter().map(|s| s.key).collect(),
            _ => Vec::new(),
        };
        let output = r.output;
        let mut fx = Vec::new();
        for s in r.entered.iter().filter(|s| !now.contains(s)) {
            fx.push(Effect::Leave {
                output,
                surface: *s,
            });
        }
        for s in now.iter().filter(|s| !r.entered.contains(s)) {
            fx.push(Effect::Enter {
                output,
                surface: *s,
            });
        }
        r.entered = now;
        fx
    }

    fn layer_scale(&self, i: usize) -> Vec<Effect> {
        let layer = self.layers[i].id;
        let Some((scale_120, integer_scale)) = self.layer_preferred_scale(layer) else {
            return Vec::new();
        };
        vec![Effect::LayerPreferredScale {
            layer,
            scale_120,
            integer_scale,
        }]
    }

    /// L5: a mapped `top`/`overlay` surface asking for exclusive interactivity takes the keyboard;
    /// one that holds it and no longer may lets it go.
    fn layer_keyboard(&mut self, i: usize) -> Vec<Effect> {
        let r = &self.layers[i];
        let id = r.id;
        let exclusive = r.mapped()
            && r.layer().above_canvas()
            && r.state.keyboard == KeyboardInteractivity::Exclusive;
        let keeps = r.mapped() && r.state.keyboard != KeyboardInteractivity::None;
        if exclusive {
            self.focus.layer_take(id, true)
        } else if self.focus.layer() == Some(id) && !keeps {
            self.focus.layer_release(id)
        } else {
            Vec::new()
        }
    }

    /// M10 after a layer change on `output`: one true-position event when the pointer is on it,
    /// free and not held by a kept lock.
    fn layer_pointer(&mut self, output: OutputId, fx: &mut Vec<Effect>) {
        let on_it = matches!(self.pointer, crate::pointer::PointerState::Located { output: o, .. } if o == output);
        if on_it && !self.input.interaction.active() {
            fx.extend(self.reconcile_constraints());
            if !self.locked() {
                self.true_position_into(fx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Layer surfaces in Core (L1–L8). Expected values are hand-derived literals.

    use super::*;
    use crate::geom::{CanvasPoint, CanvasPos, LocalPoint};
    use crate::navigation::PersistedCameras;
    use crate::output::{OutputDescriptor, Transform};
    use crate::snapshot::{SnapshotSurface, SurfaceRole};
    use crate::{Core, InputPort, OutputLifecycleToken, PhysicalEvent};

    fn tree(k: u64, w: i32, h: i32, mapped: bool) -> SurfaceTree {
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: SurfaceKey::from_raw(std::num::NonZeroU64::new(k).unwrap()),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        }
    }

    fn key(k: u64) -> SurfaceKey {
        SurfaceKey::from_raw(std::num::NonZeroU64::new(k).unwrap())
    }

    /// A Core with one 1920 × 1080 output at scale 1, and its ports (in a struct: no function
    /// returns an authority value, source rule `authority-return`).
    struct Fx {
        c: Core,
        i: InputPort,
        t: OutputLifecycleToken,
        o: OutputId,
        s: crate::ShellPort,
    }

    fn setup() -> Fx {
        let (mut c, i, s, mut t) = Core::new(PersistedCameras::default());
        let (o, _) = c
            .output_connected(
                &mut t,
                OutputDescriptor {
                    key: crate::output::tests::key("A"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        Fx { c, i, t, o, s }
    }

    fn state(layer: Layer, a: (bool, bool, bool, bool), size: (u32, u32), zone: i32) -> LayerState {
        LayerState {
            layer: Some(layer),
            anchors: Anchors {
                top: a.0,
                bottom: a.1,
                left: a.2,
                right: a.3,
            },
            size,
            margins: (0, 0, 0, 0),
            exclusive_zone: zone,
            keyboard: KeyboardInteractivity::None,
        }
    }

    /// Creates, configures and maps a layer surface; returns it and the effects of its map.
    fn map(c: &mut Core, o: OutputId, k: u64, s: LayerState) -> (LayerId, Vec<Effect>) {
        let id = c.layer_created(Some(o)).unwrap();
        let fx = c.layer_committed(id, s, tree(k, 0, 0, false)).unwrap();
        let size = fx
            .iter()
            .find_map(|e| match e {
                Effect::LayerConfigure { layer, size } if *layer == id => Some(*size),
                _ => None,
            })
            .expect("the initial commit is answered with a configure");
        let fx = c
            .layer_committed(id, s, tree(k, size.w, size.h, true))
            .unwrap();
        (id, fx)
    }

    fn map_win(c: &mut Core, k: u64, w: i32, h: i32) -> crate::stage::ElementId {
        let id = c.toplevel_created();
        c.toplevel_committed(id, tree(k, w, h, false)).unwrap();
        c.toplevel_committed(id, tree(k, w, h, true)).unwrap();
        id
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> LocalRect {
        LocalRect {
            loc: LocalPoint { x, y },
            size: LocalSize { w, h },
        }
    }

    /// L4 and the protocol's arrangement: a 30-px bar on top reserves its strip, a 60-px dock on
    /// the left is laid out below the bar and reserves its own; a wallpaper with zone −1 covers the
    /// whole output.
    #[test]
    fn exclusive_zones_arrange_in_order_and_shrink_the_usable_area() {
        let Fx { mut c, o, .. } = setup();
        let (bar, fx) = map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        assert!(
            fx.contains(&Effect::Enter {
                output: o,
                surface: key(1)
            }),
            "{fx:?}"
        );
        assert!(fx.contains(&Effect::LayerPreferredScale {
            layer: bar,
            scale_120: 120,
            integer_scale: 1
        }));
        assert_eq!(c.layer(bar).unwrap().geometry, rect(0, 0, 1920, 30));
        let (dock, _) = map(
            &mut c,
            o,
            2,
            state(Layer::Top, (true, true, true, false), (60, 0), 60),
        );
        assert_eq!(c.layer(dock).unwrap().geometry, rect(0, 30, 60, 1050));
        let (wall, _) = map(
            &mut c,
            o,
            3,
            state(Layer::Background, (true, true, true, true), (0, 0), -1),
        );
        assert_eq!(c.layer(wall).unwrap().geometry, rect(0, 0, 1920, 1080));
        assert_eq!(c.usable_area(o), Some((60.0, 30.0, 1860.0, 1050.0)));
        // the camera's view is not changed by any of it
        assert_eq!(c.output(o).unwrap().view_size(), (1920, 1080));
    }

    /// A buffer before the initial configure is refused (W-SM-6's rule for layer surfaces).
    #[test]
    fn a_buffer_before_the_initial_configure_is_refused() {
        let Fx { mut c, o, .. } = setup();
        let id = c.layer_created(Some(o)).unwrap();
        let s = state(Layer::Top, (true, false, true, true), (0, 30), 30);
        assert_eq!(
            c.layer_committed(id, s, tree(1, 1920, 30, true)),
            Err(ProtocolError::BufferBeforeConfigure)
        );
    }

    /// L3: overlay and top above the Canvas, background below it; a layer target is in its own
    /// surface-local frame (origin 0, the location the position in it).
    #[test]
    fn hit_testing_follows_the_layer_order_and_reports_surface_local_positions() {
        let Fx {
            mut c, mut i, o, ..
        } = setup();
        let w = map_win(&mut c, 9, 400, 300);
        let (_wall, _) = map(
            &mut c,
            o,
            3,
            state(Layer::Background, (true, true, true, true), (0, 0), -1),
        );
        // the pointer starts at the view centre (960, 540): on the window
        let (target, _) = c.route_pointer().unwrap();
        assert_eq!(
            target.map(|t| t.surface),
            Some(key(9)),
            "the window, over the wallpaper"
        );
        let _ = w;
        // off the window: the wallpaper, below the Canvas
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (500.0, 0.0),
                delta_unaccel: (500.0, 0.0),
                time_us: 1,
            },
        )
        .unwrap();
        let (target, loc) = c.route_pointer().unwrap();
        assert_eq!(
            target.map(|t| (t.surface, t.origin)),
            Some((key(3), CanvasPoint { x: 0, y: 0 }))
        );
        assert_eq!(
            loc,
            CanvasPos {
                x: 1460 << 16,
                y: 540 << 16
            }
        );
        // an overlay at the top right covers it
        let (_ov, _) = map(
            &mut c,
            o,
            4,
            state(Layer::Overlay, (true, false, false, true), (600, 1080), 0),
        );
        let (target, loc) = c.route_pointer().unwrap();
        assert_eq!(target.map(|t| t.surface), Some(key(4)));
        assert_eq!(
            loc,
            CanvasPos {
                x: (1460 - 1320) << 16,
                y: 540 << 16
            }
        );
    }

    /// L5: a click on a bar never focuses the window under it; an on-demand surface is focused by
    /// a click; an exclusive overlay takes the keyboard, window focus changes under it do not take
    /// it back, and it returns to the window when the overlay goes.
    #[test]
    fn keyboard_interactivity_follows_the_protocol() {
        let Fx {
            mut c, mut i, o, ..
        } = setup();
        let w = map_win(&mut c, 9, 1920, 1080);
        c.set_focus(&mut i, Some(w)).unwrap();
        let mut s = state(Layer::Top, (true, false, true, true), (0, 30), 30);
        s.keyboard = KeyboardInteractivity::OnDemand;
        let (bar, _) = map(&mut c, o, 1, s);
        let press = |c: &mut Core, i: &mut InputPort, t: u64| {
            for pressed in [true, false] {
                c.physical_input_vec(
                    i,
                    PhysicalEvent::PointerButton {
                        button: 0x110,
                        pressed,
                        modifiers: Default::default(),
                        time_us: t,
                    },
                )
                .unwrap();
            }
        };
        // move onto the bar (y 540 → 10)
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (0.0, -530.0),
                delta_unaccel: (0.0, -530.0),
                time_us: 2,
            },
        )
        .unwrap();
        press(&mut c, &mut i, 3);
        assert_eq!(
            c.focus().layer(),
            Some(bar),
            "an on-demand bar is focused by a click"
        );
        assert_eq!(
            c.focus().target(),
            Some(w),
            "the window focus is kept underneath"
        );

        let mut l = state(Layer::Overlay, (false, false, false, false), (600, 400), 0);
        l.keyboard = KeyboardInteractivity::Exclusive;
        let (launcher, fx) = map(&mut c, o, 5, l);
        assert!(fx.contains(&Effect::LayerKeyboardFocus(launcher)), "{fx:?}");
        let other = map_win(&mut c, 8, 200, 200);
        let fx = c.set_focus(&mut i, Some(other)).unwrap();
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::KeyboardFocus(_))),
            "the exclusive launcher keeps the keyboard: {fx:?}"
        );
        assert_eq!(c.focus().layer(), Some(launcher));
        let fx = c.layer_destroyed(launcher);
        assert!(fx.contains(&Effect::KeyboardFocus(Some(other))), "{fx:?}");
        assert_eq!(c.focus().layer(), None);
    }

    /// L4: F-PLACE-1 centres a new window in the usable area; F-FIT-1 fits a window into it and
    /// shows its centre at the usable centre.
    #[test]
    fn placement_and_fit_use_the_usable_area() {
        let Fx {
            mut c,
            mut i,
            o,
            s: mut port,
            ..
        } = setup();
        map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        // usable (0, 30, 1920, 1050): centre y 555, 15 below the view centre (camera at 0, zoom 1)
        let w = map_win(&mut c, 9, 400, 300);
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: -200, y: -135 }));
        // fit: zoom min(1920/400, 1050/300) = 3.5; the centre moves up by 15/3.5 Canvas px
        c.shell_command(
            &mut port,
            crate::ShellCommand::ViewportFit {
                output: o,
                element: crate::stage::ElementRef {
                    instance: c.stage().instance(),
                    element: w,
                },
            },
        )
        .unwrap();
        assert_eq!(c.cameras().zoom(0), Some(3.5));
        let cy = (-135 * 2 + 300) * (crate::geom::Q16_ONE / 2) - crate::geom::q16(15.0 / 3.5);
        assert_eq!(c.cameras().center(0), Some(CanvasPos { x: 0, y: cy }));
        let _ = &mut i;
    }

    /// L6: while the output presents, only overlay surfaces are shown.
    #[test]
    fn a_presentation_hides_all_but_overlay_surfaces() {
        let Fx {
            mut c, mut i, o, ..
        } = setup();
        let (_bar, _) = map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        let (_osd, _) = map(
            &mut c,
            o,
            2,
            state(Layer::Overlay, (false, false, false, false), (100, 100), 0),
        );
        let w = map_win(&mut c, 9, 400, 300);
        c.set_focus(&mut i, Some(w)).unwrap();
        assert_eq!(c.layer_frame(o).above.len(), 2);
        c.toplevel_request(
            w,
            crate::protocol::ToplevelRequest::SetFullscreen { output: Some(o) },
        )
        .unwrap();
        assert_eq!(c.presentation(o), Some(w));
        let shown: Vec<SurfaceKey> = c.layer_frame(o).above.iter().map(|e| e.surface).collect();
        assert_eq!(shown, [key(2)]);
    }

    /// INV-OUTPUT-1: an output's layer surfaces leave and are closed before the global goes.
    #[test]
    fn an_output_removal_closes_its_layer_surfaces_first() {
        let Fx {
            mut c, mut t, o, ..
        } = setup();
        let (bar, _) = map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        let fx = c.output_disconnected(&mut t, o).unwrap();
        let pos = |e: &Effect| fx.iter().position(|x| x == e).unwrap_or(usize::MAX);
        let (leave, closed, removed) = (
            pos(&Effect::Leave {
                output: o,
                surface: key(1),
            }),
            pos(&Effect::LayerClosed(bar)),
            pos(&Effect::OutputRemoved(o)),
        );
        assert!(leave < closed && closed < removed, "{fx:?}");
        assert!(c.layers().is_empty());
    }

    fn margins(mut s: LayerState, m: (i32, i32, i32, i32)) -> LayerState {
        s.margins = m;
        s
    }

    /// Bottom and right anchors with margins: a bottom bar 40 high, 10 above the bottom edge and
    /// 5 in from each side, reserves 40 + 10; a right dock is laid out in what is left.
    #[test]
    fn bottom_and_right_anchors_with_margins() {
        let Fx { mut c, o, .. } = setup();
        let bottom = margins(
            state(Layer::Top, (false, true, true, true), (0, 40), 40),
            (0, 5, 10, 5),
        );
        let (bar, _) = map(&mut c, o, 1, bottom);
        assert_eq!(c.layer(bar).unwrap().geometry, rect(5, 1030, 1910, 40));
        assert_eq!(c.usable_area(o), Some((0.0, 0.0, 1920.0, 1030.0)));
        let (dock, _) = map(
            &mut c,
            o,
            2,
            state(Layer::Top, (true, true, false, true), (80, 0), 80),
        );
        assert_eq!(c.layer(dock).unwrap().geometry, rect(1840, 0, 80, 1030));
        assert_eq!(c.usable_area(o), Some((0.0, 0.0, 1840.0, 1030.0)));
    }

    /// A zone is meaningful only on one edge (or an edge and both perpendicular ones): a surface in
    /// a corner, or anchored to two parallel edges, reserves nothing.
    #[test]
    fn corner_and_parallel_anchors_reserve_nothing() {
        let Fx { mut c, o, .. } = setup();
        let (corner, _) = map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, false), (100, 100), 50),
        );
        assert_eq!(c.layer(corner).unwrap().geometry, rect(0, 0, 100, 100));
        map(
            &mut c,
            o,
            2,
            state(Layer::Top, (true, true, false, false), (100, 0), 50),
        );
        assert_eq!(c.usable_area(o), Some((0.0, 0.0, 1920.0, 1080.0)));
    }

    /// Zone 0 keeps a surface out of the strips reserved before it; −1 ignores them.
    #[test]
    fn zone_zero_avoids_reserved_strips_and_minus_one_ignores_them() {
        let Fx { mut c, o, .. } = setup();
        map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        let (note, _) = map(
            &mut c,
            o,
            2,
            state(Layer::Overlay, (true, false, false, true), (300, 100), 0),
        );
        assert_eq!(c.layer(note).unwrap().geometry, rect(1620, 30, 300, 100));
        let (osd, _) = map(
            &mut c,
            o,
            3,
            state(Layer::Overlay, (true, false, false, true), (300, 100), -1),
        );
        assert_eq!(c.layer(osd).unwrap().geometry, rect(1620, 0, 300, 100));
    }

    /// A new size or zone from the client: configured again, the usable area follows.
    #[test]
    fn a_new_size_is_configured_and_the_usable_area_follows() {
        let Fx { mut c, o, .. } = setup();
        let (bar, _) = map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        let fx = c
            .layer_committed(
                bar,
                state(Layer::Top, (true, false, true, true), (0, 50), 50),
                tree(1, 1920, 30, true),
            )
            .unwrap();
        assert!(
            fx.contains(&Effect::LayerConfigure {
                layer: bar,
                size: LocalSize { w: 1920, h: 50 }
            }),
            "{fx:?}"
        );
        assert_eq!(c.usable_area(o), Some((0.0, 50.0, 1920.0, 1030.0)));
        // the same state again: nothing to configure
        let fx = c
            .layer_committed(
                bar,
                state(Layer::Top, (true, false, true, true), (0, 50), 50),
                tree(1, 1920, 50, true),
            )
            .unwrap();
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::LayerConfigure { .. })),
            "{fx:?}"
        );
    }

    /// An output reconfigured to 2560 × 1440 at scale 2 (logical 1280 × 720): its bar is arranged
    /// in the new extent and told the new scale.
    #[test]
    fn an_output_reconfiguration_rearranges_and_rescales() {
        let Fx {
            mut c, mut t, o, ..
        } = setup();
        let (bar, _) = map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        let fx = c
            .output_reconfigured(
                &mut t,
                o,
                OutputDescriptor {
                    key: crate::output::tests::key("A"),
                    mode: (2560, 1440),
                    transform: Transform::Normal,
                    scale_120: 240,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        assert!(
            fx.contains(&Effect::LayerConfigure {
                layer: bar,
                size: LocalSize { w: 1280, h: 30 }
            }),
            "{fx:?}"
        );
        assert!(
            fx.contains(&Effect::LayerPreferredScale {
                layer: bar,
                scale_120: 240,
                integer_scale: 2
            }),
            "{fx:?}"
        );
        assert_eq!(c.usable_area(o), Some((0.0, 30.0, 1280.0, 690.0)));
        // until the client answers, its old content is drawn as it is (1920 wide, at scale 2)
        let e = &c.layer_frame(o).above[0];
        assert_eq!(
            (e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1),
            (0, 0, 3840, 60)
        );
        // its answer at the configured size: its physical rect is its logical rect at scale 2
        c.layer_committed(
            bar,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
            tree(1, 1280, 30, true),
        )
        .unwrap();
        let e = &c.layer_frame(o).above[0];
        assert_eq!(
            (e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1),
            (0, 0, 2560, 60)
        );
    }

    /// L1: no camera zoom ever scales a layer surface's events: after a fit (zoom 3.5) the bar
    /// still gets the pointer's position in its own logical px.
    #[test]
    fn a_layer_surfaces_events_ignore_the_camera_zoom() {
        let Fx {
            mut c,
            mut i,
            o,
            s: mut port,
            ..
        } = setup();
        map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        let w = map_win(&mut c, 9, 400, 300);
        c.shell_command(
            &mut port,
            crate::ShellCommand::ViewportFit {
                output: o,
                element: crate::stage::ElementRef {
                    instance: c.stage().instance(),
                    element: w,
                },
            },
        )
        .unwrap();
        assert_eq!(c.cameras().zoom(0), Some(3.5));
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (0.0, -530.0),
                delta_unaccel: (0.0, -530.0),
                time_us: 1,
            },
        )
        .unwrap();
        let (target, loc) = c.route_pointer().unwrap();
        assert_eq!(target.map(|t| t.surface), Some(key(1)));
        assert_eq!(
            loc,
            CanvasPos {
                x: 960 << 16,
                y: 10 << 16
            }
        );
    }

    /// L7: Logo + middle button on a bar starts a camera pan, as over a window.
    #[test]
    fn a_pan_that_starts_on_a_bar_pans() {
        let Fx {
            mut c, mut i, o, ..
        } = setup();
        map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (0.0, -530.0),
                delta_unaccel: (0.0, -530.0),
                time_us: 1,
            },
        )
        .unwrap();
        let logo = crate::input::bindings::Modifiers {
            logo: true,
            ..Default::default()
        };
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerButton {
                button: crate::input::bindings::BTN_MIDDLE,
                pressed: true,
                modifiers: logo,
                time_us: 2,
            },
        )
        .unwrap();
        assert!(c.interaction().active(), "the press started a pan");
        let before = c.cameras().center(0);
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (100.0, 0.0),
                delta_unaccel: (100.0, 0.0),
                time_us: 3,
            },
        )
        .unwrap();
        assert_ne!(c.cameras().center(0), before, "the camera moved");
    }

    /// Destroying the bar gives its strip back.
    #[test]
    fn destroying_a_bar_frees_its_strip() {
        let Fx { mut c, o, .. } = setup();
        let (bar, _) = map(
            &mut c,
            o,
            1,
            state(Layer::Top, (true, false, true, true), (0, 30), 30),
        );
        assert_eq!(c.usable_area(o), Some((0.0, 30.0, 1920.0, 1050.0)));
        c.layer_destroyed(bar);
        assert_eq!(c.usable_area(o), Some((0.0, 0.0, 1920.0, 1080.0)));
        assert!(c.layer_frame(o).above.is_empty());
    }

    /// A popup of a bar (`zwlr_layer_surface_v1.get_popup`) is part of the bar's tree: drawn with
    /// it, in front of it, and hit-tested before the window under it, in its own surface-local
    /// frame.
    #[test]
    fn a_bars_popup_is_drawn_and_hit_in_front_of_the_window_under_it() {
        let Fx {
            mut c, mut i, o, ..
        } = setup();
        let _w = map_win(&mut c, 9, 1920, 1080);
        let s = state(Layer::Top, (true, false, true, true), (0, 30), 30);
        let (bar, _) = map(&mut c, o, 1, s);
        let mut t = tree(1, 1920, 30, true);
        t.surfaces.insert(
            0,
            SnapshotSurface {
                key: key(2),
                role: SurfaceRole::Popup {
                    position: LocalPoint { x: 100, y: 30 },
                    geometry_loc: LocalPoint { x: 0, y: 0 },
                },
                size: LocalSize { w: 200, h: 300 },
                mapped: true,
                input: InputRegion::Infinite,
                frame_requested: false,
            },
        );
        c.layer_committed(bar, s, t).unwrap();
        let frame = c.layer_frame(o);
        assert_eq!(
            frame
                .above
                .iter()
                .map(|e| (e.surface, e.origin))
                .collect::<Vec<_>>(),
            [(key(2), (100, 30)), (key(1), (0, 0))],
            "the popup in front of its bar"
        );
        // from the view centre (960, 540) to (150, 100): inside the popup, over the window
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (-810.0, -440.0),
                delta_unaccel: (-810.0, -440.0),
                time_us: 1,
            },
        )
        .unwrap();
        let (target, loc) = c.route_pointer().unwrap();
        assert_eq!(target.map(|t| t.surface), Some(key(2)));
        assert_eq!(
            loc,
            CanvasPos {
                x: 50 << 16,
                y: 70 << 16
            }
        );
    }

    /// A layer popup's constraint target is its whole output, in the surface's coordinates: for a
    /// bar at the bottom, the output starts 1050 px above it. Before the surface is arranged there
    /// is none.
    #[test]
    fn a_layer_popup_is_constrained_to_the_whole_output() {
        let Fx { mut c, o, .. } = setup();
        let pending = c.layer_created(Some(o)).unwrap();
        assert_eq!(c.layer_popup_constraint_target(pending), None);
        let (bar, _) = map(
            &mut c,
            o,
            1,
            state(Layer::Top, (false, true, true, true), (0, 30), 30),
        );
        assert_eq!(c.layer(bar).unwrap().geometry, rect(0, 1050, 1920, 30));
        assert_eq!(
            c.layer_popup_constraint_target(bar),
            Some(rect(0, -1050, 1920, 1080))
        );
        assert_eq!(c.layer_preferred_scale(bar), Some((120, 1)));
    }
}
