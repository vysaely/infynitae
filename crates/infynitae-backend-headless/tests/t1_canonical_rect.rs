//! INC-10 T1: one canonical rectangle per surface (INV-RENDER-1, F-RENDER-1 as amended by C12.F15,
//! option B) on the production path — Core's views and render lists (`View`, `entries_for`,
//! `Core::render_list`, `ProjectionFrame::select`), the adapter's `convert::render_elements` /
//! `convert::render_output` with its `CanonicalRect` wrapper, real `wl_shm` buffers committed by a
//! real client, Smithay 0.7.0's `OutputDamageTracker` and its software renderer (Pixman).
//!
//! Established here: at k ∉ n/120, swept by single ulps around rounding points, the rect drawn,
//! the rect hit and the rect Core computes are one and the same (geometry, pixels and pointer
//! columns); Smithay's own geometry for the same element differs there (the defect option B
//! removes); damage and opaque regions of a remapped surface are conservative; a rect beyond the
//! i32 range is cropped to `[−2³⁰, 2³⁰]` with `src` cropped in proportion for every buffer transform.
//! Not established: GLES or any GPU path, dmabuf import, direct scanout, hardware planes. Pixman
//! cannot draw magnifications beyond about 2¹⁵ (16.16 transform), so the crop at extreme
//! magnification is checked on the values the damage tracker hands to `draw` (`dst`, `src`).

mod common;

use std::cell::RefCell;
use std::os::fd::AsFd;
use std::rc::Rc;

use common::{Lockstep, output};
use infynitae_core::geom::{CanvasPoint, CanvasPos, OutputLogicalPoint, PhysicalRect, Q16_ONE};
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::projection::{DELTA, View, ViewParams};
use infynitae_core::render::{FrameEntry, ProjectionFrame, entries_for};
use infynitae_core::snapshot::SurfaceTree;
use infynitae_core::stage::ElementId;
use infynitae_wayland::convert::{self, CanonicalRect, RENDER_LIMIT};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::test::DummyRenderer;
use smithay::backend::renderer::utils::CommitCounter;
use smithay::backend::renderer::{Bind, Color32F, ExportMem, Offscreen, Renderer, TextureFilter};
use smithay::reexports::pixman::Image;
use smithay::utils::{Buffer, Physical, Rectangle, Scale, Transform};
use wayland_client::protocol::{wl_output, wl_shm};

type R4 = (i64, i64, i64, i64);

fn ulps(x: f64, i: i64) -> f64 {
    f64::from_bits((x.to_bits() as i64 + i) as u64)
}

fn r4(g: Rectangle<i32, Physical>) -> R4 {
    (
        g.loc.x as i64,
        g.loc.y as i64,
        (g.loc.x + g.size.w) as i64,
        (g.loc.y + g.size.h) as i64,
    )
}

fn core4(e: &FrameEntry) -> R4 {
    (e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1)
}

fn contains(r: R4, x: i64, y: i64) -> bool {
    x >= r.0 && x < r.2 && y >= r.1 && y < r.3
}

/// Deterministic xorshift for zooms and centres (seeded; the sweep is reproducible).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}

// ------------------------------------------------------------------ client buffers

/// Attaches an ARGB buffer of `buffer` size, filled by `px(x, y)`, with `transform`, and commits.
fn attach(
    ls: &mut Lockstep,
    idx: usize,
    buffer: (i32, i32),
    transform: wl_output::Transform,
    px: impl Fn(i32, i32) -> u32,
) {
    attach_damaged(ls, idx, buffer, transform, px, (0, 0, buffer.0, buffer.1));
}

/// [`attach`] with only `damage` (buffer coordinates x, y, w, h) damaged.
fn attach_damaged(
    ls: &mut Lockstep,
    idx: usize,
    buffer: (i32, i32),
    transform: wl_output::Transform,
    px: impl Fn(i32, i32) -> u32,
    damage: (i32, i32, i32, i32),
) {
    let (w, h) = buffer;
    let len = (w * h * 4) as usize;
    let fd = smithay::reexports::rustix::fs::memfd_create(
        "infynitae-t1-canonical-rect",
        smithay::reexports::rustix::fs::MemfdFlags::CLOEXEC,
    )
    .expect("memfd");
    let file = std::fs::File::from(fd);
    let mut bytes = Vec::with_capacity(len);
    for y in 0..h {
        for x in 0..w {
            bytes.extend_from_slice(&px(x, y).to_le_bytes());
        }
    }
    std::os::unix::fs::FileExt::write_all_at(&file, &bytes, 0).expect("fill");
    let shm = ls.probe.shm.clone().expect("wl_shm");
    let pool = shm.create_pool(file.as_fd(), len as i32, &ls.qh, ());
    let b = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &ls.qh, ());
    let wl = &ls.probe.surfaces[idx].wl;
    wl.set_buffer_transform(transform);
    wl.attach(Some(&b), 0, 0);
    wl.damage_buffer(damage.0, damage.1, damage.2, damage.3);
    wl.commit();
    ls.pump();
}

fn solid(ls: &mut Lockstep, idx: usize, argb: u32) {
    let size = ls.probe.surfaces[idx].size;
    attach(ls, idx, size, wl_output::Transform::Normal, |_, _| argb);
}

/// Marks the whole surface opaque (wl_surface.set_opaque_region) on its next commit.
fn opaque(ls: &mut Lockstep, idx: usize) {
    let (w, h) = ls.probe.surfaces[idx].size;
    let qh = ls.qh.clone();
    let region = ls
        .probe
        .compositor
        .as_ref()
        .expect("wl_compositor")
        .create_region(&qh, ());
    region.add(0, 0, w, h);
    ls.probe.surfaces[idx].wl.set_opaque_region(Some(&region));
}

/// W (root, 803 × 601, no shadow) with subsurfaces above it: A (101 × 53 at (100, 100)), B right
/// of A sharing A's right edge (77 × 53 at (201, 100)), C below A sharing A's bottom edge
/// (101 × 37 at (100, 153)). Every surface a distinct opaque colour.
struct Scene {
    ls: Lockstep,
    id: ElementId,
    tree: SurfaceTree,
    idx: [usize; 4],
}

const W_RGB: u32 = 0xFF20_2020;
const A_RGB: u32 = 0xFFFF_0000;
const B_RGB: u32 = 0xFF00_FF00;
const C_RGB: u32 = 0xFF00_00FF;

impl Scene {
    fn new(ls: Lockstep) -> Scene {
        let mut ls = ls;
        let qh = ls.qh.clone();
        let w = ls.probe.toplevel(&qh, "W", (803, 601), 0);
        ls.pump();
        solid(&mut ls, w, W_RGB);
        let sub = |ls: &mut Lockstep, label, off, size, argb| {
            let qh = ls.qh.clone();
            let i = ls.probe.subsurface(&qh, label, w, off, size, false);
            ls.pump();
            solid(ls, i, argb);
            ls.probe.surfaces[w].wl.commit();
            ls.pump();
            i
        };
        let a = sub(&mut ls, "A", (100, 100), (101, 53), A_RGB);
        let b = sub(&mut ls, "B", (201, 100), (77, 53), B_RGB);
        let c = sub(&mut ls, "C", (100, 153), (101, 37), C_RGB);
        let id = ls.state.elements().next().expect("element");
        let tree = ls.state.core().stage().tree(id).expect("tree").clone();
        assert_eq!(tree.surfaces.len(), 4);
        Scene {
            ls,
            id,
            tree,
            idx: [w, a, b, c],
        }
    }

    fn key_of(&self, i: usize) -> infynitae_core::snapshot::SurfaceKey {
        let size = self.ls.probe.surfaces[self.idx[i]].size;
        // surfaces are distinguished by size in this scene
        self.tree
            .surfaces
            .iter()
            .find(|s| (s.size.w, s.size.h) == size)
            .expect("surface")
            .key
    }
}

// ------------------------------------------------------------------ 1. render/hit sweep

#[derive(Clone, Copy, Debug, PartialEq)]
enum Axis {
    X,
    Y,
}

/// One sweep configuration: output scale, the kept reference's distance `|v − R|·k` (signed: R
/// left of / above the view when positive; `None` = the per-frame reference) and the swept axis.
struct Config {
    label: &'static str,
    scale: f64,
    kept: Option<f64>,
    axis: Axis,
    /// The element's anchor: negative and large-magnitude origins included.
    anchor: CanvasPoint,
}

