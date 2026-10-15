//! OB-42 (INV-ALLOC-1, C14.3) instrumentation: heap allocations of Core's hot paths, counted
//! by a test-only global allocator (UNSAFE_INVENTORY U-002 … U-004).
//!
//! This is a **ratchet, not the contract**. The contract is zero allocations after warm-up; it
//! needs Core entries that write into caller-owned buffers instead of returning a `Vec` of
//! effects, which no branch has. What this file asserts:
//!
//! - no hot path allocates more than it does today (the counts below, recorded on this branch);
//! - the counts do not depend on the number of windows on the Canvas (no per-window allocation,
//!   so nothing proportional to, or quadratic in, the number of windows);
//! - repeating an event repeats its count (no growth across repeated events).
//!
//! Counting is per thread: the allocator counts only on a thread that switched counting on,
//! and only between the two calls of [`count`], so other tests of this binary, the harness and
//! output capture are never counted. Core is single-threaded; nothing it calls allocates on
//! another thread.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::num::NonZeroU64;

use infynitae_core::geom::{LocalPoint, LocalRect, LocalSize};
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::snapshot::{
    InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree,
};
use infynitae_core::stage::ElementId;
use infynitae_core::{Core, InputPort, PhysicalEvent};

/// Counts allocations made on the current thread while it counts; forwards everything to the
/// system allocator unchanged.
struct Counting;

thread_local! {
    /// `Some(n)`: this thread counts, and has made `n` allocations since it started counting.
    static COUNT: Cell<Option<u64>> = const { Cell::new(None) };
}

// SAFETY (U-002): every method forwards its arguments unchanged to `System`, which upholds the
// `GlobalAlloc` contract; the only addition is a thread-local counter of a `Copy` type with a
// constant initialiser and no destructor, which neither allocates nor unwinds.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get().map(|n| n + 1)));
        // SAFETY (U-003): `layout` comes from our caller, who must meet `alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY (U-004): `ptr` was returned by `alloc` above, i.e. by `System`, with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The allocations `f` makes on this thread. `realloc` and `alloc_zeroed` are the trait's
/// defaults, which call `alloc`: a reallocation counts as one allocation.
fn count(f: impl FnOnce()) -> u64 {
    COUNT.with(|c| c.set(Some(0)));
    f();
    COUNT.with(|c| c.replace(None)).expect("counting was on")
}

const LOGO: Modifiers = Modifiers {
    logo: true,
    ctrl: false,
    alt: false,
    shift: false,
};
const NONE: Modifiers = Modifiers {
    logo: false,
    ctrl: false,
    alt: false,
    shift: false,
};

fn desc(name: &str) -> OutputDescriptor {
    OutputDescriptor {
        key: OutputKey {
            device: "t0".into(),
            connector: name.into(),
            make: "x".into(),
            model: "y".into(),
            serial: None,
        },
        mode: (2560, 1440),
        transform: Transform::Normal,
        scale_120: 120,
        refresh_mhz: 60_000,
    }
}

/// A 640x480 window with two 50x50 subsurfaces (the `core_bench` window).
fn tree(key: u64, mapped: bool) -> SurfaceTree {
    let k = |n: u64| SurfaceKey::from_raw(NonZeroU64::new(n).unwrap());
    let subs = if mapped { 2 } else { 0 };
    let mut surfaces = Vec::new();
    for i in 0..subs {
        surfaces.push(SnapshotSurface {
            key: k(key * 16 + 1 + i),
            role: SurfaceRole::Subsurface {
                parent: subs as usize,
                offset: LocalPoint {
                    x: 10 * i as i32,
                    y: 10,
                },
            },
            size: LocalSize { w: 50, h: 50 },
            mapped,
            input: InputRegion::Infinite,
        });
    }
    surfaces.push(SnapshotSurface {
        key: k(key * 16),
        role: SurfaceRole::Root,
        size: if mapped {
            LocalSize { w: 640, h: 480 }
        } else {
            LocalSize::default()
        },
        mapped,
        input: InputRegion::Infinite,
    });
    SurfaceTree {
        geometry: LocalRect {
            loc: LocalPoint::default(),
            size: LocalSize { w: 640, h: 480 },
        },
        surfaces,
    }
}

