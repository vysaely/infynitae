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
//! output lifecycle recomputes the table ([`MembershipTable::compute`]). The scoped passes write
//! into storage their caller reuses (C14.3: membership diffs into a reused buffer) and sort
//! without allocating: rows are unique after `dedup`, so an unstable sort orders them the same.

use crate::output::{OutputId, OutputModel};
use crate::projection::View;
use crate::protocol::Effect;
use crate::render::visit_entries;
use crate::snapshot::SurfaceKey;
use crate::stage::{ElementId, Stage};

/// One membership row: (element, surface, output).
pub(crate) type Row = (ElementId, SurfaceKey, OutputId);

/// (element, surface, output) rows, sorted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MembershipTable {
    rows: Vec<Row>,
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

    /// Outputs observing an element, once per row: an output can repeat.
    pub(crate) fn observing_outputs(
        &self,
        element: ElementId,
    ) -> impl Iterator<Item = &OutputId> + Clone {
        self.rows[self.range(element)].iter().map(|(_, _, o)| o)
    }

    /// Whether any output observes the element (M16: Observed).
    pub fn is_observed(&self, element: ElementId) -> bool {
        !self.range(element).is_empty()
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
        let culls: Vec<_> = views.iter().map(|(_, v)| v.cull()).collect();
        for p in stage.placed_all() {
            for ((output, view), cull) in views.iter().zip(&culls) {
                visit_entries(view, cull, &p, |i, _, _| {
                    rows.push((p.id, p.tree.surfaces[i].key, *output));
                });
            }
        }
        rows.sort_unstable();
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
    /// appends its leaves then its enters to `out`: what [`MembershipTable::diff`] gives when only
    /// this element's rows differ. An element that is not mapped has no rows. `scratch` is the
    /// caller's reusable storage for the new rows (cleared, not freed).
    pub(crate) fn replace_element(
        &mut self,
        element: ElementId,
        stage: &Stage,
        views: &[(OutputId, View)],
        scratch: &mut Vec<Row>,
        out: &mut Vec<Effect>,
    ) {
        scratch.clear();
        if let Some(p) = stage.placed(element) {
            for (output, view) in views {
                visit_entries(view, &view.cull(), &p, |i, _, _| {
                    scratch.push((element, p.tree.surfaces[i].key, *output));
                });
            }
        }
        scratch.sort_unstable();
        scratch.dedup();
        let range = self.range(element);
        let old = &self.rows[range.clone()];
        if stage.state(element).is_some() {
            for &(_, s, o) in old.iter().filter(|r| scratch.binary_search(r).is_err()) {
                out.push(Effect::Leave {
                    output: o,
                    surface: s,
                });
            }
        }
        for &(_, s, o) in scratch.iter().filter(|r| old.binary_search(r).is_err()) {
            out.push(Effect::Enter {
                output: o,
                surface: s,
            });
        }
        // an exact-size drain: the tail moves once, nothing is collected on the side
        self.rows.splice(range, scratch.drain(..));
    }

    /// This table becomes `from` with the rows of `output` recomputed under `view` (`None`: the
    /// output has none): the table after the view of one output changed, built in this table's
    /// storage. The other outputs' rows of `from` are already sorted, so only the new rows (the
    /// surfaces in view) are sorted, in `scratch`, and the two runs are merged: linear in the
    /// table, no allocation once the storage has grown.
    pub(crate) fn assign_with_output(
        &mut self,
        from: &MembershipTable,
        output: OutputId,
        view: Option<&View>,
        stage: &Stage,
        scratch: &mut Vec<Row>,
    ) {
        scratch.clear();
        if let Some(view) = view {
            let cull = view.cull();
            for p in stage.placed_all() {
                visit_entries(view, &cull, &p, |i, _, _| {
                    scratch.push((p.id, p.tree.surfaces[i].key, output));
                });
            }
        }
        scratch.sort_unstable();
        scratch.dedup();
        self.rows.clear();
        let mut kept = from.rows.iter().filter(|(_, _, o)| *o != output).peekable();
        let mut new = scratch.iter().peekable();
        loop {
            let next = match (kept.peek(), new.peek()) {
                (Some(a), Some(b)) if a <= b => kept.next(),
                (Some(_), Some(_)) | (None, Some(_)) => new.next(),
                (Some(_), None) => kept.next(),
                (None, None) => break,
            };
            self.rows.extend(next.copied());
        }
    }

    /// The rows of `output` recomputed under `view`, in place (a second output in one pass,
    /// rare: a camera shown on several outputs, a presentation change on several outputs).
    pub(crate) fn recompute_output(
        &mut self,
        output: OutputId,
        view: Option<&View>,
        stage: &Stage,
    ) {
        self.rows.retain(|(_, _, o)| *o != output);
        if let Some(view) = view {
            let rows = &mut self.rows;
            let cull = view.cull();
            for p in stage.placed_all() {
                visit_entries(view, &cull, &p, |i, _, _| {
                    rows.push((p.id, p.tree.surfaces[i].key, output));
                });
            }
        }
        self.rows.sort_unstable();
        self.rows.dedup();
    }

    /// Enter/leave effects from `self` (old) to `new`, leaves first. A leave is emitted only while
    /// its element exists: a destroyed client gets no event (INV-STAGE-4).
    pub(crate) fn diff(&self, new: &MembershipTable, stage: &Stage, out: &mut Vec<Effect>) {
        for &(e, s, o) in &self.rows {
            if !new.contains(e, s, o) && stage.state(e).is_some() {
                out.push(Effect::Leave {
                    output: o,
                    surface: s,
                });
            }
        }
        for &(e, s, o) in &new.rows {
            if !self.contains(e, s, o) {
                out.push(Effect::Enter {
                    output: o,
                    surface: s,
                });
            }
        }
    }
}

