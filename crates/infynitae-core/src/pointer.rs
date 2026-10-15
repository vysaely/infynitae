//! Seat physical pointer state (OBJ-20), core part: the authoritative `(OutputId, OutputLogical p)`
//! (D-B2-PTR-STATE, INV-COORD-5), moved by physical deltas in the Arrangement (B2.5 T1) and by the
//! output lifecycle (INV-OUTPUT-1).

use crate::geom::{CanvasPoint, CanvasPos, LocalPoint, LocalRect, Q16_ONE};
use crate::output::{OutputId, OutputModel};
use crate::snapshot::SurfaceKey;

/// wl_fixed resolution (2⁻⁸): the half-open clamp bound of F-RELOCATE-1 and F-RECONF-1.
pub const WL_FIXED_EPS: f64 = 1.0 / 256.0;

/// `Located { output, p }` with p in the output's half-open OutputLogical extent, or `NoOutput`
/// (no output exists: nothing is fabricated).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerState {
    Located { output: OutputId, p: (f64, f64) },
    NoOutput,
}

/// Constraint kind: lock or confinement (OBJ-21, M13, M14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintKind {
    Lock,
    Confine,
}

/// Constraint lifetime: persistent or oneshot (M13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintLifetime {
    Persistent,
    Oneshot,
}

/// Authoritative constraint state (OBJ-21, M13).
/// Note: per INV-COORD-5, this never stores an output ID or a Canvas lock point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstraintState {
    pub surface: SurfaceKey,
    pub kind: ConstraintKind,
    pub lifetime: ConstraintLifetime,
    pub lock_point: LocalPoint,
    pub region: Option<LocalRect>,
}

/// F-LOCK-1: Derived Canvas lock point from surface Canvas origin and surface-local lock point.
/// `q_lock = surface_origin(S) + lock_point`. Never stored (INV-COORD-5).
pub fn derive_lock_point(surface_origin: CanvasPoint, lock_point: LocalPoint) -> CanvasPos {
    CanvasPos {
        x: (surface_origin.x + lock_point.x as i64) * Q16_ONE,
        y: (surface_origin.y + lock_point.y as i64) * Q16_ONE,
    }
}

/// Clamp into `[0, extent − 2⁻⁸]` per coordinate.
fn clamp_halfopen(p: (f64, f64), extent: (f64, f64)) -> (f64, f64) {
    (
        p.0.clamp(0.0, extent.0 - WL_FIXED_EPS),
        p.1.clamp(0.0, extent.1 - WL_FIXED_EPS),
    )
}

/// Startup rule: the centre of the new output's logical extent (INV-OUTPUT-1).
pub(crate) fn startup(output: &OutputModel) -> PointerState {
    let e = output.logical_extent();
    PointerState::Located {
        output: output.id,
        p: clamp_halfopen((e.0 / 2.0, e.1 / 2.0), e),
    }
}

/// F-RECONF-1: p clamped into the reconfigured output's new logical extent; a p already inside
/// is unchanged. No camera value is read.
pub(crate) fn reconfigured(state: PointerState, output: &OutputModel) -> PointerState {
    match state {
        PointerState::Located { output: o, p } if o == output.id => PointerState::Located {
            output: o,
            p: clamp_halfopen(p, output.logical_extent()),
        },
        other => other,
    }
}

/// F-RELOCATE-1 on loss of the interaction output. Its domain is a NON-EMPTY survivor set, so the
/// signature takes the first survivor separately: evaluating it on an empty set is unrepresentable
/// (E-F2-05). `a = arr_origin(O_lost) + p`, then [`locate`].
pub(crate) fn relocate(
    lost: &OutputModel,
    p: (f64, f64),
    first: &OutputModel,
    rest: &[OutputModel],
) -> PointerState {
    locate(
        (lost.arrangement.0 + p.0, lost.arrangement.1 + p.1),
        first,
        rest,
    )
}

