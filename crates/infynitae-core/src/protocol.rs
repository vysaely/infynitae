//! Policy for events derived from untrusted Wayland client requests (C1.5 item 4): element
//! lifecycle (M1), popup placement targets (M3), and the `Effect` vocabulary. No camera
//! vocabulary: nothing here can mint an `Authorized` proof, so nothing here can mutate a camera
//! (INV-CAMERA-1).

use crate::Core;
use crate::geom::{self, CanvasPoint, CanvasPos, LocalPoint, LocalRect, LocalSize, Q16_ONE};
use crate::input::Axis;
use crate::output::{self, OutputId};
use crate::pointer;
use crate::snapshot::{SnapshotError, SurfaceKey, SurfaceTree};
use crate::stage::{ElementId, Lifecycle};

/// The surface a pointer event is routed to and its Canvas origin (B2.5): Smithay's focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerTarget {
    pub surface: SurfaceKey,
    pub origin: CanvasPoint,
}

/// Decisions the adapter applies to protocol objects (C2.1 `Effect` list), in order. Only core
/// constructs them (source rule `effects-only-from-core`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Send the toplevel's initial configure (M1).
    InitialConfigure(ElementId),
    /// Create the output's `wl_output` global from Core's OutputModel (M4).
    OutputAdded(OutputId),
    /// Re-announce the output's state from Core's OutputModel (M4 reconfiguration).
    OutputChanged(OutputId),
    /// Destroy the output's global; every leave for it precedes this effect (INV-OUTPUT-1).
    OutputRemoved(OutputId),
    /// `wl_surface.enter` (F-MEMBER-1, INV-RENDER-3).
    Enter {
        output: OutputId,
        surface: SurfaceKey,
    },
    /// `wl_surface.leave`.
    Leave {
        output: OutputId,
        surface: SurfaceKey,
    },
    /// Routed absolute motion at the pointer's true position (M10): Smithay `motion` with focus
    /// `target` and `location` in Canvas (INV-COORD-5). `target = None` clears the focus.
    PointerMotion {
        target: Option<PointerTarget>,
        location: CanvasPos,
        time_ms: u32,
    },
    /// Relative motion from a physical delta on the client path only (INV-INPUT-4). `target` is
    /// the routing of the same event (Smithay's pending focus).
    PointerRelative {
        target: Option<PointerTarget>,
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
    },
    /// A client-owned button (INV-INPUT-1).
    PointerButton {
        button: u32,
        pressed: bool,
        time_ms: u32,
    },
    /// A client-routed axis event.
    PointerAxis { axis: Axis, time_ms: u32 },
    /// `wl_pointer.frame`: ends the events of one physical event.
    PointerFrame,
    /// The last output is gone (M10 → NoOutput, EE-3): install the zero-output grab with
    /// `Focus::Clear` (one `wl_pointer.leave`, nothing delivered while installed).
    PointerZeroOutputBegin,
    /// The first output after NoOutput: remove the zero-output grab before the startup placement.
    PointerZeroOutputEnd,
    /// Change keyboard focus to `Option<ElementId>` (INV-INPUT-8).
    KeyboardFocus(Option<ElementId>),
    /// Send a configure to an xdg_toplevel, e.g. for `activated` state updates (INV-PROTO-1).
    ToplevelConfigure(ElementId),
    /// Dismiss active popup grab for an element before a camera change (INV-INPUT-2, OB-4).
    DismissPopupGrab(ElementId),
    /// A client-owned key event to forward to the focused client (K1, K2, K3).
    KeyboardForward {
        key: u32,
        pressed: bool,
        mods_changed: bool,
        time_us: u64,
    },
    /// A compositor-owned key event intercepted and consumed by the compositor.
    KeyboardIntercept { key: u32 },
    /// Activate a pointer constraint (zwp_pointer_constraints_v1) on `surface` (M13).
    ConstraintActivate {
        surface: SurfaceKey,
        kind: crate::pointer::ConstraintKind,
    },
    /// Deactivate a pointer constraint on `surface` (M13, W-SM-4a).
    ConstraintDeactivate {
        surface: SurfaceKey,
        kind: crate::pointer::ConstraintKind,
    },
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
        let mut fx = match (state, has_buffer) {
            (Lifecycle::AwaitingInitialCommit | Lifecycle::Unmapped, true) => {
                return Err(ProtocolError::BufferBeforeConfigure);
            }
            (Lifecycle::AwaitingInitialCommit, false) => {
                let anchor = self.initial_anchor();
                self.stage.place(id, anchor).map_err(stale)?;
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::AwaitingFirstBuffer)
                    .map_err(stale)?;
                vec![Effect::InitialConfigure(id)]
            }
            (Lifecycle::Unmapped, false) => {
                // anchor kept (INV-STAGE-5)
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::AwaitingFirstBuffer)
                    .map_err(stale)?;
                vec![Effect::InitialConfigure(id)]
            }
            (Lifecycle::AwaitingFirstBuffer, false) | (Lifecycle::Mapped, true) => {
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                vec![]
            }
            (Lifecycle::AwaitingFirstBuffer, true) => {
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage.set_state(id, Lifecycle::Mapped).map_err(stale)?;
                self.stage.raise(id);
                vec![]
            }
            (Lifecycle::Mapped, false) => {
                // null-buffer commit: unmap; identity and anchor kept, snapshot dropped
                self.stage.set_tree(id, None).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::Unmapped)
                    .map_err(stale)?;
                let live: Vec<ElementId> = self
                    .stage
                    .mapped_elements()
                    .into_iter()
                    .filter(|&e| e != id)
                    .collect();
                self.focus.target_lost(id, &live)
            }
        };
        fx.extend(self.visibility_pass());
        fx.extend(self.reconcile_constraints());
        Ok(fx)
    }

    /// A change of a mapped element's surface tree that is NOT a commit of its root surface: a
    /// child surface (popup, desynchronised subsurface) committed, or a popup was destroyed. It
    /// replaces the snapshot of a Mapped element (hit-testing and rendering follow, INV-HIT-1) and
    /// never drives M1: only the root surface's own commit (`toplevel_committed`) may map, unmap
    /// or configure an element. An element that is not Mapped has no hit-testable tree, so the
    /// change is ignored until its root commits. A rejected tree changes nothing.
    pub fn toplevel_tree_changed(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
    ) -> Result<Vec<Effect>, ProtocolError> {
        tree.validate().map_err(ProtocolError::Snapshot)?;
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        // The root's mapped state is committed state of the root surface; only its own commit
        // changes it, so a tree without a mapped root is never applied here.
        if state != Lifecycle::Mapped || !tree.root().is_some_and(|r| r.mapped) {
            return Ok(Vec::new());
        }
        self.stage
            .set_tree(id, Some(tree))
            .map_err(|_| ProtocolError::StaleElement)?;
        let mut fx = self.visibility_pass();
        fx.extend(self.reconcile_constraints());
        Ok(fx)
    }

    /// M1 destroy: purges every reference; idempotent and order-independent (INV-STAGE-4). The
    /// dead client gets no leave: its membership rows are dropped silently. Target loss triggers
    /// successor focus transition (K7, M11).
    pub fn toplevel_destroyed(&mut self, id: ElementId) -> Vec<Effect> {
        for (_, frame) in &mut self.frames {
            frame.entries.retain(|e| e.element != id);
        }
        let existed = self.stage.destroy(id);
        if !existed {
            return Vec::new();
        }
        let live: Vec<ElementId> = self.stage.mapped_elements();
        let mut fx = self.focus.target_lost(id, &live);
        let vis = self.visibility_pass();
        debug_assert!(
            vis.is_empty(),
            "a destroyed element produced visibility effects"
        );
        fx.extend(vis);
        fx.extend(self.reconcile_constraints());
        fx
    }

    /// M3: the constraint target for a popup of `parent`: the anchor output's viewport,
    /// inward-rounded (R8), relative to the parent's window-geometry origin. The anchor output is
    /// (D-B6-BOUNDARY):
    /// - Rule 1: output on which causing input event occurred, if parent is visible on it;
    /// - Rule 2: else parent's frame-clock output (highest refresh observing output);
    /// - Rule 3: else none (`None`: no constraint adjustment).
    ///
    /// `None` also when the target does not fit protocol integers.
    pub fn popup_constraint_target(
        &self,
        parent: ElementId,
        causing_output: Option<OutputId>,
    ) -> Option<LocalRect> {
        let anchor = self.stage.anchor(parent)?;
        let output = match causing_output {
            Some(o) if self.membership.observers(parent).contains(&o) => o,
            _ => self.clock_output_of(parent)?,
        };
        let view = self.view(output)?;
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
    /// connected output (lowest `OutputId`), rounded R3; the Canvas origin without an output.
    fn initial_anchor(&self) -> CanvasPoint {
        self.outputs
            .first()
            .and_then(|o| self.cameras.camera_for_output(o.id))
            .and_then(|c| self.cameras.center(c))
            .and_then(|c| c.round_to_point())
            .unwrap_or(CanvasPoint { x: 0, y: 0 })
    }

    /// Requests a pointer constraint from client protocol (M13, OBJ-21).
    /// If preconditions are met, activates immediately; otherwise recorded as pending.
    pub fn request_pointer_constraint(
        &mut self,
        surface: SurfaceKey,
        kind: crate::pointer::ConstraintKind,
        lifetime: crate::pointer::ConstraintLifetime,
        region: Option<LocalRect>,
        cursor_position_hint: Option<LocalPoint>,
    ) -> Vec<Effect> {
        let lock_point = match cursor_position_hint {
            Some(hint) => hint,
            None => {
                let hit_local = self.route_pointer_hit_local(surface);
                hit_local.unwrap_or_default()
            }
        };
        let candidate = crate::pointer::ConstraintState {
            surface,
            kind,
            lifetime,
            lock_point,
            region,
        };

        if let Some(cur) = self.constraint {
            if cur.surface == surface && cur.kind == kind {
                return Vec::new();
            }
        }

        self.pending_constraint = Some(candidate);

        if self.can_activate_constraint(&candidate) {
            let mut fx = Vec::new();
            if let Some(prev) = self.constraint {
                if prev.surface != surface || prev.kind != kind {
                    fx.push(Effect::ConstraintDeactivate {
                        surface: prev.surface,
                        kind: prev.kind,
                    });
                }
            }
            self.constraint = Some(candidate);
            if candidate.lifetime == crate::pointer::ConstraintLifetime::Oneshot {
                self.pending_constraint = None;
            }
            fx.push(Effect::ConstraintActivate { surface, kind });
            fx
        } else {
            Vec::new()
        }
    }

    /// Sets or updates the cursor position hint of a locked pointer constraint.
    pub fn set_cursor_position_hint(&mut self, surface: SurfaceKey, hint: LocalPoint) {
        if let Some(c) = &mut self.constraint {
            if c.surface == surface && c.kind == crate::pointer::ConstraintKind::Lock {
                c.lock_point = hint;
            }
        }
        if let Some(c) = &mut self.pending_constraint {
            if c.surface == surface && c.kind == crate::pointer::ConstraintKind::Lock {
                c.lock_point = hint;
            }
        }
    }

    /// Client destroys a pointer constraint (M13).
    pub fn destroy_pointer_constraint(&mut self, surface: SurfaceKey) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(c) = self.constraint {
            if c.surface == surface {
                self.constraint = None;
                // W-SM-4a: deactivate only if delivered focus does not change in this batch
                let delivered_focus = self
                    .route_pointer()
                    .and_then(|(tgt, _)| tgt)
                    .map(|t| t.surface);
                if delivered_focus == Some(surface) {
                    fx.push(Effect::ConstraintDeactivate {
                        surface,
                        kind: c.kind,
                    });
                }
            }
        }
        if let Some(c) = self.pending_constraint {
            if c.surface == surface {
                self.pending_constraint = None;
            }
        }
        fx
    }

    /// M13 batch-end reconciliation pass (INV-INPUT-5).
    /// Runs once at the end of every core batch where view/descriptor, geometry/origin,
    /// output set, ActiveElementFocus, or an interaction/grab ended.
    pub fn reconcile_constraints(&mut self) -> Vec<Effect> {
        // Deferral rule: deferred while compositor pointer interaction or client-owned grab sequence is active.
        if self.input.interaction.active() || self.input.ownership.client_owns_any() {
            return Vec::new();
        }

        let mut fx = Vec::new();

        // 1. Check active constraint, if any
        if let Some(c) = self.constraint {
            let still_valid = self.is_constraint_still_valid(&c);
            if !still_valid {
                self.constraint = None;
                if c.lifetime == crate::pointer::ConstraintLifetime::Oneshot {
                    self.pending_constraint = None;
                }

                // W-SM-4a: explicit deactivate ONLY if delivered focus does not change in this batch;
                // otherwise Smithay's leave() does it.
                let delivered_focus = self
                    .route_pointer()
                    .and_then(|(tgt, _)| tgt)
                    .map(|t| t.surface);
                if delivered_focus == Some(c.surface) {
                    fx.push(Effect::ConstraintDeactivate {
                        surface: c.surface,
                        kind: c.kind,
                    });
                }
            } else if c.kind == crate::pointer::ConstraintKind::Lock {
                // Lock is maintained. Update physical pointer to derived lock point.
                if let Some(p_lock) = self.derived_lock_output_point(&c) {
                    if let pointer::PointerState::Located { output, .. } = self.pointer {
                        self.pointer = pointer::PointerState::Located {
                            output,
                            p: (p_lock.x, p_lock.y),
                        };
                    }
                }
            }
        }

        // 2. If no constraint is active, check if pending constraint can activate
        if self.constraint.is_none() {
            if let Some(pending) = self.pending_constraint {
                if self.can_activate_constraint(&pending) {
                    self.constraint = Some(pending);
                    if pending.lifetime == crate::pointer::ConstraintLifetime::Oneshot {
                        self.pending_constraint = None;
                    }
                    fx.push(Effect::ConstraintActivate {
                        surface: pending.surface,
                        kind: pending.kind,
                    });
                }
            }
        }

        fx
    }

    /// Surface-local point if pointer currently hits `surface`.
    fn route_pointer_hit_local(&mut self, surface: SurfaceKey) -> Option<LocalPoint> {
        let pointer::PointerState::Located { output, p } = self.pointer else {
            return None;
        };
        let pt = geom::OutputLogicalPoint { x: p.0, y: p.1 };
        let hit = self.hit_test(output, pt)?;
        if hit.surface == surface {
            Some(LocalPoint {
                x: (hit.local.x / geom::Q16_ONE) as i32,
                y: (hit.local.y / geom::Q16_ONE) as i32,
            })
        } else {
            None
        }
    }

    /// M13 activation precondition guard:
    /// [ActiveElementFocus = element(S) ∧ pointer focus S ∧ inside region ∧ no acquisition]
    fn can_activate_constraint(&mut self, c: &crate::pointer::ConstraintState) -> bool {
        // 1. no compositor pointer interaction or client-owned grab being acquired
        if self.input.interaction.active() || self.input.ownership.client_owns_any() {
            return false;
        }

        // 2. ActiveElementFocus == element(S)
        let Some(elem) = self.stage.element_containing_surface(c.surface) else {
            return false;
        };
        if self.focus.target() != Some(elem) {
            return false;
        }

        // 3. Pointer focus on S
        let Some(local_pt) = self.route_pointer_hit_local(c.surface) else {
            return false;
        };

        // 4. Pointer inside region
        if let Some(r) = c.region {
            if !r.contains(local_pt.x as i64, local_pt.y as i64) {
                return false;
            }
        }

        true
    }

    /// Checks if an active constraint remains valid at batch end.
    fn is_constraint_still_valid(&mut self, c: &crate::pointer::ConstraintState) -> bool {
        let Some(elem) = self.stage.element_containing_surface(c.surface) else {
            return false;
        };
        if self.focus.target() != Some(elem) {
            return false;
        }

        match c.kind {
            crate::pointer::ConstraintKind::Lock => {
                let pointer::PointerState::Located { output, .. } = self.pointer else {
                    return false;
                };
                let Some(model) = self.output(output) else {
                    return false;
                };
                let Some(view) = self.view(output) else {
                    return false;
                };
                let Some(origin) = self.surface_origin(c.surface) else {
                    return false;
                };
                let q_lock = crate::pointer::derive_lock_point(origin, c.lock_point);
                let p_lock = view.project(q_lock);
                let extent = model.logical_extent();
                p_lock.x >= 0.0 && p_lock.y >= 0.0 && p_lock.x < extent.0 && p_lock.y < extent.1
            }
            crate::pointer::ConstraintKind::Confine => {
                let Some(local_pt) = self.route_pointer_hit_local(c.surface) else {
                    return false;
                };
                if let Some(r) = c.region {
                    if !r.contains(local_pt.x as i64, local_pt.y as i64) {
                        return false;
                    }
                }
                true
            }
        }
    }

    fn derived_lock_output_point(
        &mut self,
        c: &crate::pointer::ConstraintState,
    ) -> Option<geom::OutputLogicalPoint> {
        let pointer::PointerState::Located { output, .. } = self.pointer else {
            return None;
        };
        let origin = self.surface_origin(c.surface)?;
        let q_lock = crate::pointer::derive_lock_point(origin, c.lock_point);
        let view = self.view(output)?;
        Some(view.project(q_lock))
    }

    /// Routes motion when a pointer lock is active: zero absolute motion to the locked surface;
    /// relative motion emitted from physical delta only.
    pub(crate) fn locked_motion(
        &mut self,
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
    ) -> Vec<Effect> {
        let target = self.route_pointer().and_then(|(tgt, _)| tgt);
        vec![
            Effect::PointerRelative {
                target,
                delta,
                delta_unaccel,
                time_us,
            },
            Effect::PointerFrame,
        ]
    }

    /// F-CONFINE-1: Clamps physical pointer half-open to project_O(region) ∩ [0, logical_O).
    /// No Arrangement crossing.
    pub(crate) fn confined_motion(
        &mut self,
        output: output::OutputId,
        p: (f64, f64),
        delta: (f64, f64),
        c: &crate::pointer::ConstraintState,
    ) -> pointer::PointerState {
        let (Some(model), Some(view), Some(origin)) = (
            self.output(output),
            self.view(output),
            self.surface_origin(c.surface),
        ) else {
            return pointer::PointerState::Located { output, p };
        };

        let rect = c.region.unwrap_or_else(|| {
            let size = self
                .stage
                .tree(self.stage.element_containing_surface(c.surface).unwrap())
                .and_then(|t| t.surfaces.iter().find(|s| s.key == c.surface))
                .map(|s| s.size)
                .unwrap_or_default();
            LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size,
            }
        });

        let x0 = (origin.x + rect.loc.x as i64) * geom::Q16_ONE;
        let y0 = (origin.y + rect.loc.y as i64) * geom::Q16_ONE;
        let x1 = (origin.x + rect.loc.x as i64 + rect.size.w as i64) * geom::Q16_ONE;
        let y1 = (origin.y + rect.loc.y as i64 + rect.size.h as i64) * geom::Q16_ONE;

        let p0 = view.project(geom::CanvasPos { x: x0, y: y0 });
        let p1 = view.project(geom::CanvasPos { x: x1, y: y1 });

        let rx_min = p0.x.min(p1.x);
        let rx_max = p0.x.max(p1.x);
        let ry_min = p0.y.min(p1.y);
        let ry_max = p0.y.max(p1.y);

        let extent = model.logical_extent();
        let min_x = rx_min.max(0.0);
        let max_x = rx_max.min(extent.0);
        let min_y = ry_min.max(0.0);
        let max_y = ry_max.min(extent.1);

        let clamped_x = (p.0 + delta.0).clamp(min_x, (max_x - pointer::WL_FIXED_EPS).max(min_x));
        let clamped_y = (p.1 + delta.1).clamp(min_y, (max_y - pointer::WL_FIXED_EPS).max(min_y));

        pointer::PointerState::Located {
            output,
            p: (clamped_x, clamped_y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{OutputDescriptor, Transform};
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
        let (mut core, _i, _s, mut token) =
            Core::new(crate::navigation::PersistedCameras::default());
        core.output_connected(
            &mut token,
            OutputDescriptor {
                key: crate::output::tests::key("A"),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap();
        core
    }

    /// B1: a surface-tree change that is not a root commit never drives M1 — in every lifecycle
    /// state the state, anchor and effects are those of a no-op, except that a Mapped element's
    /// snapshot is replaced (literal expectations, independent of `toplevel_committed`).
    #[test]
    fn tree_change_never_drives_the_lifecycle() {
        let mut core = connected(120);
        let id = core.toplevel_created();
        // AwaitingInitialCommit: no placement, no configure
        assert_eq!(core.toplevel_tree_changed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        assert_eq!(core.stage().anchor(id), None);
        core.toplevel_committed(id, tree(false)).unwrap();
        // AwaitingFirstBuffer: a mapped-looking tree does not map it
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::AwaitingFirstBuffer));
        core.toplevel_committed(id, tree(true)).unwrap();
        // Mapped: an unmapped-looking tree does not unmap it; a mapped tree replaces the snapshot
        assert_eq!(core.toplevel_tree_changed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        assert!(core.stage().tree(id).is_some());
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        core.toplevel_committed(id, tree(false)).unwrap();
        // Unmapped: no re-arming of the initial configure, and the snapshot stays dropped even for
        // a mapped-looking tree (M1: an unmapped element has no hit-testable tree)
        assert_eq!(core.toplevel_tree_changed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Unmapped));
        assert!(core.stage().tree(id).is_none());
        // a destroyed element is stale
        core.toplevel_destroyed(id);
        assert_eq!(
            core.toplevel_tree_changed(id, tree(true)),
            Err(ProtocolError::StaleElement)
        );
    }

    #[test]
    fn m1_map_unmap_remap_destroy() {
        let mut core = connected(120);
        let o = core.outputs()[0].id;
        let enter = Effect::Enter {
            output: o,
            surface: key(1),
        };
        let leave = Effect::Leave {
            output: o,
            surface: key(1),
        };
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
        // first buffer: mapped, on top, and its surface enters the output (F-MEMBER-1)
        assert_eq!(core.toplevel_committed(id, tree(true)), Ok(vec![enter]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        assert_eq!(core.stage().z_order(), &[id]);
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Ok(vec![]),
            "no repeated enter"
        );
        // unmap keeps id and anchor, drops the snapshot, and leaves every output (M1)
        assert_eq!(core.toplevel_committed(id, tree(false)), Ok(vec![leave]));
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
        assert_eq!(core.toplevel_committed(id, tree(true)), Ok(vec![enter]));
        assert_eq!(core.stage().anchor(id), Some(anchor));
        assert_eq!(
            core.stage().z_order(),
            &[other, id],
            "remap re-shows on top"
        );
        // destroy: no event to the dead client; membership rows dropped
        let _fx = core.toplevel_destroyed(id);
        assert!(!core.membership().rows().iter().any(|(e, _, _)| *e == id));
        assert!(
            core.toplevel_destroyed(id).is_empty(),
            "destroy is idempotent"
        );
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

    /// M3 / R8 / D-B6-BOUNDARY: the target is the anchor output's viewport (here the parent's
    /// clock output), inward-rounded, relative to the parent's geometry origin (the anchor).
    /// Expected values are hand-derived with exact rational arithmetic
    /// (evidence/phase-f/INC-1/corrective/unproject_reference.py), not with production code.
    #[test]
    fn popup_target_is_the_viewport_inward_rounded() {
        // k = 1, camera centre (0,0), 1920x1080 → viewport [−960,960) × [−540,540); anchor (0,0)
        let mut core = connected(120);
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        assert_eq!(
            core.popup_constraint_target(id, None),
            None,
            "unobserved parent: no anchor output (rule 3)"
        );
        core.toplevel_committed(id, tree(true)).unwrap();
        let t = core.popup_constraint_target(id, None).unwrap();
        assert_eq!(
            (t.loc, t.size),
            (
                LocalPoint { x: -960, y: -540 },
                LocalSize { w: 1920, h: 1080 }
            )
        );
        // k = 1.75: c_eff = (−35951177, −20222537) Q16; the viewport's fractional edges
        // (−548.57…, 548.57…) × (−308.57…, 308.57…) round inward
        let mut core = connected(210);
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        let t = core.popup_constraint_target(id, None).unwrap();
        assert_eq!(
            (t.loc, t.size),
            (
                LocalPoint { x: -548, y: -308 },
                LocalSize { w: 1096, h: 616 }
            )
        );
        core.toplevel_destroyed(id);
        assert_eq!(
            core.popup_constraint_target(id, None),
            None,
            "a destroyed parent has no target"
        );
    }

    #[test]
    fn popup_target_causing_output_rule1_and_rule2_fallback() {
        use crate::navigation::PersistedCameras;
        use crate::output::tests::key as okey;

        // Setup outputs: o1, o2, and o3 (with far-away camera)
        let persisted = PersistedCameras::from_entries(vec![(
            okey("out3"),
            crate::geom::CanvasPos {
                x: 100_000 << 16,
                y: 100_000 << 16,
            },
            1.0,
        )])
        .unwrap();
        let (mut core, _, _, mut tok) = Core::new(persisted);
        let (o1, _) = core
            .output_connected(
                &mut tok,
                OutputDescriptor {
                    key: okey("out1"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        let (o2, _) = core
            .output_connected(
                &mut tok,
                OutputDescriptor {
                    key: okey("out2"),
                    mode: (800, 600),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 120_000, // higher refresh -> would be clock output (rule 2)
                },
            )
            .unwrap();
        let (o3, _) = core
            .output_connected(
                &mut tok,
                OutputDescriptor {
                    key: okey("out3"),
                    mode: (640, 480),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 30_000,
                },
            )
            .unwrap();

        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();

        // If parent is visible on both o1 and o2:
        // Rule 1: causing_output = o1 selects o1, even though o2 has higher refresh!
        if core.membership().observers(id).contains(&o1)
            && core.membership().observers(id).contains(&o2)
        {
            let t_o1 = core.popup_constraint_target(id, Some(o1)).unwrap();
            let t_o2 = core.popup_constraint_target(id, Some(o2)).unwrap();
            assert_eq!(t_o1.size, LocalSize { w: 1920, h: 1080 });
            assert_eq!(t_o2.size, LocalSize { w: 800, h: 600 });
        }

        // Rule 2 fallback: None selects clock output (o2)
        if core.membership().observers(id).contains(&o2) {
            let t_default = core.popup_constraint_target(id, None).unwrap();
            assert_eq!(t_default.size, LocalSize { w: 800, h: 600 });
        }

        // Rule 1 visibility guard (ATK-12 / Mutation P): o3 does not observe parent id.
        // Therefore passing Some(o3) must NOT select o3 (640x480), but fall back to Rule 2 (o2, 800x600)!
        assert!(!core.membership().observers(id).contains(&o3));
        let t_unobserved = core.popup_constraint_target(id, Some(o3)).unwrap();
        assert_eq!(t_unobserved.size, LocalSize { w: 800, h: 600 });
    }
}
