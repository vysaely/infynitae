//! Scale policy (OBJ-16, F-SCALE-1, F-SCALE-2, F-SCALE-3, INV-SCALE-1…5).
//!
//! Canonical requirements:
//! - `F-SCALE-1`: Scale requirement `ρ(E) = max over observing presentations p of output_scale(p) * presentation_zoom(p)`.
//!   A protocol-fullscreen presentation contributes `scale_O` (zoom 1); no fullscreen override (INV-SCALE-1).
//! - `F-SCALE-2`: Ladder `L(E)` and device ceiling `H_max(E) = min(H_cfg, H_dev(E))`.
//!   Device bound `H_dev(E) = sup{h > 0 : h · side ≤ D_max for every logical side of every buffered surface of E}`
//!   `= D_max / s_max(E)` when `s_max(E) > 0` and `+∞` when `s_max(E) = 0` (initial commit with no buffer, E-F2-06).
//!   Ladder contains connected output scales `≤ H_max`, integers `1..=floor(H_max)`, and geometric fill with ratio `≤ r`.
//! - `F-SCALE-3`: Target `t(ρ) = smallest rung of L(E) ≥ ρ / (1 + σ)` else `H_max(E)`.
//!   Emission: `preferred_scale = round(h · 120)`, `preferred_buffer_scale = b(h)` with
//!   `b(h) = ⌈h⌉` if `⌈h⌉ ≤ H_dev(E)` else `max(1, ⌊H_dev(E)⌋)` (E1R-N3), only on change.
//!   Initial hint: `h := t(ρ_pred)` before initial configure (INV-SCALE-5, M15).
//! - `INV-SCALE-3` / `M7`: `ScaleInteractionEpoch` for `InteractionKind ∈ {CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream}`.
//!   Decreases suppressed during epoch; increases happen only at rung crossings in epoch direction;
//!   P7 immediate device-bound decrease fixes direction to down; settlement at interaction end or `T_settle`.
//! - `INV-SCALE-4`: Churn contract and `SC-E` monotonicity outside epochs (prevents scale storms and feedback loops).

use crate::geom::{LocalRect, Q16_ONE};
use crate::output::OutputModel;
use crate::projection::View;
use crate::snapshot::SurfaceTree;

/// Candidate constants (provisional, configurable, PROV-1, PROV-2, PROV-3, PROV-5).
pub const DEFAULT_H_CFG: f64 = 3.0;
pub const DEFAULT_R: f64 = 1.25;
pub const DEFAULT_SIGMA: f64 = 0.05;
pub const DEFAULT_T_SETTLE_MS: u64 = 300;
pub const DEFAULT_D_MAX: u32 = 16384;

/// Configurable scale policy parameters (PROV-1..3, PROV-5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScalePolicyConfig {
    /// Client scale ceiling H_cfg (PROV-1, candidate 3.0, allowed ≥ max connected output scale).
    pub h_cfg: f64,
    /// Ladder ratio r (PROV-2, candidate 1.25, allowed 1 < r ≤ 2).
    pub r: f64,
    /// Sharpness tolerance σ (PROV-3, candidate 0.05, allowed 0 ≤ σ < r − 1).
    pub sigma: f64,
    /// ScaleInteractionEpoch settlement timeout T_settle in milliseconds (PROV-5, candidate 300 ms).
    pub t_settle_ms: u64,
    /// Maximum texture side D_max queried from renderer capability (default 16384).
    pub d_max: u32,
}

impl Default for ScalePolicyConfig {
    fn default() -> Self {
        Self {
            h_cfg: DEFAULT_H_CFG,
            r: DEFAULT_R,
            sigma: DEFAULT_SIGMA,
            t_settle_ms: DEFAULT_T_SETTLE_MS,
            d_max: DEFAULT_D_MAX,
        }
    }
}

/// Interaction kinds participating in ScaleInteractionEpoch (INV-SCALE-3, M7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionKind {
    CameraPan,
    CameraZoom,
    WindowMove,
    WindowResize,
    ShellStream,
}

/// ScaleInteractionEpoch state machine (M7, OBJ-16).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScaleInteractionEpoch {
    pub open: bool,
    pub kind: Option<InteractionKind>,
    pub continuous: bool,
    /// Direction: +1 up, -1 down, 0 undecided.
    pub direction: i8,
    /// Discrete settlement deadline in monotonic milliseconds.
    pub deadline: Option<u64>,
}