#[derive(Debug, Default)]
struct Stats {
    rects: u64,
    wrapped_mismatch: u64,
    inner_mismatch: u64,
    remapped: u64,
    remapped_without_mismatch: u64,
    seams: u64,
    pointer_checked: u64,
    pointer_wrong: u64,
}

/// The view for `zoom`, with the kept reference at `kept` (distance `|v − R|·k` on `axis`).
fn make_view(cfg: &Config, centre: CanvasPos, zoom: f64) -> View {
    let params = ViewParams {
        centre,
        zoom,
        scale: cfg.scale,
        view_size: (1920, 1080),
    };
    let own = View::new(params).expect("view");
    let Some(dist) = cfg.kept else {
        return own;
    };
    let m = ((dist / own.k) / 120.0).round() as i64;
    let mut r = own.r;
    match cfg.axis {
        Axis::X => r.0 -= 120 * m,
        Axis::Y => r.1 -= 120 * m,
    }
    let view = View::with_kept_reference(params, Some(r)).expect("view");
    assert_eq!(
        view.r, r,
        "{}: the kept reference must be within the bound",
        cfg.label
    );
    view
}

fn sweep(sc: &Scene, renderer: &mut DummyRenderer, cfg: &Config, rng: &mut Rng) -> Stats {
    let mut st = Stats::default();
    let a_key = sc.key_of(1);
    let (b_key, c_key) = (sc.key_of(2), sc.key_of(3));
    for _ in 0..24 {
        let zoom0 = 0.3 + 3.5 * rng.unit(); // a fit-like zoom
        let centre = CanvasPos {
            x: (cfg.anchor.x + 150 + rng.range(-60, 60)) * Q16_ONE + rng.range(0, Q16_ONE - 1),
            y: (cfg.anchor.y + 126 + rng.range(-40, 40)) * Q16_ONE + rng.range(0, Q16_ONE - 1),
        };
        let v0 = make_view(cfg, centre, zoom0);
        let entries0 = entries_for(&v0, sc.id, cfg.anchor, &sc.tree);
        let Some(a) = entries0.iter().find(|e| e.surface == a_key) else {
            continue;
        };
        // the zoom whose (edge − R)·k + δ is A's far edge's rounding point
        let d = match cfg.axis {
            Axis::X => a.origin.x + 101 - v0.r.0,
            Axis::Y => a.origin.y + 53 - v0.r.1,
        };
        let h = (d as f64 * v0.k).floor() + 0.5;
        let target = (h - DELTA) / d as f64 / cfg.scale;
        for i in -60..=60 {
            let view = make_view(cfg, centre, ulps(target, i));
            let entries = entries_for(&view, sc.id, cfg.anchor, &sc.tree);
            let elements =
                convert::render_elements(renderer, sc.ls.state.registry(), &entries, view.k)
                    .expect("elements");
            assert_eq!(elements.len(), entries.len());
            let shown: Vec<R4> = elements
                .iter()
                .map(|(_, el)| r4(el.geometry(Scale::from(view.k))))
                .collect();
            for ((e, el), p) in elements.iter().zip(&shown) {
                st.rects += 1;
                let inner = r4(el.inner().geometry(Scale::from(view.k)));
                st.wrapped_mismatch += u64::from(*p != core4(e));
                st.inner_mismatch += u64::from(inner != core4(e));
                st.remapped += u64::from(el.is_remapped());
                st.remapped_without_mismatch += u64::from(el.is_remapped() && inner == core4(e));
            }
            let pos = |k| entries.iter().position(|e| e.surface == k);
            let (Some(ia), Some(ib), Some(ic)) = (pos(a_key), pos(b_key), pos(c_key)) else {
                continue;
            };
            // shared boundaries, as presented: A|B on x, A over C on y (Core: equal by construction)
            assert_eq!(entries[ia].rect.x1, entries[ib].rect.x0);
            assert_eq!(entries[ia].rect.y1, entries[ic].rect.y0);
            st.seams +=
                u64::from(shown[ia].2 != shown[ib].0) + u64::from(shown[ia].3 != shown[ic].1);
            // pointer: every pixel across A's far edge, against the surface shown there
            let frame = ProjectionFrame {
                view,
                entries: entries.clone(),
            };
            let ar = entries[ia].rect;
            let across: Vec<(i64, i64)> = match cfg.axis {
                Axis::X => ((ar.x1 - 3)..=(ar.x1 + 2))
                    .flat_map(|x| [(x, (ar.y0 + ar.y1) / 2), (x, ar.y0), (x, ar.y1 - 1)])
                    .collect(),
                Axis::Y => ((ar.y1 - 3)..=(ar.y1 + 2))
                    .flat_map(|y| [((ar.x0 + ar.x1) / 2, y), (ar.x0, y), (ar.x1 - 1, y)])
                    .collect(),
            };
            for (x, y) in across {
                if !(0..1920).contains(&x) || !(0..1080).contains(&y) {
                    continue;
                }
                let p = OutputLogicalPoint {
                    x: (x as f64 + 0.5) / cfg.scale,
                    y: (y as f64 + 0.5) / cfg.scale,
                };
                let hit = frame.select(p).map(|h| h.surface);
                let drawn = entries
                    .iter()
                    .zip(&shown)
                    .find(|(_, r)| contains(**r, x, y))
                    .map(|(e, _)| e.surface);
                st.pointer_checked += 1;
                st.pointer_wrong += u64::from(hit != drawn);
            }
        }
    }
    st
}

/// Render/hit agreement through the wrapper (INC-10 exit item 2, C12.F15.8 control 1): k swept by
/// single ulps around rounding points of A's right edge (x) and bottom edge (y); the per-frame
/// reference at scales 1 and 1.5; kept references at |v − R|·k ≈ 2²⁰ and ≈ 2³¹ (just inside the
/// bound) on both sides of R, on both axes; negative and 2³⁶-scale origins. Every presented rect is
/// Core's rect, no shared boundary opens a seam, and every pointer pixel across the edges hits the
/// surface drawn there; Smithay's own geometry of the same elements mismatches Core's in every
/// configuration (the sweep reaches rounding points), and the wrapper remaps exactly those.
#[test]
fn render_and_hit_agree_through_the_wrapper_at_rounding_points() {
    let sc = Scene::new(Lockstep::new(Some(output((1920, 1080), 120))));
    let mut renderer = DummyRenderer;
    let near = CanvasPoint { x: 1000, y: 500 };
    let negative = CanvasPoint {
        x: -1_000_003,
        y: -777_777,
    };
    let far = CanvasPoint {
        x: (1 << 36) - 77_777,
        y: -(1 << 36) + 99_999,
    };
    let edge31 = 0.999 * (1u64 << 31) as f64;
    let configs = [
        ("per-frame, scale 1, x", 1.0, None, Axis::X, near),
        ("per-frame, scale 1.5, x", 1.5, None, Axis::X, near),
        ("per-frame, scale 1, y", 1.0, None, Axis::Y, near),
        (
            "per-frame, scale 1.5, y, negative origin",
            1.5,
            None,
            Axis::Y,
            negative,
        ),
        (
            "per-frame, scale 1, x, origin near 2^36",
            1.0,
            None,
            Axis::X,
            far,
        ),
        (
            "kept 2^20, R left, x",
            1.0,
            Some((1u64 << 20) as f64),
            Axis::X,
            near,
        ),
        (
            "kept 2^20, R right, x",
            1.0,
            Some(-((1u64 << 20) as f64)),
            Axis::X,
            near,
        ),
        ("kept 2^31, R left, x", 1.0, Some(edge31), Axis::X, near),
        ("kept 2^31, R right, x", 1.0, Some(-edge31), Axis::X, near),
        (
            "kept 2^20, R above, y",
            1.5,
            Some((1u64 << 20) as f64),
            Axis::Y,
            near,
        ),
        (
            "kept 2^31, R above, y",
            1.0,
            Some(edge31),
            Axis::Y,
            negative,
        ),
        ("kept 2^31, R below, y", 1.0, Some(-edge31), Axis::Y, near),
    ];
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let mut total = Stats::default();
    for (label, scale, kept, axis, anchor) in configs {
        let cfg = Config {
            label,
            scale,
            kept,
            axis,
            anchor,
        };
        let st = sweep(&sc, &mut renderer, &cfg, &mut rng);
        println!("{label}: {st:?}");
        assert!(st.rects > 1000, "{label}: {st:?}");
        assert_eq!(st.wrapped_mismatch, 0, "{label}: presented ≠ Core: {st:?}");
        assert_eq!(st.seams, 0, "{label}: seam: {st:?}");
        assert_eq!(st.pointer_wrong, 0, "{label}: pointer ≠ drawn: {st:?}");
        assert!(
            st.inner_mismatch > 0,
            "{label}: the sweep met no rounding point: {st:?}"
        );
        assert_eq!(
            st.remapped, st.inner_mismatch,
            "{label}: remapped ≠ mismatched: {st:?}"
        );
        assert_eq!(st.remapped_without_mismatch, 0, "{label}: {st:?}");
        total.rects += st.rects;
        total.inner_mismatch += st.inner_mismatch;
        total.pointer_checked += st.pointer_checked;
    }
    println!(
        "TOTAL: {} rects, {} unwrapped mismatches removed, {} pointer pixels",
        total.rects, total.inner_mismatch, total.pointer_checked
    );
}

