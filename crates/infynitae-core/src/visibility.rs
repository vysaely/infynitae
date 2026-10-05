//! Visibility pass: per-surface output membership (OBJ-13, F-MEMBER-1, INV-RENDER-3) and the frame
//! clock of observed elements (INV-FRAME-2).
//!
//! Membership is computed with the render path's own physical rects (`render::entries_for`): a
//! surface is on O exactly when some pixel of it is presented on O. The table is derived and
//! rewritten each pass; enter/leave are its diffs.

use crate::output::{OutputId, OutputModel};
use crate::projection::View;
use crate::protocol::Effect;
use crate::render::entries_for;
use crate::snapshot::SurfaceKey;
use crate::stage::{ElementId, Stage};

/// (element, surface, output) rows, sorted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MembershipTable {
    rows: Vec<(ElementId, SurfaceKey, OutputId)>,
}

impl MembershipTable {
    /// Whether the surface is on the output.
    pub fn contains(&self, element: ElementId, surface: SurfaceKey, output: OutputId) -> bool {
        self.rows.binary_search(&(element, surface, output)).is_ok()
    }

    /// Outputs observing an element, ascending.
    pub fn observers(&self, element: ElementId) -> Vec<OutputId> {
        let mut out: Vec<OutputId> = self
            .rows
            .iter()
            .filter(|(e, _, _)| *e == element)
            .map(|(_, _, o)| *o)
            .collect();
        out.dedup();
        out.sort();
        out.dedup();
        out
    }

    pub fn rows(&self) -> &[(ElementId, SurfaceKey, OutputId)] {
        &self.rows
    }

    /// F-MEMBER-1 over every mapped element and every output view.
    pub(crate) fn compute(stage: &Stage, views: &[(OutputId, View)]) -> MembershipTable {
        let mut rows = Vec::new();
        for (element, anchor, tree) in stage.front_to_back() {
            for (output, view) in views {
                for e in entries_for(view, element, anchor, tree) {
                    rows.push((element, e.surface, *output));
                }
            }
        }
        rows.sort();
        rows.dedup();
        MembershipTable { rows }
    }

    /// Enter/leave effects from `self` (old) to `new`, leaves first. A leave is emitted only while
    /// its element exists: a destroyed client gets no event (INV-STAGE-4).
    pub(crate) fn diff(&self, new: &MembershipTable, stage: &Stage) -> Vec<Effect> {
        let mut fx = Vec::new();
        for &(e, s, o) in &self.rows {
            if !new.contains(e, s, o) && stage.state(e).is_some() {
                fx.push(Effect::Leave {
                    output: o,
                    surface: s,
                });
            }
        }
        for &(e, s, o) in &new.rows {
            if !self.contains(e, s, o) {
                fx.push(Effect::Enter {
                    output: o,
                    surface: s,
                });
            }
        }
        fx
    }
}

/// INV-FRAME-2: the clock output of an element is its highest-refresh observing output; ties go
/// to the lowest `OutputId`. `None` when unobserved.
pub(crate) fn clock_output(observers: &[OutputId], outputs: &[OutputModel]) -> Option<OutputId> {
    let mut best: Option<&OutputModel> = None;
    for o in outputs.iter().filter(|o| observers.contains(&o.id)) {
        best = match best {
            Some(b)
                if b.refresh_mhz > o.refresh_mhz
                    || (b.refresh_mhz == o.refresh_mhz && b.id < o.id) =>
            {
                Some(b)
            }
            _ => Some(o),
        };
    }
    best.map(|o| o.id)
}

