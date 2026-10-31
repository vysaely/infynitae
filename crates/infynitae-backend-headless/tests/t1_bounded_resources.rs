//! Bounded descriptor usage across repeated Lockstep cycles (INC3-F09), in a test binary of its own
//! so no other test shares the process whose `/proc/self/fd` it counts.

mod common;

use common::{Lockstep, output_k};
use infynitae_backend_headless::VirtualOutput;
use infynitae_core::output::Transform;

fn rotated(o: Transform, scale_120: u32) -> VirtualOutput {
    let mut v = output_k("A", (1920, 1080), scale_120, 60_000);
    v.transform = o;
    v
}

/// W's root plus a 12×16 grid of 167×131 tiles around the camera centre, Canvas
/// [−1002, 1002) × [−1048, 1048).
fn tile_view(ls: &mut Lockstep) {
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (40, 30), 0);
    // the window geometry is the root alone (40x30), centred on the camera: the root origin is
    // the camera centre − (20, 15), and the grid is offset by (20, 15) to stay around the centre
    let xdg = ls.probe.surfaces[w].xdg.clone().expect("xdg");
    xdg.set_window_geometry(0, 0, 40, 30);
    ls.pump();
    for iy in 0..16 {
        for ix in 0..12 {
            let off = (20 - 1002 + ix * 167, 15 - 1048 + iy * 131);
            ls.probe.subsurface(&qh, "T", w, off, (167, 131), false);
        }
    }
    ls.pump();
}

/// Bounded descriptor usage across repeated Lockstep construction/destruction cycles (INC3-F09).
/// Shared shm pool and Lockstep Drop client cleanup ensure file descriptors are not leaked.
///
/// This test is alone in its own test binary on purpose: `/proc/self/fd` is per process, so any
/// other test running in a parallel thread would add its own descriptors to the count and fail it
/// at random. With one test in the process, only this test's cycles can change the number.
#[test]
fn inc3_f09_bounded_resource_cycles() {
    #[cfg(target_os = "linux")]
    let fd_count = || -> usize {
        std::fs::read_dir("/proc/self/fd")
            .map(|entries| entries.flatten().count())
            .unwrap_or(0)
    };

    #[cfg(target_os = "linux")]
    let before = fd_count();

    for _ in 0..20 {
        let mut ls = Lockstep::new(Some(rotated(Transform::Normal, 120)));
        tile_view(&mut ls);
        drop(ls);
    }

    #[cfg(target_os = "linux")]
    {
        let after = fd_count();
        // Allow up to 5 descriptors of variance for standard test runner background threads.
        // The uncorrected leak would accumulate 20 * 193 = 3860 descriptors.
        assert!(
            after <= before + 5,
            "INC3-F09 regression: file descriptors accumulated across construction/destruction cycles (before: {before}, after: {after})"
        );
    }
}
