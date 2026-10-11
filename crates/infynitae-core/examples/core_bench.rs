//! Core event-path cost against the number of elements on the Canvas, through the public API.
//! Model only: no Smithay, no rendering. Run with
//! `cargo run --release -p infynitae-core --example core_bench [N ...]`.
//!
//! N windows (root + two subsurfaces) are laid out on a 700 px grid, 40 per row; a 2560x1440
//! output looks at the first ones, so only a handful are in view whatever N is. A second output
//! (`two_outputs`) looks at the same place.

use std::num::NonZeroU64;
use std::time::Instant;

use infynitae_core::geom::{LocalPoint, LocalRect, LocalSize};
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, Transform};
use infynitae_core::snapshot::{
    InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree,
};
use infynitae_core::stage::ElementId;
use infynitae_core::{Core, InputPort, PhysicalEvent};

const LOGO: Modifiers = Modifiers {
    logo: true,
    ctrl: false,
    alt: false,
    shift: false,
};

fn desc(name: &str, mode: (u32, u32), scale_120: u32, refresh_mhz: u32) -> OutputDescriptor {
    OutputDescriptor {
        key: OutputKey {
            device: "bench".into(),
            connector: name.into(),
            make: "x".into(),
            model: "y".into(),
            serial: None,
        },
        mode,
        transform: Transform::Normal,
        scale_120,
        refresh_mhz,
    }
}

