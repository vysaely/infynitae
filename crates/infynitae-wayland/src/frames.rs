//! Completion of withheld frame callbacks (M16): Core decides which unobserved elements complete
//! and when (the kick, the fallback); the adapter only completes them. An unobserved element has
//! no output, so its callbacks are completed directly rather than through Smithay's per-output
//! frame throttling; its presentation feedback stays pending until it is shown or superseded
//! (INV-FRAME-5).

use std::time::Duration;

use infynitae_core::protocol::Effect;
use infynitae_core::stage::ElementId;
use smithay::desktop::PopupManager;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::{
    SurfaceAttributes, TraversalAction, with_surface_tree_downward,
};

use crate::WaylandState;

/// Completes every callback committed on `root`'s surface tree, with `time_ms` as the
/// callback's timestamp.
fn complete_tree(root: &WlSurface, time_ms: u32) {
    with_surface_tree_downward(
        root,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |_, states, _| {
            for callback in states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .frame_callbacks
                .drain(..)
            {
                callback.done(time_ms);
            }
        },
        |_, _, _| true,
    );
}

impl WaylandState {
    /// Completes the withheld callbacks of an unobserved element, at `now` on the presentation
    /// clock: its root tree and every popup tree, once.
    pub fn complete_withheld(&mut self, element: ElementId, now: Duration) {
        let Some(root) = self
            .window(element)
            .and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()))
        else {
            return;
        };
        let time_ms = now.as_millis() as u32;
        complete_tree(&root, time_ms);
        for (popup, _) in PopupManager::popups_for_surface(&root) {
            complete_tree(popup.wl_surface(), time_ms);
        }
    }

    /// The element whose popup holds the active popup grab: the one client a `popup_done`
    /// reaches when the grab is dismissed.
    fn popup_grab_owner(&self) -> Option<ElementId> {
        let grab = self.popup_grab.as_ref().filter(|g| !g.has_ended())?;
        let root = self.toplevel_root(&grab.current_grab()?)?;
        self.element_index_by_root(&root)
            .map(|i| self.elements[i].id)
    }

    /// INV-FRAME-3 (a): before `effect` reaches a client, the withheld callbacks of each
    /// unobserved recipient Core names complete once. A dismissal reaches the owner of the
    /// active popup grab, whichever element Core named, and no client when no grab is active;
    /// the adapter, which holds the grab, tells Core which.
    pub(crate) fn kick_before(&mut self, effect: &Effect) {
        let grab_owner = match effect {
            Effect::DismissPopupGrab(_) => self.popup_grab_owner(),
            _ => None,
        };
        let kicked = self.core.frame_kick(effect, grab_owner);
        if kicked.iter().any(Option::is_some) {
            let now = crate::presentation_clock_now();
            for element in kicked.into_iter().flatten() {
                self.complete_withheld(element, now);
            }
        }
    }

    /// INV-FRAME-3 (b): the fallback evaluation at `now` (presentation clock). Completes the
    /// elements that are due and returns when the fallback timer must fire next; `None` means
    /// no timer (INV-IDLE-1). A backend calls this once per loop iteration, after the
    /// iteration's effects were applied.
    pub fn frame_fallback(&mut self, now: Duration) -> Option<Duration> {
        for element in self.core.frame_fallback(now) {
            self.complete_withheld(element, now);
        }
        self.core.frame_fallback_deadline()
    }
}
