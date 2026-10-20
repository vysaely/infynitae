//! OBJ-22: ActiveElementFocus and focus history (M11, INV-INPUT-8).
//!
//! Core's FocusModel is the sole policy authority for desktop keyboard focus. It changes only by
//! explicit action (click-to-focus, focus-navigation bindings, shell focus command, activation grant)
//! or target loss (surface destroyed or unmapped). Cameras, visibility, and pointer motion never
//! change it.

use crate::protocol::Effect;
use crate::stage::ElementId;

/// What an `xdg_activation_v1` request may do (INV-INPUT-9; PROV-10, provisional; OB-40 decides).
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
    pub fn set_focus(&mut self, next: Option<ElementId>) -> Vec<Effect> {
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
            self.history.retain(|&e| e != id);
            self.history.insert(0, id);
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

    /// FocusNext binding (Super+Enter): cycles to the next live element in stage order.
    pub fn focus_next(&mut self, live_elements: &[ElementId]) -> Vec<Effect> {
        self.initial_eligible = false;
        if live_elements.is_empty() {
            return self.set_focus(None);
        }
        let next = match self.target {
            Some(cur) => {
                let pos = live_elements.iter().position(|&e| e == cur).unwrap_or(0);
                live_elements[(pos + 1) % live_elements.len()]
            }
            None => live_elements[0],
        };
        self.set_focus(Some(next))
    }

    /// Target loss (K7, M11): focused element was destroyed or unmapped.
    /// The successor policy selects the most recently focused live element.
    pub fn target_lost(&mut self, dead: ElementId, live_elements: &[ElementId]) -> Vec<Effect> {
        self.history.retain(|&e| e != dead);
        if live_elements.is_empty() {
            self.initial_eligible = true;
        }
        self.attention.retain(|&e| e != dead);
        if self.target != Some(dead) {
            return Vec::new();
        }
        let successor = self
            .history
            .iter()
            .copied()
            .find(|e| live_elements.contains(e));
        let prev = self.target;
        self.target = successor;
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

    /// An activation request for `element` (INV-INPUT-9; outcome policy PROV-10, provisional):
    /// focus when the policy lets this request focus (`validated`: its token passed the
    /// provenance check) and the element may take focus (`eligible`), attention otherwise.
    /// Never a camera change.
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

        // focus next among [e1, e2, e3]
        let live = [e1, e2, e3];
        let fx = fm.focus_next(&live);
        assert_eq!(fm.target(), Some(e3));
        assert_eq!(
            fx,
            vec![
                Effect::ToplevelConfigure(e2),
                Effect::ToplevelConfigure(e3),
                Effect::KeyboardFocus(Some(e3)),
            ]
        );

        // cycle again from e3 -> e1
        let fx = fm.focus_next(&live);
        assert_eq!(fm.target(), Some(e1));
        assert_eq!(
            fx,
            vec![
                Effect::ToplevelConfigure(e3),
                Effect::ToplevelConfigure(e1),
                Effect::KeyboardFocus(Some(e1)),
            ]
        );

        // target loss: e1 destroyed, live is [e2, e3].
        // history is [e1, e3, e2]. Successor should be e3!
        let fx = fm.target_lost(e1, &[e2, e3]);
        assert!(
            !fm.history().contains(&e1),
            "dead element retained in history"
        );
        assert_eq!(fm.target(), Some(e3));
        assert_eq!(
            fx,
            vec![
                Effect::ToplevelConfigure(e1),
                Effect::ToplevelConfigure(e3),
                Effect::KeyboardFocus(Some(e3)),
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
}