fn tree(key: u64, mapped: bool, subs: u64) -> SurfaceTree {
    let k = |n: u64| SurfaceKey::from_raw(NonZeroU64::new(n).unwrap());
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

fn pan(core: &mut Core, port: &mut InputPort, dx: f64, dy: f64) -> usize {
    let mut n = 0;
    let mut ev = |core: &mut Core, e| n += core.physical_input(port, e).unwrap().len();
    ev(
        core,
        PhysicalEvent::PointerButton {
            button: 0x112,
            pressed: true,
            modifiers: LOGO,
            time_us: 0,
        },
    );
    ev(
        core,
        PhysicalEvent::PointerMotion {
            delta: (dx, dy),
            delta_unaccel: (dx, dy),
            time_us: 0,
        },
    );
    ev(
        core,
        PhysicalEvent::PointerButton {
            button: 0x112,
            pressed: false,
            modifiers: LOGO,
            time_us: 0,
        },
    );
    n
}

fn map(core: &mut Core, key: u64) -> ElementId {
    let id = core.toplevel_created();
    core.toplevel_committed(id, tree(key, false, 0)).unwrap();
    core.toplevel_committed(id, tree(key, true, 2)).unwrap();
    id
}

fn centre(core: &Core, o: OutputId) -> (f64, f64) {
    let c = core
        .cameras()
        .center(core.cameras().camera_for_output(o).unwrap())
        .unwrap();
    (c.x as f64 / 65536.0, c.y as f64 / 65536.0)
}

fn timed(iters: usize, mut f: impl FnMut(usize)) -> f64 {
    let t = Instant::now();
    for i in 0..iters {
        f(i);
    }
    t.elapsed().as_nanos() as f64 / iters as f64 / 1e3
}

fn motion(d: f64) -> PhysicalEvent {
    PhysicalEvent::PointerMotion {
        delta: (d, 0.0),
        delta_unaccel: (d, 0.0),
        time_us: 0,
    }
}

fn button(pressed: bool) -> PhysicalEvent {
    PhysicalEvent::PointerButton {
        button: 0x112,
        pressed,
        modifiers: LOGO,
        time_us: 0,
    }
}

fn scaling(n: usize, two_outputs: bool) {
    let (mut core, mut port, _s, mut tok) = Core::new(PersistedCameras::default());
    let (o, _) = core
        .output_connected(&mut tok, desc("A", (2560, 1440), 120, 165_000))
        .unwrap();
    let mut ids = Vec::new();
    for i in 0..n {
        ids.push(map(&mut core, i as u64 + 1));
        pan(
            &mut core,
            &mut port,
            -700.0,
            if i % 40 == 39 { -520.0 } else { 0.0 },
        );
    }
    let (cx, cy) = centre(&core, o);
    pan(&mut core, &mut port, cx, cy);
    if two_outputs {
        core.output_connected(&mut tok, desc("B", (3840, 2160), 240, 60_000))
            .unwrap();
    }
    let visible = core.render_list(o).unwrap().entries.len();
    let iters = if n >= 1000 { 300 } else { 3000 };

    // camera translation that changes no membership (3 px back and forth)
    core.physical_input(&mut port, button(true)).unwrap();
    let pan_small = timed(iters, |i| {
        let d = if i % 2 == 0 { 3.0 } else { -3.0 };
        std::hint::black_box(core.physical_input(&mut port, motion(d)).unwrap());
    });
    // camera translation that changes membership (one grid column back and forth)
    let pan_cross = timed(iters, |i| {
        let d = if i % 2 == 0 { -700.0 } else { 700.0 };
        std::hint::black_box(core.physical_input(&mut port, motion(d)).unwrap());
    });
    core.physical_input(&mut port, button(false)).unwrap();
    let render = timed(iters, |_| {
        std::hint::black_box(core.render_list(o).unwrap().entries.len());
    });
    // commit of one already-mapped, visible window, and of one far outside every view
    let commit_visible = timed(iters, |_| {
        std::hint::black_box(core.toplevel_committed(ids[0], tree(1, true, 2)).unwrap());
    });
    let far = ids[n - 1];
    let commit_far = timed(iters, |_| {
        std::hint::black_box(
            core.toplevel_committed(far, tree(n as u64, true, 2))
                .unwrap(),
        );
    });
    // unmap, initial commit and map of one visible window
    let remap = timed(iters, |_| {
        std::hint::black_box(core.toplevel_committed(ids[0], tree(1, false, 0)).unwrap());
        std::hint::black_box(core.toplevel_committed(ids[0], tree(1, false, 0)).unwrap());
        std::hint::black_box(core.toplevel_committed(ids[0], tree(1, true, 2)).unwrap());
    });
    // create, map, destroy
    let churn = timed(iters, |i| {
        let id = map(&mut core, 1_000_000 + i as u64);
        std::hint::black_box(core.toplevel_destroyed(id));
    });
    let idle_motion = timed(iters, |i| {
        let d = if i % 2 == 0 { 1.0 } else { -1.0 };
        std::hint::black_box(core.physical_input(&mut port, motion(d)).unwrap());
    });
    let tick = timed(iters, |_| {
        std::hint::black_box(core.frame_tick(o));
    });
    // output hotplug with N elements on the Canvas
    let hotplug = timed(iters.min(100), |_| {
        let (c, _) = core
            .output_connected(&mut tok, desc("C", (1920, 1080), 120, 60_000))
            .unwrap();
        std::hint::black_box(core.output_disconnected(&mut tok, c).unwrap());
    });
    println!(
        "{n:>6} {:>2} {visible:>4} | {pan_small:>9.2} {pan_cross:>9.2} {render:>9.2} | {commit_visible:>9.2} {commit_far:>9.2} {remap:>9.2} {churn:>9.2} | {idle_motion:>9.2} {tick:>9.2} {hotplug:>9.2}",
        if two_outputs { 2 } else { 1 },
    );
}

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let sizes = if args.is_empty() {
        vec![1, 10, 100, 1000, 3000, 10000]
    } else {
        args
    };
    println!("microseconds per operation (release build expected)");
    println!(
        "{:>6} {:>2} {:>4} | {:>9} {:>9} {:>9} | {:>9} {:>9} {:>9} {:>9} | {:>9} {:>9} {:>9}",
        "N",
        "O",
        "vis",
        "pan",
        "pan-cross",
        "render",
        "commit",
        "commit-far",
        "remap",
        "churn",
        "motion",
        "tick",
        "hotplug"
    );
    for two in [false, true] {
        for &n in &sizes {
            scaling(n, two);
        }
    }
}
