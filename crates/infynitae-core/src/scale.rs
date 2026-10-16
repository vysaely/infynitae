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
use crate::stage::ElementId;

/// Candidate constants (provisional, configurable, PROV-1, PROV-2, PROV-3, PROV-5).
pub const DEFAULT_H_CFG: f64 = 3.0;
pub const DEFAULT_R: f64 = 1.25;
pub const DEFAULT_SIGMA: f64 = 0.05;
pub const DEFAULT_T_SETTLE_MS: u64 = 300;
/// EC-16 (`maxFramebufferWidth`, RX 9070 XT / RADV): a recorded device fact used as the
/// provisional `D_max`, not a runtime query.
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
    /// Device limit `D_max` supplied by the backend for its renderer (F-SCALE-2, OBJ-29).
    /// The default is the EC-16 device fact, not a query.
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

/// A scale policy parameter outside its allowed range (§PROV, F-SCALE-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleConfigError {
    /// `H_cfg` not finite and positive (PROV-1; F-SCALE-2 needs it finite).
    HCfg,
    /// `r` outside `1 < r ≤ 2` (PROV-2).
    Ratio,
    /// `σ` outside `0 ≤ σ < r − 1` (PROV-3).
    Sigma,
    /// `T_settle` not positive (PROV-5).
    SettleTimeout,
    /// `D_max` is 0: no buffer fits such a device and `H_dev` would be 0 (F-SCALE-2).
    DeviceMax,
}

impl ScalePolicyConfig {
    /// The per-value ranges of §PROV. They are what keeps h in `(0, H_cfg]`: every hint is a
    /// rung `≤ H_max(E) = min(H_cfg, D_max / s_max(E))`.
    pub fn validate(&self) -> Result<(), ScaleConfigError> {
        if !(self.h_cfg.is_finite() && self.h_cfg > 0.0) {
            return Err(ScaleConfigError::HCfg);
        }
        if !(self.r > 1.0 && self.r <= 2.0) {
            return Err(ScaleConfigError::Ratio);
        }
        if !(self.sigma >= 0.0 && self.sigma < self.r - 1.0) {
            return Err(ScaleConfigError::Sigma);
        }
        if self.t_settle_ms == 0 {
            return Err(ScaleConfigError::SettleTimeout);
        }
        if self.d_max == 0 {
            return Err(ScaleConfigError::DeviceMax);
        }
        Ok(())
    }
}

/// A time on the session's monotonic clock, in milliseconds since the session started. It is
/// the only clock a scale-epoch deadline is expressed on and compared with (M7).
///
/// Two other clocks exist in a running compositor and are different clocks: the time an input
/// device stamps on an event (forwarded to clients, never compared with a deadline), and the
/// presentation clock announced by `wp_presentation` (frame times). Neither is a `SessionTime`,
/// and there is deliberately no conversion from a bare number, no `Default` and no zero
/// constant: a caller has to say which clock it read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionTime(u64);

impl SessionTime {
    /// `ms` milliseconds after the session started. For the session clock's owner (and tests).
    pub const fn from_millis(ms: u64) -> SessionTime {
        SessionTime(ms)
    }

