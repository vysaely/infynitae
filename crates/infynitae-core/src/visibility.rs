//! Visibility pass: per-surface output membership (OBJ-13, F-MEMBER-1, INV-RENDER-3) and the frame
//! clock of observed elements (INV-FRAME-2).
//!
//! Membership is computed with the render path's own physical rects (`render::entries_for`): a
//! surface is on O exactly when some pixel of it is presented on O. The table is derived;
//! enter/leave are its diffs.
//!
//! A row (E, s, O) depends on E's anchor and snapshot and on O's view, and on nothing else. So a
//! change of one element rewrites that element's rows ([`MembershipTable::replace_element`]), a
//! change of one view rewrites that output's rows ([`MembershipTable::with_output`]), and only
//! output lifecycle recomputes the table ([`MembershipTable::compute`]).

use crate::output::{OutputId, OutputModel};
use crate::projection::View;
use crate::protocol::Effect;
use crate::render::entries_culled;
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

    /// The rows of one element: contiguous, because the table is sorted by element first.
    fn range(&self, element: ElementId) -> std::ops::Range<usize> {
        let start = self.rows.partition_point(|(e, _, _)| *e < element);
        let len = self.rows[start..].partition_point(|(e, _, _)| *e == element);
        start..start + len
    }

    /// Outputs observing an element, ascending.
    pub fn observers(&self, element: ElementId) -> Vec<OutputId> {
        let mut out: Vec<OutputId> = self.rows[self.range(element)]
            .iter()
            .map(|(_, _, o)| *o)
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The elements that have at least one row, ascending.
    pub(crate) fn observed(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.rows.chunk_by(|a, b| a.0 == b.0).map(|rows| rows[0].0)
    }

    pub fn rows(&self) -> &[(ElementId, SurfaceKey, OutputId)] {
        &self.rows
    }

    /// F-MEMBER-1 over every mapped element and every output view.
    pub(crate) fn compute(stage: &Stage, views: &[(OutputId, View)]) -> MembershipTable {
        let mut rows = Vec::new();
        for (element, anchor, tree, bounds) in stage.mapped_boxed() {
            for (output, view) in views {
                for e in entries_culled(view, element, anchor, tree, bounds) {
                    rows.push((element, e.surface, *output));
                }
            }
        }
        rows.sort();
        rows.dedup();
        MembershipTable { rows }
    }

    /// [`MembershipTable::compute`] as it was before any cull or scoping: every surface of every
    /// mapped element against every view. The oracle of the differential tests.
    #[cfg(test)]
    pub(crate) fn compute_reference(stage: &Stage, views: &[(OutputId, View)]) -> MembershipTable {
        let mut rows = Vec::new();
        for (element, anchor, tree) in stage.front_to_back() {
            for (output, view) in views {
                for e in crate::render::entries_for(view, element, anchor, tree) {
                    rows.push((element, e.surface, *output));
                }
            }
        }
        rows.sort();
        rows.dedup();
        MembershipTable { rows }
    }

    /// Rewrites the rows of one element after its anchor, snapshot or lifecycle changed, and
    /// returns its leaves then its enters: what [`MembershipTable::diff`] returns when only this
    /// element's rows differ. An element that is not mapped has no rows.
    pub(crate) fn replace_element(
        &mut self,
        element: ElementId,
        stage: &Stage,
        views: &[(OutputId, View)],
    ) -> Vec<Effect> {
        let mut new = Vec::new();
        if let Some((anchor, tree, bounds)) = stage.mapped(element) {
            for (output, view) in views {
                for e in entries_culled(view, element, anchor, tree, bounds) {
                    new.push((element, e.surface, *output));
                }
            }
        }
        new.sort();
        new.dedup();
        let range = self.range(element);
        let old = &self.rows[range.clone()];
        let mut fx = Vec::new();
        if stage.state(element).is_some() {
            for &(_, s, o) in old.iter().filter(|r| new.binary_search(r).is_err()) {
                fx.push(Effect::Leave {
                    output: o,
                    surface: s,
                });
            }
        }
        for &(_, s, o) in new.iter().filter(|r| old.binary_search(r).is_err()) {
            fx.push(Effect::Enter {
                output: o,
                surface: s,
            });
        }
        self.rows.splice(range, new);
        fx
    }

    /// The table after the view of one output changed: the rows of every other output are kept
    /// and this output's rows are recomputed.
    pub(crate) fn with_output(
        &self,
        output: OutputId,
        view: Option<&View>,
        stage: &Stage,
    ) -> MembershipTable {
        let mut rows: Vec<_> = self
            .rows
            .iter()
            .copied()
            .filter(|(_, _, o)| *o != output)
            .collect();
        if let Some(view) = view {
            for (element, anchor, tree, bounds) in stage.mapped_boxed() {
                for e in entries_culled(view, element, anchor, tree, bounds) {
                    rows.push((element, e.surface, output));
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
                // the pointer (960, 540) on A relocates to (0, 540) on B (F-RELOCATE-1); B's
                // c_eff is (−960, −540), so Canvas (−960, 0): outside W [0, 400) × [0, 300)
                Effect::PointerMotion {
                    target: None,
                    location: CanvasPos {
                        x: -960 << 16,
                        y: 0
                    },
                    time_ms: 0
                },
                Effect::PointerFrame,
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

    /// The pass that ends `toplevel_destroyed` evaluates the scale policy of every mapped
    /// element (INV-SCALE-5: once per core batch), so it may return a `PreferredScale` for an
    /// element other than the destroyed one. What a destroy never produces is an enter or a
    /// leave (INV-STAGE-4).
    ///
    /// The six calls: Y is mapped on a 2.0x output (hint 240/120); it grows to the device bound
    /// (P7 lowers the hint to 120/120, direction down); it shrinks again, and SC-E holds the
    /// client-caused increase; then an element that was never mapped is created and destroyed.
    /// While a destroy was evaluated as a non-client stimulus, Y's held increase was applied in
    /// its pass, and the old assertion panicked on exactly that effect. A destroy is now
    /// evaluated as client-caused (see `destroying_an_element_is_not_a_scale_stimulus`), so in
    /// these six calls the hint stays held; the shape asserted below holds either way.
    #[test]
    fn destroying_an_element_may_change_the_scale_hint_of_another() {
        let wide = |w: i32, mapped: bool| SurfaceTree {
            surfaces: vec![SnapshotSurface {
                size: if mapped {
                    LocalSize { w, h: 300 }
                } else {
                    LocalSize::default()
                },
                ..tree(mapped).surfaces.remove(0)
            }],
            ..tree(mapped)
        };
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        let d = OutputDescriptor {
            scale_120: 240,
            ..desc("A", 60_000)
        };
        c.output_connected(&mut t, d).unwrap();
        let y = c.toplevel_created();
        c.toplevel_committed(y, wide(400, false)).unwrap();
        c.toplevel_committed(y, wide(400, true)).unwrap();
        assert_eq!(c.preferred_scale_of(y), Some((240, 2)));
        c.toplevel_committed(y, wide(16384, true)).unwrap();
        assert_eq!(c.preferred_scale_of(y), Some((120, 1)), "P7 at the bound");
        let fx = c.toplevel_committed(y, wide(400, true)).unwrap();
        assert!(fx.is_empty(), "SC-E holds a client-caused increase: {fx:?}");
        assert_eq!(c.preferred_scale_of(y), Some((120, 1)));
        let x = c.toplevel_created();
        // What this test fixes is the shape of a destroy's effects, not the scale policy: no
        // enter or leave, and nothing but a hint for the surviving element. Whether X's
        // destruction should release Y's held hint at all is an open policy question (SC-E
        // does not list it among the stimuli), so an empty result is accepted as well.
        let fx = c.toplevel_destroyed(x);
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Enter { .. } | Effect::Leave { .. })),
            "a destroy produced an enter or a leave: {fx:?}"
        );
        assert!(
            fx.iter()
                .all(|e| matches!(e, Effect::PreferredScale { element, .. } if *element == y)),
            "a destroy produced something other than a hint for the survivor: {fx:?}"
        );
        assert!(
            [Some((120, 1)), Some((240, 2))].contains(&c.preferred_scale_of(y)),
            "Y's hint is neither held nor released: {:?}",
            c.preferred_scale_of(y)
        );
    }

    /// SC-E (C14.4, INV-SCALE-4 d): a hint held after a P7 decrease is reversed only by a
    /// non-client stimulus. Destroying another element is not one, whether that element was
    /// mapped or not, so one client cannot make the hint oscillate by pairing a resize with a
    /// throwaway toplevel. An output change is a stimulus and does release the hold.
    #[test]
    fn destroying_an_element_is_not_a_scale_stimulus() {
        let wide = |w: i32, mapped: bool| SurfaceTree {
            surfaces: vec![SnapshotSurface {
                size: if mapped {
                    LocalSize { w, h: 300 }
                } else {
                    LocalSize::default()
                },
                ..tree(mapped).surfaces.remove(0)
            }],
            ..tree(mapped)
        };
        let other = |mapped: bool| SurfaceTree {
            surfaces: vec![SnapshotSurface {
                key: skey(2),
                ..tree(mapped).surfaces.remove(0)
            }],
            ..tree(mapped)
        };
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        let d = || OutputDescriptor {
            scale_120: 240,
            ..desc("A", 60_000)
        };
        let (o, _) = c.output_connected(&mut t, d()).unwrap();
        let y = c.toplevel_created();
        c.toplevel_committed(y, wide(400, false)).unwrap();
        c.toplevel_committed(y, wide(400, true)).unwrap();
        assert_eq!(c.preferred_scale_of(y), Some((240, 2)));
        let hints_of_y = |fx: &[Effect]| {
            fx.iter()
                .filter(|e| matches!(e, Effect::PreferredScale { element, .. } if *element == y))
                .count()
        };
        let mut changes = 0;
        for cycle in 0..50 {
            changes += hints_of_y(&c.toplevel_committed(y, wide(16384, true)).unwrap());
            changes += hints_of_y(&c.toplevel_committed(y, wide(400, true)).unwrap());
            let x = c.toplevel_created();
            if cycle % 2 == 1 {
                // every other throwaway element is mapped before it is destroyed
                c.toplevel_committed(x, other(false)).unwrap();
                changes += hints_of_y(&c.toplevel_committed(x, other(true)).unwrap());
            }
            let fx = c.toplevel_destroyed(x);
            assert!(
                !fx.iter()
                    .any(|e| matches!(e, Effect::Enter { .. } | Effect::Leave { .. })),
                "cycle {cycle}: a destroy produced an enter or a leave: {fx:?}"
            );
            assert_eq!(
                hints_of_y(&fx),
                0,
                "cycle {cycle}: destroying X re-hinted Y: {fx:?}"
            );
            changes += hints_of_y(&fx);
        }
        assert_eq!(changes, 1, "one P7 decrease, then the hint is held");
        assert_eq!(c.preferred_scale_of(y), Some((120, 1)));
        // a listed stimulus (output configuration change) releases the hold, once
        let fx = c.output_reconfigured(&mut t, o, d()).unwrap();
        assert_eq!(hints_of_y(&fx), 1, "an output change is a stimulus: {fx:?}");
        assert_eq!(c.preferred_scale_of(y), Some((240, 2)));
    }

    /// The settlement deadline a backend's loop sleeps on (INV-SCALE-3, INV-IDLE-1): none while
    /// no discrete epoch is open, one while it is, unchanged by a tick before it is due, and
    /// consumed by the tick that settles it — a loop that sleeps until the deadline and then
    /// ticks does not spin on a deadline that stays due. A continuous epoch has no deadline.
    #[test]
    fn a_due_settlement_deadline_is_consumed_by_the_tick() {
        use crate::scale::{DEFAULT_T_SETTLE_MS, InteractionKind};
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        c.output_connected(&mut t, desc("A", 60_000)).unwrap();
        assert_eq!(c.next_settlement_deadline(), None, "no element, no epoch");
        let id = map(&mut c);
        assert_eq!(c.next_settlement_deadline(), None, "no epoch open");
        c.start_scale_interaction(id, InteractionKind::CameraZoom, false, 1_000);
        let deadline = 1_000 + DEFAULT_T_SETTLE_MS;
        assert_eq!(c.next_settlement_deadline(), Some(deadline));
        c.scale_tick(deadline - 1);
        assert_eq!(c.next_settlement_deadline(), Some(deadline), "not due yet");
        c.scale_tick(deadline);
        assert_eq!(
            c.next_settlement_deadline(),
            None,
            "settled: nothing left to wake for"
        );
        c.start_scale_interaction(id, InteractionKind::CameraPan, true, 5_000);
        assert_eq!(
            c.next_settlement_deadline(),
            None,
            "a continuous epoch has no deadline"
        );
    }

    // ------------------------------------------------------------ differential tests

    use crate::input::bindings::Modifiers;
    use crate::render::ProjectionFrame;
    use crate::snapshot::SurfaceKey;
    use crate::visibility::MembershipTable;
    use crate::{InputPort, OutputLifecycleToken, PhysicalEvent};

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            (self.next() >> 11) % n
        }
        fn range(&mut self, lo: i64, hi: i64) -> i64 {
            lo + self.below((hi - lo) as u64) as i64
        }
    }

    /// A valid random tree with keys `base..`: a root, subsurfaces (some nested, some without a
    /// buffer, some of zero size, some far outside the root) and sometimes a popup with its own
    /// subsurface.
    fn random_tree(rng: &mut Rng, base: u64, mapped: bool) -> SurfaceTree {
        let size = |rng: &mut Rng| LocalSize {
            w: rng.range(0, 900) as i32,
            h: rng.range(0, 700) as i32,
        };
        let offset = |rng: &mut Rng| LocalPoint {
            x: rng.range(-1500, 1500) as i32,
            y: rng.range(-1500, 1500) as i32,
        };
        let mut surfaces = vec![SnapshotSurface {
            key: skey(base),
            role: SurfaceRole::Root,
            size: if mapped {
                LocalSize {
                    w: rng.range(1, 900) as i32,
                    h: rng.range(1, 700) as i32,
                }
            } else {
                LocalSize::default()
            },
            mapped,
            input: InputRegion::Infinite,
        }];
        if mapped {
            for i in 0..rng.below(4) {
                let parent = rng.below(surfaces.len() as u64) as usize;
                surfaces.push(SnapshotSurface {
                    key: skey(base + 1 + i),
                    role: SurfaceRole::Subsurface {
                        parent,
                        offset: offset(rng),
                    },
                    size: size(rng),
                    mapped: rng.below(5) != 0,
                    input: InputRegion::Infinite,
                });
            }
            if rng.below(3) == 0 {
                let popup = surfaces.len();
                surfaces.push(SnapshotSurface {
                    key: skey(base + 6),
                    role: SurfaceRole::Popup {
                        position: offset(rng),
                        geometry_loc: LocalPoint {
                            x: rng.range(0, 20) as i32,
                            y: rng.range(0, 20) as i32,
                        },
                    },
                    size: size(rng),
                    mapped: true,
                    input: InputRegion::Infinite,
                });
                surfaces.push(SnapshotSurface {
                    key: skey(base + 7),
                    role: SurfaceRole::Subsurface {
                        parent: popup,
                        offset: offset(rng),
                    },
                    size: size(rng),
                    mapped: true,
                    input: InputRegion::Infinite,
                });
            }
        }
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint {
                    x: rng.range(0, 30) as i32,
                    y: rng.range(0, 30) as i32,
                },
                size: LocalSize { w: 400, h: 300 },
            },
            surfaces,
        }
    }

    fn random_desc(rng: &mut Rng, connector: &str) -> OutputDescriptor {
        OutputDescriptor {
            key: key(connector),
            mode: [(1920, 1080), (2560, 1440), (3840, 2160), (800, 600)][rng.below(4) as usize],
            transform: [Transform::Normal, Transform::R90][rng.below(2) as usize],
            scale_120: [120, 150, 180, 240, 90][rng.below(5) as usize],
            refresh_mhz: [60_000, 144_000, 165_000][rng.below(3) as usize],
        }
    }

    /// Everything the scoped passes and the bounding-box cull derive, against the unscoped,
    /// unculled reference computed from the same Stage, cameras and outputs.
    fn check(c: &mut Core, before: &MembershipTable, fx: &[Effect], what: &str) {
        let views = c.views();
        let reference = MembershipTable::compute_reference(c.stage(), &views);
        assert_eq!(c.membership(), &reference, "membership table after {what}");
        let expected = before.diff(&reference, c.stage());
        let got: Vec<Effect> = fx
            .iter()
            .filter(|e| matches!(e, Effect::Enter { .. } | Effect::Leave { .. }))
            .cloned()
            .collect();
        assert_eq!(got, expected, "enter/leave effects of {what}");
        for (o, view) in views {
            let reference_frame = ProjectionFrame::build_reference(view, c.stage());
            assert_eq!(
                c.render_list(o).unwrap(),
                &reference_frame,
                "render list after {what}"
            );
            let mut tick: Vec<ElementId> = c
                .stage()
                .z_order()
                .iter()
                .copied()
                .filter(|&e| c.clock_output_of(e) == Some(o))
                .collect();
            tick.sort();
            assert_eq!(c.frame_tick(o), tick, "frame tick after {what}");
        }
        for &e in c.stage().z_order() {
            let mut observers: Vec<OutputId> = reference
                .rows()
                .iter()
                .filter(|r| r.0 == e)
                .map(|r| r.2)
                .collect();
            observers.sort();
            observers.dedup();
            assert_eq!(
                c.membership().observers(e),
                observers,
                "observers after {what}"
            );
        }
    }

    const LOGO: Modifiers = Modifiers {
        logo: true,
        ctrl: false,
        alt: false,
        shift: false,
    };

    fn pan(
        c: &mut Core,
        port: &mut InputPort,
        rng: &mut Rng,
        before: &mut MembershipTable,
        steps: u64,
    ) {
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: LOGO,
            time_us: 0,
        };
        let fx = c.physical_input(port, button(true)).unwrap();
        check(c, before, &fx, "pan press");
        for _ in 0..steps {
            // mostly small steps (elements cross the view edge one at a time), sometimes a jump
            let d = if rng.below(6) == 0 { 6000 } else { 90 };
            let delta = (rng.range(-d, d) as f64 + 0.25, rng.range(-d, d) as f64);
            *before = c.membership().clone();
            let fx = c
                .physical_input(
                    port,
                    PhysicalEvent::PointerMotion {
                        delta,
                        delta_unaccel: delta,
                        time_us: 0,
                    },
                )
                .unwrap();
            check(c, before, &fx, "pan motion");
        }
        *before = c.membership().clone();
        let fx = c.physical_input(port, button(false)).unwrap();
        check(c, before, &fx, "pan release");
    }

    fn run(seed: u64, ops: usize) -> (usize, usize) {
        let mut rng = Rng(seed);
        let (mut c, mut port, _shell, mut t): (Core, InputPort, _, OutputLifecycleToken) =
            Core::new(persisted(false));
        let mut outputs: Vec<(OutputId, String)> = Vec::new();
        // (element, key base, mapped)
        let mut elements: Vec<(ElementId, u64, bool)> = Vec::new();
        let mut next_key = 1u64;
        let mut next_connector = 0u32;
        let (mut changes, mut nonempty) = (0, 0);
        for _ in 0..ops {
            let before = c.membership().clone();
            let mut b = before.clone();
            match rng.below(100) {
                0..=3 if outputs.len() < 3 => {
                    let name = format!("X{next_connector}");
                    next_connector += 1;
                    let (o, fx) = c
                        .output_connected(&mut t, random_desc(&mut rng, &name))
                        .unwrap();
                    outputs.push((o, name));
                    check(&mut c, &before, &fx, "output connect");
                }
                4..=5 if !outputs.is_empty() => {
                    let (o, _) = outputs.remove(rng.below(outputs.len() as u64) as usize);
                    let fx = c.output_disconnected(&mut t, o).unwrap();
                    check(&mut c, &before, &fx, "output disconnect");
                }
                6..=9 if !outputs.is_empty() => {
                    let (o, name) = outputs[rng.below(outputs.len() as u64) as usize].clone();
                    let fx = c
                        .output_reconfigured(&mut t, o, random_desc(&mut rng, &name))
                        .unwrap();
                    check(&mut c, &before, &fx, "output reconfigure");
                }
                10..=29 => {
                    let id = c.toplevel_created();
                    let base = next_key;
                    next_key += 8;
                    let fx = c
                        .toplevel_committed(id, random_tree(&mut rng, base, false))
                        .unwrap();
                    check(&mut c, &before, &fx, "initial commit");
                    let before = c.membership().clone();
                    let fx = c
                        .toplevel_committed(id, random_tree(&mut rng, base, true))
                        .unwrap();
                    check(&mut c, &before, &fx, "map");
                    elements.push((id, base, true));
                }
                30..=59 if !elements.is_empty() => {
                    let i = rng.below(elements.len() as u64) as usize;
                    let (id, base, mapped) = elements[i];
                    if mapped {
                        let tree = random_tree(&mut rng, base, true);
                        let fx = if rng.below(2) == 0 {
                            c.toplevel_committed(id, tree).unwrap()
                        } else {
                            c.toplevel_tree_changed(id, tree).unwrap()
                        };
                        check(&mut c, &before, &fx, "commit of a mapped element");
                    } else {
                        // remap: initial commit, then a buffer
                        let fx = c
                            .toplevel_committed(id, random_tree(&mut rng, base, false))
                            .unwrap();
                        check(&mut c, &before, &fx, "initial commit of a remap");
                        let before = c.membership().clone();
                        let fx = c
                            .toplevel_committed(id, random_tree(&mut rng, base, true))
                            .unwrap();
                        check(&mut c, &before, &fx, "remap");
                        elements[i].2 = true;
                    }
                }
                60..=67 if !elements.is_empty() => {
                    let i = rng.below(elements.len() as u64) as usize;
                    let (id, base, mapped) = elements[i];
                    if mapped {
                        let fx = c
                            .toplevel_committed(id, random_tree(&mut rng, base, false))
                            .unwrap();
                        check(&mut c, &before, &fx, "unmap");
                        elements[i].2 = false;
                    }
                }
                68..=75 if !elements.is_empty() => {
                    let (id, _, _) = elements.remove(rng.below(elements.len() as u64) as usize);
                    let fx = c.toplevel_destroyed(id);
                    check(&mut c, &before, &fx, "destroy");
                    assert!(
                        !c.membership().rows().iter().any(|r| r.0 == id),
                        "a destroyed element kept membership rows"
                    );
                }
                _ if !outputs.is_empty() => {
                    let steps = rng.below(12) + 1;
                    pan(&mut c, &mut port, &mut rng, &mut b, steps);
                }
                _ => {}
            }
            if c.membership() != &before {
                changes += 1;
            }
            if !c.membership().rows().is_empty() {
                nonempty += 1;
            }
        }
        (changes, nonempty)
    }

    /// F-MEMBER-1, INV-RENDER-3, INV-FRAME-2 under scoping and culling: after every operation of
    /// a random sequence (output connect, disconnect and reconfigure; create, map, commit, tree
    /// change, unmap, remap, destroy; camera pans in small and large steps) the membership table,
    /// the enter/leave effects, every render list, every frame tick and every observer set equal
    /// the reference that projects every surface of every element against every view.
    #[test]
    fn scoped_visibility_equals_the_full_reference() {
        let (mut changes, mut nonempty) = (0, 0);
        for seed in 1..=40u64 {
            let (c, n) = run(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15), 400);
            changes += c;
            nonempty += n;
        }
        // the sequences must exercise the thing under test: membership that exists and changes
        assert!(
            changes > 2000,
            "membership changed in only {changes} operations"
        );
        assert!(
            nonempty > 8000,
            "membership was non-empty after only {nonempty} operations"
        );
    }

    /// One sequence in which outputs, elements and the pan interleave freely: the pan button is
    /// pressed and released as its own operation, so outputs are connected, reconfigured and
    /// removed, and elements are mapped, committed and destroyed, while a pan is held or orphaned.
    /// Returns (output changes under an active pan, of which membership changed, pan outputs
    /// removed under the pan, motions consumed by an orphaned pan).
    fn run_under_pan(seed: u64, ops: usize) -> (usize, usize, usize, usize) {
        use crate::input::interaction::Interaction;
        let mut rng = Rng(seed);
        let (mut c, mut port, _shell, mut t): (Core, InputPort, _, OutputLifecycleToken) =
            Core::new(persisted(false));
        let mut outputs: Vec<(OutputId, String)> = Vec::new();
        let mut elements: Vec<(ElementId, u64)> = Vec::new();
        let mut next_key = 1u64;
        let mut next_connector = 0u32;
        let mut pressed = false;
        let (mut under_pan, mut changed, mut removed, mut consumed) = (0, 0, 0, 0);
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: LOGO,
            time_us: 0,
        };
        for _ in 0..ops {
            let before = c.membership().clone();
            let panning = matches!(c.input.interaction, Interaction::CameraPan { .. });
            let pan_output = match c.input.interaction {
                Interaction::CameraPan { output, .. } => Some(output),
                _ => None,
            };
            let mut output_op = false;
            match rng.below(100) {
                0..=7 => {
                    pressed = !pressed;
                    let fx = c.physical_input(&mut port, button(pressed)).unwrap();
                    check(&mut c, &before, &fx, "pan button");
                }
                8..=37 => {
                    let d = if rng.below(6) == 0 { 6000 } else { 90 };
                    let delta = (rng.range(-d, d) as f64 + 0.25, rng.range(-d, d) as f64);
                    let orphaned = matches!(c.input.interaction, Interaction::Orphaned { .. });
                    let fx = c
                        .physical_input(
                            &mut port,
                            PhysicalEvent::PointerMotion {
                                delta,
                                delta_unaccel: delta,
                                time_us: 0,
                            },
                        )
                        .unwrap();
                    if orphaned {
                        assert_eq!(c.membership(), &before, "an orphaned pan moved a view");
                        consumed += 1;
                    }
                    check(&mut c, &before, &fx, "motion");
                }
                38..=47 if outputs.len() < 3 => {
                    let name = format!("X{next_connector}");
                    next_connector += 1;
                    let (o, fx) = c
                        .output_connected(&mut t, random_desc(&mut rng, &name))
                        .unwrap();
                    outputs.push((o, name));
                    check(&mut c, &before, &fx, "output connect under a pan");
                    output_op = true;
                }
                48..=55 if !outputs.is_empty() => {
                    let (o, _) = outputs.remove(rng.below(outputs.len() as u64) as usize);
                    let fx = c.output_disconnected(&mut t, o).unwrap();
                    check(&mut c, &before, &fx, "output disconnect under a pan");
                    output_op = true;
                    if pan_output == Some(o) {
                        assert!(
                            matches!(c.input.interaction, Interaction::Orphaned { .. }),
                            "the pan outlived its output"
                        );
                        removed += 1;
                    }
                }
                56..=69 if !outputs.is_empty() => {
                    // half of the time the output the pan is on, when there is one
                    let pick = outputs
                        .iter()
                        .position(|(o, _)| Some(*o) == pan_output)
                        .filter(|_| rng.below(2) == 0)
                        .unwrap_or(rng.below(outputs.len() as u64) as usize);
                    let (o, name) = outputs[pick].clone();
                    let fx = c
                        .output_reconfigured(&mut t, o, random_desc(&mut rng, &name))
                        .unwrap();
                    check(&mut c, &before, &fx, "output reconfigure under a pan");
                    output_op = true;
                }
                70..=81 => {
                    let id = c.toplevel_created();
                    let base = next_key;
                    next_key += 8;
                    let fx = c
                        .toplevel_committed(id, random_tree(&mut rng, base, false))
                        .unwrap();
                    check(&mut c, &before, &fx, "initial commit under a pan");
                    let before = c.membership().clone();
                    let fx = c
                        .toplevel_committed(id, random_tree(&mut rng, base, true))
                        .unwrap();
                    check(&mut c, &before, &fx, "map under a pan");
                    elements.push((id, base));
                }
                82..=93 if !elements.is_empty() => {
                    let (id, base) = elements[rng.below(elements.len() as u64) as usize];
                    let tree = random_tree(&mut rng, base, true);
                    let fx = if rng.below(2) == 0 {
                        c.toplevel_committed(id, tree).unwrap()
                    } else {
                        c.toplevel_tree_changed(id, tree).unwrap()
                    };
                    check(&mut c, &before, &fx, "commit under a pan");
                }
                94..=99 if elements.len() > 4 => {
                    let (id, _) = elements.remove(rng.below(elements.len() as u64) as usize);
                    let fx = c.toplevel_destroyed(id);
                    check(&mut c, &before, &fx, "destroy under a pan");
                }
                _ => {}
            }
            if output_op && panning {
                under_pan += 1;
                if c.membership() != &before {
                    changed += 1;
                }
            }
        }
        (under_pan, changed, removed, consumed)
    }

    /// F-MEMBER-1, INV-RENDER-3, INV-FRAME-2 when the set of outputs or an output's facts change
    /// while a camera pan is held (C4.6, INV-OUTPUT-1): after every operation the scoped passes
    /// equal the full reference, a pan whose output is removed is orphaned, and an orphaned pan
    /// moves no view.
    #[test]
    fn output_changes_under_a_pan_equal_the_full_reference() {
        let (mut under_pan, mut changed, mut removed, mut consumed) = (0, 0, 0, 0);
        for seed in 1..=40u64 {
            let (u, c, r, m) = run_under_pan(seed.wrapping_mul(0xd1b5_4a32_d192_ed03), 600);
            under_pan += u;
            changed += c;
            removed += r;
            consumed += m;
        }
        // the sequences must exercise the thing under test
        assert!(
            under_pan > 1000,
            "only {under_pan} output changes under a pan"
        );
        assert!(
            changed > 800,
            "membership changed in only {changed} of them"
        );
        assert!(
            removed > 100,
            "the pan's own output was removed only {removed} times"
        );
        assert!(
            consumed > 500,
            "an orphaned pan consumed only {consumed} motions"
        );
    }

    /// The cull is conservative at the view edges: an element one Canvas pixel inside the view on
    /// each side is a member, and one that only touches the edge is not, under fractional scales
    /// and at the Canvas bound; both agree with the reference.
    #[test]
    fn cull_agrees_with_the_reference_at_the_view_edges() {
        for scale_120 in [90u32, 120, 150, 180, 240] {
            for far in [0i64, 1 << 20, (1 << 36) - 4000, -(1 << 36) + 4000] {
                // one walk per view edge, so that each edge is the one the element leaves by
                for step in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    let persisted = PersistedCameras::from_entries(vec![(
                        key("A"),
                        CanvasPos {
                            x: far << 16,
                            y: far << 16,
                        },
                        1.0,
                    )])
                    .unwrap();
                    let (mut c, mut port, _s, mut t) = Core::new(persisted);
                    let d = OutputDescriptor {
                        scale_120,
                        ..desc("A", 60_000)
                    };
                    let (o, _) = c.output_connected(&mut t, d).unwrap();
                    let id = map(&mut c);
                    let button = |pressed| PhysicalEvent::PointerButton {
                        button: 0x112,
                        pressed,
                        modifiers: LOGO,
                        time_us: 0,
                    };
                    c.physical_input(&mut port, button(true)).unwrap();
                    let mut seen = [false; 2];
                    for _ in 0..1800 {
                        let before = c.membership().clone();
                        let fx = c
                            .physical_input(
                                &mut port,
                                PhysicalEvent::PointerMotion {
                                    delta: step,
                                    delta_unaccel: step,
                                    time_us: 0,
                                },
                            )
                            .unwrap();
                        check(&mut c, &before, &fx, "edge walk");
                        seen[c.membership().contains(id, skey(1), o) as usize] = true;
                    }
                    assert_eq!(
                        seen,
                        [true, true],
                        "scale {scale_120}, centre {far}, step {step:?}"
                    );
                }
            }
        }
        let _: SurfaceKey = skey(1);
    }
}