/// Authoritative scale policy state per element (OBJ-16).
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleState {
    /// Authoritative client hint h.
    pub h: f64,
    /// Last requirement ρ seen.
    pub last_rho: Option<f64>,
    /// Last emitted preferred fractional scale in 1/120 units (`round(h · 120)`).
    pub last_emitted_scale_120: Option<u32>,
    /// Last emitted integer buffer scale (`b(h)`).
    pub last_emitted_buffer_scale: Option<u32>,
    /// Last non-client stimulus direction (+1 up, -1 down, 0 none) for SC-E oscillation prevention.
    pub last_stimulus_direction: i8,
    /// Active interaction epoch state.
    pub epoch: ScaleInteractionEpoch,
}

impl ScaleState {
    pub fn new(initial_h: f64) -> Self {
        Self {
            h: initial_h,
            last_rho: None,
            last_emitted_scale_120: None,
            last_emitted_buffer_scale: None,
            last_stimulus_direction: 0,
            epoch: ScaleInteractionEpoch::default(),
        }
    }

    /// Evaluates one step of scale policy for this element.
    /// Returns true if authoritative hint `h` changed.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        now_ms: u64,
        kind: Option<InteractionKind>,
        rho: Option<f64>,
        continuous: bool,
        ladder: &[f64],
        h_max: f64,
        _h_dev: f64,
        config: &ScalePolicyConfig,
        is_client_response: bool,
    ) -> bool {
        self.tick(now_ms, ladder, h_max, config.sigma);
        self.last_rho = rho;

        if let Some(k) = kind {
            if !self.epoch.open {
                self.epoch.open = true;
                self.epoch.kind = Some(k);
                self.epoch.direction = 0;
            }
            self.epoch.continuous = continuous;
            self.epoch.deadline = if continuous {
                None
            } else {
                Some(now_ms + config.t_settle_ms)
            };
        }

        let Some(req) = rho else {
            // Unobserved elements hold h (INV-SCALE-5, M15).
            return false;
        };

        let t = target(req, ladder, h_max, config.sigma);

        // P7: Device ceiling exceeded is a correctness rule, not damping.
        // Immediate decrease applies and fixes direction to down (INV-SCALE-2, INV-SCALE-3).
        if self.h > h_max + 1e-12 {
            self.h = t.min(h_max);
            self.epoch.direction = -1;
            self.last_stimulus_direction = -1;
            return true;
        }

        if self.epoch.open {
            // Grouped interaction epoch (INV-SCALE-3):
            // Decreases suppressed; increases happen only in the epoch's direction.
            if t > self.h + 1e-12 && self.epoch.direction != -1 {
                self.h = t;
                self.epoch.direction = 1;
                self.last_stimulus_direction = 1;
                return true;
            }
            false
        } else {
            // Outside an epoch (atomic stimulus or client-caused re-evaluation):
            if (t - self.h).abs() > 1e-12 {
                let d = if t > self.h { 1 } else { -1 };
                if is_client_response {
                    // SC-E: client-caused re-evaluations move h only in the direction of the last
                    // non-client stimulus. A reversal waits for a new stimulus.
                    if self.last_stimulus_direction != 0 && d != self.last_stimulus_direction {
                        return false;
                    }
                } else {
                    self.last_stimulus_direction = d;
                }
                self.h = t;
                return true;
            }
            false
        }
    }

    /// P7 check on surface geometry update: immediately lowers h if h > H_max(E).
    pub fn p7_check(&mut self, ladder: &[f64], h_max: f64, sigma: f64) -> bool {
        if self.h > h_max + 1e-12 {
            let req = self.last_rho.unwrap_or(self.h);
            let t = target(req, ladder, h_max, sigma);
            self.h = t.min(h_max);
            self.epoch.direction = -1;
            self.last_stimulus_direction = -1;
            true
        } else {
            false
        }
    }

    /// Explicit release of a continuous interaction (INV-SCALE-3, M7).
    pub fn release(&mut self, ladder: &[f64], h_max: f64, sigma: f64) -> bool {
        if self.epoch.open && self.epoch.continuous {
            self.settle(ladder, h_max, sigma)
        } else {
            false
        }
    }

    /// Settles the epoch once to the target of the final requirement (INV-SCALE-3, F-SCALE-3).
    pub fn settle(&mut self, ladder: &[f64], h_max: f64, sigma: f64) -> bool {
        if !self.epoch.open {
            return false;
        }
        self.epoch.open = false;
        self.epoch.deadline = None;
        self.epoch.kind = None;
        if let Some(req) = self.last_rho {
            let t = target(req, ladder, h_max, sigma);
            if (t - self.h).abs() > 1e-12 {
                self.last_stimulus_direction = if t > self.h { 1 } else { -1 };
                self.h = t;
                return true;
            }
        }
        false
    }

    /// Timer tick: settles discrete epochs whose deadline has expired (INV-SCALE-3, INV-IDLE-1).
    pub fn tick(&mut self, now_ms: u64, ladder: &[f64], h_max: f64, sigma: f64) -> bool {
        if self.epoch.open && self.epoch.deadline.is_some_and(|dl| now_ms >= dl) {
            self.settle(ladder, h_max, sigma)
        } else {
            false
        }
    }
}

