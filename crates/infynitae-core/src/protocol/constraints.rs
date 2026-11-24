//! Pointer locks and confinements: requests, activation, motion under a constraint.

use super::*;

impl Core {
    /// Super held or let go (every key event): a held Super releases the pointer constraint,
    /// letting it go re-arms it (at the batch-end reconciliation, under the usual conditions).
    pub(crate) fn suspend_constraints(&mut self, suspended: bool) -> Vec<Effect> {
        if self.constraints_suspended == suspended {
            return Vec::new();
        }
        self.constraints_suspended = suspended;
        self.reconcile_constraints()
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

        self.pending_constraints.retain(|c| c.surface != surface);
        self.pending_constraints.push(candidate);

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
                self.pending_constraints.retain(|c| c.surface != surface);
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
        for c in &mut self.pending_constraints {
            if c.surface == surface && c.kind == crate::pointer::ConstraintKind::Lock {
                c.lock_point = hint;
            }
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
        self.pending_constraints.retain(|c| c.surface != surface);
        fx
    }

    /// M13 batch-end reconciliation pass (INV-INPUT-5).
    /// Runs once at the end of every core batch where view/descriptor, geometry/origin,
    /// output set, ActiveElementFocus, or an interaction/grab ended.
    pub fn reconcile_constraints(&mut self) -> Vec<Effect> {
        // Deferral rule: deferred while compositor pointer interaction or client-owned grab sequence is active.
        // Only taking a constraint away waits for a client's press to end. Granting one does not:
        // a game asks for its lock on the press itself (hold to look around), and through Xwayland
        // an ungranted lock turns every warp into a jump back and forth.
        if self.input.interaction.active()
            || (self.input.ownership.client_owns_any() && self.constraint.is_some())
        {
            return Vec::new();
        }

        let mut fx = Vec::new();

        // 1. Check active constraint, if any
        if let Some(c) = self.constraint {
            let still_valid = self.is_constraint_still_valid(&c);
            if !still_valid {
                self.constraint = None;
                if self.constraints_suspended {
                    // released for Super only: it comes back when Super is let go
                    if !self
                        .pending_constraints
                        .iter()
                        .any(|p| p.surface == c.surface)
                    {
                        self.pending_constraints.push(c);
                    }
                } else if c.lifetime == crate::pointer::ConstraintLifetime::Oneshot {
                    self.pending_constraints.retain(|p| p.surface != c.surface);
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
        if self.constraint.is_none() {
            let focused_surface = self
                .route_pointer()
                .and_then(|(tgt, _)| tgt)
                .map(|t| t.surface);

            let pending = self.pending_constraints.clone();
            let candidate = if let Some(surf) = focused_surface {
                pending
                    .iter()
                    .find(|p| p.surface == surf && self.can_activate_constraint(p))
                    .copied()
            } else {
                None
            }
            .or_else(|| {
                pending
                    .iter()
                    .find(|p| self.can_activate_constraint(p))
                    .copied()
            });

            if let Some(candidate) = candidate {
                self.constraint = Some(candidate);
                if candidate.lifetime == crate::pointer::ConstraintLifetime::Oneshot {
                    self.pending_constraints
                        .retain(|c| c.surface != candidate.surface);
                }
                fx.push(Effect::ConstraintActivate {
                    surface: candidate.surface,
                    kind: candidate.kind,
                });
            }
        }

        fx
    }

    /// Surface-local point if pointer currently hits `surface`.
    fn route_pointer_hit_local(&mut self, surface: SurfaceKey) -> Option<LocalPoint> {
        let pointer::PointerState::Located { output, p } = self.pointer else {
            return None;
        };
        // a panel or overlay that takes input above the canvas covers the window (as in
        // `route_pointer`); click-through overlays have no input region there
        if self.layer_hit(output, p, true).is_some() {
            return None;
        }
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
        // 0. not while Super is held (the user takes the cursor back)
        if self.constraints_suspended {
            return false;
        }
        // 1. no compositor pointer interaction being acquired
        if self.input.interaction.active() {
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
        if self.constraints_suspended {
            return false;
        }
        let Some(elem) = self.stage.element_containing_surface(c.surface) else {
            return false;
        };
        if self.focus.target() != Some(elem) {
            return false;
        }

        match c.kind {
            crate::pointer::ConstraintKind::Lock => {
                let pointer::PointerState::Located { output, p } = self.pointer else {
                    return false;
                };
                // an input-taking layer opened over the locked pointer ends the lock
                if self.layer_hit(output, p, true).is_some() {
                    return false;
                }
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
                .element_containing_surface(c.surface)
                .and_then(|e| self.stage.tree(e))
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
