//! Stage (OBJ-01, OBJ-02): element identity, anchors, z-order and lifecycle state. The only store
//! and writer of placement and z-order (INV-STAGE-1). Writers are crate-private.

use std::num::NonZeroU64;

use crate::geom::CanvasPoint;
use crate::snapshot::{Extent, SurfaceTree, TreeLayout};

/// Element identity: non-zero, monotonic, never reused within a process (INV-STAGE-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(NonZeroU64);

impl ElementId {
    #[allow(dead_code)]
    pub(crate) const fn new(id: NonZeroU64) -> Self {
        ElementId(id)
    }

    pub fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// Identity of this compositor instance, for external references.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstanceId(pub(crate) u64);

impl InstanceId {
    /// The raw value, as external references carry it.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// An element named from outside the process (IPC): resolves or fails with `StaleElement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElementRef {
    pub instance: InstanceId,
    pub element: ElementId,
}

impl ElementRef {
    /// A reference as an external client spelled it. It names nothing until [`Stage::resolve`]
    /// accepts it; `None` for element 0, which no element ever has.
    pub fn from_raw(instance: u64, element: u64) -> Option<ElementRef> {
        Some(ElementRef {
            instance: InstanceId(instance),
            element: ElementId(NonZeroU64::new(element)?),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaleElement;

/// M1 states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    AwaitingInitialCommit,
    AwaitingFirstBuffer,
    Mapped,
    Unmapped,
}

/// A mapped element as the projection reads it: anchor, snapshot, the snapshot's layout and its
/// Canvas bounding box (`None`: nothing of it can be shown).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed<'a> {
    pub(crate) id: ElementId,
    pub(crate) anchor: CanvasPoint,
    pub(crate) tree: &'a SurfaceTree,
    pub(crate) layout: &'a TreeLayout,
    pub(crate) bounds: Option<CanvasBox>,
}

#[derive(Debug)]
struct Record {
    id: ElementId,
    state: Lifecycle,
    anchor: Option<CanvasPoint>,
    tree: Option<SurfaceTree>,
    /// `tree.extent()`, written with the tree in `set_tree` (its only writer).
    extent: Option<Extent>,
    /// The tree's origins and shown flags (F-ORIGIN-1), rewritten in place by `set_tree`. Out of
    /// line: the cull walks every record, and reads a layout only for an element in view.
    layout: Box<TreeLayout>,
    /// The element has been Mapped at least once (written by `set_state`): its placement is
    /// final and a remap keeps it (INV-STAGE-5).
    mapped_once: bool,
    /// The default windowed placement has been resolved for this element (written by
    /// `set_default_placed`): at its first windowed presentation, which is its first map
    /// unless it was in protocol fullscreen then. From then on only a drag or the shell moves it.
    default_placed: bool,
    /// The toplevel's title and app_id as the client last set them (INC-11, D9): display
    /// metadata for the shell, never authority. Out of line, like `layout`; `None` until set.
    meta: Option<Box<ToplevelMeta>>,
}

/// What a toplevel calls itself (`xdg_toplevel.set_title` / `set_app_id`): derived protocol
/// metadata the shell reads (INC-11). Nothing in Core decides anything by it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToplevelMeta {
    pub title: Option<String>,
    pub app_id: Option<String>,
}

/// An element's bounding box in Canvas pixels: `[x0, x1) × [y0, y1)`.
pub(crate) type CanvasBox = (i64, i64, i64, i64);

fn at(anchor: CanvasPoint, e: Extent) -> CanvasBox {
    (
        anchor.x + e.x0,
        anchor.y + e.y0,
        anchor.x + e.x1,
        anchor.y + e.y1,
    )
}

#[derive(Debug)]
pub struct Stage {
    instance: InstanceId,
    last_id: u64,
    records: Vec<Record>,
    /// Bottom to top. Contains every element that has been mapped and not destroyed.
    z: Vec<ElementId>,
}

impl Stage {
    pub(crate) fn new(instance: InstanceId) -> Self {
        Stage {
            instance,
            last_id: 0,
            records: Vec::new(),
            z: Vec::new(),
        }
    }

    pub fn instance(&self) -> InstanceId {
        self.instance
    }

    // ------------------------------------------------------------ readers

    pub fn state(&self, id: ElementId) -> Option<Lifecycle> {
        self.record(id).map(|r| r.state)
    }

    /// The anchor: the Canvas position of the window-geometry origin (INV-STAGE-2).
    pub fn anchor(&self, id: ElementId) -> Option<CanvasPoint> {
        self.record(id).and_then(|r| r.anchor)
    }

    /// Whether the element has been Mapped before (now or earlier).
    pub fn mapped_before(&self, id: ElementId) -> bool {
        self.record(id).is_some_and(|r| r.mapped_once)
    }

    /// Whether the element's default windowed placement has been resolved.
    pub fn default_placed(&self, id: ElementId) -> bool {
        self.record(id).is_some_and(|r| r.default_placed)
    }

