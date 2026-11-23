//! Explicit sync (`linux-drm-syncobj-v1`): a client says through a timeline point when the GPU has
//! finished its frame (acquire) and is told through another when the compositor is done with the
//! buffer (release), instead of fences hidden in the buffer (implicit sync). Vulkan through Mesa and
//! NVIDIA's driver use it; xwayland-satellite passes it on for X11 games.
//!
//! Offered only when the backend's DRM device can wait for a timeline point through an eventfd
//! ([`WaylandState::enable_explicit_sync`]). A commit with an acquire point is held until the point
//! is signalled, as a frame the GPU is still drawing is (`handlers::wait_for_frame`), so neither
//! drawing nor direct scanout ever reads an unfinished buffer. Smithay signals the release point
//! when the last use of the buffer ends (drawn over, or replaced on the screen it was scanned out
//! to).

use smithay::backend::drm::DrmDeviceFd;
use smithay::wayland::drm_syncobj::{DrmSyncobjHandler, DrmSyncobjState, supports_syncobj_eventfd};

use crate::WaylandState;
use crate::log_line;

impl DrmSyncobjHandler for WaylandState {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.syncobj.as_mut()
    }
}

impl WaylandState {
    /// Offers explicit sync with the timelines of `device` (the GPU the compositor draws with),
    /// when the device supports it. The backend must also watch frames ([`Self::enable_frame_waits`]):
    /// that is where a held commit waits for its acquire point.
    pub fn enable_explicit_sync(&mut self, device: &DrmDeviceFd) {
        if self.syncobj.is_some() {
            return;
        }
        if supports_syncobj_eventfd(device) {
            self.syncobj = Some(DrmSyncobjState::new::<Self>(&self.dh, device.clone()));
            log_line!("[infynitae] Explicit sync offered (linux-drm-syncobj-v1)");
        } else {
            log_line!(
                "[infynitae] Explicit sync not offered: the GPU cannot wait for timeline points"
            );
        }
    }
}

smithay::delegate_drm_syncobj!(WaylandState);
