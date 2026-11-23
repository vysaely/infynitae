//! Explicit sync (`linux-drm-syncobj-v1`, `syncobj`) is offered only by a backend whose DRM device
//! can wait for timeline points (`WaylandState::enable_explicit_sync`). The headless backend has no
//! DRM device: a client must not see the global, or it would hand over frames nothing waits for.

mod common;

use common::Lockstep;
use common::input::a_1080;

#[test]
fn without_a_drm_device_explicit_sync_is_not_offered() {
    let ls = Lockstep::new(Some(a_1080()));
    assert!(
        ls.probe
            .globals
            .iter()
            .any(|(_, i, _)| i == "wp_fifo_manager_v1"),
        "the globals were read"
    );
    assert!(
        !ls.probe
            .globals
            .iter()
            .any(|(_, i, _)| i == "wp_linux_drm_syncobj_manager_v1")
    );
}