fn motion(dx: f64, dy: f64) -> PhysicalEvent {
    PhysicalEvent::PointerMotion {
        delta: (dx, dy),
        delta_unaccel: (dx, dy),
        time_us: 0,
    }
}

fn pan_button(pressed: bool) -> PhysicalEvent {
    PhysicalEvent::PointerButton {
        button: 0x112,
        pressed,
        modifiers: LOGO,
        time_us: 0,
    }
}

fn key(pressed: bool) -> PhysicalEvent {
    PhysicalEvent::KeyboardKey {
        key: 30,
        pressed,
        modifiers: NONE,
        mods_changed: false,
        time_us: 0,
    }
}

/// `n` windows on a 700 px grid, 40 per row, the output's camera on the first one; the first
/// window has keyboard focus and the pointer is over it.
fn canvas(n: usize) -> (Core, InputPort, OutputId, Vec<ElementId>) {
    let (mut core, mut port, _shell, mut token) = Core::new(PersistedCameras::default());
    let (o, _) = core.output_connected(&mut token, desc("A")).unwrap();
    let mut ids = Vec::new();
    for i in 0..n {
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(i as u64 + 1, false))
            .unwrap();
        core.toplevel_committed(id, tree(i as u64 + 1, true))
            .unwrap();
        ids.push(id);
        // move the camera one grid cell on, so the next window is placed there
        core.physical_input(&mut port, pan_button(true)).unwrap();
        let dy = if i % 40 == 39 { -520.0 } else { 0.0 };
        core.physical_input(&mut port, motion(-700.0, dy)).unwrap();
        core.physical_input(&mut port, pan_button(false)).unwrap();
    }
    let camera = core.cameras().camera_for_output(o).unwrap();
    let c = core.cameras().center(camera).unwrap();
    core.physical_input(&mut port, pan_button(true)).unwrap();
    core.physical_input(
        &mut port,
        motion(c.x as f64 / 65536.0, c.y as f64 / 65536.0),
    )
    .unwrap();
    core.physical_input(&mut port, pan_button(false)).unwrap();
    core.grant_initial_focus(&mut port, ids[0]).unwrap();
    (core, port, o, ids)
}

/// Allocations of each hot-path event, `ROUNDS` times after `WARM_UP` rounds.
const WARM_UP: usize = 20;
const ROUNDS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Path {
    /// A pan step that changes no membership (the whole drag, minus its first step).
    Pan,
    /// A pan step by one grid cell, back and forth: windows enter and leave the view.
    PanCrossing,
    /// Pointer motion over a window: hit-test and routing.
    Route,
    /// A key press and release to the focused window.
    Key,
    /// A commit of the visible, focused window: its visibility and scale evaluation.
    Commit,
    /// A commit of a window outside every view.
    CommitOffView,
    /// The render list of the output.
    RenderList,
    /// The frame tick of the output.
    FrameTick,
}

const PATHS: [Path; 8] = [
    Path::Pan,
    Path::PanCrossing,
    Path::Route,
    Path::Key,
    Path::Commit,
    Path::CommitOffView,
    Path::RenderList,
    Path::FrameTick,
];

