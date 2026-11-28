//! OBJ-22: ActiveElementFocus and focus history (M11, INV-INPUT-8).
//!
//! Core's FocusModel is the sole policy authority for desktop keyboard focus. It changes only by
//! explicit action (click-to-focus, focus-navigation bindings, shell focus command, activation grant)
//! or target loss (surface destroyed or unmapped). Cameras, visibility, and pointer motion never
//! change it.

use crate::protocol::Effect;
use crate::stage::ElementId;

/// What an `xdg_activation_v1` request may do (INV-INPUT-9, PROV-10, OB-40).
/// The adapter tells Core whether the request's token passed the provenance check (W-SM-8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActivationPolicy {
    /// The default: a request with a validated token focuses its window (when the window may
    /// take focus, INV-INPUT-8); any other request marks the window for attention.
    #[default]
    Strict,
    /// Opt-in: no request ever moves the focus; every request marks attention.
    AttentionOnly,
    /// Opt-in: any request focuses its window, validated or not (no focus-stealing prevention).
    Permissive,
}

/// Authoritative keyboard focus model for the seat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusModel {
    target: Option<ElementId>,
    /// Focus history: most recently focused live elements first.
    history: Vec<ElementId>,
    /// Whether the seat is eligible for initial automatic activation on first map.
    initial_eligible: bool,
    /// Elements whose activation request was answered with attention instead of focus
    /// (INV-INPUT-9, B10.7): state the shell may render; it never moves a camera. Oldest first.
    attention: Vec<ElementId>,
    /// What activation requests may do.
    activation_policy: ActivationPolicy,
    /// A layer surface holds the keyboard (L5), and whether it holds it exclusively. The window
    /// focus (`target`) is kept underneath and gets the keyboard back when the layer lets go.
    layer: Option<(crate::layer::LayerId, bool)>,
    /// When the pointer moved the focus to the window now focused, until it counts as used
    /// ([`FocusModel::settle_hover`]); until then it is not in the history.
    hovered_since: Option<u64>,
    /// Super+Tab while Super is held.
    cycle: Option<Cycle>,
}

/// A Super+Tab cycle: the windows in the order they had at its first Tab, the one it stands on
/// (`None` before the first step), and the focus before it, for Escape.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Cycle {
    order: Vec<ElementId>,
    at: Option<usize>,
    before: Option<ElementId>,
}

/// How long the pointer must keep a window focused before it counts as used: a window the pointer
/// only passed on its way elsewhere does not become Super+Tab's "last window" (owner, 2026-11-01).
const HOVER_SETTLES_US: u64 = 300_000;

impl Default for FocusModel {
    fn default() -> Self {
        Self {
            target: None,
            history: Vec::new(),
            initial_eligible: true,
            attention: Vec::new(),
            activation_policy: ActivationPolicy::Strict,
            layer: None,
            hovered_since: None,
            cycle: None,
        }
    }
}

impl FocusModel {
    pub fn target(&self) -> Option<ElementId> {
        self.target
    }

    /// Whether the seat is eligible for initial automatic activation.
    pub fn is_initial_eligible(&self) -> bool {
        self.initial_eligible
    }

    /// Grants initial focus to `candidate` if the seat is eligible for initial automatic activation.
    pub fn grant_initial_focus(&mut self, candidate: ElementId) -> Vec<Effect> {
        if self.target.is_none() && self.initial_eligible {
            self.initial_eligible = false;
            self.set_focus(Some(candidate))
        } else {
            Vec::new()
        }
    }

    /// Explicitly clears focus (e.g. background click), preventing automatic initial activation.
    #[cfg(test)]
    pub fn clear_focus_explicit(&mut self) -> Vec<Effect> {
        self.initial_eligible = false;
        self.set_focus(None)
    }

    /// Sets keyboard focus directly via an explicit action (click-to-focus, activation, shell).
    pub fn set_focus(&mut self, next: Option<ElementId>) -> Vec<Effect> {
        self.hovered_since = None;
        let fx = self.focus_to(next, true);
        // a click on the window the pointer had already focused: it is used now
        if let Some(id) = next
            && self.history.first() != Some(&id)
        {
            self.record(id);
        }
        fx
    }

