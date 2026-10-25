//! Production path tests for pixels (`Core::render_list` → `convert::render_elements` →
//! the output's damage tracker, i.e. `convert::render_output`, the function the nested backend
//! calls each frame) composited by Smithay's software renderer into an image that is read back.
//!
//! What this establishes: real `wl_shm` buffers committed over the Wayland protocol are imported,
//! placed, stacked, clipped and scaled into the pixels Core's render list says; nothing is drawn
//! outside them; an unchanged scene produces no damage and a changed one damages only its
//! pixels. What it does not establish: anything about the GLES renderer, a GPU, dmabuf import or
//! a display. Those need a nested or physical run.

mod common;

use std::os::fd::AsFd;

use common::{Lockstep, output};
use infynitae_backend_headless::inject;
use infynitae_core::PhysicalEvent;
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::render::FrameEntry;
use infynitae_wayland::convert;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ExportMem, Offscreen};
use smithay::reexports::pixman::Image;
use smithay::utils::{Physical, Rectangle};
use wayland_client::protocol::wl_shm;

const CLEAR: u32 = 0x0000_0000;
const RED: u32 = 0xFFFF_0000;
const GREEN: u32 = 0xFF00_FF00;
const BLUE: u32 = 0xFF00_00FF;
const WHITE: u32 = 0xFFFF_FFFF;

/// The render target of one output: a software renderer and an ARGB image of the mode's size.
struct Screen {
    renderer: PixmanRenderer,
    image: Image<'static, 'static>,
    size: (i32, i32),
}

impl Screen {
    fn new(size: (i32, i32)) -> Screen {
        let mut renderer = PixmanRenderer::new().expect("software renderer");
        let image = renderer
            .create_buffer(Fourcc::Argb8888, size.into())
            .expect("render target");
        Screen {
            renderer,
            image,
            size,
        }
    }

    /// One frame through `convert::render_output`. `age` 0 redraws everything; 1 means the
    /// target still holds the previous frame. Returns the damage and the pixels (ARGB).
    fn frame(
        &mut self,
        ls: &mut Lockstep,
        age: usize,
    ) -> (Option<Vec<Rectangle<i32, Physical>>>, Vec<u32>) {
        let o = ls.output();
        let mut target = self.renderer.bind(&mut self.image).expect("bind");
        let damage = convert::render_output(&mut ls.state, &mut self.renderer, &mut target, o, age)
            .expect("render_output");
        let mapping = self
            .renderer
            .copy_framebuffer(
                &target,
                Rectangle::from_size(self.size.into()),
                Fourcc::Argb8888,
            )
            .expect("read back");
        let bytes = self.renderer.map_texture(&mapping).expect("map");
        let pixels = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        (damage, pixels)
    }
}

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

/// A physical rect `[x0, x1) × [y0, y1)` and the colour drawn in it; lists are front to back.
type Layer = ((i64, i64, i64, i64), u32);

/// Every pixel of the frame against a painter's oracle over `layers`. With `exact_edges` every
/// pixel must equal the front-most layer covering it. Without it (a scaled buffer: the software
/// renderer filters the outermost pixel row of a layer) the outermost one-pixel ring of the
/// front-most layer is not checked, so an edge that is off by one pixel inwards goes unnoticed
/// at fractional scales; every other pixel must equal the front-most layer, and no pixel
/// outside every layer may be touched.
fn assert_pixels(
    pixels: &[u32],
    size: (i32, i32),
    layers: &[Layer],
    exact_edges: bool,
    what: &str,
) {
    assert_eq!(pixels.len(), (size.0 * size.1) as usize);
    let covers = |r: &(i64, i64, i64, i64), x: i64, y: i64, inset: i64| {
        x >= r.0 + inset && x < r.2 - inset && y >= r.1 + inset && y < r.3 - inset
    };
    let mut inside = 0usize;
    for y in 0..size.1 as i64 {
        for x in 0..size.0 as i64 {
            let got = pixels[(y * size.0 as i64 + x) as usize];
            let front = layers.iter().find(|(r, _)| covers(r, x, y, 0));
            match front {
                None => assert_eq!(got, CLEAR, "{what}: pixel ({x},{y}) outside every surface"),
                Some((r, colour)) => {
                    if exact_edges || covers(r, x, y, 1) {
                        assert_eq!(
                            got, *colour,
                            "{what}: pixel ({x},{y}) is {got:#010x}, expected {colour:#010x}"
                        );
                        inside += 1;
                    }
                }
            }
        }
    }
    assert!(inside > 0, "{what}: no surface pixel was checked");
}

fn rect(e: &FrameEntry) -> (i64, i64, i64, i64) {
    (e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1)
}