#[cfg(test)]
mod tests {
    use crate::Core;
    use crate::geom::{CanvasPos, LocalPoint, LocalRect, LocalSize};
    use crate::navigation::PersistedCameras;
    use crate::output::tests::key;
    use crate::output::{OutputDescriptor, OutputId, Transform};
    use crate::protocol::Effect;
    use crate::snapshot::tests::key as skey;
    use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, SurfaceTree};
    use crate::stage::ElementId;

    fn desc(c: &str, refresh_mhz: u32) -> OutputDescriptor {
        OutputDescriptor {
            key: key(c),
            mode: (1920, 1080),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz,
        }
    }

    fn tree(mapped: bool) -> SurfaceTree {
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w: 400, h: 300 },
            },
            surfaces: vec![SnapshotSurface {
                key: skey(1),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w: 400, h: 300 }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
            }],
        }
    }

    fn map(c: &mut Core) -> ElementId {
        let id = c.toplevel_created();
        c.toplevel_committed(id, tree(false)).unwrap();
        c.toplevel_committed(id, tree(true)).unwrap();
        id
    }

    /// Cameras at the origin (default) unless persisted; with `far`, C's camera is far away.
    fn persisted(far: bool) -> PersistedCameras {
        let entries = if far {
            vec![(
                key("C"),
                CanvasPos {
                    x: 9000 << 16,
                    y: 0,
                },
                1.0,
            )]
        } else {
            vec![]
        };
        PersistedCameras::from_entries(entries).unwrap()
    }

    /// INV-OUTPUT-1: every leave for the removed output precedes `OutputRemoved`; surfaces on other
    /// outputs are untouched.
    #[test]
    fn disconnect_emits_leaves_then_removal() {
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        let (a, _) = c.output_connected(&mut t, desc("A", 60_000)).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B", 60_000)).unwrap();
        map(&mut c);
        let fx = c.output_disconnected(&mut t, a).unwrap();
        assert_eq!(
            fx,
            vec![
                Effect::Leave {
                    output: a,
                    surface: skey(1)
                },
                Effect::OutputRemoved(a)
            ]
        );
        let id = c.stage().z_order()[0];
        assert_eq!(c.membership().observers(id), vec![b]);
    }

    /// A connect emits `OutputAdded` before the enters it causes.
    #[test]
    fn connect_emits_addition_then_enters() {
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        let (a, _) = c.output_connected(&mut t, desc("A", 60_000)).unwrap();
        map(&mut c);
        let (b, fx) = c.output_connected(&mut t, desc("B", 60_000)).unwrap();
        assert_eq!(
            fx,
            vec![
                Effect::OutputAdded(b),
                Effect::Enter {
                    output: b,
                    surface: skey(1)
                }
            ]
        );
        let _ = a;
    }

    /// INV-FRAME-2: the clock is the highest-refresh observer; equal refresh → lowest OutputId;
    /// `frame_tick` returns the element only for its clock output; an unobserved element is never
    /// returned (withheld; INC-8 owns kick and fallback).
    #[test]
    fn clock_selection_and_frame_tick() {
        let (mut c, _, _, mut t) = Core::new(persisted(true));
        let (a, _) = c.output_connected(&mut t, desc("A", 60_000)).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B", 60_000)).unwrap();
        let (far, _) = c.output_connected(&mut t, desc("C", 240_000)).unwrap();
        let id = map(&mut c);
        assert_eq!(
            c.clock_output_of(id),
            Some(a),
            "equal refresh: lowest OutputId"
        );
        assert_eq!(c.frame_tick(a), vec![id]);
        assert!(
            c.frame_tick(b).is_empty(),
            "an observer that is not the clock completes nothing"
        );
        assert!(
            c.frame_tick(far).is_empty(),
            "C (240 Hz) does not observe the element"
        );
        let (fast, _) = c.output_connected(&mut t, desc("D", 144_000)).unwrap();
        assert_eq!(
            c.clock_output_of(id),
            Some(fast),
            "highest refresh observer"
        );
        assert_eq!(c.frame_tick(fast), vec![id]);
        assert!(c.frame_tick(a).is_empty());
        for o in [a, b, fast] {
            c.output_disconnected(&mut t, o).unwrap();
        }
        assert_eq!(c.clock_output_of(id), None, "unobserved");
        assert!(c.frame_tick(far).is_empty());
        let _: OutputId = far;
    }

    /// An unmapped element has no membership and no clock.
    #[test]
    fn unmapped_elements_are_not_observed() {
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        let (a, _) = c.output_connected(&mut t, desc("A", 60_000)).unwrap();
        let id = map(&mut c);
        let fx = c.toplevel_committed(id, tree(false)).unwrap();
        assert_eq!(
            fx,
            vec![Effect::Leave {
                output: a,
                surface: skey(1)
            }]
        );
        assert_eq!(c.clock_output_of(id), None);
        assert!(c.frame_tick(a).is_empty());
    }
}