    /// [`Self::set_focus`] at input time `now_us`: a window the pointer focused and kept long
    /// enough is recorded as used first, so a click or a shell command moving focus away does
    /// not drop it from the Super+Tab history (AUDIT_FOCUS_HOVER finding 8).
    pub fn set_focus_at(&mut self, now_us: u64, next: Option<ElementId>) -> Vec<Effect> {
        self.settle_hover(now_us);
        self.set_focus(next)
    }

    /// The pointer crossed into `next` (focus follows the mouse) at `now_us`: the focus moves, and
    /// `next` counts as used only once it stayed focused a moment ([`Self::settle_hover`]).
    pub fn hover_focus(&mut self, next: Option<ElementId>, now_us: u64) -> Vec<Effect> {
        self.settle_hover(now_us);
        self.hovered_since = next.map(|_| now_us);
        self.focus_to(next, false)
    }

    /// A window the pointer focused and that has stayed focused long enough becomes the most
    /// recently used. Called with each key and each crossing, before they act.
    pub fn settle_hover(&mut self, now_us: u64) {
        let settled = self
            .hovered_since
            .is_some_and(|since| now_us.saturating_sub(since) >= HOVER_SETTLES_US);
        if settled {
            self.hovered_since = None;
            if let Some(id) = self.target {
                self.record(id);
            }
        }
    }

    fn record(&mut self, id: ElementId) {
        self.history.retain(|&e| e != id);
        self.history.insert(0, id);
    }

    /// Moves the focus to `next`; `record`: it becomes the most recently used window. A window loses its attention mark when it is focused, any
    /// way (owner, 2026-10-31).
    fn focus_to(&mut self, next: Option<ElementId>, record: bool) -> Vec<Effect> {
        if next.is_some() {
            self.initial_eligible = false;
        }
        // an on-demand layer holding the keyboard gives it up to an explicit window focus, even to
        // the window that was focused before it (L5)
        let from_layer = matches!(self.layer, Some((_, false)));
        if self.target == next && !from_layer {
            return Vec::new();
        }
        if from_layer {
            self.layer = None;
        }
        let prev = self.target;
        self.target = next;
        if let Some(id) = next {
            if record {
                self.record(id);
            }
            // a focused element needs no attention
            self.attention.retain(|&e| e != id);
        }
        let mut effects = Vec::new();
        if let Some(prev_id) = prev {
            effects.push(Effect::ToplevelConfigure(prev_id));
        }
        if let Some(new_id) = next
            && Some(new_id) != prev
        {
            effects.push(Effect::ToplevelConfigure(new_id));
        }
        // while an exclusive layer holds the keyboard, the window focus changes underneath it and
        // the keyboard stays with the layer until it lets go (L5)
        if !self.layer_exclusive() {
            effects.push(Effect::KeyboardFocus(next));
        }
        effects
    }

    /// Super+Tab starts a cycle (owner, 2026-11-28, as Alt+Tab on Windows): the windows in the
    /// order they have now, kept until Super is let go so that each Tab goes one further. `shown`:
    /// the windows that may be focused, bottom to top; `minimized`: the hidden ones, which come
    /// last. The window that most recently asked for attention is the first step; it is returned.
    pub(crate) fn cycle_start(
        &mut self,
        shown: &[ElementId],
        minimized: &[ElementId],
    ) -> Option<ElementId> {
        self.initial_eligible = false;
        let mut order = self.recently_used(shown);
        order.extend(self.recently_used(minimized));
        // the focused window is where the cycle stands; a window the pointer focused without it
        // settling is not, so the first step goes back to the last one used
        let at = (self.target.is_some() && order.first() == self.target.as_ref()).then_some(0);
        let attention = self
            .attention
            .iter()
            .rev()
            .copied()
            .find(|e| shown.contains(e));
        if let Some(marked) = attention {
            order.retain(|&e| e != marked);
            order.insert(at.map_or(0, |a| a + 1), marked);
        }
        self.cycle = Some(Cycle {
            order,
            at,
            before: self.target,
        });
        attention
    }

    /// Whether a Super+Tab cycle is going on (Super still held).
    pub fn cycling(&self) -> bool {
        self.cycle.is_some()
    }

