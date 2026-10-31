//! OBJ-22: ActiveElementFocus and focus history (M11, INV-INPUT-8).
//!
//! Core's FocusModel is the sole policy authority for desktop keyboard focus. It changes only by
//! explicit action (click-to-focus, focus-navigation bindings, shell focus command, activation grant)
//! or target loss (surface destroyed or unmapped). Cameras, visibility, and pointer motion never
//! change it.

use crate::input::bindings::Modifiers;
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
    /// A Super+Tab step-through in progress ([`Cycle`]).
    cycle: Option<Cycle>,
}

/// A Super+Tab step-through (owner, 2026-10-31, "like Alt+Tab"): while Super stays held, each
/// Tab steps one window further back through the recently used ones, in the order they had when
/// the first Tab was pressed. The history is left alone meanwhile; letting Super go ends it and
/// makes the window it stopped on the most recent ([`FocusModel::end_cycle`]).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Cycle {
    /// The windows to step through, most recently used first, as they were at the first Tab.
    order: Vec<ElementId>,
    /// The modifiers held at the first Tab: letting any of them go ends the step-through.
    held: Modifiers,
}

impl Default for FocusModel {
    fn default() -> Self {
        Self {
            target: None,
            history: Vec::new(),
            initial_eligible: true,
            attention: Vec::new(),
            activation_policy: ActivationPolicy::Strict,
            layer: None,
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
    pub fn clear_focus_explicit(&mut self) -> Vec<Effect> {
        self.initial_eligible = false;
        self.set_focus(None)
    }

    /// Sets keyboard focus directly via an explicit action (click-to-focus, activation, shell).
    /// A Super+Tab step-through in progress ends first, keeping the window it reached as used.
    pub fn set_focus(&mut self, next: Option<ElementId>) -> Vec<Effect> {
        self.end_cycle();
        self.focus_to(next, true)
    }

    /// Moves the focus to `next`; `record`: it becomes the most recently used window (not while a
    /// Super+Tab step-through runs). A window loses its attention mark when it is focused, any
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
                self.history.retain(|&e| e != id);
                self.history.insert(0, id);
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

    /// FocusNext binding (Super+Tab, owner 2026-10-31): the recently used windows, most recent
    /// first. A tap goes to the window used before the focused one, so two taps flip between the
    /// last two. `held`: the modifiers held with the key. While they stay held, another tap steps
    /// one window further back (wrapping round); letting them go ends the step-through
    /// ([`Self::modifiers_changed`]). With no modifier held there is nothing to hold, and every
    /// press is a tap. `live_elements`: the windows that may be focused, bottom to top; those
    /// never focused come after the used ones, the newest first.
    pub fn focus_next(&mut self, live_elements: &[ElementId], held: Modifiers) -> Vec<Effect> {
        self.initial_eligible = false;
        if live_elements.is_empty() {
            self.cycle = None;
            return self.set_focus(None);
        }
        // Tab again while held: one step further back from where the step-through is now
        if let Some(mut cycle) = self.cycle.take() {
            cycle.order.retain(|e| live_elements.contains(e));
            if !cycle.order.is_empty() {
                let at = self
                    .target
                    .and_then(|t| cycle.order.iter().position(|&e| e == t));
                let next = cycle.order[at.map_or(0, |i| (i + 1) % cycle.order.len())];
                let fx = self.focus_to(Some(next), false);
                self.cycle = Some(cycle);
                return fx;
            }
        }
        let order = self.recently_used(live_elements);
        // the window used before the focused one; with nothing focused, the last one used
        let next = match self.target {
            Some(cur) if order.first() == Some(&cur) => order[1 % order.len()],
            _ => order[0],
        };
        self.step(order, next, held)
    }

    /// Attention first (owner, 2026-10-31): a Super+Tab that starts a step-through goes to the
    /// window that most recently asked for attention, if it may be focused, and clears its mark;
    /// it is then the most recent window, so the next tap goes back. `None` when no window asks
    /// for attention, or a step-through is already running (Tab steps on).
    pub fn focus_attention(
        &mut self,
        live_elements: &[ElementId],
        held: Modifiers,
    ) -> Option<Vec<Effect>> {
        if self.cycle.is_some() {
            return None;
        }
        let marked = self
            .attention
            .iter()
            .rev()
            .copied()
            .find(|e| live_elements.contains(e))?;
        // recorded at once: the window that asked is the most recent one, the focused one before
        // it comes next
        let mut fx = self.set_focus(Some(marked));
        let order = self.recently_used(live_elements);
        fx.extend(self.step(order, marked, held));
        Some(fx)
    }

    /// Focuses `next` as a step of a step-through over `order`, which runs on while `held` stay
    /// held; with no modifier held it ends at once.
    fn step(&mut self, order: Vec<ElementId>, next: ElementId, held: Modifiers) -> Vec<Effect> {
        let fx = self.focus_to(Some(next), false);
        self.cycle = Some(Cycle { order, held });
        if held == Modifiers::default() {
            self.end_cycle();
        }
        fx
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

    /// The seat's modifiers changed (`now`: those held now): a step-through whose modifiers are
    /// no longer all held ends (Super let go).
    pub fn modifiers_changed(&mut self, now: Modifiers) {
        let Some(cycle) = &self.cycle else {
            return;
        };
        let h = cycle.held;
        let still = (!h.logo || now.logo)
            && (!h.ctrl || now.ctrl)
            && (!h.alt || now.alt)
            && (!h.shift || now.shift);
        if !still {
            self.end_cycle();
        }
    }

    /// Ends a Super+Tab step-through: the window it stopped on becomes the most recently used;
    /// the others keep the order they had before it.
    pub fn end_cycle(&mut self) {
        if self.cycle.take().is_none() {
            return;
        }
        if let Some(id) = self.target {
            self.history.retain(|&e| e != id);
            self.history.insert(0, id);
        }
    }

    /// Whether a Super+Tab step-through is running (Super still held since the first Tab).
    pub fn cycling(&self) -> bool {
        self.cycle.is_some()
    }

    /// Target loss (K7, M11): focused element was destroyed or unmapped.
    /// The successor policy selects the most recently focused live element.
    pub fn target_lost(&mut self, dead: ElementId, live_elements: &[ElementId]) -> Vec<Effect> {
        self.history.retain(|&e| e != dead);
        if live_elements.is_empty() {
            self.initial_eligible = true;
        }
        self.attention.retain(|&e| e != dead);
        // a closed window drops out of a step-through; the step-through ends with its window
        if let Some(cycle) = &mut self.cycle {
            cycle.order.retain(|&e| e != dead);
        }
        if self.target != Some(dead) {
            return Vec::new();
        }
        self.cycle = None;
        let successor = self
            .history
            .iter()
            .copied()
            .find(|e| live_elements.contains(e));
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

    /// The layer surface holding the keyboard, if any (L5).
    pub fn layer(&self) -> Option<crate::layer::LayerId> {
        self.layer.map(|(l, _)| l)
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

        // focus next among [e1, e2, e3]: toggles strictly between the two last focused (e2 <-> e1)
        let live = [e1, e2, e3];
        let fx = fm.focus_next(&live, Modifiers::default());
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
        let fx = fm.focus_next(&live, Modifiers::default());
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

    const SUPER: Modifiers = Modifiers {
        logo: true,
        ctrl: false,
        alt: false,
        shift: false,
    };

    /// Super+Tab (owner, 2026-10-31): a tap goes to the window used before the focused one, so two
    /// taps (Super let go between them) come back: Tab, Tab returns.
    #[test]
    fn super_tab_flips_between_the_last_two_windows() {
        let [a, b, c, _] = ids();
        let live = [a, b, c];
        let mut fm = FocusModel::default();
        for e in [a, b, c, b] {
            let _ = fm.set_focus(Some(e));
        }
        // used: b, c, a
        let _ = fm.focus_next(&live, SUPER);
        fm.modifiers_changed(Modifiers::default());
        assert_eq!(fm.target(), Some(c), "the window used before b");
        let _ = fm.focus_next(&live, SUPER);
        fm.modifiers_changed(Modifiers::default());
        assert_eq!(fm.target(), Some(b), "Tab, Tab returns");
        assert_eq!(fm.history(), &[b, c, a]);
    }

    /// Holding Super, each further Tab steps one window further back (wrapping round); the
    /// history is left alone until Super is let go, and then the chosen window is the most recent.
    #[test]
    fn holding_super_steps_further_back_and_letting_go_chooses() {
        let [a, b, c, d] = ids();
        let live = [a, b, c, d];
        let mut fm = FocusModel::default();
        for e in [a, b, c, d] {
            let _ = fm.set_focus(Some(e));
        }
        // used: d, c, b, a
        let _ = fm.focus_next(&live, SUPER);
        assert_eq!(fm.target(), Some(c));
        assert!(fm.cycling());
        let _ = fm.focus_next(&live, SUPER);
        assert_eq!(fm.target(), Some(b), "one further back");
        assert_eq!(fm.history(), &[d, c, b, a], "untouched while held");
        // Shift pressed as well keeps it going; only Super let go ends it
        fm.modifiers_changed(Modifiers {
            shift: true,
            ..SUPER
        });
        assert!(fm.cycling());
        let _ = fm.focus_next(&live, SUPER);
        let _ = fm.focus_next(&live, SUPER);
        assert_eq!(fm.target(), Some(d), "wraps round to the start");
        let _ = fm.focus_next(&live, SUPER);
        assert_eq!(fm.target(), Some(c));
        fm.modifiers_changed(Modifiers::default());
        assert!(!fm.cycling());
        assert_eq!(
            fm.history(),
            &[c, d, b, a],
            "the chosen one is the most recent"
        );
        // the next tap flips back to the window left before
        let _ = fm.focus_next(&live, SUPER);
        assert_eq!(fm.target(), Some(d));
    }

    /// A press with no modifier held (a user binding such as F5) is always a tap.
    #[test]
    fn a_binding_without_modifiers_is_always_a_tap() {
        let [a, b, c, _] = ids();
        let live = [a, b, c];
        let mut fm = FocusModel::default();
        for e in [a, b, c] {
            let _ = fm.set_focus(Some(e));
        }
        let _ = fm.focus_next(&live, Modifiers::default());
        assert!(!fm.cycling());
        let _ = fm.focus_next(&live, Modifiers::default());
        assert_eq!(fm.target(), Some(c), "flips back, never steps on");
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
        let fx = fm.focus_attention(&live, SUPER).expect("attention first");
        assert!(fx.contains(&Effect::KeyboardFocus(Some(d))), "{fx:?}");
        assert_eq!(fm.target(), Some(d), "the most recent mark");
        assert_eq!(fm.attention(), &[c], "its mark is cleared");
        // Tab again while held: a step-through, not another attention jump
        assert_eq!(fm.focus_attention(&live, SUPER), None);
        let _ = fm.focus_next(&live, SUPER);
        assert_eq!(fm.target(), Some(b), "back to where the user was");
        fm.modifiers_changed(Modifiers::default());
        // a new press goes to the next window asking
        let _ = fm.focus_attention(&live, SUPER).expect("c asks");
        fm.modifiers_changed(Modifiers::default());
        assert_eq!(fm.target(), Some(c));
        assert!(fm.attention().is_empty());
        let _ = fm.focus_next(&live, SUPER);
        fm.modifiers_changed(Modifiers::default());
        assert_eq!(fm.target(), Some(b), "then the toggle again");
        // a mark on a window that may not be focused (hidden) is passed over
        fm.mark_attention(a);
        assert_eq!(fm.focus_attention(&[b, c, d], SUPER), None);
    }

    /// A window loses its mark when it is focused any way: a click (set_focus), a step-through,
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
        let _ = fm.focus_next(&live, SUPER);
        let _ = fm.focus_next(&live, SUPER);
        assert_eq!(fm.target(), Some(c), "stepped to");
        assert!(fm.attention().is_empty(), "stepped to");
        fm.modifiers_changed(Modifiers::default());
        // a successor
        fm.mark_attention(b);
        let _ = fm.target_lost(c, &[a, b]);
        let _ = fm.set_focus(Some(a));
        fm.mark_attention(b);
        let _ = fm.target_lost(a, &[b]);
        assert_eq!(fm.target(), Some(b));
        assert!(fm.attention().is_empty(), "the successor");
    }

    /// A closed window leaves the history and a running step-through; one that was the
    /// step-through's window ends it.
    #[test]
    fn a_closed_window_drops_out_of_the_history_and_the_step_through() {
        let [a, b, c, d] = ids();
        let mut fm = FocusModel::default();
        for e in [a, b, c, d] {
            let _ = fm.set_focus(Some(e));
        }
        let _ = fm.focus_next(&[a, b, c, d], SUPER);
        assert_eq!(fm.target(), Some(c));
        // b closes while Super is held: the next Tab skips it
        let _ = fm.target_lost(b, &[a, c, d]);
        assert!(!fm.history().contains(&b));
        let _ = fm.focus_next(&[a, c, d], SUPER);
        assert_eq!(fm.target(), Some(a), "b is gone");
        // the window the step-through is on closes: it ends, the successor is the last used
        let _ = fm.target_lost(a, &[c, d]);
        assert!(!fm.cycling());
        assert_eq!(fm.target(), Some(d));
        assert_eq!(fm.history(), &[d, c]);
        let _ = fm.focus_next(&[c, d], SUPER);
        fm.modifiers_changed(Modifiers::default());
        assert_eq!(fm.target(), Some(c));
    }
}
