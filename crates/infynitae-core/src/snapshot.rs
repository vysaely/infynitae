//! Surface-tree snapshot (OBJ-05, CP2-2) and the single surface-origin function (F-ORIGIN-1,
//! INV-COORD-6).
//!
//! A snapshot is client state copied from Smithay by the adapter's commit hook. It holds values
//! only: an opaque [`SurfaceKey`] per surface, never a protocol handle. Core never writes it.

use std::num::NonZeroU64;

use crate::geom::{CanvasPoint, LocalPoint, LocalRect, LocalSize};

/// Opaque surface name minted by the adapter (OBJ-06). Core cannot dereference it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SurfaceKey(NonZeroU64);

impl SurfaceKey {
    pub fn from_raw(raw: NonZeroU64) -> Self {
        SurfaceKey(raw)
    }

    pub fn raw(self) -> NonZeroU64 {
        self.0
    }
}

/// How a surface is positioned relative to its tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceRole {
    /// The toplevel's root surface.
    Root,
    /// A subsurface at `offset` (`wl_subsurface` position) from the surface at index `parent`.
    Subsurface { parent: usize, offset: LocalPoint },
    /// A popup root: `position` is the popup's configured position relative to the toplevel's
    /// window-geometry origin; `geometry_loc` is the popup's own window-geometry origin.
    Popup {
        position: LocalPoint,
        geometry_loc: LocalPoint,
    },
}

/// One input-region request, in protocol order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionOp {
    pub add: bool,
    pub rect: LocalRect,
}

/// `wl_surface.set_input_region`: unset means the whole surface. The operations are shared, not
/// copied: every render list that shows the surface holds the same list (a clone is a reference
/// count, so building a frame allocates nothing for it, INV-ALLOC-1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputRegion {
    Infinite,
    Ops(std::sync::Arc<[RegionOp]>),
}

impl InputRegion {
    /// Whether the integral surface-local cell is in the region: add/subtract applied in request
    /// order, half-open rectangles (F-HIT-1).
    pub fn contains(&self, x: i64, y: i64) -> bool {
        match self {
            InputRegion::Infinite => true,
            InputRegion::Ops(ops) => ops
                .iter()
                .rev()
                .find(|op| op.rect.contains(x, y))
                .is_some_and(|op| op.add),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotSurface {
    pub key: SurfaceKey,
    pub role: SurfaceRole,
    /// Logical size after viewport and buffer scale.
    pub size: LocalSize,
    /// The surface has a buffer.
    pub mapped: bool,
    pub input: InputRegion,
    /// The surface has frame callbacks that have not completed yet (`wl_surface.frame` in
    /// committed state; D7 refinement PD-F1-06). Read by the frame policy (OBJ-17, M16).
    pub frame_requested: bool,
}

/// One element's surfaces, front-to-back: each popup tree, then the root tree, each traversed
/// downward (INV-RENDER-2, C3.3). The adapter supplies the order; core only uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceTree {
    /// Window geometry in root-surface-local coordinates.
    pub geometry: LocalRect,
    pub surfaces: Vec<SnapshotSurface>,
}

/// Integral Canvas edges relative to an element's anchor (see [`SurfaceTree::extent`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// Not exactly one root surface.
    RootCount(usize),
    /// A subsurface names a parent index that is out of range or itself.
    BadParent { surface: usize },
    /// A subsurface chain does not end at the root or a popup.
    Cycle { surface: usize },
    /// The same key appears twice.
    DuplicateKey { surface: usize },
    /// A negative size.
    NegativeSize { surface: usize },
}

impl SurfaceTree {
    /// Whether any surface of the tree has a pending frame callback (OBJ-05).
    pub fn frame_requested(&self) -> bool {
        self.surfaces.iter().any(|s| s.frame_requested)
    }

    /// Structural validation; a snapshot that fails it is rejected whole.
    pub fn validate(&self) -> Result<(), SnapshotError> {
        let n = self.surfaces.len();
        let roots = self
            .surfaces
            .iter()
            .filter(|s| s.role == SurfaceRole::Root)
            .count();
        if roots != 1 {
            return Err(SnapshotError::RootCount(roots));
        }
        for (i, s) in self.surfaces.iter().enumerate() {
            if s.size.w < 0 || s.size.h < 0 {
                return Err(SnapshotError::NegativeSize { surface: i });
            }
            if self.surfaces[..i].iter().any(|t| t.key == s.key) {
                return Err(SnapshotError::DuplicateKey { surface: i });
            }
            // walk the parent chain; it must reach a root or popup within n steps
            let mut cur = i;
            for step in 0..=n {
                match self.surfaces[cur].role {
                    SurfaceRole::Root | SurfaceRole::Popup { .. } => break,
                    SurfaceRole::Subsurface { parent, .. } => {
                        if parent >= n || parent == cur {
                            return Err(SnapshotError::BadParent { surface: cur });
                        }
                        cur = parent;
                    }
                }
                if step == n {
                    return Err(SnapshotError::Cycle { surface: i });
                }
            }
        }
        Ok(())
    }

