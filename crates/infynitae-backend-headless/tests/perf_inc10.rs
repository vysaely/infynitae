//! INC-10 exit item 8: frame time of the production render path at zoom 1 and at zooms ≠ 1.
//! Measurement, not a gate: `#[ignore]`, run explicitly in release:
//!
//! `cargo test --release -p infynitae-backend-headless --test perf_inc10 -- --ignored --nocapture`
//!
//! The path: `Core::render_list` (the output's ProjectionFrame) → `convert::render_elements` (Smithay
//! surface elements in the `CanonicalRect` wrapper) → `OutputDamageTracker::render_output` with the
//! software renderer Pixman (nearest filtering), at a 1920 × 1080 output, 20 client windows of
//! 640 × 480 with two subsurfaces each (real `wl_shm` buffers), and the same with Smithay's test
//! renderer (no drawing) to separate Core and adapter cost from rasterisation. Each frame either
//! pans the camera by one physical event first (a full repaint) or changes nothing (no damage).
//!
//! What it measures: CPU time of these calls on this machine, per frame, wall clock
//! (`Instant`), after warm-up. What it does not: GLES or any GPU, presentation timing or vblank
//! pacing (the headless backend has no display), scanout, or another machine.

mod common;

use std::os::fd::AsFd;
use std::time::Instant;

use common::input::*;
use common::{Lockstep, output};
use infynitae_backend_headless::{VirtualOutput, pointer_motion};
use infynitae_core::geom::CanvasPos;
use infynitae_core::navigation::PersistedCameras;
use infynitae_wayland::convert;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::test::{DummyFramebuffer, DummyRenderer};
use smithay::backend::renderer::{Bind, Color32F, Offscreen, Renderer, TextureFilter};
use smithay::utils::{Scale, Transform};
use wayland_client::protocol::wl_shm;

fn paint(ls: &mut Lockstep, idx: usize, argb: u32) {
    let (w, h) = ls.probe.surfaces[idx].size;
    let len = (w * h * 4) as usize;
    let fd = smithay::reexports::rustix::fs::memfd_create(
        "infynitae-perf",
        smithay::reexports::rustix::fs::MemfdFlags::CLOEXEC,
    )
    .unwrap();
    let file = std::fs::File::from(fd);
    std::os::unix::fs::FileExt::write_all_at(&file, &argb.to_le_bytes().repeat(len / 4), 0)
        .unwrap();
    let shm = ls.probe.shm.clone().unwrap();
    let pool = shm.create_pool(file.as_fd(), len as i32, &ls.qh, ());
    let b = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &ls.qh, ());
    let wl = &ls.probe.surfaces[idx].wl;
    wl.attach(Some(&b), 0, 0);
    wl.damage_buffer(0, 0, w, h);
    wl.commit();
    ls.pump();
}

/// The scene at camera `zoom`: 20 windows on a 5 × 4 grid around the view centre.
fn scene(zoom: f64) -> Rig {
    let persisted = PersistedCameras::from_entries(vec![(
        VirtualOutput::key("A"),
        CanvasPos { x: 0, y: 0 },
        zoom,
    )])
    .unwrap();
    let mut r = Rig::new(Lockstep::with(persisted, &[output((1920, 1080), 120)]));
    let qh = r.ls.qh.clone();
    for i in 0..20u32 {
        let w = r.ls.probe.toplevel(&qh, "W", (640, 480), 0);
        r.ls.pump();
        paint(&mut r.ls, w, 0xFF00_0000 | (i * 0x0A_0B0C));
        for (j, off) in [(37, 41), (301, 222)].into_iter().enumerate() {
            let s =
                r.ls.probe
                    .subsurface(&qh, "S", w, off, (150 + 20 * j as i32, 90), false);
            r.ls.pump();
            paint(&mut r.ls, s, 0xFFFF_0000 >> (8 * j));
        }
        r.ls.probe.surfaces[w].wl.commit();
        r.ls.pump();
        r.frame();
        // the next window is placed at the camera centre: move the camera on by one cell
        let (dx, dy) = if i % 5 == 4 {
            (4.0 * 700.0, -520.0)
        } else {
            (-700.0, 0.0)
        };
        r.button(BTN_MIDDLE, true, logo());
        r.motion((dx * zoom, dy * zoom));
        r.button(BTN_MIDDLE, false, logo());
    }
    // centre the camera on the grid: the mean of the windows' anchors plus half a window
    let core = r.ls.state.core();
    let ids: Vec<_> = r.ls.state.elements().collect();
    let (mut sx, mut sy) = (0i64, 0i64);
    for &id in &ids {
        let a = core.stage().anchor(id).unwrap();
        sx += a.x + 320;
        sy += a.y + 240;
    }
    let (gx, gy) = (sx / ids.len() as i64, sy / ids.len() as i64);
    let c = core.cameras().center(0).unwrap();
    let (cx, cy) = (c.x as f64 / 65536.0, c.y as f64 / 65536.0);
    // C = start − Σ/z: a motion of (c − g)·z moves the camera to g
    r.button(BTN_MIDDLE, true, logo());
    r.motion(((cx - gx as f64) * zoom, (cy - gy as f64) * zoom));
    r.button(BTN_MIDDLE, false, logo());
    r
}