/// TST-04's k ∉ n/120 counterpart (C12.F15.8, TST-04 redesign): at rounding-point k the element
/// Smithay builds at the F-RENDER-1 location — the unwrapped element — mismatches Core's rect, and
/// the wrapper is what removes it. With the wrapper's geometry seeded back to the wrapped element's
/// (C12.F15.8 control 1), the presented rects mismatch here and in the sweep above.
#[test]
fn the_wrapper_removes_the_unwrapped_mismatch() {
    let sc = Scene::new(Lockstep::new(Some(output((1920, 1080), 120))));
    let mut renderer = DummyRenderer;
    let cfg = Config {
        label: "per-frame, scale 1, x",
        scale: 1.0,
        kept: None,
        axis: Axis::X,
        anchor: CanvasPoint { x: 1000, y: 500 },
    };
    let st = sweep(&sc, &mut renderer, &cfg, &mut Rng(99));
    assert!(st.inner_mismatch > 0, "unwrapped: {st:?}");
    assert_eq!(st.wrapped_mismatch, 0, "wrapped: {st:?}");
}

// ------------------------------------------------------------------ 2. pixels and damage

/// A software-rendered output: Pixman with nearest filtering (exact edges at any k).
struct Screen {
    renderer: PixmanRenderer,
    image: Image<'static, 'static>,
    size: (i32, i32),
}

impl Screen {
    fn new(size: (i32, i32)) -> Screen {
        let mut renderer = PixmanRenderer::new().expect("pixman");
        renderer.upscale_filter(TextureFilter::Nearest).unwrap();
        renderer.downscale_filter(TextureFilter::Nearest).unwrap();
        let image = renderer
            .create_buffer(Fourcc::Argb8888, size.into())
            .expect("target");
        Screen {
            renderer,
            image,
            size,
        }
    }

    fn read(&mut self) -> Vec<u32> {
        let target = self.renderer.bind(&mut self.image).expect("bind");
        let m = self
            .renderer
            .copy_framebuffer(
                &target,
                Rectangle::from_size(self.size.into()),
                Fourcc::Argb8888,
            )
            .expect("read back");
        self.renderer
            .map_texture(&m)
            .expect("map")
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect()
    }

    /// One frame of the production path, `convert::render_output`.
    fn frame(
        &mut self,
        ls: &mut Lockstep,
        age: usize,
    ) -> (Option<Vec<Rectangle<i32, Physical>>>, Vec<u32>) {
        let o = ls.output();
        let mut target = self.renderer.bind(&mut self.image).expect("bind");
        let damage = convert::render_output(&mut ls.state, &mut self.renderer, &mut target, o, age)
            .expect("render_output");
        drop(target);
        (damage, self.read())
    }

    /// One frame of `elements` through `tracker` (a tracker the test owns).
    fn draw<E: RenderElement<PixmanRenderer>>(
        &mut self,
        tracker: &mut OutputDamageTracker,
        elements: &[E],
    ) -> Vec<u32> {
        let mut target = self.renderer.bind(&mut self.image).expect("bind");
        tracker
            .render_output(
                &mut self.renderer,
                &mut target,
                0,
                elements,
                Color32F::TRANSPARENT,
            )
            .expect("render");
        drop(target);
        self.read()
    }
}

/// The painter's oracle: the colour of the front-most entry whose canonical rect covers each pixel
/// (`colour(entry)`), transparent where none does; returns the wrong pixels.
fn wrong_pixels(
    frame: &[FrameEntry],
    colour: impl Fn(&FrameEntry) -> u32,
    px: &[u32],
    size: (i32, i32),
) -> Vec<(i64, i64, u32, u32)> {
    let mut wrong = Vec::new();
    for y in 0..size.1 as i64 {
        for x in 0..size.0 as i64 {
            let want = frame
                .iter()
                .find(|e| contains(core4(e), x, y))
                .map_or(0, &colour);
            let got = px[(y * size.0 as i64 + x) as usize];
            if got != want {
                wrong.push((x, y, want, got));
            }
        }
    }
    wrong
}

/// A camera zoom at which A's right edge is presented one pixel narrower (`narrower`) or wider
/// than Core's by the unwrapped element. Candidates are swept by single ulps around A's right-edge
/// rounding point with the production `View` / `entries_for` / `render_elements` on one scene; the
/// production stack is then built at the zoom found (camera centre 0, W centred: its anchor does
/// not depend on the zoom, F-PLACE-1) and the mismatch is checked again on its own render list.
/// Returns that stack with C opaque.
fn scene_at_rounding_point(narrower: bool) -> (Scene, f64) {
    let size = (640u32, 400u32);
    let at = |zoom: f64| {
        let persisted = PersistedCameras::from_entries(vec![(
            infynitae_backend_headless::VirtualOutput::key("A"),
            CanvasPos { x: 0, y: 0 },
            zoom,
        )])
        .unwrap();
        Scene::new(Lockstep::with(persisted, &[output(size, 120)]))
    };
    let mismatch = |els: &[(FrameEntry, convert::SurfaceElement<DummyRenderer>)], k: f64| {
        els.iter().any(|(e, el)| {
            let inner = r4(el.inner().geometry(Scale::from(k)));
            e.size.w == 101
                && e.size.h == 53
                && inner.0 == e.rect.x0
                && inner.2 == e.rect.x1 + if narrower { -1 } else { 1 }
        })
    };
    let base = at(1.0);
    let anchor = base
        .ls
        .state
        .core()
        .stage()
        .anchor(base.id)
        .expect("anchor");
    let mut r = DummyRenderer;
    let view = |zoom: f64| {
        View::new(ViewParams {
            centre: CanvasPos { x: 0, y: 0 },
            zoom,
            scale: 1.0,
            view_size: size,
        })
        .unwrap()
    };
    let mut rng = Rng(0xC0FFEE);
    for _ in 0..64 {
        let zoom0 = 0.55 + 0.9 * rng.unit();
        let v0 = view(zoom0);
        let entries = entries_for(&v0, base.id, anchor, &base.tree);
        let Some(a) = entries.iter().find(|e| e.size.w == 101 && e.size.h == 53) else {
            continue;
        };
        let d = a.origin.x + 101 - v0.r.0;
        let h = (d as f64 * v0.k).floor() + 0.5;
        let target = (h - DELTA) / d as f64;
        for i in -60..=60 {
            let zoom = ulps(target, i);
            let v = view(zoom);
            let entries = entries_for(&v, base.id, anchor, &base.tree);
            let els =
                convert::render_elements(&mut r, base.ls.state.registry(), &entries, v.k).unwrap();
            if !mismatch(&els, v.k) {
                continue;
            }
            let mut sc = at(zoom);
            let o = sc.ls.output();
            assert_eq!(sc.ls.state.core().stage().anchor(sc.id), Some(anchor));
            let frame = sc.ls.state.core_mut().render_list(o).unwrap().clone();
            let els = convert::render_elements(
                &mut r,
                sc.ls.state.registry(),
                &frame.entries,
                frame.view.k,
            )
            .unwrap();
            assert!(
                mismatch(&els, frame.view.k),
                "zoom {zoom}: the stack's own frame"
            );
            // C shares A's right-edge arithmetic (same x and width) and has W, not B, right of it
            let c = sc.idx[3];
            opaque(&mut sc.ls, c);
            solid(&mut sc.ls, c, C_RGB);
            return (sc, zoom);
        }
    }
    panic!("no rounding point found (narrower = {narrower})");
}