    /// One Tab of the cycle, `back` with Shift: the next window (the one before), wrapping round
    /// and passing over those no longer `open`, is focused without becoming the most recently
    /// used. `None`: no cycle, or no open window in it.
    pub(crate) fn cycle_step(
        &mut self,
        back: bool,
        open: impl Fn(ElementId) -> bool,
    ) -> Option<Vec<Effect>> {
        let cycle = self.cycle.as_mut()?;
        let len = cycle.order.len();
        let mut at = cycle.at;
        for _ in 0..len {
            let next = match (at, back) {
                (None, false) => 0,
                (None, true) => len - 1,
                (Some(a), false) => (a + 1) % len,
                (Some(a), true) => (a + len - 1) % len,
            };
            at = Some(next);
            let id = cycle.order[next];
            if open(id) {
                cycle.at = at;
                return Some(self.preview(Some(id)));
            }
        }
        None
    }

    /// Super let go: the window the cycle stands on, if it still has the focus, becomes the most
    /// recently used, the others keeping their order; it is returned.
    pub(crate) fn cycle_commit(&mut self) -> Option<ElementId> {
        let cycle = self.cycle.take()?;
        let chosen = cycle.order.get(cycle.at?).copied()?;
        if self.target != Some(chosen) {
            return None;
        }
        self.record(chosen);
        Some(chosen)
    }

    /// Escape during the cycle: it ends, and the window focused before its first Tab is returned
    /// (`Some(None)`: none was). `None`: no cycle.
    pub(crate) fn cycle_cancel(&mut self) -> Option<Option<ElementId>> {
        self.cycle.take().map(|c| c.before)
    }

    /// The focus moves to `next` without changing the history (a step of the cycle, or its end
    /// by Escape).
    pub(crate) fn preview(&mut self, next: Option<ElementId>) -> Vec<Effect> {
        self.hovered_since = None;
        self.focus_to(next, false)
    }

    /// The live windows, most recently used first; those never focused after them, the newest
    /// (topmost) first.
    fn recently_used(&self, live_elements: &[ElementId]) -> Vec<ElementId> {
        let mut order: Vec<ElementId> = self
            .history
            .iter()
            .copied()
            .filter(|e| live_elements.contains(e))
            .collect();
        for &e in live_elements.iter().rev() {
            if !order.contains(&e) {
                order.push(e);
            }
        }
        order
    }

    /// Target loss (K7, M11): focused element was destroyed or unmapped.
    /// The successor policy selects the most recently focused live element.
    pub fn target_lost(&mut self, dead: ElementId, live_elements: &[ElementId]) -> Vec<Effect> {
        self.target_lost_to(dead, live_elements, None)
    }

    /// [`Self::target_lost`] with the successor already chosen (`Some(next)`, strict focus: the
    /// window under the pointer or none), so the focus moves once instead of first to the last
    /// used window and then again (AUDIT_FOCUS_HOVER finding 6). `None`: the last used live one.
    pub fn target_lost_to(
        &mut self,
        dead: ElementId,
        live_elements: &[ElementId],
        chosen: Option<Option<ElementId>>,
    ) -> Vec<Effect> {
        self.history.retain(|&e| e != dead);
        if live_elements.is_empty() {
            self.initial_eligible = true;
        }
        self.attention.retain(|&e| e != dead);
        if self.target != Some(dead) {
            return Vec::new();
        }
        self.hovered_since = None;
        let successor = chosen.unwrap_or_else(|| {
            self.history
                .iter()
                .copied()
                .find(|e| live_elements.contains(e))
        });
        let prev = self.target;
        self.target = successor;
        // the successor is focused: its attention mark goes
        self.attention.retain(|&e| Some(e) != successor);
        let mut effects = Vec::new();
        if let Some(prev_id) = prev {
            effects.push(Effect::ToplevelConfigure(prev_id));
        }
        if let Some(next_id) = successor {
            effects.push(Effect::ToplevelConfigure(next_id));
        }
        // a layer holding the keyboard keeps it; the successor gets it when the layer lets go
        if self.layer.is_none() {
            effects.push(Effect::KeyboardFocus(successor));
        }
        effects
    }

