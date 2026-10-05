//! OBJ-22: ActiveElementFocus and focus history (M11, INV-INPUT-8).
//!
//! Core's FocusModel is the sole policy authority for desktop keyboard focus. It changes only by
//! explicit action (click-to-focus, focus-navigation bindings, shell focus command, activation grant)
//! or target loss (surface destroyed or unmapped). Cameras, visibility, and pointer motion never
//! change it.

use crate::protocol::Effect;
use crate::stage::ElementId;

/// Authoritative keyboard focus model for the seat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusModel {
    target: Option<ElementId>,
    /// Focus history: most recently focused live elements first.
    history: Vec<ElementId>,
}

impl FocusModel {
    pub fn target(&self) -> Option<ElementId> {
        self.target
    }

    /// Sets keyboard focus directly via an explicit action (click-to-focus, activation, shell).
    pub fn set_focus(&mut self, next: Option<ElementId>) -> Vec<Effect> {
        if self.target == next {
            return Vec::new();
        }
        let prev = self.target;
        self.target = next;
        if let Some(id) = next {
            self.history.retain(|&e| e != id);
            self.history.insert(0, id);
        }
        let mut effects = Vec::new();
        if let Some(prev_id) = prev {
            effects.push(Effect::ToplevelConfigure(prev_id));
        }
        if let Some(new_id) = next {
            effects.push(Effect::ToplevelConfigure(new_id));
        }
        effects.push(Effect::KeyboardFocus(next));
        effects
    }

    /// FocusNext binding (Super+Enter): cycles to the next live element in stage order.
    pub fn focus_next(&mut self, live_elements: &[ElementId]) -> Vec<Effect> {
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
        if self.target != Some(dead) {
            return Vec::new();
        }
        let successor = self
            .history
            .iter()
            .copied()
            .find(|e| live_elements.contains(e))
            .or_else(|| live_elements.last().copied());
        let prev = self.target;
        self.target = successor;
        let mut effects = Vec::new();
        if let Some(prev_id) = prev {
            effects.push(Effect::ToplevelConfigure(prev_id));
        }
        if let Some(next_id) = successor {
            effects.push(Effect::ToplevelConfigure(next_id));
        }
        effects.push(Effect::KeyboardFocus(successor));
        effects
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
}