    /// The toplevel's title and app_id (INC-11); `None` for an unknown element or before the client
    /// set either.
    pub fn meta(&self, id: ElementId) -> Option<&ToplevelMeta> {
        self.record(id).and_then(|r| r.meta.as_deref())
    }

    /// Replaces the title and app_id wholesale (the adapter reads both from the toplevel's state).
    pub(crate) fn set_meta(
        &mut self,
        id: ElementId,
        meta: ToplevelMeta,
    ) -> Result<(), StaleElement> {
        self.record_mut(id).ok_or(StaleElement)?.meta = Some(Box::new(meta));
        Ok(())
    }

    pub(crate) fn set_default_placed(&mut self, id: ElementId) -> Result<(), StaleElement> {
        self.record_mut(id).ok_or(StaleElement)?.default_placed = true;
        Ok(())
    }

    pub fn tree(&self, id: ElementId) -> Option<&SurfaceTree> {
        self.record(id).and_then(|r| r.tree.as_ref())
    }

    /// Z-order, bottom to top.
    pub fn z_order(&self) -> &[ElementId] {
        &self.z
    }

    /// Returns all element IDs currently in the mapped lifecycle state (OBJ-22, M11).
    pub fn mapped_elements(&self) -> Vec<ElementId> {
        self.z
            .iter()
            .copied()
            .filter(|&id| self.state(id) == Some(Lifecycle::Mapped))
            .collect()
    }

    /// Finds the mapped element that contains `surface` in its surface tree.
    pub fn element_containing_surface(
        &self,
        surface: crate::snapshot::SurfaceKey,
    ) -> Option<ElementId> {
        self.records.iter().find_map(|r| {
            if r.state == Lifecycle::Mapped
                && r.tree
                    .as_ref()
                    .is_some_and(|t| t.surfaces.iter().any(|s| s.key == surface))
            {
                Some(r.id)
            } else {
                None
            }
        })
    }

    /// Mapped elements front to back (reverse z-order) with anchor and snapshot.
    pub fn front_to_back(&self) -> impl Iterator<Item = (ElementId, CanvasPoint, &SurfaceTree)> {
        self.front_to_back_boxed().map(|(id, a, t, _)| (id, a, t))
    }

    /// One mapped element's anchor, snapshot and Canvas bounding box (`None` box: nothing of it
    /// can be shown).
    pub(crate) fn mapped(
        &self,
        id: ElementId,
    ) -> Option<(CanvasPoint, &SurfaceTree, Option<CanvasBox>)> {
        let r = self.record(id)?;
        match (r.state, r.anchor, r.tree.as_ref()) {
            (Lifecycle::Mapped, Some(a), Some(t)) => Some((a, t, r.extent.map(|e| at(a, e)))),
            _ => None,
        }
    }

    /// One mapped element as the projection reads it ([`Placed`]).
    pub(crate) fn placed(&self, id: ElementId) -> Option<Placed<'_>> {
        Self::placed_record(self.record(id)?)
    }

    /// Every mapped element as the projection reads it, ascending id (no z-order meaning).
    pub(crate) fn placed_all(&self) -> impl Iterator<Item = Placed<'_>> {
        self.records.iter().filter_map(Self::placed_record)
    }

