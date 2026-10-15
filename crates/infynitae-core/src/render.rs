//! Render list and ProjectionFrame (OBJ-15), and the physical-pixel hit-test (F-HIT-1).
//!
//! The render list is the front-to-back sequence of visible surfaces of one output: elements in
//! reverse z-order, each element's surfaces in snapshot order (INV-RENDER-2). The ProjectionFrame
//! is the read-only record of that list; hit-testing reads it, so what is hit is what was shown
//! (INV-HIT-1).

use crate::geom::{
    CanvasPoint, LocalPos, LocalSize, OutputLogicalPoint, PhysicalRect, Q16_ONE, RenderLocation,
};
use crate::projection::View;
use crate::snapshot::{InputRegion, SurfaceKey, SurfaceTree};
use crate::stage::{CanvasBox, ElementId, Placed, Stage};

#[derive(Clone, Debug, PartialEq)]
pub struct FrameEntry {
    pub element: ElementId,
    pub surface: SurfaceKey,
    /// Canvas origin from the single origin function (F-ORIGIN-1).
    pub origin: CanvasPoint,
    pub size: LocalSize,
    /// Physical edges in view space (F-LATTICE-1).
    pub rect: PhysicalRect,
    /// Render location for Smithay (F-RENDER-1).
    pub location: RenderLocation,
    pub input: InputRegion,
}

/// What hit-testing selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub element: ElementId,
    pub surface: SurfaceKey,
    /// The surface's Canvas origin (F-ORIGIN-1), the focus origin given to Smithay (B2.5).
    pub origin: CanvasPoint,
    /// Delivered coordinates: `q − origin`, clamped into `[0, w) × [0, h)` (Q16).
    pub local: LocalPos,
}

/// The render-list entries of one element under `view`: its shown surfaces in snapshot order,
/// with origins from the single origin function, culled on i64 edges (F-LATTICE-1).
pub fn entries_for(
    view: &View,
    element: ElementId,
    anchor: CanvasPoint,
    tree: &SurfaceTree,
) -> Vec<FrameEntry> {
    // a presentation view shows one element's tree and nothing else (OBJ-19)
    if view.only.is_some_and(|w| w != element) {
        return Vec::new();
    }
    let Ok(origins) = tree.origins(anchor) else {
        return Vec::new();
    };
    let shown = tree.shown();
    let mut out = Vec::new();
    for ((s, origin), shown) in tree.surfaces.iter().zip(origins).zip(shown) {
        if !shown {
            continue;
        }
        let rect = view.physical_rect(origin, s.size);
        if !view.intersects_view(&rect) {
            continue;
        }
        out.push(FrameEntry {
            element,
            surface: s.key,
            origin,
            size: s.size,
            rect,
            location: view.render_location(origin),
            input: s.input.clone(),
        });
    }
    out
}

/// The production projection of one element under `view`, shared by the render list and
/// membership: for each shown surface (snapshot order) whose physical rect intersects the view,
/// `f(surface index, Canvas origin, rect)`. Behind the bounding-box cull ([`View::may_show`]),
/// from the element's stored layout, without allocating (INV-ALLOC-1). [`entries_for`], which
/// recomputes origins and shown flags from the tree, is its reference.
pub(crate) fn visit_entries(
    view: &View,
    p: &Placed<'_>,
    mut f: impl FnMut(usize, CanvasPoint, PhysicalRect),
) {
    // a presentation view shows one element's tree and nothing else (OBJ-19)
    if view.only.is_some_and(|w| w != p.id) || !p.layout.is_valid() {
        return;
    }
    match p.bounds {
        Some((x0, y0, x1, y1)) if view.may_show(x0, y0, x1, y1) => {}
        _ => return,
    }
    for (i, s) in p.tree.surfaces.iter().enumerate() {
        if !p.layout.shown(i) {
            continue;
        }
        let origin = p.layout.origin(i, p.anchor);
        let rect = view.physical_rect(origin, s.size);
        if view.intersects_view(&rect) {
            f(i, origin, rect);
        }
    }
}