/// B2.5 T1 for a relative device: `a' = arr_origin(O) + p + Δ`, clamped to the union of the
/// output rects and located. `None` when `from` is not connected.
pub(crate) fn moved(
    outputs: &[OutputModel],
    from: OutputId,
    p: (f64, f64),
    delta: (f64, f64),
) -> Option<PointerState> {
    let o = outputs.iter().find(|o| o.id == from)?;
    let (first, rest) = outputs.split_first()?;
    Some(locate(
        (
            o.arrangement.0 + p.0 + delta.0,
            o.arrangement.1 + p.1 + delta.1,
        ),
        first,
        rest,
    ))
}

/// An Arrangement point located on the output whose rect is nearest to it (distance 0 when a rect
/// contains it; ties: first in order), clamped half-open into that rect: the nearest point of the
/// union of the output rects. The output set is non-empty by signature.
pub(crate) fn locate(a: (f64, f64), first: &OutputModel, rest: &[OutputModel]) -> PointerState {
    let candidate = |o: &OutputModel| {
        let local = clamp_halfopen(
            (a.0 - o.arrangement.0, a.1 - o.arrangement.1),
            o.logical_extent(),
        );
        let d =
            (o.arrangement.0 + local.0 - a.0).powi(2) + (o.arrangement.1 + local.1 - a.1).powi(2);
        (d, o.id, local)
    };
    let mut best = candidate(first);
    for o in rest {
        let c = candidate(o);
        if c.0 < best.0 {
            best = c;
        }
    }
    PointerState::Located {
        output: best.1,
        p: best.2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{OutputDescriptor, Transform};

    fn model(id: u64, mode: (u32, u32), arrangement: (f64, f64)) -> OutputModel {
        let desc = OutputDescriptor {
            key: crate::output::tests::key("X"),
            mode,
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        };
        OutputModel::from_descriptor(OutputId::for_test(id), &desc, arrangement)
    }

    /// The nearest point of the union of output rects, half-open; ties go to the first output.
    #[test]
    fn locate_clamps_to_the_union_of_output_rects() {
        let a = model(1, (100, 100), (0.0, 0.0));
        let b = model(2, (50, 50), (100.0, 0.0));
        let at = |p: (f64, f64)| locate(p, &a, std::slice::from_ref(&b));
        let eps = WL_FIXED_EPS;
        assert_eq!(
            at((120.0, 10.0)),
            PointerState::Located {
                output: b.id,
                p: (20.0, 10.0)
            }
        );
        assert_eq!(
            at((100.0, 10.0)),
            PointerState::Located {
                output: b.id,
                p: (0.0, 10.0)
            }
        );
        assert_eq!(
            at((120.0, 70.0)),
            PointerState::Located {
                output: a.id,
                p: (100.0 - eps, 70.0)
            }
        );
        assert_eq!(
            at((-5.0, -5.0)),
            PointerState::Located {
                output: a.id,
                p: (0.0, 0.0)
            }
        );
        assert_eq!(
            at((1e9, 10.0)),
            PointerState::Located {
                output: b.id,
                p: (50.0 - eps, 10.0)
            }
        );
        // far below-right, A's corner (100, 100) is nearer than B's (150, 50)
        assert_eq!(
            at((1e9, 1e9)),
            PointerState::Located {
                output: a.id,
                p: (100.0 - eps, 100.0 - eps)
            }
        );
        // equidistant from both rects (A's half-open edge is 100 − 2⁻⁸): the first in order
        let c = model(3, (100, 100), (200.0, 0.0));
        assert_eq!(
            locate((149.998046875, 200.0), &a, std::slice::from_ref(&c)),
            PointerState::Located {
                output: a.id,
                p: (100.0 - eps, 100.0 - eps)
            }
        );
    }

    /// F-LOCK-1: Canvas lock point is strictly derived from surface origin and local lock point.
    /// q_lock = surface_origin(S) + lock_point. Never stored.
    #[test]
    fn f_lock_1_derivation_is_exact_and_pure() {
        let origin = CanvasPoint::new(100, 200).unwrap();
        let local = LocalPoint { x: 15, y: -25 };
        let q_lock = derive_lock_point(origin, local);
        assert_eq!(
            q_lock,
            CanvasPos {
                x: 115 * Q16_ONE,
                y: 175 * Q16_ONE,
            }
        );

        let neg_origin = CanvasPoint::new(-500, -800).unwrap();
        let zero_local = LocalPoint { x: 0, y: 0 };
        assert_eq!(
            derive_lock_point(neg_origin, zero_local),
            CanvasPos {
                x: -500 * Q16_ONE,
                y: -800 * Q16_ONE,
            }
        );
    }

    /// OBJ-21: ConstraintState equality and properties.
    #[test]
    fn constraint_state_properties() {
        let s = crate::snapshot::tests::key(42);
        let c = ConstraintState {
            surface: s,
            kind: ConstraintKind::Lock,
            lifetime: ConstraintLifetime::Persistent,
            lock_point: LocalPoint { x: 10, y: 20 },
            region: None,
        };
        assert_eq!(c.kind, ConstraintKind::Lock);
        assert_eq!(c.lifetime, ConstraintLifetime::Persistent);
        assert_eq!(c.lock_point, LocalPoint { x: 10, y: 20 });
        assert_eq!(c.region, None);
    }

    /// M13 activation preconditions, lock motion isolation, and batch-end deactivation.
    #[test]
    fn m13_activation_and_reconciliation_policy() {
        use crate::geom::{LocalRect, LocalSize};
        use crate::navigation::PersistedCameras;
        use crate::output::Transform;
        use crate::protocol::Effect;
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, SurfaceTree};
        use crate::{Core, PhysicalEvent};

        let desc = OutputDescriptor {
            key: crate::output::tests::key("O1"),
            mode: (800, 600),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        };

        let (mut core, mut input_port, _, mut token) = Core::new(PersistedCameras::default());
        let (output_id, _) = core.output_connected(&mut token, desc).unwrap();

        // Create an element with a 400x300 surface
        let elem = core.toplevel_created();
        let skey = crate::snapshot::tests::key(1);
        let tree = SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size: LocalSize { w: 400, h: 300 },
            },
            surfaces: vec![SnapshotSurface {
                key: skey,
                role: SurfaceRole::Root,
                size: LocalSize { w: 400, h: 300 },
                input: InputRegion::Infinite,
                mapped: true,
            }],
        };

        let mut tree_unmapped = tree.clone();
        tree_unmapped.surfaces[0].mapped = false;
        tree_unmapped.surfaces[0].size = LocalSize::default();

        // Commit unmapped then mapped per M1
        let _ = core.toplevel_committed(elem, tree_unmapped).unwrap();
        let _ = core.toplevel_committed(elem, tree).unwrap();
        core.render_list(output_id).unwrap();

        // Startup pointer is at centre of output: (400, 300)
        // Hit test on the element should hit skey
        assert_eq!(
            core.pointer(),
            PointerState::Located {
                output: output_id,
                p: (400.0, 300.0)
            }
        );

        // 1. Request constraint without focus: must NOT activate (M13 precondition failed)
        let fx = core.request_pointer_constraint(
            skey,
            ConstraintKind::Lock,
            ConstraintLifetime::Persistent,
            None,
            None,
        );
        assert!(
            fx.is_empty(),
            "must not activate without ActiveElementFocus"
        );
        assert_eq!(core.constraint(), None);
        assert!(core.pending_constraint().is_some());

        // 2. Grant ActiveElementFocus: reconciliation at batch end activates pending constraint!
        let fx = core.set_focus(&mut input_port, Some(elem)).unwrap();
        assert!(
            fx.contains(&Effect::ConstraintActivate {
                surface: skey,
                kind: ConstraintKind::Lock
            }),
            "should activate once ActiveElementFocus is granted"
        );
        assert!(core.constraint().is_some());

        // 3. Pointer motion while locked delivers relative motion only; zero absolute motion
        let motion_ev = PhysicalEvent::PointerMotion {
            delta: (10.0, 5.0),
            delta_unaccel: (8.0, 4.0),
            time_us: 1000,
        };
        let fx = core.physical_input_vec(&mut input_port, motion_ev).unwrap();
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::PointerMotion { .. })),
            "no absolute motion while locked"
        );
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::PointerRelative { .. })),
            "relative motion delivered while locked"
        );
        // Physical pointer position on output does NOT move
        assert_eq!(
            core.pointer(),
            PointerState::Located {
                output: output_id,
                p: (400.0, 300.0)
            }
        );

        // 4. Focus loss deactivates constraint
        let fx = core.set_focus(&mut input_port, None).unwrap();
        assert_eq!(core.constraint(), None);
        assert!(
            fx.contains(&Effect::ConstraintDeactivate {
                surface: skey,
                kind: ConstraintKind::Lock
            }),
            "must emit ConstraintDeactivate on focus loss"
        );
    }

    /// M14 / F-CONFINE-1: Confinement clamps physical delta into region projection and inhibits Arrangement crossing.
    #[test]
    fn m14_confinement_clamping_and_arrangement_isolation() {
        use crate::geom::{LocalRect, LocalSize};
        use crate::navigation::PersistedCameras;
        use crate::output::Transform;
        use crate::protocol::Effect;
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, SurfaceTree};
        use crate::{Core, PhysicalEvent};

        let desc1 = OutputDescriptor {
            key: crate::output::tests::key("O1"),
            mode: (800, 600),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        };
        let desc2 = OutputDescriptor {
            key: crate::output::tests::key("O2"),
            mode: (800, 600),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        };

        let (mut core, mut input_port, _, mut token) = Core::new(PersistedCameras::default());
        let (o1, _) = core.output_connected(&mut token, desc1).unwrap();
        let (_o2, _) = core.output_connected(&mut token, desc2).unwrap();

        let elem = core.toplevel_created();
        let skey = crate::snapshot::tests::key(1);
        let tree = SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size: LocalSize { w: 400, h: 300 },
            },
            surfaces: vec![SnapshotSurface {
                key: skey,
                role: SurfaceRole::Root,
                size: LocalSize { w: 400, h: 300 },
                input: InputRegion::Infinite,
                mapped: true,
            }],
        };
        let mut tree_unmapped = tree.clone();
        tree_unmapped.surfaces[0].mapped = false;
        tree_unmapped.surfaces[0].size = LocalSize::default();

        let _ = core.toplevel_committed(elem, tree_unmapped).unwrap();
        let _ = core.toplevel_committed(elem, tree).unwrap();
        core.render_list(o1).unwrap();

        // Focus element
        let _ = core.set_focus(&mut input_port, Some(elem)).unwrap();

        // The 400x300 window is centred on the 800x600 view: anchor (−200, −150), so the startup
        // pointer (Canvas (0, 0)) is at surface-local (200, 150). Request confinement to the
        // 100x100 region whose corner is that point: Canvas [0, 100)², projected x up to 500.
        let region = LocalRect {
            loc: LocalPoint { x: 200, y: 150 },
            size: LocalSize { w: 100, h: 100 },
        };
        let fx = core.request_pointer_constraint(
            skey,
            ConstraintKind::Confine,
            ConstraintLifetime::Persistent,
            Some(region),
            None,
        );
        assert!(fx.contains(&Effect::ConstraintActivate {
            surface: skey,
            kind: ConstraintKind::Confine
        }));

        // Large physical delta to the right that would normally cross into O2:
        // Under confinement, it clamps within the region and stays on O1!
        let motion_ev = PhysicalEvent::PointerMotion {
            delta: (5000.0, 0.0),
            delta_unaccel: (5000.0, 0.0),
            time_us: 1000,
        };
        let fx = core.physical_input_vec(&mut input_port, motion_ev).unwrap();
        assert!(fx.iter().any(|e| matches!(e, Effect::PointerMotion { .. })));

        // Pointer must stay on O1 and be clamped
        match core.pointer() {
            PointerState::Located { output, p } => {
                assert_eq!(output, o1, "must NOT cross into O2 under confinement");
                assert!(p.0 <= 550.0, "p.x must be clamped to region projection");
            }
            PointerState::NoOutput => panic!("unexpected NoOutput"),
        }
    }

    /// INV-INPUT-5: Reconciliation is deferred while a client-owned grab sequence is active.
    #[test]
    fn m13_reconciliation_deferred_under_client_grab() {
        use crate::geom::{LocalRect, LocalSize};
        use crate::input::bindings::Modifiers;
        use crate::navigation::PersistedCameras;
        use crate::output::Transform;
        use crate::protocol::Effect;
        use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, SurfaceTree};
        use crate::{Core, PhysicalEvent};

        let desc = OutputDescriptor {
            key: crate::output::tests::key("O1"),
            mode: (800, 600),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        };

        let (mut core, mut input_port, _, mut token) = Core::new(PersistedCameras::default());
        let (o1, _) = core.output_connected(&mut token, desc).unwrap();

        let elem = core.toplevel_created();
        let skey = crate::snapshot::tests::key(1);
        let tree = SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size: LocalSize { w: 400, h: 300 },
            },
            surfaces: vec![SnapshotSurface {
                key: skey,
                role: SurfaceRole::Root,
                size: LocalSize { w: 400, h: 300 },
                input: InputRegion::Infinite,
                mapped: true,
            }],
        };
        let mut tree_unmapped = tree.clone();
        tree_unmapped.surfaces[0].mapped = false;
        tree_unmapped.surfaces[0].size = LocalSize::default();

        let _ = core.toplevel_committed(elem, tree_unmapped).unwrap();
        let _ = core.toplevel_committed(elem, tree).unwrap();
        core.render_list(o1).unwrap();

        let _ = core.set_focus(&mut input_port, Some(elem)).unwrap();
        let fx = core.request_pointer_constraint(
            skey,
            ConstraintKind::Lock,
            ConstraintLifetime::Persistent,
            None,
            None,
        );
        assert!(fx.contains(&Effect::ConstraintActivate {
            surface: skey,
            kind: ConstraintKind::Lock
        }));
        assert!(core.constraint().is_some());

        // Client presses button (implicit click grab begins)
        let btn_down = PhysicalEvent::PointerButton {
            button: 0x110,
            pressed: true,
            modifiers: Modifiers::default(),
            time_us: 1000,
        };
        let _ = core.physical_input_vec(&mut input_port, btn_down).unwrap();
        assert!(core.buttons().client_owns_any());

        // ActiveElementFocus moves to None while button is held:
        // Because client grab is active, reconciliation is DEFERRED!
        let fx = core.set_focus(&mut input_port, None).unwrap();
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::ConstraintDeactivate { .. }))
        );
        assert!(
            core.constraint().is_some(),
            "constraint must stay active while deferred"
        );

        // Client releases button: grab sequence ends -> reconciliation runs!
        let btn_up = PhysicalEvent::PointerButton {
            button: 0x110,
            pressed: false,
            modifiers: Modifiers::default(),
            time_us: 2000,
        };
        let fx = core.physical_input_vec(&mut input_port, btn_up).unwrap();
        assert!(
            fx.contains(&Effect::ConstraintDeactivate {
                surface: skey,
                kind: ConstraintKind::Lock
            }),
            "reconciliation at end of client grab deactivates constraint"
        );
        assert_eq!(core.constraint(), None);
    }
}