    /// Every mapped element as the projection reads it, front to back (INV-RENDER-2).
    pub(crate) fn placed_front_to_back(&self) -> impl Iterator<Item = Placed<'_>> {
        self.z.iter().rev().filter_map(|&id| self.placed(id))
    }

    fn placed_record(r: &Record) -> Option<Placed<'_>> {
        match (r.state, r.anchor, r.tree.as_ref()) {
            (Lifecycle::Mapped, Some(anchor), Some(tree)) => Some(Placed {
                id: r.id,
                anchor,
                tree,
                layout: &r.layout,
                bounds: r.extent.map(|e| at(anchor, e)),
            }),
            _ => None,
        }
    }

    /// The z-order walk (INV-RENDER-2): mapped elements front to back, with each element's
    /// Canvas bounding box. The render list is built from this.
    pub(crate) fn front_to_back_boxed(
        &self,
    ) -> impl Iterator<Item = (ElementId, CanvasPoint, &SurfaceTree, Option<CanvasBox>)> {
        self.z.iter().rev().filter_map(|&id| {
            let (a, t, b) = self.mapped(id)?;
            Some((id, a, t, b))
        })
    }

    pub fn resolve(&self, r: ElementRef) -> Result<ElementId, StaleElement> {
        if r.instance == self.instance && self.record(r.element).is_some() {
            Ok(r.element)
        } else {
            Err(StaleElement)
        }
    }

    /// `records` is ascending by id: `create` appends a strictly larger id and `destroy` keeps
    /// the order, so a lookup is a binary search.
    fn index(&self, id: ElementId) -> Option<usize> {
        self.records.binary_search_by_key(&id, |r| r.id).ok()
    }

    fn record(&self, id: ElementId) -> Option<&Record> {
        self.index(id).map(|i| &self.records[i])
    }

    fn record_mut(&mut self, id: ElementId) -> Option<&mut Record> {
        self.index(id).map(|i| &mut self.records[i])
    }

    // ------------------------------------------------------------ writers (crate-private)

    /// Mints a new identity (M1: `new_toplevel`).
    pub(crate) fn create(&mut self) -> ElementId {
        self.last_id += 1;
        let id = ElementId(NonZeroU64::new(self.last_id).expect("ids start at 1 and only grow"));
        self.records.push(Record {
            id,
            state: Lifecycle::AwaitingInitialCommit,
            anchor: None,
            tree: None,
            extent: None,
            layout: Box::default(),
            mapped_once: false,
            default_placed: false,
            meta: None,
        });
        id
    }

    /// `stage::place`: the single writer of anchors (INV-STAGE-1). The point is already integral
    /// and within bounds by construction of [`CanvasPoint`].
    pub(crate) fn place(&mut self, id: ElementId, anchor: CanvasPoint) -> Result<(), StaleElement> {
        self.record_mut(id).ok_or(StaleElement)?.anchor = Some(anchor);
        Ok(())
    }

    pub(crate) fn set_state(
        &mut self,
        id: ElementId,
        state: Lifecycle,
    ) -> Result<(), StaleElement> {
        let r = self.record_mut(id).ok_or(StaleElement)?;
        r.state = state;
        r.mapped_once |= state == Lifecycle::Mapped;
        Ok(())
    }

    /// Replaces the snapshot wholesale (OBJ-05), or drops it.
    pub(crate) fn set_tree(
        &mut self,
        id: ElementId,
        tree: Option<SurfaceTree>,
    ) -> Result<(), StaleElement> {
        let r = self.record_mut(id).ok_or(StaleElement)?;
        r.layout.compute(tree.as_ref());
        r.extent = tree.as_ref().and_then(|t| r.layout.extent(t));
        r.tree = tree;
        Ok(())
    }

    /// Puts the element on top of the z-order (map and remap, INV-STAGE-5).
    pub(crate) fn raise(&mut self, id: ElementId) {
        self.z.retain(|&e| e != id);
        self.z.push(id);
    }

    /// Destroy purges every reference; idempotent (INV-STAGE-4). Returns whether it existed.
    pub(crate) fn destroy(&mut self, id: ElementId) -> bool {
        self.z.retain(|&e| e != id);
        let before = self.records.len();
        self.records.retain(|r| r.id != id);
        before != self.records.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_nonzero_monotonic_and_never_reused() {
        let mut s = Stage::new(InstanceId(1));
        let a = s.create();
        let b = s.create();
        assert_eq!(a.get().get(), 1);
        assert!(b > a);
        assert!(s.destroy(a));
        let c = s.create();
        assert!(c > b, "an id was reused after destroy");
    }

    #[test]
    fn destroy_is_idempotent_and_external_refs_go_stale() {
        let mut s = Stage::new(InstanceId(7));
        let a = s.create();
        let r = ElementRef {
            instance: InstanceId(7),
            element: a,
        };
        assert_eq!(s.resolve(r), Ok(a));
        assert_eq!(
            s.resolve(ElementRef {
                instance: InstanceId(8),
                element: a
            }),
            Err(StaleElement),
            "a reference from another instance must not alias"
        );
        assert!(s.destroy(a));
        assert!(!s.destroy(a));
        assert_eq!(s.resolve(r), Err(StaleElement));
        assert_eq!(s.place(a, CanvasPoint { x: 0, y: 0 }), Err(StaleElement));
    }

    /// The lookup is a binary search over `records`; it must agree with a linear scan through
    /// creates and destroys in any order.
    #[test]
    fn lookup_agrees_with_a_linear_scan_under_churn() {
        let mut s = Stage::new(InstanceId(1));
        let mut live: Vec<ElementId> = Vec::new();
        let mut dead: Vec<ElementId> = Vec::new();
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..2000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            if live.is_empty() || !x.is_multiple_of(3) {
                live.push(s.create());
            } else {
                let id = live.remove((x >> 8) as usize % live.len());
                assert!(s.destroy(id));
                dead.push(id);
            }
            assert!(s.records.windows(2).all(|w| w[0].id < w[1].id));
            for &id in &live {
                let linear = s.records.iter().position(|r| r.id == id);
                assert!(linear.is_some());
                assert_eq!(s.index(id), linear);
            }
            for &id in &dead {
                assert_eq!(s.index(id), None);
                assert_eq!(s.state(id), None);
            }
        }
    }

    #[test]
    fn raise_puts_on_top_once() {
        let mut s = Stage::new(InstanceId(1));
        let (a, b) = (s.create(), s.create());
        s.raise(a);
        s.raise(b);
        s.raise(a);
        assert_eq!(s.z_order(), &[b, a]);
    }
}
