//! FIFO presentation (`wp_fifo_v1`): Vulkan's FIFO present mode through Mesa (games with vsync on,
//! DXVK through Proton) and video players. A commit that waits for the barrier the previous commit
//! set is held until that frame was used for a refresh; Smithay holds it (managed mode), this file
//! lifts the barriers.
//!
//! A barrier is lifted wherever the surface's frame callbacks complete: at the refresh of the
//! window's clock output (INV-FRAME-2, with GPU relief's pace for a window relief slows), and for a
//! window no screen shows at the kick and the fallback (INV-FRAME-3), so a hidden window runs at the
//! off-screen pace instead of being blocked. For that a barrier waiting to be lifted counts as a
//! requested frame in the window's snapshot (`snapshot.rs`), also for a client that asks for no
//! frame callback.

use infynitae_core::stage::ElementId;
use smithay::desktop::PopupManager;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::wayland::compositor::{SurfaceData, TraversalAction, with_surface_tree_downward};
use smithay::wayland::fifo::{FifoBarrierCachedState, FifoManagerState};

use crate::WaylandState;

/// Offers `wp_fifo_manager_v1`.
pub(crate) fn global(display: &DisplayHandle) -> FifoManagerState {
    FifoManagerState::new::<WaylandState>(display)
}

/// Whether the surface's current frame set a barrier that is not lifted yet (a commit waits, or
/// will wait, for it).
pub(crate) fn barrier_waiting(states: &SurfaceData) -> bool {
    states.cached_state.has::<FifoBarrierCachedState>()
        && states
            .cached_state
            .get::<FifoBarrierCachedState>()
            .current()
            .barrier
            .as_ref()
            .is_some_and(|b| !b.is_signaled())
}

/// Lifts the barriers of `root`'s surface tree; whether there was one.
fn lift_tree(root: &WlSurface) -> bool {
    let mut lifted = false;
    with_surface_tree_downward(
        root,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |_, states, _| {
            if !states.cached_state.has::<FifoBarrierCachedState>() {
                return;
            }
            let barrier = states
                .cached_state
                .get::<FifoBarrierCachedState>()
                .current()
                .barrier
                .take();
            if let Some(barrier) = barrier {
                barrier.signal();
                lifted = true;
            }
        },
        |_, _, _| true,
    );
    lifted
}

impl WaylandState {
    /// The surface tree of `root` was used for a refresh, or its frame callbacks completed
    /// without one (kick, fallback): its barriers are lifted, and the commits that waited for them
    /// apply at [`Self::apply_released`].
    pub(crate) fn lift_fifo_barriers(&mut self, root: &WlSurface) {
        if lift_tree(root)
            && let Some(client) = root.client()
            && !self.released.contains(&client)
        {
            self.released.push(client);
        }
    }

    /// [`Self::lift_fifo_barriers`] for a window: its root tree and every popup tree.
    pub(crate) fn lift_window_barriers(&mut self, element: ElementId) {
        let Some(root) = self.window_surface(element) else {
            return;
        };
        self.lift_fifo_barriers(&root);
        for (popup, _) in PopupManager::popups_for_surface(&root) {
            self.lift_fifo_barriers(popup.wl_surface());
        }
    }
}

smithay::delegate_fifo!(WaylandState);