fn colour_of(sc: &Scene) -> impl Fn(&FrameEntry) -> u32 + '_ {
    move |e: &FrameEntry| match (e.size.w, e.size.h) {
        (803, 601) => W_RGB,
        (101, 53) => A_RGB,
        (77, 53) => B_RGB,
        (101, 37) => C_RGB,
        _ => panic!("unknown surface {:?} in {:p}", e.size, sc),
    }
}

/// Pixels through `convert::render_output` at a camera zoom where the unwrapped element presents A
/// one pixel narrower than Core's rect (A|B seam, the C12.F15.3 case), INC-10 exit item 2 and
/// C12.F15.8 controls 2–3: every pixel is the front-most canonical rect's colour; the pointer hits,
/// column by column, the surface whose colour is drawn; an unchanged frame produces no damage; a
/// repaint of A at age 1 damages exactly A's canonical rect and leaves no stale column.
#[test]
fn pixels_and_pointer_follow_the_canonical_rect_at_a_seam_zoom() {
    let (mut sc, zoom) = scene_at_rounding_point(true);
    let size = (640, 400);
    let mut screen = Screen::new(size);
    let o = sc.ls.output();
    let (_, px) = screen.frame(&mut sc.ls, 0);
    let frame = sc.ls.state.core_mut().render_list(o).unwrap().clone();
    assert!(
        frame.view.k == zoom && (frame.view.k * 120.0).fract() != 0.0,
        "k ∉ n/120"
    );
    let wrong = wrong_pixels(&frame.entries, colour_of(&sc), &px, size);
    assert!(
        wrong.is_empty(),
        "zoom {zoom}: {} wrong pixels, e.g. {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(5)]
    );
    // pointer, every column of A's middle row and of the A|B boundary rows
    let a = frame
        .entries
        .iter()
        .find(|e| e.size.w == 101 && e.size.h == 53)
        .unwrap()
        .rect;
    for y in [a.y0, (a.y0 + a.y1) / 2, a.y1 - 1] {
        for x in (a.x0 - 2)..(a.x1 + 80).min(size.0 as i64) {
            let p = OutputLogicalPoint {
                x: x as f64 + 0.5,
                y: y as f64 + 0.5,
            };
            let hit = sc.ls.state.core_mut().hit_test(o, p).map(|h| h.surface);
            let drawn = px[(y * size.0 as i64 + x) as usize];
            let hit_colour =
                hit.map(|s| colour_of(&sc)(frame.entries.iter().find(|e| e.surface == s).unwrap()));
            assert_eq!(hit_colour, Some(drawn), "column {x} row {y}: hit ≠ drawn");
        }
    }
    // an unchanged scene: no damage
    let (damage, _) = screen.frame(&mut sc.ls, 1);
    assert!(
        damage.as_ref().is_none_or(|d| d.is_empty()),
        "unchanged frame damaged {damage:?}"
    );
    // A repainted (remapped), only its last buffer column damaged: the age-1 damage is all of A's
    // canonical rect, so its last canonical column — where that buffer column is drawn — is repainted
    // (passed-through damage would be in Smithay's own, narrower coordinates and miss it)
    let ai = sc.idx[1];
    let new = 0xFFFF_FF00;
    attach_damaged(
        &mut sc.ls,
        ai,
        (101, 53),
        wl_output::Transform::Normal,
        |x, _| if x == 100 { new } else { A_RGB },
        (100, 0, 1, 53),
    );
    let (damage, px) = screen.frame(&mut sc.ls, 1);
    let want = Rectangle::new(
        (a.x0 as i32, a.y0 as i32).into(),
        ((a.x1 - a.x0) as i32, (a.y1 - a.y0) as i32).into(),
    );
    assert_eq!(
        damage,
        Some(vec![want]),
        "age-1 damage of a remapped repaint"
    );
    for y in a.y0..a.y1 {
        let at = |x: i64| px[(y * size.0 as i64 + x) as usize];
        assert_eq!(
            at(a.x1 - 1),
            new,
            "row {y}: A's last canonical column not repainted"
        );
        assert_eq!(at(a.x0), A_RGB, "row {y}");
    }
    // B repainted (not remapped): damage passes through, at the tracker's Scale(k) — exactly B's rect
    let b = frame.entries.iter().find(|e| e.size.w == 77).unwrap();
    let bi = sc.idx[2];
    solid(&mut sc.ls, bi, 0xFF00_FFFF);
    let (damage, px) = screen.frame(&mut sc.ls, 1);
    let rb = b.rect;
    let want = Rectangle::new(
        (rb.x0 as i32, rb.y0 as i32).into(),
        ((rb.x1 - rb.x0) as i32, (rb.y1 - rb.y0) as i32).into(),
    );
    assert_eq!(
        damage,
        Some(vec![want]),
        "age-1 damage of a pass-through repaint"
    );
    let colours = |e: &FrameEntry| match e.size.w {
        77 => 0xFF00_FFFF,
        _ => colour_of(&sc)(e),
    };
    let mut wrong = wrong_pixels(&frame.entries, colours, &px, size);
    wrong.retain(|&(x, y, _, _)| {
        !contains(
            core4(
                frame
                    .entries
                    .iter()
                    .find(|e| e.size.w == 101 && e.size.h == 53)
                    .unwrap(),
            ),
            x,
            y,
        )
    });
    assert!(
        wrong.is_empty(),
        "after the repaints: {} wrong, e.g. {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(5)]
    );
}

/// Opaque regions after a remap (C12.F15.8 control 4): C opaque, presented one pixel *wider* by
/// the unwrapped element than Core's rect, with W (not another subsurface) right of it. When W is
/// repainted, Smithay skips W's pixels under the opaque regions the elements report; C must claim
/// none (remapped), so the column right of C's canonical rect is repainted with W's new colour.
#[test]
fn a_remapped_opaque_surface_claims_no_opaque_region() {
    let (mut sc, zoom) = scene_at_rounding_point(false);
    let size = (640, 400);
    let mut screen = Screen::new(size);
    let o = sc.ls.output();
    let (_, px) = screen.frame(&mut sc.ls, 0);
    let frame = sc.ls.state.core_mut().render_list(o).unwrap().clone();
    let wrong = wrong_pixels(&frame.entries, colour_of(&sc), &px, size);
    assert!(wrong.is_empty(), "zoom {zoom}: {} wrong", wrong.len());
    let w = sc.idx[0];
    attach(
        &mut sc.ls,
        w,
        (803, 601),
        wl_output::Transform::Normal,
        |_, _| 0xFFC0_C0C0,
    );
    let (_, px) = screen.frame(&mut sc.ls, 1);
    let colours = |e: &FrameEntry| {
        if e.size.w == 803 {
            0xFFC0_C0C0
        } else {
            colour_of(&sc)(e)
        }
    };
    let wrong = wrong_pixels(&frame.entries, colours, &px, size);
    assert!(
        wrong.is_empty(),
        "zoom {zoom}: {} stale pixels, e.g. {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(5)]
    );
}

// ------------------------------------------------------------------ 3. crop and buffer transforms

const TRANSFORMS: [(wl_output::Transform, Transform); 8] = [
    (wl_output::Transform::Normal, Transform::Normal),
    (wl_output::Transform::_90, Transform::_90),
    (wl_output::Transform::_180, Transform::_180),
    (wl_output::Transform::_270, Transform::_270),
    (wl_output::Transform::Flipped, Transform::Flipped),
    (wl_output::Transform::Flipped90, Transform::Flipped90),
    (wl_output::Transform::Flipped180, Transform::Flipped180),
    (wl_output::Transform::Flipped270, Transform::Flipped270),
];

/// A buffer whose every pixel is distinct: red = x, green = y.
fn pattern(x: i32, y: i32) -> u32 {
    0xFF00_0080 | ((x as u32 & 0xFF) << 16) | ((y as u32 & 0xFF) << 8)
}

const BUF: (i32, i32) = (64, 40);