    /// F-ORIGIN-1: the Canvas origin of every surface, in snapshot order.
    /// * root: `anchor − geometry.loc`;
    /// * popup: `root_origin + geometry.loc + position − popup geometry_loc`;
    /// * subsurface: parent origin + `offset` (accumulated).
    pub fn origins(&self, anchor: CanvasPoint) -> Result<Vec<CanvasPoint>, SnapshotError> {
        self.validate()?;
        let root = CanvasPoint {
            x: anchor.x - self.geometry.loc.x as i64,
            y: anchor.y - self.geometry.loc.y as i64,
        };
        let mut out: Vec<Option<CanvasPoint>> = vec![None; self.surfaces.len()];
        // validate() guarantees every chain terminates, so this converges in at most n passes
        while out.iter().any(Option::is_none) {
            for (i, s) in self.surfaces.iter().enumerate() {
                if out[i].is_some() {
                    continue;
                }
                out[i] = match s.role {
                    SurfaceRole::Root => Some(root),
                    SurfaceRole::Popup {
                        position,
                        geometry_loc,
                    } => Some(CanvasPoint {
                        x: root.x + self.geometry.loc.x as i64 + position.x as i64
                            - geometry_loc.x as i64,
                        y: root.y + self.geometry.loc.y as i64 + position.y as i64
                            - geometry_loc.y as i64,
                    }),
                    SurfaceRole::Subsurface { parent, offset } => {
                        out[parent].map(|p| CanvasPoint {
                            x: p.x + offset.x as i64,
                            y: p.y + offset.y as i64,
                        })
                    }
                };
            }
        }
        Ok(out.into_iter().flatten().collect())
    }

    /// Whether each surface is shown: it has a buffer and so does every ancestor in its tree
    /// (Smithay skips the children of a surface without a view).
    pub fn shown(&self) -> Vec<bool> {
        (0..self.surfaces.len())
            .map(|i| {
                let mut cur = i;
                loop {
                    let s = &self.surfaces[cur];
                    if !s.mapped {
                        return false;
                    }
                    match s.role {
                        SurfaceRole::Subsurface { parent, .. } if parent < self.surfaces.len() => {
                            cur = parent
                        }
                        _ => return true,
                    }
                }
            })
            .collect()
    }

    /// The bounding box, relative to the anchor, of every surface that can produce a render-list
    /// entry: shown and of positive size. `None` when there is none (or the tree is invalid), so
    /// the element is on no output whatever the view. Origins are the anchor plus integers
    /// (F-ORIGIN-1), so the box translates with the anchor exactly.
    pub fn extent(&self) -> Option<Extent> {
        let origins = self.origins(CanvasPoint { x: 0, y: 0 }).ok()?;
        let mut out: Option<Extent> = None;
        for ((s, o), shown) in self.surfaces.iter().zip(origins).zip(self.shown()) {
            if !shown || s.size.w <= 0 || s.size.h <= 0 {
                continue;
            }
            let e = Extent {
                x0: o.x,
                y0: o.y,
                x1: o.x + s.size.w as i64,
                y1: o.y + s.size.h as i64,
            };
            out = Some(match out {
                None => e,
                Some(b) => Extent {
                    x0: b.x0.min(e.x0),
                    y0: b.y0.min(e.y0),
                    x1: b.x1.max(e.x1),
                    y1: b.y1.max(e.y1),
                },
            });
        }
        out
    }

    pub fn root(&self) -> Option<&SnapshotSurface> {
        self.surfaces.iter().find(|s| s.role == SurfaceRole::Root)
    }
}

/// F-ORIGIN-1 and the shown flags of one surface tree, relative to its anchor, computed once per
/// snapshot into storage that its owner reuses (the Stage keeps one per element): the render list,
/// membership and the bounding box read it instead of recomputing them on every pass, and a new
/// snapshot rewrites it in place, cleared, not freed (INV-ALLOC-1, C14.3). Equal by construction
/// to [`SurfaceTree::origins`] and [`SurfaceTree::shown`], its reference implementations.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TreeLayout {
    /// The Canvas origin of each surface with the anchor at (0, 0), in snapshot order.
    rel: Vec<CanvasPoint>,
    /// Whether each surface is shown: it has a buffer, and so does every ancestor.
    shown: Vec<bool>,
    /// Scratch of the origin computation (which origins are resolved).
    done: Vec<bool>,
    /// The tree was valid; an invalid tree has an empty layout and no entries.
    valid: bool,
}