/// Core's render list as layers; each surface is identified by its (unique) buffer size.
fn layers_from_core(ls: &mut Lockstep, colours: &[((i32, i32), u32)]) -> Vec<Layer> {
    let o = ls.output();
    let entries = ls.state.core_mut().render_list(o).unwrap().entries.clone();
    entries
        .iter()
        .map(|e| {
            let colour = colours
                .iter()
                .find(|(s, _)| *s == (e.size.w, e.size.h))
                .expect("a surface of this size")
                .1;
            (rect(e), colour)
        })
        .collect()
}

const LOGO: Modifiers = Modifiers {
    logo: true,
    ctrl: false,
    alt: false,
    shift: false,
};

fn pan(ls: &mut Lockstep, delta: (f64, f64)) {
    let button = |pressed| PhysicalEvent::PointerButton {
        button: 0x112,
        pressed,
        modifiers: LOGO,
        time_us: 0,
    };
    inject(&mut ls.state, &mut ls.input, button(true)).unwrap();
    inject(
        &mut ls.state,
        &mut ls.input,
        PhysicalEvent::PointerMotion {
            delta,
            delta_unaccel: delta,
            time_us: 0,
        },
    )
    .unwrap();
    inject(&mut ls.state, &mut ls.input, button(false)).unwrap();
    ls.pump();
}

/// Buffer import and placement at scale 1, against positions derived by hand: a window maps
/// centred in the view (F-PLACE-1), the centre of a 640x480 view is physical (320, 240), so a
/// 100x80 buffer has its origin at (320 − 50, 240 − 40) and covers [270, 370) × [200, 280), and
/// nothing else is drawn.
#[test]
fn a_solid_buffer_lands_on_exactly_its_pixels() {
    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    toplevel(&mut ls, "W", (100, 80), RED);
    let mut screen = Screen::new((640, 480));
    let (damage, pixels) = screen.frame(&mut ls, 0);
    assert!(damage.is_some(), "the first frame is damaged");
    assert_pixels(
        &pixels,
        (640, 480),
        &[((270, 200, 370, 280), RED)],
        true,
        "scale 1",
    );
}

/// Surface-tree composition and window stacking at scale 1, against positions derived by hand.
/// W1 is a red 200x150 root with a green 50x40 subsurface above it at (+20, +10) and a blue
/// 60x44 subsurface below it at (−15, +50). W1 is placed once, at its first map, when it is
/// the 200x150 root alone: its geometry origin goes to (320 − 100, 240 − 75) = (220, 165). No
/// window geometry is set, so it is the bounding box of the tree (xdg_surface.set_window_geometry:
/// "the full bounds of the surface, including any subsurfaces"): once the blue subsurface exists
/// the geometry origin is 15 px left of the root, and it is that origin which stays at
/// (220, 165). The root is therefore at (235, 165), green at (255, 175) and blue at (220, 215).
/// W2 (white 80x60) maps later, centred at (320 − 40, 240 − 30) = (280, 210), above all of W1;
/// of the blue subsurface only the part left of the root shows.
#[test]
fn subsurfaces_and_windows_stack_in_render_order() {
    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    let w1 = toplevel(&mut ls, "W1", (200, 150), RED);
    subsurface(&mut ls, "above", w1, (20, 10), (50, 40), false, GREEN);
    subsurface(&mut ls, "below", w1, (-15, 50), (60, 44), true, BLUE);
    toplevel(&mut ls, "W2", (80, 60), WHITE);
    let mut screen = Screen::new((640, 480));
    let (_, pixels) = screen.frame(&mut ls, 0);
    let layers = [
        ((280, 210, 360, 270), WHITE),
        ((255, 175, 305, 215), GREEN),
        ((235, 165, 435, 315), RED),
        ((220, 215, 280, 259), BLUE),
    ];
    assert_pixels(&pixels, (640, 480), &layers, true, "composition");
    // the same frame through Core's own render list: the list is what was drawn
    let from_core = layers_from_core(
        &mut ls,
        &[
            ((80, 60), WHITE),
            ((50, 40), GREEN),
            ((200, 150), RED),
            ((60, 44), BLUE),
        ],
    );
    assert_eq!(
        from_core, layers,
        "Core's render list equals the hand-derived layers"
    );
}