/// A toplevel with the pattern buffer attached under `transform`; returns the scene's stack, the
/// element and its tree.
fn transformed(t: wl_output::Transform) -> (Lockstep, ElementId, SurfaceTree) {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let rotated = matches!(
        t,
        wl_output::Transform::_90
            | wl_output::Transform::_270
            | wl_output::Transform::Flipped90
            | wl_output::Transform::Flipped270
    );
    let logical = if rotated { (BUF.1, BUF.0) } else { BUF };
    let w = ls.probe.toplevel(&qh, "T", logical, 0);
    ls.pump();
    attach(&mut ls, w, BUF, t, pattern);
    let id = ls.state.elements().next().expect("element");
    let tree = ls.state.core().stage().tree(id).expect("tree").clone();
    assert_eq!(
        (tree.surfaces[0].size.w, tree.surfaces[0].size.h),
        logical,
        "{t:?}: the surface's logical size follows its buffer transform"
    );
    (ls, id, tree)
}

/// The entry of the tree's only surface under a view at `zoom` whose origin puts the surface's
/// canvas origin `at` physical px from the view's origin (scale 1).
fn entry_at(
    ls: &Lockstep,
    id: ElementId,
    tree: &SurfaceTree,
    zoom: f64,
    at: (i64, i64),
) -> FrameEntry {
    let anchor = CanvasPoint { x: 0, y: 0 };
    let size = (400u32, 400u32);
    // v = −at/zoom (Canvas): the centre is v + view/(2k)
    let centre = CanvasPos {
        x: ((-(at.0 as f64) / zoom + size.0 as f64 / (2.0 * zoom)) * Q16_ONE as f64) as i64,
        y: ((-(at.1 as f64) / zoom + size.1 as f64 / (2.0 * zoom)) * Q16_ONE as f64) as i64,
    };
    let view = View::new(ViewParams {
        centre,
        zoom,
        scale: 1.0,
        view_size: size,
    })
    .unwrap();
    let _ = ls;
    entries_for(&view, id, anchor, tree)
        .into_iter()
        .next()
        .expect("in view")
}

/// The crop, drawn (C12.F15.5, INC-10 exit item 6), for every buffer transform: a surface cut by a
/// crop box drawn by Pixman through Smithay's damage tracker is, pixel for pixel, the uncropped
/// surface inside the box and nothing outside it — the `src` crop maps exactly the visible part.
/// The production box `[−2³⁰, 2³⁰]` cannot cut a drawable surface (Pixman stops near 2¹⁵), so the
/// box is made small (`CanonicalRect::with_crop_box`); the crop code is the same. Two placements:
/// cut on the right and bottom; cut on all four sides (left and top outside the view).
#[test]
fn a_cropped_surface_draws_exactly_its_visible_part_for_every_buffer_transform() {
    let size = (400, 400);
    for (wl_t, t) in TRANSFORMS {
        let (mut ls, id, tree) = transformed(wl_t);
        // cuts on whole buffer pixels: Smithay's Pixman renderer rounds `src` out to integers
        // (`to_i32_up`), so it cannot draw a fractional cut exactly; fractional `src` values are
        // checked arithmetically below (`rects_beyond_the_i32_range_are_cropped_with_a_proportional_src`)
        for (zoom, at, limit) in [(4.0, (200i64, 100i64), 292i64), (8.0, (-200, -152), 104)] {
            let e = entry_at(&ls, id, &tree, zoom, at);
            assert_eq!((e.rect.x0, e.rect.y0), at, "{t:?}");
            let mut screen = Screen::new(size);
            let mut pixels = |box_limit: i64| {
                let inner = convert::surface_element(&mut screen.renderer, ls.state.registry(), &e)
                    .unwrap()
                    .expect("element");
                assert_eq!(inner.transform(), t);
                let el = CanonicalRect::with_crop_box(inner, e.rect, zoom, box_limit);
                let mut tracker =
                    OutputDamageTracker::new(size, Scale::from(zoom), Transform::Normal);
                (screen.draw(&mut tracker, &[el]), ())
            };
            let (full, _) = pixels(RENDER_LIMIT);
            let (cut, _) = pixels(limit);
            let (mut inside, mut drawn) = (0, 0);
            for y in 0..size.1 as i64 {
                for x in 0..size.0 as i64 {
                    let i = (y * size.0 as i64 + x) as usize;
                    if x < limit && y < limit {
                        assert_eq!(
                            cut[i], full[i],
                            "{t:?} limit {limit}: pixel ({x},{y}) inside the box"
                        );
                        inside += 1;
                        drawn += u32::from(full[i] != 0);
                    } else {
                        assert_eq!(
                            cut[i], 0,
                            "{t:?} limit {limit}: pixel ({x},{y}) outside the box drawn"
                        );
                    }
                }
            }
            assert!(
                inside > 0 && drawn > 1000,
                "{t:?}: {drawn} drawn pixels inside the box"
            );
            // the uncropped surface is the pattern under t, each buffer pixel zoom × zoom px
            ls.pump();
        }
        let _ = &mut ls;
    }
}

/// The buffer pixel each transform shows at a surface-local point `(u, v)` ∈ [0, 1)², as fractions
/// of the buffer — the oracle of the extreme-magnification crop below, validated against Pixman's
/// pixels for all eight transforms in `the_transform_oracle_matches_the_pixels`.
fn shown_buffer_fraction(t: Transform, u: f64, v: f64) -> (f64, f64) {
    match t {
        Transform::Normal => (u, v),
        Transform::_90 => (v, 1.0 - u),
        Transform::_180 => (1.0 - u, 1.0 - v),
        Transform::_270 => (1.0 - v, u),
        Transform::Flipped => (1.0 - u, v),
        Transform::Flipped90 => (v, u),
        Transform::Flipped180 => (u, 1.0 - v),
        Transform::Flipped270 => (1.0 - v, 1.0 - u),
    }
}

/// The oracle against Smithay's GLES texture matrix (algebra only, no GPU): `build_texture_mat`
/// (`gles/mod.rs`, private) maps a destination pixel p to `src.loc + T·(t.matrix()·p)` with the
/// per-transform translation T copied from it; its fraction of `src` is the oracle's, for all eight
/// transforms. With `the_transform_oracle_matches_the_pixels` (Pixman) this pins the mapping the
/// crop must follow on both renderers Smithay ships.
#[test]
fn the_transform_oracle_matches_the_gles_texture_matrix() {
    let (sw, sh) = (64.0f32, 40.0f32);
    for (_, t) in TRANSFORMS {
        let m = t.matrix();
        // dst_src_size = transform_size(src.size); the destination has that size at scale 1
        let (dw, dh) = if matches!(
            t,
            Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270
        ) {
            (sh, sw)
        } else {
            (sw, sh)
        };
        let translation = match t {
            Transform::Normal => (0.0, 0.0),
            Transform::_90 => (0.0, dw),
            Transform::_180 => (dw, dh),
            Transform::_270 => (dh, 0.0),
            Transform::Flipped => (dw, 0.0),
            Transform::Flipped90 => (0.0, 0.0),
            Transform::Flipped180 => (0.0, dh),
            Transform::Flipped270 => (dh, dw),
        };
        for (u, v) in [
            (0.0f32, 0.0f32),
            (1.0, 0.0),
            (0.0, 1.0),
            (0.25, 0.75),
            (0.6, 0.1),
        ] {
            let (px, py) = (u * dw, v * dh);
            let tx = m.x.x * px + m.y.x * py + m.z.x + translation.0;
            let ty = m.x.y * px + m.y.y * py + m.z.y + translation.1;
            let (fx, fy) = shown_buffer_fraction(t, u as f64, v as f64);
            assert!(
                ((tx / sw) as f64 - fx).abs() < 1e-6 && ((ty / sh) as f64 - fy).abs() < 1e-6,
                "{t:?} ({u},{v}): GLES samples ({},{}) of src, oracle ({fx},{fy})",
                tx / sw,
                ty / sh
            );
        }
    }
}