impl TreeLayout {
    /// Rewrites the layout for `tree` (`None`: no tree), reusing this layout's storage.
    pub(crate) fn compute(&mut self, tree: Option<&SurfaceTree>) {
        self.rel.clear();
        self.shown.clear();
        self.done.clear();
        self.valid = false;
        let Some(tree) = tree else {
            return;
        };
        if tree.validate().is_err() {
            return;
        }
        let n = tree.surfaces.len();
        let root = CanvasPoint {
            x: -(tree.geometry.loc.x as i64),
            y: -(tree.geometry.loc.y as i64),
        };
        self.rel.resize(n, root);
        self.done.resize(n, false);
        // validate() guarantees every chain terminates, so this converges in at most n passes
        while self.done.iter().any(|d| !d) {
            for (i, s) in tree.surfaces.iter().enumerate() {
                if self.done[i] {
                    continue;
                }
                match s.role {
                    SurfaceRole::Root => {
                        self.rel[i] = root;
                        self.done[i] = true;
                    }
                    SurfaceRole::Popup {
                        position,
                        geometry_loc,
                    } => {
                        self.rel[i] = CanvasPoint {
                            x: root.x + tree.geometry.loc.x as i64 + position.x as i64
                                - geometry_loc.x as i64,
                            y: root.y + tree.geometry.loc.y as i64 + position.y as i64
                                - geometry_loc.y as i64,
                        };
                        self.done[i] = true;
                    }
                    SurfaceRole::Subsurface { parent, offset } => {
                        if self.done[parent] {
                            let p = self.rel[parent];
                            self.rel[i] = CanvasPoint {
                                x: p.x + offset.x as i64,
                                y: p.y + offset.y as i64,
                            };
                            self.done[i] = true;
                        }
                    }
                }
            }
        }
        for i in 0..n {
            let mut cur = i;
            let shown = loop {
                let s = &tree.surfaces[cur];
                if !s.mapped {
                    break false;
                }
                match s.role {
                    SurfaceRole::Subsurface { parent, .. } if parent < n => cur = parent,
                    _ => break true,
                }
            };
            self.shown.push(shown);
        }
        self.valid = true;
    }

    /// Whether the layout is of a valid tree (an invalid one projects nothing).
    pub(crate) fn is_valid(&self) -> bool {
        self.valid
    }

    /// Surface `i`'s Canvas origin for the element anchored at `anchor` (F-ORIGIN-1).
    pub(crate) fn origin(&self, i: usize, anchor: CanvasPoint) -> CanvasPoint {
        CanvasPoint {
            x: anchor.x + self.rel[i].x,
            y: anchor.y + self.rel[i].y,
        }
    }

    pub(crate) fn shown(&self, i: usize) -> bool {
        self.shown[i]
    }

