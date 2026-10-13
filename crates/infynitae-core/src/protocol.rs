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

/// Where the placement policy puts an ordinary toplevel (INV-STAGE-1 writer (a), C12.F1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlacementPolicy {
    /// The window-geometry origin at the placement output's camera centre, decided at the
    /// initial commit and never revised: the rule of INC-1…INC-5. Kept as an explicit policy.
    OriginAtCameraCentre,
    /// F-PLACE-1, the default: the window geometry centred in the placement output's usable
    /// region, resolved once at the first map; an oversized window aligned to the region's
    /// origin.
    #[default]
    CentredInView,
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
                // First map only: the size is known now, so the placement policy can use it.
                // A remap keeps its anchor (INV-STAGE-5).
                if !self.stage.mapped_before(id)
                    && let Some(anchor) = self.first_map_anchor(&tree)
                {
                    self.stage.place(id, anchor).map_err(stale)?;
                }
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
        fx.extend(self.visibility_pass_element(id));
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
        let mut fx = self.visibility_pass_element(id);
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
        let vis = self.visibility_pass_element(id);
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

    /// The anchor written at the initial commit (INV-STAGE-1, C12.F1): the bound camera's centre
    /// of the first connected output (lowest `OutputId`), rounded R3; the Canvas origin without
    /// an output. Under the default policy it is provisional until the first map; under
    /// [`PlacementPolicy::OriginAtCameraCentre`] it is final.
    fn initial_anchor(&self) -> CanvasPoint {
        self.outputs
            .first()
            .and_then(|o| self.cameras.camera_for_output(o.id))
            .and_then(|c| self.cameras.center(c))
            .and_then(|c| c.round_to_point())
            .unwrap_or(CanvasPoint { x: 0, y: 0 })
    }

    /// Default placement at first map, when the window-geometry size is known (F-PLACE-1).
    ///
    /// [`PlacementPolicy::CentredInView`]: the window geometry is centred in the usable region of
    /// the placement output (the first connected output, as for [`Core::initial_anchor`]), in
    /// Canvas coordinates through that output's camera: per axis, with `c` the camera centre,
    /// `u` the usable logical extent and `s` the outer size,
    /// `anchor = c − u/2 + (u − s)/2 = c − s/2` when `s ≤ u`. A window larger than the region on
    /// an axis is aligned to the region's origin on that axis (`anchor = c − u/2`), so that its
    /// top-left corner — title bar, menus — is the part that stays reachable. The result is
    /// rounded once by R3 ([`CanvasPos::round_to_point`]). The usable region is the whole
    /// output view; nothing reserves part of it yet.
    ///
    /// `None` keeps the anchor of the initial commit: the legacy policy, no output, or a
    /// position outside the Canvas bound.
    fn first_map_anchor(&self, tree: &SurfaceTree) -> Option<CanvasPoint> {
        if self.placement != PlacementPolicy::CentredInView {
            return None;
        }
        let output = self.outputs.first()?;
        let centre = self
            .cameras
            .camera_for_output(output.id)
            .and_then(|c| self.cameras.center(c))?;
        let usable = output.logical_extent();
        let axis = |c: i64, usable: f64, outer: i32| {
            let half_outer = i64::from(outer.max(0)) * Q16_ONE / 2;
            let half_usable = geom::q16(usable / 2.0);
            c - half_outer.min(half_usable)
        };
        CanvasPos {
            x: axis(centre.x, usable.0, tree.geometry.size.w),
            y: axis(centre.y, usable.1, tree.geometry.size.h),
        }
        .round_to_point()
    }

    /// The placement policy for elements mapped from now on. It never moves an element that is
    /// already placed.
    pub fn set_placement_policy(&mut self, policy: PlacementPolicy) {
        self.placement = policy;
    }

    pub fn placement_policy(&self) -> PlacementPolicy {
        self.placement
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

        self.pending_constraint = Some(candidate);

        if self.can_activate_constraint(&candidate) {
            let mut fx = Vec::new();
            if let Some(prev) = self.constraint
                && (prev.surface != surface || prev.kind != kind)
            {
                fx.push(Effect::ConstraintDeactivate {
                    surface: prev.surface,
                    kind: prev.kind,
                });
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
        if let Some(c) = &mut self.constraint
            && c.surface == surface
            && c.kind == crate::pointer::ConstraintKind::Lock
        {
            c.lock_point = hint;
        }
        if let Some(c) = &mut self.pending_constraint
            && c.surface == surface
            && c.kind == crate::pointer::ConstraintKind::Lock
        {
            c.lock_point = hint;
        }
    }

    /// Client destroys a pointer constraint (M13).
    pub fn destroy_pointer_constraint(&mut self, surface: SurfaceKey) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(c) = self.constraint
            && c.surface == surface
        {
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
        if let Some(c) = self.pending_constraint
            && c.surface == surface
        {
            self.pending_constraint = None;
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
                if let Some(p_lock) = self.derived_lock_output_point(&c)
                    && let pointer::PointerState::Located { output, .. } = self.pointer
                {
                    self.pointer = pointer::PointerState::Located {
                        output,
                        p: (p_lock.x, p_lock.y),
                    };
                }
            }
        }

        // 2. If no constraint is active, check if pending constraint can activate
        if self.constraint.is_none()
            && let Some(pending) = self.pending_constraint
            && self.can_activate_constraint(&pending)
        {
            self.constraint = Some(pending);
            if pending.lifetime == crate::pointer::ConstraintLifetime::Oneshot {
                self.pending_constraint = None;
            }
            fx.push(Effect::ConstraintActivate {
                surface: pending.surface,
                kind: pending.kind,
            });
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
        if let Some(r) = c.region
            && !r.contains(local_pt.x as i64, local_pt.y as i64)
        {
            return false;
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
                if let Some(r) = c.region
                    && !r.contains(local_pt.x as i64, local_pt.y as i64)
                {
                    return false;
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
        // INV-STAGE-1 (C12.F1): a provisional anchor at the initial commit, the camera centre
        let provisional = core.stage().anchor(id).expect("placed at initial commit");
        assert_eq!(provisional, CanvasPoint { x: 0, y: 0 }, "camera centre");
        assert_eq!(core.toplevel_committed(id, tree(false)), Ok(vec![]));
        // first buffer: mapped, on top, and its surface enters the output (F-MEMBER-1)
        assert_eq!(core.toplevel_committed(id, tree(true)), Ok(vec![enter]));
        // default placement resolved at the first map: 100x50 centred on (0,0)
        let anchor = core.stage().anchor(id).expect("placed");
        assert_eq!(anchor, CanvasPoint { x: -50, y: -25 }, "F-PLACE-1");
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
        // k = 1, camera centre (0,0), 1920x1080 → viewport [−960,960) × [−540,540). The target is
        // parent-relative: before the first map the anchor is the camera centre; after it the
        // 100x50 window is centred, anchor (−50,−25), and the viewport is at (−910,−515)
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
                LocalPoint { x: -910, y: -515 },
                LocalSize { w: 1920, h: 1080 }
            )
        );
        // k = 1.75: c_eff = (−35951177, −20222537) Q16; the viewport's fractional edges
        // (−548.57…, 548.57…) × (−308.57…, 308.57…) round inward to [−548, 548) × [−308, 308);
        // relative to the anchor (−50,−25) that is (−498,−283)
        let mut core = connected(210);
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        let t = core.popup_constraint_target(id, None).unwrap();
        assert_eq!(
            (t.loc, t.size),
            (
                LocalPoint { x: -498, y: -283 },
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
    // ------------------------------------------------------------ placement at first map

    fn sized(w: i32, h: i32, mapped: bool) -> SurfaceTree {
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: key(1),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
            }],
        }
    }

    use crate::PhysicalEvent;
    use crate::input::bindings::Modifiers;
    use crate::navigation::PersistedCameras;
    use crate::output::tests::key as okey;

    fn plain(connector: &str) -> OutputDescriptor {
        OutputDescriptor {
            key: okey(connector),
            mode: (1280, 800),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        }
    }

    fn placed(c: &mut Core, w: i32, h: i32) -> (ElementId, CanvasPoint) {
        let id = c.toplevel_created();
        c.toplevel_committed(id, sized(w, h, false)).unwrap();
        c.toplevel_committed(id, sized(w, h, true)).unwrap();
        (id, c.stage().anchor(id).unwrap())
    }

    fn centred(mode: (u32, u32), scale_120: u32, camera: Option<(i64, i64)>) -> Core {
        let entries = camera
            .map(|(x, y)| {
                vec![(
                    okey("A"),
                    CanvasPos {
                        x: x * Q16_ONE,
                        y: y * Q16_ONE,
                    },
                    1.0,
                )]
            })
            .unwrap_or_default();
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::from_entries(entries).unwrap());
        c.set_placement_policy(PlacementPolicy::CentredInView);
        let d = OutputDescriptor {
            key: okey("A"),
            mode,
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: 60_000,
        };
        c.output_connected(&mut t, d).unwrap();
        c
    }

    /// Core's default is the centred policy (C12.F1). The legacy rule stays available as an
    /// explicit policy: the geometry origin at the camera centre, decided at the initial commit
    /// and not revised at the first map, for any size.
    #[test]
    fn the_legacy_policy_places_the_origin_at_the_camera_centre() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        assert_eq!(c.placement_policy(), PlacementPolicy::CentredInView);
        c.set_placement_policy(PlacementPolicy::OriginAtCameraCentre);
        c.output_connected(&mut t, plain("A")).unwrap();
        assert_eq!(placed(&mut c, 646, 513).1, CanvasPoint { x: 0, y: 0 });
        assert_eq!(placed(&mut c, 5000, 4000).1, CanvasPoint { x: 0, y: 0 });
        // a window placed under the legacy rule keeps its anchor when the policy changes
        let (id, a) = placed(&mut c, 400, 300);
        c.set_placement_policy(PlacementPolicy::CentredInView);
        c.toplevel_committed(id, sized(400, 300, true)).unwrap();
        assert_eq!(c.stage().anchor(id), Some(a));
        assert_eq!(placed(&mut c, 400, 300).1, CanvasPoint { x: -200, y: -150 });
    }

    /// The legacy rule through a persisted camera away from the origin, negative coordinates
    /// included: the anchor is the camera centre, rounded by R3.
    #[test]
    fn the_legacy_policy_follows_the_camera() {
        let entries = vec![(
            okey("A"),
            CanvasPos {
                x: -5000 * Q16_ONE + Q16_ONE / 2,
                y: 3001 * Q16_ONE + Q16_ONE / 2,
            },
            1.0,
        )];
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::from_entries(entries).unwrap());
        c.set_placement_policy(PlacementPolicy::OriginAtCameraCentre);
        c.output_connected(&mut t, plain("A")).unwrap();
        // −4999.5 → −5000 and 3001.5 → 3002: ties to even
        assert_eq!(
            placed(&mut c, 646, 513).1,
            CanvasPoint { x: -5000, y: 3002 }
        );
    }

    /// A window that fits is centred: x = usable_x + (usable_w − w)/2, y likewise. KCalc's
    /// 646x513 on a 1280x800 output with the camera at the Canvas origin: the view is
    /// [−640, 640) × [−400, 400), so the anchor is (−640 + 317, −400 + 143.5) = (−323, −256.5),
    /// and R3 (ties to even) gives −256. The whole window is inside the view.
    #[test]
    fn a_window_that_fits_is_centred_in_the_view() {
        let mut c = centred((1280, 800), 120, None);
        let (_, a) = placed(&mut c, 646, 513);
        assert_eq!(a, CanvasPoint { x: -323, y: -256 });
        assert!(a.x >= -640 && a.x + 646 <= 640 && a.y >= -400 && a.y + 513 <= 400);
        // even sizes centre exactly
        let (_, a) = placed(&mut c, 400, 300);
        assert_eq!(a, CanvasPoint { x: -200, y: -150 });
    }

    /// An oversized window is aligned to the region's origin on the axis it does not fit, and
    /// centred on the other: its top-left corner is what stays reachable.
    #[test]
    fn an_oversized_window_is_aligned_to_the_region_origin() {
        let mut c = centred((1280, 800), 120, None);
        assert_eq!(
            placed(&mut c, 2000, 300).1,
            CanvasPoint { x: -640, y: -150 }
        );
        assert_eq!(
            placed(&mut c, 300, 2000).1,
            CanvasPoint { x: -150, y: -400 }
        );
        assert_eq!(
            placed(&mut c, 5000, 4000).1,
            CanvasPoint { x: -640, y: -400 }
        );
        // exactly the size of the region: both rules agree
        assert_eq!(
            placed(&mut c, 1280, 800).1,
            CanvasPoint { x: -640, y: -400 }
        );
    }

    /// The usable region is the output's logical extent, so the output scale matters and the
    /// physical mode does not: 1920x1080 at 1.5 is 1280x720 logical; 3840x2160 at 2.0 is
    /// 1920x1080 logical.
    #[test]
    fn placement_uses_the_logical_extent_of_a_scaled_output() {
        let mut c = centred((1920, 1080), 180, None);
        assert_eq!(placed(&mut c, 646, 513).1, CanvasPoint { x: -323, y: -256 });
        // 800 logical px fits in 1280 but 750 does not fit in 720: aligned to the top
        assert_eq!(placed(&mut c, 800, 750).1, CanvasPoint { x: -400, y: -360 });
        let mut c = centred((3840, 2160), 240, None);
        assert_eq!(
            placed(&mut c, 1900, 1000).1,
            CanvasPoint { x: -950, y: -500 }
        );
        assert_eq!(
            placed(&mut c, 2000, 1000).1,
            CanvasPoint { x: -960, y: -500 }
        );
    }

    /// F-PLACE-1 in exact integers, independent of Core's Q16 and f64 path: per axis
    /// `c − min(s, u)/2` with `u = 120·view/scale_120`, rounded half to even.
    fn exact_axis(c: i64, view: u32, scale_120: u32, s: i32) -> i64 {
        let d = 2 * i128::from(scale_120);
        let half = (i128::from(s) * i128::from(scale_120)).min(120 * i128::from(view));
        let n = 2 * i128::from(c) * i128::from(scale_120) - half;
        let (q, r) = (n.div_euclid(d), n.rem_euclid(d));
        let up = 2 * r > d || (2 * r == d && q % 2 != 0);
        (q + i128::from(up)) as i64
    }

    /// The five examples of the amendment draft that no other test carries (fractional usable
    /// extent, half-pixel ties of both parities), then a sweep against the exact model:
    /// 6 scales × 3 cameras × 4 modes × 40 sizes.
    #[test]
    fn placement_agrees_with_the_exact_model_on_fractional_extents_and_ties() {
        let mut c = centred((1366, 768), 150, None); // 1092.8 × 614.4 logical
        assert_eq!(placed(&mut c, 801, 601).1, CanvasPoint { x: -400, y: -300 });
        assert_eq!(
            placed(&mut c, 2000, 601).1,
            CanvasPoint { x: -546, y: -300 }
        );
        let mut c = centred((1280, 800), 120, None);
        // −322.5 → −322 and −255.5 → −256: ties go to the even pixel
        assert_eq!(placed(&mut c, 645, 511).1, CanvasPoint { x: -322, y: -256 });
        assert_eq!(placed(&mut c, 1, 1).1, CanvasPoint { x: 0, y: 0 });
        let mut c = centred((1280, 800), 120, Some((1, 1)));
        // −321.5 → −322 and −254.5 → −254
        assert_eq!(placed(&mut c, 645, 511).1, CanvasPoint { x: -322, y: -254 });

        let mut checked = 0;
        for scale_120 in [90u32, 120, 150, 180, 210, 240] {
            for camera in [(0i64, 0i64), (7001, 333), (-123_457, -9)] {
                for mode in [(1280u32, 800u32), (1366, 768), (1365, 767), (3840, 2160)] {
                    let mut c = centred(mode, scale_120, Some(camera));
                    for k in 0..40 {
                        let (w, h) = (1 + 97 * k, 3 + 61 * k);
                        let want = CanvasPoint {
                            x: exact_axis(camera.0, mode.0, scale_120, w),
                            y: exact_axis(camera.1, mode.1, scale_120, h),
                        };
                        assert_eq!(
                            placed(&mut c, w, h).1,
                            want,
                            "{w}×{h} on {mode:?} at {scale_120}/120, camera {camera:?}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 2880);
    }

    /// Placement goes through the camera: with the camera elsewhere on the Canvas, positive or
    /// negative, the window is centred in what that camera shows.
    #[test]
    fn placement_follows_the_camera_including_negative_canvas_coordinates() {
        let mut c = centred((1280, 800), 120, Some((9000, 700)));
        assert_eq!(
            placed(&mut c, 646, 513).1,
            CanvasPoint {
                x: 9000 - 323,
                y: 700 - 256
            }
        );
        let mut c = centred((1280, 800), 120, Some((-5000, -3000)));
        // −3000 − 256.5 = −3256.5, ties to even: −3256
        assert_eq!(
            placed(&mut c, 646, 513).1,
            CanvasPoint { x: -5323, y: -3256 }
        );
        assert_eq!(
            placed(&mut c, 2000, 300).1,
            CanvasPoint { x: -5640, y: -3150 }
        );
    }

    /// Placement happens once. A later commit with another size, a tree change, a camera pan,
    /// an output reconfiguration, a second output, and an unmap followed by a remap all leave
    /// the anchor where it is (INV-STAGE-5; placement is not camera movement).
    #[test]
    fn a_placed_window_is_not_moved_again() {
        let entries = PersistedCameras::default();
        let (mut c, mut port, _, mut t) = Core::new(entries);
        c.set_placement_policy(PlacementPolicy::CentredInView);
        let d = |mode, scale_120| OutputDescriptor {
            key: okey("A"),
            mode,
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: 60_000,
        };
        let (o, _) = c.output_connected(&mut t, d((1280, 800), 120)).unwrap();
        let (id, a) = placed(&mut c, 646, 513);
        assert_eq!(a, CanvasPoint { x: -323, y: -256 });
        let anchor = |c: &Core| c.stage().anchor(id).unwrap();

        c.toplevel_committed(id, sized(900, 700, true)).unwrap();
        assert_eq!(anchor(&c), a, "a resize commit");
        c.toplevel_tree_changed(id, sized(300, 200, true)).unwrap();
        assert_eq!(anchor(&c), a, "a tree change");

        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: Modifiers {
                logo: true,
                ..Modifiers::default()
            },
            time_us: 0,
        };
        c.physical_input(&mut port, button(true)).unwrap();
        c.physical_input(
            &mut port,
            PhysicalEvent::PointerMotion {
                delta: (-400.0, 250.0),
                delta_unaccel: (-400.0, 250.0),
                time_us: 0,
            },
        )
        .unwrap();
        c.physical_input(&mut port, button(false)).unwrap();
        assert_eq!(anchor(&c), a, "a camera pan");

        c.output_reconfigured(&mut t, o, d((2560, 1440), 180))
            .unwrap();
        assert_eq!(anchor(&c), a, "an output reconfiguration");
        let second = OutputDescriptor {
            key: okey("B"),
            ..d((1920, 1080), 120)
        };
        c.output_connected(&mut t, second).unwrap();
        assert_eq!(anchor(&c), a, "another output");

        c.toplevel_committed(id, sized(300, 200, false)).unwrap(); // unmap
        c.toplevel_committed(id, sized(300, 200, false)).unwrap(); // initial commit of the remap
        c.toplevel_committed(id, sized(300, 200, true)).unwrap(); // remap
        assert_eq!(anchor(&c), a, "a remap keeps its anchor");

        // a new window is placed by the camera as it is now, not as it was
        let (_, b) = placed(&mut c, 400, 300);
        assert_ne!(
            b,
            CanvasPoint { x: -200, y: -150 },
            "the pan moved the view"
        );
    }

    /// Popups and subsurfaces belong to their parent's tree: they do not change where the
    /// window is placed (only the window geometry counts) and are never placed by themselves.
    #[test]
    fn popups_and_subsurfaces_do_not_take_part_in_placement() {
        let mut c = centred((1280, 800), 120, None);
        let id = c.toplevel_created();
        c.toplevel_committed(id, sized(646, 513, false)).unwrap();
        let mut tree = sized(646, 513, true);
        tree.surfaces.push(SnapshotSurface {
            key: key(2),
            role: SurfaceRole::Subsurface {
                parent: 0,
                offset: LocalPoint { x: -200, y: 900 },
            },
            size: LocalSize { w: 3000, h: 50 },
            mapped: true,
            input: InputRegion::Infinite,
        });
        tree.surfaces.push(SnapshotSurface {
            key: key(3),
            role: SurfaceRole::Popup {
                position: LocalPoint { x: 10, y: 30 },
                geometry_loc: LocalPoint::default(),
            },
            size: LocalSize { w: 180, h: 240 },
            mapped: true,
            input: InputRegion::Infinite,
        });
        c.toplevel_committed(id, tree).unwrap();
        assert_eq!(c.stage().anchor(id), Some(CanvasPoint { x: -323, y: -256 }));
    }

    /// Without an output there is no view to centre in: the anchor of the initial commit is
    /// kept, and an output that connects later does not move the window.
    #[test]
    fn without_an_output_the_initial_anchor_is_kept() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        c.set_placement_policy(PlacementPolicy::CentredInView);
        let (id, a) = placed(&mut c, 646, 513);
        assert_eq!(a, CanvasPoint { x: 0, y: 0 });
        c.output_connected(&mut t, plain("A")).unwrap();
        assert_eq!(c.stage().anchor(id), Some(a));
    }
}