    /// The window focus moves to `next` underneath a layer that holds the keyboard: the layer
    /// keeps it, and `next` gets it when the layer lets go (as `target_lost`'s successor). With
    /// no layer holding it, the keyboard follows at once.
    pub fn set_target_under_layer(&mut self, next: Option<ElementId>) -> Vec<Effect> {
        if self.target == next {
            return Vec::new();
        }
        let prev = self.target;
        self.target = next;
        if let Some(id) = next {
            self.attention.retain(|&e| e != id);
        }
        let mut effects = Vec::new();
        if let Some(prev_id) = prev {
            effects.push(Effect::ToplevelConfigure(prev_id));
        }
        if let Some(new_id) = next {
            effects.push(Effect::ToplevelConfigure(new_id));
        }
        if self.layer.is_none() {
            effects.push(Effect::KeyboardFocus(next));
        }
        effects
    }

    /// The layer surface holding the keyboard, if any (L5).
    pub fn layer(&self) -> Option<crate::layer::LayerId> {
        self.layer.map(|(l, _)| l)
    }

    /// An on-demand layer (a panel the user clicked) holds the keyboard.
    pub fn layer_on_demand(&self) -> bool {
        matches!(self.layer, Some((_, false)))
    }

    fn layer_exclusive(&self) -> bool {
        matches!(self.layer, Some((_, true)))
    }

    /// A layer surface takes the keyboard (L5): `exclusive` for a `top`/`overlay` surface asking
    /// for it while mapped, otherwise a click on an on-demand surface. An exclusive holder is not
    /// displaced by an on-demand one.
    pub(crate) fn layer_take(
        &mut self,
        layer: crate::layer::LayerId,
        exclusive: bool,
    ) -> Vec<Effect> {
        if self.layer_exclusive() && !exclusive && self.layer() != Some(layer) {
            return Vec::new();
        }
        if self.layer == Some((layer, exclusive)) {
            return Vec::new();
        }
        self.initial_eligible = false;
        self.layer = Some((layer, exclusive));
        vec![Effect::LayerKeyboardFocus(layer)]
    }

    /// A layer holding the keyboard exclusively now asks for it on demand: it keeps it, and a
    /// window clicked or focused takes it as from any on-demand holder (owner, 2026-11-22: the
    /// edge's panel opened by its key takes the keyboard, then lets the desktop have it).
    pub(crate) fn layer_demote(&mut self, layer: crate::layer::LayerId) {
        if self.layer == Some((layer, true)) {
            self.layer = Some((layer, false));
        }
    }

    /// The layer surface lets the keyboard go (unmapped, destroyed, no longer interactive): it
    /// returns to the window focus.
    pub(crate) fn layer_release(&mut self, layer: crate::layer::LayerId) -> Vec<Effect> {
        if self.layer() != Some(layer) {
            return Vec::new();
        }
        self.layer = None;
        vec![Effect::KeyboardFocus(self.target)]
    }

    pub fn activation_policy(&self) -> ActivationPolicy {
        self.activation_policy
    }

    pub(crate) fn set_activation_policy(&mut self, policy: ActivationPolicy) {
        self.activation_policy = policy;
    }

    /// An activation request for `element` (INV-INPUT-9, PROV-10): focus when the policy lets
    /// this request focus (`validated`: its token passed the provenance check) and the element
    /// may take focus (`eligible`), attention otherwise. Never a camera change.
    pub fn activation(
        &mut self,
        element: ElementId,
        validated: bool,
        eligible: bool,
    ) -> Vec<Effect> {
        let may_focus = match self.activation_policy {
            ActivationPolicy::Strict => validated,
            ActivationPolicy::AttentionOnly => false,
            ActivationPolicy::Permissive => true,
        };
        if may_focus && eligible {
            return self.set_focus(Some(element));
        }
        if self.target != Some(element) && !self.attention.contains(&element) {
            self.attention.push(element);
        }
        Vec::new()
    }

    /// A new window opened without the focus (FOCUS_ON_OPEN "user" or "priority" held it back):
    /// it is marked for attention, so Super+Tab goes to it first (owner, 2026-10-31). Never a
    /// focus or camera change. A window already marked becomes the most recent mark.
    pub(crate) fn mark_attention(&mut self, element: ElementId) {
        if self.target == Some(element) {
            return;
        }
        self.attention.retain(|&x| x != element);
        self.attention.push(element);
    }

