//! Production path tests for the one-shot viewport fit and restore (core + adapter +
//! headless backend + a real client over a socketpair), driven by the virtual keyboard (Super+Enter;
//! Level D default bindings). The oracle is the client's own wire log and
//! hand-derived values.
//!
//! What it establishes: F-FIT-1's mutation and M10's single true-position event on the wire; no
//! configure and no geometry change from a camera action (INV-CAMERA-2/-4/-5); client resizes
//! never move a fitted camera (ER-5 R3, 200 commits); the output's damage tracker is rebuilt when
//! the zoom changes, so an age-1 repaint damages exactly the surface's canonical rect at the new k
//! (C12.F15.4, software renderer Pixman). Not established: GLES or any GPU path.

mod common;

use std::os::fd::AsFd;

use common::input::*;
use common::{Lockstep, output};
use infynitae_backend_headless::keyboard_key;
use infynitae_core::geom::CanvasPos;
use infynitae_core::input::keys::{KEY_ENTER, KEY_LEFTMETA};
use infynitae_core::navigation::CameraValue;
use infynitae_core::render::FrameEntry;
use infynitae_wayland::convert;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ExportMem, Offscreen, Renderer, TextureFilter};
use smithay::reexports::pixman::Image;
use smithay::utils::{Physical, Rectangle};
use wayland_client::protocol::wl_shm;

/// Super+Enter on the virtual keyboard. `_restore` names what the press is meant to do: Super+Shift+F is gone (owner
/// decision 2026-10-21), and at the fit the same Super+Enter restores.
fn combo(r: &mut Rig, _restore: bool) {
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    press(KEY_LEFTMETA, true);
    press(KEY_ENTER, true);
    press(KEY_ENTER, false);
    press(KEY_LEFTMETA, false);
    r.frame();
}

fn camera(r: &Rig) -> CameraValue {
    let c = r.ls.state.core().cameras();
    CameraValue {
        center: c.center(0).unwrap(),
        zoom: c.zoom(0).unwrap(),
    }
}

fn history_len(r: &Rig) -> usize {
    r.ls.state.core().cameras().history(0).unwrap().len()
}

fn same(a: CameraValue, b: CameraValue) -> bool {
    a.center == b.center && a.zoom.to_bits() == b.zoom.to_bits()
}

/// Super+Enter on the x8 setup (W: 800 × 600 content with a 20 px shadow, centred; the pointer on W):
/// the camera becomes W's window-geometry centre (0, 0) at zoom min(1920/800, 1080/600) = 1.8; the
/// client receives exactly one pointer motion and its frame (M10: the true position under the new
/// view) and no configure, and W keeps its anchor; Enter itself never reaches the client (a
/// compositor binding). A second Super+Enter restores the exact prior camera with one motion again; a
/// third restore is refused and delivers nothing.
#[test]
fn fit_and_restore_through_the_keyboard_bindings() {
    let mut r = Rig::x8();
    r.button(BTN_MIDDLE, true, logo());
    r.motion((13.7, -4.2));
    r.button(BTN_MIDDLE, false, logo());
    let before = camera(&r);
    let anchor = r.anchor();
    let configures = r.ls.probe.configures.len();
    let m0 = r.mark();
    combo(&mut r, false);
    let after = camera(&r);
    assert_eq!(after.zoom, 1.8);
    assert_eq!(after.center, CanvasPos { x: 0, y: 0 });
    assert_eq!(history_len(&r), 1);
    let lines = r.since(m0);
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("motion")).count(),
        1,
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|l| l.starts_with("motion") || *l == "frame"),
        "{lines:?}"
    );
    assert_eq!(
        r.ls.probe.configures.len(),
        configures,
        "INV-CAMERA-4: a configure"
    );
    assert_eq!(r.anchor(), anchor, "INV-CAMERA-2: W moved");
    assert!(
        !r.ls.probe.key_events.contains(&(KEY_ENTER, true)),
        "the binding's F reached the client"
    );
    let m1 = r.mark();
    combo(&mut r, true);
    assert!(same(camera(&r), before), "restore is exact");
    assert_eq!(history_len(&r), 0);
    let lines = r.since(m1);
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("motion")).count(),
        1,
        "{lines:?}"
    );
    // with the history empty a restore is refused and delivers nothing (the key is a toggle now —
    // pressed here it would fit again — so the M8 restore is asked through Core's shell entry)
    let m2 = r.mark();
    let o = r.ls.output();
    assert_eq!(
        r.ls.state.core_mut().shell_command(
            &mut r.ls.shell,
            infynitae_core::ShellCommand::ViewportRestore { output: o }
        ),
        Err(infynitae_core::ShellError::Refused)
    );
    r.ls.pump();
    r.frame();
    assert!(same(camera(&r), before));
    assert!(
        r.since(m2).is_empty(),
        "a refused restore delivered {:?}",
        r.since(m2)
    );
}