/// F-SCALE-2 / E-F2-06: Largest logical side over all surfaces of E's tree (0 if no surface has a non-zero extent).
pub fn s_max_for_tree(tree: Option<&SurfaceTree>) -> u32 {
    let mut s_max = 0u32;
    if let Some(t) = tree {
        for s in &t.surfaces {
            if s.mapped {
                s_max = s_max
                    .max(s.size.w.max(0) as u32)
                    .max(s.size.h.max(0) as u32);
            }
        }
    }
    s_max
}

/// F-SCALE-2: Device bound H_dev(E) = D_max / s_max(E) when s_max > 0, +∞ when s_max = 0 (E-F2-06).
pub fn h_dev(s_max: u32, d_max: u32) -> f64 {
    if s_max > 0 {
        d_max as f64 / s_max as f64
    } else {
        f64::INFINITY
    }
}

/// F-SCALE-2: Client scale ceiling H_max(E) = min(H_cfg, H_dev(E)).
pub fn h_max(h_cfg: f64, s_max: u32, d_max: u32) -> f64 {
    h_cfg.min(h_dev(s_max, d_max))
}

/// F-SCALE-2: Ladder L(E) containing connected output scales ≤ H_max, integers 1..=floor(H_max),
/// and geometric fill so consecutive rungs differ by ratio ≤ r.
pub fn ladder(output_scales: &[f64], h_max: f64, r: f64) -> Vec<f64> {
    let mut base = Vec::new();
    for &s in output_scales {
        if s > 0.0 && s <= h_max + 1e-12 {
            base.push((s * 1e6).round() / 1e6);
        }
    }
    if h_max >= 1.0 {
        let max_i = h_max.floor() as u32;
        for i in 1..=max_i {
            base.push(i as f64);
        }
    }
    base.push((h_max * 1e6).round() / 1e6);

    base.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    base.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

    if base.is_empty() {
        return vec![h_max];
    }

    let mut rungs = vec![base[0]];
    for &nxt in &base[1..] {
        let prev = *rungs.last().expect("non-empty");
        if nxt > prev + 1e-6 {
            let n = ((nxt / prev).ln() / r.ln() - 1e-9).ceil().max(1.0) as usize;
            for i in 1..n {
                rungs.push(prev * (nxt / prev).powf(i as f64 / n as f64));
            }
            rungs.push(nxt);
        }
    }
    rungs
}

/// F-SCALE-3: Target t(ρ) = smallest rung of L(E) ≥ ρ / (1 + σ), else H_max(E).
pub fn target(rho: f64, ladder: &[f64], h_max: f64, sigma: f64) -> f64 {
    let need = rho / (1.0 + sigma);
    for &rung in ladder {
        if rung >= need - 1e-12 {
            return rung.min(h_max);
        }
    }
    h_max
}

/// F-SCALE-3 / wp_fractional_scale_v1: Preferred scale in 1/120 units.
pub fn preferred_scale_120(h: f64) -> u32 {
    (h * 120.0).round().max(120.0) as u32
}

/// F-SCALE-3 / E1R-N3: Integer buffer scale b(h) bounded by H_dev(E).
pub fn preferred_buffer_scale(h: f64, h_dev: f64) -> u32 {
    let c = (h - 1e-9).ceil().max(1.0) as u32;
    if (c as f64) <= h_dev + 1e-9 {
        c
    } else {
        ((h_dev + 1e-9).floor() as u32).max(1)
    }
}