/// Why the crop does not use Smithay's `CropRenderElement` formula, as C12.F15.5 names it (evidence
/// for change control, pinned to Smithay 0.7.0): Smithay's own `CropRenderElement` around the same
/// surface element, cut by the same box, draws the wrong part of the buffer in Pixman under the four
/// rotating transforms (90°, 270°, flipped 90°, flipped 270°), while the production crop
/// (`a_cropped_surface_draws_exactly_…`) draws exactly the visible part under all eight.
#[test]
fn smithays_crop_element_misdraws_rotated_buffers() {
    use smithay::backend::renderer::element::utils::CropRenderElement;
    let size = (400, 400);
    let mut wrong_under = Vec::new();
    for (wl_t, t) in TRANSFORMS {
        let (ls, id, tree) = transformed(wl_t);
        let (zoom, limit) = (4.0, 292i64);
        let e = entry_at(&ls, id, &tree, zoom, (200, 100));
        let mut screen = Screen::new(size);
        let full = {
            let el = convert::surface_element(&mut screen.renderer, ls.state.registry(), &e)
                .unwrap()
                .unwrap();
            let mut tracker = OutputDamageTracker::new(size, Scale::from(zoom), Transform::Normal);
            screen.draw(&mut tracker, &[el])
        };
        let cut = {
            let el = convert::surface_element(&mut screen.renderer, ls.state.registry(), &e)
                .unwrap()
                .unwrap();
            let crop = Rectangle::new(
                (-1000, -1000).into(),
                ((limit + 1000) as i32, (limit + 1000) as i32).into(),
            );
            let el = CropRenderElement::from_element(el, Scale::from(zoom), crop).expect("overlap");
            let mut tracker = OutputDamageTracker::new(size, Scale::from(zoom), Transform::Normal);
            screen.draw(&mut tracker, &[el])
        };
        let differs = (0..size.1 as i64).any(|y| {
            (0..size.0 as i64).any(|x| {
                let i = (y * size.0 as i64 + x) as usize;
                x < limit && y < limit && cut[i] != full[i]
            })
        });
        if differs {
            wrong_under.push(t);
        }
    }
    println!("Smithay CropRenderElement misdraws under {wrong_under:?}");
    assert_eq!(
        wrong_under,
        vec![
            Transform::_90,
            Transform::_270,
            Transform::Flipped90,
            Transform::Flipped270
        ],
        "Smithay 0.7.0's CropRenderElement: the transforms it misdraws"
    );
}

/// The oracle's validation: for every transform, the colour Pixman draws at the centre of every
/// buffer pixel's image is the buffer pixel `shown_buffer_fraction` names.
#[test]
fn the_transform_oracle_matches_the_pixels() {
    let size = (400, 400);
    for (wl_t, t) in TRANSFORMS {
        let (ls, id, tree) = transformed(wl_t);
        let zoom = 4.0;
        let e = entry_at(&ls, id, &tree, zoom, (10, 20));
        let mut screen = Screen::new(size);
        let inner = convert::surface_element(&mut screen.renderer, ls.state.registry(), &e)
            .unwrap()
            .expect("element");
        let el = CanonicalRect::new(inner, e.rect, zoom);
        let mut tracker = OutputDamageTracker::new(size, Scale::from(zoom), Transform::Normal);
        let px = screen.draw(&mut tracker, &[el]);
        let (w, h) = (e.rect.x1 - e.rect.x0, e.rect.y1 - e.rect.y0);
        for j in 0..(h / 4) {
            for i in 0..(w / 4) {
                let (x, y) = (e.rect.x0 + 4 * i + 2, e.rect.y0 + 4 * j + 2);
                let (u, v) = (
                    (x - e.rect.x0) as f64 / w as f64,
                    (y - e.rect.y0) as f64 / h as f64,
                );
                let (fx, fy) = shown_buffer_fraction(t, u, v);
                let (bx, by) = ((fx * BUF.0 as f64) as i32, (fy * BUF.1 as f64) as i32);
                assert_eq!(
                    px[(y * size.0 as i64 + x) as usize],
                    pattern(bx, by),
                    "{t:?}: pixel ({x},{y})"
                );
            }
        }
    }
}

/// The crop at extreme magnification (C12.F15.5): canonical rects with edges beyond ±2³¹ physical
/// px (k = 2¹⁹ and beyond, both signs, both axes) for every buffer transform. The presented
/// geometry is the rect cropped to `[−2³⁰, 2³⁰]` (exact, no i32 saturation), the visible view is
/// inside it, and `src` is the visible fraction of the buffer under the transform (the oracle);
/// an uncropped rect keeps Smithay's `src`.
#[test]
fn rects_beyond_the_i32_range_are_cropped_with_a_proportional_src() {
    let lim = RENDER_LIMIT;
    for (wl_t, t) in TRANSFORMS {
        let (ls, id, tree) = transformed(wl_t);
        let e = entry_at(&ls, id, &tree, 1.0, (10, 20));
        let mut r = DummyRenderer;
        let base = convert::surface_element(&mut r, ls.state.registry(), &e)
            .unwrap()
            .unwrap();
        let src = base.src();
        // inside the box: nothing changes
        let inside = CanonicalRect::new(
            convert::surface_element(&mut r, ls.state.registry(), &e)
                .unwrap()
                .unwrap(),
            e.rect,
            1.0,
        );
        assert_eq!(inside.src(), src);
        assert!(!inside.is_remapped());
        let cases: [PhysicalRect; 5] = [
            PhysicalRect {
                x0: -(1 << 40),
                y0: -(1 << 40),
                x1: 997,
                y1: 613,
            },
            PhysicalRect {
                x0: -50,
                y0: -70,
                x1: 1 << 41,
                y1: 1 << 35,
            },
            PhysicalRect {
                x0: -(1 << 33),
                y0: -(1 << 31) - 5,
                x1: (1 << 33) + 3,
                y1: (1 << 31) + 7,
            },
            PhysicalRect {
                x0: -(1i64 << 31) + 1,
                y0: 0,
                x1: (1i64 << 31) - 1,
                y1: 400,
            },
            PhysicalRect {
                x0: 100,
                y0: -(1 << 36),
                x1: 1 << 30,
                y1: -(1 << 30) - 1,
            }, // off-range on y
        ];
        for rect in cases {
            let el = CanonicalRect::new(
                convert::surface_element(&mut r, ls.state.registry(), &e)
                    .unwrap()
                    .unwrap(),
                rect,
                (1u64 << 19) as f64,
            );
            let g = el.geometry(Scale::from(1.0));
            // the box, with a whole-box span one pixel short of 2³¹ (Smithay's i32 size)
            let (x0, y0) = (rect.x0.clamp(-lim, lim), rect.y0.clamp(-lim, lim));
            let c = (
                x0,
                y0,
                rect.x1.clamp(-lim, lim).min(x0 + i32::MAX as i64),
                rect.y1.clamp(-lim, lim).min(y0 + i32::MAX as i64),
            );
            let want_w = (c.2 - c.0).max(0);
            let want_h = (c.3 - c.1).max(0);
            assert_eq!(
                r4(g),
                if want_w > 0 && want_h > 0 {
                    c
                } else {
                    (c.0, c.1, c.0 + want_w, c.1 + want_h)
                },
                "{t:?} {rect:?}: geometry"
            );
            assert!(el.is_remapped(), "{t:?} {rect:?}");
            if want_w == 0 || want_h == 0 {
                continue; // nothing of it is presented
            }
            // the visible fraction of the uncropped rect, through the transform oracle
            let (fw, fh) = ((rect.x1 - rect.x0) as f64, (rect.y1 - rect.y0) as f64);
            let (u0, u1) = ((c.0 - rect.x0) as f64 / fw, (c.2 - rect.x0) as f64 / fw);
            let (v0, v1) = ((c.1 - rect.y0) as f64 / fh, (c.3 - rect.y0) as f64 / fh);
            let corners = [
                shown_buffer_fraction(t, u0, v0),
                shown_buffer_fraction(t, u1, v1),
            ];
            let (bx0, bx1) = (
                corners[0].0.min(corners[1].0),
                corners[0].0.max(corners[1].0),
            );
            let (by0, by1) = (
                corners[0].1.min(corners[1].1),
                corners[0].1.max(corners[1].1),
            );
            let want = Rectangle::<f64, Buffer>::new(
                (src.loc.x + bx0 * src.size.w, src.loc.y + by0 * src.size.h).into(),
                ((bx1 - bx0) * src.size.w, (by1 - by0) * src.size.h).into(),
            );
            let got = el.src();
            let tol = 1e-9 * src.size.w.max(src.size.h);
            for (a, b, what) in [
                (got.loc.x, want.loc.x, "x"),
                (got.loc.y, want.loc.y, "y"),
                (got.size.w, want.size.w, "w"),
                (got.size.h, want.size.h, "h"),
            ] {
                assert!(
                    (a - b).abs() <= tol,
                    "{t:?} {rect:?}: src.{what} {a} ≠ {b} (got {got:?})"
                );
            }
            // the view [0, 2¹⁵)² lies inside the presented geometry wherever the rect covers it
            let (gx0, gy0, gx1, gy1) = r4(g);
            assert!(gx0 <= rect.x0.max(0) && gx1 >= rect.x1.min(1 << 15));
            assert!(gy0 <= rect.y0.max(0) && gy1 >= rect.y1.min(1 << 15));
        }
    }
}