/// Client resize never moves the camera (ER-5 R3, 200 commits): after a fit, W resizes itself 200 times
/// — window geometry and buffer — and the camera stays exactly the fitted value; nothing re-fits
/// and the history keeps its one entry.
#[test]
fn client_resizes_never_move_a_fitted_camera() {
    let mut r = Rig::x8();
    combo(&mut r, false);
    let fitted = camera(&r);
    assert_eq!(history_len(&r), 1);
    let qh = r.ls.qh.clone();
    for i in 0..200i32 {
        let content = (400 + (i * 37) % 900, 300 + (i * 53) % 500);
        let s = &mut r.ls.probe.surfaces[0];
        s.size = (content.0 + 40, content.1 + 40);
        s.xdg
            .as_ref()
            .unwrap()
            .set_window_geometry(20, 20, content.0, content.1);
        r.ls.probe.draw(&qh, 0);
        r.ls.pump();
        r.frame();
        assert!(same(camera(&r), fitted), "commit {i} moved the camera");
        assert_eq!(history_len(&r), 1, "commit {i}");
    }
    let geometry =
        r.ls.state
            .core()
            .stage()
            .tree(r.ls.state.elements().next().unwrap())
            .unwrap()
            .geometry;
    assert_eq!(
        (geometry.size.w, geometry.size.h),
        (400 + (199 * 37) % 900, 300 + (199 * 53) % 500),
        "the resizes reached Core"
    );
}

// ------------------------------------------------------------------ C12.F15.4

/// Attaches a buffer of `size` filled with `argb` and commits.
fn paint(ls: &mut Lockstep, idx: usize, argb: u32) {
    let (w, h) = ls.probe.surfaces[idx].size;
    let len = (w * h * 4) as usize;
    let fd = smithay::reexports::rustix::fs::memfd_create(
        "infynitae-t1-navigation",
        smithay::reexports::rustix::fs::MemfdFlags::CLOEXEC,
    )
    .expect("memfd");
    let file = std::fs::File::from(fd);
    std::os::unix::fs::FileExt::write_all_at(&file, &argb.to_le_bytes().repeat(len / 4), 0)
        .expect("fill");
    let shm = ls.probe.shm.clone().expect("wl_shm");
    let pool = shm.create_pool(file.as_fd(), len as i32, &ls.qh, ());
    let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &ls.qh, ());
    let wl = &ls.probe.surfaces[idx].wl;
    wl.attach(Some(&buffer), 0, 0);
    wl.damage_buffer(0, 0, w, h);
    wl.commit();
    ls.pump();
}

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
            .unwrap();
        Screen {
            renderer,
            image,
            size,
        }
    }

    fn frame(
        &mut self,
        ls: &mut Lockstep,
        age: usize,
    ) -> (Option<Vec<Rectangle<i32, Physical>>>, Vec<u32>) {
        let o = ls.output();
        let mut target = self.renderer.bind(&mut self.image).expect("bind");
        let damage = convert::render_output(&mut ls.state, &mut self.renderer, &mut target, o, age)
            .expect("render_output");
        let m = self
            .renderer
            .copy_framebuffer(
                &target,
                Rectangle::from_size(self.size.into()),
                Fourcc::Argb8888,
            )
            .unwrap();
        let px = self
            .renderer
            .map_texture(&m)
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        (damage, px)
    }
}