/// Fractional scales and clipping at the view edge: the pixels are the physical rects of Core's
/// render list (F-LATTICE-1), a window panned half out of the view is cut at the edge, and one
/// panned out entirely leaves an empty frame.
#[test]
fn scaled_and_clipped_surfaces_match_the_render_list() {
    for scale_120 in [120u32, 150, 180, 210] {
        let exact = scale_120 == 120;
        let mut ls = Lockstep::new(Some(output((640, 480), scale_120)));
        let w = toplevel(&mut ls, "W", (120, 90), RED);
        subsurface(&mut ls, "S", w, (30, 20), (40, 30), false, GREEN);
        let colours = [((120, 90), RED), ((40, 30), GREEN)];
        let mut screen = Screen::new((640, 480));

        let layers = layers_from_core(&mut ls, &colours);
        assert_eq!(layers.len(), 2);
        let (_, pixels) = screen.frame(&mut ls, 0);
        assert_pixels(&pixels, (640, 480), &layers, exact, "in view");

        // the window is centred, so its middle is at the view centre: pan the camera right by
        // the distance from the centre to the left edge and the window straddles that edge
        let k = scale_120 as f64 / 120.0;
        let to_edge = 320.0 / k;
        pan(&mut ls, (-to_edge, 0.0));
        let layers = layers_from_core(&mut ls, &colours);
        assert!(
            layers.iter().any(|(r, c)| *c == RED && r.0 < 0 && r.2 > 0),
            "scale {scale_120}: the window straddles the left edge: {layers:?}"
        );
        let (_, pixels) = screen.frame(&mut ls, 0);
        assert_pixels(
            &pixels,
            (640, 480),
            &layers,
            exact,
            "straddling the left edge",
        );

        pan(&mut ls, (-400.0, 0.0));
        assert!(layers_from_core(&mut ls, &colours).is_empty());
        let (_, pixels) = screen.frame(&mut ls, 0);
        assert!(
            pixels.iter().all(|p| *p == CLEAR),
            "scale {scale_120}: a window outside the view draws nothing"
        );
    }
}

/// Damage: an unchanged scene produces no damage (so a backend has nothing to submit), a commit
/// of one surface damages that surface's pixels and no others, and the picture after a damaged
/// redraw equals a full redraw.
#[test]
fn damage_follows_commits_and_nothing_else() {
    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    let w = toplevel(&mut ls, "W", (100, 80), RED);
    let s = subsurface(&mut ls, "S", w, (10, 10), (20, 20), false, GREEN);
    let mut screen = Screen::new((640, 480));
    let (first, _) = screen.frame(&mut ls, 0);
    assert!(first.is_some());
    for _ in 0..3 {
        let (damage, _) = screen.frame(&mut ls, 1);
        assert_eq!(damage, None, "an unchanged scene is not damaged");
    }
    paint(&mut ls, s, BLUE);
    let (damage, pixels) = screen.frame(&mut ls, 1);
    let damage = damage.expect("the commit damaged the output");
    let subsurface_rect: Rectangle<i32, Physical> =
        Rectangle::new((280, 210).into(), (20, 20).into());
    for d in &damage {
        assert!(
            subsurface_rect.contains_rect(*d),
            "damage {d:?} lies outside the committed surface {subsurface_rect:?}"
        );
    }
    // W (100x80) at (270, 200) as above; S at W + (10, 10)
    let layers = [((280, 210, 300, 230), BLUE), ((270, 200, 370, 280), RED)];
    assert_pixels(&pixels, (640, 480), &layers, true, "after a damaged redraw");
    let (_, full) = Screen::new((640, 480)).frame(&mut ls, 0);
    assert_eq!(pixels, full, "a damaged redraw equals a full redraw");
    let (damage, _) = screen.frame(&mut ls, 1);
    assert_eq!(damage, None);
}

/// The nested backend's target (`set_host_window_target`): a GL framebuffer's rows run bottom-up,
/// so the frame is drawn flipped — read back top-down it is the scene upside down — and the empty
/// Canvas is opaque (a transparent clear let the host's desktop show through the window; found
/// in the first live run, 2026-10-21). The window is panned off the vertical centre, so a missing
/// flip cannot pass by symmetry.
#[test]
fn a_host_window_target_is_drawn_flipped_on_an_opaque_canvas() {
    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    ls.state.set_host_window_target();
    toplevel(&mut ls, "W", (100, 80), RED);
    pan(&mut ls, (0.0, 100.0));
    let layers = layers_from_core(&mut ls, &[((100, 80), RED)]);
    let (x0, y0, x1, y1) = layers[0].0;
    assert_ne!(y0, 480 - y1, "precondition: off the vertical centre");
    let mut screen = Screen::new((640, 480));
    let (_, pixels) = screen.frame(&mut ls, 0);
    let canvas = pixels[0];
    assert_eq!(
        canvas >> 24,
        0xFF,
        "the empty Canvas is opaque: {canvas:#010x}"
    );
    assert_ne!(canvas & 0x00FF_FFFF, 0, "and not black: {canvas:#010x}");
    let flipped = (x0, 480 - y1, x1, 480 - y0);
    for y in 0..480i64 {
        for x in 0..640i64 {
            let got = pixels[(y * 640 + x) as usize];
            let inside = x >= flipped.0 && x < flipped.2 && y >= flipped.1 && y < flipped.3;
            let want = if inside { RED } else { canvas };
            assert_eq!(
                got, want,
                "pixel ({x},{y}) is {got:#010x}, expected {want:#010x}"
            );
        }
    }
}