/// Each path's counts, round by round, on a Canvas of `n` windows.
fn measure(n: usize) -> Vec<(Path, Vec<u64>)> {
    let (mut core, mut port, o, ids) = canvas(n);
    let (first, last) = (ids[0], *ids.last().unwrap());
    let mut out = Vec::new();
    for path in PATHS {
        let mut counts = Vec::new();
        if matches!(path, Path::Pan | Path::PanCrossing) {
            core.physical_input(&mut port, pan_button(true)).unwrap();
            // the first step of a drag opens every element's epoch; not the steady state
            core.physical_input(&mut port, motion(1.0, 0.0)).unwrap();
            core.physical_input(&mut port, motion(-1.0, 0.0)).unwrap();
        }
        for round in 0..WARM_UP + ROUNDS {
            let sign = if round % 2 == 0 { 1.0 } else { -1.0 };
            let c = match path {
                Path::Pan => count(|| {
                    core.physical_input(&mut port, motion(3.0 * sign, 0.0))
                        .unwrap();
                }),
                Path::PanCrossing => count(|| {
                    core.physical_input(&mut port, motion(-700.0 * sign, 0.0))
                        .unwrap();
                }),
                Path::Route => count(|| {
                    core.physical_input(&mut port, motion(sign, 0.0)).unwrap();
                }),
                Path::Key => count(|| {
                    core.physical_input(&mut port, key(true)).unwrap();
                    core.physical_input(&mut port, key(false)).unwrap();
                }),
                Path::Commit => {
                    let t = tree(1, true);
                    count(|| {
                        core.toplevel_committed(first, t).unwrap();
                    })
                }
                Path::CommitOffView => {
                    let t = tree(n as u64, true);
                    count(|| {
                        core.toplevel_committed(last, t).unwrap();
                    })
                }
                Path::RenderList => count(|| {
                    core.render_list(o).unwrap();
                }),
                Path::FrameTick => count(|| {
                    core.frame_tick(o);
                }),
            };
            if round >= WARM_UP {
                counts.push(c);
            }
        }
        if matches!(path, Path::Pan | Path::PanCrossing) {
            core.physical_input(&mut port, pan_button(false)).unwrap();
        }
        out.push((path, counts));
    }
    out
}

/// The most each path may allocate per event (debug build, the Canvas above), measured on
/// `main` at `ccb82c5` and on this branch with this file: the counts are equal, so the INC-6
/// scale policy adds no allocation to these paths. (`40ddd7d`, before P-1 was repaired, allocated
/// 5,028 per pan step and 5,022 per commit at 1,000 windows.) Lower a count when a change removes
/// allocations; raising one needs a recorded reason. The contract (OB-42) is 0 for every path.
const RATCHET: [(Path, u64); 8] = [
    (Path::Pan, 15),
    (Path::PanCrossing, 20),
    (Path::Route, 1),
    (Path::Key, 2),
    (Path::Commit, 9),
    (Path::CommitOffView, 4),
    (Path::RenderList, 15),
    (Path::FrameTick, 4),
];

/// OB-42 ratchet (not the contract; see the file comment): on Canvases of 10, 100 and 1,000
/// windows, every hot path allocates at most its recorded count, exactly as much whatever the
/// number of windows, and the same again each time the same event repeats.
#[test]
fn hot_path_allocations_do_not_grow() {
    let sizes = [10, 100, 1000];
    let runs: Vec<Vec<(Path, Vec<u64>)>> = sizes.iter().map(|&n| measure(n)).collect();
    for (n, run) in sizes.iter().zip(&runs) {
        for ((path, counts), (ratchet_path, ceiling)) in run.iter().zip(RATCHET) {
            assert_eq!(*path, ratchet_path);
            let most = *counts.iter().max().unwrap();
            assert!(
                most <= ceiling,
                "{path:?} at {n} windows: {most} allocations, ratchet {ceiling}: {counts:?}"
            );
            // events alternate between two directions, so a round repeats the round before last
            for (i, w) in counts.windows(3).enumerate() {
                assert_eq!(
                    w[0],
                    w[2],
                    "{path:?} at {n} windows: rounds {i} and {} differ: {counts:?}",
                    i + 2
                );
            }
        }
        for ((path, counts), (_, fewest)) in run.iter().zip(&runs[0]) {
            assert_eq!(
                counts, fewest,
                "{path:?}: the count depends on the number of windows ({} against {n})",
                sizes[0]
            );
        }
    }
}
