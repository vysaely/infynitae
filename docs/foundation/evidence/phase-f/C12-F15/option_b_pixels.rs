//! C12.F15 evidence (not compiled, not part of the test suite): option B prototype at pixel level (Pixman).
mod common;
use std::os::fd::AsFd;
use common::{Lockstep, output};
use infynitae_core::geom::{CanvasPoint, CanvasPos, OutputLogicalPoint, Q16_ONE};
use infynitae_core::projection::{DELTA, View, ViewParams};
use infynitae_core::render::{FrameEntry, ProjectionFrame, entries_for};
use infynitae_wayland::convert;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::{Element, RenderElement};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, Color32F, ExportMem, Offscreen, Renderer, TextureFilter};
use smithay::reexports::pixman::Image;
use smithay::utils::{Physical, Rectangle, Scale};
use wayland_client::protocol::wl_shm;

const RED: u32 = 0xFFFF_0000;
const GREEN: u32 = 0xFF00_FF00;
const BLUE: u32 = 0xFF00_00FF;
const WHITE: u32 = 0xFFFF_FFFF;

/// Attaches a buffer filled with one opaque colour to a probe surface and commits it.
fn paint(ls: &mut Lockstep, idx: usize, argb: u32) {
    let (w, h) = ls.probe.surfaces[idx].size;
    let len = (w * h * 4) as usize;
    let fd = smithay::reexports::rustix::fs::memfd_create(
        "infynitae-t1-pixels",
        smithay::reexports::rustix::fs::MemfdFlags::CLOEXEC,
    )
    .expect("memfd");
    let file = std::fs::File::from(fd);
    let bytes: Vec<u8> = argb.to_le_bytes().repeat(len / 4);
    std::os::unix::fs::FileExt::write_all_at(&file, &bytes, 0).expect("fill");
    let shm = ls.probe.shm.clone().expect("wl_shm");
    let pool = shm.create_pool(file.as_fd(), len as i32, &ls.qh, ());
    let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &ls.qh, ());
    let wl = &ls.probe.surfaces[idx].wl;
    wl.attach(Some(&buffer), 0, 0);
    wl.damage_buffer(0, 0, w, h);
    wl.commit();
    ls.pump();
}

fn toplevel(ls: &mut Lockstep, label: &'static str, size: (i32, i32), argb: u32) -> usize {
    let qh = ls.qh.clone();
    let idx = ls.probe.toplevel(&qh, label, size, 0);
    ls.pump();
    paint(ls, idx, argb);
    idx
}

fn subsurface(
    ls: &mut Lockstep,
    label: &'static str,
    parent: usize,
    offset: (i32, i32),
    size: (i32, i32),
    below: bool,
    argb: u32,
) -> usize {
    let qh = ls.qh.clone();
    let idx = ls.probe.subsurface(&qh, label, parent, offset, size, below);
    ls.pump();
    paint(ls, idx, argb);
    ls.probe.surfaces[parent].wl.commit();
    ls.pump();
    idx
}



fn draw<E: RenderElement<PixmanRenderer>>(
    r: &mut PixmanRenderer, img: &mut Image<'static, 'static>, t: &mut OutputDamageTracker, age: usize, els: &[E],
) -> (Option<Vec<Rectangle<i32, Physical>>>, Vec<u32>) {
    let mut target = r.bind(img).unwrap();
    let res = t.render_output(r, &mut target, age, els, Color32F::TRANSPARENT).unwrap();
    let damage = res.damage.cloned();
    let m = r.copy_framebuffer(&target, Rectangle::from_size((1920, 1080).into()), Fourcc::Argb8888).unwrap();
    let px = r.map_texture(&m).unwrap().as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
    (damage, px)
}

