//! Policy for events derived from untrusted Wayland client requests (C1.5 item 4): element
//! lifecycle (M1) and popup placement targets (M3). No camera vocabulary: nothing here can mint an
//! `Authorized` proof, so nothing here can mutate a camera (INV-CAMERA-1).

use crate::Core;
use crate::geom::{CanvasPoint, LocalPoint, LocalRect, LocalSize, Q16_ONE};
use crate::snapshot::{SnapshotError, SurfaceTree};
use crate::stage::{ElementId, Lifecycle};

/// Decisions the adapter applies to protocol objects (C2.1 `Effect` list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Send the toplevel's initial configure (M1).
    InitialConfigure(ElementId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    /// The element does not exist (destroyed or never created).
    StaleElement,
    /// The snapshot is structurally invalid; it was not stored.
    Snapshot(SnapshotError),
    /// A buffer was committed before the initial configure (M1: `ensure_configured` → protocol
    /// error, W-SM-6).
    BufferBeforeConfigure,
}

impl Core {
    /// M1 `new_toplevel`: mints an `ElementId`; no anchor yet.
    pub fn toplevel_created(&mut self) -> ElementId {
        self.stage.create()
    }

    /// A committed surface-tree state of an element (written wholesale per commit, OBJ-05).
    /// Drives M1 from the root surface's buffer state. A rejected commit changes nothing.
    pub fn toplevel_committed(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
    ) -> Result<Vec<Effect>, ProtocolError> {
        tree.validate().map_err(ProtocolError::Snapshot)?;
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        let has_buffer = tree.root().is_some_and(|r| r.mapped);
        let stale = |_| ProtocolError::StaleElement;
        match (state, has_buffer) {
            (Lifecycle::AwaitingInitialCommit | Lifecycle::Unmapped, true) => {
                Err(ProtocolError::BufferBeforeConfigure)
            }
            (Lifecycle::AwaitingInitialCommit, false) => {
                let anchor = self.initial_anchor();
                self.stage.place(id, anchor).map_err(stale)?;
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::AwaitingFirstBuffer)
                    .map_err(stale)?;
                Ok(vec![Effect::InitialConfigure(id)])
            }
            (Lifecycle::Unmapped, false) => {
                // anchor kept (INV-STAGE-5)
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::AwaitingFirstBuffer)
                    .map_err(stale)?;
                Ok(vec![Effect::InitialConfigure(id)])
            }
            (Lifecycle::AwaitingFirstBuffer, false) | (Lifecycle::Mapped, true) => {
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                Ok(vec![])
            }
            (Lifecycle::AwaitingFirstBuffer, true) => {
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage.set_state(id, Lifecycle::Mapped).map_err(stale)?;
                self.stage.raise(id);
                Ok(vec![])
            }
            (Lifecycle::Mapped, false) => {
                // null-buffer commit: unmap; identity and anchor kept, snapshot dropped
                self.stage.set_tree(id, None).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::Unmapped)
                    .map_err(stale)?;
                Ok(vec![])
            }
        }
    }

    /// M1 destroy: purges every reference; idempotent and order-independent (INV-STAGE-4).
    pub fn toplevel_destroyed(&mut self, id: ElementId) -> bool {
        for (_, frame) in &mut self.frames {
            frame.entries.retain(|e| e.element != id);
        }
        self.stage.destroy(id)
    }

    /// M3: the constraint target for a popup of `parent`: the anchor output's viewport,
    /// inward-rounded (R8), relative to the parent's window-geometry origin. INC-1 has one output,
    /// which is the anchor output. `None` without an output, an anchor, or when the target does
    /// not fit protocol integers.
    pub fn popup_constraint_target(&self, parent: ElementId) -> Option<LocalRect> {
        let anchor = self.stage.anchor(parent)?;
        let output = self.outputs.first()?;
        let view = self.view(output.id)?;
        let extent = |px: u32| crate::geom::q16(px as f64 / view.k);
        let (x0q, y0q) = (view.c_eff.x, view.c_eff.y);
        let (x1q, y1q) = (x0q + extent(view.size.0), y0q + extent(view.size.1));
        // R8: ceiling at the minimum edges, floor at the maximum edges
        let ceil = |v: i64| -(-v).div_euclid(Q16_ONE);
        let floor = |v: i64| v.div_euclid(Q16_ONE);
        let to_i32 = |v: i64| i32::try_from(v).ok();
        let x0 = to_i32(ceil(x0q) - anchor.x)?;
        let y0 = to_i32(ceil(y0q) - anchor.y)?;
        let x1 = to_i32(floor(x1q) - anchor.x)?;
        let y1 = to_i32(floor(y1q) - anchor.y)?;
        Some(LocalRect {
            loc: LocalPoint { x: x0, y: y0 },
            size: LocalSize {
                w: x1.checked_sub(x0)?.max(0),
                h: y1.checked_sub(y0)?.max(0),
            },
        })
    }

    /// Placement policy at initial commit (Level D): the bound camera's centre of the first
    /// output, rounded R3; the Canvas origin without an output.
    fn initial_anchor(&self) -> CanvasPoint {
        self.outputs
            .first()
            .and_then(|o| self.cameras.camera_for_output(o.id))
            .and_then(|c| self.cameras.center(c))
            .and_then(|c| c.round_to_point())
            .unwrap_or(CanvasPoint { x: 0, y: 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::OutputLogicalPoint;
    use crate::output::{OutputDescriptor, OutputKey, Transform};
    use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, tests::key};

    fn tree(mapped: bool) -> SurfaceTree {
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size: LocalSize { w: 100, h: 50 },
            },
            surfaces: vec![SnapshotSurface {
                key: key(1),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w: 100, h: 50 }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
            }],
        }
    }

    fn connected(scale_120: u32) -> Core {
        let (mut core, _i, _s, mut token) = Core::new();
        core.output_connected(
            &mut token,
            OutputDescriptor {
                key: OutputKey::opaque(),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap();
        core
    }

    #[test]
    fn m1_map_unmap_remap_destroy() {
        let mut core = connected(120);
        let id = core.toplevel_created();
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        assert_eq!(core.stage().anchor(id), None);
        assert_eq!(
            core.toplevel_committed(id, tree(false)),
            Ok(vec![Effect::InitialConfigure(id)])
        );
        let anchor = core.stage().anchor(id).expect("placed at initial commit");
        assert_eq!(anchor, CanvasPoint { x: 0, y: 0 }, "default camera centre");
        assert_eq!(core.toplevel_committed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.toplevel_committed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        assert_eq!(core.stage().z_order(), &[id]);
        // unmap keeps id and anchor, drops the snapshot
        assert_eq!(core.toplevel_committed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Unmapped));
        assert_eq!(core.stage().anchor(id), Some(anchor));
        assert!(core.stage().tree(id).is_none());
        // remap: initial commit again, anchor kept, on top
        let other = core.toplevel_created();
        core.toplevel_committed(other, tree(false)).unwrap();
        core.toplevel_committed(other, tree(true)).unwrap();
        assert_eq!(
            core.toplevel_committed(id, tree(false)),
            Ok(vec![Effect::InitialConfigure(id)])
        );
        core.toplevel_committed(id, tree(true)).unwrap();
        assert_eq!(core.stage().anchor(id), Some(anchor));
        assert_eq!(
            core.stage().z_order(),
            &[other, id],
            "remap re-shows on top"
        );
        assert!(core.toplevel_destroyed(id));
        assert!(!core.toplevel_destroyed(id), "destroy is idempotent");
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Err(ProtocolError::StaleElement)
        );
    }

    #[test]
    fn m1_illegal_transitions_are_rejected_without_mutation() {
        let mut core = connected(120);
        let id = core.toplevel_created();
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Err(ProtocolError::BufferBeforeConfigure)
        );
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        assert_eq!(core.stage().anchor(id), None);
        let mut bad = tree(false);
        bad.surfaces.clear();
        assert!(matches!(
            core.toplevel_committed(id, bad),
            Err(ProtocolError::Snapshot(_))
        ));
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        // after unmap, a buffer before the new initial configure is rejected too
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        core.toplevel_committed(id, tree(false)).unwrap();
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Err(ProtocolError::BufferBeforeConfigure)
        );
        assert_eq!(core.stage().state(id), Some(Lifecycle::Unmapped));
    }

    /// Protocol activity never touches cameras (INV-CAMERA-1).
    #[test]
    fn protocol_events_never_move_cameras() {
        let mut core = connected(150);
        let before = (
            core.cameras().center(0),
            core.cameras().zoom(0),
            core.cameras().len(),
        );
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        core.toplevel_destroyed(id);
        assert_eq!(
            before,
            (
                core.cameras().center(0),
                core.cameras().zoom(0),
                core.cameras().len()
            )
        );
    }

    /// M3 / R8: the target is the output viewport, inward-rounded, relative to the parent's
    /// geometry origin (the anchor).
    #[test]
    fn popup_target_is_the_viewport_inward_rounded() {
        let mut core = connected(120);
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        // camera centre (0,0), 1920x1080 at k=1 → viewport [−960,960) × [−540,540); anchor (0,0)
        let t = core.popup_constraint_target(id).unwrap();
        assert_eq!(t.loc, LocalPoint { x: -960, y: -540 });
        assert_eq!(t.size, LocalSize { w: 1920, h: 1080 });
        // at k = 1.25 the viewport is 1536 x 864 Canvas px; at k = 1.75, 1097.14… x 617.14…:
        // fractional edges round inward
        let mut core = connected(210);
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        let t = core.popup_constraint_target(id).unwrap();
        let v = core.view(core.outputs()[0].id).unwrap();
        let lo = v.unproject(OutputLogicalPoint { x: 0.0, y: 0.0 });
        assert!(t.loc.x as i64 * Q16_ONE >= lo.x && t.loc.y as i64 * Q16_ONE >= lo.y);
        assert!((t.loc.x + t.size.w) as f64 <= lo.x as f64 / Q16_ONE as f64 + 1920.0 / 1.75);
        assert!((t.loc.y + t.size.h) as f64 <= lo.y as f64 / Q16_ONE as f64 + 1080.0 / 1.75);
        core.toplevel_destroyed(id);
        assert_eq!(
            core.popup_constraint_target(id),
            None,
            "a destroyed parent has no target"
        );
    }
}
