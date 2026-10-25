//! Adapter keyboard helpers and preemption configuration (C4.4, M12).
//!
//! Keyboard intake and ownership decisions are owned solely by Core intake
//! (`Core::physical_input`), returning `Effect::KeyboardForward` or `Effect::KeyboardIntercept`.
//! The adapter executes protocol effects only.
//!
//! Ordered preemption (INV-INPUT-10, M3, M11):
//! ActiveElementFocus transitions tear down popup grabs in one atomic batch:
//! 1. ungrab(All) -> popup_done topmost first;
//! 2. keyboard.unset_grab;
//! 3. pointer.unset_grab;
//! 4. keyboard.set_focus(target).

use smithay::input::keyboard::KeyboardHandle;

use crate::WaylandState;

/// Preemption strategies for popup grabs under focus changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PreemptionStrategy {
    /// Canonical ORDERED preemption (INV-INPUT-10): ungrab -> kb.unset -> ptr.unset -> set_focus.
    #[default]
    Ordered,
    /// NEG-1 negative control: naive set_focus without ungrabbing popup.
    Naive,
    /// NEG-2 negative control: pointer unset before keyboard unset (W-SM-15 violation).
    PointerFirst,
}

impl WaylandState {
    /// The seat's keyboard handle (internal to adapter).
    pub(crate) fn keyboard_handle(&self) -> KeyboardHandle<WaylandState> {
        self.seat.get_keyboard().expect("the seat has a keyboard")
    }

    /// Sets whether nested popup grabs preserve the installed pointer grab (W-SM-14).
    #[doc(hidden)]
    #[cfg(feature = "probe_nested_grab_keep_pointer")]
    pub fn set_nested_grab_keep_pointer(&mut self, keep: bool) {
        self.nested_grab_keep_pointer = keep;
    }

    /// Sets the preemption strategy under focus changes (default: Ordered).
    #[doc(hidden)]
    #[cfg(feature = "probe_preemption_strategy")]
    pub fn set_preemption_strategy(&mut self, strategy: PreemptionStrategy) {
        self.ordered_preemption_strategy = strategy;
    }
}