#[test]
fn option_b_pixels() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let w = toplevel(&mut ls, "W", (400, 300), WHITE);
    let a = subsurface(&mut ls, "A", w, (100, 100), (101, 53), false, RED);
    let _b = subsurface(&mut ls, "B", w, (201, 100), (77, 53), false, GREEN);
    let id = ls.state.elements().next().unwrap();
    let tree = ls.state.core().stage().tree(id).unwrap().clone();
    let anchor = CanvasPoint { x: 1000, y: 500 };
    let model = ls.state.core().output(ls.output()).unwrap().clone();
    let mut renderer = PixmanRenderer::new().unwrap();
    renderer.upscale_filter(TextureFilter::Nearest).unwrap();
    renderer.downscale_filter(TextureFilter::Nearest).unwrap();
    let mut img = renderer.create_buffer(Fourcc::Argb8888, (1920, 1080).into()).unwrap();

    // a production View at a fit-like zoom where Smithay's own right edge of A differs from Core's
    let centre = CanvasPos { x: (anchor.x + 150) * Q16_ONE + 12345, y: (anchor.y + 126) * Q16_ONE };
    let mk = |zoom: f64| View::new(ViewParams { centre, zoom, scale: 1.0, view_size: (1920, 1080) }).unwrap();
    let mut chosen = None;
    'search: for j in 0..400 {
        let z0 = 0.7 + 0.0123 * j as f64;
        let v0 = mk(z0);
        let d1 = (anchor.x + 100 + 101) - v0.r.0;
        let h = (d1 as f64 * v0.k).floor() + 0.5;
        let zt = (h - DELTA) / d1 as f64;
        for i in -80i64..=80 {
            let view = mk(f64::from_bits((zt.to_bits() as i64 + i) as u64));
            let entries = entries_for(&view, id, anchor, &tree);
            let els = convert::render_elements_k(&mut renderer, ls.state.registry(), &entries, view.k).unwrap();
            for (e, el) in &els {
                if e.size.w == 101 && el.inner().geometry(Scale::from(view.k)).loc.x + el.inner().geometry(Scale::from(view.k)).size.w != e.rect.x1 as i32 {
                    chosen = Some(view);
                    break 'search;
                }
            }
        }
    }
    let view = chosen.expect("a mismatching view");
    let entries: Vec<FrameEntry> = entries_for(&view, id, anchor, &tree);
    let frame = ProjectionFrame { view, entries: entries.clone() };
    let ea = entries.iter().find(|e| e.size.w == 101).unwrap().clone();
    let row = (ea.rect.y0 + ea.rect.y1) / 2;
    let span = (ea.rect.x0 - 3)..(ea.rect.x1 + 80);
    let check = |px: &[u32], label: &str| -> (usize, usize) {
        let (mut wrong_px, mut wrong_hit) = (0, 0);
        for col in span.clone() {
            let c = px[(row * 1920 + col) as usize];
            let core = entries.iter().find(|e| e.rect.contains(col, row)).map(|e| e.size.w);
            let want = match core { Some(101) => RED, Some(77) => GREEN, Some(_) => WHITE, None => 0 };
            if c != want { wrong_px += 1; println!("  {label}: col {col}: drawn {c:#010x}, Core rect says {want:#010x}"); }
            let p = OutputLogicalPoint { x: col as f64 + 0.5, y: row as f64 + 0.5 };
            let hit = frame.select(p).map(|h| h.surface);
            let drawn_owner = entries.iter().find(|e| match c { RED => e.size.w == 101, GREEN => e.size.w == 77, WHITE => e.size.w == 400, _ => false }).map(|e| e.surface);
            if hit != drawn_owner { wrong_hit += 1; }
        }
        (wrong_px, wrong_hit)
    };
    println!("PIXELS k = {} A: Core x [{}, {})", view.k, ea.rect.x0, ea.rect.x1);

    // negative control: the current handoff (unwrapped elements)
    let els = convert::render_elements_k(&mut renderer, ls.state.registry(), &entries, view.k).unwrap();
    let inner: Vec<_> = els.iter().map(|(_, e)| e.inner()).collect();
    let ia = els.iter().position(|(e, _)| e.size.w == 101).unwrap();
    let g = inner[ia].geometry(Scale::from(view.k));
    println!("PIXELS current handoff: Smithay A geometry x [{}, {})", g.loc.x, g.loc.x + g.size.w);
    let mut t0 = convert::damage_tracker(&model, &view);
    let (_, px) = draw(&mut renderer, &mut img, &mut t0, 0, &inner);
    let (p, h) = check(&px, "current");
    println!("PIXELS current handoff: wrong pixels {p}, pointer != drawn {h} over {} columns", span.end - span.start);

    // option B: wrapped elements, three frames through one tracker (full, unchanged, A repainted)
    let wrapped: Vec<_> = els.into_iter().map(|(_, e)| e).collect();
    let mut t = convert::damage_tracker(&model, &view);
    let (_, px) = draw(&mut renderer, &mut img, &mut t, 0, &wrapped);
    let (p, h) = check(&px, "B frame 1");
    println!("PIXELS option B frame 1: wrong pixels {p}, pointer != drawn {h}");
    let (dmg, _) = draw(&mut renderer, &mut img, &mut t, 1, &wrapped);
    println!("PIXELS option B frame 2 (unchanged): damage {dmg:?}");
    paint(&mut ls, a, BLUE);
    let els = convert::render_elements_k(&mut renderer, ls.state.registry(), &entries, view.k).unwrap();
    let wrapped: Vec<_> = els.into_iter().map(|(_, e)| e).collect();
    let (dmg, px) = draw(&mut renderer, &mut img, &mut t, 1, &wrapped);
    let blue = (ea.rect.x0..ea.rect.x1).filter(|&c| px[(row * 1920 + c) as usize] == BLUE).count();
    println!("PIXELS option B frame 3 (A repainted, age 1): damage {dmg:?}; A columns blue {blue}/{}", ea.rect.x1 - ea.rect.x0);

    // i32 limit: A's entry with a far edge beyond i32 (a synthetic rect: only the crop is tested)
    for (x0, x1) in [(ea.rect.x0, 1i64 << 40), (-(1i64 << 40), ea.rect.x1)] {
        let mut e = ea.clone();
        e.rect.x0 = x0;
        e.rect.x1 = x1;
        let els = convert::render_elements_k(&mut renderer, ls.state.registry(), &[e], view.k).unwrap();
        let el = &els[0].1;
        let g = el.geometry(Scale::from(view.k));
        let (s, si) = (el.src(), el.inner().src());
        let want_w = si.size.w * (g.size.w as f64) / ((x1 - x0) as f64);
        let want_x = si.loc.x + si.size.w * ((g.loc.x as i64 - x0) as f64) / ((x1 - x0) as f64);
        let shown = 0;
        println!("PIXELS crop x [{x0}, {x1}): geometry x {} w {}; src x {:.9} w {:.9e} (proportional: x {:.9} w {:.9e}); blue columns in view {shown}", g.loc.x, g.size.w, s.loc.x, s.size.w, want_x, want_w);
    }

    // current handoff, far-left origin: Smithay's own geometry (no wrapper)
    let mut e = ea.clone();
    e.location.x = -((1u64 << 40) as f64) + 0.25;
    let els = convert::render_elements_k(&mut renderer, ls.state.registry(), &[e], view.k).unwrap();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| els[0].1.inner().geometry(Scale::from(((1u64 << 40) as f64 + 997.0) / 101.0))));
    println!("PIXELS current handoff far-left (w·k ≈ 2^40 + 997): {:?}", r.map_err(|e| e.downcast_ref::<String>().cloned().or(e.downcast_ref::<&str>().map(|s| s.to_string()))));
}