/// INV-FRAME-2: the clock output of an element is its highest-refresh observing output; ties go
/// to the lowest `OutputId`. `None` when unobserved.
pub(crate) fn clock_output<'a>(
    observers: impl Iterator<Item = &'a OutputId> + Clone,
    outputs: &[OutputModel],
) -> Option<OutputId> {
    let mut best: Option<&OutputModel> = None;
    for o in outputs
        .iter()
        .filter(|o| observers.clone().any(|x| *x == o.id))
    {
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
                frame_requested: false,
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
                // c_eff is (−960, −540), so Canvas (−960, 0): outside W, centred at
                // [−200, 200) × [−150, 150)
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
        assert_eq!(c.frame_tick_vec(a), vec![id]);
        assert!(
            c.frame_tick_vec(b).is_empty(),
            "an observer that is not the clock completes nothing"
        );
        assert!(
            c.frame_tick_vec(far).is_empty(),
            "C (240 Hz) does not observe the element"
        );
        let (fast, _) = c.output_connected(&mut t, desc("D", 144_000)).unwrap();
        assert_eq!(
            c.clock_output_of(id),
            Some(fast),
            "highest refresh observer"
        );
        assert_eq!(c.frame_tick_vec(fast), vec![id]);
        assert!(c.frame_tick_vec(a).is_empty());
        for o in [a, b, fast] {
            c.output_disconnected(&mut t, o).unwrap();
        }
        assert_eq!(c.clock_output_of(id), None, "unobserved");
        assert!(c.frame_tick_vec(far).is_empty());
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
        assert!(c.frame_tick_vec(a).is_empty());
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

    // ------------------------------------------------ H-1, formulation B (EXPERIMENTAL)
    //
    // The device bound as the largest multiple of 1/120 not above `D_max / s_max(E)`. These
    // tests go through Core's entry points and read what Core would put on the wire. Their
    // oracle is integer arithmetic written here, not `scale::h_dev`.

    /// A single-surface tree `w × 300`.
    fn side(w: i32, mapped: bool) -> SurfaceTree {
        let size = LocalSize { w, h: 300 };
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size,
            },
            surfaces: vec![SnapshotSurface {
                size: if mapped { size } else { LocalSize::default() },
                ..tree(mapped).surfaces.remove(0)
            }],
        }
    }

    /// Into a new Core: device limit `d_max`, one output of the given scale, and one mapped
    /// 400-wide element shown on it.
    fn device_setup(
        c: &mut Core,
        t: &mut crate::OutputLifecycleToken,
        d_max: u32,
        scale_120: u32,
    ) -> (OutputId, ElementId) {
        c.set_renderer_device_max(d_max).unwrap();
        let d = OutputDescriptor {
            scale_120,
            ..desc("A", 60_000)
        };
        let (o, _) = c.output_connected(t, d).unwrap();
        let id = c.toplevel_created();
        c.toplevel_committed(id, side(400, false)).unwrap();
        c.toplevel_committed(id, side(400, true)).unwrap();
        (o, id)
    }

    /// `⌊120 · D / s⌋`: the bound in wire units, in integers.
    fn bound_120(s: u32, d_max: u32) -> u64 {
        120 * u64::from(d_max) / u64::from(s)
    }

    /// The buffer side a client makes for logical side `s` at `w/120`, for the three ways a
    /// client may round: down, to nearest (half up, as wp_fractional_scale_v1 asks), up.
    fn buffers(s: u32, w: u32) -> [u64; 3] {
        let n = u64::from(s) * u64::from(w);
        [n / 120, (2 * n + 120) / 240, n.div_ceil(120)]
    }

    fn hints(fx: &[Effect], id: ElementId) -> usize {
        fx.iter()
            .filter(|e| matches!(e, Effect::PreferredScale { element, .. } if *element == id))
            .count()
    }

    /// Every side from 1 to 4·`D_max` at three device limits, on a 3.0 output (ρ = H_cfg, so
    /// the hint is H_max(E) throughout): what Core emits is exactly `⌊120·D_max/s⌋` when the
    /// device binds and 360 when it does not, and the client's buffer fits the device however
    /// the client rounds. Under the sealed bound the same sweep exceeds the device on about
    /// half the bound sides (counted here from the sealed formula, for the record).
    #[test]
    fn h1_no_emitted_hint_exceeds_the_device_at_any_side() {
        for d_max in [4096u32, 8192, 16384] {
            let (mut c, _, _, mut t) = Core::new(persisted(false));
            let (_o, id) = device_setup(&mut c, &mut t, d_max, 360);
            assert_eq!(
                c.preferred_scale_of(id),
                Some((360, 3)),
                "400 px fits at 3.0"
            );
            let (mut bound_sides, mut sealed_over) = (0u32, 0u32);
            for s in 1..=4 * d_max {
                c.toplevel_committed(id, side(s as i32, true)).unwrap();
                let (w, b) = c.preferred_scale_of(id).unwrap();
                let exact = bound_120(s, d_max);
                if exact >= 360 {
                    assert_eq!(w, 360, "D_max {d_max}, side {s}: H_cfg, not the device");
                } else {
                    bound_sides += 1;
                    assert_eq!(u64::from(w), exact, "D_max {d_max}, side {s}");
                    // largest such value: one unit more no longer satisfies s·h ≤ D_max
                    assert!(u64::from(s) * (exact + 1) > 120 * u64::from(d_max));
                    let sealed = (240 * u64::from(d_max) + u64::from(s)) / (2 * u64::from(s));
                    sealed_over += u32::from(buffers(s, sealed as u32)[1] > u64::from(d_max));
                }
                for (mode, side_px) in ["down", "nearest", "up"].iter().zip(buffers(s, w)) {
                    assert!(
                        side_px <= u64::from(d_max),
                        "D_max {d_max}, side {s}, hint {w}/120, rounding {mode}: {side_px} px"
                    );
                }
                assert_eq!(u64::from(b), (exact / 120).clamp(1, 3), "b(h), side {s}");
                if s <= d_max {
                    assert!(u64::from(s) * u64::from(b) <= u64::from(d_max));
                }
            }
            println!(
                "D_max {d_max}: {bound_sides} device-bound sides, 0 over the limit \
                 (sealed bound: {sealed_over} over)"
            );
            assert!(
                sealed_over > bound_sides / 3,
                "the sealed overshoot is real"
            );
        }
    }

    /// The three device ceilings of the T1 suite, by hand: 120·4096/3000 = 163.84,
    /// 120·8192/3000 = 327.68, 120·16384/8000 = 245.76. Rounded down they fit
    /// (3000·163/120 = 4075, 3000·327/120 = 8175, 8000·245/120 = 16333.3); rounded to nearest,
    /// as sealed, they do not (4100, 8200, 16400).
    #[test]
    fn h1_hand_derived_ceilings() {
        for (d_max, s, want, sealed) in [
            (4096u32, 3000, 163u32, 164u32),
            (8192, 3000, 327, 328),
            (16384, 8000, 245, 246),
        ] {
            let (mut c, _, _, mut t) = Core::new(persisted(false));
            let (_o, id) = device_setup(&mut c, &mut t, d_max, 360);
            c.toplevel_committed(id, side(s, true)).unwrap();
            assert_eq!(c.preferred_scale_of(id).map(|p| p.0), Some(want));
            assert!(buffers(s as u32, want)[2] <= u64::from(d_max));
            assert!(buffers(s as u32, sealed)[1] > u64::from(d_max), "H-1");
        }
    }

    /// Zero, the boundaries and the stated residual, through Core: no surface extent means no
    /// bound; a side equal to `D_max` is exactly 1.0; one more pixel is 119/120; the largest
    /// side with any emittable hint is 120·`D_max`; beyond it the hint is the wire minimum and
    /// the buffer does not fit (nothing emittable would).
    #[test]
    fn h1_zero_and_boundary_sides() {
        let d_max = 4096u32;
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        let (_o, id) = device_setup(&mut c, &mut t, d_max, 360);
        let mut at = |s: u32| {
            c.toplevel_committed(id, side(s as i32, true)).unwrap();
            c.preferred_scale_of(id).unwrap()
        };
        assert_eq!(at(d_max / 3), (360, 3), "3·1365 = 4095: H_cfg fits");
        assert_eq!(at(d_max / 3 + 1), (359, 2), "3·1366 = 4098 does not");
        assert_eq!(at(d_max - 1), (120, 1));
        assert_eq!(at(d_max), (120, 1), "side = D_max: exactly 1.0");
        assert_eq!(at(d_max + 1), (119, 1));
        assert_eq!(at(120 * d_max), (1, 1), "the last side with a fitting hint");
        assert_eq!(buffers(120 * d_max, 1), [u64::from(d_max); 3]);
        assert_eq!(at(120 * d_max + 1), (1, 1), "residual: wire minimum");
        assert!(
            buffers(120 * d_max + 1, 1)[2] > u64::from(d_max),
            "and it does not fit"
        );
        assert_eq!(
            at(i32::MAX as u32),
            (1, 1),
            "largest protocol size: no overflow"
        );

        // no extent: unmapped tree, and a zero-sized mapped one
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        let (_o, _) = device_setup(&mut c, &mut t, 1, 360);
        let id = c.toplevel_created();
        c.toplevel_committed(id, side(0, false)).unwrap();
        assert_eq!(crate::scale::s_max_for_tree(c.stage().tree(id)), 0);
        assert_eq!(c.preferred_scale_of(id), Some((360, 3)), "nothing to bound");
    }

    /// A client alone cannot move the hint by resizing between two device-bound sizes: after
    /// the one P7 decrease the hint is held, 50 cycles produce no further change, and the value
    /// on the wire stays the one for the larger size. (Bounding the emission instead of the
    /// bound, draft A1, gives 99 changes on this sequence.) A non-client stimulus releases it,
    /// once, to the bound of the size then in force.
    #[test]
    fn h1_client_resizes_do_not_oscillate_the_hint() {
        for (d_max, big, small, held, released) in [
            (4096u32, 3000, 2990, 163u32, 164u32),
            (16384, 8000, 7990, 245, 246),
        ] {
            let (mut c, _, _, mut t) = Core::new(persisted(false));
            let (o, id) = device_setup(&mut c, &mut t, d_max, 360);
            let fx = c.toplevel_committed(id, side(big, true)).unwrap();
            assert_eq!(hints(&fx, id), 1, "the P7 decrease");
            assert_eq!(c.preferred_scale_of(id).map(|p| p.0), Some(held));
            let mut changes = 0;
            for _ in 0..50 {
                changes += hints(&c.toplevel_committed(id, side(small, true)).unwrap(), id);
                assert_eq!(c.preferred_scale_of(id).map(|p| p.0), Some(held));
                changes += hints(&c.toplevel_committed(id, side(big, true)).unwrap(), id);
                assert_eq!(c.preferred_scale_of(id).map(|p| p.0), Some(held));
            }
            assert_eq!(changes, 0, "D_max {d_max}: client-only churn");

            c.toplevel_committed(id, side(small, true)).unwrap();
            let d = OutputDescriptor {
                scale_120: 360,
                ..desc("A", 60_000)
            };
            let fx = c.output_reconfigured(&mut t, o, d).unwrap();
            assert_eq!(hints(&fx, id), 1, "a stimulus releases the hold, once");
            assert_eq!(c.preferred_scale_of(id).map(|p| p.0), Some(released));
            assert!(buffers(small as u32, released)[2] <= u64::from(d_max));
            // growing again is P7, immediately
            let fx = c.toplevel_committed(id, side(big, true)).unwrap();
            assert_eq!(hints(&fx, id), 1);
            assert_eq!(c.preferred_scale_of(id).map(|p| p.0), Some(held));
        }
    }

    /// An arbitrary client resize sequence with stimuli mixed in (a fixed pseudo-random script,
    /// 20,000 steps): after every step the hint on the wire fits the device at the size in
    /// force, and the hint changes at most once per step.
    #[test]
    fn h1_any_resize_sequence_keeps_the_buffer_within_the_device() {
        let d_max = 4096u32;
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        let (o, id) = device_setup(&mut c, &mut t, d_max, 360);
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let (mut size, mut total) = (400u32, 0usize);
        for step in 0..20_000 {
            let r = next();
            let fx = if r % 16 == 0 {
                let d = OutputDescriptor {
                    scale_120: [150, 210, 360][(r >> 8) as usize % 3],
                    ..desc("A", 60_000)
                };
                c.output_reconfigured(&mut t, o, d).unwrap()
            } else {
                size = match (r >> 4) % 4 {
                    0 => 1 + (r >> 8) as u32 % (4 * d_max),
                    1 => (size + 1).min(4 * d_max),
                    2 => size.saturating_sub(1).max(1),
                    _ => d_max - 8 + (r >> 8) as u32 % 16,
                };
                c.toplevel_committed(id, side(size as i32, true)).unwrap()
            };
            let n = hints(&fx, id);
            assert!(n <= 1, "step {step}: {n} hint changes in one batch");
            total += n;
            let (w, b) = c.preferred_scale_of(id).unwrap();
            assert!(
                buffers(size, w)[2] <= u64::from(d_max),
                "step {step}: side {size} at {w}/120"
            );
            assert!(u64::from(w) <= bound_120(size, d_max).max(1));
            if size <= d_max {
                assert!(u64::from(size) * u64::from(b) <= u64::from(d_max));
            }
        }
        assert!(
            total > 100,
            "the script did exercise the policy: {total} changes"
        );
    }

    /// Fractional output scales: while the output's scale fits the device it is the hint,
    /// unchanged; once the device binds, the hint is the quantised bound and never above it.
    #[test]
    fn h1_fractional_output_scales_meet_the_bound() {
        let d_max = 4096u32;
        for scale_120 in [150u32, 180, 210, 270] {
            let (mut c, _, _, mut t) = Core::new(persisted(false));
            let (_o, id) = device_setup(&mut c, &mut t, d_max, scale_120);
            assert_eq!(c.preferred_scale_of(id).map(|p| p.0), Some(scale_120));
            for s in (401..=2 * d_max).step_by(3) {
                c.toplevel_committed(id, side(s as i32, true)).unwrap();
                let w = c.preferred_scale_of(id).unwrap().0;
                let exact = bound_120(s, d_max);
                if exact >= u64::from(scale_120) {
                    assert_eq!(
                        w, scale_120,
                        "scale {scale_120}, side {s}: not device-bound"
                    );
                } else {
                    assert_eq!(u64::from(w), exact, "scale {scale_120}, side {s}");
                }
                assert!(buffers(s, w)[2] <= u64::from(d_max));
            }
        }
    }

    /// The device limit changing under a device-bound element: lowering it is P7, at once, to
    /// the new quantised bound. Raising it does not raise the fractional hint by itself (it is
    /// held; H-14 is a separate open residual); the next stimulus releases it to the new bound.
    #[test]
    fn h1_device_limit_transitions() {
        let (mut c, _, _, mut t) = Core::new(persisted(false));
        let (o, id) = device_setup(&mut c, &mut t, 16384, 360);
        c.toplevel_committed(id, side(3000, true)).unwrap();
        assert_eq!(c.preferred_scale_of(id), Some((360, 3)));
        for (d_max, want) in [
            (8192u32, (327u32, 2u32)),
            (4096, (163, 1)),
            (4095, (163, 1)),
        ] {
            let fx = c.set_renderer_device_max(d_max).unwrap();
            assert_eq!(c.preferred_scale_of(id), Some(want), "lowered to {d_max}");
            assert_eq!(hints(&fx, id), usize::from(d_max != 4095));
            assert!(buffers(3000, want.0)[2] <= u64::from(d_max));
        }
        // raised: the fractional hint is held at 163; the integer hint ⌈h⌉ = 2 now fits the
        // device (3000·2 ≤ 8192) and is sent, which is F-SCALE-3 as sealed
        let fx = c.set_renderer_device_max(8192).unwrap();
        assert_eq!(c.preferred_scale_of(id), Some((163, 2)), "h held");
        assert_eq!(hints(&fx, id), 1, "the integer hint alone");
        let d = OutputDescriptor {
            scale_120: 360,
            ..desc("A", 60_000)
        };
        c.output_reconfigured(&mut t, o, d).unwrap();
        assert_eq!(c.preferred_scale_of(id), Some((327, 2)));
        // the smallest limit Core accepts
        c.set_renderer_device_max(1).unwrap();
        assert_eq!(c.preferred_scale_of(id), Some((1, 1)), "3000 px at D_max 1");
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
        c.start_scale_interaction(
            id,
            InteractionKind::CameraZoom,
            false,
            crate::scale::SessionTime::from_millis(1_000),
        );
        let deadline = 1_000 + DEFAULT_T_SETTLE_MS;
        assert_eq!(
            c.next_settlement_deadline(),
            Some(crate::scale::SessionTime::from_millis(deadline))
        );
        c.scale_tick(crate::scale::SessionTime::from_millis(deadline - 1));
        assert_eq!(
            c.next_settlement_deadline(),
            Some(crate::scale::SessionTime::from_millis(deadline)),
            "not due yet"
        );
        c.scale_tick(crate::scale::SessionTime::from_millis(deadline));
        assert_eq!(
            c.next_settlement_deadline(),
            None,
            "settled: nothing left to wake for"
        );
        c.start_scale_interaction(
            id,
            InteractionKind::CameraPan,
            true,
            crate::scale::SessionTime::from_millis(5_000),
        );
        assert_eq!(
            c.next_settlement_deadline(),
            None,
            "a continuous epoch has no deadline"
        );
    }

    /// M7 `Open | client response | Open`: only a step of the interaction moves a discrete
    /// epoch's deadline. Core's batch-end evaluation is not a step and has no clock. Client
    /// commits, another element's lifecycle and a camera drag (a continuous interaction, with
    /// real input) all leave an open discrete epoch discrete, with the deadline its step set.
    /// The defect this pins evaluated every batch as a step at time 0, which put the deadline at
    /// `T_settle` after the session's start: already past for any session older than that.
    #[test]
    fn an_evaluation_does_not_move_an_open_epochs_deadline() {
        use crate::PhysicalEvent;
        use crate::input::bindings::Modifiers;
        use crate::scale::{DEFAULT_T_SETTLE_MS, InteractionKind};
        let (mut c, mut i, _, mut t) = Core::new(persisted(false));
        c.output_connected(&mut t, desc("A", 60_000)).unwrap();
        let id = map(&mut c);
        let step = 5_000;
        c.start_scale_interaction(
            id,
            InteractionKind::WindowResize,
            false,
            crate::scale::SessionTime::from_millis(step),
        );
        let deadline = step + DEFAULT_T_SETTLE_MS;
        let unchanged = |c: &Core, what: &str| {
            let e = c.scale_state(id).unwrap().epoch;
            assert!(e.open && !e.continuous, "{what}: {e:?}");
            assert_eq!(e.kind, Some(InteractionKind::WindowResize), "{what}");
            assert_eq!(
                c.next_settlement_deadline(),
                Some(crate::scale::SessionTime::from_millis(deadline)),
                "{what}"
            );
        };
        unchanged(&c, "opened");
        // the session clock moves on (no deadline is due): an evaluation after this must not
        // re-arm the epoch from the later time either
        assert!(
            c.scale_tick(crate::scale::SessionTime::from_millis(step + 100))
                .is_empty()
        );
        unchanged(&c, "a tick before the deadline");
        for w in [500, 401, 400] {
            c.toplevel_committed(id, side(w, true)).unwrap();
            unchanged(&c, "a client commit");
        }
        let other = c.toplevel_created();
        c.toplevel_destroyed(other);
        unchanged(&c, "another element's lifecycle");
        // a camera drag: a continuous interaction is active while the discrete epoch is open
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: logo,
            time_us: 0,
        };
        c.physical_input_vec(&mut i, button(true)).unwrap();
        assert!(c.interaction().active(), "the drag was acquired");
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (30.0, 0.0),
                delta_unaccel: (30.0, 0.0),
                time_us: 0,
            },
        )
        .unwrap();
        unchanged(&c, "a camera drag step");
        c.physical_input_vec(&mut i, button(false)).unwrap();
        unchanged(&c, "the drag's release settles drag epochs only");

        assert!(
            c.scale_tick(crate::scale::SessionTime::from_millis(deadline - 1))
                .is_empty()
        );
        unchanged(&c, "a tick before the deadline");
        // a further step of the discrete interaction does move it. The clock has reached
        // `deadline − 1` (the tick above), so the step is taken there: 299 ms after the first.
        let second = deadline - 1;
        c.start_scale_interaction(
            id,
            InteractionKind::WindowResize,
            false,
            crate::scale::SessionTime::from_millis(second),
        );
        assert_eq!(
            c.next_settlement_deadline(),
            Some(crate::scale::SessionTime::from_millis(
                second + DEFAULT_T_SETTLE_MS
            ))
        );
        c.scale_tick(crate::scale::SessionTime::from_millis(deadline));
        assert_eq!(
            c.next_settlement_deadline(),
            Some(crate::scale::SessionTime::from_millis(
                second + DEFAULT_T_SETTLE_MS
            )),
            "superseded"
        );
        c.scale_tick(crate::scale::SessionTime::from_millis(
            second + DEFAULT_T_SETTLE_MS,
        ));
        assert_eq!(
            c.next_settlement_deadline(),
            None,
            "settled at its deadline"
        );
        assert!(!c.scale_state(id).unwrap().epoch.open);
    }

    /// The session clock is monotonic as far as the scale policy is concerned. A time earlier
    /// than one already seen is read as the latest seen: a step stamped in the past cannot pull a
    /// deadline earlier or place one behind the clock, and a tick with an old time settles
    /// nothing that a later tick would not have.
    #[test]
    fn epoch_deadlines_do_not_go_backwards_with_the_clock() {
        use crate::scale::{DEFAULT_T_SETTLE_MS, InteractionKind};
        let t = DEFAULT_T_SETTLE_MS;
        let (mut c, _, _, mut tok) = Core::new(persisted(false));
        c.output_connected(&mut tok, desc("A", 60_000)).unwrap();
        let id = map(&mut c);
        c.start_scale_interaction(
            id,
            InteractionKind::CameraZoom,
            false,
            crate::scale::SessionTime::from_millis(5_000),
        );
        assert_eq!(
            c.next_settlement_deadline(),
            Some(crate::scale::SessionTime::from_millis(5_000 + t))
        );
        // a step stamped two seconds in the past: read as 5 000, the deadline stays
        c.start_scale_interaction(
            id,
            InteractionKind::CameraZoom,
            false,
            crate::scale::SessionTime::from_millis(3_000),
        );
        assert_eq!(
            c.next_settlement_deadline(),
            Some(crate::scale::SessionTime::from_millis(5_000 + t))
        );
        // a later step moves it later
        c.start_scale_interaction(
            id,
            InteractionKind::CameraZoom,
            false,
            crate::scale::SessionTime::from_millis(5_100),
        );
        assert_eq!(
            c.next_settlement_deadline(),
            Some(crate::scale::SessionTime::from_millis(5_100 + t))
        );
        // a tick with an old time does not settle; the clock Core holds is still 5 100
        assert!(
            c.scale_tick(crate::scale::SessionTime::from_millis(1))
                .is_empty()
        );
        assert!(c.scale_state(id).unwrap().epoch.open);
        c.scale_tick(crate::scale::SessionTime::from_millis(5_100 + t));
        assert_eq!(c.next_settlement_deadline(), None);
        // after a settlement at 5 400, a new epoch stamped at 100 is armed from 5 400, not 100
        c.start_scale_interaction(
            id,
            InteractionKind::CameraZoom,
            false,
            crate::scale::SessionTime::from_millis(100),
        );
        assert_eq!(
            c.next_settlement_deadline(),
            Some(crate::scale::SessionTime::from_millis(5_100 + 2 * t))
        );
        // a continuous epoch that becomes discrete takes the step's deadline
        c.scale_tick(crate::scale::SessionTime::from_millis(5_100 + 2 * t));
        c.start_scale_interaction(
            id,
            InteractionKind::CameraPan,
            true,
            crate::scale::SessionTime::from_millis(9_000),
        );
        assert_eq!(c.next_settlement_deadline(), None);
        c.start_scale_interaction(
            id,
            InteractionKind::CameraPan,
            false,
            crate::scale::SessionTime::from_millis(9_050),
        );
        assert_eq!(
            c.next_settlement_deadline(),
            Some(crate::scale::SessionTime::from_millis(9_050 + t))
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
            frame_requested: false,
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
                    frame_requested: false,
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
                    frame_requested: false,
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
                    frame_requested: false,
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
        let mut expected = Vec::new();
        before.diff(&reference, c.stage(), &mut expected);
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
            assert_eq!(c.frame_tick_vec(o), tick, "frame tick after {what}");
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
        let fx = c.physical_input_vec(port, button(true)).unwrap();
        check(c, before, &fx, "pan press");
        for _ in 0..steps {
            // mostly small steps (elements cross the view edge one at a time), sometimes a jump
            let d = if rng.below(6) == 0 { 6000 } else { 90 };
            let delta = (rng.range(-d, d) as f64 + 0.25, rng.range(-d, d) as f64);
            *before = c.membership().clone();
            let fx = c
                .physical_input_vec(
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
        let fx = c.physical_input_vec(port, button(false)).unwrap();
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

    /// The scoped batch-end scale evaluation (`scale::Sweep`) against the reference that
    /// evaluates every mapped element at the end of every batch: after every operation of a
    /// random sequence, every element's scale state (hint, requirement, emitted values, SC-E
    /// direction, epoch) and every effect are the same. Operations: outputs connected, removed and
    /// reconfigured; elements created, mapped, committed, unmapped, remapped and destroyed; a
    /// camera drag pressed, moved and released as separate operations, so that commits and output
    /// changes happen under it; discrete and continuous interaction steps; settlement ticks on an
    /// advancing session clock; configuration and device-limit changes (small limits make P7
    /// fire); a camera batch outside a drag.
    #[test]
    fn scoped_scale_evaluation_equals_the_full_reference() {
        let mut totals = [0usize; 6];
        for seed in 1..=40u64 {
            let run = scale_run(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15), 400);
            for (t, n) in totals.iter_mut().zip(run) {
                *t += n;
            }
        }
        let [scoped, full, hints, held, opened, lowered] = totals;
        // the sequences must exercise what is under test: passes that skip elements, hints that
        // change, SC-E holds, epochs a drag opens, and forced P7 decreases
        assert!(
            scoped > 5000,
            "only {scoped} scoped evaluations ({full} full)"
        );
        assert!(full > 1000, "only {full} full evaluations");
        assert!(hints > 7000, "only {hints} hint emissions");
        assert!(
            held > 750,
            "an element was held after only {held} operations"
        );
        assert!(
            opened > 400,
            "a drag opened an epoch in only {opened} operations"
        );
        assert!(
            lowered > 190,
            "P7 lowered a hint in only {lowered} operations"
        );
    }

    /// P-1: the steady state of a drag and a client's commits evaluate the scale policy for what
    /// they changed, not for every element. On a Canvas of 200 windows: after its first step, a
    /// drag's further steps are scoped passes, and so are commits during and after it; the first
    /// step of a drag, its release (a settlement) and an output change are what visit every
    /// element.
    #[test]
    fn steady_state_batches_do_not_evaluate_every_element() {
        let (mut c, mut port, _shell, mut t): (Core, InputPort, _, OutputLifecycleToken) =
            Core::new(persisted(false));
        let (o, _) = c.output_connected(&mut t, desc("A", 60_000)).unwrap();
        let ids: Vec<ElementId> = (0..200).map(|_| map(&mut c)).collect();
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: LOGO,
            time_us: 0,
        };
        let motion = |d: f64| PhysicalEvent::PointerMotion {
            delta: (d, 0.0),
            delta_unaccel: (d, 0.0),
            time_us: 0,
        };
        let full = |c: &Core| c.scale_passes.0;

        // a drag: its first step visits every element (it opens their epochs), then no more
        c.physical_input_vec(&mut port, button(true)).unwrap();
        let before = full(&c);
        c.physical_input_vec(&mut port, motion(5.0)).unwrap();
        assert_eq!(
            full(&c),
            before + 1,
            "the first step of a drag is a full pass"
        );
        let steady = full(&c);
        for (i, &id) in ids.iter().take(50).enumerate() {
            let d = if i % 2 == 0 { 900.0 } else { -900.0 };
            c.physical_input_vec(&mut port, motion(d)).unwrap();
            c.toplevel_committed(id, tree(true)).unwrap();
        }
        assert_eq!(full(&c), steady, "steps and commits during a drag");
        c.physical_input_vec(&mut port, button(false)).unwrap();

        // commits outside a drag
        let steady = full(&c);
        for &id in &ids[..50] {
            c.toplevel_committed(id, tree(true)).unwrap();
        }
        assert_eq!(full(&c), steady, "commits outside a drag");

        // an output change is evaluated for every element
        let doubled = OutputDescriptor {
            scale_120: 240,
            ..desc("A", 60_000)
        };
        c.output_reconfigured(&mut t, o, doubled).unwrap();
        assert!(full(&c) > steady, "an output change");
        assert!(
            c.scale_passes.1 >= 150,
            "scoped passes: {}",
            c.scale_passes.1
        );
    }

    /// One sequence of [`scoped_scale_evaluation_equals_the_full_reference`]. Returns (scoped
    /// passes, full passes, hint emissions, operations after which a mapped element was held,
    /// operations in which a drag opened an epoch, operations in which P7 lowered a hint).
    fn scale_run(seed: u64, ops: usize) -> [usize; 6] {
        use crate::input::interaction::Interaction;
        use crate::scale::{InteractionKind, ScalePolicyConfig, SessionTime};
        let mut rng = Rng(seed);
        // [scoped, reference]
        let mut sides: Vec<(Core, InputPort, OutputLifecycleToken)> = (0..2)
            .map(|_| {
                let (c, port, _shell, t) = Core::new(persisted(false));
                (c, port, t)
            })
            .collect();
        sides[1].0.scale_reference = true;
        let mut outputs: Vec<(OutputId, String)> = Vec::new();
        // (element, mapped)
        let mut elements: Vec<(ElementId, bool)> = Vec::new();
        let (mut next_key, mut next_connector, mut now) = (1u64, 0u32, 0u64);
        let (mut hints, mut held, mut opened, mut lowered) = (0, 0, 0, 0);
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: LOGO,
            time_us: 0,
        };
        let mut pressed = false;
        for _ in 0..ops {
            // every random choice is drawn once, then applied to both sides
            now += rng.below(400);
            let op = rng.below(100);
            let pick = rng.next();
            let desc_seed = rng.next() | 1;
            let desc = |name: &str| random_desc(&mut Rng(desc_seed), name);
            let trees = [
                random_tree(&mut rng, next_key, false),
                random_tree(&mut rng, next_key, true),
            ];
            let d = if rng.below(6) == 0 { 6000 } else { 90 };
            let delta = (rng.range(-d, d) as f64 + 0.25, rng.range(-d, d) as f64);
            let kind = [
                InteractionKind::CameraPan,
                InteractionKind::CameraZoom,
                InteractionKind::WindowMove,
                InteractionKind::WindowResize,
                InteractionKind::ShellStream,
            ][rng.below(5) as usize];
            let continuous = rng.below(3) == 0;
            let config = ScalePolicyConfig {
                h_cfg: [1.5, 2.0, 3.0][rng.below(3) as usize],
                r: [1.25, 1.5, 2.0][rng.below(3) as usize],
                sigma: [0.0, 0.05][rng.below(2) as usize],
                t_settle_ms: [100, 300][rng.below(2) as usize],
                d_max: 16384,
            };
            let d_max = [16384, 2000, 900, 500][rng.below(4) as usize];
            let output = (!outputs.is_empty()).then(|| outputs[pick as usize % outputs.len()].0);
            let element = (!elements.is_empty()).then(|| elements[pick as usize % elements.len()]);

            let mut results = Vec::new();
            for (i, (c, port, t)) in sides.iter_mut().enumerate() {
                let before: Vec<Option<crate::scale::ScaleState>> = elements
                    .iter()
                    .map(|&(e, _)| c.scale_state(e).cloned())
                    .collect();
                let mut fx = Vec::new();
                let mut connected = None;
                match (op, element) {
                    (0..=2, _) if outputs.len() < 3 => {
                        let name = format!("X{next_connector}");
                        let (o, f) = c.output_connected(t, desc(&name)).unwrap();
                        connected = Some(o);
                        fx = f;
                    }
                    (3, _) if output.is_some() => {
                        fx = c.output_disconnected(t, output.unwrap()).unwrap();
                    }
                    (4..=6, _) if output.is_some() => {
                        let o = output.unwrap();
                        let name = &outputs.iter().find(|(x, _)| *x == o).unwrap().1;
                        fx = c.output_reconfigured(t, o, desc(name)).unwrap();
                    }
                    (7..=21, _) => {
                        let id = c.toplevel_created();
                        fx.extend(c.toplevel_committed(id, trees[0].clone()).unwrap());
                        fx.extend(c.toplevel_committed(id, trees[1].clone()).unwrap());
                    }
                    (22..=44, Some((id, true))) if pick % 7 < 4 => {
                        fx = c.toplevel_committed(id, trees[1].clone()).unwrap();
                    }
                    (22..=44, Some((id, true))) => {
                        fx = c.toplevel_tree_changed(id, trees[1].clone()).unwrap();
                    }
                    (22..=44, Some((id, false))) => {
                        fx.extend(c.toplevel_committed(id, trees[0].clone()).unwrap());
                        fx.extend(c.toplevel_committed(id, trees[1].clone()).unwrap());
                    }
                    (45..=49, Some((id, true))) => {
                        fx = c.toplevel_committed(id, trees[0].clone()).unwrap();
                    }
                    (50..=53, Some((id, _))) => fx = c.toplevel_destroyed(id),
                    (54..=59, _) => fx = c.physical_input_vec(port, button(!pressed)).unwrap(),
                    (60..=79, _) => {
                        let motion = PhysicalEvent::PointerMotion {
                            delta,
                            delta_unaccel: delta,
                            time_us: 0,
                        };
                        fx = c.physical_input_vec(port, motion).unwrap();
                    }
                    (80..=85, Some((id, _))) => {
                        let at = SessionTime::from_millis(now);
                        c.start_scale_interaction(id, kind, continuous, at);
                    }
                    (86..=91, _) => fx = c.scale_tick(SessionTime::from_millis(now)),
                    (92..=94, _) => fx = c.set_scale_config(config).unwrap(),
                    (95..=97, _) => fx = c.set_renderer_device_max(d_max).unwrap(),
                    // a camera batch that is not part of a drag (none exists yet: the case a
                    // future camera move outside a continuous interaction would hit)
                    (98..=99, _) if output.is_some() && !pressed => {
                        if let Some(camera) = c.cameras().camera_for_output(output.unwrap()) {
                            fx = Vec::new();
                            c.visibility_pass_camera(camera, &mut fx);
                        }
                    }
                    _ => {}
                }
                // scale emissions compared as a set (a scoped pass visits elements in ascending
                // order, a full one bottom to top), every other effect in order
                let (mut scale, other): (Vec<Effect>, Vec<Effect>) = fx
                    .into_iter()
                    .partition(|e| matches!(e, Effect::PreferredScale { .. }));
                scale.sort_by_key(|e| match e {
                    Effect::PreferredScale { element, .. } => *element,
                    _ => unreachable!(),
                });
                if i == 1 {
                    hints += scale.len();
                    let drag = matches!(c.input.interaction, Interaction::CameraPan { .. });
                    let now_open = |e| c.scale_state(e).is_some_and(|s| s.epoch.open);
                    if drag
                        && elements.iter().zip(&before).any(|(&(e, _), b)| {
                            b.as_ref().is_some_and(|b| !b.epoch.open) && now_open(e)
                        })
                    {
                        opened += 1;
                    }
                    if elements.iter().zip(&before).any(|(&(e, _), b)| {
                        matches!((b, c.scale_state(e)), (Some(b), Some(s)) if s.h < b.h && s.epoch.direction == -1 && b.epoch.direction != -1)
                    }) {
                        lowered += 1;
                    }
                }
                results.push((scale, other, connected));
            }
            assert_eq!(
                (&results[0].0, &results[0].1),
                (&results[1].0, &results[1].1),
                "effects of operation {op} (seed {seed:#x})"
            );
            let (a, r) = (&sides[0].0, &sides[1].0);
            // no stale entry: every scale state belongs to an element the Stage still has
            // (destruction removes it; an unmapped element keeps its hint, INV-SCALE-5)
            for c in [a, r] {
                for (e, _) in c.scales.iter() {
                    assert!(
                        c.stage().state(*e).is_some(),
                        "a scale state outlives {e:?} after operation {op} (seed {seed:#x})"
                    );
                }
            }
            for &(e, _) in &elements {
                assert_eq!(
                    a.scale_state(e),
                    r.scale_state(e),
                    "scale state of {e:?} after operation {op} (seed {seed:#x})"
                );
            }

            // the model, identical for both sides
            match (op, element) {
                (0..=2, _) if outputs.len() < 3 => {
                    let o = results[0].2.unwrap();
                    assert_eq!(Some(o), results[1].2, "output ids diverged");
                    outputs.push((o, format!("X{next_connector}")));
                    next_connector += 1;
                }
                (3, _) if output.is_some() => outputs.retain(|(o, _)| Some(*o) != output),
                (7..=21, _) => {
                    let id = r.stage().z_order().iter().copied().max().unwrap();
                    elements.push((id, true));
                    next_key += 8;
                }
                (22..=44, Some((id, _))) => {
                    elements.iter_mut().find(|(e, _)| *e == id).unwrap().1 = true;
                }
                (45..=49, Some((id, _))) => {
                    elements.iter_mut().find(|(e, _)| *e == id).unwrap().1 = false;
                }
                (50..=53, Some((id, _))) => elements.retain(|(e, _)| *e != id),
                (54..=59, _) => pressed = !pressed,
                _ => {}
            }
            let sigma = r.scale_config().sigma;
            let is_held = |e: ElementId| {
                let s_max = crate::scale::s_max_for_tree(r.stage().tree(e));
                let cfg = r.scale_config();
                let h_max = crate::scale::h_max(cfg.h_cfg, s_max, cfg.d_max);
                let scales: Vec<f64> = r.outputs.iter().map(|o| o.scale).collect();
                let ladder = crate::scale::ladder(&scales, h_max, cfg.r);
                let target = r
                    .compute_rho(e)
                    .map(|rho| crate::scale::target(rho, &ladder, h_max, sigma));
                // outside an epoch, SC-E held the hint away from its target
                r.scale_state(e).is_some_and(|s| {
                    !s.epoch.open && target.is_some_and(|t| (t - s.h).abs() > 1e-12)
                })
            };
            if elements.iter().any(|&(e, mapped)| mapped && is_held(e)) {
                held += 1;
            }
        }
        let (full, scoped) = sides[0].0.scale_passes;
        [scoped, full, hints, held, opened, lowered]
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
                    let fx = c.physical_input_vec(&mut port, button(pressed)).unwrap();
                    check(&mut c, &before, &fx, "pan button");
                }
                8..=37 => {
                    let d = if rng.below(6) == 0 { 6000 } else { 90 };
                    let delta = (rng.range(-d, d) as f64 + 0.25, rng.range(-d, d) as f64);
                    let orphaned = matches!(c.input.interaction, Interaction::Orphaned { .. });
                    let fx = c
                        .physical_input_vec(
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
                    c.physical_input_vec(&mut port, button(true)).unwrap();
                    let mut seen = [false; 2];
                    for _ in 0..1800 {
                        let before = c.membership().clone();
                        let fx = c
                            .physical_input_vec(
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