/// What `draw` receives (C12.F15.3 item 1, INC-10 exit item 6): Smithay's damage tracker draws the
/// wrapper at its geometry with its `src`, and the wrapper hands both to the wrapped element
/// unchanged — also for a cropped rect at k = 2¹⁹.
#[test]
fn the_tracker_draws_the_wrapped_element_at_the_canonical_rect() {
    /// The (src, dst) of every draw call.
    type Draws = Rc<RefCell<Vec<(Rectangle<f64, Buffer>, Rectangle<i32, Physical>)>>>;
    #[derive(Debug)]
    struct Recording {
        id: Id,
        src: Rectangle<f64, Buffer>,
        geometry: Rectangle<i32, Physical>,
        drawn: Draws,
    }
    impl Element for Recording {
        fn id(&self) -> &Id {
            &self.id
        }
        fn current_commit(&self) -> CommitCounter {
            CommitCounter::default()
        }
        fn src(&self) -> Rectangle<f64, Buffer> {
            self.src
        }
        fn transform(&self) -> Transform {
            Transform::_90
        }
        fn geometry(&self, _: Scale<f64>) -> Rectangle<i32, Physical> {
            self.geometry
        }
        fn kind(&self) -> Kind {
            Kind::Unspecified
        }
    }
    impl RenderElement<DummyRenderer> for Recording {
        fn draw(
            &self,
            _: &mut smithay::backend::renderer::test::DummyFrame,
            src: Rectangle<f64, Buffer>,
            dst: Rectangle<i32, Physical>,
            _: &[Rectangle<i32, Physical>],
            _: &[Rectangle<i32, Physical>],
        ) -> Result<(), <DummyRenderer as smithay::backend::renderer::RendererSuper>::Error>
        {
            self.drawn.borrow_mut().push((src, dst));
            Ok(())
        }
    }
    let drawn = Rc::new(RefCell::new(Vec::new()));
    let src = Rectangle::<f64, Buffer>::new((0.0, 0.0).into(), (64.0, 40.0).into());
    for rect in [
        PhysicalRect {
            x0: 923,
            y0: 100,
            x1: 997,
            y1: 153,
        },
        PhysicalRect {
            x0: -(1 << 40),
            y0: -(1 << 32),
            x1: 1 << 40,
            y1: 1 << 32,
        },
    ] {
        drawn.borrow_mut().clear();
        let inner = Recording {
            id: Id::new(),
            src,
            geometry: Rectangle::new((900, 100).into(), (73, 53).into()), // Smithay's own: 1 px off
            drawn: drawn.clone(),
        };
        let el = CanonicalRect::new(inner, rect, (1u64 << 19) as f64);
        let (g, s) = (el.geometry(Scale::from(1.0)), el.src());
        let mut tracker =
            OutputDamageTracker::new((1920, 1080), Scale::from(1.0), Transform::Normal);
        let mut renderer = DummyRenderer;
        let mut fb = smithay::backend::renderer::test::DummyFramebuffer;
        tracker
            .render_output(&mut renderer, &mut fb, 0, &[el], Color32F::TRANSPARENT)
            .expect("render");
        let calls = drawn.borrow().clone();
        assert_eq!(calls.len(), 1, "{rect:?}");
        assert_eq!(
            calls[0],
            (s, g),
            "{rect:?}: draw got the wrapper's src and geometry"
        );
    }
}

/// Pixels for every configuration of the render/hit sweep above (INC-10 exit review, finding L4):
/// on each frame where the wrapper remaps a surface — Smithay's own geometry differs from Core's,
/// the case option B exists for — the production `render_elements` output is drawn by Smithay's
/// `OutputDamageTracker` with Pixman into a 1920 × 1080 image, and every pixel must be the colour of
/// the front-most surface whose canonical rect covers it (the painter's oracle on Core's entries,
/// not on the presented geometry). Both axes, scales 1 and 1.5, the per-frame reference and kept
/// references at ≈ 2²⁰ and ≈ 2³¹ on both sides, negative and 2³⁶-scale origins. Pixman only (integer
/// cuts, nearest filtering); no GLES or GPU claim.
#[test]
fn pixels_follow_the_canonical_rect_in_every_sweep_configuration() {
    let sc = Scene::new(Lockstep::new(Some(output((1920, 1080), 120))));
    let near = CanvasPoint { x: 1000, y: 500 };
    let negative = CanvasPoint {
        x: -1_000_003,
        y: -777_777,
    };
    let far = CanvasPoint {
        x: (1 << 36) - 77_777,
        y: -(1 << 36) + 99_999,
    };
    let edge31 = 0.999 * (1u64 << 31) as f64;
    let configs = [
        ("per-frame, scale 1, x", 1.0, None, Axis::X, near),
        ("per-frame, scale 1.5, x", 1.5, None, Axis::X, near),
        ("per-frame, scale 1, y", 1.0, None, Axis::Y, near),
        (
            "per-frame, scale 1.5, y, negative origin",
            1.5,
            None,
            Axis::Y,
            negative,
        ),
        (
            "per-frame, scale 1, x, origin near 2^36",
            1.0,
            None,
            Axis::X,
            far,
        ),
        (
            "kept 2^20, R left, x",
            1.0,
            Some((1u64 << 20) as f64),
            Axis::X,
            near,
        ),
        (
            "kept 2^20, R right, x",
            1.0,
            Some(-((1u64 << 20) as f64)),
            Axis::X,
            near,
        ),
        ("kept 2^31, R left, x", 1.0, Some(edge31), Axis::X, near),
        ("kept 2^31, R right, x", 1.0, Some(-edge31), Axis::X, near),
        (
            "kept 2^20, R above, y",
            1.5,
            Some((1u64 << 20) as f64),
            Axis::Y,
            near,
        ),
        (
            "kept 2^31, R above, y",
            1.0,
            Some(edge31),
            Axis::Y,
            negative,
        ),
        ("kept 2^31, R below, y", 1.0, Some(-edge31), Axis::Y, near),
    ];
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let mut screen = Screen::new((1920, 1080));
    let a_key = sc.key_of(1);
    let mut frames = 0;
    for (label, scale, kept, axis, anchor) in configs {
        let cfg = Config {
            label,
            scale,
            kept,
            axis,
            anchor,
        };
        let mut cfg_frames = 0;
        for _ in 0..24 {
            let zoom0 = 0.3 + 3.5 * rng.unit();
            let centre = CanvasPos {
                x: (cfg.anchor.x + 150 + rng.range(-60, 60)) * Q16_ONE + rng.range(0, Q16_ONE - 1),
                y: (cfg.anchor.y + 126 + rng.range(-40, 40)) * Q16_ONE + rng.range(0, Q16_ONE - 1),
            };
            let v0 = make_view(&cfg, centre, zoom0);
            let e0 = entries_for(&v0, sc.id, cfg.anchor, &sc.tree);
            let Some(a) = e0.iter().find(|e| e.surface == a_key) else {
                continue;
            };
            // A's right (x) or bottom (y) edge: the zoom at which it rounds, swept by single ulps
            let d = match cfg.axis {
                Axis::X => a.origin.x + 101 - v0.r.0,
                Axis::Y => a.origin.y + 53 - v0.r.1,
            };
            let h = (d as f64 * v0.k).floor() + 0.5;
            let target = (h - DELTA) / d as f64 / cfg.scale;
            for i in -60..=60 {
                let view = make_view(&cfg, centre, ulps(target, i));
                let entries = entries_for(&view, sc.id, cfg.anchor, &sc.tree);
                let els = convert::render_elements(
                    &mut screen.renderer,
                    sc.ls.state.registry(),
                    &entries,
                    view.k,
                )
                .unwrap();
                if !els.iter().any(|(_, el)| el.is_remapped()) {
                    continue;
                }
                let mut tracker =
                    OutputDamageTracker::new((1920, 1080), Scale::from(view.k), Transform::Normal);
                let els: Vec<_> = els.into_iter().map(|(_, e)| e).collect();
                let px = screen.draw(&mut tracker, &els);
                let colour = |e: &FrameEntry| match (e.size.w, e.size.h) {
                    (803, 601) => W_RGB,
                    (101, 53) => A_RGB,
                    (77, 53) => B_RGB,
                    _ => C_RGB,
                };
                let wrong = wrong_pixels(&entries, colour, &px, (1920, 1080));
                assert!(
                    wrong.is_empty(),
                    "{label} zoom {}: {} wrong pixels, e.g. {:?}",
                    view.zoom,
                    wrong.len(),
                    &wrong[..wrong.len().min(3)]
                );
                cfg_frames += 1;
            }
        }
        println!("{label}: {cfg_frames} remapped frames drawn, 0 wrong pixels");
        assert!(
            cfg_frames >= 3,
            "{label}: too few remapped frames ({cfg_frames})"
        );
        frames += cfg_frames;
    }
    println!("TOTAL: {frames} remapped frames × 1920 × 1080, 0 wrong pixels");
}