/// Every pixel equals the front-most canonical rect's colour (transparent where none).
fn wrong(
    entries: &[FrameEntry],
    colour: impl Fn(&FrameEntry) -> u32,
    px: &[u32],
    w: i64,
    h: i64,
) -> usize {
    let mut n = 0;
    for y in 0..h {
        for x in 0..w {
            let want = entries
                .iter()
                .find(|e| x >= e.rect.x0 && x < e.rect.x1 && y >= e.rect.y0 && y < e.rect.y1)
                .map_or(0, &colour);
            n += usize::from(px[(y * w + x) as usize] != want);
        }
    }
    n
}

/// C12.F15.4's regression test: a fit to a zoom ≠ 1 (k = 1080/601 ∉ n/120) changes k without any
/// `OutputChanged`; the next frames are right, an unchanged frame damages nothing, and a client
/// repaint of the subsurface S at age 1 damages S's canonical rect at the new k (rounded outwards by at
/// most one pixel, as Smithay rounds damage) and repaints it. The tracker built at the old k would compute S's pass-through damage at zoom 1 (seeded
/// control in tools/verify-test-efficacy).
#[test]
fn a_zoom_change_rebuilds_the_damage_tracker() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (803, 601), 0);
    ls.pump();
    paint(&mut ls, w, 0xFF20_2020);
    let s = ls
        .probe
        .subsurface(&qh, "S", w, (211, 97), (157, 89), false);
    ls.pump();
    paint(&mut ls, s, 0xFFFF_0000);
    ls.probe.surfaces[w].wl.commit();
    ls.pump();
    let mut r = Rig::new(ls);
    r.frame();
    let mut screen = Screen::new((1920, 1080));
    let colour = |c_s: u32| move |e: &FrameEntry| if e.size.w == 157 { c_s } else { 0xFF20_2020 };
    let o = r.ls.output();
    screen.frame(&mut r.ls, 0);
    combo(&mut r, false);
    let k = r.ls.state.core().view(o).unwrap().k;
    assert_eq!(k, 1080.0 / 601.0, "fitted to W's height");
    assert!((k * 120.0).fract() != 0.0, "k ∉ n/120");
    let (_, px) = screen.frame(&mut r.ls, 1);
    let frame = r.ls.state.core_mut().render_list(o).unwrap().clone();
    assert_eq!(
        wrong(&frame.entries, colour(0xFFFF_0000), &px, 1920, 1080),
        0,
        "after the fit"
    );
    let (damage, _) = screen.frame(&mut r.ls, 1);
    assert!(
        damage.as_ref().is_none_or(|d| d.is_empty()),
        "unchanged frame damaged {damage:?}"
    );
    paint(&mut r.ls, s, 0xFF00_FF00);
    let (damage, px) = screen.frame(&mut r.ls, 1);
    let rs = frame.entries.iter().find(|e| e.size.w == 157).unwrap().rect;
    let want = Rectangle::new(
        (rs.x0 as i32, rs.y0 as i32).into(),
        ((rs.x1 - rs.x0) as i32, (rs.y1 - rs.y0) as i32).into(),
    );
    // Smithay rounds pass-through damage outwards (`to_physical_precise_up`): it covers the rect
    // and exceeds it by at most one pixel at the far edges
    let d = damage.expect("damage");
    assert_eq!(d.len(), 1, "{d:?}");
    let g = d[0];
    assert!(
        g.loc == want.loc
            && (want.size.w..=want.size.w + 1).contains(&g.size.w)
            && (want.size.h..=want.size.h + 1).contains(&g.size.h),
        "age-1 repaint at the new k: damage {g:?}, canonical rect {want:?}"
    );
    assert_eq!(
        wrong(&frame.entries, colour(0xFF00_FF00), &px, 1920, 1080),
        0,
        "after the repaint"
    );
    // the restore changes k back: the tracker follows again
    combo(&mut r, true);
    assert_eq!(r.ls.state.core().view(o).unwrap().k, 1.0);
    let (_, px) = screen.frame(&mut r.ls, 1);
    let frame = r.ls.state.core_mut().render_list(o).unwrap().clone();
    assert_eq!(
        wrong(&frame.entries, colour(0xFF00_FF00), &px, 1920, 1080),
        0,
        "after the restore"
    );
}