/// [`entries_for`] behind the element's bounding box: an element whose box cannot intersect the
/// view ([`View::may_show`]) has no entries, and is skipped without projecting its surfaces.
pub(crate) fn entries_culled(
    view: &View,
    element: ElementId,
    anchor: CanvasPoint,
    tree: &SurfaceTree,
    bounds: Option<CanvasBox>,
) -> Vec<FrameEntry> {
    match bounds {
        Some((x0, y0, x1, y1)) if view.may_show(x0, y0, x1, y1) => {
            entries_for(view, element, anchor, tree)
        }
        _ => Vec::new(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionFrame {
    pub view: View,
    /// Front to back; only surfaces that are shown and intersect `[0, view_O)`.
    pub entries: Vec<FrameEntry>,
}

impl ProjectionFrame {
    /// Builds the render list of every mapped element of `stage` under `view`.
    pub fn build(view: View, stage: &Stage) -> ProjectionFrame {
        let mut frame = ProjectionFrame {
            view,
            entries: Vec::new(),
        };
        frame.rebuild(view, stage);
        frame
    }

    /// [`ProjectionFrame::build`] into this frame's storage: the entries are cleared, not freed,
    /// so a frame rebuilt every vblank allocates only when it holds more entries than ever before
    /// (C14.3 "per-output buffers reused frame to frame").
    pub(crate) fn rebuild(&mut self, view: View, stage: &Stage) {
        self.view = view;
        self.entries.clear();
        let entries = &mut self.entries;
        for p in stage.placed_front_to_back() {
            visit_entries(&view, &p, |i, origin, rect| {
                let s = &p.tree.surfaces[i];
                entries.push(FrameEntry {
                    element: p.id,
                    surface: s.key,
                    origin,
                    size: s.size,
                    rect,
                    location: view.render_location(origin),
                    input: s.input.clone(),
                });
            });
        }
    }

    /// The render list without the bounding-box cull: every surface of every mapped element is
    /// projected. The oracle of the differential tests.
    #[cfg(test)]
    pub(crate) fn build_reference(view: View, stage: &Stage) -> ProjectionFrame {
        let mut entries = Vec::new();
        for (element, anchor, tree) in stage.front_to_back() {
            entries.extend(entries_for(&view, element, anchor, tree));
        }
        ProjectionFrame { view, entries }
    }

    /// F-HIT-1 on this frame: the first entry (front to back) whose physical rect covers the
    /// pixel `⌊p · scale⌋` and whose input region contains the clamped surface-local point of
    /// `q = F-UNPROJECT-1(p)`.
    pub fn select(&self, p: OutputLogicalPoint) -> Option<Hit> {
        let (px, py) = self.view.pixel(p);
        let q = self.view.unproject(p);
        for e in &self.entries {
            if !e.rect.contains(px, py) || e.size.w <= 0 || e.size.h <= 0 {
                continue;
            }
            let clamp = |v: i64, extent: i32| v.clamp(0, extent as i64 * Q16_ONE - 1);
            let local = LocalPos {
                x: clamp(q.x - e.origin.x * Q16_ONE, e.size.w),
                y: clamp(q.y - e.origin.y * Q16_ONE, e.size.h),
            };
            if e.input
                .contains(local.x.div_euclid(Q16_ONE), local.y.div_euclid(Q16_ONE))
            {
                return Some(Hit {
                    element: e.element,
                    surface: e.surface,
                    origin: e.origin,
                    local,
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{CanvasPos, LocalPoint, LocalRect};
    use crate::projection::tests::{Rng, view};
    use crate::snapshot::{RegionOp, SnapshotSurface, SurfaceRole, SurfaceTree, tests::key};
    use crate::stage::{InstanceId, Lifecycle};

    fn pt(x: i32, y: i32) -> LocalPoint {
        LocalPoint { x, y }
    }
    fn rect(x: i32, y: i32, w: i32, h: i32) -> LocalRect {
        LocalRect {
            loc: pt(x, y),
            size: LocalSize { w, h },
        }
    }
    fn surf(k: u64, role: SurfaceRole, w: i32, h: i32, input: InputRegion) -> SnapshotSurface {
        SnapshotSurface {
            key: key(k),
            role,
            size: LocalSize { w, h },
            mapped: true,
            input,
        }
    }

    /// The xhit tree (TST-10): W 440x340 with geometry (20,20) 400x300 and input region
    /// geometry minus a hole (20,55) 20x30; S1 above, S1a nested above S1, S2 below at (−15,50);
    /// popup P (shadow 10) at (100,50) with PS. Front-to-back. Keys: W=1 S1=2 S1a=3 S2=4 P=5 PS=6.
    fn xhit_tree() -> SurfaceTree {
        let w_region = InputRegion::Ops(std::sync::Arc::from(vec![
            RegionOp {
                add: true,
                rect: rect(20, 20, 400, 300),
            },
            RegionOp {
                add: false,
                rect: rect(20, 55, 20, 30),
            },
        ]));
        SurfaceTree {
            geometry: rect(20, 20, 400, 300),
            surfaces: vec![
                surf(
                    6,
                    SurfaceRole::Subsurface {
                        parent: 1,
                        offset: pt(5, 5),
                    },
                    30,
                    30,
                    InputRegion::Infinite,
                ),
                surf(
                    5,
                    SurfaceRole::Popup {
                        position: pt(100, 50),
                        geometry_loc: pt(10, 10),
                    },
                    140,
                    100,
                    InputRegion::Infinite,
                ),
                surf(
                    3,
                    SurfaceRole::Subsurface {
                        parent: 3,
                        offset: pt(10, 10),
                    },
                    30,
                    20,
                    InputRegion::Infinite,
                ),
                surf(
                    2,
                    SurfaceRole::Subsurface {
                        parent: 4,
                        offset: pt(37, 11),
                    },
                    101,
                    53,
                    InputRegion::Infinite,
                ),
                surf(1, SurfaceRole::Root, 440, 340, w_region),
                surf(
                    4,
                    SurfaceRole::Subsurface {
                        parent: 4,
                        offset: pt(-15, 50),
                    },
                    60,
                    40,
                    InputRegion::Infinite,
                ),
            ],
        }
    }

    /// Stage with the xhit tree mapped at anchor (500,400): root surface origin (480,380).
    fn stage_with(tree: SurfaceTree) -> (Stage, ElementId) {
        let mut s = Stage::new(InstanceId(1));
        let id = s.create();
        s.place(id, CanvasPoint { x: 500, y: 400 }).unwrap();
        s.set_tree(id, Some(tree)).unwrap();
        s.set_state(id, Lifecycle::Mapped).unwrap();
        s.raise(id);
        (s, id)
    }

    /// Semantic cases from xhit at k = 1 with an integral camera: the view origin is chosen so
    /// that root-surface-local (x,y) is physical pixel (x,y).
    #[test]
    fn xhit_semantic_cases_at_scale_one() {
        let (stage, _) = stage_with(xhit_tree());
        // v = root origin (480,380): centre = v + view/2
        let v = view(480.0 + 960.0, 380.0 + 540.0, 1.0, (1920, 1080));
        assert_eq!(
            v.v,
            CanvasPos {
                x: 480 * Q16_ONE,
                y: 380 * Q16_ONE
            }
        );
        let frame = ProjectionFrame::build(v, &stage);
        let at = |x: i32, y: i32| {
            frame
                .select(OutputLogicalPoint {
                    x: x as f64 + 0.5,
                    y: y as f64 + 0.5,
                })
                .map(|h| h.surface.raw().get())
        };
        assert_eq!(at(200, 250), Some(1), "root window area");
        assert_eq!(at(5, 5), None, "root shadow outside the input region");
        assert_eq!(at(40, 15), Some(2), "S1 above root");
        assert_eq!(at(50, 25), Some(3), "S1a nested above S1");
        assert_eq!(at(40, 12), Some(2), "S1 outside root's input region");
        assert_eq!(at(30, 88), Some(1), "root over S2 outside the hole");
        assert_eq!(at(25, 60), Some(4), "hole falls through to S2 below");
        assert_eq!(at(-10, 60), Some(4), "S2 left of the root");
        assert_eq!(at(150, 100), Some(5), "popup over root");
        assert_eq!(at(120, 70), Some(6), "popup subsurface");
        assert_eq!(at(41, 70), Some(1), "right of the hole, below S1");
        assert_eq!(at(41, 60), Some(2), "inside S1 next to the hole");
    }

    /// Delivered coordinates are `q − origin`, clamped into the surface (half-open).
    #[test]
    fn delivered_coordinates_are_clamped_into_the_surface() {
        let (stage, _) = stage_with(xhit_tree());
        let v = view(480.0 + 960.0, 380.0 + 540.0, 1.0, (1920, 1080));
        let frame = ProjectionFrame::build(v, &stage);
        let h = frame
            .select(OutputLogicalPoint {
                x: 200.25,
                y: 250.5,
            })
            .unwrap();
        assert_eq!(
            h.local,
            LocalPos {
                x: (200 * 4 + 1) * Q16_ONE / 4,
                y: 250 * Q16_ONE + Q16_ONE / 2
            }
        );
        // at a fractional scale the covered pixel can extend past the logical edge: the delivered
        // point stays inside [0, w)
        let v = view(480.0 + 960.0 / 1.5, 380.0 + 540.0 / 1.5, 1.5, (1920, 1080));
        let frame = ProjectionFrame::build(v, &stage);
        for e in &frame.entries {
            for (x, y) in [(e.rect.x0, e.rect.y0), (e.rect.x1 - 1, e.rect.y1 - 1)] {
                let p = OutputLogicalPoint {
                    x: (x as f64 + 0.5) / 1.5,
                    y: (y as f64 + 0.5) / 1.5,
                };
                if let Some(h) = frame.select(p) {
                    let s = frame
                        .entries
                        .iter()
                        .find(|f| f.surface == h.surface)
                        .unwrap();
                    assert!(h.local.x >= 0 && h.local.x < s.size.w as i64 * Q16_ONE);
                    assert!(h.local.y >= 0 && h.local.y < s.size.h as i64 * Q16_ONE);
                }
            }
        }
    }

    /// Reference evaluator: paints the region cell by cell, op by op, on a bitmap.
    fn reference(ops: &[RegionOp], extent: i32) -> Vec<bool> {
        let n = (extent * 2) as usize;
        let mut bits = vec![false; n * n];
        for op in ops {
            for y in -extent..extent {
                for x in -extent..extent {
                    let inside = x >= op.rect.loc.x
                        && y >= op.rect.loc.y
                        && x < op.rect.loc.x + op.rect.size.w
                        && y < op.rect.loc.y + op.rect.size.h;
                    if inside {
                        bits[((y + extent) as usize) * n + (x + extent) as usize] = op.add;
                    }
                }
            }
        }
        bits
    }

    /// OB-2 (T0): multi-rect, interleaved add/subtract, random; equals the reference evaluator.
    #[test]
    fn ob2_region_equals_reference_evaluator() {
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        let extent = 24;
        for _ in 0..400 {
            let ops: Vec<RegionOp> = (0..rng.range(1, 8))
                .map(|_| RegionOp {
                    add: rng.range(0, 2) > 0,
                    rect: rect(
                        rng.range(-20, 20) as i32,
                        rng.range(-20, 20) as i32,
                        rng.range(-3, 25) as i32,
                        rng.range(-3, 25) as i32,
                    ),
                })
                .collect();
            let want = reference(&ops, extent);
            let region = InputRegion::Ops(ops.into());
            let n = (extent * 2) as usize;
            for y in -extent..extent {
                for x in -extent..extent {
                    assert_eq!(
                        region.contains(x as i64, y as i64),
                        want[((y + extent) as usize) * n + (x + extent) as usize],
                        "region mismatch at ({x},{y})"
                    );
                }
            }
        }
    }

    /// Every covered pixel selects the frontmost covering, eligible entry: the physical rule.
    /// Negative control: the canvas-exact rule (D-B2-I9) disagrees at a fractional scale.
    #[test]
    fn physical_selection_differs_from_the_canvas_exact_rule_at_fractional_scale() {
        let (stage, _) = stage_with(xhit_tree());
        let v = view(
            400.3 + 960.0 / 1.25,
            300.7 + 540.0 / 1.25,
            1.25,
            (1920, 1080),
        );
        let frame = ProjectionFrame::build(v, &stage);
        let mut differ = 0;
        for j in 0..700 {
            for i in 0..800 {
                let p = OutputLogicalPoint {
                    x: (i as f64 + 0.5) / 1.25,
                    y: (j as f64 + 0.5) / 1.25,
                };
                let ours = frame.select(p).map(|h| h.surface);
                let q = v.unproject(p);
                let exact = frame.entries.iter().find(|e| {
                    let lx = q.x - e.origin.x * Q16_ONE;
                    let ly = q.y - e.origin.y * Q16_ONE;
                    lx >= 0
                        && ly >= 0
                        && lx < e.size.w as i64 * Q16_ONE
                        && ly < e.size.h as i64 * Q16_ONE
                        && e.input
                            .contains(lx.div_euclid(Q16_ONE), ly.div_euclid(Q16_ONE))
                });
                if ours != exact.map(|e| e.surface) {
                    differ += 1;
                }
            }
        }
        assert!(
            differ > 0,
            "the canvas-exact control must disagree with the physical rule"
        );
    }
}