/// F-SCALE-1: Requirement ρ(E) over observing presentations.
/// Camera presentations use camera zoom; protocol-fullscreen presentation uses zoom 1 (INV-SCALE-1).
pub fn compute_rho(
    observers: &[crate::output::OutputId],
    outputs: &[OutputModel],
    cameras: &crate::navigation::CameraSet,
    fullscreen_output: Option<crate::output::OutputId>,
) -> Option<f64> {
    let mut max_rho = 0.0f64;
    let mut observed = false;
    for &o_id in observers {
        if let Some(model) = outputs.iter().find(|o| o.id == o_id) {
            observed = true;
            let zoom = if fullscreen_output == Some(o_id) {
                1.0
            } else {
                cameras
                    .camera_for_output(o_id)
                    .and_then(|c| cameras.zoom(c))
                    .unwrap_or(1.0)
            };
            let req = model.scale * zoom;
            if req > max_rho {
                max_rho = req;
            }
        }
    }
    if observed { Some(max_rho) } else { None }
}

/// F-SCALE-3 / INV-SCALE-5: Initial scale prediction ρ_pred.
/// Computes F-SCALE-1 over presentations whose view intersects the placed window-geometry rect
/// (or the anchor point when size is 0×0); if none, max connected output scale.
pub fn predict_rho(
    anchor: crate::geom::CanvasPoint,
    geometry: LocalRect,
    outputs: &[OutputModel],
    views: &[(crate::output::OutputId, View)],
) -> f64 {
    let mut max_rho = 0.0f64;
    let mut intersected = false;

    // Canvas extent of the placed element geometry:
    let q0_x = anchor.x + (geometry.loc.x as i64) * Q16_ONE;
    let q0_y = anchor.y + (geometry.loc.y as i64) * Q16_ONE;
    let q1_x = q0_x + (geometry.size.w as i64) * Q16_ONE;
    let q1_y = q0_y + (geometry.size.h as i64) * Q16_ONE;
    let is_zero_size = geometry.size.w == 0 || geometry.size.h == 0;

    for (o_id, view) in views {
        let Some(model) = outputs.iter().find(|o| o.id == *o_id) else {
            continue;
        };
        let (view_w, view_h) = model.view_size();
        let logical_w = view_w as f64 / model.scale;
        let logical_h = view_h as f64 / model.scale;

        // View Canvas extent:
        let v0_x = view.c_eff.x;
        let v0_y = view.c_eff.y;
        let v1_x = v0_x + ((logical_w / view.zoom) * Q16_ONE as f64).round() as i64;
        let v1_y = v0_y + ((logical_h / view.zoom) * Q16_ONE as f64).round() as i64;

        let hit = if is_zero_size {
            q0_x >= v0_x && q0_x < v1_x && q0_y >= v0_y && q0_y < v1_y
        } else {
            q0_x < v1_x && q1_x > v0_x && q0_y < v1_y && q1_y > v0_y
        };

        if hit {
            intersected = true;
            let req = model.scale * view.zoom;
            if req > max_rho {
                max_rho = req;
            }
        }
    }

    if intersected {
        max_rho
    } else {
        // Fallback: max connected output scale (INV-SCALE-5).
        outputs
            .iter()
            .map(|o| o.scale)
            .fold(1.0f64, |acc, s| acc.max(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TST-16 — cp3a scale model (walkthroughs A–E, EC-19).
    #[test]
    fn tst16_cp3a_scale_model_walkthroughs() {
        let config = ScalePolicyConfig::default();
        let outs = [1.0, 1.5];
        let l = ladder(&outs, 3.0, config.r);
        assert_eq!(
            l.iter()
                .map(|x| (x * 1e4).round() / 1e4)
                .collect::<Vec<_>>(),
            vec![1.0, 1.2247, 1.5, 1.7321, 2.0, 2.4495, 3.0]
        );

        // Case A — mixed DPI baseline: A 1.0 x zoom 1, B 1.5 x zoom 1, W on both.
        let rho_a = 1.5f64.max(1.0);
        let h_a = target(rho_a, &l, 3.0, config.sigma);
        assert!((h_a - 1.5).abs() < 1e-9, "Case A reproduces MaxGesture 1.5");
        assert_eq!(preferred_scale_120(h_a), 180);
        assert_eq!(preferred_buffer_scale(h_a, f64::INFINITY), 2);

        // Case B — zoom dominates DPI: A 1.0 x zoom 2.0, B 1.5 x zoom 1.
        let rho_b = (1.0 * 2.0f64).max(1.5 * 1.0);
        let h_b = target(rho_b, &l, 3.0, config.sigma);
        assert!((h_b - 2.0).abs() < 1e-9, "Case B target is 2.0");

        // Case C — protocol fullscreen on A (zoom 1) while B (1.5) observes.
        let rho_c = (1.0 * 1.0f64).max(1.5 * 1.0);
        let h_c = target(rho_c, &l, 3.0, config.sigma);
        assert!((h_c - 1.5).abs() < 1e-9, "Case C target is 1.5");
        // When B stops observing:
        let rho_c_alone = 1.0 * 1.0;
        let h_c_alone = target(rho_c_alone, &l, 3.0, config.sigma);
        assert!(
            (h_c_alone - 1.0).abs() < 1e-9,
            "Case C alone settles to 1.0"
        );

        // Case D — viewport-fit W on A (zoom = min(1920/800, 1080/600) = 1.8), B observes normally.
        let z_fit = 1.8f64;
        let mut st = ScaleState::new(1.5);
        let rho_d = (1.0 * z_fit).max(1.5);
        st.step(
            0,
            None,
            Some(rho_d),
            false,
            &l,
            3.0,
            f64::INFINITY,
            &config,
            false,
        );
        let expected_h_d = 3.0f64.sqrt();
        assert!(
            (st.h - expected_h_d).abs() < 1e-4,
            "Case D fitted target is ~1.7321 (smallest rung >= 1.8/1.05 = 1.714), got {}",
            st.h
        );
        assert_eq!(preferred_scale_120(st.h), 208);
        assert_eq!(preferred_buffer_scale(st.h, f64::INFINITY), 2);
        // Restore:
        let rho_d_restore = 1.5f64;
        st.step(
            0,
            None,
            Some(rho_d_restore),
            false,
            &l,
            3.0,
            f64::INFINITY,
            &config,
            false,
        );
        assert!((st.h - 1.5).abs() < 1e-9, "Case D restore returns to 1.5");

        // Case E — continuous pinch zoom 1.0 -> 8.0 over 240 steps, reverse to 4.0 over 120 steps.
        let mut st_e = ScaleState::new(1.5);
        let mut events = Vec::new();
        for i in 1..=240 {
            let z = 1.0 * 8.0f64.powf(i as f64 / 240.0);
            let rho = (1.0 * z).max(1.5);
            let prev = st_e.h;
            if st_e.step(
                i,
                Some(InteractionKind::CameraZoom),
                Some(rho),
                true,
                &l,
                3.0,
                f64::INFINITY,
                &config,
                false,
            ) {
                events.push((prev, st_e.h));
            }
        }
        assert!((st_e.h - 3.0).abs() < 1e-9, "Hint saturated at H_cfg 3.0");

        // Reverse to 4.0 during gesture: decreases suppressed!
        for i in 1..=120 {
            let z = 8.0 * (4.0 / 8.0f64).powf(i as f64 / 120.0);
            let rho = (1.0 * z).max(1.5);
            let changed = st_e.step(
                240 + i,
                Some(InteractionKind::CameraZoom),
                Some(rho),
                true,
                &l,
                3.0,
                f64::INFINITY,
                &config,
                false,
            );
            assert!(
                !changed,
                "No hint change during zoom reversal inside gesture"
            );
        }
        assert!(
            (st_e.h - 3.0).abs() < 1e-9,
            "Hint remained at 3.0 during reversal"
        );

        // W leaves A while gesture continues:
        st_e.step(
            361,
            Some(InteractionKind::CameraZoom),
            Some(1.5),
            true,
            &l,
            3.0,
            f64::INFINITY,
            &config,
            false,
        );
        assert!(
            (st_e.h - 3.0).abs() < 1e-9,
            "Decrease deferred while gesture continues"
        );

        // Gesture ends: settles to 1.5.
        let settled = st_e.release(&l, 3.0, config.sigma);
        assert!(settled);
        assert!(
            (st_e.h - 1.5).abs() < 1e-9,
            "Gesture release settles once to 1.5"
        );
        assert!(
            events.len() <= l.len(),
            "Total changes during sweep bounded by ladder size"
        );
    }

    /// TST-17 — feedback model and SC-E oscillation prevention (EC-22).
    #[test]
    fn tst17_feedback_model_and_wheel_wiggle() {
        let config = ScalePolicyConfig::default();
        let outs = [1.0, 1.5];
        let l = ladder(&outs, 3.0, config.r);

        // Adversarial client under 60 rounds: inverts size with each move.
        let mut h = 1.5f64;
        let mut st = ScaleState::new(h);
        let mut changes = 0;
        let mut last_move = 0i8;

        for i in 0..60 {
            let stimulus = i == 0;
            let g = if stimulus {
                (800.0, 600.0)
            } else if last_move > 0 {
                (1200.0, 900.0)
            } else {
                (300.0, 225.0)
            };
            let z: f64 = (1920.0f64 / g.0).min(1080.0f64 / g.1);
            let rho: f64 = (1.0f64 * z).max(1.5f64);
            if st.step(
                i as u64,
                None,
                Some(rho),
                false,
                &l,
                3.0,
                f64::INFINITY,
                &config,
                !stimulus,
            ) {
                last_move = if st.h > h { 1 } else { -1 };
                h = st.h;
                changes += 1;
            }
        }
        assert!(
            changes <= 2,
            "Adversarial client caused {changes} changes; SC-E bounded to <= 2"
        );

        // Wheel wiggle across rung boundary (1.57 <-> 1.59 for 40 ticks).
        let mut st_wiggle = ScaleState::new(1.5);
        let mut wiggle_changes = 0;
        for i in 0..40 {
            let z: f64 = if i % 2 == 0 { 1.59 } else { 1.57 };
            let rho: f64 = z.max(1.5f64);
            if st_wiggle.step(
                i,
                Some(InteractionKind::CameraZoom),
                Some(rho),
                false,
                &l,
                3.0,
                f64::INFINITY,
                &config,
                false,
            ) {
                wiggle_changes += 1;
            }
        }
        assert_eq!(
            wiggle_changes, 1,
            "Discrete zoom increases at first crossing then suppresses wiggle decreases"
        );
        // Interaction end: settles at last zoom.
        if st_wiggle.settle(&l, 3.0, config.sigma) {
            wiggle_changes += 1;
        }
        assert_eq!(
            wiggle_changes, 2,
            "Discrete zoom wiggle produced exactly 2 changes (1 rung + 1 settlement)"
        );
    }

    /// TST-23 R2 & TST-24 — Spatial interaction epoch across interaction kinds (ER-5, ER2-2).
    #[test]
    fn tst24_spatial_interaction_epochs_across_kinds() {
        let config = ScalePolicyConfig::default();
        let outs = [1.0, 1.5];
        let l = ladder(&outs, 3.0, config.r);

        // Case 1: WindowMove oscillating across scale boundary for 40 steps.
        let mut st_move = ScaleState::new(1.5);
        let mut move_changes = 0;
        for i in 0..40 {
            let rho = if i % 2 == 0 { 1.5 } else { 1.0 };
            if st_move.step(
                i,
                Some(InteractionKind::WindowMove),
                Some(rho),
                true,
                &l,
                3.0,
                f64::INFINITY,
                &config,
                false,
            ) {
                move_changes += 1;
            }
        }
        assert_eq!(
            move_changes, 0,
            "No changes during window drag across boundary"
        );
        if st_move.release(&l, 3.0, config.sigma) {
            move_changes += 1;
        }
        assert_eq!(
            move_changes, 1,
            "Window drag settles in exactly 1 change on release"
        );

        // Case 2: WindowResize crossing device bound twice.
        let mut st_resize = ScaleState::new(1.5);
        let mut resize_changes = 0;
        for i in 0..40 {
            let s_max = if i == 13 || i == 27 { 12000 } else { 4000 };
            let hd = h_dev(s_max, 16384);
            let hm = h_max(3.0, s_max, 16384);
            let rho = if i % 2 == 0 { 1.5 } else { 1.0 };
            if st_resize.step(
                i,
                Some(InteractionKind::WindowResize),
                Some(rho),
                true,
                &l,
                hm,
                hd,
                &config,
                false,
            ) {
                resize_changes += 1;
            }
        }
        st_resize.release(&l, 3.0, config.sigma);
        assert!(
            resize_changes <= 5,
            "Resize with P7 crossings bounded by <= 5 changes"
        );

        // Case 3: ShellMove 3 bursts of 10 oscillating steps with pauses > T_settle.
        let mut st_shell = ScaleState::new(1.5);
        let mut shell_changes = 0;
        let mut t = 0u64;
        for _burst in 0..3 {
            for i in 0..10 {
                t += 16;
                let rho = if i % 2 == 0 { 1.5 } else { 1.0 };
                if st_shell.step(
                    t,
                    Some(InteractionKind::ShellStream),
                    Some(rho),
                    false,
                    &l,
                    3.0,
                    f64::INFINITY,
                    &config,
                    false,
                ) {
                    shell_changes += 1;
                }
            }
            t += 1000; // pause > T_settle
            if st_shell.tick(t, &l, 3.0, config.sigma) {
                shell_changes += 1;
            }
        }
        assert!(shell_changes <= 6, "Shell bursts bounded by <= 6 changes");
    }

    /// EE-5 / E-F2-06 / E1R-N3 — Zero-extent domain, first buffer P7, and integer emission bound.
    #[test]
    fn ee5_zero_extent_domain_and_integer_emission_bound() {
        let d_max = 16384u32;
        let h_cfg = 3.0f64;

        // E-F2-06: s_max = 0 has H_dev = +inf and H_max = H_cfg without division by zero.
        assert_eq!(h_dev(0, d_max), f64::INFINITY);
        assert_eq!(h_max(h_cfg, 0, d_max), 3.0);
        assert_eq!(h_max(h_cfg, 600, d_max), 3.0);
        assert_eq!(h_max(h_cfg, 800, d_max), 3.0);
        assert!((h_max(h_cfg, 8000, d_max) - 2.048).abs() < 1e-9);

        // Transition zero extent -> first buffer with P7 device bound:
        let h0 = 3.0;
        let hm_8000 = h_max(h_cfg, 8000, d_max);
        assert!(h0 > hm_8000);
        let mut st = ScaleState::new(h0);
        let l = ladder(&[1.0, 1.5], hm_8000, 1.25);
        let p7_triggered = st.p7_check(&l, hm_8000, 0.05);
        assert!(p7_triggered, "P7 triggered when buffer 8000 attaches");
        assert!((st.h - 2.048).abs() < 1e-9);
        assert_eq!(st.last_stimulus_direction, -1);

        // E1R-N3: Integer emission b(h) does not exceed D_max.
        let cases = [
            (800, 3.0, 3),
            (8000, 2.048, 2),
            (6000, 2.7307, 2),
            (5000, 3.0, 3),
            (0, 3.0, 3),
            (20000, 0.8192, 1),
        ];
        for (side, h, expected_b) in cases {
            let hd = h_dev(side, d_max);
            let b = preferred_buffer_scale(h, hd);
            assert_eq!(
                b, expected_b,
                "side {side} at h {h} expected b {expected_b}"
            );
            if side > 0 && side <= d_max {
                assert!(
                    (side as u64) * (b as u64) <= d_max as u64,
                    "side {side} * b {b} = {} must fit D_max {d_max}",
                    side * b
                );
            }
        }
    }

    /// Negative control: sealed ceil(h) exceeds D_max in exactly 2 cases (reproducing E1R-N3).
    #[test]
    fn e1r_n3_negative_control_sealed_ceil_exceeds_device_bound() {
        let d_max = 16384u32;
        let cases = [(8000, 2.048), (6000, 2.7307)];
        for (side, h) in cases {
            let sealed_b = (h - 1e-9_f64).ceil() as u32; // sealed ceil(h) = 3
            assert!(
                side * sealed_b > d_max,
                "Sealed ceil {sealed_b} * side {side} = {} exceeds D_max {d_max}",
                side * sealed_b
            );
            let corrected_b = preferred_buffer_scale(h, h_dev(side, d_max));
            assert!(
                side * corrected_b <= d_max,
                "Corrected b {corrected_b} * side {side} = {} fits D_max {d_max}",
                side * corrected_b
            );
        }
    }
}