    /// The shell acknowledged `element`'s attention mark (OBJ-04): it is cleared. No focus change.
    pub(crate) fn acknowledge_attention(&mut self, element: ElementId) {
        self.attention.retain(|&e| e != element);
    }

    /// The elements that asked for attention (INV-INPUT-9), oldest first.
    pub fn attention(&self) -> &[ElementId] {
        &self.attention
    }

    /// Focus history (tests / audit).
    pub fn history(&self) -> &[ElementId] {
        &self.history
    }
}

/// The window to go to from `from` in a direction: of the windows whose centre lies that way, the
/// one nearest along it, a window off to the side counting double. `windows`: their centres in
/// canvas coordinates (y grows downwards).
pub(crate) fn nearest_in_direction(
    from: (i64, i64),
    direction: crate::input::keys::Direction,
    windows: &[(ElementId, (i64, i64))],
) -> Option<ElementId> {
    use crate::input::keys::Direction;
    windows
        .iter()
        .filter_map(|&(id, (x, y))| {
            let (dx, dy) = (x - from.0, y - from.1);
            let (along, side) = match direction {
                Direction::Left => (-dx, dy),
                Direction::Right => (dx, dy),
                Direction::Up => (-dy, dx),
                Direction::Down => (dy, dx),
            };
            (along > 0).then_some((along + 2 * side.abs(), id))
        })
        .min_by_key(|&(score, _)| score)
        .map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_transitions_and_successor_policy() {
        use std::num::NonZeroU64;
        let mut fm = FocusModel::default();
        let e1 = ElementId::new(NonZeroU64::new(1).unwrap());
        let e2 = ElementId::new(NonZeroU64::new(2).unwrap());
        let e3 = ElementId::new(NonZeroU64::new(3).unwrap());

        // initial focus e1
        let fx = fm.set_focus(Some(e1));
        assert_eq!(
            fx,
            vec![
                Effect::ToplevelConfigure(e1),
                Effect::KeyboardFocus(Some(e1)),
            ]
        );
        assert_eq!(fm.target(), Some(e1));

        // transition e1 -> e2
        let fx = fm.set_focus(Some(e2));
        assert_eq!(
            fx,
            vec![
                Effect::ToplevelConfigure(e1),
                Effect::ToplevelConfigure(e2),
                Effect::KeyboardFocus(Some(e2)),
            ]
        );
        assert_eq!(fm.target(), Some(e2));
        assert_eq!(fm.history(), &[e2, e1]);

        // Super+Tab tapped among [e1, e2, e3]: toggles strictly between the two last focused (e2 <-> e1)
        let live = [e1, e2, e3];
        let (_, fx) = tap(&mut fm, &live);
        assert_eq!(fm.target(), Some(e1));
        assert_eq!(
            fx,
            vec![
                Effect::ToplevelConfigure(e2),
                Effect::ToplevelConfigure(e1),
                Effect::KeyboardFocus(Some(e1)),
            ]
        );

        // toggle again from e1 -> e2
        let (_, fx) = tap(&mut fm, &live);
        assert_eq!(fm.target(), Some(e2));
        assert_eq!(
            fx,
            vec![
                Effect::ToplevelConfigure(e1),
                Effect::ToplevelConfigure(e2),
                Effect::KeyboardFocus(Some(e2)),
            ]
        );

        // focus e3 explicitly, then target loss: e3 destroyed, live is [e1, e2].
        let _ = fm.set_focus(Some(e3));
        let fx = fm.target_lost(e3, &[e1, e2]);
        assert!(
            !fm.history().contains(&e3),
            "dead element retained in history"
        );
        assert_eq!(fm.target(), Some(e2));
        assert_eq!(
            fx,
            vec![
                Effect::ToplevelConfigure(e3),
                Effect::ToplevelConfigure(e2),
                Effect::KeyboardFocus(Some(e2)),
            ]
        );
    }

    #[test]
    fn initial_focus_eligibility_and_explicit_unfocus() {
        use std::num::NonZeroU64;
        let mut fm = FocusModel::default();
        let e1 = ElementId::new(NonZeroU64::new(1).unwrap());
        let e2 = ElementId::new(NonZeroU64::new(2).unwrap());

        assert!(fm.is_initial_eligible());
        assert_eq!(fm.target(), None);

        // 1. Initial activation grant to first toplevel succeeds
        let fx = fm.grant_initial_focus(e1);
        assert_eq!(
            fx,
            vec![
                Effect::ToplevelConfigure(e1),
                Effect::KeyboardFocus(Some(e1)),
            ]
        );
        assert_eq!(fm.target(), Some(e1));
        assert!(!fm.is_initial_eligible());

        // 2. Second toplevel mapping cannot steal focus
        let fx = fm.grant_initial_focus(e2);
        assert!(fx.is_empty(), "second window stole focus");
        assert_eq!(fm.target(), Some(e1));

        // 3. Explicit background click clears focus
        let fx = fm.clear_focus_explicit();
        assert_eq!(
            fx,
            vec![Effect::ToplevelConfigure(e1), Effect::KeyboardFocus(None),]
        );
        assert_eq!(fm.target(), None);
        assert!(
            !fm.is_initial_eligible(),
            "explicit unfocus must not remain eligible"
        );

        // 4. Repeated grant attempts while explicitly unfocused are rejected
        let fx = fm.grant_initial_focus(e2);
        assert!(fx.is_empty(), "mapping after explicit unfocus stole focus");
        assert_eq!(fm.target(), None);

        // 5. Explicit user click sets focus to e2
        let fx = fm.set_focus(Some(e2));
        assert_eq!(fm.target(), Some(e2));
        assert!(!fx.is_empty());

        // 6. When all windows are closed, eligibility re-arms
        fm.target_lost(e2, &[]);
        assert_eq!(fm.target(), None);
        assert!(
            fm.is_initial_eligible(),
            "eligibility re-arms when all windows are gone"
        );

        // 7. Newly mapped window receives initial focus again
        let fx = fm.grant_initial_focus(e1);
        assert_eq!(fm.target(), Some(e1));
        assert!(!fx.is_empty());
    }

    fn ids() -> [ElementId; 4] {
        use std::num::NonZeroU64;
        [1, 2, 3, 4].map(|i| ElementId::new(NonZeroU64::new(i).unwrap()))
    }

    /// Super+Tab tapped: a cycle of one step, committed as Super is let go. Returns the window
    /// asking for attention the cycle put first, and the step's effects.
    fn tap(fm: &mut FocusModel, live: &[ElementId]) -> (Option<ElementId>, Vec<Effect>) {
        let attention = fm.cycle_start(live, &[]);
        let fx = fm.cycle_step(false, |_| true).unwrap_or_default();
        fm.cycle_commit();
        (attention, fx)
    }

    /// Super+Tab tapped (Super let go each time) goes to the window used before the focused one:
    /// Tab, Tab returns, Tab, Tab, Tab is the other one.
    #[test]
    fn super_tab_taps_flip_between_the_last_two_windows() {
        let [a, b, c, _] = ids();
        let live = [a, b, c];
        let mut fm = FocusModel::default();
        for e in [a, b, c, b] {
            let _ = fm.set_focus(Some(e));
        }
        // used: b, c, a
        tap(&mut fm, &live);
        assert_eq!(fm.target(), Some(c), "the window used before b");
        tap(&mut fm, &live);
        assert_eq!(fm.target(), Some(b), "Tab, Tab returns");
        tap(&mut fm, &live);
        assert_eq!(fm.target(), Some(c), "only ever these two");
        assert_eq!(fm.history(), &[c, b, a]);
    }

    /// Super held (owner, 2026-11-28, as Alt+Tab on Windows): each Tab goes one further in the
    /// order of use and Shift+Tab one back, wrapping round; the order of use is the same until
    /// Super is let go, and then only the chosen window moves to the front.
    #[test]
    fn a_held_cycle_steps_in_the_order_of_use_and_commits_once() {
        let [a, b, c, d] = ids();
        let live = [a, b, c, d];
        let mut fm = FocusModel::default();
        for e in [a, b, c, d] {
            let _ = fm.set_focus(Some(e));
        }
        // used: d, c, b, a
        assert_eq!(fm.cycle_start(&live, &[]), None);
        assert!(fm.cycling());
        for expected in [c, b, a, d, c] {
            let fx = fm.cycle_step(false, |_| true).expect("a step");
            assert!(
                fx.contains(&Effect::KeyboardFocus(Some(expected))),
                "{fx:?}"
            );
            assert_eq!(fm.history(), &[d, c, b, a], "no reorder while held");
        }
        for expected in [d, a, b] {
            let _ = fm.cycle_step(true, |_| true);
            assert_eq!(fm.target(), Some(expected), "Shift+Tab steps back");
        }
        assert_eq!(fm.cycle_commit(), Some(b));
        assert!(!fm.cycling());
        assert_eq!(fm.history(), &[b, d, c, a], "the others keep their order");
    }

    /// Minimized windows come after the shown ones, in their order of use; a closed window is
    /// passed over; Escape ends the cycle with the focus it started from, and nothing reordered.
    #[test]
    fn minimized_come_last_closed_are_passed_over_and_escape_goes_back() {
        let [a, b, c, d] = ids();
        let mut fm = FocusModel::default();
        for e in [d, a, b, c] {
            let _ = fm.set_focus(Some(e));
        }
        // used: c, b, a, d; d is minimized and a closes during the cycle
        fm.cycle_start(&[a, b, c], &[d]);
        let _ = fm.cycle_step(false, |_| true);
        assert_eq!(fm.target(), Some(b));
        let _ = fm.cycle_step(false, |e| e != a);
        assert_eq!(fm.target(), Some(d), "a passed over, d the last");
        let _ = fm.cycle_step(false, |e| e != a);
        assert_eq!(fm.target(), Some(c), "round to the start");
        let _ = fm.cycle_step(false, |e| e != a);
        assert_eq!(
            fm.cycle_cancel(),
            Some(Some(c)),
            "the focus before the first Tab"
        );
        assert!(!fm.cycling());
        assert_eq!(fm.cycle_commit(), None, "nothing left to commit");
        assert_eq!(fm.history(), &[c, b, a, d]);
    }

    /// A window asking for attention is the first step only; the cycle goes on from there in the
    /// order of use.
    #[test]
    fn attention_is_only_the_first_step() {
        let [a, b, c, d] = ids();
        let live = [a, b, c, d];
        let mut fm = FocusModel::default();
        for e in [a, b, c, d] {
            let _ = fm.set_focus(Some(e));
        }
        fm.mark_attention(a);
        assert_eq!(fm.cycle_start(&live, &[]), Some(a));
        for expected in [a, c, b, d] {
            let _ = fm.cycle_step(false, |_| true);
            assert_eq!(fm.target(), Some(expected));
        }
        assert!(fm.attention().is_empty(), "its mark went as it was focused");
    }

    /// Super+arrow (owner, 2026-11-01): the nearest window that way; none that way, no move.
    #[test]
    fn the_arrows_go_to_the_nearest_window_that_way() {
        use crate::input::keys::Direction::{Down, Left, Right, Up};
        let [a, b, c, d] = ids();
        let windows = [
            (a, (1000, 0)),
            (b, (300, 900)),
            (c, (0, 500)),
            (d, (-400, 0)),
        ];
        assert_eq!(
            super::nearest_in_direction((0, 0), Right, &windows),
            Some(a)
        );
        assert_eq!(super::nearest_in_direction((0, 0), Left, &windows), Some(d));
        assert_eq!(super::nearest_in_direction((0, 0), Down, &windows), Some(c));
        assert_eq!(super::nearest_in_direction((0, 0), Up, &windows), None);
    }

    /// The pointer passing over a window on its way elsewhere does not make it the last window
    /// used (owner, 2026-11-01); a window it stays on does, and a click at once.
    #[test]
    fn a_window_the_pointer_only_passed_is_not_the_last_used() {
        let [a, b, c, _] = ids();
        let live = [a, b, c];
        let mut fm = FocusModel::default();
        for e in [a, b] {
            let _ = fm.set_focus(Some(e));
        }
        // from b, past c, onto a: c had the focus for a moment only
        let _ = fm.hover_focus(Some(c), 1_000_000);
        let _ = fm.hover_focus(Some(a), 1_050_000);
        fm.settle_hover(2_000_000);
        assert_eq!(fm.history()[..2], [a, b]);
        tap(&mut fm, &live);
        assert_eq!(fm.target(), Some(b), "back to b, not c");

        // a click on the window the pointer just focused counts at once
        let _ = fm.hover_focus(Some(c), 3_000_000);
        let _ = fm.set_focus(Some(c));
        assert_eq!(fm.history()[0], c);
    }

    /// Attention first (owner, 2026-10-31): Super+Tab goes to the window that most recently asked
    /// for attention and clears its mark; the next Super+Tab goes back to where the user was.
    #[test]
    fn attention_comes_first_then_super_tab_goes_back() {
        let [a, b, c, d] = ids();
        let live = [a, b, c, d];
        let mut fm = FocusModel::default();
        for e in [a, b] {
            let _ = fm.set_focus(Some(e));
        }
        fm.mark_attention(c);
        let _ = fm.activation(d, false, true);
        assert_eq!(fm.attention(), &[c, d]);
        let (attention, fx) = tap(&mut fm, &live);
        assert_eq!(attention, Some(d), "attention first");
        assert!(fx.contains(&Effect::KeyboardFocus(Some(d))), "{fx:?}");
        assert_eq!(fm.target(), Some(d), "the most recent mark");
        assert_eq!(fm.attention(), &[c], "its mark is cleared");
        // a new press goes to the next window asking
        assert_eq!(tap(&mut fm, &live).0, Some(c), "c asks");
        assert_eq!(fm.target(), Some(c));
        assert_eq!(fm.attention(), &[] as &[ElementId]);
        assert_eq!(tap(&mut fm, &live).0, None, "nobody asks now");
        assert_eq!(fm.target(), Some(d), "then the flip");
        // a mark on a window that may not be focused (hidden) is passed over
        fm.mark_attention(a);
        assert_eq!(fm.cycle_start(&[b, c, d], &[]), None);
    }

    /// A window loses its mark when it is focused any way: a click (set_focus), Super+Tab,
    /// or as the successor of a closed window. The focused window is never marked.
    #[test]
    fn a_mark_is_cleared_whenever_the_window_is_focused() {
        let [a, b, c, _] = ids();
        let live = [a, b, c];
        let mut fm = FocusModel::default();
        let _ = fm.set_focus(Some(a));
        fm.mark_attention(a);
        assert!(
            fm.attention().is_empty(),
            "the focused window is not marked"
        );
        fm.mark_attention(b);
        let _ = fm.set_focus(Some(b));
        assert!(fm.attention().is_empty(), "clicked");
        // used: b, a; c never focused, marked
        fm.mark_attention(c);
        let _ = fm.set_focus(Some(a));
        fm.mark_attention(c);
        tap(&mut fm, &live);
        assert_eq!(fm.target(), Some(c), "gone to");
        assert!(fm.attention().is_empty(), "gone to");
        // a successor
        fm.mark_attention(b);
        let _ = fm.target_lost(c, &[a, b]);
        let _ = fm.set_focus(Some(a));
        fm.mark_attention(b);
        let _ = fm.target_lost(a, &[b]);
        assert_eq!(fm.target(), Some(b));
        assert!(fm.attention().is_empty(), "the successor");
    }

    /// A closed window leaves the history; the flip goes on without it.
    #[test]
    fn a_closed_window_drops_out_of_the_history() {
        let [a, b, c, d] = ids();
        let mut fm = FocusModel::default();
        for e in [a, b, c, d] {
            let _ = fm.set_focus(Some(e));
        }
        // used: d, c, b, a; c closes, the one before d is b now
        let _ = fm.target_lost(c, &[a, b, d]);
        assert!(!fm.history().contains(&c));
        tap(&mut fm, &[a, b, d]);
        assert_eq!(fm.target(), Some(b));
        // the focused window closes: the successor is the last one used
        let _ = fm.target_lost(b, &[a, d]);
        assert_eq!(fm.target(), Some(d));
        assert_eq!(fm.history(), &[d, a]);
        tap(&mut fm, &[a, d]);
        assert_eq!(fm.target(), Some(a));
    }
}
