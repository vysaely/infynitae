//! OB-42 (INV-ALLOC-1, C14.3) instrumentation: heap allocations of Core's hot paths, counted
//! by a test-only global allocator (UNSAFE_INVENTORY U-002 … U-004).
//!
//! This is **the contract** of OB-42: zero heap allocations per event after warm-up, on every
//! measured hot path, at 10, 100 and 1,000 windows, every round. The events go through the
//! entries the adapter uses (`physical_input`, `toplevel_committed_into` (the adapter: its `_acked`
//! twin), `render_list`,
//! `frame_tick_into`) with caller-owned buffers created once, outside every count (C14.3: an
//! `EffectBuf` and a `Vec` of elements, cleared, not freed). Only the Core call is counted: the
//! trees a commit takes are built before it, and what is freed is not counted.
//!
//! (Until 2026-10-15 this file was a ratchet on non-zero counts; the history is at [`RATCHET`].)
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
use infynitae_core::protocol::{EffectBuf, ToplevelRequest};
use infynitae_core::snapshot::{
    InputRegion, RegionOp, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree,
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

/// A 640x480 window with two 50x50 subsurfaces (the `core_bench` window). Window 1, the one the
/// pointer routes to, that is committed and presented, has an explicit input region (a hole in
/// one corner): its render-list entries carry it, and a region is shared, not copied.
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
            frame_requested: false,
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
        input: if key == 1 {
            InputRegion::Ops(std::sync::Arc::from(vec![
                RegionOp {
                    add: true,
                    rect: LocalRect {
                        loc: LocalPoint::default(),
                        size: LocalSize { w: 640, h: 480 },
                    },
                },
                RegionOp {
                    add: false,
                    rect: LocalRect {
                        loc: LocalPoint { x: 600, y: 0 },
                        size: LocalSize { w: 40, h: 40 },
                    },
                },
            ]))
        } else {
            InputRegion::Infinite
        },
        frame_requested: false,
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

/// `BTN_LEFT` with no modifier: a client-owned button.
fn click(pressed: bool) -> PhysicalEvent {
    PhysicalEvent::PointerButton {
        button: 0x110,
        pressed,
        modifiers: Modifiers::default(),
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
    canvas_at(n, PersistedCameras::default())
}

/// [`canvas`] with the output's camera seeded from `persisted` (a zoom other than 1).
fn canvas_at(n: usize, persisted: PersistedCameras) -> (Core, InputPort, OutputId, Vec<ElementId>) {
    let (mut core, mut port, _shell, mut token) = Core::new(persisted);
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
        core.physical_input_vec(&mut port, pan_button(true))
            .unwrap();
        let dy = if i % 40 == 39 { -520.0 } else { 0.0 };
        core.physical_input_vec(&mut port, motion(-700.0, dy))
            .unwrap();
        core.physical_input_vec(&mut port, pan_button(false))
            .unwrap();
    }
    let camera = core.cameras().camera_for_output(o).unwrap();
    let c = core.cameras().center(camera).unwrap();
    // a pan moves the camera by the motion over the zoom (F-PAN-1)
    let z = core.cameras().zoom(camera).unwrap();
    core.physical_input_vec(&mut port, pan_button(true))
        .unwrap();
    core.physical_input_vec(
        &mut port,
        motion(c.x as f64 / 65536.0 * z, c.y as f64 / 65536.0 * z),
    )
    .unwrap();
    core.physical_input_vec(&mut port, pan_button(false))
        .unwrap();
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
    /// A commit of the focused window while it is presented fullscreen on the output (INC-7;
    /// measured last: the presentation hides every other window).
    PresentedCommit,
    /// A client button pressed and released over the focused window (input intake, INV-ALLOC-1;
    /// audit Q-P1): ownership, click-to-focus on the same element, routing.
    Click,
}

const PATHS: [Path; 10] = [
    Path::Pan,
    Path::PanCrossing,
    Path::Route,
    Path::Key,
    Path::Commit,
    Path::CommitOffView,
    Path::RenderList,
    Path::FrameTick,
    Path::Click,
    Path::PresentedCommit,
];

/// Each path's counts, round by round, on a Canvas of `n` windows.
fn measure(n: usize) -> Vec<(Path, Vec<u64>)> {
    let (mut core, mut port, o, ids) = canvas(n);
    let (first, last) = (ids[0], *ids.last().unwrap());
    // the caller-owned buffers of C14.3, created once outside every count, as the adapter
    // holds them: each event clears and refills them
    let mut fx = EffectBuf::new();
    let mut ticked = Vec::new();
    let mut out = Vec::new();
    for path in PATHS {
        let mut counts = Vec::new();
        if path == Path::PresentedCommit {
            core.toplevel_request(first, ToplevelRequest::SetFullscreen { output: Some(o) })
                .unwrap();
            assert_eq!(core.presentation(o), Some(first));
        }
        if matches!(path, Path::Pan | Path::PanCrossing) {
            core.physical_input_vec(&mut port, pan_button(true))
                .unwrap();
            // the first step of a drag opens every element's epoch; not the steady state
            core.physical_input_vec(&mut port, motion(1.0, 0.0))
                .unwrap();
            core.physical_input_vec(&mut port, motion(-1.0, 0.0))
                .unwrap();
        }
        for round in 0..WARM_UP + ROUNDS {
            let sign = if round % 2 == 0 { 1.0 } else { -1.0 };
            let c = match path {
                Path::Pan => count(|| {
                    fx.clear();
                    core.physical_input(&mut port, motion(3.0 * sign, 0.0), &mut fx)
                        .unwrap();
                }),
                Path::PanCrossing => count(|| {
                    fx.clear();
                    core.physical_input(&mut port, motion(-700.0 * sign, 0.0), &mut fx)
                        .unwrap();
                }),
                Path::Route => count(|| {
                    fx.clear();
                    core.physical_input(&mut port, motion(sign, 0.0), &mut fx)
                        .unwrap();
                }),
                Path::Key => count(|| {
                    fx.clear();
                    core.physical_input(&mut port, key(true), &mut fx).unwrap();
                    fx.clear();
                    core.physical_input(&mut port, key(false), &mut fx).unwrap();
                }),
                Path::Commit => {
                    let t = tree(1, true);
                    count(|| {
                        fx.clear();
                        core.toplevel_committed_into(first, t, &mut fx).unwrap();
                    })
                }
                Path::CommitOffView => {
                    let t = tree(n as u64, true);
                    count(|| {
                        fx.clear();
                        core.toplevel_committed_into(last, t, &mut fx).unwrap();
                    })
                }
                Path::RenderList => count(|| {
                    core.render_list(o).unwrap();
                }),
                Path::Click => count(|| {
                    for pressed in [true, false] {
                        fx.clear();
                        core.physical_input(&mut port, click(pressed), &mut fx)
                            .unwrap();
                    }
                }),
                Path::FrameTick => count(|| {
                    ticked.clear();
                    core.frame_tick_into(o, &mut ticked);
                }),
                Path::PresentedCommit => {
                    let t = tree(1, true);
                    count(|| {
                        fx.clear();
                        core.toplevel_committed_into(first, t, &mut fx).unwrap();
                    })
                }
            };
            if round >= WARM_UP {
                counts.push(c);
            }
        }
        if matches!(path, Path::Pan | Path::PanCrossing) {
            core.physical_input_vec(&mut port, pan_button(false))
                .unwrap();
        }
        out.push((path, counts));
    }
    out
}

/// The most each path may allocate per event: 0 (OB-42). History, kept: the counts were first a
/// ratchet (debug build, the Canvas above), measured on
/// `main` at `ccb82c5` and on this branch with this file: the counts are equal, so the INC-6
/// scale policy adds no allocation to these paths. (`40ddd7d`, before P-1 was repaired, allocated
/// 5,028 per pan step and 5,022 per commit at 1,000 windows.) Lower a count when a change removes
/// allocations; raising one needs a recorded reason. The contract (OB-42) is 0 for every path.
///
/// Raised on the integration with INC-7 (main `23752f1`): Commit 9 → 10 and CommitOffView 4 → 5.
/// Ratified INC-7 snapshots each output's presentation at every root commit
/// (`Core::presentations`, from `f98309b`), one `Vec` per commit whatever the number of windows.
/// This file run on `23752f1` alone gives the same 10 and 5; INC-6 adds nothing.
/// PresentedCommit (added on `integration/inc-6-exit-hardening-2026-10-15`): a commit of a
/// window INC-7 presents fullscreen re-derives that output's presentation view and membership
/// (`presentation_membership`); 24 per commit whatever the number of windows, as on `23752f1`.
/// All set to 0 on `integration/inc-6-final-resolution-2026-10-15` (2026-10-15): C14.3's
/// caller-owned effect buffers, per-element tree layouts, reused membership, frame and scratch
/// storage, and shared input regions.
const RATCHET: [(Path, u64); 10] = [
    (Path::Pan, 0),
    (Path::PanCrossing, 0),
    (Path::Route, 0),
    (Path::Key, 0),
    (Path::Commit, 0),
    (Path::CommitOffView, 0),
    (Path::RenderList, 0),
    (Path::FrameTick, 0),
    (Path::Click, 0),
    (Path::PresentedCommit, 0),
];

/// OB-42: on Canvases of 10, 100 and 1,000 windows, every hot path allocates nothing after
/// warm-up (at most its count in [`RATCHET`], which is 0 for every path), in every round.
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

fn nav_key(pressed: bool, shift: bool) -> PhysicalEvent {
    PhysicalEvent::KeyboardKey {
        key: 28, // KEY_ENTER: Super+Enter fits, and restores at the fit (owner decision 2026-10-21)
        pressed,
        modifiers: Modifiers { shift, ..LOGO },
        mods_changed: false,
        time_us: 0,
    }
}

/// INC-10 (INV-ALLOC-1 at zoom ≠ 1; C12.F16.3 item 8): on a Canvas of 100 windows with the camera at
/// zoom 0.73 (k ∉ n/120, the output's lattice reference kept while panning), a pan step, a pan
/// step across windows, a routed motion and the render list allocate nothing after warm-up; and
/// fits and restores — below the history's capacity, at it (each fit evicting the oldest entry)
/// and unwinding it — allocate nothing once the binding's ring is reserved (its first fit).
#[test]
fn zoomed_hot_paths_and_fit_restore_allocate_nothing() {
    let persisted = PersistedCameras::from_entries(vec![(
        desc("A").key,
        infynitae_core::geom::CanvasPos { x: 0, y: 0 },
        0.73,
    )])
    .unwrap();
    let (mut core, mut port, o, _ids) = canvas_at(100, persisted);
    assert!(
        (core.view(o).unwrap().k * 120.0).fract() != 0.0,
        "k ∉ n/120"
    );
    let mut fx = EffectBuf::new();
    let mut results = Vec::new();
    core.physical_input_vec(&mut port, pan_button(true))
        .unwrap();
    core.physical_input_vec(&mut port, motion(1.0, 0.0))
        .unwrap();
    core.physical_input_vec(&mut port, motion(-1.0, 0.0))
        .unwrap();
    for (name, step) in [("pan", 3.0), ("pan across windows", 700.0)] {
        let mut counts = Vec::new();
        for round in 0..WARM_UP + ROUNDS {
            let sign = if round % 2 == 0 { 1.0 } else { -1.0 };
            let c = count(|| {
                fx.clear();
                core.physical_input(&mut port, motion(step * sign, 0.0), &mut fx)
                    .unwrap();
            });
            if round >= WARM_UP {
                counts.push(c);
            }
        }
        results.push((name, counts));
    }
    core.physical_input_vec(&mut port, pan_button(false))
        .unwrap();
    let mut counts = Vec::new();
    for round in 0..WARM_UP + ROUNDS {
        let sign = if round % 2 == 0 { 1.0 } else { -1.0 };
        let c = count(|| {
            fx.clear();
            core.physical_input(&mut port, motion(sign, 0.0), &mut fx)
                .unwrap();
        });
        let r = count(|| {
            core.render_list(o).unwrap();
        });
        if round >= WARM_UP {
            counts.push(c + r);
        }
    }
    results.push(("route + render list", counts));
    // fit and restore: the first fit reserves the ring (outside the count), then 300 fits take the
    // history to capacity and past it, then 256 restores unwind it
    let camera = core.cameras().camera_for_output(o).unwrap();
    let mut press = |core: &mut Core, port: &mut InputPort, shift: bool| {
        count(|| {
            for pressed in [true, false] {
                fx.clear();
                core.physical_input(port, nav_key(pressed, shift), &mut fx)
                    .unwrap();
            }
        })
    };
    press(&mut core, &mut port, false);
    assert_eq!(
        core.cameras().history(camera).unwrap().len(),
        1,
        "the fit was refused"
    );
    core.render_list(o).unwrap();
    // Super+Enter is a toggle (owner decision 2026-10-21): pressed at the fit it restores. A
    // one-pixel pan after each fit (not counted) makes the next press a fit again, so 300 fits take
    // the history to capacity and past it; then, at capacity, each press at the fit restores and
    // the next one fits again (evicting nothing: the restore made room)
    // its own buffer (not counted): `press` holds the counted one
    let mut nudge_fx = infynitae_core::protocol::EffectBuf::new();
    let mut nudge = |core: &mut Core, port: &mut InputPort| {
        for ev in [pan_button(true), motion(1.0, 0.0), pan_button(false)] {
            nudge_fx.clear();
            core.physical_input(port, ev, &mut nudge_fx).unwrap();
        }
        core.render_list(o).unwrap();
    };
    let mut fits = Vec::new();
    for _ in 0..300 {
        nudge(&mut core, &mut port);
        fits.push(press(&mut core, &mut port, false));
        core.render_list(o).unwrap();
    }
    assert_eq!(core.cameras().history(camera).unwrap().len(), 256);
    let mut restores = Vec::new();
    for _ in 0..256 {
        restores.push(press(&mut core, &mut port, false));
        core.render_list(o).unwrap();
        assert_eq!(
            core.cameras().history(camera).unwrap().len(),
            255,
            "a restore at the fit"
        );
        fits.push(press(&mut core, &mut port, false));
        core.render_list(o).unwrap();
        assert_eq!(
            core.cameras().history(camera).unwrap().len(),
            256,
            "a fit again"
        );
    }
    results.push(("fit", fits));
    results.push(("restore", restores));
    for (name, counts) in &results {
        println!(
            "{name}: max {} over {} events",
            counts.iter().max().unwrap(),
            counts.len()
        );
        assert!(counts.iter().all(|&c| c == 0), "{name}: {counts:?}");
    }
}
