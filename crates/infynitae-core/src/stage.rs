//! Stage (OBJ-01, OBJ-02): element identity, anchors, z-order and lifecycle state. The only store
//! and writer of placement and z-order (INV-STAGE-1). Writers are crate-private.

use std::num::NonZeroU64;

use crate::geom::CanvasPoint;
use crate::snapshot::SurfaceTree;

/// Element identity: non-zero, monotonic, never reused within a process (INV-STAGE-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(NonZeroU64);

impl ElementId {
    pub const fn new(id: NonZeroU64) -> Self {
        ElementId(id)
    }

    pub fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// Identity of this compositor instance, for external references.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstanceId(pub(crate) u64);

/// An element named from outside the process (IPC): resolves or fails with `StaleElement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElementRef {
    pub instance: InstanceId,
    pub element: ElementId,
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

#[derive(Debug)]
struct Record {
    id: ElementId,
    state: Lifecycle,
    anchor: Option<CanvasPoint>,
    tree: Option<SurfaceTree>,
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

    pub fn tree(&self, id: ElementId) -> Option<&SurfaceTree> {
        self.record(id).and_then(|r| r.tree.as_ref())
    }

    /// Z-order, bottom to top.
    pub fn z_order(&self) -> &[ElementId] {
        &self.z
    }

    /// Mapped elements front to back (reverse z-order) with anchor and snapshot.
    pub fn front_to_back(&self) -> impl Iterator<Item = (ElementId, CanvasPoint, &SurfaceTree)> {
        self.z.iter().rev().filter_map(|&id| {
            let r = self.record(id)?;
            match (r.state, r.anchor, r.tree.as_ref()) {
                (Lifecycle::Mapped, Some(a), Some(t)) => Some((id, a, t)),
                _ => None,
            }
        })
    }

    pub fn resolve(&self, r: ElementRef) -> Result<ElementId, StaleElement> {
        if r.instance == self.instance && self.record(r.element).is_some() {
            Ok(r.element)
        } else {
            Err(StaleElement)
        }
    }

    fn record(&self, id: ElementId) -> Option<&Record> {
        self.records.iter().find(|r| r.id == id)
    }

    fn record_mut(&mut self, id: ElementId) -> Option<&mut Record> {
        self.records.iter_mut().find(|r| r.id == id)
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
        self.record_mut(id).ok_or(StaleElement)?.state = state;
        Ok(())
    }

    /// Replaces the snapshot wholesale (OBJ-05), or drops it.
    pub(crate) fn set_tree(
        &mut self,
        id: ElementId,
        tree: Option<SurfaceTree>,
    ) -> Result<(), StaleElement> {
        self.record_mut(id).ok_or(StaleElement)?.tree = tree;
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