#[derive(Default)]
struct Times {
    shown: usize,
    core: Vec<f64>,
    elements: Vec<f64>,
    draw: Vec<f64>,
    total: Vec<f64>,
}

fn pct(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[((s.len() - 1) as f64 * p).round() as usize]
}

fn report(label: &str, t: &Times) {
    let line = |name: &str, v: &[f64]| {
        format!(
            "{name} p50 {:.3} p99 {:.3} max {:.3}",
            pct(v, 0.5),
            pct(v, 0.99),
            v.iter().cloned().fold(0.0, f64::max)
        )
    };
    println!(
        "{label:<46} n={} surfaces={} | {} | {} | {} | {}  (ms)",
        t.total.len(),
        t.shown,
        line("total", &t.total),
        line("core", &t.core),
        line("elements", &t.elements),
        line("tracker+draw", &t.draw)
    );
}

const WARM: usize = 60;
const FRAMES: usize = 600;

/// One configuration: `pan` moves the camera by one event before every frame.
fn run(zoom: f64, pan: bool, pixman: bool) -> Times {
    let mut r = scene(zoom);
    let o = r.ls.output();
    let view = r.ls.state.core().view(o).unwrap();
    let shown = r.ls.state.core_mut().render_list(o).unwrap().entries.len();
    assert!(shown >= 1, "zoom {zoom}: nothing in view");
    let mut t = Times {
        shown,
        ..Times::default()
    };
    let mut pix = PixmanRenderer::new().unwrap();
    pix.upscale_filter(TextureFilter::Nearest).unwrap();
    pix.downscale_filter(TextureFilter::Nearest).unwrap();
    let mut image = pix
        .create_buffer(Fourcc::Argb8888, (1920, 1080).into())
        .unwrap();
    let mut dummy = DummyRenderer;
    let mut tracker =
        OutputDamageTracker::new((1920, 1080), Scale::from(view.k), Transform::Normal);
    if pan {
        r.button(BTN_MIDDLE, true, logo());
    }
    for i in 0..WARM + FRAMES {
        if pan {
            let s = if (i / 50) % 2 == 0 { 1.0 } else { -1.0 };
            r.t += 1000;
            pointer_motion(
                &mut r.ls.state,
                &mut r.ls.input,
                (3.7 * s, 1.3 * s),
                (0.0, 0.0),
                r.t,
            )
            .unwrap();
        }
        let t0 = Instant::now();
        let frame = r.ls.state.core_mut().render_list(o).unwrap();
        let (k, entries) = (frame.view.k, frame.entries.clone());
        let t1 = Instant::now();
        let t2;
        if pixman {
            let els: Vec<_> =
                convert::render_elements(&mut pix, r.ls.state.registry(), &entries, k)
                    .unwrap()
                    .into_iter()
                    .map(|(_, e)| e)
                    .collect();
            t2 = Instant::now();
            let mut target = pix.bind(&mut image).unwrap();
            tracker
                .render_output(&mut pix, &mut target, 1, &els, Color32F::TRANSPARENT)
                .unwrap();
        } else {
            let els: Vec<_> =
                convert::render_elements(&mut dummy, r.ls.state.registry(), &entries, k)
                    .unwrap()
                    .into_iter()
                    .map(|(_, e)| e)
                    .collect();
            t2 = Instant::now();
            tracker
                .render_output(
                    &mut dummy,
                    &mut DummyFramebuffer,
                    1,
                    &els,
                    Color32F::TRANSPARENT,
                )
                .unwrap();
        }
        let t3 = Instant::now();
        if i >= WARM {
            let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1e3;
            t.core.push(ms(t0, t1));
            t.elements.push(ms(t1, t2));
            t.draw.push(ms(t2, t3));
            t.total.push(ms(t0, t3));
        }
    }
    if pan {
        r.button(BTN_MIDDLE, false, logo());
    }
    t
}

#[test]
#[ignore = "measurement; run in release with --ignored --nocapture"]
fn frame_times_at_zoom_1_and_other_zooms() {
    println!(
        "INC-10 frame times, {} frames after {} warm-up, 1920x1080, 20 windows x 3 surfaces",
        FRAMES, WARM
    );
    for pixman in [false, true] {
        for zoom in [1.0, 0.73, 1.797_004_991_680_532_4, 2.391, 0.25, 8.0] {
            for pan in [false, true] {
                let t = run(zoom, pan, pixman);
                report(
                    &format!(
                        "{} zoom {zoom} {}",
                        if pixman { "pixman" } else { "dummy " },
                        if pan { "panning" } else { "static " }
                    ),
                    &t,
                );
            }
        }
    }
}