    pub const fn millis(self) -> u64 {
        self.0
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
    /// C12.F13 (REHEARSAL, NOT ADOPTED): another client's presentation lowered this element's
    /// requirement and its hint was held; client-response evaluations do not lower it until a
    /// non-client stimulus (not another client's presentation) evaluates it.
    pub presentation_hold: bool,
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
            presentation_hold: false,
        }
    }

    /// A timed step of an interaction followed by one evaluation: [`ScaleState::tick`], then
    /// [`ScaleState::arm`] if `kind` is given, then [`ScaleState::evaluate`]. The model tests
    /// (TST-16/17/24) drive the policy through it with explicit times.
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
        match kind {
            Some(k) if continuous => self.continuous_step(k),
            Some(k) => self.discrete_step(k, now_ms, config.t_settle_ms),
            None => {}
        }
        self.evaluate(rho, ladder, h_max, config, is_client_response)
    }

    fn open(&mut self, kind: InteractionKind) {
        if !self.epoch.open {
            self.epoch.open = true;
            self.epoch.kind = Some(kind);
            self.epoch.direction = 0;
        }
    }

    /// Closes an open epoch without evaluating; the evaluation that follows settles it (M9
    /// "settlement" of a fullscreen subject, C12.F13 REHEARSAL).
    pub(crate) fn end_epoch(&mut self) {
        self.epoch.open = false;
        self.epoch.deadline = None;
        self.epoch.kind = None;
    }

    /// A step of a continuous interaction (M7 `Closed → Open{continuous}`): opens the epoch if
    /// none is open. It has no deadline, so it needs no time; its end settles it.
    pub fn continuous_step(&mut self, kind: InteractionKind) {
        self.open(kind);
        self.epoch.continuous = true;
        self.epoch.deadline = None;
    }

    /// A discrete step at `now_ms` (M7 `Closed → Open{discrete, deadline = now + T_settle}` and
    /// `Open | further step`): opens the epoch if none is open and moves its deadline to
    /// `now_ms + T_settle`. With [`ScaleState::continuous_step`] the only writer of a deadline,
    /// and the only one that needs the time of the step. A discrete epoch's deadline never moves
    /// earlier: a step stamped before the previous one keeps the later deadline.
    pub fn discrete_step(&mut self, kind: InteractionKind, now_ms: u64, t_settle_ms: u64) {
        let was_discrete = self.epoch.open && !self.epoch.continuous;
        self.open(kind);
        self.epoch.continuous = false;
        let deadline = now_ms.saturating_add(t_settle_ms);
        self.epoch.deadline = Some(match self.epoch.deadline {
            Some(earlier) if was_discrete => earlier.max(deadline),
            _ => deadline,
        });
    }

    /// One evaluation of the policy against the requirement `rho`, with no notion of time: it
    /// neither opens nor re-arms nor settles an epoch (M7 `Open | client response | Open`). What
    /// Core runs at the end of a batch. Returns true if the authoritative hint `h` changed.
    pub fn evaluate(
        &mut self,
        rho: Option<f64>,
        ladder: &[f64],
        h_max: f64,
        config: &ScalePolicyConfig,
        is_client_response: bool,
    ) -> bool {
        self.last_rho = rho;

        // P7: Device ceiling exceeded is a correctness rule, not damping.
        // Immediate decrease applies and fixes direction to down (INV-SCALE-2, INV-SCALE-3).
        // It comes before everything else: an unobserved element holds its hint against
        // ordinary evaluation, not against the device bound.
        if self.h > h_max + 1e-12 {
            let t = target(rho.unwrap_or(self.h), ladder, h_max, config.sigma);
            self.h = t.min(h_max);
            self.epoch.direction = -1;
            self.last_stimulus_direction = -1;
            return true;
        }

        let Some(req) = rho else {
            // Unobserved elements hold h (INV-SCALE-5, M15).
            return false;
        };

        let t = target(req, ladder, h_max, config.sigma);

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
            // C12.F13: a held element is not lowered by a settlement that never touched it; the
            // device bound (P7) still applies
            if self.presentation_hold && t < self.h - 1e-12 && self.h <= h_max + 1e-12 {
                return false;
            }
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

/// OBJ-16: the scale state of each element, and the only store of it. Kept sorted by element,
/// so a lookup is a binary search, as the Stage's own records are: identities are monotonic and
/// never reused (INV-STAGE-3), so a destroyed element's entry can never be found by a later one.
/// Iteration is in element order.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScaleStates(Vec<(ElementId, ScaleState)>);

impl ScaleStates {
    fn find(&self, element: ElementId) -> Result<usize, usize> {
        self.0.binary_search_by_key(&element, |(e, _)| *e)
    }

    pub(crate) fn get(&self, element: ElementId) -> Option<&ScaleState> {
        self.find(element).ok().map(|i| &self.0[i].1)
    }

    pub(crate) fn get_mut(&mut self, element: ElementId) -> Option<&mut ScaleState> {
        self.find(element).ok().map(|i| &mut self.0[i].1)
    }

    /// The state of `element`, created by `init` if it has none.
    pub(crate) fn get_or_insert_with(
        &mut self,
        element: ElementId,
        init: impl FnOnce() -> ScaleState,
    ) -> &mut ScaleState {
        let i = match self.find(element) {
            Ok(i) => i,
            Err(i) => {
                self.0.insert(i, (element, init()));
                i
            }
        };
        &mut self.0[i].1
    }

    /// Sets the state of `element`, replacing the one it had.
    pub(crate) fn set(&mut self, element: ElementId, state: ScaleState) {
        match self.find(element) {
            Ok(i) => self.0[i].1 = state,
            Err(i) => self.0.insert(i, (element, state)),
        }
    }

    pub(crate) fn remove(&mut self, element: ElementId) {
        if let Ok(i) = self.find(element) {
            self.0.remove(i);
        }
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, (ElementId, ScaleState)> {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut ScaleStates {
    type Item = &'a mut (ElementId, ScaleState);
    type IntoIter = std::slice::IterMut<'a, (ElementId, ScaleState)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
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

/// F-SCALE-2 as proposed by the H-1 amendment (formulation B; EXPERIMENTAL, not adopted):
/// the device bound is the largest multiple of 1/120 that does not exceed `D_max / s_max(E)`,
/// `H_dev(E) = ⌊120 · D_max / s_max(E)⌋ / 120`, and `+∞` when `s_max(E) = 0` (E-F2-06).
///
/// A hint is put on the wire in 1/120 units, so a bound that is itself such a unit is emitted
/// exactly, and `s · h ≤ D_max` holds for every side `s ≤ s_max(E)` before the client rounds:
/// the buffer fits whether the client rounds down, to nearest or up. The sealed bound
/// `D_max / s_max(E)` is emitted as `round(120 · D_max / s_max(E))`, which exceeds it in about
/// half the cases (H-1).
///
/// The quotient is computed in integers (`120 · D_max < 2³⁹`). When it is 0
/// (`s_max(E) > 120 · D_max`) the bound is `1/120`, the wire minimum; that buffer does not fit
/// the device and nothing emittable would (stated residual).
pub fn h_dev(s_max: u32, d_max: u32) -> f64 {
    if s_max > 0 {
        let units = (120 * u64::from(d_max) / u64::from(s_max)).max(1);
        units as f64 / 120.0
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

/// Which mapped elements a batch-end evaluation has to visit. Derived from the scale states and
/// never authoritative: it decides only whether to visit, never what a hint is.
///
/// Evaluating an element again with unchanged inputs (its observers, their scales and zooms,
/// its surface tree, the outputs, the configuration) changes nothing ([`ScaleState::evaluate`]),
/// except in two cases:
/// - its epoch is closed and the batch belongs to a continuous interaction: the epoch opens;
/// - its epoch is closed, SC-E held its hint away from the target, and the batch is not
///   client-caused: the hint moves.
///
/// A batch therefore visits the elements marked for it (those whose inputs it changed, and those
/// a step changed outside an evaluation), unless another element may need it: when `all` says
/// an input common to every element changed; when the batch belongs to a continuous interaction
/// and `closed` says some mapped element may have a closed epoch; and always for a batch that is
/// neither client-caused nor part of a continuous interaction (an element SC-E held could
/// move). An element held by SC-E has a closed epoch, so inside a continuous interaction
/// `closed` covers it. `closed` is set by whatever may close an epoch, and cleared only by a
/// full pass, which computes it exactly; set without need, it costs one full pass.
#[derive(Debug)]
pub(crate) struct Sweep {
    pub(crate) all: bool,
    pub(crate) closed: bool,
    /// The elements marked for the next evaluation, ascending; between passes also the buffer
    /// a pass lists the elements it visits in (cleared, not freed).
    marked: Vec<ElementId>,
    /// C12.F13 (REHEARSAL): the elements whose own presentation started or ended in this batch
    /// (the subjects of a client's fullscreen enter/exit). Ascending; cleared, not freed.
    stimulated: Vec<ElementId>,
    /// C12.F13 (REHEARSAL): a client's request, commit or destruction started or ended a
    /// presentation in this batch; every other element it touches may rise, never fall.
    pub(crate) presentation_change: bool,
    /// The elements this batch's stimulus touched (the marks it made), ascending, kept while
    /// the pass runs: a camera action releases the hold of these only (C12.F13). Cleared, not
    /// freed.
    touched: Vec<ElementId>,
    /// A system change (an output added, removed or reconfigured) in this batch touches every
    /// element (C12.F13). Not the same as `all`, which only asks for a full re-evaluation (a
    /// device limit or a scale configuration set since the last pass).
    pub(crate) system: bool,
}

impl Default for Sweep {
    fn default() -> Self {
        Sweep {
            all: true,
            closed: true,
            marked: Vec::new(),
            stimulated: Vec::new(),
            presentation_change: false,
            touched: Vec::new(),
            system: false,
        }
    }
}

impl Sweep {
    pub(crate) fn needs_full_pass(&self, continuous: bool, is_client_response: bool) -> bool {
        self.all || (continuous && self.closed) || (!continuous && !is_client_response)
    }

    /// The next evaluation visits `element`.
    pub(crate) fn mark(&mut self, element: ElementId) {
        if let Err(i) = self.marked.binary_search(&element) {
            self.marked.insert(i, element);
        }
    }

    /// C12.F13 (REHEARSAL): `element`'s own presentation started or ended in this batch.
    pub(crate) fn stimulate(&mut self, element: ElementId) {
        self.mark(element);
        if let Err(i) = self.stimulated.binary_search(&element) {
            self.stimulated.insert(i, element);
        }
    }

    pub(crate) fn is_stimulated(&self, element: ElementId) -> bool {
        self.stimulated.binary_search(&element).is_ok()
    }

    /// Remembers the marks of this batch (the elements its stimulus touched) for the pass.
    pub(crate) fn remember_touched(&mut self, marked: &[ElementId]) {
        self.touched.clear();
        self.touched.extend_from_slice(marked);
    }

    pub(crate) fn is_touched(&self, element: ElementId) -> bool {
        self.touched.binary_search(&element).is_ok()
    }

    /// The marked elements, ascending; hand the list back with [`Sweep::evaluated`].
    pub(crate) fn take_marked(&mut self) -> Vec<ElementId> {
        std::mem::take(&mut self.marked)
    }

    /// After an evaluation that visited `visited` (the list [`Sweep::take_marked`] gave):
    /// whether an element it visited was left with a closed epoch.
    pub(crate) fn evaluated(&mut self, full: bool, closed: bool, mut visited: Vec<ElementId>) {
        if full {
            (self.all, self.closed) = (false, closed);
        } else {
            self.closed |= closed;
        }
        visited.clear();
        self.marked = visited;
        self.stimulated.clear();
        self.presentation_change = false;
        self.touched.clear();
        self.system = false;
    }
}

/// The last ladder built, with everything it was built from. `L(E)` depends on the element only
/// through `H_max(E)`, and nearly every element has the same one (`H_cfg`), so most lookups find
/// it. Derived and self-checking: a lookup compares the output scales, `r` and `H_max` it is
/// given with those the ladder was built from and builds it again on any difference, so no
/// change elsewhere has to invalidate it.
#[derive(Debug, Default)]
pub(crate) struct Ladders {
    output_scales: Vec<f64>,
    r: f64,
    h_max: f64,
    ladder: Vec<f64>,
}

impl Ladders {
    /// [`ladder`] of the outputs' scales, `h_max` and `r`.
    pub(crate) fn get(&mut self, outputs: &[OutputModel], h_max: f64, r: f64) -> &[f64] {
        let same = |a: f64, b: f64| a.to_bits() == b.to_bits();
        let built = !self.ladder.is_empty()
            && same(self.h_max, h_max)
            && same(self.r, r)
            && self.output_scales.len() == outputs.len()
            && self
                .output_scales
                .iter()
                .zip(outputs)
                .all(|(&s, o)| same(s, o.scale));
        if !built {
            self.output_scales.clear();
            self.output_scales.extend(outputs.iter().map(|o| o.scale));
            (self.h_max, self.r) = (h_max, r);
            self.ladder = ladder(&self.output_scales, h_max, r);
        }
        &self.ladder
    }
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

/// F-SCALE-3 / wp_fractional_scale_v1: `preferred_scale = round(h · 120)`, in 1/120 units.
///
/// Domain: h finite and positive. That holds for every authoritative hint, because the
/// parameters that bound it are validated where they enter Core
/// ([`ScalePolicyConfig::validate`]). h < 1 is in the domain (an output scale below 1; a logical
/// side above `D_max`) and is emitted as such. The only bounds applied are those of the wire
/// type, a `uint` numerator: at least 1, at most `u32::MAX`.
///
/// A value outside the domain is a Core defect, reported by the assertion in debug and test
/// builds. A release build still never puts 0 on the wire (`max` maps NaN to 1).
pub fn preferred_scale_120(h: f64) -> u32 {
    debug_assert!(h.is_finite() && h > 0.0, "hint {h} outside its domain");
    wire_scale_120(h)
}

/// The wire conversion alone, total over f64.
fn wire_scale_120(h: f64) -> u32 {
    (h * 120.0).round().max(1.0).min(u32::MAX as f64) as u32
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
/// `presenting(O)`: O shows a protocol-fullscreen presentation (OBJ-19), not its camera's view.
/// `observers` may name an output more than once.
pub fn compute_rho<'a>(
    observers: impl IntoIterator<Item = &'a crate::output::OutputId>,
    outputs: &[OutputModel],
    cameras: &crate::navigation::CameraSet,
    presenting: impl Fn(crate::output::OutputId) -> bool,
) -> Option<f64> {
    let mut max_rho = 0.0f64;
    let mut observed = false;
    for &o_id in observers {
        if let Some(model) = outputs.iter().find(|o| o.id == o_id) {
            observed = true;
            let zoom = if presenting(o_id) {
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
    predict_rho_observed(anchor, geometry, outputs, views).unwrap_or_else(|| {
        // Fallback: max connected output scale (INV-SCALE-5).
        outputs
            .iter()
            .map(|o| o.scale)
            .fold(1.0f64, |acc, s| acc.max(s))
    })
}

/// [`predict_rho`] without its fallback: `None` when no view intersects the placed rect.
pub fn predict_rho_observed(
    anchor: crate::geom::CanvasPoint,
    geometry: LocalRect,
    outputs: &[OutputModel],
    views: &[(crate::output::OutputId, View)],
) -> Option<f64> {
    let mut max_rho = 0.0f64;
    let mut intersected = false;

    // Canvas extent of the placed element geometry:
    // the anchor is in Canvas px, the views in Q16 (independent review 2026-10-15: the px anchor
    // was compared with Q16 view edges, so a far window could predict an output near the origin)
    let q0_x = (anchor.x + geometry.loc.x as i64) * Q16_ONE;
    let q0_y = (anchor.y + geometry.loc.y as i64) * Q16_ONE;
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

    intersected.then_some(max_rho)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ladder memo answers every lookup with exactly what [`ladder`] builds from the same
    /// output scales, `H_max` and `r`, whichever of them changed since the previous lookup: one
    /// at a time, and the output list growing or shrinking behind the same leading scales.
    #[test]
    fn the_ladder_memo_equals_a_fresh_ladder() {
        use crate::output::{OutputDescriptor, OutputIds, Transform};
        let mut ids = OutputIds::default();
        let mut outputs = |scales: &[u32]| -> Vec<OutputModel> {
            scales
                .iter()
                .map(|&scale_120| {
                    let d = OutputDescriptor {
                        key: crate::output::tests::key("A"),
                        mode: (1920, 1080),
                        transform: Transform::Normal,
                        scale_120,
                        refresh_mhz: 60_000,
                    };
                    OutputModel::from_descriptor(ids.next(), &d, (0.0, 0.0))
                })
                .collect()
        };
        let sets = [
            outputs(&[]),
            outputs(&[120]),
            outputs(&[120, 240]),
            outputs(&[120, 240, 150]),
            outputs(&[150]),
            outputs(&[90, 180]),
        ];
        let (h_maxes, rs) = ([3.0, 2.0, 1.5, 0.75, 2.4], [1.25, 1.5, 2.0]);
        let mut memo = Ladders::default();
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let (mut set, mut h, mut r) = (0, 0, 0);
        for _ in 0..3000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            // change one input, or none
            match x % 4 {
                0 => set = (x / 4 % sets.len() as u64) as usize,
                1 => h = (x / 4 % h_maxes.len() as u64) as usize,
                2 => r = (x / 4 % rs.len() as u64) as usize,
                _ => {}
            }
            let scales: Vec<f64> = sets[set].iter().map(|o| o.scale).collect();
            assert_eq!(
                memo.get(&sets[set], h_maxes[h], rs[r]),
                ladder(&scales, h_maxes[h], rs[r]).as_slice(),
                "outputs {scales:?}, H_max {}, r {}",
                h_maxes[h],
                rs[r]
            );
        }
    }

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
        // H-1 (B): 120·16384/8000 = 245.76, so the bound is 245/120, not 2.048
        assert_eq!(h_max(h_cfg, 8000, d_max), 245.0 / 120.0);

        // Transition zero extent -> first buffer with P7 device bound:
        let h0 = 3.0;
        let hm_8000 = h_max(h_cfg, 8000, d_max);
        assert!(h0 > hm_8000);
        let mut st = ScaleState::new(h0);
        let l = ladder(&[1.0, 1.5], hm_8000, 1.25);
        let p7_triggered = st.p7_check(&l, hm_8000, 0.05);
        assert!(p7_triggered, "P7 triggered when buffer 8000 attaches");
        assert_eq!(st.h, 245.0 / 120.0);
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

    /// C1 (INC-6 hardening): `preferred_scale = round(h · 120)` over the whole domain of h
    /// (F-SCALE-3). h < 1 is legitimate: an output scale below 1 is a rung (F-SCALE-2), and
    /// `H_max(E) < 1` when a logical side exceeds `D_max` (the EE-5 case `(20000, 0.8192)`).
    #[test]
    fn c1_preferred_scale_is_round_h_times_120_below_one() {
        assert_eq!(preferred_scale_120(0.75), 90);
        assert_eq!(preferred_scale_120(1.0), 120);
        assert_eq!(preferred_scale_120(1.25), 150);
        assert_eq!(preferred_scale_120(0.8192), 98, "EE-5: side 20000 at D_max");
        assert_eq!(preferred_scale_120(2.048), 246);
    }

    /// C1: the only bounds are those of the wire type. `wp_fractional_scale_v1.preferred_scale`
    /// is a `uint` numerator over 120, so a positive h never becomes 0 and never wraps.
    #[test]
    fn c1_preferred_scale_wire_bounds() {
        assert_eq!(preferred_scale_120(1.0 / 120.0), 1);
        assert_eq!(
            preferred_scale_120(1.0 / 240.0),
            1,
            "0.5 rounds away from zero"
        );
        assert_eq!(
            preferred_scale_120(1e-9),
            1,
            "positive h is never sent as 0"
        );
        assert_eq!(preferred_scale_120(f64::MIN_POSITIVE), 1);
        assert_eq!(preferred_scale_120(35_791_394.0), 4_294_967_280);
        assert_eq!(preferred_scale_120(35_791_395.0), u32::MAX, "saturates");
        assert_eq!(preferred_scale_120(1e300), u32::MAX);
    }

    /// C1: an out-of-domain hint is a Core defect and is reported, not emitted (debug and test
    /// builds; a release build keeps the wire value at least 1).
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "outside its domain")]
    fn c1_out_of_domain_hint_is_reported() {
        preferred_scale_120(f64::NAN);
    }

    /// C1: whatever reaches the wire conversion, 0 never leaves it (a release build has no
    /// assertion in front of it).
    #[test]
    fn c1_wire_conversion_never_yields_zero() {
        for h in [f64::NAN, 0.0, -0.0, -3.0, f64::NEG_INFINITY] {
            assert_eq!(wire_scale_120(h), 1, "{h}");
        }
        assert_eq!(wire_scale_120(f64::INFINITY), u32::MAX);
        assert_eq!(wire_scale_120(0.75), 90);
    }

    /// C1: values outside the domain of h are refused where they would enter Core, not hidden
    /// by the emission: every parameter that determines `H_max` is validated (PROV-1…3, PROV-5,
    /// `D_max`).
    #[test]
    fn c1_invalid_policy_parameters_are_rejected() {
        let ok = ScalePolicyConfig::default();
        assert_eq!(ok.validate(), Ok(()));
        for h_cfg in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            let c = ScalePolicyConfig { h_cfg, ..ok };
            assert_eq!(c.validate(), Err(ScaleConfigError::HCfg), "h_cfg {h_cfg}");
        }
        for r in [f64::NAN, f64::INFINITY, 1.0, 0.5, 2.5] {
            let c = ScalePolicyConfig { r, ..ok };
            assert_eq!(c.validate(), Err(ScaleConfigError::Ratio), "r {r}");
        }
        for sigma in [f64::NAN, f64::INFINITY, -0.01, 0.25, 1.0] {
            let c = ScalePolicyConfig { sigma, ..ok };
            assert_eq!(c.validate(), Err(ScaleConfigError::Sigma), "sigma {sigma}");
        }
        let c = ScalePolicyConfig {
            t_settle_ms: 0,
            ..ok
        };
        assert_eq!(c.validate(), Err(ScaleConfigError::SettleTimeout));
        let c = ScalePolicyConfig { d_max: 0, ..ok };
        assert_eq!(c.validate(), Err(ScaleConfigError::DeviceMax));
        // the edges of the allowed ranges are accepted
        let c = ScalePolicyConfig {
            r: 2.0,
            sigma: 0.0,
            d_max: 1,
            ..ok
        };
        assert_eq!(c.validate(), Ok(()));
    }

    /// C1: with valid parameters h never leaves its domain, so the emission never needs a
    /// policy clamp. Sweeps `D_max` ∈ {4096, 8192, 16384}, extents from 0 to past `i32::MAX / 2`
    /// and output scales on both sides of 1.
    #[test]
    fn c1_hint_domain_is_closed_under_valid_parameters() {
        let outputs: [&[f64]; 5] = [&[], &[0.75], &[1.0, 1.5], &[0.5, 2.0, 3.0], &[1.0 / 120.0]];
        for d_max in [4096u32, 8192, 16384] {
            for h_cfg in [1.0f64, 1.5, 3.0, 8.0] {
                for r in [1.05f64, 1.25, 2.0] {
                    for s_max in [0u32, 1, 600, 4096, 4097, 5000, 8000, 20000, 1_500_000_000] {
                        for scales in outputs {
                            let hm = h_max(h_cfg, s_max, d_max);
                            let hd = h_dev(s_max, d_max);
                            assert!(hm.is_finite() && hm > 0.0);
                            let l = ladder(scales, hm, r);
                            for rho in [0.01f64, 0.5, 0.75, 1.0, 1.5, 2.9, 3.0, 50.0] {
                                let h = target(rho, &l, hm, 0.04);
                                assert!(
                                    h.is_finite() && h > 0.0 && h <= hm + 1e-12,
                                    "h {h} outside (0, H_max {hm}]"
                                );
                                assert!(preferred_scale_120(h) >= 1);
                                let b = preferred_buffer_scale(h, hd);
                                assert!(b >= 1);
                                if s_max > 0 && s_max <= d_max {
                                    assert!(
                                        s_max as u64 * b as u64 <= d_max as u64,
                                        "integer buffer {s_max} × {b} exceeds D_max {d_max}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// H-1 (B), the function alone: `h_dev` is `⌊120·D/s⌋/120` computed in integers, exact
    /// in f64 (the wire conversion gives the same integer back), never 0, monotone, and safe at
    /// the ends of `u32`.
    #[test]
    fn h1_device_bound_function_is_exact_and_total() {
        // exhaustive over every wire value a 16384-pixel device can produce
        for units in 1..=120u32 * 16384 {
            assert_eq!(wire_scale_120(f64::from(units) / 120.0), units);
        }
        for d_max in [1u32, 2, 4096, 16384, 65536, i32::MAX as u32, u32::MAX] {
            let mut prev = f64::INFINITY;
            for s in [
                1u32,
                2,
                3,
                7,
                119,
                120,
                121,
                4095,
                4096,
                4097,
                16384,
                65535,
                1 << 20,
                i32::MAX as u32,
                u32::MAX,
            ] {
                let units = (120 * u64::from(d_max) / u64::from(s)).max(1);
                let h = h_dev(s, d_max);
                assert!(h.is_finite() && h > 0.0);
                assert_eq!(h, units as f64 / 120.0, "D_max {d_max}, side {s}");
                assert!(h <= prev, "not monotone in the side");
                prev = h;
                if units <= u64::from(u32::MAX) {
                    assert_eq!(u64::from(preferred_scale_120(h)), units);
                }
                // s·h ≤ D_max in exact arithmetic, unless nothing emittable fits
                if 120 * u64::from(d_max) >= u64::from(s) {
                    assert!(u64::from(s) * units <= 120 * u64::from(d_max));
                }
                // b(h) is unchanged by the quantisation: ⌊⌊120x⌋/120⌋ = ⌊x⌋
                assert_eq!(
                    u64::from(preferred_buffer_scale(h, h)),
                    (u64::from(d_max) / u64::from(s)).clamp(1, u64::from(u32::MAX))
                );
            }
        }
        assert_eq!(h_dev(0, 4096), f64::INFINITY);
        assert_eq!(h_dev(4096, 4096), 1.0);
        assert_eq!(
            h_dev(u32::MAX, 1),
            1.0 / 120.0,
            "residual: the wire minimum"
        );
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