/// A `wp_viewport`: the source rect in buffer-local logical px (x, y, w, h) and the destination size.
type Viewport = ((f64, f64, f64, f64), (i32, i32));

/// A toplevel with a pattern buffer of `buf` px under `t` at buffer scale `bs`, optionally with a
/// viewport; returns the stack, the element and its tree.
fn transformed_src(
    t: wl_output::Transform,
    buf: (i32, i32),
    bs: i32,
    viewport: Option<Viewport>,
) -> (Lockstep, ElementId, SurfaceTree) {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let rotated = matches!(
        t,
        wl_output::Transform::_90
            | wl_output::Transform::_270
            | wl_output::Transform::Flipped90
            | wl_output::Transform::Flipped270
    );
    let logical = if rotated {
        (buf.1 / bs, buf.0 / bs)
    } else {
        (buf.0 / bs, buf.1 / bs)
    };
    let size = viewport.map_or(logical, |(_, d)| d);
    let w = ls.probe.toplevel(&qh, "T", size, 0);
    ls.pump();
    if let Some(((x, y, sw, sh), (dw, dh))) = viewport {
        let vp = ls.probe.get_viewport(&qh, w).expect("viewporter");
        vp.set_source(x, y, sw, sh);
        vp.set_destination(dw, dh);
    }
    ls.probe.surfaces[w].wl.set_buffer_scale(bs);
    attach(&mut ls, w, buf, t, pattern);
    let id = ls.state.elements().next().expect("element");
    let tree = ls.state.core().stage().tree(id).expect("tree").clone();
    assert_eq!(
        (tree.surfaces[0].size.w, tree.surfaces[0].size.h),
        size,
        "{t:?}: the surface's logical size"
    );
    (ls, id, tree)
}

/// The crop's source-sampling requirement with a non-trivial `src` (INC-10 exit review E1,
/// proposed C12.F17): a viewport source offset, buffer scale 2, and both together, for all eight buffer
/// transforms. A surface cut by a crop box and drawn by Pixman through Smithay's damage tracker
/// is, pixel for pixel, the uncropped surface inside the box and nothing outside it. Cuts lie on
/// whole buffer pixels (8 physical px = 1 logical px at zoom 8): Pixman rounds `src` to integers,
/// so fractional cuts are not drawn here; no GLES or GPU claim.
#[test]
fn a_cropped_surface_keeps_its_sampling_with_a_viewport_source_and_buffer_scale() {
    let size = (400, 400);
    let mut cases = 0;
    for (wl_t, t) in TRANSFORMS {
        for (label, buf, bs, vp) in [
            (
                "viewport src (4, 8, 24, 24)",
                (64, 40),
                1,
                Some(((4.0, 8.0, 24.0, 24.0), (24, 24))),
            ),
            ("buffer scale 2", (128, 80), 2, None),
            (
                "viewport src (2, 4, 24, 16), buffer scale 2",
                (128, 80),
                2,
                Some(((2.0, 4.0, 24.0, 16.0), (24, 16))),
            ),
        ] {
            let (ls, id, tree) = transformed_src(wl_t, buf, bs, vp);
            let zoom = 8.0;
            let e = entry_at(&ls, id, &tree, zoom, (40, 32));
            assert_eq!((e.rect.x0, e.rect.y0), (40, 32), "{t:?} {label}");
            let mut screen = Screen::new(size);
            let mut pixels = |box_limit: i64| {
                let inner = convert::surface_element(&mut screen.renderer, ls.state.registry(), &e)
                    .unwrap()
                    .expect("element");
                assert_eq!(inner.transform(), t);
                let el = CanonicalRect::with_crop_box(inner, e.rect, zoom, box_limit);
                let mut tracker =
                    OutputDamageTracker::new(size, Scale::from(zoom), Transform::Normal);
                screen.draw(&mut tracker, &[el])
            };
            let full = pixels(RENDER_LIMIT);
            // a cut one logical px (a whole buffer px at both scales) past the rect's middle
            let limit = (e.rect.x0 + e.rect.x1) / 2 / 8 * 8 + 8;
            assert!(
                limit < e.rect.x1 && limit < e.rect.y1,
                "{t:?} {label}: no cut"
            );
            let cut = pixels(limit);
            let mut drawn = 0;
            for y in 0..size.1 as i64 {
                for x in 0..size.0 as i64 {
                    let i = (y * size.0 as i64 + x) as usize;
                    if x < limit && y < limit {
                        assert_eq!(
                            cut[i], full[i],
                            "{t:?} {label} limit {limit}: pixel ({x},{y}) inside the box"
                        );
                        drawn += u32::from(full[i] != 0);
                    } else {
                        assert_eq!(
                            cut[i], 0,
                            "{t:?} {label} limit {limit}: pixel ({x},{y}) outside the box drawn"
                        );
                    }
                }
            }
            assert!(drawn > 100, "{t:?} {label}: {drawn} drawn pixels inside");
            cases += 1;
        }
    }
    assert_eq!(cases, 24);
}

/// The production crop box's boundaries (INC-10 exit review E3, proposed C12.F17): each edge is clamped to
/// `[−2³⁰, 2³⁰]`, so the presented pixels lie in the half-open `[−2³⁰, 2³⁰)`; Smithay's i32 size
/// holds at most `i32::MAX = 2³¹ − 1`, so a rect covering the whole box is presented as
/// `[−2³⁰, 2³⁰ − 1)`, one pixel short at the far edge (outside every view, `view_O ≤ 2¹⁵`); any
/// other rect keeps its edges inside the box; a rect wholly outside it is presented empty.
#[test]
fn the_crop_box_is_half_open_with_the_width_capped_at_i32_max() {
    let (ls, id, tree) = transformed(wl_output::Transform::Normal);
    let e = entry_at(&ls, id, &tree, 1.0, (10, 20));
    let mut r = DummyRenderer;
    let l = RENDER_LIMIT;
    let x = |x0: i64, x1: i64| PhysicalRect {
        x0,
        y0: 0,
        x1,
        y1: 10,
    };
    for (name, rect, want) in [
        ("the box exactly", x(-l, l), (-l, l - 1)),
        (
            "beyond the box on both sides",
            x(-l - 5, l + 5),
            (-l, l - 1),
        ),
        ("[0, 2³⁰)", x(0, l), (0, l)),
        ("[−2³⁰, 0)", x(-l, 0), (-l, 0)),
        ("[−2³⁰ + 1, 2³⁰)", x(-l + 1, l), (-l + 1, l)),
        ("[−2³⁰, 2³⁰ − 1)", x(-l, l - 1), (-l, l - 1)),
        ("[−2⁴⁰, 2³⁰ − 7)", x(-(1 << 40), l - 7), (-l, l - 7)),
    ] {
        let inner = convert::surface_element(&mut r, ls.state.registry(), &e)
            .unwrap()
            .expect("element");
        let el = CanonicalRect::new(inner, rect, 1.0);
        let g = el.geometry(Scale::from(1.0));
        let got = (g.loc.x as i64, g.loc.x as i64 + g.size.w as i64);
        assert_eq!(got, want, "{name}");
        assert_eq!((g.loc.y, g.size.h), (0, 10), "{name}: y");
    }
    for (name, rect) in [
        ("[2³⁰, 2³⁰ + 9)", x(l, l + 9)),
        ("[−2⁴⁰, −2³⁰ − 1)", x(-(1 << 40), -l - 1)),
    ] {
        let inner = convert::surface_element(&mut r, ls.state.registry(), &e)
            .unwrap()
            .expect("element");
        let el = CanonicalRect::new(inner, rect, 1.0);
        assert_eq!(el.geometry(Scale::from(1.0)).size.w, 0, "{name}: not empty");
    }
}