    /// [`SurfaceTree::extent`] from the layout: the box, relative to the anchor, of every shown
    /// surface of positive size.
    pub(crate) fn extent(&self, tree: &SurfaceTree) -> Option<Extent> {
        if !self.valid {
            return None;
        }
        let mut out: Option<Extent> = None;
        for (i, s) in tree.surfaces.iter().enumerate() {
            if !self.shown[i] || s.size.w <= 0 || s.size.h <= 0 {
                continue;
            }
            let o = self.rel[i];
            let e = Extent {
                x0: o.x,
                y0: o.y,
                x1: o.x + s.size.w as i64,
                y1: o.y + s.size.h as i64,
            };
            out = Some(match out {
                None => e,
                Some(b) => Extent {
                    x0: b.x0.min(e.x0),
                    y0: b.y0.min(e.y0),
                    x1: b.x1.max(e.x1),
                    y1: b.y1.max(e.y1),
                },
            });
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn key(n: u64) -> SurfaceKey {
        SurfaceKey::from_raw(NonZeroU64::new(n).unwrap())
    }

    fn surf(k: u64, role: SurfaceRole, w: i32, h: i32) -> SnapshotSurface {
        SnapshotSurface {
            key: key(k),
            role,
            size: LocalSize { w, h },
            mapped: true,
            input: InputRegion::Infinite,
            frame_requested: false,
        }
    }

    fn pt(x: i32, y: i32) -> LocalPoint {
        LocalPoint { x, y }
    }

    /// The xsnap tree (TST-09): W with CSD shadow 20, S1 above, S2 below at a negative offset,
    /// popup P (shadow 10) at (100,50) in the parent geometry, PS inside P. Front-to-back.
    pub(crate) fn xsnap_tree() -> SurfaceTree {
        SurfaceTree {
            geometry: LocalRect {
                loc: pt(20, 20),
                size: LocalSize { w: 400, h: 300 },
            },
            surfaces: vec![
                surf(
                    5,
                    SurfaceRole::Subsurface {
                        parent: 1,
                        offset: pt(5, 5),
                    },
                    30,
                    30,
                ),
                surf(
                    4,
                    SurfaceRole::Popup {
                        position: pt(100, 50),
                        geometry_loc: pt(10, 10),
                    },
                    140,
                    100,
                ),
                surf(
                    2,
                    SurfaceRole::Subsurface {
                        parent: 3,
                        offset: pt(37, 11),
                    },
                    101,
                    53,
                ),
                surf(1, SurfaceRole::Root, 440, 340),
                surf(
                    3,
                    SurfaceRole::Subsurface {
                        parent: 3,
                        offset: pt(-15, 50),
                    },
                    60,
                    40,
                ),
            ],
        }
    }

    #[test]
    fn origin_function_matches_hand_derived_xsnap_truth() {
        let t = xsnap_tree();
        let anchor = CanvasPoint { x: 500, y: 400 };
        let o = t.origins(anchor).unwrap();
        // root origin = anchor − geometry.loc = (480,380); RootSurfaceLocal truth from xsnap:
        // PS (115,65), P (110,60), S1 (37,11), W (0,0), S2 (−15,50)
        let local: Vec<(i64, i64)> = o.iter().map(|p| (p.x - 480, p.y - 380)).collect();
        assert_eq!(
            local,
            vec![(115, 65), (110, 60), (37, 11), (0, 0), (-15, 50)]
        );
    }

    #[test]
    fn nested_subsurfaces_accumulate() {
        let t = SurfaceTree {
            geometry: LocalRect::default(),
            surfaces: vec![
                surf(
                    3,
                    SurfaceRole::Subsurface {
                        parent: 1,
                        offset: pt(10, 10),
                    },
                    30,
                    20,
                ),
                surf(
                    2,
                    SurfaceRole::Subsurface {
                        parent: 2,
                        offset: pt(37, 11),
                    },
                    101,
                    53,
                ),
                surf(1, SurfaceRole::Root, 100, 100),
            ],
        };
        let o = t.origins(CanvasPoint { x: 0, y: 0 }).unwrap();
        assert_eq!(o[0], CanvasPoint { x: 47, y: 21 });
    }

    #[test]
    fn malformed_trees_are_rejected() {
        let mut no_root = xsnap_tree();
        no_root.surfaces.retain(|s| s.role != SurfaceRole::Root);
        assert_eq!(no_root.validate(), Err(SnapshotError::RootCount(0)));

        let mut two_roots = xsnap_tree();
        two_roots.surfaces[0].role = SurfaceRole::Root;
        assert_eq!(two_roots.validate(), Err(SnapshotError::RootCount(2)));

        let mut out_of_range = xsnap_tree();
        out_of_range.surfaces[2].role = SurfaceRole::Subsurface {
            parent: 9,
            offset: pt(0, 0),
        };
        assert_eq!(
            out_of_range.validate(),
            Err(SnapshotError::BadParent { surface: 2 })
        );

        let mut cycle = xsnap_tree();
        cycle.surfaces[0].role = SurfaceRole::Subsurface {
            parent: 2,
            offset: pt(0, 0),
        };
        cycle.surfaces[2].role = SurfaceRole::Subsurface {
            parent: 0,
            offset: pt(0, 0),
        };
        assert!(matches!(cycle.validate(), Err(SnapshotError::Cycle { .. })));

        let mut dup = xsnap_tree();
        dup.surfaces[2].key = dup.surfaces[0].key;
        assert_eq!(
            dup.validate(),
            Err(SnapshotError::DuplicateKey { surface: 2 })
        );

        let mut neg = xsnap_tree();
        neg.surfaces[1].size.w = -1;
        assert_eq!(
            neg.validate(),
            Err(SnapshotError::NegativeSize { surface: 1 })
        );
    }

    #[test]
    fn children_of_an_unmapped_surface_are_not_shown() {
        let mut t = xsnap_tree();
        t.surfaces[1].mapped = false; // popup P without a buffer
        assert_eq!(t.shown(), vec![false, false, true, true, true]);
    }

    #[test]
    fn input_region_is_evaluated_in_request_order() {
        let r = |x, y, w, h| LocalRect {
            loc: pt(x, y),
            size: LocalSize { w, h },
        };
        let region = InputRegion::Ops(std::sync::Arc::from(vec![
            RegionOp {
                add: true,
                rect: r(0, 0, 10, 10),
            },
            RegionOp {
                add: false,
                rect: r(2, 2, 4, 4),
            },
            RegionOp {
                add: true,
                rect: r(3, 3, 1, 1),
            },
        ]));
        assert!(region.contains(0, 0));
        assert!(!region.contains(2, 2));
        assert!(region.contains(3, 3));
        assert!(!region.contains(5, 5));
        assert!(region.contains(6, 6));
        assert!(!region.contains(10, 0), "half-open");
        assert!(
            !InputRegion::Ops(std::sync::Arc::from(Vec::new())).contains(0, 0),
            "empty region"
        );
    }
}