/// One pointer motion of `delta` logical px.
fn nudge(ls: &mut Lockstep, delta: (f64, f64)) {
    inject(
        &mut ls.state,
        &mut ls.input,
        PhysicalEvent::PointerMotion {
            delta,
            delta_unaccel: delta,
            time_us: 0,
        },
    )
    .unwrap();
    ls.pump();
}

/// The console target (owner decision T8, 2026-10-22): nobody else draws the pointer, so the
/// compositor draws the built-in arrow at it, its tip the pointer's pixel. The pointer starts at
/// the view centre (320, 240) and is moved 10 px right, to (330, 240): the outline's row r covers
/// x ∈ [330, 331 + r), the fill's row r (r ≥ 2) x ∈ [331, 330 + r).
#[test]
fn the_console_target_draws_the_arrow_at_the_pointer() {
    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    ls.state.set_console_target();
    nudge(&mut ls, (10.0, 0.0));
    let mut screen = Screen::new((640, 480));
    let (_, pixels) = screen.frame(&mut ls, 0);
    let at = |x: usize, y: usize| pixels[y * 640 + x];
    let canvas = at(0, 0);
    assert_eq!(canvas >> 24, 0xFF, "opaque Canvas");
    const BLACK: u32 = 0xFF00_0000;
    assert_eq!(at(330, 240), BLACK, "the tip, on the pointer's pixel");
    assert_eq!(at(329, 240), canvas, "nothing left of the tip");
    assert_eq!(at(330, 245), BLACK, "row 5's outline, left column");
    assert_eq!(at(331, 245), WHITE, "row 5's fill");
    assert_eq!(at(334, 245), WHITE, "row 5's fill ends at 330 + 5");
    assert_eq!(at(335, 245), BLACK, "row 5's outline, the diagonal");
    assert_eq!(at(336, 245), canvas, "nothing right of the diagonal");
    // the nested and headless targets draw no pointer
    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    nudge(&mut ls, (10.0, 0.0));
    let (_, pixels) = screen.frame(&mut ls, 0);
    assert!(
        pixels.iter().all(|&p| p == CLEAR),
        "no cursor without the console target"
    );
}

/// A client's cursor surface (`wl_pointer.set_cursor` with a hotspot) is drawn with its hotspot
/// on the pointer; `set_cursor(None)` hides it. W (100 × 80, red) is centred, under the pointer
/// at (320, 240); the cursor is an 8 × 8 green buffer with hotspot (2, 3), so it covers
/// [318, 326) × [237, 245).
#[test]
fn the_console_target_draws_the_clients_cursor_at_its_hotspot() {
    let mut r = common::input::Rig::new(Lockstep::new(Some(output((640, 480), 120))));
    r.ls.state.set_console_target();
    toplevel(&mut r.ls, "W", (100, 80), RED);
    r.motion((1.0, 0.0));
    r.motion((-1.0, 0.0));
    let ls = &mut r.ls;
    let serial = ls.probe.last_serial;
    assert!(
        ls.probe.ptr.iter().any(|l| l.starts_with("enter W")),
        "precondition: the pointer entered W: {:?}",
        ls.probe.ptr
    );
    let qh = ls.qh.clone();
    let cursor = ls.probe.bare_surface(&qh, "cursor", (8, 8));
    paint(ls, cursor, GREEN);
    let wl = ls.probe.surfaces[cursor].wl.clone();
    let pointer = ls.probe.pointer.clone().expect("wl_pointer");
    pointer.set_cursor(serial, Some(&wl), 2, 3);
    ls.pump();
    let mut screen = Screen::new((640, 480));
    let (_, pixels) = screen.frame(ls, 0);
    let at = |x: usize, y: usize| pixels[y * 640 + x];
    for (x, y) in [(318, 237), (325, 244), (320, 240)] {
        assert_eq!(at(x, y), GREEN, "cursor pixel ({x},{y})");
    }
    for (x, y) in [(317, 240), (326, 240), (320, 236), (320, 245)] {
        assert_eq!(at(x, y), RED, "window pixel ({x},{y}) around the cursor");
    }
    pointer.set_cursor(serial, None, 0, 0);
    ls.pump();
    let (_, pixels) = screen.frame(ls, 0);
    assert_eq!(pixels[240 * 640 + 320], RED, "hidden: the window under it");
}
